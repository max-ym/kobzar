use tokio::io::AsyncReadExt;

use super::*;

/// Page size in records to store Primary Index Records.
pub const PAGE_SIZE_RECS: u64 = 16384; // 1 MiB per page (which is 64 byte)

/// Page of primary index records.
/// Page is a fixed-size structure that contains metadata about the records in the heap file.
/// The page consists of multiple arrays for each field (column), so to allow SIMD
/// acceleration for different operations on the columns.
/// Some fields are grouped together for cache locality, since they are not processed
/// with SIMD, but often accessed together.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub struct Page {
    /// Grouped fields for cache locality.
    pub group: [PageGroupFields; PAGE_SIZE_RECS as usize],

    /// Record flags.
    pub flags: [Flags; PAGE_SIZE_RECS as usize],

    /// Transaction ID that created the record.
    pub xmin: [u64; PAGE_SIZE_RECS as usize],

    /// Transaction ID that deleted the record.
    /// Set to [u64::MAX] if the record is not deleted.
    pub xmax: [u64; PAGE_SIZE_RECS as usize],

    /// Next record version ID.
    /// Set to [u64::MAX] if there is no next record version.
    pub next: [u64; PAGE_SIZE_RECS as usize],

    /// Previous record version ID.
    /// Set to [u64::MAX] if there is no previous record version.
    pub prev: [u64; PAGE_SIZE_RECS as usize],

    /// Schema version of the record.
    pub schema_version: [u64; PAGE_SIZE_RECS as usize],
}

/// We inline some of the fields for cache locality, since these fields are not
/// processed with SIMD, but often are accessed together.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub struct PageGroupFields {
    /// Size of the record in bytes.
    pub size: u64,

    /// Offset of the record in the heap file.
    pub offset: u64,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Flags(u64);

#[derive(Debug)]
pub struct PageStore {
    /// Generation counter for LRU cache eviction.
    /// This is incremented every time a page is accessed or modified.
    last_gen: u64,

    /// Path to the page store directory.
    path: PathBuf,

    /// Cache of pages in memory.
    pages: Vec<Page>,

    /// Map page ID to page metadata.
    page_map: HashMap<Key, CacheMeta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    /// Type ID.
    pub type_id: u64,

    /// Primary index item number.
    pub item: u64,
}

impl Key {
    /// Create a new key for the given type ID and item number where
    /// the item number is aligned to the page boundaries.
    /// This effectively can create a key for the page in the cache from the item number.
    pub const fn page_bound(self) -> Self {
        Key {
            type_id: self.type_id,
            item: self.item - (self.item % PAGE_SIZE_RECS),
        }
    }

    /// Get the in-page index for the item.
    pub const fn in_page_offset(self) -> usize {
        (self.item % PAGE_SIZE_RECS) as usize
    }
}

/// Metadata for a page in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheMeta {
    /// Index of the page in the cache array.
    idx: usize,

    /// Whether the page is dirty and needs to be written back to disk.
    dirty: bool,

    /// Last accessed generation.
    last_accessed: u64,
}

impl PageStore {
    /// Opens the page store at the given path.
    pub async fn open(path: PathBuf) -> std::io::Result<Self> {
        // Ensure the directory exists
        tokio::fs::create_dir_all(&path).await?;

        Ok(PageStore {
            last_gen: 0,
            path,
            pages: Vec::with_capacity(cfg().primary_index_cache_recs()),
            page_map: HashMap::with_capacity_and_hasher(
                cfg().primary_index_cache_recs(),
                Default::default(),
            ),
        })
    }

    /// Update access time for the page in the cache.
    /// This is used to mark the page as accessed, updating its last accessed generation.
    /// If the page is not in the cache, it returns `None`.
    fn touch(&mut self, item_key: Key) -> Option<&mut CacheMeta> {
        // Update last accessed generation for the page
        if let Some(meta) = self.page_map.get_mut(&item_key.page_bound()) {
            meta.last_accessed = self.last_gen;
            self.last_gen += 1;
            Some(meta)
        } else {
            None
        }
    }

    /// Fetch a page that contains a primary index item. This either reads the page from the cache
    /// or loads it from disk if it's not in the cache. This also marks page as accessed,
    /// updating its last accessed generation.
    async fn fetch(&mut self, item_key: Key) -> Result<&mut CacheMeta, PageReadError> {
        let key = item_key.page_bound();

        // Check if the page is already in the cache
        if self.touch(item_key).is_none() {
            let page = self.load_from_disk(key).await?;

            // Add the page to the cache
            let idx = self.pages.len();
            self.pages.push(page);
            self.page_map.insert(
                key,
                CacheMeta {
                    idx,
                    dirty: false,
                    last_accessed: self.last_gen,
                },
            );
        }

        // Return the metadata for the page
        let meta = self
            .page_map
            .get_mut(&key)
            .expect("Page should be in the cache, as we just loaded it");
        Ok(meta)
    }

    /// Load a page from disk by its item key.
    async fn load_from_disk(&self, item_key: Key) -> Result<Page, PageReadError> {
        // If not in cache, we need to load it from disk
        let filename = Self::filename(item_key.page_bound());
        let path = self.path.join(&filename);
        let mut file = tokio::fs::File::open(&path).await?;

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
    fn filename(item_key: Key) -> String {
        format!(
            "{:x}_{:x}.pidx",
            item_key.type_id,
            item_key.page_bound().item
        )
    }

    /// Get a mutable reference to the page that contains the item with the given key,
    /// marking the page as dirty.
    pub async fn item_page_mut(&mut self, item_key: Key) -> Result<&mut Page, PageReadError> {
        let idx = {
            let meta = self.fetch(item_key).await?;
            meta.dirty = true; // Mark the page as dirty since we're going to modify it
            meta.idx
        };

        Ok(&mut self.pages[idx])
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
