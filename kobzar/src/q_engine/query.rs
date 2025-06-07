use super::*;

type FieldIdx = u64;

/// Query variants for the query engine.
/// Note that full user's code is not considered a query, but rather a group of statements,
/// which can include queries, among other computational tasks.
/// When compiling byte code, normal computations are directly translated to machine code,
/// while queries are called through ABI, which in turn creates these query structures.
#[derive(Debug, Clone)]
pub enum Query {
    /// Upsert query can be used to insert or update a record in the database.
    /// We also store normal insert and update queries as upsert queries,
    /// as most of the logic is the same. We only differentiate them when necessary,
    /// such as when we need to fail on duplicate key for insert,
    /// or when we need to update only if the record exists.
    Upsert(UpsertQuery),
    Delete(DeleteQuery),
    Select(SelectQuery),
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct FieldOp {
    /// Index of the field in the record.
    pub idx: FieldIdx,

    /// The operation to perform to calculate the value of the field.
    pub op: FieldOpType,
}

#[derive(Debug, Clone)]
pub enum FieldOpType {
    /// Set the field to a constant value. This value may be explicitly provided by the user,
    /// or it may be the result of the code execution, which leads to this result stored here.
    SetConstant(DataValue),

    /// Set the field to a value which is the result of an another query.
    SetFromQuery(Box<Query>),
}

#[derive(Debug, Clone)]
pub struct DataValue(Vec<u8>);

#[derive(Debug, Clone)]
pub struct DeleteQuery {
    /// The type of the record to delete.
    pub table: TypeId,

    /// The condition to match records to delete.
    /// If no conditions are provided, all records of the type will be deleted.
    pub conditions: Vec<Condition>,

    /// Type of the value being returned after query execution.
    pub returning: Option<TypeId>,
}

#[derive(Debug, Clone)]
pub enum Condition {
    /// A condition that matches records based on a range of field values.
    /// If [min](Condition::Range::min) is [None], it means no lower bound, 
    /// and if [max](Condition::Range::max) is [None], it means no upper bound.
    /// Effectively, if min and max are set to the same [Some] value, it matches only that value,
    /// and acts as an equality condition. When min is set to [None] and max is set to [Some],
    /// it matches all values less than or equal to the max value.
    /// If min is set to [Some] and max is set to [None], it matches all values greater than or equal to the min value.
    /// The boolean values indicate whether the min and max values are inclusive (true) or exclusive (false).
    Range { field: FieldIdx, min: Option<(DataValue, bool)>, max: Option<(DataValue, bool)> },

    /// Match a field against values returned by a subquery. [is_not](Condition::InQuery::is_not)
    /// indicates whether to match records that are in the subquery result (false) or not in the subquery result (true).
    InQuery { field: FieldIdx, query: Box<Query>, is_not: bool },
}

#[derive(Debug, Clone)]
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
