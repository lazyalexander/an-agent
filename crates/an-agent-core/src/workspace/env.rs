//! Environment storage only: immutable bytes, committed reference, live projection.
use super::*;
use std::io;

impl Workspace {
    pub(super) fn committed_env(
        &self,
    ) -> Result<(Option<WorkspaceRecord>, String), WorkspaceError> {
        let record = self.last_kind("env")?;
        let text = match &record {
            None => String::new(),
            Some(record) => {
                let sha = record
                    .sha256
                    .as_deref()
                    .ok_or_else(|| WorkspaceError::InvalidEnv("generation has no digest".into()))?;
                String::from_utf8(self.env_bytes(sha)?)
                    .map_err(|_| WorkspaceError::InvalidEnv(format!("non-UTF-8 blob: {sha}")))?
            }
        };
        Ok((record, text))
    }

    pub(super) fn publish_env_blob(&self, sha: &str, bytes: &[u8]) -> Result<(), WorkspaceError> {
        let path = self.env_path(sha)?;
        if path.exists() {
            self.env_bytes(sha)?;
            fs::File::open(&path)?.sync_all()?;
            sync_directory(&self.root.join("env"))?;
            return Ok(());
        }
        let temp = self
            .root
            .join("env")
            .join(format!(".pending-{}", Entropy::os().uuid_v4()));
        let result = (|| -> Result<(), WorkspaceError> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            #[cfg(test)]
            self.env_checkpoint("blob_write")?;
            file.write_all(bytes)?;
            #[cfg(test)]
            self.env_checkpoint("blob_sync")?;
            file.sync_all()?;
            #[cfg(test)]
            self.env_checkpoint("blob_publish")?;
            // Atomic no-replace publication; an existing identity is never rewritten.
            match fs::hard_link(&temp, &path) {
                Ok(()) => (),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    self.env_bytes(sha)?;
                }
                Err(error) => return Err(error.into()),
            }
            #[cfg(test)]
            self.env_checkpoint("blob_directory_sync")?;
            sync_directory(&self.root.join("env"))?;
            Ok(())
        })();
        let _ = fs::remove_file(&temp);
        result
    }

    pub(super) fn refresh_env_projection(&self) -> Result<(), WorkspaceError> {
        let (record, text) = self.committed_env()?;
        let temp = self
            .root
            .join(format!(".env-{}.tmp", Entropy::os().uuid_v4()));
        let result = (|| -> io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            #[cfg(test)]
            self.env_checkpoint("projection_write")?;
            file.write_all(text.as_bytes())?;
            #[cfg(test)]
            self.env_checkpoint("projection_sync")?;
            file.sync_all()?;
            #[cfg(test)]
            self.env_checkpoint("projection_rename")?;
            fs::rename(&temp, self.root.join("env.md"))?;
            #[cfg(test)]
            self.env_checkpoint("projection_directory_sync")?;
            sync_directory(&self.root)
        })();
        let _ = fs::remove_file(&temp);
        result.map_err(|source| WorkspaceError::EnvProjection {
            generation: record.map(|record| record.id),
            source,
        })
    }

    #[cfg(test)]
    pub(super) fn env_checkpoint(&self, stage: &'static str) -> io::Result<()> {
        let mut fault = self.fail_env_at.lock().unwrap();
        if *fault == Some(stage) {
            *fault = None;
            Err(io::Error::other(format!(
                "injected environment failure: {stage}"
            )))
        } else {
            Ok(())
        }
    }
}

pub(super) fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
