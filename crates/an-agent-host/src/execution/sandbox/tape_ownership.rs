//! Host-owned process tests for tape ownership across competing and killed processes.
#![allow(clippy::unwrap_used, clippy::disallowed_methods)] // Test-only process watchdog uses monotonic time.
use an_agent_core::memstream::{AppendEvent, FromKind, JsonlStore, Kind, StoreError};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn event() -> AppendEvent {
    AppendEvent {
        from: "writer".into(),
        from_kind: FromKind::Agent,
        kind: Kind::Utterance,
        session: "session".into(),
        content: "committed".into(),
        tags: vec![],
        refs: vec![],
        act: None,
        card: None,
    }
}

#[test]
fn independent_writers_are_rejected_before_read_or_append() {
    let dir = std::env::temp_dir().join(format!(
        "tape-owner-{}",
        an_agent_core::det_seam::Entropy::os().uuid_v4()
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("memory.jsonl");
    let first = JsonlStore::open(&path).unwrap();
    assert_eq!(first.append(event()).unwrap().seq, 1);
    let before = fs::read(&path).unwrap();
    assert!(
        matches!(JsonlStore::open(&path), Err(StoreError::WriterBusy(_))),
        "second writer must be refused"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(first.append(event()).unwrap().seq, 2);
    drop(first);
    let resumed = JsonlStore::open(&path).unwrap();
    assert_eq!(resumed.append(event()).unwrap().seq, 3);
    drop(resumed);
    fs::remove_dir_all(dir).unwrap();
}

struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn child(path: &Path, mode: &str) -> Worker {
    Worker(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "execution::sandbox::tape_ownership::process_worker",
                "--nocapture",
            ])
            .env("AN_TAPE_TEST_PATH", path)
            .env("AN_TAPE_TEST_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    )
}
fn wait(child: &mut Worker) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        assert!(Instant::now() < deadline, "child timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn active_torn_tail_is_not_healed_by_competitor_and_recovers_after_process_death() {
    let dir = std::env::temp_dir().join(format!(
        "tape-process-{}",
        an_agent_core::det_seam::Entropy::os().uuid_v4()
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("memory.jsonl");
    let mut writer = child(&path, "hold");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !dir.join("ready").exists() {
        assert!(
            writer.0.try_wait().unwrap().is_none(),
            "writer exited before ready"
        );
        assert!(Instant::now() < deadline, "writer timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
    let before = fs::read(&path).unwrap();
    assert!(matches!(
        JsonlStore::open(&path),
        Err(StoreError::WriterBusy(_))
    ));
    wait(&mut child(&path, "refuse"));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!dir.join("memory.jsonl.corrupt").exists());
    writer.0.kill().unwrap();
    writer.0.wait().unwrap();
    wait(&mut child(&path, "recover"));
    let tape = JsonlStore::open(&path).unwrap();
    let events = tape.read_all().unwrap();
    assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2]);
    assert!(
        fs::read_to_string(dir.join("memory.jsonl.corrupt"))
            .unwrap()
            .contains("unfinished")
    );
    drop(tape);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn process_worker() {
    let Ok(path) = std::env::var("AN_TAPE_TEST_PATH") else {
        return;
    };
    match std::env::var("AN_TAPE_TEST_MODE").unwrap().as_str() {
        "hold" => {
            let tape = JsonlStore::open(&path).unwrap();
            tape.append(event()).unwrap();
            // Retain the owner while representing a write paused in its final line.
            let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(b"{\"unfinished\":").unwrap();
            file.sync_all().unwrap();
            fs::write(Path::new(&path).parent().unwrap().join("ready"), b"ready").unwrap();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).unwrap();
            drop(tape);
        }
        "refuse" => assert!(matches!(
            JsonlStore::open(&path),
            Err(StoreError::WriterBusy(_))
        )),
        "recover" => {
            let tape = JsonlStore::open(&path).unwrap();
            assert_eq!(tape.read_all().unwrap().len(), 1);
            assert_eq!(tape.append(event()).unwrap().seq, 2);
        }
        _ => panic!("unknown child mode"),
    }
}

#[test]
#[cfg(unix)]
fn dropping_owner_unlocks_even_while_a_fork_inherits_the_descriptor() {
    let dir = an_agent_core::testkit::TempDir::new("inherited-tape-lock");
    let path = dir.path().join("memory.jsonl");
    let owner = JsonlStore::open(&path).unwrap();
    owner.append(event()).unwrap();
    let mut pipe = [0; 2];
    // The child only invokes async-signal-safe functions, retains inherited file
    // descriptions, and waits until the parent has checked immediate reacquisition.
    unsafe {
        assert_eq!(libc::pipe(pipe.as_mut_ptr()), 0);
        let pid = libc::fork();
        assert!(pid >= 0);
        if pid == 0 {
            libc::close(pipe[1]);
            let mut byte = 0u8;
            libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
            libc::_exit(0);
        }
        libc::close(pipe[0]);
        drop(owner);
        let resumed = JsonlStore::open(&path);
        // Reap even on an assertion failure so the regression cannot strand a child.
        libc::close(pipe[1]);
        let mut status = 0;
        assert_eq!(libc::waitpid(pid, &mut status, 0), pid);
        assert_eq!(status, 0);
        assert_eq!(resumed.unwrap().append(event()).unwrap().seq, 2);
    }
}

#[cfg(unix)]
mod fork_owner;
