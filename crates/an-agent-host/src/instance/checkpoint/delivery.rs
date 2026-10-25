//! Frozen delivery identities, exclusive attempts and reconstructible acknowledgements.
use super::*;
use std::path::PathBuf;

fn journal(root: &Path, reference: &ArtifactRef, recipient: &str) -> Result<PathBuf> {
    instance_path(root, recipient)?;
    let event = store::digest(&serde_json::to_vec(reference)?);
    let path = root.join("deliveries").join(event).join(recipient);
    store::no_links(&path)?;
    Ok(path)
}

fn create_journal(root: &Path, reference: &ArtifactRef, recipient: &str) -> Result<PathBuf> {
    let path = journal(root, reference, recipient)?;
    store::directory(&root.join("deliveries"))?;
    store::directory(path.parent().ok_or("missing delivery parent")?)?;
    store::directory(&path)?;
    Ok(path)
}

fn acknowledge(
    root: &Path,
    reference: &ArtifactRef,
    recipient: &str,
    revision: &str,
) -> Result<()> {
    let path = create_journal(root, reference, recipient)?.join("ack.json");
    let receipt = json!({"schema":"an-delivery-ack-v1","event":reference,
        "recipient":recipient,"checkpoint":revision});
    if path.exists() {
        let existing: Value = serde_json::from_slice(&regular(&path, 8192)?)?;
        if existing != receipt {
            return Err("acknowledgement conflicts with committed evidence".into());
        }
    } else {
        store::write_new(&path, &serde_json::to_vec(&receipt)?)?;
    }
    Ok(())
}

/// Explicit delivery or acknowledgement reconciliation; never retry an unresolved attempt.
pub fn deliver(
    root: &Path,
    publisher: &str,
    revision: &str,
    recipient: &str,
    execute: bool,
    cancelled: &AtomicBool,
) -> Result<Value> {
    let (root, package, input_revision) = {
        let (root, _lock) = root_lock(root)?;
        let reference = ArtifactRef {
            instance: publisher.into(),
            revision: revision.into(),
        };
        let checkpoint = event(&root, &reference)?;
        if !checkpoint
            .manifest
            .recipients
            .iter()
            .any(|r| r == recipient)
        {
            return Err("instance is not a frozen recipient of this event".into());
        }
        if let Some(consumed) = consumed_at(&root, recipient, &reference)? {
            if execute {
                acknowledge(&root, &reference, recipient, &consumed)?;
            }
            return Ok(
                json!({"status":"succeeded","disposition":"already-consumed",
                "checkpoint":consumed,"executed":false,"acknowledged":execute}),
            );
        }
        let path = instance_path(&root, recipient)?;
        let package = release(&path, recipient)?;
        let expected = head(&path)?;
        let (mut bundle, binding) = read_bundle(&path, &expected)?;
        validate_state(&root, &binding)?;
        bundle.product = String::from_utf8(product(&root, &checkpoint)?)?;
        let bytes = serde_json::to_vec(&bundle)?;
        if bytes.len() > 8192 {
            return Err("delivery input exceeds native input limit".into());
        }
        let source = json!({"kind":"checkpoint-delivery-input","event":reference,
            "recipient":recipient,"expected":expected});
        let intent = journal(&root, &reference, recipient)?.join("intent.json");
        if intent.exists() {
            return Err("delivery attempt unresolved; inspect and commit a settled candidate, never auto-retry".into());
        }
        if !execute {
            return Ok(
                json!({"plan":source,"input_sha256":store::digest(&bytes),"executed":false}),
            );
        }
        let input = artifact_versions::publish(
            &root.join("delivery-inputs"),
            &bytes,
            None,
            source.clone(),
        )?;
        let input_revision = input["revision"]
            .as_str()
            .ok_or("missing delivery input")?
            .to_owned();
        let journal = create_journal(&root, &reference, recipient)?;
        store::write_new(
            &journal.join("intent.json"),
            &serde_json::to_vec(&json!({
            "schema":"an-delivery-attempt-v1","input_revision":input_revision,"binding":source}))?,
        )?;
        (root, package, input_revision)
    };
    // The underlying executor owns process identity, cancellation and settlement.
    // The durable attempt fences the lock-release gap and uncertain launch results.
    execution::run_revision(
        &root,
        &package,
        &root.join("delivery-inputs"),
        &input_revision,
        true,
        cancelled,
    )
}

pub(super) fn verify_input(
    root: &Path,
    id: &str,
    expected: &str,
    input: &Value,
) -> Result<ArtifactRef> {
    let revision = input["revision"]
        .as_str()
        .ok_or("missing delivery input binding")?;
    let (_, stored) = artifact_versions::read(&root.join("delivery-inputs"), revision)?;
    if &stored != input
        || input["source"]["kind"] != "checkpoint-delivery-input"
        || input["source"]["recipient"] != id
        || input["source"]["expected"] != expected
    {
        return Err("delivery input does not bind this instance checkpoint".into());
    }
    let reference: ArtifactRef = serde_json::from_value(input["source"]["event"].clone())?;
    let checkpoint = event(root, &reference)?;
    if !checkpoint.manifest.recipients.iter().any(|r| r == id) {
        return Err("delivery recipient was not authorized by the published manifest".into());
    }
    product(root, &checkpoint)?;
    let intent: Value = serde_json::from_slice(&regular(
        &journal(root, &reference, id)?.join("intent.json"),
        8192,
    )?)?;
    if intent["input_revision"] != revision || intent["binding"] != input["source"] {
        return Err("delivery attempt binding mismatch".into());
    }
    Ok(reference)
}
