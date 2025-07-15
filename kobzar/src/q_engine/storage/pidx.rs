use pulp::{Arch, Simd, WithSimd, bytemuck};

use super::*;

/// Page size in records to store Primary Index Records.
pub const PAGE_SIZE_RECS: usize = 4096;

/// Bitmap size that represents the visibility of records in a page.
pub const PAGE_SIZE_BITMAP: usize = PAGE_SIZE_RECS / 8;

/// Item number in the primary index file.
pub type ItemId = u64;

/// Page of primary index records.
/// Page is a fixed-size structure that contains metadata about the records in the heap file.
/// The page consists of multiple arrays for each field (column), so to allow SIMD
/// acceleration for different operations on the columns.
/// Some fields are grouped together for cache locality, since they are not processed
/// with SIMD, but often accessed together.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C, align(64))] // Align to SIMD-friendly size for better performance
pub struct Page {
    /// Grouped fields for cache locality.
    pub group: [PageGroupFields; PAGE_SIZE_RECS],

    /// Transaction ID that created the record.
    pub xmin: [u64; PAGE_SIZE_RECS],

    /// Transaction ID that deleted the record.
    /// Set to [u64::MAX] if the record is not deleted.
    pub xmax: [u64; PAGE_SIZE_RECS],

    /// Next record version ID.
    /// Set to [u64::MAX] if there is no next record version.
    pub next: [u64; PAGE_SIZE_RECS],

    /// Previous record version ID.
    /// Set to [u64::MAX] if there is no previous record version.
    pub prev: [u64; PAGE_SIZE_RECS],

    /// Schema version of the record.
    pub schema_version: [schema::Version; PAGE_SIZE_RECS],
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}

struct VisibilityMapCalc<'a> {
    page: &'a Page,
    x: u64,
}

impl<'a> WithSimd for VisibilityMapCalc<'a> {
    type Output = [u8; PAGE_SIZE_BITMAP];

    // TODO optimize: transactions almost never get IDs more than 32 bits can represent,
    // but this database supports 64-bit transaction IDs,
    // we can optimize this by using 32-bit transaction IDs
    // and using 32-bit SIMD operations for visibility map calculations, when we check
    // for having transaction ID above 32 bits representation capacity.
    #[inline(always)]
    fn with_simd<S: Simd>(self, simd: S) -> Self::Output {
        let mut map = [0; PAGE_SIZE_BITMAP];

        let (xmax, tail1) = S::as_simd_u64s(&self.page.xmin);
        let (xmin, tail2) = S::as_simd_u64s(&self.page.xmax);
        const TAIL_ERR: &str = "Tail should be empty for SIMD operations";
        debug_assert_eq!(0, tail1.len(), "{TAIL_ERR}");
        debug_assert_eq!(0, tail2.len(), "{TAIL_ERR}");

        let x_splat = simd.splat_u64s(self.x);

        fn and<S: Simd>(simd: S, max: S::u64s, min: S::u64s, x_splat: S::u64s) -> S::m64s {
            let mask_min = simd.greater_than_or_equal_u64s(max, x_splat);
            let mask_max = simd.less_than_u64s(min, x_splat);
            simd.and_m64s(mask_min, mask_max)
        }

        if S::U64_LANES == 8 {
            for i in 0..PAGE_SIZE_BITMAP {
                let and = and(simd, xmax[i], xmin[i], x_splat);

                let elements: [u64; 8] = bytemuck::cast(and);
                map[i] = ((elements[0] as u8) & (1 << 0))
                    | ((elements[1] as u8) & (1 << 1))
                    | ((elements[2] as u8) & (1 << 2))
                    | ((elements[3] as u8) & (1 << 3))
                    | ((elements[4] as u8) & (1 << 4))
                    | ((elements[5] as u8) & (1 << 5))
                    | ((elements[6] as u8) & (1 << 6))
                    | ((elements[7] as u8) & (1 << 7));
            }
        } else if S::U64_LANES == 4 {
            for i in 0..(PAGE_SIZE_BITMAP * 2) {
                let a = and(simd, xmax[i], xmin[i], x_splat);
                let b = and(simd, xmax[i + 1], xmin[i + 1], x_splat);

                let a: [u64; 4] = bytemuck::cast(a);
                let b: [u64; 4] = bytemuck::cast(b);
                map[i / 2] = ((a[0] as u8) & (1 << 0))
                    | ((a[1] as u8) & (1 << 1))
                    | ((a[2] as u8) & (1 << 2))
                    | ((a[3] as u8) & (1 << 3))
                    | ((b[0] as u8) & (1 << 4))
                    | ((b[1] as u8) & (1 << 5))
                    | ((b[2] as u8) & (1 << 6))
                    | ((b[3] as u8) & (1 << 7));
            }
        } else if S::U64_LANES == 2 {
            for i in 0..(PAGE_SIZE_BITMAP * 4) {
                let a = and(simd, xmax[i], xmin[i], x_splat);
                let b = and(simd, xmax[i + 1], xmin[i + 1], x_splat);
                let c = and(simd, xmax[i + 2], xmin[i + 2], x_splat);
                let d = and(simd, xmax[i + 3], xmin[i + 3], x_splat);

                let a: [u64; 2] = bytemuck::cast(a);
                let b: [u64; 2] = bytemuck::cast(b);
                let c: [u64; 2] = bytemuck::cast(c);
                let d: [u64; 2] = bytemuck::cast(d);
                map[i / 4] = ((a[0] as u8) & (1 << 0))
                    | ((a[1] as u8) & (1 << 1))
                    | ((b[0] as u8) & (1 << 2))
                    | ((b[1] as u8) & (1 << 3))
                    | ((c[0] as u8) & (1 << 4))
                    | ((c[1] as u8) & (1 << 5))
                    | ((d[0] as u8) & (1 << 6))
                    | ((d[1] as u8) & (1 << 7));
            }
        } else {
            let xmax = &self.page.xmax;
            let xmin = &self.page.xmin;
            // Fallback for when SIMD lanes are not supported
            for i in 0..PAGE_SIZE_RECS {
                let max = xmax[i];
                let min = xmin[i];
                let mask_min = max >= self.x;
                let mask_max = min < self.x;

                // Combine the masks into a single byte
                map[i / 8] |= if mask_min && mask_max {
                    1 << (i % 8)
                } else {
                    0
                };
            }
        }

        map
    }
}

