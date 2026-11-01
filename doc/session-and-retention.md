# Artifact retention and runtime ownership

Status: accepted development direction, with the first native input-reference
slice implemented in [native storage](native-storage.md).
Registered-copy outputs support explicit payload deletion after admission closure. A bounded native session/dispatch adapter
is implemented below; general model-session coordination remains planned. The qualified boundary
remains reviewed headless CLI spools; see the [platform matrix](platforms.md).
GUI/TUI, new backends and process-memory restoration are deferred.

## Reuse before adding a manager

| Existing implementation | Reuse and limit |
| --- | --- |
| [Native storage](native-storage.md) | Settled scratch/snapshot collection, capacity observations and new-input retirement; explicit collection of newly registered-copy versions |
| [Artifact admission](../crates/an-agent-host/src/artifact_versions/admission.rs) | Monotonic closure with concurrent immutable readers; no reference inventory |
| [Checkpoint retention](../crates/an-agent-host/src/instance/checkpoint/retention.rs) | Explicit recipients/state references in a closed store; not a global census of native catalogs |
| [Core session](../crates/an-agent-core/src/agent/session.rs) and [session control](../crates/an-agent-core/src/control/session.rs) | Private tape/files, seal, finished-thread release and identity-checked resume; not a native-spool session coordinator or whole-machine recovery |
| [Native events](native-events.md) | Fixed versions, finite approved dispatch, admission controls and explicit reconciliation; no background agent loop |
| [Model/context](model-services.md) and [bounded export](model-services.md) | Explicit audience/clip selection, receipt-linked excerpts and byte limits; no token estimator, automatic compaction or cross-thread authorization service |

Do not rename existing core sessions, thread UUIDs, tape records or wire formats.
A future host adapter maps its coordination records to these identities explicitly;
a native spool attempt need not create a core model session at all.

## Ownership and proposed identities

| Concept | Meaning | Not equivalent to |
| --- | --- | --- |
| Workspace | Host-controlled versioned material and fact namespace | A peer mailbox or shared model conversation |
| Runtime session (target) | One activation generation of a spool instance, owning resource and endpoint references | A model conversation, a grant or a new isolation boundary |
| OS thread/process | Backend execution machinery | A context identity or a domain workflow node |
| Spool instance | Release-bound domain identity with explicit committed state | One conversation or model session |
| Attempt | One exact admitted execution, selected inputs/state and settlement evidence | The persistent session or an automatically resumable task |
| Artifact reference | Exact managed version and declared reason for retaining bytes | A mutable path, implicit latest or permission to execute |

The target starts with one active runtime session per instance. Reuse instance
activation records rather than adding a parallel manager. Attempts retain their
existing execution isolation and exact input/state identities; sharing a session
does not merge attempt scratch or grant concurrent state writes. Reactivation
must fence old endpoints/results with a new generation. This is not implemented
by the current `session-*` CLI: that compatible adapter groups finite dispatches,
and its `THREAD` argument is a one-shot dispatch identity, not an OS thread.
Do not reinterpret historical records or delete duplicate-admission guards.

Host workers and OS threads may serve different operations over time. Bind model
context to the authorized participant, selected materials and harness handle,
never a worker/TID or the entire workspace. Private contexts inside one shared
spool environment are not independent security boundaries. Existing harnesses
retain their own history; revoking file access does not erase prior disclosure.

The host owns effective grants, identities, cancellation, reference custody and
boundary evidence. The spool owns domain state sufficiency, internal sequencing
and useful summary content. Loom describes intended interfaces, retention needs
and context contracts, validates and packages them; it does not grant access,
run a scheduler or collect runtime files. Existing harnesses own their native
conversation history and compaction. Weaver stays a replaceable development
assistant, not a second runtime supervisor.

## Communication and context boundary

Workspace facts describe accepted material changes. Addressing, request/reply,
subscription authorization and delivery acknowledgement belong to the host
protocol boundary. Files may carry protocol envelopes or immutable referenced
payloads; arbitrary shared-directory writes must not become implicit commands.
Receiving a message, completing execution and accepting an output are different
events. Causal downstream work does not inherit upstream cancellation ownership.
Retain existing event/subscription delivery identities and no-replay fences.

