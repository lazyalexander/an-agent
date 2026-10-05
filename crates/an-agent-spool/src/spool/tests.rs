use super::*;
use an_agent_core::testkit::TempDir;

fn read_yaml() -> String {
    r#"
v: 1
kind: spool
name: fs_read
version: 1.0.0
summary: Read a workspace file
constructor: host
host: fs_read
effect:
  file: { op: r, path: "/srv" }
  memory: { op: ignore }
  net: none
  proc: none
  flow: in
inverse: irreversible
requires: []
"#
    .to_string()
}

fn write_yaml() -> String {
    r#"
v: 1
kind: spool
name: fs_write
version: 1.0.0
summary: Write a workspace file
constructor: rhai
script: |
  write(params.path, params.body)
effect:
  file: { op: w, path: "/srv", recursive: true }
  memory: { op: ignore }
  net: none
  proc: none
  flow: none
inverse:
  name: fs_restore
  version: 1.0.0
config:
  timeout_ms: 1000
requires:
  - { name: fs_read, version: 1.0.0 }
"#
    .to_string()
}

#[test]
fn parses_a_hosted_spool() {
    let spec = parse(&read_yaml()).unwrap();
    assert!(matches!(spec.constructor, Constructor::Host { .. }));
    assert_eq!(spec.effect.flow, Flow::In);
    assert_eq!(spec.inverse, Inverse::Irreversible);
    assert!(spec.requires.is_empty());
    assert_eq!(spec.sha256.len(), 64);
}

#[test]
fn rejects_a_tool_an_mcp_block_and_a_permit() {
    let tool = read_yaml().replace("kind: spool", "kind: tool");
    assert!(
        parse(&tool)
            .unwrap_err()
            .to_string()
            .contains("kind must be spool")
    );
    let mcp = read_yaml().replace("constructor: host\nhost: fs_read", "constructor: mcp");
    assert!(
        parse(&mcp)
            .unwrap_err()
            .to_string()
            .contains("rhai or host")
    );
    let permit = read_yaml().replace("kind: spool", "kind: spool\npermit: go");
    assert!(
        parse(&permit)
            .unwrap_err()
            .to_string()
            .contains("unknown field")
    );
}

#[test]
fn ceiling_is_per_face() {
    let read = parse(&read_yaml()).unwrap();
    let mut rights = read.effect.clone();
    assert!(covers(&rights, &read.effect));
    rights.net = Net::None;
    rights.flow = Flow::None;
    assert!(!covers(&rights, &read.effect));
    rights.flow = Flow::Both;
    rights.net = Net::Egress;
    assert!(covers(&rights, &read.effect));
}

#[test]
fn published_versions_stay_readable() {
    let tmp = TempDir::new("spools");
    let reg = Registry::open(tmp.path()).unwrap();
    let reader = reg.publish(&read_yaml()).unwrap();
    assert!(matches!(reader.constructor, Constructor::Host { .. }));
    let restore = read_yaml()
        .replace("name: fs_read", "name: fs_restore")
        .replace("host: fs_read", "host: fs_restore")
        .replace("summary: Read a workspace file", "summary: Restore");
    reg.publish(&restore).unwrap();
    let first = reg.publish(&write_yaml()).unwrap();
    let again = reg.publish(&write_yaml()).unwrap();
    assert_eq!(first.sha256, again.sha256);
    let changed = write_yaml().replace("timeout_ms: 1000", "timeout_ms: 2000");
    assert!(reg.publish(&changed).is_err());
    let next = write_yaml().replace(
        "name: fs_write\nversion: 1.0.0",
        "name: fs_write\nversion: 1.0.1",
    );
    reg.publish(&next).unwrap();
    let old = reg.recover("fs_write", "1.0.0").unwrap();
    assert_eq!(old.body, write_yaml());
    assert_eq!(old.sha256, first.sha256);
    let by_hash = reg.recover_hash(&first.sha256).unwrap();
    assert_eq!(by_hash.summary, "Write a workspace file");
    let history = reg.versions("fs_write").unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].version, "1.0.0");
    assert_eq!(history[1].version, "1.0.1");
    assert!(
        reg.publish(
            &write_yaml()
                .replace(
                    "inverse:\n  name: fs_restore\n  version: 1.0.0",
                    "inverse:\n  name: missing\n  version: 9.9.9"
                )
                .replace("version: 1.0.0", "version: 2.0.0")
        )
        .unwrap_err()
        .to_string()
        .contains("not published")
    );
}

#[test]
fn closure_folds_ceiling_and_reversibility() {
    let tmp = TempDir::new("spool-closure");
    let reg = Registry::open(tmp.path()).unwrap();
    reg.publish(&read_yaml()).unwrap();
    let restore = read_yaml()
        .replace("name: fs_read", "name: fs_restore")
        .replace("host: fs_read", "host: fs_restore")
        .replace("summary: Read a workspace file", "summary: Restore");
    reg.publish(&restore).unwrap();
    reg.publish(&write_yaml()).unwrap();

    let closure = reg.closure("fs_write", "1.0.0").unwrap();
    // file faces join: w /srv (recursive) ∪ r /srv = rw /srv recursive.
    assert_eq!(
        closure.ceiling.file,
        FileFacet::ReadWrite {
            path: "/srv".into(),
            recursive: true
        }
    );
    // flow joins across the closure: write is none, read is in.
    assert_eq!(closure.ceiling.flow, Flow::In);
    assert_eq!(closure.ceiling.net, Net::None);
    // fs_write has an inverse but requires fs_read, which is
    // irreversible — the whole mount cannot be unwound.
    assert!(!closure.reversible);
    assert_eq!(closure.members.len(), 2);

    // The root's own faces alone would under-report: read alone does
    // not cover the closure ceiling, and that is the point of the fold.
    let root = reg.recover("fs_write", "1.0.0").unwrap();
    assert!(covers(&closure.ceiling, &root.effect));
    let read_only = reg.recover("fs_read", "1.0.0").unwrap();
    assert!(!covers(&read_only.effect, &closure.ceiling));
}

#[test]
fn inverse_none_parses_and_keeps_the_closure_reversible() {
    let pure = read_yaml().replace("inverse: irreversible", "inverse: none");
    let spec = parse(&pure).unwrap();
    assert_eq!(spec.inverse, Inverse::None);
    let tmp = TempDir::new("spool-inverse-none");
    let reg = Registry::open(tmp.path()).unwrap();
    reg.publish(&pure).unwrap();
    // None means detach-only at unmount; unlike Irreversible it does
    // not taint the whole closure's reversibility fold.
    assert!(reg.closure("fs_read", "1.0.0").unwrap().reversible);
    let garbage = read_yaml().replace("inverse: irreversible", "inverse: maybe");
    assert!(
        parse(&garbage)
            .unwrap_err()
            .to_string()
            .contains("inverse must be")
    );
}
