# Native storage, private collection and artifact admission

Private input copies can be collected after their round is settled and its work
area has been explicitly collected. A separate operation retires one accepted
output from new revision-based input acquisition. Formal artifact bytes,
checkpoint payloads, release/kit files and execution/control evidence remain
retained. Neither operation implements formal artifact GC or automatic cleanup.

```text
an-agent-host spool-storage INSTANCE
an-agent-host spool-reclaim-snapshots INSTANCE RUN [--approve HASH]
```

The storage query takes no execution lock. It reports logical file lengths for
release, kit, state, per-run work/snapshots/artifacts and the whole instance,
plus observed filesystem headroom. Symlinks are not followed. Marker presence
and cleanup candidates are observations, not deletion authority. The bounded
walk may race ongoing writes and is not an atomic filesystem snapshot. Logical
bytes are not physical savings: APFS clones and open file descriptors can share
or retain blocks after an unlink.

## Independent lifetimes

- Release and selected kit bytes are execution inputs, retained independently.
- Checkpoint state is an explicitly committed object, not an input snapshot.
- Formally accepted artifacts remain fixed-version publications for downstream
  checkout. This command cannot delete them or treat a missing reader as consent.
- Work scratch uses the existing approved, termination-confirmed collection.
- Input snapshots retain the exact code/input bytes for short-lived diagnosis.
  Their bodies may then expire; plans, hashes, mappings and receipts remain.

Input snapshot paths are private evidence, not durable artifact handles. A
downstream consumer uses formal artifact checkout or an explicitly selected
checkpoint. Reclaiming snapshots abandons byte-for-byte replay from those private
copies; it does not delete the independent registered state or formal outputs.
Opaque domain references are not inferred by scanning state text. Formal artifact
deletion requires its own explicit reference/delivery contract. Closing new input
admission is a separate operation described below.

## Collection evidence

Collection holds the owning instance and workspace locks, verifies the native
ownership binding and the existing archived work-collection evidence, and checks
the retained plan/receipt/execution mapping. Successful outputs require complete
formal acceptance. Registered state/artifact input roots and the current native
checkpoint must remain independently readable.

Only declared input filenames and their directory ancestors are eligible.
Complete execution snapshots require matching payload hashes. Positively
unstarted preparation failures may have an incomplete set or partial file bytes;
their durable no-spawn receipt authorizes inspection and exact approved cleanup,
never execution or publication. Unregistered files, links, unknown termination,
changed complete inputs, missing roots and foreign ownership refuse.

The preview binds exact inventory and receipt/mapping identity. An exclusive
intent precedes deletion. A complete receipt supports read-only confirmation;
an incomplete deletion or lost receipt returns `uncertain` without retrying
deletion, recreating data or manufacturing a completion record. Existing garbage
must be inspected explicitly. No recursive removal of arbitrary host paths is
provided. Inspection, accepted artifact discovery and checkpoint continuation
still work after successful collection, including from a suspended instance.

## Admission pressure

Workspace preparation observes available filesystem bytes before copying inputs
or starting a process. The default minimum is 128 MiB plus the full logical input
size, even if APFS can clone those inputs cheaply. An operator-owned workspace
profile can raise `minimum_free_bytes`; setting it lower cannot bypass the floor.
The effective minimum is part of the approved plan. Current free space is an
execution precondition, not a volatile value inside the approval hash.

Insufficient headroom produces retained positive no-spawn evidence and zero
automatic retries. This is admission backpressure, not an atomic reservation,
runtime quota or guarantee against concurrent writers filling the volume.
The existing supervised execution and file-permission boundaries are unchanged.

Qualification includes a real 16 MiB input, retained products/state after input
collection, fixed downstream handoff, suspension/resume, confirmed cancellation,
changed/unregistered-input refusal, partial no-spawn preparation and lost receipt
reconciliation. A real headroom denial uses an explicitly raised operator request;
tests do not fill the user's disk or replace filesystem observations with mocks.

## Finite private-material maintenance

```text
an-agent-host spool-storage-collect INSTANCE REQUEST_JSON [--approve HASH]
```

The strict request is `{"schema":"an-native-private-collection-request-v1",
"material":"work","runs":["round-0","round-1"]}`. Select either `work` or
`snapshots`, with 1..16 unique explicit run names. These are separate phases:
collect work first, then preview/approve snapshots. No wildcard, age-based scan,
formal product/state selector or recursive host-path removal is accepted.

