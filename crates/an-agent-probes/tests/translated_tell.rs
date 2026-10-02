//! Translated tell. One tape, existing mechanisms only: publish order,
//! closure ceilings against mount rights, a silk tell rewritten by an
//! enhancer, a reconstructed file write, and a tainted clip that never
//! reaches that write. Nothing here is a new kernel path.
//!
//! The human utterance is on the tape before the refused address. A tell
//! is what the gate refuses; an empty projection would halt and never
//! touch it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
mod support;

use std::sync::{Arc, Mutex};

use an_agent_core::act::{ActCtx, FileFacet, MemoryFacet, Permit, Tool, ToolCtx, ToolTag};
use an_agent_core::memstream::{AppendEvent, FromKind, JsonlStore, Kind, Memevent};
use an_agent_spool::descriptor::{Net, Proc};
use an_agent_spool::spool::{Faces, Flow, Registry};
use serde_json::{Map, Value, json};
use sha2::Digest;
use support::mount::{HostCtors, Mounter};
use support::silk::Envelope;
use support::{AgentError, Assistant, Model, StepOpts, TempDir, policy_step};

const SESSION: &str = "s1";

const TRANSLATOR_YAML: &str = r#"
v: 1
kind: spool
name: translator
version: 1.0.0
summary: prefix payload text
constructor: rhai
script: |
  let p = params.envelope.payload;
  #{ text: "译:" + p.text }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: both
inverse: none
requires: []
"#;

const EDITOR_YAML: &str = r#"
v: 1
kind: spool
name: editor
version: 1.0.0
summary: write a file
constructor: host
host: editor
effect:
  file: { op: w, path: "/srv", recursive: true }
  memory: { op: ignore }
  net: none
  proc: none
  flow: none
inverse: irreversible
requires: []
"#;

const BOX_YAML: &str = r#"
v: 1
kind: spool
name: box
version: 1.0.0
summary: utter the translated tell preview
constructor: rhai
script: |
  let inbox = params.clips.filter(|c| c.tags.contains("silk") && c.tags.contains("tell"));
  if inbox.is_empty() {
      #{ kind: "halt" }
  } else {
      #{ kind: "utter", text: inbox[0].preview }
  }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: in
inverse: none
requires:
  - { name: translator, version: 1.0.0 }
"#;

const CLERK_YAML: &str = r#"
v: 1
kind: spool
name: clerk
version: 1.0.0
summary: tell config.to the human preview
constructor: rhai
script: |
  let human = params.clips.filter(|c| c.kind == "utterance" && c.from == "human");
  let told = params.clips.filter(|c| c.who == config.name && c.kind == "action" && c.tags.contains("tell"));
  if human.is_empty() || !told.is_empty() {
      #{ kind: "halt" }
  } else {
      #{ kind: "silk", silk: #{ kind: "tell", to: config.to, payload: #{ text: human[0].preview } } }
  }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: out
inverse: none
requires:
  - { name: box, version: 1.0.0 }
"#;

const SCRIBE_YAML: &str = r#"
v: 1
kind: spool
name: scribe
version: 1.0.0
summary: write the translated line it authored
constructor: rhai
script: |
  #{ kind: "invoke_tool", name: "editor", args: #{ path: "out.txt", content: "译:hello" } }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: none
inverse: none
requires:
  - { name: editor, version: 1.0.0 }
"#;

const NAIVE_YAML: &str = r#"
v: 1
kind: spool
name: naive
version: 1.0.0
summary: forward the human clip into the file
constructor: rhai
script: |
  let human = params.clips.filter(|c| c.from == "human");
  if human.is_empty() {
      #{ kind: "halt" }
  } else {
      #{ kind: "invoke_tool", name: "editor", args: #{ path: "out.txt", content: #{ "$clip": human[0].id } } }
  }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: none
inverse: none
requires:
  - { name: editor, version: 1.0.0 }
"#;

