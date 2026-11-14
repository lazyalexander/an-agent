use super::*;
use std::{os::unix::net::UnixListener, thread};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let dir = PathBuf::from("/tmp").canonicalize().unwrap().join(format!(
            "an-cc-{}",
            an_agent_core::det_seam::Entropy::os().uuid_v4()
        ));
        store::directory(&dir).unwrap();
        Self(dir)
    }
    fn profile(&self) -> Profile {
        Profile {
            socket: self.0.join("api.sock"),
            grant: "a".repeat(64),
            caller: "main".into(),
            vm: an_agent_core::det_seam::Entropy::os().uuid_v4().to_string(),
            generation: 1,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn transport_loss_fences_replay_and_rejects_changed_intent() {
    let f = Fixture::new();
    let p = f.profile();
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let operation = json!({"operation":"stop","vm":p.vm});
    assert_eq!(
        submit(&f.0, &p, &id, operation.clone()).unwrap()["state"],
        "unknown"
    );
    let listener = UnixListener::bind(&p.socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    assert_eq!(
        submit(&f.0, &p, &id, operation).unwrap()["state"],
        "unknown"
    );
    assert!(listener.accept().is_err());
    assert!(submit(&f.0, &p, &id, json!({"operation":"context","vm":p.vm})).is_err());
    let journal = fs::read_to_string(f.0.join(format!("{id}.json"))).unwrap();
    assert!(!journal.contains(&p.grant));
}
#[test]
fn foreign_generation_and_management_never_reach_transport() {
    let f = Fixture::new();
    let p = f.profile();
    assert!(validate(&p, &json!({"operation":"issue_grant"})).is_err());
    assert!(validate(&p,&json!({"operation":"guest","request":{"identity":{"vm":p.vm,"generation":2,"caller":p.caller}}})).is_err());
    assert!(
        validate(
            &p,
            &json!({"operation":"stop","vm":an_agent_core::det_seam::Entropy::os().uuid_v4()})
        )
        .is_err()
    );
}
#[test]
fn reply_is_correlated_and_never_accepts_artifact() {
    let f = Fixture::new();
    let p = f.profile();
    let listener = UnixListener::bind(&p.socket).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut header = [0; 4];
        stream.read_exact(&mut header).unwrap();
        let mut bytes = vec![0; u32::from_be_bytes(header) as usize];
        stream.read_exact(&mut bytes).unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        let response=serde_json::to_vec(&json!({"request":request["request"],"caller":request["caller"],"result":{"Ok":{"state":"running"}}})).unwrap();
        stream
            .write_all(&(response.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&response).unwrap();
    });
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let result = submit(&f.0, &p, &id, json!({"operation":"context","vm":p.vm})).unwrap();
    assert_eq!(result["state"], "observed");
    assert_eq!(result["artifact_accepted"], false);
    assert_eq!(observe(&f.0, &id).unwrap(), result);
    server.join().unwrap();
}

#[test]
fn reconciliation_queries_original_request_without_reexecution() {
    let f = Fixture::new();
    let p = f.profile();
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    submit(&f.0, &p, &id, json!({"operation":"stop","vm":p.vm})).unwrap();
    let binding: Value =
        serde_json::from_slice(&fs::read(f.0.join(format!("{id}.json"))).unwrap()).unwrap();
    assert_ne!(binding["request"], id);
    let listener = UnixListener::bind(&p.socket).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut header = [0; 4];
        stream.read_exact(&mut header).unwrap();
        let mut bytes = vec![0; u32::from_be_bytes(header) as usize];
        stream.read_exact(&mut bytes).unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            request["operation"],
            json!({"operation":"operation_status","id":binding["request"]})
        );
        let bytes=serde_json::to_vec(&json!({"request":request["request"],"caller":request["caller"],"result":{"Ok":{"status":"succeeded","result":null}}})).unwrap();
        stream
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&bytes).unwrap();
    });
    assert_eq!(reconcile(&f.0, &p, &id).unwrap()["state"], "observed");
    server.join().unwrap();
    assert_eq!(
        submit(&f.0, &p, &id, json!({"operation":"stop","vm":p.vm})).unwrap()["state"],
        "observed"
    );
}

#[test]
fn reconnect_lists_receipts_and_rejects_reconciliation_target_change() {
    let f = Fixture::new();
    let mut p = f.profile();
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    submit(&f.0, &p, &id, json!({"operation":"stop","vm":p.vm})).unwrap();
    assert_eq!(receipts(&f.0).unwrap()[0]["operation"], id);
    p.generation += 1;
    assert!(reconcile(&f.0, &p, &id).is_err());
    assert_eq!(observe(&f.0, &id).unwrap()["state"], "unknown");
}

#[test]
fn capability_cannot_widen_profile_or_operation() {
    use crate::task_permissions::Permission;
    let permission = Permission::Cocoon {
        profile_sha256: "a".repeat(64),
        operation_sha256: "b".repeat(64),
    };
    assert!(permission.validate().is_ok());
    assert!(permission.within(&permission));
    let wider = Permission::Cocoon {
        profile_sha256: "a".repeat(64),
        operation_sha256: "c".repeat(64),
    };
    assert!(!wider.within(&permission));
    let invalid = Permission::Cocoon {
        profile_sha256: "invalid".into(),
        operation_sha256: "b".repeat(64),
    };
    assert!(invalid.validate().is_err());
}
