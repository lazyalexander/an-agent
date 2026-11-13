//! Provider-neutral execution, capabilities and reconnect over original task custody.
use super::*;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Kind {
    CodexExec,
    CodexAppServer,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Capabilities {
    pub version: &'static str,
    #[serde(skip)]
    pub interactive: bool,
    pub steer: bool,
    pub continue_turn: bool,
    pub reconnect_observation: bool,
    pub resume_execution: bool,
    pub cancel: &'static str,
    pub usage: &'static str,
}

pub(super) trait HarnessAdapter {
    fn capabilities(&self) -> Capabilities;
    fn execute(
        &self,
        root: &Path,
        dir: &Path,
        task: &harness::Task,
        authority: Option<&Binding>,
        cancelled: &AtomicBool,
    ) -> Result<Value>;
    fn event(&self, native: &Value) -> Value;
}

pub(super) fn kind(task: &harness::Task) -> Result<Kind> {
    if task.app_server && task.adapter == Some(Kind::CodexExec) {
        return Err("conflicting harness adapter selectors".into());
    }
    Ok(task.adapter.unwrap_or(if task.app_server {
        Kind::CodexAppServer
    } else {
        Kind::CodexExec
    }))
}
pub(super) fn interactive(task: &harness::Task) -> Result<bool> {
    Ok(selected(task)?.capabilities().interactive)
}
pub(super) fn selected(task: &harness::Task) -> Result<&'static dyn HarnessAdapter> {
    Ok(match kind(task)? {
        Kind::CodexExec => &super::codex::Codex { interactive: false },
        Kind::CodexAppServer => &super::codex::Codex { interactive: true },
    })
}
