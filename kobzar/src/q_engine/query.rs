use crate::q_engine::ffi::{FieldTransformCall, FieldValidationCall, SelfStructCall};

use super::*;

type FieldIdx = u64;

/// Query variants for the query engine.
/// Note that full user's code is not considered a query, but rather a group of statements,
/// which can include queries, among other computational tasks.
/// When compiling byte code, normal computations are directly translated to machine code,
/// while queries are called through ABI, which in turn creates these query structures.
#[derive(Debug)]
pub enum Query {
    /// Upsert query can be used to insert or update a record in the database.
    /// We also store normal insert and update queries as upsert queries,
    /// as most of the logic is the same. We only differentiate them when necessary,
    /// such as when we need to fail on duplicate key for insert,
    /// or when we need to update only if the record exists.
    Upsert(UpsertQuery),

    /// Delete query can be used to delete records from the database.
    /// It can also be used to delete records based on some conditions,
    /// or to delete all records of a certain type.
    Delete(DeleteQuery),

    /// Select query can be used to select records from the database.
    /// It can also be used to select records based on some conditions,
    /// or to select only certain fields of the record.
    Select(SelectQuery),

    /// Create table query can be used to create a new table in the database.
    /// This table can be schemafull, schemaless, or a mix of both.
    /// It can also be an ADT (Algebraic Data Type) table, which allows to have different types of records
    /// in the same table with their own specific fields.
    /// The table can also be a singleton, which means that only one record of
    /// this type can be stored in the database.
    CreateTable(CreateTableQuery),

    /// Comment table query can be used to add a comment to a table,
    CommentTable(CommentTableQuery),

    /// Rename table query can be used to rename a table.
    RenameTable(RenameTableQuery),

    /// Create table validation query can be used to create a validation for the table.
    /// This validation is run on every insert or update operation on the table,
    /// and can be used to ensure that the table record is in a valid state.
    /// This operates on all fields of the table once they are set, unlike field validations
    /// which operate on individual fields.
    CreateTableValidation(CreateTableValidationQuery),

    /// Comment table validation query can be used to add a comment to a table validation.
    CommentTableValidation(CommentTableValidationQuery),

    /// Drop table validation query can be used to drop a validation from the table.
    DropTableValidation(DropTableValidationQuery),

    /// Drop table query can be used to drop a table from the database.
    DropTable(DropTableQuery),

    /// Create ADT variant query can be used to create a new variant in an ADT table.
    CreateAdtVariant(CreateAdtVariantQuery),

    /// Comment ADT variant query can be used to add a comment to an ADT variant.
    CommentAdtVariant(CommentAdtVariantQuery),

    /// Rename ADT variant query can be used to rename an ADT variant.
    RenameAdtVariant(RenameAdtVariantQuery),

    /// Drop ADT variant query can be used to drop a variant from an ADT table.
    DropAdtVariant(DropAdtVariantQuery),

    /// Bind a schemaless field to a table.
    /// This will allocate a unique field index for the field,
    /// and will allow to use this field in query conditions, operations and indexes.
    BindSchemalessField(BindSchemalessFieldQuery),

    /// Unbind a schemaless field from a table.
    UnbindSchemalessField(UnbindSchemalessFieldQuery),

    /// Create a trigger for a table.
    /// This trigger can be used to execute some code when a record is inserted, updated or deleted.
    /// Only one trigger of each kind can be created for a table,
    CreateTableTrigger(CreateTableTriggerQuery),

    /// Drop a trigger from a table.
    DropTableTrigger(DropTableTriggerQuery),

    /// Create a new field in a table. This can only be done for schemafull tables, either
    /// ADT, normal, or mixed schemafull-schemaless tables.
    CreateField(CreateFieldQuery),

    /// Set a default value for a field in a table. This value is set each time
    /// a new record is created, when user does not explicitly set the field value.
    SetFieldDefault(SetFieldDefaultQuery),

