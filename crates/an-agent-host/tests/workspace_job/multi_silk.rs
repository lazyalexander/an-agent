//! One reviewed spool body, two silks, one native group and one custody boundary.
use super::*;
use an_agent_host::artifact_versions;
use sha2::{Digest, Sha256};

fn setup(temp: &TempDir, mode: &str) -> Result<(PathBuf, PathBuf)> {
    let (profile, root) = fixture(temp, "unused", if mode == "timeout" { 1 } else { 20 })?;
    let selected = temp.path().canonicalize()?.join("selected");
    for (name, source) in [
        (
            "coordinator.sh",
            include_str!("../fixtures/two-silks/coordinator.sh"),
        ),
        (
            "producer.sh",
            include_str!("../fixtures/two-silks/producer.sh"),
        ),
        (
            "consumer.sh",
            include_str!("../fixtures/two-silks/consumer.sh"),
        ),
    ] {
        fs::write(selected.join(name), source)?;
    }
    let mut p: Value = serde_json::from_slice(&fs::read(&profile)?)?;
    p["args"] = json!(["{input:0}", "{inputs}", mode]);
    p["inputs"] = json!([
        selected.join("coordinator.sh"),
        selected.join("producer.sh"),
        selected.join("consumer.sh")
    ]);
    fs::write(&profile, serde_json::to_vec(&p)?)?;
    Ok((profile, root))
}

