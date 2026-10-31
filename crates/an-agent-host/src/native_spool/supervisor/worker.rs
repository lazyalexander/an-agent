//! Original process ownership remains here even when its requesting CLI dies.
use super::*;
use std::sync::{Arc, atomic::Ordering};

struct StopObserver<'a>(&'a AtomicBool);
impl Drop for StopObserver<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "bounded supervisor control observation interval"
)]
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
    let done = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        let monitor = scope.spawn(|| {
            while !done.load(Ordering::Acquire) {
                match dir.join("cancel.json").try_exists() {
                    Ok(false) => (),
                    Ok(true) | Err(_) => {
                        // A malformed or unreadable control record fails closed.
                        cancelled.store(true, Ordering::Release);
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        });
        let stop_observer = StopObserver(&done);
        let outcome = run::start_supervised(root, name, seconds, &record, &cancelled);
        drop(stop_observer);
        monitor
            .join()
            .map_err(|_| "supervisor control observer panicked")?;
        outcome
    });
    let response = match &result {
        Ok(value) => json!({"request":hash,"result":value}),
        Err(error) => json!({"request":hash,"error":error.to_string()}),
    };
    write_json(&dir.join("completion.json"), &response)?;
    sync(&dir)?;
    result
}
