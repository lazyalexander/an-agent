//! Check historical release receipts for one retired output. No mutation or reader census.
use super::*;
use std::collections::BTreeSet;

pub fn review(path: &Path, name: &str, output: &str, consumers: &[String]) -> Result<Value> {
    if consumers.len() > 16 {
        return Err("retirement review supports at most 16 selected consumer roots".into());
    }
    let mut allowed = BTreeSet::new();
    for consumer in consumers {
        if !allowed.insert(location(Path::new(consumer))?) {
            return Err("duplicate review consumer".into());
        }
    }
    let observation = artifact_references(path, name, output)?;
    let catalog = Path::new(observation["catalog"].as_str().ok_or("catalog")?);
    let revision = observation["binding"]["revision"]
        .as_str()
        .ok_or("revision")?;
    let retirement = crate::artifact_versions::admission::status(catalog, revision)?;
    if retirement["mode"] != "retired" {
        return Err("review requires confirmed input retirement".into());
    }
    let claims = observation["claims"].as_array().ok_or("source claims")?;
    if claims.len() > 64 {
        return Err("retirement review exceeds 64 source claims".into());
    }
    let mut reviews = Vec::new();
    let mut confirmed = 0;
    for claim in claims {
        let reference = &claim["reference"];
        let owner = &reference["owner"];
        let mut item = json!({"owner":owner,"reference_sha256":digest(reference)?});
        let disposition = if reference["schema"] == "an-native-checkpoint-reference-v1" {
            "checkpoint-held"
        } else if claim["release"].is_null() {
            "attempt-held-or-unconfirmed"
        } else if let Some(root) = allowed.iter().find(|root| json!(root) == owner["root"]) {
            match confirm(root, claim) {
                Ok(hash) => {
                    confirmed += 1;
                    item["approval_sha256"] = json!(hash);
                    "release-confirmed"
                }
                Err(error) => {
                    item["diagnostic"] = json!(error.to_string());
                    "release-unconfirmed"
                }
            }
        } else {
            "consumer-not-selected"
        };
        item["disposition"] = json!(disposition);
        reviews.push(item);
    }
    Ok(
        json!({"schema":"an-native-retirement-review-v1","producer":observation["producer"],
        "revision":revision,"retirement_sha256":retirement["record"]["approval_sha256"],
        "registered_claims":claims.len(),"confirmed_releases":confirmed,"claims":reviews,
        "reference_census_complete":false,"collection_authorized":false,"atomic_snapshot":false,
        "collection_blocker":"unmanaged-and-legacy-readers-not-enumerated"}),
    )
}

fn digest(value: &Value) -> Result<String> {
    Ok(store::digest(&serde_json::to_vec(value)?))
}
fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&regular(path, 2 * 1024 * 1024)?)?)
}
fn confirm(root: &Path, claim: &Value) -> Result<String> {
    let reference = &claim["reference"];
    let run_name = reference["owner"]["run"].as_str().ok_or("attempt owner")?;
    let registration = input_references::registration(root, run_name)?;
    let slot = reference["slot"].as_u64().ok_or("reference slot")?;
    if registration["references"].get(usize::try_from(slot)?) != Some(reference) {
        return Err("consumer registration does not own this exact source claim".into());
    }
    let dir = root.join("runs").join(run_name);
    let intent = read(&dir.join("input-release-intent.json"))?;
    let hash = digest(&intent)?;
    let result = run::settled(root, run_name)?;
    let schema = match result["status"].as_str() {
        Some("succeeded") => "an-native-success-input-release-v1",
        Some("cancelled") => "an-native-input-release-v1",
        _ => return Err("unsupported release settlement".into()),
    };
    if intent["schema"] != schema
        || intent["registration"] != registration
        || intent["settlement_sha256"] != digest(&result)?
        || result["workspace"]["termination_confirmed"] != true
        || claim["release"]["approval_sha256"] != hash
        || read(&dir.join("input-released.json"))?
            != json!({"status":"succeeded","approval_sha256":hash,"payloads_deleted":false})
    {
        return Err("consumer release evidence mismatch".into());
    }
    Ok(hash)
}
