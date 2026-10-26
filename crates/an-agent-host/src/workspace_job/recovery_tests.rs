//! Process-exit fault windows use tiny inputs, not timing-sensitive large files.
use super::*;
use an_agent_core::testkit::TempDir;
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn crash_child() -> Result<()> {
    let Ok(base) = std::env::var("AN_AGENT_PRELAUNCH_CRASH_FIXTURE") else {
        return Ok(());
    };
    let base = PathBuf::from(base);
    let stage = std::env::var("AN_AGENT_PRELAUNCH_CRASH_STAGE")?;
    let prior = fs::read_to_string(base.join("prior"))?;
    let root = base.join("job");
    let profile = base.join("profile.json");
    let stop = AtomicBool::new(false);
    let preview = run(&profile, &root, Some(&prior), None, &stop)?;
    run_inner(
        &profile,
        &root,
        Some(&prior),
        preview["approval_sha256"].as_str(),
        &stop,
        |boundary, dir| {
            let hit = matches!(
                (stage.as_str(), boundary),
                ("prepared", not_started::Boundary::Prepared)
                    | ("committed", not_started::Boundary::IntentPersisted)
                    | ("ready", not_started::Boundary::ReadyToLaunch)
                    | ("launch", not_started::Boundary::LaunchFenced)
            );
            if hit {
                fs::write(
                    base.join("child-attempt"),
                    dir.file_name().unwrap().to_str().unwrap(),
                )
                .unwrap();
                // No Rust unwinding: retain the actual on-disk crash state and lock.
                std::process::exit(73);
            }
        },
    )?;
    Err("fault boundary was not reached".into())
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "real child-process deadline uses a monotonic clock"
)]
fn restart_uses_only_committed_prelaunch_evidence() -> Result<()> {
    for stage in ["prepared", "committed", "ready", "launch"] {
        let temp = TempDir::new("prelaunch-restart");
        let base = temp.path().canonicalize()?;
        let root = base.join("job");
        let profile = base.join("profile.json");
        fs::write(base.join("input.txt"), b"small")?;
        fs::write(
            &profile,
            serde_json::to_vec(&json!({"schema":"an-workspace-job-v2",
            "executable":"/bin/bash", "args":["-c","printf preserved > output.txt"],
            "inputs":[base.join("input.txt")],"readonly":[],"environment":{},"outputs":["output.txt"],
            "wall_seconds":10,"trusted_process_group":true}))?,
        )?;
        let stop = AtomicBool::new(false);
        let preview = run(&profile, &root, None, None, &stop)?;
        let receipt = run(
            &profile,
            &root,
            None,
            preview["approval_sha256"].as_str(),
            &stop,
        )?;
        let prior = receipt["attempt"].as_str().unwrap();
        let preview = accept(&root, prior, None)?;
        let accepted = accept(&root, prior, preview["approval_sha256"].as_str())?;
        fs::write(base.join("prior"), prior)?;
        let mut child = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "workspace_job::recovery_tests::crash_child",
                "--nocapture",
            ])
            .env("AN_AGENT_PRELAUNCH_CRASH_FIXTURE", &base)
            .env("AN_AGENT_PRELAUNCH_CRASH_STAGE", stage)
            .stdout(Stdio::null())
            .spawn()?;
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if start.elapsed() > Duration::from_secs(15) {
                child.kill()?;
                child.wait()?;
                return Err("crash fixture exceeded its deadline".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(73));
        let id = fs::read_to_string(base.join("child-attempt"))?;
        assert!(reclaim(&root, &id, None).is_err()); // stale lock still fences APIs
        // Operator qualification is explicit: child is reaped, no launch occurred
        // in this fixture. Production never removes a stale lock automatically.
        fs::remove_file(root.join("host.lock"))?;
        assert!(accept(&root, &id, None).is_err());
        match stage {
            "prepared" => {
                assert!(record(&root, prior).is_ok());
                assert!(record(&root, &id).is_err()); // orphan cannot reclaim shared work
            }
            "launch" => {
                assert!(record(&root, prior).is_err());
                assert!(reclaim(&root, &id, None).is_err());
                assert!(run(&profile, &root, Some(&id), None, &stop).is_err());
            }
            "committed" => {
                assert!(record(&root, prior).is_err());
                let preview = reclaim(&root, &id, None)?;
                reclaim(&root, &id, preview["approval_sha256"].as_str())?;
                assert!(!root.join("work").exists());
            }
            _ => {
                assert!(record(&root, prior).is_err());
                assert!(reclaim(&root, &id, None).is_ok());
                let preview = run(&profile, &root, Some(&id), None, &stop)?;
                let next = run(
                    &profile,
                    &root,
                    Some(&id),
                    preview["approval_sha256"].as_str(),
                    &stop,
                )?;
                assert_eq!(next["status"], "succeeded");
            }
        }
        let revision = accepted["outputs"][0]["binding"]["revision"]
            .as_str()
            .unwrap();
        assert_eq!(
            crate::artifact_versions::read(&root.join("artifacts"), revision)?.0,
            b"preserved"
        );
    }
    Ok(())
}
