use super::*;
use std::{os::unix::net::UnixListener, thread};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from("/tmp").canonicalize().unwrap().join(format!(
            "an-io-{}",
            an_agent_core::det_seam::Entropy::os().uuid_v4()
        ));
        store::directory(&root).unwrap();
        Self(root)
    }
    fn profile(&self) -> Profile {
        Profile {
            socket: self.0.join("cc.sock"),
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
fn serve(listener: UnixListener, bytes: Vec<u8>, upload: bool) -> thread::JoinHandle<Vec<Value>> {
    thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..bytes.len().max(1).div_ceil(CHUNK) {
            let (mut stream, _) = listener.accept().unwrap();
            let mut header = [0; 4];
            stream.read_exact(&mut header).unwrap();
            let mut data = vec![0; u32::from_be_bytes(header) as usize];
            stream.read_exact(&mut data).unwrap();
            let request: Value = serde_json::from_slice(&data).unwrap();
            let guest = &request["operation"]["request"];
            let operation = &guest["operation"];
            let offset = operation["offset"].as_u64().unwrap() as usize;
            let end = (offset + CHUNK).min(bytes.len());
            let result = if upload {
                assert_eq!(
                    decode(operation["content_hex"].as_str().unwrap()).unwrap(),
                    bytes[offset..end]
                );
                assert_eq!(operation["sync"], end == bytes.len());
                json!({"kind":"written"})
            } else {
                json!({"kind":"chunk","content_hex":bytes[offset..end].iter().map(|b|format!("{b:02x}")).collect::<String>(),"next_offset":end,"eof":end==bytes.len()})
            };
            let result = json!({"identity":guest["identity"],"result":{"Ok":result}});
            let body=serde_json::to_vec(&json!({"request":request["request"],"caller":request["caller"],"result":{"Ok":result}})).unwrap();
            stream
                .write_all(&(body.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(&body).unwrap();
            requests.push(request);
        }
        requests
    })
}
fn plan(direction: &str, bytes: &[u8]) -> Value {
    json!({"direction":direction,"path":"/workspace/document.bin","attempt":an_agent_core::det_seam::Entropy::os().uuid_v4().to_string(),"sha256":store::digest(bytes),"bytes":bytes.len()})
}
#[test]
fn binary_download_is_bounded_digest_checked_and_not_accepted() {
    let f = Fixture::new();
    let p = f.profile();
    let data: Vec<_> = (0..10000).map(|i| (i % 256) as u8).collect();
    let server = serve(UnixListener::bind(&p.socket).unwrap(), data.clone(), false);
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let plan = plan("download", &data);
    let result = run(&f.0, &p, &id, plan.clone(), "task", || Ok(())).unwrap();
    assert_eq!(result["state"], "staged");
    assert_eq!(result["artifact_accepted"], false);
    assert_eq!(fs::read(f.0.join(&id).join("candidate.bin")).unwrap(), data);
    assert_eq!(server.join().unwrap().len(), 2);
    // Re-enter from durable chunk receipts with no backend listener.
    assert_eq!(
        run(&f.0, &p, &id, plan.clone(), "task", || Ok(())).unwrap(),
        result
    );
    assert!(run(&f.0, &p, &id, plan, "another-task", || Ok(())).is_err());
}
#[test]
fn binary_upload_checks_source_and_replays_no_guest_writes() {
    let f = Fixture::new();
    let p = f.profile();
    let bytes = vec![0xff; 9000];
    let source = f.0.join("source.pdf");
    store::write_new(&source, &bytes).unwrap();
    let mut plan = plan("upload", &bytes);
    plan["source"] = json!(source);
    let server = serve(UnixListener::bind(&p.socket).unwrap(), bytes, true);
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    assert_eq!(
        run(&f.0, &p, &id, plan.clone(), "task", || Ok(())).unwrap()["state"],
        "staged"
    );
    assert_eq!(server.join().unwrap().len(), 2);
    assert_eq!(
        run(&f.0, &p, &id, plan.clone(), "task", || Ok(())).unwrap()["state"],
        "staged"
    );
    fs::write(source, b"changed").unwrap();
    assert!(run(&f.0, &p, &id, plan, "task", || Ok(())).is_err());
}
#[test]
fn digest_failure_and_cancellation_leave_no_candidate() {
    let f = Fixture::new();
    let p = f.profile();
    let bytes = b"xlsx bytes".to_vec();
    let server = serve(UnixListener::bind(&p.socket).unwrap(), bytes.clone(), false);
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let mut plan = plan("download", &bytes);
    plan["sha256"] = json!("a".repeat(64));
    assert!(run(&f.0, &p, &id, plan.clone(), "task", || Ok(())).is_err());
    server.join().unwrap();
    assert!(!f.0.join(&id).join("candidate.bin").exists());
    assert!(run(&f.0, &p, &id, plan, "task", || Err("revoked".into())).is_err());
}
#[test]
fn invalid_paths_and_unbounded_transfers_are_rejected_before_transport() {
    let f = Fixture::new();
    let p = f.profile();
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    for path in ["/workspace/../private", "/private/input", "relative"] {
        let mut plan = plan("download", b"");
        plan["path"] = json!(path);
        assert!(run(&f.0, &p, &id, plan, "task", || Ok(())).is_err());
    }
    let mut plan = plan("download", b"");
    plan["bytes"] = json!(LIMIT + 1);
    assert!(run(&f.0, &p, &id, plan, "task", || Ok(())).is_err());
    assert!(!f.0.join(id).exists());
}

#[test]
fn empty_and_exact_chunk_outputs_preserve_binary_boundaries() {
    for bytes in [Vec::new(), vec![0; CHUNK]] {
        let f = Fixture::new();
        let p = f.profile();
        let server = serve(UnixListener::bind(&p.socket).unwrap(), bytes.clone(), false);
        let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
        let result = run(&f.0, &p, &id, plan("download", &bytes), "task", || Ok(())).unwrap();
        assert_eq!(result["state"], "staged");
        assert_eq!(fs::read(f.0.join(id).join("candidate.bin")).unwrap(), bytes);
        assert_eq!(server.join().unwrap().len(), 1);
    }
}
