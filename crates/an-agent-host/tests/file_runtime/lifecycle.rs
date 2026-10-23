//! Host-authorized handoff and work reclamation through the real native sandbox.
use super::revisions::{cli, digest, path};
use super::*;
use an_agent_host::{artifact_versions, capability, retention};
use std::sync::atomic::AtomicBool;

fn id(run: &Path) -> execution::Result<&str> {
    run.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "missing fixture run identity".into())
}
fn approval(preview: &Value) -> execution::Result<&str> {
    preview["approval_sha256"]
        .as_str()
        .ok_or_else(|| "missing fixture approval".into())
}
fn request(revision: &str) -> String {
    format!(
        "capability:{}",
        json!({"schema":"an-capability-request-v1",
        "capability":"artifact.read","revision":revision})
    )
}

#[test]
fn cli_authorized_handoff_and_reclamation_preserve_products_and_evidence() -> execution::Result<()>
{
    let fixture = Fixture::new()?;
    let store = fixture.root.join("store");
    let catalog = fixture.root.join("catalog");
    let (_, producer) = fixture.run("alpha")?;
    let revision = artifact_versions::import(&catalog, &producer)?;
    let revision = revision["revision"].as_str().unwrap();
    // The catalog now exists. The requesting kit checks that direct directory
    // access, host secrets, networking and subprocess launches are denied.
    let (_, requester) = fixture.run(&request(revision))?;
    let args = vec![
        "capability-read".into(),
        path(&store),
        id(&requester)?.into(),
        path(&catalog),
        fixture.release.clone(),
    ];
    let count = fs::read_dir(store.join("runs"))?.count();
    let preview = cli(&args)?;
    assert_eq!(preview["executed"], false);
    assert_eq!(preview["plan"]["revision"], revision);
    assert!(!store.join("capabilities").exists());
    assert_eq!(fs::read_dir(store.join("runs"))?.count(), count);
    let mut denied = args.clone();
    denied.extend(["--approve".into(), "0".repeat(64)]);
    assert!(cli(&denied).is_err());
    assert!(!store.join("capabilities").exists());
    let mut approved = args;
    approved.extend(["--approve".into(), approval(&preview)?.into()]);
    let result = cli(&approved)?;
    assert_eq!(result["status"], "succeeded");
    assert_eq!(result["request_run"], id(&requester)?);
    let downstream = Path::new(result["continuation"]["receipt"].as_str().unwrap())
        .parent()
        .unwrap();
    assert_eq!(fs::read(downstream.join("input.txt"))?, b"ALPHA");
    assert_eq!(fs::read(downstream.join("artifact.txt"))?, b"ALPHA");
    let receipt: Value = serde_json::from_slice(&fs::read(downstream.join("receipt.json"))?)?;
    assert_eq!(receipt["input_revision"]["revision"], revision);
    assert!(fs::read_to_string(downstream.join("audit.json"))?.contains("applied"));
    let journal = store.join("capabilities").join(approval(&preview)?);
    assert!(journal.join("intent.json").is_file());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(journal.join("result.json"))?)?,
        result
    );
    assert!(cli(&approved).is_err());
    assert_eq!(fs::read_dir(store.join("runs"))?.count(), count + 1);

    for run in [&producer, &requester, downstream] {
        let preserved = [
            "artifact.txt",
            "input.txt",
            "receipt.json",
            "audit.json",
            "settled.json",
            "plan.json",
            "program",
        ]
        .map(|name| Ok((name, fs::read(run.join(name))?)))
        .into_iter()
        .collect::<std::io::Result<Vec<_>>>()?;
        let args = vec!["work-reclaim".into(), path(&store), id(run)?.into()];
        let preview = cli(&args)?;
        assert!(run.join("work/input.txt").exists());
        let mut wrong = args.clone();
        wrong.extend(["--approve".into(), "0".repeat(64)]);
        assert!(cli(&wrong).is_err());
        assert!(!run.join("reclaim-intent.json").exists());
        let mut approved = args;
        approved.extend(["--approve".into(), approval(&preview)?.into()]);
        assert_eq!(cli(&approved)?["status"], "succeeded");
        assert!(!run.join("work").exists());
        for (name, bytes) in preserved {
            assert_eq!(fs::read(run.join(name))?, bytes, "{name}");
        }
        assert!(cli(&approved).is_err());
        artifact_versions::import(&catalog, run)?;
    }
    artifact_versions::checkout(&catalog, revision, &fixture.root.join("still-editable.txt"))?;
    assert_eq!(fs::read(fixture.root.join("still-editable.txt"))?, b"ALPHA");
    assert_eq!(fs::read(fixture.root.join("secret"))?, b"private host data");
    Ok(())
}

