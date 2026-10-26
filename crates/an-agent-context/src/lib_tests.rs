use super::index::load_rows;
use super::*;
use an_agent_core::act::{FileFacet, MemoryFacet, Permit, ToolTag};
use an_agent_core::control::{AgentControl, Registration};
use an_agent_core::memstream::{AppendEvent, FromKind, JsonlStore, Kind};
use an_agent_core::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
use an_agent_core::recorder::Recorder;
use an_agent_core::testkit::{TempDir, bash_registry};
use serde_json::json;
use uuid::Uuid;

fn card() -> AgentCard {
    AgentCard {
        v: 1,
        id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
        model: ModelSpec {
            base_url: "https://example.com".into(),
            model: "m".into(),
            extra_body: None,
        },
        prompt: "prompt-text".into(),
        tools: vec![ToolGrant {
            name: "bash".into(),
            tag: ToolTag {
                file: FileFacet::None,
                permit: Permit::Deny,
                memory: MemoryFacet::Ignore,
            },
        }],
        topology: Topology::Leaf,
        kernel: "0.1.0".into(),
        supersedes: None,
    }
}

fn seal(store: &JsonlStore, dir: &Path, summary: &str) -> PathBuf {
    let anchor = store.read_all().unwrap().pop().unwrap();
    store
        .append(AppendEvent {
            from: anchor.from.clone(),
            from_kind: FromKind::Agent,
            kind: Kind::Utterance,
            session: anchor.session.clone().unwrap_or_default(),
            content: summary.to_string(),
            tags: vec!["summary".into()],
            refs: vec![],
            act: None,
            card: anchor.card.clone(),
        })
        .unwrap();
    let events = store.read_all().unwrap();
    let cut = cut_if_long(dir, &events, 0).unwrap().unwrap();
    store
        .append(AppendEvent {
            from: anchor.from,
            from_kind: FromKind::Agent,
            kind: Kind::Action,
            session: anchor.session.unwrap_or_default(),
            content: json!({ "md_sha256": cut.sha256, "events": cut.events }).to_string(),
            tags: vec!["compress".into()],
            refs: cut.events,
            act: None,
            card: anchor.card,
        })
        .unwrap();
    rebuild_index(dir, &store.read_all().unwrap()).unwrap();
    cut.path
}

fn session(tmp: &TempDir) -> PathBuf {
    let s = Recorder::open(
        &card(),
        tmp.path(),
        "{\"k\":1}",
        "[\"c0\"]",
        &an_agent_factory::tool_registry(),
    )
    .unwrap();
    s.agent().session().root().to_path_buf()
}

#[test]
fn audit_and_quarantined_evidence_do_not_enter_assembly_or_compressed_memory() {
    let tmp = TempDir::new("ctx-quarantine");
    let dir = session(&tmp);
    let tape = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
    let anchor = tape.read_all().unwrap().pop().unwrap();
    let mut audit_ids = Vec::new();
    for tag in ["model_quarantined", "semantic", "review_request"] {
        let event = tape
            .append(AppendEvent {
                from: anchor.from.clone(),
                from_kind: FromKind::Agent,
                kind: Kind::Observation,
                session: anchor.session.clone().unwrap_or_default(),
                content: format!("excluded-{tag}"),
                tags: vec![tag.into(), "audit".into()],
                refs: vec![],
                act: None,
                card: anchor.card.clone(),
            })
            .unwrap();
        audit_ids.push(event.id);
    }
    let events = tape.read_all().unwrap();
    assert!(
        super::assemble::uncompressed(&events)
            .iter()
            .all(|e| !audit_ids.contains(&e.id))
    );
    let legacy_event = events.iter().find(|e| e.id == audit_ids[0]).unwrap();
    let legacy_cut = super::assemble::write_cut(&dir, &[legacy_event]).unwrap();
    let retained = fs::read(&legacy_cut.path).unwrap();
    let assembly = assemble(&dir, &events, AssembleMode::Continue, 100_000).unwrap();
    assert!(
        assembly
            .context
            .iter()
            .all(|piece| !piece.text.contains("excluded-"))
    );
    assert_eq!(fs::read(&legacy_cut.path).unwrap(), retained);
    let path = seal(&tape, &dir, "ordinary summary");
    assert!(!fs::read_to_string(&path).unwrap().contains("excluded-"));
    assert!(
        segment_sources(&dir, &path)
            .unwrap()
            .iter()
            .all(|(id, _)| !audit_ids.contains(id))
    );
}