fn wait_for(root: &Path, name: &str) -> Result<()> {
    let start = Instant::now();
    while !root.join("work").join(name).exists() {
        if start.elapsed() > Duration::from_secs(10) {
            return Err(format!("silk did not reach {name}").into());
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

fn members(root: &Path) -> Result<Value> {
    let mut result = Vec::new();
    for name in ["coordinator", "producer", "consumer"] {
        let pid: i32 = fs::read_to_string(root.join("work").join(format!("{name}.pid")))?
            .trim()
            .parse()?;
        // SAFETY: read-only metadata query of a fixture-recorded member.
        let group = unsafe { libc::getpgid(pid) };
        result.push(json!({"name":name,"pid":pid,"group":group}));
    }
    Ok(json!(result))
}

fn denied(root: &Path, id: &str) -> Result<Value> {
    let accept = job::accept(root, id, None)
        .err()
        .ok_or("unsafe output acceptance")?;
    let reclaim = job::reclaim(root, id, None)
        .err()
        .ok_or("unsafe scratch reclamation")?;
    Ok(json!({"accept":accept.to_string(),"reclaim":reclaim.to_string()}))
}

struct Report {
    root: PathBuf,
    value: Value,
}
impl Drop for Report {
    fn drop(&mut self) {
        let Some(folder) = std::env::var_os("AN_AGENT_ACCEPTANCE_DIR") else {
            return;
        };
        // Capture even when an assertion fails, before the isolated TempDir drops.
        if let Ok(entries) = fs::read_dir(self.root.join("attempts")) {
            let mut snapshots = Vec::new();
            for entry in entries.flatten() {
                let mut files = serde_json::Map::new();
                for name in [
                    "plan.json",
                    "intent.json",
                    "receipt.json",
                    "accepted.json",
                    "stdout.txt",
                    "stderr.txt",
                ] {
                    if let Ok(bytes) = fs::read_to_string(entry.path().join(name)) {
                        files.insert(
                            name.into(),
                            serde_json::from_str(&bytes).unwrap_or(json!(bytes)),
                        );
                    }
                }
                snapshots
                    .push(json!({"attempt":entry.file_name().to_string_lossy(),"files":files}));
            }
            self.value["snapshots"] = json!(snapshots);
        }
        for name in ["reclaim-intent.json", "reclaimed.json"] {
            if let Ok(bytes) = fs::read_to_string(self.root.join(name)) {
                self.value[name] = serde_json::from_str(&bytes).unwrap_or(json!(bytes));
            }
        }
        let _ = fs::create_dir_all(&folder);
        let name = self
            .root
            .parent()
            .and_then(Path::file_name)
            .unwrap_or_default()
            .to_string_lossy();
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.value) {
            let _ = fs::write(
                Path::new(&folder).join(format!("multi-silk-{name}.json")),
                bytes,
            );
        }
    }
}

fn scenario(mode: &str) -> Result<()> {
    let temp = TempDir::new("two-silks");
    let (profile, root) = setup(&temp, mode)?;
    let mut report = Report {
        root: root.clone(),
        value: json!({"schema":"an-multi-silk-acceptance-v1","mode":mode,"passed":false}),
    };
    let stop = AtomicBool::new(false);
    let start = Instant::now();
    let receipt = if mode.starts_with("cancel-") {
        thread::scope(|scope| -> Result<Value> {
            let running =
                scope.spawn(|| run(&profile, &root, None, &stop).map_err(|e| e.to_string()));
            // On failed observations, request cancellation before joining the worker.
            let observed = (|| -> Result<()> {
                for name in ["producer.ready", "consumer.ready", "consumer.pid"] {
                    wait_for(&root, name)?;
                }
                if mode == "cancel-after" {
                    wait_for(&root, "output.txt")?;
                }
                let group = members(&root)?;
                let list = group.as_array().ok_or("member list")?;
                assert!(list.iter().all(|m| m["group"] == list[0]["pid"]));
                report.value["live_members"] = group;
                report.value["live_at_ms"] = json!(start.elapsed().as_millis());
                let id = fs::read_dir(root.join("attempts"))?
                    .next()
                    .ok_or("attempt")??
                    .file_name();
                report.value["live_denials"] = denied(&root, id.to_str().ok_or("ID")?)?;
                report.value["cancel_requested_at_ms"] = json!(start.elapsed().as_millis());
                Ok(())
            })();
            stop.store(true, Ordering::Relaxed);
            let result = running
                .join()
                .map_err(|_| "host worker panicked")?
                .map_err(std::io::Error::other)?;
            observed?;
            Ok(result)
        })?
    } else {
        run(&profile, &root, None, &stop)?
    };
    report.value["settled_at_ms"] = json!(start.elapsed().as_millis());
    report.value["receipt"] = receipt.clone();
    assert_eq!(receipt["termination_confirmed"], true, "{receipt}");
    assert_eq!(receipt["group_observation"]["empty"], true);
    let id = receipt["attempt"].as_str().ok_or("attempt")?;
    let pid = receipt["pid"].as_i64().ok_or("leader PID")?;
    // All explicitly named members must be gone, independent of the host flag.
    let recorded = members(&root)?;
    for member in recorded.as_array().ok_or("members")? {
        assert_eq!(member["group"], -1, "surviving silk: {member}");
    }
    report.value["settled_members"] = recorded;
    assert!(pid > 0);
    assert_eq!(receipt["automatic_retries"], 0);
    match mode {
        "success" => {
            assert_eq!(receipt["status"], "succeeded");
            assert_eq!(receipt["group_cleanup_required"], false);
            assert_eq!(
                fs::read(root.join("work/output.txt"))?,
                b"PRECISE INTERNAL HANDOFF\n"
            );
        }
        "orphan" => {
            assert_eq!(receipt["exit_code"], 0);
            assert_eq!(receipt["status"], "failed");
            assert_eq!(receipt["group_cleanup_required"], true);
        }
        "fail" => assert_eq!(receipt["status"], "failed"),
        "timeout" => assert_eq!(receipt["status"], "timed_out"),
        _ => assert_eq!(receipt["status"], "cancelled"),
    }
    // Independently qualify fail-closed record validation after the lock is gone.
    let file = root.join("attempts").join(id).join("receipt.json");
    let original = fs::read(&file)?;
    let mut refusals = Vec::new();
    for pointer in [
        "/termination_confirmed",
        "/reaped",
        "/group_observation/empty",
    ] {
        let mut uncertain = receipt.clone();
        *uncertain.pointer_mut(pointer).ok_or("receipt field")? = json!(false);
        fs::write(&file, serde_json::to_vec(&uncertain)?)?;
        refusals.push(json!({"field":pointer,"denials":denied(&root, id)?}));
        assert!(job::run(&profile, &root, Some(id), None, &AtomicBool::new(false)).is_err());
        assert!(root.join("work").exists());
    }
    if mode == "orphan" {
        let mut forged_success = receipt.clone();
        forged_success["status"] = json!("succeeded");
        fs::write(&file, serde_json::to_vec(&forged_success)?)?;
        refusals.push(
            json!({"field":"leader-success-with-forced-cleanup", "denials":denied(&root, id)?}),
        );
    }
    fs::write(&file, original)?;
    report.value["uncertain_denials"] = json!(refusals);
    if mode == "success" {
        let preview = job::accept(&root, id, None)?;
        let accepted = job::accept(&root, id, preview["approval_sha256"].as_str())?;
        let revision = accepted["outputs"][0]["binding"]["revision"]
            .as_str()
            .ok_or("revision")?;
        report.value["accepted"] = accepted.clone();
        // The accepted copy is independent of the former producer's mutable inode.
        fs::write(root.join("work/output.txt"), b"late scratch edit")?;
        let snapshot = temp.path().canonicalize()?.join("selected/pinned.txt");
        artifact_versions::checkout(&root.join("artifacts"), revision, &snapshot)?;
        assert_eq!(fs::read(&snapshot)?, b"PRECISE INTERNAL HANDOFF\n");
        let mut downstream: Value = serde_json::from_slice(&fs::read(&profile)?)?;
        downstream["args"] = json!(["-c", "cat \"$1\" > output.txt", "downstream", "{input:0}"]);
        downstream["inputs"] = json!([snapshot]);
        downstream["artifact_inputs"] =
            json!([{"path":snapshot,"catalog":root.join("artifacts"),"revision":revision}]);
        let downstream_profile = temp.path().canonicalize()?.join("downstream.json");
        fs::write(&downstream_profile, serde_json::to_vec(&downstream)?)?;
        let downstream_root = temp.path().canonicalize()?.join("downstream");
        let received = run(
            &downstream_profile,
            &downstream_root,
            None,
            &AtomicBool::new(false),
        )?;
        assert_eq!(received["status"], "succeeded");
        assert_eq!(
            fs::read(downstream_root.join("work/output.txt"))?,
            fs::read(&snapshot)?
        );
        report.value["downstream_plan"] = serde_json::from_slice(&fs::read(
            downstream_root
                .join("attempts")
                .join(received["attempt"].as_str().ok_or("downstream attempt")?)
                .join("plan.json"),
        )?)?;
        report.value["downstream"] = received;
        let reclaim = job::reclaim(&root, id, None)?;
        job::reclaim(&root, id, reclaim["approval_sha256"].as_str())?;
        let after = temp.path().canonicalize()?.join("after.txt");
        artifact_versions::checkout(&root.join("artifacts"), revision, &after)?;
        assert_eq!(fs::read(after)?, fs::read(snapshot)?);
    } else {
        assert!(job::accept(&root, id, None).is_err());
        assert!(!root.join("artifacts").exists());
        let reclaim = job::reclaim(&root, id, None)?;
        report.value["reclaim"] = job::reclaim(&root, id, reclaim["approval_sha256"].as_str())?;
    }
    assert!(!root.join("work").exists());
    report.value["elapsed_ms"] = json!(start.elapsed().as_millis());
    report.value["receipt_sha256"] = json!(format!("{:x}", Sha256::digest(fs::read(&file)?)));
    report.value["passed"] = json!(true);
    println!(
        "MULTI_SILK mode={mode} elapsed_ms={}",
        start.elapsed().as_millis()
    );
    Ok(())
}

#[test]
fn multi_silk_handoff_and_whole_round_cancellation() -> Result<()> {
    for mode in [
        "success",
        "cancel-before",
        "cancel-after",
        "orphan",
        "fail",
        "timeout",
    ] {
        scenario(mode)?;
    }
    Ok(())
}

#[test]
#[ignore = "bounded native concurrency soak; run explicitly on a qualified macOS host"]
fn multi_silk_soak() -> Result<()> {
    thread::scope(|scope| -> Result<()> {
        let workers: Vec<_> = (0..4)
            .map(|worker| {
                scope.spawn(move || -> std::result::Result<(), String> {
                    for round in 0..10 {
                        scenario(
                            ["success", "cancel-before", "cancel-after", "orphan", "fail"]
                                [(worker + round) % 5],
                        )
                        .map_err(|e| format!("worker={worker} round={round}: {e}"))?;
                    }
                    Ok(())
                })
            })
            .collect();
        for worker in workers {
            worker
                .join()
                .map_err(|_| "soak worker panicked")?
                .map_err(std::io::Error::other)?;
        }
        Ok(())
    })
}
