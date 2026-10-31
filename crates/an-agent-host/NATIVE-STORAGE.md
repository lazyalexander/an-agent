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

Retirement and revision byte acquisition share the existing catalog lock. An
acquisition ordered before closure may retain its bytes and finish; closure does
not cancel it. Workspace preparation rechecks while acquiring its private input
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

## Remaining formal payload collection contract

The generic instance checkpoint/delivery ledger already has explicit retention,
recipient acknowledgements and registered state-reference checks. Native outputs
also live in per-run artifact catalogs and can be selected from independently
located instances. That usage is not a closed global reference inventory. Do not
apply generic `product-reclaim` to a native catalog or infer deletion consent from
observing no current reader or receiving an event acknowledgement.

Closing admission does not prove that existing consumers, state references or
unmanaged readers have released a version. Native payload collection still needs
explicit host-owned reference acquisition/release and a defined boundary for
unmanaged readers. Opaque state references cannot be inferred by scanning text.
The current operations reduce private-copy accumulation and close selected new
uses while preserving formal bytes. Loom may describe intended retention, but
cannot supply runtime deletion authority. General native formal-product/state GC
remains unimplemented.
