//! Crash-window disk snapshots come from a real sandboxed successful run.
use super::revisions::{cli, digest, path};
use super::*;
use an_agent_host::artifact_versions;

fn refused(catalog: &Path, source: &Path) -> execution::Result<()> {
    assert!(artifact_versions::import(catalog, source).is_err());
    assert!(cli(&["artifact-import".into(), path(catalog), path(source)]).is_err());
    assert!(
        !catalog.exists(),
        "refusal must precede catalog publication"
    );
    Ok(())
}

#[test]
fn receipt_and_audit_without_settlement_cannot_publish() -> execution::Result<()> {
    let fixture = Fixture::new()?;
    let (_, run) = fixture.run("alpha")?;
    let catalog = fixture.root.join("catalog");
    let marker = fs::read(run.join("settled.json"))?;
    let audit = fs::read(run.join("audit.json"))?;
    fs::remove_file(run.join("settled.json"))?;
    fs::remove_file(run.join("audit.json"))?;
    // Receipt exists, but kernel applied/finish and audit have not committed.
    refused(&catalog, &run)?;
    fs::write(run.join("audit.json"), &audit)?;
    // Even a complete audit is not the final settlement commit point.
    refused(&catalog, &run)?;
    fs::write(run.join("settled.json"), &marker)?;
    let lock = fixture.root.join("store/host.lock");
    // Cooperating execution still owns the store: import must not bypass it.
    fs::write(&lock, b"held")?;
    refused(&catalog, &run)?;
    fs::remove_file(lock)?;
    let revision = artifact_versions::import(&catalog, &run)?;
    assert_eq!(artifact_versions::import(&catalog, &run)?, revision);
    let manifest: Value = serde_json::from_slice(&fs::read(
        catalog
            .join("revisions")
            .join(revision["revision"].as_str().ok_or("revision")?)
            .join("revision.json"),
    )?)?;
    assert_eq!(
        manifest["source"]["settlement"],
        serde_json::from_slice::<Value>(&marker)?
    );
    // Previously published manifests are evidence, not retroactively invalidated.
    let mut legacy = manifest.clone();
    legacy["source"]
        .as_object_mut()
        .unwrap()
        .remove("settlement");
    let bytes = serde_json::to_vec(&legacy)?;
    let legacy_id = digest(&bytes);
    let directory = catalog.join("revisions").join(&legacy_id);
    fs::create_dir(&directory)?;
    fs::write(directory.join("revision.json"), &bytes)?;
    fs::write(directory.join("content.txt"), b"ALPHA")?;
    let checkout = fixture.root.join("legacy-checkout.txt");
    artifact_versions::checkout(&catalog, &legacy_id, &checkout)?;
    assert_eq!(fs::read(checkout)?, b"ALPHA");
    assert_ne!(revision["revision"], legacy_id);
    assert_eq!(artifact_versions::import(&catalog, &run)?, revision);
    assert_eq!(fs::read(directory.join("revision.json"))?, bytes);
    assert_eq!(fs::read(run.join("artifact.txt"))?, b"ALPHA");
    Ok(())
}

#[test]
fn import_reuses_settlement_identity_and_termination_checks() -> execution::Result<()> {
    let fixture = Fixture::new()?;
    let (_, run) = fixture.run("alpha")?;
    let catalog = fixture.root.join("catalog");
    let names = [
        "receipt.json",
        "settled.json",
        "operation.json",
        "plan.json",
        "audit.json",
    ];
    let originals: Vec<_> = names
        .iter()
        .map(|name| fs::read(run.join(name)))
        .collect::<std::io::Result<_>>()?;
    let original: Value = serde_json::from_slice(&originals[0])?;
    for (pointer, value) in [
        ("/operation/observation/termination_confirmed", Value::Null),
        ("/operation/observation/termination_confirmed", json!(false)),
        ("/process/reaped", json!(false)),
        (
            "/operation/observation/operation/operation_id",
            json!(uuid::Uuid::nil()),
        ),
        ("/backend", json!("openshell-vm-file-v1")),
        ("/backend", json!("unknown-file-v1")),
    ] {
        let mut receipt = original.clone();
        *receipt.pointer_mut(pointer).ok_or("field")? = value;
        let bytes = serde_json::to_vec(&receipt)?;
        fs::write(run.join("receipt.json"), &bytes)?;
        // Reseal so refusal proves the semantic check, not only a stale hash.
        let mut marker: Value = serde_json::from_slice(&originals[1])?;
        marker["receipt_sha256"] = json!(digest(&bytes));
        fs::write(run.join("settled.json"), serde_json::to_vec(&marker)?)?;
        refused(&catalog, &run)?;
    }
    for (name, bytes) in names.iter().zip(&originals) {
        fs::write(run.join(name), bytes)?;
    }
    for name in names {
        let bytes = fs::read(run.join(name))?;
        fs::write(run.join(name), b"{}")?;
        refused(&catalog, &run)?;
        fs::write(run.join(name), bytes)?;
    }
    // A successful legacy receipt alone cannot confer publication authority.
    let mut legacy = original.clone();
    legacy.as_object_mut().unwrap().remove("operation");
    fs::write(run.join("receipt.json"), serde_json::to_vec(&legacy)?)?;
    fs::remove_file(run.join("settled.json"))?;
    refused(&catalog, &run)?;
    for (name, bytes) in names.iter().zip(&originals) {
        fs::write(run.join(name), bytes)?;
    }
    assert!(artifact_versions::import(&catalog, &run).is_ok());
    Ok(())
}
