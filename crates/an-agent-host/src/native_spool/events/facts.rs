//! Frozen observation records never initialize or repair the workspace.
use super::*;
use an_agent_core::workspace::WorkspaceRecord;

pub(super) enum Facts {
    Live(Workspace),
    Observed(Vec<WorkspaceRecord>),
}
impl Facts {
    pub fn observed(bytes: &[u8]) -> Result<Self> {
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            return Err("incomplete workspace log record; inspect before continuing".into());
        }
        let mut records = Vec::new();
        for line in bytes.split(|b| *b == b'\n') {
            if !line.iter().all(u8::is_ascii_whitespace) {
                records.push(serde_json::from_slice(line)?);
                if records.len() > 512 {
                    return Err("native event workspace record limit exceeded".into());
                }
            }
        }
        Ok(Self::Observed(records))
    }
    pub fn log(&self) -> Result<Vec<WorkspaceRecord>> {
        match self {
            Self::Live(space) => Ok(space.log()?),
            Self::Observed(records) => Ok(records.clone()),
        }
    }
    pub fn writable(&self) -> Result<&Workspace> {
        match self {
            Self::Live(space) => Ok(space),
            Self::Observed(_) => Err("observation cannot mutate workspace facts".into()),
        }
    }
    pub fn push_event(&self, name: &str, body: &str) -> Result<WorkspaceRecord> {
        Ok(self.writable()?.push_event(name, body)?)
    }
    pub fn put_config(&self, name: &str, bytes: &[u8]) -> Result<WorkspaceRecord> {
        Ok(self.writable()?.put_config(name, bytes)?)
    }
}
