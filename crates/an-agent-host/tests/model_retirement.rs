//! Retired model entry points cannot reuse host grants to disclose material.
use an_agent_core::testkit::TempDir;
use an_agent_host::execution::Result;
use std::{fs, process::Command};

#[test]
fn retired_command_refuses_before_input_or_output_access() -> Result<()> {
    let temp = TempDir::new("retired-model");
    let root = temp.path().canonicalize()?;
    let output = root.join("result");
    let packet = root.join("private-context.json");
    fs::write(&packet, "private material must not reach stdout or stderr")?;
    for approved in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_an-agent-host"));
        command
            .arg("model")
            .arg(root.join("missing-profile.json"))
            .arg(&packet)
            .arg(&output);
        if approved {
            command.args(["--approve", &"0".repeat(64)]);
        }
        let result = command.output()?;
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        let diagnostic = String::from_utf8(result.stderr)?;
        assert!(
            diagnostic.contains("model service is retired"),
            "{diagnostic}"
        );
        assert!(!diagnostic.contains("private material"));
        assert!(!output.exists());
    }
    Ok(())
}
