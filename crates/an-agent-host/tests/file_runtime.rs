//! Real fixed file ABI execution, independent of platform-specific build support.
#![cfg(any(target_os = "macos", target_os = "linux"))]
use an_agent_core::testkit::TempDir;
use an_agent_host::{artifact_context, context_exchange::ContextPacket, execution};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[path = "file_runtime/agent_tools.rs"]
mod agent_tools;
#[path = "file_runtime/checkpoint.rs"]
mod checkpoint;
#[path = "file_runtime/concurrent.rs"]
mod concurrent;
#[path = "file_runtime/instance.rs"]
mod instance;
#[path = "file_runtime/lifecycle.rs"]
mod lifecycle;
#[path = "file_runtime/revisions.rs"]
mod revisions;
#[cfg(target_os = "macos")]
#[path = "file_runtime/workspaces.rs"]
mod workspaces;

struct Fixture {
    _directory: TempDir,
    _listener: std::net::TcpListener,
    root: PathBuf,
    release: String,
}

impl Fixture {
    fn new() -> execution::Result<Self> {
        Self::with_source(None)
    }

    fn with_source(source: Option<&str>) -> execution::Result<Self> {
        let directory = TempDir::new("file-runtime");
        let root = directory.path().canonicalize()?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        fs::write(root.join("secret"), b"private host data")?;
        let source = source.unwrap_or(r#"
use std::{fs, io::{self, Write}};
fn main() -> io::Result<()> {
    let input = fs::read_to_string("input.txt")?;
    assert!(fs::write("input.txt", "corrupt").is_err());
    assert!(fs::write("extra.txt", "bad").is_err());
    assert!(fs::write("../audit.json", "bad").is_err());
    assert!(fs::read("../plan.json").is_err());
    assert!(fs::read("HOST_SECRET").is_err());
    assert!(fs::read_dir("HOST_CATALOG").is_err());
    assert!(fs::create_dir("new-directory").is_err());
    assert!(fs::remove_file("input.txt").is_err());
    assert!(fs::rename("input.txt", "moved-input").is_err());
    assert!(std::os::unix::fs::symlink("input.txt", "other-link").is_err());
    assert!(fs::hard_link("input.txt", "other-hardlink").is_err());
    assert!(std::net::TcpStream::connect("NETWORK_TARGET").is_err());
    assert!(std::process::Command::new("/usr/bin/true").status().is_err());
    assert!(std::env::var("FILE_KIT_SENTINEL").is_err());
    match input.as_str() {
        "missing" => return Ok(()),
        "symlink" => return std::os::unix::fs::symlink("input.txt", "output.txt"),
        "hardlink" => return fs::hard_link("input.txt", "output.txt"),
        "oversize" => return fs::write("output.txt", vec![b'x'; 65537]),
        "boundary" => return fs::write("output.txt", vec![b'x'; 65536]),
        "empty" => return fs::write("output.txt", b""),
        "fail" => { fs::write("output.txt", "partial")?; std::process::exit(7); },
        "hang" => { fs::write("output.txt", "partial")?; loop { std::thread::sleep(std::time::Duration::from_millis(10)); } },
        _ => (),
    }
    let mut output = fs::OpenOptions::new().write(true).create(true).truncate(true).open("output.txt")?;
    let result = input.strip_prefix("capability:").map(str::to_owned).unwrap_or_else(|| input.to_uppercase());
    output.write_all(result.as_bytes())?;
    output.sync_all()?;
    println!("diagnostics are not the product");
    Ok(())
}
"#).replace("HOST_SECRET", root.join("secret").to_str().ok_or("invalid path")?)
    .replace("HOST_INSTANCES", root.join("store/instances").to_str().ok_or("invalid path")?)
    .replace("HOST_CATALOG", root.join("catalog").to_str().ok_or("invalid path")?)
    .replace("NETWORK_TARGET", &listener.local_addr()?.to_string());
        fs::write(root.join("main.rs"), source)?;
        let deployment = root.join("deployment");
        fs::create_dir(&deployment)?;
        // Reviewed test source is compiled explicitly outside the kit sandbox.
        assert!(
            Command::new("rustc")
                .arg("--edition=2024")
                .arg(root.join("main.rs"))
                .arg("-o")
                .arg(deployment.join("program"))
                .status()?
                .success()
        );
        fs::write(
            deployment.join("spool.yaml"),
            format!(
                "v: 1\nkind: spool\nname: file-runtime\nsummary: Fixed file ABI test\nversion: 0.1.0\nsort: beat\nconstructor: host\nhost: an-agent-file-v1\neffect:\n  file: {{ op: rw, path: /work, recursive: true }}\n  memory: {{ op: ignore }}\n  net: none\n  proc: spawn\n  model: {{ op: none }}\n  flow: both\ninverse: irreversible\nrequires: []\nconsumes: [kit.request]\nproduces: [kit.execute]\nconfig:\n  profile: file-kit-v1\n  executable_sha256: \"{:x}\"\n",
                Sha256::digest(fs::read(deployment.join("program"))?)
            ),
        )?;
        let registration = execution::register(&deployment, &root.join("store"))?;
        Ok(Self {
            _directory: directory,
            _listener: listener,
            root,
            release: registration["registered"]
                .as_str()
                .ok_or("missing release")?
                .into(),
        })
    }

    fn run(&self, input: &str) -> execution::Result<(Value, PathBuf)> {
        fs::write(self.root.join("input.txt"), input)?;
        let result = execution::run(
            &self.root.join("store"),
            &self.release,
            &self.root.join("input.txt"),
            true,
        )?;
        let path = Path::new(result["receipt"].as_str().ok_or("missing receipt")?)
            .parent()
            .ok_or("missing run")?
            .to_owned();
        assert_eq!(fs::read(self.root.join("input.txt"))?, input.as_bytes());
        assert_eq!(fs::read(path.join("work/input.txt"))?, input.as_bytes());
        assert!(!path.join("work/extra.txt").exists());
        assert_eq!(fs::read(self.root.join("secret"))?, b"private host data");
        Ok((result, path))
    }
}

#[test]
fn successful_file_output_is_accepted_before_context_and_cannot_be_overwritten()
-> execution::Result<()> {
    let fixture = Fixture::new()?;
    fs::write(fixture.root.join("input.txt"), "alpha\n")?;
    let preview = execution::run(
        &fixture.root.join("store"),
        &fixture.release,
        &fixture.root.join("input.txt"),
        false,
    )?;
    assert_eq!(preview["supported"], true);
    assert!(!fixture.root.join("store/runs").exists());
    let (result, run) = fixture.run("alpha\n")?;
    assert_eq!(
        result["status"],
        "succeeded",
        "{}",
        fs::read_to_string(run.join("stderr.txt"))?
    );
    let receipt: Value = serde_json::from_slice(&fs::read(run.join("receipt.json"))?)?;
    assert_eq!(receipt["process"]["reaped"], true);
    assert_eq!(
        receipt["artifact"],
        json!({"path":"artifact.txt", "sha256":format!("{:x}", Sha256::digest(b"ALPHA\n")), "bytes":6})
    );
    assert_eq!(fs::read(run.join("artifact.txt"))?, b"ALPHA\n");
    assert_eq!(
        fs::read(run.join("stdout.txt"))?,
        b"diagnostics are not the product\n"
    );
    assert!(fs::read_to_string(run.join("audit.json"))?.contains("applied"));
    fs::write(run.join("work/output.txt"), "changed working copy")?;
    let view = fixture.root.join("context");
    artifact_context::export(&run, "artifact", uuid::Uuid::nil(), 128, &view)?;
    let bytes = fs::read(view.join("context.json"))?;
    assert_eq!(
        ContextPacket::parse(&bytes, uuid::Uuid::nil())?.text,
        "ALPHA\n"
    );
    assert!(ContextPacket::parse(&bytes, uuid::Uuid::from_u128(1)).is_err());
    assert!(artifact_context::export(&run, "artifact", uuid::Uuid::nil(), 128, &view).is_err());
    fs::write(run.join("artifact.txt"), "tampered accepted copy")?;
    assert!(
        artifact_context::export(
            &run,
            "artifact",
            uuid::Uuid::nil(),
            128,
            &fixture.root.join("tampered")
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn missing_linked_oversized_or_failed_products_never_become_applied() -> execution::Result<()> {
    let fixture = Fixture::new()?;
    for input in ["missing", "symlink", "hardlink", "oversize", "fail", "hang"] {
        let (result, run) = fixture.run(input)?;
        assert_ne!(result["status"], "succeeded", "{input}");
        assert!(!run.join("artifact.txt").exists(), "{input}");
        assert!(!fs::read_to_string(run.join("audit.json"))?.contains("applied"));
        let view = fixture.root.join(format!("rejected-{input}"));
        assert!(artifact_context::export(&run, "artifact", uuid::Uuid::nil(), 128, &view).is_err());
        assert!(!view.exists());
        let receipt: Value = serde_json::from_slice(&fs::read(run.join("receipt.json"))?)?;
        assert_eq!(receipt["process"]["reaped"], true);
        if input == "hang" {
            assert_eq!(result["status"], "timed_out");
        }
    }
    Ok(())
}

#[test]
fn empty_and_exact_limit_outputs_are_distinct_from_missing_output() -> execution::Result<()> {
    let fixture = Fixture::new()?;
    for (input, expected) in [("empty", 0), ("boundary", 65536)] {
        let (result, run) = fixture.run(input)?;
        assert_eq!(
            result["status"],
            "succeeded",
            "{}",
            fs::read_to_string(run.join("stderr.txt"))?
        );
        assert_eq!(fs::read(run.join("artifact.txt"))?.len(), expected);
    }
    Ok(())
}

#[test]
#[allow(clippy::disallowed_methods, reason = "real cancellation deadline")]
fn cancelling_a_written_but_unfinished_output_never_accepts_it() -> execution::Result<()> {
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, Instant},
    };
    let fixture = Fixture::new()?;
    fs::write(fixture.root.join("input.txt"), "hang")?;
    let cancelled = AtomicBool::new(false);
    let result = std::thread::scope(|scope| -> execution::Result<Value> {
        let watcher = scope.spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(4);
            while Instant::now() < deadline {
                if let Ok(runs) = fs::read_dir(fixture.root.join("store/runs")) {
                    for run in runs.flatten() {
                        if fs::read(run.path().join("work/output.txt"))
                            .is_ok_and(|bytes| bytes == b"partial")
                        {
                            assert!(
                                an_agent_host::retention::reclaim(
                                    &fixture.root.join("store"),
                                    run.file_name().to_str().unwrap(),
                                    None,
                                )
                                .is_err()
                            );
                            assert_eq!(
                                fs::read(run.path().join("work/output.txt")).unwrap(),
                                b"partial"
                            );
                            cancelled.store(true, Ordering::Relaxed);
                            return true;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        });
        let result = execution::run_cancellable(
            &fixture.root.join("store"),
            &fixture.release,
            &fixture.root.join("input.txt"),
            true,
            &cancelled,
        )?;
        assert!(
            watcher.join().expect("watcher"),
            "kit did not reach partial output"
        );
        Ok(result)
    })?;
    assert_eq!(result["status"], "cancelled");
    let run = Path::new(result["receipt"].as_str().ok_or("missing receipt")?)
        .parent()
        .ok_or("missing run")?;
    let receipt: Value = serde_json::from_slice(&fs::read(run.join("receipt.json"))?)?;
    assert_eq!(receipt["process"]["reaped"], true);
    assert!(
        an_agent_host::retention::reclaim(
            &fixture.root.join("store"),
            run.file_name().unwrap().to_str().unwrap(),
            None,
        )
        .is_ok()
    );
    assert!(!run.join("artifact.txt").exists());
    assert!(!fs::read_to_string(run.join("audit.json"))?.contains("applied"));
    Ok(())
}
