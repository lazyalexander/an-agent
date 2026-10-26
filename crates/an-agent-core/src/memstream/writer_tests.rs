//! Ownership must survive errors until the owner drops; reopen reconciles bytes.
use super::tests::{base, tmp};
use super::*;

#[test]
fn append_faults_keep_ownership_and_recover_without_duplicate_sequences() {
    for stage in ["partial_write", "append_sync"] {
        for restart in [false, true] {
            let (_dir, path) = tmp();
            let mut store = JsonlStore::open(&path).unwrap();
            store.append(base("first")).unwrap();
            store.state.lock().unwrap().fail_io_at = Some(stage);
            assert!(store.append(base("uncertain")).is_err());
            assert!(matches!(
                JsonlStore::open(&path),
                Err(StoreError::WriterBusy(_))
            ));
            if restart {
                drop(store);
                store = JsonlStore::open(&path).unwrap();
            }
            let next = store.append(base("explicit next action")).unwrap();
            let expected = if stage == "partial_write" { 2 } else { 3 };
            assert_eq!(next.seq, expected);
            let all = store.read_all().unwrap();
            assert_eq!(
                all.iter().map(|e| e.seq).collect::<Vec<_>>(),
                (1..=expected).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn quarantine_faults_retain_evidence_and_reopen_can_finish_recovery() {
    for stage in ["before_truncate", "truncate_sync"] {
        for restart in [false, true] {
            let (_dir, path) = tmp();
            let mut store = JsonlStore::open(&path).unwrap();
            store.append(base("first")).unwrap();
            let torn = b"{\"unfinished\":";
            OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(torn)
                .unwrap();
            store.state.lock().unwrap().fail_io_at = Some(stage);
            assert!(store.read_all().is_err());
            assert!(matches!(
                JsonlStore::open(&path),
                Err(StoreError::WriterBusy(_))
            ));
            let side = path.with_file_name("memory.jsonl.corrupt");
            assert!(fs::read_to_string(&side).unwrap().contains("unfinished"));
            if restart {
                drop(store);
                store = JsonlStore::open(&path).unwrap();
            }
            // Direct append after failed read/repair must rescan, even without reopening.
            assert_eq!(store.append(base("next")).unwrap().seq, 2);
            assert_eq!(store.read_all().unwrap().len(), 2);
            assert!(fs::read_to_string(side).unwrap().contains("unfinished"));
        }
    }
}

#[test]
fn independent_threads_cannot_open_an_owned_tape() {
    let (_dir, path) = tmp();
    let owner = JsonlStore::open(&path).unwrap();
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || {
                assert!(matches!(
                    JsonlStore::open(path),
                    Err(StoreError::WriterBusy(_))
                ))
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(owner.append(base("only owner")).unwrap().seq, 1);
}

#[cfg(unix)]
#[test]
fn aliases_cannot_bypass_the_writer_lock() {
    let (dir, path) = tmp();
    let owner = JsonlStore::open(&path).unwrap();
    let hard = dir.path().join("hard.jsonl");
    let soft = dir.path().join("soft.jsonl");
    fs::hard_link(&path, &hard).unwrap();
    std::os::unix::fs::symlink(&path, &soft).unwrap();
    for alias in [&hard, &soft] {
        assert!(matches!(
            JsonlStore::open(alias),
            Err(StoreError::WriterBusy(_))
        ));
    }
    drop(owner);
    assert_eq!(
        JsonlStore::open(hard)
            .unwrap()
            .append(base("next"))
            .unwrap()
            .seq,
        1
    );
}
