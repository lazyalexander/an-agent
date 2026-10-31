# Instance commits and checkpoint delivery

## Instance state commits

A release contains immutable domain code and knowledge. An instance pins that
release and owns a committed state revision. Each execution attempt starts in a
fresh working directory. The spool defines state transitions; the host manages
identity, authorization, evidence and publication without interpreting business
rules. Backend process or VM state is not the authoritative instance state.

This bounded native profile reuses artifact revisions, `run_revision`, kernel
admission, file-kit isolation and settled-run verification. It adds no executor,
core abstraction or Loom dependency.

This document describes the original bundle-only profile. The opt-in
[checkpoint manifest profile](checkpoints.md) separates delivery product
storage from state, adds frozen recipients and committed consumption evidence.
After opting in, subsequent commits must provide a manifest; the legacy commit
command cannot silently discard delivery or retention metadata.

### Bundle and commands

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

### Publication and interruption

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

### Acceptance

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

## Checkpoints, commit manifests and delivery evidence

### Contract

A spool release defines domain state, transitions and validation. A checkpoint
records committed state and the fixed data references needed to continue it.
The host owns publication, delivery evidence and retention; a backend working
disk or memory snapshot is not the state authority. Tape explains execution and
does not replace these records.

The bounded file-kit profile keeps the existing state/product/report envelope at
the executable boundary. A checkpoint commit extracts the product into a separate
artifact revision, clears the product field in the stored state envelope, and
includes a commit manifest in the immutable checkpoint's provenance. Updating
the instance head publishes the state, product reference, frozen recipient list
and cumulative consumption evidence together. Staging alone publishes nothing.

The operator-reviewed manifest declares `recipients` (instance UUIDs),
`state_refs` (publisher instance and committed checkpoint pairs), and `retention`
(`keep` or `delivery`). These declarations do not authorize filesystem access.
The host validates identities and references; kits still define domain semantics.
The profile handles one UTF-8 product per commit, up to 32 recipients/references
and 128 consumed events per instance. Exceeding a bound fails explicitly.

### Delivery and interruption

An event is identified by its publisher and immutable checkpoint revision. Its
product and recipient set never follow a mutable latest pointer. A downstream
execution input contains its current state and the event product in the existing
`product` field. A host-generated input revision binds the event, recipient and
expected downstream state. A successful kit output is only a candidate.

The downstream checkpoint commit validates this binding and records consumption
with its new state. Separate acknowledgement files are reconstructible receipts,
not the authority for whether business processing happened. If an acknowledgement
is lost, delivery finds committed consumption and restores the receipt without
running the kit. No generic exactly-once external side-effect guarantee is made.

Before a delivery execution, the host records an exclusive attempt intent. An
unconsumed attempt is never automatically relaunched, including after a crash.
An operator can inspect a settled candidate and commit it explicitly. Missing or
unconfirmed settlement cannot commit. An interruption after checkpoint publication
can be reconciled from committed history even when the final commit result or
acknowledgement is absent. External effects with unknown outcomes require their
own reconciliation; this profile does not issue external effects.

The existing single-store lock protects publication and reclamation. This slice
does not implement distributed writers, live leases, VM recovery or power-loss
transactions. Stale locks and interrupted intents require operator inspection.

### Retention

Every manifest declares retention explicitly; the standard example uses `keep`.
Explicitly approved reclamation of a
`delivery` product requires every frozen recipient to have committed consumption
and no retained checkpoint to reference it through `state_refs`. All committed
checkpoint history is retained in this slice, so historical references also pin
data. There is deliberately no implicit history pruning or reference expiry.

Reclamation deletes only the separate delivery product content. Its revision
manifest and a tombstone remain. Execution evidence and raw accepted run output
are preserved under their existing policy; reclamation is not secure erasure of
every copy. Instance state and reports remain inspectable and runnable. Unpublished
staging is not automatically collected. Existing settled work-copy cleanup remains
separate and still requires confirmed termination.

The scan is bounded to 256 instances and 1024 revisions per instance; malformed,
unrecognized or excessive entries fail closed. No background garbage collector,
subscription service or domain workflow engine is introduced.

### CLI and acceptance

The manifest is a bounded JSON file, for example:

```json
{
  "schema": "an-checkpoint-manifest-v1",
  "recipients": [],
  "state_refs": [],
  "retention": "keep"
}
```

Recipients are initialized instance UUIDs. Each state reference has `instance`
and `revision` fields identifying a publisher's committed checkpoint. State
references must be declared completely by the spool/operator; the host cannot
discover dependencies hidden inside arbitrary domain data. Approval binds the
declared recovery roots but does not automatically mount referenced files into
the kit. This slice validates their availability and protects retention; richer
state dependency materialization is deferred. Approval also binds the
manifest with the exact candidate and expected state. The file is data, not an
authorization script. Existing `instance-init`, `instance-run` and `instance-show`
remain the initialization, ordinary execution and inspection entry points.

```text
an-agent-host checkpoint-commit STORE INSTANCE EXPECTED RUN MANIFEST [--approve DIGEST]
an-agent-host checkpoint-deliver STORE PUBLISHER REVISION RECIPIENT [--execute]
an-agent-host product-reclaim STORE PUBLISHER REVISION [--approve DIGEST]
```

Execution requires the trusted operator's explicit `--execute`; commit and
reclamation first produce an approval digest. After a delivery run, submit its
settled candidate through `checkpoint-commit`. Call `checkpoint-deliver` again to
reconcile the acknowledgement; this will not execute an already-consumed event.
Missing acknowledgements alone do not prevent reclamation when committed
consumption is available. Once a checkpoint manifest is used, legacy
`instance-commit` refuses to drop its metadata.

```sh
cargo test -p an-agent-host --test file_runtime checkpoint::
```

The registered native fixture produces `count=5`; two independent instances add
it to their domain states, producing 15 and 25. Arithmetic and input interpretation
live entirely in the kit. Tests cover:

- Missing settlement refuses commit and leaves the previous checkpoint unchanged.
- A fault between staging and head publication leaves an invisible orphan. The
  previous state can feed an explicitly authorized new run; the orphan cannot
  deliver events or become a reclamation target.
- A filesystem fault after head publication makes the CLI fail; a new CLI invocation
  reconciles committed consumption instead of reexecuting the kit.
- Removing an acknowledgement file does not change consumption or create a new run.
- An unconsumed attempt is fenced against implicit reexecution.
- Both consumers can finish without retaining state references, allowing explicit
  product reclamation; the upstream continues from state without the old product.
- A retained state reference prevents reclamation even after both acknowledgements
  and after the current state drops its reference, because historical checkpoints
  are still retained. `keep` products remain protected independently.

The injected publication failure models an interrupted result/acknowledgement
path; it is not a power-cut durability test or VM checkpoint experiment. Temporary
execution copies and raw evidence retain their existing, separately documented
cleanup policy.

### Interrupted product reclamation

Reclamation writes intent before deleting product bytes. A later invocation first
checks that retained intent against the committed product reference and hashed
revision manifest, before trying to read the now possibly absent content. This is
a read-only reconciliation path, not a deletion retry. A complete matching receipt
with absent content reports success; absent, partial, or unreadable receipts report
`uncertain` with observed content presence and an operator-reconciliation action.
Contradictory identities or a success receipt with surviving content fail closed.
A result-write failure explicitly reports that deletion may already have happened.
State, manifests, and original evidence remain retained. No automatic receipt repair,
resurrection, or retry is performed. Tests exercise a real receipt-create failure
after deletion and retained crash states around partial write and sync acknowledgement;
they do not claim power-loss durability or prove why missing bytes disappeared.
