# Reviewed execution profiles and process ownership

## Reviewed local workspace jobs

This bounded macOS profile runs an operator-selected, reviewed local workflow
engine and manages its scratch and selected UTF-8 outputs. It reuses the native
supervisor, exclusive host storage and existing immutable artifact revision
catalog. It does not make Nextflow, Loom or a domain scheduler part of the host.

This is an operator development/acceptance boundary, not a new spool deployment
ABI or an untrusted code-agent execution profile. Existing file/stdio kits retain
their stricter process and filesystem restrictions.

### Authority and profile

The caller selects a private JSON profile outside the job. Never accept this
profile as authority from a spool, model output or writable project configuration.

```json
{
  "schema": "an-workspace-job-v2",
  "executable": "/bin/bash",
  "args": ["{input:0}", "--output", "{work}/result.txt"],
  "environment": {},
  "readonly": [],
  "inputs": ["/operator/selected/runner"],
  "outputs": ["result.txt"],
  "wall_seconds": 60,
  "trusted_process_group": true
}
```

`{work}`, `{input:N}` (zero-based selected input) and `{inputs}` (the snapshot
directory) expand literally in argv and environment values; the host adds no shell
interpretation. Selected files require unique UTF-8 basenames without braces.
Use placeholders rather than original selected paths in arguments or environment.
V1 profiles are refused; update their schema, input arguments and runtime roots
explicitly rather than silently changing their execution semantics. Selecting `/bin/bash` explicitly is selecting a shell. The plan
shows exact commands, environment, selected input hashes and effective Seatbelt
policy. Environment inheritance is cleared; PATH, TMPDIR and LANG are fixed.
Runtime roots must exclude the host job and its ancestors and must not contain
selected inputs. After approval, the host copies selected bytes to a fresh private
snapshot, checks their approved hashes and records the resolved execution manifest.
Selected original paths are denied data reads and execution in the workload policy;
snapshots are read-only and outside writable scratch. Pinned artifacts are copied
from their immutable catalog revision, not the editable checkout. A changed ordinary
input before snapshot verification refuses preparation; no workload is launched.

On macOS, ordinary selected files first use descriptor-based filesystem cloning.
Supported same-volume filesystems share blocks using copy-on-write, with a new
private inode rather than a hard link or a live source mount. The destination is
still checked against the approved digest before launch. Only unsupported-volume
or cross-device errors select the existing full-copy path; permission, size,
existing-destination and other errors refuse preparation. Artifact catalog inputs
keep their existing validated copy path. Input limits and CLI schemas are unchanged.
See [external input clone qualification](artifacts.md) for measurements and limits.

Output acceptance verifies the receipt-bound execution manifest, its exact mapping
from the approved plan, and the snapshot hashes. Source files may subsequently
change or disappear without invalidating those persisted input bytes. Missing or
substituted snapshot evidence refuses acceptance, including old attempts without
that evidence. This does not change the existing termination gates for continuation
or collection. Preparation failures and cancellations record explicit no-spawn evidence when
that evidence can be durably written. They never automatically retry or promote
partial snapshots; see the prelaunch lifecycle below.

The runtime executable and directory dependencies remain operator-controlled and
are **not snapshotted**. The executable still has before/after hash checks; these do
not close a replace-and-restore race. Copying an Apple platform executable failed
native qualification, so this slice does not relocate, re-sign or silently replace
interpreters. This is selected-input custody, not a hermetic runtime closure or
protection against a hostile same-user process modifying host evidence.

The macOS policy permits standard system tools and selected runtime roots to be
read/executed, and permits writes only under `work/` plus `/dev/null`. Network
operations are denied. The qualified JVM additionally needs the specific
`com.apple.system.opendirectoryd.libinfo` Mach lookup for user-property startup;
allowing arbitrary Mach services is unnecessary. Process metadata queries and
signals are limited to the same sandbox. There is no fallback on Linux or another
unsupported platform; their existing host profiles remain unchanged.

### Run, interrupt and explicitly continue

