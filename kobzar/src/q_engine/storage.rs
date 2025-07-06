use blake2::digest::consts::U32;
use blake2::digest::generic_array::GenericArray;
use blake2::Digest;
use std::io::SeekFrom;
use std::path::PathBuf;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};

use super::*;

pub mod layout;

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

/// Write-ahead log (WAL) for the database.
/// It is used to store changes to the database before they are written to the storage.
/// This allows to recover the database in case of a crash or a power failure.
/// The WAL is a sequential log of changes, which can be replayed to restore the database
/// to a consistent state.
pub struct Wal {
    /// Path to the WAL-containing directory.
    path: PathBuf,

    /// Current WAL file handle wrapped into [tokio::io::BufWriter].
    file: BufWriter<File>,

    /// Offset LSN (Log Sequence Number), which is used to calculate global LSN
    /// in the WAL for the current file, which contains only a fragment of the WAL.
    offset_lsn: u64,
}

impl Wal {
    /// Create or open the latest WAL file.
    pub async fn open_latest(path: PathBuf) -> std::io::Result<Self> {
        let lsn = Self::latest_file_lsn(&path).await?;
        Self::open_by_lsn(path, lsn, true).await
    }

    /// Open the WAL file by LSN. The LSN is used to determine the file name.
    /// The file name is expected to be in the format `main_<lsn>.wal`.
    /// If `create` is true, the file will be created if it does not exist.
    /// If `create` is false, the file must exist.
    /// If the file does not exist, an error will be returned.
    pub async fn open_by_lsn(path: PathBuf, lsn: u64, create: bool) -> std::io::Result<Self> {
        let capacity = cfg().device_at(&path).io_combine_bytes as usize;

        let file = OpenOptions::new()
            .create(create)
            .append(true)
            .read(true)
            .open(path.join(format!("main_{}.wal", lsn)))
            .await?;

        Ok(Wal {
            path,
            file: BufWriter::with_capacity(capacity, file),
            offset_lsn: lsn,
        })
    }

    /// Find the latest WAL file LSN.
    /// To get the latest file, we look for files named `main_<lsn>.wal`,
    /// where <lsn> is the log sequence number.
    async fn latest_file_lsn(path: &PathBuf) -> std::io::Result<u64> {
        let mut max_lsn = None;
        let mut lsns = WalFileLsnStream::new(path).await?;
        while let Some(lsn) = lsns.next().await? {
            let max_lsn = max_lsn.get_or_insert(0);
            if lsn > *max_lsn {
                *max_lsn = lsn;
            }
        }

        let max_lsn = max_lsn.unwrap_or(0);
        Ok(max_lsn)
    }

