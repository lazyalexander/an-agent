//! Concurrent real native attempts in one store; no alternate execution harness.
use super::*;
use an_agent_host::{artifact_versions, retention};
use std::{
    process::{Child, Stdio},
    time::{Duration, Instant},
};

// Declared before child guards so failure cleanup settles children before capture.
struct Capture(PathBuf);
impl Drop for Capture {
    fn drop(&mut self) {
        let Some(folder) = std::env::var_os("AN_AGENT_ACCEPTANCE_DIR") else {
            return;
        };
        let snapshots = fs::read_dir(self.0.join("store/runs"))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| {
                let data = evidence(&entry.path())
                    .unwrap_or_else(|error| json!({"capture_error":error.to_string()}));
                (entry.file_name().to_string_lossy().into_owned(), data)
            })
            .collect::<serde_json::Map<_, _>>();
        let _ = fs::create_dir_all(&folder);
        if let Ok(bytes) = serde_json::to_vec_pretty(&snapshots) {
            let _ = fs::write(Path::new(&folder).join("run-snapshots.json"), bytes);
        }
    }
}

struct Running(Option<Child>);
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            // The CLI owns cancellation and reaping, including test failure paths.
            // SAFETY: signal the still-owned CLI child, never a discovered PID.
            unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
            let _ = child.wait();
        }
    }
}

fn launch(f: &Fixture, release: &str, label: &str, input: &str) -> execution::Result<Running> {
    let path = f.root.join(format!("{label}.txt"));
    fs::write(&path, input)?;
    Ok(Running(Some(
        Command::new(env!("CARGO_BIN_EXE_an-agent-host"))
            .arg("run")
            .arg(f.root.join("store"))
            .arg(release)
            .arg(path)
            .arg("--execute")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    )))
}

