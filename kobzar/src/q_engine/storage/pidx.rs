use std::ops::DerefMut;

use pulp::Arch;
use tokio::fs::OpenOptions;
use tokio::io::AsyncReadExt;

use super::*;

/// Page size in records to store Primary Index Records.
pub const PAGE_SIZE_RECS: usize = 4096;

/// Bitmap size that represents the visibility of records in a page.
pub const PAGE_SIZE_BITMAP: usize = PAGE_SIZE_RECS / 8;

/// Number of generations to keep for LRU cache eviction.
/// This is for modified LRU cache eviction, where we keep the last accessed generations
/// to determine which pages to evict when the cache is full, but also to account for how
/// many times the page was accessed in the last generations.
/// This allows us to keep frequently accessed pages in the cache longer, while evicting
/// less frequently accessed pages.
pub const LAST_ACCESSED_GENS: usize = 4;

/// Page of primary index records.
/// Page is a fixed-size structure that contains metadata about the records in the heap file.
/// The page consists of multiple arrays for each field (column), so to allow SIMD
/// acceleration for different operations on the columns.
/// Some fields are grouped together for cache locality, since they are not processed
/// with SIMD, but often accessed together.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C, align(64))] // Align to SIMD-friendly size for better performance
pub struct Page {
    /// Grouped fields for cache locality.
    pub group: [PageGroupFields; PAGE_SIZE_RECS],

    /// Transaction ID that created the record.
    pub xmin: [u64; PAGE_SIZE_RECS],

    /// Transaction ID that deleted the record.
    /// Set to [u64::MAX] if the record is not deleted.
    pub xmax: [u64; PAGE_SIZE_RECS],

    /// Next record version ID.
    /// Set to [u64::MAX] if there is no next record version.
    pub next: [u64; PAGE_SIZE_RECS],

    /// Previous record version ID.
    /// Set to [u64::MAX] if there is no previous record version.
    pub prev: [u64; PAGE_SIZE_RECS],

    /// Schema version of the record.
    pub schema_version: [u32; PAGE_SIZE_RECS],
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}

impl Page {
    /// Create a new empty page.
    pub const fn new() -> Self {
        const INVALID: u64 = u64::MAX;
        const INVALID32: u32 = u32::MAX;
        Self {
            group: [PageGroupFields {
                size: INVALID32,
                offset: INVALID32,
            }; PAGE_SIZE_RECS],
            xmin: [INVALID; PAGE_SIZE_RECS],
            xmax: [INVALID; PAGE_SIZE_RECS],
            next: [INVALID; PAGE_SIZE_RECS],
            prev: [INVALID; PAGE_SIZE_RECS],
            schema_version: [INVALID32; PAGE_SIZE_RECS],
        }
    }

    /// Check visibility of the record for the given transaction ID.
    pub fn xmin_visibility_map(&self, x: u64) -> [u8; PAGE_SIZE_BITMAP] {
        Self::visibility_map_inner(&self.xmin, x, false)
    }

    /// Check visibility of the record for the given transaction ID.
    pub fn xmax_visibility_map(&self, x: u64) -> [u8; PAGE_SIZE_BITMAP] {
        Self::visibility_map_inner(&self.xmax, x, true)
    }

    fn visibility_map_inner(
        xmap: &[u64; PAGE_SIZE_RECS],
        x: u64,
        lt: bool,
    ) -> [u8; PAGE_SIZE_BITMAP] {
        let mut map = [0; PAGE_SIZE_BITMAP];
        Arch::new().dispatch(|| {
            for (i, el) in map.iter_mut().enumerate() {
                let mut byte: u8 = 0;
                for j in 0..8 {
                    let val = xmap[i * 8 + j];
                    byte |= if lt { (val > x) as u8 } else { (val < x) as u8 } << j;
                }
                *el = byte;
            }
        });

        map
    }

    /// Check `xmin` and `xmax` visibility maps for the given transaction ID.
    /// This combines the visibility maps for both `xmin` and `xmax` into a single map.
    /// This is useful for determining if a record is visible to the transaction.
    pub fn visibility_map(&self, x: u64) -> [u8; PAGE_SIZE_BITMAP] {
        let xmax = self.xmax_visibility_map(x);
        let xmin = self.xmin_visibility_map(x);
        let mut map = [0u8; PAGE_SIZE_BITMAP];

        // Combine the visibility maps for xmin and xmax
        Arch::new().dispatch(|| {
            for i in 0..PAGE_SIZE_BITMAP {
                map[i] = xmax[i] & xmin[i];
            }
        });
        map
    }
}

/// We inline some of the fields for cache locality, since these fields are not
/// processed with SIMD, but often are accessed together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub struct PageGroupFields {
    /// Size of the record in bytes. Since we store BLOBs separately, u32 limitations
    /// are enough for the size of the record. Using it instead of [u64] saves space for
    /// better cache usage.
    pub size: u32,

    /// Offset of the record in the heap file.
    pub offset: u32,
}

