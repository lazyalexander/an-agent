//! Opt-in: uses real, separately owned VMs and leaves evidence for review.
#![cfg(target_os = "macos")]
use an_agent_host::execution::{self, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[test]
#[ignore = "requires an operator OpenShell VM profile and Linux ARM64 kit/probe binaries"]
fn real_submission_timeout_and_cancellation() -> Result<()> {
    let profile = PathBuf::from(std::env::var("AN_OPENSHELL_PROFILE")?);
    let root = PathBuf::from(std::env::var("AN_OPENSHELL_EVIDENCE")?);
    fs::create_dir(&root)?; // Never overwrite prior evidence.
    for (name, variable, input, expected) in [
        (
            "kit",
            "AN_OPENSHELL_KIT",
            " beta\nalpha\nbeta\n\n",
            "succeeded",
        ),
        ("timeout", "AN_OPENSHELL_PROBE", "hang", "timed_out"),
        ("cancel", "AN_OPENSHELL_PROBE", "hang", "cancelled"),
    ] {
        let dir = root.join(name);
        fs::create_dir(&dir)?;
        let deployment = dir.join("deployment");
        fs::create_dir(&deployment)?;
        let binary = fs::read(std::env::var(variable)?)?;
        fs::write(deployment.join("program"), &binary)?;
        fs::write(
            deployment.join("spool.yaml"),
            format!(
                "v: 1\nkind: spool\nname: file-probe\nsummary: VM acceptance fixture\nversion: 0.1.0\nsort: beat\nconstructor: host\nhost: an-agent-file-v1\neffect:\n  file: {{ op: rw, path: /work, recursive: true }}\n  memory: {{ op: ignore }}\n  net: none\n  proc: spawn\n  model: {{ op: none }}\n  flow: both\ninverse: irreversible\nrequires: []\nconsumes: [kit.request]\nproduces: [kit.execute]\nconfig:\n  profile: file-kit-v1\n  executable_sha256: \"{:x}\"\n",
                Sha256::digest(&binary)
            ),
        )?;
        let store = dir.join("store");
        let release = execution::register(&deployment, &store)?["registered"]
            .as_str()
            .unwrap()
            .to_owned();
        let input_path = dir.join("input.txt");
        fs::write(&input_path, input)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let observer = if name == "cancel" {
            let (cancelled, done, store) = (cancelled.clone(), done.clone(), store.clone());
            Some(thread::spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    if fs::read_dir(store.join("runs")).ok().is_some_and(|runs| {
                        runs.flatten()
                            .any(|r| r.path().join("openshell-execute-request.json").exists())
                    }) {
                        thread::sleep(Duration::from_millis(500));
                        cancelled.store(true, Ordering::Relaxed);
                        break;
                    }
                    thread::sleep(Duration::from_millis(25));
                }
            }))
        } else {
            None
        };
        let result =
            execution::run_openshell(&profile, &store, &release, &input_path, true, &cancelled);
        done.store(true, Ordering::Relaxed);
        if let Some(observer) = observer {
            observer.join().unwrap();
        }
        let result = result?;
        assert_eq!(result["status"], expected, "{result}");
        let receipt_path = PathBuf::from(result["receipt"].as_str().unwrap());
        let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
        assert_eq!(receipt["openshell"]["termination_confirmed"], true);
        let observation = &receipt["operation"]["observation"];
        assert_eq!(observation["operation"], receipt["operation"]["intent"]);
        assert_eq!(observation["termination_confirmed"], true);
        assert_eq!(
            observation["environment"],
            observation["settled_environment"]
        );
        assert_eq!(
            observation["environment"]["id"],
            receipt["openshell"]["sandbox_id"]
        );
        assert_eq!(
            observation["environment"]["generation"],
            receipt["openshell"]["runtime_generation"]
        );
        let artifact = receipt_path.parent().unwrap().join("artifact.txt");
        if expected == "succeeded" {
            assert_eq!(fs::read(&artifact)?, b"alpha\nbeta\n");
        } else {
            assert!(!artifact.exists());
            assert!(receipt["artifact"].is_null());
            assert!(
                !receipt_path
                    .parent()
                    .unwrap()
                    .join("openshell-candidate-request.json")
                    .exists()
            );
        }
    }
    Ok(())
}
