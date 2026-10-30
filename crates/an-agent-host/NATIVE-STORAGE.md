# Native storage observation and private snapshot retention

This slice collects private input copies after their round is settled and its
work area has been explicitly collected. Formal artifacts, checkpoint payloads,
release/kit files and execution/control evidence remain retained. It is not
general artifact GC, a cross-instance lease manager or automatic disk cleanup.

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
Opaque domain references are not inferred by scanning state text. General formal
artifact retirement requires its own explicit reference/delivery contract; it is
not enabled by this slice.

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