    /// Set a computed value for a field in a table. This value is calculated
    /// each time the record is inserted or updated, and can be used to derive
    /// the value from other fields or from some code execution. User cannot set this value
    /// directly, but can set the fields that are used to calculate it.
    SetFieldComputed(SetFieldComputedQuery),

    /// Set a check function for a field in a table. This function is executed
    /// each time the record is inserted or updated, and can be used to ensure that the field
    /// value is valid. Field checks are run after the field default and computed values are set,
    /// and after transformations are applied.
    SetFieldCheck(SetFieldCheckQuery),

    /// Set a transformation function for a field in a table. This function is executed
    /// each time the record is inserted or updated, and can be used to transform the field value
    /// before it is stored in the database. This can be used to normalize the value, or to
    /// apply some custom logic to the value before it is stored. The same can be achieved
    /// by setting a trigger on the table, but this is more convenient
    /// and allows to set the transformation function directly on the field.
    /// This can have an added benefit of being able to run the transformation
    /// function asynchronously on multiple fields in parallel,
    /// while triggers are run sequentially.
    SetFieldTransform(SetFieldTransformQuery),

    /// Comment a field in a table. This can be used to add or remove a comment
    /// to a field in a table.
    CommentField(CommentFieldQuery),

    /// Rename a field in a table.
    RenameField(RenameFieldQuery),

    /// Drop a field from a table.
    DropField(DropFieldQuery),

    /// Create an index on a table. This can be used to speed up queries
    /// that filter or sort records based on the indexed fields. This also allows
    /// to create unique indexes, which ensure that the indexed fields are unique across all records.
    CreateIndex(CreateIndexQuery),

    /// Comment an index on a table.
    CommentIndex(CommentIndexQuery),
    
    /// Rename an index on a table.
    RenameIndex(RenameIndexQuery),

    /// Drop an index from a table.
    DropIndex(DropIndexQuery),

    /// Create a function or a method in the database.
    CreateFn(CreateFnQuery),

    /// Comment a function or a method in the database.
    CommentFn(CommentFnQuery),

    /// Rename a function or a method in the database.
    RenameFn(RenameFnQuery),

    /// Drop a function or a method from the database.
    DropFn(DropFnQuery),
}

#[derive(Debug)]
pub struct UpsertQuery {
    /// The type of the record to upsert.
    pub table: TypeId,

    /// Operations to calculate the values of each inserted field.
    /// All skipped fields will be set to their default values.
    pub fields: Vec<FieldOp>,

    /// Kind of operation to perform.
    pub kind: UpsertKind,

    /// The value type that is returned after the query execution.
    pub returning: Option<TypeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UpsertKind {
    /// Insert a new record, or update the existing one.
    InsertOrUpdate,

    /// Update an existing record, or fail if it does not exist.
    UpdateOrFail,

    /// Insert a new record, or fail if it already exists.
    InsertOrFail,
}

#[derive(Debug)]
pub struct FieldOp {
    /// Index of the field in the record.
    pub idx: FieldIdx,

    /// The operation to perform to calculate the value of the field.
    pub op: FieldOpType,
}

#[derive(Debug)]
pub enum FieldOpType {
    /// Set the field to a constant value. This value may be explicitly provided by the user,
    /// or it may be the result of the code execution, which leads to this result stored here.
    SetConstant(DataValue),

    /// Set the field to a value which is the result of an another query.
    SetFromQuery(Box<Query>),
}

#[derive(Debug, Clone)]
pub struct DataValue(Vec<u8>);

#[derive(Debug)]
pub struct DeleteQuery {
    /// The type of the record to delete.
    pub table: TypeId,

    /// The condition to match records to delete.
    /// If no conditions are provided, all records of the type will be deleted.
    pub conditions: Vec<Condition>,

