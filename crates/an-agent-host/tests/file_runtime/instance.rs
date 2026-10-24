//! State continuity through separate CLI processes and real sandboxed runs.
use super::revisions::{cli, path};
use super::*;
use an_agent_host::{instance as api, retention};

fn initialized(fixture: &Fixture, state: &str) -> execution::Result<(String, String)> {
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let seed = fixture.root.join(format!("{id}.txt"));
    fs::write(&seed, state)?;
    let mut args = vec![
        "instance-init".into(),
        path(&fixture.root.join("store")),
        id.clone(),
        fixture.release.clone(),
        path(&seed),
    ];
    let preview = cli(&args)?;
    assert!(!fixture.root.join("store/instances").join(&id).exists());
    args.extend([
        "--approve".into(),
        preview["approval_sha256"]
            .as_str()
            .ok_or("missing approval")?
            .into(),
    ]);
    let result = cli(&args)?;
    Ok((
        id,
        result["revision"]
            .as_str()
            .ok_or("missing revision")?
            .into(),
    ))
}

fn execute(fixture: &Fixture, id: &str, revision: &str) -> execution::Result<(String, PathBuf)> {
    let result = cli(&[
        "instance-run".into(),
        path(&fixture.root.join("store")),
        id.into(),
        revision.into(),
        "--execute".into(),
    ])?;
    let run = Path::new(result["receipt"].as_str().ok_or("missing receipt")?)
        .parent()
        .ok_or("missing run")?
        .to_owned();
    let id = run
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("missing run identity")?
        .to_owned();
    Ok((id, run))
}

fn preview(fixture: &Fixture, id: &str, expected: &str, run: &str) -> execution::Result<String> {
    let result = api::commit(&fixture.root.join("store"), id, expected, run, None)?;
    Ok(result["approval_sha256"]
        .as_str()
        .ok_or("missing commit approval")?
        .into())
}

#[test]
fn separate_cli_runs_commit_conflict_and_continue_without_previous_work() -> execution::Result<()> {
    let fixture = Fixture::with_source(Some(include_str!("../fixtures/state_counter.rs")))?;
    let store = fixture.root.join("store");
    let (id, zero) = initialized(&fixture, "0")?;
    let (a, a_dir) = execute(&fixture, &id, &zero)?;
    let (b, b_dir) = execute(&fixture, &id, &zero)?;
    assert_ne!(a_dir, b_dir);
    let a_grant = preview(&fixture, &id, &zero, &a)?;
    let b_grant = preview(&fixture, &id, &zero, &b)?;
    assert_eq!(api::inspect(&store, &id, None)?["head"], zero);
    assert!(api::commit(&store, &id, &zero, &a, Some(&"0".repeat(64))).is_err());
    let result = cli(&[
        "instance-commit".into(),
        path(&store),
        id.clone(),
        zero.clone(),
        a.clone(),
        "--approve".into(),
        a_grant,
    ])?;
    let one = result["revision"].as_str().ok_or("missing revision")?;
    let current = cli(&["instance-show".into(), path(&store), id.clone()])?;
    assert_eq!(current["bundle"]["state"], "1");
    assert_eq!(current["bundle"]["product"], "count=1");
    assert_eq!(current["bundle"]["report"], "incremented");
    assert_eq!(current["binding"]["parent"], zero);
    let conflict = api::commit(&store, &id, &zero, &b, Some(&b_grant))?;
    assert_eq!(conflict["status"], "conflict");
    assert_eq!(conflict["actual"], one);
    assert_eq!(api::inspect(&store, &id, None)?, current);
    assert!(
        store
            .join("instances")
            .join(&id)
            .join("commits")
            .join(&b_grant)
            .join("conflict.json")
            .exists()
    );

    for (run_id, run) in [(&a, &a_dir), (&b, &b_dir)] {
        let preview = retention::reclaim(&store, run_id, None)?;
        retention::reclaim(&store, run_id, preview["approval_sha256"].as_str())?;
        assert!(!run.join("work").exists());
        assert!(run.join("artifact.txt").exists());
    }
    let (c, c_dir) = execute(&fixture, &id, one)?;
    assert_ne!(c_dir, a_dir);
    let receipt: Value = serde_json::from_slice(&fs::read(c_dir.join("receipt.json"))?)?;
    assert_eq!(receipt["input_revision"]["revision"], one);
    let grant = preview(&fixture, &id, one, &c)?;
    let two = api::commit(&store, &id, one, &c, Some(&grant))?;
    let final_view = api::inspect(&store, &id, None)?;
    assert_eq!(final_view["head"], two["revision"]);
    assert_eq!(final_view["bundle"]["state"], "2");
    assert_eq!(final_view["bundle"]["product"], "count=2");
    assert_eq!(
        api::inspect(&store, &id, Some(one))?["bundle"]["state"],
        "1"
    );
    assert_eq!(
        api::inspect(&store, &id, Some(&zero))?["bundle"]["state"],
        "0"
    );
    assert!(fs::read_to_string(c_dir.join("audit.json"))?.contains("applied"));
    Ok(())
}

