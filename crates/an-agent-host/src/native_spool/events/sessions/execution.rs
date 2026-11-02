//! Session admission fences precede dispatch; interruptions never replay a thread.
use super::*;
use std::sync::atomic::Ordering;

pub fn run(
    workspace: &Path,
    name: &str,
    thread: &str,
    request: &Path,
    approved: Option<&str>,
    external_cancel: &AtomicBool,
) -> Result<Value> {
    prepare(
        workspace,
        name,
        thread,
        request,
        approved,
        external_cancel,
        None,
    )
}

pub fn run_supervised(
    workspace: &Path,
    name: &str,
    thread: &str,
    request: &Path,
    approved: Option<&str>,
    cancel: &AtomicBool,
    exe: &Path,
) -> Result<Value> {
    prepare(
        workspace,
        name,
        thread,
        request,
        approved,
        cancel,
        Some(exe),
    )
}

fn prepare(
    workspace: &Path,
    name: &str,
    thread: &str,
    request: &Path,
    approved: Option<&str>,
    external_cancel: &AtomicBool,
    exe: Option<&Path>,
) -> Result<Value> {
    native::bundle::identifier(thread)?;
    let (space, root, record) = open(workspace, name)?;
    let dir = root.join("threads").join(thread);
    store::no_links(&dir)?;
    if dir.exists() {
        return Err("thread already admitted or uncertain; inspect, never replay".into());
    }
    let prior = control(&root)?;
    if prior["mode"] != "open" {
        return Err("session admission is not open".into());
    }
    let mut selected = dispatch::Request::read(request)?;
    selected.name = hash(&json!({"session":record,"thread":thread}))?;
    let bytes = serde_json::to_vec(&selected)?;
    let stop = AtomicBool::new(false);
    let cancel = AtomicBool::new(false);
    let preview = dispatch::execute_request(
        workspace,
        selected,
        None,
        &stop,
        &cancel,
        coordinator::Driver::inside(exe, name, thread),
        &|| {},
    )?;
    if preview.get("plan").is_none() {
        return Err("dispatch already exists; no new ownership".into());
    }
    let mut plan = json!({"operation":"session-thread-run","session":record,"thread":thread,
        "control":prior,"dispatch":preview,"request_sha256":store::digest(&bytes)});
    let scope = coordinator::Scope::Thread(name, thread);
    if let Some(exe) = exe {
        plan["supervision"] = scope.descriptor(
            &space.root,
            &crate::execution::supervisor_process::host_executable(exe)?,
        )?;
    }
    let (digest, execute) = native::approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    {
        // Never hold this lock during native execution or observation.
        let _lock = store::lock(&root)?;
        if control(&root)? != prior {
            return Err("session generation changed".into());
        }
        if external_cancel.load(Ordering::Acquire) {
            return Err("cancelled before admission".into());
        }
        if threads(&root)?.len() >= 32 {
            return Err("session thread limit reached".into());
        }
        fs::create_dir(&dir)?;
        fs::File::open(root.join("threads"))?.sync_all()?;
        store::write_new(&dir.join("request.json"), &bytes)?;
        write_json(
            &dir.join("intent.json"),
            &json!({"plan":plan,"approval_sha256":digest}),
        )?;
        fs::File::open(&dir)?.sync_all()?;
    }
    if let Some(exe) = exe {
        return coordinator::launch(&space.root, scope, &digest, exe, &stop, external_cancel);
    }
    run_thread(&space.root, &dir, &plan, &stop, external_cancel, None)
}

pub(in crate::native_spool::events) fn run_thread(
    workspace: &Path,
    dir: &Path,
    plan: &Value,
    external_stop: &AtomicBool,
    external_cancel: &AtomicBool,
    exe: Option<&Path>,
) -> Result<Value> {
    let name = text(&plan["session"], "name")?;
    let (_, root, record) = open(workspace, name)?;
    let thread = text(plan, "thread")?;
    if record != plan["session"]
        || dir != root.join("threads").join(thread)
        || store::digest(&regular(&dir.join("request.json"), 65536)?) != plan["request_sha256"]
    {
        return Err("session thread does not bind original request".into());
    }
    let digest = hash(plan)?;
    let prior = &plan["control"];
    let preview = &plan["dispatch"];
    let stop = AtomicBool::new(external_stop.load(Ordering::Acquire));
    let cancel = AtomicBool::new(external_cancel.load(Ordering::Acquire));
    let poll = || {
        if exe.is_some() {
            native::supervisor::poll_control(&dir.join("supervisor"), Some(&stop), &cancel);
        }

        if external_stop.load(Ordering::Acquire) {
            stop.store(true, Ordering::Release);
        }
        if external_cancel.load(Ordering::Acquire) {
            cancel.store(true, Ordering::Release);
        }
        match control(&root) {
            Ok(value) => {
                if &value != prior {
                    stop.store(true, Ordering::Release);
                }
                if value["mode"] == "cancelled" {
                    cancel.store(true, Ordering::Release);
                }
            }
            Err(_) => cancel.store(true, Ordering::Release),
        }
    };
    let result = native::supervisor::monitor(poll, || {
        dispatch::execute_request(
            workspace,
            dispatch::Request::read(&dir.join("request.json"))?,
            preview["approval_sha256"].as_str(),
            &stop,
            &cancel,
            coordinator::Driver::inside(exe, name, thread),
            &poll,
        )
    });
    // A lost result stays uncertain. It is not permission for a new attempt.
    let value = match result {
        Ok(value) if value["schema"] == "an-native-event-dispatch-observation-v1" => {
            json!({"status":"uncertain","diagnostic":"dispatch already exists; cannot claim another admission"})
        }
        Ok(value) => value,
        Err(error) => json!({"status":"uncertain","diagnostic":error.to_string()}),
    };
    write_json(
        &dir.join("result.json"),
        &json!({"approval_sha256":digest,"result":value}),
    )?;
    fs::File::open(dir)?.sync_all()?;
    Ok(
        json!({"session":record,"thread":thread,"status":value["status"],"result":value,"automatic_retries":0}),
    )
}
