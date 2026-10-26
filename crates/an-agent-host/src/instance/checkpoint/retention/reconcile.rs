//! Read-only reconciliation: uncertain deletion is never retried implicitly.
use super::*;

pub(super) fn inspect(
    directory: &Path,
    root: &Path,
    reference: &ArtifactRef,
    product: &str,
) -> Result<Option<Value>> {
    let intent = directory.join("reclaim-intent.json");
    store::no_links(&intent)?;
    if !intent.try_exists()? {
        return Ok(None);
    }
    let plan: Value = serde_json::from_slice(&regular(&intent, 65536)?)?;
    let manifest_bytes = regular(&directory.join("revision.json"), 65536)?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)?;
    if store::digest(&manifest_bytes) != product
        || manifest["schema"] != "an-artifact-revision-v1"
        || plan["schema"] != "an-product-reclaim-v1"
        || plan["store"] != json!(root)
        || plan["event"] != json!(reference)
        || plan["product"] != product
        || plan["sha256"] != manifest["content_sha256"]
        || !plan["sha256"].as_str().is_some_and(store::valid_digest)
    {
        return Err("reclaim intent does not match committed product identity".into());
    }
    let digest = store::digest(&serde_json::to_vec(&plan)?);
    let content = directory.join("content.txt");
    store::no_links(&content)?;
    let present = content.try_exists()?;
    if present {
        let bytes = regular(&content, 65536)?;
        if json!(store::digest(&bytes)) != plan["sha256"] || json!(bytes.len()) != manifest["bytes"]
        {
            return Err("reclaim content does not match retained intent".into());
        }
    }
    let receipt = directory.join("reclaimed.json");
    store::no_links(&receipt)?;
    // A complete retained receipt may survive a reported sync error. Partial or
    // unreadable receipts are evidence of uncertainty, never permission to retry.
    let confirmed = receipt.try_exists()?
        && regular(&receipt, 65536)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|r| {
                r["status"] == "succeeded"
                    && r["product"] == product
                    && r["approval_sha256"] == digest
            });
    if confirmed && present {
        return Err("reclaim receipt contradicts retained content".into());
    }
    Ok(Some(
        json!({"status":if confirmed {"succeeded"} else {"uncertain"},
        "product":product,"approval_sha256":digest,"reconciled":true,
        "content_present":present,"executed":false,
        "next_action":if confirmed {"none"} else {"operator reconciliation required; no automatic deletion or retry"}}),
    ))
}
