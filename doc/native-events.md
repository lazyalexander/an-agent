# Native workspace artifact events

Status: bounded, explicit headless delivery under the
[qualified native workspace backend](platforms.md). An accepted output can become
a workspace fact, activate a separately approved receiver, and produce a causal
result event after formal acceptance. This is not a business graph engine,
background listener, low-latency channel or arbitrary agent sandbox.

Business branches, loops, delegation choices and result interpretation remain in
silk/kit. The host routes facts and enforces identity, authority and custody.
Events and subscriptions do not create execution permission. A subagent is a role,
not a requirement to create another spool for every tool call. Shuttles remain
separately authorized bridges; this slice does not claim GUI execution.

## Release and workspace contracts

Native v3 retains v2 requirements and adds a required strict `events` object:

```json
{"consumes":["artifact.accepted"],"produces":["artifact.accepted"]}
```

Each array may be empty or contain that one supported event name. Unknown names,
duplicates and unknown fields refuse. V1/v2 cannot carry this object; their bytes
and ordinary behavior remain compatible. Older hosts reject v3. Roles describe
interfaces, not peer access, subscriptions, model services or network grants.
The reviewed macOS policy and one-input profile apply. Optional v3
[binary products and operator-selected Python](native-spool.md) use the same
delivery/confirmation protocol. V1/v2 and existing text packages are unchanged.
The local control store remains trusted against same-user tampering. Hashes bind
bytes and identities, not publisher authentication or a remote trust protocol.

An event workspace is a host-owned private directory, separate from instances.
`facts/` uses core `Workspace` configuration snapshots and append-only event log;
there is no new log engine or runtime dependency on Loom. Native subscription
bindings adapt the existing frozen-recipient/intent/confirmation discipline to
the native ABI rather than impersonating trusted in-process `SpoolBeat` bodies
or treating legacy file-instance receipts as interchangeable.

```text
spool-events-init NEW_WORKSPACE
spool-events-show WORKSPACE
spool-event-subscribe WORKSPACE NAME ROUTE_JSON [--approve HASH]
spool-event-publish WORKSPACE INSTANCE RUN OUTPUT [--approve HASH]
spool-event-deliver WORKSPACE EVENT_DIGEST NAME [--approve HASH]
spool-accept TARGET DELIVERY_RUN [--approve HASH]
spool-event-complete WORKSPACE EVENT_DIGEST NAME [--approve HASH]
spool-event-revoke WORKSPACE NAME [--approve HASH]
```

