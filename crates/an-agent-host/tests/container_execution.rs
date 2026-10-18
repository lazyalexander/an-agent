//! Opt-in real Docker acceptance. No daemon is contacted unless explicitly configured.
use an_agent_host::execution::{self, Result, container::Profile};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn container_enforces_stdio_and_retains_evidence() -> Result<()> {
    let Some(config) = std::env::var_os("AN_AGENT_CONTAINER_PROFILE") else {
        eprintln!("skipped real Docker acceptance: AN_AGENT_CONTAINER_PROFILE is unset");
        return Ok(());
    };
    let profile = Profile::load(Path::new(&config))?;
    let root = std::env::temp_dir().canonicalize()?.join(format!(
        "container-test-{}",
        an_agent_core::det_seam::Entropy::os().uuid_v4()
    ));
    fs::create_dir(&root)?;
    let _cleanup = Directory(root.clone());
    let deployment = root.join("deployment");
    fs::create_dir(&deployment)?;
    if let Some(probe) = std::env::var_os("AN_AGENT_CONTAINER_PROBE") {
        fs::copy(probe, deployment.join("program"))?;
    } else {
        if !cfg!(target_os = "linux") {
            return Err("provide a Linux static probe on non-Linux hosts".into());
        }
        let source = root.join("probe.rs");
        fs::write(&source, include_bytes!("fixtures/container_probe.rs"))?;
        assert!(
            std::process::Command::new("rustc")
                .args(["--edition=2024", "-C", "target-feature=+crt-static"])
                .arg(source)
                .arg("-o")
                .arg(deployment.join("program"))
                .status()?
                .success()
        );
    }
    let hash = format!(
        "{:x}",
        Sha256::digest(fs::read(deployment.join("program"))?)
    );
    fs::write(
        deployment.join("spool.yaml"),
        format!(
            "v: 1\nkind: spool\nname: container-test\nversion: 1.0.0\nsummary: Controlled static CLI\nsort: beat\nconstructor: host\nhost: an-agent-stdio-v1\neffect:\n  file: {{ op: none }}\n  memory: {{ op: ignore }}\n  net: none\n  proc: spawn\n  model: {{ op: none }}\n  flow: both\ninverse: irreversible\nrequires: []\nconsumes: [kit.request]\nproduces: [kit.execute]\nconfig:\n  profile: stdio-only-v1\n  executable_sha256: \"{hash}\"\n"
        ),
    )?;
    let store = root.join("store");
    let registered = execution::register(&deployment, &store)?;
    let release = registered["registered"].as_str().ok_or("missing release")?;
    let input = root.join("input.txt");
    fs::write(&input, "hello")?;
    let idle = AtomicBool::new(false);
    let plan =
        execution::run_with_container(&store, release, &input, false, Some(&profile), &idle)?;
    assert_eq!(plan["backend"], "linux-docker-stdio-v1");
    assert!(!store.join("runs").exists());
    let secret = root.join("secret.txt");
    fs::write(&secret, "host-only data")?;
    for (data, expected) in [
        ("hello", "succeeded"),
        ("write", "succeeded"),
        ("network", "succeeded"),
        ("spawn", "succeeded"),
        ("env", "succeeded"),
        ("socket", "succeeded"),
        (secret.to_str().ok_or("path")?, "succeeded"),
        ("violate", "failed"),
        ("hang", "timed_out"),
        ("flood", "output_limit"),
        ("cancel", "cancelled"),
    ] {
        fs::write(&input, if data == "cancel" { "hang" } else { data })?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = if data == "cancel" {
            let flag = Arc::clone(&cancelled);
            Some(std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(2));
                flag.store(true, Ordering::Relaxed);
            }))
        } else {
            None
        };
        let result = execution::run_with_container(
            &store,
            release,
            &input,
            true,
            Some(&profile),
            &cancelled,
        )?;
        if let Some(handle) = cancellation {
            handle.join().map_err(|_| "cancel thread")?;
        }
        let folder = Path::new(result["receipt"].as_str().ok_or("receipt")?)
            .parent()
            .ok_or("folder")?;
        assert_eq!(
            result["status"],
            expected,
            "{data}: {}",
            fs::read_to_string(folder.join("stderr.txt"))?
        );
        let receipt: Value = serde_json::from_slice(&fs::read(folder.join("receipt.json"))?)?;
        assert_eq!(
            receipt["context"]["sha256"],
            format!("{:x}", Sha256::digest(fs::read(&input)?))
        );
        assert_eq!(receipt["image"], profile.image);
        let cleanup: Value =
            serde_json::from_slice(&fs::read(folder.join("container-cleanup.json"))?)?;
        assert_eq!(cleanup["removed"], true);
        let audit = fs::read_to_string(folder.join("audit.json"))?;
        assert_eq!(audit.contains("\"applied\""), expected == "succeeded");
        assert!(fs::metadata(folder.join("stdout.txt"))?.len() <= 65536);
        assert!(!folder.join("forbidden.txt").exists());
    }
    let mut missing = profile.clone();
    missing.image = format!("sha256:{}", "0".repeat(64));
    let refused =
        execution::run_with_container(&store, release, &input, true, Some(&missing), &idle)?;
    assert_eq!(refused["status"], "refused");
    #[cfg(unix)]
    {
        fs::write(&input, "hang")?;
        let output = root.join("signal-output.jsonl");
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_an-agent-host"))
            .arg("run")
            .arg(&store)
            .arg(release)
            .arg(&input)
            .arg("--container")
            .arg(&config)
            .arg("--execute")
            .stdout(fs::File::create(&output)?)
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let mut created = false;
        for _ in 0..200 {
            if let Ok(text) = fs::read_to_string(&output)
                && let Some(line) = text.lines().next()
                && let Ok(plan) = serde_json::from_str::<Value>(line)
                && let Some(path) = plan["artifact_boundary"].as_str()
                && Path::new(path).join("container-config.json").exists()
            {
                created = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !created {
            let _ = child.kill();
            let _ = child.wait();
            return Err("signal test container was not created".into());
        }
        assert!(
            std::process::Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status()?
                .success()
        );
        assert!(!child.wait()?.success());
        let text = fs::read_to_string(output)?;
        let result: Value =
            serde_json::from_str(text.lines().last().ok_or("missing signal result")?)?;
        assert_eq!(result["status"], "cancelled");
        let folder = Path::new(result["receipt"].as_str().ok_or("signal receipt")?)
            .parent()
            .ok_or("signal folder")?;
        let cleanup: Value =
            serde_json::from_slice(&fs::read(folder.join("container-cleanup.json"))?)?;
        assert_eq!(cleanup["removed"], true);
        assert!(!fs::read_to_string(folder.join("audit.json"))?.contains("\"applied\""));
    }
    Ok(())
}
