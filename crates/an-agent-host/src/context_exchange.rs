//! Explicit, bounded text exchange. Provenance is a caller claim, not authority.
use an_agent_core::control::AgentControl;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// No roles, paths to dereference, endpoint choices, or inherited conversation.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPacket {
    pub schema: String,
    pub audience: Uuid,
    pub source: String,
    pub text: String,
    pub sha256: String,
}

impl ContextPacket {
    /// Check before deserializing so a caller cannot allocate unbounded input.
    pub fn parse(bytes: &[u8], audience: Uuid) -> Result<Self, String> {
        if bytes.len() > 4096 {
            return Err("context packet exceeds 4096 bytes".into());
        }
        let packet: Self = serde_json::from_slice(bytes).map_err(|_| "invalid context packet")?;
        if packet.schema != "an-context-v1"
            || packet.audience != audience
            || packet.source.is_empty()
            || packet.source.len() > 256
            || packet.source.chars().any(char::is_control)
            || packet.sha256 != format!("{:x}", Sha256::digest(packet.text.as_bytes()))
        {
            return Err("context identity or digest mismatch".into());
        }
        Ok(packet)
    }
}

/// Import into one explicit thread as a user-data clip. The caller authorizes
/// disclosure; a digest proves byte identity, not who authored the text.
/// Return the exact clip ID for InvokeModel, rather than selecting recent history.
pub fn import(control: &AgentControl, audience: Uuid, bytes: &[u8]) -> Result<String, String> {
    let packet = ContextPacket::parse(bytes, audience)?;
    let content = serde_json::to_string(&packet).map_err(|_| "context serialization failed")?;
    control
        .note(audience, "external-context-untrusted", &content)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_role_injection_tampering_and_oversize() -> Result<(), Box<dyn std::error::Error>> {
        let mut value = serde_json::json!({"schema":"an-context-v1", "audience":Uuid::nil(), "source":"kit:receipt", "text":"data", "sha256":format!("{:x}", Sha256::digest(b"data"))});
        assert!(ContextPacket::parse(&serde_json::to_vec(&value)?, Uuid::nil()).is_ok());
        value["role"] = "system".into();
        assert!(ContextPacket::parse(&serde_json::to_vec(&value)?, Uuid::nil()).is_err());
        value.as_object_mut().ok_or("not object")?.remove("role");
        value["text"] = "changed".into();
        assert!(ContextPacket::parse(&serde_json::to_vec(&value)?, Uuid::nil()).is_err());
        assert!(ContextPacket::parse(&vec![b' '; 4097], Uuid::nil()).is_err());
        Ok(())
    }
}
