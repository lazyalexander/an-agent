//! Platform policies and execution for bounded Rust builds and file kits.
#[cfg(target_os = "macos")]
use crate::execution::store;
use crate::execution::{Result, sandbox};
use serde::Deserialize;
use serde_json::Value;
#[cfg(not(target_os = "linux"))]
use serde_json::json;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::time::Duration;
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

mod artifacts;
mod build;
pub(crate) use artifacts::{accept_output, prepare_output, regular};
pub use build::build;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Toolchain {
    rustc: PathBuf,
    sdk: PathBuf,
    linker: PathBuf,
}

/// Linux Landlock grants an inode, not a name that an untrusted kit may create.
/// Keep the original inode open and reject an untouched output marker. This
/// permits a deliberate empty output without confusing it with no output.
pub(crate) struct OutputSlot {
    initial: Vec<u8>,
    #[cfg(target_os = "linux")]
    file: fs::File,
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
    let published = crate::execution::publication::publish(
        &output.join("accepted/program"),
        &program,
        cancelled,
    );
    let evidence = match &published {
        Ok(result) => result.evidence.clone(),
        Err(error) => error
            .downcast_ref::<crate::execution::publication::PublicationError>()
            .map(|error| error.evidence.clone())
            .unwrap_or(Value::Null),
    };
    store::write_new(
        &output.join("publication.json"),
        &serde_json::to_vec_pretty(&evidence)?,
    )?;
    if let Some(stopped) = published?.stopped {
        return Ok(json!({"status":stopped.status,"stage":"publication"}));
    }
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
    #[cfg(target_os = "linux")]
    {
        Ok(sandbox::linux::file_plan(executable, work))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
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
    #[cfg(target_os = "linux")]
    {
        let mut command = sandbox::linux::file_command(executable, work)?;
        command.env_clear();
        sandbox::execute_command(command, work, b"", Duration::from_secs(5), cancelled, false)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (executable, work, cancelled);
        Err("file-kit-v1 is unsupported on this platform; no fallback".into())
    }
}
