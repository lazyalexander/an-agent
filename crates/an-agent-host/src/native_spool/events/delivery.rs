//! One event/subscription admits at most one attempt, with a durable start fence.
use super::*;

pub(super) fn context(space: &Space, event: &Value, route: &Value) -> Result<Value> {
    let mut value = json!({"workspace":space.id,"event":event["id"],"subscription":hash(route)?});
    value["delivery"] = json!(hash(&value)?);
    Ok(value)
}
pub(super) fn directory(space: &Space, context: &Value) -> Result<PathBuf> {
    let id = text(context, "delivery")?;
    if !store::valid_digest(id) {
        return Err("invalid delivery identity".into());
    }
    Ok(space.root.join("deliveries").join(id))
}
pub(super) fn intent(space: &Space, event: &Value, route: &Value) -> Result<Value> {
    let ctx = context(space, event, route)?;
    let value: Value = serde_json::from_slice(&regular(
        &directory(space, &ctx)?.join("intent.json"),
        2 * 1024 * 1024,
    )?)?;
    if value["schema"] != "an-native-event-delivery-intent-v1"
        || value["approval_sha256"] != hash(&value["plan"])?
        || value["plan"]["context"] != ctx
        || value["plan"]["event"] != *event
        || value["plan"]["route"] != *route
        || value["plan"]["start"]["event_input"] != ctx
        || value["plan"]["start"]["run"] != ctx["delivery"]
        || value["plan"]["start"]["instance"] != route["target"]["instance"]
        || value["plan"]["start"]["release"] != route["target"]["release"]
    {
        return Err("delivery intent binding mismatch".into());
    }
    Ok(value)
}
pub(super) fn routed(event: &Value, route: &Value) -> Result<()> {
    if event["event"]["source"] != route["source"]
        || event["event"]["output"] != route["output"]
        || event["event"]["kind"] != route["kind"]
    {
        return Err("event is outside the selected subscription".into());
    }
    Ok(())
}
fn selected(event: &Value, context: &Value) -> Result<Value> {
    let artifact = &event["event"]["artifact"];
    let catalog = Path::new(text(artifact, "catalog")?);
    let binding = &artifact["binding"];
    let revision = text(binding, "revision")?;
    let (_, actual) = crate::artifact_versions::read(catalog, revision)?;
    if actual != *binding || binding.get("encoding").is_some() {
        return Err("event delivery requires one fixed text artifact".into());
    }
    Ok(
        json!({"path":catalog.join("revisions").join(revision).join("content.txt"),
        "catalog":catalog,"binding":binding,"event_input":context}),
    )
}

pub fn deliver(
    path: &Path,
    event_id: &str,
    name: &str,
    approved: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Value> {
    let space = Space::open(path)?;
    let (event, route, ctx) = {
        let _lock = store::lock(&space.root)?;
        let event = space.event(event_id)?;
        let route = space.route(name)?;
        routed(&event, &route)?;
        let ctx = context(&space, &event, &route)?;
        if directory(&space, &ctx)?.exists() {
            return settlement::observe(&space, &event, &route);
        }
        space.check_active(&route)?;
        publication::verify(&space, &event)?;
        (event, route, ctx)
    };
    let root = Path::new(text(&route["target"], "root")?);
    let run = text(&ctx, "delivery")?;
    let input = selected(&event, &ctx)?;
    let seconds = route["wall_seconds"].as_u64().ok_or("wall seconds")?;
    let preview =
        native::run::start_event(root, run, &input, seconds, None, cancelled, |_| Ok(()))?;
    let plan = json!({"operation":"deliver","context":ctx,"event":event,"route":route,"start":preview["plan"],
        "consumption":"requires separate formal acceptance and event completion"});
    let (digest, execute) = native::approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    let result = native::run::start_event(
        root,
        run,
        &input,
        seconds,
        preview["approval_sha256"].as_str(),
        cancelled,
        |start| {
            let _lock = store::lock(&space.root)?;
            if space.route(name)? != route
                || space.event(event_id)? != event
                || *start != plan["start"]
            {
                return Err("delivery changed before start admission".into());
            }
            space.check_active(&route)?;
            publication::verify(&space, &event)?;
            // Exclusive directory also fences a crash before the intent is complete.
            if fs::read_dir(space.root.join("deliveries"))?
                .take(128)
                .count()
                >= 128
            {
                return Err("native event delivery limit reached".into());
            }
            let dir = directory(&space, &ctx)?;
            fs::create_dir(&dir)?;
            fs::File::open(space.root.join("deliveries"))?.sync_all()?;
            write_json(
                &dir.join("intent.json"),
                &json!({"schema":"an-native-event-delivery-intent-v1",
            "plan":plan,"approval_sha256":digest}),
            )?;
            fs::File::open(&dir)?.sync_all()?;
            Ok(())
        },
    );
    match result {
        Ok(result) => {
            Ok(json!({"status":result["status"],"context":ctx,"result":result,"consumed":false}))
        }
        Err(error) if directory(&space, &ctx)?.exists() => {
            Ok(json!({"status":"uncertain","context":ctx,
            "diagnostic":error.to_string(),"automatic_retries":0,"consumed":false}))
        }
        Err(error) => Err(error),
    }
}
