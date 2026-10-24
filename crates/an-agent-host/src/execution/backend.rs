//! Internal execution boundary, not a VM controller or a package extension API.
mod native;
mod openshell;

pub(super) use native::Native;
pub(super) use openshell::OpenShell;

pub(super) use super::environment::{Candidate, Environment, Operation, Outcome, Request, accept};
use super::{Result, sandbox};
use serde_json::Value;
use std::path::Path;
#[cfg(test)]
use std::sync::atomic::AtomicBool;

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

#[cfg(test)]
mod tests;

#[cfg(all(test, unix))]
mod host_tests;
