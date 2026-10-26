//! Race ordering uses real immutable catalog writes, with no sleeps or retries.
use super::*;
use an_agent_core::testkit::TempDir;
use std::{
    fs,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

fn response() -> Value {
    json!({"status":"succeeded","execution_status":"succeeded", "run_id":"settled-run",
        "receipt_sha256": "bound-native-receipt"})
}

#[test]
fn cancellation_after_the_old_flag_check_prevents_any_catalog_revision() -> Result<()> {
    let temp = TempDir::new("publication-cancel-first");
    let dir = temp.path().canonicalize()?;
    let catalog = dir.join("artifacts");
    let call = Arc::new(Call::default());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel();
    thread::scope(|scope| -> Result<()> {
        let (call, dir, catalog) = (&call, &dir, &catalog);
        let worker = scope.spawn(move || -> std::result::Result<Value, String> {
            assert!(!call.stop().load(Ordering::SeqCst));
            ready_tx.send(()).unwrap();
            go_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let mut result = response();
            call.publish(dir, &mut result, || {
                crate::artifact_versions::publish_bytes(catalog, b"approved output", json!({}))
            })
            .map_err(|e| e.to_string())?;
            Ok(result)
        });
        ready_rx.recv_timeout(Duration::from_secs(5))?;
        call.cancel();
        go_tx.send(())?;
        let result = worker.join().unwrap().map_err(std::io::Error::other)?;
        assert_eq!(result["status"], "cancelled");
        assert_eq!(result["execution_status"], "succeeded");
        assert_eq!(result["publication"]["status"], "cancelled-before-commit");
        assert!(result.get("artifact").is_none());
        assert!(!catalog.exists());
        assert!(!dir.join("publication-intent.json").exists());
        Ok(())
    })
}

#[test]
fn publication_first_survives_late_cancellation_with_explicit_commit_evidence() -> Result<()> {
    let temp = TempDir::new("publication-commit-first");
    let dir = temp.path().canonicalize()?;
    let catalog = dir.join("artifacts");
    let call = Arc::new(Call::default());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel();
    thread::scope(|scope| -> Result<()> {
        let (call, dir, catalog) = (&call, &dir, &catalog);
        let worker = scope.spawn(move || -> std::result::Result<Value, String> {
            let mut result = response();
            call.publish(dir, &mut result, || {
                ready_tx.send(())?;
                go_rx.recv_timeout(Duration::from_secs(5))?;
                crate::artifact_versions::publish_bytes(catalog, b"approved output", json!({}))
            })
            .map_err(|e| e.to_string())?;
            Ok(result)
        });
        ready_rx.recv_timeout(Duration::from_secs(5))?;
        call.cancel();
        go_tx.send(())?;
        let result = worker.join().unwrap().map_err(std::io::Error::other)?;
        assert_eq!(result["status"], "succeeded");
        assert_eq!(result["publication"]["status"], "committed");
        assert_eq!(
            result["publication"]["cancellation"],
            "too-late-after-commit-start"
        );
        assert!(call.stop().load(Ordering::SeqCst));
        let intent: Value =
            serde_json::from_slice(&fs::read(dir.join("publication-intent.json"))?)?;
        assert_eq!(intent["receipt_sha256"], result["receipt_sha256"]);
        assert_eq!(intent["run_id"], result["run_id"]);
        let (bytes, _) = crate::artifact_versions::read(
            catalog,
            result["artifact"]["revision"].as_str().unwrap(),
        )?;
        assert_eq!(bytes, b"approved output");
        assert_eq!(fs::read_dir(catalog.join("revisions"))?.count(), 1);
        Ok(())
    })
}

#[test]
fn a_lost_result_after_catalog_commit_is_not_rolled_back_or_retried() -> Result<()> {
    let temp = TempDir::new("publication-uncertain");
    let dir = temp.path().canonicalize()?;
    let catalog = dir.join("artifacts");
    let call = Call::default();
    assert!(
        call.publish(&dir, &mut response(), || {
            crate::artifact_versions::publish_bytes(&catalog, b"retained", json!({}))?;
            Err("fault after catalog commit".into())
        })
        .is_err()
    );
    call.cancel();
    assert!(
        call.publish(&dir, &mut response(), || panic!("must not retry"))
            .is_err()
    );
    assert_eq!(fs::read_dir(catalog.join("revisions"))?.count(), 1);
    assert!(dir.join("publication-intent.json").exists());
    Ok(())
}

#[test]
fn failed_intent_write_cannot_start_import_or_reopen_the_decision() -> Result<()> {
    let temp = TempDir::new("publication-intent-failure");
    let dir = temp.path().canonicalize()?;
    fs::create_dir(dir.join("publication-intent.json"))?;
    let call = Call::default();
    assert!(
        call.publish(&dir, &mut response(), || panic!("must not import"))
            .is_err()
    );
    assert!(
        call.publish(&dir, &mut response(), || panic!("must not retry"))
            .is_err()
    );
    Ok(())
}
