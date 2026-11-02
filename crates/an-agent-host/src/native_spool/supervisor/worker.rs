//! Original process ownership remains here even when its requesting CLI dies.
use super::*;
use std::sync::{Arc, atomic::Ordering};

pub fn execute(root: &Path, name: &str, hash: &str, cancelled: Arc<AtomicBool>) -> Result<Value> {
    let (dir, record) = request(root, name)?;
    if record["approval_sha256"] != hash
        || location(&std::env::current_exe()?)? != Path::new(text(&record["plan"], "executable")?)
    {
        return Err("supervisor launch identity mismatch".into());
    }
    // Atomic exclusive creation admits exactly one worker. A crash after this
    // claim remains unknown, even if no backend child was subsequently started.
    write_json(
        &dir.join("owner.json"),
        &json!({"request":hash,"pid":std::process::id()}),
    )?;
    sync(&dir)?;
    let seconds = record["plan"]["seconds"]
        .as_u64()
        .ok_or("supervisor deadline")?;
    let parent = events::ParentCancel::from_route(&record["plan"]["start"]["routed_supervision"])?;
    let result = monitor(
        || {
            poll_control(&dir, None, &cancelled);
            if parent
                .as_ref()
                .is_some_and(|p| p.cancelled().unwrap_or(true))
            {
                cancelled.store(true, Ordering::Release);
            }
        },
        || {
            if cancelled.load(Ordering::Acquire) {
                return Ok(json!({"status":"cancelled","executed":false,"process_started":false}));
            }
            let result = run::start_supervised(root, name, seconds, &record, &cancelled)?;
            finish_route(root, &record["plan"]["start"], &result)?;
            Ok(result)
        },
    );
    let response = match &result {
        Ok(value) => json!({"request":hash,"result":value}),
        Err(error) => json!({"request":hash,"error":error.to_string()}),
    };
    write_json(&dir.join("completion.json"), &response)?;
    sync(&dir)?;
    result
}
