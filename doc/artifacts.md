# Artifact versions, fixed inputs and mediated access

## Bounded artifact revisions

This first profile checkpoints one UTF-8 file, independently of VM retention.
Spools carry domain knowledge and validation rules. The host verifies generic
byte identity, lineage and execution boundaries; it does not judge the business
correctness of an edit. Loom authors/packages releases; revision custody belongs
to the host and does not depend on a developer tool or VM lifecycle.

### Operator workflow

Start with a settled successful native file-kit run. `RUN` denotes its retained run
directory, not the guest working directory. `CATALOG` is a separate private
host-owned directory; its parent must exist. `STORE` remains the execution store.

```sh
an-agent-host artifact-import CATALOG RUN
## Record the returned revision as V1.
an-agent-host artifact-checkout CATALOG V1 editable.txt
## Edit and save editable.txt, then compute and approve its SHA-256.
an-agent-host artifact-edit CATALOG V1 editable.txt APPROVED_SHA256
## Record the returned revision as V2.
an-agent-host run-version STORE RELEASE CATALOG V1
an-agent-host run-version STORE RELEASE CATALOG V1 --execute
an-agent-host run-version STORE RELEASE CATALOG V2 --execute
```

Use a SHA-256 utility such as `shasum -a 256 editable.txt` on macOS or
`sha256sum editable.txt` on Linux. The approved digest is mandatory: changed bytes
are refused, not silently adopted. This does not detect whether an application
has finished a logically consistent save; the operator or future shuttle must
provide that checkpoint. Checkouts exclusively create a new file outside the
catalog, never overwrite an existing file or lock the editor working copy.

`run-version` currently selects the native backend and uses the existing
registration, AgentControl admission, sandbox and artifact acceptance path.
It previews unless `--execute` is given; cancellation uses existing CLI signal
handling. Existing `run` and `run-openshell` commands are unchanged. Import
requires a native `STORE/runs/RUN` directory and the shared `settled_run` gate,
while holding the source store lock. A successful receipt alone is insufficient:
receipt, operation, termination observation, plan and final settlement/audit
bindings must agree. Missing or inconsistent evidence is refused before any
catalog publication. No missing evidence is synthesized and no run is retried.

OpenShell and unknown backends are refused by this import command until they
have an explicit host settlement contract. OpenShell's remote termination
observation does not substitute for completed host bookkeeping. Its retained
output remains available for inspection; native marker files are not imposed on
that backend or inferred from its receipt.

### Identity and settlement

Each revision ID is the SHA-256 of its exact manifest bytes. The manifest binds
the content digest, byte count, optional parent and source evidence. Content
identity and revision identity are distinct: identical bytes with different
lineage may have different revisions. Re-importing an identical checkpoint is
idempotent; execution is not automatically deduplicated or retried.

A newly imported accepted-run source records the run ID, release, receipt digest,
verified settlement marker and a bounded reference to the selected upstream
revision. Existing catalog revisions remain readable with their original IDs;
re-import under the stronger source-evidence contract may produce a new revision
for identical content. Legacy runs without settlement evidence cannot be imported. An edit records its parent and an
operator-checkpoint declaration, not authenticated human authorship. References
do not recursively embed the entire history or make data trusted instructions.

The host verifies manifest and content hashes on every read. New revision-based
input acquisition also checks the monotonic closure record before and after its
read, without excluding other readers. After selection,
the exact bytes are held for this invocation and copied to its existing
`input.txt` snapshot. Both plan and receipt record `input_revision`; input hashing
continues to use the actual byte snapshot. New edits do not change this input,
cancel the run, invalidate completed output or schedule downstream work.

```text
accepted output -> V1 -> downstream run bound to V1
                   |
             editable copy -> V2 -> another run explicitly bound to V2
                   |
             alternative edit -> V3 (may also name V1 as parent)
```

