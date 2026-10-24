//! Host-owned execution identity and settlement, independent of backend mechanics.
use super::{Result, sandbox};
use serde::Serialize;
use serde_json::Value;
use std::{path::Path, sync::atomic::AtomicBool};

/// Host-created before submission. A new attempt must get a new identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Operation {
    pub run_id: String,
    pub operation_id: String,
}

/// Backend identity is distinct from a process ID or a host operation ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Environment {
    pub id: String,
    pub generation: String,
}

pub(super) struct Request<'a> {
    pub operation: &'a Operation,
    pub executable: &'a Path,
    pub directory: &'a Path,
    pub input: &'a [u8],
    pub cancelled: &'a AtomicBool,
}

/// Backend observations are evidence, never permission to accept or publish.
pub(super) struct Outcome {
    pub operation: Operation,
    pub execution: sandbox::Execution,
    pub environment: Option<Environment>,
    pub settled_environment: Option<Environment>,
    pub termination_confirmed: bool,
    pub requires_environment: bool,
    pub candidate: Candidate,
    pub evidence: Value,
}

pub(super) enum Candidate {
    None,
    NativeFile(Option<crate::file_job::OutputSlot>),
    Bytes(Vec<u8>),
}

impl Outcome {
    /// Only successful results can cross the acceptance boundary. Failure
    /// evidence is still retained, including unconfirmed termination.
    pub fn reconcile(&self, expected: &Operation) -> Result<()> {
        if &self.operation != expected {
            return Err("backend result belongs to another operation".into());
        }
        if self.execution.status == "succeeded" {
            if self.requires_environment && self.environment.is_none() {
                return Err("backend result lacks environment identity".into());
            }
            if !self.termination_confirmed {
                return Err("backend result lacks termination confirmation".into());
            }
            if self.environment != self.settled_environment {
                return Err("backend environment identity or generation changed".into());
            }
        }
        Ok(())
    }
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
