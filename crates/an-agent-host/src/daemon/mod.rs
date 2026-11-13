//! User-local dispatch over existing supervisors, lifecycle and artifact custody.
//! Queue claims are scheduling fences, never replacement execution receipts.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod activity;
mod app_server;
mod codex;
mod control;
mod delegation;
pub(crate) mod harness;
mod harness_adapter;
mod harness_metrics;
mod observation;
mod permissions;
mod scheduler;
mod storage;
mod task_session;
#[cfg(unix)]
mod transport;
mod web;

use crate::task_permissions::Binding;
use crate::{
    execution::{Result, store},
    native_spool as spool,
};
pub use harness::worker as harness_worker;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
pub use storage::{init, launchd};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema: String,
    concurrency: usize,
    executable: PathBuf,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Job {
    Spool {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        authorization: Option<Binding>,
        instance: PathBuf,
        run: String,
        request: String,
    },
    Harness {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        authorization: Option<Binding>,
        task: Box<harness::Task>,
        request: String,
    },
}

impl Job {
    fn authorization(&self) -> Option<&Binding> {
        match self {
            Self::Spool { authorization, .. } | Self::Harness { authorization, .. } => {
                authorization.as_ref()
            }
        }
    }
    fn request(&self) -> &str {
        match self {
            Self::Spool { request, .. } | Self::Harness { request, .. } => request,
        }
    }
}

fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&store::read(
        path,
        2 * 1024 * 1024,
    )?)?)
}
fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    store::write_new(path, &serde_json::to_vec(value)?)?;
    fs::File::open(path.parent().ok_or("record parent")?)?.sync_all()?;
    Ok(())
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("missing {key}").into())
}
fn approval(plan: Value, supplied: Option<&str>) -> Result<(Value, bool)> {
    let hash = store::digest(&serde_json::to_vec(&plan)?);
    if supplied.is_some_and(|value| value != hash) {
        return Err("daemon approval mismatch".into());
    }
    Ok((
        json!({"plan":plan,"approval_sha256":hash,"executed":false}),
        supplied.is_some(),
    ))
}

pub fn request(root: &Path, value: &Value) -> Result<Value> {
    #[cfg(unix)]
    {
        transport::request(root, value, false)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, value);
        Err("local daemon requires macOS".into())
    }
}

pub fn task_request(root: &Path, value: &Value) -> Result<Value> {
    #[cfg(unix)]
    {
        transport::request(root, value, true)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, value);
        Err("local daemon requires macOS".into())
    }
}

pub fn serve(root: &Path, cancelled: &AtomicBool) -> Result<Value> {
    #[cfg(target_os = "macos")]
    {
        transport::serve(root, cancelled)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, cancelled);
        Err("native daemon is qualified for macOS only".into())
    }
}
