//! Host-owned scheduling, authorization, execution and artifact custody.
//! Domain packages are loaded through `spool_loading`; Cocoon is a backend.
//! Trusted in-process embedding is optional and is not the TaskSession path.

pub mod agent_tools;
pub mod application;
pub mod artifact_context;
pub mod artifact_versions;
pub mod capability;
#[cfg(unix)]
mod cocoon;
pub mod context_exchange;
pub mod daemon;
pub mod execution;
pub mod file_job;
pub mod instance;
pub mod native_spool;
pub mod openshell;
pub mod retention;
mod settled_run;
mod task_permissions;
pub mod workspace_job;

#[cfg(feature = "embedded-rhai")]
pub mod embedded;
pub mod spool_loading;
#[cfg(feature = "embedded-rhai")]
pub use embedded::{Host, HostBodies, HostError, HostHooks};
