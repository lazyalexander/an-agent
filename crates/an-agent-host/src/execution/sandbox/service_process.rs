//! Host-owned finite client construction and read-only OS resource observations.
use super::{Execution, Result};
use serde_json::{Value, json};
use std::{path::Path, sync::atomic::AtomicBool};

/// Runtime binaries are not artifact payloads; hash them without buffering them.
pub(crate) fn executable_hash(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    crate::execution::store::no_links(path)?;
    let mut file = std::fs::File::open(path)?;
    let before = file.metadata()?;
    if !before.is_file() || before.len() > 512 * 1024 * 1024 {
        return Err("harness binary exceeds 512 MiB".into());
    }
    let mut hash = Sha256::new();
    let mut chunk = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > 512 * 1024 * 1024 {
            return Err("harness binary grew beyond its limit".into());
        }
        hash.update(&chunk[..count]);
    }
    let after = file.metadata()?;
    if total != before.len()
        || before.len() != after.len()
        || before.modified()? != after.modified()?
    {
        return Err("harness binary changed while hashing".into());
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub(crate) fn codex(
    task: &crate::daemon::harness::Task,
    dir: &Path,
    args: &[String],
    cancelled: &AtomicBool,
) -> Result<Execution> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new(&task.binary);
        command.args(args).env_clear().process_group(0);
        for key in [
            "HOME",
            "CODEX_HOME",
            "PATH",
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "SSL_CERT_FILE",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        super::execute_command(
            command,
            &dir.join("work"),
            b"",
            std::time::Duration::from_secs(180),
            cancelled,
            true,
        )
    }
    #[cfg(not(unix))]
    {
        let _ = (task, dir, args, cancelled);
        Err("finite Codex client requires macOS".into())
    }
}

pub(crate) fn app_server(
    task: &crate::daemon::harness::Task,
    dir: &Path,
    args: &[String],
    cancelled: &AtomicBool,
    exchange: impl FnOnce(
        std::process::ChildStdin,
        std::process::ChildStdout,
        std::sync::Arc<AtomicBool>,
    ) -> std::io::Result<Vec<u8>>
    + Send,
) -> Result<Execution> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new(&task.binary);
        command.args(args).env_clear().process_group(0);
        for key in [
            "HOME",
            "CODEX_HOME",
            "PATH",
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "SSL_CERT_FILE",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let home = dir.join("client-home");
        crate::execution::store::directory(&home)?;
        let original = std::env::var_os("CODEX_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join(".codex"))
            })
            .ok_or("native Codex home unavailable")?;
        // Reference native login only; no user config, skills or private sessions are copied.
        let auth = original.join("auth.json");
        if auth.is_file() {
            std::os::unix::fs::symlink(auth, home.join("auth.json"))?;
        }
        command.env("CODEX_HOME", &home);
        let result = super::execute_dialogue(
            command,
            &dir.join("work"),
            std::time::Duration::from_secs(180),
            cancelled,
            true,
            exchange,
        );
        if result.as_ref().is_ok_and(|r| r.reaped) {
            // This private runtime contains authentication references, never accepted products.
            std::fs::remove_dir_all(&home)?;
        }
        result
    }
    #[cfg(not(unix))]
    {
        let _ = (task, dir, args, cancelled, exchange);
        Err("app-server profile requires native process-group supervision".into())
    }
}

pub(crate) fn current_resources() -> Value {
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
        let count = unsafe {
            libc::proc_pidinfo(
                std::process::id() as i32,
                libc::PROC_PIDTASKINFO,
                0,
                info.as_mut_ptr().cast(),
                std::mem::size_of::<libc::proc_taskinfo>() as i32,
            )
        };
        if count as usize == std::mem::size_of::<libc::proc_taskinfo>() {
            let info = unsafe { info.assume_init() };
            return json!({"available":true,"scope":"daemon process only","resident_bytes":info.pti_resident_size,
                "cpu_user_ns":info.pti_total_user,"cpu_system_ns":info.pti_total_system});
        }
    }
    json!({"available":false})
}

pub(crate) fn identity(pid: u32) -> Value {
    #[cfg(target_os = "macos")]
    if let Some(info) = bsd(pid) {
        return json!({"pid":pid,"started_seconds":info.pbi_start_tvsec,"started_microseconds":info.pbi_start_tvusec});
    }
    let _ = pid;
    Value::Null
}

#[cfg(target_os = "macos")]
fn bsd(pid: u32) -> Option<libc::proc_bsdinfo> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let count = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size as i32,
        )
    };
    (count as usize == size).then(|| unsafe { info.assume_init() })
}

/// Live descendants only, sampled by parentage and birth identity. No signaling.
pub(crate) fn owned_resources(owner: &Value) -> Value {
    #[cfg(target_os = "macos")]
    if let Some(pid) = owner["pid"].as_u64().filter(|p| *p <= i32::MAX as u64) {
        let pid = pid as u32;
        if identity(pid) != *owner {
            return json!({"available":false,"reason":"original owner no longer live"});
        }
        let mut pids = vec![0i32; 4096];
        let count = unsafe {
            libc::proc_listallpids(
                pids.as_mut_ptr().cast(),
                (pids.len() * size_of::<i32>()) as i32,
            )
        };
        if count <= 0 || count as usize >= pids.len() {
            return json!({"available":false,"reason":"process census unavailable or full"});
        }
        let census: Vec<_> = pids
            .into_iter()
            .take(count as usize)
            .filter(|p| *p > 0)
            .filter_map(|p| bsd(p as u32))
            .filter(|p| p.pbi_uid == unsafe { libc::geteuid() })
            .collect();
        let mut owned = vec![pid];
        for _ in 0..32 {
            let mut found = false;
            for process in &census {
                if owned.contains(&process.pbi_ppid) && !owned.contains(&process.pbi_pid) {
                    owned.push(process.pbi_pid);
                    found = true;
                }
            }
            if !found {
                break;
            }
        }
        let mut rss = 0;
        let mut user = 0;
        let mut system = 0;
        let mut samples = 0;
        for pid in &owned {
            let before = identity(*pid);
            let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
            let size = size_of::<libc::proc_taskinfo>();
            let count = unsafe {
                libc::proc_pidinfo(
                    *pid as i32,
                    libc::PROC_PIDTASKINFO,
                    0,
                    info.as_mut_ptr().cast(),
                    size as i32,
                )
            };
            let expected = census.iter().find(|p| p.pbi_pid == *pid);
            if count as usize != size
                || before.is_null()
                || identity(*pid) != before
                || expected.is_none_or(|p| {
                    before["started_seconds"] != p.pbi_start_tvsec
                        || before["started_microseconds"] != p.pbi_start_tvusec
                })
            {
                continue;
            }
            let info = unsafe { info.assume_init() };
            rss += info.pti_resident_size;
            user += info.pti_total_user;
            system += info.pti_total_system;
            samples += 1;
        }
        if identity(pid) != *owner {
            return json!({"available":false,"reason":"owner changed during sample"});
        }
        return json!({"available":true,"resident_bytes":rss,"cpu_user_ns":user,"cpu_system_ns":system,
            "processes":samples,"partial":samples!=owned.len(),
            "scope":"live owner and observed descendants; summed RSS may include shared pages; excludes exited workers and external apps"});
    }
    let _ = owner;
    json!({"available":false,"reason":"original process birth identity unavailable"})
}
