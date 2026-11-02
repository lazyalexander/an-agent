//! An enrolled attempt needs both its source release and the consumer's durable completion.
use super::*;
use std::collections::BTreeSet;

pub(super) type Confirm = fn(&Value) -> Result<()>;

pub(super) fn released(
    l: &Ledger,
    binding: &Value,
    confirm: Option<Confirm>,
) -> Result<BTreeMap<String, String>> {
    let dir = l
        .catalog
        .join("revisions")
        .join(&l.revision)
        .join("native-input-references");
    store::no_links(&dir)?;
    let mut expected = BTreeSet::new();
    let mut releases = BTreeMap::new();
    for (key, reader) in &l.readers {
        if reader.phase == "copied" && reader.release.is_some() {
            continue;
        }
        if !matches!(reader.phase.as_str(), "attempt" | "checkpoint") {
            return Err(
                "registered checkpoint or unreleased/uncertain reader blocks deletion".into(),
            );
        }
        let reference = &reader.plan;
        check_claim(l, reference)?;
        let id = hash(reference)?;
        if key != &format!("{}:{id}", reader.phase)
            || reference["catalog"] != json!(l.catalog)
            || reference["binding"] != *binding
        {
            return Err("registered attempt source identity mismatch".into());
        }
        let claim = references::mirror(reference)?;
        if reader.phase == "checkpoint" && claim["release"].is_null() {
            return Err(
                "registered checkpoint or unreleased/uncertain reader blocks deletion".into(),
            );
        }
        let approval = claim["release"]["approval_sha256"]
            .as_str()
            .ok_or("native attempt release is unconfirmed")?;
        if let Some(confirm) = confirm {
            confirm(&claim)?;
        }
        expected.insert(format!("{id}.json"));
        expected.insert(format!("{id}.released.json"));
        releases.insert(id, approval.into());
    }
    if dir.try_exists()? {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|n| expected.remove(n))
            {
                return Err("additional source reference evidence blocks deletion".into());
            }
        }
    }
    if !expected.is_empty() {
        return Err("registered source release evidence is missing".into());
    }
    Ok(releases)
}