#[allow(
    clippy::disallowed_methods,
    reason = "real concurrent process deadline"
)]
fn started(f: &Fixture, input: &str) -> execution::Result<PathBuf> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(runs) = fs::read_dir(f.root.join("store/runs")) {
            for entry in runs {
                let run = entry?.path();
                if fs::read(run.join("work/input.txt")).is_ok_and(|v| v == input.as_bytes())
                    && fs::read(run.join("work/output.txt")).is_ok_and(|v| !v.is_empty())
                {
                    return Ok(run);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err(format!("native attempt did not reach output: {input}").into())
}

fn finish(mut running: Running, expected: &str) -> execution::Result<PathBuf> {
    let output = running
        .0
        .take()
        .ok_or("missing CLI child")?
        .wait_with_output()?;
    let stdout = String::from_utf8(output.stdout)?;
    let result: Value = serde_json::from_str(stdout.lines().last().ok_or("missing CLI result")?)?;
    assert_eq!(result["status"], expected, "{stdout}; {:?}", output.stderr);
    assert_eq!(output.status.success(), expected == "succeeded");
    let run = Path::new(result["receipt"].as_str().ok_or("missing receipt")?)
        .parent()
        .ok_or("missing run directory")?;
    let receipt: Value = serde_json::from_slice(&fs::read(run.join("receipt.json"))?)?;
    assert_eq!(receipt["process"]["reaped"], true);
    assert_eq!(
        receipt["operation"]["observation"]["termination_confirmed"],
        true
    );
    assert_eq!(receipt["operation"]["automatic_retries"], 0);
    Ok(run.to_owned())
}

fn reclaim(store: &Path, run: &Path) -> execution::Result<()> {
    let id = run
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("missing run ID")?;
    let preview = retention::reclaim(store, id, None)?;
    retention::reclaim(store, id, preview["approval_sha256"].as_str())?;
    assert!(!run.join("work").exists());
    Ok(())
}

fn evidence(run: &Path) -> execution::Result<Value> {
    let mut files = serde_json::Map::new();
    for name in [
        "plan.json",
        "operation.json",
        "receipt.json",
        "settled.json",
        "audit.json",
        "reclaim.json",
    ] {
        if run.join(name).exists() {
            files.insert(
                name.into(),
                serde_json::from_slice(&fs::read(run.join(name))?)?,
            );
        }
    }
    for name in ["input.txt", "artifact.txt", "stdout.txt", "stderr.txt"] {
        if run.join(name).exists() {
            files.insert(name.into(), json!(fs::read_to_string(run.join(name))?));
        }
    }
    Ok(Value::Object(files))
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "measured native lifecycle acceptance"
)]
fn cancel_restart_and_reclaim_one_spool_while_peer_keeps_its_pinned_input() -> execution::Result<()>
{
    let f = Fixture::with_source(Some(
        r#"
use std::{fs, thread, time::Duration};
fn main() {
    let input = fs::read_to_string("input.txt").unwrap();
    fs::write("output.txt", "started").unwrap();
    if input == "cancel-me" { loop { thread::sleep(Duration::from_millis(10)); } }
    if input == "peer-v1" { while fs::read_to_string("output.txt").unwrap_or_default() != "release-peer" { thread::sleep(Duration::from_millis(10)); } }
    fs::write("output.txt", input).unwrap();
}
"#,
    ))?;
    let _capture = Capture(f.root.clone());
    let store = f.root.join("store");
    let catalog = f.root.join("catalog");
    // Two independently named releases, sharing only the trusted host store.
    let yaml = f.root.join("deployment/spool.yaml");
    fs::write(
        &yaml,
        fs::read_to_string(&yaml)?.replace("name: file-runtime", "name: peer-spool"),
    )?;
    let registered = execution::register(&f.root.join("deployment"), &store)?;
    let peer = registered["registered"].as_str().unwrap();
    assert_ne!(peer, f.release);
    // Prepare an immutable input revision before either concurrent attempt starts.
    let seed = finish(launch(&f, &f.release, "seed", "seed")?, "succeeded")?;
    let base = artifact_versions::import(&catalog, &seed)?;
    let edit = f.root.join("edit.txt");
    fs::write(&edit, "peer-v1")?;
    let v1 = artifact_versions::edit(
        &catalog,
        base["revision"].as_str().unwrap(),
        &edit,
        &revisions::digest(b"peer-v1"),
    )?;
    let revision = v1["revision"].as_str().unwrap();
    let start = Instant::now();
    let mut events = Vec::new();
    let mut record = |event: &str| {
        let entry = json!({"event":event,"elapsed_ms":start.elapsed().as_millis()});
        println!("CONCURRENCY_EVENT={entry}");
        events.push(entry);
        if let Some(folder) = std::env::var_os("AN_AGENT_ACCEPTANCE_DIR") {
            fs::create_dir_all(&folder).expect("create evidence folder");
            fs::write(
                Path::new(&folder).join("events.json"),
                serde_json::to_vec_pretty(&events).unwrap(),
            )
            .expect("retain event timeline");
        }
    };
    let mut b = Running(Some(
        Command::new(env!("CARGO_BIN_EXE_an-agent-host"))
            .arg("run-version")
            .arg(&store)
            .arg(peer)
            .arg(&catalog)
            .arg(revision)
            .arg("--execute")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    ));
    let b_run = started(&f, "peer-v1")?;
    record("peer_started");
    let a = launch(&f, &f.release, "cancel", "cancel-me")?;
    let a_run = started(&f, "cancel-me")?;
    record("both_running");
    assert!(b.0.as_mut().unwrap().try_wait()?.is_none());
    for run in [&a_run, &b_run] {
        assert!(
            retention::reclaim(&store, run.file_name().unwrap().to_str().unwrap(), None).is_err()
        );
        assert!(run.join("work/output.txt").exists());
    }
    record("live_reclamation_denied");
    // Signal only A's CLI. B has its own process, cancellation and run identity.
    // SAFETY: A is a still-owned child; the host handles its process cleanup.
    assert_eq!(
        unsafe { libc::kill(a.0.as_ref().unwrap().id() as i32, libc::SIGTERM) },
        0
    );
    record("cancel_requested");
    assert_eq!(finish(a, "cancelled")?, a_run);
    record("cancel_confirmed");
    assert!(!a_run.join("artifact.txt").exists());
    assert!(artifact_versions::import(&catalog, &a_run).is_err());
    assert!(
        an_agent_host::capability::artifact_read(
            &store,
            a_run.file_name().unwrap().to_str().unwrap(),
            &catalog,
            peer,
            None,
            &std::sync::atomic::AtomicBool::new(false)
        )
        .is_err()
    );
    let cancelled_partial = fs::read(a_run.join("work/output.txt"))?;
    let cancelled_receipt = fs::read(a_run.join("receipt.json"))?;
    reject_uncertain_cancelled_cleanup(&store, &a_run)?;
    reclaim(&store, &a_run)?;
    assert_eq!(fs::read(a_run.join("receipt.json"))?, cancelled_receipt);
    record("cancelled_scratch_reclaimed");
    let restarted = finish(
        launch(&f, &f.release, "restart", "new-attempt")?,
        "succeeded",
    )?;
    assert_ne!(restarted, a_run);
    assert!(
        b.0.as_mut().unwrap().try_wait()?.is_none(),
        "peer must overlap cancel, cleanup and restart"
    );
    record("restart_completed_while_peer_running");
    fs::write(&edit, "peer-v2")?;
    let v2 = artifact_versions::edit(&catalog, revision, &edit, &revisions::digest(b"peer-v2"))?;
    assert_ne!(v2["revision"], revision);
    record("new_input_revision_published");
    // Release only after cancellation, collection, restart and input-version change.
    fs::write(b_run.join("work/output.txt"), b"release-peer")?;
    assert_eq!(finish(b, "succeeded")?, b_run);
    record("peer_completed");
    assert_eq!(fs::read(b_run.join("artifact.txt"))?, b"peer-v1");
    let receipt: Value = serde_json::from_slice(&fs::read(b_run.join("receipt.json"))?)?;
    assert_eq!(receipt["input_revision"]["revision"], revision);
    let product = artifact_versions::import(&catalog, &b_run)?;
    for run in [&seed, &restarted, &b_run] {
        reclaim(&store, run)?;
    }
    for (label, selected, expected) in [
        ("product", product["revision"].as_str().unwrap(), "peer-v1"),
        ("v1", revision, "peer-v1"),
        ("v2", v2["revision"].as_str().unwrap(), "peer-v2"),
    ] {
        let target = f.root.join(format!("retained-{label}"));
        artifact_versions::checkout(&catalog, selected, &target)?;
        assert_eq!(fs::read_to_string(target)?, expected);
    }
    record("scratch_reclaimed_versions_preserved");
    let report = json!({"schema":"an-native-concurrency-acceptance-v1", "events":events,
        "cancelled":evidence(&a_run)?,"restart":evidence(&restarted)?,"peer":evidence(&b_run)?,
        "cancelled_partial_sha256":revisions::digest(&cancelled_partial),
        "input_v1":v1,"input_v2":v2,"product":product});
    println!("CONCURRENCY_EVIDENCE={report}");
    // Optional local/CI collection; no fixture paths or machine data are committed.
    if let Some(folder) = std::env::var_os("AN_AGENT_ACCEPTANCE_DIR") {
        fs::create_dir_all(&folder)?;
        fs::write(
            Path::new(&folder).join("native-concurrency.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    Ok(())
}

// Reseal only in the trusted test fixture to isolate each lifecycle condition.
fn reject_uncertain_cancelled_cleanup(store: &Path, run: &Path) -> execution::Result<()> {
    let original = fs::read(run.join("receipt.json"))?;
    let seal = fs::read(run.join("settled.json"))?;
    let id = run
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("missing run ID")?;
    for pointer in [
        "/process/reaped",
        "/operation/observation/termination_confirmed",
        "/operation/cancel_requested",
    ] {
        let mut receipt: Value = serde_json::from_slice(&original)?;
        *receipt
            .pointer_mut(pointer)
            .ok_or("missing receipt field")? = json!(false);
        let bytes = serde_json::to_vec(&receipt)?;
        fs::write(run.join("receipt.json"), &bytes)?;
        let mut marker: Value = serde_json::from_slice(&seal)?;
        marker["receipt_sha256"] = json!(revisions::digest(&bytes));
        fs::write(run.join("settled.json"), serde_json::to_vec(&marker)?)?;
        assert!(retention::reclaim(store, id, None).is_err(), "{pointer}");
        assert!(run.join("work/output.txt").exists());
        assert!(!run.join("reclaim-intent.json").exists());
    }
    fs::write(run.join("receipt.json"), original)?;
    fs::write(run.join("settled.json"), seal)?;
    Ok(())
}
