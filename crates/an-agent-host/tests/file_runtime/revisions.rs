//! Reuse the real sandboxed file-kit fixture; no parallel execution harness.
use super::*;
use an_agent_host::artifact_versions;

pub(super) fn cli(args: &[String]) -> execution::Result<Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_an-agent-host"))
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let stdout = std::str::from_utf8(&output.stdout)?;
    Ok(serde_json::from_str(
        stdout.lines().last().ok_or("missing CLI result")?,
    )?)
}

pub(super) fn path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn cli_versions_preserve_human_edits_and_pin_downstream_inputs() -> execution::Result<()> {
    let fixture = Fixture::new()?;
    let (_, source) = fixture.run("alpha\n")?;
    let catalog = fixture.root.join("catalog");
    let first = cli(&["artifact-import".into(), path(&catalog), path(&source)])?;
    let v1 = first["revision"].as_str().ok_or("missing revision")?;
    assert_eq!(
        artifact_versions::import(&catalog, &source)?["revision"],
        v1
    );
    let working = fixture.root.join("editable.txt");
    cli(&[
        "artifact-checkout".into(),
        path(&catalog),
        v1.into(),
        path(&working),
    ])?;
    assert_eq!(fs::read(&working)?, b"ALPHA\n");
    fs::write(&working, b"human beta\n")?;
    let second = cli(&[
        "artifact-edit".into(),
        path(&catalog),
        v1.into(),
        path(&working),
        digest(b"human beta\n"),
    ])?;
    let v2 = second["revision"].as_str().ok_or("missing revision")?;
    assert_ne!(v1, v2);
    assert_eq!(second["parent"], v1);
    // Continuing to edit does not mutate either accepted checkpoint.
    fs::write(&working, b"unsaved-to-catalog third draft\n")?;
    assert!(artifact_versions::edit(&catalog, v2, &working, &digest(b"human beta\n")).is_err());
    assert!(artifact_versions::checkout(&catalog, v1, &working).is_err());
    for (revision, input, output) in [
        (v1, "ALPHA\n", "ALPHA\n"),
        (v2, "human beta\n", "HUMAN BETA\n"),
    ] {
        let args = vec![
            "run-version".into(),
            path(&fixture.root.join("store")),
            fixture.release.clone(),
            path(&catalog),
            revision.into(),
        ];
        let before = fs::read_dir(fixture.root.join("store/runs"))?.count();
        let preview = cli(&args)?;
        assert_eq!(preview["input_revision"]["revision"], revision);
        assert_eq!(
            fs::read_dir(fixture.root.join("store/runs"))?.count(),
            before
        );
        let mut execute = args;
        execute.push("--execute".into());
        let result = cli(&execute)?;
        let receipt_path = Path::new(result["receipt"].as_str().ok_or("missing receipt")?);
        let run = receipt_path.parent().ok_or("missing run")?;
        let receipt: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
        assert_eq!(receipt["input_revision"]["revision"], revision);
        assert_eq!(
            receipt["input_revision"]["content_sha256"],
            digest(input.as_bytes())
        );
        assert_eq!(receipt["context"]["sha256"], digest(input.as_bytes()));
        assert_eq!(fs::read(run.join("input.txt"))?, input.as_bytes());
        assert_eq!(fs::read(run.join("artifact.txt"))?, output.as_bytes());
        assert!(fs::read_to_string(run.join("audit.json"))?.contains("applied"));
        let downstream = artifact_versions::import(&catalog, run)?;
        let manifest: Value = serde_json::from_slice(&fs::read(
            catalog
                .join("revisions")
                .join(downstream["revision"].as_str().ok_or("missing revision")?)
                .join("revision.json"),
        )?)?;
        assert_eq!(manifest["source"]["input_revision"]["revision"], revision);
    }
    assert_eq!(fs::read(source.join("artifact.txt"))?, b"ALPHA\n");
    let fork = artifact_versions::edit(&catalog, v1, &working, &digest(&fs::read(&working)?))?;
    assert_eq!(fork["parent"], v1);
    assert_ne!(fork["revision"], v2);
    assert_eq!(
        artifact_versions::edit(&catalog, v1, &working, &digest(&fs::read(&working)?))?["revision"],
        fork["revision"]
    );
    assert_eq!(fs::read(&working)?, b"unsaved-to-catalog third draft\n");
    Ok(())
}

#[test]
fn revision_validation_rejects_tampering_links_partial_runs_and_oversized_inputs()
-> execution::Result<()> {
    let fixture = Fixture::new()?;
    let (_, source) = fixture.run("alpha")?;
    let catalog = fixture.root.join("catalog");
    let initial = artifact_versions::import(&catalog, &source)?;
    assert!(artifact_versions::import(&source.join("nested-catalog"), &source).is_err());
    let v1 = initial["revision"].as_str().ok_or("missing revision")?;
    let working = fixture.root.join("working");
    artifact_versions::checkout(&catalog, v1, &working)?;
    assert!(artifact_versions::checkout(&catalog, v1, &catalog.join("editable")).is_err());
    assert!(
        artifact_versions::edit(&catalog, &"0".repeat(64), &working, &digest(b"ALPHA")).is_err()
    );
    fs::write(&working, vec![b'x'; 8193])?;
    let large = artifact_versions::edit(&catalog, v1, &working, &digest(&fs::read(&working)?))?;
    let run_count = fs::read_dir(fixture.root.join("store/runs"))?.count();
    assert!(
        execution::run_revision(
            &fixture.root.join("store"),
            &fixture.release,
            &catalog,
            large["revision"].as_str().ok_or("missing revision")?,
            true,
            &std::sync::atomic::AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(
        fs::read_dir(fixture.root.join("store/runs"))?.count(),
        run_count
    );
    fs::write(&working, [255])?;
    assert!(artifact_versions::edit(&catalog, v1, &working, &digest(&[255])).is_err());
    let link = fixture.root.join("link");
    std::os::unix::fs::symlink(&working, &link)?;
    assert!(artifact_versions::edit(&catalog, v1, &link, &digest(&[255])).is_err());
    let hardlink = fixture.root.join("hardlink");
    fs::hard_link(&working, &hardlink)?;
    assert!(artifact_versions::edit(&catalog, v1, &hardlink, &digest(&[255])).is_err());
    let (_, failed) = fixture.run("fail")?;
    assert!(artifact_versions::import(&catalog, &failed).is_err());
    fs::write(source.join("artifact.txt"), b"corrupt source")?;
    assert!(artifact_versions::import(&catalog, &source).is_err());
    let content = catalog.join("revisions").join(v1).join("content.txt");
    fs::write(&content, b"corrupt catalog")?;
    assert!(
        execution::run_revision(
            &fixture.root.join("store"),
            &fixture.release,
            &catalog,
            v1,
            true,
            &std::sync::atomic::AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(artifact_versions::checkout(&catalog, v1, &fixture.root.join("rejected")).is_err());
    assert!(
        artifact_versions::checkout(&catalog, "../escape", &fixture.root.join("rejected")).is_err()
    );
    fs::write(&content, b"ALPHA")?;
    fs::write(
        catalog.join("revisions").join(v1).join("revision.json"),
        b"{}",
    )?;
    assert!(artifact_versions::checkout(&catalog, v1, &fixture.root.join("rejected")).is_err());
    Ok(())
}
