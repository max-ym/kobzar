use thiserror::Error;

use super::*;

/// Raw Rust that was not yet compiled to machine code.
/// This is used to store queries that are not yet compiled, so that they can be
/// compiled later when needed. We receive Rust code from the client,
/// so that we can compile it to machine code on the server side, ensuring safe
/// sandboxed libraries during compilation.
#[derive(Debug, Clone)]
pub struct Raw(pub String);

/// Function that operates on the structure's field to validate its content.
/// It should return [Ok] with potentially modified field value,
/// or [Err] with an error message if the validation fails.
#[derive(Debug)]
pub struct FieldValidationCode {
    /// The type of the field that the function operates on.
    pub field_type: TypeId,

    /// The code for the function that operates on the field.
    pub code: Raw,
}

impl FieldValidationCode {
    pub fn new(field_type: TypeId, code: Raw) -> Self {
        Self { field_type, code }
    }

    /// Compile and dynamically link the validation code.
    pub fn compile_dynlink(&self) -> Result<FieldValidationCall, CompilationError> {
        todo!()
    }
}

#[derive(Debug)]
pub struct FieldValidationCall {
    pub lib: libloading::Library,
}

impl FieldValidationCall {
    /// Call the validation function with the given field value.
    pub fn call(&self, field_value: DataValue) -> Result<DataValue, String> {
        // Here we would use the libloading to call the function in the library.
        // This is a placeholder for the actual implementation.
        Ok(field_value) // Placeholder
    }
}

#[derive(Debug)]
pub struct FieldTransformCall {
    /// The code for the function that transforms the field value.
    pub lib: libloading::Library,
}

#[derive(Debug, Error)]
pub enum CompilationError {
    // TODO
}

/// Function that operates on the structure's "self" argument.
#[derive(Debug)]
pub struct SelfStructCall {
    /// The code for the function that operates on the structure in form of compiled
    /// Rust (C ABI) code library.
    pub lib: libloading::Library,
}
