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

use std::path::PathBuf;

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

type Id = u64;

/// Database storage engine.
pub struct DbStore {
    tys: HashMap<TypeId, schema::EngineTypeSchema>,
    pidx: pidx::PageStore,
}

impl DbStore {
    pub fn open(_path: PathBuf) -> std::io::Result<Self> {
        todo!()
    }
}

/// Change operation issued on the heap record. This allows to selectively modify parts
/// of the record, copying other parts as is to form a new record version entry.
#[derive(Debug)]
pub enum ChangeOp<'data> {
    Field {
        /// Index of the field to change.
        idx: u32,

        /// New value for the field.
        value: &'data [u8],

        /// Sub-byte part of the field to change.
        bits: u8,

        /// Number of bits in the sub-byte part.
        bits_cnt: u8,
    },
}
