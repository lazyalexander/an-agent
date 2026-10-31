//! Native consumer claims are retention evidence, never a complete reader census.
use super::*;
mod observation;
pub(crate) use observation::observe;

fn paths(reference: &Value) -> Result<(PathBuf, PathBuf)> {
    if reference["schema"] != "an-native-input-reference-v1" {
        return Err("unsupported input reference".into());
    }
    let catalog = Path::new(reference["catalog"].as_str().ok_or("reference catalog")?);
    store::no_links(catalog)?;
    if json!(catalog.canonicalize()?) != reference["catalog"] {
        return Err("input reference catalog is not canonical".into());
    }
    let revision = reference["binding"]["revision"]
        .as_str()
        .ok_or("revision")?;
    if !store::valid_digest(revision) {
        return Err("invalid reference revision".into());
    }
    let id = store::digest(&serde_json::to_vec(reference)?);
    let directory = catalog
        .join("revisions")
        .join(revision)
        .join("native-input-references");
    store::no_links(&directory)?;
    Ok((
        directory.join(format!("{id}.json")),
        directory.join(format!("{id}.released.json")),
    ))
}

pub(crate) fn inspect(reference: &Value) -> Result<Value> {
    let (claim, released) = paths(reference)?;
    let actual: Value = serde_json::from_slice(&crate::file_job::regular(&claim, LIMIT)?)?;
    if actual != *reference {
        return Err("input reference ownership changed".into());
    }
    store::no_links(&released)?;
    let release = if released.try_exists()? {
        let value: Value = serde_json::from_slice(&crate::file_job::regular(&released, LIMIT)?)?;
        if value["schema"] != "an-native-input-reference-release-v1"
            || value["reference_sha256"] != store::digest(&serde_json::to_vec(reference)?)
            || !value["approval_sha256"]
                .as_str()
                .is_some_and(store::valid_digest)
        {
            return Err("input reference release is unconfirmed".into());
        }
        value
    } else {
        Value::Null
    };
    Ok(json!({"reference":reference,"release":release}))
}

pub(crate) fn acquire(reference: &Value) -> Result<()> {
    let (claim, _) = paths(reference)?;
    let catalog = Path::new(reference["catalog"].as_str().ok_or("catalog")?);
    let _lock = store::lock(catalog)?;
    let revision = reference["binding"]["revision"]
        .as_str()
        .ok_or("revision")?;
    let (_, binding) = admission::read_input(catalog, revision)?;
    if binding != reference["binding"] {
        return Err("input reference no longer binds selected bytes".into());
    }
    let directory = claim.parent().ok_or("reference directory")?;
    store::directory(directory)?;
    if fs::read_dir(directory)?.take(512).count() >= 512 {
        return Err("native input reference history is full".into());
    }
    // Even an identical existing claim is not permission to retry an uncertain start.
    store::write_new(&claim, &serde_json::to_vec(reference)?)?;
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}

pub(crate) fn release(reference: &Value, approval: &str) -> Result<()> {
    if !store::valid_digest(approval) {
        return Err("invalid release approval".into());
    }
    let (_, released) = paths(reference)?;
    let _lock = store::lock(Path::new(reference["catalog"].as_str().ok_or("catalog")?))?;
    if !inspect(reference)?["release"].is_null() {
        return Err("reference release already exists; inspect instead of retrying".into());
    }
    let record = json!({"schema":"an-native-input-reference-release-v1",
        "reference_sha256":store::digest(&serde_json::to_vec(reference)?),"approval_sha256":approval});
    store::write_new(&released, &serde_json::to_vec(&record)?)?;
    fs::File::open(released.parent().ok_or("reference directory")?)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests;
