//! Environment citations use committed identity and verified blob bytes together.
use super::*;
use crate::testkit::TempDir;

fn assert_cite(space: &Workspace, record: &WorkspaceRecord, text: &str) {
    let cite = space.cite().unwrap();
    assert_eq!(cite.env_id.as_ref(), Some(&record.id));
    assert_eq!(cite.env_sha256, record.sha256);
    assert_eq!(cite.env_markdown, text);
    assert_eq!(Some(sha256(cite.env_markdown.as_bytes())), cite.env_sha256);
}

#[test]
fn uncommitted_live_bytes_never_join_an_old_reference_and_restart_repairs_projection() {
    let tmp = TempDir::new("env-uncommitted-live");
    let root = tmp.path().join("workspace");
    let space = Workspace::open(&root).unwrap();
    let first = space.put_env("stage", "committed A").unwrap();
    // Disk state from the former projection-before-log crash window.
    fs::write(
        root.join("env").join(sha256(b"uncommitted B")),
        b"uncommitted B",
    )
    .unwrap();
    fs::write(root.join("env.md"), b"uncommitted B").unwrap();
    fs::write(root.join("env/.pending-abandoned"), b"partial blob").unwrap();
    fs::write(root.join(".env-abandoned.tmp"), b"partial projection").unwrap();
    assert_cite(&space, &first, "committed A");
    drop(space);
    let reopened = Workspace::open(&root).unwrap();
    assert_cite(&reopened, &first, "committed A");
    assert_eq!(fs::read(root.join("env.md")).unwrap(), b"committed A");
    assert_eq!(reopened.log().unwrap().len(), 1);
}

#[test]
fn missing_corrupt_and_unreadable_committed_blobs_refuse_without_live_fallback() {
    for fault in ["missing", "corrupt", "unreadable"] {
        let tmp = TempDir::new("env-invalid-blob");
        let root = tmp.path().join("workspace");
        let space = Workspace::open(&root).unwrap();
        let record = space.put_env("stage", "committed A").unwrap();
        let sha = record.sha256.as_deref().unwrap();
        let path = root.join("env").join(sha);
        match fault {
            "missing" => fs::remove_file(&path).unwrap(),
            "corrupt" => fs::write(&path, b"wrong").unwrap(),
            _ => {
                fs::remove_file(&path).unwrap();
                fs::create_dir(&path).unwrap();
            }
        }
        assert!(space.cite().is_err(), "{fault}");
        assert!(space.env_bytes(sha).is_err(), "{fault}");
        assert!(space.put_env("stage", "committed A").is_err(), "{fault}");
        drop(space);
        assert!(Workspace::open(&root).is_err(), "{fault}");
        assert_eq!(fs::read(root.join("env.md")).unwrap(), b"committed A");
    }
}

#[test]
fn projection_loss_or_mutation_does_not_change_citations_and_reopen_rebuilds_it() {
    for remove in [false, true] {
        let tmp = TempDir::new("env-projection-recovery");
        let root = tmp.path().join("workspace");
        let space = Workspace::open(&root).unwrap();
        let record = space.put_env("stage", "A").unwrap();
        if remove {
            fs::remove_file(root.join("env.md")).unwrap();
        } else {
            fs::write(root.join("env.md"), b"uncommitted edit").unwrap();
        }
        assert_cite(&space, &record, "A");
        drop(space);
        let space = Workspace::open(&root).unwrap();
        assert_cite(&space, &record, "A");
        assert_eq!(fs::read(root.join("env.md")).unwrap(), b"A");
    }
}

#[test]
fn orphan_projection_has_no_generation_and_missing_log_is_not_recreated() {
    let tmp = TempDir::new("env-no-generation");
    let root = tmp.path().join("workspace");
    let space = Workspace::open(&root).unwrap();
    fs::write(root.join("env.md"), b"orphan").unwrap();
    let cite = space.cite().unwrap();
    assert!(cite.env_id.is_none());
    assert!(cite.env_sha256.is_none());
    assert!(cite.env_markdown.is_empty());
    space.put_env("stage", "A").unwrap();
    fs::remove_file(root.join("log.jsonl")).unwrap();
    assert!(space.cite().is_err());
    drop(space);
    assert!(Workspace::open(&root).is_err());
    assert!(!root.join("log.jsonl").exists());
}

