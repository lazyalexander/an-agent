//! Bounded artifact events and explicit delivery, not a business workflow engine.
mod control;
mod delivery;
mod dispatch;
mod dispatch_reconcile;
pub use dispatch_reconcile::reconcile_dispatch;
mod facts;
mod monitor;
mod publication;
mod replacement;
pub use replacement::replace;
#[cfg(test)]
mod observation_tests;
mod settlement;
mod space;
mod subscription;
#[cfg(test)]
mod tests;
pub use control::subscription_control;
pub use delivery::{deliver, deliver_with_state};
pub use dispatch::{dispatch, inspect_dispatch};
pub use monitor::{pending, watch};
pub use publication::publish;
pub use settlement::complete;
pub(super) use settlement::confirmed_input_consumption;
pub use space::{init, inspect};
pub use subscription::{revoke, subscribe};

use super::{self as native, Result, Value, json, regular, store, write_json};
use an_agent_core::workspace::Workspace;
use space::Space;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("missing native event {key}").into())
}
fn hash(value: &Value) -> Result<String> {
    Ok(store::digest(&serde_json::to_vec(value)?))
}
fn separate(a: &Path, b: &Path) -> Result<()> {
    if a.starts_with(b) || b.starts_with(a) {
        return Err("event workspace and instances must be separate nonnested directories".into());
    }
    Ok(())
}
fn identity(path: &Path) -> Result<Value> {
    let root = native::location(path)?;
    let (instance, _) = native::state(&root)?;
    Ok(json!({"root":root,"instance":instance["instance"],"release":instance["release"]}))
}
