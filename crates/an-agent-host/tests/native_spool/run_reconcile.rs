use super::*;
#[test]
fn completed_execution_is_reconciled_without_retry_or_implicit_acceptance() -> Result<()> {
    for failed in [false, true] {
        let temp = TempDir::new("spool-run-reconcile");
        let base = temp.path().canonicalize()?;
        let source = package(&base)?;
        let root = base.join("instance");
        spool::load(&source, &root)?;
        let input = base.join("input.txt");
        fs::write(&input, b"hello\n")?;
        if failed {
            let edit = base.join("failure.sh");
            fs::write(&edit, b"exit 7\n")?;
            let plan = spool::update_kit(&root, "transform.sh", &edit, None)?;
            spool::update_kit(
                &root,
                "transform.sh",
                &edit,
                plan["approval_sha256"].as_str(),
            )?;
        }
        let preview = spool::start(&root, "run", &input, 30, None, &AtomicBool::new(false))?;
        assert!(spool::reconcile_run(&root, "run", None).is_err());
        let target = root.join("runs/run/result.json");
        // Real failure after process settlement, not a fabricated success receipt.
        fs::create_dir(&target)?;
        assert!(
            spool::start(
                &root,
                "run",
                &input,
                30,
                preview["approval_sha256"].as_str(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(spool::reconcile_run(&root, "run", None).is_err());
        fs::remove_dir(&target)?;
        assert_eq!(spool::status(&root, "run")?["termination_confirmed"], false);
        assert!(accept(&root, "run").is_err());
        assert!(reclaim(&root, "run").is_err());
        assert!(unload(&root).is_err());
        let attempts = root.join("runs/run/job/attempts");
        let dir = fs::read_dir(&attempts)?.next().unwrap()?.path();
        let execution = fs::read(dir.join("execution.json"))?;
        fs::write(dir.join("execution.json"), b"changed")?;
        assert!(spool::reconcile_run(&root, "run", None).is_err());
        fs::write(dir.join("execution.json"), &execution)?;
        let receipt = fs::read(dir.join("receipt.json"))?;
        let mut uncertain: Value = serde_json::from_slice(&receipt)?;
        uncertain["termination_confirmed"] = json!(false);
        fs::write(dir.join("receipt.json"), serde_json::to_vec(&uncertain)?)?;
        assert!(spool::reconcile_run(&root, "run", None).is_err());
        fs::write(dir.join("receipt.json"), &receipt)?;
        let start_path = root.join("runs/run/start.json");
        let start = fs::read(&start_path)?;
        let mut foreign: Value = serde_json::from_slice(&start)?;
        foreign["instance"] = json!("foreign");
        fs::write(&start_path, serde_json::to_vec(&foreign)?)?;
        assert!(spool::reconcile_run(&root, "run", None).is_err());
        fs::write(&start_path, &start)?;
        let extra = attempts.join("00000000-0000-0000-0000-000000000000");
        fs::create_dir(&extra)?;
        assert!(spool::reconcile_run(&root, "run", None).is_err());
        fs::remove_dir(&extra)?;
        let metadata = attempts.join(".DS_Store");
        fs::write(&metadata, b"Finder metadata")?;
        let plan = spool::reconcile_run(&root, "run", None)?;
        assert_eq!(
            plan["plan"]["ignored_attempt_entries"],
            json!([".DS_Store"])
        );
        for (name, directory) in [
            ("editor.swap", false),
            ("unexpected-directory", true),
            ("11111111-1111-4111-A111-111111111111", true),
            ("00000000-0000-0000-0000-000000000000", false),
        ] {
            let path = attempts.join(name);
            if directory {
                fs::create_dir(&path)?;
            } else {
                fs::write(&path, b"unexpected")?;
            }
            let error = spool::reconcile_run(&root, "run", None).unwrap_err();
            assert!(error.to_string().contains(name), "{error}");
            if directory {
                fs::remove_dir(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
        fs::create_dir(&extra)?;
        assert!(spool::reconcile_run(&root, "run", None).is_err());
        fs::remove_dir(&extra)?;
        fs::remove_file(&metadata)?;
        fs::create_dir(&metadata)?;
        assert!(
            spool::reconcile_run(&root, "run", None)
                .unwrap_err()
                .to_string()
                .contains(".DS_Store")
        );
        fs::remove_dir(&metadata)?;
        std::os::unix::fs::symlink(&dir, &metadata)?;
        assert!(
            spool::reconcile_run(&root, "run", None)
                .unwrap_err()
                .to_string()
                .contains(".DS_Store")
        );
        fs::remove_file(&metadata)?;
        fs::write(&metadata, b"Finder metadata")?;
        let withheld = root.join("withheld-attempt");
        fs::rename(&dir, &withheld)?;
        assert!(spool::reconcile_run(&root, "run", None).is_err());
        fs::rename(&withheld, &dir)?;
        assert!(!target.exists());
        assert!(spool::reconcile_run(&root, "run", Some(&"0".repeat(64))).is_err());
        assert!(!target.exists());
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_an-agent-host"))
            .args([
                "spool-reconcile-run",
                root.to_str().unwrap(),
                "run",
                "--approve",
                plan["approval_sha256"].as_str().unwrap(),
            ])
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(result["execution_retried"], false);
        assert_eq!(result["artifacts_published"], false);
        assert_eq!(
            spool::status(&root, "run")?["execution_status"],
            if failed { "failed" } else { "succeeded" }
        );
        assert_eq!(
            spool::reconcile_run(&root, "run", plan["approval_sha256"].as_str())?["already_recorded"],
            true
        );
        assert_eq!(fs::read_dir(&attempts)?.count(), 2);
        assert_eq!(fs::read(&metadata)?, b"Finder metadata");
        assert_eq!(fs::read(dir.join("execution.json"))?, execution);
        assert_eq!(fs::read(dir.join("receipt.json"))?, receipt);
        assert!(!root.join("runs/run/accepted.json").exists());
        assert!(!root.join("runs/run/job/artifacts").exists());
        let saved = fs::read(&target)?;
        fs::write(&target, b"{}")?;
        assert!(spool::reconcile_run(&root, "run", None).is_err());
        assert_eq!(fs::read(&target)?, b"{}");
        fs::write(&target, saved)?;
        if failed {
            assert!(accept(&root, "run").is_err());
        } else {
            accept(&root, "run")?;
        }
        reclaim(&root, "run")?;
        unload(&root)?;
    }
    Ok(())
}