#[test]
fn old_context_cache_cannot_restore_audit_events_or_segments() {
    for segment in [false, true] {
        let tmp = TempDir::new("ctx-old-audit-cache");
        let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
        let id = control.open_thread(&card()).unwrap();
        let evidence = control
            .note(id, "audit", "excluded-control-evidence")
            .unwrap();
        let events = control.events(id).unwrap();
        let dir = control.directory(id).unwrap();
        let event = events.iter().find(|e| e.id == evidence).unwrap();
        let piece = if segment {
            let cut = super::assemble::write_cut(&dir, &[event]).unwrap();
            json!({ "kind": "hot", "segment": format!("{}.md#s0", cut.sha256) })
        } else {
            json!({ "kind": "recent", "event_id": evidence })
        };
        control
            .commit_context(
                id,
                &json!({
                    "mode": "continue", "budget_chars": 100_000,
                    "config_id": null, "env_id": null, "pieces": [piece],
                })
                .to_string(),
                &[evidence],
            )
            .unwrap();
        let assembly = assemble_thread(&control, id, AssembleMode::Continue, 100_000).unwrap();
        assert!(
            assembly
                .context
                .iter()
                .all(|piece| !piece.text.contains("excluded-control-evidence"))
        );
    }
}

#[test]
fn summary_cites_events_and_second_cut_keeps_the_first_file() {
    let tmp = TempDir::new("ctx-cut");
    let dir = session(&tmp);
    let tape = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
    let anchor = tape.read_all().unwrap().pop().unwrap();
    tape.append(AppendEvent {
        from: anchor.from.clone(),
        from_kind: FromKind::Agent,
        kind: Kind::Utterance,
        session: anchor.session.clone().unwrap_or_default(),
        content: "saw the bug".into(),
        tags: vec![],
        refs: vec![],
        act: None,
        card: anchor.card.clone(),
    })
    .unwrap();
    let first = seal(&tape, &dir, "user summary one");
    let first_bytes = fs::read(&first).unwrap();
    let sources = segment_sources(&dir, &first).unwrap();
    let sum: f64 = sources.iter().map(|(_, c)| c).sum();
    assert!((sum - sources.len() as f64).abs() < 1e-6);
    assert!(sources.iter().any(|(_, c)| (*c - 1.0).abs() < 1e-9));
    tape.append(AppendEvent {
        from: anchor.from.clone(),
        from_kind: FromKind::Agent,
        kind: Kind::Utterance,
        session: anchor.session.clone().unwrap_or_default(),
        content: "later".into(),
        tags: vec![],
        refs: vec![],
        act: None,
        card: anchor.card.clone(),
    })
    .unwrap();
    let second = seal(&tape, &dir, "user summary two");
    assert_ne!(first, second);
    assert_eq!(fs::read(&first).unwrap(), first_bytes);
}

#[test]
fn continue_takes_recent_and_hot_and_bumps_weak_count() {
    let tmp = TempDir::new("ctx-hot");
    let dir = session(&tmp);
    let tape = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
    let anchor = tape.read_all().unwrap().pop().unwrap();
    tape.append(AppendEvent {
        from: anchor.from.clone(),
        from_kind: FromKind::Agent,
        kind: Kind::Utterance,
        session: anchor.session.clone().unwrap_or_default(),
        content: "fact".into(),
        tags: vec![],
        refs: vec![],
        act: None,
        card: anchor.card.clone(),
    })
    .unwrap();
    seal(&tape, &dir, "sum");
    tape.append(AppendEvent {
        from: anchor.from.clone(),
        from_kind: FromKind::Agent,
        kind: Kind::Utterance,
        session: anchor.session.clone().unwrap_or_default(),
        content: "tail".into(),
        tags: vec![],
        refs: vec![],
        act: None,
        card: anchor.card.clone(),
    })
    .unwrap();
    let got = assemble(
        &dir,
        &tape.read_all().unwrap(),
        AssembleMode::Continue,
        10_000,
    )
    .unwrap();
    assert!(
        got.context
            .iter()
            .any(|p| p.kind == "recent" && p.text == "tail")
    );
    assert!(got.context.iter().any(|p| p.kind == "hot"));
    assert_eq!(
        got.prompt,
        tape.read_all()
            .unwrap()
            .iter()
            .rev()
            .find(|e| e.tags.iter().any(|t| t == "prompt"))
            .unwrap()
            .content
    );
    let rows = load_rows(&dir).unwrap();
    assert!(rows.iter().any(|r| r.weak >= 1));
}