Material read permission does not imply disclosure permission to a model service.
Domain code/harnesses select useful context; the host checks access, recipient,
versions and effective authority. The core workspace `env` remains trusted host
configuration, not a sink for peer messages, retrieved text or arbitrary files:
it enters the system envelope. Context packets remain untrusted selected data;
their hashes and audience fields do not independently authorize disclosure.

## Runtime acceptance matrix

| Design requirement | Current evidence | Remaining gap |
| --- | --- | --- |
| Fixed authorized artifact delivery | `native_events.py`, `native_data_state.py`, `native_peer.py` | No general live IPC endpoint |
| No implicit private-note delivery | `native_material_boundary.py`: real read/write denials, unchanged fact log, no delivery, declared outputs only | No model ingestion tested; this is a foreground adapter, not an automatic watcher |
| Private contents remain outside another attempt | Same suite checks another instance's output/identity and workspace log | Native policy allows file metadata; it does not hide existence or provide hostile-code containment |
| Cancel one owner without cancelling another | `native_sessions.py`: overlapping real execution | Caller-independent supervisor ownership remains unqualified |
| Unknown execution cannot release materials | `native_data_state.py`, lost-receipt session cases | Missing receipts are injected after execution; arbitrary crash recovery is not proved |
| Old delivery cannot become new-instance work | `native_route_successor.py`; `native_peer.py` refuses stale reply admission after pause/resume | Peer endpoints use instance identity and round generation, not a separate session manager |
| Waiting A leaves B able to execute | `native_peer.py`: B runs while A has independent active work; the wait has no process or held execution lock | No general blocking IPC waiter, fairness guarantee or global scheduler |

The first local material run denied all three protected content reads/writes but
exposed metadata for all three paths. Keep that limitation explicit rather than
weakening an assertion or presenting protocol identity checks as information-flow
isolation. Raw machine paths and command evidence remain local.

## Split-phase peer exchange

For a reviewed headless pair A and B, the trusted host converts an accepted A
output into a request to one exact B endpoint. A has finished its computation;
waiting is a host-owned record under the requesting instance, not an idle OS
thread, a live model, a workspace event or a grant to scan directories. The
request schema is `an-native-peer-request-v1`; specify `source_run`,
`source_output`, `target`, `reply_output`, and `wall_seconds` (1–300). Optional
`request_state` and `response_state` are exact revisions for the respective
recipient's declared state input, never implicit HEAD. The native generation
is the existing `(instance, release, round-control generation)` binding. A reload
has a different instance identity; pause/resume changes the round generation.

```text
spool-peer request A NAME SPEC.json        # preview; approve its exact digest
spool-peer show A NAME                    # observation only
spool-peer run A NAME request             # separately approve B's exact start
spool-accept B B_RUN                      # formally accept B's declared output
spool-peer reply A NAME                   # separately approve the correlated reply
spool-peer run A NAME response            # separately approve A's continuation
spool-accept A A_RUN                      # accept the final product independently
spool-peer cancel A NAME                  # revoke future peer admission; no signal
```

Every mutation takes `--approve HASH` after preview. Each stage has one native
attempt identity derived from the approved request and phase. The durable stage
intent precedes execution. Already admitted or uncertain stages refuse replay;
no implicit forwarding or next run occurs. A reply must match both endpoints,
the original request, verified target termination, accepted output and exact
artifact version. Observation distinguishes uncertain, revoked, stale and ready;
it never authorizes execution or collection.

Revocation fences future admission and consumption; work already admitted can
finish and remains owned by its original process supervisor. Reclaim requires
original termination evidence. There is no background worker or live socket
service, no code-agent access to this operator CLI and no workspace transcript.
Request names are single-use (64 per requester). Each admitted native phase pins
its input. Successful peer pins currently stay retained: event acknowledgement
cannot release them. Waiting itself is not a permanent retention grant; retired
or unavailable materials refuse new execution. No formal payload GC is enabled.

## Artifact lifecycle: retain bytes only for explicit reasons

Keep three concepts separate:

- **Evidence reference:** identity, digest, origin, outcome and causal linkage.
  It survives payload collection but does not promise that bytes remain available.
- **Retention pin:** a registered owner still needs the exact bytes. Examples:
  an admitted attempt, an unresolved delivery, an explicitly selected checkpoint
  dependency, or an operator retention decision.
- **Independent copy:** a verified checkout/snapshot with separate custody.
  It can outlive the original, but does not prove its owner released the original.

