
/// Bytecode that was not yet compiled to machine code.
/// This is used to store queries that are not yet compiled, so that they can be
/// compiled later when needed. We also receive bytecode from the client in this format,
/// so that we can compile it to machine code on the server side.
#[derive(Debug, Clone)]
pub struct Raw(pub Vec<u8>);
