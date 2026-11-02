//! One-way input admission closure; retained bytes remain readable as evidence.
use super::*;

fn marker(catalog: &Path, revision: &str) -> Result<PathBuf> {
    if !store::valid_digest(revision) {
        return Err("revision must be a full SHA-256 digest".into());
    }
    let path = catalog
        .join("revisions")
        .join(revision)
        .join("admission-retired.json");
    store::no_links(&path)?;
    Ok(path)
}

/// Immutable bytes and a monotonic closure fence allow concurrent readers.
pub(crate) fn read_input(catalog: &Path, revision: &str) -> Result<(Vec<u8>, Value)> {
    store::no_links(catalog)?;
    ensure_active(catalog, revision)?;
    let selected = read(catalog, revision)?;
    // A concurrent exclusive marker creation either precedes this final check
    // (refuse) or follows it (these bytes were acquired before closure).
    ensure_active(catalog, revision)?;
    Ok(selected)
}

/// Read-only observation; input acquisition checks both sides of its byte read.
pub(crate) fn ensure_active(catalog: &Path, revision: &str) -> Result<()> {
    if marker(catalog, revision)?.try_exists()? {
        // Even a torn/invalid record closes admission. Never repair it by overwriting.
        return Err(
            "artifact revision is retired or retirement is unconfirmed; no new input".into(),
        );
    }
    Ok(())
}

pub(crate) fn status(catalog: &Path, revision: &str) -> Result<Value> {
    let binding = super::metadata(catalog, revision)?;
    if binding["reader_profile"] == super::registered::PROFILE {
        return super::registered::admission_status(catalog, &binding);
    }
    let path = marker(catalog, revision)?;
    if !path.try_exists()? {
        return Ok(json!({"mode":"active"}));
    }
    let record: Value = serde_json::from_slice(&crate::file_job::regular(&path, LIMIT)?)?;
    let (_, binding) = read(catalog, revision)?;
    let plan = &record["plan"];
    if record["schema"] != "an-artifact-admission-retired-v1"
        || record["approval_sha256"] != store::digest(&serde_json::to_vec(plan)?)
        || plan["schema"] != "an-artifact-admission-retirement-v1"
        || plan["catalog"] != json!(catalog.canonicalize()?)
        || plan["artifact"] != binding
        || plan["payload_deleted"] != false
    {
        return Err(
            "artifact retirement evidence is incomplete or changed; admission stays closed".into(),
        );
    }
    Ok(json!({"mode":"retired","record":record,"payload_deleted":false}))
}

/// Only a host owner that has verified formal acceptance may supply authority.
pub(crate) fn retire(
    catalog: &Path,
    revision: &str,
    authority: Value,
    approved: Option<&str>,
) -> Result<Value> {
    store::no_links(catalog)?;
    let _lock = store::lock(catalog)?;
    let (_, binding) = read(catalog, revision)?;
    let plan = json!({"schema":"an-artifact-admission-retirement-v1",
        "catalog":catalog.canonicalize()?,"artifact":binding,"authority":authority,
        "payload_deleted":false,"already_acquired_inputs_cancelled":false,"automatic_retries":0});
    let hash = store::digest(&serde_json::to_vec(&plan)?);
    if approved.is_some_and(|a| a != hash) {
        return Err("artifact retirement approval mismatch".into());
    }
    let existing = status(catalog, revision)?;
    if existing["mode"] == "retired" {
        if existing["record"]["plan"] != plan {
            return Err("artifact already retired under another ownership binding".into());
        }
        return Ok(
            json!({"status":"succeeded","executed":false,"already_retired":true,
            "record":existing["record"],"payload_deleted":false}),
        );
    }
    if approved.is_none() {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    let record =
        json!({"schema":"an-artifact-admission-retired-v1","plan":plan,"approval_sha256":hash});
    let path = marker(catalog, revision)?;
    // Exclusive creation is the closure fence. Failed/partial writes remain closed.
    store::write_new(&path, &serde_json::to_vec_pretty(&record)?)?;
    fs::File::open(path.parent().ok_or("revision directory")?)?.sync_all()?;
    Ok(json!({"status":"succeeded","executed":true,"record":record,"payload_deleted":false}))
}

#[cfg(test)]
mod tests;
