//! Real upstream/downstream kits: publication recovery, deduplication and retention roots.
use super::instance::{execute, initialized};
use super::revisions::{cli, digest, path};
use super::*;
use an_agent_host::instance::{self as instances, checkpoint as api};

fn manifest(
    f: &Fixture,
    recipients: &[&str],
    refs: Value,
    retention: &str,
) -> execution::Result<PathBuf> {
    let bytes = serde_json::to_vec(
        &json!({"schema":"an-checkpoint-manifest-v1","recipients":recipients,"state_refs":refs,"retention":retention}),
    )?;
    let file = f.root.join(format!("{}.json", digest(&bytes)));
    fs::write(&file, bytes)?;
    Ok(file)
}

fn grant(
    f: &Fixture,
    id: &str,
    expected: &str,
    run: &str,
    manifest: &Path,
) -> execution::Result<String> {
    Ok(
        api::commit(&f.root.join("store"), id, expected, run, manifest, None)?["approval_sha256"]
            .as_str()
            .ok_or("missing approval")?
            .into(),
    )
}

fn commit(
    f: &Fixture,
    id: &str,
    expected: &str,
    run: &str,
    manifest: &Path,
) -> execution::Result<String> {
    let approved = grant(f, id, expected, run, manifest)?;
    let result = cli(&[
        "checkpoint-commit".into(),
        path(&f.root.join("store")),
        id.into(),
        expected.into(),
        run.into(),
        path(manifest),
        "--approve".into(),
        approved,
    ])?;
    Ok(result["revision"]
        .as_str()
        .ok_or("missing checkpoint")?
        .into())
}

fn deliver(
    f: &Fixture,
    publisher: &str,
    revision: &str,
    recipient: &str,
) -> execution::Result<Value> {
    cli(&[
        "checkpoint-deliver".into(),
        path(&f.root.join("store")),
        publisher.into(),
        revision.into(),
        recipient.into(),
        "--execute".into(),
    ])
}

fn run_identity(result: &Value) -> execution::Result<(String, PathBuf)> {
    let directory = Path::new(result["receipt"].as_str().ok_or("missing receipt")?)
        .parent()
        .ok_or("missing run directory")?
        .to_owned();
    Ok((
        directory
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("missing run id")?
            .into(),
        directory,
    ))
}

fn ack_path(
    store: &Path,
    publisher: &str,
    revision: &str,
    recipient: &str,
) -> execution::Result<PathBuf> {
    let reference = api::ArtifactRef {
        instance: publisher.into(),
        revision: revision.into(),
    };
    Ok(store
        .join("deliveries")
        .join(digest(&serde_json::to_vec(&reference)?))
        .join(recipient)
        .join("ack.json"))
}

#[test]
fn interrupted_staging_never_publishes_an_event_and_old_state_can_continue() -> execution::Result<()>
{
    let f = Fixture::with_source(Some(include_str!("../fixtures/delivery_counter.rs")))?;
    let store = f.root.join("store");
    let (up, zero) = initialized(&f, "4")?;
    let (a, _) = initialized(&f, "10")?;
    let (b, _) = initialized(&f, "20")?;
    let config = manifest(&f, &[&a, &b], json!([]), "delivery")?;
    let (run, _) = execute(&f, &up, &zero)?;
    let approved = grant(&f, &up, &zero, &run, &config)?;
    let folder = store.join("instances").join(&up);
    // Fail between staging the immutable bundle and publishing its HEAD.
    fs::write(
        folder.join(format!("HEAD.{approved}.pending")),
        b"injected pointer preparation failure",
    )?;
    assert!(api::commit(&store, &up, &zero, &run, &config, Some(&approved)).is_err());
    assert_eq!(instances::inspect(&store, &up, None)?["head"], zero);
    let staged = fs::read_dir(folder.join("catalog/revisions"))?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<Vec<_>>>()?;
    let orphan = staged
        .iter()
        .find(|r| r.as_str() != zero)
        .ok_or("missing staged checkpoint")?;
    assert!(instances::inspect(&store, &up, Some(orphan)).is_err());
    assert!(deliver(&f, &up, orphan, &a).is_err());
    assert!(api::reclaim(&store, &up, orphan, None).is_err());
    assert!(api::commit(&store, &up, &zero, &run, &config, Some(&approved)).is_err());
    // Explicit new execution starts from the last committed state, not the orphan.
    let (new_run, _) = execute(&f, &up, &zero)?;
    let revision = commit(&f, &up, &zero, &new_run, &config)?;
    assert_ne!(&revision, orphan);
    assert_eq!(
        instances::inspect(&store, &up, None)?["bundle"]["state"],
        "5"
    );
    assert!(deliver(&f, &up, &revision, &a)?["receipt"].is_string());
    Ok(())
}

