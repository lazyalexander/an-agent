//! Real worker execution at the deterministic reservation/cancellation boundary.
use super::*;
use spool::supervisor::{self, Input};

#[test]
fn cancelled_reservation_closes_without_launch_or_custody() -> Result<()> {
    let temp = TempDir::new("prelaunch-cancel");
    let base = temp.path().canonicalize()?;
    let release = package(&base)?;
    let root = base.join("instance");
    spool::load(&release, &root)?;
    let input = base.join("input.txt");
    fs::write(&input, b"hello\n")?;
    // Prove this exact release can execute and retain an accepted product.
    run(&root, "baseline", &input, &AtomicBool::new(false))?;
    accept(&root, "baseline")?;
    let products = spool::artifacts(&root, "baseline")?;
    let executable = Path::new(env!("CARGO_BIN_EXE_an-agent-host")).canonicalize()?;
    let name = "cancelled";
    let preview = supervisor::prepare(&root, name, Input::File(&input), 15, &executable, None)?;
    let hash = preview["approval_sha256"].as_str().ok_or("approval")?;
    supervisor::prepare(
        &root,
        name,
        Input::File(&input),
        15,
        &executable,
        Some(hash),
    )?;
    let cancellation = supervisor::cancel(&root, name, None)?;
    supervisor::cancel(&root, name, cancellation["approval_sha256"].as_str())?;
    // A request alone is not a terminal receipt, even before any worker launches.
    assert!(spool::unload(&root, None).is_err());
    assert_eq!(spool::status(&root, name)?["termination_confirmed"], false);
    // Removed input must not prevent acknowledging a cancellation already requested.
    fs::remove_file(&input)?;
    let output = std::process::Command::new(&executable)
        .args(["spool-supervisor", "worker"])
        .arg(&root)
        .args([name, hash])
        .output()?;
    let result: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(result["status"], "cancelled", "{output:?}");
    assert_eq!(result["process_started"], false);
    assert_eq!(supervisor::completion(&root, name, hash)?, result);
    let view = spool::status(&root, name)?;
    assert_eq!(view["phase"], "cancelled-before-launch");
    assert_eq!(view["termination_confirmed"], true);
    let dir = root.join("runs").join(name);
    assert!(!dir.join("job").exists());
    assert!(!dir.join("start.json").exists());
    assert!(!dir.join("input-references.json").exists());
    assert!(accept(&root, name).is_err());
    assert!(reclaim(&root, name).is_err());
    assert!(spool::unload(&root, None).is_ok());

    // Missing ownership, mismatched completion and contradictory admission all
    // remain unknown; none is repaired or retried by the product.
    let owner = dir.join("supervisor/owner.json");
    let original = fs::read(&owner)?;
    fs::remove_file(&owner)?;
    assert!(spool::unload(&root, None).is_err());
    fs::write(&owner, original)?;
    let completion = dir.join("supervisor/completion.json");
    let original = fs::read(&completion)?;
    fs::write(&completion, b"{}")?;
    assert!(spool::unload(&root, None).is_err());
    fs::write(&completion, original)?;
    fs::write(dir.join("start.json"), b"{}")?;
    assert!(supervisor::completion(&root, name, hash).is_err());
    assert!(spool::unload(&root, None).is_err());
    assert_eq!(spool::status(&root, name)?["termination_confirmed"], false);
    fs::remove_file(dir.join("start.json"))?;
    let repeated = std::process::Command::new(&executable)
        .args(["spool-supervisor", "worker"])
        .arg(&root)
        .args([name, hash])
        .output()?;
    assert!(!repeated.status.success());
    assert!(!dir.join("job").exists());
    assert_eq!(spool::artifacts(&root, "baseline")?, products);
    unload(&root)?;
    assert!(dir.join("supervisor/completion.json").exists());
    Ok(())
}
