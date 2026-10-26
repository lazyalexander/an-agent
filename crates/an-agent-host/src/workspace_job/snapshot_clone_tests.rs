use super::*;
use an_agent_core::testkit::TempDir;
use std::fs;

#[test]
fn clone_does_not_overwrite_follow_links_or_weaken_size_bounds() -> Result<()> {
    let temp = TempDir::new("clone-boundaries");
    let root = temp.path().canonicalize()?;
    let source = root.join("source");
    let target = root.join("target");
    fs::write(&source, b"approved")?;
    assert!(create(&source, &target, 1).is_err());
    assert!(!target.exists());
    std::os::unix::fs::symlink(&source, &target)?;
    assert!(create(&target, &root.join("other"), 8).is_err());
    assert!(create(&source, &target, 8).is_err());
    fs::remove_file(&target)?;
    fs::hard_link(&source, &target)?;
    assert!(create(&source, &root.join("other"), 8).is_err());
    fs::remove_file(&target)?;
    fs::write(&target, b"preserve")?;
    assert!(create(&source, &target, 8).is_err());
    assert_eq!(fs::read(&target)?, b"preserve");
    fs::remove_file(&target)?;
    if create(&source, &target, 8)? {
        fs::write(&source, b"changed!")?;
        assert_eq!(fs::read(&target)?, b"approved");
        fs::remove_file(&target)?;
        assert_eq!(fs::read(&source)?, b"changed!");
    }
    Ok(())
}
