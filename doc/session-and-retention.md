# Artifact retention, sessions and logical threads

Status: accepted development direction, not a new runtime guarantee. This slice
is documentation only. The current qualified boundary remains local macOS,
reviewed headless CLI spools. GUI/TUI, new backends and process-memory restoration
are deferred. Existing Linux code and CI remain intact.

## Reuse before adding a manager

| Existing implementation | Reuse and limit |
| --- | --- |
| [Native storage](../crates/an-agent-host/NATIVE-STORAGE.md) | Settled scratch/snapshot collection, capacity observations and new-input retirement; no formal native payload deletion |
| [Artifact admission](../crates/an-agent-host/src/artifact_versions/admission.rs) | Monotonic closure with concurrent immutable readers; no reference inventory |
| [Checkpoint retention](../crates/an-agent-host/src/instance/checkpoint/retention.rs) | Explicit recipients/state references in a closed store; not a global census of native catalogs |
| [Core session](../crates/an-agent-core/src/agent/session.rs) and [session control](../crates/an-agent-core/src/control/session.rs) | Private tape/files, seal, finished-thread release and identity-checked resume; not a native-spool session coordinator or whole-machine recovery |
| [Native events](../crates/an-agent-host/NATIVE-EVENTS.md) | Fixed versions, finite approved dispatch, admission controls and explicit reconciliation; no background agent loop |
| [Model/context](../crates/an-agent-host/MODEL-CONTEXT.md) and [bounded export](../crates/an-agent-host/CONTEXT-OUTPUT.md) | Explicit audience/clip selection, receipt-linked excerpts and byte limits; no token estimator, automatic compaction or cross-thread authorization service |

Do not rename existing core sessions, thread UUIDs, tape records or wire formats.
A future host adapter maps its coordination records to these identities explicitly;
a native spool attempt need not create a core model session at all.

## Ownership and proposed identities

| Concept | Meaning | Not equivalent to |
| --- | --- | --- |
| Workspace | Host-controlled collaboration and artifact namespace | A shared writable directory granted to every spool |
| Host session | One bounded engagement: selected participants, thread references, budgets and lifecycle controls | Spool business state, an authorization grant or a VM |
| Logical thread | One context/disclosure scope and cancellation scope within that engagement | An OS thread, process or independent execution backend |
| Spool instance | Release-bound domain identity with explicit committed state | One conversation or model session |
| Attempt | One exact admitted execution, selected inputs/state and settlement evidence | The persistent session or an automatically resumable task |
| Artifact reference | Exact managed version and declared reason for retaining bytes | A mutable path, implicit latest or permission to execute |

One host session may coordinate several instances; one logical thread may refer
to several attempts over time. Two sessions can refer to the same instance only
through its existing ownership/currentness rules. No new concurrent-write right
is implied. Parent/child thread links explain causality; children receive no
implicit context, filesystem access, model credentials or spending authority.

The host owns effective grants, identities, cancellation, reference custody and
boundary evidence. The spool owns domain state sufficiency, internal sequencing
and useful summary content. Loom describes intended interfaces, retention needs
and context contracts, validates and packages them; it does not grant access,
run a scheduler or collect runtime files. Existing harnesses own their native
conversation history and compaction. Weaver stays a replaceable development
assistant, not a second runtime supervisor.

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

## Minimal session and thread management

Start with inspectable records and explicit foreground CLI actions, not a daemon.
The proposed session record binds a workspace, session identity, generation,
thread references and operator-owned limits. A thread record binds its own
identity/generation, optional parent, selected instance/attempt references,
explicit context selections, cancellation intent and observed outcomes.
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
independent. A logical child link alone cannot cancel another session's work.
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
| A: native artifact ownership | New managed outputs acquire/release explicit pins through real producer/two-consumer execution; active and unknown owners, retained checkpoint dependencies and legacy catalogs block deletion; no payload deletion yet |
| B: formal payload collection | Closed admission plus released pins permits exact approved deletion; retained provenance remains inspectable, content reads fail clearly, unrelated outputs/checkpoints survive; race, cancellation and lost deletion receipt refuse unsafe progress |
| C: session/thread skeleton | Foreground create/inspect/pause/close and explicitly scoped cancel over existing attempts; two sessions, multiple threads, one cancelled and one uninterrupted; reopen after host interruption without replay; stale generations and late results refused |
| D: bounded context handoff and combined qualification | Reuse explicit packets/export across two logical threads; unrelated data excluded, incorrect audience refused, oversized input explicit, missing source reported; combine lifecycle, pins, collection and interruption in one real headless run |

A and B close the remaining P1 storage contract. C and D are the next small
management milestone, not permission to endlessly expand P1. GUI/TUI, general
conversation engines, semantic search/GNN routing, provider catalogs, GPU/VM
suspend and automatic state migration remain out of scope.

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
