//! Predictable pin errors must leave the entire subtree and all effects intact.
use super::*;
use std::sync::Mutex;

fn track(
    tree: &mut ScopeTree,
    scope: ScopeId,
    name: &'static str,
    calls: &Arc<Mutex<Vec<&'static str>>>,
) {
    let calls = calls.clone();
    tree.collect(
        scope,
        name,
        Box::new(move |_| calls.lock().unwrap().push(name)),
    )
    .unwrap();
}

#[test]
fn a_pinned_parent_or_child_preserves_every_node_and_disposable() {
    for pin_parent in [true, false] {
        let mut tree = ScopeTree::new();
        let parent = mount(&mut tree, None, "parent");
        let child = mount(&mut tree, Some(parent), "child");
        let sibling = mount(&mut tree, Some(parent), "sibling");
        let external = mount(&mut tree, None, "external");
        let target = if pin_parent { "parent" } else { "child" };
        tree.wrap(external, target, Box::new(|inner| inner))
            .unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        for (scope, name) in [(parent, "parent"), (child, "child"), (sibling, "sibling")] {
            track(&mut tree, scope, name, &calls);
        }
        assert!(matches!(
            tree.unmount(parent),
            Err(ScopeError::Pinned { .. })
        ));
        assert!(calls.lock().unwrap().is_empty());
        for scope in [parent, child, sibling, external] {
            assert_eq!(tree.status(scope), Some(Status::Active));
        }
        for name in ["parent", "child", "sibling"] {
            assert!(tree.resolve(name).is_some());
        }
        // The refused plan consumed no FnOnce callbacks; a later valid plan
        // still executes every inverse exactly once, newest child first.
        tree.unmount(external).unwrap();
        let report = tree.unmount(parent).unwrap();
        assert_eq!(
            report.iter().map(|r| r.scope).collect::<Vec<_>>(),
            vec![sibling, child, parent]
        );
        assert_eq!(*calls.lock().unwrap(), vec!["sibling", "child", "parent"]);
    }
}

#[test]
fn an_internal_child_wrapper_can_leave_before_its_parent_target() {
    let mut tree = ScopeTree::new();
    let root = mount(&mut tree, None, "target");
    let old = mount(&mut tree, Some(root), "old-wrapper");
    let new = mount(&mut tree, Some(root), "new-wrapper");
    tree.wrap(old, "target", Box::new(|inner| inner)).unwrap();
    tree.wrap(new, "target", Box::new(|inner| inner)).unwrap();
    let report = tree.unmount(root).unwrap();
    assert_eq!(
        report.iter().map(|r| r.scope).collect::<Vec<_>>(),
        vec![new, old, root]
    );
    assert!(tree.resolve("target").is_none());
    assert!(tree.active_roots().is_empty());
}

#[test]
fn sibling_wrapper_order_is_preflighted_before_an_unrelated_child_is_removed() {
    let mut tree = ScopeTree::new();
    let target = mount(&mut tree, None, "target");
    let root = mount(&mut tree, None, "root");
    let old = mount(&mut tree, Some(root), "old-wrapper");
    let new = mount(&mut tree, Some(root), "new-wrapper");
    let unrelated = mount(&mut tree, Some(root), "unrelated");
    // Wrapper registration reverses mount order. Fixed newest-child-first
    // disposal would reach a pinned wrapper after removing unrelated.
    tree.wrap(new, "target", Box::new(|inner| inner)).unwrap();
    tree.wrap(old, "target", Box::new(|inner| inner)).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    track(&mut tree, unrelated, "unrelated", &calls);
    assert!(matches!(tree.unmount(root), Err(ScopeError::Pinned { .. })));
    assert!(calls.lock().unwrap().is_empty());
    for scope in [target, root, old, new, unrelated] {
        assert_eq!(tree.status(scope), Some(Status::Active));
    }
    tree.unmount(old).unwrap();
    let report = tree.unmount(root).unwrap();
    assert_eq!(
        report.iter().map(|r| r.scope).collect::<Vec<_>>(),
        vec![unrelated, new, root]
    );
    assert!(tree.resolve("target").is_some());
}

#[test]
fn a_refused_update_keeps_the_old_body_and_its_children() {
    let mut tree = ScopeTree::new();
    let root = mount(&mut tree, None, "target");
    let child = mount(&mut tree, Some(root), "child");
    let external = mount(&mut tree, None, "external");
    tree.wrap(external, "target", Box::new(|inner| inner))
        .unwrap();
    assert!(matches!(
        tree.update(root, info("target", "2.0.0", "a")),
        Err(ScopeError::Pinned { .. })
    ));
    assert_eq!(tree.node(root).unwrap().version, "1.0.0");
    assert_eq!(tree.status(child), Some(Status::Active));
    assert_eq!(tree.node(root).unwrap().children, vec![child]);
}
