//! Host-owned immutable checkpoints of bounded file artifacts, not VM snapshots.
use crate::execution::{Result, store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub(crate) mod admission;
pub(crate) mod references;
pub(crate) mod registered;

const LIMIT: usize = 65536;
pub(crate) const BINARY_LIMIT: usize = 8 * 1024 * 1024;

fn storage(schema: &str) -> Result<(&'static str, usize, bool)> {
    match schema {
        "an-artifact-revision-v1" => Ok(("content.txt", LIMIT, true)),
        "an-artifact-revision-v2" => Ok(("content.bin", BINARY_LIMIT, false)),
        "an-registered-text-v1" => Ok((registered::FILE, LIMIT, true)),
        "an-registered-bytes-v1" => Ok((registered::FILE, BINARY_LIMIT, false)),
        _ => Err("unsupported artifact revision schema".into()),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    schema: String,
    content_sha256: String,
    bytes: usize,
    parent: Option<String>,
    source: Value,
}

/// Import a settled native file output under the source store lock. Evidence
/// integrity is not authorship or domain correctness; host storage stays private.
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
    let run = run.canonicalize()?;
    let runs = run.parent().ok_or("missing source runs directory")?;
    if runs.file_name().and_then(|s| s.to_str()) != Some("runs") {
        return Err("import requires a retained host STORE/runs/RUN directory".into());
    }
    let root = runs.parent().ok_or("missing source store")?;
    let id = run
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("missing run identity")?;
    // Share the source execution/retention lock, then use their evidence gate.
    // Unsupported backends cannot fall back to receipt-only publication.
    let _source_lock = store::lock(root)?;
    let accepted = crate::settled_run::accepted(root, id)?;
    let receipt_bytes = crate::file_job::regular(&run.join("receipt.json"), LIMIT)?;
    let receipt: Value = serde_json::from_slice(&receipt_bytes)?;
    let release = receipt["release"]
        .as_str()
        .filter(|s| store::valid_digest(s))
        .ok_or("missing source release identity")?;
    publish(
        catalog,
        &accepted.artifact,
        None,
        json!({"kind":"accepted-run",
        "run_id":id,"release":release,
        "receipt_sha256":store::digest(&receipt_bytes),
        "settlement":accepted.evidence["settlement"],"input_revision":{
            "revision":receipt["input_revision"]["revision"],
            "content_sha256":receipt["input_revision"]["content_sha256"]}}),
    )
}

/// Explicitly checkpoint an operator-saved file. The digest approves exact bytes;
/// it does not prove the editor saved a semantically consistent document.
pub fn edit(catalog: &Path, parent: &str, file: &Path, approved: &str) -> Result<Value> {
    let (_, binding) = admission::read_input(catalog, parent)?;
    let binary = binding["encoding"] == "bytes";
    if !store::valid_digest(approved) {
        return Err("invalid approved content digest".into());
    }
    let bytes = crate::file_job::regular(file, if binary { BINARY_LIMIT } else { LIMIT })?;
    if store::digest(&bytes) != approved {
        return Err("edited file changed since approval".into());
    }
    publish_format(
        catalog,
        &bytes,
        Some(parent.to_owned()),
        binary,
        json!({"kind":"operator-checkpoint","authorship":"operator-declared; not authenticated"}),
        false,
    )
}

pub(crate) fn publish(
    catalog: &Path,
    bytes: &[u8],
    parent: Option<String>,
    source: Value,
) -> Result<Value> {
    publish_format(catalog, bytes, parent, false, source, false)
}

/// Explicit byte-object acceptance. File format validation belongs to its consumer.
pub(crate) fn publish_bytes(catalog: &Path, bytes: &[u8], source: Value) -> Result<Value> {
    publish_format(catalog, bytes, None, true, source, false)
}

pub(crate) fn publish_registered(
    catalog: &Path,
    bytes: &[u8],
    binary: bool,
    source: Value,
) -> Result<Value> {
    publish_format(catalog, bytes, None, binary, source, true)
}

fn publish_format(
    catalog: &Path,
    bytes: &[u8],
    parent: Option<String>,
    binary: bool,
    source: Value,
    registered: bool,
) -> Result<Value> {
    let schema = match (registered, binary) {
        (true, false) => "an-registered-text-v1",
        (true, true) => "an-registered-bytes-v1",
        (false, false) => "an-artifact-revision-v1",
        (false, true) => "an-artifact-revision-v2",
    };
    let (content, limit, text) = storage(schema)?;
    if bytes.len() > limit {
        return Err("artifact content exceeds limit".into());
    }
    if text {
        std::str::from_utf8(bytes)?;
    }
    store::directory(catalog)?;
    let revisions = catalog.join("revisions");
    let manifest = Revision {
        schema: schema.into(),
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
    // Hash the immutable in-memory payload before taking the shared commit lock.
    // Keep copying inside: rejected contenders should not write full payloads.
    let _lock = store::lock(catalog)?;
    store::directory(&revisions)?;
    if target.exists() {
        if registered {
            return Err("registered publication already exists; inspect original evidence".into());
        }
        read(catalog, &revision)?;
    } else {
        let pending = revisions.join(format!(
            "{}.pending",
            an_agent_core::det_seam::Entropy::os().uuid_v4()
        ));
        store::directory(&pending)?;
        let mut staged = Pending(Some(pending.clone()));
        if registered {
            registered::create(&pending, catalog, &revision, bytes)?;
        } else {
            store::write_new(&pending.join(content), bytes)?;
        }
        store::write_new(&pending.join("revision.json"), &encoded)?;
        if registered {
            fs::File::open(&pending)?.sync_all()?;
        }
        fs::rename(&pending, &target)?;
        if registered {
            fs::File::open(&revisions)?.sync_all()?;
        }
        staged.0 = None;
    }
    let mut result = json!({"status":"succeeded","revision":revision,"content_sha256":manifest.content_sha256,
        "bytes":manifest.bytes,"parent":manifest.parent});
    if registered {
        result["reader_profile"] = json!(registered::PROFILE);
    }
    if binary {
        result["encoding"] = json!("bytes");
    }
    Ok(result)
}

// Only this invocation's exclusively created staging files may be discarded.
// Abrupt process death can leave unindexed .pending material for inspection.
struct Pending(Option<PathBuf>);
impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            for name in [
                "content.bin",
                "content.txt",
                "revision.json",
                registered::FILE,
            ] {
                let _ = fs::remove_file(path.join(name));
            }
            let _ = fs::remove_dir(path);
        }
    }
}

