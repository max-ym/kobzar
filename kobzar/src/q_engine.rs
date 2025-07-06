use std::ops::Deref;

use super::*;
use smallvec::SmallVec;
use thiserror::Error;
use tokio::sync::RwLock;

/// Data types and structures storeable and queryable in Kobzar.
mod dt;
pub use dt::*;

/// Query structure.
mod query;
pub(crate) use query::*;

/// Executable foreign functions, that can be defined by the user for custom operations,
/// field validations, transformations, etc.
/// These functions can be used in queries, and are compiled to machine code for fast execution.
mod ffi;

/// Storage layer, which is responsible for storing and retrieving data from the database,
/// as well as implementing caching and indexing strategies.
mod storage;
pub use storage::*;

/// Valid identifier for a type in the schema.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentPath(String);

impl Deref for IdentPath {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for IdentPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// Database read-write lock.
pub type DbRw = RwLock<Db>;

/// Generation of the schema.
/// This is used to track transaction generations and ensure that
/// the schema is consistent across transactions.
/// Each time the transaction is created,
/// the generation is incremented, so that other transactions could account for a "point in time"
/// of the schema can mask their view of schema to access only changes that were made before
/// the transaction started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Generation(u64);

impl std::fmt::Display for Generation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "x{:X}", self.0)
    }
}

impl From<u64> for Generation {
    fn from(value: u64) -> Self {
        Generation(value)
    }
}

impl From<Generation> for u64 {
    fn from(value: Generation) -> Self {
        value.0.into()
    }
}

impl Generation {
    /// Increment the generation by one.
    pub fn advance(self) -> Self {
        Generation(self.0 + 1)
    }

    pub fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }
}

/// Schema information and database controls for some database in the server.
#[derive(Debug)]
pub struct Db {
    /// The name of the database.
    name: IdentPath,

    /// Last type ID used in the schema.
    /// This is used to generate unique type IDs for new types in the schema.
    last_type_id: TypeId,

    /// Next transaction generation of the database.
    last_generation: Generation,

    /// Types that are defined in the schema.
    types: Types,

    /// Active transactions in the database.
    active_transactions: HashMap<Generation, TransactionLocal>,

    /// The parent transaction (value generation) of the child transaction (key).
    /// This is used to track the transaction hierarchy and ensure that
    /// child can see the changes made by the parent transaction, by looking up the parent
    /// generation in this map.
    transaction_nesting: HashMap<Generation, Generation>,
}

/// Local transaction context, which can be used to store temporary data
/// during the execution of a transaction.
/// This is used to store data that is not yet committed to the database,
/// and can be used to perform operations that are not yet finalized.
#[derive(Debug, Default)]
pub struct TransactionLocal {
    /// Isolation level of the transaction, which defines how the transaction
    /// interacts with other transactions and the visibility of changes.
    isolation: TransactionIsolation,

    /// Local types that were defined in the transaction,
    /// but not yet committed to the database schema.
    ///
    /// Currently, we allow only to create new types in the transactions,
    /// but to simplify the implementation, we don't allow to modify existing types,
    /// or to remove them. Such operations are always applied to the main schema
    /// directly.
    types: Types,
}

/// Transaction isolation, which can be used to define the behavior of the transaction data
/// access and visibility.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionIsolation {
    /// All committed changes from other transactions are visible to the current transaction.
    /// Changes made by the current transaction are not visible to other transactions
    /// until the transaction is committed.
    #[default]
    ReadCommitted,

    /// Transaction registers conditions for the data it accesses. If any of the conditions
    /// would yield other set of records than the ones that were visible at the start of
    /// the transaction, due to actions of other transactions, this transaction will not
    /// be allowed to commit, unless those external changes are acknowledged by the transaction.
    GuardedReadCommitted,

    /// Transaction only is able to read data that was committed at the time
    /// the transaction started. Any changes made by other transactions after the start
    /// of this transaction are not visible to it. If there were changes
    /// to the data that this transaction read, those changes are not
    /// notified to the current transaction even on commit - it will be able to commit
    /// anyhow with no errors or warnings. However, if the transaction tries to modify
    /// the data that was changed by other transactions, it will receive an error on commit attempt.
    PointOfTime,
}

