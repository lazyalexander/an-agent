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

## Fixed file runtime

`file-kit-v1` uses `linux-landlock-seccomp-file-v1`. It shares the existing
supervisor, Landlock ABI requirement, no-new-privileges, descriptor cleanup,
memory/CPU/FD limits and five-second wall limit. The stdio profile's filesystem
and syscall permissions remain unchanged.

The host snapshots at most 8 KiB UTF-8 into `work/input.txt`, read-only to the kit.
It creates only `work/output.txt` as an allowed writable inode. Landlock grants
read/write/truncate on that inode, input reads and work-directory listing; no
directory creation/removal, link or rename permissions are granted. Seccomp adds
only file sync/truncate operations to the stdio syscall set. The file-size limit
is 64 KiB. Network and subprocess access remain denied. Package requests do not
choose additional paths, syscalls or writable directories.

Linux cannot grant a filename that does not exist yet. The host therefore keeps
the original output inode open and seeds it with a fresh unproduced marker. The
kit must use create-or-truncate, not `create_new`, atomic rename or temporary
files. An unchanged marker is rejected; a deliberately empty result is valid.
The marker detects absence of a product, not the meaning or correctness of one.

After successful exit and confirmed reaping, acceptance requires the original
inode, a bounded regular file and no links. The host copies bytes to `artifact.txt`
outside the writable directory and records their hash/size. Missing, unchanged,
replaced, linked, oversized, failed, timed-out or cancelled output gets no accepted
artifact or `applied`. Diagnostics never substitute for a product. Context export
selects the accepted copy and checks its receipt/hash rather than reading the
kit's writable working copy.

This adds native file execution, not a Linux compiler sandbox. Explicitly compile
reviewed source with the trusted native toolchain before registration. Do not
forge a successful isolated-build receipt or run untrusted generated build hooks.
Waver still only scaffolds, checks and packages; it does not enforce these rules.
SPACE qualification remains separate.

```sh
cargo test -p an-agent-host --test file_runtime --locked
```

The real cross-platform test covers exact accepted output versus diagnostics,
unchanged input, private file/network/process/write denials, link attempts,
missing and oversized output, deliberate empty and exact-limit products,
nonzero exit, timeout, cancellation, context audience and post-acceptance tamper.
Linux acceptance also checks an untouched slot, inode replacement and hardlinks.

### ARM VM acceptance, 2026-10-22

Validated in the existing Lima 2.2.1 VZ/plain ARM VM: Ubuntu 24.04, Linux
`6.8.0-146-generic`, Rust 1.98.0, non-root user, Landlock ABI 4 and no outer
seccomp filter. No host directories or SSH agent were shared. The host workspace
passed 290 tests, with zero failures/ignored tests, plus clippy with warnings
denied and formatting. Existing stdio denial/lifecycle tests stayed enabled.

Waver's real Linux cross-repository suite passed its five file handoff tests and
one stdio/model-fixture handoff test against this host. It explicitly compiled
the reviewed file-list starter and supplied matching portable build metadata;
it did not invoke a Linux isolated-build backend or fabricate its receipt.
The macOS cross-repository isolated-build/file-run acceptance also passed with
the updated create-or-truncate template.

A retained CLI run consumed ` pear\napple\npear\n` and accepted exactly
`apple\npear\n`, SHA-256
`9ff482bbad59dc6d2dda31549c8431f4cfd280a2e6b52f4b0f761b5961593322`.
Input stayed unchanged; a later edit to `work/output.txt` did not alter the
selected accepted product. The Mac checked four explicitly copied evidence files
against transfer hashes and validated the artifact context's intended audience.
Wrong audience and changed accepted bytes were refused. This verifies local
handoff integrity, not authenticated remote execution attestation.

Raw logs, local endpoints and machine identities remain private. The VM disk and
evidence are retained after stopping the VM. No SPACE implementation or
qualification is included in this result.

## Validation

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
