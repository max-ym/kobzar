use thiserror::Error;

/// ID of a type in the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TypeId(u64);

impl std::fmt::Display for TypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_build_in() {
            write!(f, "B")?;
        }
        if self.is_custom() {
            write!(f, "C")?;
        }
        if self.is_derived() {
            write!(f, "D")?;
        }
        if self.is_option() {
            write!(f, "O")?;
        }

        write!(f, "{}", self.discard_mask())
    }
}

/// Data size in bytes. Can also be used as an offset to navigate through data structures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct DataSize(u64);

macro_rules! ty_const {
    ($value:expr, $name:ident, $opt_name:ident) => {
        pub const $name: TypeId = TypeId($value | TypeId::BUILT_IN_BIT);
        pub const $opt_name: TypeId = TypeId($value | TypeId::BUILT_IN_BIT | TypeId::OPTION_BIT);
    };

    // Sucessively add one to each next value.
    ($start:expr, $name:ident, $opt_name:ident, $($tt:tt),+ $(,)?) => {
        ty_const!($start, $name, $opt_name);
        ty_const!($start + 1, $($tt),+);
    };
}

impl TypeId {
    /// Invalid type ID, used to represent an uninitialized or invalid type.
    pub const INVALID: TypeId = TypeId(0);

    /// Optional type bit.
    pub const OPTION_BIT: u64 = 1 << 63;

    /// Built-in type bit.
    pub const BUILT_IN_BIT: u64 = 0b01 << 61;

    /// Custom type bit, for user-defined types or plugins.
    pub const CUSTOM_BIT: u64 = 0b10 << 61;

    /// Derived type bit, for types that are derived from other types.
    /// These normally are types that have generics, hence many concrete types
    /// based on those generic types can be derived.
    /// Example: `Vec<String>`.
    pub const DERIVED_BIT: u64 = 0b11 << 61;

    /// Mask for the bits that are used to identify the type.
    pub const FLAG_MASK: u64 = 0b111 << 61;

    /// Mask for the bits that are used to identify the type kind (derived, custom, built-in).
    pub const TYPE_KIND_MASK: u64 = 0b11 << 61;

    ty_const! {
        1, // Reserve "0" type for invalid type.
        BOOL, BOOL_OPT,
        I8, I8_OPT,
        I16, I16_OPT,
        I32, I32_OPT,
        I64, I64_OPT,
        I128, I128_OPT,
        U8, U8_OPT,
        U16, U16_OPT,
        U32, U32_OPT,
        U64, U64_OPT,
        U128, U128_OPT,
        F32, F32_OPT,
        F64, F64_OPT,
        STR, STR_OPT,
        DATE, DATE_OPT,
        TIME, TIME_OPT,
        DATETIME, DATETIME_OPT,
        UUID, UUID_OPT,
        SET, SET_OPT, // Set<T>
        MAP, MAP_OPT, // Map<K, V>
        FK, FK_OPT, // generic foreign key
    }

    ty_const! {
        1 | TypeId::DERIVED_BIT,

        VEC_U8, VEC_U8_OPT, // for byte arrays
        VEC_U64, VEC_U64_OPT, // predefine for U64 (PK) arrays
        VEC_U128, VEC_U128_OPT, // predefine for U128 (PK) arrays
        VEC, VEC_OPT, // generic Vec<T>
        VEC_STR, VEC_STR_OPT, // String arrays
        VEC_UUID, VEC_UUID_OPT, // predefine for UUID (PK) arrays

        FK_U32, FK_U32_OPT,
        FK_U64, FK_U64_OPT,
        FK_U128, FK_U128_OPT,
        FK_STR, FK_STR_OPT,
        FK_UUID, FK_UUID_OPT,
        FK_DATE, FK_DATE_OPT,
        FK_TIME, FK_TIME_OPT,
        FK_DATETIME, FK_DATETIME_OPT, // foreign key to DateTime
        FK_VEC_U8, FK_VEC_U8_OPT, // foreign key to Vec<u8>
        VEC_FK_U64, VEC_FK_U64_OPT, // array of foreign keys to U64
        VEC_FK_U128, VEC_FK_U128_OPT, // array of foreign keys to U128
        VEC_FK_STR, VEC_FK_STR_OPT, // array of foreign keys to String
        VEC_FK_UUID, VEC_FK_UUID_OPT, // array of foreign keys to UUID
        VEC_FK_DATE, VEC_FK_DATE_OPT, // array of foreign keys to Date
        VEC_FK_TIME, VEC_FK_TIME_OPT, // array of foreign keys to Time
        VEC_FK_DATETIME, VEC_FK_DATETIME_OPT, // array of foreign keys to DateTime
        VEC_FK_VEC_U8, VEC_FK_VEC_U8_OPT, // array of foreign keys to Vec<u8>
        SET_FK_U64, SET_FK_U64_OPT, // Set of foreign keys to U64
        SET_FK_U128, SET_FK_U128_OPT, // Set of foreign keys to U128
        SET_FK_STR, SET_FK_STR_OPT, // Set of foreign keys to String
        SET_FK_UUID, SET_FK_UUID_OPT, // Set of foreign keys to UUID
        SET_FK_DATE, SET_FK_DATE_OPT, // Set of foreign keys to Date
        SET_FK_TIME, SET_FK_TIME_OPT, // Set of foreign keys to Time
        SET_FK_DATETIME, SET_FK_DATETIME_OPT, // Set of foreign keys to DateTime
        SET_FK_VEC_U8, SET_FK_VEC_U8_OPT, // Set of foreign keys to Vec<u8>
    }