/// Types that are defined in the schema. This does not include built-in types,
/// as they are managed separately. This is used to store user-defined types,
/// derived types, and generic instances that are defined in the schema.
#[derive(Debug, Default)]
pub struct Types {
    /// Map of structure names to their type IDs.
    /// This is used to quickly look up the type ID of a structure by its name.
    /// Also this allows to list all structures in the schema.
    ///
    /// Note that this map can contain dangling references to types that were removed
    /// from the schema, so if you need to check the existence of a structure,
    /// you should also check the `types` map for the type ID.
    /// If the structure was removed, it will still be present in this map,
    /// but the type ID will not be present in the `types` map.
    struct_names_to_id: HashMap<IdentPath, TypeId>,

    /// All derived and user-defined types in the schema.
    /// Note that built-in types are not included here, as they are managed separately.
    types: HashMap<TypeId, TypeKind>,
}

impl Types {
    /// Ensure that the given name is free and does not conflict with existing structure names.
    /// If the name is already taken, returns an error.
    /// If the name is free, returns the name as is.
    pub fn ensure_free_name(&self, name: IdentPath) -> Result<IdentPath, RegisterStructError> {
        if self.struct_names_to_id.contains_key(&name) {
            Err(RegisterStructError::NameTaken { name })
        } else {
            Ok(name)
        }
    }

    /// Add a structure to the schema.
    pub fn add_struct(
        &mut self,
        name: IdentPath,
        struc: Struct,
        type_id: TypeId,
    ) -> Result<(), RegisterStructError> {
        let name = self.ensure_free_name(name)?;
        self.struct_names_to_id.insert(name, type_id);
        self.types.insert(type_id, TypeKind::Struct(struc));
        Ok(())
    }

    pub fn struct_ref(&self, id: TypeId) -> Result<&Struct, GetStructError> {
        match self.types.get(&id) {
            Some(TypeKind::Struct(struc)) => Ok(struc),
            Some(_) => Err(GetStructError::NotStruct(id)),
            None => Err(GetStructError::NotFound(id)),
        }
    }

    pub fn struct_mut(&mut self, id: TypeId) -> Result<&mut Struct, GetStructError> {
        match self.types.get_mut(&id) {
            Some(TypeKind::Struct(struc)) => Ok(struc),
            Some(_) => Err(GetStructError::NotStruct(id)),
            None => Err(GetStructError::NotFound(id)),
        }
    }

    pub fn replace_struct(
        &mut self,
        id: TypeId,
        new_struct: Struct,
    ) -> Result<Struct, (Struct, GetStructError)> {
        use hashbrown::hash_map::Entry::*;
        match self.types.entry(id) {
            Vacant(_) => Err((new_struct, GetStructError::NotFound(id))),
            Occupied(mut entry) => {
                if entry.get().is_struct() {
                    Ok(entry
                        .insert(TypeKind::Struct(new_struct))
                        .try_into_struct()
                        .expect("we checked that the type is a struct"))
                } else {
                    Err((new_struct, GetStructError::NotStruct(id)))
                }
            }
        }
    }
}

#[repr(transparent)]
#[must_use = "Generation token should be returned to the Db after use to free the transaction"]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenerationToken(Generation);

impl Deref for GenerationToken {
    type Target = Generation;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub enum TableStoreBehavior {
    /// This table can independently store records in the database.
    DirectlyStorable,

    /// The table cannot be stored directly in the database, and can only be used
    /// as a field in other records. This is useful for defining structures that
    /// are not meant to be stored independently.
    FieldOnly,

    /// Only one record of this type can be stored in the database.
    /// This is useful for defining singleton records that, for example,
    /// store a configuration or some global state. Such table cannot be used as
    /// a field in other records, as it is not meant to be stored
    /// as a part of other records, but rather as a standalone single record.
    /// Note that there will always be exactly one record of this type in the database,
    /// both zero or many number of records are not allowed. However, this
    /// record may have no fields, and then it can be used as a marker of sorts or as a
    /// "module" to store functions (as its methods) in some named context.
    Singleton,
}

/// Triggers for structures, which can be used to define custom behavior
/// for operations on the structure, such as insert, update, delete, etc.
/// These triggers can be used to perform custom actions, such as validation,
/// transformation, or logging, when the structure is modified.
#[derive(Debug, Default)]
pub struct StructTriggers {
    /// Trigger for the structure before insert.
    /// This trigger is called before the structure is inserted into the database.
    /// It can be used to perform custom validation or transformation of the structure
    /// before it is inserted.
    before_insert: Option<ffi::SelfStructCall>,

