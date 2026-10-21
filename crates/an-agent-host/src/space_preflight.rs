//! Read-only observations for the documented SPACE Linux worker path.
//! This is a prerequisite filter, not an execution adapter or an authorization.
use serde_json::{Value, json};

#[derive(Debug)]
struct Facts<'a> {
    os: &'a str,
    arch: &'a str,
    kvm_access: bool,
    tun_access: bool,
}

fn assess(facts: Facts<'_>) -> Value {
    let mut blockers = Vec::new();
    if facts.os != "linux" {
        blockers.push("linux_worker_required");
    }
    if facts.arch != "x86_64" {
        blockers.push("documented_space_worker_requires_x86_64");
    }
    if !facts.kvm_access {
        blockers.push("kvm_character_device_access_unconfirmed");
    }
    if !facts.tun_access {
        blockers.push("tun_character_device_access_unconfirmed");
    }
    json!({
        "schema": "an-space-preflight-v1",
        "scope": "local_machine_only",
        "os": facts.os,
        "architecture": facts.arch,
        "candidate": blockers.is_empty(),
        "ready": false,
        "authorized": false,
        "blockers": blockers,
        "unchecked": [
            "kvm_api_and_vm_creation", "nested_virtualization_if_applicable",
            "loop_devices_and_btrfs", "space_deployment_and_template",
            "authentication_and_transport", "resource_and_egress_policy",
            "process_cleanup_and_artifact_acceptance"
        ]
    })
}

#[cfg(target_os = "linux")]
fn device_access(path: &str) -> bool {
    use std::os::unix::fs::FileTypeExt;
    // Opening these character devices does not create a VM or configure a TUN
    // interface. No ioctl is issued, and the descriptor is immediately dropped.
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .and_then(|file| file.metadata())
        .is_ok_and(|metadata| metadata.file_type().is_char_device())
}

/// Observe this machine without network access, subprocesses or configuration
/// changes. Exit success means the diagnostic ran, never that SPACE is ready.
pub fn inspect() -> Value {
    #[cfg(target_os = "linux")]
    let (kvm_access, tun_access) = (device_access("/dev/kvm"), device_access("/dev/net/tun"));
    #[cfg(not(target_os = "linux"))]
    let (kvm_access, tun_access) = (false, false);
    assess(Facts {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        kvm_access,
        tun_access,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_never_implies_readiness_or_authority() {
        let report = assess(Facts {
            os: "linux",
            arch: "x86_64",
            kvm_access: true,
            tun_access: true,
        });
        assert_eq!(report["candidate"], true);
        assert_eq!(report["ready"], false);
        assert_eq!(report["authorized"], false);
        assert!(!report["unchecked"].as_array().unwrap().is_empty());
    }

    #[test]
    fn each_missing_prerequisite_blocks_candidate() {
        for facts in [
            Facts {
                os: "macos",
                arch: "x86_64",
                kvm_access: true,
                tun_access: true,
            },
            Facts {
                os: "linux",
                arch: "aarch64",
                kvm_access: true,
                tun_access: true,
            },
            Facts {
                os: "linux",
                arch: "x86_64",
                kvm_access: false,
                tun_access: true,
            },
            Facts {
                os: "linux",
                arch: "x86_64",
                kvm_access: true,
                tun_access: false,
            },
        ] {
            let report = assess(facts);
            assert_eq!(report["candidate"], false);
            assert_eq!(report["blockers"].as_array().unwrap().len(), 1);
        }
    }

    #[test]
    fn observation_contains_no_machine_identity_or_credentials() {
        let report = inspect();
        assert_eq!(report["scope"], "local_machine_only");
        assert_eq!(report["ready"], false);
        assert_eq!(report.as_object().unwrap().len(), 9);
    }
}
