//! Read-only delivery inventory and bounded foreground change observation.
use super::*;
use an_agent_core::det_seam::Clock;
use std::{sync::atomic::Ordering, time::Duration};

fn observation(space: &Space, event: &Value, route: &Value) -> Result<Value> {
    let ctx = delivery::context(space, event, route)?;
    let dir = delivery::directory(space, &ctx)?;
    store::no_links(&dir)?;
    let refusal = space
        .check_active(route)
        .and_then(|()| publication::admission(event))
        .err()
        .map(|e| e.to_string());
    let mut row = json!({"context":ctx,"event":event["id"],"subscription":route["name"],
        "subscription_control":control::current(space,route)?,"retired_by":replacement::retired_by(space,route)?,
        "phase":if refusal.is_some() {"blocked"} else {"awaiting-authorization"},
        "admission_refusal":refusal,"automatic_retries":0});
    if dir.exists() {
        let observed = settlement::observe(space, event, route)?;
        let phase = if observed["disposition"] == "already-consumed" {
            "consumed"
        } else {
            let run = &observed["run_observation"];
            if run["termination_confirmed"] != true {
                "running-or-unconfirmed"
            } else if run["execution_status"] != "succeeded" {
                "settled-unsuccessful"
            } else {
                match run["artifact_status"].as_str() {
                    Some("accepted") => "awaiting-confirmation",
                    Some("awaiting-acceptance") => "awaiting-acceptance",
                    _ => "needs-reconciliation",
                }
            }
        };
        row["phase"] = json!(phase);
        row["evidence"] = observed;
    }
    Ok(row)
}

/// Inventory does not prepare runs, select state, acknowledge or grant execution.
pub fn pending(path: &Path) -> Result<Value> {
    let space = Space::open(path)?;
    let _lock = store::lock(&space.root)?;
    let records = space.facts.log()?;
    let mut routes = Vec::new();
    for record in &records {
        if record.kind == "config"
            && let Some(name) = record
                .name
                .as_deref()
                .and_then(|n| n.strip_prefix("native.events."))
        {
            routes.push(space.route(name)?);
        }
    }
    if routes.len() > 16 {
        return Err("static subscription observation limit exceeded".into());
    }
    let mut rows = Vec::new();
    for record in &records {
        if record.kind != "event"
            || record.name.as_deref() != Some(native::event_contract::ARTIFACT)
        {
            continue;
        }
        let raw: Value = serde_json::from_str(record.body.as_deref().ok_or("missing event body")?)?;
        let event = space.event(text(&raw, "id")?)?;
        for route in &routes {
            if delivery::routed(&space, &event, route).is_ok() {
                rows.push(observation(&space, &event, route)?);
            }
        }
    }
    Ok(
        json!({"schema":"an-native-event-pending-v1","workspace":space.id,"deliveries":rows,
        "execution_granted":false,"automatic_retries":0,
        "authority":"observation only; delivery, state selection, acceptance and completion remain explicit"}),
    )
}

/// Emit an initial snapshot and changed evidence only. Fail closed on read errors.
pub fn watch(
    path: &Path,
    seconds: u64,
    interval_ms: u64,
    cancelled: &AtomicBool,
    mut emit: impl FnMut(&Value) -> Result<()>,
) -> Result<Value> {
    if !(1..=3600).contains(&seconds) || !(100..=5000).contains(&interval_ms) {
        return Err("watch requires 1..3600 seconds and 100..5000 millisecond interval".into());
    }
    let deadline = Clock::monotonic_now() + Duration::from_secs(seconds);
    let mut previous = None;
    let mut snapshots = 0;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let current = pending(path)?;
        if previous.as_ref() != Some(&current) {
            emit(&current)?;
            previous = Some(current);
            snapshots += 1;
        }
        if Clock::monotonic_now() >= deadline {
            break;
        }
        // Short waits keep Ctrl-C responsive even with a large poll interval.
        let next = (Clock::monotonic_now() + Duration::from_millis(interval_ms)).min(deadline);
        while Clock::monotonic_now() < next && !cancelled.load(Ordering::Relaxed) {
            std::thread::sleep(
                Duration::from_millis(50)
                    .min(next.saturating_duration_since(Clock::monotonic_now())),
            );
        }
    }
    Ok(
        json!({"schema":"an-native-event-watch-stopped-v1","status":"succeeded",
        "reason":if cancelled.load(Ordering::Relaxed) {"cancelled"} else {"deadline"},
        "snapshots":snapshots,"execution_granted":false,"automatic_retries":0,
        "execution_cancelled":false}),
    )
}
