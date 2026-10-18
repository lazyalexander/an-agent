//! Explicit Docker authority belongs to the trusted host, never the deployment.
use super::{Result, sandbox::Execution, store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub const BACKEND: &str = "linux-docker-stdio-v1";
const LIMIT: usize = 65536;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub docker: PathBuf,
    pub endpoint: String,
    pub image: String,
}

impl Profile {
    pub fn load(path: &Path) -> Result<Self> {
        let profile: Self = serde_json::from_slice(&store::read(path, 4096)?)?;
        profile.validate()?;
        Ok(profile)
    }

    fn validate(&self) -> Result<()> {
        if self.schema != "an-container-profile-v1"
            || !self.docker.is_absolute()
            || !self.endpoint.starts_with("unix:///")
            || self.endpoint.chars().any(char::is_control)
            || self.endpoint.contains([',', '\n'])
            || !self
                .image
                .strip_prefix("sha256:")
                .is_some_and(store::valid_digest)
        {
            return Err("container profile requires an absolute Docker CLI, local Unix endpoint, and immutable local image ID".into());
        }
        Ok(())
    }

    fn command(&self, run: &Path) -> Command {
        let mut cmd = Command::new(&self.docker);
        cmd.env_clear()
            .args(["--host", &self.endpoint, "--config"])
            .arg(run.join("docker-config"));
        cmd
    }

    fn create_args(&self, run: &Path, name: &str) -> Result<Vec<String>> {
        let file = run.join("program");
        let source = file.to_str().ok_or("non UTF-8 container mount")?;
        if source.contains(',') || source.chars().any(char::is_control) {
            return Err("ambiguous container mount path".into());
        }
        Ok(vec![
            "create".into(),
            "--pull=never".into(),
            "--name".into(),
            name.into(),
            "--label=an-agent.backend=linux-docker-stdio-v1".into(),
            "--interactive".into(),
            "--init=false".into(),
            "--no-healthcheck".into(),
            "--network=none".into(),
            "--read-only".into(),
            "--user=65534:65534".into(),
            "--cap-drop=ALL".into(),
            "--security-opt=no-new-privileges=true".into(),
            format!(
                "--security-opt=seccomp={}",
                run.join("seccomp.json").display()
            ),
            "--pids-limit=1".into(),
            "--memory=128m".into(),
            "--memory-swap=128m".into(),
            "--cpus=1".into(),
            "--ulimit=nofile=64:64".into(),
            "--ulimit=core=0:0".into(),
            "--ulimit=cpu=5:5".into(),
            "--ipc=none".into(),
            "--restart=no".into(),
            "--log-driver=none".into(),
            "--hostname=kit".into(),
            "--env=HOME=/nonexistent".into(),
            "--env=PATH=/nonexistent".into(),
            "--env=HOSTNAME=kit".into(),
            "--workdir=/".into(),
            "--entrypoint=/program".into(),
            "--mount".into(),
            format!("type=bind,source={source},target=/program,readonly"),
            self.image.clone(),
        ])
    }

    pub(super) fn plan(&self, run: &Path, name: &str) -> Result<Value> {
        self.validate()?;
        Ok(json!({"backend":BACKEND,"container":true,"profile":self,
            "command":self.create_args(run,name)?,"container_name":name,
            "environment":{"HOME":"/nonexistent","PATH":"/nonexistent","HOSTNAME":"kit"},
            "seccomp_sha256":store::digest(&serde_json::to_vec(&seccomp())?),
            "readiness":"checked only on execution; no implicit pull or native fallback"}))
    }
}

/// A narrow static-ELF ABI, not a general Linux application policy. In
/// particular no sockets, process creation, file creation/truncation or io_uring.
/// OCI startup reopens its existing FIFO writable. Existing writable descriptors
/// and device nodes remain usable; read-only mounts protect persistent files.
pub fn seccomp() -> Value {
    let readonly = |name, index| {
        json!({"names":[name],"action":"SCMP_ACT_ALLOW",
        "args":[{"index":index,"value":4195904_u64,"valueTwo":0,"op":"SCMP_CMP_MASKED_EQ"}]})
    };
    json!({"defaultAction":"SCMP_ACT_ERRNO","defaultErrnoRet":1,"syscalls":[
        {"names":["read","write","readv","writev","close","close_range","fstat","fstatfs","newfstatat","stat","lstat","statx","lseek","mmap","mprotect","munmap","mremap","brk","arch_prctl","set_tid_address","set_robust_list","rseq","rt_sigaction","rt_sigprocmask","rt_sigreturn","sigaltstack","prlimit64","getrlimit","getrandom","getpid","gettid","getuid","geteuid","getgid","getegid","clock_gettime","clock_nanosleep","nanosleep","futex","sched_getaffinity","sched_yield","exit","exit_group","execve","readlink","readlinkat","access","faccessat","faccessat2","getcwd","uname","restart_syscall"],"action":"SCMP_ACT_ALLOW"},
        readonly("open",1),readonly("openat",2),
        {"names":["getdents64","poll","ppoll"],"action":"SCMP_ACT_ALLOW"},
        {"names":["fcntl"],"action":"SCMP_ACT_ALLOW","args":[{"index":1,"value":1030,"op":"SCMP_CMP_EQ"}]},
        {"names":["fcntl"],"action":"SCMP_ACT_ALLOW","args":[{"index":1,"value":1,"op":"SCMP_CMP_EQ"}]},
        {"names":["fcntl"],"action":"SCMP_ACT_ALLOW","args":[{"index":1,"value":3,"op":"SCMP_CMP_EQ"}]}
    ]})
}

