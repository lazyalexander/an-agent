//! Optional OpenShell CLI adapter. The host owns admission and publication;
//! OpenShell owns the VM. One invocation owns one sandbox, never a shared one.
use crate::execution::{Result, sandbox, store};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
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

trait Transport {
    fn call(
        &mut self,
        step: &str,
        args: &[String],
        cancel: &AtomicBool,
        seconds: u64,
    ) -> Result<sandbox::Execution>;
}

struct Cli<'a> {
    profile: &'a Profile,
    cwd: &'a Path,
}

impl Transport for Cli<'_> {
    fn call(
        &mut self,
        step: &str,
        args: &[String],
        cancel: &AtomicBool,
        seconds: u64,
    ) -> Result<sandbox::Execution> {
        self.profile.verify_cli()?;
        let cleanup = matches!(step, "verify-owner" | "stop" | "confirm-stop");
        let journal = store::write_new(
            &self.cwd.join(format!("openshell-{step}-request.json")),
            &serde_json::to_vec(&json!({"step":step,"args":args,"deadline_seconds":seconds}))?,
        );
        // Exhausted local storage must not prevent remote termination.
        if !cleanup {
            journal?;
        }
        let mut command = Command::new(&self.profile.cli);
        // No ambient API keys, proxy overrides, active gateway or user config.
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("XDG_CONFIG_HOME", &self.profile.config_home)
            .env("OPENSHELL_TELEMETRY_ENABLED", "false")
            .env(
                "OPENSHELL_SYSTEM_GATEWAY_DIR",
                self.cwd.join("no-system-gateways"),
            )
            .args(["--gateway", &self.profile.gateway, "--color", "never"])
            .args(args);
        let result = sandbox::execute_command(
            command,
            self.cwd,
            b"",
            Duration::from_secs(seconds),
            cancel,
            false,
        );
        let evidence = match &result {
            Ok(r) => json!({"status":r.status,"exit_code":r.code,"cli_reaped":r.reaped,
                "stdout_sha256":store::digest(&r.stdout),"stderr":String::from_utf8_lossy(&r.stderr)}),
            Err(e) => json!({"error":e.to_string()}),
        };
        let journal = store::write_new(
            &self.cwd.join(format!("openshell-{step}-result.json")),
            &serde_json::to_vec(&evidence)?,
        );
        if !cleanup {
            journal?;
        }
        result
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).to_owned()).collect()
}
fn succeeded(result: &sandbox::Execution) -> bool {
    result.status == "succeeded" && result.code == Some(0) && result.reaped
}

struct Session<'a, T> {
    transport: T,
    cancelled: &'a AtomicBool,
    seconds: u64,
    evidence: Vec<Value>,
}

impl<T: Transport> Session<'_, T> {
    fn call(
        &mut self,
        step: &str,
        args: Vec<String>,
        cleanup: bool,
        seconds: u64,
    ) -> Result<sandbox::Execution> {
        // Cancellation must not cancel its own cleanup or state confirmation.
        let never_cancel = AtomicBool::new(false);
        let result = self.transport.call(
            step,
            &args,
            if cleanup {
                &never_cancel
            } else {
                self.cancelled
            },
            seconds,
        );
        match &result {
            Ok(r) => self.evidence.push(json!({"step":step,"status":r.status,"exit_code":r.code,
                "cli_pid":r.pid,"cli_reaped":r.reaped,
                "stdout":if step == "candidate" { String::new() } else { String::from_utf8_lossy(&r.stdout).into_owned() },
                "stdout_sha256":store::digest(&r.stdout),"stderr":String::from_utf8_lossy(&r.stderr)})),
            Err(e) => self.evidence.push(json!({"step":step,"error":e.to_string()})),
        }
        result
    }
    fn control(&mut self, step: &str, args: Vec<String>) -> Result<sandbox::Execution> {
        let result = self.call(step, args, false, self.seconds)?;
        if !succeeded(&result) {
            return Err(result.status.into());
        }
        Ok(result)
    }
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

