//! Host-owned immutable checkpoints of bounded file artifacts, not VM snapshots.
use crate::execution::{Result, store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, path::Path};

const LIMIT: usize = 65536;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    schema: String,
    content_sha256: String,
    bytes: usize,
    parent: Option<String>,
    source: Value,
}

/// Import only an accepted file output. Receipt integrity is not authorship or
/// domain correctness; the source run and catalog must be host-owned and private.
pub fn import(catalog: &Path, run: &Path) -> Result<Value> {
    store::no_links(run)?;
    store::no_links(catalog)?;
    let catalog_path = if catalog.exists() {
        catalog.canonicalize()?
    } else {
        catalog
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?
            .join(catalog.file_name().ok_or("missing catalog name")?)
    };
    if catalog_path.starts_with(run.canonicalize()?) {
        return Err("artifact catalog must be outside the source run".into());
    }
    let receipt_bytes = crate::file_job::regular(&run.join("receipt.json"), LIMIT)?;
    let receipt: Value = serde_json::from_slice(&receipt_bytes)?;
    let run_id = receipt["run_id"]
        .as_str()
        .ok_or("missing source run identity")?;
    uuid::Uuid::parse_str(run_id)?;
    let release = receipt["release"]
        .as_str()
        .filter(|s| store::valid_digest(s))
        .ok_or("missing source release identity")?;
    if receipt["schema_version"] != 1
        || receipt["status"] != "succeeded"
        || receipt["exit_code"] != 0
        || receipt["process"]["reaped"] != true
        || receipt["artifact"]["path"] != "artifact.txt"
    {
        return Err("only a host-accepted successful file artifact can be imported".into());
    }
    let bytes = crate::file_job::regular(&run.join("artifact.txt"), LIMIT)?;
    if receipt["artifact"]["sha256"] != store::digest(&bytes)
        || receipt["artifact"]["bytes"].as_u64() != Some(bytes.len() as u64)
    {
        return Err("accepted artifact identity mismatch".into());
    }
    publish(
        catalog,
        &bytes,
        None,
        json!({"kind":"accepted-run",
        "run_id":run_id,"release":release,
        "receipt_sha256":store::digest(&receipt_bytes),"input_revision":{
            "revision":receipt["input_revision"]["revision"],
            "content_sha256":receipt["input_revision"]["content_sha256"]}}),
    )
}

/// Explicitly checkpoint an operator-saved file. The digest approves exact bytes;
/// it does not prove the editor saved a semantically consistent document.
pub fn edit(catalog: &Path, parent: &str, file: &Path, approved: &str) -> Result<Value> {
    read(catalog, parent)?;
    if !store::valid_digest(approved) {
        return Err("invalid approved content digest".into());
    }
    let bytes = crate::file_job::regular(file, LIMIT)?;
    if store::digest(&bytes) != approved {
        return Err("edited file changed since approval".into());
    }
    publish(
        catalog,
        &bytes,
        Some(parent.to_owned()),
        json!({"kind":"operator-checkpoint","authorship":"operator-declared; not authenticated"}),
    )
}

pub(crate) fn publish(
    catalog: &Path,
    bytes: &[u8],
    parent: Option<String>,
    source: Value,
) -> Result<Value> {
    std::str::from_utf8(bytes)?;
    store::directory(catalog)?;
    let _lock = store::lock(catalog)?;
    let revisions = catalog.join("revisions");
    store::directory(&revisions)?;
    let manifest = Revision {
        schema: "an-artifact-revision-v1".into(),
        content_sha256: store::digest(bytes),
        bytes: bytes.len(),
        parent,
        source,
    };
    let encoded = serde_json::to_vec(&manifest)?;
    if encoded.len() > LIMIT {
        return Err("revision metadata exceeds limit".into());
    }
    let revision = store::digest(&encoded);
    let target = revisions.join(&revision);
    if target.exists() {
        read(catalog, &revision)?;
    } else {
        let pending = revisions.join(format!(
            "{}.pending",
            an_agent_core::det_seam::Entropy::os().uuid_v4()
        ));
        store::directory(&pending)?;
        store::write_new(&pending.join("content.txt"), bytes)?;
        store::write_new(&pending.join("revision.json"), &encoded)?;
        fs::rename(&pending, &target)?;
    }
    Ok(
        json!({"status":"succeeded","revision":revision,"content_sha256":manifest.content_sha256,
        "bytes":manifest.bytes,"parent":manifest.parent}),
    )
}

/// Read a specific immutable revision; never resolve a mutable latest pointer.
pub(crate) fn read(catalog: &Path, revision: &str) -> Result<(Vec<u8>, Value)> {
    if !store::valid_digest(revision) {
        return Err("revision must be a full SHA-256 digest".into());
    }
    store::no_links(catalog)?;
    let folder = catalog.join("revisions").join(revision);
    let encoded = crate::file_job::regular(&folder.join("revision.json"), LIMIT)?;
    if store::digest(&encoded) != revision {
        return Err("revision manifest digest mismatch".into());
    }
    let manifest: Revision = serde_json::from_slice(&encoded)?;
    if manifest.schema != "an-artifact-revision-v1"
        || manifest
            .parent
            .as_ref()
            .is_some_and(|p| !store::valid_digest(p))
    {
        return Err("invalid revision manifest".into());
    }
    let bytes = crate::file_job::regular(&folder.join("content.txt"), LIMIT)?;
    if bytes.len() != manifest.bytes || store::digest(&bytes) != manifest.content_sha256 {
        return Err("revision content identity mismatch".into());
    }
    std::str::from_utf8(&bytes)?;
    Ok((
        bytes,
        json!({"revision":revision,"content_sha256":manifest.content_sha256,
        "bytes":manifest.bytes,"parent":manifest.parent,"source":manifest.source,
        "trust":"untrusted data; identity is not permission or domain correctness"}),
    ))
}

/// Create a separate editable copy, never overwrite a revision or existing file.
pub fn checkout(catalog: &Path, revision: &str, output: &Path) -> Result<Value> {
    let (bytes, binding) = read(catalog, revision)?;
    store::no_links(output)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let target = parent
        .canonicalize()?
        .join(output.file_name().ok_or("missing output name")?);
    if target.starts_with(catalog.canonicalize()?) {
        return Err("editable copy must be outside catalog".into());
    }
    store::write_new(&target, &bytes)?;
    Ok(json!({"status":"succeeded","file":target,"binding":binding}))
}