    /// Trigger for the structure after insert.
    /// This trigger is called after the structure is inserted into the database.
    /// It can be used to perform custom actions after the structure is inserted,
    /// such as logging or sending notifications.
    after_insert: Option<ffi::SelfStructCall>,

    /// Trigger for the structure before update.
    /// This trigger is called before the structure is updated in the database.
    before_update: Option<ffi::SelfStructCall>,

    /// Trigger for the structure after update.
    /// This trigger is called after the structure is updated in the database.
    after_update: Option<ffi::SelfStructCall>,

    /// Trigger for the structure before delete.
    /// This trigger is called before the structure is deleted from the database.
    before_delete: Option<ffi::SelfStructCall>,

    /// Trigger for the structure after delete.
    /// This trigger is called after the structure is deleted from the database.
    after_delete: Option<ffi::SelfStructCall>,
}

/// Kind of type in the database schema. Note that here we define only user-defined types and
/// types derived from generic types. Built-in types are not included here, as we manage them
/// separately.
#[derive(Debug)]
pub enum TypeKind {
    /// A structure defined by the user in the database schema. Normal struct cannot be
    /// directly stored in the database, but can be used as a field type in other structures.
    Struct(Struct),

    /// A tuple type, which is a fixed-size collection of types.
    ///
    /// The size of the inline array is choosen so to not affect total enum's size.
    Tuple(SmallVec<[TypeId; 42]>),

    /// A generic type, which can be used to define types like `Vec<T>`, where `T` is a type parameter.
    /// The first type in the generic is the base type, and the rest are type parameters.
    GenericInstance(GenericInstance),
}

impl TypeKind {
    pub const fn is_struct(&self) -> bool {
        matches!(self, TypeKind::Struct(_))
    }

