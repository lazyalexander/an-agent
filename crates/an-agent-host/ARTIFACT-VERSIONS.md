# Bounded artifact revisions

This first profile checkpoints one UTF-8 file, independently of VM retention.
Spools carry domain knowledge and validation rules. The host verifies generic
byte identity, lineage and execution boundaries; it does not judge the business
correctness of an edit. Loom authors/packages releases; revision custody belongs
to the host and does not depend on a developer tool or VM lifecycle.

## Operator workflow

Start with a settled successful native file-kit run. `RUN` denotes its retained run
directory, not the guest working directory. `CATALOG` is a separate private
host-owned directory; its parent must exist. `STORE` remains the execution store.

```sh
an-agent-host artifact-import CATALOG RUN
# Record the returned revision as V1.
an-agent-host artifact-checkout CATALOG V1 editable.txt
# Edit and save editable.txt, then compute and approve its SHA-256.
an-agent-host artifact-edit CATALOG V1 editable.txt APPROVED_SHA256
# Record the returned revision as V2.
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

## Identity and settlement

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

## Storage and limits

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

## Reproducible acceptance

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

## Opt-in opaque byte revisions

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

## Native output admission retirement

`spool-artifact-retire INSTANCE RUN OUTPUT [--approve HASH]` closes one verified
native formal output to new revision-based use without deleting its payload or
changing its identity. Checkout, edit-parent selection and managed revision inputs
honor this one-way record. Historical evidence reads, already-acquired inputs,
causal completion and state commit remain independent. A malformed record fails
closed, never requests automatic repair. Raw-file copies and older hosts are
outside this cooperating-host policy; no access revocation or GC is implied.
See [the storage contract](NATIVE-STORAGE.md#retiring-new-artifact-input-admission).

## Publication gate regression coverage

`tests/file_runtime/settlement.rs` starts with real sandboxed file executions,
then reconstructs persisted crash-window snapshots: receipt without audit or
settlement, and complete audit without settlement. Neither the library nor CLI
may publish. Tests also cover a held source lock, malformed evidence, resealed
receipts with invalid termination/operation/backend facts, legacy receipts, and
restoration of complete evidence followed by idempotent import. These are
controlled disk-state tests, not power-loss or filesystem durability proofs.
