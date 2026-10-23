//! Native Linux confinement. Docker, user namespaces and external launchers are
//! not part of the runtime contract. All policy construction happens before fork.
use super::super::Result;
use landlock::{
    ABI, Access, AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr,
};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule,
};
use std::{
    collections::BTreeMap,
    io,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    path::Path,
    process::Command,
};

pub const BACKEND: &str = "linux-landlock-seccomp-stdio-v1";
const RUNTIME_DIRS: &[&str] = &["/usr/lib", "/lib", "/lib64"];
const RUNTIME_FILES: &[&str] = &[
    "/etc/ld.so.cache",
    "/dev/null",
    "/dev/random",
    "/dev/urandom",
];

pub fn plan(executable: &Path) -> serde_json::Value {
    serde_json::json!({"backend":BACKEND,"command":[executable],"supported":true,
        "policy":{"landlock_min_abi":3,"compatibility":"hard-requirement","runtime_read_directories":RUNTIME_DIRS,"runtime_read_files":RUNTIME_FILES,"network":false,"child_processes":false,"inherited_fds":"stdio only after exec","no_new_privs":true},
        "resource_limits":{"address_space_bytes":268435456,"cpu_seconds":5,"open_files":64,"regular_file_bytes":0,"core_bytes":0},
        "readiness":"enforced at spawn; no fallback when kernel features are unavailable"})
}

pub fn file_plan(executable: &Path, work: &Path) -> serde_json::Value {
    let mut result = plan(executable);
    result["backend"] = serde_json::json!("linux-landlock-seccomp-file-v1");
    result["resource_limits"]["regular_file_bytes"] = serde_json::json!(65536);
    result["policy"]["files"] = serde_json::json!({
        "directory": work, "input": work.join("input.txt"),
        "output": work.join("output.txt"), "other_writes": false,
        "output_initialization": "host-created inode with unproduced marker; truncate or replace contents",
        "create_remove_rename_link": false
    });
    result
}

fn filesystem(executable: &Path, work: Option<&Path>) -> Result<OwnedFd> {
    let mut rules = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V3))?
        .create()?
        .add_rule(PathBeneath::new(
            PathFd::new(executable)?,
            AccessFs::ReadFile | AccessFs::Execute,
        ))?;
    for path in RUNTIME_DIRS {
        if Path::new(path).exists() {
            rules = rules.add_rule(PathBeneath::new(
                PathFd::new(path)?,
                AccessFs::from_read(ABI::V3),
            ))?;
        }
    }
    for path in RUNTIME_FILES {
        if Path::new(path).exists() {
            rules = rules.add_rule(PathBeneath::new(PathFd::new(path)?, AccessFs::ReadFile))?;
        }
    }
    if let Some(work) = work {
        rules = rules
            .add_rule(PathBeneath::new(PathFd::new(work)?, AccessFs::ReadDir))?
            .add_rule(PathBeneath::new(
                PathFd::new(work.join("input.txt"))?,
                AccessFs::ReadFile,
            ))?
            .add_rule(PathBeneath::new(
                PathFd::new(work.join("output.txt"))?,
                AccessFs::ReadFile | AccessFs::WriteFile | AccessFs::Truncate,
            ))?;
    }
    // The public conversion exposes an owned FD without applying restrictions
    // to the host. HardRequirement above refuses partial or absent enforcement.
    Option::<OwnedFd>::from(rules).ok_or_else(|| "Landlock ruleset unavailable".into())
}