fn drain(mut stream: impl Read, exceeded: Arc<AtomicBool>) -> std::io::Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut bytes = [0; 4096];
    loop {
        let n = stream.read(&mut bytes)?;
        if n == 0 {
            return Ok(result);
        }
        let keep = n.min(LIMIT.saturating_sub(result.len()));
        result.extend_from_slice(&bytes[..keep]);
        if keep < n {
            exceeded.store(true, Ordering::Relaxed);
        }
    }
}

fn capture(
    cmd: &mut Command,
    input: &[u8],
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<Execution> {
    if cancelled.load(Ordering::Relaxed) {
        return Ok(Execution {
            status: "cancelled",
            code: None,
            stdout: vec![],
            stderr: vec![],
        });
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("missing Docker stdin")?;
    let stdout = child.stdout.take().ok_or("missing Docker stdout")?;
    let stderr = child.stderr.take().ok_or("missing Docker stderr")?;
    let exceeded = Arc::new(AtomicBool::new(false));
    let a = Arc::clone(&exceeded);
    let b = Arc::clone(&exceeded);
    let bytes = input.to_vec();
    let writer = thread::spawn(move || stdin.write_all(&bytes));
    let out = thread::spawn(move || drain(stdout, a));
    let err = thread::spawn(move || drain(stderr, b));
    #[allow(
        clippy::disallowed_methods,
        reason = "Docker process deadline must use OS time"
    )]
    let start = Instant::now();
    let mut reason = None;
    let status = loop {
        if cancelled.load(Ordering::Relaxed) {
            reason = Some("cancelled");
        } else if exceeded.load(Ordering::Relaxed) {
            reason = Some("output_limit");
        } else if start.elapsed() >= timeout {
            reason = Some("timed_out");
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if reason.is_some() {
            // Killing the client is not container cleanup; the caller must
            // remove the named container before returning any success receipt.
            let _ = child.kill();
            break child.wait()?;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let wrote = writer.join().map_err(|_| "Docker writer panicked")?;
    let stdout = out.join().map_err(|_| "Docker reader panicked")??;
    let stderr = err.join().map_err(|_| "Docker reader panicked")??;
    if exceeded.load(Ordering::Relaxed) {
        reason = Some("output_limit");
    }
    Ok(Execution {
        status: reason.unwrap_or(if status.success() && wrote.is_ok() {
            "succeeded"
        } else {
            "failed"
        }),
        code: status.code(),
        stdout,
        stderr,
    })
}

fn control(profile: &Profile, run: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let result = capture(
        profile.command(run).args(args),
        b"",
        Duration::from_secs(20),
        &AtomicBool::new(false),
    )?;
    if result.status != "succeeded" {
        return Err(format!(
            "Docker control operation {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(result.stdout)
}

fn ready(profile: &Profile, run: &Path, executable: &Path) -> Result<()> {
    let info: Value =
        serde_json::from_slice(&control(profile, run, &["info", "--format", "{{json .}}"])?)?;
    if info["OSType"] != "linux"
        || info["CpuCfsQuota"] != true
        || info["CpuCfsPeriod"] != true
        || info["MemoryLimit"] != true
        || info["PidsLimit"] != true
        || info["SwapLimit"] != true
        || !info["SecurityOptions"].as_array().is_some_and(|a| {
            a.iter()
                .any(|v| v.as_str().is_some_and(|s| s.starts_with("name=seccomp")))
        })
    {
        return Err("Docker requires Linux, CPU/memory/swap/PID limits and seccomp".into());
    }
    let image: Value = serde_json::from_slice(&control(
        profile,
        run,
        &["image", "inspect", "--format", "{{json .}}", &profile.image],
    )?)?;
    if image["Id"] != profile.image
        || image["Os"] != "linux"
        || image["Config"]["Env"].as_array().is_some_and(|a| {
            a.iter().any(|v| {
                !v.as_str().is_some_and(|s| {
                    ["PATH=", "HOME=", "HOSTNAME="]
                        .iter()
                        .any(|prefix| s.starts_with(prefix))
                })
            })
        })
        || image["Config"]["Volumes"]
            .as_object()
            .is_some_and(|a| !a.is_empty())
        || image["Config"]["OnBuild"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
    {
        return Err(
            "image must be Linux, locally pinned, and have no extra environment, volumes or build hooks"
                .into(),
        );
    }
    let elf = store::read(executable, 20 * 1024 * 1024)?;
    let machine = match image["Architecture"].as_str() {
        Some("amd64") => 62,
        Some("arm64") => 183,
        _ => return Err("unsupported container architecture".into()),
    };
    let engine_machine = match info["Architecture"].as_str() {
        Some("x86_64" | "amd64") => 62,
        Some("aarch64" | "arm64") => 183,
        _ => return Err("unsupported engine architecture".into()),
    };
    if engine_machine != machine {
        return Err("cross-architecture emulation is outside this profile".into());
    }
    if elf.len() < 64
        || &elf[..4] != b"\x7fELF"
        || elf[4] != 2
        || elf[5] != 1
        || u16::from_le_bytes([elf[18], elf[19]]) != machine
    {
        return Err("kit must be a matching 64-bit little-endian Linux ELF".into());
    }
    let offset = u64::from_le_bytes(elf[32..40].try_into()?) as usize;
    let size = u16::from_le_bytes([elf[54], elf[55]]) as usize;
    let count = u16::from_le_bytes([elf[56], elf[57]]) as usize;
    if size < 56 || count == 0 || count > 1024 {
        return Err("invalid ELF program headers".into());
    }
    for i in 0..count {
        let p = offset.checked_add(i * size).ok_or("ELF overflow")?;
        let header = elf
            .get(p..p.checked_add(size).ok_or("ELF overflow")?)
            .ok_or("truncated ELF")?;
        if u32::from_le_bytes(header[..4].try_into()?) == 3 {
            return Err("dynamic interpreter is unsupported; build a static kit".into());
        }
    }
    Ok(())
}

fn verify_created(profile: &Profile, run: &Path, name: &str) -> Result<()> {
    let value: Value = serde_json::from_slice(&control(
        profile,
        run,
        &["inspect", "--format", "{{json .}}", name],
    )?)?;
    store::write_new(
        &run.join("container-config.json"),
        &serde_json::to_vec_pretty(&value)?,
    )?;
    let host = &value["HostConfig"];
    let mounts = value["Mounts"]
        .as_array()
        .ok_or("missing container mounts")?;
    if host["ReadonlyRootfs"] != true
        || host["Privileged"] != false
        || host["NetworkMode"] != "none"
        || host["PidsLimit"] != 1
        || host["Memory"] != 134217728
        || host["MemorySwap"] != 134217728
        || host["NanoCpus"] != 1_000_000_000_u64
        || value["Config"]["User"] != "65534:65534"
        || mounts.len() != 1
        || mounts[0]["RW"] != false
        || mounts[0]["Destination"] != "/program"
        || mounts[0]["Source"] != json!(run.join("program"))
    {
        return Err("created container does not match host confinement requirements".into());
    }
    Ok(())
}

pub(super) fn execute(
    profile: &Profile,
    run: &Path,
    name: &str,
    input: &[u8],
    cancelled: &AtomicBool,
) -> Result<Execution> {
    profile.validate()?;
    fs::create_dir(run.join("docker-config"))?;
    store::write_new(&run.join("seccomp.json"), &serde_json::to_vec(&seccomp())?)?;
    ready(profile, run, &run.join("program"))?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("cancelled before container creation".into());
    }
    let args = profile.create_args(run, name)?;
    let created = control(
        profile,
        run,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    // Even a timed-out create may have reached the daemon. Always attempt
    // cleanup by this host-generated, unguessable name, never a broad filter.
    let result = match created {
        Ok(_) => (|| {
            verify_created(profile, run, name)?;
            capture(
                profile
                    .command(run)
                    .args(["start", "--attach", "--interactive", name]),
                input,
                Duration::from_secs(5),
                cancelled,
            )
        })(),
        Err(error) => Err(error),
    };
    let cleanup = control(profile, run, &["rm", "--force", name]);
    store::write_new(
        &run.join("container-cleanup.json"),
        &serde_json::to_vec_pretty(
            &json!({"name":name,"removed":cleanup.is_ok(),"error":cleanup.as_ref().err().map(ToString::to_string)}),
        )?,
    )?;
    if cleanup.is_err() {
        return Err("container removal not confirmed; inspect container-cleanup.json and the exact container name before retrying".into());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_and_profile_are_narrow() {
        let mut p = Profile {
            schema: "an-container-profile-v1".into(),
            docker: "/usr/bin/docker".into(),
            endpoint: "unix:///var/run/docker.sock".into(),
            image: format!("sha256:{}", "a".repeat(64)),
        };
        assert!(p.validate().is_ok());
        let args = p.create_args(Path::new("/safe/run"), "host-test").unwrap();
        assert!(args.contains(&"--network=none".into()));
        assert!(args.contains(&"--pids-limit=1".into()));
        assert!(args.contains(&"--pull=never".into()));
        assert!(p.create_args(Path::new("/unsafe,path"), "test").is_err());
        let policy = seccomp().to_string();
        assert!(!policy.contains("socket"));
        assert!(!policy.contains("clone"));
        assert!(!policy.contains("io_uring"));
        p.image = "latest".into();
        assert!(p.validate().is_err());
        p.image = format!("sha256:{}", "a".repeat(64));
        p.endpoint = "tcp://remote:2375".into();
        assert!(p.validate().is_err());
    }
}
