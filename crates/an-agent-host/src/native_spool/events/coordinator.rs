//! One controller per approved finite dispatch or session thread, with one supervisor per admitted run.
use super::*;
mod parent;
pub(in crate::native_spool) use parent::ParentCancel;
pub(super) use parent::verify_parent;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy)]
pub(super) enum Driver<'a> {
    Embedded,
    Detached(&'a Path),
    WithinThread(&'a Path, &'a str, &'a str),
}
impl<'a> Driver<'a> {
    pub(super) fn inside(exe: Option<&'a Path>, name: &'a str, thread: &'a str) -> Self {
        exe.map_or(Self::Embedded, |exe| Self::WithinThread(exe, name, thread))
    }
    pub(super) fn event_owner(self, dispatch: &str) -> Option<delivery::Owner<'a>> {
        let (executable, parent) = match self {
            Self::Embedded => return None,
            Self::Detached(exe) => (exe, json!({"dispatch":dispatch})),
            Self::WithinThread(exe, session, thread) => (
                exe,
                json!({"dispatch":dispatch,"session":session,"thread":thread}),
            ),
        };
        Some(delivery::Owner {
            executable,
            parent: Some(parent),
        })
    }
}

#[derive(Clone, Copy)]
pub enum Scope<'a> {
    Dispatch(&'a str),
    Thread(&'a str, &'a str),
}
impl<'a> Scope<'a> {
    fn names(self) -> (&'a str, Option<&'a str>) {
        match self {
            Self::Dispatch(name) => (name, None),
            Self::Thread(name, thread) => (name, Some(thread)),
        }
    }
    fn directory(self, root: &Path) -> Result<PathBuf> {
        let (name, thread) = self.names();
        native::bundle::identifier(name)?;
        let dir = match thread {
            Some(t) => {
                native::bundle::identifier(t)?;
                root.join("sessions").join(name).join("threads").join(t)
            }
            None => root.join("dispatches").join(name),
        };
        store::no_links(&dir)?;
        Ok(dir)
    }
    pub(super) fn descriptor(self, root: &Path, exe: &Path) -> Result<Value> {
        let (name, thread) = self.names();
        Ok(
            json!({"schema":"an-native-finite-supervisor-v1","workspace":root,"name":name,"thread":thread,"executable":exe}),
        )
    }
}

pub(super) fn read(file: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&regular(file, 8 * 1024 * 1024)?)?)
}
fn original(path: &Path, scope: Scope<'_>) -> Result<(Space, PathBuf, Value)> {
    let space = Space::observe(path)?;
    let dir = scope.directory(&space.root)?;
    let record = read(&dir.join("intent.json"))?;
    let plan = &record["plan"];
    let exe = Path::new(text(&plan["supervision"], "executable")?);
    if plan["supervision"] != scope.descriptor(&space.root, exe)?
        || record["approval_sha256"] != hash(plan)?
    {
        return Err("finite supervisor admission identity mismatch".into());
    }
    let matches = match scope {
        Scope::Dispatch(name) => {
            record["schema"] == "an-native-event-dispatch-intent-v1"
                && plan["schema"] == "an-native-event-dispatch-plan-v1"
                && plan["workspace"] == space.id
                && plan["request"]["name"] == name
        }
        Scope::Thread(name, thread) => {
            plan["operation"] == "session-thread-run"
                && plan["session"]["workspace"] == space.id
                && plan["session"]["name"] == name
                && plan["thread"] == thread
        }
    };
    if !matches {
        return Err("finite supervisor scope mismatch".into());
    }
    Ok((space, dir, record))
}

pub(super) fn launch(
    path: &Path,
    scope: Scope<'_>,
    grant: &str,
    exe: &Path,
    stop: &AtomicBool,
    cancel: &AtomicBool,
) -> Result<Value> {
    let (space, dir, record) = original(path, scope)?;
    if record["approval_sha256"] != grant {
        return Err("finite supervisor approval mismatch".into());
    }
    let plan = &record["plan"];
    let entries = match scope {
        Scope::Dispatch(_) => &plan["entries"],
        Scope::Thread(..) => &plan["dispatch"]["plan"]["entries"],
    };
    let entries = entries
        .as_array()
        .filter(|v| (1..=16).contains(&v.len()))
        .ok_or("bounded supervisor selection")?;
    let mut seconds = 30;
    for entry in entries {
        seconds += entry["plan"]["route"]["wall_seconds"]
            .as_u64()
            .filter(|v| (1..=300).contains(v))
            .ok_or("entry deadline")?
            + 30;
    }
    let owner = dir.join("supervisor");
    fs::create_dir(&owner)?;
    write_json(&owner.join("launch.json"), &json!({"request":grant}))?;
    fs::File::open(&owner)?.sync_all()?;
    fs::File::open(&dir)?.sync_all()?;
    let (name, thread) = scope.names();
    let mut child = crate::execution::supervisor_process::spawn_coordinator(
        exe,
        &space.root,
        name,
        thread,
        grant,
        &owner,
    )?;
    let (mut stopped, mut cancelled) = (false, false);
    native::supervisor::wait(&mut child, seconds, || {
        for (op, flag, forwarded) in [
            ("cancel", cancel, &mut cancelled),
            ("stop", stop, &mut stopped),
        ] {
            if flag.load(Ordering::Acquire) && !*forwarded {
                let preview = control(path, scope, op, None)?;
                control(path, scope, op, Some(text(&preview, "approval_sha256")?))?;
                *forwarded = true;
            }
        }
        Ok(())
    })?;
    verified_result(&space, &dir, &record, scope)
}

