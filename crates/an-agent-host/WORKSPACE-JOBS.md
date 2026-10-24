# Reviewed local workspace jobs

This bounded macOS profile runs an operator-selected, reviewed local workflow
engine and manages its scratch and selected UTF-8 outputs. It reuses the native
supervisor, exclusive host storage and existing immutable artifact revision
catalog. It does not make Nextflow, Loom or a domain scheduler part of the host.

This is an operator development/acceptance boundary, not a new spool deployment
ABI or an untrusted code-agent execution profile. Existing file/stdio kits retain
their stricter process and filesystem restrictions.

## Authority and profile

The caller selects a private JSON profile outside the job. Never accept this
profile as authority from a spool, model output or writable project configuration.

```json
{
  "schema": "an-workspace-job-v1",
  "executable": "/bin/bash",
  "args": ["/operator/selected/runner", "--output", "{work}/result.txt"],
  "environment": {},
  "readonly": ["/operator/selected"],
  "inputs": ["/operator/selected/runner"],
  "outputs": ["result.txt"],
  "wall_seconds": 60,
  "trusted_process_group": true
}
```

`{work}` expands literally in argv and environment values; the host adds no shell
interpretation. Selecting `/bin/bash` explicitly is selecting a shell. The plan
shows exact commands, environment, selected input hashes and effective Seatbelt
policy. Environment inheritance is cleared; PATH, TMPDIR and LANG are fixed.
Runtime roots must exclude the host job and its ancestors. Selected inputs are
digest-bound and checked again before output admission; directory dependencies
must remain operator-controlled and are not transitively hashed.

The macOS policy permits standard system tools and selected runtime roots to be
read/executed, and permits writes only under `work/` plus `/dev/null`. Network
operations are denied. The qualified JVM additionally needs the specific
`com.apple.system.opendirectoryd.libinfo` Mach lookup for user-property startup;
allowing arbitrary Mach services is unnecessary. Process metadata is visible to
this trusted-runtime profile. There is no fallback execution on Linux or another
unsupported platform; their existing host profiles remain unchanged.

## Run, interrupt and explicitly continue

```sh
an-agent-host workspace-run PROFILE NEW_JOB
an-agent-host workspace-run PROFILE NEW_JOB --approve REVIEWED_DIGEST
# SIGINT/SIGTERM requests cancellation of the host-owned execution.
an-agent-host workspace-resume PROFILE JOB PREVIOUS_ATTEMPT
an-agent-host workspace-resume PROFILE JOB PREVIOUS_ATTEMPT --approve REVIEWED_DIGEST
```

Each attempt has a fresh UUID and records its plan and intent before spawn. The
host retains bounded stdout/stderr and a receipt even when post-run inventory
cannot be collected. A missing receipt, unconfirmed termination, active host lock
or already-selected successor fences further continuation and collection. The
predecessor and scratch inventory are bound into approval. No stale lock is
removed and there is no automatic retry after an uncertain outcome.

Continuation preserves engine work files and cache at the same paths. The caller
supplies the engine's own resume argument; the host does not reconstruct its
internal task graph or infer which tasks should rerun. A resumed engine may
internally retry/recompute its tasks; host `automatic_retries: 0` describes host
submission policy, not an override of the engine's semantics.

## Separate output acceptance and collection

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
cache and undeclared temporary files. Symlinks are unlinked, never traversed.
Regular hardlinks and special files cause refusal. A changed inventory requires
fresh approval; a partial deletion is fenced by a durable intent without retry.

Accepted revisions, receipts, plans and bounded diagnostic streams stay outside
the writable/reclaimed tree. Existing consumers can keep selecting the same
revision after collection. Formal artifact GC, delivery subscriptions and domain
checkpoint completeness are not reimplemented here.

## Layout and limits

```text
JOB/
  attempts/UUID/   plan.json, intent.json, stdout.txt, stderr.txt, receipt.json
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
group observation before declaring termination confirmed. Reviewed workers must
not daemonize, change process groups or launch remote work. Group emptiness cannot
prove that an escaped/remote worker stopped. This boundary does not support such
workloads or hostile agent-generated code; a stronger qualified backend is needed.
Host crash, SIGKILL, stale locks, hostile same-user renames and power loss retain
the existing manual-investigation limitations. No VM framework or automatic GC
service is introduced.

## Acceptance

`cargo test -p an-agent-host --test workspace_job` exercises actual Seatbelt
execution, network/outside-write denial, cancellation, timeout, current-attempt
checks, rejected linked/failed outputs, changed approvals and accepted-version
survival after scratch collection. An external Nextflow acceptance in an-loom
also sends a real CLI SIGINT during a task, resumes through the native engine
cache and consumes pinned output revisions after reclamation. Native tape remains
engine/agent-owned; the host records its boundary evidence, not internal replay.
