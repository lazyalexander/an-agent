//! Optional OpenShell CLI adapter. The host owns admission and publication;
//! OpenShell owns the VM. One invocation owns one sandbox, never a shared one.
mod lifecycle;
mod transport;

use lifecycle::{Session, drive};
use transport::Cli;

use crate::execution::{Result, sandbox, store};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

const POLICY: &str = "version: 1\nfilesystem_policy:\n  include_workdir: true\n  read_only: [/usr, /lib, /lib64, /etc, /proc, /dev/urandom]\n  read_write: [/tmp, /dev/null]\nlandlock:\n  compatibility: hard_requirement\nnetwork_policies: {}\n";

/// Operator-owned configuration. Packages cannot select gateways or CLI flags.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    cli: PathBuf,
    cli_sha256: String,
    config_home: PathBuf,
    gateway: String,
    image: String,
    wall_seconds: u64,
    control_seconds: u64,
}

impl Profile {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let profile: Self = serde_json::from_slice(&store::read(path, 16384)?)?;
        if !profile.cli.is_absolute()
            || !profile.config_home.is_absolute()
            || !profile.config_home.is_dir()
            || profile.gateway.is_empty()
            || profile.gateway.len() > 64
            || !profile
                .gateway
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || !(1..=300).contains(&profile.wall_seconds)
            || !(5..=180).contains(&profile.control_seconds)
            || !profile
                .image
                .split_once("@sha256:")
                .is_some_and(|(name, hash)| {
                    !name.is_empty() && !name.starts_with('-') && store::valid_digest(hash)
                })
        {
            return Err(
                "invalid OpenShell operator profile; use absolute paths and a digest-pinned image"
                    .into(),
            );
        }
        store::no_links(&profile.config_home)?;
        profile.verify_cli()?;
        Ok(profile)
    }

    fn verify_cli(&self) -> Result<()> {
        if store::digest(&store::read(&self.cli, 128 * 1024 * 1024)?) != self.cli_sha256 {
            return Err("OpenShell CLI digest mismatch".into());
        }
        Ok(())
    }

    pub(crate) fn plan(&self) -> Value {
        json!({"backend":"openshell-vm-file-v1","cli":self.cli,"cli_sha256":self.cli_sha256,
            "gateway":self.gateway,"config_home":self.config_home,"image":self.image,
            "version":"0.1.5","supported":cfg!(target_os="macos"),"one_sandbox_per_run":true,
            "wall_seconds":self.wall_seconds,"control_seconds":self.control_seconds,
            "policy_sha256":store::digest(POLICY.as_bytes()),"automatic_retries":0,
            "artifact_acceptance":"successful exec, bounded candidate, confirmed sandbox stop",
            "native_fallback":false,"retention":"stopped sandbox retained for operator cleanup"})
    }
}

pub(crate) struct RemoteExecution {
    pub execution: sandbox::Execution,
    pub artifact: Option<Vec<u8>>,
    pub evidence: Value,
}

/// Run only after the existing host admission path authorizes this invocation.
pub(crate) fn execute(
    profile: &Profile,
    executable: &Path,
    directory: &Path,
    input: &[u8],
    cancelled: &AtomicBool,
) -> Result<RemoteExecution> {
    if !cfg!(target_os = "macos") {
        return Err("OpenShell backend has only been qualified on macOS; no fallback".into());
    }
    let bytes = store::read(executable, 20 * 1024 * 1024)?;
    if bytes.len() < 20 || &bytes[..6] != b"\x7fELF\x02\x01" || bytes[18..20] != [183, 0] {
        return Err("OpenShell Mac VM profile requires a Linux ARM64 ELF kit".into());
    }
    store::write_new(&directory.join("openshell-policy.yaml"), POLICY.as_bytes())?;
    store::write_new(&directory.join("openshell-input.txt"), input)?;
    // OpenShell v0.1.5 limits sandbox names to 19 characters.
    let nonce = an_agent_core::det_seam::Entropy::os()
        .uuid_v4()
        .simple()
        .to_string();
    let name = format!("an-{}", &nonce[..16]);
    // Durable identity before create: ambiguous failures can be reconciled by an operator.
    store::write_new(
        &directory.join("openshell-owner.json"),
        &serde_json::to_vec_pretty(&json!({
        "sandbox":name,"gateway":profile.gateway,"run":directory.file_name().unwrap_or_default().to_string_lossy(),"shared":false,"automatic_restart":false,
        "recovery":"stop this sandbox, verify Stopped; never adopt its artifacts automatically"}))?,
    )?;
    let mut session = Session {
        transport: Cli {
            profile,
            cwd: directory,
        },
        cancelled,
        seconds: profile.control_seconds,
        evidence: Vec::new(),
    };
    let result = drive(&mut session, &name, profile, executable, directory);
    store::write_new(
        &directory.join("openshell.json"),
        &serde_json::to_vec_pretty(&result.evidence)?,
    )?;
    Ok(result)
}
