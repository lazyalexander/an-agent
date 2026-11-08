# Implementation status and next steps

This page separates qualified local contracts from the architecture's broader
intent. Example success never expands a runtime grant. Reviewed headless execution
is the current product scope; platform-specific coverage and deferred targets are
maintained in the [support and qualification matrix](platforms.md).

## P0: bounded headless event delivery

P0 is qualified for reviewed local CLI spools, not arbitrary untrusted agents:

- Immutable release/instance binding, two internal silk steps, scoped kit edits,
  independent instances, whole-group cancellation and baseline restoration.
- Explicit v3 workspace artifact facts, fixed subscriptions, separate delivery
  approval, accepted-output custody and causal consumption confirmation.
- One fixed business input combined with one explicitly selected opaque committed
  checkpoint; stale-state conflict refusal and confirmed-cancellation retention.
- Explicit per-attempt supervision for file, checkpoint and combined data/state
  starts survives client loss. Native CLI event deliveries and both peer phases
  use the same attempt owner. A finite controller owns each approved batch or thread.
  Confirmed prelaunch cancellation can close a round without granting custody.
- Explicit reconciliation of complete original execution/acceptance evidence.
  Missing termination or uncertain execution never authorizes a rerun.
- Optional operator-selected Python environments and declared binary products;
  the paired document example exercises saved XLSX and fixed downstream PPTX.

See [native spool](native-spool.md),
[native events](native-events.md),
[native state](native-state.md) and
[Python/binary inputs](native-spool.md).

## P1: daily local event operation

| Capability | Qualified boundary |
| --- | --- |
| Inventory and watch | Read-only pending inventory and bounded foreground change monitor; no execution authority |
| Finite dispatch | 1..16 explicitly approved selections, fixed state per entry, drain-stop/cancel and an exclusive no-resume fence; custody remains explicit |
| Subscription control | Generation-bound admission pause/resume and permanent revocation; admitted custody survives |
| Receiver succession | Immutable successor and fixed event cutover; no historical replay, automatic state migration or retargeting of old work |
| Completed-summary repair | Explicit reconstruction from every original entry/start/delivery and confirmed-success receipt; no execution or partial-batch resume |
| Private capacity maintenance | Explicit finite work/snapshot collection through original ownership/settlement gates; four real 8 MiB rounds and consumer progress qualified |
| Artifact input retirement | One accepted output can close new revision-based acquisition; admitted work, independent checkpoints, original bytes and causal evidence remain retained |
| Native input ownership | Exact managed claims; explicit cancelled-owner or acknowledged stateless-success release after private collection; explicit immutable checkpoint pins; stateful event release after approved exhaustive input coverage; uncertain owners retained; retirement review checks explicitly selected consumer receipts; legacy reader census remains incomplete |
| Registered publication | New `--registered` acceptance records native input attempts, explicit copy readers and checkpoint pins from first publication. Explicit exact-version collection requires closure and verified reader releases. Legacy catalogs stay retained. See [native storage](native-storage.md#registered-copies-and-explicit-payload-deletion). |

The local headless P1 path is qualified across three producer rounds and two
consumers. A fixed event feeds both consumers after a newer version exists. A
stateful consumer commits while its peer is active, and the producer can collect
a released version after confirmed cancellation. A new client can cancel after the submitting CLI exits. Losing the attempt
owner leaves one held execution. Paired qualification records disk use and the exact bytes removed.
Enrolled checkpoints can now close admission and explicitly release their own
pins after reader settlement. Legacy reader histories keep their pins. See
[checkpoint retirement](native-state.md#explicit-checkpoint-retirement).
The routed supervision suite also qualifies client, controller and attempt-owner
loss separately. See [execution ownership](session-and-retention.md#routed-execution-ownership).

Native cohorts control finite selections. A peer can receive one
audience-checked context packet from a fixed, accepted text revision.
A core agent's conversation tape remains private. See the
[bounded handoff and identity contract](session-and-retention.md#bounded-peer-context)
for supported sources and disclosure limits.

## Deferred scope and existing surfaces

| Surface | Current scope or reason to defer |
| --- | --- |
| Core/embedded agents | Existing event/tape/admission/supervision; in-process bodies remain trusted embedding code |
| Native registered profiles | Hash-bound stdio/file deployment and platform-specific enforcement; not arbitrary hostile-agent containment |
| Optional OpenShell | Qualified single-program execution and termination/identity reconciliation; no new VM backend in this phase |
| Reviewed workspace jobs | Operator-selected workflows and binary custody; not a general production spool ABI |
| Semantics | Pure claims/assessments and existing adapters; not comprehensive static analysis or a security boundary |
| Weaver | Optional Loom-side reviewed harness workflow; host kit proposal slot is not a host-owned agent loop |
| Coding harness internals | Native clients own shell tools, session assembly and compaction. Unused context/factory crates, the generic ReAct policy, HTTP model transport and one-shot model service are retired; finite MCP, explicit context disclosure and supervised host execution remain supported. See [harness ownership](model-services.md#native-harness-ownership). |
| State/release migration | Explicit fresh-instance checkpoint seeding and read-only release comparison; no automatic lineage, domain merge or in-place migration |
| Live suspension | Admission pause and checkpoint/stop suspension are qualified; process-memory/VM restore and in-flight safe-point signaling are not |
| User-local scheduling | [macOS service](daemon.md) queues existing supervisor reservations; original lifecycle/custody remain authoritative. No automatic replay or business graph scheduler |
| GPU/GUI and hostile harnesses | Require independent platform, application and enforcement qualification |

Spool interaction remains workspace-event routing. Domain branches, loops and
subagent decisions stay in silk/kit. A future high-frequency channel needs a
separate grant; it is not a reason to turn the host into a business graph engine.

Loom owns authoring, checks, packaging and narrow paired compatibility evidence.
an-agent owns runtime authority, supervision, state/artifact custody and retention.
There is no reverse runtime dependency. Preserve existing CLI/profile/tape
identities and old release bytes; unsupported enforcement refuses rather than
falling back. Dev merges require diff review, real acceptance and every CI gate,
followed by user notification. Main promotion remains user-triggered.
