use std::ops::DerefMut;

use pulp::Arch;
use tokio::fs::OpenOptions;

use super::*;

pub trait KeyExt: std::hash::Hash + Eq + Copy + Clone {
    type PageBoundKey: KeyExt;

    /// Get the page bound key for the item.
    fn page_bound(&self) -> Self::PageBoundKey;

    /// Get a filename that stores the index for the given key.
    /// PIDX is split into multiple files, per type ID, but other indexes may be in the single
    /// file per index definition.
    fn filename(&self) -> Cow<'static, str>;
}

/// Number of generations to keep for LRU cache eviction.
/// This is for modified LRU cache eviction, where we keep the last accessed generations
/// to determine which pages to evict when the cache is full, but also to account for how
/// many times the page was accessed in the last generations.
/// This allows us to keep frequently accessed pages in the cache longer, while evicting
/// less frequently accessed pages.
const LAST_ACCESSED_GENS: usize = 4;

/// Page store for the database.
/// This is a generic page store that can be used to store pages of any type.
/// The page store is used to store pages in memory and on disk, and provides methods
/// to access and modify pages.
/// The page store is generic over the key type and the page type, which allows to use
/// different key types and page types for different database usecases.
///
/// The `PAGE_SIZE` constant defines the size of the page in bytes, which is a workaround
/// to `std::mem::size_of::<P>()` which won't compile in const context as of Rust 1.82.
#[derive(Debug)]
pub struct PageStore<K: KeyExt, P, const PAGE_SIZE: usize> {
    /// Generation counter for LRU cache eviction.
    /// This is incremented every time a page is accessed or modified.
    last_gen: u64,

    /// Path to the page store file.
    path: PathBuf,

    /// Cache of pages in memory.
    pages: Vec<P>,

    /// List of vacant pages in the cache.
    vacant_pages: Vec<u32>,

    /// Map page ID to page metadata.
    page_map: HashMap<K::PageBoundKey, CacheMeta>,
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

impl<K: KeyExt, P, const PAGE_SIZE: usize> PageStore<K, P, PAGE_SIZE> {
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
    fn touch(&mut self, item_key: K) -> Option<&mut CacheMeta> {
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
    async fn fetch(&mut self, item_key: K, create: bool) -> Result<&mut CacheMeta, PageReadError> {
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
        item_key: K::PageBoundKey,
        create: bool,
    ) -> Result<P, PageReadError> {
        // If not in cache, we need to load it from disk
        let filename = item_key.filename();
        let path = self.path.join(&*filename);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create) // Create the file if it doesn't exist
            .open(&path)
            .await?;

        // Read the page data
        assert_eq!(PAGE_SIZE, std::mem::size_of::<P>());
        let mut buffer = [0u8; PAGE_SIZE];
        let size = file.read_exact(&mut buffer).await?;
        if size != buffer.len() {
            return Err(PageReadError::UnexpectedEof {
                name: filename.into_owned(),
                expected: buffer.len(),
                actual: size,
            });
        }

        // Deserialize the page
        Ok(unsafe { std::ptr::read(buffer.as_ptr() as *const P) })
    }

    /// Get a mutable reference to the page that contains the item with the given key,
    /// marking the page as dirty.
    pub async fn item_page_mut(
        &mut self,
        item_key: K,
        create: bool,
    ) -> Result<&mut P, PageReadError> {
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

        struct Scored<K: KeyExt> {
            idx: usize,
            score: u64,
            key: K::PageBoundKey,
        }
        let mut to_evict = SmallVec::<[Scored<K>; 64]>::with_capacity(count);

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
