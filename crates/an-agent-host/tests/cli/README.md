# Host CLI acceptance

Build the selected host binary once, then pass its explicit path. No runner
builds a binary, reads model credentials or invokes Loom. Python 3 standard
library is sufficient. Platform eligibility is centralized in
`host_qualification.py`; see the [support and qualification matrix](../../../../doc/platforms.md).
This prerequisite does not replace the host enforcement exercised by each case.

```sh
cargo build --locked -p an-agent-host --bin an-agent-host
python3 crates/an-agent-host/tests/cli/native_state.py \
  --host target/debug/an-agent-host --evidence /private/tmp/native-state-evidence
```

`native_state.py` requires the qualified native workspace backend; other platforms report
a skip, not successful runtime qualification. It tests explicit initialization,
fixed state input, successful accepted-output commit, stale conflicts, cancelled
candidate refusal, interrupted HEAD publication, orphan refusal, idempotent
commit confirmation, scratch reclamation and a fresh instance seeded explicitly
from a fixed checkpoint. The counter rules belong to this fixture, not the host.

The round-control scenario also qualifies explicit pause/resume, checkpoint-bound
suspension, stale approvals/proposals, lost-head fencing, interrupted control
publication, changed-kit/state refusal, an independent second instance and
scratch reclamation while suspended. Live execution and unknown start evidence
refuse control; the live test requires confirmed cancellation before proceeding.
The safe point is a completed, formally accepted round, not in-process signaling.
Optional `--legacy-host /path/to/pre-control/an-agent-host` verifies that an older
host reads the legacy instance before activation and refuses the upgraded instance
after pause and resume. Build each selected binary once and use stable copies;
never rebuild a shared executable while an external suite is invoking it.

Storage coverage uses a real 16 MiB selected input, verifies logical snapshot
removal with unchanged formal artifacts/state, continues after suspension and
feeds a fixed report into another instance. Unknown entries/changed inputs and
lost collection receipts cannot authorize new deletion. A raised operator
headroom request exercises the real no-spawn denial path without filling disk.

`native_release.py --host HOST --evidence NEW_DIRECTORY` uses real qualified native execution
to qualify read-only release comparison, same-label changed content, unsupported
capability/dependency refusal, separate instance/state initialization, stale
approval refusal, failed candidate output rejection and original-instance
continuation. It adds no in-place updater or automatic migration. The host owns
this failure matrix; Loom separately tests its actual exported revisions.

`native_events.py --host HOST --evidence NEW_DIRECTORY` qualifies real v3 artifact
event publication, separate subscription/start approvals, pinned old-version input,
explicit accepted-output consumption, duplicate delivery, missing confirmation,
stale/revoked/foreign identities, concurrent observation, actual cancellation and
lost facade evidence. Missing-record cases deliberately remove records after
known execution; they do not claim arbitrary process-crash or power-loss recovery.
It also qualifies explicit stateless-success reference release, a retained peer,
missing result/acknowledgement/release receipts, partial state initialization and
unsupported isolation requests. Stateful-success claims stay retained in
`native_data_state.py`; no payload deletion is qualified.
No model, background dispatch or GUI is exercised. The paired Loom suite owns
actual archive/export handoff; this host suite owns the refusal/fault matrix.

`native_binary.py` uses the same `--host` and `--evidence` arguments. It qualifies
the Python/binary contract, dependency changes before start and acceptance,
read-only runtime roots, fixed byte-event consumption, actual cancellation and
retained products after the selected runtime disappears. Only Python's standard
library is needed by this host-owned refusal suite. The separate Loom document
suite installs its example dependencies and validates editable XLSX/PPTX.

`native_data_state.py` uses the same --host/--evidence arguments. It qualifies
separate fixed business/checkpoint inputs through direct and event starts,
stale commits after scratch collection, unknown-delivery fencing and changed
state refusal on an existing delivery. Real cancellation refuses collection
before termination confirmation, then preserves formal data/state/products while
collecting private files. A fresh Python instance explicitly seeded from retained
state verifies the second qualified runtime. Business arithmetic is a test fixture.

Each command has a bounded wait and records stdout/stderr and its exit status.
On a client deadline the runner requests host cancellation; unknown termination
fails the suite and keeps evidence. It never automatically reruns an execution.
All fixture directories, receipts and command observations are retained, including
on failure. Operational evidence contains local paths; do not publish it raw.

`native_input_references.py --host HOST --fixture COMPLETED_DATA_STATE_DIRECTORY
--evidence NEW_DIRECTORY` consumes a fresh successful `native_data_state.py` case.
It inspects successful-owner references, starts and cancels an independent real
consumer, and releases only after confirmed work and snapshot collection. It
checks unknown settlement, wrong approval, another retained consumer, unchanged
payload hashes and missing release confirmation without repair. An injected second
catalog lock proves partial registration cannot execute or silently reacquire.
The producer-side optional reference view is checked under held producer/catalog
locks while a real consumer runs. It distinguishes source markers from consumer
completion, includes partial registrations, and refuses malformed source records.
These fault probes are deliberate refusal checks, not production retry policy.
The fixture's producer is retired during the case; do not reuse it for another run.
This covers new managed inputs, not a complete pin census or formal product GC.

