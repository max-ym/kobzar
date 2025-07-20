use std::cmp;

use super::*;

pub type FilePageOffset = u64;

/// Branch fanout for the B-tree index.
/// This is selected so that [Node] and [Leaf] structures fit into a single page
/// of 4KB, which is the typical page size for databases.
pub const BRANCH_FANOUT: usize = 168;

#[derive(Clone)]
#[repr(C)]
pub struct Node {
    /// Keys in the node.
    keys: Keys,

    /// Parent node in the B-tree index.
    parent: FilePageOffset,

    /// Key count in the node.
    keys_count: u64,

    /// Children of the node.
    /// This is a fixed-size array of children, which allows for efficient traversal
    /// of the B-tree index.
    children: [ChildFilePageOffset; BRANCH_FANOUT],
}

impl Node {
    /// Convert a mutable reference to a [Node] from a mutable reference to a generic [IdxPage].
    ///
    /// # Safety
    /// This function is unsafe because it assumes that the provided [IdxPage] represents
    /// a valid [Node] structure.
    pub unsafe fn from_raw(page: &IdxPage) -> &Self {
        unsafe { &*(page.0.as_ptr() as *const Self) }
    }
}

/// File offset used to reference items in the B-tree index, that can
/// indicate absent values.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct OptionFileOffset(FilePageOffset);

impl OptionFileOffset {
    pub const NONE: Self = Self(FilePageOffset::MAX);

    pub const fn is_some(self) -> bool {
        self.0 != Self::NONE.0
    }

    pub const fn get(self) -> Option<FilePageOffset> {
        if self.is_some() { Some(self.0) } else { None }
    }
}

/// Child file offset is used to reference child nodes in the B-tree index.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct ChildFilePageOffset(FilePageOffset);

impl ChildFilePageOffset {
    /// If this bit is set, the child is a leaf node. Otherwise, it is a branch node.
    pub const LEAF_FLAG: u64 = 1 << 63;

    /// Returns true if the child is a leaf node.
    pub const fn is_leaf(self) -> bool {
        self.0 & Self::LEAF_FLAG != 0
    }

    /// Returns the file offset of the child node.
    pub const fn into_inner(self) -> FilePageOffset {
        self.0 & !Self::LEAF_FLAG
    }
}

/// Array of keys in sorted order.
/// This structure is specifically optimized to align compatibly with SIMD operations,
/// which allows for efficient comparison and sorting of keys in the B-tree index.
#[derive(Clone)]
#[repr(align(64))]
pub struct Keys(pub [Key; BRANCH_FANOUT]);

/// A key for the B-tree index.
/// This is a union of two types: [BlobStorable] and an inline key.
/// The actual kind comes from inner index definition in the schema file,
/// which establishes whether the keys being inline or stored as BLOBs in the heap file.
///
/// Note that this only affects the file layout, but DB may reinterpret the key
/// and cache more efficient representation in memory.
#[repr(C)]
#[derive(Clone, Copy)]
pub union Key {
    blob: heap::BlobStorable,
    inline: [u8; 16],
}

/// Leaf entry in the B-tree index.
/// This is a leaf node in the B-tree index, which contains the keys and their corresponding
/// primary index item numbers.
#[repr(C)]
#[derive(Clone)]
pub struct Leaf {
    /// Number of keys in the leaf entry.
    key_count: u64,

    parent: FilePageOffset,

    /// Keys in the leaf entry.
    keys: Keys,

    /// Primary index item numbers for the keys.
    pidx: [pidx::ItemId; BRANCH_FANOUT],
}

impl Leaf {
    /// Convert a mutable reference to a [Leaf] from a mutable reference to a generic [IdxPage].
    ///
    /// # Safety
    /// This function is unsafe because it assumes that the provided [IdxPage] represents
    /// a valid [Leaf] structure.
    pub unsafe fn from_raw(page: &IdxPage) -> &Self {
        unsafe { &*(page.0.as_ptr() as *const Self) }
    }
}

/// File handle for the index.
#[derive(Debug)]
pub struct File {
    file: tokio::fs::File,
}

/// Write handle for the index file.
/// This handle is used to write data to the index file.
#[derive(Debug)]
pub struct Write {
    file: BufWriter<tokio::fs::File>,
    offset: FilePageOffset,
}

impl Write {
    /// Creates a new write handle for the index file.
    pub fn new(file: File) -> Self {
        let file = BufWriter::new(file.file);
        let offset = 0;
        Self { file, offset }
    }

    pub async unsafe fn write_node(&mut self, node: &Node) -> std::io::Result<()> {
        todo!()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SearchOutcome {
    /// The first page ID where the key was matched.
    pub start_page: Id,

    /// The last page ID where the key was matched.
    pub end_page: Id,

    /// The key ID in the start page where the match was found.
    pub start_key_id: u64,

    /// The last key ID in the last page that still matches the search value.
    pub end_key_id: u64,
}

impl SearchOutcome {
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            start_page: self.start_page.min(other.start_page),
            end_page: self.end_page.max(other.end_page),
            start_key_id: self.start_key_id.min(other.start_key_id),
            end_key_id: self.end_key_id.max(other.end_key_id),
        }
    }

    pub fn key_len(&self) -> u64 {
        (self.start_key_id..self.end_key_id).count() as u64
    }
}