#[test]
fn io_failure_windows_keep_references_consistent_and_reopen_recovers_projection() {
    for stage in [
        "blob_write",
        "blob_sync",
        "blob_publish",
        "blob_directory_sync",
        "log_open",
        "log_write",
        "log_sync",
        "projection_write",
        "projection_sync",
        "projection_rename",
        "projection_directory_sync",
    ] {
        let tmp = TempDir::new("env-io-failure");
        let root = tmp.path().join("workspace");
        let space = Workspace::open(&root).unwrap();
        let a = space.put_env("stage", "A").unwrap();
        *space.fail_env_at.lock().unwrap() = Some(stage);
        let error = space.put_env("stage", "B").unwrap_err();
        assert!(
            space.fail_env_at.lock().unwrap().is_none(),
            "unused fault: {stage}"
        );
        // A failed sync has an uncertain commit outcome. Here the full log write
        // survived; recovery may use B only with its verified blob, never A's ID.
        let retained = stage == "log_sync" || stage.starts_with("projection_");
        let expected = if retained { "B" } else { "A" };
        let latest = space.log().unwrap().into_iter().last().unwrap();
        assert_cite(&space, &latest, expected);
        assert_eq!(space.env_bytes(a.sha256.as_deref().unwrap()).unwrap(), b"A");
        if stage.starts_with("projection_") {
            assert!(
                matches!(error, WorkspaceError::EnvProjection { generation: Some(ref id), .. } if id == &latest.id)
            );
        }
        drop(space);
        let reopened = Workspace::open(&root).unwrap();
        assert_cite(&reopened, &latest, expected);
        assert_eq!(fs::read(root.join("env.md")).unwrap(), expected.as_bytes());
        assert_eq!(reopened.log().unwrap().len(), if retained { 2 } else { 1 });
        let b = reopened.put_env("stage", "B").unwrap();
        assert_cite(&reopened, &b, "B");
        assert_eq!(
            reopened.log().unwrap().len(),
            2,
            "duplicate generation: {stage}"
        );
    }
}

#[test]
fn invalid_committed_metadata_utf8_and_torn_logs_fail_without_rewriting_history() {
    for fault in [
        "missing-digest",
        "invalid-digest",
        "non-utf8",
        "torn-json",
        "no-newline",
    ] {
        let tmp = TempDir::new("env-invalid-ledger");
        let root = tmp.path().join("workspace");
        let space = Workspace::open(&root).unwrap();
        let mut record = space.put_env("stage", "A").unwrap();
        let bytes = match fault {
            "torn-json" => b"{\"id\":\"incomplete".to_vec(),
            "no-newline" => serde_json::to_vec(&record).unwrap(),
            _ => {
                record.sha256 = match fault {
                    "missing-digest" => None,
                    "invalid-digest" => Some("../env.md".into()),
                    _ => {
                        let sha = sha256(&[0xff]);
                        fs::write(root.join("env").join(&sha), [0xff]).unwrap();
                        Some(sha)
                    }
                };
                let mut bytes = serde_json::to_vec(&record).unwrap();
                bytes.push(b'\n');
                bytes
            }
        };
        fs::write(root.join("log.jsonl"), &bytes).unwrap();
        assert!(space.cite().is_err(), "{fault}");
        drop(space);
        assert!(Workspace::open(&root).is_err(), "{fault}");
        assert_eq!(fs::read(root.join("log.jsonl")).unwrap(), bytes);
        assert_eq!(fs::read(root.join("env.md")).unwrap(), b"A");
    }
}

#[test]
fn duplicate_named_write_repairs_current_projection_without_rewinding_it() {
    let tmp = TempDir::new("env-named-generation");
    let space = Workspace::open(tmp.path().join("workspace")).unwrap();
    let first = space.put_env("one", "A").unwrap();
    let current = space.put_env("two", "B").unwrap();
    fs::write(space.root.join("env.md"), b"stale").unwrap();
    assert_eq!(space.put_env("one", "A").unwrap(), first);
    assert_cite(&space, &current, "B");
    assert_eq!(fs::read(space.root.join("env.md")).unwrap(), b"B");
    assert_eq!(space.log().unwrap().len(), 2);
}

#[test]
fn uncertain_append_retry_requires_sync_without_duplicate_generation() {
    let tmp = TempDir::new("env-uncertain-sync");
    let space = Workspace::open(tmp.path().join("workspace")).unwrap();
    space.put_env("stage", "A").unwrap();
    *space.fail_env_at.lock().unwrap() = Some("log_sync");
    assert!(space.put_env("stage", "B").is_err());
    let retained = space.log().unwrap().pop().unwrap();
    *space.fail_env_at.lock().unwrap() = Some("log_sync");
    assert!(space.put_env("stage", "B").is_err());
    assert!(space.fail_env_at.lock().unwrap().is_none());
    assert_eq!(space.put_env("stage", "B").unwrap(), retained);
    assert_eq!(space.log().unwrap().len(), 2);
    assert_cite(&space, &retained, "B");
}

#[test]
fn a_bad_existing_blob_is_never_overwritten_or_given_a_generation() {
    let tmp = TempDir::new("env-bad-orphan");
    let space = Workspace::open(tmp.path().join("workspace")).unwrap();
    let first = space.put_env("stage", "A").unwrap();
    let path = space.root.join("env").join(sha256(b"B"));
    fs::write(&path, b"bad orphan").unwrap();
    assert!(space.put_env("stage", "B").is_err());
    assert_eq!(fs::read(path).unwrap(), b"bad orphan");
    assert_eq!(space.log().unwrap().len(), 1);
    assert_cite(&space, &first, "A");
}
