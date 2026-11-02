//! Closing admission retains history. Releasing a declaration requires a separate exact approval.
use super::*;
use crate::artifact_versions::{references, registered};

fn hash(value: &Value) -> Result<String> {
    Ok(store::digest(&serde_json::to_vec(value)?))
}

pub(in crate::native_spool) fn lock(root: &Path, revision: &str) -> Result<store::Lock> {
    if !store::valid_digest(revision) {
        return Err("invalid checkpoint revision".into());
    }
    let dir = catalog(root).join("revisions").join(revision);
    store::no_links(&dir)?;
    store::lock(&dir)
}

fn owner(root: &Path, binding: &Value) -> Value {
    let source = &binding["source"];
    json!({"root":root,"instance":source["instance"],"release":source["release"],"checkpoint_revision":binding["revision"]})
}

pub fn retire(path: &Path, revision: &str, approved: Option<&str>) -> Result<Value> {
    let root = location(path)?;
    let binding = committed(&root, revision)?["binding"].clone();
    if binding["reader_profile"] != registered::PROFILE {
        // Legacy starts can reread their source through launch. Serialize closing with the instance.
        let _instance = store::lock(&root)?;
        let _revision = lock(&root, revision)?;
        return artifact_versions::admission::retire(
            &catalog(&root),
            revision,
            owner(&root, &binding),
            approved,
        );
    }
    let _revision = lock(&root, revision)?;
    registered::operate(
        &catalog(&root),
        &binding,
        owner(&root, &binding),
        registered::Operation::Close,
        super::super::retention_review::confirm_registered,
        approved,
    )
}

fn release_plan(declaration: &Value, drain: &Value, reservations: &Value) -> Result<Value> {
    let binding = &declaration["checkpoint"]["binding"];
    if drain["owner"] != declaration["owner"] || drain["binding"] != *binding {
        return Err("reader drain belongs to a different checkpoint".into());
    }
    let reserved = reservations
        .as_object()
        .ok_or("invalid supervisor reservation inventory")?;
    if reserved.len() > 64
        || reserved.iter().any(|(name, value)| {
            bundle::identifier(name).is_err()
                || !["request_sha256", "result_sha256"]
                    .iter()
                    .all(|key| value[key].as_str().is_some_and(store::valid_digest))
        })
    {
        return Err("invalid supervisor reservation identity".into());
    }
    let pins = declaration["references"]
        .as_array()
        .ok_or("declared references")?
        .iter()
        .map(hash)
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"schema":"an-native-checkpoint-pin-release-plan-v1","owner":declaration["owner"],
        "declaration_sha256":hash(declaration)?,"references":pins,"reader_evidence":drain,"verified_reservations":reservations,
        "payloads_deleted":false,"automatic_retries":0}),
    )
}

fn receipt(plan: &Value) -> Result<Value> {
    Ok(
        json!({"schema":"an-native-checkpoint-pin-release-receipt-v1","owner":plan["owner"],
        "approval_sha256":hash(plan)?,"declaration_sha256":plan["declaration_sha256"],
        "released_references":plan["references"],"payloads_deleted":false}),
    )
}

pub(super) fn outcome(root: &Path, revision: &str, declaration: &Value) -> Result<Option<Value>> {
    let dir = dependencies::directory(root, revision)?;
    let intent = dir.join("release-intent.json");
    let complete = dir.join("release-receipt.json");
    store::no_links(&intent)?;
    store::no_links(&complete)?;
    if !intent.try_exists()? {
        if complete.try_exists()? {
            return Err("checkpoint release receipt has no intent".into());
        }
        return Ok(None);
    }
    let plan = dependencies::read(&intent)?;
    let binding = &declaration["checkpoint"]["binding"];
    let drain = registered::drained(&catalog(root), binding, &declaration["owner"], None)?;
    if plan != release_plan(declaration, &drain, &plan["verified_reservations"])? {
        return Err("checkpoint release intent or admission mismatch".into());
    }
    if !complete.try_exists()? {
        return Ok(Some(
            json!({"status":"release-unconfirmed","approval_sha256":hash(&plan)?,
            "automatic_retries":0,"collection_authorized":false}),
        ));
    }
    let proof = dependencies::read(&complete)?;
    if proof != receipt(&plan)? {
        return Err("checkpoint release receipt mismatch".into());
    }
    Ok(Some(
        json!({"status":"released","receipt":proof,"collection_authorized":false}),
    ))
}

/// The source checks the owner's completion before first collecting a released pin.
pub(in crate::native_spool) fn confirm_pin(claim: &Value) -> Result<()> {
    let pin = &claim["reference"];
    let root = location(Path::new(
        pin["owner"]["root"].as_str().ok_or("checkpoint owner")?,
    ))?;
    let revision = pin["owner"]["checkpoint_revision"]
        .as_str()
        .ok_or("checkpoint revision")?;
    committed(&root, revision)?;
    let declaration = dependencies::declaration(&root, revision)?;
    let status =
        outcome(&root, revision, &declaration)?.ok_or("missing checkpoint release intent")?;
    if status["status"] != "released"
        || pin["owner"] != declaration["owner"]
        || !declaration["references"]
            .as_array()
            .ok_or("references")?
            .contains(pin)
        || status["receipt"]["approval_sha256"] != claim["release"]["approval_sha256"]
    {
        return Err("checkpoint pin release is unconfirmed".into());
    }
    Ok(())
}

pub fn release(path: &Path, revision: &str, approved: Option<&str>) -> Result<Value> {
    let root = location(path)?;
    let binding = committed(&root, revision)?["binding"].clone();
    let _revision = lock(&root, revision)?;
    if binding["reader_profile"] != registered::PROFILE {
        return Err(
            "historical checkpoint lacks a complete reader registry; retain its pins".into(),
        );
    }
    if !dependencies::directory(&root, revision)?.try_exists()? {
        return Err("checkpoint has no confirmed dependency declaration to release".into());
    }
    let declaration = dependencies::declaration(&root, revision)?;
    if outcome(&root, revision, &declaration)?.is_some() {
        return Err(
            "checkpoint release already submitted; inspect original evidence, never replay".into(),
        );
    }
    let drain = registered::drained(
        &catalog(&root),
        &binding,
        &owner(&root, &binding),
        Some(super::super::retention_review::confirm_registered),
    )?;
    let reservations = super::super::supervisor::checkpoint_reservations(&root, &binding)?;
    let plan = release_plan(&declaration, &drain, &reservations)?;
    let (digest, execute) = approval(&plan, approved)?;
    let pins = declaration["references"]
        .as_array()
        .ok_or("declared pins")?;
    for pin in pins {
        if !references::inspect(pin)?["release"].is_null() {
            return Err("unexpected prior checkpoint pin release".into());
        }
    }
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    let dir = dependencies::directory(&root, revision)?;
    write_json(&dir.join("release-intent.json"), &plan)?;
    fs::File::open(&dir)?.sync_all()?;
    for pin in pins {
        references::release_pin(pin, &digest)?;
    }
    let proof = receipt(&plan)?;
    write_json(&dir.join("release-receipt.json"), &proof)?;
    fs::File::open(&dir)?.sync_all()?;
    Ok(json!({"status":"succeeded","receipt":proof}))
}
