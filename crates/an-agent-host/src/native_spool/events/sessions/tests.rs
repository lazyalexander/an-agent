use super::*;

#[test]
fn controls_are_generation_bound_and_cancel_is_terminal() -> Result<()> {
    let temp = an_agent_core::testkit::TempDir::new("native-session");
    let ws = temp.path().canonicalize()?.join("workspace");
    super::super::init(&ws)?;
    create(&ws, "one")?;
    let pause = change(&ws, "one", "paused", None)?;
    change(&ws, "one", "paused", pause["approval_sha256"].as_str())?;
    assert!(change(&ws, "one", "paused", pause["approval_sha256"].as_str()).is_err());
    let cancel = change(&ws, "one", "cancelled", None)?;
    change(&ws, "one", "cancelled", cancel["approval_sha256"].as_str())?;
    assert!(change(&ws, "one", "open", None).is_err());
    let close = change(&ws, "one", "closed", None)?;
    change(&ws, "one", "closed", close["approval_sha256"].as_str())?;
    assert!(change(&ws, "one", "open", None).is_err());
    assert!(create(&ws, "../escape").is_err());
    Ok(())
}

#[test]
fn partial_thread_and_control_history_never_mean_empty() -> Result<()> {
    let temp = an_agent_core::testkit::TempDir::new("native-session");
    let ws = temp.path().canonicalize()?.join("workspace");
    super::super::init(&ws)?;
    create(&ws, "one")?;
    let root = ws.join("sessions/one");
    fs::create_dir(root.join("threads/lost"))?;
    assert_eq!(inspect(&ws, "one")?["threads"][0]["settled"], false);
    assert!(change(&ws, "one", "closed", None).is_err());
    store::write_new(&root.join("controls/002.json"), b"{}")?;
    assert!(inspect(&ws, "one").is_err());
    Ok(())
}

#[test]
fn staged_control_is_not_a_committed_pause_or_cancel() -> Result<()> {
    let temp = an_agent_core::testkit::TempDir::new("session-publication");
    let ws = temp.path().canonicalize()?.join("workspace");
    super::super::init(&ws)?;
    create(&ws, "one")?;
    store::write_new(&ws.join("sessions/one/control.pending"), b"{")?;
    assert_eq!(inspect(&ws, "one")?["control"]["mode"], "open");
    let pause = change(&ws, "one", "paused", None)?;
    assert!(change(&ws, "one", "paused", pause["approval_sha256"].as_str()).is_err());
    assert_eq!(inspect(&ws, "one")?["control"]["generation"], 0);
    Ok(())
}
