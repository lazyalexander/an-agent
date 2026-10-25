//! Trusted executable publication; the embedding process never opens the
//! executable inode for writing. Ordinary data files still use store::write_new.
use super::{Result, sandbox, store};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
    time::Duration,
};

pub(crate) struct Publication {
    pub evidence: Value,
    pub stopped: Option<sandbox::Execution>,
}

#[derive(Debug, thiserror::Error)]
#[error("executable publication refused: {source}")]
pub(crate) struct PublicationError {
    #[source]
    source: Box<dyn std::error::Error>,
    pub evidence: Value,
}

struct Staging(PathBuf);

pub(crate) fn plan() -> Value {
    json!({"method":"separate-process-copy-v1", "helper":"/bin/cp",
        "environment":{}, "helper_wall_seconds":10, "retry_attempted":false,
        "publication":"verified read-only inode; atomic no-replace link"})
}

impl Drop for Staging {
    fn drop(&mut self) {
        // Only our two named files and empty directory; never recurse through
        // an output tree or remove the published destination.
        let _ = fs::remove_file(self.0.join("source.data"));
        let _ = fs::remove_file(self.0.join("candidate"));
        let _ = fs::remove_dir(&self.0);
    }
}

pub(crate) fn publish(path: &Path, bytes: &[u8], cancelled: &AtomicBool) -> Result<Publication> {
    let mut evidence = json!({"method":"separate-process-copy-v1", "published":false,
        "sha256":store::digest(bytes), "bytes":bytes.len(), "retry_attempted":false});
    let result = publish_inner(path, bytes, cancelled, &mut evidence);
    match result {
        Ok(stopped) => Ok(Publication { evidence, stopped }),
        Err(source) => Err(Box::new(PublicationError { source, evidence })),
    }
}

