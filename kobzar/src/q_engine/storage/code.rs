use super::*;

/// ID into index file, which is used to reference code records.
pub type Id = u32;

/// A fingerprint of the compiler used to compile the code.
/// This is generated accounting to the compiler version, target architecture,
/// API library version,
/// and other relevant parameters that affect the generated code.
/// This allows to detect if the code was compiled with a different compiler,
/// and thus should be recompiled before execution.
pub type CompilerFingerprint = u128;

/// A record in the code index file, which contains metadata about the code.
/// If whole record is zeroed, it means the code was deleted, and index entry is free for reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct IndexRecord {
    /// Fingerprint of the compiler used to compile the code.
    /// If set to zero, the code is not compiled and is in raw format only.
    pub fingerprint: CompilerFingerprint,

    /// A checksum to verify the integrity of the compiled binary.
    pub compiled_checksum: u128,

    /// A checksum of the raw code.
    /// This is used to verify that the raw code has not changed since it was compiled.
    pub raw_checksum: u128,
}
