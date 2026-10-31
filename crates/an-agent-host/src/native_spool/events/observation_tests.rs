use super::*;
use an_agent_core::testkit::TempDir;

#[test]
fn observation_never_takes_writer_lock_or_repairs_projection() -> Result<()> {
    let temp = TempDir::new("native-observation-boundary");
    let root = temp.path().canonicalize()?.join("events");
    init(&root)?;
    let projection = root.join("facts/env.md");
    fs::write(&projection, b"operator evidence; do not repair")?;
    let before = fs::metadata(&projection)?.modified()?;
    let _writer = store::lock(&root)?;
    let lock = fs::read(root.join("host.lock"))?;
    for _ in 0..3 {
        assert_eq!(pending(&root)?["deliveries"], json!([]));
        assert_eq!(inspect(&root)?["execution_granted"], false);
        assert!(store::lock(&root).is_err());
    }
    assert_eq!(fs::read(&projection)?, b"operator evidence; do not repair");
    assert_eq!(fs::metadata(&projection)?.modified()?, before);
    assert_eq!(fs::read(root.join("host.lock"))?, lock);
    Ok(())
}

#[test]
fn frozen_facts_reject_writes_and_report_later_publication() -> Result<()> {
    let temp = TempDir::new("native-observation-version");
    let root = temp.path().canonicalize()?.join("events");
    init(&root)?;
    let observed = Space::observe(&root)?;
    assert_eq!(monitor::boundary(&observed)?["log_unchanged_at_end"], true);
    assert!(observed.facts.push_event("forbidden", "{}").is_err());
    assert!(observed.facts.put_config("forbidden", b"{}").is_err());
    assert!(observed.upgrade(2).is_err());
    assert!(observed.append(&json!({})).is_err());
    let writer = Space::open(&root)?;
    let _guard = store::lock(&root)?;
    writer.facts.push_event("test-only", "{}")?;
    assert!(observed.facts.log()?.is_empty());
    assert_eq!(monitor::boundary(&observed)?["log_unchanged_at_end"], false);
    assert_eq!(Space::observe(&root)?.facts.log()?.len(), 1);
    Ok(())
}

#[test]
fn observed_log_is_bounded_strict_and_never_repaired() -> Result<()> {
    let temp = TempDir::new("native-observation-partial");
    let root = temp.path().canonicalize()?.join("events");
    init(&root)?;
    for bytes in [b"{torn".as_slice(), b"{bad}\n"] {
        fs::write(root.join("facts/log.jsonl"), bytes)?;
        assert!(pending(&root).is_err());
        assert!(inspect(&root).is_err());
        assert_eq!(fs::read(root.join("facts/log.jsonl"))?, bytes);
        assert!(!root.join("host.lock").exists());
    }
    Ok(())
}
