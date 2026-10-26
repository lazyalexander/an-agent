# Settled work reclamation and mediated artifact reads

The host owns authorization, accepted artifact identity and retention. A spool
owns domain computation and output meaning. A backend enforces execution limits
and reports termination. Loom remains development and packaging tooling; neither
Loom nor core needs a new API for this profile.

Internal material is disposable by default, but that is not permission to delete
a running or uncertain environment. This first implementation is deliberately
limited to the two work files of a native `file-kit-v1` run. It does not reclaim
VM disks, arbitrary scratch trees, arbitrary failed runs, logs or artifact catalogs.

## Evidence before reclamation

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
# Inspect plan and approval_sha256 before approving it.
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

## External capability request and approval

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
# Inspect the catalog, requested revision, continuation release and limits.
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

## Trust and limits

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

## Executable acceptance

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
