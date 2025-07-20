use super::*;

/// Heap file offset is smaller, as it is also used in Primary Index File
/// to reference records in the heap file, and thus the record is limited to 4GB.
pub type HeapFileOffset = u32;

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
    /// Returns true if the BLOB is stored inline in the target data record (not the index record).
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
    /// Opens the heap file at the given path.
    pub async fn open(path: PathBuf) -> std::io::Result<Self> {
        let file = fs::File::open(path).await?;
        Ok(Self { file })
    }
}

macro_rules! read {
    ($this:ident, $it:ident) => {{
        let mut buf = [0u8; std::mem::size_of::<$it>()];
        $this.file.read_exact(&mut buf).await?;
        Ok(unsafe { std::mem::transmute(buf) })
    }};
}

/// A reader for the heap file, which adds buffering to the file operations.
#[derive(Debug)]
pub struct HeapRead<'file> {
    file: BufReader<&'file mut fs::File>,
    record_start: u64,
}

impl<'file> HeapRead<'file> {
    pub fn new(file: &'file mut HeapFile) -> Self {
        Self {
            file: BufReader::new(&mut file.file),
            record_start: 0,
        }
    }

    /// Goto the given record by offset in the heap file,
    /// and set the record start offset to the given value.
    pub async fn goto_record_at(&mut self, offset: HeapFileOffset) -> std::io::Result<()> {
        let offset = offset as u64;
        self.file.get_mut().seek(SeekFrom::Start(offset)).await?;
        self.record_start = offset;
        Ok(())
    }

    /// Read the bitmap of the given size in bytes.
    ///
    /// # Safety
    /// This function does not validate whether the read data is actually a valid bitmap nor
    /// whether the current read position is at the start of the bitmap. It has no guarantees
    /// on the size of the actual bitmap to match the size of the requested one via the buffer size.
    pub async unsafe fn read_bitmap(&mut self, bitmap: &mut [u8]) -> std::io::Result<()> {
        let cnt = self.file.get_mut().read_exact(bitmap).await?;
        if cnt == bitmap.len() {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "short bitmap read",
            ))
        }
    }

    /// Read the ADT variant at the current position, advancing the cursor.
    ///
    /// # Safety
    /// This function does not validate whether the read data is actually a valid ADT variant
    /// record.
    pub async unsafe fn adt_variant(&mut self) -> std::io::Result<u32> {
        read!(self, u32)
    }

    /// Read the next BLOB field header record at the current position,
    /// advancing the cursor.
    ///
    /// # Safety
    /// This function does not validate whether the read data is actually a valid BLOB storable
    /// record.
    pub async unsafe fn read_blob_storable(&mut self) -> std::io::Result<BlobStorable> {
        read!(self, BlobStorable)
    }

    /// Read the inline data at the current position, advancing the cursor.
    ///
    /// # Safety
    /// This function has no validation agains the type of data being read,
    /// and does not guarantee that the data is actually inline field data.
    pub async unsafe fn read_inline(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        let cnt = self.file.get_mut().read_exact(buf).await?;
        if cnt == buf.len() {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "short inline read",
            ))
        }
    }
}

/// A file that stores BLOB data.
/// This is a separate file from the heap file, and is used to store BLOB data
/// that is too large to fit in the heap file record itself.
/// The BLOB data is stored in a separate file to avoid bloating the heap file.
#[derive(Debug)]
pub struct BlobFile {
    /// File to operate on.
    file: fs::File,
}

impl BlobFile {
    /// Opens the BLOB file at the given path.
    pub async fn open(path: PathBuf) -> std::io::Result<Self> {
        let file = fs::File::open(path).await?;
        Ok(Self { file })
    }

    /// Returns the underlying file.
    pub fn into_inner(self) -> fs::File {
        self.file
    }

    /// Read the BLOB data by the given offset. The size is determined by the size of the buffer.
    ///
    /// # Errors
    /// Returns all IO errors that normally can occur. In addition to that,
    /// reading any other amount of bytes than the buffer size is considered an error.
    pub async fn read(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(offset)).await?;
        let cnt = self.file.read_exact(buf).await?;
        if cnt == buf.len() {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "short BLOB read",
            ))
        }
    }
}

/// A writer for the heap file, which adds buffering to the file operations.
#[derive(Debug)]
pub struct HeapWrite<'file> {
    file: BufWriter<&'file mut fs::File>,
}

impl<'file> HeapWrite<'file> {
    pub fn new(file: &'file mut HeapFile) -> Self {
        Self {
            file: BufWriter::new(&mut file.file),
        }
    }

    /// Get current position in the file.
    pub async fn position(&mut self) -> std::io::Result<HeapFileOffset> {
        self.file
            .stream_position()
            .await
            .map(|pos| pos as HeapFileOffset)
    }

    pub async fn move_to(&mut self, offset: HeapFileOffset) -> std::io::Result<()> {
        let offset = offset as u64;
        self.file.seek(SeekFrom::Start(offset)).await?;
        Ok(())
    }

    /// Write the bitmap of the given size in bytes.
    ///
    /// # Safety
    /// This function does not ensure current position is valid to write a bitmap.
    pub async unsafe fn write_bitmap(&mut self, bitmap: &[u8]) -> std::io::Result<()> {
        self.file.write_all(bitmap).await?;
        Ok(())
    }

    /// Write the ADT variant at the current position, advancing the cursor.
    ///
    /// # Safety
    /// This function does not ensure current position is valid to write an ADT variant.
    pub async unsafe fn write_adt_variant(&mut self, variant: u32) -> std::io::Result<()> {
        self.file.write_all(&variant.to_le_bytes()).await?;
        Ok(())
    }

    /// Write the BLOB storable record at the current position, advancing the cursor.
    ///
    /// # Safety
    /// This function does not ensure current position is valid to write a BLOB storable record.
    pub async unsafe fn write_blob_storable(&mut self, blob: BlobStorable) -> std::io::Result<()> {
        self.file.write_all(&blob.blob_offset.to_le_bytes()).await?;
        self.file.write_all(&blob.blob_size.to_le_bytes()).await?;
        Ok(())
    }

    /// Write the inline data at the current position, advancing the cursor.
    ///
    /// # Safety
    /// This function does not ensure current position is valid to write inline data,
    /// and does not validate the data being written.
    pub async fn write_inline(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.file.write_all(buf).await?;
        Ok(())
    }

    /// Store a field hash value at the current position.
    ///
    /// # Safety
    /// This function does not ensure current position is valid to write a field hash.
    pub async fn store_hash(&mut self, hash: u128) -> std::io::Result<()> {
        self.file.write_all(&hash.to_le_bytes()).await?;
        Ok(())
    }

    /// Store a BLOB file offset for a BLOB field at the current position.
    ///
    /// # Safety
    /// This function does not ensure current position is valid to write a BLOB file offset.
    pub async fn store_blob_offset(&mut self, offset: u64) -> std::io::Result<()> {
        self.file.write_all(&offset.to_le_bytes()).await?;
        Ok(())
    }
}