#[test]
fn two_consumers_recover_commit_and_lost_ack_without_reexecuting() -> execution::Result<()> {
    let f = Fixture::with_source(Some(include_str!("../fixtures/delivery_counter.rs")))?;
    let store = f.root.join("store");
    let (up, s0) = initialized(&f, "4")?;
    let (a, a0) = initialized(&f, "10")?;
    let (b, b0) = initialized(&f, "20")?;
    let (run, _) = execute(&f, &up, &s0)?;
    let publish = manifest(&f, &[&a, &b], json!([]), "delivery")?;
    let event = commit(&f, &up, &s0, &run, &publish)?;
    let upstream = instances::inspect(&store, &up, None)?;
    assert_eq!(upstream["bundle"]["state"], "5");
    assert_eq!(upstream["bundle"]["product"], "");
    assert!(api::reclaim(&store, &up, &event, None).is_err());
    assert!(deliver(&f, &up, &event, &up).is_err());
    let no_refs = manifest(&f, &[], json!([]), "keep")?;
    let (a_run, a_dir) = run_identity(&deliver(&f, &up, &event, &a)?)?;
    // No state commit yet: an unresolved attempt must not launch another kit.
    assert!(deliver(&f, &up, &event, &a).is_err());
    let seal = fs::read(a_dir.join("settled.json"))?;
    fs::remove_file(a_dir.join("settled.json"))?;
    assert!(grant(&f, &a, &a0, &a_run, &no_refs).is_err());
    assert_eq!(instances::inspect(&store, &a, None)?["head"], a0);
    fs::write(a_dir.join("settled.json"), seal)?;
    let approved = grant(&f, &a, &a0, &a_run, &no_refs)?;
    let journal = store
        .join("instances")
        .join(&a)
        .join("commits")
        .join(&approved);
    fs::create_dir_all(&journal)?;
    // Inject a result-write failure AFTER the atomic head update, without test hooks.
    fs::write(
        journal.join("result.json"),
        b"injected result publication failure",
    )?;
    assert!(
        cli(&[
            "checkpoint-commit".into(),
            path(&store),
            a.clone(),
            a0.clone(),
            a_run.clone(),
            path(&no_refs),
            "--approve".into(),
            approved
        ])
        .is_err()
    );
    assert_eq!(
        instances::inspect(&store, &a, None)?["bundle"]["state"],
        "15"
    );
    assert!(!ack_path(&store, &up, &event, &a)?.exists());
    let cleanup = an_agent_host::retention::reclaim(&store, &a_run, None)?;
    an_agent_host::retention::reclaim(&store, &a_run, cleanup["approval_sha256"].as_str())?;
    assert!(!a_dir.join("work").exists());
    let runs = fs::read_dir(store.join("runs"))?.count();
    let reconciled = deliver(&f, &up, &event, &a)?;
    assert_eq!(reconciled["disposition"], "already-consumed");
    assert_eq!(fs::read_dir(store.join("runs"))?.count(), runs);
    fs::remove_file(ack_path(&store, &up, &event, &a)?)?;
    assert_eq!(deliver(&f, &up, &event, &a)?["executed"], false);
    assert_eq!(fs::read_dir(store.join("runs"))?.count(), runs);

    let (b_run, _) = run_identity(&deliver(&f, &up, &event, &b)?)?;
    let b1 = commit(&f, &b, &b0, &b_run, &no_refs)?;
    assert_eq!(
        instances::inspect(&store, &b, None)?["bundle"]["state"],
        "25"
    );
    // Consumption, not ack existence, is authoritative for retention.
    assert!(!ack_path(&store, &up, &event, &b)?.exists());
    let preview = api::reclaim(&store, &up, &event, None)?;
    api::reclaim(&store, &up, &event, preview["approval_sha256"].as_str())?;
    assert_eq!(
        deliver(&f, &up, &event, &b)?["disposition"],
        "already-consumed"
    );
    assert_eq!(instances::inspect(&store, &b, None)?["head"], b1);
    let product = upstream["binding"]["source"]["checkpoint"]["product"]
        .as_str()
        .ok_or("missing product")?;
    let artifact = store.join("products/revisions").join(product);
    assert!(!artifact.join("content.txt").exists());
    assert!(artifact.join("revision.json").exists());
    assert!(artifact.join("reclaimed.json").exists());
    // Removing the delivery product does not remove instance state or require its bytes.
    let (next, _) = execute(&f, &up, &event)?;
    commit(&f, &up, &event, &next, &no_refs)?;
    assert_eq!(
        instances::inspect(&store, &up, None)?["bundle"]["state"],
        "6"
    );
    Ok(())
}