    pub const fn is_build_in(self) -> bool {
        self.0 & Self::TYPE_KIND_MASK == Self::BUILT_IN_BIT
    }

    pub const fn is_custom(self) -> bool {
        self.0 & Self::TYPE_KIND_MASK == Self::CUSTOM_BIT
    }

    pub const fn is_derived(self) -> bool {
        self.0 & Self::TYPE_KIND_MASK == Self::DERIVED_BIT
    }

    pub const fn is_option(self) -> bool {
        self.0 & Self::OPTION_BIT != 0
    }

    pub const fn discard_mask(self) -> u64 {
        self.0 & !Self::FLAG_MASK
    }

    pub const unsafe fn new_unchecked(id: u64) -> Self {
        Self(id)
    }

    /// Advance to the next type ID. Can be used to generate new type IDs sequentially.
    pub const fn advance(self) -> Self {
        TypeId(self.0 + 1)
    }
}

impl From<TypeId> for u64 {
    fn from(id: TypeId) -> Self {
        id.0
    }
}

impl From<DataSize> for u64 {
    fn from(size: DataSize) -> Self {
        size.0 as u64
    }
}

impl From<u64> for DataSize {
    fn from(size: u64) -> Self {
        DataSize(size)
    }
}

/// Inner type representation variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InnerTypeRepr {
    /// Normal data type, stored inline in the record.
    Normal {
        /// Size in bytes.
        bytes: DataSize,

        /// Bits for sub-byte data, if applicable.
        /// For example, an `Option<T>` type will have 1 bit for the presence of the value,
        /// or an enum with 4 variants will have 2 bits for the value.
        bits: u8,
    },

    /// BLOB data type, stored separately in the BLOB storage.
    ///
    /// Blob entry has the following layout:
    ///
    /// 1. BLOB ID
    /// 2. Length of the BLOB data
    /// 3. Inlined data (if any)
    /// 4. Hash of the BLOB data (if `eq_hash` is true)
    BlobPtr {
        /// Inline size of the BLOB data. If this is less than the size of the BLOB,
        /// only the first `inline_size` bytes are stored inline in the record,
        /// and the rest is stored in the BLOB storage.
        /// This allows for efficient storage of small BLOBs inline, while still
        /// allowing for larger BLOBs to be stored separately.
        /// This is useful for types like `String`, where we can store the first
        /// `inline_size` bytes inline in the record, and the rest in the BLOB storage,
        /// which will allow for faster comparison and lookups
        /// when the inline size is small enough.
        inline_size: DataSize,

        /// Extra bits for inlined part of the BLOB data.
        /// For example, if BLOB is a [Option<String>], it will have 1 bit for the presence of the value.
        bits: u8,

        /// Whether to store hash for equality checks.
        /// If true, this will also store length of the BLOB data,
        /// which will allow for fast equality checks without loading the entire BLOB.
        eq_hash: bool,
    },
}

#[derive(Debug, Error)]
#[error("Type ID {0} does not represent a primitive type")]
pub struct NonPrimitiveTypeError(TypeId);

impl TryFrom<TypeId> for InnerTypeRepr {
    type Error = NonPrimitiveTypeError;