A content digest is not a retention pin or publisher authentication. Never scan
opaque state or agent prose to guess dependencies. Spool-declared state references
must be explicitly committed with the checkpoint; old checkpoints keep their
pins until explicitly retired. State copied into an independently retained
checkpoint need not pin its source merely because provenance mentions it.

Proposed bounded sequence:

1. Accept immutable output through existing settlement and custody gates.
2. Acquire a durable pin before a managed consumer relies on that version.
   Bind catalog/revision/digest, owning instance and attempt/checkpoint identity,
   generation and purpose. Registration and admission must have an ordering that
   collection cannot race. A crash between pin and run intent may leak a pin;
   it must never permit deleting live input.
3. Close new input admission using the existing monotonic retirement operation.
4. Explicitly release each eligible pin after its owner no longer needs bytes.
   Process exit alone, an acknowledgement, session closure, elapsed time or an
   absent heartbeat cannot release it. Unknown execution/delivery stays pinned.
5. Preview and approve collection only in a declared closed managed scope with
   no remaining pins. Revalidate closure, pins, ownership and exact inventory
   under the original custody boundary before recording the deletion intent.
6. Retain immutable metadata, receipts and a payload-unavailable tombstone.
   Content reads must report collected/unavailable, never pretend success.
   Unknown deletion remains fenced for inspection, without automatic retry.

This is a target protocol, not a second implementation beside existing custody.
Reuse existing collection intents, receipts and checkpoint checks where their
assumptions hold. Derive eligibility from durable ownership records, not a bare
counter that can lose a decrement. The first implementation needs no global GC,
TTL leases, distributed coordinator or automatic periodic deletion.

### Migration and evidence readers are part of the work

Existing native catalogs predate pins. Missing pins do not mean zero consumers:
legacy outputs remain retained by default. First qualify collection for newly
created managed catalogs with a recorded policy; old catalogs need a separate
explicit adoption procedure, not a scan that assumes all clients are visible.
Unmanaged readers have no retention guarantee unless the operator explicitly
pins the original or hands off an independently retained copy. Until that boundary
is declared, refuse collection. Older host executables must not operate a store
that requires the new retention contract; qualify a version fence before deletion.

Current acceptance, state commit and causal verification can re-read original
payloads. Before deleting any native payload, distinguish metadata-only history
inspection from operations that still need bytes. Keep required pins until those
operations settle, and qualify tombstone-aware history inspection. Do not weaken
content verification globally to make deletion pass. Checkpoint payload GC is a
separate operation; the first artifact collector must not delete committed state.

## Implemented native session slice

The foreground event adapter exposes the following host commands. `WORKSPACE` is
an existing native event workspace, not a directory granted to spool processes.

```sh
an-agent-host session-create WORKSPACE SESSION
an-agent-host session-run WORKSPACE SESSION THREAD dispatch-request.json
an-agent-host session-run WORKSPACE SESSION THREAD dispatch-request.json --approve HASH
an-agent-host session-show WORKSPACE SESSION
an-agent-host session-control WORKSPACE SESSION paused
an-agent-host session-control WORKSPACE SESSION paused --approve HASH
```

Control modes are `open`, `paused`, `cancelled` and `closed`. Each control change
requires a fresh digest-bound preview. Pause requests drain-stop: once observed,
the runner finishes its current entry and prevents the next. Control responses
acknowledge publication, not a synchronous barrier across all runners; an entry
concurrently crossing admission may still start. It remains owned and must settle.
Reopen permits new threads, never resumes a partial dispatch.
Cancel is permanent admission closure and asks each live session runner to forward
cancellation through its existing supervisor. Close requires retained thread
completion and original confirmed attempt evidence. Closure never accepts products,
commits state, releases references, unloads instances or deletes bytes.

A thread is one immutable finite dispatch (1..16 explicitly selected event/route/
state entries). It has a session-scoped identity and a durable pre-execution intent.
Changing source request bytes or session generation invalidates approval. Thread
names cannot be reused, including after cancellation or missing results. The host
keeps only selected artifact/state context, not another harness's conversation tape.
Parents, cross-thread model packets and general multi-attempt thread continuation
are deferred. No release field or Loom runtime dependency is introduced.

