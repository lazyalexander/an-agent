//! Seal and resume. The live workspace is not rolled back.

use super::*;
use super::{card, listed};

#[test]
fn seal_records_the_workspace_generation_and_resume_reopens_the_tape() {
    let tmp = TempDir::new("control-session");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    let spec = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "bash", Permit::Deny);
    let registered = control
        .register(&listed(&[], &["canvas"], &["stage"]))
        .unwrap();
    let config = control.put_config("canvas", b"{\"a\":1}").unwrap();
    let env = control.put_env("stage", "# alpha\n").unwrap();
    let id = control.open_thread(&spec).unwrap();
    control.advance(id, "remember this").unwrap();
    let manifest = control.seal_session(id).unwrap();
    assert_eq!(manifest.config_id.as_deref(), Some(config.id.as_str()));
    assert_eq!(manifest.env_id.as_deref(), Some(env.id.as_str()));
    assert_eq!(
        manifest.register_id.as_deref(),
        Some(registered.id.as_str())
    );
    let register_sha = manifest.register_sha256.clone().unwrap();
    let register_bytes = control.registration_bytes(&register_sha).unwrap();
    control.put_env("stage", "# beta\n").unwrap();
    let old_sha = manifest.env_sha256.clone().unwrap();
    assert_eq!(control.env_bytes(&old_sha).unwrap(), b"# alpha\n");
    let next = control
        .register(&listed(&[], &["canvas"], &["stage", "note"]))
        .unwrap();
    assert_ne!(next.id, registered.id);
    assert_eq!(
        control.registration_bytes(&register_sha).unwrap(),
        register_bytes
    );
    assert_eq!(
        SessionManifest::read(&control.directory(id).unwrap()).unwrap(),
        manifest
    );
    control.finish(id, "done").unwrap();
    control.release(id).unwrap();
    let resumed = control.resume_session(&spec, manifest.id).unwrap();
    assert_eq!(resumed, id);
    assert!(
        control
            .events(id)
            .unwrap()
            .iter()
            .any(|event| { event.content.contains("remember this") })
    );
    assert!(
        control
            .events(id)
            .unwrap()
            .iter()
            .any(|event| { event.tags.iter().any(|tag| tag == "resume") })
    );
    control.finish(id, "done").unwrap();
    control.release(id).unwrap();
    control.open_thread(&spec).unwrap();
    assert!(
        control
            .events(id)
            .unwrap()
            .iter()
            .all(|event| !event.content.contains("remember this"))
    );
    let old = crate::agent::Session::open(tmp.path(), spec.id, manifest.id).unwrap();
    assert!(
        old.tape()
            .read_all()
            .unwrap()
            .iter()
            .any(|event| { event.content.contains("remember this") })
    );
}
