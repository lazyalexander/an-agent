//! One atomic decision orders request cancellation against artifact admission.
use crate::execution::{Result, store};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

const OPEN: u8 = 0;
const CANCELLED: u8 = 1;
const COMMITTING: u8 = 2;

#[derive(Default)]
pub(super) struct Call {
    stop: AtomicBool,
    decision: AtomicU8,
}

impl Call {
    /// Signal supervision even when cancellation is too late to prevent import.
    pub(super) fn cancel(&self) {
        let _ = self
            .decision
            .compare_exchange(OPEN, CANCELLED, Ordering::SeqCst, Ordering::SeqCst);
        self.stop.store(true, Ordering::SeqCst);
    }

    pub(super) fn stop(&self) -> &AtomicBool {
        &self.stop
    }

    pub(super) fn publish(
        &self,
        dir: &Path,
        response: &mut Value,
        import: impl FnOnce() -> Result<Value>,
    ) -> Result<()> {
        // Only this transition admits import. Never reset it after an error or
        // retry an uncertain publication, even if the client cancels meanwhile.
        match self
            .decision
            .compare_exchange(OPEN, COMMITTING, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => (),
            Err(CANCELLED) => {
                response["status"] = json!("cancelled");
                response["publication"] = json!({"status":"cancelled-before-commit"});
                return Ok(());
            }
            Err(_) => return Err("publication already admitted; inspect retained evidence".into()),
        }
        store::write_new(
            &dir.join("publication-intent.json"),
            &serde_json::to_vec(&json!({
            "schema":"an-agent-tool-publication-v1", "run_id":response["run_id"],
            "receipt_sha256":response["receipt_sha256"],
            "cancellation":"too-late-after-commit-start"}))?,
        )?;
        response["artifact"] = import()?;
        response["publication"] = json!({"status":"committed",
            "cancellation":"too-late-after-commit-start"});
        Ok(())
    }
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
