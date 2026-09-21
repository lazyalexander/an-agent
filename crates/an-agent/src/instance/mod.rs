//! Instance runtime for the loop: private session, live agent, registration
//! tree, bounded turn pool. Workplace mount is not wired.

mod agent;
mod pool;
mod session;
mod tree;

pub use agent::{Agent, AgentError};
pub use pool::{Pool, PoolError, Turn};
pub use session::{Session, SessionError};
pub use tree::{Seat, Tree, TreeError};

use std::path::Path;
use std::sync::Arc;

use thiserror::Error;

use crate::principal::card::AgentCard;
use crate::principal::factory::{self, FactoryError};

#[derive(Debug, Error)]
pub enum InstanceError {
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error("card hash: {0}")]
    CardHash(#[from] serde_json::Error),
}

/// Card → grants (factory) then a fresh private session. The loop's constructor.
pub fn spawn(card: &AgentCard, sessions_root: impl AsRef<Path>) -> Result<Agent, InstanceError> {
    let hash = card.hash()?;
    let rt = factory::build(card, &hash)?;
    let session = Session::create(sessions_root, card.id)?;
    Ok(Agent::from_runtime(rt, session)?)
}

pub fn spawn_arc(
    card: &AgentCard,
    sessions_root: impl AsRef<Path>,
) -> Result<Arc<Agent>, InstanceError> {
    Ok(Arc::new(spawn(card, sessions_root)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::{Permit, ToolCall, ToolCtx, ToolTag, run_tool_act};
    use crate::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
    use crate::testkit::TempDir;
    use uuid::Uuid;

    fn card(id: &str) -> AgentCard {
        AgentCard {
            v: 1,
            id: Uuid::parse_str(id).unwrap(),
            model: ModelSpec {
                base_url: "https://example.com".into(),
                model: "m".into(),
                extra_body: None,
            },
            prompt: "p".into(),
            tools: vec![ToolGrant {
                name: "bash".into(),
                tag: ToolTag::none_permit(Permit::Forbidden),
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes: None,
        }
    }

    fn ctx() -> ToolCtx {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolCtx { signal: Some(rx) }
    }

    #[test]
    fn spawn_binds_private_session_and_act_ctx() {
        let tmp = TempDir::new("instance-spawn");
        let agent = spawn(&card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"), tmp.path()).unwrap();
        assert!(agent.session().files().is_dir());
        let actx = agent.act_ctx();
        assert_eq!(actx.agent_id, agent.id_str());
        assert_eq!(actx.session, agent.session().id_str());
        assert_eq!(actx.card, Some(agent.card_hash()));
        assert!(actx.store.is_some());
    }

    #[test]
    fn tree_mount_drop_and_parent_cascade() {
        let tmp = TempDir::new("instance-tree");
        let parent = spawn_arc(&card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"), tmp.path()).unwrap();
        let child = spawn_arc(&card("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"), tmp.path()).unwrap();
        let tree = Tree::new();
        let p_id = parent.id();
        let c_id = child.id();
        let p_seat = tree.mount(parent, None).unwrap();
        let _c_seat = tree.mount(child, Some(p_id)).unwrap();
        assert_eq!(tree.len(), 2);
        assert_eq!(tree.children(p_id).unwrap(), vec![c_id]);
        drop(p_seat);
        assert!(tree.is_empty());
        assert!(tree.get(c_id).is_none());
    }

    #[test]
    fn pool_bounds_turns() {
        let tmp = TempDir::new("instance-pool");
        let a = spawn_arc(&card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"), tmp.path()).unwrap();
        let b = spawn_arc(&card("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"), tmp.path()).unwrap();
        let pool = Pool::new(1).unwrap();
        let a_id = a.id();
        let b_id = b.id();
        let _sa = pool.tree().mount(a, None).unwrap();
        let _sb = pool.tree().mount(b, None).unwrap();
        let t1 = pool.begin_turn(a_id).unwrap();
        assert!(matches!(pool.begin_turn(a_id), Err(PoolError::Busy(_))));
        assert!(matches!(
            pool.begin_turn(b_id),
            Err(PoolError::AtCapacity(1))
        ));
        drop(t1);
        let t2 = pool.begin_turn(b_id).unwrap();
        assert_eq!(t2.agent().id(), b_id);
    }

    #[tokio::test]
    async fn spawned_agent_can_admit_through_session_tape() {
        let tmp = TempDir::new("instance-act");
        let agent = spawn(&card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"), tmp.path()).unwrap();
        let call = ToolCall {
            id: "c1".into(),
            name: "bash".into(),
            arguments: r#"{"command":"true"}"#.into(),
        };
        let result = run_tool_act(&agent.act_ctx(), agent.tools(), &call, &ctx())
            .await
            .unwrap();
        assert_eq!(result.message.content, "forbidden");
        assert_eq!(agent.session().tape().read_all().unwrap().len(), 2);
    }
}