fn filter(file: bool) -> Result<BpfProgram> {
    use libc::*;
    let calls = [
        SYS_read,
        // x86_64 glibc reads ELF headers by offset while loading shared objects.
        // Landlock still controls which files can be opened for this operation.
        SYS_pread64,
        SYS_write,
        SYS_readv,
        SYS_writev,
        SYS_close,
        SYS_fstat,
        SYS_newfstatat,
        SYS_statx,
        SYS_lseek,
        SYS_mmap,
        SYS_mprotect,
        SYS_munmap,
        SYS_mremap,
        SYS_brk,
        SYS_set_tid_address,
        SYS_set_robust_list,
        SYS_rseq,
        SYS_rt_sigaction,
        SYS_rt_sigprocmask,
        SYS_rt_sigreturn,
        SYS_sigaltstack,
        SYS_getrandom,
        SYS_getpid,
        SYS_gettid,
        SYS_getppid,
        SYS_getuid,
        SYS_geteuid,
        SYS_getgid,
        SYS_getegid,
        SYS_clock_gettime,
        SYS_clock_nanosleep,
        SYS_nanosleep,
        SYS_futex,
        SYS_sched_getaffinity,
        SYS_sched_yield,
        SYS_exit,
        SYS_exit_group,
        SYS_execve,
        SYS_readlinkat,
        SYS_faccessat,
        SYS_getcwd,
        SYS_uname,
        SYS_restart_syscall,
        SYS_openat,
        SYS_getdents64,
        SYS_ppoll,
        SYS_fcntl,
    ];
    let mut rules: BTreeMap<_, _> = calls.into_iter().map(|call| (call, vec![])).collect();
    if file {
        // Landlock limits these descriptor operations to the approved output.
        // Do not add creation, link, rename, chmod or process syscalls.
        for call in [SYS_fsync, SYS_fdatasync, SYS_ftruncate] {
            rules.insert(call, vec![]);
        }
    }
    #[cfg(target_arch = "x86_64")]
    for call in [
        SYS_arch_prctl,
        SYS_open,
        SYS_stat,
        SYS_lstat,
        SYS_readlink,
        SYS_access,
        SYS_poll,
        SYS_getrlimit,
    ] {
        rules.insert(call, vec![]);
    }
    // Query limits only; the kit cannot raise or change supervisor-set limits.
    rules.insert(
        SYS_prlimit64,
        vec![SeccompRule::new(vec![SeccompCondition::new(
            2,
            SeccompCmpArgLen::Qword,
            SeccompCmpOp::Eq,
            0,
        )?])?],
    );
    Ok(SeccompFilter::new(
        rules,
        SeccompAction::Errno(EPERM as u32),
        SeccompAction::Allow,
        std::env::consts::ARCH.try_into()?,
    )?
    .try_into()?)
}

pub fn command(executable: &Path) -> Result<Command> {
    confined_command(executable, None)
}

pub fn file_command(executable: &Path, work: &Path) -> Result<Command> {
    confined_command(executable, Some(work))
}

fn confined_command(executable: &Path, work: Option<&Path>) -> Result<Command> {
    // Non-root use is intentional; the host never escalates or changes identity.
    // SAFETY: geteuid/getpid have no pointer arguments and do not change state.
    let (uid, parent) = unsafe { (libc::geteuid(), libc::getpid()) };
    if uid == 0 {
        return Err("native Linux kit execution requires a non-root host".into());
    }
    let rules = filesystem(executable, work)?;
    let program = filter(work.is_some())?;
    let file_bytes = if work.is_some() { 65536 } else { 0 };
    let mut command = Command::new(executable);
    // SAFETY: the post-fork callback performs only raw OS calls, stack work and
    // errno conversion. Rules and BPF storage are prepared/owned before fork;
    // no allocator, formatting, locks or Landlock builder run in the child.
    unsafe {
        command.pre_exec(move || {
            let fail = || io::Error::last_os_error();
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                return Err(fail());
            }
            if libc::getppid() != parent {
                return Err(io::Error::from_raw_os_error(libc::ESRCH));
            }
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(fail());
            }
            for (resource, value) in [
                (libc::RLIMIT_AS, 268435456),
                (libc::RLIMIT_CPU, 5),
                (libc::RLIMIT_NOFILE, 64),
                (libc::RLIMIT_FSIZE, file_bytes),
                (libc::RLIMIT_CORE, 0),
            ] {
                let limit = libc::rlimit {
                    rlim_cur: value,
                    rlim_max: value,
                };
                if libc::setrlimit(resource, &limit) != 0 {
                    return Err(fail());
                }
            }
            // CLOEXEC rather than closing now preserves std::process's error
            // pipe until exec, while preventing unrelated host FD inheritance.
            if libc::syscall(
                libc::SYS_close_range,
                3_u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            ) != 0
            {
                return Err(fail());
            }
            if libc::syscall(libc::SYS_landlock_restrict_self, rules.as_raw_fd(), 0_u32) != 0 {
                return Err(fail());
            }
            let bpf = libc::sock_fprog {
                len: program.len() as u16,
                filter: program.as_ptr().cast::<libc::sock_filter>().cast_mut(),
            };
            if libc::syscall(libc::SYS_seccomp, libc::SECCOMP_SET_MODE_FILTER, 0, &bpf) != 0 {
                return Err(fail());
            }
            Ok(())
        });
    }
    Ok(command)
}