impl Page {
    /// Create a new empty page.
    pub const fn new() -> Self {
        const INVALID: u64 = u64::MAX;
        const INVALID32: u32 = u32::MAX;
        Self {
            group: [PageGroupFields {
                size: INVALID32,
                offset: INVALID32,
            }; PAGE_SIZE_RECS],
            xmin: [INVALID; PAGE_SIZE_RECS],
            xmax: [INVALID; PAGE_SIZE_RECS],
            next: [INVALID; PAGE_SIZE_RECS],
            prev: [INVALID; PAGE_SIZE_RECS],
            schema_version: [INVALID32; PAGE_SIZE_RECS],
        }
    }

    /// Check `xmin` and `xmax` visibility maps for the given transaction ID.
    /// This combines the visibility maps for both `xmin` and `xmax` into a single map.
    /// This is useful for determining if a record is visible to the transaction.
    pub fn visibility_map(&self, x: u64) -> [u8; PAGE_SIZE_BITMAP] {
        Arch::new().dispatch(VisibilityMapCalc { page: self, x })
    }
}

/// We inline some of the fields for cache locality, since these fields are not
/// processed with SIMD, but often are accessed together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub struct PageGroupFields {
    /// Size of the record in bytes. Since we store BLOBs separately, u32 limitations
    /// are enough for the size of the record. Using it instead of [u64] saves space for
    /// better cache usage.
    pub size: u32,

    /// Offset of the record in the heap file.
    pub offset: u32,
}

pub type PageStore = idx_common::PageStore<PageBoundKey, Page, { size_of::<Page>() }>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    /// Type ID.
    pub type_id: u64,

    /// Primary index item number.
    pub item: ItemId,

    /// Database ID.
    pub db: u64,
}

/// A key that is aligned to the page boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct PageBoundKey(Key);

impl Deref for PageBoundKey {
    type Target = Key;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl idx_common::KeyExt for Key {
    type PageBoundKey = PageBoundKey;

    /// Get the page bound key for the item.
    fn page_bound(&self) -> Self::PageBoundKey {
        Self::page_bound(*self)
    }

    fn filename(&self) -> Cow<'static, str> {
        format!("pidx-{}-{}.idx", self.db, self.type_id).into()
    }
}

impl Key {
    /// Create a new key for the given type ID and item number where
    /// the item number is aligned to the page boundaries.
    /// This effectively can create a key for the page in the cache from the item number.
    pub const fn page_bound(self) -> PageBoundKey {
        PageBoundKey(Key {
            type_id: self.type_id,
            item: self.item - (self.item % PAGE_SIZE_RECS as u64),
            db: self.db,
        })
    }

    /// Get the in-page index for the item.
    pub const fn in_page_offset(self) -> usize {
        (self.item % PAGE_SIZE_RECS as u64) as usize
    }
}

impl idx_common::KeyExt for PageBoundKey {
    type PageBoundKey = Self;

    /// Get the page bound key for the item.
    fn page_bound(&self) -> Self::PageBoundKey {
        *self
    }

    fn filename(&self) -> Cow<'static, str> {
        self.0.filename()
    }
}
