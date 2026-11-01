//! Bounded source-side observations, never a complete census or collection grant.
use super::*;
use std::collections::BTreeSet;

pub(crate) fn observe(catalog: &Path, revision: &str) -> Result<Value> {
    store::no_links(catalog)?;
    let catalog = catalog.canonicalize()?;
    let binding = super::super::registered::verify(&catalog, revision)?;
    let directory = catalog
        .join("revisions")
        .join(revision)
        .join("native-input-references");
    store::no_links(&directory)?;
    let mut entries = BTreeSet::new();
    if directory.try_exists()? {
        for entry in fs::read_dir(&directory)?.take(1025) {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                return Err("reference observation encountered a non-regular entry".into());
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "reference filename")?;
            let id = name
                .strip_suffix(".released.json")
                .or_else(|| name.strip_suffix(".json"))
                .ok_or("reference filename")?;
            if !store::valid_digest(id) || !entries.insert(name) || entries.len() > 1024 {
                return Err(
                    "reference observation inventory is invalid or exceeds its bound".into(),
                );
            }
        }
    }
    let mut claims = Vec::new();
    let mut observed = BTreeSet::new();
    let mut bytes = 0;
    for name in entries
        .iter()
        .filter(|name| !name.ends_with(".released.json"))
    {
        let path = directory.join(name);
        let reference: Value = serde_json::from_slice(&crate::file_job::regular(&path, LIMIT)?)?;
        let (expected, released) = paths(&reference)?;
        if expected != path
            || reference["catalog"] != json!(catalog)
            || reference["binding"] != binding
        {
            return Err("source reference filename, catalog or binding mismatch".into());
        }
        let mut claim = inspect(&reference)?;
        observed.insert(name.clone());
        if !claim["release"].is_null() {
            let release_name = released
                .file_name()
                .and_then(|v| v.to_str())
                .ok_or("release filename")?;
            if !entries.contains(release_name) {
                return Err("reference observation changed during inspection".into());
            }
            observed.insert(release_name.to_owned());
        }
        // A source marker does not confirm the consumer's final release receipt.
        claim["observation"] = json!(if claim["release"].is_null() {
            "claim_without_source_release"
        } else {
            "source_release_recorded"
        });
        bytes += serde_json::to_vec(&claim)?.len();
        if bytes > 8 * 1024 * 1024 {
            return Err("reference observation exceeds its byte bound".into());
        }
        claims.push(claim);
    }
    if observed != entries {
        return Err("reference observation contains an orphan or changed release record".into());
    }
    let released = claims.iter().filter(|c| !c["release"].is_null()).count();
    Ok(
        json!({"schema":"an-native-artifact-references-v1","catalog":catalog,"binding":binding,
        "registered_claims":claims.len(),"source_release_records":released,"claims":claims,
        "observation_only":true,"atomic_snapshot":false,"reference_census_complete":false,
        "collection_authorized":false,"consumer_receipts_checked":false}),
    )
}
