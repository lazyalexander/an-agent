# Bounded Rust build and file-kit handoff

This slice adds one concrete build recipe and one file ABI. It does not add a
workflow framework, package manager, code generator, container adapter, or SPACE
backend. Core is unchanged. Waver scaffolds and packages; this host executes.

## Build contract

`an-agent-host build-kit SOURCE TOOLCHAIN_JSON NEW_JOB` previews the source hash,
tool fingerprints, exact arguments, working directories, environments, policies,
output boundary and limits. It creates no job. The operator reviews the plan and
passes its `approval_sha256` with `--approve`. Changed source/profile/paths/tools
invalidate that approval. An existing job is never overwritten or auto-resumed.

The toolchain JSON contains exactly `rustc`, `sdk`, and `linker`: absolute paths
to a concrete Rust compiler (not the rustup shim), the selected macOS SDK, and
clang. Use `rustc --print sysroot` and `xcrun --show-sdk-path/--find clang` to
locate operator-installed tools. They are trusted dependencies; this is not a
compiler/SDK supply-chain attestation. The full SDK/toolchain trees are not hashed.
Keep tools and job parents private to trusted operators during preview and run.

Only a UTF-8 Rust source file, at most 64 KiB, is copied into the job. The fixed
recipe invokes rustc twice (program and test harness), then runs the tests. It
has no Cargo resolution, network download, dependency installation, project
flags, procedural macros, build scripts, or automatic repair/retry. Source-level
macros remain untrusted and compilation itself runs inside Seatbelt.

| Stage | Read boundary | Write / process boundary |
| --- | --- | --- |
| Compiler | Source, selected Rust/SDK/linker trees and OS runtime data | Job build directory; selected trusted compiler/linker programs |
| Tests | Test executable and OS runtime data | No file writes, networking or child processes |
| Acceptance | Host checks exact regular source/program files | New accepted directory, inaccessible to either workload |

Compilation may read data inside approved toolchain/SDK trees. Do not place
secrets there. System library/metadata reads are necessary allowances, not a
claim of total filesystem invisibility. No ambient environment is forwarded.
Local job and Rust toolchain source paths are remapped in emitted binaries.
This reduces path disclosure, not arbitrary private strings in user source.

Each stage has a 60-second wall limit and 64 KiB per captured output stream.
Compiler writes have a 20 MiB per-file limit; there is no aggregate disk/memory
quota or hostile compiler containment. Tests use the stricter no-write/no-fork
policy. The fixed test command uses one test thread; an author can still write
expensive tests, so the supervisor remains authoritative.

The real AgentControl registration, described mount, request, admitted intent,
and tape precede compilation. The host binds the internal `/job` declaration
slot to the new job directory in the plan. Read-only toolchain inputs are
additional plan allowances. Stage status, bounded streams, PID/reaping evidence,
receipt and core audit remain in the job. Only complete success gets `applied`.
Tests passing is evidence of the supplied tests, not proof of domain correctness.

## Artifact reception

On success, `accepted/` contains `main.rs`, `program`, and `manifest.json`.
The portable `an-rust-build-artifacts-v1` manifest binds the two SHA-256 hashes
and passing-test state. The enclosing `an-rust-build-receipt-v1` records the
approval hash and matching manifest. Failed or interrupted jobs do not publish
accepted artifacts. Files must be bounded regular files, not symlinks/hardlinks.

Waver's `receive-build PROJECT JOB` checks those bytes against the current
project source and successful receipt. It copies only the program and portable
manifest, never the private plan, compiler paths, logs or audit. Waver declares,
packages, verifies and exports the two-file deployment. The host independently
validates and registers that deployment; reception grants no execution rights.
Hashes do not authenticate a maliciously forged job directory.

## Runtime contract: file-kit-v1

The existing two-file deployment format is reused. The new exact descriptor
combination is `host: an-agent-file-v1`, `profile: file-kit-v1`, and
`file: { op: rw, path: /work, recursive: true }`. Other fields match the narrow
stdio contract. `/work` is a reserved **declaration slot for this host ABI**,
not access to the machine's literal /work directory or a core-wide path alias.
The execution plan records its binding to one run's private `work/` directory.
Do not interpret this descriptor with another host.

The kit starts in that directory. It reads `input.txt` (a snapshot, at most
8 KiB UTF-8) and creates `output.txt` (at most 64 KiB). The actual policy is
stricter than the coarse YAML request: only that output path is writable.
It cannot modify input, audit, registry, source documents, or other runs.
Network, model calls and subprocesses remain denied. Stdin is empty; stdout
and stderr remain bounded diagnostics. Wall limit is five seconds.

After reaping a successful process, the host rejects a missing, oversized,
symlinked, hard-linked or non-regular output. Accepted bytes are copied to
`artifact.txt` outside the writable directory and identified in `receipt.json`.
Nothing is automatically applied to the user's original document. The original
stdio-only-v1 permissions and native Linux implementation are unchanged.

To pass the accepted product to a subsequent model request, explicitly select
`context-export RUN artifact THREAD_UUID TEXT_BYTE_BUDGET NEW_OUTPUT`. The host
checks success, exit/reaping evidence, fixed path, size and digest before making
a bounded context view. It never substitutes diagnostics or the kit's writable
working copy. [Context export](CONTEXT-OUTPUT.md) and model approval remain
separate steps; Waver does not own runtime context selection.

## Platform and lifecycle limits

This new build/file profile currently executes on **macOS only**. Linux and other
platforms fail closed; no Docker substitute or unrestricted fallback is used.
Native Linux file/build policy and SPACE machine acceptance are subsequent work,
not represented as completed by these tests.

Compiler stages reuse the host's bounded pipe/cancel/deadline supervisor with a
dedicated process group. The leader remains unreaped while the group is signaled,
avoiding PGID reuse. macOS member signaling on denied group signals follows the
approach inspected in Codex's process-group helpers; no Codex runtime dependency
or copied module is introduced. A member cleanup error remains an error.
This covers trusted compiler children, not adversarial setsid/double-fork jobs.
Only the direct leader is reaped by the host; descendant exit is not a cgroup
emptiness proof. Kits and tests are forbidden to fork.

SIGINT/SIGTERM settle through cancellation; no automatic retry occurs. Hard host
death, uninterruptible kernel states and storage errors can leave partial
evidence. Retention is manual. Inspect partial jobs; use a fresh destination for
another attempt. This slice neither promises rollback nor expands workspace GC.

## Acceptance

- `cargo test -p an-agent-host --test file_job`: real compile/test, file execution,
  outside-source include denial, unchanged input, audit/network/child denial,
  missing/oversized/linked outputs, stale approval, cancellation, no false applied.
- Supervisor tests: compiler-group deadline/cancel, exited leader with inherited
  descendant pipes, and existing stdio lifecycle tests.
- Waver's pinned cross-repository `file_job_handoff` test: scaffold → check →
  approved host build → receive → declare → pack/verify → export → register →
  approved file execution. Both libraries remain independently buildable.
