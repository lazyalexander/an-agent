//! Real macOS workspace custody through the public host boundary, without Java.
#![cfg(target_os = "macos")]
#![allow(clippy::disallowed_methods)]
use an_agent_core::testkit::TempDir;
use an_agent_host::{execution::Result, workspace_job as job};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

fn fixture(temp: &TempDir, script: &str, deadline: u64) -> Result<(PathBuf, PathBuf)> {
    let root = temp.path().canonicalize()?;
    let input = root.join("selected");
    fs::create_dir(&input)?;
    fs::write(input.join("input.txt"), b"immutable")?;
    let profile = root.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&json!({"schema":"an-workspace-job-v1",
        "executable":"/bin/bash","args":["-c",script,"fixture",root.join("forbidden.txt")],
        "readonly":[input],"inputs":[input.join("input.txt")],"environment":{},
        "outputs":["output.txt"],"wall_seconds":deadline,"trusted_process_group":true}))?,
    )?;
    Ok((profile, root.join("job")))
}

fn run(profile: &Path, root: &Path, prior: Option<&str>, stop: &AtomicBool) -> Result<Value> {
    let preview = job::run(profile, root, prior, None, stop)?;
    job::run(
        profile,
        root,
        prior,
        preview["approval_sha256"].as_str(),
        stop,
    )
}

#[test]
fn acceptance_copies_versions_and_reclamation_preserves_them() -> Result<()> {
    let temp = TempDir::new("workspace-custody");
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let script = format!(
        "if (printf probe > /dev/tcp/127.0.0.1/{port}) 2>/dev/null; then exit 91; fi; if printf nope > \"$1\" 2>/dev/null; then exit 90; fi; printf 'ALPHA\\n' > output.txt"
    );
    let (profile, root) = fixture(&temp, &script, 10)?;
    let stop = AtomicBool::new(false);
    assert!(job::run(&profile, &root, None, Some(&"0".repeat(64)), &stop).is_err());
    assert!(!root.exists());
    let receipt = run(&profile, &root, None, &stop)?;
    assert_eq!(receipt["status"], "succeeded");
    assert_eq!(receipt["termination_confirmed"], true);
    assert!(!temp.path().join("forbidden.txt").exists());
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    let id = receipt["attempt"].as_str().unwrap();
    assert!(job::reclaim(&root, id, None).is_err());
    let preview = job::accept(&root, id, None)?;
    fs::write(root.join("work/output.txt"), b"changed")?;
    assert!(job::accept(&root, id, preview["approval_sha256"].as_str()).is_err());
    fs::write(root.join("work/output.txt"), b"ALPHA\n")?;
    let preview = job::accept(&root, id, None)?;
    let accepted = job::accept(&root, id, preview["approval_sha256"].as_str())?;
    assert!(job::accept(&root, id, preview["approval_sha256"].as_str()).is_err());
    let revision = accepted["outputs"][0]["binding"]["revision"]
        .as_str()
        .unwrap();
    let catalog = root.join("artifacts");
    let host_temp = temp.path().canonicalize()?;
    an_agent_host::artifact_versions::checkout(&catalog, revision, &host_temp.join("before.txt"))?;
    fs::write(root.join("work/diagnostic.tmp"), b"internal")?;
    let reclaim = job::reclaim(&root, id, None)?;
    fs::write(root.join("work/late.tmp"), b"late")?;
    assert!(job::reclaim(&root, id, reclaim["approval_sha256"].as_str()).is_err());
    assert!(root.join("work/output.txt").exists());
    let reclaim = job::reclaim(&root, id, None)?;
    job::reclaim(&root, id, reclaim["approval_sha256"].as_str())?;
    assert!(!root.join("work").exists());
    an_agent_host::artifact_versions::checkout(&catalog, revision, &host_temp.join("after.txt"))?;
    assert_eq!(
        fs::read(temp.path().join("before.txt"))?,
        fs::read(temp.path().join("after.txt"))?
    );
    assert!(job::run(&profile, &root, Some(id), None, &stop).is_err());
    Ok(())
}

#[test]
fn running_uncertain_and_stale_attempts_cannot_collect_or_resume() -> Result<()> {
    let temp = TempDir::new("workspace-cancel-resume");
    let (profile, root) = fixture(
        &temp,
        "if [ ! -f checkpoint ]; then printf ready > checkpoint; /bin/sleep 30; fi; printf 'BETA\\n' > output.txt",
        20,
    )?;
    let stop = AtomicBool::new(false);
    let first = thread::scope(|scope| -> Result<Value> {
        let running = scope.spawn(|| run(&profile, &root, None, &stop).map_err(|e| e.to_string()));
        let start = Instant::now();
        while !root.join("work/checkpoint").exists() {
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(10));
        }
        // A real process is still writing/holding the workspace, not a fixture flag.
        let id = fs::read_dir(root.join("attempts"))?
            .next()
            .unwrap()?
            .file_name()
            .into_string()
            .unwrap();
        assert!(job::reclaim(&root, &id, None).is_err());
        assert!(job::accept(&root, &id, None).is_err());
        stop.store(true, Ordering::Relaxed);
        Ok(running.join().unwrap().map_err(std::io::Error::other)?)
    })?;
    assert_eq!(first["status"], "cancelled");
    assert_eq!(first["termination_confirmed"], true);
    let id = first["attempt"].as_str().unwrap();
    assert!(job::accept(&root, id, None).is_err());
    assert!(root.join("work/checkpoint").exists());
    let file = root.join("attempts").join(id).join("receipt.json");
    let bytes = fs::read(&file)?;
    let mut uncertain = first.clone();
    uncertain["termination_confirmed"] = json!(false);
    fs::write(&file, serde_json::to_vec_pretty(&uncertain)?)?;
    stop.store(false, Ordering::Relaxed);
    assert!(job::reclaim(&root, id, None).is_err());
    assert!(job::run(&profile, &root, Some(id), None, &stop).is_err());
    fs::write(&file, bytes)?;
    let resumed = run(&profile, &root, Some(id), &stop)?;
    assert_eq!(resumed["status"], "succeeded");
    assert_eq!(fs::read(root.join("work/output.txt"))?, b"BETA\n");
    assert!(job::reclaim(&root, id, None).is_err());
    assert!(job::run(&profile, &root, Some(id), None, &stop).is_err());
    Ok(())
}

#[test]
fn deadline_and_linked_or_failed_outputs_are_not_promoted() -> Result<()> {
    for (name, script, seconds) in [
        ("timeout", "printf partial > output.txt; /bin/sleep 30", 1),
        ("failure", "printf partial > output.txt; exit 7", 10),
        (
            "symlink",
            "printf result > target; ln -s target output.txt",
            10,
        ),
        (
            "hardlink",
            "printf result > target; ln target output.txt",
            10,
        ),
    ] {
        let temp = TempDir::new(name);
        let (profile, root) = fixture(&temp, script, seconds)?;
        let result = run(&profile, &root, None, &AtomicBool::new(false))?;
        assert_eq!(result["termination_confirmed"], true);
        if name == "timeout" {
            assert_eq!(result["status"], "timed_out");
        }
        assert!(job::accept(&root, result["attempt"].as_str().unwrap(), None).is_err());
        assert!(!root.join("artifacts").exists());
    }
    Ok(())
}

#[path = "workspace_job/binary.rs"]
mod binary;

#[path = "workspace_job/multi_silk.rs"]
mod multi_silk;

#[path = "workspace_job/large_handoff.rs"]
mod large_handoff;

#[path = "workspace_job/settlement.rs"]
mod settlement;
