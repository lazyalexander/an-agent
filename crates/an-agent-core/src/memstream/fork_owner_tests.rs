//! An inherited handle must refuse before touching even a locked state mutex.
use super::*;
use crate::testkit::TempDir;
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

#[test]
fn owner_check_precedes_state_mutex_for_read_and_append() {
    let dir = TempDir::new("owner-before-mutex");
    let mut store = JsonlStore::open(dir.path().join("memory.jsonl")).unwrap();
    let expected_owner = std::process::id().wrapping_add(1);
    store.owner_pid = expected_owner;
    let store = Arc::new(store);
    let locked = store.state.lock().unwrap();
    let worker_store = store.clone();
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let read = worker_store.read_all();
        let append = worker_store.append(super::tests::base("forbidden"));
        send.send((read, append)).unwrap();
    });
    let result = receive.recv_timeout(Duration::from_secs(3));
    // Even a regression is unblocked before joining, so the test cannot hang CI.
    drop(locked);
    worker.join().unwrap();
    let (read, append) = result.expect("inherited handle attempted to acquire state mutex");
    assert!(
        matches!(read, Err(StoreError::InheritedProcess { owner, .. }) if owner == expected_owner)
    );
    assert!(
        matches!(append, Err(StoreError::InheritedProcess { owner, .. }) if owner == expected_owner)
    );
    assert!(
        std::fs::read(dir.path().join("memory.jsonl"))
            .unwrap()
            .is_empty()
    );
}
