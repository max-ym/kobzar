use std::ops;

use super::*;

/// Layout calculator. Calculate the layout of fields of a structure's record,
/// taking into account the types of fields, their sizes, whether they are BLOBs,
/// and possible requirements for metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// Buffer capacity to store the record.
    pub fn required_buf_capacity(&self) -> usize {
        let total_data_size: u64 = self.total_data_size.into();
        let total_bits = self.total_bits;
        let bitmap_size = (total_bits + 7) / 8; // Round up to the nearest byte
        let total_size = total_data_size + bitmap_size;
        total_size as usize
    }

    /// Union of two layout calculations.
    /// This will return a new layout calculation that is the union of the two,
    /// taking the maximum of the total data size and total bits.
    /// 
    /// This is useful for calculating the allocation requirements for a ADT node
    /// that can contain different types of records, hence different layouts for each
    /// possible variant.
    fn union(self, other: LayoutCalc) -> Self {
        LayoutCalc {
            total_data_size: self.total_data_size.max(other.total_data_size),
            total_bits: self.total_bits.max(other.total_bits),
        }
    }
}

impl ops::Add<FieldLayout> for LayoutCalc {
    type Output = Self;

    fn add(mut self, layout: FieldLayout) -> Self {
        self += layout;
        self
    }
}

impl ops::AddAssign<FieldLayout> for LayoutCalc {
    fn add_assign(&mut self, layout: FieldLayout) {
        self.total_data_size += layout.data_size;
        self.total_bits += layout.bits as u64;
    }
}

impl ops::BitOr for LayoutCalc {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

impl ops::BitOrAssign for LayoutCalc {
    fn bitor_assign(&mut self, other: Self) {
        *self = self.union(other);
    }
}

impl Default for LayoutCalc {
    fn default() -> Self {
        Self::new()
    }
}
