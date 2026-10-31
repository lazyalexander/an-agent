use super::*;
use an_agent_core::testkit::TempDir;

fn fixture(base: &Path) -> Result<PathBuf> {
    let root = base.join("source");
    fs::create_dir(&root)?;
    fs::create_dir(root.join("silk"))?;
    let code = b"printf result > result.txt\n";
    fs::write(root.join("silk/main.sh"), code)?;
    write_json(
        &root.join(bundle::FILE),
        &json!({"schema":"an-native-spool-v1","name":"sample","version":"1.0.0",
        "entry":"silk/main.sh","files":{"silk/main.sh":store::digest(code)},"outputs":["result.txt"]}),
    )?;
    Ok(root)
}
#[test]
fn load_is_nonexecuting_exclusive_and_checks_all_release_bytes() -> Result<()> {
    let temp = TempDir::new("native-package-contract");
    let base = temp.path().canonicalize()?;
    let source = fixture(&base)?;
    let target = base.join("instance");
    assert!(load(&source, &source.join("nested")).is_err());
    let loaded = load(&source, &target)?;
    assert_eq!(loaded["execution_granted"], false);
    assert_eq!(loaded["capabilities"]["pause"], false);
    assert_eq!(
        loaded["capabilities"]["pause_between_attempts"],
        cfg!(target_os = "macos")
    );
    assert_eq!(loaded["round_control"]["generation"], 0);
    assert_eq!(loaded["round_control"]["mode"], "active");
    assert!(!target.join("round-control.json").exists());
    assert!(!target.join("result.txt").exists());
    assert!(load(&source, &target).is_err());
    fs::write(source.join("silk/main.sh"), b"substituted")?;
    assert!(load(&source, &base.join("changed")).is_err());
    assert!(!base.join("changed").exists());
    Ok(())
}
#[test]
fn manifest_cannot_request_runtime_grants_or_import_unlisted_files() -> Result<()> {
    let temp = TempDir::new("native-unlisted");
    let base = temp.path().canonicalize()?;
    let source = fixture(&base)?;
    fs::write(source.join("extra"), b"unlisted")?;
    assert!(load(&source, &base.join("extra-target")).is_err());
    fs::remove_file(source.join("extra"))?;
    let path = source.join(bundle::FILE);
    let mut m: Value = serde_json::from_slice(&fs::read(&path)?)?;
    m["network"] = json!(true);
    fs::write(&path, serde_json::to_vec(&m)?)?;
    assert!(load(&source, &base.join("grant-target")).is_err());
    Ok(())
}

#[test]
fn binary_output_encoding_is_versioned_unique_and_declared() -> Result<()> {
    let temp = TempDir::new("native-binary-contract");
    let base = temp.path().canonicalize()?;
    let source = fixture(&base)?;
    let path = source.join(bundle::FILE);
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path)?)?;
    manifest["binary_outputs"] = json!(["result.txt"]);
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    assert!(bundle::inspect(&source).is_err());
    manifest["schema"] = json!("an-native-spool-v3");
    manifest["events"] = json!({"consumes":[],"produces":["artifact.accepted"]});
    manifest["requirements"] = json!({"runtime":"macos-native-process-v1","os":"macos",
        "arch":"portable-source","interaction":"headless","gpu":"none",
        "inputs":["source"],"editable":[],"capabilities":["read-input","write-declared-outputs"],"dependencies":[]});
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    assert!(bundle::inspect(&source).is_ok());
    for binary in [
        json!(["undeclared.bin"]),
        json!(["result.txt", "result.txt"]),
    ] {
        manifest["binary_outputs"] = binary;
        fs::write(&path, serde_json::to_vec(&manifest)?)?;
        assert!(bundle::inspect(&source).is_err());
    }
    Ok(())
}