#[test]
fn foreign_candidates_and_uncertain_commits_cannot_change_state() -> execution::Result<()> {
    let fixture = Fixture::with_source(Some(include_str!("../fixtures/state_counter.rs")))?;
    let store = fixture.root.join("store");
    let (id, zero) = initialized(&fixture, "0")?;
    let (other, foreign) = initialized(&fixture, "0")?;
    let (run, directory) = execute(&fixture, &id, &zero)?;
    assert!(api::commit(&store, &other, &foreign, &run, None).is_err());
    let marker = fs::read(directory.join("settled.json"))?;
    fs::remove_file(directory.join("settled.json"))?;
    assert!(api::commit(&store, &id, &zero, &run, None).is_err());
    fs::write(directory.join("settled.json"), marker)?;
    let grant = preview(&fixture, &id, &zero, &run)?;
    let journal = store
        .join("instances")
        .join(&id)
        .join("commits")
        .join(&grant);
    fs::create_dir_all(&journal)?;
    fs::write(journal.join("intent.json"), b"interrupted attempt")?;
    assert!(api::commit(&store, &id, &zero, &run, Some(&grant)).is_err());
    assert_eq!(api::inspect(&store, &id, None)?["head"], zero);
    assert_eq!(
        fs::read(journal.join("intent.json"))?,
        b"interrupted attempt"
    );
    let (bad_id, failed_state) = initialized(&fixture, "not-a-decimal")?;
    assert_ne!(failed_state, zero);
    // The host accepts opaque state; the domain kit, not the host, rejects it.
    let result = api::run(
        &store,
        &bad_id,
        &failed_state,
        true,
        &std::sync::atomic::AtomicBool::new(false),
    )?;
    assert_ne!(result["status"], "succeeded");
    assert_eq!(api::inspect(&store, &bad_id, None)?["head"], failed_state);
    assert!(api::inspect(&store, "../escape", None).is_err());
    Ok(())
}

#[test]
fn staged_revisions_and_invalid_bundles_are_not_committed_products() -> execution::Result<()> {
    let fixture = Fixture::with_source(Some(include_str!("../fixtures/state_counter.rs")))?;
    let store = fixture.root.join("store");
    let (id, zero) = initialized(&fixture, "0")?;
    let path = store.join("instances").join(&id);
    let staged_file = fixture.root.join("staged-bundle.json");
    let staged =
        br#"{"schema":"an-instance-bundle-v1","state":"9","product":"count=9","report":"orphan"}"#;
    fs::write(&staged_file, staged)?;
    let revision = an_agent_host::artifact_versions::edit(
        &path.join("catalog"),
        &zero,
        &staged_file,
        &super::revisions::digest(staged),
    )?;
    let orphan = revision["revision"]
        .as_str()
        .ok_or("missing orphan revision")?;
    assert!(api::inspect(&store, &id, Some(orphan)).is_err());
    assert!(
        api::run(
            &store,
            &id,
            orphan,
            true,
            &std::sync::atomic::AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(api::inspect(&store, &id, None)?["bundle"]["state"], "0");
    // Corrupting an immutable bundle also fails closed before launching a run.
    fs::write(
        path.join("catalog/revisions")
            .join(&zero)
            .join("content.txt"),
        b"tampered",
    )?;
    assert!(api::inspect(&store, &id, None).is_err());
    assert!(
        api::run(
            &store,
            &id,
            &zero,
            true,
            &std::sync::atomic::AtomicBool::new(false)
        )
        .is_err()
    );

    let source = include_str!("../fixtures/state_counter.rs")
        .replace("an-instance-bundle-v1", "an-instance-bundle-v9");
    let invalid = Fixture::with_source(Some(&source))?;
    let (id, zero) = initialized(&invalid, "0")?;
    let (run, _) = execute(&invalid, &id, &zero)?;
    assert!(api::commit(&invalid.root.join("store"), &id, &zero, &run, None).is_err());
    assert_eq!(
        api::inspect(&invalid.root.join("store"), &id, None)?["head"],
        zero
    );
    Ok(())
}
