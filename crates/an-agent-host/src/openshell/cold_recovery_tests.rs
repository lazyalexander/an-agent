//! Operator-only acceptance through the real host, followed by backend cold restart.
//! This never adds resume authority to the one-shot production adapter.
use super::{
    Profile,
    transport::{Cli, Transport},
};
use crate::execution::{self, Result, sandbox, store};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    thread,
    time::Duration,
};

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn call(profile: &Profile, root: &Path, step: &str, args: &[&str]) -> Result<sandbox::Execution> {
    Cli { profile, cwd: root }.call(
        step,
        &args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
        &AtomicBool::new(false),
        profile.control_seconds,
    )
}

fn control(profile: &Profile, root: &Path, step: &str, args: &[&str]) -> Result<Vec<u8>> {
    let r = call(profile, root, step, args)?;
    require(
        r.status == "succeeded" && r.code == Some(0) && r.reaped,
        &format!(
            "{step}: {}: {}",
            r.status,
            String::from_utf8_lossy(&r.stderr)
        ),
    )?;
    Ok(r.stdout)
}

fn state(profile: &Profile, root: &Path, step: &str, name: &str) -> Result<Value> {
    Ok(serde_json::from_slice(&control(
        profile,
        root,
        step,
        &["sandbox", "get", name, "-o", "json"],
    )?)?)
}