#[derive(Debug)]
pub struct PageStore {
    /// Generation counter for LRU cache eviction.
    /// This is incremented every time a page is accessed or modified.
    last_gen: u64,

    /// Path to the page store directory.
    path: PathBuf,

    /// Cache of pages in memory.
    pages: Vec<Page>,

    /// List of vacant pages in the cache.
    vacant_pages: Vec<u32>,

    /// Map page ID to page metadata.
    page_map: HashMap<PageBoundKey, CacheMeta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    /// Type ID.
    pub type_id: u64,

    /// Primary index item number.
    pub item: u64,

    /// Database ID.
    pub db: u64,
}

/// A key that is aligned to the page boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct PageBoundKey(Key);

impl Deref for PageBoundKey {
    type Target = Key;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Key {
    /// Create a new key for the given type ID and item number where
    /// the item number is aligned to the page boundaries.
    /// This effectively can create a key for the page in the cache from the item number.
    pub const fn page_bound(self) -> PageBoundKey {
        PageBoundKey(Key {
            type_id: self.type_id,
            item: self.item - (self.item % PAGE_SIZE_RECS as u64),
            db: self.db,
        })
    }

    /// Get the in-page index for the item.
    pub const fn in_page_offset(self) -> usize {
        (self.item % PAGE_SIZE_RECS as u64) as usize
    }
}

/// Metadata for a page in the cache.
#[derive(Debug, Clone, Copy)]
struct CacheMeta {
    /// Index of the page in the cache array.
    idx: usize,

    /// Whether the page is dirty and needs to be written back to disk.
    dirty: bool,

    /// Last accessed generation.
    last_accessed: RingInline,
}

#[derive(Debug, Clone, Copy)]
struct RingInline {
    /// Last accessed generations for the page.
    /// This is used for LRU cache eviction, where we keep the last accessed generations
    /// to determine which pages to evict when the cache is full.
    last_accessed: LastAccessedSimd,

