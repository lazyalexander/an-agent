//! Product reclamation is distinct from delivery acknowledgement and work cleanup.
use super::*;
mod reconcile;

/// Reclaim only a delivery-policy product with complete consumption and no state roots.
/// Raw accepted output and audit evidence remain under their existing retention policy.
pub fn reclaim(
    root: &Path,
    publisher: &str,
    revision: &str,
    approved: Option<&str>,
) -> Result<Value> {
    let (root, _lock) = root_lock(root)?;
    let reference = ArtifactRef {
        instance: publisher.into(),
        revision: revision.into(),
    };
    let checkpoint = event(&root, &reference)?;
    if !matches!(checkpoint.manifest.retention, Retention::Delivery) {
        return Err(
            "product has keep retention; delivery success does not authorize deletion".into(),
        );
    }
    let mut consumers = Vec::new();
    for recipient in &checkpoint.manifest.recipients {
        let consumed = consumed_at(&root, recipient, &reference)?
            .ok_or("required recipient has not committed consumption")?;
        consumers.push(json!({"instance":recipient,"checkpoint":consumed}));
    }
    let instances = root.join("instances");
    store::no_links(&instances)?;
    let mut entries = fs::read_dir(&instances)?;
    for count in 0..=256 {
        let Some(entry) = entries.next() else {
            break;
        };
        if count == 256 {
            return Err("instance scan exceeds bounded profile".into());
        }
        let entry = entry?;
        let id = entry
            .file_name()
            .into_string()
            .map_err(|_| "invalid instance directory")?;
        let path = instance_path(&root, &id)?;
        release(&path, &id)?;
        for (_, binding) in history(&path)? {
            if metadata(&binding)?.is_some_and(|m| m.manifest.state_refs.contains(&reference)) {
                return Err("product is referenced by a retained checkpoint".into());
            }
        }
    }
    let directory = root.join("products/revisions").join(&checkpoint.product);
    if let Some(result) = reconcile::inspect(&directory, &root, &reference, &checkpoint.product)? {
        return Ok(result);
    }
    let bytes = product(&root, &checkpoint)?;
    let plan = json!({"schema":"an-product-reclaim-v1","store":root,"event":reference,
        "product":checkpoint.product,"sha256":store::digest(&bytes),"consumers":consumers});
    let (digest, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    // Intent survives partial cleanup. There is no implicit retry or resurrection.
    store::write_new(
        &directory.join("reclaim-intent.json"),
        &serde_json::to_vec(&plan)?,
    )?;
    if let Err(error) = fs::remove_file(directory.join("content.txt")) {
        return Err(format!("product reclaim interrupted: {error}; retained intent requires read-only reconciliation").into());
    }
    let result = json!({"status":"succeeded","product":checkpoint.product,"approval_sha256":digest,
        "preserved":"checkpoint state, revision manifest and execution evidence"});
    if let Err(error) = store::write_new(
        &directory.join("reclaimed.json"),
        &serde_json::to_vec(&result)?,
    ) {
        return Err(format!("product removed but receipt not confirmed: {error}; retained intent requires read-only reconciliation").into());
    }
    Ok(result)
}
