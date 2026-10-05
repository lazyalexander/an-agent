use super::*;
use crate::act::{Charter, FileFacet, MemoryFacet, Permit, ToolTag};
use crate::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
use crate::recorder::Recorder;
use crate::testkit::TempDir;
use sha2::{Digest, Sha256};

fn card(id: &str, permit: Permit) -> AgentCard {
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
            tag: ToolTag {
                file: FileFacet::None,
                permit,
                memory: MemoryFacet::Ignore,
            },
        }],
        topology: Topology::Leaf,
        kernel: "0.1.0".into(),
        supersedes: None,
    }
}

fn recorder(tmp: &TempDir) -> Recorder {
    Recorder::open(
        &card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
        tmp.path(),
        "{}",
        "[]",
        &crate::testkit::bash_registry(),
    )
    .unwrap()
}

fn bound() -> Charter {
    Charter::new(ToolTag::none_permit(Permit::Go))
}

fn worker(s: &Recorder) -> (std::sync::Arc<crate::agent::Agent>, String) {
    let w = s
        .spawn_worker(
            &card("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
            &bound(),
        )
        .unwrap();
    let id = s.workspace().subwp_of(w.id()).unwrap();
    (w, id)
}

#[test]
fn continue_replaces_same_true_name_and_keeps_bytes() {
    let tmp = TempDir::new("wp-continue");
    let s = recorder(&tmp);
    let (w1, sub1) = worker(&s);
    let name = s
        .workspace()
        .write(&sub1, "src/a.txt", b"v1", WriteMode::Create)
        .unwrap();
    assert!(matches!(
        s.close_worker(w1.id(), &[]).unwrap(),
        CloseOut::Published(_)
    ));
    let blob = w1
        .session()
        .root()
        .join("subwp/blobs")
        .join(format!("{:x}", Sha256::digest(b"v1")));
    let (w2, sub2) = {
        let w = s
            .spawn_worker(
                &card("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                &bound(),
            )
            .unwrap();
        let id = s.workspace().subwp_of(w.id()).unwrap();
        (w, id)
    };
    let again = s
        .workspace()
        .write(&sub2, "src/a.txt", b"v2", WriteMode::Continue { base: 1 })
        .unwrap();
    assert_eq!(again, name);
    s.close_worker(w2.id(), &[]).unwrap();
    let paths = s.workspace().current_paths();
    assert_eq!(paths, vec![("src/a.txt".into(), name, 2)]);
    assert_eq!(std::fs::read(blob).unwrap(), b"v1");
}

#[test]
fn continue_rejects_a_stale_base_without_writing() {
    let tmp = TempDir::new("wp-stale");
    let s = recorder(&tmp);
    let (w1, sub1) = worker(&s);
    s.workspace()
        .write(&sub1, "src/a.txt", b"v1", WriteMode::Create)
        .unwrap();
    s.close_worker(w1.id(), &[]).unwrap();
    let (w2, sub2) = {
        let w = s
            .spawn_worker(
                &card("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                &bound(),
            )
            .unwrap();
        let id = s.workspace().subwp_of(w.id()).unwrap();
        (w, id)
    };
    let err = s
        .workspace()
        .write(&sub2, "src/a.txt", b"v2", WriteMode::Continue { base: 9 })
        .unwrap_err();
    assert!(matches!(
        err,
        WpError::StaleBase {
            current: 1,
            base: 9,
            ..
        }
    ));
    let log = std::fs::read_to_string(w2.session().root().join("subwp/log.jsonl")).unwrap();
    assert!(!log.contains("\"op\":\"write\"") && !log.contains("\"op\": \"write\""));
}

#[test]
fn create_on_taken_path_asks_then_reject_or_rename() {
    let tmp = TempDir::new("wp-conflict");
    let s = recorder(&tmp);
    let (w1, sub1) = worker(&s);
    s.workspace()
        .write(&sub1, "src/a.txt", b"old", WriteMode::Create)
        .unwrap();
    s.close_worker(w1.id(), &[]).unwrap();
    let (w2, sub2) = {
        let w = s
            .spawn_worker(
                &card("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                &bound(),
            )
            .unwrap();
        let id = s.workspace().subwp_of(w.id()).unwrap();
        (w, id)
    };
    s.workspace()
        .write(&sub2, "src/a.txt", b"new", WriteMode::Create)
        .unwrap();
    let conflict = s.close_worker(w2.id(), &[]).unwrap();
    let CloseOut::Conflict(c) = conflict else {
        panic!("expected conflict");
    };
    assert!(s.workspace().is_live(&sub2));
    s.workspace()
        .resolve(
            &sub2,
            Resolve::Allow {
                true_name: c.incoming.clone(),
                path: "src/b.txt".into(),
            },
        )
        .unwrap();
    assert!(!s.workspace().is_live(&sub2));
    let paths = s.workspace().current_paths();
    assert!(paths.iter().any(|p| p.0 == "src/a.txt"));
    assert!(
        paths
            .iter()
            .any(|p| p.0 == "src/b.txt" && p.1 == c.incoming)
    );
}

#[test]
fn empty_close_publishes_nothing() {
    let tmp = TempDir::new("wp-empty");
    let s = recorder(&tmp);
    let (w, sub) = worker(&s);
    assert!(matches!(
        s.close_worker(w.id(), &[]).unwrap(),
        CloseOut::Empty
    ));
    assert!(!s.workspace().is_live(&sub));
    assert!(s.workspace().current_paths().is_empty());
}

#[test]
fn cyclic_extra_is_rejected() {
    let tmp = TempDir::new("wp-cycle");
    let s = recorder(&tmp);
    let (w, sub) = worker(&s);
    let a = s
        .workspace()
        .write(&sub, "src/a.txt", b"a", WriteMode::Create)
        .unwrap();
    let b = s
        .workspace()
        .write(&sub, "src/b.txt", b"b", WriteMode::Create)
        .unwrap();
    let err = s.close_worker(
        w.id(),
        &[
            DepEdge {
                dependent: VerId {
                    true_name: a.clone(),
                    version: 1,
                },
                depends_on: VerId {
                    true_name: b.clone(),
                    version: 1,
                },
            },
            DepEdge {
                dependent: VerId {
                    true_name: b,
                    version: 1,
                },
                depends_on: VerId {
                    true_name: a,
                    version: 1,
                },
            },
        ],
    );
    assert!(matches!(
        err,
        Err(crate::recorder::RecorderError::Wp(WpError::Cycle))
    ));
    assert!(s.workspace().is_live(&sub));
}

#[test]
fn rollback_without_cascade_leaves_dependents() {
    let tmp = TempDir::new("wp-roll-quiet");
    let s = recorder(&tmp);
    let (w, sub) = worker(&s);
    let a = s
        .workspace()
        .write(&sub, "src/a.txt", b"a1", WriteMode::Create)
        .unwrap();
    s.close_worker(w.id(), &[]).unwrap();
    let (w2, sub2) = {
        let w = s
            .spawn_worker(
                &card("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                &bound(),
            )
            .unwrap();
        let id = s.workspace().subwp_of(w.id()).unwrap();
        (w, id)
    };
    s.workspace()
        .write(&sub2, "src/a.txt", b"a2", WriteMode::Continue { base: 1 })
        .unwrap();
    let b = s
        .workspace()
        .write(&sub2, "src/b.txt", b"b1", WriteMode::Create)
        .unwrap();
    s.close_worker(
        w2.id(),
        &[DepEdge {
            dependent: VerId {
                true_name: b.clone(),
                version: 1,
            },
            depends_on: VerId {
                true_name: a.clone(),
                version: 2,
            },
        }],
    )
    .unwrap();
    s.workspace().rollback(&a, 1, false).unwrap();
    let quiet = s.workspace().current_paths();
    assert!(quiet.iter().any(|p| p.1 == a && p.2 == 1));
    assert!(quiet.iter().any(|p| p.1 == b && p.2 == 1));
}

#[test]
fn rollback_can_cascade_and_keeps_prior_view() {
    let tmp = TempDir::new("wp-roll");
    let s = recorder(&tmp);
    let (w, sub) = worker(&s);
    let a = s
        .workspace()
        .write(&sub, "src/a.txt", b"a1", WriteMode::Create)
        .unwrap();
    s.close_worker(w.id(), &[]).unwrap();
    let (w2, sub2) = {
        let w = s
            .spawn_worker(
                &card("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                &bound(),
            )
            .unwrap();
        let id = s.workspace().subwp_of(w.id()).unwrap();
        (w, id)
    };
    s.workspace()
        .write(&sub2, "src/a.txt", b"a2", WriteMode::Continue { base: 1 })
        .unwrap();
    let b = s
        .workspace()
        .write(&sub2, "src/b.txt", b"b1", WriteMode::Create)
        .unwrap();
    s.close_worker(
        w2.id(),
        &[DepEdge {
            dependent: VerId {
                true_name: b.clone(),
                version: 1,
            },
            depends_on: VerId {
                true_name: a.clone(),
                version: 2,
            },
        }],
    )
    .unwrap();
    let before = std::fs::read(tmp.path().join("wp/views.jsonl")).unwrap();
    s.workspace().rollback(&a, 1, true).unwrap();
    let cascaded = s.workspace().current_paths();
    assert!(cascaded.iter().any(|p| p.1 == a && p.2 == 1));
    assert!(
        !cascaded.iter().any(|p| p.1 == b),
        "dependent introduced with A@2 leaves the view"
    );
    let after = std::fs::read(tmp.path().join("wp/views.jsonl")).unwrap();
    assert!(after.starts_with(&before));
}

#[test]
fn unclosed_subwp_is_not_the_view() {
    let tmp = TempDir::new("wp-open");
    let s = recorder(&tmp);
    let (_w, sub) = worker(&s);
    s.workspace()
        .write(&sub, "src/a.txt", b"x", WriteMode::Create)
        .unwrap();
    assert!(s.workspace().is_live(&sub));
    assert!(s.workspace().current_paths().is_empty());
}
