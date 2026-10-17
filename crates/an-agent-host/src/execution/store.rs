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

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

// Store ownership is trusted: these checks reject static symlinks, not hostile
// same-user concurrent rename attacks. Never expose the store to a kit.
pub(super) fn no_links(path: &Path) -> Result<()> {
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

pub(super) fn directory(path: &Path) -> Result<()> {
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

pub(super) fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    no_links(path)?;
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

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(super) struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
pub(super) fn lock(root: &Path) -> Result<Lock> {
    let path = root.join("host.lock");
    write_new(
        &path,
        b"exclusive host operation; inspect before removing stale lock\n",
    )?;
    Ok(Lock(path))
}

pub(super) fn load(folder: &Path) -> Result<Deployment> {
    let yaml = String::from_utf8(read(&folder.join("spool.yaml"), 64 * 1024)?)?;
    let spec = spool::parse(&yaml)?;
    // A descriptor requests a capability; this host independently limits the
    // accepted ABI. Source analysis from the producer is never trusted here.
    if spec.constructor
        != (Constructor::Host {
            name: "an-agent-stdio-v1".into(),
        })
        || spec.sort != Sort::Beat
        || !spec.requires.is_empty()
        || spec.inverse != Inverse::Irreversible
        || spec.consumes != ["kit.request"]
        || spec.produces != ["kit.execute"]
        || spec.effect.file != FileFacet::None
        || spec.effect.memory != MemoryFacet::Ignore
        || spec.effect.net != Net::None
        || spec.effect.proc_ != Proc::Spawn
        || spec.effect.model != ModelFacet::None
        || spec.effect.flow != Flow::Both
        || spec.config.get("profile").and_then(|v| v.as_str()) != Some("stdio-only-v1")
    {
        return Err("descriptor exceeds the supported host stdio contract".into());
    }
    if spec
        .config
        .keys()
        .any(|key| !["profile", "executable_sha256", "source_sha256"].contains(&key.as_str()))
    {
        return Err("unknown stdio configuration; no silent interpretation".into());
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
