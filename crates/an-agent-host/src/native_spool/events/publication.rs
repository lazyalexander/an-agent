//! Publish only complete native custody; filenames and exit status are insufficient.
use super::*;

pub(super) fn candidate(space: &Space, root: &Path, run: &str, output: &str) -> Result<Value> {
    separate(&space.root, root)?;
    let (_, bundle) = native::state(root)?;
    native::event_contract::permits(&bundle.manifest, false)?;
    let (artifacts, _) = native::artifacts::verified(root, run, true)?;
    let selected = artifacts["outputs"]
        .as_array()
        .ok_or("outputs")?
        .iter()
        .find(|v| v["path"] == output)
        .ok_or("event must select an accepted declared output")?;
    let (_, binding) = crate::artifact_versions::read(
        Path::new(text(&artifacts, "catalog")?),
        text(&selected["binding"], "revision")?,
    )?;
    let start: Value = serde_json::from_slice(&regular(
        &root.join("runs").join(run).join("start.json"),
        2 * 1024 * 1024,
    )?)?;
    let cause = if let Some(context) = start.get("event_input") {
        let directory = space
            .root
            .join("deliveries")
            .join(text(context, "delivery")?);
        let intent: Value =
            serde_json::from_slice(&regular(&directory.join("intent.json"), 2 * 1024 * 1024)?)?;
        if context["workspace"] != space.id
            || intent["plan"]["start"] != start
            || intent["plan"]["context"] != *context
            || hash(&intent["plan"])? != intent["approval_sha256"]
        {
            return Err("result event does not bind its incoming delivery".into());
        }
        Some(context.clone())
    } else {
        None
    };
    let event = json!({"schema":"an-native-artifact-event-v1","workspace":space.id,
        "kind":native::event_contract::ARTIFACT,"source":identity(root)?,"run":run,"output":output,
        "artifact":{"catalog":artifacts["catalog"],"binding":binding},"cause":cause});
    Ok(json!({"id":hash(&event)?,"event":event}))
}

pub(super) fn verify(space: &Space, event: &Value) -> Result<()> {
    let payload = &event["event"];
    let root = native::location(Path::new(text(&payload["source"], "root")?))?;
    if candidate(
        space,
        &root,
        text(payload, "run")?,
        text(payload, "output")?,
    )? != *event
    {
        return Err("event differs from verified native custody".into());
    }
    Ok(())
}

pub(super) fn admission(event: &Value) -> Result<()> {
    let artifact = &event["event"]["artifact"];
    crate::artifact_versions::admission::ensure_active(
        Path::new(text(artifact, "catalog")?),
        text(&artifact["binding"], "revision")?,
    )
}

pub fn publish(
    path: &Path,
    instance: &Path,
    run: &str,
    output: &str,
    approved: Option<&str>,
) -> Result<Value> {
    let space = Space::open(path)?;
    let root = native::location(instance)?;
    let _instance = store::lock(&root)?;
    let _lock = store::lock(&space.root)?;
    let event = candidate(&space, &root, run, output)?;
    let plan = json!({"operation":"publish","event":event,"execution_granted":false});
    let (digest, execute) = native::approval(&plan, approved)?;
    if let Some(prior) = space.find_event(text(&event, "id")?)? {
        if prior != event {
            return Err("event conflicts with prior publication".into());
        }
        space.append(&event)?;
        return Ok(
            json!({"status":"succeeded","event":event,"executed":false,"already_published":true}),
        );
    }
    admission(&event)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    space.append(&event)?;
    Ok(json!({"status":"succeeded","event":event,"executed":true}))
}
