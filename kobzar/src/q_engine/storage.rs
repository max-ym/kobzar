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

/// Primary index file, which is used to store metadata about the records in the heap file.
pub mod pidx;

type Id = u64;

/// Database storage engine.
pub struct DbStore {
    // TODO
}

impl DbStore {
    pub fn open(_path: PathBuf) -> std::io::Result<Self> {
        Ok(DbStore {
            // TODO
        })
    }
}
