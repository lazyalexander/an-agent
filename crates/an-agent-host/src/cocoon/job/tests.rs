use super::*;
use std::{
    os::unix::{fs::PermissionsExt, net::UnixListener},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
struct Fixture {
    root: PathBuf,
    spec: PathBuf,
    hash: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    executions: Arc<Mutex<usize>>,
}
impl Fixture {
    fn new(drop_reply: bool) -> Self {
        let root = PathBuf::from("/tmp").canonicalize().unwrap().join(format!(
            "an-job-{}",
            an_agent_core::det_seam::Entropy::os().uuid_v4()
        ));
        store::directory(&root).unwrap();
        let socket = root.join("cc.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let vm = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
        let attempt = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
        let profile = root.join("transport.json");
        store::write_new(&profile,&serde_json::to_vec(&json!({"socket":socket,"vm":vm,"generation":1,"caller":"primary","grant":"a".repeat(64)})).unwrap()).unwrap();
        fs::set_permissions(&profile, fs::Permissions::from_mode(0o600)).unwrap();
        let program = root.join("program");
        store::write_new(&program, b"fixed reviewed program").unwrap();
        let spec = root.join("spec.json");
        let bytes=serde_json::to_vec(&json!({"transport":profile,"transport_sha256":store::digest(&fs::read(&profile).unwrap()),
            "program":program,"program_sha256":store::digest(b"fixed reviewed program"),"interpreter":"/bin/sh",
            "arguments":["{program}","{request}","{output}"],"outputs":{"answer.txt":"utf-8"}})).unwrap();
        store::write_new(&spec, &bytes).unwrap();
        let hash = store::digest(&bytes);
        let stop = Arc::new(AtomicBool::new(false));
        let finish = Arc::clone(&stop);
        let executions = Arc::new(Mutex::new(0));
        let count = Arc::clone(&executions);
        let worker = thread::spawn(move || {
            let mut files = BTreeMap::<String, Vec<u8>>::new();
            let mut original = BTreeMap::<String, Value>::new();
            let mut stopped = false;
            while !finish.load(Ordering::Acquire) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut header = [0; 4];
                stream.read_exact(&mut header).unwrap();
                let mut bytes = vec![0; u32::from_be_bytes(header) as usize];
                stream.read_exact(&mut bytes).unwrap();
                let req: Value = serde_json::from_slice(&bytes).unwrap();
                let op = &req["operation"];
                let id = req["request"].as_str().unwrap();
                let result = match op["operation"].as_str().unwrap() {
                    "context" => {
                        json!({"vm":vm,"generation":1,"attempt":attempt,"state":"running","work":{"guest_ready":true,"actions":["inspect","guest","stop"]}})
                    }
                    "operation_status" => {
                        json!({"status":"succeeded","result":original[op["id"].as_str().unwrap()]})
                    }
                    "stop" => {
                        stopped = true;
                        Value::Null
                    }
                    "show" => {
                        json!({"id":vm,"generation":1,"active_attempt":attempt,"active_lease":if stopped{Value::Null}else{json!("lease")},"state":if stopped{"stopped"}else{"running"}})
                    }
                    "guest" => {
                        let request = &op["request"];
                        let operation = &request["operation"];
                        let body = match operation["kind"].as_str().unwrap() {
                            "exec" => {
                                if operation["argv"][0] != "/bin/mkdir" {
                                    *count.lock().unwrap() += 1;
                                    let cwd = operation["cwd"].as_str().unwrap();
                                    let data = b"ok\n".to_vec();
                                    files.insert(format!("{cwd}/answer.txt"), data.clone());
                                    files.insert(format!("{cwd}/manifest.json"),serde_json::to_vec(&json!({"answer.txt":{"sha256":store::digest(&data),"bytes":data.len()}})).unwrap());
                                }
                                json!({"kind":"exited","code":0,"stdout":"","stderr":"","timed_out":false})
                            }
                            "write_bytes" => {
                                let bytes = super::super::transfer::decode(
                                    operation["content_hex"].as_str().unwrap(),
                                )
                                .unwrap();
                                let file = files
                                    .entry(operation["path"].as_str().unwrap().into())
                                    .or_default();
                                let offset = operation["offset"].as_u64().unwrap() as usize;
                                if offset == 0 {
                                    file.clear();
                                }
                                assert_eq!(file.len(), offset);
                                file.extend(bytes);
                                json!({"kind":"written"})
                            }
                            "read_chunk" => {
                                let file = &files[operation["path"].as_str().unwrap()];
                                let offset = operation["offset"].as_u64().unwrap() as usize;
                                let end = (offset
                                    + operation["max_bytes"].as_u64().unwrap() as usize)
                                    .min(file.len());
                                json!({"kind":"chunk","content_hex":file[offset..end].iter().map(|b|format!("{b:02x}")).collect::<String>(),"next_offset":end,"eof":end==file.len()})
                            }
                            other => panic!("unexpected guest operation {other}"),
                        };
                        json!({"identity":request["identity"],"result":{"Ok":body}})
                    }
                    other => panic!("unexpected operation {other}"),
                };
                original.insert(id.into(), result.clone());
                if drop_reply
                    && op["operation"] == "guest"
                    && op["request"]["operation"]["kind"] == "exec"
                    && op["request"]["operation"]["argv"][0] != "/bin/mkdir"
                {
                    continue;
                }
                let bytes = serde_json::to_vec(
                    &json!({"request":id,"caller":req["caller"],"result":{"Ok":result}}),
                )
                .unwrap();
                stream
                    .write_all(&(bytes.len() as u32).to_be_bytes())
                    .unwrap();
                stream.write_all(&bytes).unwrap();
            }
        });
        Self {
            root,
            spec,
            hash,
            stop,
            worker: Some(worker),
            executions,
        }
    }
    fn session(&self, name: &str) -> PathBuf {
        store::directory(&self.root.join("jobs")).unwrap();
        let p = self.root.join("jobs").join(name);
        store::directory(&p).unwrap();
        p
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.worker.take().unwrap().join();
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn fixed_job_recovers_original_execution_and_releases_claimed_vm() -> Result<()> {
    let f = Fixture::new(true);
    let session = f.session("primary");
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let result = run(
        &session,
        &f.spec,
        &f.hash,
        &id,
        "task",
        json!({"proposal":"data"}),
        vec![],
        || Ok(()),
    )?;
    assert_eq!(result["state"], "proposed");
    assert_eq!(result["outputs"]["answer.txt"]["text"], "ok\n");
    assert_eq!(result["artifact_accepted"], false);
    assert_eq!(*f.executions.lock().unwrap(), 1);
    assert_eq!(
        run(
            &session,
            &f.spec,
            &f.hash,
            &id,
            "task",
            json!({"proposal":"data"}),
            vec![],
            || Ok(())
        )?,
        result
    );
    assert!(
        run(
            &session,
            &f.spec,
            &f.hash,
            &id,
            "task",
            json!({"proposal":"changed"}),
            vec![],
            || Ok(())
        )
        .is_err()
    );
    assert_eq!(
        fs::read(output(&session, &id, "answer.txt", "task")?.0)?,
        b"ok\n"
    );
    assert!(output(&session, &id, "answer.txt", "foreign").is_err());
    let foreign = f.session("foreign");
    assert!(
        run(
            &foreign,
            &f.spec,
            &f.hash,
            &an_agent_core::det_seam::Entropy::os().uuid_v4().to_string(),
            "task",
            Value::Null,
            vec![],
            || Ok(())
        )
        .is_err()
    );
    assert_eq!(*f.executions.lock().unwrap(), 1);
    assert_eq!(release(&session)?["termination_confirmed"], true);
    assert_eq!(release(&session)?["termination_confirmed"], true);
    Ok(())
}
#[test]
fn revoked_task_cannot_begin_a_fixed_job() -> Result<()> {
    let f = Fixture::new(false);
    let session = f.session("primary");
    assert!(
        run(
            &session,
            &f.spec,
            &f.hash,
            &an_agent_core::det_seam::Entropy::os().uuid_v4().to_string(),
            "task",
            Value::Null,
            vec![],
            || Err("revoked".into())
        )
        .is_err()
    );
    assert_eq!(*f.executions.lock().unwrap(), 0);
    assert!(!session.join("cocoon-jobs").exists());
    Ok(())
}

#[test]
fn fixed_job_capabilities_cannot_switch_specs_or_widen_output_targets() -> Result<()> {
    use crate::task_permissions::Permission;
    let f = Fixture::new(false);
    let job = Permission::CocoonJob {
        spec_sha256: f.hash.clone(),
    };
    assert!(job.validate().is_ok());
    assert!(
        !Permission::CocoonJob {
            spec_sha256: "a".repeat(64)
        }
        .within(&job)
    );
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let permission = Permission::RunJobOutput {
        instance: f.root.clone(),
        run: "deliver".into(),
        operation: id.clone(),
        output: "answer.txt".into(),
        seconds: 30,
    };
    permission.validate()?;
    assert!(
        Permission::RunJobOutput {
            instance: f.root.clone(),
            run: "deliver".into(),
            operation: id.clone(),
            output: "answer.txt".into(),
            seconds: 15
        }
        .within(&permission)
    );
    assert!(
        !Permission::RunJobOutput {
            instance: f.root.clone(),
            run: "deliver".into(),
            operation: id,
            output: "other.txt".into(),
            seconds: 15
        }
        .within(&permission)
    );
    Ok(())
}
