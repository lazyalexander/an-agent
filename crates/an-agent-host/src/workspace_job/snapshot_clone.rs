//! Optional filesystem acceleration; never borrow a mutable source inode.
use crate::execution::Result;
use std::path::Path;

/// False means the filesystem cannot clone; all other errors fail closed.
pub(super) fn create(source: &Path, target: &Path, limit: usize) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        use crate::execution::{metrics::Span, store};
        use std::{
            ffi::CString,
            fs::OpenOptions,
            os::unix::{
                ffi::OsStrExt,
                fs::{MetadataExt, OpenOptionsExt},
                io::AsRawFd,
            },
        };
        store::no_links(source)?;
        store::no_links(target)?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(source)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > limit as u64 {
            return Err("snapshot clone requires a bounded unlinked regular input".into());
        }
        let destination = CString::new(target.as_os_str().as_bytes())?;
        let span = Span::new("snapshot_clone", target, metadata.len() as usize);
        // Darwin sys/clonefile.h; libc exposes the function but not this flag.
        const CLONE_NOOWNERCOPY: u32 = 0x0002;
        // The live descriptor pins the source inode across pathname replacement.
        // fclonefileat exclusively creates a new inode; writes to either diverge.
        let result = unsafe {
            libc::fclonefileat(
                file.as_raw_fd(),
                libc::AT_FDCWD,
                destination.as_ptr(),
                CLONE_NOOWNERCOPY,
            )
        };
        if result == 0 {
            span.finish("cloned");
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(libc::EXDEV | libc::ENOTSUP)) {
            span.finish("copy_fallback");
            return Ok(false);
        }
        span.finish("error");
        Err(error.into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (source, target, limit);
        Ok(false)
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "snapshot_clone_tests.rs"]
mod tests;
