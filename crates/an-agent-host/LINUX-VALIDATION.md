# Native Linux execution

Linux is the deployment target. Docker may supply a disposable development
environment on macOS; it is not a runtime backend, dependency, or Waver contract.
The same host run command selects the native platform implementation.

## Boundary

The host applies Landlock ABI 3 with hard compatibility requirements, a seccomp
syscall allowlist, no-new-privileges and resource limits. Missing enforcement
refuses execution. The host must run as a non-root user.

Only the selected executable and standard runtime libraries are readable.
Arbitrary workspace reads, writes, sockets, process creation, ptrace and mounts
are denied. Environment variables are cleared. Descriptors above stdio become
close-on-exec. Linux limits address space to 256 MiB, CPU to five seconds, open
files to 64, regular-file output and core dumps to zero. Shared supervision
bounds wall time and output and kills/reaps cancelled children. Linux also sets
a parent-death signal. These Linux mechanisms do not imply equivalent macOS
resource quotas.

All policy construction occurs before fork; the child installs prepared rules
using raw OS calls. The host itself is not restricted by the child's policy.
Ordinary dynamically linked Linux CLI programs are supported; static linking
and container images are not required.

Landlock does not conceal all filesystem metadata. Standard runtime directories
are readable. The private host store and same-user supervisor remain trusted.
This is not a complete multi-user security boundary or arbitrary dependency
mounting facility.

## Validation

The [ARM Linux VM recipe](ARM-LINUX-VALIDATION.md) provides a separate native
guest validation path without Docker, host directory sharing or SPACE changes.
It uses the existing stdio profile; Linux file-kit/build support remains deferred.

Run cargo test -p an-agent-host --locked as a non-root Linux user with Landlock
ABI 3 or newer. Compilation alone does not prove containment. An outer sandbox
may prevent feature activation; refusal is correct, not a reason to weaken policy.

The initial Linux/aarch64 execution integration suite passed all five tests in
a disposable official Rust development image, using Rust 1.99.1. Tests ran as
UID 65534. The outer Docker seccomp filter was disabled so it could not substitute
for the host's native syscall policy. Successful execution, write denial,
context retention, audit, registration and tamper rejection passed. This is
initial evidence, not a completed security certification or real-provider test.

The Waver cross-repository handoff also passed on Linux/aarch64: explicit
context, approved model CLI call against a local HTTP fixture, matching model
response/kit input digests, and native controlled kit execution. The same
handoff passed on macOS. This does not claim a new live Kimi/Grok provider run.

Policy construction reuses [landlock](https://docs.rs/landlock/0.4.7/landlock/)
and [seccompiler](https://docs.rs/seccompiler/0.5.0/seccompiler/). Model networking
remains an independently approved host operation; only explicit selected bytes
enter the offline kit. Waver owns development and packaging, not Linux policy.