fn generation(state: &Value) -> Result<&str> {
    state["annotations"]["internal.openshell.ai/runtime-generation"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or("missing runtime generation".into())
}

fn stop_owned(
    profile: &Profile,
    root: &Path,
    label: &str,
    name: &str,
    original: &Value,
) -> Result<Value> {
    let before = state(profile, root, &format!("{label}-owner"), name)?;
    require(
        before["id"] == original["id"]
            && before["annotations"]["an-agent.run"] == original["annotations"]["an-agent.run"],
        "refuse to stop a replaced sandbox",
    )?;
    control(profile, root, label, &["sandbox", "stop", name])?;
    let stopped = state(profile, root, &format!("{label}-confirmed"), name)?;
    require(
        stopped["phase"] == "Stopped"
            && stopped["id"] == before["id"]
            && generation(&stopped)? == generation(&before)?,
        "stop not confirmed for this generation",
    )?;
    Ok(stopped)
}

#[test]
#[ignore = "requires an operator OpenShell 0.1.5 VM profile and Linux ARM64 file-list kit"]
fn real_workspace_cold_restart_preserves_files_not_processes() -> Result<()> {
    let profile_path = PathBuf::from(std::env::var("AN_OPENSHELL_PROFILE")?);
    let profile = Profile::load(&profile_path)?;
    let root = PathBuf::from(std::env::var("AN_OPENSHELL_EVIDENCE")?);
    fs::create_dir(&root)?; // Never overwrite earlier evidence.
    let deployment = root.join("deployment");
    fs::create_dir(&deployment)?;
    let binary = fs::read(std::env::var("AN_OPENSHELL_KIT")?)?;
    store::write_new(&deployment.join("program"), &binary)?;
    store::write_new(&deployment.join("spool.yaml"), format!(
        "v: 1\nkind: spool\nname: file-probe\nsummary: Cold recovery fixture\nversion: 0.1.0\nsort: beat\nconstructor: host\nhost: an-agent-file-v1\neffect:\n  file: {{ op: rw, path: /work, recursive: true }}\n  memory: {{ op: ignore }}\n  net: none\n  proc: spawn\n  model: {{ op: none }}\n  flow: both\ninverse: irreversible\nrequires: []\nconsumes: [kit.request]\nproduces: [kit.execute]\nconfig:\n  profile: file-kit-v1\n  executable_sha256: \"{}\"\n", store::digest(&binary)).as_bytes())?;
    let host_store = root.join("store");
    let registration = execution::register(&deployment, &host_store)?;
    let release = registration["registered"]
        .as_str()
        .ok_or("missing release")?;
    store::write_new(&root.join("input.txt"), b" beta\nalpha\nbeta\n")?;
    let run = execution::run_openshell(
        &profile_path,
        &host_store,
        release,
        &root.join("input.txt"),
        true,
        &AtomicBool::new(false),
    )?;
    require(run["status"] == "succeeded", "initial real host run failed")?;
    let receipt_path = PathBuf::from(run["receipt"].as_str().ok_or("missing receipt")?);
    let receipt_bytes = fs::read(&receipt_path)?;
    let receipt: Value = serde_json::from_slice(&receipt_bytes)?;
    let artifact_path = receipt_path
        .parent()
        .ok_or("missing run directory")?
        .join("artifact.txt");
    let artifact = fs::read(&artifact_path)?;
    require(
        artifact == b"alpha\nbeta\n",
        "unexpected accepted kit output",
    )?;
    let name = receipt["openshell"]["sandbox"]
        .as_str()
        .ok_or("missing sandbox")?;
    let original = state(&profile, &root, "original", name)?;
    require(
        original["phase"] == "Stopped"
            && original["id"] == receipt["openshell"]["sandbox_id"]
            && generation(&original)?
                == receipt["openshell"]["runtime_generation"]
                    .as_str()
                    .ok_or("missing receipt generation")?,
        "initial host settlement does not match backend",
    )?;

    // Everything after this point is explicit operator inspection, not a host retry.
    let result = (|| -> Result<Value> {
        control(
            &profile,
            &root,
            "start-inspection",
            &["sandbox", "start", name],
        )?;
        let first = state(&profile, &root, "first-ready", name)?;
        require(
            first["phase"] == "Ready"
                && first["id"] == original["id"]
                && generation(&first)? != generation(&original)?,
            "cold start did not change generation",
        )?;
        let probe = "cd /sandbox && cat /proc/sys/kernel/random/boot_id > boot.before && printf checkpoint-v1 > checkpoint.txt && sync && printf ready > ready.txt && sleep 12 && printf OLD_PROCESS_SURVIVED > late.txt";
        thread::scope(|scope| -> Result<()> {
            let worker = scope.spawn(|| {
                call(
                    &profile,
                    &root,
                    "live-process",
                    &[
                        "sandbox",
                        "exec",
                        name,
                        "--no-login-shell",
                        "--",
                        "/bin/sh",
                        "-c",
                        probe,
                    ],
                )
                .map_err(|error| error.to_string())
            });
            let observed = (|| -> Result<()> {
                let mut ready = false;
                // Read-only bounded observations; never retry the workload.
                for i in 0..20 {
                    let r = call(
                        &profile,
                        &root,
                        &format!("checkpoint-{i}"),
                        &[
                            "sandbox",
                            "exec",
                            name,
                            "--no-login-shell",
                            "--",
                            "/bin/sh",
                            "-c",
                            "test -f /sandbox/ready.txt && test -s /sandbox/boot.before",
                        ],
                    )?;
                    if r.status == "succeeded" {
                        ready = true;
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                require(ready, "checkpoint not observed")?;
                Ok(())
            })();
            // Always stop before joining the workload, including a failed observation.
            let stopped = stop_owned(&profile, &root, "stop-probe", name, &original);
            let worker = worker.join().map_err(|_| "probe worker panicked")??;
            observed?;
            stopped?;
            require(
                worker.status != "succeeded",
                "probe completed before stop; invalid witness",
            )
        })?;
        let refused = call(
            &profile,
            &root,
            "stopped-exec",
            &[
                "sandbox",
                "exec",
                name,
                "--timeout",
                "1",
                "--no-login-shell",
                "--",
                "/bin/true",
            ],
        )?;
        require(
            refused.status == "failed"
                && refused.code == Some(1)
                && refused.reaped
                && String::from_utf8_lossy(&refused.stderr).contains("phase: Stopped"),
            "missing explicit stopped-state refusal",
        )?;
        control(
            &profile,
            &root,
            "start-recovery",
            &["sandbox", "start", name],
        )?;
        let restored = state(&profile, &root, "restored", name)?;
        require(
            restored["phase"] == "Ready"
                && restored["id"] == original["id"]
                && generation(&restored)? != generation(&first)?
                && generation(&restored)? != generation(&original)?
                && restored["policy"] == original["policy"],
            "restart identity or policy mismatch",
        )?;
        let proof = control(
            &profile,
            &root,
            "verify-disk-and-boot",
            &[
                "sandbox",
                "exec",
                name,
                "--no-login-shell",
                "--",
                "/bin/sh",
                "-c",
                "cd /sandbox && test \"$(cat checkpoint.txt)\" = checkpoint-v1 && test \"$(cat boot.before)\" != \"$(cat /proc/sys/kernel/random/boot_id)\" && test \"$(cat output.txt)\" = \"$(printf 'alpha\\nbeta')\" && echo COLD_RECOVERY_VERIFIED",
            ],
        )?;
        // More than the original probe delay: no old process may finish after restore.
        thread::sleep(Duration::from_secs(13));
        control(
            &profile,
            &root,
            "verify-no-old-process",
            &[
                "sandbox",
                "exec",
                name,
                "--no-login-shell",
                "--",
                "/bin/sh",
                "-c",
                "test ! -e /sandbox/late.txt && printf inspection-only > /sandbox/output.txt",
            ],
        )?;
        require(
            fs::read(&artifact_path)? == artifact && fs::read(&receipt_path)? == receipt_bytes,
            "inspection changed accepted host output or receipt",
        )?;
        Ok(json!({"schema":"an-openshell-cold-recovery-evidence-v1",
            "backend_version":"0.1.5", "sandbox_id":original["id"],
            "generations":[generation(&original)?, generation(&first)?, generation(&restored)?],
            "checkpoint_preserved":true,"boot_changed":true,"old_process_absent_after_seconds":13,
            "stopped_exec_refused":true,"policy_preserved":true,"host_receipt_unchanged":true,
            "host_artifact_sha256":store::digest(&artifact),"proof":String::from_utf8_lossy(&proof),
            "memory_resume":false,"vm_backup":false,"automatic_retry":false}))
    })();
    let cleanup = stop_owned(&profile, &root, "final-stop", name, &original);
    store::write_new(
        &root.join("cold-recovery.json"),
        &serde_json::to_vec_pretty(&json!({
            "result":match &result { Ok(v) => v.clone(), Err(e) => json!({"error":e.to_string()}) },
            "final_stop_confirmed":cleanup.is_ok(),
            "cleanup_error":cleanup.as_ref().err().map(|e| e.to_string())
        }))?,
    )?;
    cleanup?;
    result?;
    Ok(())
}