Inspection returns compact identities and outcome metadata; full dispatch evidence
stays available through `spool-events-dispatch-show`, not copied into every summary.
The session lock covers short admission/control publication only. Observation and
execution do not hold it. A foreground runner polls its bounded control history
at 25 ms intervals; this is not a cancellation-latency guarantee. A read error
requests cancellation, not successful settlement. Reopening records after a host
crash is read-only inspection: missing receipts remain unknown, and no daemon
restarts work or signals a process based on a reused PID. A dead runner cannot
forward a new cancellation; original backend reconciliation remains necessary.

Existing event admission locks remain fail-fast. Simultaneous admissions in the
same workspace can refuse; the qualified case explicitly admits independent
threads and then overlaps their execution. It does not claim a queue or conflict-free
parallel admission. Neither refusal nor unknown execution is automatically retried.
Limits are 32 sessions per workspace, 32 threads per session and 128 control records.
Legacy executables do not manage these new session records; operator-issued direct
commands remain separate authority and are not retroactively governed by sessions.

`tests/cli/native_sessions.py` under the host crate qualifies independent overlapping
execution, scoped cancellation, drain-stop, stale generation/request refusal,
multiple logical threads, pinned input/state, products after close and injected
lost-receipt refusal after real execution. It uses zero model calls. Session
closure rechecks native evidence; formal payload GC remains blocked by its separate
unfinished retention contract.

## Foreground supervisor loss qualification

The original foreground native entrypoints keep their supervisor inside the initiating CLI.
`OwnedChild` keeps the group leader unreaped while observing and terminating
other group members, then reaps the leader. The original bound workspace receipt
records that outcome. Ordinary errors and unwinding run its cleanup guard;
SIGKILL bypasses the guard, cancellation polling and the wall-clock deadline.

`native_supervisor_loss.py` exercises the real CLI with two finite workers.

| Fault | Observed execution | Custody outcome |
| --- | --- | --- |
| SIGTERM to the initiating CLI | Original supervisor terminates its group and records cancellation | Confirmed private collection succeeds |
| SIGKILL to the initiating CLI | Both workers survive, exceed the 3-second deadline and finish writing | Status remains unconfirmed; late output cannot be accepted or collected |
| Same terminated fixture with stale operation locks removed | No live group remains in external observations | Missing original receipts still refuse reconciliation, collection, unload and same-run admission |

The fixture verifies a separate instance can run and previously accepted products
remain intact. Finite workers bound the experiment's lifetime. Process samples
and lock removal are diagnostic fault injection in disposable test directories.
Neither numeric PIDs, absent processes, absent locks nor completed output files
establish original supervisor ownership or recover a lost execution result.

## Independent attempt supervisor

The explicit `spool-supervisor` entry owns one native file, checkpoint or combined
business-data/state run in a separate process. It uses the same reviewed spool release, approved start plan,
execution boundary, process-group settlement and formal artifact commands.

```text
spool-supervisor start INSTANCE RUN INPUT SECONDS [--approve HASH]
spool-supervisor start-state INSTANCE RUN REVISION SECONDS [--approve HASH]
spool-supervisor start-data-state INSTANCE RUN SELECTION_JSON SECONDS [--approve HASH]
spool-supervisor show INSTANCE RUN
spool-supervisor cancel INSTANCE RUN [--approve HASH]
spool-status INSTANCE RUN
spool-reclaim INSTANCE RUN [--approve HASH]
```

Start previews a plan binding the canonical input, executable, instance, release,
activation generation and native start. Approval durably reserves that run before
spawning a detached supervisor. The worker must exclusively claim the request,
then revalidate the original native plan before starting any domain process.
Direct starts also reject reserved identities. A torn reservation or owner claim
remains fenced. A client timeout, missing response or launch failure never retries.

The worker owns the native process group and deadline. The client waits for its
response and forwards handled signals as durable cancellation. Abrupt client loss
leaves execution supervised until completion or the original deadline. Another
operator CLI can request cancellation for the exact request without taking the
instance execution lock. Controls live in the host-private run directory, beyond
the domain process write grant. An unreadable control marker requests cancellation.
PID fields are diagnostics only; control never signals a recorded numeric PID.

The supervisor stops after execution and writes a request-bound response. Native
settlement remains authoritative for acceptance and collection. Supervisor exit,
a response file or a cancellation request alone grants no custody. A supervisor
crash leaves unresolved records and blocks collection even after observed process
exit. A lost client can inspect the original run without restarting it.

