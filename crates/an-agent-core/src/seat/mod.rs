//! Who is mounted in this process, and which one beat may run.
//! `AgentControl` and the recorder both use this. Dropping a seat drops
//! its subtree. The pool does not run guest scripts.

mod pool;
mod tree;

pub use pool::{Pool, PoolError, Turn};
pub use tree::{Seat, Tree, TreeError};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::act::{Permit, ToolTag};
    use crate::agent::spawn_arc_with;
    use crate::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
    use crate::testkit::{TempDir, bash_registry};
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
                tag: ToolTag::none_permit(Permit::Deny),
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes: None,
        }
    }

    fn spawn_arc(id: &str, root: &std::path::Path) -> Arc<crate::agent::Agent> {
        spawn_arc_with(&card(id), root, &bash_registry()).unwrap()
    }

    #[test]
    fn tree_mount_drop_and_parent_cascade() {
        let tmp = TempDir::new("instance-tree");
        let parent = spawn_arc("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", tmp.path());
        let child = spawn_arc("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", tmp.path());
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
        let a = spawn_arc("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", tmp.path());
        let b = spawn_arc("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", tmp.path());
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
}
