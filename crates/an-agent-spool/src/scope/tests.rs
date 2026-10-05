use super::*;
use an_agent_core::act::{ToolCtx, ToolTag};
use serde_json::json;

struct Echo(&'static str);

#[async_trait::async_trait]
impl Tool for Echo {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "echo"
    }

    fn parameters(&self) -> Value {
        json!({})
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        Some(ToolTag::none())
    }

    async fn execute(&self, args: Value, _ctx: &ToolCtx) -> Result<String, String> {
        Ok(format!(
            "{}:{}",
            self.0,
            args["text"].as_str().unwrap_or("")
        ))
    }
}

fn echo(name: &'static str) -> Arc<dyn Tool> {
    Arc::new(Echo(name))
}

fn info(name: &str, version: &str, tool: &'static str) -> MountInfo {
    MountInfo {
        name: name.into(),
        version: version.into(),
        sha256: format!("hash-{name}-{version}"),
        config: Map::new(),
        inverse: Inverse::None,
        set: "s".into(),
        instance: echo(tool),
    }
}

fn info_in_set(name: &str, tool: &'static str, set: &str) -> MountInfo {
    MountInfo {
        set: set.into(),
        ..info(name, "1.0.0", tool)
    }
}

fn mount(tree: &mut ScopeTree, parent: Option<ScopeId>, name: &str) -> ScopeId {
    let tool = if name == "a" { "a" } else { "b" };
    tree.mount(parent, info(name, "1.0.0", tool)).unwrap()
}

async fn say(tool: Arc<dyn Tool>, text: &str) -> String {
    tool.execute(json!({"text": text}), &ToolCtx { signal: None })
        .await
        .unwrap()
}

#[tokio::test]
async fn siblings_share_a_name_and_leave_in_any_order() {
    // Two mounts of one body are two independent forks (koishi-style);
    // resolve answers with the newest, and either may leave first.
    let mut tree = ScopeTree::new();
    let b1 = mount(&mut tree, None, "b");
    let b2 = tree.mount(None, info("b", "1.0.1", "a")).unwrap();
    assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "a:hi");
    // The older sibling leaves first — no pinning between siblings.
    tree.unmount(b1).unwrap();
    assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "a:hi");
    tree.unmount(b2).unwrap();
    assert!(tree.resolve("b").is_none());
}

#[tokio::test]
async fn withdrawal_restores_the_pre_wrap_state() {
    // The shape of the claim under test: A wrapped B; once A leaves,
    // what answers is B exactly as before A arrived.
    let mut tree = ScopeTree::new();
    mount(&mut tree, None, "b");
    let a = mount(&mut tree, None, "a");
    tree.wrap(
        a,
        "b",
        Box::new(|inner| Arc::new(Mark("a", inner)) as Arc<dyn Tool>),
    )
    .unwrap();
    assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "[a]b:hi");
    tree.unmount(a).unwrap();
    assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "b:hi");
}

/// A decorator that truly layers: it delegates to the inner tool and
/// marks the output, so a test can tell a live chain from a stale one.
struct Mark(&'static str, Arc<dyn Tool>);

#[async_trait::async_trait]
impl Tool for Mark {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "mark"
    }

    fn parameters(&self) -> Value {
        json!({})
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        Some(ToolTag::none())
    }

    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<String, String> {
        Ok(format!("[{}]{}", self.0, self.1.execute(args, ctx).await?))
    }
}

#[tokio::test]
async fn a_wrapped_target_cannot_leave_and_wrappers_leave_from_the_top() {
    let mut tree = ScopeTree::new();
    let b = mount(&mut tree, None, "b");
    let a = mount(&mut tree, None, "a");
    tree.wrap(
        a,
        "b",
        Box::new(|inner| Arc::new(Mark("a", inner)) as Arc<dyn Tool>),
    )
    .unwrap();
    // B is pinned as a wrap target; the error names A.
    let err = tree.unmount(b).unwrap_err();
    assert!(err.to_string().contains("pinned"), "{err}");
    assert_eq!(tree.status(b), Some(Status::Active));
    // A second wrapper makes A's own layer non-top: A cannot leave
    // before C, and the error names C.
    let c = mount(&mut tree, None, "x");
    tree.wrap(
        c,
        "b",
        Box::new(|inner| Arc::new(Mark("c", inner)) as Arc<dyn Tool>),
    )
    .unwrap();
    assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "[c][a]b:hi");
    let err = tree.unmount(a).unwrap_err();
    assert!(err.to_string().contains("wrapper"), "{err}");
    // Stack order out: C, then A, then B.
    tree.unmount(c).unwrap();
    tree.unmount(a).unwrap();
    tree.unmount(b).unwrap();
    assert!(tree.resolve("b").is_none());
}

#[test]
fn wrap_cannot_cross_component_sets() {
    let mut tree = ScopeTree::new();
    tree.mount(None, info_in_set("b", "b", "s1")).unwrap();
    let a = tree.mount(None, info_in_set("a", "a", "s2")).unwrap();
    let err = tree
        .wrap(
            a,
            "b",
            Box::new(|inner| Arc::new(Mark("a", inner)) as Arc<dyn Tool>),
        )
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cannot cross"), "{msg}");
    assert!(msg.contains("s1") && msg.contains("s2"), "{msg}");
    // The target is untouched and unwrapped.
    assert!(!tree.wrappers.contains_key("b"));
}

#[test]
fn children_leave_before_their_parent_and_disposables_run_lifo() {
    let mut tree = ScopeTree::new();
    let parent = mount(&mut tree, None, "a");
    let child = mount(&mut tree, Some(parent), "b");
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    for (scope, label) in [(parent, "p1"), (parent, "p2"), (child, "c1")] {
        let order = order.clone();
        tree.collect(
            scope,
            label,
            Box::new(move |_| order.lock().unwrap().push(label.to_string())),
        )
        .unwrap();
    }
    let report = tree.unmount(parent).unwrap();
    // c1 first (child before parent), then the parent's own two in
    // reverse registration order.
    assert_eq!(*order.lock().unwrap(), vec!["c1", "p2", "p1"]);
    assert_eq!(report.len(), 2);
    assert_eq!(report[0].name, "b");
    assert_eq!(report[1].name, "a");
}

#[tokio::test]
async fn update_remounts_in_place_and_reports_the_old_subtree() {
    let mut tree = ScopeTree::new();
    let old = mount(&mut tree, None, "b");
    let (new, report) = tree.update(old, info("b", "1.1.0", "a")).unwrap();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].version, "1.0.0");
    assert_eq!(tree.status(old), None);
    assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "a:hi");
    assert_eq!(tree.status(new), Some(Status::Active));
}
