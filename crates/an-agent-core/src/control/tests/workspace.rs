//! Registration is a snapshot. Unknown writes are refused.

use super::listed;
use super::*;

#[test]
fn registration_is_a_snapshot_and_unknown_writes_are_refused() {
    let tmp = TempDir::new("control-register");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    assert!(matches!(
        control.push_event("stroke", "no"),
        Err(ControlError::NotRegistered)
    ));
    assert!(matches!(
        control.put_config("canvas", b"{}"),
        Err(ControlError::NotRegistered)
    ));
    assert!(matches!(
        control.put_env("stage", "# x\n"),
        Err(ControlError::NotRegistered)
    ));
    assert!(matches!(
        control.register(&listed(&[("stroke", &[]), ("stroke", &["clip"])], &[], &[])),
        Err(ControlError::InvalidRegistration(_))
    ));
    assert!(matches!(
        control.register(&listed(&[], &[""], &[])),
        Err(ControlError::InvalidRegistration(_))
    ));
    assert!(matches!(
        control.register(&listed(&[("stroke", &["clip", "clip"])], &[], &[])),
        Err(ControlError::InvalidRegistration(_))
    ));
    assert!(control.workspace_log().unwrap().is_empty());

    let spec = listed(&[("stroke", &["clip", "ink"])], &["canvas"], &["stage"]);
    let first = control.register(&spec).unwrap();
    let second = control.register(&spec).unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(
        control
            .workspace_log()
            .unwrap()
            .iter()
            .filter(|record| record.kind == "register")
            .count(),
        1
    );
    let sha = first.sha256.clone().unwrap();
    assert_eq!(
        control.registration_bytes(&sha).unwrap(),
        serde_json::to_vec(&spec).unwrap()
    );

    let before = control.workspace_log().unwrap().len();
    assert!(matches!(
        control.push_event("pressure", "1"),
        Err(ControlError::Unregistered(_))
    ));
    assert!(matches!(
        control.put_config("ink", b"{}"),
        Err(ControlError::Unregistered(_))
    ));
    assert!(matches!(
        control.put_env("note", "# n\n"),
        Err(ControlError::Unregistered(_))
    ));
    assert!(matches!(
        control.put_config("canvas", b"width: 512"),
        Err(ControlError::InvalidConfig(_))
    ));
    assert_eq!(control.workspace_log().unwrap().len(), before);
    control
        .put_config(
            "canvas",
            b"[[spool]]\nname = \"clip\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
    control.put_env("stage", "# draw\n").unwrap();
    control.push_event("stroke", "20,20").unwrap();

    let flipped = listed(&[("stroke", &["ink", "clip"])], &["canvas"], &["stage"]);
    let third = control.register(&flipped).unwrap();
    assert_ne!(third.id, first.id);
    assert_eq!(
        control.registration_bytes(&sha).unwrap(),
        serde_json::to_vec(&spec).unwrap()
    );
}

/// Config admission: TOML parsed as WorkspaceConfig, strict on shape.
#[test]
fn config_is_toml_and_strict() {
    let tmp = TempDir::new("control-config-toml");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control.register(&listed(&[], &["canvas"], &[])).unwrap();

    // A full document parses: spool pin with mount config, event contract,
    // hook chains.
    let full = br#"
[[spool]]
name = "translator"
version = "1.2.0"
config = { target_lang = "en" }

[event."discord.message"]
consumers = ["translator"]
schema = { type = "object", required = ["text"] }

[hooks]
before = [{ type = "spool", name = "schema_check", version = "1.0.0" }]
after = [{ type = "mcp", server = "guard", tool = "screen_reply" }]
"#;
    control.put_config("canvas", full).unwrap();

    // An empty document is a legal (all-default) config.
    control.put_config("canvas", b"").unwrap();

    // JSON bytes are no longer config: the format broke deliberately.
    assert!(matches!(
        control.put_config("canvas", br#"{"w":512}"#),
        Err(ControlError::InvalidConfig(_))
    ));
    // Unknown top-level key: refused, not ignored.
    assert!(matches!(
        control.put_config("canvas", b"chef = 1\n"),
        Err(ControlError::InvalidConfig(_))
    ));
    // Duplicate spool name.
    assert!(matches!(
        control.put_config(
            "canvas",
            b"[[spool]]\nname = \"a\"\nversion = \"1.0.0\"\n[[spool]]\nname = \"a\"\nversion = \"2.0.0\"\n"
        ),
        Err(ControlError::InvalidConfig(_))
    ));
    // Loose version: the shelf pins exact x.y.z.
    assert!(matches!(
        control.put_config("canvas", b"[[spool]]\nname = \"a\"\nversion = \"1\"\n"),
        Err(ControlError::InvalidConfig(_))
    ));
    // Empty event name.
    assert!(matches!(
        control.put_config("canvas", b"[event.\"\"]\n"),
        Err(ControlError::InvalidConfig(_))
    ));
    // Hook handlers are tagged mechanisms: a bare string no longer parses,
    // and a "command" handler is refused by the type system — arbitrary
    // host commands are not a hook kind.
    assert!(matches!(
        control.put_config("canvas", b"[hooks]\nbefore = [\"schema_check\"]\n"),
        Err(ControlError::InvalidConfig(_))
    ));
    assert!(matches!(
        control.put_config(
            "canvas",
            b"[hooks]\nbefore = [{ type = \"command\", command = \"rm -rf /\" }]\n"
        ),
        Err(ControlError::InvalidConfig(_))
    ));
    // Empty names inside a handler.
    assert!(matches!(
        control.put_config(
            "canvas",
            b"[hooks]\nbefore = [{ type = \"spool\", name = \"\", version = \"1.0.0\" }]\n"
        ),
        Err(ControlError::InvalidConfig(_))
    ));
    assert!(matches!(
        control.put_config(
            "canvas",
            b"[hooks]\nafter = [{ type = \"mcp\", server = \"\", tool = \"x\" }]\n"
        ),
        Err(ControlError::InvalidConfig(_))
    ));
    assert!(matches!(
        control.put_config(
            "canvas",
            b"[hooks]\nbefore = [{ type = \"spool\", name = \"a\", version = \"1\" }]\n"
        ),
        Err(ControlError::InvalidConfig(_))
    ));
}
