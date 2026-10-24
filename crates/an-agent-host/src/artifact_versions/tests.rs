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
