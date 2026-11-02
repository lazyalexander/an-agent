//! macOS adapter for reviewed multi-process tools. Not an adversarial VM boundary.
use super::{Execution, Result};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

// Compatibility identities describe this backend, not a portable spool ABI.
pub(crate) const TARGET_OS: &str = "macos";
pub(crate) const SHELL_RUNTIME: &str = "macos-native-process-v1";
pub(crate) const PYTHON_RUNTIME: &str = "macos-native-python-v1";
pub(crate) const PLAN_BACKEND: &str = "macos-trusted-workspace-v1";
pub(crate) const SPOOL_PROFILE: &str = "macos-reviewed-native-spool-v1";

/// Qualification eligibility only. Actual policy installation can still refuse.
pub(crate) const fn supported() -> bool {
    cfg!(target_os = "macos")
}

#[cfg(target_os = "macos")]
pub(crate) fn policy(
    executable: &Path,
    work: &Path,
    roots: &[PathBuf],
    sources: &[PathBuf],
) -> Result<String> {
    use super::macos::quoted;
    let mut policy = format!(
        r#"(version 1)
(deny default)
(allow file-read-metadata)
(allow file-read* (literal "/") (subpath "/System/Library") (subpath "/usr/lib") (subpath "/usr/share") (subpath "/bin") (subpath "/usr/bin") (literal "/dev/urandom") (literal "/dev/random") (literal {exe}))
(allow file-read* file-write* (subpath {work}) (literal "/dev/null"))
(allow process-fork sysctl-read)
(allow process-info* (target same-sandbox))
(allow signal (target same-sandbox))
; JVM system-property initialization queries the macOS user directory service.
; Without this specific lookup, the qualified JDK crashes before Java starts.
(allow mach-lookup (global-name "com.apple.system.opendirectoryd.libinfo"))
(allow process-exec (subpath "/bin") (subpath "/usr/bin") (literal {exe}))
"#,
        exe = quoted(executable)?,
        work = quoted(work)?
    );
    for root in roots {
        policy.push_str(&format!(
            "(allow file-read* process-exec (subpath {}))\n",
            quoted(root)?
        ));
    }
    for source in sources {
        policy.push_str(&format!(
            "(deny file-read-data process-exec (literal {}))\n",
            quoted(source)?
        ));
    }
    // Keep the registered store and its SQLite sidecars outside even broad runtime read roots.
    policy.push_str(r#"(deny file-read-data file-write* (regex #"/[Rr][Ee][Vv][Ii][Ss][Ii][Oo][Nn][Ss]/[^/]+/[Rr][Ee][Gg][Ii][Ss][Tt][Ee][Rr][Ee][Dd]-[Cc][Uu][Ss][Tt][Oo][Dd][Yy][.][Ss][Qq][Ll][Ii][Tt][Ee]3"))"#);
    Ok(policy)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn policy(_: &Path, _: &Path, _: &[PathBuf], _: &[PathBuf]) -> Result<String> {
    Ok(String::new())
}

#[cfg(target_os = "macos")]
pub(crate) fn execute(plan: &Value, cancelled: &AtomicBool) -> Result<Execution> {
    use std::{os::unix::process::CommandExt, time::Duration};
    let mut command = std::process::Command::new("/usr/bin/sandbox-exec");
    command.args(["-p", plan["policy"].as_str().ok_or("missing policy")?]);
    command.arg(plan["executable"].as_str().ok_or("missing executable")?);
    for arg in plan["args"].as_array().ok_or("missing args")? {
        command.arg(arg.as_str().ok_or("invalid arg")?);
    }
    command.env_clear().process_group(0);
    for (key, value) in plan["environment"]
        .as_object()
        .ok_or("missing environment")?
    {
        command.env(key, value.as_str().ok_or("invalid environment")?);
    }
    // Bound each generated file independently; total scratch is checked before
    // acceptance/collection, not enforced as a filesystem quota in this profile.
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: 64 * 1024 * 1024,
                rlim_max: 64 * 1024 * 1024,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let cwd = Path::new(plan["root"].as_str().ok_or("missing root")?).join("work");
    super::execute_command(
        command,
        &cwd,
        b"",
        Duration::from_secs(plan["wall_seconds"].as_u64().ok_or("missing deadline")?),
        cancelled,
        true,
    )
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn execute(_: &Value, _: &AtomicBool) -> Result<Execution> {
    Err("trusted workspace execution is macOS-only; no fallback".into())
}

pub(crate) fn group_empty(pid: u32) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        let mut buffer = [0i32; 1];
        let count = unsafe {
            libc::proc_listpgrppids(
                pid as i32,
                buffer.as_mut_ptr().cast(),
                size_of_val(&buffer) as i32,
            )
        };
        if count < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(count == 0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        Ok(false)
    }
}

/// Observe settlement after kill/reap; repeated reads are not execution retries.
/// Never signal a numeric group after the leader has been reaped.
#[allow(
    clippy::disallowed_methods,
    reason = "bounded native group settlement observation"
)]
pub(crate) fn settlement(pid: Option<u32>, reaped: bool) -> Value {
    use serde_json::json;
    use std::time::{Duration, Instant};
    let start = Instant::now();
    let Some(pid) = pid.filter(|_| reaped) else {
        return json!({"empty":false,"samples":0,"error":"no reaped process group"});
    };
    let mut samples = 0;
    loop {
        samples += 1;
        match group_empty(pid) {
            Ok(empty) if empty || start.elapsed() >= Duration::from_secs(1) => {
                return json!({"group":pid,"empty":empty,"samples":samples,
                    "elapsed_ms":start.elapsed().as_millis()});
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                return json!({"group":pid,"empty":false,"samples":samples,
                "elapsed_ms":start.elapsed().as_millis(),"error":error.to_string()});
            }
        }
    }
}
