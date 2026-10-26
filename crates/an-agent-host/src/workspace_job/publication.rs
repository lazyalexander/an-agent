//! Small host-owned publications under the workspace lock, never kit-writable.
use crate::{
    execution::{Result, store},
    file_job::regular,
};
use serde_json::Value;
use std::{fs, path::Path};

// Publish complete bytes before exposing a successor or replacing a provisional
// receipt. A crash before rename leaves the previous complete receipt intact.
pub(super) fn publish(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    let parent = path.parent().ok_or("missing publication parent")?;
    let pending = parent.join(format!(
        ".pending-{}",
        an_agent_core::det_seam::Entropy::os().uuid_v4()
    ));
    store::write_new(&pending, bytes)?;
    let result = (|| -> Result<()> {
        if replace {
            fs::rename(&pending, path)?;
        } else {
            fs::hard_link(&pending, path)?;
        }
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(&pending);
    result
}

pub(super) fn receipt(dir: &Path, bytes: &[u8]) -> Result<()> {
    let path = dir.join("receipt.json");
    let replace = path.try_exists()?;
    if replace {
        let previous: Value = serde_json::from_slice(&regular(&path, 2 * 1024 * 1024)?)?;
        let next: Value = serde_json::from_slice(bytes)?;
        if previous["attempt"] != next["attempt"]
            || previous["plan_sha256"] != next["plan_sha256"]
            || previous["schema"] != "an-workspace-not-started-v1"
            || previous["stage"] != "prepared"
        {
            return Err("only a provisional prelaunch receipt may be replaced".into());
        }
    }
    publish(&path, bytes, replace)
}
