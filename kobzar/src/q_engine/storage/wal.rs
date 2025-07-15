use super::*;

use blake2::Digest;
use blake2::digest::consts::U32;
use blake2::digest::generic_array::GenericArray;
use tokio::fs::{File, OpenOptions};

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
    /// `generation: u64` - generation of the transaction that created this entry
    /// `kind_code: u64`
    /// `array of bytes representing the entry data`
    /// `padding to align to 8 bytes`
    ///
    /// We use padding to ensure that the entry is aligned to 8 bytes,
    /// which will make processing more efficient for modern 64-bit CPUs.
    async fn write_entry(&mut self, tx: Generation, entry: &WalEntry<'_>) -> Result<(), WalError> {
        let kind_code = entry.wal_code();

        let mut size = 0u64;
        let mut hasher = blake2::Blake2b::new();

        size += 8; // for entry size
        size += 32; // for blake2 hash (256 bits = 32 bytes)
        size += 8; // for transaction generation
        size += 8; // for kind_code
        hasher.update(&tx.to_le_bytes());
        hasher.update(&kind_code.to_le_bytes());
        write_entry(entry, async |data| {
            size += data.len() as u64;
            hasher.update(data);
            Ok(())
        })
        .await?;
        let padding = 8 - (size % 8);

        let hash: GenericArray<u8, U32> = hasher.finalize();
        debug_assert_eq!(hash.len(), 32, "Blake2b hash should be 32 bytes long");

        // Make actual write to the file.
        self.file.write_all(&size.to_le_bytes()).await?;
        self.file.write_all(&hash).await?;
        self.file.write_all(&tx.to_le_bytes()).await?;
        self.file.write_all(&kind_code.to_le_bytes()).await?;
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
                    storage,
                    record,
                    data,
                } => {
                    write(&storage.to_le_bytes()).await?;
                    write(&record.to_le_bytes()).await?;
                    write(data).await?;
                }
                Update {
                    storage,
                    old_record,
                    new_record,
                    data,
                } => {
                    write(&storage.to_le_bytes()).await?;
                    write(&old_record.to_le_bytes()).await?;
                    write(&new_record.to_le_bytes()).await?;
                    write(data).await?;
                }
                Delete { storage, record } => {
                    write(&storage.to_le_bytes()).await?;
                    write(&record.to_le_bytes()).await?;
                }
                Commit => {
                    // No data to write for Commit
                }
                Rollback => {
                    // No data to write for Rollback
                }
                CreateTable {
                    type_id,
                    kind,
                    store_behavior,
                    name,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&kind.wal_code_bytes()).await?;
                    write(&store_behavior.wal_code_bytes()).await?;
                    write(name.as_bytes()).await?;
                }
                DropTable { type_id } => {
                    write(&type_id.to_le_bytes()).await?;
                }
                CommentTable { type_id, comment } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(comment.as_bytes()).await?;
                }
                RenameTable { type_id, new_name } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(new_name.as_bytes()).await?;
                }
                CreateTableValidation {
                    type_id,
                    name,
                    code,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&(name.as_bytes().len() as u64).to_le_bytes()).await?;
                    write(name.as_bytes()).await?;
                    write(code.as_bytes()).await?;
                }
                CommentTableValidation {
                    type_id,
                    validation_id,
                    comment,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&validation_id.to_le_bytes()).await?;
                    write(comment.as_bytes()).await?;
                }
                DropTableValidation {
                    type_id,
                    validation_id,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&validation_id.to_le_bytes()).await?;
                }
                RenameTableValidation {
                    type_id,
                    validation_id,
                    new_name,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&validation_id.to_le_bytes()).await?;
                    write(new_name.as_bytes()).await?;
                }
                CreateAdtVariant { type_id, name } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(name.as_bytes()).await?;
                }
                CommentAdtVariant {
                    type_id,
                    variant_id,
                    comment,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(comment.as_bytes()).await?;
                }
                RenameAdtVariant {
                    type_id,
                    variant_id,
                    new_name,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(new_name.as_bytes()).await?;
                }
                DropAdtVariant {
                    type_id,
                    variant_id,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                }
                BindSchemalessField {
                    type_id,
                    field_type,
                    field_name,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&field_type.to_le_bytes()).await?;
                    write(field_name.as_bytes()).await?;
                }
                UnbindSchemalessField {
                    type_id,
                    field_type,
                    field_name,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&field_type.to_le_bytes()).await?;
                    write(field_name.as_bytes()).await?;
                }
                CreateTableTrigger {
                    type_id,
                    kind,
                    code,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&kind.wal_code_bytes()).await?;
                    write(code.as_bytes()).await?;
                }
                DropTableTrigger { type_id, kind } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&kind.wal_code_bytes()).await?;
                }
                CreateFieldTriggerQuery {
                    type_id,
                    variant_id,
                    field_id,
                    kind,
                    code,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&field_id.to_le_bytes()).await?;
                    write(&kind.wal_code_bytes()).await?;
                    write(code.as_bytes()).await?;
                }
                DropFieldTrigger {
                    type_id,
                    variant_id,
                    field_id,
                    kind,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&field_id.to_le_bytes()).await?;
                    write(&kind.wal_code_bytes()).await?;
                }
                CreateField {
                    type_id,
                    variant_id,
                    field_type,
                    field_name,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&field_type.to_le_bytes()).await?;
                    write(field_name.as_bytes()).await?;
                }
                SetFieldDefault {
                    type_id,
                    variant_id,
                    field_id,
                    default_value,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&field_id.to_le_bytes()).await?;
                    write(default_value).await?;
                }
                SetFieldComputed {
                    type_id,
                    variant_id,
                    field_id,
                    code
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&field_id.to_le_bytes()).await?;
                    write(code.as_bytes()).await?;
                }
                SetFieldCheck {
                    type_id,
                    variant_id,
                    field_id,
                    code
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&field_id.to_le_bytes()).await?;
                    write(code.as_bytes()).await?;
                }
                SetFieldTransform {
                    type_id,
                    variant_id,
                    field_id,
                    code
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&field_id.to_le_bytes()).await?;
                    write(code.as_bytes()).await?;
                }
                CommentField {
                    type_id,
                    variant_id,
                    field_id,
                    comment,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&field_id.to_le_bytes()).await?;
                    write(comment.as_bytes()).await?;
                }
                RenameField {
                    type_id,
                    variant_id,
                    field_id,
                    new_name,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&field_id.to_le_bytes()).await?;
                    write(new_name.as_bytes()).await?;
                }
                DropField {
                    type_id,
                    variant_id,
                    field_id,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&field_id.to_le_bytes()).await?;
                }
                CreateIndexUnique {
                    type_id,
                    variant_id,
                    fields,
                    none_is_unique,
                    name,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&(fields.len() as u64).to_le_bytes()).await?;
                    for field_id in *fields {
                        write(&field_id.to_le_bytes()).await?;
                    }
                    write(&(*none_is_unique as u8).to_le_bytes()).await?;
                    write(name.as_bytes()).await?;
                }
                CreateIndexOrder {
                    type_id,
                    variant_id,
                    fields,
                    name,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&(fields.len() as u64).to_le_bytes()).await?;
                    for field_cfg in *fields {
                        write(&field_cfg.field_idx.to_le_bytes()).await?;
                        let flags = field_cfg.is_ascending as u8
                            | (field_cfg.none_is_first as u8) << 1;
                        write(&flags.to_le_bytes()).await?;
                    }
                    write(name.as_bytes()).await?;
                }
                CreateIndexEqual {
                    type_id,
                    variant_id,
                    fields,
                    name,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&(fields.len() as u64).to_le_bytes()).await?;
                    for field_id in *fields {
                        write(&field_id.to_le_bytes()).await?;
                    }
                    write(name.as_bytes()).await?;
                }
                CommentIndex {
                    type_id,
                    variant_id,
                    index_id,
                    comment,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&index_id.to_le_bytes()).await?;
                    write(comment.as_bytes()).await?;
                }
                RenameIndex {
                    type_id,
                    variant_id,
                    index_id,
                    new_name,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&index_id.to_le_bytes()).await?;
                    write(new_name.as_bytes()).await?;
                }
                DropIndex {
                    type_id,
                    variant_id,
                    index_id,
                } => {
                    write(&type_id.to_le_bytes()).await?;
                    write(&variant_id.to_le_bytes()).await?;
                    write(&index_id.to_le_bytes()).await?;
                }
                CreateFn {
                    self_arg,
                    returns,
                    by_ref,
                    name,
                    code,
                } => {
                    write(&self_arg.to_le_bytes()).await?;
                    write(&returns.to_le_bytes()).await?;
                    write(&(*by_ref as u8).to_le_bytes()).await?;
                    write(name.as_bytes()).await?;
                    write(code.as_bytes()).await?;
                }
                CommentFn {
                    self_arg,
                    name,
                    comment,
                } => {
                    write(&self_arg.to_le_bytes()).await?;
                    write(&(name.len() as u64).to_le_bytes()).await?;
                    write(name.as_bytes()).await?;
                    write(comment.as_bytes()).await?;
                }
                RenameFn {
                    self_arg,
                    old_name,
                    new_name,
                } => {
                    write(&self_arg.to_le_bytes()).await?;
                    write(&(old_name.len() as u64).to_le_bytes()).await?;
                    write(old_name.as_bytes()).await?;
                    write(new_name.as_bytes()).await?;
                }
                DropFn {
                    self_arg,
                    name,
                } => {
                    write(&self_arg.to_le_bytes()).await?;
                    write(name.as_bytes()).await?;
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
    /// Insert a new record into the storage.
    Insert {
        storage: Id,
        record: Id,
        data: &'data [u8],
    },

    /// Update an existing record in the storage. This inserts a new record and marks
    /// the old record as deleted.
    Update {
        storage: Id,
        old_record: Id,
        new_record: Id,
        data: &'data [u8],
    },

    /// Delete a record from the storage. This marks the record as deleted
    /// and does not remove it from the storage immediately.
    Delete {
        storage: Id,
        record: Id,
    },

    /// Commit a transaction, which means that all changes made by the transaction
    /// will be applied to the storage. The start of the transaction is not explicitly logged,
    /// but it can be tracked by the advanced generation number introduced in the WAL.
    Commit,

    /// Rollback a transaction, which means that all changes made by the transaction
    /// will be discarded and the storage will be restored to the state before the transaction.
    Rollback,

    /// Create a new table with the given ID.
    CreateTable {
        type_id: Id,
        kind: TableKind,
        store_behavior: TableStoreBehavior,
        name: &'data str,
    },

    /// Drop a table with the given ID.
    DropTable {
        type_id: Id,
    },

    /// Create a comment for a table.
    CommentTable {
        type_id: Id,
        comment: &'data str,
    },

    /// Rename a table with the given ID.
    RenameTable {
        type_id: Id,
        new_name: &'data str,
    },

    CreateTableValidation {
        type_id: Id,
        name: &'data str,
        code: &'data str,
    },

    CommentTableValidation {
        type_id: Id,
        validation_id: Id,
        comment: &'data str,
    },

    DropTableValidation {
        type_id: Id,
        validation_id: Id,
    },

    RenameTableValidation {
        type_id: Id,
        validation_id: Id,
        new_name: &'data str,
    },

    CreateAdtVariant {
        type_id: Id,
        name: &'data str,
    },

    CommentAdtVariant {
        type_id: Id,
        variant_id: Id,
        comment: &'data str,
    },

    RenameAdtVariant {
        type_id: Id,
        variant_id: Id,
        new_name: &'data str,
    },

    DropAdtVariant {
        type_id: Id,
        variant_id: Id,
    },

    BindSchemalessField {
        type_id: Id,
        field_type: TypeId, // Set to INVALID if not specified ("any" type)
        field_name: &'data str,
    },

    UnbindSchemalessField {
        type_id: Id,
        field_type: TypeId, // The same as passed in Bind operation
        field_name: &'data str,
    },

    CreateTableTrigger {
        type_id: Id,
        kind: TriggerExecKind,
        code: &'data str,
    },

    DropTableTrigger {
        type_id: Id,
        kind: TriggerExecKind,
    },

    CreateFieldTriggerQuery {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        field_id: Id,
        kind: TriggerExecKind,
        code: &'data str,
    },

    DropFieldTrigger {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        field_id: Id,
        kind: TriggerExecKind,
    },

    CreateField {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        field_type: TypeId,
        field_name: &'data str,
    },

    SetFieldDefault {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        field_id: Id,
        default_value: &'data [u8],
    },

    SetFieldComputed {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        field_id: Id,
        code: &'data str,
    },

    SetFieldCheck {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        field_id: Id,
        code: &'data str,
    },

    SetFieldTransform {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        field_id: Id,
        code: &'data str,
    },

    CommentField {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        field_id: Id,
        comment: &'data str,
    },

    RenameField {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        field_id: Id,
        new_name: &'data str,
    },

    DropField {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        field_id: Id,
    },

    CreateIndexUnique {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        fields: &'data [Id],
        none_is_unique: bool,
        name: &'data str,
    },

    CreateIndexOrder {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        fields: &'data [OrderIndexFieldCfg],
        name: &'data str,
    },

    CreateIndexEqual {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        fields: &'data [Id],
        name: &'data str,
    },

    CommentIndex {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        index_id: Id,
        comment: &'data str,
    },

    RenameIndex {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        index_id: Id,
        new_name: &'data str,
    },

    DropIndex {
        type_id: Id,
        variant_id: Id, // Should be MAX for non-ADT tables
        index_id: Id,
    },

    CreateFn {
        self_arg: TypeId, // INVALID if not a method
        returns: TypeId,
        by_ref: bool, // true if self is passed by reference, ignored if self_arg is INVALID
        name: &'data str,
        code: &'data str,
    },

    CommentFn {
        self_arg: TypeId, // INVALID if not a method
        name: &'data str,
        comment: &'data str,
    },

    RenameFn {
        self_arg: TypeId, // INVALID if not a method
        old_name: &'data str,
        new_name: &'data str,
    },

    DropFn {
        self_arg: TypeId, // INVALID if not a method
        name: &'data str,
    },
}

