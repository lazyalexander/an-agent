# Bounded artifact revisions

This first profile checkpoints one UTF-8 file, independently of VM retention.
Spools carry domain knowledge and validation rules. The host verifies generic
byte identity, lineage and execution boundaries; it does not judge the business
correctness of an edit. No core, Waver or VM lifecycle change is required.

## Operator workflow

Start with a successful existing file-kit run. `RUN` denotes its retained run
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
handling. Existing `run` and `run-openshell` commands are unchanged. Accepted
OpenShell file outputs may be imported by the same receipt-checking command;
this slice does not add a VM-specific revision execution command.

## Identity and settlement

Each revision ID is the SHA-256 of its exact manifest bytes. The manifest binds
the content digest, byte count, optional parent and source evidence. Content
identity and revision identity are distinct: identical bytes with different
lineage may have different revisions. Re-importing an identical checkpoint is
idempotent; execution is not automatically deduplicated or retried.

An accepted-run source records the run ID, release, receipt digest and a bounded
reference to the selected upstream revision. An edit records its parent and an
operator-checkpoint declaration, not authenticated human authorship. References
do not recursively embed the entire history or make data trusted instructions.

The host verifies manifest and content hashes on every read. After selection,
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
