//! Hands an unloaded model's memory back; Windows does it itself.

/// Asks the allocator to return its free pages to the system.
pub(crate) fn give_back() {
    #[cfg(target_os = "macos")]
    mac();
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    linux();
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn mac() {
    unsafe extern "C" {
        /// libmalloc: frees what it can in `zone`, or every zone for null.
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }
    // SAFETY: null asks every zone to release free pages; live blocks stay.
    unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[allow(unsafe_code)]
fn linux() {
    unsafe extern "C" {
        /// glibc: returns free heap pages to the system, keeping `pad` bytes.
        fn malloc_trim(pad: usize) -> std::ffi::c_int;
    }
    // SAFETY: only free pages go back; live blocks stay where they are.
    unsafe { malloc_trim(0) };
}

#[cfg(test)]
mod tests {
    #[test]
    fn giving_back_leaves_live_memory_alone() {
        let kept = vec![7u8; 1 << 20];
        drop(vec![1u8; 64 << 20]);
        super::give_back();
        assert!(kept.iter().all(|b| *b == 7));
    }
}
