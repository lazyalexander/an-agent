use super::*;

const BASH: &str = r#"
v: 1
name: bash
version: 1.0.0
constructor: rhai
script: |
  let out = exec(params.command, #{ timeout: 120 });
  out.stdout
summary: Run a shell command
effect:
  proc: spawn
  file: { op: unbounded }
  net: egress
  memory: { op: ignore }
requires: []
"#;

const MCP: &str = r#"
v: 1
name: fs_read
version: 1.0.0
constructor: mcp
mcp:
  transport: stdio
  command: ["npx", "-y", "server-filesystem", "/srv"]
  tool: read_file
summary: Read files under /srv
effect:
  file: { op: r, path: "/srv" }
  net: none
  proc: none
  memory: { op: ignore }
requires: []
"#;

#[test]
fn parses_rhai_and_mcp() {
    let d = parse(BASH).unwrap();
    assert!(matches!(d.constructor, Constructor::Rhai { .. }));
    assert_eq!(d.effect.net, Net::Egress);
    assert_eq!(d.effect.proc_, Proc::Spawn);
    let m = parse(MCP).unwrap();
    assert!(matches!(m.constructor, Constructor::Mcp { .. }));
}

/// Rejection tests must pin the *reason*: a test that only checks
/// `is_err()` passes even when the descriptor fails for the wrong cause
/// (e.g. an accidental YAML syntax break) — a fake-green branch.
fn err_of(yaml: &str) -> String {
    parse(yaml).unwrap_err().to_string()
}

#[test]
fn rejects_unknown_field() {
    let bad = BASH.replace("summary:", "summray:");
    assert!(err_of(&bad).contains("unknown field"));
}

#[test]
fn rejects_oversized_input_before_parsing() {
    let huge = format!("{BASH}{}", " ".repeat(MAX_DESCRIPTOR_BYTES));
    assert!(err_of(&huge).contains("too large"));
}

#[test]
fn rejects_wrong_v_and_sloppy_identity() {
    assert!(err_of(&BASH.replace("v: 1", "v: 2")).contains("v must be 1"));
    assert!(err_of(&BASH.replace("name: bash", "name: Bash")).contains("invalid tool name"));
    assert!(err_of(&BASH.replace("version: 1.0.0", "version: 1.0")).contains("invalid version"));
}

#[test]
fn enforces_constructor_block_pairing() {
    let no_script = BASH.replace(
        "script: |\n  let out = exec(params.command, #{ timeout: 120 });\n  out.stdout\n",
        "",
    );
    assert!(err_of(&no_script).contains("requires a non-empty script"));
    let both = BASH.replace(
        "requires: []",
        "mcp: { transport: stdio, command: [\"x\"], tool: t }\nrequires: []",
    );
    assert!(err_of(&both).contains("must not carry an mcp block"));
    assert!(err_of(&MCP.replace("transport: stdio", "transport: http")).contains("must be stdio"));
}

#[test]
fn effect_facets_reject_swallowed_keys() {
    // Internally tagged enums silently drop extra keys; without
    // Value-level checks these would look like restrictions but admit
    // none/unbounded/ignore.
    let none_path = MCP.replace(
        "file: { op: r, path: \"/srv\" }",
        "file: { op: none, path: \"/etc/passwd\" }",
    );
    assert!(err_of(&none_path).contains("unknown key"));
    let unbounded_recursive = BASH.replace(
        "file: { op: unbounded }",
        "file: { op: unbounded, recursive: true }",
    );
    assert!(err_of(&unbounded_recursive).contains("unknown key"));
    let ignore_aspect = BASH.replace(
        "memory: { op: ignore }",
        "memory: { op: ignore, aspect: \"x\" }",
    );
    assert!(err_of(&ignore_aspect).contains("unknown key"));
}

#[test]
fn effect_facets_pin_shape_and_types() {
    let bare = MCP.replace("file: { op: r, path: \"/srv\" }", "file: r");
    assert!(err_of(&bare).contains("must be a mapping"));
    let no_path = MCP.replace("file: { op: r, path: \"/srv\" }", "file: { op: r }");
    assert!(err_of(&no_path).contains("requires a string path"));
    let bad_recursive = MCP.replace(
        "file: { op: r, path: \"/srv\" }",
        "file: { op: r, path: \"/srv\", recursive: \"maybe\" }",
    );
    assert!(err_of(&bad_recursive).contains("recursive must be a bool"));
    let unknown_op = MCP.replace("file: { op: r, path: \"/srv\" }", "file: { op: x }");
    assert!(err_of(&unknown_op).contains("unknown effect.file op"));
    // Valid full forms still parse.
    let full = MCP.replace(
        "file: { op: r, path: \"/srv\" }",
        "file: { op: r, path: \"/srv\", recursive: true }",
    );
    assert!(parse(&full).is_ok());
    let remember = BASH.replace(
        "memory: { op: ignore }",
        "memory: { op: remember, aspect: \"prefs\" }",
    );
    assert!(parse(&remember).is_ok());
}

#[test]
fn effect_faces_required_and_constrained() {
    let no_net = BASH.replace("  net: egress\n", "");
    assert!(err_of(&no_net).contains("net"));
    let rel = MCP.replace("path: \"/srv\"", "path: \"srv/data\"");
    assert!(err_of(&rel).contains("absolute"));
    let forget = BASH.replace(
        "memory: { op: ignore }",
        "memory: { op: forget, rememberId: x }",
    );
    assert!(err_of(&forget).contains("ignore or remember"));
}

#[test]
fn requires_strictness() {
    let missing = BASH.replace("requires: []\n", "");
    assert!(err_of(&missing).contains("requires is required"));
    let self_ref = BASH.replace("requires: []", "requires: [{ name: bash, version: 1.0.0 }]");
    assert!(err_of(&self_ref).contains("requires itself"));
    let dup = BASH.replace(
        "requires: []",
        "requires:\n  - { name: a, version: 1.0.0 }\n  - { name: a, version: 2.0.0 }",
    );
    assert!(err_of(&dup).contains("duplicate require"));
}

#[test]
fn presence_check_is_lookup_only() {
    let d = parse(&BASH.replace(
        "requires: []",
        "requires: [{ name: http_fetch, version: 1.4.0 }]",
    ))
    .unwrap();
    let mut index = HashSet::new();
    assert!(check_requires_present(&d, &index).is_err());
    index.insert(("http_fetch".into(), "1.4.0".into()));
    assert!(check_requires_present(&d, &index).is_ok());
}

#[test]
fn content_hash_is_stable() {
    assert_eq!(content_hash(BASH), content_hash(BASH));
    assert_ne!(content_hash(BASH), content_hash(MCP));
}
