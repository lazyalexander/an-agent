//! Explicit controls: delegated approvals are checked again by the owning facade.
use super::*;

pub(super) fn dispatch(root: &Path, req: &Value) -> Result<Value> {
    let approved = req
        .get("approve")
        .map(|_| text(req, "approve"))
        .transpose()?;
    match text(req, "op")? {
        "delegate" | "update-delegation" | "delegation-inbox" => {
            delegation::dispatch(root, req, approved)
        }
        "record-adoption" => activity::adopt(root, req, approved),
        "activity" => activity::view(root, text(req, "id")?),
        "show" => observation::snapshot(root),
        "task" => observation::task(root, text(req, "id")?),
        "register" => storage::register(root, Path::new(text(req, "instance")?)),
        "submit" => submit(root, req, approved),
        "harness" => harness::submit(root, req, approved),
        "harness-result" => {
            let (dir, job) = storage::job(root, text(req, "id")?)?;
            if !matches!(job, Job::Harness { .. }) {
                return Err("not a harness task".into());
            }
            let result = scheduler::result(&job, &dir)?;
            if result["status"] != "succeeded" || result["proposal_available"] != true {
                return Err("harness produced no completed proposal".into());
            }
            Ok(json!({"result":result,"proposal":read(&dir.join("decision.json"))?}))
        }
        "cancel" => {
            let (dir, job) = storage::job(root, text(req, "id")?)?;
            let (preview, execute) = approval(
                json!({"operation":"cancel-daemon-task", "root":root,
                "id":text(req,"id")?,"request":job.request()}),
                approved,
            )?;
            if !execute {
                return Ok(preview);
            }
            if !dir.join("cancel.json").exists() {
                write(&dir.join("cancel.json"), &preview)?;
            }
            scheduler::cancel(&job)?;
            Ok(json!({"cancellation_requested":true,"termination_confirmed":false}))
        }
        "pause" | "resume" | "suspend" | "unload" | "accept" | "reclaim" => {
            let path = Path::new(text(req, "instance")?);
            storage::instance(root, path)?;
            match text(req, "op")? {
                "pause" => spool::pause(path, approved),
                "resume" => spool::resume(path, approved),
                "suspend" => spool::suspend(path, text(req, "revision")?, approved),
                "unload" => spool::unload(path, approved),
                "accept" => spool::accept(path, text(req, "run")?, approved),
                _ => spool::reclaim(path, text(req, "run")?, approved),
            }
        }
        _ => Err("unsupported daemon operation".into()),
    }
}

fn submit(root: &Path, req: &Value, approved: Option<&str>) -> Result<Value> {
    let instance = Path::new(text(req, "instance")?);
    let run = text(req, "run")?;
    storage::instance(root, instance)?;
    let jobs = storage::entries(root, "jobs", 256)?;
    if jobs.len() >= 256 {
        return Err("daemon task registry full".into());
    }
    for path in jobs {
        let existing: Job = serde_json::from_value(read(&path.join("request.json"))?)?;
        if let Job::Spool {
            instance: selected,
            run: name,
            ..
        } = existing
            && selected == instance.canonicalize()?
            && name == run
        {
            return Err("task already submitted; inspect its original ID, never replay".into());
        }
    }
    let input = Path::new(text(req, "input")?);
    let seconds = req["seconds"].as_u64().ok_or("seconds")?;
    let executable = storage::config(root)?.executable;
    // Reuse the native preview/approval once per request. Rebuilding it twice
    // re-hashes the selected runtime inventory without adding authority.
    let prepared = spool::supervisor::prepare(
        instance,
        run,
        spool::supervisor::Input::File(input),
        seconds,
        &executable,
        approved,
    )?;
    if prepared["reserved"] != true {
        return Ok(prepared);
    }
    let job = Job::Spool {
        instance: instance.canonicalize()?,
        run: run.into(),
        request: text(&prepared, "approval_sha256")?.into(),
    };
    let id = scheduler::enqueue(root, &job)?;
    Ok(json!({"id":id,"queued":true,"executed":false,"request":job.request()}))
}