The host validates every selected original per-run preview before any deletion.
One approval binds the material, instance/release identity, order and all exact
child inventories/evidence. Each original collection operation revalidates under
its existing locks and writes its own durable one-shot intent and receipt. No
new collector, deletion policy or second retention ledger is introduced. A
changed owner/selection/inventory cannot inherit an old approval. An invalid,
already-collected or uncertain item refuses the selection rather than being
silently skipped.

This is not an atomic all-or-nothing transaction. Cancellation stops before the
next item; an admitted child operation settles using its original protocol.
An error or unconfirmed child result stops subsequent items without rollback,
auto-retry or auto-resume. If the client response is lost, inspect the existing
per-run collection evidence. Do not resend a batch to infer that remaining items
are safe: uncertain original intents remain fenced. Known unstarted remaining
items need a new explicit selection and approval.

Formal artifacts, checkpoint state, release/kit files, execution evidence and
external input files remain retained. Logical bytes removed are not physical
space savings on APFS; this entry point makes no runtime-quota or disk-reservation
claim. Real macOS qualification runs four rounds with an 8 MiB external input,
starts maintenance while an independent downstream receiver runs, measures the
producer's logical footprint, and confirms fixed products and checkpoint history
after collection. It also qualifies maintenance cancellation before deletion,
whole-selection refusal, confirmed execution cancellation and a lost collection
receipt without further deletion. This is controlled receipt-loss injection,
not arbitrary crash or power-loss recovery.

## Retiring new artifact input admission

```text
an-agent-host spool-artifact-retire INSTANCE RUN OUTPUT [--approve HASH]
```

The host verifies the owning instance, successful whole-attempt settlement,
formal acceptance and exact declared output. Preview binds its immutable catalog,
revision, content identity and acceptance evidence; approval writes one exclusive
`admission-retired.json` closure record beside that revision. No bytes, manifests,
accepted-output bindings or historical event identities are rewritten or deleted.
Other outputs and newer revisions remain independent. Committed checkpoint
catalogs cannot be retired through this command.

New revision-based checkout/edit, native business-input selection, workspace
artifact input preparation and registered native revision execution refuse the
closed version. New event publication refuses it; an existing event can still be
read back. Unstarted direct/delivery/batch previews cannot bypass the fence with
an old approval. Pending inventory reports unstarted deliveries as blocked.

Retirement writes use the existing catalog lock and exclusive marker creation.
Input readers stay concurrent: immutable bytes are checked with the monotonic
closure fence before and after acquisition. If closure precedes the final check,
the acquisition refuses; otherwise its bytes were acquired before closure and
may finish. Workspace preparation rechecks while acquiring its private input
snapshot. A concurrent closure can therefore refuse preparation after execution
intent was written; existing no-spawn/uncertain evidence governs reconciliation,
without automatic retry. Retirement does not promise that every previously
approved process has already started or stopped.

Evidence reads remain separate from new input acquisition. Already-admitted
output acceptance, state commit, causal completion and private collection can
still verify retained bytes. A checkpoint committed from a retired accepted
output is an independent version, not a lease on disposable scratch. A result
fact needed to confirm an admitted delivery may still be recorded after its
output admission was closed; it does not reopen input admission.

Repeating a complete matching closure confirms it without rewriting. A torn,
malformed, linked or contradictory marker closes new admission and requires
inspection; it is never overwritten as a retry. This profile has no reopen or
delete operation. `spool-artifacts` reports admission separately from its stable
accepted-output inventory. Missing/corrupt payloads still fail evidence checks.

This is cooperating-host policy, not access revocation or a sandbox boundary.
It does not revoke independent checkouts, copied or raw-file inputs, caches or
an older executable that does not implement this gate. Use the qualified host
revision for all managed operations; mixed host versions are unqualified. The
original artifact revision schemas and release formats remain unchanged.

Real headless qualification runs one receiver during producer retirement,
refuses a second receiver's stale direct/event/batch approvals, completes custody
and state commit after closure, and verifies independent checkpoints, unrelated
outputs, cancellation, retained bytes and torn-record refusal. No formal payload
is collected and no model is invoked.

## Native input ownership and explicit release

