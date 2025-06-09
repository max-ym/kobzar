use std::path::PathBuf;

use super::*;

type Id = u64;

/// Database storage engine.
pub struct DbStore {
    // TODO
}

impl DbStore {
    pub fn open(_path: PathBuf) -> std::io::Result<Self> {
        Ok(DbStore {
            // TODO
        })
    }
}

/// Layout calculator. Calculate the layout of fields of a structure's record,
/// taking into account the types of fields, their sizes, whether they are BLOBs,
/// and possible requirements for metadata.
pub struct LayoutCalc {
    /// Total data size of the record, in bytes, excluding the size of the bitmap.
    total_data_size: DataSize,

    /// Total size of the bitmap, in bits.
    total_bits: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldLayout {
    /// Data size in bytes.
    data_size: DataSize,

    /// Field bits, for records that have sub-byte data associated with them.
    /// For example, [Option] type will have 1 bit for the presence of the value,
    /// or enum of 4 elements will have 2 bits for the value. This always should
    /// be between 0 and 7 bits, inclusive.
    bits: u8,
}

impl LayoutCalc {
    pub fn new() -> Self {
        LayoutCalc {
            total_data_size: 0.into(),
            total_bits: 0,
        }
    }

    /// Push the layout of a field to the calculator.
    /// This will update the total data size and total bits, as well as the order index.
    /// Order is affected by the size of the field.
    pub fn push(&mut self, layout: FieldLayout) {
        self.total_data_size += layout.data_size;
        self.total_bits += layout.bits as u64;
    }

    /// Buffer capacity to store the record.
    pub fn required_buf_capacity(&self) -> usize {
        let total_data_size: u64 = self.total_data_size.into();
        let total_bits = self.total_bits;
        let bitmap_size = (total_bits + 7) / 8; // Round up to the nearest byte
        let total_size = total_data_size + bitmap_size;
        total_size as usize
    }
}

impl Default for LayoutCalc {
    fn default() -> Self {
        Self::new()
    }
}
