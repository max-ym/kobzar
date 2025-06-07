use super::*;

/// Data types and structures storeable and queryable in Kobzar.
mod dt;
pub use dt::*;

/// Query structure.
mod query;
pub(crate) use query::*;

/// Executable bytecode for the query engine.
mod bytecode;

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
    Table(Struct),

    /// A tuple type, which is a fixed-size collection of types.
    Tuple(Vec<TypeId>),

    /// A generic type, which can be used to define types like `Vec<T>`, where `T` is a type parameter.
    /// The first type in the generic is the base type, and the rest are type parameters.
    Generic(Generic),
}

#[derive(Debug)]
pub struct Struct {
    /// The fields of the struct.
    fields: Vec<Field>,
}

/// Generic type configuration, which can be used for types like `Vec<T>`.
#[derive(Debug)]
pub struct Generic(Vec<TypeId>);

impl Generic {
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