    /// Type of the value being returned after query execution.
    pub returning: Option<TypeId>,
}

#[derive(Debug)]
pub enum Condition {
    /// A condition that matches records based on a range of field values.
    /// If [min](Condition::Range::min) is [None], it means no lower bound,
    /// and if [max](Condition::Range::max) is [None], it means no upper bound.
    /// Effectively, if min and max are set to the same [Some] value, it matches only that value,
    /// and acts as an equality condition. When min is set to [None] and max is set to [Some],
    /// it matches all values less than or equal to the max value.
    /// If min is set to [Some] and max is set to [None], it matches all values greater than or equal to the min value.
    /// The boolean values indicate whether the min and max values are inclusive (true) or exclusive (false).
    Range {
        field: FieldIdx,
        min: Option<(DataValue, bool)>,
        max: Option<(DataValue, bool)>,
    },

    /// Match a field against values returned by a subquery. [is_not](Condition::InQuery::is_not)
    /// indicates whether to match records that are in the subquery result (false) or not in the subquery result (true).
    InQuery {
        field: FieldIdx,
        query: Box<Query>,
        is_not: bool,
    },
}

#[derive(Debug)]
pub struct SelectQuery {
    /// The type of the record to select.
    pub table: TypeId,

    /// Fields to select from the record.
    /// If empty, all fields will be selected.
    pub fields: Vec<FieldIdx>,

    /// Conditions to match records to select.
    /// If no conditions are provided, all records of the type will be selected.
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone)]
pub struct CreateTableQuery {
    /// The name of the table to create.
    pub name: String,

    /// The kind of the table to create.
    /// This determines how the table will be structured and what kind of records it can hold.
    pub table_kind: TableKind,

    /// The behavior of the table in terms of storage.
    pub store_behavior: TableStoreBehavior,
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

#[derive(Debug, Clone)]
pub enum TableKind {
    /// A normal schemafull table, which is a structure that can be used to store records.
    Schemafull,

    /// A schemafull ADT (Algebraic Data Type) table, which is a structure that can be used to
    /// store records with a fixed schema variants - the ability to have different types of records
    /// in the same table with their own specific fields.
    SchemafullAdt,

    /// A schemaless table, which is a structure that can be used to store records
    /// without a predefined schema. This is useful for storing records with dynamic fields,
    /// or for storing records that do not have a fixed schema.
    Schemaless,

    /// Mixed schemaless and schemafull behavior table.
    /// This allows to enforce some fields to be schemafull,
    /// while allowing other fields to be schemaless.
    /// This will allow to make queries that create table fields like in normal schemafull tables,
    /// and in the same time allow to bind schemaless fields to the table,
    /// which can hold any value but which could be used in query conditions,
    /// operations and indexes.
    SchemafullSchemaless,

    /// The same as [SchemafullSchemaless](TableKind::SchemafullSchemaless), but the schemafull
    /// part is an ADT (Algebraic Data Type) table, which allows to have different types of records
    /// in the same table with their own specific fields.
    SchemafullAdtSchemaless,
}

#[derive(Debug, Clone)]
pub struct CommentTableQuery {
    /// The type of the table to comment.
    pub table: TypeId,

    /// The comment to add to the table.
    /// If empty, the comment will be removed.
    pub comment: String,
}

#[derive(Debug, Clone)]
pub struct RenameTableQuery {
    /// The type of the table to rename.
    pub table: TypeId,

    /// The new name of the table.
    pub new_name: String,
}

/// Create a validation for the table. This validation is being run on every insert or update
/// operation on the table, and can be used to ensure that the table record is in a valid state.
/// This operates on all fields of the table once they are set, unlike field validations
/// which operate on individual fields.
#[derive(Debug)]
pub struct CreateTableValidationQuery {
    /// The type of the table to create validation for.
    pub table: TypeId,

    /// The name of the validation to create.
    /// This is used to identify the validation in the schema.
    pub name: String,