A cancellation published before worker admission is checked synchronously. The
original worker can acknowledge `cancelled-before-launch` without rereading an
input that is no longer available. Round closure/unload requires its matching
request, exclusive owner and exact no-launch completion, plus absence of job,
start, input-reference and custody records. A cancellation request alone or any
contradictory/partial record remains uncertain. This closes an unused reservation;
it grants neither artifact acceptance nor collection, and never permits replay.
`native_spool::prelaunch_cancel` exercises this deterministic boundary using the
real worker CLI, including removed input, missing/mismatched evidence, duplicate
worker refusal and preservation of an earlier accepted product.

`native_supervisor.py` qualifies client SIGKILL with continuing deadline and
cancellation enforcement, handled client SIGTERM, duplicate-worker and stale-control
refusal, a torn launch reservation, supervisor SIGKILL, an independent instance
and preservation of formal products. Its finite workers bound fault experiments.
The original foreground fault case remains in CI to preserve its explicit limits.

Explicit state supervision uses `start-state` with an exact checkpoint revision.
The revision and native start plan are bound into the durable approval, and the
worker selects that revision again through the existing state-input path.
Advancing HEAD never selects a different checkpoint. The real state fault suite
advances HEAD before launching the old revision and tests client loss, timeout,
cancellation and unknown-owner retention. Its `--combined` mode repeats the fault
matrix with separate business data and state, including stale selector refusal
and preserved exact input references.

Combined supervision uses `start-data-state` with the existing selection JSON.
Approval retains the resolved native plan and both exact bindings, not the mutable
selector path. The worker revalidates those revisions and admission before starting;
selector replacement/removal after reservation cannot retarget it. A retired input
fails closed, with no fallback or new attempt. A deterministic real-execution test
covers selector removal and retirement between reservation and worker claim.

This slice covers explicit supervised file, checkpoint and combined input starts.
Existing foreground starts, event/session dispatch and peer phases retain their
existing lifecycle.
Their migration needs ownership-specific qualification. Before-launch uncertain
reservations remain retained; no automatic lock repair or process adoption is
implemented. Local host records and the installed supervisor binary are trusted.
A permanently running host-wide service awaits a separate scheduling requirement.
Loom packages the release and receives verified evidence; the host owns execution
and control. Platform detachment remains in the execution backend.

## Runtime control and legacy session compatibility

Keep inspectable records and explicit CLI actions. The current foreground adapter
does not transfer supervisor ownership when its caller disappears. A future
caller-independent supervisor must retain backend ownership and reconcile actual
execution before accepting control; reopening metadata is insufficient.
The target activation record binds one instance/release, generation, owned attempt
references, endpoint references and operator-owned limits. Reuse existing attempt
identities rather than adding a second execution ledger or OS-thread scheduler.
Provider/harness session IDs, when supported, are opaque private adapter data.
They are neither spool IDs nor new authority. Do not ingest private harness tapes.

Use the following lifecycle distinctions rather than one overloaded status:

- Admission is open or paused/closed; pausing prevents new work only.
- Execution is running, settling, positively settled or unknown, derived from
  original attempt evidence. Requesting cancellation is not confirmation.
- A session can close after its owned work is settled; unresolved work remains
  visible and prevents a successful closed result. Archiving its view is not
  permission to kill processes or delete products.

A session-wide stop closes admission before forwarding cancellation to an exact,
generation-bound set of owned active attempts. Stop and cancel remain separate:
stop drains existing work; cancel requests termination. Two sessions must remain
independent. A causal protocol link alone cannot cancel another session's work.
Reference release and scratch cleanup still require their own original evidence.

After interruption, reopen records and inspect attempts before offering new work.
Resuming a core conversation does not resume a native command. A new execution
needs a new attempt and explicit input/checkpoint/grant selection. Unknown old
work cannot be retried by creating a fresh session or thread. Late results are
bound to their original attempt/generation and cannot claim new work's output.
Do not add OS-thread pools, PTYs or idle model sessions without measured need.

## Responsiveness and token discipline

The fast path is deterministic: inspect identity/status, route a durable event,
check grants, select references and call an existing CLI. Do not invoke a model
for polling, counting pending work, copying files or routine lifecycle decisions.

- Send references and compact outcome metadata by default. Fetch exact content
  only for an approved audience and a concrete task. A reference conveys no read
  authorization; resolving it must recheck access and retention status.
