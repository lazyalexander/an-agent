//! Fully materialized large inputs through real native execution and custody.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    io::{Seek, SeekFrom, Write},
    os::unix::fs::MetadataExt,
};

fn large_source(path: &Path) -> Result<String> {
    let mut file = fs::File::create(path)?;
    let mut digest = Sha256::new();
    let mut block = vec![0u8; 1024 * 1024];
    for index in 0..64u64 {
        for (offset, chunk) in block.as_chunks_mut::<8>().0.iter_mut().enumerate() {
            chunk.copy_from_slice(&(index * 131071 + offset as u64).to_le_bytes());
        }
        file.write_all(&block)?;
        digest.update(&block);
    }
    file.sync_all()?;
    assert_eq!(file.metadata()?.len(), 64 * 1024 * 1024);
    Ok(format!("{:x}", digest.finalize()))
}

#[test]
fn large_snapshot_survives_source_mutation_cancel_and_collection() -> Result<()> {
    for mode in ["overwrite", "truncate", "replace", "unlink", "cancel"] {
        let temp = TempDir::new("large-clone-handoff");
        let (profile, root) = fixture(
            &temp,
            "printf ready > ready; while [ ! -f go ]; do /bin/sleep 0.01; done; /usr/bin/shasum -a 256 \"$1\" | /usr/bin/cut -d ' ' -f 1 > output.txt",
            30,
        )?;
        let source = temp.path().canonicalize()?.join("selected/input.txt");
        let expected = large_source(&source)?;
        let mut p: Value = serde_json::from_slice(&fs::read(&profile)?)?;
        p["args"][3] = json!("{input:0}");
        fs::write(&profile, serde_json::to_vec(&p)?)?;
        let stop = AtomicBool::new(false);
        let receipt = thread::scope(|scope| -> Result<Value> {
            let running =
                scope.spawn(|| run(&profile, &root, None, &stop).map_err(|e| e.to_string()));
            let start = Instant::now();
            while !root.join("work/ready").exists() {
                // Admission hashes and snapshot preparation precede the workload's
                // independent 30-second execution deadline. Keep a bounded CI
                // preparation budget without treating it as a launch SLA.
                if running.is_finished() || start.elapsed() >= Duration::from_secs(120) {
                    stop.store(true, Ordering::Release);
                    let result = running.join().map_err(|_| "host worker panicked")?;
                    return Err(format!(
                        "case {mode} never became ready after {:?}; host result: {result:?}",
                        start.elapsed()
                    )
                    .into());
                }
                thread::sleep(Duration::from_millis(10));
            }
            eprintln!(
                "CLONE_HANDOFF_READY={}",
                json!({"case":mode,
                "bytes":64*1024*1024,"admission_and_snapshot_ms":start.elapsed().as_millis()})
            );
            let attempt = fs::read_dir(root.join("attempts"))?.next().unwrap()?.path();
            let snapshot = attempt.join("snapshot/inputs/input.txt");
            assert_ne!(fs::metadata(&source)?.ino(), fs::metadata(&snapshot)?.ino());
            assert_eq!(fs::metadata(&snapshot)?.nlink(), 1);
            let id = attempt.file_name().unwrap().to_str().unwrap();
            assert!(job::reclaim(&root, id, None).is_err());
            match mode {
                "truncate" => {
                    fs::OpenOptions::new()
                        .write(true)
                        .open(&source)?
                        .set_len(4096)?;
                }
                "replace" => {
                    let replacement = source.with_extension("new");
                    fs::write(&replacement, b"different inode")?;
                    fs::rename(replacement, &source)?;
                }
                "unlink" => fs::remove_file(&source)?,
                _ => {
                    let mut file = fs::OpenOptions::new().write(true).open(&source)?;
                    file.seek(SeekFrom::Start(32 * 1024 * 1024))?;
                    file.write_all(&[0xff; 4096])?;
                    file.sync_all()?;
                }
            }
            assert_eq!(
                format!("{:x}", Sha256::digest(fs::read(&snapshot)?)),
                expected
            );
            if mode == "cancel" {
                stop.store(true, Ordering::Release);
            } else {
                fs::write(root.join("work/go"), b"go")?;
            }
            Ok(running.join().unwrap().map_err(std::io::Error::other)?)
        })?;
        let id = receipt["attempt"].as_str().unwrap();
        let attempt = root.join("attempts").join(id);
        assert_eq!(receipt["termination_confirmed"], true);
        let mut revision = None;
        if mode == "cancel" {
            assert_eq!(receipt["status"], "cancelled");
            assert!(job::accept(&root, id, None).is_err());
            // Unknown termination must retain scratch and input evidence.
            let path = attempt.join("receipt.json");
            let bytes = fs::read(&path)?;
            let mut unknown = receipt.clone();
            unknown["termination_confirmed"] = json!(false);
            fs::write(&path, serde_json::to_vec(&unknown)?)?;
            assert!(job::reclaim(&root, id, None).is_err());
            assert!(root.join("work").exists());
            fs::write(path, bytes)?;
        } else {
            assert_eq!(receipt["status"], "succeeded");
            assert_eq!(
                fs::read_to_string(root.join("work/output.txt"))?.trim(),
                expected
            );
            let preview = job::accept(&root, id, None)?;
            let accepted = job::accept(&root, id, preview["approval_sha256"].as_str())?;
            revision = Some(
                accepted["outputs"][0]["binding"]["revision"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
        }
        let reclaim = job::reclaim(&root, id, None)?;
        job::reclaim(&root, id, reclaim["approval_sha256"].as_str())?;
        assert!(!root.join("work").exists());
        // Existing retention contract keeps snapshots as attempt evidence.
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(fs::read(attempt.join("snapshot/inputs/input.txt"))?)
            ),
            expected
        );
        if let Some(revision) = revision {
            let checkout = temp.path().canonicalize()?.join("accepted.txt");
            an_agent_host::artifact_versions::checkout(
                &root.join("artifacts"),
                &revision,
                &checkout,
            )?;
            assert_eq!(fs::read_to_string(checkout)?.trim(), expected);
        }
        eprintln!(
            "CLONE_HANDOFF_CASE={}",
            json!({"case":mode,"bytes":64*1024*1024,"receipt":receipt,"reclaim":true,"snapshot_retained":true})
        );
    }
    Ok(())
}