There is no mutable latest pointer and no automatic merge. Parent references
support independent branches; a future application/spool may provide merge
semantics with explicit preconditions. Recovering old bytes is not reversing an
external operation, resuming a process or restoring a VM.

### Storage and limits

`CATALOG/revisions/REVISION/{revision.json,content.txt}` is a private, host-owned
checkpoint bundle. Publication reuses host exclusive writes, link checks,
bounded reads and the cooperating-writer lock. Complete bundles are published
from a private pending directory; partial failures are retained and never
reported as accepted. Reads do not select pending directories. A stale lock
after a crash requires operator investigation; there is no automated recovery.

This bounded profile reuses existing host storage primitives rather than adding
a second general workspace engine. The existing `an-agent-workspace` mutable
tree/HEAD/branch API is not silently adopted as the artifact provenance contract.
Future storage consolidation can preserve these manifest and binding semantics.

- Artifact content and manifests are limited to 64 KiB; text must be UTF-8.
- The existing execution input limit remains 8 KiB. A larger valid checkpoint
  is rejected as input before creating a run, without truncation.
- Failed, cancelled or unaccepted outputs cannot seed accepted-run revisions.
- Tampered manifests/content, symlinks and hardlinks fail validation.
- Catalogs and run stores are trusted host storage, not writable by kits. Hashes
  detect corruption against selected identities, not hostile same-user ownership
  of the whole store. No filesystem immutability or multi-user service is claimed.
- Retention is manual. There is no garbage collection, independent backup,
  directory-tree versioning, format-specific merge or power-loss recovery promise.

### Reproducible acceptance

```sh
cargo test -p an-agent-host --test file_runtime revisions
```

The test reuses the actual isolated file-kit fixture (uppercase transformation
with filesystem/network denial probes). It executes the CLI import, checkout,
edit, preview and pinned execution commands. It simulates an operator edit:
`ALPHA` becomes `human beta`; V1 still produces `ALPHA`, while V2 produces
`HUMAN BETA`. A later working-copy edit changes neither version. Assertions check
the actual input snapshot, output, receipt binding and kernel applied audit.
Additional cases reject corrupt sources/catalogs, unaccepted runs, linked files,
unapproved edits, missing parents, invalid UTF-8 and over-limit execution input.

### Opt-in opaque byte revisions

Workspace profiles may explicitly list `binary_outputs`, a unique subset of
`outputs`. Other outputs keep the existing 64 KiB UTF-8 contract. Byte objects
are limited to 8 MiB each and 16 MiB per acceptance; their host-owned revision
manifest uses `an-artifact-revision-v2` with `content.bin`. Existing v1 text
manifests and identities remain unchanged. The host does not inspect extensions,
parse Office ZIPs, evaluate formulas or certify a document safe to open.

`artifact-checkout` makes a new editable copy. `artifact-edit` inherits the
parent's storage format and commits only the approved saved bytes as a child
revision. Operator provenance is a declaration, not authenticated human authorship.
Original revisions remain immutable; there is no mutable `latest` reference.
Text execution profiles explicitly refuse byte revisions, even when their bytes
happen to be valid UTF-8. This does not widen `file-kit-v1` or VM output profiles.

A workspace can separately declare `artifact_inputs` with `path`, `catalog` and
an exact `revision`. The selected path must already be a declared read-only
input and its bytes must match that revision. The catalog remains host-only;
workloads receive only a checked-out snapshot. Approval binds both the snapshot
and immutable revision. Publication rechecks the original input and catalog
binding. Candidate provenance records compact revision/hash/size references,
not a recursively copied transcript or domain interpretation.

Cancelled, failed, unconfirmed or superseded attempts cannot publish bytes.
A late candidate does not repair a cancelled receipt. Reclamation still requires
settled evidence and separate approval; accepted revisions remain outside scratch.
The existing trusted-local-ownership and nontransactional multi-output acceptance
limitations apply. A content digest is neither a publisher signature nor an
independent backup. Tests exercise native byte execution and tampering, saved
edits, fixed downstream consumption, cancellation and late-output rejection.