    /// The code to execute to validate the table.
    /// The output type should be a [Result], indicating whether the table is valid or not.
    pub code: SelfStructCall,
}

#[derive(Debug, Clone)]
pub struct CommentTableValidationQuery {
    /// The type of the table to comment the validation in.
    pub table: TypeId,

    /// The index of the validation to comment.
    pub validation_idx: FieldIdx,

    /// The comment to add to the validation.
    /// If empty, the comment will be removed.
    pub comment: String,
}

#[derive(Debug, Clone)]
pub struct DropTableValidationQuery {
    /// The type of the table to drop the validation from.
    pub table: TypeId,

    /// The index of the validation to drop.
    pub validation_idx: FieldIdx,
}

#[derive(Debug, Clone)]
pub struct DropTableQuery {
    /// The type of the table to drop.
    pub table: TypeId,
}

#[derive(Debug, Clone)]
pub struct CreateAdtVariantQuery {
    /// The type of the ADT table to create the variant for.
    pub table: TypeId,

    /// The name of the variant to create.
    pub variant_name: String,
}

#[derive(Debug, Clone)]
pub struct CommentAdtVariantQuery {
    /// The type of the ADT table to comment the variant in.
    pub table: TypeId,

    /// The index of the variant to comment.
    pub variant_idx: FieldIdx,

    /// The comment to add to the variant.
    /// If empty, the comment will be removed.
    pub comment: String,
}

#[derive(Debug, Clone)]
pub struct RenameAdtVariantQuery {
    /// The type of the ADT table to rename the variant in.
    pub table: TypeId,

    /// The index of the variant to rename.
    pub variant_idx: FieldIdx,

    /// The new name of the variant.
    pub new_name: String,
}

#[derive(Debug, Clone)]
pub struct DropAdtVariantQuery {
    /// The type of the ADT table to drop the variant from.
    pub table: TypeId,

    /// The index of the variant to drop.
    pub variant_idx: FieldIdx,
}

#[derive(Debug, Clone)]
pub struct BindSchemalessFieldQuery {
    /// The type of the table to bind the schemaless field in.
    pub table: TypeId,

    /// The name of the field to bind.
    /// This will allocate unique field index for the field,
    /// and will allow to use this field in query conditions, operations and indexes.
    pub field_name: String,

    /// The type of the field to bind.
    /// Field name and field type form a unique identifier for the field,
    /// so when the value under the same name but different type is inserted,
    /// it will be considered a different field.
    ///
    /// If this is [None], the field will be bound as a schemaless field,
    /// which means that it can hold any value. This will still allow to use the field
    /// in query conditions, operations and indexes, and will match any value type.
    /// There should ever be either one such schemaless binding per field name, or
    /// any number of bindings with different field types for the same name.
    /// Violation of this rule will result in an transaction error.
    pub field_type: Option<TypeId>,
}

#[derive(Debug, Clone)]
pub struct UnbindSchemalessFieldQuery {
    /// The type of the table to unbind the schemaless field from.
    pub table: TypeId,

    /// The name of the field to unbind.
    /// This will remove the field from the table, and will not allow to use it in query conditions,
    /// operations and indexes.
    pub field_name: String,

    /// The type of the field to unbind. This should match the type passed during the binding.
    pub field_type: Option<TypeId>,
}

#[derive(Debug)]
pub struct CreateTableTriggerQuery {
    /// The type of the table to create the trigger for.
    pub table: TypeId,

    /// The kind of the trigger to create.
    pub kind: TriggerExecKind,

    /// The code to execute when the trigger is fired.
    /// The output type should be a [Result], indicating whether the trigger execution was successful or not.
    /// Triggers should return [Ok] with the modified record,
    /// or [Err] with an error message if the execution fails.
    pub code: SelfStructCall,
}

#[derive(Debug, Clone)]
pub enum TriggerExecKind {
    /// Trigger that is executed before the record is inserted into the table.
    BeforeInsert,

    /// Trigger that is executed before the record is updated in the table.
    BeforeUpdate,