#[test]
fn manifest_authority_and_ack_files_cannot_override_commit_evidence() -> execution::Result<()> {
    let f = Fixture::with_source(Some(include_str!("../fixtures/delivery_counter.rs")))?;
    let store = f.root.join("store");
    let (up, u0) = initialized(&f, "4")?;
    let (a, a0) = initialized(&f, "10")?;
    let (b, b0) = initialized(&f, "20")?;
    let (run, _) = execute(&f, &up, &u0)?;
    let duplicate = manifest(&f, &[&a, &a], json!([]), "delivery")?;
    assert!(grant(&f, &up, &u0, &run, &duplicate).is_err());
    let original = manifest(&f, &[&a], json!([]), "delivery")?;
    let approved = grant(&f, &up, &u0, &run, &original)?;
    let changed = manifest(&f, &[&a, &b], json!([]), "delivery")?;
    assert!(api::commit(&store, &up, &u0, &run, &changed, Some(&approved)).is_err());
    assert_eq!(instances::inspect(&store, &up, None)?["head"], u0);
    let event = commit(&f, &up, &u0, &run, &original)?;
    let ack = ack_path(&store, &up, &event, &a)?;
    fs::create_dir_all(ack.parent().ok_or("missing ack parent")?)?;
    fs::write(&ack, b"{\"status\":\"consumed\"}")?;
    assert!(
        api::reclaim(&store, &up, &event, None)
            .unwrap_err()
            .to_string()
            .contains("has not committed")
    );
    fs::remove_file(&ack)?;
    let (candidate, _) = run_identity(&deliver(&f, &up, &event, &a)?)?;
    let empty = manifest(&f, &[], json!([]), "keep")?;
    assert!(grant(&f, &b, &b0, &candidate, &empty).is_err());
    let missing = manifest(
        &f,
        &[],
        json!([{"instance":up,"revision":"0".repeat(64)}]),
        "keep",
    )?;
    assert!(grant(&f, &a, &a0, &candidate, &missing).is_err());
    commit(&f, &a, &a0, &candidate, &empty)?;
    let preview = api::reclaim(&store, &up, &event, None)?;
    assert!(api::reclaim(&store, &up, &event, Some(&"0".repeat(64))).is_err());
    // A new recovery root between preview and approval invalidates reclamation.
    let pinned = manifest(&f, &[], json!([{"instance":up,"revision":event}]), "keep")?;
    let (run, _) = execute(&f, &b, &b0)?;
    commit(&f, &b, &b0, &run, &pinned)?;
    assert!(
        api::reclaim(&store, &up, &event, preview["approval_sha256"].as_str())
            .unwrap_err()
            .to_string()
            .contains("retained checkpoint")
    );
    Ok(())
}

