//! Private input bodies may expire after work collection; immutable evidence stays.
use super::*;
use std::collections::BTreeSet;

fn validate_inventory(inventory: &Value, execution: &Value, complete: bool) -> Result<()> {
    let mut expected = BTreeSet::new();
    for input in execution["inputs"].as_array().ok_or("snapshot inputs")? {
        let name = input["name"].as_str().ok_or("snapshot name")?;
        inventory::relative(name)?;
        let item = &inventory[name];
        if (complete || !item.is_null())
            && (item["kind"] != "file"
                || (complete && item["sha256"] != input["sha256"])
                || !item["sha256"].as_str().is_some_and(store::valid_digest)
                || item["bytes"].as_u64().is_none())
        {
            return Err("snapshot inventory differs from the recorded input".into());
        }
        expected.insert(name.to_owned());
        let mut parent = Path::new(name).parent();
        while let Some(path) = parent.filter(|p| !p.as_os_str().is_empty()) {
            let name = path.to_str().ok_or("snapshot parent")?;
            if (complete || !inventory[name].is_null()) && inventory[name]["kind"] != "directory" {
                return Err("snapshot parent inventory differs".into());
            }
            expected.insert(name.into());
            parent = path.parent();
        }
    }
    let actual: BTreeSet<_> = inventory
        .as_object()
        .ok_or("snapshot inventory")?
        .keys()
        .cloned()
        .collect();
    if (complete && actual != expected) || !actual.is_subset(&expected) {
        return Err("unexpected snapshot material; no collection authorized".into());
    }
    Ok(())
}

pub(crate) fn reclaim(path: &Path, id: &str, approved: Option<&str>) -> Result<Value> {
    let root = root(path)?;
    let _lock = store::lock(&root)?;
    let (dir, receipt) = archived_record(&root, id)?;
    let complete = receipt["schema"] == "an-workspace-receipt-v1";
    let plan: Value = serde_json::from_slice(&regular(&dir.join("plan.json"), 2 * 1024 * 1024)?)?;
    let execution = if complete {
        snapshot::mapping(&dir, &plan, &receipt)?
    } else {
        snapshot::launch(&plan, &dir)?
    };
    let target = dir.join("snapshot/inputs");
    store::no_links(&target)?;
    let receipt_sha = store::digest(&regular(&dir.join("receipt.json"), 2 * 1024 * 1024)?);
    let intent_path = dir.join("snapshot-reclaim-intent.json");
    if intent_path.try_exists()? {
        let intent: Value = serde_json::from_slice(&regular(&intent_path, 2 * 1024 * 1024)?)?;
        validate_inventory(&intent["inventory"], &execution, complete)?;
        if intent["schema"] != "an-workspace-snapshot-reclaim-v1"
            || intent["attempt"] != id
            || intent["target"] != json!(target)
            || intent["receipt_sha256"] != receipt_sha
            || intent["execution_sha256"] != receipt["execution_sha256"]
            || intent["snapshot_mapping_sha256"] != store::digest(&serde_json::to_vec(&execution)?)
        {
            return Err("snapshot collection intent binding mismatch".into());
        }
        let hash = store::digest(&serde_json::to_vec(&intent)?);
        let result_path = dir.join("snapshot-reclaimed.json");
        let confirmed = result_path.try_exists()?
            && regular(&result_path, 65536)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .is_some_and(|r| {
                    r["status"] == "succeeded" && r["attempt"] == id && r["approval_sha256"] == hash
                });
        store::no_links(&target)?;
        let present = target.try_exists()?;
        if confirmed && present {
            return Err("snapshot collection receipt contradicts retained inputs".into());
        }
        return Ok(
            json!({"status":if confirmed {"succeeded"} else {"uncertain"},
            "attempt":id,"executed":false,"reconciled":true,"inputs_present":present,
            "automatic_retries":0,"approval_sha256":hash}),
        );
    }
    if dir.join("snapshot-reclaimed.json").try_exists()? {
        return Err("snapshot collection receipt has no matching intent".into());
    }
    if !target.try_exists()? && !complete {
        return Ok(json!({"status":"succeeded","attempt":id,"executed":false,
            "disposition":"not-created","removed_logical_bytes":0}));
    }
    if complete {
        super::profile::verify_payloads(&execution)?;
    }
    // Registered state/artifact roots must remain readable independently of this copy.
    if let Some(inputs) = plan["artifact_inputs"].as_array() {
        for input in inputs {
            let (_, binding) = crate::artifact_versions::read(
                Path::new(input["catalog"].as_str().ok_or("input catalog")?),
                input["binding"]["revision"]
                    .as_str()
                    .ok_or("input revision")?,
            )?;
            if binding != input["binding"] {
                return Err("retained state/artifact input root changed".into());
            }
        }
    }
    let inventory = inventory::scan(&target)?;
    validate_inventory(&inventory, &execution, complete)?;
    let plan = json!({"schema":"an-workspace-snapshot-reclaim-v1","attempt":id,
        "target":target,"inventory":inventory,"receipt_sha256":receipt_sha,
        "execution_sha256":receipt["execution_sha256"],
        "snapshot_mapping_sha256":store::digest(&serde_json::to_vec(&execution)?),
        "preserve":"formal artifacts, checkpoints, input identities, code baseline and execution evidence"});
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    store::write_new(&intent_path, &serde_json::to_vec(&plan)?)?;
    inventory::remove(&target, &inventory)?;
    let removed: u64 = inventory
        .as_object()
        .ok_or("inventory")?
        .values()
        .filter_map(|v| v["bytes"].as_u64())
        .sum();
    let result = json!({"status":"succeeded","attempt":id,"approval_sha256":hash,
        "removed_logical_bytes":removed,"automatic_retries":0});
    store::write_new(
        &dir.join("snapshot-reclaimed.json"),
        &serde_json::to_vec(&result)?,
    )?;
    Ok(result)
}
