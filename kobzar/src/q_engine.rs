use super::*;
use smallvec::SmallVec;

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

/// Schema information for some database in the server.
#[derive(Debug)]
pub struct Schema {
    /// The name of the database.
    name: String,

    /// Map of structure names to their type IDs.
    /// This is used to quickly look up the type ID of a structure by its name.
    /// Also this allows to list all structures in the schema.
    struct_names_to_id: HashMap<String, TypeId>,

    /// All derived and user-defined types in the schema.
    /// Note that built-in types are not included here, as they are managed separately.
    types: HashMap<TypeId, TypeKind>,
}

/// Triggers for structures, which can be used to define custom behavior
/// for operations on the structure, such as insert, update, delete, etc.
/// These triggers can be used to perform custom actions, such as validation,
/// transformation, or logging, when the structure is modified.
#[derive(Debug)]
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

#[derive(Debug)]
pub struct SchemafullTable {
    /// Fields of the schemafull table.
    fields: Vec<Field>,
}

#[derive(Debug)]
pub struct AdtTable {
    /// Variants of the ADT table, each variant can have its own structure.
    /// The map is keyed by the variant name, and the value is the structure of the variant.
    variants: HashMap<String, Vec<Field>>,
}

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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
    name: String,

    /// The type of the field.
    ty: TypeId,

    /// Configuration for the field, which includes validation and transformation code,
    /// default value, and other field-specific settings.
    cfg: FieldCfg,
}
