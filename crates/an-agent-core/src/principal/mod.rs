pub mod card;
pub mod factory;
pub mod registry;

pub use card::{AgentCard, ModelSpec, ToolGrant, Topology};

use std::fs;
use std::path::Path;

use thiserror::Error;
use uuid::Uuid;

use crate::det_seam::Entropy;

const DNS_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

#[derive(Debug, Error)]
pub enum PrincipalError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

fn stdin_namespace() -> Uuid {
    Uuid::new_v5(&DNS_NAMESPACE, b"an-agent.stdin")
}

pub fn stdin_counterpart_id(agent_id: Uuid) -> Uuid {
    Uuid::new_v5(&stdin_namespace(), format!("{agent_id}/stdin").as_bytes())
}

pub fn load_or_create_id(path: &Path) -> Result<Uuid, PrincipalError> {
    if path.exists() {
        let raw = fs::read_to_string(path)?;
        if let Ok(id) = raw.trim().parse::<Uuid>() {
            return Ok(id);
        }
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let id = Entropy::os().uuid_v4();
    fs::write(path, format!("{id}\n"))?;
    Ok(id)
}

pub fn local_agent_id(root: &Path) -> Result<Uuid, PrincipalError> {
    load_or_create_id(&root.join("agent-id"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::TempDir;

    #[test]
    fn stdin_id_is_stable_and_not_the_agent() {
        let a = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let b = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
        assert_eq!(stdin_counterpart_id(a), stdin_counterpart_id(a));
        assert_ne!(stdin_counterpart_id(a), stdin_counterpart_id(b));
        assert_ne!(stdin_counterpart_id(a), a);
        assert_eq!(
            stdin_counterpart_id(a).get_version(),
            Some(uuid::Version::Sha1)
        );
    }

    #[test]
    fn persists_agent_id_without_human_id() {
        let tmp = TempDir::new("ids");
        let dir = tmp.path().to_path_buf();
        let first = local_agent_id(&dir).unwrap();
        let second = local_agent_id(&dir).unwrap();
        assert_eq!(first, second);
        assert!(!dir.join("human-id").exists());
    }
}
