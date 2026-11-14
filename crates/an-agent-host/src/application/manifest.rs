//! Portable offline payload identities; content hashes do not authenticate publishers.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub name: String,
    pub version: String,
    pub platform: String,
    pub licenses: Vec<String>,
    pub components: BTreeMap<String, String>,
    pub files: BTreeMap<String, File>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub sha256: String,
    pub bytes: u64,
    pub executable: bool,
}
impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != "an-offline-runtime-v1"
            || self.platform != "macos-aarch64"
            || self.files.is_empty()
            || self.files.len() > 16384
            || self.name.is_empty()
            || self.name.len() > 128
            || self.version.is_empty()
            || self.version.len() > 128
            || self.licenses.is_empty()
            || self.components.is_empty()
        {
            return Err("invalid offline runtime manifest".into());
        }
        let mut total = 0u64;
        for (name, file) in &self.files {
            if !safe_path(name)
                || name == "runtime.json"
                || file.sha256.len() != 64
                || !file
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("invalid runtime payload identity".into());
            }
            total = total
                .checked_add(file.bytes)
                .ok_or("runtime size overflow")?;
        }
        if total > 32 * 1024 * 1024 * 1024 {
            return Err("runtime size limit".into());
        }
        for path in self.components.values().chain(self.licenses.iter()) {
            if !self.files.contains_key(path) {
                return Err("missing runtime component or license".into());
            }
        }
        Ok(())
    }
}
pub fn safe_path(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && name.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
        })
}
