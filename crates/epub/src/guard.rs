//! Size caps read from the zip directory; zip enforces declared sizes.
use std::io::Cursor;

use zip::ZipArchive;

use crate::OpenError;

/// Files in one book; real books hold hundreds.
pub const MAX_ENTRIES: usize = 10_000;
/// One unpacked file, generous for a large picture.
pub const MAX_ENTRY: u64 = 128 << 20;
/// Everything unpacked together.
pub const MAX_TOTAL: u64 = 512 << 20;

pub(crate) fn check(bytes: &[u8]) -> Result<(), OpenError> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|_| OpenError::Invalid)?;
    if zip.len() > MAX_ENTRIES {
        return Err(OpenError::TooLarge);
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        let size = zip.by_index_raw(i).map_err(|_| OpenError::Invalid)?.size();
        total = total.saturating_add(size);
        if size > MAX_ENTRY || total > MAX_TOTAL {
            return Err(OpenError::TooLarge);
        }
    }
    Ok(())
}
