# Implementation status and next steps

This page describes the bounded implementation, not all capabilities suggested
by the architecture diagram. An example's success does not widen a runtime grant.

| Area | Current evidence and limitation |
| --- | --- |
| Embedded agents | Existing event/admission/tape and supervision paths; in-process bodies are trusted code, not isolated workloads |
| Native deployment | Registered hash-bound stdio/file profiles with platform-specific enforcement and receipts; not arbitrary code-agent execution |
| Concurrent native attempts | Two single-entry spool releases in one store; independent cancellation/restart, pinned inputs and evidence-gated scratch cleanup; no queue or general multi-silk supervisor |
| Optional OpenShell | Qualified single-program file execution, owned environment/identity checks, cancellation/timeout settlement and guarded output acceptance |
| Reviewed workspace jobs | Trusted native macOS workflow execution, retained engine scratch and selected binary artifact custody; not a general production spool ABI |
| Document handoff | Independent example acceptance for saved XLSX versions and fixed downstream PPT consumption; not a built-in Office application |
| State and artifacts | Bounded revision, instance commit, checkpoint/delivery and reclamation contracts; no automatic domain merge or complete rollback |
| Semantics | Pure claims/assessments and existing admission adapters; not comprehensive static program analysis or a security boundary |
| Multi-silk environment | Reviewed macOS coordinator plus two internal silks qualified through workspace jobs; early-leader-exit failure, whole-group cancellation and copied artifact custody; not a registered general spool runtime |
| Release-bound native spool | v1/v2 compatibility, v3 events and explicit byte products, operator-selected Python dependency inventories, independent instances, approved kit edits/reset, cancellation and explicit ledger repair; reviewed macOS headless tools |
| Weaver / zero-mount development | Optional Loom-side reviewed harness example and standalone CLI qualification; not a host agent loop or hostile-harness sandbox |
| Workspace-driven native events | Explicit v3 interfaces, static subscriptions, accepted artifact facts and one foreground delivery per approved event/subscription; consumed-output confirmation and no automatic retry; read-only foreground inventory/watch; no background dispatcher or generic graph |
| Pause/suspend/backup | Backend-specific capabilities to qualify; not general host guarantees |

macOS remains the primary local platform. Existing native Linux implementation
and CI are retained. This documentation does not add a new platform or backend.

## P1: daily event operation

The first slice is a read-only pending-delivery inventory and bounded foreground
change monitor, reusing the P0 delivery identities and evidence. It does not
prepare runs or extend execution authority. Finite explicitly approved dispatch
now reuses those identities for 1..16
selected existing deliveries with fixed state selectors, drain-stop, cancellation
and an exclusive no-resume intent. Per-entry confirmed success gates the next
selection; acceptance/state commit/completion remain explicit. Subscription
admission pause/resume is now explicit and generation-bound, with stale approvals
refused and already-admitted custody retained. It does not replace route targets;
instance/generation fencing and permanent revocation remain in force.
Do not introduce implicit latest-state selection, automatic retries, domain
branch/loop routing, a second Loom scheduler or another execution backend.
Background admission, route replacement and retention retirement
remain separate unqualified operations. This is a staged local operational path,
not a claim that all P1 daily operation is complete.

## Remaining local spool milestones

The immediate event-first slice is now qualified: workspace artifact facts can
drive explicit native delivery, acceptance and causal result confirmation. Keep
business control flow in silk/kit. Real XLSX/PPTX handoff now uses explicit binary
outputs and operator-selected Python through the same event contract. A separate
explicit state slot now combines one fixed business artifact with one committed
checkpoint, including event delivery and stale-commit refusal. Multiple business
inputs, broader environment support and daily
subscription/lifecycle/retention management remain separate work. P1 now starts
with a read-only delivery inventory and change monitor; execution authorization,
state selection and consumption confirmation remain explicit. Low-latency
channels require an actual high-frequency consumer. The runtime milestones below
remain an inventory, not a host business-graph roadmap.

The bounded headless shell route is usable today. It is not yet the complete
local spool host. Track these five milestones rather than a completion percentage
or an unlimited sequence of examples; each may require several focused PRs.

1. **Interrupted completion.** Explicitly reconcile complete execution/custody
   evidence without retrying work. Result and acceptance handoff repair are
   supported; missing termination evidence stays fenced. Close with real write
   failures and refusal cases, not claims of universal crash recovery.
2. **Explicit state continuation.** Bind the existing state/checkpoint primitives
   to a release-bound spool attempt. Selected state, stale-commit refusal and
   continuation without inherited scratch are qualified for one opaque text state
   output. See [native state](../crates/an-agent-host/NATIVE-STATE.md). A fresh load
   requires an explicit saved-version seed; automatic lineage/migration is separate.
3. **Pause and suspension.** Qualify cooperative safe points, explicit resume and
   checkpoint/stop suspension for the native backend. The first
   [round-boundary control](../crates/an-agent-host/NATIVE-CONTROL.md) slice supports
   admission pause, fixed-checkpoint suspension and explicit resume after verified
   settlement/acceptance. In-flight safe-point signaling and process/VM restoration
   remain unqualified; their generic capability flags stay false.
4. **Retention and capacity.** Expose retained references and leases, safely retire
   unreferenced products/input snapshots, and apply backpressure before storage
   exhaustion. [Private snapshot collection](../crates/an-agent-host/NATIVE-STORAGE.md),
   bounded logical storage observation and preparation headroom are qualified.
   Formal products/state remain retained; general cross-instance reference/lease
   retirement and runtime quotas remain outside this slice.
5. **Release compatibility and final qualification.** Resolve declared dependencies
   to exact supported bindings, show update/permission differences and preserve old
   releases/state on rejected updates. [Release review](../crates/an-agent-host/NATIVE-RELEASES.md)
   now verifies package differences, reports unsupported requests and qualifies
   independent fresh-instance continuation while preserving the old state.
   Authorized lineage, in-place updates, external dependency resolution and
   generic migration remain separate, unimplemented capabilities.
   Reuse the two-instance/two-silk acceptance,
   with explicit supported scope and unsupported cases, as the local completion
   gate. Publisher authentication and a public registry are not prerequisites.

Loom owns authoring, verification and delivery. Domain rules stay in the spool;
an-agent owns execution and custody. Further Weaver orchestration, GPU/GUI,
arbitrary hostile harnesses and additional VM backends are outside this local
headless milestone set and require their own qualification.

Preserve CLI/profile/tape compatibility. Unsupported enforcement must refuse,
not silently fall back. Development merges require the existing CI and acceptance
gates plus a user notification; main promotion remains user-triggered.

Completed dispatches now support explicit missing-summary reconstruction from all
original entry/start/delivery and confirmed-success native receipts. Incomplete,
failed or unknown execution remains fenced; reconciliation never runs or resumes
work. Background admission, route replacement and formal retention retirement
remain unqualified P1 gaps.