fn drive<T: Transport>(
    session: &mut Session<'_, T>,
    name: &str,
    profile: &Profile,
    executable: &Path,
    directory: &Path,
) -> RemoteExecution {
    let owner = directory.file_name().unwrap_or_default().to_string_lossy();
    let mut attempted = false;
    let mut identity: Option<String> = None;
    let mut generation: Option<String> = None;
    let mut candidate = None;
    let mut execution = sandbox::Execution {
        status: "failed",
        code: None,
        stdout: vec![],
        stderr: vec![],
        pid: None,
        reaped: false,
    };
    let work = (|| -> Result<()> {
        let info = session.control("gateway", strings(&["gateway", "info", "-o", "json"]))?;
        let info: Value = serde_json::from_slice(&info.stdout)?;
        let drivers = info["compute_drivers"]
            .as_array()
            .ok_or("missing gateway driver evidence")?;
        if drivers.len() != 1 || drivers[0]["name"] != "vm" || info["version"] != "0.1.5" {
            return Err("gateway must use the VM driver".into());
        }
        let version = session.control("version", strings(&["--version"]))?;
        if String::from_utf8_lossy(&version.stdout).trim() != "openshell 0.1.5" {
            return Err("unqualified OpenShell CLI version".into());
        }
        if session.cancelled.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        attempted = true;
        session.control(
            "create",
            strings(&[
                "sandbox",
                "create",
                "--name",
                name,
                "--from",
                &profile.image,
                "--policy",
                &directory.join("openshell-policy.yaml").to_string_lossy(),
                "--no-auto-providers",
                "--annotation",
                &format!("an-agent.run={owner}"),
                "--restart-policy",
                "never",
                "--detach",
                "--",
                "/bin/sleep",
                "86400",
            ]),
        )?;
        let state = session.control("ready", strings(&["sandbox", "get", name, "-o", "json"]))?;
        let state: Value = serde_json::from_slice(&state.stdout)?;
        if state["name"] != name
            || state["phase"] != "Ready"
            || state["annotations"]["an-agent.run"] != owner.as_ref()
        {
            return Err("sandbox not ready".into());
        }
        identity = Some(state["id"].as_str().ok_or("missing sandbox ID")?.to_owned());
        generation = Some(
            state["annotations"]["internal.openshell.ai/runtime-generation"]
                .as_str()
                .ok_or("missing runtime generation")?
                .to_owned(),
        );
        if state["policy"]["landlock"]["compatibility"] != "hard_requirement"
            || state["policy"]["network_policies"]
                .as_object()
                .is_some_and(|rules| !rules.is_empty())
        {
            return Err("sandbox policy does not match the offline file-kit boundary".into());
        }
        for (step, source, target) in [
            (
                "upload-program",
                executable.to_path_buf(),
                "/sandbox/program",
            ),
            (
                "upload-input",
                directory.join("openshell-input.txt"),
                "/sandbox/input.txt",
            ),
        ] {
            session.control(
                step,
                strings(&[
                    "sandbox",
                    "upload",
                    name,
                    &source.to_string_lossy(),
                    target,
                    "--no-git-ignore",
                ]),
            )?;
        }
        session.control(
            "permissions",
            strings(&[
                "sandbox",
                "exec",
                name,
                "--no-login-shell",
                "--",
                "/bin/chmod",
                "500",
                "/sandbox/program",
            ]),
        )?;
        execution = session.call(
            "execute",
            strings(&[
                "sandbox",
                "exec",
                name,
                "--no-login-shell",
                "--workdir",
                "/sandbox",
                "--",
                "/sandbox/program",
            ]),
            false,
            profile.wall_seconds,
        )?;
        if !succeeded(&execution) {
            return Err(execution.status.into());
        }
        // The guest never extracts an archive onto the host. Read one bounded,
        // untrusted candidate. A guest race cannot exceed the host stream limit.
        let output = session.control("candidate", strings(&["sandbox","exec",name,"--no-login-shell","--","/bin/sh","-c",
            "test ! -L /sandbox/output.txt && test -f /sandbox/output.txt && exec /usr/bin/head -c 65537 -- /sandbox/output.txt"]))?;
        std::str::from_utf8(&output.stdout)?;
        candidate = Some(output.stdout);
        Ok(())
    })();
    let mut status = match work {
        Ok(()) => "succeeded",
        Err(ref error) => {
            execution
                .stderr
                .extend_from_slice(format!("\nadapter: {error}").as_bytes());
            match error.to_string().as_str() {
                "cancelled" => "cancelled",
                "timed_out" => "timed_out",
                _ => "failed",
            }
        }
    };
    let mut stopped = !attempted;
    if attempted {
        // An ambiguous create must never authorize stopping someone else's VM.
        let owned = session
            .call(
                "verify-owner",
                strings(&["sandbox", "get", name, "-o", "json"]),
                true,
                session.seconds,
            )
            .ok()
            .filter(succeeded)
            .and_then(|r| serde_json::from_slice::<Value>(&r.stdout).ok())
            .is_some_and(|state| {
                state["name"] == name && state["annotations"]["an-agent.run"] == owner.as_ref()
            });
        if owned {
            // Never retry create or exec. Stop is a separate authority request.
            let _ = session.call(
                "stop",
                strings(&["sandbox", "stop", name]),
                true,
                session.seconds,
            );
            let observed = session.call(
                "confirm-stop",
                strings(&["sandbox", "get", name, "-o", "json"]),
                true,
                session.seconds,
            );
            stopped = observed
                .ok()
                .filter(succeeded)
                .and_then(|r| serde_json::from_slice::<Value>(&r.stdout).ok())
                .is_some_and(|state| {
                    state["name"] == name
                        && state["annotations"]["an-agent.run"] == owner.as_ref()
                        && state["phase"] == "Stopped"
                        && state["id"]
                            .as_str()
                            .is_some_and(|id| identity.as_ref().is_none_or(|prior| prior == id))
                        && generation.as_ref().is_none_or(|prior| {
                            state["annotations"]["internal.openshell.ai/runtime-generation"]
                                .as_str()
                                == Some(prior)
                        })
                });
        }
    }
    if session.cancelled.load(Ordering::Relaxed) && status == "succeeded" {
        status = "cancelled";
    }
    let reason = status;
    if !stopped {
        status = "termination_unconfirmed";
    }
    let accept = status == "succeeded" && stopped;
    execution.status = status;
    let evidence = json!({"schema":"an-openshell-execution-v1","sandbox":name,"sandbox_id":identity,"runtime_generation":generation,
        "submitted":attempted,"status":status,"stop_reason":reason,"termination_confirmed":attempted && stopped,
        "candidate_eligible":accept,"automatic_retries":0,"steps":session.evidence});
    RemoteExecution {
        execution,
        artifact: if accept { candidate } else { None },
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake<'a> {
        cancelled: &'a AtomicBool,
        failure: &'static str,
        steps: Vec<String>,
        stopped: bool,
    }
    fn ok(bytes: Vec<u8>) -> sandbox::Execution {
        sandbox::Execution {
            status: "succeeded",
            code: Some(0),
            stdout: bytes,
            stderr: vec![],
            pid: Some(7),
            reaped: true,
        }
    }
    impl Transport for Fake<'_> {
        fn call(
            &mut self,
            step: &str,
            _args: &[String],
            cancel: &AtomicBool,
            _seconds: u64,
        ) -> Result<sandbox::Execution> {
            self.steps.push(step.into());
            if matches!(step, "stop" | "confirm-stop") {
                assert!(!cancel.load(Ordering::Relaxed), "cleanup was cancelled");
            }
            if step == "stop" {
                if self.failure == "stop" {
                    return Err("gateway unreachable".into());
                }
                self.stopped = true;
            }
            if step == "execute" && self.failure == "timeout" {
                let mut result = ok(vec![]);
                result.status = "timed_out";
                result.code = None;
                return Ok(result);
            }
            if step == "candidate" && self.failure == "cancel" {
                self.cancelled.store(true, Ordering::Relaxed);
            }
            if step == "candidate" && self.failure == "oversize" {
                let mut result = ok(vec![b'x'; 65536]);
                result.status = "output_limit";
                return Ok(result);
            }
            if step == "create" && self.failure == "create" {
                return Err("transport lost after create".into());
            }
            Ok(match step {
                "gateway" => {
                    ok(br#"{"version":"0.1.5","compute_drivers":[{"name":"vm"}]}"#.to_vec())
                }
                "version" => ok(b"openshell 0.1.5\n".to_vec()),
                "ready" | "verify-owner" | "confirm-stop" => ok(serde_json::to_vec(&json!({
                    "name":"an-test","id":if step=="confirm-stop" && self.failure=="identity" {"replacement"} else {"original"},
                    "phase":if self.stopped {"Stopped"} else {"Ready"},
                    "annotations":{"an-agent.run":if self.failure=="owner" {"another-run"} else {"run"},"internal.openshell.ai/runtime-generation":if step=="confirm-stop" && self.failure=="generation" {"replacement"} else {"generation-1"}},
                    "policy":{"landlock":{"compatibility":"hard_requirement"}}
                }))?),
                "candidate" => ok(b"alpha\nbeta\n".to_vec()),
                _ => ok(vec![]),
            })
        }
    }
    fn fixture(failure: &'static str) -> (RemoteExecution, Vec<String>) {
        let cancelled = AtomicBool::new(false);
        let profile = Profile {
            cli: "/test/openshell".into(),
            cli_sha256: "0".repeat(64),
            config_home: "/test/config".into(),
            gateway: "test".into(),
            image: format!("test@sha256:{}", "0".repeat(64)),
            wall_seconds: 1,
            control_seconds: 5,
        };
        let mut session = Session {
            transport: Fake {
                cancelled: &cancelled,
                failure,
                steps: vec![],
                stopped: false,
            },
            cancelled: &cancelled,
            seconds: 5,
            evidence: vec![],
        };
        let result = drive(
            &mut session,
            "an-test",
            &profile,
            Path::new("/test/program"),
            Path::new("/test/run"),
        );
        (result, session.transport.steps)
    }
    #[test]
    fn successful_candidate_requires_stop_confirmation() {
        let (result, steps) = fixture("");
        assert_eq!(result.execution.status, "succeeded");
        assert_eq!(result.artifact, Some(b"alpha\nbeta\n".to_vec()));
        assert_eq!(&steps[steps.len() - 2..], ["stop", "confirm-stop"]);
    }
    #[test]
    fn timeout_never_collects_late_artifacts_or_retries_execution() {
        let (result, steps) = fixture("timeout");
        assert_eq!(result.execution.status, "timed_out");
        assert_eq!(result.evidence["termination_confirmed"], true);
        assert!(result.artifact.is_none());
        assert!(!steps.iter().any(|s| s == "candidate"));
        assert_eq!(steps.iter().filter(|s| s.as_str() == "execute").count(), 1);
    }
    #[test]
    fn cancellation_after_capture_discards_candidate_and_does_not_cancel_cleanup() {
        let (result, _) = fixture("cancel");
        assert_eq!(result.execution.status, "cancelled");
        assert_eq!(result.evidence["termination_confirmed"], true);
        assert!(result.artifact.is_none());
    }
    #[test]
    fn unconfirmed_or_replaced_sandbox_cannot_publish_success() {
        for failure in ["stop", "identity", "generation"] {
            let (result, _) = fixture(failure);
            assert_eq!(result.execution.status, "termination_unconfirmed");
            assert!(result.artifact.is_none());
        }
    }
    #[test]
    fn foreign_owner_is_not_stopped_or_published() {
        let (result, steps) = fixture("owner");
        assert_eq!(result.execution.status, "termination_unconfirmed");
        assert!(!steps.iter().any(|step| step == "stop"));
        assert!(result.artifact.is_none());
    }
    #[test]
    fn ambiguous_create_is_stopped_without_recreation() {
        let (result, steps) = fixture("create");
        assert_eq!(result.execution.status, "failed");
        assert_eq!(steps.iter().filter(|s| s.as_str() == "create").count(), 1);
        assert!(!steps.iter().any(|s| s == "execute"));
        assert_eq!(result.evidence["termination_confirmed"], true);
    }
    #[test]
    fn over_limit_candidate_is_rejected_and_sandbox_stopped() {
        let (result, _) = fixture("oversize");
        assert_eq!(result.execution.status, "failed");
        assert!(result.artifact.is_none());
        assert_eq!(result.evidence["termination_confirmed"], true);
    }
}