#[test]
fn length_cut_freezes_a_long_tail() {
    let tmp = TempDir::new("ctx-len");
    let dir = session(&tmp);
    let tape = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
    let anchor = tape.read_all().unwrap().pop().unwrap();
    tape.append(AppendEvent {
        from: anchor.from,
        from_kind: FromKind::Agent,
        kind: Kind::Utterance,
        session: anchor.session.unwrap_or_default(),
        content: "x".repeat(40),
        tags: vec![],
        refs: vec![],
        act: None,
        card: anchor.card,
    })
    .unwrap();
    let events = tape.read_all().unwrap();
    let cut = cut_if_long(&dir, &events, 20).unwrap().unwrap();
    tape.append(AppendEvent {
        from: "a".into(),
        from_kind: FromKind::Agent,
        kind: Kind::Action,
        session: "s".into(),
        content: json!({ "md_sha256": cut.sha256, "events": cut.events }).to_string(),
        tags: vec!["compress".into()],
        refs: cut.events.clone(),
        act: None,
        card: None,
    })
    .unwrap();
    let bytes = fs::read(&cut.path).unwrap();
    let again = cut_if_long(&dir, &tape.read_all().unwrap(), 20).unwrap();
    assert!(again.is_none());
    assert_eq!(fs::read(&cut.path).unwrap(), bytes);
}

#[test]
fn independent_task_has_no_context() {
    let tmp = TempDir::new("ctx-empty");
    let dir = session(&tmp);
    let tape = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
    seal(&tape, &dir, "sum");
    let got = assemble(
        &dir,
        &tape.read_all().unwrap(),
        AssembleMode::Independent,
        10_000,
    )
    .unwrap();
    assert!(got.context.is_empty());
    assert_eq!(got.config, "{\"k\":1}");
    assert!(!got.prompt.is_empty());
}

#[test]
fn rebuild_restores_citations_after_the_sidecar_is_removed() {
    let tmp = TempDir::new("ctx-rebuild");
    let dir = session(&tmp);
    let tape = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
    let anchor = tape.read_all().unwrap().pop().unwrap();
    tape.append(AppendEvent {
        from: anchor.from,
        from_kind: FromKind::Agent,
        kind: Kind::Utterance,
        session: anchor.session.unwrap_or_default(),
        content: "kept".into(),
        tags: vec![],
        refs: vec![],
        act: None,
        card: anchor.card,
    })
    .unwrap();
    let md = seal(&tape, &dir, "sum");
    fs::remove_file(dir.join("ctx/index.jsonl")).unwrap();
    rebuild_index(&dir, &tape.read_all().unwrap()).unwrap();
    let rows = load_rows(&dir).unwrap();
    let sources = segment_sources(&dir, &md).unwrap();
    for (id, _) in sources {
        assert!(rows.iter().any(|r| r.event_id == id && r.segment.is_some()));
    }
}

fn thread_card(id: &str) -> AgentCard {
    AgentCard {
        v: 1,
        id: Uuid::parse_str(id).unwrap(),
        model: ModelSpec {
            base_url: "https://example.com".into(),
            model: "m".into(),
            extra_body: None,
        },
        prompt: "prompt-text".into(),
        tools: vec![ToolGrant {
            name: "bash".into(),
            tag: ToolTag::none_permit(Permit::Deny),
        }],
        topology: Topology::Leaf,
        kernel: "0.1.0".into(),
        supersedes: None,
    }
}

