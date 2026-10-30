//! Portable release bytes, independent of the developer tool that produced them.
use crate::{
    execution::{Result, store},
    file_job::regular,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(super) const FILE: &str = "native-spool.json";
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    pub schema: String,
    pub name: String,
    pub version: String,
    pub entry: String,
    pub files: BTreeMap<String, String>,
    pub outputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirements: Option<super::requirements::Requirements>,
}

pub(super) fn identifier(text: &str) -> Result<()> {
    if text.is_empty()
        || text.len() > 64
        || text == "."
        || text == ".."
        || !text
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err("expected a bounded portable identifier".into());
    }
    Ok(())
}

pub(super) struct Bundle {
    pub manifest: Manifest,
    pub bytes: Vec<u8>,
    pub payload: BTreeMap<String, Vec<u8>>,
    pub release: String,
}

pub(super) fn read(root: &Path) -> Result<Bundle> {
    let bundle = inspect(root)?;
    super::requirements::validate(&bundle.manifest)?;
    Ok(bundle)
}

/// Verify release bytes for inspection without admitting their runtime requests.
pub(super) fn inspect(root: &Path) -> Result<Bundle> {
    let bytes = regular(&root.join(FILE), 65536)?;
    let m: Manifest = serde_json::from_slice(&bytes)?;
    identifier(&m.name)?;
    identifier(&m.version)?;
    super::requirements::schema(&m)?;
    if !matches!(
        m.schema.as_str(),
        "an-native-spool-v1" | "an-native-spool-v2"
    ) || m.files.is_empty()
        || m.files.len() > 12
        || m.outputs.is_empty()
        || m.outputs.len() > 16
        || !m.entry.starts_with("silk/")
        || !m.files.contains_key(&m.entry)
    {
        return Err("unsupported native spool manifest".into());
    }
    let mut names = BTreeSet::new();
    let mut payload = BTreeMap::new();
    let mut total = 0;
    for (path, hash) in &m.files {
        let (role, name) = path.split_once('/').ok_or("missing code role")?;
        identifier(name)?;
        if !matches!(role, "silk" | "kit") || !names.insert(name) || !store::valid_digest(hash) {
            return Err("invalid role, duplicate basename or digest".into());
        }
        let data = regular(&root.join(path), 1024 * 1024)?;
        total += data.len();
        if total > 8 * 1024 * 1024 || store::digest(&data) != *hash {
            return Err("release payload size or hash mismatch".into());
        }
        payload.insert(path.clone(), data);
    }
    let mut outputs = BTreeSet::new();
    for output in &m.outputs {
        identifier(output)?;
        if matches!(output.as_str(), "kit" | "tmp") || !outputs.insert(output) {
            return Err("invalid or duplicate output slot".into());
        }
    }
    // No unregistered files, links or nested directories travel with the release.
    let mut expected: BTreeSet<_> = m.files.keys().cloned().collect();
    expected.insert(FILE.into());
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("invalid path")?;
        if matches!(name, "silk" | "kit") {
            store::no_links(&path)?;
            for file in fs::read_dir(&path)? {
                let file = file?.path();
                regular(&file, 1024 * 1024)?;
                actual.insert(format!(
                    "{name}/{}",
                    file.file_name()
                        .and_then(|n| n.to_str())
                        .ok_or("invalid path")?
                ));
            }
        } else {
            actual.insert(name.to_owned());
        }
    }
    if actual != expected {
        return Err("unregistered release material".into());
    }
    Ok(Bundle {
        manifest: m,
        release: store::digest(&bytes),
        bytes,
        payload,
    })
}
