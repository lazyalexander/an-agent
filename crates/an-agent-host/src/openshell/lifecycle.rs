//! One lifecycle authority: submission, cancellation, stop proof and acceptance.
use super::{Profile, RemoteExecution, RuntimeIdentity, transport::Transport};
use crate::execution::{Result, sandbox, store};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).to_owned()).collect()
}
fn succeeded(result: &sandbox::Execution) -> bool {
    result.status == "succeeded" && result.code == Some(0) && result.reaped
}

pub(super) struct Session<'a, T> {
    pub(super) transport: T,
    pub(super) cancelled: &'a AtomicBool,
    pub(super) seconds: u64,
    pub(super) evidence: Vec<Value>,
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

pub(super) fn drive<T: Transport>(
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
        group_cleanup_required: false,
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
    let mut settled_environment = None;
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
                    settled_environment = state["id"]
                        .as_str()
                        .zip(
                            state["annotations"]["internal.openshell.ai/runtime-generation"]
                                .as_str(),
                        )
                        .map(|(id, generation)| RuntimeIdentity {
                            id: id.into(),
                            generation: generation.into(),
                        });
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
        environment: identity
            .zip(generation)
            .map(|(id, generation)| RuntimeIdentity { id, generation }),
        settled_environment,
        termination_confirmed: attempted && stopped,
        execution,
        artifact: if accept { candidate } else { None },
        evidence,
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
