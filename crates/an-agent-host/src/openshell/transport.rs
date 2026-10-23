//! CLI transport, bounded local execution and control-request journals.
use super::Profile;
use crate::execution::{Result, sandbox, store};
use serde_json::json;
use std::{path::Path, process::Command, sync::atomic::AtomicBool, time::Duration};

pub(super) trait Transport {
    fn call(
        &mut self,
        step: &str,
        args: &[String],
        cancel: &AtomicBool,
        seconds: u64,
    ) -> Result<sandbox::Execution>;
}

pub(super) struct Cli<'a> {
    pub(super) profile: &'a Profile,
    pub(super) cwd: &'a Path,
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