// Check the manifest bytes against the selected content address.
fn manifest(catalog: &Path, revision: &str) -> Result<Revision> {
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
    let (_, limit, _) = storage(&manifest.schema)?;
    if manifest.bytes > limit
        || !store::valid_digest(&manifest.content_sha256)
        || manifest
            .parent
            .as_ref()
            .is_some_and(|p| !store::valid_digest(p))
    {
        return Err("invalid revision manifest".into());
    }
    Ok(manifest)
}

fn describe(manifest: &Revision, revision: &str) -> Value {
    let mut binding = json!({"revision":revision,"content_sha256":manifest.content_sha256,
        "bytes":manifest.bytes,"parent":manifest.parent,"source":manifest.source,
        "trust":"untrusted data; identity is not permission or domain correctness"});
    if manifest.schema.starts_with("an-registered-") {
        binding["reader_profile"] = json!(registered::PROFILE);
    }
    if matches!(
        manifest.schema.as_str(),
        "an-artifact-revision-v2" | "an-registered-bytes-v1"
    ) {
        binding["encoding"] = json!("bytes");
    }
    binding
}

/// Verify immutable metadata only. This does not establish payload availability.
pub(crate) fn metadata(catalog: &Path, revision: &str) -> Result<Value> {
    Ok(describe(&manifest(catalog, revision)?, revision))
}

/// Read a specific immutable revision; never resolve a mutable latest pointer.
pub(crate) fn read(catalog: &Path, revision: &str) -> Result<(Vec<u8>, Value)> {
    let manifest = manifest(catalog, revision)?;
    let (content, limit, text) = storage(&manifest.schema)?;
    let folder = catalog.join("revisions").join(revision);
    if content == registered::FILE {
        return Err("registered payload requires a registered reader".into());
    }
    let bytes = crate::file_job::regular(&folder.join(content), limit)?;
    if bytes.len() != manifest.bytes || store::digest(&bytes) != manifest.content_sha256 {
        return Err("revision content identity mismatch".into());
    }
    if text {
        std::str::from_utf8(&bytes)?;
    }
    Ok((bytes, describe(&manifest, revision)))
}

/// Create a separate editable copy, never overwrite a revision or existing file.
pub fn checkout(catalog: &Path, revision: &str, output: &Path) -> Result<Value> {
    let (bytes, binding) = admission::read_input(catalog, revision)?;
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

#[cfg(test)]
mod tests;
