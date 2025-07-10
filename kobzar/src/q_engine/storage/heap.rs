use tokio::fs;

use super::*;

/// A metadata that describes a BLOB record in the heap file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct BlobStorable {
    /// BLOB offset determines where the BLOB data starts in the BLOB file.
    /// If this is set to [u64::MAX], it means that the BLOB is not stored in the BLOB file,
    /// but is stored inline in the record itself.
    pub blob_offset: u64,

    /// The size of the BLOB data in bytes.
    pub blob_size: u64,
}

impl BlobStorable {
    /// Returns true if the BLOB is stored inline in the record.
    pub fn is_inline(&self) -> bool {
        self.blob_offset == u64::MAX
    }
}

#[derive(Debug)]
pub struct HeapFile {
    /// File to operate on.
    file: fs::File,
}

impl HeapFile {

}
