use super::*;
use std::io::Cursor;
fn archive(root: &Path, extra: bool, tamper: bool) -> Result<std::path::PathBuf> {
    let path = root.join(format!("{}-{}.tar.gz", extra, tamper));
    let content = b"runtime fixture";
    let value = json!({"schema":"an-offline-runtime-v1","name":"documents","version":"1","platform":"macos-aarch64",
        "licenses":["NOTICE"],"components":{"domain":"NOTICE"},"files":{"NOTICE":{"sha256":store::digest(content),"bytes":content.len(),"executable":false}}});
    let file = fs::File::create(&path)?;
    let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
        file,
        flate2::Compression::default(),
    ));
    for (name, bytes) in [
        ("runtime.json", serde_json::to_vec(&value)?),
        (
            "NOTICE",
            if tamper {
                b"altered fixture".to_vec()
            } else {
                content.to_vec()
            },
        ),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o400);
        header.set_cksum();
        archive.append_data(&mut header, name, Cursor::new(bytes))?;
    }
    if extra {
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o400);
        header.set_cksum();
        archive.append_data(&mut header, "extra", Cursor::new(b"x"))?;
    }
    archive.into_inner()?.finish()?;
    Ok(path)
}
#[test]
fn verifies_bytes_and_refuses_tampering_or_undeclared_payloads() -> Result<()> {
    let root = std::env::temp_dir()
        .canonicalize()?
        .join(an_agent_core::det_seam::Entropy::os().uuid_v4().to_string());
    store::directory(&root)?;
    let valid = archive(&root, false, false)?;
    assert_eq!(preview(&valid)?["manifest"]["name"], "documents");
    assert!(preview(&archive(&root, true, false)?).is_err());
    assert!(preview(&archive(&root, false, true)?).is_err());
    fs::remove_dir_all(root)?;
    Ok(())
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn import_is_atomic_bound_and_revalidates_reuse() -> Result<()> {
    let root = std::env::temp_dir()
        .canonicalize()?
        .join(an_agent_core::det_seam::Entropy::os().uuid_v4().to_string());
    store::directory(&root)?;
    let valid = archive(&root, false, false)?;
    let checked = preview(&valid)?;
    let digest = checked["approval_sha256"].as_str().ok_or("digest")?;
    let store = root.join("installed");
    assert!(import(&store, &valid, "wrong").is_err());
    assert_eq!(import(&store, &valid, digest)?["reused"], false);
    assert_eq!(import(&store, &valid, digest)?["reused"], true);
    use std::os::unix::fs::PermissionsExt;
    let notice = store.join(digest).join("NOTICE");
    fs::set_permissions(&notice, fs::Permissions::from_mode(0o600))?;
    fs::write(notice, b"changed fixture")?;
    assert!(import(&store, &valid, digest).is_err());
    assert!(
        !fs::read_dir(&store)?
            .any(|entry| entry
                .is_ok_and(|v| v.file_name().to_string_lossy().starts_with(".pending")))
    );
    fs::remove_dir_all(root)?;
    Ok(())
}
