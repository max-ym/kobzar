use super::*;

pub type FileOffset = u64;

/// Branch fanout for the B-tree index.
/// This is selected so that [Node] and [Leaf] structures fit into a single page
/// of 4KB, which is the typical page size for databases.
pub const BRANCH_FANOUT: usize = 168;

#[derive(Clone)]
#[repr(C)]
pub struct Node {
    /// Parent node in the B-tree index.
    parent: FileOffset,

    /// Key count in the node.
    keys_count: u64,

    /// Keys in the node.
    keys: Keys,

    /// Children of the node.
    /// This is a fixed-size array of children, which allows for efficient traversal
    /// of the B-tree index.
    children: [ChildFileOffset; BRANCH_FANOUT],
}

impl Node {
    /// Convert a mutable reference to a [Node] from a mutable reference to a generic [IdxPage].
    ///
    /// # Safety
    /// This function is unsafe because it assumes that the provided [IdxPage] represents
    /// a valid [Node] structure.
    pub unsafe fn from_raw(page: &mut IdxPage) -> &mut Self {
        unsafe { &mut *(page.0.as_mut_ptr() as *mut Self) }
    }
}

/// File offset used to reference items in the B-tree index, that can
/// indicate absent values.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct OptionFileOffset(FileOffset);

impl OptionFileOffset {
    pub const NONE: Self = Self(FileOffset::MAX);

    pub const fn is_some(self) -> bool {
        self.0 != Self::NONE.0
    }

    pub const fn get(self) -> Option<FileOffset> {
        if self.is_some() { Some(self.0) } else { None }
    }
}

/// Child file offset is used to reference child nodes in the B-tree index.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct ChildFileOffset(FileOffset);

impl ChildFileOffset {
    /// If this bit is set, the child is a leaf node. Otherwise, it is a branch node.
    pub const LEAF_FLAG: u64 = 1 << 63;

    /// Returns true if the child is a leaf node.
    pub const fn is_leaf(self) -> bool {
        self.0 & Self::LEAF_FLAG != 0
    }

    /// Returns the file offset of the child node.
    pub const fn into_inner(self) -> FileOffset {
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
/// This is a union of two types: `BlobStorable` and an inline key.
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

    parent: FileOffset,

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
    pub unsafe fn from_raw(page: &mut IdxPage) -> &mut Self {
        unsafe { &mut *(page.0.as_mut_ptr() as *mut Self) }
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
    offset: FileOffset,
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
