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
  starts survives client loss; event/session/peer dispatch remains foreground.
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
| Native input ownership | Exact managed claims; explicit cancelled-owner or acknowledged stateless-success release after private collection; explicit immutable checkpoint pins; stateful/unknown owners retained; producer-side observations, no complete reader census or payload deletion |

P1 is not closed. The remaining required capacity contract is **formal payload
collection**: identify registered references, release them explicitly, and define
what unmanaged readers mean before deleting bytes. Admission retirement is not
proof that all old references have expired. The native per-run catalogs do not
have a global reference census; absence of a reader or an acknowledgement is not
deletion authority. See [native storage](native-storage.md).

After that bounded contract, qualify the combined local headless path rather
than adding another sequence of isolated features: concurrent instances,
explicit inputs/state, finite event operation, interruption, retained products
and measured capacity. Preserve unknown results without automatic replay. This
is the P1 closure direction, not a requirement to build a daemon or graph engine.

The next [session and retention plan](session-and-retention.md) makes the
remaining storage work explicit, then proposes a small session/thread coordination
layer over existing attempts. Its first foreground session/finite-thread adapter is
implemented; general model-context threads and formal payload collection remain
unimplemented. See the plan for exact commands and concurrency limits.
GUI and TUI are deferred; no new user interface is required for qualification.

## Deferred scope and existing surfaces

| Surface | Current scope or reason to defer |
| --- | --- |
| Core/embedded agents | Existing event/tape/admission/supervision; in-process bodies remain trusted embedding code |
| Native registered profiles | Hash-bound stdio/file deployment and platform-specific enforcement; not arbitrary hostile-agent containment |
| Optional OpenShell | Qualified single-program execution and termination/identity reconciliation; no new VM backend in this phase |
| Reviewed workspace jobs | Operator-selected workflows and binary custody; not a general production spool ABI |
| Semantics | Pure claims/assessments and existing adapters; not comprehensive static analysis or a security boundary |
| Weaver | Optional Loom-side reviewed harness workflow; host kit proposal slot is not a host-owned agent loop |
| State/release migration | Explicit fresh-instance checkpoint seeding and read-only release comparison; no automatic lineage, domain merge or in-place migration |
| Live suspension | Admission pause and checkpoint/stop suspension are qualified; process-memory/VM restore and in-flight safe-point signaling are not |
| Additional orchestration | Multiple business inputs, producer/topology editing, background admission and low-latency channels await concrete demand |
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