    pub fn try_into_struct(self) -> Result<Struct, ()> {
        match self {
            TypeKind::Struct(struc) => Ok(struc),
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
pub struct Struct {
    /// Triggers, which can be used to define custom behavior
    /// for operations on the record, such as insert, update, delete, etc.
    triggers: StructTriggers,

    /// Comment for the struct, which can be used to describe the struct's purpose,
    /// usage, or any other relevant information.
    /// Can be empty if no comment is provided.
    comment: String,

    /// Structure specialization, which defines the specific kind of the structure.
    spec: StructSpec,
}

impl Struct {
    pub fn new(spec: StructSpec) -> Self {
        Struct {
            triggers: StructTriggers::default(),
            comment: String::new(),
            spec,
        }
    }
}

#[derive(Debug)]
pub enum StructSpec {
    /// Table is a special case of a struct that is used to store records in the database.
    SchemafullTable(SchemafullTable),

    /// An ADT (Algebraic Data Type) table, which is a table that can have multiple variants,
    /// each with its own structure. This is used to represent complex data structures
    /// that can have different shapes, like a union type.
    /// The ADT table is a collection of variants, each with its own structure.
    AdtTable(AdtTable),

    /// A schemaless table, which is a table that can have any number of fields,
    /// and the fields have no fixed type.
    SchemalessTable(SchemalessTable),

    /// Mixed schemafull schemaless table,
    /// which is a table that can have both schemafull and schemaless fields.
    SchemafullSchemalessTable(SchemafullSchemalessTable),

    /// Mixed schemafull adt schemaless table,
    /// which is a table that can have both schemafull ADT and schemaless fields.
    /// This is used to represent complex data structures that can have different shapes,
    /// like a union type, with a fixed schema for the ADT part, but also with
    /// schemaless fields that can have any type.
    SchemafullAdtSchemalessTable(SchemafullAdtSchemalessTable),
}

#[derive(Debug, Default)]
pub struct SchemafullTable {
    /// Fields of the schemafull table.
    fields: Vec<Field>,
}

#[derive(Debug, Default)]
pub struct AdtTable {
    /// Variants of the ADT table, each variant can have its own structure.
    /// The map is keyed by the variant name, and the value is the structure of the variant.
    variants: HashMap<String, Vec<Field>>,
}

#[derive(Debug, Default)]
pub struct SchemalessTable {
    /// The bound fields of the schemaless table.
    /// The map is keyed by the field name,
    /// and the value is the configuration of the field.
    fields: Vec<FieldCfg>,

    /// Map of field names to their type IDs bound to schema.
    /// This is used to check whether there are any typed variants of the schemaless field
    /// bound to the schema, and which types were bound. This check is used when defining new
    /// bounds, especially when the bound is to "any" type, in which case only one binding
    /// for the same field name is allowed.
    bound_to_types: HashMap<String, SmallVec<[TypeId; 1]>>,

    /// Map of field names to their type IDs, mapped to indexes in the `fields` vector.
    field_to_idx: HashMap<(String, Option<TypeId>), usize>,
}

#[derive(Debug, Default)]
pub struct SchemafullSchemalessTable {
    /// The bound and fixed fields of the schemafull schemaless table.
    fields: Vec<FieldCfg>,

    /// Map of fixed field names to their indexes in the `fields` vector.
    fixed: HashMap<String, (TypeId, usize)>,

    /// Map of bound field names to their type IDs bound to schema.
    bound_to_types: HashMap<String, SmallVec<[TypeId; 1]>>,

    /// Map of bound field names and their type IDs, mapped to indexes in the `fields` vector.
    bound_to_idx: HashMap<(String, Option<TypeId>), usize>,
}

#[derive(Debug, Default)]
pub struct SchemafullAdtSchemalessTable {
    /// Variants of the ADT table, which can have multiple variants,
    /// each with its own structure.
    variants: HashMap<String, Vec<Field>>,

    /// The bound and fixed fields of the schemaless part.
    bound_fields: Vec<FieldCfg>,

    /// Map of fixed field names to their indexes in the `bound_fields` vector.
    fixed: HashMap<String, (TypeId, usize)>,

    /// Map of bound field names to their type IDs bound to schema.
    bound_to_types: HashMap<String, SmallVec<[TypeId; 1]>>,
}

#[derive(Debug)]
pub struct FieldCfg {
    /// Comment for the field, which can be used to describe the field's purpose,
    /// usage, or any other relevant information.
    /// Can be empty if no comment is provided.
    comment: String,

    /// Optional validation code for the field.
    validation: Option<ffi::FieldValidationCode>,

    /// Optional transformation code for the field before insert.
    /// Individual field triggers are run before the general table insert trigger.
    /// This allows these triggers to act as a pre-insert validation or transformation
    /// for the field, and can be used to modify the field value before it is further
    /// processed by table triggers and validators.
    trigger_before_insert: Option<ffi::FieldTransformCall>,

    /// Optional transformation code for the field before update.
    /// The same logic as for `trigger_before_insert` applies here.
    trigger_before_update: Option<ffi::FieldTransformCall>,

    /// Optional transformation code for the field before delete.
    /// The same logic as for `trigger_before_insert` applies here.
    /// if delete statement returns the deleted value,
    /// this trigger can transform the returned value before it is emitted.
    trigger_before_delete: Option<ffi::FieldTransformCall>,

    /// Optional trigger for the field after insert.
    /// This does not modify the field value, but can be used to perform additional actions.
    /// Note that the field can be different from the one emitted by "before insert" transformation,
    /// as after that transformation, the general table insert trigger is called, which itself
    /// can again modify the field value.
    trigger_after_insert: Option<ffi::FieldTransformCall>,

    /// Optional trigger for the field after update. The same logic as for `trigger_after_insert`
    /// applies here. However, this trigger is only called if the field value was changed
    /// during the update operation. This does not run if the field value was not changed,
    /// even if the user has had passed a different value for the field for the operation, as that
    /// value could have been transformed by the "before update" transformation to the same
    /// one already stored in the database. Hence, only if the value was changed
    /// after all transformations, this trigger is called.
    trigger_after_update: Option<ffi::FieldTransformCall>,

    /// Optional trigger for the field after delete.
    /// This trigger is called after the field value was deleted from the database.
    /// This trigger only receives the field value that was deleted,
    /// and does not receive the whole structure that was deleted.
    /// This also allows to independently run multiple triggers concurrently,
    /// which can be more efficient than running a single trigger
    /// that receives the whole structure in cases when operations on the structure
    /// do not depend on each other.
    trigger_after_delete: Option<ffi::FieldTransformCall>,

    /// Optional default value for the field.
    default: Option<DataValue>,
}

/// Generic type configuration, which can be used for types like `Vec<T>`.
///
/// The size of the inner inline array is chosen so to not affect total [TypeKind] enum's size.
#[derive(Debug)]
pub struct GenericInstance {
    /// The generic type ID, which is the base type of the generic.
    /// For example, for `Vec<T>`, this would be the type ID of `Vec`.
    generic_type: TypeId,

    /// The type parameters of the generic type.
    type_params: SmallVec<[TypeId; 41]>,
}

#[derive(Debug)]
pub struct Field {
    /// The name of the field.
    name: IdentPath,

    /// The type of the field.
    ty: TypeId,

    /// Configuration for the field, which includes validation and transformation code,
    /// default value, and other field-specific settings.
    cfg: FieldCfg,
}

impl Db {
    pub fn new(name: IdentPath) -> Self {
        Db {
            name,
            last_generation: Generation(0),
            last_type_id: TypeId::INVALID,
            types: Types::default(),
            active_transactions: HashMap::default(),
            transaction_nesting: HashMap::default(),
        }
    }

    /// Add a structure to the schema.
    fn next_type_id(&mut self) -> TypeId {
        let next = self.last_type_id.advance();
        self.last_type_id = next;
        next
    }
}

#[derive(Debug, Error)]
pub enum RegisterStructError {
    #[error("name {name} already is taken in the schema")]
    NameTaken { name: IdentPath },
}

#[derive(Debug, Error)]
pub enum GetStructError {
    #[error("structure with ID {0} not found in the schema")]
    NotFound(TypeId),

    #[error("structure with ID {0} is not a struct")]
    NotStruct(TypeId),
}

#[derive(Debug, Error)]
pub enum IdentError {
    #[error("identifier cannot be empty")]
    Empty,

    #[error("identifier `{ident}` contains invalid characters: {chars:?}")]
    InvalidChars { ident: String, chars: Vec<char> },

    #[error("identifier `{ident}` cannot start with a digit")]
    CannotStartWithDigit { ident: String },

    #[error("identifier `{ident}` cannot start with a colon")]
    CannotStartWithColon { ident: String },

    #[error("identifier `{ident}` cannot end with a colon")]
    CannotEndWithColon { ident: String },
}

impl IdentPath {
    /// Create a new identifier from a string.
    /// Returns an error if the identifier is empty or contains invalid characters.
    ///
    /// Identifier can only contain alphanumeric characters and underscores.
    /// It can also have a separator "::" to allow namespacing.
    pub fn new(ident: String) -> Result<Self, IdentError> {
        if let Some(first) = ident.chars().next() {
            if first.is_numeric() {
                return Err(IdentError::CannotStartWithDigit { ident });
            }
            if first == ':' {
                return Err(IdentError::CannotStartWithColon { ident });
            }
            if ident.chars().last().unwrap() == ':' {
                return Err(IdentError::CannotEndWithColon { ident });
            }
        } else {
            return Err(IdentError::Empty);
        }

        let mut invalid_chars = Vec::new();
        let mut push_invalid = |c: char| {
            if !invalid_chars.contains(&c) {
                invalid_chars.push(c);
            }
        };
        let mut colon = 0;
        for c in ident.chars() {
            if c.is_alphanumeric() || c == '_' {
                if colon != 2 && colon != 0 {
                    push_invalid(c);
                }
            } else if c == ':' {
                colon += 1;
            } else {
                push_invalid(c);
            }
        }

        if !invalid_chars.is_empty() {
            return Err(IdentError::InvalidChars {
                ident,
                chars: invalid_chars,
            });
        }

        Ok(IdentPath(ident))
    }
}

/// An enum that has a corresponding type code written to the WAL file.
/// This is used to identify the type of the entry in the WAL file,
/// so that it can be read and processed correctly.
pub trait WalCode {
    fn wal_code(&self) -> u64;

    fn wal_code_bytes(&self) -> [u8; 8] {
        self.wal_code().to_le_bytes()
    }
}

/// Trait for types that can be created from a WAL code.
pub trait FromWalCode: WalCode {
    fn from_wal_code(code: u64) -> Self;
}