The event suite also qualifies read-only delivery inventory and foreground
monitoring during actual execution, evidence changes, observation cancellation,
deadline exit and restart without replaying unknown work. The monitor emits
NDJSON, whereas ordinary host commands return one JSON value.

`native_dispatch.py --host HOST --evidence NEW_DIRECTORY` qualifies finite
approved stateful headless dispatch. It covers exact inputs, no implicit state
chaining, stale commits, drain-stop versus cancellation, confirmed-only cleanup,
actual dispatch-ledger write failure and interrupted observation without replay.

The dispatch suite also qualifies subscription admission pause/resume, old direct
and batch approval refusal, running-work custody and no replay after resume.
Optionally pass `--legacy-host OLD_HOST` to invoke a previous v1-only binary and
verify upgrade refusal. Its summary distinguishes whether this extra check ran.

The dispatch suite also loses a known completed summary, checks inert/wrong
approval and missing-entry refusal, reconstructs it explicitly and checks unchanged
attempt counts. Partial stopped batches cannot use this reconciliation path.

`native_route_successor.py --host HOST --evidence NEW_DIRECTORY` runs actual old
and new stateful receivers, including cutover while old work runs, historical and
uncertain-event refusal, changed-approval refusal, explicit state transfer and
retained custody. `--legacy-host OLD_HOST` optionally qualifies old-host refusal;
the portable summary records whether it ran and the selected binary hash.

`native_capacity.py --host HOST --evidence NEW_DIRECTORY` runs four 8 MiB rounds
through two real headless instances, measures producer logical bytes before/after
finite private collection, and verifies concurrent consumer progress, cancellation,
retained products/state/external inputs and lost-receipt refusal. It reports no
physical APFS savings or formal-product GC guarantee.

`native_retirement.py --host HOST --evidence NEW_DIRECTORY` retires an accepted
output while one real stateful receiver runs, rejects another receiver's stale
direct/event/batch approvals, and completes acceptance, state commit and causal
confirmation after closure. It checks unrelated/new versions, private collection,
confirmed cancellation, preserved bytes and a torn retirement record without
retry. This qualifies admission closure, not formal payload deletion or revocation
of independent copies. Unit tests own concurrent-reader and marker-fault checks.

`native_monitor.py` consumes a freshly completed `native_events.py` fixture and
creates independent measured workspaces. It adds six real producer rounds, then
runs 144 unique revocations under 0/1/4 observers with no retries. Run it once per
fresh fixture; a failed/unknown scenario must not reuse the same round IDs. It
records every command, watcher outcome and opt-in timing, and rejects observation
writes, control refusal or watcher failure. It does not weaken stale-approval,
cancellation or reclamation assertions in the preceding real event suite.

`native_sessions.py --host HOST --evidence NEW_DIRECTORY` qualifies session-owned
finite threads using real pinned-input/state consumers. Explicit admissions overlap
in execution; cancellation affects one session, pause drains, stale requests fail,
closure preserves products and a lost completion receipt prevents closure/replay.
Simultaneous same-workspace admission remains fail-fast, not queued or retried.

`native_material_boundary.py --host HOST --evidence NEW_DIRECTORY` starts two
real native spools. A path supplied as ordinary input does not grant access to
another instance's output/identity or the workspace event log: content reads and
writes must fail. A private instruction note remains scratch, creates no delivery,
and is excluded from accepted products. Metadata visibility is measured, not
claimed secret. No model runs; this does not qualify context ingestion, general
request/reply IPC or scheduler fairness. See the runtime acceptance matrix in
[the session contract](../../../../doc/session-and-retention.md).

`native_peer.py --host HOST --evidence NEW_DIRECTORY` runs a split-phase
request/reply/continuation through actual native spools. Waiting creates no process
or held instance lock; an independent sender job can overlap receiver execution.
It checks fixed input/reply pins, stale generations, wrong correlation, cancellation,
missing completion without replay and confirmed-only collection. Revocation closes
future delivery, while the receiver's supervisor owns termination. This is not
live IPC, a fairness benchmark or a background scheduler.

`native_supervisor_loss.py --host HOST --evidence NEW_DIRECTORY` contrasts
SIGTERM with SIGKILL of the actual launching CLI. Two finite workers make orphan
execution observable after the original deadline. It checks original group
settlement for handled cancellation, conservative custody after owner loss,
independent-instance progress and preserved formal products. The disposable
fixture then removes stale lock markers after sampled process exit to verify
that missing termination evidence independently blocks acceptance, collection,
unload and replay. That injection is a test operation, never a recovery recipe.
Its summary distinguishes passing refusal checks from the observed missing
caller-independent supervisor. Local process samples carry no cleanup authority.

`native_supervisor.py` and `native_supervisor_state.py` take `--host HOST
--evidence NEW_DIRECTORY`. They qualify file-input and fixed-checkpoint supervision:
client exit, continuing timeout/cancellation, supervisor loss, no replay and
confirmed-only collection. The state suite advances HEAD before selecting the
old revision and verifies retained checkpoints. PIDs serve fault injection and
observation only; accepted products and independent instances remain unaffected.
