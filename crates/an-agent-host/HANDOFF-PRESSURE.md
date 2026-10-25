# CLI handoff pressure: measurements and bounded changes

## Decision

Move the immutable payload/manifest hash calculation outside the artifact catalog
commit lock. Keep copying and synchronized publication inside that lock for now.
Do not introduce a queue, automatic retry, global file service, new backend or
zero-copy API. Per-job work ownership and reclamation locks are unchanged.

The fully staged alternative shortened an uncontended lock further, but every
refused contender also wrote its entire candidate. At 32-way contention it wrote
100 payloads for 32 successful publications; the selected hash-only version wrote
24 payloads for 24 successes and achieved higher successful-publish throughput.
This is a measured tradeoff for bounded CLI tools, not a universal filesystem rule.

## Protocol and limits

Use an optimized release host binary with opt-in timing on the local macOS host.
All inputs are synthetic, with warm filesystem caches. The machine was shared
with normal development activity; these are observed samples, not an isolated
benchmark, statistical significance claim or latency SLA. Trace overhead is
included. Different variants have recorded binary hashes in the local evidence.

For each of 1/4/8/16/32 concurrent workers:

- Execute 100 independent jobs, cycling 1 KiB, 1 MiB, 8 MiB and 16 MiB total output.
  The 16 MiB case uses two 8 MiB files: neither per-file nor aggregate limits change.
- Each job uses the real approved workspace CLI, native isolation, termination
  evidence, artifact acceptance, checkout, reclamation and post-reclaim checkout.
- Every tenth job is cancelled after candidate creation. It must not accept any
  output and may reclaim only after confirmed termination.
- Separately submit 100 unique 8 MiB edits to one already-initialized catalog,
  aligning the first concurrent wave. Count explicit lock conflicts; never retry.
  Check every successful content digest. This is deliberately a contention test,
  not the default independent-spool layout.

The observer invokes ordinary CLI commands and captures JSON results and stderr;
it does not become a runtime scheduler. Observer scripts/raw records remain in
local lab/output material under repository discipline. Product regression tests
cover the actual lifecycle and custody paths. The lab protocol does not simulate
untrusted worker escape, disk exhaustion, power loss or zero-copy handoff.

## Catalog results

Each row contains 100 requests; lock timings include successful acquisition,
fsync and diagnostic overhead through release. Conflicts do not count as success.
The before/selected variants use the same lock/error contract with no waiting.

| Concurrency | Before conflicts | Selected conflicts | Before hold p50/p95/p99 ms | Selected hold p50/p95/p99 ms |
| --- | --- | --- | --- | --- |
| 1 | 0% | 0% | 27.923 / 31.497 / 33.350 | 13.974 / 21.571 / 27.334 |
| 4 | 62% | 65% | 28.039 / 30.127 / 30.299 | 14.255 / 18.891 / 19.739 |
| 8 | 82% | 66% | 29.575 / 31.993 / 31.993 | 13.627 / 18.406 / 18.624 |
| 16 | 88% | 77% | 31.457 / 36.481 / 36.481 | 17.316 / 23.106 / 23.392 |
| 32 | 89% | 76% | 32.489 / 38.338 / 38.338 | 17.216 / 22.348 / 26.560 |

The four-worker sample did not improve its conflict rate. A short critical section
reduces one cost; it does not eliminate scheduling or disk contention. In the
32-worker sample, successful publication throughput changed from 11 / 0.476 s
(23.1/s) to 24 / 0.594 s (40.4/s). Refusal throughput is not useful throughput.
Single-worker end-to-end publication latency stayed similar (p50 62.1 -> 63.1 ms):
this change does not remove hashing or copying work.

### Why not move the copy too?

| 32-worker variant | Accepted / 100 | Hold p50/p95/p99 ms | Payload writes | Successful publications/s |
| --- | --- | --- | --- | --- |
| Original lock scope | 11 | 32.489 / 38.338 / 38.338 | 11 | 23.1 |
| Hash and copy outside lock | 32 | 20.158 / 71.442 / 89.334 | 100 | 27.5 |
| Selected: hash outside only | 24 | 17.216 / 22.348 / 26.560 | 24 | 40.4 |

