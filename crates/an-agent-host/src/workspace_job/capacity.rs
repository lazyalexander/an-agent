//! Admission headroom, not a runtime filesystem quota or reservation.
use crate::execution::Result;
use std::path::Path;

pub(super) const RESERVE: u64 = 128 * 1024 * 1024;

#[cfg(unix)]
pub(crate) fn available(root: &Path) -> Result<u64> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let path = CString::new(root.as_os_str().as_bytes())?;
    let mut value = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // The C path stays owned for the call; output is read only on success.
    if unsafe { libc::statvfs(path.as_ptr(), value.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let value = unsafe { value.assume_init() };
    Ok(u64::try_from(
        u128::from(value.f_bavail) * u128::from(value.f_frsize),
    )?)
}

#[cfg(not(unix))]
pub(crate) fn available(_: &Path) -> Result<u64> {
    Err("native storage observation is unsupported on this platform".into())
}

pub(super) fn ensure(root: &Path, required: u64) -> Result<()> {
    let free = available(root)?;
    if free < required {
        return Err(format!("insufficient storage headroom: need {required} bytes, observed {free}; no workload started").into());
    }
    Ok(())
}
