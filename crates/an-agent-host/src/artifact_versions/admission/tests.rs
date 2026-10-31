use super::*;
use an_agent_core::testkit::TempDir;

#[test]
fn retirement_fences_new_acquisition_without_excluding_readers_or_destroying_evidence() -> Result<()>
{
    let temp = TempDir::new("artifact-admission");
    let catalog = temp.path().canonicalize()?.join("catalog");
    let artifact = publish(&catalog, b"retained", None, json!({"kind":"fixture"}))?;
    let revision = artifact["revision"].as_str().unwrap();
    let authority = json!({"owner":"qualified-host"});
    let (selected, binding) = read_input(&catalog, revision)?;
    let preview = retire(&catalog, revision, authority.clone(), None)?;
    let hash = preview["approval_sha256"].as_str().unwrap();
    let path = marker(&catalog, revision)?;
    assert!(!path.exists());
    let lock = store::lock(&catalog)?;
    assert_eq!(read_input(&catalog, revision)?.0, b"retained");
    assert!(retire(&catalog, revision, authority.clone(), Some(hash)).is_err());
    drop(lock);
    assert!(retire(&catalog, revision, json!({"owner":"other"}), Some(hash)).is_err());
    assert!(!path.exists());
    retire(&catalog, revision, authority.clone(), Some(hash))?;
    assert_eq!(selected, b"retained");
    assert_eq!(read(&catalog, revision)?, (selected, binding));
    assert!(read_input(&catalog, revision).is_err());
    assert!(checkout(&catalog, revision, &temp.path().join("copy")).is_err());
    assert_eq!(status(&catalog, revision)?["mode"], "retired");
    assert_eq!(
        retire(&catalog, revision, authority, Some(hash))?["already_retired"],
        true
    );
    Ok(())
}

#[test]
fn incomplete_or_linked_retirement_is_a_fence_never_a_repair_request() -> Result<()> {
    let temp = TempDir::new("artifact-retirement-fault");
    let catalog = temp.path().canonicalize()?.join("catalog");
    let artifact = publish_bytes(&catalog, &[0xff, 0], json!({"kind":"fixture"}))?;
    let revision = artifact["revision"].as_str().unwrap();
    let path = marker(&catalog, revision)?;
    fs::write(&path, b"{")?;
    assert!(read_input(&catalog, revision).is_err());
    assert!(status(&catalog, revision).is_err());
    assert!(retire(&catalog, revision, json!({}), None).is_err());
    assert_eq!(fs::read(&path)?, b"{");
    assert_eq!(read(&catalog, revision)?.0, [0xff, 0]);
    #[cfg(unix)]
    {
        fs::remove_file(&path)?;
        std::os::unix::fs::symlink(temp.path().join("absent"), &path)?;
        assert!(read_input(&catalog, revision).is_err());
        assert!(status(&catalog, revision).is_err());
    }
    Ok(())
}