```text
an-agent-host spool-input-references INSTANCE RUN
an-agent-host spool-input-release INSTANCE RUN [--approve HASH]
an-agent-host spool-input-release INSTANCE RUN --delivery WORKSPACE SUBSCRIPTION [--approve HASH]
```

New approved native starts register each managed artifact/checkpoint input at its
source revision before execution. Preview does not acquire references. A claim
binds the consumer root, instance, release, unique run, exact start-plan digest,
input slot and full source binding. Two roles using the same revision have distinct
claims. Raw-file inputs do not acquire source claims. Historical runs remain
readable; an `unregistered` observation is not proof that no consumer exists.

The consumer writes a durable registration intent, then acquires each source claim
under the existing catalog write lock and finally confirms registration. Native
starts have at most two managed input slots. Acquisition refuses when a revision
already has 512 history entries, counting claims and release records; existing
owners can still record releases, so retained history can grow to 1,024 entries.
No history is pruned to make room. Only one source lock is held at a time, inside the
consumer's existing instance operation. Read-only inspection takes neither lock;
it cannot authorize deletion and may fail if it captures incomplete publication.
Catalog write contention remains an explicit refusal, without retry or queuing.

Registration is not an atomic transaction across catalogs. Partial acquisition
retains the claims already written and refuses execution/reuse of that run identity.
It does not automatically complete missing claims or roll back ownership evidence.
Source admission closure can still refuse preparation after registration; retained
claims are conservative evidence, not a promise that the process started.

The original release command supports **confirmed cancelled runs only**, after
both work collection and private input snapshot collection are positively confirmed.
It reuses original settlement and collection checks, then binds an explicit approval
to those records and the complete registration. An exclusive intent precedes
source-side release markers. A matching complete result can be confirmed without
rewriting; a missing completion, partial release, changed owner or unknown settlement
refuses without repair or replay. A source release never releases another run's claim.

The `--delivery` form supports successful **stateless event consumers**. It requires
confirmed whole-attempt success, accepted outputs, the original matching delivery
acknowledgement and causal result event, plus confirmed work and snapshot collection.
Its separate `an-native-success-input-release-v1` approval binds that consumption
receipt; the original cancelled-run plan and CLI remain compatible. An execution
exit or an accepted output alone cannot release the claim. A missing acknowledgement
refuses rather than invoking acknowledgement repair. Release is explicit, not a
side effect of collection or observation.

Without `--coverage`, the successful release path requires exactly one managed event input and no explicit
state/data selection, checkpoint store or pending checkpoint initialization. Stateful runs retain their inputs until an operator separately approves exhaustive
coverage for their specific committed checkpoint. Unstarted or partially registered failures also stay retained pending
a separate reconciliation contract. The command deletes **no payload** and grants
no collection authority. A source marker without the matching final consumer
receipt remains uncertain; release cannot reconstruct that receipt or replay work.
Existing artifact/release formats are unchanged. Mixed old/new hosts, unmanaged
readers, opaque checkpoint dependencies and legacy catalogs prevent treating these
claims as a complete reference census. Do not infer GC permission from zero claims
or from release records alone.

A successful event consuming business data and a parent checkpoint can use:

```text
spool-input-release INSTANCE RUN --delivery WORKSPACE SUBSCRIPTION --coverage FILE [--approve HASH]
```

The spool must declare and emit the text output `checkpoint-input-policy.json`
in the **same attempt** as its state output. The qualified policy names the two
managed roles and exhaustively describes direct checkpoint dependencies within
that inventory. For example, a counter retaining its batch for audit emits:

```json
{"schema":"an-native-checkpoint-managed-input-policy-v1","state_output":"state.txt","data":"checkpoint-pin","parent_state":"not-required"}
```

The host reads the immutable accepted policy revision through the delivery receipt.
An operator cannot substitute a later file for this output. All pins in the
checkpoint declaration must refer to these managed inputs. A missing or contrary
policy refuses release, even when the operator statement and positive pins agree.

`FILE` attests under `schema: an-checkpoint-exhaustive-input-coverage-v1` that the
named checkpoint declaration exhausts the new checkpoint's direct retention needs.
It supplies the exact `checkpoint`, `declaration_sha256` and ordered `inputs`.
Each input names its original `catalog`, `revision` and explicit `disposition`:
`checkpoint-pin` or `not-required`. Preview is read-only. Approval binds this
attestation, the original registered inputs, commit and consumption evidence.

