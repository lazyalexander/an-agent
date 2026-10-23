//! Artifact validation and host-owned output inode acceptance.
use super::OutputSlot;
use crate::execution::{Result, store};
use std::{fs, path::Path};

pub(crate) fn regular(path: &Path, limit: usize) -> Result<Vec<u8>> {
    store::no_links(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if fs::symlink_metadata(path)?.nlink() != 1 {
            return Err("linked artifact refused".into());
        }
    }
    store::read(path, limit)
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
