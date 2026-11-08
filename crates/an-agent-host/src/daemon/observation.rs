//! Read-only projections. No PID observation can manufacture settlement evidence.
use super::*;

pub(super) fn snapshot(root: &Path) -> Result<Value> {
    let mut instances = Vec::new();
    for path in storage::entries(root, "instances", 128)? {
        let registration = read(&path)?;
        let path = Path::new(text(&registration, "path")?);
        let view = storage::instance(root, path).unwrap_or_else(|e| json!({"error":e.to_string()}));
        instances.push(json!({"registration":registration,"view":view}));
    }
    let mut jobs = Vec::new();
    for dir in storage::entries(root, "jobs", 256)? {
        let job: Job = serde_json::from_value(read(&dir.join("request.json"))?)?;
        let claimed = dir.join("claimed.json").exists();
        let result = scheduler::result(&job, &dir);
        let mut row = json!({"id":dir.file_name().and_then(|s|s.to_str()),
            "order":read(&dir.join("order.json"))?,"claimed":claimed,
            "cancellation_requested":dir.join("cancel.json").exists(),"request":job.request(),
            "state":if result.is_ok(){"settled"}else if claimed{"running-or-unconfirmed"}else{"queued"}});
        match &job {
            Job::Spool { instance, run, .. } => {
                row["kind"] = json!("spool");
                row["instance"] = json!(instance);
                row["run"] = json!(run);
                row["native"] =
                    spool::status(instance, run).unwrap_or_else(|e| json!({"error":e.to_string()}));
                row["resources"] = resources(
                    &instance.join("runs").join(run).join("supervisor"),
                    job.request(),
                );
            }
            Job::Harness { task, .. } => {
                row["kind"] = json!("harness");
                row["label"] = json!(task.label);
                row["resources"] = resources(&dir, job.request());
            }
        }
        match result {
            Ok(value) => {
                row["outcome"] = value["status"].clone();
            }
            Err(error) if claimed => {
                row["diagnostic"] = json!(error.to_string());
            }
            _ => (),
        }
        jobs.push(row);
    }
    jobs.sort_by_key(|row| row["order"].as_u64());
    Ok(
        json!({"schema":"an-user-daemon-view-v1","instances":instances,"tasks":jobs,
        "concurrency":storage::config(root)?.concurrency,"automatic_retries":0,
        "authority":"projection of original lifecycle and execution evidence",
        "service_resources":crate::execution::sandbox::service_process::current_resources()}),
    )
}

fn resources(dir: &Path, request: &str) -> Value {
    match read(&dir.join("owner.json")) {
        Ok(owner) if owner["request"] == request => {
            crate::execution::sandbox::service_process::owned_resources(&owner["process_identity"])
        }
        _ => {
            json!({"available":false,"scope":"no original process identity; usage unknown, not zero"})
        }
    }
}