    /// Trigger that is executed before the record is deleted from the table.
    BeforeDelete,

    /// Trigger that is executed after the record is inserted into the table.
    AfterInsert,

    /// Trigger that is executed after the record is updated in the table.
    AfterUpdate,

    /// Trigger that is executed after the record is deleted from the table.
    AfterDelete,
}

#[derive(Debug, Clone)]
pub struct DropTableTriggerQuery {
    /// The type of the table to drop the trigger from.
    pub table: TypeId,

    /// The kind of the trigger to drop.
    pub kind: TriggerExecKind,
}

#[derive(Debug, Clone)]
pub struct CreateFieldQuery {
    /// The type of the table to add a field to.
    pub table: TypeId,

    /// The index of the variant to add the field to, if table is an ADT.
    /// This should be [None] for normal schemafull tables.
    pub variant_idx: Option<FieldIdx>,

    /// The name of the field to add.
    pub field_name: String,

    /// The type of the field to add.
    pub field_type: TypeId,
}

#[derive(Debug, Clone)]
pub struct SetFieldDefaultQuery {
    /// The type of the table to set the field default in.
    pub table: TypeId,

    /// The index of the variant to set the default for, if table is an ADT.
    pub variant_idx: Option<FieldIdx>,

    /// The index of the field to set the default for.
    pub field_idx: FieldIdx,

    /// The default value to set for the field.
    /// If `None`, the default value will be removed.
    pub default: Option<DataValue>,
}

#[derive(Debug)]
pub struct SetFieldComputedQuery {
    /// The type of the table to set the field computed value in.
    pub table: TypeId,

    /// The index of the variant to set the computed value for, if table is an ADT.
    pub variant_idx: Option<FieldIdx>,

    /// The index of the field to set the computed value for.
    pub field_idx: FieldIdx,

    /// The code to execute to compute the value of the field.
    /// The output type should match the type of the field.
    pub code: SelfStructCall,
}

#[derive(Debug)]
pub struct SetFieldCheckQuery {
    /// The type of the table to set the field check in.
    pub table: TypeId,

    /// The index of the variant to set the check for, if table is an ADT.
    pub variant_idx: Option<FieldIdx>,

    /// The index of the field to set the check for.
    pub field_idx: FieldIdx,

    /// The code to execute to check the value of the field.
    /// The output type should be a boolean, indicating whether the value is valid or not.
    pub code: FieldValidationCall,
}

#[derive(Debug)]
pub struct SetFieldTransformQuery {
    /// The type of the table to set the field transformation.
    pub table: TypeId,

    /// The index of the variant to set the transformation for, if table is an ADT.
    pub variant_idx: Option<FieldIdx>,

    /// The index of the field to set the transformation for.
    pub field_idx: FieldIdx,

    /// The code to execute to transform the value of the field.
    /// The output type should match the type of the field.
    pub code: FieldTransformCall,
}

#[derive(Debug, Clone)]
pub struct CommentFieldQuery {
    /// The type of the table to comment the field in.
    pub table: TypeId,

    /// The index of the variant to comment the field in, if table is an ADT.
    pub variant_idx: Option<FieldIdx>,

    /// The index of the field to comment.
    pub field_idx: FieldIdx,

    /// The comment to add to the field.
    /// If empty, the comment will be removed.
    pub comment: String,
}

#[derive(Debug, Clone)]
pub struct RenameFieldQuery {
    /// The type of the table to rename the field in.
    pub table: TypeId,

    /// The index of the variant to rename the field in, if table is an ADT.
    pub variant_idx: Option<FieldIdx>,

    /// The index of the field to rename.
    pub field_idx: FieldIdx,

    /// The new name of the field.
    pub new_name: String,
}

#[derive(Debug, Clone)]
pub struct DropFieldQuery {
    /// The type of the table to drop the field from.
    pub table: TypeId,

