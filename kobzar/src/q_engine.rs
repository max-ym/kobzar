use super::*;

/// Data types and structures storeable and queryable in Kobzar.
mod dt;
pub use dt::*;

/// Query structure.
mod query;
pub(crate) use query::*;
use smallvec::SmallVec;

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
    types: HashMap<TypeId, TypeCfg>,
}

#[derive(Debug)]
pub struct TypeCfg {
    /// The kind of the type, which can be a struct, tuple, or generic.
    kind: TypeKind,
}

/// Kind of type in the database schema. Note that here we define only user-defined types and
/// types derived from generic types. Built-in types are not included here, as we manage them
/// separately.
#[derive(Debug)]
pub enum TypeKind {
    /// A structure defined by the user in the database schema. Normal struct cannot be
    /// directly stored in the database, but can be used as a field type in other structures.
    Struct(Struct),

    /// Table is a special case of a struct that is used to store records in the database.
    SchemafullTable(Struct),

    /// An ADT (Algebraic Data Type) table, which is a table that can have multiple variants,
    /// each with its own structure. This is used to represent complex data structures
    /// that can have different shapes, like a union type.
    /// The ADT table is a collection of variants, each with its own structure.
    AdtTable(AdtTable),

    /// A schemaless table, which is a table that can have any number of fields,
    /// and the fields have no fixed type.
    SchemalessTable(SchemalessTable),

    SchemafullSchemalessTable(SchemafullSchemalessTable),

    /// A tuple type, which is a fixed-size collection of types.
    Tuple(Vec<TypeId>),

    /// A generic type, which can be used to define types like `Vec<T>`, where `T` is a type parameter.
    /// The first type in the generic is the base type, and the rest are type parameters.
    GenericInstance(GenericInstance),
}

#[derive(Debug)]
pub struct Struct {
    /// The fields of the struct.
    fields: Vec<Field>,
}

#[derive(Debug)]
pub struct AdtTable {
    variants: Vec<AdtTableVariant>,
}

#[derive(Debug)]
pub struct AdtTableVariant {
    /// The name of the variant.
    name: String,

    /// The fields of the variant.
    fields: Vec<Field>,
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
    map: HashMap<String, SmallVec<[TypeId; 1]>>,

    /// Map of field names to their type IDs, mapped to indexes in the `fields` vector.
    typed_map: HashMap<(String, Option<TypeId>), usize>,
}

#[derive(Debug)]
pub struct FieldCfg {
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
#[derive(Debug)]
pub struct GenericInstance(Vec<TypeId>);

impl GenericInstance {
    /// Get the generic type ID, for which concrete types was derived.
    /// For example, for `Vec<T>`, the generic type is `Vec`.
    pub fn generic(&self) -> TypeId {
        // The first type in the generic is the base type.
        self.0.first().copied().expect(
            "generic type config must have an array with the first entry holding the generic type ID",
        )
    }
}

#[derive(Debug)]
pub struct Field {
    /// The name of the field.
    name: String,

    /// The type of the field.
    ty: TypeId,
}
