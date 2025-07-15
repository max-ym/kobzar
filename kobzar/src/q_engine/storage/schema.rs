use std::mem::transmute;

use super::*;

/// Offset in the file, used to reference items in the schema file.
pub type FileOffset = u64;

/// Version of the schema, used to track changes in the schema. This is the ID into
/// the index file, which maps schema versions to their offsets in the schema file.
pub type Version = u32;

/// Flags for field metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct FieldFlags(u32);

impl FieldFlags {
    /// Mask for the sub-byte length of the field.
    /// 0 means that the field only has full bytes, and no sub-byte length.
    /// 1-7 indicate the number of bits used for the sub-byte length.
    pub const SUBBYTE_MASK: u32 = 0b111 << 0;

    /// Whether hash is included to allow for quick equality checks.
    /// Not useful for small fields (< ≈64 bytes).
    pub const HASH: Self = Self(1 << 3);

    /// Whether this field is a BLOB. This changes the structure of the field's header in the
    /// heap file.
    pub const BLOB: Self = Self(1 << 4);

    /// Clear sub-byte length bits.
    pub fn without_subbyte(self) -> Self {
        Self(self.0 & !Self::SUBBYTE_MASK)
    }
}

impl std::ops::BitOr for FieldFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.without_subbyte().0)
    }
}

impl std::ops::BitAnd for FieldFlags {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.without_subbyte().0)
    }
}

impl std::ops::BitOrAssign for FieldFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.without_subbyte().0;
    }
}

impl std::ops::BitAndAssign for FieldFlags {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.without_subbyte().0;
    }
}

impl std::ops::Not for FieldFlags {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(!self.without_subbyte().0)
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Field {
    /// Type ID of the field. For "any" type, where applicable, this is set to zero.
    pub type_id: u64,

    /// Offset of the field name in the schema file.
    pub name_offset: FileOffset,

    /// Offset of the optional comment in the schema file. [u64::MAX] if not set.
    pub comment_offset: FileOffset,

    /// Schema version of the field, which is the ID into the index file.
    /// When the underlying type schema is changed, the field version here remains the same,
    /// but once the new record of the type this field belongs to is created,
    /// the new version of the entire enclosing type is created,
    /// and the field version is updated to the new version there.
    pub schema_version: Version,

    /// Offset of the field name in the schema file.
    pub name_len: u16,
    
    /// The size limit of the inline data in bytes.
    /// Note that this is always the size of the actual field in the record,
    /// which all have constant sizes among entries of the same type, to optimize
    /// lookup.
    pub inline_size: u32,

    /// Flags for the field.
    pub flags: FieldFlags,

    /// Optional validation function ID. [u32::MAX] if not set.
    pub validation: code::Id,

    /// Optional trigger before insert function ID. [u32::MAX] if not set.
    pub trigger_before_insert: code::Id,

    /// Optional trigger after insert function ID. [u32::MAX] if not set.
    pub trigger_after_insert: code::Id,

    /// Optional trigger before update function ID. [u32::MAX] if not set.
    pub trigger_before_update: code::Id,

    /// Optional trigger after update function ID. [u32::MAX] if not set.
    pub trigger_after_update: code::Id,
    
    /// Optional trigger before delete function ID. [u32::MAX] if not set.
    pub trigger_before_delete: code::Id,

    /// Optional trigger after delete function ID. [u32::MAX] if not set.
    pub trigger_after_delete: code::Id,

    /// Offset of the default value in the schema file.
    pub default_value_offset: FileOffset,
}

/// Header of each record in the schema file.
#[derive(Debug)]
#[repr(C)]
pub struct RecordHeader {
    /// Optional comment about the type.
    /// Offset in the schema file. [u64::MAX] if not set.
    pub comment_offset: FileOffset,

    /// The number of ADT variants described next. Zero if this is not an ADT.
    pub adt_variant_count: u32,

    /// The ID of the tablespace this record belongs to.
    pub tablespace_id: u32,
}

#[derive(Debug)]
#[repr(C)]
pub struct AdtVariant {
    /// The name of the ADT variant.
    pub name_offset: FileOffset,

    /// The offset of the first field in the schema file.
    pub first_field_offset: FileOffset,

