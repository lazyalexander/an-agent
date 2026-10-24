//! Explicit checkpoint manifests over the existing instance publication boundary.
mod delivery;
mod retention;
use super::*;
pub use delivery::deliver;
pub use retention::reclaim;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub instance: String,
    pub revision: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Retention {
    Keep,
    Delivery,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub recipients: Vec<String>,
    pub state_refs: Vec<ArtifactRef>,
    pub retention: Retention,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Checkpoint {
    product: String,
    manifest: Manifest,
    consumed: Vec<ArtifactRef>,
}

fn metadata(binding: &Value) -> Result<Option<Checkpoint>> {
    let value = &binding["source"]["checkpoint"];
    if value.is_null() {
        Ok(None)
    } else {
        Ok(Some(serde_json::from_value(value.clone())?))
    }
}

fn event(root: &Path, reference: &ArtifactRef) -> Result<Checkpoint> {
    let path = instance_path(root, &reference.instance)?;
    storage::committed(&path, &head(&path)?, &reference.revision)?;
    let (_, binding) = read_bundle(&path, &reference.revision)?;
    metadata(&binding)?.ok_or_else(|| "revision has no checkpoint product".into())
}

fn product(root: &Path, checkpoint: &Checkpoint) -> Result<Vec<u8>> {
    Ok(artifact_versions::read(&root.join("products"), &checkpoint.product)?.0)
}

pub(super) fn validate_state(root: &Path, binding: &Value) -> Result<()> {
    if let Some(checkpoint) = metadata(binding)? {
        for reference in &checkpoint.manifest.state_refs {
            product(root, &event(root, reference)?)?;
        }
    }
    Ok(())
}

/// Preview/approve the manifest together with the candidate and expected state.
pub fn commit(
    root: &Path,
    id: &str,
    expected: &str,
    run: &str,
    manifest: &Path,
    approved: Option<&str>,
) -> Result<Value> {
    let manifest: Manifest = serde_json::from_slice(&regular(manifest, 16384)?)?;
    super::commit_selected(root, id, expected, run, Some(&manifest), approved)
}

pub(super) fn prepare(
    root: &Path,
    id: &str,
    expected: &str,
    binding: &Value,
    input: &Value,
    manifest: Option<&Manifest>,
) -> Result<Option<Checkpoint>> {
    let previous = metadata(binding)?;
    let Some(manifest) = manifest else {
        if previous.is_some() || input != binding {
            return Err(
                "checkpoint instances require an explicit manifest; input binding mismatch".into(),
            );
        }
        return Ok(None);
    };
    if manifest.schema != "an-checkpoint-manifest-v1" {
        return Err("unsupported checkpoint manifest".into());
    }
    if manifest.recipients.len() > 32 || manifest.state_refs.len() > 32 {
        return Err("checkpoint manifest exceeds bounded profile".into());
    }
    let mut recipients = manifest.recipients.clone();
    recipients.sort();
    recipients.dedup();
    if recipients.len() != manifest.recipients.len() {
        return Err("duplicate recipients".into());
    }
    for recipient in &recipients {
        let path = instance_path(root, recipient)?;
        release(&path, recipient)?;
        read_bundle(&path, &head(&path)?)?;
    }
    for reference in &manifest.state_refs {
        product(root, &event(root, reference)?)?;
    }
    let mut consumed = previous.map(|p| p.consumed).unwrap_or_default();
    if input != binding {
        let reference = delivery::verify_input(root, id, expected, input)?;
        if consumed.contains(&reference) {
            return Err("event already consumed; reconcile acknowledgement instead".into());
        }
        consumed.push(reference);
    }
    if consumed.len() > 128 {
        return Err("consumption history exceeds bounded profile".into());
    }
    Ok(Some(Checkpoint {
        product: String::new(),
        manifest: manifest.clone(),
        consumed,
    }))
}

pub(super) fn stage(
    root: &Path,
    run: &str,
    bytes: &[u8],
    prepared: Option<Checkpoint>,
) -> Result<(Vec<u8>, Option<Checkpoint>)> {
    let Some(mut checkpoint) = prepared else {
        return Ok((bytes.to_vec(), None));
    };
    let mut bundle = Bundle::parse(bytes)?;
    let artifact = artifact_versions::publish(
        &root.join("products"),
        bundle.product.as_bytes(),
        None,
        json!({"kind":"checkpoint-product","run_id":run}),
    )?;
    checkpoint.product = artifact["revision"]
        .as_str()
        .ok_or("missing product revision")?
        .into();
    bundle.product.clear();
    Ok((serde_json::to_vec(&bundle)?, Some(checkpoint)))
}

fn history(path: &Path) -> Result<Vec<(String, Value)>> {
    let mut cursor = head(path)?;
    let mut values = Vec::new();
    for _ in 0..1024 {
        let (_, binding) = read_bundle(path, &cursor)?;
        let parent = binding["parent"].as_str().map(str::to_owned);
        values.push((cursor, binding));
        match parent {
            Some(parent) => cursor = parent,
            None => return Ok(values),
        }
    }
    Err("checkpoint history exceeds bounded profile".into())
}

fn consumed_at(root: &Path, recipient: &str, reference: &ArtifactRef) -> Result<Option<String>> {
    let path = instance_path(root, recipient)?;
    // The earliest retained checkpoint proves the consuming transition, not an ack file.
    for (revision, binding) in history(&path)?.into_iter().rev() {
        if metadata(&binding)?.is_some_and(|m| m.consumed.contains(reference)) {
            return Ok(Some(revision));
        }
    }
    Ok(None)
}