    fn try_from(type_id: TypeId) -> Result<Self, Self::Error> {
        if !type_id.is_build_in() {
            return Err(NonPrimitiveTypeError(type_id));
        }

        use InnerTypeRepr::*;
        let v = match type_id {
            TypeId::BOOL => Normal {
                bytes: DataSize(0),
                bits: 1,
            },
            TypeId::BOOL_OPT => Normal {
                bytes: DataSize(0),
                bits: 2,
            },
            TypeId::I8 | TypeId::U8 => Normal {
                bytes: DataSize(1),
                bits: 0,
            },
            TypeId::I8_OPT | TypeId::U8_OPT => Normal {
                bytes: DataSize(1),
                bits: 1,
            },
            TypeId::I16 | TypeId::U16 => Normal {
                bytes: DataSize(2),
                bits: 0,
            },
            TypeId::I16_OPT | TypeId::U16_OPT => Normal {
                bytes: DataSize(2),
                bits: 1,
            },
            TypeId::I32 | TypeId::U32 | TypeId::FK_U32 | TypeId::F32 => Normal {
                bytes: DataSize(4),
                bits: 0,
            },
            TypeId::I32_OPT | TypeId::U32_OPT | TypeId::FK_U32_OPT | TypeId::F32_OPT => Normal {
                bytes: DataSize(4),
                bits: 1,
            },
            TypeId::I64 | TypeId::U64 | TypeId::FK_U64 | TypeId::F64 => Normal {
                bytes: DataSize(8),
                bits: 0,
            },
            TypeId::I64_OPT | TypeId::U64_OPT | TypeId::FK_U64_OPT | TypeId::F64_OPT => Normal {
                bytes: DataSize(8),
                bits: 1,
            },
            TypeId::I128 | TypeId::U128 | TypeId::FK_U128 | TypeId::UUID | TypeId::FK_UUID => {
                Normal {
                    bytes: DataSize(16),
                    bits: 0,
                }
            }
            TypeId::I128_OPT
            | TypeId::U128_OPT
            | TypeId::FK_U128_OPT
            | TypeId::UUID_OPT
            | TypeId::FK_UUID_OPT => Normal {
                bytes: DataSize(16),
                bits: 1,
            },
            TypeId::STR
            | TypeId::VEC
            | TypeId::SET
            | TypeId::MAP
            | TypeId::FK_STR
            | TypeId::FK_VEC_U8
            | TypeId::VEC_FK_U64
            | TypeId::VEC_FK_U128
            | TypeId::VEC_FK_STR
            | TypeId::VEC_FK_UUID
            | TypeId::VEC_FK_DATE
            | TypeId::VEC_FK_TIME
            | TypeId::VEC_FK_DATETIME
            | TypeId::VEC_FK_VEC_U8
            | TypeId::SET_FK_U64
            | TypeId::SET_FK_U128
            | TypeId::SET_FK_STR
            | TypeId::SET_FK_UUID
            | TypeId::SET_FK_DATE
            | TypeId::SET_FK_DATETIME
            | TypeId::SET_FK_TIME
            | TypeId::SET_FK_VEC_U8 => BlobPtr {
                inline_size: DataSize(0),
                bits: 0,
                eq_hash: false,
            },
            TypeId::STR_OPT
            | TypeId::VEC_OPT
            | TypeId::SET_OPT
            | TypeId::MAP_OPT
            | TypeId::FK_STR_OPT
            | TypeId::FK_VEC_U8_OPT
            | TypeId::VEC_FK_U64_OPT
            | TypeId::VEC_FK_U128_OPT
            | TypeId::VEC_FK_STR_OPT
            | TypeId::VEC_FK_UUID_OPT
            | TypeId::VEC_FK_DATE_OPT
            | TypeId::VEC_FK_TIME_OPT
            | TypeId::VEC_FK_DATETIME_OPT
            | TypeId::VEC_FK_VEC_U8_OPT
            | TypeId::SET_FK_U64_OPT
            | TypeId::SET_FK_U128_OPT
            | TypeId::SET_FK_STR_OPT
            | TypeId::SET_FK_UUID_OPT
            | TypeId::SET_FK_DATE_OPT
            | TypeId::SET_FK_DATETIME_OPT
            | TypeId::SET_FK_TIME_OPT
            | TypeId::SET_FK_VEC_U8_OPT => BlobPtr {
                inline_size: DataSize(0),
                bits: 1,
                eq_hash: false,
            },
            _ => {
                return Err(NonPrimitiveTypeError(type_id));
            }
        };
        Ok(v)
    }
}

impl std::ops::Add for DataSize {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        DataSize(self.0 + other.0)
    }
}

impl std::ops::AddAssign for DataSize {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}