#[test]
fn thread_cut_is_written_by_the_host_and_a_hit_does_not_rewrite() {
    let tmp = TempDir::new("ctx-thread");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    let id = control
        .open_thread(&thread_card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"))
        .unwrap();
    control.advance(id, "hello from the host").unwrap();
    let cut = summarize_thread(&control, id, "the summary").unwrap();
    let md = fs::read(&cut.path).unwrap();
    let first = assemble_thread(&control, id, AssembleMode::Continue, 10_000).unwrap();
    assert!(
        first
            .context
            .iter()
            .any(|piece| piece.text == "the summary")
    );
    assert_eq!(first.prompt, "prompt-text");
    assert!(first.context.iter().all(|piece| piece.outside.is_none()));
    let index = fs::read(control.directory(id).unwrap().join("ctx/index.jsonl")).unwrap();
    let second = assemble_thread(&control, id, AssembleMode::Continue, 10_000).unwrap();
    assert_eq!(second.context, first.context);
    assert_eq!(fs::read(&cut.path).unwrap(), md);
    assert_eq!(
        fs::read(control.directory(id).unwrap().join("ctx/index.jsonl")).unwrap(),
        index
    );
    let alone = assemble_thread(&control, id, AssembleMode::Independent, 10_000).unwrap();
    assert!(alone.context.is_empty());
    assert_eq!(alone.prompt, "prompt-text");
    control
        .register(&Registration {
            events: vec![],
            config: vec!["theme".into()],
            env: vec!["pref".into()],
        })
        .unwrap();
    let config = control
        .put_config("theme", b"[hooks]\nbefore = []\n")
        .unwrap();
    let env = control
        .put_env("pref", "# now\n\nprefers short diffs\n")
        .unwrap();
    let cited = assemble_thread(&control, id, AssembleMode::Continue, 10_000).unwrap();
    assert!(cited.context.iter().any(|piece| {
        piece.kind == "config" && piece.outside.as_deref() == Some(config.id.as_str())
    }));
    assert!(cited.context.iter().any(|piece| {
        piece.kind == "env"
            && piece.outside.as_deref() == Some(env.id.as_str())
            && piece.text.contains("short diffs")
    }));
    let again = assemble_thread(&control, id, AssembleMode::Continue, 10_000).unwrap();
    assert_eq!(again.context, cited.context);
    control
        .put_env("pref", "# now\n\nprefers tests first\n")
        .unwrap();
    let moved = assemble_thread(&control, id, AssembleMode::Continue, 10_000).unwrap();
    assert!(
        moved
            .context
            .iter()
            .any(|piece| piece.text.contains("tests first"))
    );
    assert_ne!(moved.context, cited.context);
}

#[test]
fn tool_references_restore_context_and_cuts_without_putting_bodies_on_tape() {
    let dir = TempDir::new("tool-context");
    let store = JsonlStore::open(dir.path().join("memory.jsonl")).unwrap();
    let reference = store.tool_payload("private tool memory").unwrap();
    let event = store
        .append(AppendEvent {
            from: "a".into(),
            from_kind: FromKind::Agent,
            kind: Kind::Utterance,
            session: "s".into(),
            content: reference,
            tags: vec![
                "memory".into(),
                an_agent_core::memstream::TOOL_PAYLOAD_TAG.into(),
            ],
            refs: vec![],
            act: None,
            card: None,
        })
        .unwrap();
    let events = vec![event];
    let assembly = assemble(dir.path(), &events, AssembleMode::Continue, 10000).unwrap();
    assert!(
        assembly
            .context
            .iter()
            .any(|p| p.text == "private tool memory")
    );
    let cut = cut_if_long(dir.path(), &events, 0).unwrap().unwrap();
    assert!(
        fs::read_to_string(cut.path)
            .unwrap()
            .contains("private tool memory")
    );
    assert!(
        !fs::read_to_string(dir.path().join("memory.jsonl"))
            .unwrap()
            .contains("private tool memory")
    );
    fs::remove_dir_all(dir.path().join("blobs")).unwrap();
    assert!(assemble(dir.path(), &events, AssembleMode::Continue, 10000).is_err());
}

#[test]
fn thin_tool_context_keeps_identity_in_assembly_and_cuts() {
    let dir = TempDir::new("tool-identity");
    let store = JsonlStore::open(dir.path().join("memory.jsonl")).unwrap();
    let reference = store.tool_payload("{}").unwrap();
    let events: Vec<_> = ["left", "right"]
        .into_iter()
        .map(|name| {
            store
                .append(AppendEvent {
                    from: "a".into(),
                    from_kind: FromKind::Agent,
                    kind: Kind::Action,
                    session: "s".into(),
                    content: reference.clone(),
                    tags: vec![an_agent_core::memstream::TOOL_PAYLOAD_TAG.into()],
                    refs: vec![],
                    card: None,
                    act: Some(an_agent_core::memstream::ActOnEvent {
                        kind: "invoke".into(),
                        permit: Permit::Go,
                        tool: Some(name.into()),
                        effect: None,
                    }),
                })
                .unwrap()
        })
        .collect();
    let assembly = assemble(dir.path(), &events, AssembleMode::Continue, 10000).unwrap();
    let cut = cut_if_long(dir.path(), &events, 0).unwrap().unwrap();
    let text = fs::read_to_string(cut.path).unwrap();
    for name in ["left", "right"] {
        let expected = format!("[tool \"{name}\"]\n{{}}");
        assert!(assembly.context.iter().any(|piece| piece.text == expected));
        assert!(text.contains(&expected));
    }
    assert_eq!(store.resolve_tool_payload(&events[0]).unwrap(), "{}");
}
