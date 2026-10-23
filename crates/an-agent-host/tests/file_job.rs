//! Acceptance of the macOS single-source build and file execution boundaries.
#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used)]
use an_agent_core::testkit::TempDir;
use an_agent_host::{artifact_context, context_exchange::ContextPacket, execution, file_job};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};

fn tool(command: &str, args: &[&str]) -> String {
    let result = Command::new(command).args(args).output().unwrap();
    assert!(result.status.success());
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}
fn profile(root: &Path) -> std::path::PathBuf {
    let compiler = Path::new(&tool("rustc", &["--print", "sysroot"])).join("bin/rustc");
    let path = root.join("toolchain.json");
    fs::write(&path,json!({"rustc":compiler,"sdk":tool("xcrun",&["--show-sdk-path"]),"linker":tool("xcrun",&["--find","clang"])}).to_string()).unwrap();
    path
}
fn build(root: &Path, source: &str) -> Value {
    fs::write(root.join("source.rs"), source).unwrap();
    let tools = profile(root);
    let job = root.join("job");
    let preview = file_job::build(
        &root.join("source.rs"),
        &tools,
        &job,
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!job.exists());
    let result = file_job::build(
        &root.join("source.rs"),
        &tools,
        &job,
        preview["approval_sha256"].as_str(),
        &AtomicBool::new(false),
    )
    .unwrap();
    if result["status"] != "succeeded" {
        for item in fs::read_dir(&job).unwrap() {
            let item = item.unwrap();
            if item.path().extension().is_some_and(|s| s == "stderr") {
                eprintln!("{}", fs::read_to_string(item.path()).unwrap());
            }
        }
    }
    result
}

#[test]
fn isolated_build_and_registered_file_kit_preserve_boundaries() {
    let temp = TempDir::new("file-job");
    let root = temp.path().canonicalize().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let source = r#"
fn main() -> std::io::Result<()> {
    let input=std::fs::read_to_string("input.txt")?;
    match input.as_str() {
        "missing" => return Ok(()),
        "oversize" => return std::fs::write("output.txt",vec![0;65537]),
        "symlink" => return std::os::unix::fs::symlink("input.txt","output.txt"),
        "hardlink" => return std::fs::hard_link("input.txt","output.txt"),
        _ => ()
    }
    assert!(std::fs::write("input.txt","corrupt").is_err());
    assert!(std::fs::write("../audit.json","corrupt").is_err());
    assert!(std::fs::read("../plan.json").is_err());
    assert!(std::net::TcpStream::connect("NETWORK_TARGET").is_err());
    assert!(std::process::Command::new("/bin/echo").status().is_err());
    std::fs::write("output.txt",input.to_uppercase())
}
#[test] fn isolated_test() { assert!(std::fs::write("leak","bad").is_err()); }
"#;
    let source = source.replace(
        "NETWORK_TARGET",
        &listener.local_addr().unwrap().to_string(),
    );
    assert_eq!(build(&root, &source)["status"], "succeeded");
    let publication: Value =
        serde_json::from_slice(&fs::read(root.join("job/publication.json")).unwrap()).unwrap();
    assert_eq!(publication["published"], true);
    assert_eq!(publication["helper"]["reaped"], true);
    assert!(!root.join("job/leak").exists());
    let program = fs::read(root.join("job/accepted/program")).unwrap();
    assert!(
        !program
            .windows(root.as_os_str().len())
            .any(|bytes| bytes == root.to_str().unwrap().as_bytes())
    );
    let deployment = root.join("deployment");
    fs::create_dir(&deployment).unwrap();
    fs::write(deployment.join("program"), &program).unwrap();
    fs::write(deployment.join("spool.yaml"),format!("v: 1\nkind: spool\nname: file-probe\nsummary: File boundary probe\nversion: 0.1.0\nsort: beat\nconstructor: host\nhost: an-agent-file-v1\neffect:\n  file: {{ op: rw, path: /work, recursive: true }}\n  memory: {{ op: ignore }}\n  net: none\n  proc: spawn\n  model: {{ op: none }}\n  flow: both\ninverse: irreversible\nrequires: []\nconsumes: [kit.request]\nproduces: [kit.execute]\nconfig:\n  profile: file-kit-v1\n  executable_sha256: \"{:x}\"\n",Sha256::digest(&program))).unwrap();
    let store = root.join("store");
    let registered = execution::register(&deployment, &store).unwrap();
    let input = root.join("input.txt");
    fs::write(&input, "alpha\n").unwrap();
    let run = execution::run(
        &store,
        registered["registered"].as_str().unwrap(),
        &input,
        true,
    )
    .unwrap();
    let receipt_path = Path::new(run["receipt"].as_str().unwrap());
    assert_eq!(
        run["status"],
        "succeeded",
        "{}",
        fs::read_to_string(receipt_path.parent().unwrap().join("stderr.txt")).unwrap()
    );
    assert_eq!(
        fs::read(receipt_path.parent().unwrap().join("artifact.txt")).unwrap(),
        b"ALPHA\n"
    );
    assert_eq!(fs::read(&input).unwrap(), b"alpha\n");
    let view = root.join("context-view");
    artifact_context::export(
        receipt_path.parent().unwrap(),
        "artifact",
        uuid::Uuid::nil(),
        128,
        &view,
    )
    .unwrap();
    let packet = ContextPacket::parse(
        &fs::read(view.join("context.json")).unwrap(),
        uuid::Uuid::nil(),
    )
    .unwrap();
    assert_eq!(packet.text, "ALPHA\n");
    assert!(packet.source.ends_with(":artifact"));
    assert!(
        fs::read_to_string(receipt_path.parent().unwrap().join("audit.json"))
            .unwrap()
            .contains("applied")
    );
    for rejected in ["missing", "oversize", "symlink", "hardlink"] {
        fs::write(&input, rejected).unwrap();
        let failed = execution::run(
            &store,
            registered["registered"].as_str().unwrap(),
            &input,
            true,
        )
        .unwrap();
        assert_ne!(failed["status"], "succeeded", "{rejected}");
        let folder = Path::new(failed["receipt"].as_str().unwrap())
            .parent()
            .unwrap();
        assert!(!folder.join("artifact.txt").exists());
        let view = root.join(format!("rejected-{rejected}"));
        assert!(
            artifact_context::export(folder, "artifact", uuid::Uuid::nil(), 128, &view).is_err()
        );
        assert!(!view.exists());
        assert!(
            !fs::read_to_string(folder.join("audit.json"))
                .unwrap()
                .contains("applied")
        );
    }
}

