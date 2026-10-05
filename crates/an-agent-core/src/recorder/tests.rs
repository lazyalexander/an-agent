use super::*;
use crate::act::{Audience, Charter, FileFacet, MemoryFacet, Permit, Signal, ToolTag};
use crate::principal::card::{ModelSpec, ToolGrant, Topology};
use crate::testkit::{TempDir, bash_registry};

fn open_at(
    card: &AgentCard,
    root: &std::path::Path,
    config: &str,
    context: &str,
) -> Result<Recorder, RecorderError> {
    Recorder::open(card, root, config, context, &bash_registry())
}

fn bare(id: &str, permit: Permit) -> AgentCard {
    AgentCard {
        v: 1,
        id: Uuid::parse_str(id).unwrap(),
        model: ModelSpec {
            base_url: "https://example.com".into(),
            model: "m".into(),
            extra_body: None,
        },
        prompt: "keep-this-prompt".into(),
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

#[test]
fn go_grant_cannot_open_a_recorder() {
    let tmp = TempDir::new("recorder-deny");
    let err = open_at(
        &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Go),
        tmp.path(),
        "{}",
        "[]",
    );
    assert!(matches!(err, Err(RecorderError::GrantNotDenied(_))));
}

#[test]
fn only_the_recorder_spawns_and_queue_steers() {
    let tmp = TempDir::new("recorder-queue");
    let recorder = open_at(
        &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
        tmp.path(),
        "{\"limit\":1}",
        "[\"c0\"]",
    )
    .unwrap();
    let worker = recorder
        .spawn_worker(
            &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
            &Charter::new(ToolTag::none_permit(Permit::Go)),
        )
        .unwrap();
    assert_eq!(
        recorder.pool.tree().parent(worker.id()).unwrap(),
        Some(recorder.id())
    );
    assert_eq!(
        recorder.submit(worker.id(), "later").unwrap(),
        Arrival::Queued
    );
    let turn = recorder.begin_turn(worker.id()).unwrap();
    assert_eq!(
        recorder.submit(worker.id(), "also check tests").unwrap(),
        Arrival::Steered
    );
    assert!(matches!(recorder.pop(), Err(RecorderError::Busy)));
    let mailed = worker.session().tape().read_all().unwrap();
    assert!(
        mailed
            .iter()
            .any(|e| e.tags.iter().any(|t| t == "send") && e.content.contains("also check tests"))
    );
    drop(turn);
    let intent = recorder.pop().unwrap();
    assert_eq!(intent.text, "later");
    let spawned = recorder.agent().session().tape().read_all().unwrap();
    assert!(
        spawned
            .iter()
            .any(|e| { e.tags.iter().any(|t| t == "spawn") && e.content.contains("\"bound\"") })
    );
}

#[test]
fn spawn_rejects_a_grant_wider_than_the_bound() {
    let tmp = TempDir::new("recorder-bound");
    let recorder = open_at(
        &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
        tmp.path(),
        "{}",
        "[]",
    )
    .unwrap();
    let err = recorder.spawn_worker(
        &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
        &Charter::new(ToolTag::none_permit(Permit::Deny)),
    );
    assert!(matches!(err, Err(RecorderError::OutsideBound(_))));
    let id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
    assert!(recorder.workspace().subwp_of(id).is_none());
}

#[test]
fn lite_has_a_seat_no_subwp_and_finishes_by_complete() {
    let tmp = TempDir::new("recorder-lite");
    let recorder = open_at(
        &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
        tmp.path(),
        "{}",
        "[]",
    )
    .unwrap();
    let wide = Charter::new(ToolTag {
        file: FileFacet::Unbounded,
        permit: Permit::Go,
        memory: MemoryFacet::Ignore,
    });
    assert!(matches!(
        recorder.spawn_lite(
            &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
            &wide
        ),
        Err(RecorderError::LiteFile)
    ));
    let bound = Charter::new(ToolTag::none_permit(Permit::Go));
    let lite = recorder
        .spawn_lite(
            &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
            &bound,
        )
        .unwrap();
    assert_eq!(
        recorder.pool.tree().parent(lite.id()).unwrap(),
        Some(recorder.id())
    );
    assert!(recorder.workspace().subwp_of(lite.id()).is_none());
    assert!(matches!(
        recorder.close_worker(lite.id(), &[]),
        Err(RecorderError::NoSubwp(_))
    ));
    recorder.complete(lite.id(), "result").unwrap();
    assert!(tagged(&lite, "complete"));
    assert!(tagged(recorder.agent(), "complete"));
    assert!(recorder.workspace().current_paths().is_empty());
}

#[test]
fn release_drops_the_subtree_without_publishing_and_child_bound_cannot_widen() {
    let tmp = TempDir::new("recorder-release");
    let recorder = open_at(
        &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
        tmp.path(),
        "{}",
        "[]",
    )
    .unwrap();
    let parent_bound = Charter::new(ToolTag::none_permit(Permit::Go));
    let parent = recorder
        .spawn_worker(
            &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
            &parent_bound,
        )
        .unwrap();
    let wider = parent_bound.clone().with_signal(Signal {
        complete: Permit::Go,
        audience: Audience::Any,
    });
    assert!(matches!(
        recorder.spawn_under(
            parent.id(),
            &bare("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
            &wider,
        ),
        Err(RecorderError::WiderThanParent)
    ));
    let child = recorder
        .spawn_under(
            parent.id(),
            &bare("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
            &parent_bound,
        )
        .unwrap();
    let sub = recorder.workspace().subwp_of(parent.id()).unwrap();
    recorder
        .workspace()
        .write(&sub, "src/a.txt", b"draft", super::wp::WriteMode::Create)
        .unwrap();
    recorder.release(parent.id()).unwrap();
    assert!(recorder.pool.tree().get(parent.id()).is_none());
    assert!(recorder.pool.tree().get(child.id()).is_none());
    assert!(recorder.workspace().subwp_of(parent.id()).is_none());
    assert!(recorder.workspace().current_paths().is_empty());
}

fn tagged(agent: &Agent, tag: &str) -> bool {
    agent
        .session()
        .tape()
        .read_all()
        .unwrap()
        .iter()
        .any(|e| e.tags.iter().any(|t| t == tag))
}

#[test]
fn cancel_propagates_down_and_restore_skips_broken_chains() {
    let tmp = TempDir::new("recorder-lease");
    let recorder = open_at(
        &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
        tmp.path(),
        "{}",
        "[]",
    )
    .unwrap();
    let bound = Charter::new(ToolTag::none_permit(Permit::Go));
    let kept = recorder
        .spawn_lite(
            &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
            &bound,
        )
        .unwrap();
    let parent = recorder
        .spawn_worker(
            &bare("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
            &bound,
        )
        .unwrap();
    let child = recorder
        .spawn_under(
            parent.id(),
            &bare("dddddddd-dddd-dddd-dddd-dddddddddddd", Permit::Go),
            &bound,
        )
        .unwrap();
    let parent_lease = recorder.lease(parent.id()).unwrap();
    let child_lease = recorder.lease(child.id()).unwrap();
    let kept_lease = recorder.lease(kept.id()).unwrap();
    recorder.propagate_cancel(parent.id()).unwrap();
    assert!(parent_lease.is_cancelled());
    assert!(child_lease.is_cancelled());
    assert!(!kept_lease.is_cancelled());
    assert!(tagged(&child, "cancel"));
    let restored = restore_seats(recorder.agent().session().root()).unwrap();
    assert!(restored.iter().any(|seat| seat.id == kept.id()));
    assert!(
        restored
            .iter()
            .all(|seat| seat.id != parent.id() && seat.id != child.id())
    );
}

#[test]
fn signals_follow_the_charter_and_only_parent_cancels() {
    let tmp = TempDir::new("recorder-signal");
    let recorder = open_at(
        &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
        tmp.path(),
        "{}",
        "[]",
    )
    .unwrap();
    let bound = Charter::new(ToolTag::none_permit(Permit::Go)).with_signal(Signal {
        complete: Permit::Deny,
        audience: Audience::Parent,
    });
    let worker = recorder
        .spawn_worker(
            &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
            &bound,
        )
        .unwrap();
    let sibling = recorder
        .spawn_worker(
            &bare("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
            &Charter::new(ToolTag::none_permit(Permit::Go)),
        )
        .unwrap();
    assert!(matches!(
        recorder.complete(worker.id(), "done"),
        Err(RecorderError::SignalDenied("complete"))
    ));
    assert!(matches!(
        recorder.send(worker.id(), sibling.id(), "hi"),
        Err(RecorderError::SignalDenied("send"))
    ));
    assert!(matches!(
        recorder.cancel(worker.id(), recorder.id()),
        Err(RecorderError::SignalDenied("cancel"))
    ));
    let id = recorder.cancel(recorder.id(), worker.id()).unwrap();
    assert!(tagged(recorder.agent(), "cancel"));
    assert!(tagged(&worker, "cancel"));
    assert!(!tagged(&sibling, "cancel"));
    let back = worker.session().tape().read_all().unwrap();
    assert!(
        back.iter()
            .any(|e| e.refs.first().map(String::as_str) == Some(id.as_str()))
    );
    recorder.send(worker.id(), recorder.id(), "ping").unwrap();
    assert!(tagged(&worker, "send"));
}

#[test]
fn collect_records_pointers_not_bodies() {
    let tmp = TempDir::new("recorder-collect");
    let recorder = open_at(
        &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
        tmp.path(),
        "{}",
        "[]",
    )
    .unwrap();
    let worker = recorder
        .spawn_worker(
            &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Deny),
            &Charter::new(ToolTag::none_permit(Permit::Deny)),
        )
        .unwrap();
    let marker = "BODY_SHOULD_NOT_APPEAR";
    worker
        .session()
        .tape()
        .append(AppendEvent {
            from: worker.id_str().into(),
            from_kind: FromKind::Agent,
            kind: Kind::Utterance,
            session: worker.session().id_str().into(),
            content: marker.into(),
            tags: vec![],
            refs: vec![],
            act: None,
            card: None,
        })
        .unwrap();
    let product = ProductPtr::from_tape_file(
        worker.session().id_str(),
        &worker.session().tape_path(),
        None,
        None,
        None,
    )
    .unwrap();
    let digest = product.tape_sha256.clone();
    let id = recorder.collect(&product).unwrap();
    let events = recorder.agent().session().tape().read_all().unwrap();
    let link = events.iter().find(|e| e.id == id).unwrap();
    assert!(link.tags.iter().any(|t| t == "link"));
    assert!(!link.content.contains(marker));
    assert!(link.content.contains(&digest));
}

#[test]
fn view_reads_linked_ids_only() {
    let tmp = TempDir::new("recorder-view");
    let recorder = open_at(
        &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
        tmp.path(),
        "{}",
        "[]",
    )
    .unwrap();
    let a = recorder
        .link(&ProductPtr {
            child_session: "s1".into(),
            tape_sha256: "aa".into(),
            md_sha256: None,
            tree_id: None,
            subwp_sha256: None,
        })
        .unwrap();
    let b = recorder
        .link(&ProductPtr {
            child_session: "s2".into(),
            tape_sha256: "bb".into(),
            md_sha256: Some("cc".into()),
            tree_id: Some("tree-1".into()),
            subwp_sha256: None,
        })
        .unwrap();
    recorder
        .define_view("main", &[a.clone(), b.clone()])
        .unwrap();
    assert_eq!(recorder.view("main").unwrap(), vec![a, b]);
}
