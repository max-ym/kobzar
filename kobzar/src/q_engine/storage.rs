//! Database storage engine and WAL (Write-Ahead Logging) implementation.
//!
//! # Write-Ahead Logging (WAL)
//!
//! The Write-Ahead Logging (WAL) is a mechanism that ensures durability and consistency of
//! the database. It records all changes to the database before they are applied, allowing
//! for recovery in case of a crash or failure. The WAL is implemented as a sequence of
//! log entries, each representing a change to the database. The log entries are written
//! to a file in a specific format, which allows for efficient recovery and replay of the
//! changes.
//!
//! WAL logs are split onto several files of similar size, and are being recycled once the
//! changes are written to the main database files. Size of WAL is controlled by corresponding
//! configuration options, and is usually set to a few megabytes. The WAL files are named
//! after the LSN (Log Sequence Number) of the first entry in the file, which allows to order
//! the files and find the latest one.
//!
//! WAL is also used to replicate changes to other nodes in the cluster, if the database is
//! running in a cluster mode. In this case, the WAL is used to send changes to other nodes,
//! which then apply the changes to their own databases. This allows for efficient replication
//! and ensures that all nodes in the cluster have the same data.
//!
//! # Database Schema
//!
//! Schema of the database is defined by the types and their fields. Each type has a unique
//! identifier, and each field has a unique identifier within the type.
//!
//! The database schema is stored in the separate file.
//!
//! # Data
//!
//! The data is stored in the following file structure:
//!
//! - Primary Index File
//! - Heap file
//!
//! ## Primary Index File
//!
//! The primary index file contains the metadata about the data stored in the heap file.
//! All entries in the primary index file are the same size, which allows for efficient
//! seek operations through binary search.
//!
//! Each entry represents a record in the heap file, and stores information about
//! whether the record is deleted, its size, its offset in the heap file,
//! `xmin`, `xmax`, next and previous record versions, type's schema version, other metadata.
//!
//! ## Heap File
//!
//! Heap file contains the actual data records.
//! Heap file can have variable-length records, which are stored in a
//! compact binary format. Schema version of the record is stored in the primary index file,
//! and the same heap file can contain records of different version within the same
//! data type in the schema. Basically, when the schema is changed, the heap file
//! can still contain records of the old version, and new records will be written in the
//! new format. This allows for efficient schema evolution and backward compatibility.

use super::*;

use smallvec::{SmallVec, smallvec};
use std::borrow::Cow;
use std::io::SeekFrom;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, BufWriter};

pub mod layout;

pub mod wal;

/// Heap primary index file, which is used to store metadata about the records in the heap file.
pub mod pidx;

/// Heap file, which is used to store the actual data records.
/// The heap file can have variable-length records, which are stored in a compact binary format.
///
/// Heap is split by table data and BLOB data.
/// The tables are organized into tablespace files, which are used to store the data
/// for each table in the database in the same file. This heap file has associated BLOB file.
/// BLOB file is used to store large binary objects (BLOBs) per tablespace.
/// BLOBs are stored in a separate heap file to allow more compact storage of the main heap file,
/// improving read performance.
///
/// Tablespace can have only one table, effectively making it a normal table file. For small
/// databases, tablespace can be used to store all tables in the same file, which allows for
/// less file system overhead and potentially better performance and maintainability.
pub mod heap;

/// Schema of the database is stored in two files.
/// First file is index file which maps schema versions to their offsets in the schema file.
/// Second file is the schema file itself, which contains the actual schema data - the
/// information about the types and their fields.
/// Heap primary index records reference the schema version of the record by the offset
/// in the schema file. The separation is made so to allow for defragmentation of the schema file,
/// since we only need to update the offset in the index file when the schema is defragmented.
pub mod schema;

/// Code is stored in separate raw-code files, which are referenced by the code primary index.
/// Compiled code is stored in separate files that are compiled as dynamic libraries
/// and can be loaded at runtime.
///
/// The raw code and compiled libraries are named after the index ID of the code record,
/// which allows for efficient lookup and loading of the code.
pub mod code;

/// Common code for indexes.
pub mod idx_common;