/// Search configuration for the B-tree index.
#[derive(Debug, Clone, Copy)]
pub struct SearchCfg<'data> {
    /// Value to search for in the B-tree index.
    pub value: &'data [u8],

    /// Identifier of the database, index ID and the page to be assumed as the root of the B-tree.
    pub root: IdxKey,

    /// Whether the root node is a leaf node.
    pub is_root_leaf: bool,

    /// Whether the index is storing BLOBs or inline data.
    pub is_blobs: bool,

    /// Maximum number of results to return.
    pub limit: u64,
}

impl DbStore {
    /// Search for a value in the B-tree index.
    /// This function will preload the necessary pages and return the search outcome.
    ///
    /// # Safety
    ///
    /// This function relies on the validity of the page passed as `root` argument to
    /// actually be a valid B-tree root node with indicated type.
    pub async unsafe fn btree_search(
        &mut self,
        cfg: SearchCfg<'_>,
    ) -> io::Result<Option<SearchOutcome>> {
        #[derive(Clone)]
        struct CmpValues<'data> {
            // Value being compared.
            val: &'data [u8],
        }

        impl<'data> CmpValues<'data> {
            fn new(val: &'data [u8]) -> Self {
                Self { val }
            }

            /// Compare the value with the given chunk of data. If the chunk is shorter than
            /// the value, we can only compare up to the length of the chunk.
            /// If the chunk is longer than the value, we can only compare up to the length of
            /// the value. Since our inline keys have fixed size, this allows to correctly compare
            /// data types that are shorter than the inline key size.
            fn cmp(&mut self, chunk: &[u8]) -> Option<cmp::Ordering> {
                let chunk = &chunk[0..self.val.len()];
                let result = self.val.cmp(chunk);
                if result != cmp::Ordering::Equal {
                    // We found a difference, return it.
                    Some(result)
                } else if self.remains() == 0 {
                    // We compared all the bytes and they are equal.
                    Some(cmp::Ordering::Equal)
                } else {
                    // We have equal so far, but there are still bytes left to compare.
                    None
                }
            }

            fn remains(&self) -> usize {
                self.val.len()
            }
        }

        fn keys<'page>(cfg: SearchCfg<'_>, current_page: &'page IdxPage) -> &'page [Key] {
            if cfg.is_root_leaf {
                let leaf = unsafe { Leaf::from_raw(current_page) };
                &leaf.keys.0[0..leaf.key_count as usize]
            } else {
                let node = unsafe { Node::from_raw(current_page) };
                &node.keys.0[0..node.keys_count as usize]
            }
        }

        async fn search_blob(
            this: &mut DbStore,
            cfg: SearchCfg<'_>,
        ) -> io::Result<Option<SearchOutcome>> {
            todo!()
        }

        async fn search_inline(
            this: &mut DbStore,
            cfg: SearchCfg<'_>,
        ) -> io::Result<Option<SearchOutcome>> {
            let value: [u8; 16] = {
                let mut buf = [0u8; 16];
                buf[0..cfg.value.len()].copy_from_slice(cfg.value);
                buf
            };
            let cur = this.load_idx(cfg.root).await?.clone();
            let keys = keys(cfg, &cur);

            let found = keys
                .binary_search_by(|key| {
                    let key = unsafe { &key.inline };
                    key.cmp(&value)
                })
                .ok();
            let Some(found) = found else {
                // If the key was not found, return None.
                return Ok(None);
            };
            let mut start_key_id = found as u64;
            let mut end_key_id = found as u64;

            // Check left and right neighbors, adjust start_key_id and end_key_id accordingly.
            while unsafe { keys[start_key_id as usize].inline } == value {
                if start_key_id > 0 {
                    start_key_id -= 1;
                } else {
                    break;
                }
            }
            while unsafe { keys[end_key_id as usize].inline } == value {
                if end_key_id < keys.len() as u64 - 1 {
                    end_key_id += 1;
                } else {
                    break;
                }
            }

            // If we are at the leaf node, we can return the result.
            let outcome = SearchOutcome {
                start_page: cfg.root.page,
                end_page: cfg.root.page,
                start_key_id,
                end_key_id,
            };
            if cfg.is_root_leaf {
                return Ok(Some(outcome));
            }

            // If we are not at the leaf node, we need to traverse the tree.
            let node = unsafe { Node::from_raw(&cur) };
            let mut merged = outcome;
            let mut results = merged.key_len();
            let mut cur = start_key_id;
            while cur < end_key_id && results < cfg.limit {
                let child = node.children[cur as usize];
                let future = search_inline(this, SearchCfg {
                    value: cfg.value,
                    root: IdxKey {
                        page: child.into_inner(),
                        ..cfg.root
                    },
                    is_root_leaf: child.is_leaf(),
                    is_blobs: cfg.is_blobs,
                    limit: cfg.limit - results,
                });

                let Some(result) = Box::pin(future).await? else {
                    // This should not happen, because we already found the key.
                    panic!("Expected to find Some result, but got None");
                };

                results += result.key_len();
                merged = merged.merge(&result);
                cur += 1;
            }

            Ok(Some(merged))
        }

        if cfg.is_blobs {
            search_blob(self, cfg).await
        } else {
            search_inline(self, cfg).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::mem::size_of;

    #[test]
    fn size_of_node() {
        assert_eq!(size_of::<Node>(), INDEX_PAGE_SIZE);
    }

    #[test]
    fn size_of_leaf() {
        assert_eq!(size_of::<Leaf>(), INDEX_PAGE_SIZE);
    }
}
