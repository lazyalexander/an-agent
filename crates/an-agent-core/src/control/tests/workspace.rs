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
    control.put_config("canvas", b"{\"w\":512}").unwrap();
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