```sh
an-agent-host workspace-run PROFILE NEW_JOB
an-agent-host workspace-run PROFILE NEW_JOB --approve REVIEWED_DIGEST
## SIGINT/SIGTERM requests cancellation of the host-owned execution.
an-agent-host workspace-resume PROFILE JOB PREVIOUS_ATTEMPT
an-agent-host workspace-resume PROFILE JOB PREVIOUS_ATTEMPT --approve REVIEWED_DIGEST
```

An already-cancelled approved call returns `status=cancelled`, `executed=false`
and `process_started=false` without creating a workspace, attempt or successor.
A preview remains a read-only preview. No attempt ID is returned in this case.

Each allocated attempt has a fresh UUID and records its plan, intent and a
provisional `an-workspace-not-started-v1` receipt (`stage=prepared`) before
publishing its predecessor's successor pointer. The pointer is published atomically
from complete bytes. An unpublished continuation cannot authorize reclamation of
the shared workspace. After publication, subsequent decisions address the successor.
The provisional receipt is positive evidence of the ordered prelaunch protocol,
not an inference from a missing PID. It is usable only while `launch.json` is absent
and its predecessor pointer, when present, binds this attempt and approval.
Immediately before entering the supervisor, the host persists `launch.json`.
That marker invalidates provisional no-spawn authority even if the process never
actually starts. Final receipts atomically replace only the provisional receipt;
a failed replacement leaves the launch fence in force.
Cancellation or failure during preparation, and cancellation explicitly returned
by the supervisor before spawn, produce `an-workspace-not-started-v1` receipts.
These bind the plan and intent, record the stage (`preparation` or `supervisor`),
and contain `process_started=false`, `pid=null`, `exit_code=null`, `reaped=false`,
`group_cleanup_required=false` and `termination_confirmed=true`. Here termination
means no process was started, not that a fictitious child was reaped. Preparation
errors retain refusal evidence. Neither case invents an empty process-group
observation.

A valid not-started receipt permits explicitly approved continuation or scratch
abandonment through the existing APIs. It can never publish outputs, even when
scratch contains a previous attempt's successful output. Accepted artifact
revisions remain outside reclamation. Missing/contradictory fields, a changed or
missing intent, or an unsuccessful receipt write confer no lifecycle authority.
A spawn/pre-exec error is still uncertain; a missing PID alone is never no-spawn
proof. A crash before the launch fence can leave usable committed prelaunch
evidence. A crash after the fence, a missing/corrupt receipt, or an incomplete
legacy attempt still requires investigation.
There is no automatic retry, receipt repair or speculative process cleanup. The
host retains bounded stdout/stderr and a receipt even when post-run inventory
cannot be collected. A missing receipt, unconfirmed termination, active host lock
or already-selected successor fences further continuation and collection. The
predecessor and scratch inventory are bound into approval. No stale lock is
removed and there is no automatic retry after an uncertain outcome.

Acceptance, continuation and reclamation share the same receipt gate. For launched
attempts, the existing `an-workspace-receipt-v1` path requires
explicit `termination_confirmed=true`, `reaped=true`, a positive process ID, and
an empty process-group observation bound to that ID without an observation error.
`group_cleanup_required` must be a boolean; successful attempts must explicitly
record `false`. Missing/null whole-round fields in older v1 receipts are not
termination authority, including failed or cancelled receipts. They remain
retained for inspection; these operations refuse rather than infer safe defaults,
rewrite history or start a successor. The new not-started schema is explicit host
evidence, not a reinterpretation of incomplete legacy receipts. Tests in `tests/workspace_job/settlement.rs`
remove fields and exercise both previews and previously approved requests.

Continuation preserves engine work files and cache at the same paths. The caller
supplies the engine's own resume argument; the host does not reconstruct its
internal task graph or infer which tasks should rerun. A resumed engine may
internally retry/recompute its tasks; host `automatic_retries: 0` describes host
submission policy, not an override of the engine's semantics.

### Separate output acceptance and collection

```sh
an-agent-host workspace-accept JOB ATTEMPT
an-agent-host workspace-accept JOB ATTEMPT --approve REVIEWED_DIGEST
an-agent-host artifact-checkout JOB/artifacts REVISION NEW_EDITABLE_FILE
an-agent-host workspace-reclaim JOB ATTEMPT
an-agent-host workspace-reclaim JOB ATTEMPT --approve REVIEWED_DIGEST
```