All commands use `an-agent-host`. Init creates no execution grant. Mutations
preview exact plans before approval. Delivery previews may prepare a bounded
native profile but create neither a delivery intent nor an execution attempt.
For a v3 release declaring a separate state input, delivery additionally requires
`--state REVISION`. The [data/state contract](native-state.md#independent-business-data-and-state)
pins it independently of the event's business artifact. The delivery identity
does not change with state, so another selection cannot force a second attempt.
The host-facing foreground caller chooses which event/subscription to dispatch;
there is no automatic discovery, global cursor, recursive drain or daemon.

The operator-owned route JSON has strict schema `an-native-artifact-route-v1`,
source and target instance paths in `source`/`target`, declared source output
`output`, declared receiver output `result`, and `wall_seconds`. The host binds
exact instance/release identities and receiver control generation. Source must
declare production; receiver must declare consumption and result production.
Instances must be distinct nonnested directories. Subscriptions are static and
immutable: a name cannot be reused for a different binding. No predicates,
scripts, branches or loops are accepted in routing configuration.

## Delivery and consumption evidence

Publishing verifies native termination, complete acceptance and exact retained
artifact bytes/provenance. Raw filenames, failed runs and exit status alone
cannot publish facts. Events carry metadata and fixed references, not full files,
prompts or agent transcripts. The stable event digest binds workspace, producer,
run, output revision and optional causal delivery. Repeated publication confirms
the same fact without appending another event. Torn or duplicate log records
are not silently repaired.

A delivery is identified by workspace, exact event and immutable subscription.
Different explicitly approved subscriptions are distinct deliveries. An exclusive
intent binds the native start plan before launch, under the receiver operation
lock and workspace admission lock. The workspace lock is released before process
execution; other workspace operations remain available. Actual input snapshots
bind the artifact revision through the existing native supervisor and custody.
Edited kits, changed generations and revoked subscriptions invalidate admission.

The intent is the admission linearization point. Revocation after admission does
not claim an already admitted process has stopped; use the running CLI's existing
SIGINT/SIGTERM cancellation. Revocation fences future admissions and new automatic
consumption confirmation. This is not an instantaneous process-spawn revocation
guarantee. Unload/reload changes instance identity, never inherits the route.

Receiving an event and a successful exit are not consumption. After explicit
`spool-accept`, completion validates the exact event-bound start, accepted outputs
and their provenance, publishes the declared result event, then records an
acknowledgement. It commits no domain state and invents no business outcome.
A result event retains its incoming delivery identity for causal diagnosis.
Pause/resume generations fence new starts, not settlement of already admitted
accepted work. Completion remains available while admission is paused or the
instance is unloaded, provided its identity/evidence and subscription remain valid.

Repeated delivery never launches once intent exists. It confirms complete
consumption or reports uncertainty with a separate run observation. Lost
acknowledgements can be explicitly reconstructed from verified accepted outputs
and an already recorded result event, without execution or republication. Missing
start/result/acceptance or partial intents remain fenced. Use existing explicit
native reconciliation where evidence allows; no command automatically retries
unknown work, overwrites contradictory receipts or clears stale locks.

Source and result artifacts, release bytes and execution evidence must remain
retained. Work and private input snapshots can be explicitly collected after the
existing settlement gates; confirming consumption still works afterward. This
does not enable formal artifact GC or infer references from opaque state.

## Scope and qualification

The first workspace permits 512 log records, 16 static subscriptions and 128
delivery slots; native
instance/run and text-artifact limits remain unchanged; explicitly declared byte
artifacts retain the existing 8 MiB bound. This is bounded local
backpressure, not a scalable broker or disk quota. Future channels need separate
grants, lifecycle fencing and measured backpressure; raw high-frequency samples
must not be forced through this fsync-backed event log or tape.

Host Rust tests cover interface compatibility and log fault windows. The real
macOS Python acceptance covers two-silk releases, fixed old inputs after newer
publication, absent/wrong approvals, stale/revoked/foreign identities, duplicate
delivery, lost confirmation, cancellation and missing facade results. Missing
record tests inject evidence loss after known execution, not arbitrary crash or
power-loss recovery. Loom separately qualifies actual v3 packaging and handoff.
No model, GUI, new backend or business workflow interpreter is introduced.

## Delivery inventory and foreground monitoring

`spool-events-pending WORKSPACE` lists matching event/subscription pairs without
preparing runs. Rows retain exact workspace/event/subscription/delivery identities
and admission refusals. Phases distinguish `awaiting-authorization`, `blocked`,
`running-or-unconfirmed`, `settled-unsuccessful`, `awaiting-acceptance`,
`awaiting-confirmation`, `needs-reconciliation` and verified `consumed`.
Unconfirmed termination is deliberately not labelled simply running. An accepted
output without an acknowledgement is not consumed, including after lost-ack
injection. A revoked route can still have an admitted or consumed delivery;
admission refusal and delivery evidence are separate fields.

`spool-events-watch WORKSPACE SECONDS INTERVAL_MS` polls that inventory in the
foreground and emits newline-delimited JSON: an initial snapshot, changed
snapshots, then an `an-native-event-watch-stopped-v1` summary. Bounds are 1..3600
seconds and 100..5000 milliseconds. Stdout is flushed after each snapshot. SIGINT
or SIGTERM stops observation with reason `cancelled`; the deadline stops with
reason `deadline`. Neither stops an independently running delivery. Missing or
invalid evidence and output failures end the command with an error; they are not
silently retried. Restarting the monitor reads durable evidence again and never
reexecutes work. There is no persistent cursor or promise to emit every transient
phase between polls. Existing workspace/delivery bounds still apply; the monitor
is for bounded local operation, not a high-frequency event channel.

Both commands are observational: they accept no approval, select no state,
create no delivery intent, acknowledge nothing, collect nothing and grant no
execution. Use the existing explicit delivery preview/approval for admission,
`--state REVISION` where required, explicit acceptance and completion for custody,
and existing subscription revocation for new-admission control. No automatic
state selection, daemon, batch authorization or business graph is introduced.
A monitor failure is not permission to reclaim, and an observation is not an
authoritative transaction across concurrently changing instance evidence.

Observation uses a strict, bounded in-memory copy of the workspace log. Pending,
show and watch do not acquire `host.lock`, initialize workspace files, repair the
environment projection or sync writes. All route/control/history checks within
one observation use that fixed log. The additive `observation` field includes its
hash and `log_unchanged_at_end`: false means the log or workspace metadata changed
during the scan. It does not assert external instance/revocation evidence was
atomic even when true. Changed views grant no action; mutation APIs still acquire
the original locks and revalidate exact authority/currentness. A torn or malformed
log fails closed without emitting a fabricated empty/complete view or retrying.

The real event CLI acceptance qualifies pending/accepted/consumed and blocked
rows, monitoring while an actual headless delivery holds the instance lock,
changed evidence after revocation, observation cancellation without execution
cancellation, deadline exit, and monitor restart with missing result evidence
without creating another attempt. Rust tests cover torn logs and output faults,
frozen-view currentness, read-only handles and observation during a held writer
lock. `native_monitor.py` then runs 144 distinct approved revocations with 0/1/4
observers across 2/8 events and 12 routes. It checks every mutation and observer
outcome, projection identity and absence of observer lock/write timing events.
No failed command is retried and the original event-test timing is unchanged.

On the local debug host, the original matrix refused 48/144 controls and 14/20
observers exited nonzero. The revised matrix completed 144/144 controls and all
20 observers; control medians across cells were 24.79–26.20 ms. Single-observer
original scans held the control lock for median 199.86/860.94 ms (2/8 events);
new observers never acquire it. These are small synthetic, warm local samples
with diagnostic overhead, not a latency SLA or production failure probability.
The original four-observer/8-event cell had no control failures because all its
observers died; observer survival is a separate acceptance condition.

## Finite approved dispatch

`spool-events-dispatch WORKSPACE REQUEST_JSON [--approve HASH]` selects a finite
list of existing artifact events. It does not listen for future events or follow
result events. The strict request is:

```json
{
  "schema": "an-native-event-dispatch-request-v1",
  "name": "reviewed-batch",
  "entries": [
    {
      "event": "FULL_EVENT_DIGEST",
      "subscription": "counter",
      "state_revision": "FULL_COMMITTED_STATE_DIGEST"
    }
  ]
}
```

Select 1..16 unique event/subscription pairs; omit `state_revision` for stateless
entries. A different state cannot create a second delivery identity. Preview
prepares the existing bounded run profiles but launches nothing. One approval
binds the workspace, ordered selections and every exact existing delivery/start
plan, including state, code, target identity/generation and runtime inputs. No
wildcard, latest-state alias, future-event grant or implicit state chaining is
accepted. Every entry still revalidates the normal delivery approval at admission.
A changed/revoked target or edited kit cannot inherit the previous grant.

An exclusive durable dispatch intent is recorded before any entry is attempted.
Entries run sequentially, and each returned result is durably recorded before
selecting the next. Only a successful result with confirmed termination permits
the next selection. Any refusal, failure, uncertain outcome or ledger write error
stops the batch; unknown work is never retried. A workspace allows 32 dispatch
identities. These are retained evidence, not a recyclable work queue.

For this command only, **SIGTERM stops selection after the current selected entry
finishes**, while **SIGINT requests cancellation through its existing supervisor
and prevents further selection**. Other commands retain their existing signal
semantics. Stop is not instantaneous revocation of an already selected entry.
Cancellation does not itself prove termination; inspect the retained native run
receipt before any acceptance or collection. Process disappearance or a missing
batch summary cannot authorize either. There is no cross-process pause/resume
service or parallel task pool in this slice.

`spool-events-dispatch-show WORKSPACE NAME` observes retained intent/entry/result
records without execution. Repeating a recorded dispatch also observes it: it
never resumes the remaining entries, even if it stopped cleanly. A partial intent,
lost entry record or missing final summary remains fenced/uncertain. Choosing a
new batch name does not bypass the existing event/subscription delivery fence.
To request still-unadmitted work after a clean stop, explicitly review a fresh
finite selection; do not resend unknown deliveries. No automatic retry or resume
command is provided.

Dispatch success means its selected executions completed with termination
confirmed, **not** that products were accepted or events consumed. Acceptance,
checkpoint compare-and-swap, event completion and cleanup remain separate host
operations. Multiple entries may explicitly consume the same checkpoint, so later
state commits can conflict; the host does not merge or rebase their business state.
Loom packages the same existing interfaces and gains no scheduler or runtime grant.

Real macOS headless acceptance covers exact state/data results, stale commit
refusal, wrong/changed approval, graceful stop, cancellation with confirmation,
blocked cleanup during execution, retained state/products, an actual ledger write
failure after execution, incomplete intent and missing receipt/summary observation
without replay. Evidence-loss injection follows known execution; it is not a
claim of arbitrary process-kill or power-loss recovery.

## Subscription admission pause and resume

`spool-event-pause WORKSPACE NAME [--approve HASH]` and
`spool-event-resume WORKSPACE NAME [--approve HASH]` control admission for an
existing immutable subscription. Both preview first and bind the exact previous
control generation in their approval. Repeating a transition or using an old
approval refuses; inspect `spool-events-show` for the current mode/generation.
Resume verifies the same loaded target identity and round-control generation;
it cannot rebind a reloaded instance or repair an outdated route. Revocation
remains permanent and neither operation can undo it.

Pause does not signal a running process, accept products, modify checkpoint
HEAD or prevent explicit completion of already admitted work. It fences new
admission under the existing workspace lock. Resume enables new review, never
reuses a pre-pause approval: every later delivery plan binds the current control
generation, and admission checks it again under that lock. Delivery identity
remains workspace/event/subscription; control transitions cannot create another
attempt for the same pair. A finite dispatch crossing a pause or pause/resume
stops at the next refused admission. Repeating that recorded batch never resumes
it. Still-unadmitted work needs a fresh finite selection and approval; unknown
work remains fenced.

Control records use the existing immutable workspace configuration snapshots and
log, with predecessor bindings and at most 32 transitions per subscription under
the existing 512-record workspace limit. Transition snapshots and the log are
synced before success is reported. Missing snapshots and duplicate, reordered or changed
committed control records refuse. An error can leave a committed transition;
inspect rather than automatically retrying. The 16-subscription bound counts
route declarations, not control records.

First use explicitly upgrades workspace metadata from
`an-native-event-workspace-v1` to `an-native-event-workspace-v2`. It preserves the
workspace identity, routes, event facts and historical delivery records. V1
workspaces without controls retain their existing format and pre-control delivery
approval shape. Earlier hosts reject v2 on open instead of ignoring pause records.
Do not concurrently operate an upgraded workspace with an older already-running
host; mixed host versions and filesystem rollback are not qualified. A crash
between metadata upgrade and control commit may leave v2 with no transition;
metadata alone is not evidence that pause completed. No automatic downgrade or
historical record rewrite is provided.

Pending/watch inventory includes subscription control and admission refusal.
It still grants no execution or collection. Real headless acceptance covers
pause/resume of direct and batch admission, rejection of pre-pause approvals,
unchanged delivery identity, pausing during a running batch, explicit custody
while paused, no batch replay after resume and permanent revocation. Local
qualification also invokes the previous host binary to verify v2 refusal;
CI checks the v2 marker and current-host contract without building a second host.

## Explicit completed-summary reconstruction

`spool-events-dispatch-reconcile WORKSPACE NAME [--approve HASH]` reconstructs
only a missing completed summary. Every selected entry must have its original
batch-bound record, exact admitted delivery/start plan, and matching successful
native receipt with confirmed termination. Preview is inert; approval binds all
those proofs and the deterministic summary. Conflicting summaries are never
overwritten. Missing entry/start/result evidence, cancelled execution and partial
or stopped batches refuse. Repeated reconstruction of the same summary is inert.
No processes run, no batch resumes, and no products/state are accepted or committed.
Historical success is not a new cleanup authorization. Complete retained native
receipts remain required, including after private collection. Fault injection
qualifies loss of the projection after known completion, not arbitrary power loss.

## Explicit receiver succession

`spool-event-replace WORKSPACE OLD_NAME NEW_NAME ROUTE_JSON [--approve HASH]`
reuses the existing strict `an-native-artifact-route-v1` request format. The
producer, event kind and source output stay fixed. Select a fresh subscription
name and a changed receiver identity/generation or result slot. Preview grants
no execution and approval binds both routes, old control generation and a fixed
workspace-log cutover. Any intervening log publication changes that approval;
re-preview explicitly instead of automatically retrying.

One immutable successor configuration commit retires old admission. It does not
rewrite the old route, move a run, cancel execution, migrate state or accept
outputs. Already-admitted work keeps its original delivery identity and may be
explicitly accepted, committed and completed through the old route. Keep the old
instance directory/evidence in place for that custody. Old pending single/batch
approvals are refused, and retired admission cannot be resumed by control commands.
Revoking a successor does not reactivate its predecessor.

The successor binds the count and canonical hash of the record prefix preceding
its commit. It only admits events published after that boundary; historical facts
cannot be resent through replacement, even if old execution is unknown. New facts
still need their own normal delivery approval and explicit checkpoint selection.
A new receiver may be seeded from an explicitly checked-out old checkpoint using
the existing state-init flow; this is a new instance history, not implicit lineage
or schema migration. An independently approved unrelated subscription remains a
distinct delivery authority: succession does not claim global exactly-once effects.

First replacement upgrades workspace metadata to `an-native-event-workspace-v3`
and stores the successor as `an-native-event-subscription-v2`. Earlier hosts
refuse new opens. Existing v1/v2 workspaces are unchanged unless upgraded; v3
supports previous control records without downgrading metadata. Do not mix old
already-running hosts with the upgraded workspace. The metadata upgrade alone is
not evidence that a successor committed. Missing/changed prefix or route snapshots
refuse; no command automatically repairs a partial log or restarts uncertain work.
A commit error may have published the successor: inspect both names before any
new explicit request. No automatic retry/downgrade is provided.

Retired declarations remain in the existing 16-route history and 512-record bounds.
This is bounded receiver cutover, not active-route garbage collection, arbitrary
producer/topology mutation, a workflow DSL or a background subscription service.
Pending/show/watch observations expose retirement without granting execution.

Real macOS qualification covers an in-flight old receiver during cutover,
old/incorrect/stale approval rejection, a concurrent publication invalidating the
preview, producer-change refusal, old uncertain result fencing, refusal of old
facts through the successor, explicit fixed checkpoint transfer, new receiver
output and retained historical custody after private cleanup. Local qualification
can pass `--legacy-host OLD_HOST` and records its binary hash when checking actual
old-host refusal; CI checks the current-host/v3 contract without another build.
