//! Descriptor bodies for the translated-tell probe.

pub(super) const TRANSLATOR_YAML: &str = r#"
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

pub(super) const EDITOR_YAML: &str = r#"
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

pub(super) const BOX_YAML: &str = r#"
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

pub(super) const CLERK_YAML: &str = r#"
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

pub(super) const SCRIBE_YAML: &str = r#"
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

pub(super) const NAIVE_YAML: &str = r#"
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