fn publish_inner(
    path: &Path,
    bytes: &[u8],
    cancelled: &AtomicBool,
    evidence: &mut Value,
) -> Result<Option<sandbox::Execution>> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return Err("executable publisher unsupported on this platform".into());
    }
    if bytes.is_empty() || bytes.len() > 20 * 1024 * 1024 {
        return Err("executable publication size outside host limits".into());
    }
    store::no_links(path)?;
    let parent = path
        .parent()
        .ok_or("missing publication directory")?
        .canonicalize()?;
    let destination = parent.join(path.file_name().ok_or("missing executable name")?);
    if destination.exists() {
        return Err("executable destination already exists".into());
    }
    let staging = parent.join(format!(
        ".publish-{}",
        an_agent_core::det_seam::Entropy::os().uuid_v4()
    ));
    let mut directory = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(&staging)?;
    let staging = Staging(staging);
    let source = staging.0.join("source.data");
    let candidate = staging.0.join("candidate");
    // This data inode is never executed, linked or renamed into an executable.
    store::write_new(&source, bytes)?;
    // Fixed trusted OS utility, absolute paths, no shell, hooks or inherited
    // environment. Only its process opens the candidate for writing. Waiting
    // for its exit closes that writer before any publication or kit launch.
    let mut copy = Command::new("/bin/cp");
    copy.arg(&source).arg(&candidate).env_clear();
    let result = sandbox::execute_command(
        copy,
        &staging.0,
        b"",
        Duration::from_secs(10),
        cancelled,
        false,
    )?;
    evidence["helper"] = json!({"program":"/bin/cp", "status":result.status,
        "pid":result.pid, "reaped":result.reaped, "exit_code":result.code,
        "stderr":String::from_utf8_lossy(&result.stderr)});
    if result.status != "succeeded" {
        let status = match result.status {
            "cancelled" => "cancelled",
            "timed_out" => "timed_out",
            _ => "refused",
        };
        return Ok(Some(sandbox::Execution {
            status,
            code: None,
            stdout: vec![],
            stderr: serde_json::to_vec(evidence)?,
            pid: None,
            reaped: false,
            group_cleanup_required: false,
        }));
    }
    if !result.reaped || result.code != Some(0) {
        return Err("publisher completion was not confirmed".into());
    }
    if store::read(&candidate, 20 * 1024 * 1024)? != bytes {
        return Err("publisher output bytes differ from admitted executable".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let input = fs::metadata(&source)?;
        let output = fs::metadata(&candidate)?;
        if output.nlink() != 1 || (input.dev(), input.ino()) == (output.dev(), output.ino()) {
            return Err("publisher did not create an independent executable inode".into());
        }
        fs::set_permissions(&candidate, fs::Permissions::from_mode(0o500))?;
    }
    // Read-only descriptor: safe for concurrent host forks to inherit.
    fs::File::open(&candidate)?.sync_all()?;
    // Same-filesystem link is atomic and never replaces an existing entry.
    // The staged name is removed on scope exit; the published inode remains.
    fs::hard_link(&candidate, &destination)?;
    evidence["published"] = json!(true);
    Ok(None)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use an_agent_core::testkit::TempDir;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn publisher_verifies_bytes_reaps_helper_and_never_overwrites() -> Result<()> {
        let root = TempDir::new("publisher");
        let path = root.path().canonicalize()?.join("program");
        let published = publish(&path, b"image bytes", &AtomicBool::new(false))?;
        assert!(published.stopped.is_none());
        assert_eq!(published.evidence["published"], true);
        assert_eq!(published.evidence["helper"]["reaped"], true);
        assert_eq!(published.evidence["helper"]["exit_code"], 0);
        assert_eq!(published.evidence["sha256"], store::digest(b"image bytes"));
        assert_eq!(fs::read(&path)?, b"image bytes");
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o500);
        assert!(publish(&path, b"replacement", &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(&path)?, b"image bytes");
        assert_eq!(fs::read_dir(root.path())?.count(), 1);
        Ok(())
    }

    #[test]
    fn cancelled_publication_starts_no_helper_and_leaves_no_executable() -> Result<()> {
        let root = TempDir::new("cancelled-publisher");
        let path = root.path().canonicalize()?.join("program");
        let published = publish(&path, b"image bytes", &AtomicBool::new(true))?;
        assert_eq!(published.evidence["published"], false);
        assert!(published.evidence["helper"]["pid"].is_null());
        assert_eq!(
            published.stopped.ok_or("missing cancellation")?.status,
            "cancelled"
        );
        assert!(!path.exists());
        assert_eq!(fs::read_dir(root.path())?.count(), 0);
        Ok(())
    }

    #[test]
    fn existing_symlink_and_invalid_payload_do_not_publish() -> Result<()> {
        let root = TempDir::new("publisher-refusal");
        let canonical = root.path().canonicalize()?;
        let protected = canonical.join("protected");
        fs::write(&protected, b"untouched")?;
        let path = canonical.join("program");
        std::os::unix::fs::symlink(&protected, &path)?;
        let error = match publish(&path, b"new bytes", &AtomicBool::new(false)) {
            Err(error) => error,
            Ok(_) => return Err("symlink accepted".into()),
        };
        assert_eq!(
            sandbox::refusal_evidence(error.as_ref())["stage"],
            "executable-publication"
        );
        assert_eq!(fs::read(&protected)?, b"untouched");
        assert!(publish(&canonical.join("empty"), b"", &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read_dir(root.path())?.count(), 2);
        Ok(())
    }

    #[test]
    fn concurrent_publication_of_one_name_has_exactly_one_winner() -> Result<()> {
        let root = TempDir::new("publisher-race");
        let path = root.path().canonicalize()?.join("program");
        let barrier = std::sync::Barrier::new(2);
        let outcomes = std::thread::scope(|scope| {
            let workers: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
                .into_iter()
                .map(|bytes| {
                    let path = &path;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        publish(path, bytes, &AtomicBool::new(false))
                            .map(|result| result.stopped.is_none())
                            .map_err(|error| error.to_string())
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            outcomes
                .iter()
                .filter(|result| matches!(result, Ok(true)))
                .count(),
            1,
            "{outcomes:?}"
        );
        let bytes = fs::read(&path)?;
        assert!(bytes == b"first" || bytes == b"second");
        assert_eq!(fs::read_dir(root.path())?.count(), 1);
        Ok(())
    }
}
