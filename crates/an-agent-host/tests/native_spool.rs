//! Real release-bound native spools with independent instance authority.
#![cfg(target_os = "macos")]
use an_agent_core::testkit::TempDir;
use an_agent_host::{execution::Result, native_spool as spool};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

fn package(base: &Path) -> Result<PathBuf> {
    let root = base.join("bundle");
    fs::create_dir(&root)?;
    fs::create_dir(root.join("silk"))?;
    fs::create_dir(root.join("kit"))?;
    let mut files = serde_json::Map::new();
    for (path, text) in [
        (
            "silk/main.sh",
            "set -eu\nif [ \"$(cat \"$AN_SPOOL_INPUT\")\" = hold ]; then printf ready > ready; sleep 30; fi\nif printf forbidden > \"$AN_SPOOL_SILK_DIR/report.sh\" 2>/dev/null; then exit 91; fi\n/bin/bash \"$AN_SPOOL_KIT_DIR/transform.sh\" \"$AN_SPOOL_INPUT\" > intermediate.txt\n/bin/bash \"$AN_SPOOL_SILK_DIR/report.sh\" intermediate.txt > report.txt\n",
        ),
        (
            "silk/report.sh",
            "set -eu\nprintf 'Report\\n'\ncat \"$1\"\n",
        ),
        (
            "kit/transform.sh",
            "set -eu\ntr '[:lower:]' '[:upper:]' < \"$1\"\n",
        ),
    ] {
        fs::write(root.join(path), text)?;
        files.insert(path.into(), json!(format!("{:x}", Sha256::digest(text))));
    }
    fs::write(
        root.join("native-spool.json"),
        serde_json::to_vec(
            &json!({"schema":"an-native-spool-v1","name":"text","version":"1.0.0","entry":"silk/main.sh","files":files,"outputs":["report.txt"]}),
        )?,
    )?;
    Ok(root)
}
fn run(root: &Path, name: &str, input: &Path, stop: &AtomicBool) -> Result<Value> {
    let preview = spool::start(root, name, input, 30, None, stop)?;
    spool::start(
        root,
        name,
        input,
        30,
        preview["approval_sha256"].as_str(),
        stop,
    )
}
fn accept(root: &Path, name: &str) -> Result<Value> {
    let p = spool::accept(root, name, None)?;
    spool::accept(root, name, p["approval_sha256"].as_str())
}
fn reclaim(root: &Path, name: &str) -> Result<Value> {
    let p = spool::reclaim(root, name, None)?;
    spool::reclaim(root, name, p["approval_sha256"].as_str())
}
fn unload(root: &Path) -> Result<Value> {
    let p = spool::unload(root, None)?;
    spool::unload(root, p["approval_sha256"].as_str())
}

