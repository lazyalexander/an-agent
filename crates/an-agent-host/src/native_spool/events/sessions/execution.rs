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
    native::bundle::identifier(thread)?;
    let (_space, root, record) = open(workspace, name)?;
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
    let preview = dispatch::execute_request(workspace, selected, None, &stop, &cancel)?;
    if preview.get("plan").is_none() {
        return Err("dispatch already exists; no new ownership".into());
    }
    let plan = json!({"operation":"session-thread-run","session":record,"thread":thread,
        "control":prior,"dispatch":preview,"request_sha256":store::digest(&bytes)});
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
    let finished = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !finished.load(Ordering::Acquire) {
                if external_cancel.load(Ordering::Acquire) {
                    cancel.store(true, Ordering::Release);
                }
                match control(&root) {
                    Ok(value) => {
                        if value != prior {
                            stop.store(true, Ordering::Release);
                        }
                        if value["mode"] == "cancelled" {
                            cancel.store(true, Ordering::Release);
                        }
                    }
                    Err(_) => {
                        cancel.store(true, Ordering::Release);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        });
        let result = (|| {
            let current = control(&root)?;
            if current != prior {
                stop.store(true, Ordering::Release);
            }
            if current["mode"] == "cancelled" || external_cancel.load(Ordering::Acquire) {
                cancel.store(true, Ordering::Release);
            }
            dispatch::execute_request(
                workspace,
                dispatch::Request::read(&dir.join("request.json"))?,
                preview["approval_sha256"].as_str(),
                &stop,
                &cancel,
            )
        })();
        finished.store(true, Ordering::Release);
        result
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
    fs::File::open(&dir)?.sync_all()?;
    Ok(
        json!({"session":record,"thread":thread,"status":value["status"],"result":value,"automatic_retries":0}),
    )
}
