//! Private tool bodies live beside the tape; events carry explicit references.
use super::{JsonlStore, Memevent, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};

pub const TOOL_PAYLOAD_LIMIT: usize = 1024 * 1024;
pub const TOOL_PAYLOAD_TAG: &str = "tool_payload_v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    schema: String,
    sha256: String,
    bytes: usize,
}

impl JsonlStore {
    fn payload_path(&self, sha: &str) -> Result<PathBuf, StoreError> {
        if sha.len() != 64
            || !sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(StoreError::Corrupt("invalid tool payload digest".into()));
        }
        Ok(self
            .path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("blobs")
            .join(sha))
    }

    /// Store bounded bytes before admitting the reference. Blobs are private,
    /// sensitive data, not sanitized text; backup must include this directory.
    pub fn tool_payload(&self, text: &str) -> Result<String, StoreError> {
        self.check_owner()?;
        if text.len() > TOOL_PAYLOAD_LIMIT {
            return Err(StoreError::Corrupt("tool payload exceeds 1 MiB".into()));
        }
        let sha = format!("{:x}", Sha256::digest(text.as_bytes()));
        let path = self.payload_path(&sha)?;
        let dir = path
            .parent()
            .ok_or_else(|| StoreError::Corrupt("missing blob parent".into()))?;
        let mut directory = fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory.create(dir)?;
        if !fs::symlink_metadata(dir)?.file_type().is_dir() {
            return Err(StoreError::Corrupt(
                "blob directory is not a directory".into(),
            ));
        }
        let temp = dir.join(format!(
            ".pending-{}",
            crate::det_seam::Entropy::os().uuid_v4()
        ));
        let result = (|| -> Result<(), StoreError> {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            match fs::hard_link(&temp, &path) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if fs::symlink_metadata(&path)?.file_type().is_symlink()
                        || fs::symlink_metadata(&path)?.len() != text.len() as u64
                        || fs::read(&path)? != text.as_bytes()
                    {
                        return Err(StoreError::Corrupt("tool blob identity mismatch".into()));
                    }
                }
                Err(e) => return Err(e.into()),
            }
            #[cfg(unix)]
            fs::File::open(dir)?.sync_all()?;
            Ok(())
        })();
        let _ = fs::remove_file(temp);
        result?;
        Ok(serde_json::to_string(&Reference {
            schema: TOOL_PAYLOAD_TAG.into(),
            sha256: sha,
            bytes: text.len(),
        })?)
    }

    /// Legacy inline records remain literal. Explicit new references fail closed
    /// on missing, corrupt or oversized bodies rather than inventing content.
    pub fn resolve_tool_payload(&self, event: &Memevent) -> Result<String, StoreError> {
        self.check_owner()?;
        resolve_tool_payload_at(
            self.path.parent().unwrap_or(std::path::Path::new(".")),
            event,
        )
    }
}

/// Resolve a private tool body for context readers that do not own the tape writer.
pub fn resolve_tool_payload_at(
    directory: &std::path::Path,
    event: &Memevent,
) -> Result<String, StoreError> {
    if !event.tags.iter().any(|t| t == TOOL_PAYLOAD_TAG) {
        return Ok(event.content.clone());
    }
    let reference: Reference = serde_json::from_str(&event.content)?;
    if reference.schema != TOOL_PAYLOAD_TAG || reference.bytes > TOOL_PAYLOAD_LIMIT {
        return Err(StoreError::Corrupt("invalid tool payload reference".into()));
    }
    if reference.sha256.len() != 64
        || !reference
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(StoreError::Corrupt("invalid tool payload digest".into()));
    }
    let path = directory.join("blobs").join(&reference.sha256);
    if !fs::symlink_metadata(&path)?.file_type().is_file() {
        return Err(StoreError::Corrupt(
            "tool payload is not a regular file".into(),
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(TOOL_PAYLOAD_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() != reference.bytes || format!("{:x}", Sha256::digest(&bytes)) != reference.sha256
    {
        return Err(StoreError::Corrupt("tool payload identity mismatch".into()));
    }
    String::from_utf8(bytes).map_err(|_| StoreError::Corrupt("tool payload is not UTF-8".into()))
}