    /// The offset of the optional comment in the schema file.
    /// [u64::MAX] if not set.
    pub comment_offset: FileOffset,

    /// The length of the name in bytes.
    pub name_len: u16,

    /// The number of fields in this ADT variant.
    pub field_count: u16,
}

/// Schemafull part of the table record. Schemafull-schemaless tables have this part,
/// as well do normal schemaful tables.
#[derive(Debug)]
#[repr(C)]
pub struct FieldCount {
    /// Number of fields with type information.
    pub field_count: u16,
}

/// ADT variant information for the engine.
#[derive(Debug)]
pub struct EngineAdtVariant {
    /// The name of the ADT variant.
    pub name: String,

    /// The comment about the ADT variant.
    pub comment: Option<String>,

    /// The fields of the ADT variant.
    pub fields: Vec<EngineField>,
}

/// The complete type schema, built from the schema file to use in the engine.
#[derive(Debug)]
pub struct EngineTypeSchema {
    /// The name of the type.
    pub name: String,

    /// The comment about the type.
    pub comment: Option<String>,

    /// Kind of the schema.
    pub kind: EngineSchemaKind,
}

#[derive(Debug)]
pub enum EngineSchemaKind {
    /// Schemaful table.
    Schemaful(Vec<EngineField>),

    /// Schemaless table.
    Schemaless,

    /// ADT type.
    Adt(Vec<EngineAdtVariant>),
}

#[derive(Debug)]
pub struct EngineField {
    /// The name of the field.
    pub name: String,

    /// Comment about the field.
    pub comment: Option<String>,

    /// The type ID of the field.
    pub type_id: TypeId,

    pub validation: Option<code::Id>,
    pub trigger_before_insert: Option<code::Id>,
    pub trigger_after_insert: Option<code::Id>,
    pub trigger_before_update: Option<code::Id>,
    pub trigger_after_update: Option<code::Id>,
    pub trigger_before_delete: Option<code::Id>,
    pub trigger_after_delete: Option<code::Id>,
}

#[derive(Debug)]
pub struct File {
    file: tokio::fs::File,
}

impl File {
    pub async fn open(path: PathBuf) -> std::io::Result<Self> {
        let file = tokio::fs::File::open(path).await?;
        Ok(Self { file })
    }
}

#[derive(Debug)]
pub struct Read(BufReader<tokio::fs::File>);

macro_rules! read {
    ($this:ident, $it:ident) => {{
        let mut buf = [0u8; std::mem::size_of::<$it>()];
        $this.0.read_exact(&mut buf).await?;
        Ok(unsafe { transmute(buf) })
    }}
}

impl Read {
    pub fn new(file: tokio::fs::File) -> Self {
        Self(BufReader::new(file))
    }

    pub fn into_inner(self) -> tokio::fs::File {
        self.0.into_inner()
    }

    /// Goto the given record by offset in the schema file,
    /// and read the record header.
    /// 
    /// # Safety
    /// This function does not validate whether the read data is actually a valid record header.
    pub async unsafe fn goto_record_read(&mut self, offset: FileOffset) -> std::io::Result<RecordHeader> {
        self.0.seek(std::io::SeekFrom::Start(offset)).await?;
        read!(self, RecordHeader)
    }

    /// Read the ADT variant at the current position, advancing the cursor.
    /// 
    /// # Safety
    /// This function does not validate whether the read data is actually a valid ADT variant
    /// record.
    pub async unsafe fn read_adt_variant(&mut self) -> std::io::Result<AdtVariant> {
        read!(self, AdtVariant)
    }

    /// Read field count of schemaful-* table record at the current position,
    /// advancing the cursor.
    /// 
    /// # Safety
    /// This function does not validate whether the read data is actually a valid field count
    /// record.  
    pub async unsafe fn read_field_count(&mut self) -> std::io::Result<FieldCount> {
        read!(self, FieldCount)
    }

    /// Read the field at the current position, advancing the cursor.
    /// 
    /// # Safety
    /// This function does not validate whether the read data is actually a valid field
    /// record.
    pub async unsafe fn read_field(&mut self) -> std::io::Result<Field> {
        read!(self, Field)
    }
}