    /// Flush pending writes to disk.
    pub async fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush().await?;
        Ok(())
    }

    /// Force sync to disk (fsync).
    pub async fn sync(&mut self) -> std::io::Result<()> {
        self.flush().await?;
        self.file.get_ref().sync_all().await?;
        Ok(())
    }

    pub async fn read_at_offset(&mut self, offset: u64) -> std::io::Result<Option<WalEntry>> {
        // Seek to the position of the entry
        self.file.seek(SeekFrom::Start(offset)).await?;
        // Read the entry
        todo!()
    }

    /// Get the current offset in the WAL file.
    pub async fn current_offset(&mut self) -> std::io::Result<u64> {
        self.file.seek(SeekFrom::End(0)).await
    }

    /// Calculate the current LSN based on the end of current WAL file and its beginning offset.
    pub async fn current_lsn(&mut self) -> std::io::Result<u64> {
        let offset = self.current_offset().await?;
        Ok(self.offset_lsn + offset)
    }

    /// Stop writing current WAL file and start writing a new one.
    pub async fn cut_wal(&mut self) -> std::io::Result<()> {
        let lsn = self.current_lsn().await?;
        self.file.flush().await?;
        self.file.get_ref().sync_all().await?;
        *self = Wal::open_by_lsn(self.path.clone(), lsn, true).await?;
        Ok(())
    }

    /// Drop old WAL segments before the given LSN. Note that this only drops the files based
    /// on the LSN number in their file name. This can effectively drop WAL that has greater records
    /// than the given LSN because LSN number from the file name would
    /// be less than the given LSN, and less than the file's max LSN (which should be calculated
    /// from the file's size).
    pub async fn drop_files_before_lsn(&mut self, lsn: u64) -> Result<(), ArchiveError> {
        let mut lsns = WalFileLsnStream::new(&self.path).await?;
        while let Some(file_lsn) = lsns.next().await? {
            if file_lsn < lsn {
                tokio::fs::remove_file(self.path.join(format!("main_{file_lsn}.wal"))).await?;
            }
        }
        Ok(())
    }

    /// Write a WAL entry to the file.
    ///
    /// The entry has the following format:
    /// `size: u64` - size of the entry in bytes (without final padding)
    /// `blake2 hash: u256` - hash of the entry data
    /// `kind_flag: u8`
    /// `array of bytes representing the entry data`
    /// `padding to align to 8 bytes`
    /// 
    /// We use padding to ensure that the entry is aligned to 8 bytes,
    /// which will make processing more efficient for modern 64-bit CPUs.
    async fn write_entry(&mut self, entry: &WalEntry<'_>) -> Result<(), WalError> {
        use WalEntry::*;
        let kind_flag: u8 = match entry {
            Insert { .. } => 0x01,
            Update { .. } => 0x02,
            Delete { .. } => 0x03,
            CreateStorage { .. } => 0x04,
            DropStorage { .. } => 0x05,
        };

        let mut size = 0u64;
        let mut hasher = blake2::Blake2b::new();
        
        size += 8; // for entry size
        size += 32; // for blake2 hash (256 bits = 32 bytes)
        size += 1; // for kind_flag
        hasher.update(&[kind_flag]);
        write_entry(entry, async |data| {
            size += data.len() as u64;
            hasher.update(data);
            Ok(())
        }).await?;
        let padding = 8 - (size % 8);

        let hash: GenericArray<u8, U32> = hasher.finalize();
        debug_assert_eq!(hash.len(), 32, "Blake2b hash should be 32 bytes long");

        // Make actual write to the file.
        self.file.write_all(&size.to_le_bytes()).await?;
        self.file.write_all(&hash).await?;
        self.file.write_all(&[kind_flag]).await?;
        write_entry(entry, async |data| self.file.write_all(data).await).await?;
        for _ in 0..padding {
            self.file.write_all(&[0]).await?; // padding with zeros
        }

        async fn write_entry(
            entry: &WalEntry<'_>,
            mut write: impl AsyncFnMut(&[u8]) -> std::io::Result<()>,
        ) -> std::io::Result<()> {
            use WalEntry::*;
            match entry {
                Insert {
                    tx,
                    storage,
                    record,
                    data,
                } => {
                    write(&tx.to_le_bytes()).await?;
                    write(&storage.to_le_bytes()).await?;
                    write(&record.to_le_bytes()).await?;
                    write(data).await?;
                }
                Update {
                    tx,
                    storage,
                    old_record,
                    new_record,
                    data,
                } => {
                    write(&tx.to_le_bytes()).await?;
                    write(&storage.to_le_bytes()).await?;
                    write(&old_record.to_le_bytes()).await?;
                    write(&new_record.to_le_bytes()).await?;
                    write(data).await?;
                }
                Delete {
                    tx,
                    storage,
                    record,
                } => {
                    write(&tx.to_le_bytes()).await?;
                    write(&storage.to_le_bytes()).await?;
                    write(&record.to_le_bytes()).await?;
                }
                CreateStorage { tx, storage } => {
                    write(&tx.to_le_bytes()).await?;
                    write(&storage.to_le_bytes()).await?;
                }
                DropStorage { tx, storage } => {
                    write(&tx.to_le_bytes()).await?;
                    write(&storage.to_le_bytes()).await?;
                }
            }
            Ok(())
        }

        Ok(())
    }
}

