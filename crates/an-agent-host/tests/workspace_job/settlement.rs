//! Older receipts are readable evidence, never implicit termination authority.
use super::*;

fn denied(profile: &Path, root: &Path, id: &str, digest: &str) -> Result<()> {
    for approved in [None, Some(digest)] {
        assert!(job::accept(root, id, approved).is_err());
        assert!(job::reclaim(root, id, approved).is_err());
        assert!(job::run(profile, root, Some(id), approved, &AtomicBool::new(false)).is_err());
    }
    let dir = root.join("attempts").join(id);
    assert!(!dir.join("successor.json").exists());
    assert!(!dir.join("accept-intent.json").exists());
    assert!(!root.join("reclaim-intent.json").exists());
    assert!(!root.join("artifacts").exists());
    assert_eq!(fs::read(root.join("work/output.txt"))?, b"candidate");
    assert_eq!(fs::read_dir(root.join("attempts"))?.count(), 1);
    Ok(())
}

#[test]
fn old_receipts_cannot_authorize_accept_resume_or_reclaim() -> Result<()> {
    let temp = TempDir::new("old-workspace-receipt");
    let (profile, root) = fixture(&temp, "printf candidate > output.txt", 10)?;
    let receipt = run(&profile, &root, None, &AtomicBool::new(false))?;
    assert_eq!(receipt["status"], "succeeded");
    let id = receipt["attempt"].as_str().ok_or("attempt")?;
    let file = root.join("attempts").join(id).join("receipt.json");
    let original = fs::read(&file)?;
    let preview = job::accept(&root, id, None)?;
    let digest = preview["approval_sha256"].as_str().ok_or("approval")?;
    for field in [
        "termination_confirmed",
        "reaped",
        "pid",
        "group_observation",
        "group_cleanup_required",
    ] {
        for null in [false, true] {
            let mut old = receipt.clone();
            if null {
                old[field] = Value::Null;
            } else {
                old.as_object_mut().unwrap().remove(field);
            }
            fs::write(&file, serde_json::to_vec(&old)?)?;
            denied(&profile, &root, id, digest)?;
        }
    }
    for field in ["group", "empty"] {
        let mut old = receipt.clone();
        old["group_observation"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        fs::write(&file, serde_json::to_vec(&old)?)?;
        denied(&profile, &root, id, digest)?;
    }
    // Failed/cancelled legacy records also lack permission to delete or resume.
    for status in ["failed", "cancelled", "timed_out"] {
        let mut old = receipt.clone();
        old["status"] = json!(status);
        old.as_object_mut().unwrap().remove("group_observation");
        fs::write(&file, serde_json::to_vec(&old)?)?;
        denied(&profile, &root, id, digest)?;
    }
    fs::write(&file, &original)?;
    assert_eq!(job::accept(&root, id, None)?, preview);
    let accepted = job::accept(&root, id, Some(digest))?;
    assert_eq!(accepted["status"], "succeeded");
    let reclaim = job::reclaim(&root, id, None)?;
    let continuation = job::run(&profile, &root, Some(id), None, &AtomicBool::new(false))?;
    for field in ["group_observation", "group_cleanup_required"] {
        let mut old = receipt.clone();
        old.as_object_mut().unwrap().remove(field);
        fs::write(&file, serde_json::to_vec(&old)?)?;
        // A real accepted product exists: rejection cannot be explained by a
        // missing accepted.json, and an old approval cannot bypass the gate.
        for approved in [None, reclaim["approval_sha256"].as_str()] {
            assert!(job::reclaim(&root, id, approved).is_err());
        }
        for approved in [None, continuation["approval_sha256"].as_str()] {
            assert!(
                job::run(&profile, &root, Some(id), approved, &AtomicBool::new(false)).is_err()
            );
        }
        assert!(!root.join("reclaim-intent.json").exists());
        assert!(
            !root
                .join("attempts")
                .join(id)
                .join("successor.json")
                .exists()
        );
        assert_eq!(fs::read(root.join("work/output.txt"))?, b"candidate");
        let revision = accepted["outputs"][0]["binding"]["revision"]
            .as_str()
            .ok_or("revision")?;
        let checkout = temp
            .path()
            .canonicalize()?
            .join(format!("retained-{field}.txt"));
        an_agent_host::artifact_versions::checkout(&root.join("artifacts"), revision, &checkout)?;
        assert_eq!(fs::read(checkout)?, b"candidate");
    }
    fs::write(&file, &original)?;
    assert_eq!(job::reclaim(&root, id, None)?, reclaim);
    assert_eq!(fs::read(&file)?, original);
    Ok(())
}