    /// The index of the variant to drop the field from, if table is an ADT.
    pub variant_idx: Option<FieldIdx>,

    /// The index of the field to drop.
    pub field_idx: FieldIdx,
}

#[derive(Debug, Clone)]
pub struct CreateIndexQuery {
    /// The type of the table to create the index on.
    pub table: TypeId,

    /// The index of the variant to create the index on, if table is an ADT.
    /// This should be [None] for normal schemafull tables.
    pub variant_idx: Option<FieldIdx>,

    /// The name of the index to create.
    pub index_name: String,

    /// Configuration of the index to create.
    pub cfg: IndexCfg,
}

#[derive(Debug, Clone)]
pub struct CommentIndexQuery {
    /// The type of the table to comment the index in.
    pub table: TypeId,

    /// The schema index of the table index to comment.
    pub index_idx: FieldIdx,

    /// The comment to add to the index.
    /// If empty, the comment will be removed.
    pub comment: String,
}

#[derive(Debug, Clone)]
pub struct RenameIndexQuery {
    /// The type of the table to rename the index in.
    pub table: TypeId,

    /// The schema index of the table index to rename.
    pub index_idx: FieldIdx,

    /// The new name of the index.
    pub new_name: String,
}

#[derive(Debug, Clone)]
pub struct DropIndexQuery {
    /// The type of the table to drop the index from.
    pub table: TypeId,

    /// The schema index of the table index to drop.
    pub index_idx: FieldIdx,
}

#[derive(Debug, Clone)]
pub enum IndexCfg {
    /// Unique index, which ensures that the indexed fields are unique across all records.
    /// This is typically used for primary keys or unique constraints.
    Unique {
        /// The fields to index.
        fields: Vec<FieldIdx>,

        /// Whether optional values with [None] should be considered unique.
        none_is_unique: bool,
    },

    /// Index that accounts for ordering of the indexed fields. This speeds up orders, range,
    /// equality and other queries that can benefit from the index.
    Order {
        /// The fields to index.
        fields: Vec<FieldIdx>,

        /// Whether the index is ascending or descending.
        /// If `true`, the index is ascending, otherwise it is descending.
        is_ascending: bool,

        /// Whether optional values with [None] should be at the top or the bottom of the index.
        none_is_first: bool,
    },

    /// Index that is used for equality checks. This is typically used for fields that are
    /// frequently used in equality conditions, such as foreign keys or other fields that
    /// are often used to match records.
    Equal {
        /// The fields to index.
        fields: Vec<FieldIdx>,
    },
}

#[derive(Debug, Clone)]
pub struct CreateFnQuery {
    /// The name of the function to create.
    pub name: String,

    /// The type of the self argument, if the function is a method.
    /// Boolean indicates whether the self argument is a reference (true) or a value (false).
    pub self_arg: Option<(bool, TypeId)>,

    /// Arguments of the function, which are the types of the arguments that the function takes.
    /// This does not include the self argument, if it is present.
    pub args: Vec<TypeId>,

    /// The return type of the function.
    pub ret: TypeId,
}

#[derive(Debug, Clone)]
pub struct CommentFnQuery {
    /// The name of the function to comment.
    pub name: String,

    /// If this is a method, the type of self argument should be passed to locate the method.
    pub self_arg: Option<TypeId>,

    /// The comment to add to the function.
    /// If empty, the comment will be removed.
    pub comment: String,
}

#[derive(Debug, Clone)]
pub struct RenameFnQuery {
    /// The name of the function to rename.
    pub name: String,

    /// If this is a method, the type of self argument should be passed to locate the method.
    pub self_arg: Option<TypeId>,

    /// The new name of the function.
    pub new_name: String,
}

#[derive(Debug, Clone)]
pub struct DropFnQuery {
    /// The name of the function to drop.
    pub name: String,

    /// If this is a method, the type of self argument should be passed to locate the method.
    pub self_arg: Option<TypeId>,
}
