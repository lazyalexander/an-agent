//! A bounded single-source Rust build, not a package manager or workflow engine.
//! The trusted host selects the toolchain; source cannot add build hooks or flags.
use crate::execution::{Result, sandbox, store};
use an_agent_core::{
    control::{
        AgentControl, DeliveryOutcome, EventRoute, Registration, ReplyIntent, SpoolBeat,
        SpoolDescriptor,
    },
    principal::card::{AgentCard, ModelSpec, Topology},
    workspace::WorkspaceRecord,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
#[cfg(target_os = "macos")]
use std::time::Duration;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

fn tool_digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err("tool must be a regular file".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Toolchain {
    rustc: PathBuf,
    sdk: PathBuf,
    linker: PathBuf,
}

struct BuildShuttle;
impl SpoolBeat for BuildShuttle {
    fn receive(&self, event: &WorkspaceRecord) -> std::result::Result<String, String> {
        Ok(json!({"raise":[{"event":"build.execute","body":event.body.clone().unwrap_or_default()}]}).to_string())
    }
}

const BUILD_DECLARATION: &str = "v: 1\nkind: spool\nname: host-rust-build\nsummary: Bounded Rust build\nversion: 0.1.0\nsort: beat\nconstructor: host\nhost: an-agent-rust-build-v1\neffect:\n  file: { op: rw, path: /job, recursive: true }\n  memory: { op: ignore }\n  net: none\n  proc: spawn\n  model: { op: none }\n  flow: both\ninverse: irreversible\nrequires: []\nconsumes: [build.request]\nproduces: [build.execute]\n";

/// Preview binds source bytes, operator-selected tools, paths and exact commands.
/// Approval authorizes one fresh job only. Existing jobs are never resumed/retried.
pub fn build(
    source: &Path,
    toolchain: &Path,
    output: &Path,
    approval: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Value> {
    let source_bytes = store::read(source, 65536)?;
    std::str::from_utf8(&source_bytes)?;
    let profile = store::read(toolchain, 16384)?;
    let tools: Toolchain = serde_json::from_slice(&profile)?;
    store::no_links(output)?;
    if output.exists() {
        return Err("build destination exists; never overwrite or retry implicitly".into());
    }
    let output = output
        .parent()
        .ok_or("missing output parent")?
        .canonicalize()?
        .join(output.file_name().ok_or("missing job name")?);
    let rustc = tools.rustc.canonicalize()?;
    let sdk = tools.sdk.canonicalize()?;
    let linker = tools.linker.canonicalize()?;
    let tools = Toolchain {
        rustc: rustc.clone(),
        sdk: sdk.clone(),
        linker: linker.clone(),
    };
    let toolchain_root = rustc
        .parent()
        .and_then(Path::parent)
        .ok_or("invalid compiler path")?;
    if rustc.file_name().and_then(|n| n.to_str()) != Some("rustc")
        || !sdk.is_dir()
        || sdk == Path::new("/")
        || !linker.is_file()
        || toolchain_root == Path::new("/")
        || linker
            .parent()
            .and_then(Path::parent)
            .is_none_or(|root| root == Path::new("/"))
    {
        return Err("expected a concrete Rust toolchain, SDK and linker".into());
    }
    let commands: Vec<Vec<String>> = [false, true]
        .into_iter()
        .map(|test| {
            let mut args = vec![
                rustc.display().to_string(),
                "--edition=2024".into(),
                "--crate-name=file_kit".into(),
                format!("--remap-path-prefix={}=/job", output.display()),
                format!(
                    "--remap-path-prefix={}=/rust-toolchain",
                    toolchain_root.display()
                ),
                "--sysroot".into(),
                toolchain_root.display().to_string(),
                "-C".into(),
                format!("linker={}", linker.display()),
                "-C".into(),
                format!(
                    "link-arg=-B{}",
                    linker.parent().unwrap_or(Path::new("/")).display()
                ),
                "-C".into(),
                "link-arg=-isysroot".into(),
                "-C".into(),
                format!("link-arg={}", sdk.display()),
                output.join("main.rs").display().to_string(),
                "-o".into(),
                output
                    .join(if test { "build/tests" } else { "build/program" })
                    .display()
                    .to_string(),
            ];
            if test {
                args.push("--test".into());
            }
            args
        })
        .collect();
    let mut plan = json!({"schema":"an-rust-build-plan-v1","source_sha256":store::digest(&source_bytes),"toolchain_profile_sha256":store::digest(&profile),"rustc_sha256":tool_digest(&rustc)?,"linker_sha256":tool_digest(&linker)?,"toolchain_root":toolchain_root,"sdk":sdk,"output":output,"commands":commands,"test_command":[output.join("build/tests"),PathBuf::from("--test-threads=1")],"environment":{"PATH":"/usr/bin:/bin","TMPDIR":output.join("build"),"SDKROOT":sdk},"backend":"macos-rust-build-v1","supported":cfg!(target_os="macos"),"limits":{"stage_seconds":60,"stream_bytes":65536,"file_bytes":20971520},"automatic_retries":0});
    plan["policies"] = policies(&output, &tools, toolchain_root)?;
    plan["compiler_cwd"] = json!(output.join("build"));
    plan["test_cwd"] = json!(output);
    plan["test_environment"] = json!({});
    plan["declaration_bindings"] = json!({"/job":output});
    let plan_bytes = serde_json::to_vec_pretty(&plan)?;
    let hash = store::digest(&plan_bytes);
    let Some(approval) = approval else {
        return Ok(json!({"plan":plan,"approval_sha256":hash}));
    };
    if approval != hash {
        return Err("build approval mismatch; preview again".into());
    }
    if !cfg!(target_os = "macos") {
        return Err("isolated Rust build is not supported on this platform; no fallback".into());
    }
    store::directory(&output)?;
    store::write_new(&output.join("plan.json"), &plan_bytes)?;
    store::write_new(&output.join("main.rs"), &source_bytes)?;
    store::directory(&output.join("build"))?;
    let control = AgentControl::open(output.join("agent"), &[])?;
    let thread = an_agent_core::det_seam::Entropy::os().uuid_v4();
    control.register(&Registration {
        events: vec![
            EventRoute {
                name: "build.request".into(),
                spools: vec!["host-rust-build".into()],
            },
            EventRoute {
                name: "build.execute".into(),
                spools: vec![],
            },
        ],
        config: vec![],
        env: vec![],
    })?;
    let spec = an_agent_spool::spool::parse(BUILD_DECLARATION)?;
    control.mount_spool_described(
        &spec.name,
        Arc::new(BuildShuttle),
        &spec.declaration(),
        &SpoolDescriptor {
            package: spec.name.clone(),
            yaml: BUILD_DECLARATION.into(),
        },
    )?;
    control.open_thread(&AgentCard {
        v: 1,
        id: thread,
        model: ModelSpec {
            base_url: "http://127.0.0.1:1".into(),
            model: "disabled".into(),
            extra_body: None,
        },
        prompt: "Explicit host build approval; source is untrusted data.".into(),
        tools: vec![],
        topology: Topology::Leaf,
        kernel: "0.1.0".into(),
        supersedes: None,
    })?;
    let event = control.push_event("build.request", &hash)?;
    let admitted = control.offer_on(thread, &spec.name, &event.id)?;
    let result = (|| -> Result<Value> {
        let DeliveryOutcome::Reply(ref reply) = admitted else {
            return Err("build admission did not reply".into());
        };
        if reply.intents
            != vec![ReplyIntent::Raise {
                event: "build.execute".into(),
                body: hash.clone(),
            }]
        {
            return Err("unexpected build intent".into());
        }
        control.push_event("build.execute", &hash)?;
        stages(&output, &tools, toolchain_root, &commands, cancelled)
    })();
    let (status, detail) = match result {
        Ok(v) => (v["status"].as_str().unwrap_or("failed").to_owned(), v),
        Err(e) => ("failed".into(), json!({"error":e.to_string()})),
    };
    let receipt = json!({"schema":"an-rust-build-receipt-v1","status":status,"plan_sha256":hash,"source_sha256":store::digest(&source_bytes),"result":detail,"retention":"manual","automatic_retries":0});
    store::write_new(
        &output.join("receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    if status == "succeeded"
        && let DeliveryOutcome::Reply(reply) = admitted
    {
        control.applied(
            thread,
            &reply,
            "Build and tests passed; accepted bytes retained",
        )?;
    }
    control.finish(thread, &format!("Build settled: {status}"))?;
    store::write_new(
        &output.join("audit.json"),
        &serde_json::to_vec_pretty(&control.events(thread)?)?,
    )?;
    Ok(json!({"status":status,"receipt":output.join("receipt.json")}))
}

#[cfg(not(target_os = "macos"))]
fn stages(_: &Path, _: &Toolchain, _: &Path, _: &[Vec<String>], _: &AtomicBool) -> Result<Value> {
    Err("unsupported isolated build".into())
}

#[cfg(target_os = "macos")]
fn policies(output: &Path, tools: &Toolchain, toolchain_root: &Path) -> Result<Value> {
    use sandbox::macos::quoted;
    let build = output.join("build");
    let compiler_policy = format!(
        "(version 1)(deny default)(allow process-fork)(allow process-exec (subpath {})(subpath {}))(allow file-read* (literal \"/\")(literal {})(subpath {})(subpath {})(subpath {})(subpath \"/System/Library\")(subpath \"/usr/lib\")(subpath \"/usr/share\")(literal \"/dev/urandom\")(literal \"/dev/random\")(literal \"/dev/null\"))(allow file-read* file-write* (subpath {}))(allow sysctl-read)",
        quoted(toolchain_root)?,
        quoted(tools.linker.parent().ok_or("linker parent")?)?,
        quoted(&output.join("main.rs"))?,
        quoted(toolchain_root)?,
        quoted(&tools.sdk)?,
        quoted(
            tools
                .linker
                .parent()
                .and_then(Path::parent)
                .ok_or("linker runtime root")?
        )?,
        quoted(&build)?
    );
    Ok(json!({"compiler":compiler_policy,"tests":sandbox::macos::profile(&build.join("tests"))?}))
}

#[cfg(not(target_os = "macos"))]
fn policies(_: &Path, _: &Toolchain, _: &Path) -> Result<Value> {
    Ok(Value::Null)
}

#[cfg(target_os = "macos")]
fn stages(
    output: &Path,
    tools: &Toolchain,
    toolchain_root: &Path,
    commands: &[Vec<String>],
    cancelled: &AtomicBool,
) -> Result<Value> {
    use std::os::unix::process::CommandExt;
    let build = output.join("build");
    let policies = policies(output, tools, toolchain_root)?;
    let compiler_policy = policies["compiler"]
        .as_str()
        .ok_or("missing compiler policy")?;
    for (i, args) in commands.iter().enumerate() {
        let mut command = std::process::Command::new("/usr/bin/sandbox-exec");
        command
            .args(["-p", compiler_policy])
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TMPDIR", &build)
            .env("SDKROOT", &tools.sdk)
            .process_group(0);
        // Trusted compiler/linker children inherit this boundary. No build scripts.
        unsafe {
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: 20 * 1024 * 1024,
                    rlim_max: 20 * 1024 * 1024,
                };
                if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let result = sandbox::execute_command(
            command,
            &build,
            b"",
            Duration::from_secs(60),
            cancelled,
            true,
        )?;
        persist_stage(output, &format!("compile-{i}"), &result)?;
        if result.status != "succeeded" {
            return Ok(json!({"status":result.status,"stage":format!("compile-{i}")}));
        }
    }
    let mut command = sandbox::macos::command(&build.join("tests"))?;
    command.arg("--test-threads=1").env_clear();
    let tested = sandbox::execute_command(
        command,
        output,
        b"",
        Duration::from_secs(60),
        cancelled,
        false,
    )?;
    persist_stage(output, "tests", &tested)?;
    if tested.status != "succeeded" {
        return Ok(json!({"status":tested.status,"stage":"tests"}));
    }
    let source = store::read(&output.join("main.rs"), 65536)?;
    let program = regular(&build.join("program"), 20 * 1024 * 1024)?;
    // Accepted files are outside every workload's writable directory.
    store::directory(&output.join("accepted"))?;
    store::write_new(&output.join("accepted/main.rs"), &source)?;
    store::write_new(&output.join("accepted/program"), &program)?;
    let manifest = json!({"schema":"an-rust-build-artifacts-v1","source_sha256":store::digest(&source),"program_sha256":store::digest(&program),"tests":"passed","compiler":"operator-selected; not a reproducible-build attestation"});
    store::write_new(
        &output.join("accepted/manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(json!({"status":"succeeded","artifacts":manifest}))
}

#[cfg(target_os = "macos")]
fn persist_stage(output: &Path, stage: &str, result: &sandbox::Execution) -> Result<()> {
    store::write_new(&output.join(format!("{stage}.stdout")), &result.stdout)?;
    store::write_new(&output.join(format!("{stage}.stderr")), &result.stderr)?;
    store::write_new(
        &output.join(format!("{stage}.json")),
        &serde_json::to_vec_pretty(
            &json!({"status":result.status,"exit_code":result.code,"pid":result.pid,"reaped":result.reaped}),
        )?,
    )
}

pub(crate) fn regular(path: &Path, limit: usize) -> Result<Vec<u8>> {
    store::no_links(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if fs::symlink_metadata(path)?.nlink() != 1 {
            return Err("linked artifact refused".into());
        }
    }
    store::read(path, limit)
}

/// Fixed file ABI: read work/input.txt, create work/output.txt; no other writes.
#[cfg(target_os = "macos")]
fn file_policy(executable: &Path, work: &Path) -> Result<String> {
    use sandbox::macos::{profile, quoted};
    Ok(format!(
        "{}\n(allow file-read* (literal {})(literal {}))\n(allow file-read* file-write* (literal {}))",
        profile(executable)?,
        quoted(work)?,
        quoted(&work.join("input.txt"))?,
        quoted(&work.join("output.txt"))?
    ))
}

pub(crate) fn file_plan(executable: &Path, work: &Path) -> Result<Value> {
    #[cfg(target_os = "macos")]
    {
        Ok(
            json!({"backend":"macos-file-kit-v1","supported":true,"command":["/usr/bin/sandbox-exec","-p",file_policy(executable, work)?,executable]}),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (executable, work);
        Ok(json!({"backend":"unsupported-file-kit-v1","supported":false,"command":null}))
    }
}

pub(crate) fn execute_file(
    executable: &Path,
    work: &Path,
    cancelled: &AtomicBool,
) -> Result<sandbox::Execution> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;
        let policy = file_policy(executable, work)?;
        let mut command = std::process::Command::new("/usr/bin/sandbox-exec");
        command.args(["-p", &policy]).arg(executable).env_clear();
        unsafe {
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: 65536,
                    rlim_max: 65536,
                };
                if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        sandbox::execute_command(command, work, b"", Duration::from_secs(5), cancelled, false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (executable, work, cancelled);
        Err("file-kit-v1 is unsupported on this platform; no fallback".into())
    }
}