    /// The start index of the deque.
    start: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(align(32))]
struct LastAccessedSimd([u64; LAST_ACCESSED_GENS]);

impl Deref for LastAccessedSimd {
    type Target = [u64; LAST_ACCESSED_GENS];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LastAccessedSimd {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl RingInline {
    /// Create a new empty ring buffer.
    pub const fn new(x: u64) -> Self {
        Self {
            last_accessed: LastAccessedSimd([x; LAST_ACCESSED_GENS]),
            start: 0,
        }
    }

    /// Update the last accessed generation for the page.
    pub fn push(&mut self, generation: u64) {
        self.last_accessed[self.start] = generation;
        self.start = (self.start + 1) % LAST_ACCESSED_GENS;
    }

    pub fn current(&self) -> u64 {
        self.nth(0)
    }

    pub fn nth(&self, n: usize) -> u64 {
        self.last_accessed[(self.start + n) % LAST_ACCESSED_GENS]
    }

    pub fn rotate_to_normalize(&mut self) {
        // Rotate the array to start from the current index
        let mut arr = self.last_accessed.0;
        arr.rotate_left(LAST_ACCESSED_GENS - self.start);
        let new = LastAccessedSimd(arr);

        self.last_accessed = new;
        self.start = 0; // Reset the start index after rotation
    }
}

impl CacheMeta {
    pub fn touch(&mut self, generation: u64) {
        self.last_accessed.push(generation);
    }

    /// Calculate the eviction score for the page based on the last accessed generations.
    /// The higher the score, the more likely the page is to be evicted.
    ///
    /// The score is affected primarily by how long ago the page was accessed,
    /// with more older recorded accesses having less impact on the evaluation.
    /// This means that a page that was accessed a long time ago will have a higher score
    /// than a page that was accessed recently.
    ///
    /// Effectively, the order should be like this:
    ///
    /// 1. Pages that were never accessed
    /// 2. Pages that were accessed a long time ago
    /// 3. Pages with one recent access, and one or more accesses a long time ago
    /// 4. Pages with multiple recent accesses, but one or more accesses a long time ago
    /// 5. Pages that were accessed recently
    pub fn eviction_score(&mut self, current_gen: u64) -> u64 {
        let mut score = 0;

        self.last_accessed.rotate_to_normalize();
        let mut arr = self.last_accessed.last_accessed.0;

        let arch = Arch::new();
        arch.dispatch(|| {
            for i in &mut arr {
                *i -= current_gen;
            }
        });
        for i in 0..LAST_ACCESSED_GENS {
            score += arr[i] >> i;
        }

        score
    }
}

impl PageStore {
    /// Opens the page store at the given path.
    pub async fn open(path: PathBuf) -> std::io::Result<Self> {
        // Ensure the directory exists
        tokio::fs::create_dir_all(&path).await?;

        let recs = cfg().primary_index_cache_recs();

        let vacant_pages = {
            let mut v = Vec::with_capacity(recs);
            for i in 0..recs {
                v.push(i as u32);
            }
            v
        };

        Ok(PageStore {
            last_gen: 0,
            path,
            pages: Vec::with_capacity(recs),
            vacant_pages,
            page_map: HashMap::with_capacity_and_hasher(recs, Default::default()),
        })
    }

    /// Update access time for the page in the cache.
    /// This is used to mark the page as accessed, updating its last accessed generation.
    /// If the page is not in the cache, it returns `None`.
    fn touch(&mut self, item_key: Key) -> Option<&mut CacheMeta> {
        // Update last accessed generation for the page
        if let Some(meta) = self.page_map.get_mut(&item_key.page_bound()) {
            meta.touch(self.last_gen);
            self.last_gen += 1;
            Some(meta)
        } else {
            None
        }
    }

    /// Fetch a page that contains a primary index item. This either reads the page from the cache
    /// or loads it from disk if it's not in the cache. This also marks page as accessed,
    /// updating its last accessed generation.
    async fn fetch(
        &mut self,
        item_key: Key,
        create: bool,
    ) -> Result<&mut CacheMeta, PageReadError> {
        let key = item_key.page_bound();

        // Check if the page is already in the cache
        if self.touch(item_key).is_none() {
            let page = self.load_from_disk(key, create).await?;

            // Add the page to the cache
            let idx = self.pages.len();
            self.pages.push(page);
            self.page_map.insert(
                key,
                CacheMeta {
                    idx,
                    dirty: false,
                    last_accessed: RingInline::new(self.last_gen),
                },
            );
            self.last_gen += 1; // Increment the generation counter
        }

        // Return the metadata for the page
        let meta = self
            .page_map
            .get_mut(&key)
            .expect("Page should be in the cache, as we just loaded it");
        Ok(meta)
    }

    /// Load a page from disk by its item key.
    async fn load_from_disk(
        &self,
        item_key: PageBoundKey,
        create: bool,
    ) -> Result<Page, PageReadError> {
        // If not in cache, we need to load it from disk
        let filename = Self::filename(item_key);
        let path = self.path.join(&filename);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create) // Create the file if it doesn't exist
            .open(&path)
            .await?;

        // Read the page data
        let mut buffer = [0u8; std::mem::size_of::<Page>()];
        let size = file.read_exact(&mut buffer).await?;
        if size != buffer.len() {
            return Err(PageReadError::UnexpectedEof {
                name: filename,
                expected: buffer.len(),
                actual: size,
            });
        }

        // Deserialize the page
        Ok(unsafe { std::ptr::read(buffer.as_ptr() as *const Page) })
    }

    /// Get the name of the file that stores the page for the given item key.
    fn filename(item_key: PageBoundKey) -> String {
        format!("{:x}_{:x}.pidx", item_key.type_id, item_key.item)
    }

    /// Get a mutable reference to the page that contains the item with the given key,
    /// marking the page as dirty.
    pub async fn item_page_mut(
        &mut self,
        item_key: Key,
        create: bool,
    ) -> Result<&mut Page, PageReadError> {
        let idx = {
            let meta = self.fetch(item_key, create).await?;
            meta.dirty = true; // Mark the page as dirty since we're going to modify it
            meta.idx
        };

        Ok(&mut self.pages[idx])
    }

    /// Evict given number of pages from the cache.
    /// This will remove the least recently used old pages from the cache.
    /// This function will try to evict as many pages as specified by `count`,
    /// but it may evict fewer pages if there are not enough pages in the cache.
    pub fn evict(&mut self, count: usize) {
        debug_assert_ne!(0, count, "Count must be greater than zero for eviction");

        struct Scored {
            idx: usize,
            score: u64,
            key: PageBoundKey,
        }
        let mut to_evict = SmallVec::<[Scored; 64]>::with_capacity(count);

        // Calculate eviction scores for pages and retain the top `count` pages.
        for (key, meta) in &mut self.page_map {
            let score = meta.eviction_score(self.last_gen);
            let score = Scored {
                idx: meta.idx,
                score,
                key: *key,
            };

            let bin = to_evict.binary_search_by_key(&score.score, |s| s.score);
            let idx = match bin {
                Ok(idx) => idx,
                Err(idx) => idx,
            };
            if to_evict.len() >= count {
                // Maintain capacity of `count` elements
                to_evict.pop();
            }
            to_evict.insert(idx, score);
        }

        // Evict the least recently used pages
        for evict in to_evict.into_iter() {
            self.page_map.remove(&evict.key);
            self.vacant_pages.push(evict.idx as u32);
        }
    }
}

#[derive(Debug, Error)]
pub enum PageReadError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("Failed to read full page from file: {name}, expected {expected}, actual {actual}")]
    UnexpectedEof {
        name: String,
        expected: usize,
        actual: usize,
    },
}
