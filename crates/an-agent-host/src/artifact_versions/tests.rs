use super::*;
use an_agent_core::testkit::TempDir;
#[test]
fn byte_revisions_preserve_lineage_bounds_and_identity_without_widening_text() -> Result<()> {
    let t = TempDir::new("binary-revisions");
    let base = t.path().canonicalize()?;
    let catalog = base.join("catalog");
    let original = b"PK\x03\x04\xff\x00";
    assert!(publish(&catalog, original, None, json!({})).is_err());
    let first = publish_bytes(&catalog, original, json!({"kind":"test"}))?;
    let id = first["revision"].as_str().ok_or("revision")?;
    let copy = base.join("saved.xlsx");
    checkout(&catalog, id, &copy)?;
    assert_eq!(fs::read(&copy)?, original);
    fs::write(&copy, b"PK\xffedited")?;
    assert!(edit(&catalog, id, &copy, &store::digest(original)).is_err());
    let second = edit(&catalog, id, &copy, &store::digest(b"PK\xffedited"))?;
    assert_eq!(second["parent"], id);
    assert_eq!(second["encoding"], "bytes");
    assert_eq!(read(&catalog, id)?.0, original);
    assert!(publish_bytes(&catalog, &vec![0; BINARY_LIMIT + 1], json!({})).is_err());
    fs::write(
        catalog.join("revisions").join(id).join("content.bin"),
        b"corrupted",
    )?;
    assert!(checkout(&catalog, id, &base.join("bad.xlsx")).is_err());
    Ok(())
}

#[test]
fn staged_publication_conflict_cannot_publish_or_leave_owned_pending_files() -> Result<()> {
    let t = TempDir::new("staged-catalog");
    let catalog = t.path().canonicalize()?.join("catalog");
    let source = json!({"kind":"staging-test"});
    let first = publish_bytes(&catalog, b"original", source.clone())?;
    let held = store::lock(&catalog)?;
    assert!(publish_bytes(&catalog, b"new candidate", source.clone()).is_err());
    assert_eq!(fs::read_dir(catalog.join("revisions"))?.count(), 1);
    assert_eq!(
        read(&catalog, first["revision"].as_str().unwrap())?.0,
        b"original"
    );
    drop(held);
    let second = publish_bytes(&catalog, b"new candidate", source.clone())?;
    assert_eq!(
        publish_bytes(&catalog, b"new candidate", source.clone())?,
        second
    );
    assert_eq!(fs::read_dir(catalog.join("revisions"))?.count(), 2);
    // Existing identity is checked under the commit lock, never overwritten.
    let path = catalog
        .join("revisions")
        .join(second["revision"].as_str().unwrap())
        .join("content.bin");
    fs::write(&path, b"tampered")?;
    assert!(publish_bytes(&catalog, b"new candidate", source).is_err());
    assert_eq!(fs::read(path)?, b"tampered");
    assert_eq!(fs::read_dir(catalog.join("revisions"))?.count(), 2);
    Ok(())
}

#[test]
fn competing_staged_publishers_keep_exact_versions_without_retry() -> Result<()> {
    let t = TempDir::new("catalog-publishers");
    let catalog = t.path().canonicalize()?.join("catalog");
    publish_bytes(&catalog, b"seed", json!({}))?;
    let barrier = std::sync::Barrier::new(8);
    let outcomes = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|i| {
                let catalog = &catalog;
                let barrier = &barrier;
                scope.spawn(move || {
                    let bytes = vec![i as u8; 1024 * 1024];
                    barrier.wait();
                    publish_bytes(catalog, &bytes, json!({"publisher":i}))
                        .map(|v| (v, bytes))
                        .map_err(|e| {
                            if e.downcast_ref::<std::io::Error>()
                                .is_some_and(|io| io.kind() == std::io::ErrorKind::AlreadyExists)
                            {
                                "lock-conflict".to_owned()
                            } else {
                                format!("unexpected publication error: {e}")
                            }
                        })
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>()
    });
    let mut successes = 0;
    for outcome in outcomes {
        let result = match outcome {
            Ok(result) => result,
            Err(error) => {
                assert_eq!(error, "lock-conflict");
                continue;
            }
        };
        successes += 1;
        assert_eq!(
            read(&catalog, result.0["revision"].as_str().unwrap())?.0,
            result.1
        );
    }
    assert!(successes > 0);
    assert_eq!(
        fs::read_dir(catalog.join("revisions"))?.count(),
        successes + 1
    );
    Ok(())
}
