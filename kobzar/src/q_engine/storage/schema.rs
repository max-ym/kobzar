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
    /// Whether hash is included to allow for quick equality checks.
    /// Not useful for small fields (< ≈64 bytes).
    pub const HAS_HASH: Self = Self(1 << 0);
}

impl std::ops::BitOr for FieldFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for FieldFlags {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::BitOrAssign for FieldFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAndAssign for FieldFlags {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl std::ops::Not for FieldFlags {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(!self.0)
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
    
    /// The size of the inline data in bytes.
    /// Not applicable for small fixed-size fields, then it is zero.
    pub inline_size: u16,

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

    /// The size of the record in bytes.
    pub size: u32,

    /// The number of ADT variants described next. Zero if this is not an ADT.
    pub adt_variant_count: u32,
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
/// as well as normal schemaful tables.
#[derive(Debug)]
#[repr(C)]
pub struct TypedPart {
    /// Number of fields with type information.
    pub field_count: u16,
}
