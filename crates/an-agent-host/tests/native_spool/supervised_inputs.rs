//! Deterministic reservation-to-worker fault windows; CLI loss is qualified in Python.
use super::*;
use spool::supervisor::{self, Input};
use std::sync::Arc;

#[test]
fn supervised_selection_survives_selector_removal_but_not_input_retirement() -> Result<()> {
    let temp = TempDir::new("supervised-data-state");
    let base = temp.path().canonicalize()?;
    let source_release = package(&base)?;
    let source = base.join("source");
    spool::load(&source_release, &source)?;
    let seed = base.join("seed.txt");
    fs::write(&seed, b"seed\n")?;
    run(&source, "data", &seed, &AtomicBool::new(false))?;
    accept(&source, "data")?;
    let data = spool::artifacts(&source, "data")?;

    let project = base.join("target-project");
    fs::create_dir(&project)?;
    let release = package(&project)?;
    let code = "set -eu\ncat \"$AN_SPOOL_INPUT\" \"$AN_SPOOL_STATE\" > report.txt\n";
    fs::write(release.join("silk/main.sh"), code)?;
    let manifest_path = release.join("native-spool.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    manifest["schema"] = json!("an-native-spool-v3");
    manifest["events"] = json!({"consumes":[], "produces":["artifact.accepted"]});
    manifest["files"]["silk/main.sh"] = json!(format!("{:x}", Sha256::digest(code)));
    manifest["requirements"] = json!({"runtime":"macos-native-process-v1", "os":"macos",
        "arch":"portable-source", "interaction":"headless", "gpu":"none",
        "inputs":["input.txt"], "state_input":"checkpoint.txt", "editable":[],
        "capabilities":["read-input","write-declared-outputs"], "dependencies":[]});
    fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;
    let root = base.join("target");
    spool::load(&release, &root)?;
    let preview = spool::checkpoint::init(&root, "report.txt", &seed, None)?;
    let checkpoint = spool::checkpoint::init(
        &root,
        "report.txt",
        &seed,
        preview["approval_sha256"].as_str(),
    )?;
    let selection = base.join("selection.json");
    let request = json!({"schema":"an-native-data-state-v1",
        "data":{"catalog":data["catalog"],"revision":data["outputs"][0]["binding"]["revision"]},
        "state_revision":checkpoint["state"]["revision"]});
    let executable = std::env::current_exe()?;
    for name in ["selected", "retired"] {
        fs::write(&selection, serde_json::to_vec(&request)?)?;
        let preview = supervisor::prepare(
            &root,
            name,
            Input::DataState(&selection),
            15,
            &executable,
            None,
        )?;
        let hash = preview["approval_sha256"].as_str().ok_or("approval")?;
        supervisor::prepare(
            &root,
            name,
            Input::DataState(&selection),
            15,
            &executable,
            Some(hash),
        )?;
        // The real worker cannot reread this path or accidentally follow a new selection.
        fs::remove_file(&selection)?;
        if name == "retired" {
            let retire = spool::retire_artifact(&source, "data", "report.txt", None)?;
            spool::retire_artifact(
                &source,
                "data",
                "report.txt",
                retire["approval_sha256"].as_str(),
            )?;
            let error = supervisor::execute(&root, name, hash, Arc::new(AtomicBool::new(false)))
                .expect_err("retired inputs must refuse before execution");
            assert!(error.to_string().contains("artifact revision is retired"));
            assert!(!root.join("runs/retired/job").exists());
            assert!(
                supervisor::execute(&root, name, hash, Arc::new(AtomicBool::new(false))).is_err()
            );
            assert!(reclaim(&root, name).is_err());
            assert!(accept(&root, name).is_err());
        } else {
            let result = supervisor::execute(&root, name, hash, Arc::new(AtomicBool::new(false)))?;
            assert_eq!(result["status"], "succeeded");
            assert_eq!(result["workspace"]["termination_confirmed"], true);
            assert_eq!(
                fs::read_to_string(root.join("runs/selected/job/work/report.txt"))?,
                "Report\nSEED\nseed\n"
            );
            assert_eq!(supervisor::completion(&root, name, hash)?, result);
            accept(&root, name)?;
            reclaim(&root, name)?;
        }
    }
    Ok(())
}
