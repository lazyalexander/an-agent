//! Consumption is accepted output plus causal evidence, not delivery or process exit.
use super::*;

/// Reuse original acknowledgement validation; this never repairs a lost receipt.
pub(in crate::native_spool) fn confirmed_input_consumption(
    path: &Path,
    subscription: &str,
    target: &Path,
    run: &str,
    start: &Value,
) -> Result<Value> {
    let space = Space::observe(path)?;
    let route = space.route(subscription)?;
    let event = space.event(text(&start["event_input"], "event")?)?;
    let ctx = delivery::context(&space, &event, &route)?;
    if ctx != start["event_input"] || ctx["delivery"] != run || route["target"] != identity(target)?
    {
        return Err("input release delivery identity mismatch".into());
    }
    delivery::routed(&space, &event, &route)?;
    publication::verify(&space, &event)?;
    let observed = observe(&space, &event, &route)?;
    if observed["status"] != "succeeded" || observed["disposition"] != "already-consumed" {
        return Err(
            "input release requires original consumption acknowledgement; no repair or retry"
                .into(),
        );
    }
    Ok(observed["receipt"].clone())
}

fn receipt(space: &Space, event: &Value, route: &Value) -> Result<Value> {
    let intent = delivery::intent(space, event, route)?;
    let ctx = &intent["plan"]["context"];
    let root = Path::new(text(&route["target"], "root")?);
    if identity(root)? != route["target"] {
        return Err("delivery target changed".into());
    }
    let run = text(ctx, "delivery")?;
    let start = regular(
        &root.join("runs").join(run).join("start.json"),
        2 * 1024 * 1024,
    )?;
    if serde_json::from_slice::<Value>(&start)? != intent["plan"]["start"] {
        return Err("accepted run does not bind this event start".into());
    }
    let (accepted, _) = native::artifacts::verified(root, run, true)?;
    let result = publication::candidate(space, root, run, text(route, "result")?)?;
    Ok(
        json!({"schema":"an-native-event-consumed-v1","context":ctx,"target":route["target"],
        "start_sha256":store::digest(&start),"accepted":accepted,"result_event":result}),
    )
}
pub(super) fn observe(space: &Space, event: &Value, route: &Value) -> Result<Value> {
    let ctx = delivery::context(space, event, route)?;
    let dir = delivery::directory(space, &ctx)?;
    let evidence = (|| -> Result<Value> {
        delivery::intent(space, event, route)?;
        let proof = receipt(space, event, route)?;
        let ack: Value = serde_json::from_slice(&regular(&dir.join("ack.json"), 2 * 1024 * 1024)?)?;
        if ack != proof
            || space.event(text(&proof["result_event"], "id")?)? != proof["result_event"]
        {
            return Err("consumption acknowledgement changed".into());
        }
        Ok(proof)
    })();
    match evidence {
        Ok(proof) => Ok(
            json!({"status":"succeeded","disposition":"already-consumed","executed":false,
            "automatic_retries":0,"receipt":proof}),
        ),
        Err(error) => {
            let root = Path::new(text(&route["target"], "root")?);
            let observation = native::status(root, text(&ctx, "delivery")?)
                .unwrap_or_else(|e| json!({"phase":"unconfirmed","diagnostic":e.to_string()}));
            Ok(
                json!({"status":"uncertain","disposition":"submitted-not-confirmed","context":ctx,
                "executed":false,"automatic_retries":0,"run_observation":observation,"diagnostic":error.to_string()}),
            )
        }
    }
}

pub fn complete(path: &Path, event_id: &str, name: &str, approved: Option<&str>) -> Result<Value> {
    let space = Space::open(path)?;
    let route = {
        let _lock = store::lock(&space.root)?;
        space.route(name)?
    };
    let target = Path::new(text(&route["target"], "root")?);
    let _instance = store::lock(target)?;
    let _lock = store::lock(&space.root)?;
    let event = space.event(event_id)?;
    delivery::routed(&space, &event, &route)?;
    publication::verify(&space, &event)?;
    let proof = receipt(&space, &event, &route)?;
    let result_id = text(&proof["result_event"], "id")?;
    let published = space.find_event(result_id)?;
    let ack_path = delivery::directory(&space, &proof["context"])?.join("ack.json");
    if ack_path.exists() {
        let ack: Value = serde_json::from_slice(&regular(&ack_path, 2 * 1024 * 1024)?)?;
        if ack != proof || published.as_ref() != Some(&proof["result_event"]) {
            return Err("consumption record or result event conflicts".into());
        }
        return Ok(
            json!({"status":"succeeded","executed":false,"already_consumed":true,"receipt":proof}),
        );
    }
    // Reconstructing a lost acknowledgement from an existing result fact grants no execution.
    if published.is_none() {
        space.not_revoked(&route)?;
    } else if published.as_ref() != Some(&proof["result_event"]) {
        return Err("result event changed".into());
    }
    let plan = json!({"operation":"complete","receipt":proof,"execution_granted":false});
    let (digest, execute) = native::approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    space.append(&proof["result_event"])?;
    write_json(&ack_path, &proof)?;
    Ok(json!({"status":"succeeded","executed":true,"acknowledged":true,"receipt":proof}))
}
