# Instance state commits

A release contains immutable domain code and knowledge. An instance pins that
release and owns a committed state revision. Each execution attempt starts in a
fresh working directory. The spool defines state transitions; the host manages
identity, authorization, evidence and publication without interpreting business
rules. Backend process or VM state is not the authoritative instance state.

This bounded native profile reuses artifact revisions, `run_revision`, kernel
admission, file-kit isolation and settled-run verification. It adds no executor,
core abstraction or Waver dependency.

This document describes the original bundle-only profile. The opt-in
[checkpoint manifest profile](CHECKPOINT-DELIVERY.md) separates delivery product
storage from state, adds frozen recipients and committed consumption evidence.
After opting in, subsequent commits must provide a manifest; the legacy commit
command cannot silently discard delivery or retention metadata.

## Bundle and commands

The kit reads `input.txt` and writes `output.txt` using this JSON envelope:

```json
{"schema":"an-instance-bundle-v1","state":"0","product":"","report":""}
```

All three payloads are opaque UTF-8 strings. Unknown envelope fields and schemas
are rejected; business correctness remains the spool's responsibility. The whole
previous bundle is the next input, including its product and report. This slice
does not add a second live-input channel or context assembler. The serialized
bundle must fit the existing 8192-byte execution input limit. Oversized output
can remain an accepted run artifact but cannot become the next state.

```text
an-agent-host instance-init STORE INSTANCE RELEASE STATE_FILE
an-agent-host instance-init STORE INSTANCE RELEASE STATE_FILE --approve DIGEST
an-agent-host instance-show STORE INSTANCE [REVISION]
an-agent-host instance-run STORE INSTANCE REVISION
an-agent-host instance-run STORE INSTANCE REVISION --execute
an-agent-host instance-commit STORE INSTANCE EXPECTED RUN_ID
an-agent-host instance-commit STORE INSTANCE EXPECTED RUN_ID --approve DIGEST
```

`INSTANCE` is a canonical UUID; release and revision identifiers are full hashes.
Initialization and commit first return a plan and approval digest. Execution and
publication are separate authorizations. Digests bind a reviewed plan; they are
not credentials. Only the trusted host may approve, access the private store or
publish state. Kits cannot self-authorize by producing a digest.

Two runs may consume the same committed revision. After the first commit advances
the head, the second returns `conflict` and the CLI exits unsuccessfully, even if
both produced identical state bytes. An approved stale commit records expected
and actual revisions. There is no automatic retry, rebase or overwrite. The
store lock still serializes head validation and commit. Independent attempts can
execute concurrently in separate run directories; this is not a new scheduler.

## Publication and interruption

The host stores instance metadata, a private revision catalog, `HEAD` and commit
journals under `STORE/instances/INSTANCE`. Initialization prepares a pending
directory and renames it into place. Existing instances and interrupted pending
initializations are never overwritten.

A commit verifies confirmed successful termination, sealed run evidence, the
pinned release and exact input revision binding. Under the store lock it checks
the expected head, writes an exclusive intent, stores the complete candidate
bundle, and atomically renames a prepared pointer over `HEAD`. That pointer change
publishes state, product and report together. A result record follows it.

An accepted run output is still a candidate until this publication. Readers may
select only the head or its verified ancestors, not staged or orphan revisions.
History traversal is bounded to 1024 revisions and fails closed beyond that
bound. The private catalog must not be advertised as a list of published products.
A trusted caller can export a product from a committed bundle; this slice does
not add automatic downstream subscriptions.

An interruption before pointer replacement leaves the old bundle visible. An
interruption afterward may leave a committed revision without a result record.
Inspect `HEAD` and its run/approval provenance before deciding what happened;
exclusive intents prevent blind resubmission. This is process-visible atomic
publication, not a power-loss durability guarantee, distributed compare-and-swap,
or rollback of external effects. The private store assumes a trusted host and is
not protection against a malicious process with the same filesystem authority.

## Acceptance

```sh
cargo test -p an-agent-host --test file_runtime instance::
```

The test fixture compiles and registers a real command-line counter kit. Decimal
parsing and increment rules exist only in that kit. Native isolation denies direct
host-state access, input mutation, writes outside its work area and child process
execution. The acceptance sequence is:

1. Initialize state 0 and execute two distinct runs from that revision.
2. Approve the first result, publishing state 1, its product and report together.
3. Reject the second result as stale without changing the committed bundle.
4. Reclaim both old work directories through the existing approved cleanup path.
5. Start a fresh host CLI and kit process in a new directory, consume state 1,
   and publish state 2 while older committed versions remain readable.

Negative checks cover foreign-instance candidates, missing settlement evidence,
interrupted commit intents, malformed domain input, invalid envelopes, orphan
revisions and corrupted stored content. This demonstrates continuation from
committed data in fresh native processes and directories. It does not claim VM
recreation, memory restoration, arbitrary application recovery or state migration.
