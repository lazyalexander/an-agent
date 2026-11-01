//! Explicit opaque checkpoint pins; no inference, release or collection authority.
use super::*;
use crate::artifact_versions::references;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    artifacts: Vec<Source>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    instance: PathBuf,
    run: String,
    output: String,
    revision: String,
}
fn directory(root: &Path, revision: &str) -> Result<PathBuf> {
    if !store::valid_digest(revision) {
        return Err("checkpoint dependency requires an exact revision".into());
    }
    let dir = root.join("checkpoints/dependencies").join(revision);
    store::no_links(&dir)?;
    Ok(dir)
}
fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&regular(path, 2 * 1024 * 1024)?)?)
}

/// One immutable dependency declaration for one committed checkpoint revision.
pub fn declare(path: &Path, revision: &str, file: &Path, approved: Option<&str>) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (instance, _) = active(&root)?;
    let checkpoint = selection(&root, revision)?;
    let dir = directory(&root, revision)?;
    if dir.try_exists()? {
        return Err("checkpoint dependencies already reserved; inspect, never retry".into());
    }
    let request: Request = serde_json::from_slice(&regular(file, 65536)?)?;
    if request.schema != "an-native-checkpoint-dependencies-v1"
        || !(1..=16).contains(&request.artifacts.len())
    {
        return Err("checkpoint dependencies require 1..16 explicit accepted artifacts".into());
    }
    let owner = json!({"root":root,"instance":instance["instance"],
        "release":instance["release"],"checkpoint_revision":revision});
    let mut seen = BTreeSet::new();
    let mut pins = Vec::new();
    for source in request.artifacts {
        let source_root = location(&source.instance)?;
        let (report, _) = super::super::artifacts::verified(&source_root, &source.run, true)?;
        let output = report["outputs"]
            .as_array()
            .ok_or("source outputs")?
            .iter()
            .find(|value| value["path"] == source.output)
            .ok_or("dependency must select an accepted output")?;
        if output["binding"]["revision"] != source.revision {
            return Err("checkpoint dependency revision mismatch".into());
        }
        let catalog = report["catalog"].as_str().ok_or("source catalog")?;
        if !seen.insert((catalog.to_owned(), source.revision.clone())) {
            return Err("duplicate checkpoint dependency".into());
        }
        let binding =
            artifact_versions::registered::pinnable(Path::new(catalog), &source.revision)?;
        if binding["revision"] != output["binding"]["revision"]
            || binding["content_sha256"] != output["binding"]["content_sha256"]
            || binding["bytes"] != output["binding"]["bytes"]
        {
            return Err("checkpoint dependency binding mismatch".into());
        }
        pins.push(
            json!({"schema":"an-native-checkpoint-reference-v1","owner":owner,
            "catalog":catalog,"binding":binding}),
        );
    }
    let mut plan = json!({"schema":"an-native-checkpoint-dependencies-plan-v1",
        "owner":owner,"checkpoint":checkpoint,"references":pins,"automatic_retries":0});
    round_control::bind(&mut plan, &root, &instance)?;
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    store::directory(dir.parent().ok_or("dependency parent")?)?;
    fs::create_dir(&dir)?;
    fs::File::open(dir.parent().ok_or("dependency parent")?)?.sync_all()?;
    write_json(&dir.join("intent.json"), &plan)?;
    fs::File::open(&dir)?.sync_all()?;
    for pin in &pins {
        references::acquire(pin)?;
    }
    write_json(&dir.join("complete.json"), &json!({"approval_sha256":hash}))?;
    fs::File::open(&dir)?.sync_all()?;
    Ok(json!({"status":"succeeded","dependencies":inspect(&root, revision)?}))
}

pub(in super::super) fn inspect(root: &Path, revision: &str) -> Result<Value> {
    let dir = directory(root, revision)?;
    if !dir.try_exists()? {
        return Ok(
            json!({"status":"undeclared","reference_census_complete":false,
            "collection_authorized":false}),
        );
    }
    let plan = read(&dir.join("intent.json"))?;
    let (instance, _) = state(root)?;
    let (_, binding) = artifact_versions::read(&catalog(root), revision)?;
    let owner = json!({"root":root,"instance":instance["instance"],
        "release":instance["release"],"checkpoint_revision":revision});
    let hash = store::digest(&serde_json::to_vec(&plan)?);
    if plan["schema"] != "an-native-checkpoint-dependencies-plan-v1"
        || plan["owner"] != owner
        || plan["checkpoint"]["binding"] != binding
        || read(&dir.join("complete.json"))? != json!({"approval_sha256":hash})
    {
        return Err("checkpoint dependency registration is unconfirmed".into());
    }
    let pins = plan["references"]
        .as_array()
        .ok_or("checkpoint references")?;
    if !(1..=16).contains(&pins.len()) {
        return Err("checkpoint reference count invalid".into());
    }
    let mut claims = Vec::new();
    for pin in pins {
        if pin["schema"] != "an-native-checkpoint-reference-v1" || pin["owner"] != owner {
            return Err("checkpoint reference owner mismatch".into());
        }
        let claim = references::inspect(pin)?;
        if !claim["release"].is_null() {
            return Err("checkpoint reference release is unsupported".into());
        }
        claims.push(claim);
    }
    Ok(
        json!({"status":"registered","revision":revision,"approval_sha256":hash,
        "claims":claims,"reference_census_complete":false,"collection_authorized":false}),
    )
}
