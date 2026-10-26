//! A workspace-produced revision uses the existing kernel-admitted consumer.
use super::*;
use an_agent_host::workspace_job;
use std::sync::atomic::AtomicBool;

#[test]
fn accepted_workspace_version_feeds_existing_kit_before_and_after_collection()
-> execution::Result<()> {
    let f = Fixture::new()?;
    let selected = f.root.join("workflow-source");
    fs::create_dir(&selected)?;
    let script = selected.join("run.sh");
    fs::write(&script, b"printf 'alpha\\n' > result.txt\n")?;
    let profile = f.root.join("workspace.json");
    fs::write(
        &profile,
        serde_json::to_vec(&json!({"schema":"an-workspace-job-v2",
        "executable":"/bin/bash","args":["{input:0}"],"environment":{},"readonly":[],
        "inputs":[script],"outputs":["result.txt"],"wall_seconds":10,"trusted_process_group":true}))?,
    )?;
    let job = f.root.join("workspace");
    let stop = AtomicBool::new(false);
    let plan = workspace_job::run(&profile, &job, None, None, &stop)?;
    let result = workspace_job::run(
        &profile,
        &job,
        None,
        plan["approval_sha256"].as_str(),
        &stop,
    )?;
    assert_eq!(result["status"], "succeeded");
    let attempt = result["attempt"].as_str().ok_or("missing attempt")?;
    let plan = workspace_job::accept(&job, attempt, None)?;
    let accepted = workspace_job::accept(&job, attempt, plan["approval_sha256"].as_str())?;
    let revision = accepted["outputs"][0]["binding"]["revision"]
        .as_str()
        .ok_or("missing revision")?;
    for collect in [false, true] {
        if collect {
            let plan = workspace_job::reclaim(&job, attempt, None)?;
            workspace_job::reclaim(&job, attempt, plan["approval_sha256"].as_str())?;
        }
        let result = execution::run_revision(
            &f.root.join("store"),
            &f.release,
            &job.join("artifacts"),
            revision,
            true,
            &stop,
        )?;
        assert_eq!(result["status"], "succeeded");
        let receipt_path = Path::new(result["receipt"].as_str().ok_or("missing receipt")?);
        let run = receipt_path.parent().ok_or("missing run")?;
        let receipt: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
        assert_eq!(receipt["input_revision"]["revision"], revision);
        assert_eq!(fs::read(run.join("input.txt"))?, b"alpha\n");
        assert_eq!(fs::read(run.join("artifact.txt"))?, b"ALPHA\n");
        assert!(fs::read_to_string(run.join("audit.json"))?.contains("applied"));
    }
    Ok(())
}
