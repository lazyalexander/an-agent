//! SPACE translates remote observations into the existing host outcome contract.
use super::*;
use crate::execution::store;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

/// Trusted operator configuration, never supplied by a spool.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    python: PathBuf,
    python_sha256: String,
    client: PathBuf,
    client_sha256: String,
    token_file: PathBuf,
    api_url: String,
    data_plane_url: String,
    template: String,
    control_seconds: u64,
}
impl Profile {
    pub(in crate::execution) fn load(path: &Path) -> Result<Self> {
        let p: Self = serde_json::from_slice(&store::read(path, 16384)?)?;
        p.verify()?;
        Ok(p)
    }
    fn verify(&self) -> Result<()> {
        for (path, hash) in [
            (&self.python, &self.python_sha256),
            (&self.client, &self.client_sha256),
        ] {
            if !path.is_absolute() || !store::valid_digest(hash) {
                return Err("SPACE requires absolute, digest-pinned operator tools".into());
            }
            store::no_links(path)?;
            if store::digest(&store::read(path, 128 * 1024 * 1024)?) != *hash {
                return Err("SPACE operator tool changed".into());
            }
        }
        store::no_links(&self.token_file)?;
        if !self.token_file.is_absolute()
            || !(30..=180).contains(&self.control_seconds)
            || !self
                .template
                .split_once("@sha256:")
                .is_some_and(|(name, hash)| {
                    !name.is_empty()
                        && name
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                        && store::valid_digest(hash)
                })
        {
            return Err(
                "invalid SPACE profile: pin template digest and bound control deadline".into(),
            );
        }
        if store::read(&self.token_file, 16384)?.is_empty() {
            return Err("SPACE token file is empty".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(&self.token_file)?.permissions().mode() & 0o077 != 0 {
                return Err("SPACE token file must be private to the operator".into());
            }
        }
        for url in [&self.api_url, &self.data_plane_url] {
            if !(url.starts_with("https://") || url.starts_with("http://127.0.0.1:"))
                || url.contains('@')
                || url.contains('?')
                || url.contains('#')
            {
                return Err("SPACE requires HTTPS or explicit loopback origins".into());
            }
        }
        Ok(())
    }
}
pub(in crate::execution) struct Space<'a>(pub &'a Profile);
impl ExecutionBackend for Space<'_> {
    fn validate(&self, file: bool) -> Result<()> {
        if !file {
            return Err("SPACE requires file-kit-v1".into());
        }
        self.0.verify()
    }
    fn plan(&self, _: &Path, _: &Path, _: bool, plan: &mut Value) -> Result<()> {
        merge_plan(
            plan,
            json!({"backend":"space-fc-file-v1","template":self.0.template,
            "api_url":self.0.api_url,"data_plane_url":self.0.data_plane_url,
            "control_seconds":self.0.control_seconds,"automatic_retries":0,"native_fallback":false,
            "client_sha256":self.0.client_sha256,"python_sha256":self.0.python_sha256,
            "transport_sha256":store::digest(include_bytes!("space_client.py")),
            "collector_sha256":store::digest(include_bytes!("space_guest.py")),
            "remote_termination":"owned sandbox deletion operation succeeded",
            "files":{"input":"input.txt","candidate":"output.txt","host_mounts":false,"output_limit":65536,"program_limit":262144,"guest_scratch":"/tmp"}}),
        )?;
        plan["context"]["kind"] = json!("explicit-file-snapshot");
        Ok(())
    }
    fn execute(&self, request: Request<'_>, _: bool) -> Result<Outcome> {
        self.0.verify()?;
        let program = store::read(request.executable, 262144)?;
        if !(program.starts_with(b"\x7fELF") || program.starts_with(b"#!")) {
            return Err("SPACE requires a Linux executable or a guest-compatible script".into());
        }
        for (name, bytes) in [
            ("space-profile.json", serde_json::to_vec(self.0)?),
            ("space-request.json", serde_json::to_vec(request.operation)?),
            (
                "space-client.py",
                include_bytes!("space_client.py").to_vec(),
            ),
            ("space-guest.py", include_bytes!("space_guest.py").to_vec()),
        ] {
            store::write_new(&request.directory.join(name), &bytes)?;
        }
        let call = |step: &str, cancel: &AtomicBool| -> Result<sandbox::Execution> {
            self.0.verify()?;
            sandbox::space_transport::call(
                &self.0.python,
                request.directory,
                step,
                self.0.control_seconds,
                cancel,
            )
        };
        let mut execution = sandbox::Execution {
            status: "failed",
            code: None,
            stdout: vec![],
            stderr: vec![],
            pid: None,
            reaped: false,
            group_cleanup_required: false,
        };
        let mut evidence = json!({"steps":[],"automatic_retries":0});
        let mut identity = None;
        let mut attempted = false;
        let work = (|| -> Result<()> {
            for step in ["create", "ready", "run"] {
                if request.cancelled.load(Ordering::Relaxed) {
                    return Err("cancelled".into());
                }
                attempted = true;
                execution = call(step, request.cancelled)?;
                evidence["steps"]
                    .as_array_mut()
                    .ok_or("missing transport evidence list")?
                    .push(
                        json!({"step":step,"status":execution.status,"exit_code":execution.code}),
                    );
                if execution.status != "succeeded" || execution.code != Some(0) || !execution.reaped
                {
                    return Err("SPACE transport did not complete".into());
                }
                if step == "ready" {
                    identity = read_identity(request.directory)?;
                }
            }
            Ok(())
        })();
        // Cleanup is independent of cancellation, local command failure and acceptance.
        let never = AtomicBool::new(false);
        let cleanup = if attempted {
            Some(call("delete", &never))
        } else {
            None
        };
        let state = store::read(
            &request.directory.join("space-control/state.json"),
            1024 * 1024,
        )
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let terminated = !attempted
            || (cleanup.as_ref().is_some_and(|r| {
                r.as_ref()
                    .is_ok_and(|r| r.status == "succeeded" && r.code == Some(0) && r.reaped)
            }) && state.as_ref().is_some_and(|s| s["deleted"] == true));
        let settled = if terminated && attempted {
            read_identity(request.directory)?
        } else {
            None
        };
        evidence["termination_confirmed"] = json!(terminated);
        evidence["submission_attempted"] = json!(attempted);
        evidence["delete_operation_id"] = state
            .as_ref()
            .map_or(Value::Null, |s| s["delete_operation_id"].clone());
        let candidate = if work.is_ok() && terminated && !request.cancelled.load(Ordering::Relaxed)
        {
            Candidate::Bytes(store::read(
                &request.directory.join("space-candidate"),
                65536,
            )?)
        } else {
            Candidate::None
        };
        if !terminated {
            execution.status = "termination_unconfirmed";
        } else if request.cancelled.load(Ordering::Relaxed) {
            execution.status = "cancelled";
        } else if let Err(e) = work {
            execution.status = "failed";
            execution.stderr.extend_from_slice(e.to_string().as_bytes());
        }
        Ok(Outcome {
            operation: request.operation.clone(),
            execution,
            environment: identity,
            settled_environment: settled,
            termination_confirmed: terminated,
            requires_environment: true,
            candidate,
            evidence,
        })
    }
    fn cancel_before_acceptance(&self) -> bool {
        true
    }
    fn decorate_receipt(&self, receipt: &mut Value, mut evidence: Value, accepted: bool) {
        evidence["artifact_accepted"] = json!(accepted);
        receipt["space"] = evidence;
        receipt["process"]["scope"] =
            json!("local SPACE transport; remote deletion recorded separately");
        receipt["container"] = Value::Null;
    }
}
fn read_identity(directory: &Path) -> Result<Option<Environment>> {
    let state: Value = serde_json::from_slice(&store::read(
        &directory.join("space-control/state.json"),
        1024 * 1024,
    )?)?;
    let i = &state["identity"];
    if i.is_null() {
        return Ok(None);
    }
    Ok(Some(Environment {
        id: i["id"].as_str().ok_or("missing SPACE identity")?.into(),
        generation: format!(
            "{}:{}",
            i["created_at"].as_str().ok_or("missing SPACE generation")?,
            i["template_digest"]
                .as_str()
                .ok_or("missing SPACE template")?
        ),
    }))
}

#[cfg(all(test, unix))]
#[path = "space_tests.rs"]
mod tests;
