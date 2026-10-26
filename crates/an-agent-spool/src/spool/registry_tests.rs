//! Separate instances and processes must agree on immutable version identity.
#![allow(clippy::disallowed_methods)]
use super::*;
use std::{
    fs,
    sync::{Arc, Barrier},
    thread,
    time::{Duration, Instant},
};

#[test]
fn concurrent_instances_commit_one_body_or_idempotently_share_one_entry() {
    for same in [false, true] {
        let temp = TempDir::new("registry-concurrent");
        let root = temp.path();
        let barrier = Arc::new(Barrier::new(8));
        let results = thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let barrier = barrier.clone();
                    scope.spawn(move || {
                        let registry = Registry::open(root).unwrap();
                        let yaml = read_yaml().replace(
                            "summary: Read a workspace file",
                            &format!("summary: body {}", if same { 0 } else { i }),
                        );
                        barrier.wait();
                        registry.publish(&yaml).map(|s| s.sha256)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        let successes: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert_eq!(successes.len(), if same { 8 } else { 1 });
        let recovered = Registry::open(root)
            .unwrap()
            .recover("fs_read", "1.0.0")
            .unwrap();
        assert!(successes.iter().all(|s| **s == recovered.sha256));
        assert_eq!(
            fs::read_to_string(root.join("index.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
}

#[test]
fn registry_child() {
    let Some(root) = std::env::var_os("AN_AGENT_REGISTRY_TEST_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let child = std::env::var("AN_AGENT_REGISTRY_TEST_CHILD").unwrap();
    let registry = Registry::open(root.join("registry")).unwrap();
    fs::write(root.join(format!("ready-{child}")), b"ready").unwrap();
    let start = Instant::now();
    while !root.join("go").exists() {
        assert!(start.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(5));
    }
    let yaml = read_yaml().replace(
        "summary: Read a workspace file",
        &format!("summary: child {child}"),
    );
    let value = match registry.publish(&yaml) {
        Ok(spec) => serde_json::json!({"hash":spec.sha256}),
        Err(error) => serde_json::json!({"error":error.to_string()}),
    };
    fs::write(
        root.join(format!("result-{child}")),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
}

#[test]
fn separate_processes_cannot_both_publish_conflicting_bodies() {
    let temp = TempDir::new("registry-processes");
    let mut children: Vec<_> = ["a", "b"]
        .iter()
        .map(|id| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "spool::tests::registry_tests::registry_child",
                    "--nocapture",
                ])
                .env("AN_AGENT_REGISTRY_TEST_ROOT", temp.path())
                .env("AN_AGENT_REGISTRY_TEST_CHILD", id)
                .spawn()
                .unwrap()
        })
        .collect();
    let start = Instant::now();
    while !["a", "b"]
        .iter()
        .all(|id| temp.path().join(format!("ready-{id}")).exists())
    {
        if start.elapsed() >= Duration::from_secs(10) {
            for child in &mut children {
                let _ = child.kill();
                let _ = child.wait();
            }
            panic!("children did not reach registry barrier");
        }
        thread::sleep(Duration::from_millis(5));
    }
    fs::write(temp.path().join("go"), b"go").unwrap();
    for child in &mut children {
        assert!(child.wait().unwrap().success());
    }
    let values: Vec<serde_json::Value> = ["a", "b"]
        .iter()
        .map(|id| {
            serde_json::from_slice(&fs::read(temp.path().join(format!("result-{id}"))).unwrap())
                .unwrap()
        })
        .collect();
    let winners: Vec<_> = values.iter().filter_map(|v| v["hash"].as_str()).collect();
    assert_eq!(winners.len(), 1);
    assert_eq!(
        Registry::open(temp.path().join("registry"))
            .unwrap()
            .recover("fs_read", "1.0.0")
            .unwrap()
            .sha256,
        winners[0]
    );
}

#[test]
fn incomplete_conflicting_or_mismatched_history_is_never_silently_selected() {
    let temp = TempDir::new("registry-corruption");
    let registry = Registry::open(temp.path()).unwrap();
    registry.publish(&read_yaml()).unwrap();
    let index = temp.path().join("index.jsonl");
    let original = fs::read_to_string(&index).unwrap();
    for changed in [
        format!("{original}{{\"name\":"),
        original.trim_end().to_owned(),
    ] {
        fs::write(&index, &changed).unwrap();
        assert!(registry.recover("fs_read", "1.0.0").is_err());
        assert!(registry.publish(&read_yaml()).is_err());
        assert_eq!(fs::read_to_string(&index).unwrap(), changed);
    }
    let mut line: serde_json::Value = serde_json::from_str(original.trim()).unwrap();
    line["sha256"] = serde_json::json!("0".repeat(64));
    fs::write(&index, format!("{original}{line}\n")).unwrap();
    assert!(registry.recover("fs_read", "1.0.0").is_err());
    let mut line: serde_json::Value = serde_json::from_str(original.trim()).unwrap();
    line["version"] = serde_json::json!("2.0.0");
    fs::write(&index, format!("{line}\n")).unwrap();
    assert!(registry.recover("fs_read", "2.0.0").is_err());
}

#[test]
fn an_unindexed_blob_is_verified_before_it_can_be_published() {
    let temp = TempDir::new("registry-orphan");
    let registry = Registry::open(temp.path()).unwrap();
    let yaml = read_yaml();
    let spec = parse(&yaml).unwrap();
    let blob = temp.path().join("blobs").join(&spec.sha256);
    fs::write(&blob, b"not the approved body").unwrap();
    assert!(registry.publish(&yaml).is_err());
    assert!(
        fs::read(temp.path().join("index.jsonl"))
            .unwrap()
            .is_empty()
    );
    fs::write(&blob, &yaml).unwrap();
    assert_eq!(registry.publish(&yaml).unwrap().sha256, spec.sha256);
    assert_eq!(registry.recover("fs_read", "1.0.0").unwrap().body, yaml);
    assert_eq!(fs::read_dir(temp.path().join("blobs")).unwrap().count(), 1);
}