#[test]
fn retained_state_references_block_reclamation_even_after_both_acknowledgements()
-> execution::Result<()> {
    let f = Fixture::with_source(Some(include_str!("../fixtures/delivery_counter.rs")))?;
    let store = f.root.join("store");
    let (up, u0) = initialized(&f, "4")?;
    let (a, a0) = initialized(&f, "10")?;
    let (b, b0) = initialized(&f, "20")?;
    let publish = manifest(&f, &[&a, &b], json!([]), "delivery")?;
    let (run, _) = execute(&f, &up, &u0)?;
    let event = commit(&f, &up, &u0, &run, &publish)?;
    let reference = json!({"instance":up,"revision":event});
    let pinned = manifest(&f, &[], json!([reference]), "keep")?;
    let unpinned = manifest(&f, &[], json!([]), "keep")?;
    let (run, _) = run_identity(&deliver(&f, &up, &event, &a)?)?;
    let a1 = commit(&f, &a, &a0, &run, &pinned)?;
    let (run, _) = run_identity(&deliver(&f, &up, &event, &b)?)?;
    commit(&f, &b, &b0, &run, &unpinned)?;
    deliver(&f, &up, &event, &a)?;
    deliver(&f, &up, &event, &b)?;
    let error = api::reclaim(&store, &up, &event, None).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("referenced by a retained checkpoint")
    );
    let (run, _) = execute(&f, &a, &a1)?;
    assert!(instances::commit(&store, &a, &a1, &run, None).is_err());
    commit(&f, &a, &a1, &run, &unpinned)?;
    // Clearing current references does not silently discard historical recovery roots.
    assert!(
        api::reclaim(&store, &up, &event, None)
            .unwrap_err()
            .to_string()
            .contains("retained checkpoint")
    );
    assert_eq!(deliver(&f, &up, &event, &a)?["executed"], false);
    assert!(
        api::reclaim(&store, &a, &a1, None)
            .unwrap_err()
            .to_string()
            .contains("keep retention")
    );
    Ok(())
}

#[test]
fn reclaim_receipt_failure_is_reconciled_without_redeleting_or_losing_state()
-> execution::Result<()> {
    let f = Fixture::with_source(Some(include_str!("../fixtures/delivery_counter.rs")))?;
    let store = f.root.join("store");
    let (up, initial) = initialized(&f, "4")?;
    let publish = manifest(&f, &[], json!([]), "delivery")?;
    let (run, _) = execute(&f, &up, &initial)?;
    let event = commit(&f, &up, &initial, &run, &publish)?;
    let preview = api::reclaim(&store, &up, &event, None)?;
    let product = preview["plan"]["product"]
        .as_str()
        .ok_or("missing product")?;
    let directory = store.join("products/revisions").join(product);
    // Deterministic result-create fault after the actual deletion, through the public path.
    fs::create_dir(directory.join("reclaimed.json"))?;
    assert!(api::reclaim(&store, &up, &event, preview["approval_sha256"].as_str()).is_err());
    assert!(!directory.join("content.txt").exists());
    let intent = fs::read(directory.join("reclaim-intent.json"))?;
    assert_eq!(
        api::reclaim(&store, &up, &event, None)?["status"],
        "uncertain"
    );
    fs::remove_dir(directory.join("reclaimed.json"))?;
    // Crash after delete, partial result write, and complete write before sync acknowledgement.
    for receipt in [
        None,
        Some(b"{".to_vec()),
        Some(serde_json::to_vec(&json!({
            "status":"succeeded","product":product,"approval_sha256":preview["approval_sha256"]
        }))?),
    ] {
        if let Some(bytes) = &receipt {
            fs::write(directory.join("reclaimed.json"), bytes)?;
        }
        let result = api::reclaim(&store, &up, &event, preview["approval_sha256"].as_str())?;
        assert_eq!(result["executed"], false);
        assert_eq!(
            result["status"],
            if receipt.as_ref().is_some_and(|b| b.len() > 1) {
                "succeeded"
            } else {
                "uncertain"
            }
        );
        assert_eq!(fs::read(directory.join("reclaim-intent.json"))?, intent);
        assert!(!directory.join("content.txt").exists());
    }
    assert_eq!(instances::inspect(&store, &up, None)?["head"], event);
    // Wrong retained identities cannot be interpreted as successful reclamation.
    let mut wrong: Value = serde_json::from_slice(&intent)?;
    wrong["sha256"] = json!("0".repeat(64));
    fs::write(
        directory.join("reclaim-intent.json"),
        serde_json::to_vec(&wrong)?,
    )?;
    assert!(api::reclaim(&store, &up, &event, None).is_err());
    Ok(())
}