/// WAL entry types for writing to the Write-Ahead Log (WAL).
/// These entries represent different types of operations that can be logged,
/// such as inserting, updating, or deleting records, as well as creating or dropping storage.
#[derive(Debug, Clone)]
pub enum WalEntry<'data> {
    Insert {
        tx: Generation,
        storage: Id,
        record: Id,
        data: &'data [u8],
    },
    Update {
        tx: Generation,
        storage: Id,
        old_record: Id,
        new_record: Id,
        data: &'data [u8],
    },
    Delete {
        tx: Generation,
        storage: Id,
        record: Id,
    },
    CreateStorage {
        tx: Generation,
        storage: Id,
    },
    DropStorage {
        tx: Generation,
        storage: Id,
    },
}

impl<'data> WalEntry<'data> {
    /// Write the entry to the WAL.
    pub async fn write_to_wal(&self, wal: &mut Wal) -> Result<(), WalError> {
        wal.write_entry(self).await?;
        Ok(())
    }
}

// Additional error types
#[derive(Debug, Error)]
pub enum WalError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] SerializationError),
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("Storage with ID {0} does not exist")]
    StorageNotFound(Id),
    #[error("Record with ID {0} does not exist")]
    RecordNotFound(Id),
    #[error(transparent)]
    Wal(#[from] WalError),
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("Read error: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Corrupt WAL entry")]
    CorruptEntry,
}

#[derive(Debug, Error)]
pub enum CheckpointError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Wal(#[from] WalError),
}

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum SerializationError {
    #[error("Failed to serialize WAL entry")]
    SerializeFailed,
    #[error("Failed to deserialize WAL entry")]
    DeserializeFailed,
}

struct WalFileLsnStream {
    read_dir: tokio::fs::ReadDir,
}

impl WalFileLsnStream {
    pub async fn new(path: &PathBuf) -> std::io::Result<Self> {
        let read_dir = tokio::fs::read_dir(&path)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(WalFileLsnStream { read_dir })
    }

    pub async fn next(&mut self) -> std::io::Result<Option<u64>> {
        if let Some(entry) = self.read_dir.next_entry().await? {
            let file_name = entry.file_name();
            if let Some(name_str) = file_name.to_str() {
                if let Some(lsn_str) = name_str
                    .strip_prefix("main_")
                    .and_then(|s| s.strip_suffix(".wal"))
                {
                    if let Ok(lsn) = lsn_str.parse::<u64>() {
                        return Ok(Some(lsn));
                    }
                }
            }
        }
        Ok(None)
    }
}

/// The metadata for a record in the storage.
/// This is a packed structure to make sure it compiles into the same layout,
/// as we store this in the file system. It has to be compatible among different versions
/// of Rust compilers and even different architectures.
///
/// For field documentation, see [RecHeader].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(packed)]
struct RecHeaderStorable {
    xmin: Generation,
    xmax: Generation,
    next_version: Id,
    prev_version: Id,
    data_ptr: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecHeader {
    /// Generation when the record was created. MAX means the record is not valid.
    pub xmin: Generation,

    /// Generation when the record was discarded. MAX means the record is still valid.
    pub xmax: Generation,

    /// The ID of the next version of the record. Zero means that this is the latest version.
    /// Note that even if the next version exists in some uncommitted transaction,
    /// this is still considered the latest version, up until that transaction is committed.
    pub next_version: Id,

    /// The ID of the previous version of the record. Zero means that this is the first version.
    /// Note that this previous' record's next_version may not point to this record.
    /// This can happen if this record was created by a transaction that was not committed.
    pub prev_version: Id,

    /// The offset of the record in the storage.
    pub data_ptr: u64,
}

impl From<RecHeaderStorable> for RecHeader {
    fn from(header: RecHeaderStorable) -> Self {
        Self {
            xmin: header.xmin,
            xmax: header.xmax,
            next_version: header.next_version,
            prev_version: header.prev_version,
            data_ptr: header.data_ptr,
        }
    }
}

impl From<RecHeader> for RecHeaderStorable {
    fn from(header: RecHeader) -> Self {
        Self {
            xmin: header.xmin,
            xmax: header.xmax,
            next_version: header.next_version,
            prev_version: header.prev_version,
            data_ptr: header.data_ptr,
        }
    }
}