struct ScriptedEditor {
    written: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl Tool for ScriptedEditor {
    fn name(&self) -> &str {
        "editor"
    }

    fn description(&self) -> &str {
        "scripted file writer"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            }
        })
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        Some(ToolTag::none())
    }

    async fn execute(&self, args: Value, _ctx: &ToolCtx) -> Result<String, String> {
        let content = args["content"].as_str().unwrap_or("").to_string();
        self.written.lock().unwrap().push(content.clone());
        Ok(format!("wrote {} bytes", content.len()))
    }
}

struct NoModel;

impl Model for NoModel {
    async fn complete(
        &self,
        _messages: &[support::ChatMessage],
        _tools: &[Arc<dyn Tool>],
    ) -> Result<Assistant, AgentError> {
        Ok(Assistant {
            content: String::new(),
            tool_calls: vec![],
            usage: None,
        })
    }
}

fn ctx() -> ToolCtx {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolCtx { signal: Some(rx) }
}

fn faces(file: FileFacet, flow: Flow) -> Faces {
    Faces {
        file,
        memory: MemoryFacet::Ignore,
        net: Net::None,
        proc_: Proc::None,
        flow,
    }
}

fn out_only() -> Faces {
    faces(FileFacet::None, Flow::Out)
}

fn file_none() -> Faces {
    faces(FileFacet::None, Flow::Both)
}

fn full_rights() -> Faces {
    faces(
        FileFacet::Write {
            path: "/srv".into(),
            recursive: true,
        },
        Flow::Both,
    )
}

fn cfg(pairs: &[(&str, &str)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), Value::String((*v).to_string())))
        .collect()
}

fn mount_events<'a>(events: &'a [Memevent], name: &str) -> Vec<&'a Memevent> {
    events
        .iter()
        .filter(|e| {
            e.act
                .as_ref()
                .is_some_and(|a| a.kind == "mount" && a.tool.as_deref() == Some(name))
        })
        .collect()
}

fn tells(store: &JsonlStore) -> Vec<Envelope> {
    store
        .read_all()
        .unwrap()
        .into_iter()
        .filter(|e| e.tags.iter().any(|t| t == "tell"))
        .filter_map(|e| serde_json::from_str(&e.content).ok())
        .collect()
}

async fn step(
    mounter: &Mounter<'_>,
    store: &JsonlStore,
    who: &str,
    id: an_agent_spool::scope::ScopeId,
    tools: &[Arc<dyn Tool>],
) -> Result<bool, AgentError> {
    let actx = ActCtx {
        store: Some(store),
        agent_id: who,
        session: SESSION,
        card: None,
    };
    let policy = mounter.policy(id).unwrap();
    policy_step(
        &actx,
        &NoModel,
        &policy,
        tools,
        &ctx(),
        StepOpts {
            silk: Some(mounter),
            ..StepOpts::default()
        },
    )
    .await
}

