//! Content-addressed store. Resource mnemonics stay in the kernel.
//! This crate is not the recorder workspace (`Wp`).

mod id;
mod store;

pub use an_agent_core::act::{Resource, ResourceKind};
pub use id::ObjectId;
pub use store::{Commit, PERMIT_FILE, Tree, TreeEntry, WORKERS_FILE, Workspace, WorkspaceError};