#[test]
fn editable_kit_is_scoped_and_fresh_load_restores_release_baseline() -> Result<()> {
    let temp = TempDir::new("spool-kit-scope");
    let base = temp.path().canonicalize()?;
    let bundle = package(&base)?;
    let root = base.join("instance");
    let loaded = spool::load(&bundle, &root)?;
    assert_eq!(loaded["execution_granted"], false);
    let input = base.join("input.txt");
    fs::write(&input, b"hello\n")?;
    let edit = base.join("edit.sh");
    fs::write(
        &edit,
        format!(
            "if printf escape > '{}' 2>/dev/null; then exit 92; fi; printf 'adapted:'; cat \"$1\"\n",
            base.join("forbidden.txt").display()
        ),
    )?;
    assert!(spool::update_kit(&root, "report.sh", &edit, None).is_err());
    assert!(spool::update_kit(&root, "../silk/report.sh", &edit, None).is_err());
    let stale = spool::start(&root, "first", &input, 30, None, &AtomicBool::new(false))?;
    let p = spool::update_kit(&root, "transform.sh", &edit, None)?;
    spool::update_kit(&root, "transform.sh", &edit, p["approval_sha256"].as_str())?;
    assert!(
        spool::start(
            &root,
            "first",
            &input,
            30,
            stale["approval_sha256"].as_str(),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(!root.join("runs/first/job").exists());
    assert_eq!(
        run(&root, "first", &input, &AtomicBool::new(false))?["status"],
        "succeeded"
    );
    assert_eq!(
        fs::read_to_string(root.join("runs/first/job/work/report.txt"))?,
        "Report\nadapted:hello\n"
    );
    assert!(!base.join("forbidden.txt").exists());
    let native_result: Value =
        serde_json::from_slice(&fs::read(root.join("runs/first/result.json"))?)?;
    let native_id = native_result["workspace"]["attempt"].as_str().unwrap();
    let native_job = root.join("runs/first/job");
    assert!(an_agent_host::workspace_job::accept(&native_job, native_id, None).is_err());
    assert!(an_agent_host::workspace_job::reclaim(&native_job, native_id, None).is_err());
    assert!(
        an_agent_host::workspace_job::run(
            &root.join("runs/first/profile.json"),
            &native_job,
            Some(native_id),
            None,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let accepted = accept(&root, "first")?;
    reclaim(&root, "first")?;
    assert!(spool::reset_kit(&root, "report.sh", None).is_err());
    assert!(spool::reset_kit(&root, "../silk/report.sh", None).is_err());
    let before_reset = spool::start(&root, "restored", &input, 30, None, &AtomicBool::new(false))?;
    let host = env!("CARGO_BIN_EXE_an-agent-host");
    let preview = std::process::Command::new(host)
        .args(["spool-kit-reset", root.to_str().unwrap(), "transform.sh"])
        .output()?;
    assert!(preview.status.success());
    assert!(fs::read_to_string(root.join("kit/transform.sh"))?.contains("adapted:"));
    let preview: Value = serde_json::from_slice(&preview.stdout)?;
    let reset = std::process::Command::new(host)
        .args([
            "spool-kit-reset",
            root.to_str().unwrap(),
            "transform.sh",
            "--approve",
            preview["approval_sha256"].as_str().unwrap(),
        ])
        .output()?;
    assert!(reset.status.success());
    assert_eq!(
        fs::read(root.join("kit/transform.sh"))?,
        fs::read(bundle.join("kit/transform.sh"))?
    );
    assert!(
        spool::start(
            &root,
            "restored",
            &input,
            30,
            before_reset["approval_sha256"].as_str(),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(
        run(&root, "restored", &input, &AtomicBool::new(false))?["status"],
        "succeeded"
    );
    assert_eq!(
        fs::read_to_string(root.join("runs/restored/job/work/report.txt"))?,
        "Report\nHELLO\n"
    );
    assert_eq!(spool::show(&root)?["instance"], loaded["instance"]);
    accept(&root, "restored")?;
    reclaim(&root, "restored")?;
    unload(&root)?;
    assert!(spool::reset_kit(&root, "transform.sh", None).is_err());
    assert!(spool::start(&root, "second", &input, 30, None, &AtomicBool::new(false)).is_err());
    let fresh = base.join("fresh");
    let reloaded = spool::load(&bundle, &fresh)?;
    assert_eq!(loaded["release"], reloaded["release"]);
    assert_ne!(loaded["instance"], reloaded["instance"]);
    assert_eq!(
        fs::read(fresh.join("kit/transform.sh"))?,
        fs::read(bundle.join("kit/transform.sh"))?
    );
    assert_eq!(
        run(&fresh, "baseline", &input, &AtomicBool::new(false))?["status"],
        "succeeded"
    );
    assert_eq!(
        fs::read_to_string(fresh.join("runs/baseline/job/work/report.txt"))?,
        "Report\nHELLO\n"
    );
    let revision = accepted["result"]["outputs"][0]["binding"]["revision"]
        .as_str()
        .unwrap();
    an_agent_host::artifact_versions::checkout(
        &root.join("runs/first/job/artifacts"),
        revision,
        &base.join("retained.txt"),
    )?;
    assert_eq!(
        fs::read_to_string(base.join("retained.txt"))?,
        "Report\nadapted:hello\n"
    );
    Ok(())
}

#[test]
#[allow(clippy::disallowed_methods, reason = "real process readiness deadline")]
fn cancelling_one_instance_cannot_unload_edit_or_claim_another() -> Result<()> {
    let temp = TempDir::new("spool-two-instances");
    let base = temp.path().canonicalize()?;
    let bundle = package(&base)?;
    let a = base.join("a");
    let b = base.join("b");
    spool::load(&bundle, &a)?;
    spool::load(&bundle, &b)?;
    let hold = base.join("hold.txt");
    fs::write(&hold, b"hold")?;
    let input = base.join("input.txt");
    fs::write(&input, b"independent")?;
    let stop = AtomicBool::new(false);
    let cancelled = thread::scope(|scope| -> Result<Value> {
        let running = scope.spawn(|| run(&a, "one", &hold, &stop).map_err(|e| e.to_string()));
        let since = Instant::now();
        while !a.join("runs/one/job/work/ready").exists() {
            assert!(!running.is_finished());
            assert!(since.elapsed() < Duration::from_secs(15));
            thread::sleep(Duration::from_millis(10));
        }
        assert!(spool::unload(&a, None).is_err());
        assert!(spool::reset_kit(&a, "transform.sh", None).is_err());
        assert!(spool::reclaim(&a, "one", None).is_err());
        assert!(spool::update_kit(&a, "transform.sh", &input, None).is_err());
        assert_eq!(
            run(&b, "two", &input, &AtomicBool::new(false))?["status"],
            "succeeded"
        );
        stop.store(true, Ordering::Release);
        Ok(running.join().unwrap().map_err(std::io::Error::other)?)
    })?;
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(cancelled["workspace"]["termination_confirmed"], true);
    assert!(spool::accept(&a, "one", None).is_err());
    let receipt = a.join("runs/one/result.json");
    let bytes = fs::read(&receipt)?;
    fs::write(&receipt, fs::read(b.join("runs/two/result.json"))?)?;
    assert!(spool::accept(&a, "one", None).is_err());
    assert!(spool::unload(&a, None).is_err());
    fs::write(&receipt, bytes)?;
    let native_receipt = a
        .join("runs/one/job/attempts")
        .join(cancelled["workspace"]["attempt"].as_str().unwrap())
        .join("receipt.json");
    let original_receipt = fs::read(&native_receipt)?;
    let mut unknown = cancelled.clone();
    unknown["workspace"]["termination_confirmed"] = json!(false);
    fs::write(&native_receipt, serde_json::to_vec(&unknown["workspace"])?)?;
    fs::write(&receipt, serde_json::to_vec(&unknown)?)?;
    assert!(spool::reclaim(&a, "one", None).is_err());
    assert!(spool::unload(&a, None).is_err());
    fs::write(&native_receipt, original_receipt)?;
    fs::write(&receipt, serde_json::to_vec(&cancelled)?)?;
    reclaim(&a, "one")?;
    unload(&a)?;
    accept(&b, "two")?;
    assert_eq!(spool::show(&b)?["loaded"], true);
    assert!(spool::load(&bundle, &b).is_err());
    assert!(spool::start(&b, "two", &input, 30, None, &AtomicBool::new(false)).is_err());
    Ok(())
}

#[path = "native_spool/weaver.rs"]
mod weaver;

#[path = "native_spool/artifacts.rs"]
mod artifacts;

#[path = "native_spool/status.rs"]
mod status;

#[path = "native_spool/reconcile.rs"]
mod reconcile;

#[path = "native_spool/run_reconcile.rs"]
mod run_reconcile;

#[path = "native_spool/supervised_inputs.rs"]
mod supervised_inputs;

#[path = "native_spool/prelaunch_cancel.rs"]
mod prelaunch_cancel;