Only successful, termination-confirmed current attempts may offer their declared
output slots. Output acceptance reads bounded regular non-linked UTF-8 files,
previews their exact hashes, then publishes through the existing artifact catalog.
Domain correctness remains the workflow/spool's responsibility. Failure or
cancellation cannot promote partial output. An exclusive acceptance intent fences
duplicate/partial publication; unknown outcomes require investigation, not retry.
This is not an atomic multi-artifact transaction or a migration/state commit.

Scratch is retained until explicit collection approval. Collection means
**abandoning engine resume rights**. Successful attempts require previously
accepted, still-valid artifact revisions. A confirmed failed/cancelled attempt
may also be explicitly abandoned; its partial work is not a formal artifact.
The approved no-follow inventory covers the whole work tree, including native
cache and undeclared temporary files. Attempt input snapshots are retained evidence,
not scratch; this slice does not add a retention/GC policy for them. Symlinks are unlinked, never traversed.
Regular hardlinks and special files cause refusal. A changed inventory requires
fresh approval; a partial deletion is fenced by a durable intent without retry.

Accepted revisions, input snapshots, receipts, plans and bounded diagnostic streams stay outside
the writable/reclaimed tree. Existing consumers can keep selecting the same
revision after collection. Formal artifact GC, delivery subscriptions and domain
checkpoint completeness are not reimplemented here.

### Layout and limits

```text
JOB/
  attempts/UUID/   plan.json, execution.json, intent.json, stdout.txt, stderr.txt, receipt.json
                  snapshot/inputs/ (approved selected bytes, retained evidence)
                  successor.json, accept-intent.json, accepted.json (when applicable)
  work/           engine-owned private workspace and recovery data
  artifacts/      existing immutable artifact revision catalog
  reclaim-intent.json, reclaimed.json (after explicit collection)
```

The profile bounds wall time to 1–300 seconds, streams to 64 KiB each, individual
generated files to 64 MiB, inventory to 2,000 entries/32 directory levels/128 MiB,
and declared deliverables to 16 files of at most 64 KiB each. The total scratch
limit is checked during inventory, not an enforced disk quota. Runtime dependencies
are read-only capabilities, not a hermetic or reproducible build attestation.

The supervisor owns a process group, reaps its direct child and requires an empty
group observation before declaring termination confirmed. After reaping, it makes
bounded read-only observations for up to one second; this is not an execution
retry and it never signals a reused numeric group during that observation window.
Receipts retain group identity, emptiness, sample count, elapsed observation time
and errors. If the coordinator exits while peers remain, required forced cleanup
is recorded and the round cannot become successful, even with leader exit code 0.
Reviewed workers must
not daemonize, change process groups or launch remote work. Group emptiness cannot
prove that an escaped/remote worker stopped. This boundary does not support such
workloads or hostile agent-generated code; a stronger qualified backend is needed.
Host crash and SIGKILL retain a stale lock: the operator must establish that its
owner and any workers are gone before clearing it. The host does not clear stale
locks or infer ownership from PID absence. Once that independent step is complete,
committed prelaunch receipts can authorize explicit continuation or collection.
Launched/unconfirmed attempts, hostile same-user renames and power loss retain
the existing manual-investigation limitations. No VM framework or automatic GC
service is introduced.

### Acceptance

`cargo test -p an-agent-host --test workspace_job` exercises actual Seatbelt
execution, network/outside-write denial, cancellation, timeout, current-attempt
checks, rejected linked/failed outputs, changed approvals and accepted-version
survival after scratch collection. An external Nextflow acceptance in an-loom
also sends a real CLI SIGINT during a task, resumes through the native engine
cache and consumes pinned output revisions after reclamation. Native tape remains
engine/agent-owned; the host records its boundary evidence, not internal replay.

#### Optional binary outputs and pinned artifacts

