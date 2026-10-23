//! Internal execution boundary, not a VM controller or a package extension API.
mod native;
mod openshell;

pub(super) use native::Native;
pub(super) use openshell::OpenShell;

use super::{Result, sandbox};
use serde_json::Value;
use std::{path::Path, sync::atomic::AtomicBool};

pub(super) struct Request<'a> {
    pub executable: &'a Path,
    pub directory: &'a Path,
    pub input: &'a [u8],
    pub cancelled: &'a AtomicBool,
}

/// Backend observations are evidence, never permission to accept or publish.
pub(super) struct Outcome {
    pub execution: sandbox::Execution,
    pub candidate: Candidate,
    pub evidence: Value,
}

pub(super) enum Candidate {
    None,
    NativeFile(Option<crate::file_job::OutputSlot>),
    Bytes(Vec<u8>),
}

pub(super) trait ExecutionBackend {
    fn validate(&self, file: bool) -> Result<()>;
    fn plan(&self, executable: &Path, directory: &Path, file: bool, plan: &mut Value)
    -> Result<()>;
    fn execute(&self, request: Request<'_>, file: bool) -> Result<Outcome>;
    /// Preserve backend-specific cancellation windows and legacy receipt fields.
    fn cancel_before_acceptance(&self) -> bool;
    fn decorate_receipt(&self, receipt: &mut Value, evidence: Value, accepted: bool);
}

pub(super) fn merge_plan(plan: &mut Value, backend: Value) -> Result<()> {
    for (key, value) in backend.as_object().ok_or("invalid backend plan")? {
        plan[key] = value.clone();
    }
    Ok(())
}

/// The host, not an adapter, validates and publishes candidate bytes.
pub(super) fn accept(candidate: Candidate, directory: &Path) -> Result<Vec<u8>> {
    match candidate {
        Candidate::None => Err("backend returned no artifact candidate".into()),
        Candidate::NativeFile(slot) => {
            crate::file_job::accept_output(&directory.join("work"), slot.as_ref())
        }
        Candidate::Bytes(bytes) => {
            if bytes.len() > 65536 {
                return Err("backend artifact exceeds output limit".into());
            }
            std::str::from_utf8(&bytes)?;
            Ok(bytes)
        }
    }
}
