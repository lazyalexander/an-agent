# Checkpoints, commit manifests and delivery evidence

## Contract

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

## Delivery and interruption

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

## Retention

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

## CLI and acceptance

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