The additive `binary_outputs` list opts selected declared outputs into bounded
opaque byte custody; absent entries retain text-only behavior. `artifact_inputs`
may bind a selected read-only snapshot to `{ "path": "/selected/snapshot",
"catalog": "/host/catalog", "revision": "FULL_SHA256" }`. The catalog is not
mounted or passed to the workload. Missing revisions, substituted snapshots,
duplicate bindings and changed approval inputs are refused.
See [artifact version semantics](artifacts.md). These are generic file
capabilities; workbook parsing and presentation composition belong to the kit.

#### Two internal silks and stress qualification

The [multi-silk plan](testing.md) defines invariants, custody transfer,
lock scope and staged stress tests. The real macOS fixture uses one coordinator,
a producer and a consumer in the same native group, with no per-silk host run.
It covers cancellation before/after handoff, early leader exit, silk failure,
timeout, uncertain observations and pinned downstream consumption after copying.
The explicit ignored soak runs four independent jobs for ten mixed rounds each.
This is reviewed spool-body qualification through the existing workspace profile;
it does not create a registered production multi-silk package ABI.

#### Bounded recovery regressions

`cargo test -p an-agent-host --lib workspace_job::recovery_tests` runs a real host
child that exits without unwinding at four persistence boundaries. Each child has
a 15-second outer deadline and uses tiny inputs. Tests qualify child exit before
explicitly removing its stale lock, then check current-attempt authority,
continuation, collection previews, refusal to publish, and retained formal outputs.
They do not claim automatic stale-lock recovery or power-loss durability.

Large-file custody remains a separate 64 MiB test with the same readiness and
execution deadlines. The test profile optimizes only SHA-256, preserving normal
Rust host debug checks while avoiding unoptimized hashing as a CI speed requirement.
Python or TypeScript may drive external acceptance scenarios; moving assertions
out of Rust does not accelerate the host's snapshot or hashing path. Persistence
ordering and fault windows remain tested against the Rust implementation.

### Preserving a reviewed input tree

`an-workspace-job-v2` accepts an optional `input_layout` array parallel to
`inputs`. Omission or an empty array retains the existing basename layout and
CLI behavior. A nonempty array must name every input with a unique portable
relative path. For example, two sources can map to `project/silk/main.sh` and
`project/kit/main.sh`, even when their source basenames match.

The host checks traversal, duplicate destinations, file/directory prefix
collisions and a 16-component depth limit. Existing file-count and byte limits
are unchanged. The mapping participates in preview approval, immutable input
snapshots, execution identity and acceptance reconciliation. Parent directories
are created only under the host-owned snapshot with existing no-link checks.
`{input:N}` resolves the selected leaf; `{inputs}` remains the snapshot root.
The workload cannot edit this tree. Writable results belong under `{work}`.

Loom uses this general file mapping to check a frozen candidate project. The
host does not parse Loom tasks, select domain tests, package releases or evaluate
business correctness. An operator still reviews the generated profile and
approves execution and formal output acceptance separately. The existing native
trusted-process-group scope is unchanged; this is not hostile-agent containment.

## Bounded Rust build and file-kit handoff

This slice adds one concrete build recipe and one file ABI. It does not add a
workflow framework, package manager, code generator, container adapter, or SPACE
backend. Core is unchanged. Loom scaffolds and packages; this host executes.

### Build contract

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

### Artifact reception

On success, `accepted/` contains `main.rs`, `program`, and `manifest.json`.
The portable `an-rust-build-artifacts-v1` manifest binds the two SHA-256 hashes
and passing-test state. The enclosing `an-rust-build-receipt-v1` records the
approval hash and matching manifest. Failed or interrupted jobs do not publish
accepted artifacts. Files must be bounded regular files, not symlinks/hardlinks.

Loom's `receive-build PROJECT JOB` checks those bytes against the current
project source and successful receipt. It copies only the program and portable
manifest, never the private plan, compiler paths, logs or audit. Loom declares,
packages, verifies and exports the two-file deployment. The host independently
validates and registers that deployment; reception grants no execution rights.
Hashes do not authenticate a maliciously forged job directory.

### Runtime contract: file-kit-v1

