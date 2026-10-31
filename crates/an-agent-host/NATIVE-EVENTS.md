# Native workspace artifact events

Status: bounded, explicit macOS headless delivery. An accepted output can become
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
[binary products and operator-selected Python](NATIVE-PYTHON.md) use the same
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
`--state REVISION`. The [data/state contract](NATIVE-STATE.md#independent-business-data-and-state)
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

The real event CLI acceptance qualifies pending/accepted/consumed and blocked
rows, monitoring while an actual headless delivery holds the instance lock,
changed evidence after revocation, observation cancellation without execution
cancellation, deadline exit, and monitor restart with missing result evidence
without creating another attempt. Rust tests cover torn logs and output faults.
