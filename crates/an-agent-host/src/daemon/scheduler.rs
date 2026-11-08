//! Bounded FIFO dispatch. Claimed work is observed after restart, never replayed.
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    process::Child,
};

pub(super) fn enqueue(root: &Path, job: &Job) -> Result<String> {
    if storage::entries(root, "jobs", 256)?.len() >= 256 {
        return Err("daemon task registry full".into());
    }
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let dir = root.join("jobs").join(&id);
    store::directory(&dir)?;
    // Submission order is monotonic within this bounded, append-only registry.
    let order = storage::entries(root, "jobs", 256)?.len();
    write(&dir.join("order.json"), &json!(order))?;
    write(&dir.join("request.json"), job)?;
    Ok(id)
}

pub(super) fn cancel(job: &Job) -> Result<()> {
    if let Job::Spool { instance, run, .. } = job {
        let preview = spool::supervisor::cancel(instance, run, None)?;
        spool::supervisor::cancel(instance, run, Some(text(&preview, "approval_sha256")?))?;
    }
    Ok(())
}

pub(super) fn result(job: &Job, dir: &Path) -> Result<Value> {
    match job {
        Job::Spool {
            instance,
            run,
            request,
        } => spool::supervisor::completion(instance, run, request),
        Job::Harness { request, .. } => harness::completion(dir, request),
    }
}

pub(super) fn tick(
    root: &Path,
    children: &mut BTreeMap<PathBuf, Child>,
    settled: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let mut reaped = Vec::new();
    for (path, child) in children.iter_mut() {
        if child.try_wait()?.is_some() {
            reaped.push(path.clone());
        }
    }
    for path in reaped {
        children.remove(&path);
    }
    let mut jobs = Vec::new();
    for dir in storage::entries(root, "jobs", 256)? {
        let job: Job = serde_json::from_value(read(&dir.join("request.json"))?)?;
        jobs.push((
            read(&dir.join("order.json"))?
                .as_u64()
                .ok_or("queue order")?,
            dir,
            job,
        ));
    }
    jobs.sort_by_key(|(order, _, _)| *order);
    let mut active = 0;
    let mut instances = Vec::new();
    for (_, dir, job) in &jobs {
        if dir.join("claimed.json").exists() && !settled.contains(dir) {
            if result(job, dir).is_ok() {
                settled.insert(dir.clone());
                continue;
            }
            active += 1;
            if let Job::Spool { instance, .. } = job {
                instances.push(instance.clone());
            }
        }
    }
    let limit = storage::config(root)?.concurrency;
    for (_, dir, job) in jobs {
        if active >= limit {
            break;
        }
        if dir.join("claimed.json").exists() {
            continue;
        }
        if let Job::Spool { instance, .. } = &job {
            if instances.contains(instance) {
                continue;
            }
            let view = storage::instance(root, instance)?;
            if view["loaded"] != true {
                continue;
            }
        }
        // A crash in the spawn window remains uncertain and consumes a slot.
        write(&dir.join("claimed.json"), &json!({"request":job.request()}))?;
        let launch = match &job {
            Job::Spool {
                instance,
                run,
                request,
            } => {
                if dir.join("cancel.json").exists() {
                    cancel(&job)?;
                }
                spool::supervisor::launch_background(instance, run, request)
            }
            Job::Harness { request, .. } => crate::execution::supervisor_process::spawn_harness(
                &storage::config(root)?.executable,
                root,
                dir.file_name().and_then(|s| s.to_str()).ok_or("task ID")?,
                request,
                &dir,
            ),
        };
        match launch {
            Ok(child) => {
                children.insert(dir, child);
            }
            Err(error) => {
                write(
                    &dir.join("launch-error.json"),
                    &json!({"diagnostic":error.to_string(),"automatic_retries":0}),
                )?;
            }
        }
        if let Job::Spool { instance, .. } = job {
            instances.push(instance);
        }
        active += 1;
    }
    Ok(())
}
