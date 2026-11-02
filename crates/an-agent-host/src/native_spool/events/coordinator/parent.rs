//! A finite selection delegates one exact event and retains cancellation authority if its waiter dies.
use super::*;

fn scope(parent: &Value) -> Result<Scope<'_>> {
    let dispatch = text(parent, "dispatch")?;
    native::bundle::identifier(dispatch)?;
    match (parent.get("session"), parent.get("thread")) {
        (None, None) => Ok(Scope::Dispatch(dispatch)),
        (Some(session), Some(thread)) => Ok(Scope::Thread(
            session.as_str().ok_or("parent session")?,
            thread.as_str().ok_or("parent thread")?,
        )),
        _ => Err("invalid finite parent scope".into()),
    }
}

pub(in crate::native_spool::events) fn verify_parent(
    space: &Space,
    route: &Value,
    event_plan: &Value,
) -> Result<()> {
    let Some(parent) = route.get("parent") else {
        return Ok(());
    };
    let scope = scope(parent)?;
    let (_, dir, owner) = original(&space.root, scope)?;
    let batch_dir = space
        .root
        .join("dispatches")
        .join(text(parent, "dispatch")?);
    let batch = dispatch::recorded(space, &batch_dir)?;
    let selected = match scope {
        Scope::Dispatch(_) => &owner,
        Scope::Thread(..) => &owner["plan"]["dispatch"],
    };
    let grant = &owner["approval_sha256"];
    if selected["plan"] != batch["plan"]
        || selected["approval_sha256"] != batch["approval_sha256"]
        || read(&dir.join("supervisor/claimed.json"))? != json!({"request":grant})
        || read(&dir.join("supervisor/owner.json"))?["request"] != *grant
        || !batch["plan"]["entries"]
            .as_array()
            .ok_or("bounded parent selection")?
            .iter()
            .any(|e| e["plan"] == *event_plan)
    {
        return Err("event is not the original finite owner's approved entry".into());
    }
    Ok(())
}

pub(in crate::native_spool) struct ParentCancel {
    controller: PathBuf,
    session: Option<PathBuf>,
}

impl ParentCancel {
    pub(in crate::native_spool) fn from_route(route: &Value) -> Result<Option<Self>> {
        let Some(parent) = route.get("parent") else {
            return Ok(None);
        };
        let workspace = native::location(Path::new(text(route, "workspace")?))?;
        let scope = scope(parent)?;
        let dir = scope.directory(&workspace)?;
        let session = match scope {
            Scope::Dispatch(_) => None,
            Scope::Thread(name, _) => Some(workspace.join("sessions").join(name)),
        };
        Ok(Some(Self {
            controller: dir.join("supervisor"),
            session,
        }))
    }

    pub(in crate::native_spool) fn cancelled(&self) -> Result<bool> {
        if !self.controller.is_dir() || self.controller.join("cancel.json").try_exists()? {
            return Ok(true);
        }
        match &self.session {
            Some(root) => Ok(sessions::control(root)?["mode"] == "cancelled"),
            None => Ok(false),
        }
    }
}
