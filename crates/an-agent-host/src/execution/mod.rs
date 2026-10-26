//! Host-owned CLI execution: immutable deployment, bounded context, native
//! sandbox, and receipts through real AgentControl admission. No Waver imports.
mod backend;
mod environment;
pub(crate) mod metrics;
pub(crate) mod publication;
pub(crate) mod sandbox;
pub(crate) mod store;

mod attempt;
mod deployment;

use attempt::{Input, run_selected};
pub use deployment::{check, register};
use serde_json::Value;
use std::path::Path;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Preview by default. The trusted caller authorizes one bounded invocation with
/// `execute=true`; it does not grant arbitrary shell or filesystem authority.
/// Stores must be private to the trusted host, not writable by callers or kits.
pub fn run(root: &Path, release: &str, input_path: &Path, execute: bool) -> Result<Value> {
    run_cancellable(
        root,
        release,
        input_path,
        execute,
        &std::sync::atomic::AtomicBool::new(false),
    )
}

/// Cancellation is host authority; it grants no additional capability to a kit.
pub fn run_cancellable(
    root: &Path,
    release: &str,
    input_path: &Path,
    execute: bool,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Value> {
    run_backend(
        root,
        release,
        input_path,
        execute,
        cancelled,
        &backend::Native,
    )
}

/// Use an explicitly selected, operator-owned OpenShell VM profile. Registration
/// and AgentControl admission are identical to native file-kit execution.
pub fn run_openshell(
    profile: &Path,
    root: &Path,
    release: &str,
    input_path: &Path,
    execute: bool,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Value> {
    let profile = crate::openshell::Profile::load(profile)?;
    run_backend(
        root,
        release,
        input_path,
        execute,
        cancelled,
        &backend::OpenShell(&profile),
    )
}

fn run_backend(
    root: &Path,
    release: &str,
    input_path: &Path,
    execute: bool,
    cancelled: &std::sync::atomic::AtomicBool,
    backend: &dyn backend::ExecutionBackend,
) -> Result<Value> {
    run_selected(
        root,
        release,
        Input::File(input_path),
        execute,
        cancelled,
        backend,
        true,
    )
}

/// Explicit version selection does not authorize execution or follow later edits.
pub fn run_revision(
    root: &Path,
    release: &str,
    catalog: &Path,
    revision: &str,
    execute: bool,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Value> {
    run_selected(
        root,
        release,
        Input::Revision(catalog, revision),
        execute,
        cancelled,
        &backend::Native,
        true,
    )
}

/// Trusted capability transport: protocol stdout must not contain CLI previews.
pub(crate) fn run_for_tool(
    root: &Path,
    release: &str,
    input: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Value> {
    run_selected(
        root,
        release,
        Input::File(input),
        true,
        cancelled,
        &backend::Native,
        false,
    )
}