The host verifies the committed ancestor was produced by this run/attempt from
this parent, rechecks its commit intent and independent checkpoint pins, and
requires an exact classification of **both** registered inputs. A retained input
must have a matching pin. A `not-required` decision conflicting with a pin is
refused. The spool author supplies the semantic completeness judgment in its accepted policy
for opaque state. The host verifies recorded identities and exhaustiveness over
this attempt's managed reads. Undeclared external dependencies cannot be proven
absent from opaque bytes. The attestation conveys no payload deletion authority.

In the counter fixture, A is the accepted batch retained as provenance for the
new total, and B is the prior subtotal checkpoint. B's contribution is materialized
in the new total. After consumption acknowledgement and private collection, both
attempt claims release; the independent new checkpoint pin still protects A.
A mismatched or missing input, commitment, pin, acknowledgement or release
completion refuses. Unknown work is never replayed. Existing parent checkpoint
history and other owners retain their products independently.

Real headless acceptance uses one producer, a successful consumer and a separate
cancelled consumer. It qualifies live/unknown-owner refusal, work/snapshot ordering,
explicit release, unrelated retained claims, partial acquisition, lost completion
and unchanged source payloads. The host owns this fault matrix; Loom only checks
the packaged producer/consumer handoff. No model or formal payload deletion runs.
The event case also qualifies successful stateless release with a second retained
consumer, missing acknowledgement/result/release receipts, stale approval and partial
state initialization. The state case requires explicit coverage for successful stateful release. Unsupported isolation/runtime/network requirements refuse before
native loading; no backend downgrade is used to make these cases pass.

## Observing a product's registered consumers

```text
an-agent-host spool-artifacts INSTANCE RUN --references OUTPUT
```

This optional view selects one formally accepted output and inspects its exact
source revision's native input claims. It takes no instance, catalog or consumer
control lock and does not open paths from consumer ownership records. The original
`spool-artifacts INSTANCE RUN` report and stored acceptance identities are unchanged.

Each entry distinguishes `claim_without_source_release` from
`source_release_recorded`. A source marker alone does not prove that the consumer
completed its final release receipt. Partial registration, successful owners and
unknown outcomes remain visible; this query neither reconciles nor releases them.
It reports `consumer_receipts_checked: false`, `reference_census_complete: false`
and `collection_authorized: false`, including when the directory is absent or empty.
Explicit checkpoint pins now appear in this inventory and cannot use attempt-release
markers. Old hosts, raw-file readers and undeclared opaque state references remain
outside it. See [checkpoint dependencies](native-state.md#explicit-checkpoint-artifact-dependencies).

The query checks source binding, content-addressed claim filenames and paired
release markers. Orphan, malformed, foreign, linked or oversized records refuse
without repair. Inspection is bounded to 1,024 directory entries, 64 KiB per record
and 8 MiB of compact JSON claim data. It is not an atomic snapshot: concurrent
publication may be absent from the view or cause a refusal. The view cannot be
used as a deletion precondition. No background polling, retry or payload deletion
is introduced. Owner paths are private operational evidence, not publication data.

Real acceptance extends the existing producer/two-consumer case: incoming claims
are observable while the consumer runs and source control locks are held; partial
registration remains visible; confirmed cancellation creates a source marker while
another consumer stays retained. Removing the consumer completion receipt does not
turn that marker into a completed-release guarantee. Malformed source evidence
refuses, and all original payload hashes remain unchanged.

## Remaining formal payload collection contract

The generic instance checkpoint/delivery ledger already has explicit retention,
recipient acknowledgements and registered state-reference checks. Native outputs
also live in per-run artifact catalogs and can be selected from independently
located instances. That usage is not a closed global reference inventory. Do not
apply generic `product-reclaim` to a native catalog or infer deletion consent from
observing no current reader or receiving an event acknowledgement.

Closing admission does not prove that existing consumers, state references or
unmanaged readers have released a version. Native input claims now record new
managed consumers; formal collection still needs successful-owner release, explicit
checkpoint dependency retirement/reconciliation and a defined boundary for unmanaged/legacy readers.
Opaque state references cannot be inferred by scanning text.
The current operations reduce private-copy accumulation and close selected new
uses while preserving formal bytes. Loom may describe intended retention, but
cannot supply runtime deletion authority. General native formal-product/state GC
remains unimplemented.