/// B-tree index implementation for the database data indexing.
/// B-tree index is used to store the data in a sorted order, which allows for efficient
/// search and retrieval of the data. B-tree index is implemented as a separate file,
/// which is used to store the index data. B-tree index can be used to index any
/// data type, and can be used to index multiple fields of the same data type.
pub mod idx_btree;

/// Index page size in bytes.
/// This is the size of the page used to store the index of heap data.
/// This size is common for all index implementations, and is used to
/// store the index data in a compact format.
pub const INDEX_PAGE_SIZE: usize = 4096;

#[repr(align(64))]
pub struct IdxPage([u8; INDEX_PAGE_SIZE]);

type Id = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IdxKey {
    /// Index identifier.
    pub index: Id,

    /// Database identifier.
    pub db: Id,

    /// Page identifier.
    pub page: Id,
}

impl idx_common::KeyExt for IdxKey {
    type PageBoundKey = Self;

    fn page_bound(&self) -> Self::PageBoundKey {
        *self
    }

    fn filename(&self) -> Cow<'static, str> {
        format!("{:x}.idx", self.index).into()
    }
}

/// Database storage engine.
pub struct DbStore {
    tys: HashMap<TypeId, schema::EngineTypeSchema>,
    pidx: pidx::PageStore,
    heap_idx: idx_common::PageStore<IdxKey, IdxPage, { size_of::<IdxPage>() }>,
}

impl DbStore {
    pub fn open(_path: PathBuf) -> std::io::Result<Self> {
        todo!()
    }
}

/// Change operation issued on the heap record. This allows to selectively modify parts
/// of the record, copying other parts as is to form a new record version entry.
#[derive(Debug)]
pub struct ChangeOp<'data> {
    /// Index of the field to change.
    idx: u32,

    /// New value for the field.
    value: &'data [u8],
}

/// Structure to help collect all edits over bitmap.
/// This is used to efficiently apply changes to the bitmap in a single write operation when
/// the bitmap is written to the disk.
#[derive(Debug)]
pub struct BitmapEdit {
    /// Bitmap with values being edited.
    /// We use `u64` words to store the bitmap, which allows for efficient bitwise operations.
    bitmap: SmallVec<[u64; 8]>,

    /// Mask of changed bits.
    mask: SmallVec<[u64; 8]>,
}

impl BitmapEdit {
    /// Create a new bitmap edit with the given bitmap and mask.
    pub fn new(bits: usize) -> Self {
        let len = bits / 64 + 1;
        Self {
            bitmap: smallvec![0; len],
            mask: smallvec![0; len],
        }
    }

    /// Update the bitmap at the given offset with the given value.
    /// Value should fit into the mask (defined by size), otherwise it will corrupt the bitmap
    /// by overwriting the bits outside of the mask.
    ///
    /// # Panic
    /// Panics if the size and offset overflow the bitmap.
    pub fn set(&mut self, offset: usize, size: u8, value: u8) {
        let word = offset / 64;
        let bit = (offset % 64) as u8;
        let overflow = size + bit > 64;
        debug_assert_eq!(
            value,
            value & ((1 << size) - 1),
            "Value must fit into the mask defined by size"
        );

        if overflow {
            self.set_inner(word + 1, 0, size + bit - 64, value >> (64 - bit));
            self.set_inner(word, bit, 64 - bit, value & ((1 << (64 - bit)) - 1));
        } else {
            self.set_inner(word, bit, size, value);
        }
    }

    #[inline]
    fn set_inner(&mut self, word: usize, bit: u8, size: u8, value: u8) {
        let size = size as usize;
        let value = value as u64;

        let mask = ((1 << size) - 1) << bit;
        self.bitmap[word] &= !mask;
        self.bitmap[word] |= value << bit;
    }

    /// Write the bitmap into the given buffer, masking the existing values to
    /// prevent overwriting the bits that are not changed.
    pub fn apply_to(&self, buf: &mut [u64]) {
        debug_assert_eq!(buf.len(), self.bitmap.len());
        for (i, word) in self.bitmap.iter().copied().enumerate() {
            buf[i] = buf[i] & !self.mask[i] | word;
        }
    }
}