Full staging's uncontended lock was only 4.077 ms p50, but speculative writes
amplified shared-disk pressure. The selected version avoids writing candidates
that already lost the commit lock. Its ordinary-error guard removes only its own
known pending files; abrupt host death can still leave unindexed staging for
inspection. Existing immutable revisions are never overwritten on conflict or
failed validation. Publication remains atomic per revision, not a multi-file
transaction or power-loss recovery protocol.

### Hashing and copying cost

Final 8 MiB samples (across CLI read/validation/publication hash calls):

| Operation | 1 worker p50/p95/p99 ms | 32 workers p50/p95/p99 ms |
| --- | --- | --- |
| SHA-256 over 8 MiB | 13.867 / 14.292 / 14.590 | 16.948 / 19.907 / 22.276 |
| Create/write/sync 8 MiB | 5.531 / 9.754 / 11.643 | 5.878 / 10.415 / 10.590 |

`write_sync` includes creation and synchronization; it is not a pure memcpy or
physical-device bandwidth measurement. Hash timings exclude file reads. The
per-CLI elapsed measurements include input validation and process startup. The
independent-job run completed all 500 jobs with zero lock conflicts, including
50 cancellations and immutable byte checks after approved scratch reclamation.
Its per-job lock durations include intentional execution/ownership and must not
be interpreted as global serialization.

## Cancellation defect exposed by measurement

An intermediate run recorded a cancellation request at 33.372 ms but CLI return
at 30054.303 ms. It rejected output and eventually confirmed termination, but
settlement was far too late. A remaining inherited pipe writer is consistent
with the 30-second fixture sleeper. Source inspection found that one group signal
or per-member fallback could miss a child forked concurrently with that signal.
The record alone does not identify the precise syscall interleaving.

Cleanup now keeps the leader unreaped, confirms its exit, then re-observes and
signals remaining members of that still-owned group before reaping and joining
pipes. The bounded repeated cleanup is not an execution retry. Unconfirmed group
cleanup is still an error, never authority to publish or reclaim. Unsupported
escaped workers and OS-level uninterruptible tasks are not qualified by this fix.

The new real native regression performs 32 cancellations while a coordinator
starts up to 16 sleepers. All passed. The final full pressure matrix's 50 cancelled
jobs had a maximum observed request-to-CLI-return interval of 179.620 ms, with no
repeat of the 30-second outlier. This is sample evidence, not a hard timing bound.
The original anomalous record is retained alongside successful runs.

## Internal files and external input references

An undeclared internal file is not a formal product: no implicit publication,
version, downstream delivery or business-level indexing. The current profile
still inventories scratch for bounded resource checks, explicit resume and safe
reclamation; this operational evidence does not make every file an artifact.
Do not remove cleanup safeguards merely because a file is semantically opaque.

For external inputs, prefer **no additional file copy** where ownership permits:
a host-managed immutable revision or qualified snapshot can be exposed read-only.
A read-only consumer mount does not prevent another writer changing the source.
A mutable application file must be snapshotted for fixed-version semantics, or
explicitly treated as live input without a reproducibility claim. Never hardlink
writable data across that boundary or grant catalog-directory access for convenience.

The current reviewed profile can read explicitly selected external paths without
copying them into work; digest checks do not freeze those paths for the whole run.
Its artifact-input route deliberately uses a checked-out snapshot and hides the
catalog. This change adds no zero-copy artifact route or clone optimization.
Hash verification still reads bytes, even when an extra filesystem copy is avoided.

## Diagnostics and regression

Set `AN_AGENT_HOST_TIMING=1` on the trusted host CLI to emit
`AN_AGENT_TIMING=` JSON records on stderr. Stdout, permissions and failure policy
are unchanged; child environments remain cleared. Records include stage, PID,
path, byte count, duration and outcome (`lock_acquire`, `lock_hold`, `sha256`,
`write_sync`, `group_cleanup`). Recording is best-effort and not an audit guarantee.
Paths can identify local workspaces: inspect before sharing. Timing is off by default.

```sh
cargo test -p an-agent-host artifact_versions::tests
cargo test -p an-agent-host --test workspace_job large_handoff
```

Coverage includes limit-preserving 16 MiB custody, explicit lock conflict,
diagnostic opt-in without changing stdout/authorization, concurrent publishers,
duplicate identity, tampered existing objects, owned staging cleanup, and
cancel-during-fork settlement. Required workspace, lint and architecture checks
continue to run unchanged; no compatibility fallback or gate exception was added.
