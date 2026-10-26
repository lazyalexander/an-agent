//! Exercise inherited objects, not only inherited descriptors waiting for exec.
use super::*;

fn inherited<T>(result: std::result::Result<T, StoreError>, parent: u32) -> bool {
    matches!(result, Err(StoreError::InheritedProcess { owner, current })
        if owner == parent && current == std::process::id() && current != parent)
}

struct ForkChild(libc::pid_t);
impl ForkChild {
    fn wait(mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mut status = 0;
            let result = unsafe { libc::waitpid(self.0, &mut status, libc::WNOHANG) };
            if result == self.0 {
                self.0 = 0;
                assert_eq!(status, 0, "fork child rejected ownership assertions");
                return;
            }
            assert!(result >= 0, "waitpid failed");
            assert!(Instant::now() < deadline, "fork child stalled");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for ForkChild {
    fn drop(&mut self) {
        if self.0 > 0 {
            unsafe {
                libc::kill(self.0, libc::SIGKILL);
                libc::waitpid(self.0, std::ptr::null_mut(), 0);
            }
        }
    }
}

#[test]
fn inherited_store_drop_does_not_unlock_the_parent() {
    let dir = an_agent_core::testkit::TempDir::new("fork-child-drop");
    let path = dir.path().join("memory.jsonl");
    let owner = JsonlStore::open(&path).unwrap();
    owner.append(event()).unwrap();
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        // Deliberately drop the inherited object; do not unwind the test harness.
        drop(owner);
        unsafe { libc::_exit(0) };
    }
    ForkChild(pid).wait();
    assert!(matches!(
        JsonlStore::open(&path),
        Err(StoreError::WriterBusy(_))
    ));
    assert_eq!(owner.append(event()).unwrap().seq, 2);
    drop(owner);
    let next = JsonlStore::open(&path).unwrap();
    assert_eq!(next.append(event()).unwrap().seq, 3);
}

#[test]
fn inherited_apis_cannot_write_or_repair_after_another_writer_takes_over() {
    use std::os::fd::{FromRawFd, OwnedFd};
    let dir = an_agent_core::testkit::TempDir::new("fork-stale-store");
    let path = dir.path().join("memory.jsonl");
    let owner = JsonlStore::open(&path).unwrap();
    let record = owner.append(event()).unwrap();
    let pending = event(); // No event allocation is needed in the fork child.
    let parent = std::process::id();
    let mut pipe = [0; 2];
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    let read_end = unsafe { OwnedFd::from_raw_fd(pipe[0]) };
    let write_end = unsafe { OwnedFd::from_raw_fd(pipe[1]) };
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        drop(write_end);
        let mut byte = 0u8;
        let ready = unsafe { libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1) } == 1;
        // Every call must reject before mutex acquisition, allocation or filesystem I/O.
        let valid = ready
            && inherited(owner.read_all(), parent)
            && inherited(owner.append(pending), parent)
            && inherited(owner.tool_payload("must not be stored"), parent)
            && inherited(owner.resolve_tool_payload(&record), parent);
        drop(owner);
        unsafe { libc::_exit(if valid { 0 } else { 1 }) };
    }
    let child = ForkChild(pid);
    drop(read_end);
    drop(owner);
    let next = JsonlStore::open(&path).unwrap();
    assert_eq!(next.append(event()).unwrap().seq, 2);
    // The new owner alone may repair this torn tail. A stale cached sequence
    // must not append duplicate seq=2 or quarantine/truncate another writer's bytes.
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{\"torn\":").unwrap();
    file.sync_all().unwrap();
    drop(file);
    let before = fs::read(&path).unwrap();
    let mut signal = fs::File::from(write_end);
    signal.write_all(b"x").unwrap();
    drop(signal);
    child.wait();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!dir.path().join("memory.jsonl.corrupt").exists());
    assert!(!dir.path().join("blobs").exists());
    assert!(matches!(
        JsonlStore::open(&path),
        Err(StoreError::WriterBusy(_))
    ));
    assert_eq!(
        next.read_all()
            .unwrap()
            .iter()
            .map(|e| e.seq)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(next.append(event()).unwrap().seq, 3);
    assert!(dir.path().join("memory.jsonl.corrupt").exists());
}
