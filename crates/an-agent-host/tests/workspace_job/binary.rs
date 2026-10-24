//! Native byte custody uses arbitrary bytes; domain document parsing is not a host dependency.
use super::*;
use an_agent_host::artifact_versions as artifacts;
use sha2::{Digest, Sha256};
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn binary_profile(path: &Path) -> Result<()> {
    let mut p: Value = serde_json::from_slice(&fs::read(path)?)?;
    p["binary_outputs"] = json!(["output.txt"]);
    fs::write(path, serde_json::to_vec(&p)?)?;
    Ok(())
}
fn accept(root: &Path, receipt: &Value) -> Result<Value> {
    let id = receipt["attempt"].as_str().ok_or("attempt")?;
    let p = job::accept(root, id, None)?;
    job::accept(root, id, p["approval_sha256"].as_str())
}

#[test]
fn binary_edits_create_revisions_and_downstream_pins_one_even_after_later_edits() -> Result<()> {
    let t = TempDir::new("binary-workflow");
    let base = t.path().canonicalize()?;
    let (profile, root) = fixture(
        &t,
        "printf '\\120\\113\\003\\004\\377\\000' > output.txt",
        10,
    )?;
    binary_profile(&profile)?;
    let stop = AtomicBool::new(false);
    let receipt = run(&profile, &root, None, &stop)?;
    assert_eq!(receipt["status"], "succeeded");
    let accepted = accept(&root, &receipt)?;
    let catalog = root.join("artifacts");
    let first = accepted["outputs"][0]["binding"]["revision"]
        .as_str()
        .unwrap();
    let selected = base.join("selected");
    let edit = selected.join("editable.xlsx");
    artifacts::checkout(&catalog, first, &edit)?;
    fs::write(&edit, b"PK\xffversion-two")?;
    let v2 = artifacts::edit(&catalog, first, &edit, &sha(&fs::read(&edit)?))?;
    let second = v2["revision"].as_str().unwrap();
    let pinned = selected.join("pinned.xlsx");
    artifacts::checkout(&catalog, second, &pinned)?;
    fs::write(&edit, b"PK\xffversion-three")?;
    let v3 = artifacts::edit(&catalog, second, &edit, &sha(&fs::read(&edit)?))?;
    assert_eq!(v3["parent"], second);
    let mut p: Value = serde_json::from_slice(&fs::read(&profile)?)?;
    p["args"] = json!(["-c", "cat \"$1\" > output.txt", "fixture", pinned]);
    p["inputs"] = json!([pinned]);
    p["artifact_inputs"] = json!([{"path":pinned,"catalog":catalog,"revision":second}]);
    let mut exposed = p.clone();
    exposed["readonly"]
        .as_array_mut()
        .unwrap()
        .push(json!(catalog));
    fs::write(&profile, serde_json::to_vec(&exposed)?)?;
    assert!(job::run(&profile, &base.join("exposed"), None, None, &stop).is_err());
    assert!(!base.join("exposed").exists());
    fs::write(&profile, serde_json::to_vec(&p)?)?;
    let downstream = base.join("downstream");
    let preview = job::run(&profile, &downstream, None, None, &stop)?;
    fs::write(&pinned, b"substituted")?;
    assert!(
        job::run(
            &profile,
            &downstream,
            None,
            preview["approval_sha256"].as_str(),
            &stop
        )
        .is_err()
    );
    assert!(!downstream.exists());
    fs::remove_file(&pinned)?;
    artifacts::checkout(&catalog, second, &pinned)?;
    let result = run(&profile, &downstream, None, &stop)?;
    let output = accept(&downstream, &result)?;
    let revision = output["outputs"][0]["binding"]["revision"]
        .as_str()
        .unwrap();
    let destination = base.join("result.bin");
    let checked = artifacts::checkout(&downstream.join("artifacts"), revision, &destination)?;
    assert_eq!(fs::read(destination)?, b"PK\xffversion-two");
    assert_eq!(
        checked["binding"]["source"]["input_revisions"][0]["revision"],
        second
    );
    let reclaim = job::reclaim(&root, receipt["attempt"].as_str().unwrap(), None)?;
    job::reclaim(
        &root,
        receipt["attempt"].as_str().unwrap(),
        reclaim["approval_sha256"].as_str(),
    )?;
    artifacts::checkout(&catalog, first, &base.join("original.bin"))?;
    assert_eq!(fs::read(base.join("original.bin"))?, b"PK\x03\x04\xff\x00");
    Ok(())
}

#[test]
fn cancelled_binary_candidate_and_late_changes_cannot_be_accepted() -> Result<()> {
    let t = TempDir::new("binary-cancel");
    let (profile, root) = fixture(
        &t,
        "printf '\\377candidate' > output.txt; /bin/sleep 30",
        60,
    )?;
    binary_profile(&profile)?;
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker_root = root.clone();
    let worker = thread::spawn(move || {
        run(&profile, &worker_root, None, &worker_stop).map_err(|e| e.to_string())
    });
    let started = Instant::now();
    while !root.join("work/output.txt").exists() {
        assert!(started.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(10));
    }
    let id = fs::read_dir(root.join("attempts"))?
        .next()
        .ok_or("attempt")??
        .file_name()
        .into_string()
        .map_err(|_| "id")?;
    assert!(job::accept(&root, &id, None).is_err());
    stop.store(true, Ordering::SeqCst);
    let receipt = worker
        .join()
        .map_err(|_| "worker panicked")?
        .map_err(|e| format!("worker: {e}"))?;
    assert_eq!(receipt["status"], "cancelled");
    assert_eq!(receipt["termination_confirmed"], true);
    assert!(job::accept(&root, &id, None).is_err());
    fs::write(root.join("work/output.txt"), b"late binary")?;
    assert!(job::accept(&root, &id, None).is_err());
    assert!(!root.join("artifacts").exists());
    Ok(())
}
