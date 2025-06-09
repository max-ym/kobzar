/// ID of a type in the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TypeId(u64);

/// Data size in bytes. Can also be used as an offset to navigate through data structures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct DataSize(u64);

macro_rules! ty_const {
    ($value:expr, $name:ident, $opt_name:ident) => {
        pub const $name: TypeId = TypeId($value);
        pub const $opt_name: TypeId = TypeId($value | TypeId::OPTION_BIT);
    };

    // Sucessively add one to each next value.
    ($start:expr, $name:ident, $opt_name:ident, $($tt:tt),+ $(,)?) => {
        ty_const!($start, $name, $opt_name);
        ty_const!($start + 1, $($tt),+);
    };
}

impl TypeId {
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
        0,
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
        VEC_U8, VEC_U8_OPT, // for byte arrays
        VEC_U64, VEC_U64_OPT, // predefine for U64 (PK) arrays
        VEC_U128, VEC_U128_OPT, // predefine for U128 (PK) arrays
        VEC, VEC_OPT, // generic Vec<T>
        VEC_STR, VEC_STR_OPT, // String arrays
        DATE, DATE_OPT,
        TIME, TIME_OPT,
        DATETIME, DATETIME_OPT,
        UUID, UUID_OPT,
        VEC_UUID, VEC_UUID_OPT, // predefine for UUID (PK) arrays
        SET, SET_OPT, // Set<T>
        MAP, MAP_OPT, // Map<K, V>

        FK, FK_OPT, // generic foreign key
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

    pub const unsafe fn new_unchecked(id: u64) -> Self {
        Self(id)
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
