# Multi-silk native execution and custody plan

## Goal and boundaries

Run one reviewed spool body containing a coordinator and two internal silk
processes inside one native execution boundary. Internal domain sequencing stays
in the spool; the host owns cancellation, whole-group settlement and formal
artifact admission. Loom supplies development/checking/packaging and does not
schedule these processes or decide retention. No per-silk VM, grant or independent
host retry is introduced.

The first implementation reuses `workspace-run`, its operator-owned profile,
macOS policy, process-group supervisor and artifact custody. The fixture scripts
are a minimal domain spool body, not a new registered production spool ABI.
Untrusted arbitrary workers, daemonization, process-group escape and remote
work remain unsupported. Existing Linux single-entry behavior is preserved;
Linux multi-process qualification is a separate task, with no weaker fallback.

## Implementation sequence

1. Qualify the existing lifecycle boundary using two separately executing silks.
   Producer publishes an internal file; consumer reads it and offers an output.
   The coordinator waits for both. One attempt, one cancellation owner, one work
   tree and one process-group identity cover all three processes.
2. Correct premature success: if the leader exits zero while group members remain,
   terminate the remaining workers but classify the round as failed. Killing a
   worker is not evidence it finished its task. Preserve this distinction in the
   host receipt even if cleanup subsequently succeeds.
3. Record the group observation and its errors separately from leader reaping.
   Unknown/live group evidence must fence acceptance, resume and reclamation.
   Test both actual running processes and synthetic uncertain observations; do
   not claim the latter proves an OS-level unkillable-process condition.
4. Reuse separate output acceptance: after whole-round settlement, copy bounded
   declared files into the host catalog, hash and publish an immutable revision.
   Downstream binds a checked-out version, not the producer's mutable work path.
   Reclaim only explicitly approved scratch; preserve accepted versions/evidence.
5. Run bounded concurrent soak tests. Profile lock contention and copying before
   introducing queues, lock sharding, clone optimizations or new backends.

## Invariants and failure policy

- A leader exit, cancellation request or successful signal delivery alone never
  establishes whole-round termination.
- Cancellation covers the owning group, including a silk waiting on another silk.
  Partial output, even if syntactically complete, is not accepted after cancellation.
- A zero-exit coordinator that abandons workers cannot publish a successful round.
- Reclamation requires a current termination-confirmed attempt and a fresh approved
  no-follow inventory. Unknown evidence is retained for investigation, not retried.
- Explicit continuation selects a confirmed predecessor; fresh execution uses a
  new job/attempt. Neither restores process memory or silently recovers a silk.
- Successful formal output remains readable after the work tree is removed. Local
  filesystem history is not a rollback guarantee for effects in external systems.

## File handoff and locks

Use `spool work -> host staging -> validated immutable workspace revision ->
read-only downstream snapshot`. This is a custody transfer, not clipboard
copy/paste and not an arbitrary move requested by the spool.

The current workspace profile waits for whole-group termination before reading
candidates; it holds a per-job lock during execution and acceptance. This protects
one resumable shared work tree, not all spools. The single-entry executor's shared
store lock covers metadata allocation, not child runtime. Both locks currently
fail closed on contention rather than queue. File locks are not inherently the
bottleneck: scope, hold duration, contention rate and synchronous disk work decide.

Current bounded acceptance reads candidates and publishes through the existing
catalog while holding the job lock. Do not claim large-file streaming, global
CAS, atomic multi-output publication or short-lock performance not yet measured.
For larger files, move copy/hash work to unique host staging outside shared commit
locks, then recheck the attempt/generation and commit a small manifest under lock.
Stage on the destination filesystem before atomic rename. Cross-filesystem rename
is not atomic; copy/verify first. A manifest is readiness, file appearance is not.