impl WalCode for WalEntry<'_> {
    fn wal_code(&self) -> u64 {
        use WalEntry::*;
        match self {
            Insert { .. } => 0x01,
            Update { .. } => 0x02,
            Delete { .. } => 0x03,
            Commit { .. } => 0x05,
            Rollback { .. } => 0x06,
            CreateTable { .. } => 0x07,
            DropTable { .. } => 0x08,
            CommentTable { .. } => 0x09,
            RenameTable { .. } => 0x0A,
            CreateTableValidation { .. } => 0x0B,
            CommentTableValidation { .. } => 0x0C,
            DropTableValidation { .. } => 0x0D,
            RenameTableValidation { .. } => 0x0E,
            CreateAdtVariant { .. } => 0x0F,
            CommentAdtVariant { .. } => 0x10,
            RenameAdtVariant { .. } => 0x11,
            DropAdtVariant { .. } => 0x12,
            BindSchemalessField { .. } => 0x13,
            UnbindSchemalessField { .. } => 0x14,
            CreateTableTrigger { .. } => 0x15,
            DropTableTrigger { .. } => 0x16,
            CreateFieldTriggerQuery { .. } => 0x17,
            DropFieldTrigger { .. } => 0x18,
            CreateField { .. } => 0x19,
            SetFieldDefault { .. } => 0x1A,
            SetFieldComputed { .. } => 0x1B,
            SetFieldCheck { .. } => 0x1C,
            SetFieldTransform { .. } => 0x1D,
            CommentField { .. } => 0x1E,
            RenameField { .. } => 0x1F,
            DropField { .. } => 0x20,
            CreateIndexUnique { .. } => 0x21,
            CreateIndexOrder { .. } => 0x22,
            CreateIndexEqual { .. } => 0x23,
            CommentIndex { .. } => 0x24,
            RenameIndex { .. } => 0x25,
            DropIndex { .. } => 0x26,
            CreateFn { .. } => 0x27,
            CommentFn { .. } => 0x28,
            RenameFn { .. } => 0x29,
            DropFn { .. } => 0x2A,
        }
    }
}

impl WalCode for TableKind {
    fn wal_code(&self) -> u64 {
        use TableKind::*;
        match self {
            Schemafull => 0x01,
            SchemafullAdt => 0x02,
            Schemaless => 0x03,
            SchemafullSchemaless => 0x04,
            SchemafullAdtSchemaless => 0x05,
        }
    }
}

impl WalCode for TableStoreBehavior {
    fn wal_code(&self) -> u64 {
        use TableStoreBehavior::*;
        match self {
            DirectlyStorable => 0x01,
            FieldOnly => 0x02,
            Singleton => 0x03,
        }
    }
}

impl WalCode for TriggerExecKind {
    fn wal_code(&self) -> u64 {
        use TriggerExecKind::*;
        match self {
            BeforeInsert => 0x01,
            BeforeUpdate => 0x02,
            BeforeDelete => 0x03,
            AfterInsert => 0x04,
            AfterUpdate => 0x05,
            AfterDelete => 0x06,
        }
    }
}

/// Additional error types
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
