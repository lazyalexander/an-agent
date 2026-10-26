# Owned process lifecycle for stdio kits

## Source review

Codex snapshot `2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4` separates process
ownership, cancellation and confirmed termination:

- [Unified process lifecycle](https://github.com/openai/codex/blob/2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4/codex-rs/core/src/unified_exec/process.rs)
  terminates on drop, notifies output consumers when readers end, and distinguishes
  termination requests from confirmed termination.
- [Child ownership](https://github.com/openai/codex/blob/2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4/codex-rs/utils/pty/src/child.rs)
  keeps ownership during wait/cancellation and separates direct-child cleanup
  from caller-owned process-tree policy.

The host adopts those invariants, not Codex's PTY, async runtime, remote process
server or session pool. It reuses Rust Child, scoped threads, fallible thread
builders and the existing signal-hook dependency. No copied Codex module or new
runtime dependency is introduced. The existing libc dependency now also supports
macOS lifecycle acceptance tests.

## Gaps fixed

Previously, errors after spawn could drop Child without killing/reaping it.
Unscoped reader/writer handles could detach when an earlier join or setup failed.
Ignored kill errors could lead to waiting without confirmed termination, and
later output/deadline observations could overwrite a cancellation reason.
Signal registration cleanup covered ordinary returns but not partial setup or
unwinding. These are host supervision issues, not core admission issues.

The new direct-child owner caches observed exit, explicitly kills and waits when
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

## Evidence and failures

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
model command, see [model cancellation and deadlines](MODEL-CLI.md#cancellation-and-deadlines).

## Limits

The separate [bounded build recipe](FILE-JOBS.md) adds a macOS process-group scope
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

## Acceptance

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

## Reviewed group completion

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
[multi-silk execution plan](MULTI-SILK-PLAN.md) for qualification and limits.

Group cancellation also accounts for forks racing the first signal. Before
reaping the leader, cleanup confirms its exit and re-observes remaining peers
while the group identity is still owned. This prevents a missed late child from
holding inherited pipes until its natural exit. Cleanup observations are bounded
and do not authorize replay or signaling a subsequently reused group ID.
The [pressure report](HANDOFF-PRESSURE.md) retains the observed delayed-cancel
case and the regression/qualification limits.