Do not rename a still-writable spool file into the formal catalog: an existing
open descriptor can continue changing the same inode. Do not hardlink writable
inputs/outputs across trust boundaries. Ordinary independent copies are the safe
baseline; clone/reflink may be an optimization only with independent-write
semantics and identical validation. A live snapshot requires a separate producer
handoff protocol; this slice publishes only after whole-round settlement.

## Test and stress matrix

| Tier | Cases | Required result |
| --- | --- | --- |
| Required regression | Normal producer/consumer handoff | Both complete; copied revision matches output and survives cleanup |
| Required regression | Cancel before and after internal handoff | Both silk processes stop; partial output is rejected; confirmed cleanup is possible |
| Required regression | Coordinator exits zero early; silk fails | No successful publication; remaining workers are cleaned or explicitly unconfirmed |
| Required regression | Deadline; live/unknown/inconsistent termination | No acceptance, resume or reclamation without confirmed current evidence |
| Bounded local soak | 4 independent jobs x 10 mixed rounds | No false success, accepted partial output, surviving named silk or changed retained revision |
| Post-hardening saturation | 1/4/8/16/32 concurrent jobs, 100 rounds each | Report throughput, queue/refusal counts, per-stage p50/p95/p99 and resource high-water marks |
| Post-hardening copy pressure | 1 KiB, 1 MiB, 8 MiB, 16 MiB outputs within profile limits | No partial publication; quantify copy/hash/fsync time and lock hold duration |
| Post-hardening faults | Disk full, process/FD limits, forced host death, signal storms, staging interruption | Fail closed, preserve diagnostic evidence, never silently replay or release unknown writers |
| Post-hardening retention | Concurrent consumers, edits and collection; stale approval; altered links | Pinned versions unchanged; no deletion of retained references or host paths |

The saturation/fault tiers are a plan, not completed evidence. Run destructive
resource-limit tests only inside isolated test roots with an external watchdog;
never fill the operator's actual disk or exhaust the whole machine. Observe
process/FD counts, RSS, scratch/catalog bytes, cleanup duration, surviving workers,
lock refusals and revision checks. Distinguish workload output failures from host
control failures. The correctness threshold is zero unsafe acceptances/deletions;
performance thresholds follow a measured baseline, not guessed millisecond limits.

## Evidence and commands

```sh
AN_AGENT_ACCEPTANCE_DIR=/absolute/path/to/evidence \
  cargo test -p an-agent-host --test workspace_job multi_silk -- --nocapture
AN_AGENT_ACCEPTANCE_DIR=/absolute/path/to/soak-evidence \
  cargo test -p an-agent-host --test workspace_job multi_silk_soak -- --ignored --nocapture
```

Retain attempt/profile identities, selected source hashes, member PIDs/group IDs,
event times, cancellation observations, settlement errors, receipts, inventory and
accepted revision hashes. Reports are test evidence, not a second production tape.
Keep synthetic fixture paths/data separate from public documentation; CI uploads
bounded reports and records which platform actually ran the tests.

## Initial measured result

The first local macOS acceptance passed all six regression cases. The explicit
four-worker, ten-round soak passed 40/40 cases (eight each of success, cancellation
before/after handoff, early coordinator exit and silk failure) in 4.73 seconds.
Per-case medians ranged from 378 to 791 ms, including fixture control/validation
and custody checks; these are not worker execution benchmarks or an SLA.
No named silk survived settlement, no cancelled/failed candidate was accepted,
and successful pinned versions survived scratch reclamation in this test set.
The one-second timeout case runs in required regression, not in the soak mix.
Saturation, disk-pressure, process-limit and host-crash tiers remain unexecuted.

## Handoff pressure follow-up

The [CLI handoff measurements](HANDOFF-PRESSURE.md) now cover 1–32 concurrent
workers, bounded 1 KiB–16 MiB handoffs and shared-catalog contention. They select
hash-only lock narrowing rather than speculative copying, and document a
cancel-during-fork defect exposed by pressure. Disk exhaustion, host crash and
long-duration resource-limit qualification remain future work.