The existing two-file deployment format is reused. The new exact descriptor
combination is `host: an-agent-file-v1`, `profile: file-kit-v1`, and
`file: { op: rw, path: /work, recursive: true }`. Other fields match the narrow
stdio contract. `/work` is a reserved **declaration slot for this host ABI**,
not access to the machine's literal /work directory or a core-wide path alias.
The execution plan records its binding to one run's private `work/` directory.
Do not interpret this descriptor with another host.

The kit starts in that directory. It reads `input.txt` (a snapshot, at most
8 KiB UTF-8) and writes `output.txt` (at most 64 KiB), using create-or-truncate.
On Linux, the host pre-creates the output inode; `create_new`, atomic replacement
and temporary files are outside this ABI. The actual policy is
stricter than the coarse YAML request: only that output path is writable.
It cannot modify input, audit, registry, source documents, or other runs.
Network, model calls and subprocesses remain denied. Stdin is empty; stdout
and stderr remain bounded diagnostics. Wall limit is five seconds.

After reaping a successful process, the host rejects a missing, oversized,
symlinked, hard-linked or non-regular output. Accepted bytes are copied to
`artifact.txt` outside the writable directory and identified in `receipt.json`.
Nothing is automatically applied to the user's original document. The original
stdio-only-v1 permissions remain unchanged.

To pass the accepted product to a subsequent model request, explicitly select
`context-export RUN artifact THREAD_UUID TEXT_BYTE_BUDGET NEW_OUTPUT`. The host
checks success, exit/reaping evidence, fixed path, size and digest before making
a bounded context view. It never substitutes diagnostics or the kit's writable
working copy. [Context export](model-services.md) and model approval remain
separate steps; Loom does not own runtime context selection.

### Platform and lifecycle limits