#[test]
fn unknown_termination_mismatched_evidence_and_unsafe_targets_cannot_be_reclaimed()
-> execution::Result<()> {
    let fixture = Fixture::new()?;
    let store = fixture.root.join("store");
    let (_, run) = fixture.run("alpha")?;
    let original = fs::read(run.join("receipt.json"))?;
    let sealed = fs::read(run.join("settled.json"))?;
    // Reseal the synthetic observation so the test exercises the lifecycle gate,
    // not merely an accidentally stale receipt digest. The kit cannot do this.
    for (pointer, value) in [
        ("/operation/observation/termination_confirmed", json!(false)),
        ("/operation/observation/termination_confirmed", Value::Null),
        ("/process/reaped", json!(false)),
        ("/status", json!("timed_out")),
        (
            "/operation/observation/operation/operation_id",
            json!(uuid::Uuid::nil()),
        ),
        ("/operation/observation/status", json!("refused")),
        ("/backend", json!("openshell-file-kit-v1")),
    ] {
        let mut receipt: Value = serde_json::from_slice(&original)?;
        *receipt.pointer_mut(pointer).unwrap() = value;
        let bytes = serde_json::to_vec(&receipt)?;
        fs::write(run.join("receipt.json"), &bytes)?;
        let mut marker: Value = serde_json::from_slice(&sealed)?;
        marker["receipt_sha256"] = json!(digest(&bytes));
        fs::write(run.join("settled.json"), serde_json::to_vec(&marker)?)?;
        assert!(
            retention::reclaim(&store, id(&run)?, None).is_err(),
            "{pointer}"
        );
        assert_eq!(fs::read(run.join("work/output.txt"))?, b"ALPHA");
        assert!(!run.join("reclaim-intent.json").exists());
    }
    fs::write(run.join("receipt.json"), &original)?;
    fs::remove_file(run.join("settled.json"))?;
    assert!(retention::reclaim(&store, id(&run)?, None).is_err());
    fs::write(run.join("settled.json"), &sealed)?;
    let preview = retention::reclaim(&store, id(&run)?, None)?;
    fs::write(run.join("work/output.txt"), b"changed after approval")?;
    assert!(retention::reclaim(&store, id(&run)?, Some(approval(&preview)?)).is_err());
    fs::write(run.join("work/output.txt"), b"ALPHA")?;
    fs::write(run.join("work/unexpected"), b"must not delete")?;
    assert!(retention::reclaim(&store, id(&run)?, Some(approval(&preview)?)).is_err());
    fs::remove_file(run.join("work/unexpected"))?;
    fs::rename(run.join("work"), run.join("saved-work"))?;
    std::os::unix::fs::symlink(run.join("saved-work"), run.join("work"))?;
    assert!(retention::reclaim(&store, id(&run)?, Some(approval(&preview)?)).is_err());
    fs::remove_file(run.join("work"))?;
    fs::rename(run.join("saved-work"), run.join("work"))?;
    fs::hard_link(
        run.join("work/output.txt"),
        fixture.root.join("linked-output"),
    )?;
    assert!(retention::reclaim(&store, id(&run)?, Some(approval(&preview)?)).is_err());
    assert_eq!(fs::read(run.join("artifact.txt"))?, b"ALPHA");
    assert!(retention::reclaim(&store, "../escape", None).is_err());
    Ok(())
}

#[test]
fn unsupported_requests_cancellation_and_uncertain_submission_never_auto_execute()
-> execution::Result<()> {
    let fixture = Fixture::new()?;
    let store = fixture.root.join("store");
    let catalog = fixture.root.join("catalog");
    let (_, source) = fixture.run("alpha")?;
    let imported = artifact_versions::import(&catalog, &source)?;
    let revision = imported["revision"].as_str().unwrap();
    let cancelled = AtomicBool::new(false);
    for text in [
        request(revision).replace("artifact.read", "shell.execute"),
        request(&"0".repeat(64)),
        format!(
            "capability:{}",
            json!({"schema":"an-capability-request-v1", "capability":"artifact.read", "revision":revision,"path":"/private"})
        ),
    ] {
        let (_, run) = fixture.run(&text)?;
        assert!(
            capability::artifact_read(
                &store,
                id(&run)?,
                &catalog,
                &fixture.release,
                None,
                &cancelled
            )
            .is_err()
        );
        assert!(!store.join("capabilities").exists());
    }
    let (_, run) = fixture.run(&request(revision))?;
    let preview = capability::artifact_read(
        &store,
        id(&run)?,
        &catalog,
        &fixture.release,
        None,
        &cancelled,
    )?;
    assert!(
        capability::artifact_read(
            &store,
            id(&run)?,
            &catalog,
            &fixture.release,
            Some(approval(&preview)?),
            &AtomicBool::new(true)
        )
        .is_err()
    );
    assert!(!store.join("capabilities").exists());
    // A recorded attempt without a result simulates a host interruption.
    fs::create_dir(store.join("capabilities"))?;
    let journal = store.join("capabilities").join(approval(&preview)?);
    fs::create_dir(&journal)?;
    fs::write(
        journal.join("intent.json"),
        serde_json::to_vec(&preview["plan"])?,
    )?;
    let count = fs::read_dir(store.join("runs"))?.count();
    assert!(
        capability::artifact_read(
            &store,
            id(&run)?,
            &catalog,
            &fixture.release,
            Some(approval(&preview)?),
            &cancelled
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(store.join("runs"))?.count(), count);
    assert!(!journal.join("result.json").exists());
    Ok(())
}
