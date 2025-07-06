use super::*;

use std::path::PathBuf;

pub mod layout;

pub mod wal;

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