#[tokio::test]
async fn translated_tell() {
    let tmp = TempDir::new("translated-tell");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let registry = Registry::open(tmp.path().join("spools")).unwrap();

    let early = registry.publish(CLERK_YAML).unwrap_err();
    assert!(
        early
            .to_string()
            .contains("required spool is not published"),
        "{early}"
    );
    assert!(store.read_all().unwrap().is_empty());

    for yaml in [
        TRANSLATOR_YAML,
        EDITOR_YAML,
        BOX_YAML,
        SCRIBE_YAML,
        CLERK_YAML,
        NAIVE_YAML,
    ] {
        registry.publish(yaml).unwrap();
    }
    let clerk = registry.closure("clerk", "1.0.0").unwrap();
    assert_eq!(clerk.ceiling.flow, Flow::Both);
    assert_eq!(clerk.ceiling.file, FileFacet::None);
    assert!(clerk.members.iter().any(|(n, _)| n == "clerk"));
    assert!(clerk.members.iter().any(|(n, _)| n == "box"));
    assert!(clerk.members.iter().any(|(n, _)| n == "translator"));
    assert!(clerk.members.iter().all(|(n, _)| n != "editor"));
    let scribe = registry.closure("scribe", "1.0.0").unwrap();
    assert_eq!(
        scribe.ceiling.file,
        FileFacet::Write {
            path: "/srv".into(),
            recursive: true,
        }
    );

    let editor = Arc::new(ScriptedEditor {
        written: Mutex::new(vec![]),
    });
    let mut hosts = HostCtors::new();
    {
        let editor = editor.clone();
        hosts.insert(
            "editor".into(),
            Box::new(move |_cfg: &Map<String, Value>| Ok(editor.clone() as Arc<dyn Tool>)),
        );
    }
    let hostx = ActCtx {
        store: Some(&store),
        agent_id: "host",
        session: SESSION,
        card: None,
    };
    let mut mounter = Mounter::new(&hostx, &registry, hosts);
    let rights = full_rights();

    let denied_clerk = mounter
        .mount(
            None,
            CLERK_YAML,
            cfg(&[("name", "clerk"), ("to", "ghost")]),
            &out_only(),
            Some("clerk".into()),
            None,
        )
        .unwrap_err();
    assert!(
        denied_clerk.to_string().contains("exceeds rights"),
        "{denied_clerk}"
    );
    let denied_scribe = mounter
        .mount(
            None,
            SCRIBE_YAML,
            Map::new(),
            &file_none(),
            Some("scribe".into()),
            None,
        )
        .unwrap_err();
    assert!(
        denied_scribe.to_string().contains("exceeds rights"),
        "{denied_scribe}"
    );
    assert!(mounter.tree().active_roots().is_empty());
    let events = store.read_all().unwrap();
    assert!(mount_events(&events, "clerk").iter().any(|e| {
        e.act
            .as_ref()
            .is_some_and(|a| a.tag.permit() == Permit::Deny)
    }));
    assert!(mount_events(&events, "scribe").iter().any(|e| {
        e.act
            .as_ref()
            .is_some_and(|a| a.tag.permit() == Permit::Deny)
    }));
    assert!(events.iter().all(|e| {
        e.act
            .as_ref()
            .is_none_or(|a| a.kind != "mount" || a.tag.permit() == Permit::Deny)
    }));

    mounter
        .mount(
            None,
            TRANSLATOR_YAML,
            Map::new(),
            &rights,
            Some("translator".into()),
            None,
        )
        .unwrap();
    mounter
        .mount(
            None,
            EDITOR_YAML,
            Map::new(),
            &rights,
            Some("editor".into()),
            None,
        )
        .unwrap();
    mounter
        .mount(
            None,
            BOX_YAML,
            Map::new(),
            &rights,
            Some("box".into()),
            None,
        )
        .unwrap();
    let scribe_id = mounter
        .mount(
            None,
            SCRIBE_YAML,
            Map::new(),
            &rights,
            Some("scribe".into()),
            None,
        )
        .unwrap();
    let clerk_id = mounter
        .mount(
            None,
            CLERK_YAML,
            cfg(&[("name", "clerk"), ("to", "ghost")]),
            &rights,
            Some("clerk".into()),
            None,
        )
        .unwrap();
    let naive_id = mounter
        .mount(
            None,
            NAIVE_YAML,
            Map::new(),
            &rights,
            Some("naive".into()),
            None,
        )
        .unwrap();
    assert_eq!(mounter.tree().active_roots().len(), 6);

    mounter.link_enhancer("box", "translator").unwrap();
    store
        .append(AppendEvent {
            from: "terminal".into(),
            from_kind: FromKind::Human,
            kind: Kind::Utterance,
            session: SESSION.into(),
            content: "hello".into(),
            tags: vec![],
            refs: vec![],
            act: None,
            card: None,
        })
        .unwrap();

    let missed = step(&mounter, &store, "clerk", clerk_id, &[])
        .await
        .unwrap_err();
    assert!(
        missed.to_string().contains("no such silk address"),
        "{missed}"
    );
    assert!(tells(&store).is_empty());

    mounter.unmount(clerk_id).unwrap();
    let clerk_id = mounter
        .mount(
            None,
            CLERK_YAML,
            cfg(&[("name", "clerk"), ("to", "box")]),
            &rights,
            Some("clerk".into()),
            None,
        )
        .unwrap();
    step(&mounter, &store, "clerk", clerk_id, &[])
        .await
        .unwrap();

    let events = store.read_all().unwrap();
    let envelopes = tells(&store);
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].from, "clerk");
    assert_eq!(envelopes[0].to, "box");
    assert_eq!(envelopes[0].payload, json!({ "text": "译:hello" }));
    let enhance = events
        .iter()
        .find(|e| e.tags.iter().any(|t| t == "enhance"))
        .unwrap();
    let orig = format!("{:x}", sha2::Sha256::digest(br#"{"text":"hello"}"#));
    let translated = format!(
        "{:x}",
        sha2::Sha256::digest("{\"text\":\"译:hello\"}".as_bytes())
    );
    assert!(enhance.content.contains(&orig), "{}", enhance.content);
    assert!(enhance.content.contains(&translated), "{}", enhance.content);
    let envelope_event = events
        .iter()
        .find(|e| e.tags.iter().any(|t| t == "tell"))
        .unwrap();
    assert_eq!(envelope_event.refs, vec![enhance.id.clone()]);

    step(
        &mounter,
        &store,
        "box",
        mounter.silk_gate().admission("box").unwrap().scope,
        &[],
    )
    .await
    .unwrap();
    let events = store.read_all().unwrap();
    assert!(events.iter().any(|e| {
        e.kind == Kind::Utterance && e.from == "box" && e.content.contains("译:hello")
    }));

    let editor_tool = mounter.resolve("editor").unwrap();
    step(
        &mounter,
        &store,
        "scribe",
        scribe_id,
        std::slice::from_ref(&editor_tool),
    )
    .await
    .unwrap();
    assert_eq!(editor.written.lock().unwrap().as_slice(), ["译:hello"]);
    let events = store.read_all().unwrap();
    assert!(events.iter().any(|e| {
        e.kind == Kind::Action
            && e.from == "scribe"
            && e.content.contains("译:hello")
            && e.act
                .as_ref()
                .is_some_and(|a| a.tool.as_deref() == Some("editor"))
    }));

    let tainted = step(
        &mounter,
        &store,
        "naive",
        naive_id,
        std::slice::from_ref(&editor_tool),
    )
    .await
    .unwrap_err();
    assert!(tainted.to_string().contains("tainted"), "{tainted}");
    assert_eq!(editor.written.lock().unwrap().as_slice(), ["译:hello"]);
    let events = store.read_all().unwrap();
    assert!(events.iter().any(|e| e.tags.iter().any(|t| t == "taint")));

    let translator_id = mounter.silk_gate().admission("translator").unwrap().scope;
    let editor_scope = mounter.silk_gate().admission("editor").unwrap().scope;
    let report = mounter.unmount(translator_id).unwrap();
    assert_eq!(report[0].name, "translator");
    let report = mounter.unmount(editor_scope).unwrap();
    assert_eq!(report[0].name, "editor");
    let events = store.read_all().unwrap();
    assert!(events.iter().any(|e| {
        e.act
            .as_ref()
            .is_some_and(|a| a.kind == "unmount" && a.tool.as_deref() == Some("editor"))
            && e.content.contains("\"inverse\":\"irreversible\"")
    }));
    assert!(events.iter().any(|e| {
        e.act
            .as_ref()
            .is_some_and(|a| a.kind == "unmount" && a.tool.as_deref() == Some("translator"))
            && e.content.contains("\"inverse\":\"none\"")
    }));
    for id in mounter.tree().active_roots() {
        mounter.unmount(id).unwrap();
    }
    assert!(mounter.tree().active_roots().is_empty());
    assert_eq!(editor.written.lock().unwrap().as_slice(), ["译:hello"]);

    if let Ok(dir) = std::env::var("AN_AGENT_PROBE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(
            tmp.path().join("memory.jsonl"),
            std::path::Path::new(&dir).join("tape.jsonl"),
        )
        .unwrap();
    }
}
