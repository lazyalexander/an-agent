//! Resolve private payloads for the agent body projection.
use super::*;

/// Resolve a thin observation's content address back to its text. A
/// missing or unreadable blob leaves the thin line as it is — the tape's
/// hash and size stay truthful, and the gap is visible instead of
/// invented content.
pub(super) fn resolve_payload(agent: &Agent, event: &crate::memstream::Memevent) -> String {
    if event
        .tags
        .iter()
        .any(|t| t == crate::memstream::TOOL_PAYLOAD_TAG)
    {
        return agent
            .session()
            .tape()
            .resolve_tool_payload(event)
            .unwrap_or_else(|_| "[tool payload unavailable; reference retained on tape]".into());
    }
    if event.kind != Kind::Observation {
        return event.content.clone();
    }
    let Ok(content) = serde_json::from_str::<Value>(&event.content) else {
        return event.content.clone();
    };
    let Some(sha) = content.get("response_sha256").and_then(|s| s.as_str()) else {
        return event.content.clone();
    };
    agent
        .session()
        .read_blob(sha)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_else(|| event.content.clone())
}
