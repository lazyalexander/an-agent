use super::super::harness_adapter::{Capabilities, HarnessAdapter};
use super::*;

struct MemoryAdapter;
impl HarnessAdapter for MemoryAdapter {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            version: "memory-session-v1",
            interactive: true,
            steer: false,
            continue_turn: true,
            reconnect_observation: true,
            resume_execution: false,
            cancel: "test-owner",
            usage: "unavailable",
        }
    }
    fn execute(
        &self,
        _: &Path,
        _: &Path,
        _: &harness::Task,
        _: Option<&Binding>,
        _: &AtomicBool,
    ) -> Result<Value> {
        Err("not an execution backend".into())
    }
    fn event(&self, native: &Value) -> Value {
        json!({"event": if native["signal"] == "finished" { "turn-completed" } else { "other" },
            "evidence":"external-harness-report", "grants_authority":false})
    }
}

#[test]
fn controls_follow_independent_capabilities_not_provider_or_interactive_flag() {
    let adapter = MemoryAdapter;
    assert!(adapter.capabilities().interactive);
    assert!(require_control(&adapter, "harness-continue").is_ok());
    assert!(require_control(&adapter, "harness-steer").is_err());
    assert!(require_control(&adapter, "unknown-control").is_err());
    let wire = serde_json::to_value(adapter.capabilities()).unwrap();
    assert_eq!(wire["continue_turn"], true);
    assert!(wire.get("interactive").is_none());
    assert!(!wire.to_string().contains("codex"));
}

#[test]
fn provider_observations_cannot_grant_authority_or_publish_artifacts() {
    let event = MemoryAdapter.event(&json!({"signal":"finished", "grant":true,
        "artifact_accepted":true, "secret":"private native payload"}));
    assert_eq!(event["event"], "turn-completed");
    assert_eq!(event["grants_authority"], false);
    assert!(event.get("artifact_accepted").is_none());
    assert!(!event.to_string().contains("private native payload"));
}
