//! Resolve selected payloads strictly; previews may display an unavailable marker.
use super::*;

/// A preview marker is not input for a provider request. Explicit clip selection
/// must call `resolve_payload` again and propagate the error before admission.
pub(super) fn preview_payload(agent: &Agent, event: &crate::memstream::Memevent) -> String {
    resolve_payload(agent, event)
        .unwrap_or_else(|_| "[payload unavailable; reference retained on tape]".into())
}

pub(super) fn resolve_payload(
    agent: &Agent,
    event: &crate::memstream::Memevent,
) -> Result<String, ControlError> {
    if event
        .tags
        .iter()
        .any(|t| t == crate::memstream::TOOL_PAYLOAD_TAG)
    {
        return Ok(crate::memstream::tool_context(
            event,
            agent.session().tape().resolve_tool_payload(event)?,
        ));
    }
    if event.kind != Kind::Observation {
        return Ok(event.content.clone());
    }
    let Ok(content) = serde_json::from_str::<Value>(&event.content) else {
        return Ok(event.content.clone());
    };
    let Some(sha) = content.get("response_sha256").and_then(|s| s.as_str()) else {
        return Ok(event.content.clone());
    };
    let corrupt =
        || crate::memstream::StoreError::Corrupt("model payload identity mismatch".into());
    if sha.len() != 64
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(corrupt().into());
    }
    let bytes = agent.session().read_blob(sha)?;
    use sha2::{Digest, Sha256};
    if format!("{:x}", Sha256::digest(&bytes)) != sha
        || content
            .get("response_bytes")
            .is_some_and(|n| n.as_u64() != Some(bytes.len() as u64))
    {
        return Err(corrupt().into());
    }
    String::from_utf8(bytes).map_err(|_| corrupt().into())
}