pub fn execute(path: &Path, scope: Scope<'_>, grant: &str, cancel: &AtomicBool) -> Result<Value> {
    let (space, dir, record) = original(path, scope)?;
    let plan = &record["plan"];
    let exe = native::location(&std::env::current_exe()?)?;
    if record["approval_sha256"] != grant || plan["supervision"]["executable"] != json!(exe) {
        return Err("finite supervisor worker identity mismatch".into());
    }
    let owner = dir.join("supervisor");
    store::no_links(&owner)?;
    let ticket = owner.join("launch.json");
    if read(&ticket)? != json!({"request":grant}) {
        return Err("finite worker launch ticket mismatch".into());
    }
    // Consuming the sole ticket fences even a crash before owner publication.
    fs::rename(ticket, owner.join("claimed.json"))?;
    fs::File::open(&owner)?.sync_all()?;
    write_json(
        &owner.join("owner.json"),
        &json!({"request":grant,"pid":std::process::id()}),
    )?;
    fs::File::open(&owner)?.sync_all()?;
    let stop = AtomicBool::new(false);
    let result =
        native::supervisor::observe_controls(&owner, Some(&stop), cancel, || match scope {
            Scope::Dispatch(_) => dispatch::run_entries(
                &space,
                &dir,
                plan,
                &stop,
                cancel,
                Driver::Detached(&exe),
                &|| native::supervisor::poll_control(&owner, Some(&stop), cancel),
            ),
            Scope::Thread(..) => {
                sessions::run_thread(&space.root, &dir, plan, &stop, cancel, Some(&exe))
            }
        });
    let evidence = match &result {
        Ok(v) => json!({"request":grant,"result":v}),
        Err(e) => json!({"request":grant,"error":e.to_string()}),
    };
    write_json(&owner.join("completion.json"), &evidence)?;
    fs::File::open(owner)?.sync_all()?;
    result
}

pub(super) fn completion(dir: &Path, record: &Value, scope: Scope<'_>) -> Result<Value> {
    let owner = dir.join("supervisor");
    let admission = &record["approval_sha256"];
    let response = read(&owner.join("completion.json"))?;
    if read(&owner.join("claimed.json"))? != json!({"request":admission})
        || read(&owner.join("owner.json"))?["request"] != *admission
        || response["request"] != *admission
    {
        return Err("finite supervisor completion ownership mismatch".into());
    }
    if let Some(error) = response["error"].as_str() {
        return Err(format!("finite execution unconfirmed: {error}").into());
    }
    let result = &response["result"];
    let original = read(&dir.join("result.json"))?;
    let matches = match scope {
        Scope::Dispatch(_) => original == *result && original["dispatch_sha256"] == *admission,
        Scope::Thread(_, thread) => {
            original["approval_sha256"] == *admission
                && result["thread"] == thread
                && result["session"] == record["plan"]["session"]
                && result["result"] == original["result"]
                && result["status"] == original["result"]["status"]
        }
    };
    if !matches {
        return Err("finite supervisor result differs from original protocol evidence".into());
    }
    Ok(result.clone())
}

fn verified_result(space: &Space, dir: &Path, record: &Value, scope: Scope<'_>) -> Result<Value> {
    let result = completion(dir, record, scope)?;
    if matches!(
        result["status"].as_str(),
        Some("succeeded" | "cancelled" | "timed_out" | "failed")
    ) {
        match scope {
            Scope::Dispatch(name) => {
                if dispatch::inspect_dispatch(&space.root, name)?["status"] != result["status"] {
                    return Err("finite execution lacks matching attempt evidence".into());
                }
            }
            Scope::Thread(..) => {
                sessions::settled(space, &record["plan"]["session"], dir)?;
            }
        }
    }
    Ok(result)
}

/// Control records request an action. They are not termination or custody receipts.
pub fn control(
    path: &Path,
    scope: Scope<'_>,
    action: &str,
    approved: Option<&str>,
) -> Result<Value> {
    let (space, dir, record) = original(path, scope)?;
    let owner = dir.join("supervisor");
    store::no_links(&owner)?;
    let request = &record["approval_sha256"];
    if action == "show" && approved.is_none() {
        let (result, diagnostic) = match verified_result(&space, &dir, &record, scope) {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e.to_string())),
        };
        return Ok(
            json!({"request":request,"owner_record_present":owner.join("owner.json").try_exists()?,
            "completion_record_present":owner.join("completion.json").try_exists()?,"stop_requested":owner.join("stop.json").try_exists()?,
            "cancel_requested":owner.join("cancel.json").try_exists()?,"result":result,"diagnostic":diagnostic,
            "automatic_retries":0,"collection_authorized":false}),
        );
    }
    if !matches!(action, "stop" | "cancel") {
        return Err("invalid finite supervisor control".into());
    }
    let plan = json!({"operation":action,"owner":record["plan"]["supervision"],"request":request});
    let (digest, apply) = native::approval(&plan, approved)?;
    if !apply {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    let _lock = store::lock(&owner)?;
    let file = owner.join(format!("{action}.json"));
    let control = json!({"plan":plan,"approval_sha256":digest});
    if file.try_exists()? {
        if read(&file)? != control {
            return Err("finite control record mismatch".into());
        }
    } else {
        write_json(&file, &control)?;
        fs::File::open(owner)?.sync_all()?;
    }
    Ok(
        json!({"status":"succeeded","request":request,"requested":action,"termination_confirmed":false}),
    )
}
