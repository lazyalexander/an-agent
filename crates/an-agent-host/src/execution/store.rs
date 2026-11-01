//! Host-owned immutable deployment snapshots. No archive extraction or build hooks.
use super::Result;
use an_agent_core::act::{FileFacet, MemoryFacet, ModelFacet};
use an_agent_spool::{
    descriptor::{Net, Proc},
    spool::{self, Constructor, Flow, Inverse, Sort, SpoolSpec},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub(super) struct Deployment {
    pub spec: SpoolSpec,
    pub program: Vec<u8>,
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    let timing = super::metrics::Span::new("sha256", Path::new(""), bytes.len());
    let result = format!("{:x}", Sha256::digest(bytes));
    timing.finish("ok");
    result
}

pub(crate) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

// Store ownership is trusted: these checks reject static symlinks, not hostile
// same-user concurrent rename attacks. Never expose the store to a kit.
pub(crate) fn no_links(path: &Path) -> Result<()> {
    let mut cursor = PathBuf::new();
    for part in path.components() {
        if matches!(part, Component::ParentDir) {
            return Err("parent traversal is not allowed".into());
        }
        cursor.push(part);
        match fs::symlink_metadata(&cursor) {
            Ok(meta) if meta.file_type().is_symlink() => return Err("symlink in host path".into()),
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub(crate) fn directory(path: &Path) -> Result<()> {
    no_links(path)?;
    if !path.exists() {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
    }
    if !path.is_dir() {
        return Err("expected host directory".into());
    }
    Ok(())
}

pub(crate) const REGISTERED_CUSTODY_FILE: &str = "registered-custody.sqlite3";

pub(crate) fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    no_links(path)?;
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.to_ascii_lowercase().starts_with(REGISTERED_CUSTODY_FILE))
        && path
            .canonicalize()?
            .ancestors()
            .nth(2)
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("revisions"))
    {
        return Err("raw access to registered host custody is refused".into());
    }
    if !fs::metadata(path)?.is_file() {
        return Err("expected regular file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err("file exceeds host limit".into());
    }
    Ok(bytes)
}

pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let timing = super::metrics::Span::new("write_sync", path, bytes.len());
    let result = (|| -> Result<()> {
        let mut file = options.open(path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    timing.finish(if result.is_ok() { "ok" } else { "error" });
    result
}

pub(crate) struct Lock(PathBuf, Option<super::metrics::Span>);
impl Drop for Lock {
    fn drop(&mut self) {
        let removed = fs::remove_file(&self.0);
        if let Some(timing) = self.1.take() {
            timing.finish(if removed.is_ok() {
                "released"
            } else {
                "release_error"
            });
        }
    }
}
pub(crate) fn lock(root: &Path) -> Result<Lock> {
    let path = root.join("host.lock");
    let hold = super::metrics::Span::new("lock_hold", root, 0);
    let timing = super::metrics::Span::new("lock_acquire", root, 0);
    let result = write_new(
        &path,
        b"exclusive host operation; inspect before removing stale lock\n",
    );
    let outcome = match &result {
        Ok(_) => "acquired",
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::AlreadyExists) =>
        {
            "conflict"
        }
        Err(_) => "error",
    };
    timing.finish(outcome);
    result?;
    Ok(Lock(path, Some(hold)))
}

pub(super) fn load(folder: &Path) -> Result<Deployment> {
    let yaml = String::from_utf8(read(&folder.join("spool.yaml"), 64 * 1024)?)?;
    let spec = spool::parse(&yaml)?;
    let file = spec.config.get("profile").and_then(|v| v.as_str()) == Some("file-kit-v1");
    // A descriptor requests a capability; this host independently limits the
    // accepted ABI. Source analysis from the producer is never trusted here.
    if spec.constructor
        != (Constructor::Host {
            name: if file {
                "an-agent-file-v1"
            } else {
                "an-agent-stdio-v1"
            }
            .into(),
        })
        || spec.sort != Sort::Beat
        || !spec.requires.is_empty()
        || spec.inverse != Inverse::Irreversible
        || spec.consumes != ["kit.request"]
        || spec.produces != ["kit.execute"]
        || spec.effect.file
            != if file {
                FileFacet::ReadWrite {
                    path: "/work".into(),
                    recursive: true,
                }
            } else {
                FileFacet::None
            }
        || spec.effect.memory != MemoryFacet::Ignore
        || spec.effect.net != Net::None
        || spec.effect.proc_ != Proc::Spawn
        || spec.effect.model != ModelFacet::None
        || spec.effect.flow != Flow::Both
        || !matches!(
            spec.config.get("profile").and_then(|v| v.as_str()),
            Some("stdio-only-v1" | "file-kit-v1")
        )
    {
        return Err("descriptor exceeds the supported host CLI contract".into());
    }
    if spec
        .config
        .keys()
        .any(|key| !["profile", "executable_sha256", "source_sha256"].contains(&key.as_str()))
    {
        return Err("unknown CLI configuration; no silent interpretation".into());
    }
    let program = read(&folder.join("program"), 20 * 1024 * 1024)?;
    let expected = spec
        .config
        .get("executable_sha256")
        .and_then(|v| v.as_str())
        .ok_or("missing executable digest")?;
    if program.is_empty() || !valid_digest(expected) || digest(&program) != expected {
        return Err("executable digest mismatch".into());
    }
    Ok(Deployment { spec, program })
}
