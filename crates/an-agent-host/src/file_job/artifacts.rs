//! Artifact validation and host-owned output inode acceptance.
use super::OutputSlot;
use crate::execution::{Result, store};
use std::{fs, path::Path};

pub(crate) fn regular(path: &Path, limit: usize) -> Result<Vec<u8>> {
    store::check_read_path(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        read_opened(&file, limit)
    }
    #[cfg(not(unix))]
    {
        store::read(path, limit)
    }
}

#[cfg(unix)]
fn read_opened(file: &fs::File, limit: usize) -> Result<Vec<u8>> {
    use std::os::unix::fs::{FileExt, MetadataExt};
    let before = file.metadata()?;
    if !before.is_file() || before.nlink() != 1 || before.len() > limit as u64 {
        return Err("artifact must be a bounded, singly linked regular file".into());
    }
    let mut bytes = vec![0; limit + 1];
    let mut length = 0;
    while length < bytes.len() {
        let count = file.read_at(&mut bytes[length..], length as u64)?;
        if count == 0 {
            break;
        }
        length += count;
    }
    let after = file.metadata()?;
    if length > limit
        || after.nlink() != 1
        || before.len() != after.len()
        || length as u64 != after.len()
    {
        return Err("artifact changed or exceeded its limit during collection".into());
    }
    bytes.truncate(length);
    Ok(bytes)
}

pub(crate) fn prepare_output(work: &Path) -> Result<Option<OutputSlot>> {
    #[cfg(target_os = "linux")]
    {
        let path = work.join("output.txt");
        let initial = format!(
            "an-agent: output not produced {}\n",
            an_agent_core::det_seam::Entropy::os().uuid_v4()
        )
        .into_bytes();
        store::write_new(&path, &initial)?;
        let file = fs::File::open(&path)?;
        Ok(Some(OutputSlot { initial, file }))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = work;
        Ok(None)
    }
}

pub(crate) fn accept_output(work: &Path, slot: Option<&OutputSlot>) -> Result<Vec<u8>> {
    #[cfg(target_os = "linux")]
    if slot.is_none() {
        return Err("missing host-owned output slot".into());
    }
    let path = work.join("output.txt");
    #[cfg(target_os = "linux")]
    let bytes = read_opened(&slot.ok_or("missing output slot")?.file, 65536)?;
    #[cfg(not(target_os = "linux"))]
    let bytes = regular(&path, 65536)?;
    if let Some(slot) = slot {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            let original = slot.file.metadata()?;
            let current = fs::symlink_metadata(&path)?;
            if original.dev() != current.dev() || original.ino() != current.ino() {
                return Err("output inode replaced".into());
            }
        }
        if bytes == slot.initial {
            return Err("kit did not produce output".into());
        }
    }
    Ok(bytes)
}

#[cfg(all(test, target_os = "linux"))]
mod output_slot_tests {
    use super::*;
    use an_agent_core::testkit::TempDir;

    #[test]
    fn rejects_an_untouched_slot_replacement_and_hardlinks() -> Result<()> {
        let temp = TempDir::new("output-slot");
        let work = temp.path().canonicalize()?;
        let slot = prepare_output(&work)?;
        assert!(accept_output(&work, slot.as_ref()).is_err());
        assert!(accept_output(&work, None).is_err());
        fs::rename(work.join("output.txt"), work.join("original"))?;
        store::write_new(&work.join("output.txt"), b"replacement")?;
        assert!(accept_output(&work, slot.as_ref()).is_err());
        fs::remove_file(work.join("output.txt"))?;
        fs::rename(work.join("original"), work.join("output.txt"))?;
        fs::write(work.join("output.txt"), b"written")?;
        fs::hard_link(work.join("output.txt"), work.join("alias"))?;
        assert!(accept_output(&work, slot.as_ref()).is_err());
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod custody_tests {
    use super::*;
    use an_agent_core::testkit::TempDir;
    use std::os::unix::fs::symlink;

    #[test]
    fn opened_inode_remains_the_only_byte_source_when_name_is_replaced() -> Result<()> {
        let temp = TempDir::new("artifact-custody");
        let root = temp.path().canonicalize()?;
        let path = root.join("candidate");
        fs::write(&path, b"original")?;
        let file = fs::File::open(&path)?;
        fs::rename(&path, temp.path().join("held"))?;
        fs::write(&path, b"replacement")?;
        assert_eq!(read_opened(&file, 32)?, b"original");
        assert_eq!(regular(&path, 32)?, b"replacement");
        fs::remove_file(&path)?;
        symlink(temp.path().join("held"), &path)?;
        assert!(regular(&path, 32).is_err());
        fs::remove_file(&path)?;
        fs::hard_link(temp.path().join("held"), &path)?;
        assert!(read_opened(&file, 32).is_err());
        assert!(regular(&path, 32).is_err());
        fs::remove_file(&path)?;
        assert!(read_opened(&file, 3).is_err());
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod registered_boundary_tests {
    use super::*;
    use an_agent_core::testkit::TempDir;
    #[test]
    fn descriptor_reads_preserve_registered_custody_and_sidecar_refusals() -> Result<()> {
        let temp = TempDir::new("registered-descriptor-boundary");
        let root = temp
            .path()
            .canonicalize()?
            .join("revisions")
            .join("a".repeat(64));
        fs::create_dir_all(&root)?;
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let path = root.join(format!("{}{suffix}", store::REGISTERED_CUSTODY_FILE));
            fs::write(&path, b"private custody bytes")?;
            let error = regular(&path, 65536).unwrap_err();
            assert!(error.to_string().contains("raw access"), "{error}");
            assert!(store::read(&path, 65536).is_err());
        }
        Ok(())
    }
}
