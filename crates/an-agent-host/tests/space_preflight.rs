//! The diagnostic must not turn observed prerequisites into execution authority.
use std::process::Command;

#[test]
fn cli_reports_without_authorizing_or_claiming_readiness() {
    let output = Command::new(env!("CARGO_BIN_EXE_an-agent-host"))
        .arg("space-preflight")
        .env_clear()
        .output()
        .expect("run diagnostic");
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "an-space-preflight-v1");
    assert_eq!(report["ready"], false);
    assert_eq!(report["authorized"], false);
    assert_eq!(report["os"], std::env::consts::OS);
    assert_eq!(report["architecture"], std::env::consts::ARCH);
    if std::env::consts::OS != "linux" || std::env::consts::ARCH != "x86_64" {
        assert_eq!(report["candidate"], false);
    }
}