- Keep full accepted output separate from a bounded model-facing view. Reuse
  existing receipt-checked export before introducing generated summaries.
- Select instructions, current goal, exact relevant input/state, outstanding
  decisions and necessary error excerpts. Do not send the entire tape or unrelated
  threads. Authority, unresolved failures and approval scope must never disappear
  through silent truncation; refuse or request a smaller selection when necessary.
- A derived summary is disposable, untrusted context with source revision/range,
  strategy/version and omission metadata. It cannot become authoritative state,
  permission or proof of success. Keep originals according to retention policy.
- Reuse a selection only for matching source identity, audience, policy and
  selection parameters. Recheck authorization; deduplication must not merge two
  distinct business events just because payload bytes match.
- Coalesce disposable progress notifications only. Keep durable artifact facts,
  cancellation, errors and required acknowledgements distinct and ordered.
- Prefer existing harness context/session reuse only where its adapter can prove
  correct identity and scope. Cancellation and unknown provider outcomes never
  trigger an automatic model retry or fallback provider.

Initial limits are explicit bytes, item counts and concurrency. Do not relabel
bytes as tokens. Exact token/spend enforcement requires provider-specific usage
and accounting; unknown usage is recorded as unknown, not zero. Do not promise
savings from prompt caching or summaries before measuring quality and actual use.

## Delivery order and acceptance

| Slice | Deliverable and end condition |
| --- | --- |
| A: native artifact ownership | New managed outputs acquire/release explicit pins through real producer/two-consumer execution; active and unknown owners, retained checkpoint dependencies and legacy catalogs block deletion; legacy outputs stay retained |
| B: formal payload collection | Closed admission plus released pins permits exact approved deletion; retained provenance remains inspectable, content reads fail clearly, unrelated outputs/checkpoints survive; race, cancellation and lost deletion receipt refuse unsafe progress |
| C: runtime ownership | Foreground dispatch groups and generation-bound peer waits run through explicit host commands. Old/unknown work cannot claim new work. Explicit file/state/data-state starts have per-attempt independent supervision; event/session/peer dispatch remains foreground |
| D: bounded context handoff and combined qualification | Reuse explicit packets/export across authorized participants; unrelated data excluded, incorrect audience refused, oversized input explicit, missing source reported; combine lifecycle, pins, collection and interruption in one real headless run |

A and B close the remaining P1 storage contract. C and D are the next small
management milestone, not permission to endlessly expand P1. GUI/TUI, general
conversation engines, semantic search/GNN routing, provider catalogs, GPU/VM
suspend and automatic state migration remain out of scope.

Slice A is partial: new native starts register exact artifact/checkpoint inputs.
Confirmed cancelled owners can release after private collection; successful
stateless event consumers can release after original consumption acknowledgement,
accepted outputs and private collection. Successful combined data/state event owners may release their temporary claims
after approval of exhaustive coverage tied to the committed checkpoint. Partial
registrations and old/unmanaged readers remain retained. Explicit checkpoint dependency declarations now acquire non-releasable source pins;
missing registration evidence blocks selection. Other successful-owner paths, dependency
retirement/reconciliation and a mixed-host version fence remain outside the opt-in registered-copy collection profile. No current reference query authorizes deletion.
The producer-side `spool-artifacts ... --references OUTPUT` observation now exposes
registered consumers without control locks. Source release markers are reported
separately from consumer completion; an empty view is never a complete census.

Use Rust tests for internal transition/race windows and Python for real CLI
acceptance. Loom keeps one narrow package-to-host compatibility case; it does not
duplicate the host's fault matrix. No live model is necessary to qualify lifecycle
or deterministic context selection. Fixture usage proves protocol handling only;
report a later live-provider measurement separately if needed.

Record input/release/binary identities, attempts, events, pin transitions,
settlement and deletion receipts, process counts and logical storage bytes.
Measure event-to-admission and cancel-to-confirm latency, context selection time,
bytes sent, actual provider usage when available, model-call count, manual
interventions and peak storage. Separate cold/warm runs and build/run time.
Report p50/p95 only with sample counts and enough samples; small runs list raw
values. APFS logical reduction is not physical reclaimed space. Compare the same
workload before/after without weakening outcome checks. The deterministic path
must require zero model calls; quantitative latency/token targets follow the
baseline, not an invented percentage.