#[test]
#[allow(clippy::disallowed_methods, reason = "real cancellation deadline")]
fn changed_approval_and_cancelled_build_never_publish() {
    use std::{
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };
    let temp = TempDir::new("cancel-file-job");
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("source.rs");
    let tools = profile(&root);
    let job = root.join("job");
    fs::write(&source, "fn main() {}").unwrap();
    let preview = file_job::build(&source, &tools, &job, None, &AtomicBool::new(false)).unwrap();
    fs::write(&source,"fn main() {} #[test] fn blocked(){std::thread::sleep(std::time::Duration::from_secs(120));}").unwrap();
    assert!(
        file_job::build(
            &source,
            &tools,
            &job,
            preview["approval_sha256"].as_str(),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(!job.exists());
    let preview = file_job::build(&source, &tools, &job, None, &AtomicBool::new(false)).unwrap();
    let cancelled = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(40);
            while !job.join("compile-1.json").exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            std::thread::sleep(Duration::from_millis(200));
            cancelled.store(true, Ordering::Relaxed);
        });
        file_job::build(
            &source,
            &tools,
            &job,
            preview["approval_sha256"].as_str(),
            &cancelled,
        )
        .unwrap()
    });
    assert_eq!(result["status"], "cancelled");
    let stage: Value = serde_json::from_slice(&fs::read(job.join("tests.json")).unwrap()).unwrap();
    assert_eq!(stage["reaped"], true);
    assert!(!job.join("accepted").exists());
    assert!(
        !fs::read_to_string(job.join("audit.json"))
            .unwrap()
            .contains("applied")
    );
    assert!(file_job::build(&source, &tools, &job, None, &cancelled).is_err());
}

#[test]
fn failed_tests_and_compile_time_file_access_never_publish_accepted_artifacts() {
    for (source, stage) in [
        ("fn main() {} #[test] fn broken(){assert!(false);}", "tests"),
        (
            "const SECRET: &str = include_str!(\"../secret\"); fn main(){println!(\"{SECRET}\");}",
            "compile-0",
        ),
    ] {
        let temp = TempDir::new("failed-file-job");
        let root = temp.path().canonicalize().unwrap();
        fs::write(root.join("secret"), "not for compiler").unwrap();
        assert_ne!(build(&root, source)["status"], "succeeded");
        let receipt: Value =
            serde_json::from_slice(&fs::read(root.join("job/receipt.json")).unwrap()).unwrap();
        assert_eq!(receipt["result"]["stage"], stage);
        assert!(!root.join("job/accepted").exists());
        assert!(
            !fs::read_to_string(root.join("job/audit.json"))
                .unwrap()
                .contains("applied")
        );
    }
}