### Native output admission retirement

`spool-artifact-retire INSTANCE RUN OUTPUT [--approve HASH]` closes one verified
native formal output to new revision-based use without deleting its payload or
changing its identity. Checkout, edit-parent selection and managed revision inputs
honor this one-way record. Historical evidence reads, already-acquired inputs,
causal completion and state commit remain independent. A malformed record fails
closed, never requests automatic repair. Raw-file copies and older hosts are
outside this cooperating-host policy; no access revocation or GC is implied.
See [the storage contract](native-storage.md#retiring-new-artifact-input-admission).

### Publication gate regression coverage

`tests/file_runtime/settlement.rs` starts with real sandboxed file executions,
then reconstructs persisted crash-window snapshots: receipt without audit or
settlement, and complete audit without settlement. Neither the library nor CLI
may publish. Tests also cover a held source lock, malformed evidence, resealed
receipts with invalid termination/operation/backend facts, legacy receipts, and
restoration of complete evidence followed by idempotent import. These are
controlled disk-state tests, not power-loss or filesystem durability proofs.

## External input clones: fixed versions without a full payload copy

### Scope and decision

Reuse macOS `fclonefileat` inside the existing attempt snapshot preparation.
No new input mode, spool API, global catalog, backend or Loom dependency is added.
Ordinary selected inputs may avoid a full payload write on a compatible filesystem.
This is copy-on-write snapshotting, not a mutable-path mount, hard link, zero-read
hashing or a guarantee of zero physical I/O. Existing 64 MiB/file and 100 MiB/plan
bounds remain unchanged. Pinned artifact inputs retain their validated catalog path.

The [Apple syscall contract](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/clonefile.2)
describes separate inode attributes and shared data blocks, private subsequent
writes, exclusive destination creation and filesystem limitations. Writes after
cloning can still exhaust disk capacity. Clone support does not make an untrusted
native process equivalent to a VM.

### Version and authority boundaries

1. Existing approval identifies selected bytes by digest.
2. Open the source without following its final symlink, pin the descriptor, and
   check regular-file type, single link and size. The source descriptor avoids
   substituting a different inode between opening and cloning.
3. Exclusively clone into the existing host-owned attempt snapshot directory.
   Do not copy source ownership or request source ACL inheritance.
4. Verify the destination against the approved digest, apply read-only mode and
   sync it. Only then publish the resolved execution manifest and launch.
5. The workload receives only snapshot placeholders. Original selected paths
   remain denied. Acceptance rechecks persisted snapshot and manifest evidence.

Source changes during preparation either leave exactly the approved snapshot or
cause preparation to fail; a successful clone alone is insufficient. Later writes,
truncation, pathname replacement and unlink of the source cannot change the
accepted snapshot inode. This does not provide a transaction across independently
changing input files or protect against a malicious same-user host administrator.

Only `EXDEV` and `ENOTSUP` fall back to the existing bounded copy. Other errors
remain errors; a pre-existing destination is never overwritten. Linux retains the
existing copy implementation and the workspace execution profile remains macOS-only.
There is no new Linux reflink or automatic execution retry in this change.

### Cancellation and retention

Cancellation is checked before preparation, between files, after the clone and
before manifest publication/launch. The filesystem syscall is synchronous and is
not claimed to be preemptible. Existing bounded reads and hash checks remain;
this is not a new cancellation latency guarantee. Failed/cancelled preparation
keeps the existing explicit not-started settlement path and never promotes a
partial snapshot as an accepted product.

During execution, the existing job lock and termination evidence prevent collection.
Unknown termination refuses collection and output acceptance. After confirmed
termination, explicit reclaim removes work scratch; successful results must first
be accepted. **Input snapshots remain attempt evidence**, as before. No snapshot
garbage collector or new retention policy is introduced. Removing a clone reference
does not delete the original; deleting the original does not free blocks still
referenced by the retained clone. Capacity savings can diminish as sources change.

### Local evidence (2026-10-26)

The real macOS host test writes every byte of a 64 MiB input. Five cases exercise
in-place overwrite, truncation, pathname replacement, unlink and cancellation.
The workload waits while the source is mutated, then hashes the snapshot itself.
Checks include distinct inodes, fixed digest, refusal to collect while running,
refusal on missing termination confirmation, accepted output checkout after work
reclamation, and retained input evidence. A cancelled attempt cannot publish.
Additional unit coverage refuses links, oversized inputs, existing destinations,
changed prelaunch input hashes and missing/substituted manifests.

An independent local filesystem probe writes complete 64 MiB and 1 GiB files,
checks allocated source blocks, calls the same syscall and compares it with a
streaming full copy. Each size has three observations on warm local storage:

| Size | Clone syscall ms | Full copy + sync ms | Initial SHA-256 read ms |
| --- | --- | --- | --- |
| 64 MiB | 0.103–0.135 | 8.299–12.671 | 23.707 |
| 1 GiB | 0.118–0.135 | 102.648–264.341 | 369.025 |

These are not end-to-end host timings, statistically isolated benchmarks or SLAs.
The clone column excludes later hashing, chmod and fsync; the copy column includes
streamed reads/writes and fsync. Development tests use a debug binary and should
not be compared to these Python/OpenSSL hash timings. The 1 GiB probe qualifies
filesystem behavior only, not host admission at that size. It also checks digest
stability after overwrite, truncation and source unlink.

The opt-in `AN_AGENT_HOST_TIMING=1` diagnostic emits `snapshot_clone` with
`cloned`, `copy_fallback` or `error`. Successful clone records mean no explicit
payload copy was performed on that path, not that the operating system performed
no I/O. Raw logs include local paths and stay in private observation output.
The observer script stays in ignored `lab/input-clones/`; product regressions live
in `tests/workspace_job/clone_handoff.rs` and adjacent snapshot unit tests.

Cross-volume/unsupported-filesystem fallback is specified but not qualified on a
second local volume in this slice. Disk exhaustion, crash durability and hostile
process escape are not qualified by these measurements. Preserve the existing
fixed-version and termination gates before broadening any of those claims.

## Settled work reclamation and mediated artifact reads

The host owns authorization, accepted artifact identity and retention. A spool
owns domain computation and output meaning. A backend enforces execution limits
and reports termination. Loom remains development and packaging tooling; neither
Loom nor core needs a new API for this profile.

Internal material is disposable by default, but that is not permission to delete
a running or uncertain environment. This first implementation is deliberately
limited to the two work files of a native `file-kit-v1` run. It does not reclaim
VM disks, arbitrary scratch trees, arbitrary failed runs, logs or artifact catalogs.

### Evidence before reclamation

New native file executions write `settled.json` only after receipt, kernel finish and
audit have been written. The marker binds the receipt and audit digests and the
operation identity. It records completion of host bookkeeping, not necessarily
successful execution. Reclamation independently requires all of:

- Matching run and operation identities, a valid plan digest, and a matching
  settlement marker and audit digest.
- A supported native file backend, reaped direct child and explicit
  `termination_confirmed: true`, with no reconciliation error.
- Either a successful exit with a separately accepted `artifact.txt` matching its
  receipt digest and length, or matching cancelled receipt/observation with a
  recorded cancellation request and no accepted artifact. The latter grants only
  scratch reclamation, never artifact import or external capability execution.
- Exactly `work/input.txt` and `work/output.txt`, both bounded regular files with
  no symlinks or hardlinks. Unknown entries cause refusal, not recursive deletion.
- An operator-approved digest binding those exact work bytes and evidence.

```sh
an-agent-host work-reclaim HOST_STORE RUN_ID
## Inspect plan and approval_sha256 before approving it.
an-agent-host work-reclaim HOST_STORE RUN_ID --approve PLAN_SHA256
```

The store lock serializes metadata/cleanup operations, not child lifetimes.
Active attempts have no valid terminal settlement and cannot be reclaimed. After
validation, an
exclusive `reclaim-intent.json` is recorded, the two files and empty directory
are removed, and `reclaim.json` records the result. A partial deletion or missing
result requires inspection, not an automatic retry. No stale lock is removed.
Input evidence, accepted artifact, executable, plan, receipt, settlement marker,
diagnostic streams and kernel audit remain unchanged. Existing artifact import,
checkout and pinned-version execution still work after reclamation.

Old runs without a settlement marker are ineligible. Failed and timed-out runs
retain their work, even if the process was reaped. Unknown
termination is never upgraded by elapsed time, an exit claim or a cancellation
request. Future policies may separately qualify these cases; no background GC
or retention scheduler is introduced here.

### External capability request and approval

A requesting kit delivers this bounded JSON as its accepted output:

```json
{
  "schema": "an-capability-request-v1",
  "capability": "artifact.read",
  "revision": "FULL_SHA256_REVISION"
}
```

Unknown fields, capabilities and invalid revisions are rejected. A request
cannot specify host paths, commands, credentials or a continuation executable.
The operator supplies a private catalog and registered continuation release:

```sh
an-agent-host capability-read HOST_STORE REQUEST_RUN_ID CATALOG RELEASE
## Inspect the catalog, requested revision, continuation release and limits.
an-agent-host capability-read HOST_STORE REQUEST_RUN_ID CATALOG RELEASE --approve PLAN_SHA256
```

Preview validates the request and existing revision-execution contract without
starting a kit or giving it catalog access. Approval is specific to that request
evidence, catalog, revision, release, backend and limits. The host rechecks the
evidence, records an exclusive `capabilities/PLAN_SHA256/intent.json`, then uses
the existing controlled revision executor. The continuation receives selected
bytes as its fixed input, never the catalog path or host credentials. The result
links the request to the continuation receipt. That receipt pins the input
revision and the normal kernel admission/audit path remains in use.

The same grant cannot be automatically submitted twice. An interruption after
intent but before result is uncertain; even if a kit actually finished, no
retry is inferred. Execution errors are conservatively recorded as uncertain.
Cancellation before submission creates no grant; cancellation during execution
uses the existing supervisor. This is a new bounded run, not resumption of the
requesting process. There is no live RPC server, recursive request dispatcher,
HTTP capability, application-write service or general workflow engine.

### Trust and limits

The CLI/library caller is trusted host authority. The approval digest binds a
reviewed plan; it is not an authentication token or a secret. Do not expose the
host store, CLI, or approval function directly to an untrusted kit. OS sandbox
enforcement, not prompt wording, prevents direct catalog reads, network access,
child processes and audit writes. The store and catalog must be private to the
trusted host. Static link checks do not defend against hostile same-user rename
races. The added records retain existing filesystem durability semantics, not
power-loss atomicity or remote attestation.

The fixed profile retains its existing 8 KiB input, 64 KiB output and five-second
limits. Internal work copies are not promoted to revisions. This example proves
the mediation boundary for `artifact.read` only; new external services still
need their own bounded authorization and actual-effect receipts.

### Executable acceptance

```sh
cargo test -p an-agent-host --test file_runtime
```

The lightweight test kit executes through registration, AgentControl admission
and the real native sandbox. It produces an artifact, emits a read request, is
denied direct catalog access, and passes selected bytes to an approved kit.
Tests check that preview/wrong approval/unsupported requests/cancellation and
uncertain duplicate submissions do not start a continuation. Reclamation tests
compare retained bytes before and after deletion, import products again, reject
altered approvals, extra entries, links, missing settlement and mismatched or
unconfirmed observations. A reclamation attempt while a real silk is still
writing is refused before cancellation; its partial output remains present.
The concurrent native regression also reclaims a confirmed cancelled attempt
without affecting its live peer or retained formal revisions.
Only isolated test-owned work copies are removed by these tests.
