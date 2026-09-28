//! Content-addressed store. Resource mnemonics stay in the kernel because
//! sentences name them. This crate is not the steward workspace (`Wp`).

mod id;
mod store;

pub use an_agent_core::act::{Resource, ResourceKind};
pub use id::ObjectId;
pub use store::{Commit, PERMIT_FILE, Tree, TreeEntry, WORKERS_FILE, Workspace, WorkspaceError};