The isolated Rust **build** recipe remains macOS-only. File-kit **execution** is
supported on macOS and native Linux; other systems refuse. Linux uses the
existing Landlock/seccomp supervisor with the fixed file permissions detailed in
[Linux validation](platforms.md#runtime-matrix). A Linux deployment
needs a separately built native program; successful file execution does not
establish isolated compilation. SPACE machine acceptance remains separate.

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

### Acceptance

- `cargo test -p an-agent-host --test file_job`: real compile/test, file execution,
  outside-source include denial, unchanged input, audit/network/child denial,
  missing/oversized/linked outputs, stale approval, cancellation, no false applied.
- Supervisor tests: compiler-group deadline/cancel, exited leader with inherited
  descendant pipes, and existing stdio lifecycle tests.
- Loom's pinned cross-repository `file_job_handoff` test: scaffold → check →
  approved host build → receive → declare → pack/verify → export → register →
  approved file execution. Both libraries remain independently buildable.

### Exported debug metadata

The fixed compiler recipe requests `-C link-arg=-Wl,-S` for both program and
test binaries. Rust source-span remapping alone leaves macOS linker object-file
records containing absolute toolchain paths. The selected native linker removes this metadata before the host publishes and
hashes accepted bytes. This does not depend on a best-effort post-link strip
helper that can fail with a compiler warning. Consumers do
not need to mutate accepted programs or rewrite their build receipts.

This removes compiler/linker debug-path records; it does not sanitize private
strings deliberately embedded by source code or dependencies. Local plans and
full diagnostics remain operator data and should not be published as portable
build evidence.

## Process ownership and termination


The direct-child owner caches observed exit, explicitly kills and waits when
stopping, and attempts cleanup on drop for early returns and unwind. Pipe workers
are scoped; the process owner is created inside the scope so its cleanup precedes
automatic joins. All normal worker joins are collected before errors propagate.
Worker creation is fallible, and ordinary I/O failures signal the supervisor.
Interrupted reads are retried instead of being confused with cancellation errors.
If an external reaper consumed the child (ECHILD), cleanup refuses to signal a
possibly reused PID and reports the missing confirmation instead.

The first observed stop reason is retained. If multiple conditions are first
observed in the same poll, priority is cancellation, output cap, I/O failure,
then deadline. A final output-cap check still catches buffered excess after a
natural exit, but does not overwrite an earlier stop reason.

The monotonic deadline begins before spawn. A blocking OS spawn cannot itself be
interrupted by this synchronous implementation; elapsed setup time counts once
control returns. Checks occur approximately every 10 ms, not at hard real-time
deadlines. No grace period or interactive interrupt protocol is granted to kits:
the host terminates the single bounded workload on cancellation/timeout.

### Evidence and failures

Receipts add process.pid, process.reaped and process.scope (direct-child).
No-start refusal or cancellation has a null PID and reaped=false. PID is an
observation, not a reusable process handle or permission to signal a future PID.
Started executions that encounter a recoverable supervision error are failed,
not mislabeled as prelaunch refusals. A cleanup error is explicitly unconfirmed;
it is never marked applied. Failed I/O/setup may prevent retaining partial stream
bytes; the diagnostic is not a claim of complete output retention.

SIGINT/SIGTERM registration ownership belongs to the CLI, not the host library.
Registration guards remove successfully installed handlers on partial failure,
normal return and unwind. Cancellation settles through normal receipt/audit;
the shared allocation lock has already been released before process launch. A later cancellation does not undo an already settled run.
This covers the kit run command. For cancellation of the separate HTTP
model command, see [model cancellation and deadlines](model-services.md#cancellation-and-deadlines).

### Limits

The separate [bounded build recipe](workspace-jobs.md) adds a macOS process-group scope
for trusted compiler/linker children. It retains the leader until group cleanup
and checks individual members when macOS denies the group signal, following the
approach reviewed in [Codex process-group helpers](https://github.com/openai/codex/blob/2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4/codex-rs/utils/pty/src/process_group.rs).
This does not grant subprocesses to stdio/file kits or tests.

The stdio profile still denies process creation. This fixes direct-child
ownership; it does not implement arbitrary descendant discovery or process-tree
management. Allowing child processes in a future profile requires a new reviewed
termination boundary, not a silent relaxation here.

Drop cleanup is best effort if the OS refuses kill/wait. Uninterruptible kernel
states can delay termination and pipe joins; no user-space wrapper guarantees a
hard cleanup deadline in that situation. Panic unwind cleans the child and joins
workers but does not guarantee a final receipt. Abort, SIGKILL, host crash and
power loss bypass Rust destructors. Linux retains its existing parent-death
signal, while macOS has no equivalent crash guarantee in this slice. Partial
artifacts/stale locks after abrupt host death require inspection, not auto-replay.

### Acceptance

- Native sandbox tests verify timeout, cancellation and output overflow with
  waitpid evidence that the direct child was reaped, not just status strings.
- Error/unwind injection with blocked input/output workers verifies owner-before-
  scope cleanup; repeated terminate is idempotent.
- Actual CLI SIGINT and SIGTERM tests inspect the resulting child PID, receipt,
  absence of applied, released store lock and a subsequent successful run.
- Prelaunch cancellation and a full host deadline test verify no-start versus
  started/reaped evidence and failed-run audit.

Signal acceptance uses the standard pgrep utility available on native macOS and
Ubuntu runners, only to identify children of the test's own CLI. Production does
not depend on pgrep or shell-based process discovery.

### Reviewed group completion

A coordinator that exits zero with remaining group members is not a completed
workflow. The supervisor observes peers while retaining the unreaped leader,
records `group_cleanup_required`, terminates the owned group and reports failure
unless a cancellation/timeout already supplies the stop reason. This avoids
promoting partial output merely because forced cleanup succeeded. Single-child
profiles and OpenShell retain their own existing completion contracts.

The reviewed workspace adapter additionally records bounded post-reap group
observations. Reaping and empty-group confirmation are distinct; missing/failed
confirmation prevents acceptance, continuation and scratch reclamation. Numerical
PIDs are diagnostic evidence, never durable handles for later signaling. See the
[multi-silk execution plan](testing.md) for qualification and limits.

Group cancellation also accounts for forks racing the first signal. Before
reaping the leader, cleanup confirms its exit and re-observes remaining peers
while the group identity is still owned. This prevents a missed late child from
holding inherited pipes until its natural exit. Cleanup observations are bounded
and do not authorize replay or signaling a subsequently reused group ID.
The [pressure report](testing.md) retains the observed delayed-cancel
case and the regression/qualification limits.
