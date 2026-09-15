//! Shared test scaffolding: self-cleaning temporary directories.

use std::path::{Path, PathBuf};

/// A temporary directory that deletes itself on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let id = crate::det_seam::Entropy::os().ulid(crate::det_seam::Clock::wall().now_ms());
        let dir = std::env::temp_dir().join(format!("an-agent-{tag}-{id}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
