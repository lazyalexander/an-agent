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
| Release-bound native spool | v1/v2 requirements, two internal silks, independent instances, approved kit edits/reset, fixed products, cancellation, status and explicit ledger repair; reviewed macOS shell tools only |
| Weaver / zero-mount development | Optional Loom-side reviewed harness example and standalone CLI qualification; not a host agent loop or hostile-harness sandbox |
| Pause/suspend/backup | Backend-specific capabilities to qualify; not general host guarantees |

macOS remains the primary local platform. Existing native Linux implementation
and CI are retained. This documentation does not add a new platform or backend.

## Remaining local spool milestones

The bounded headless shell route is usable today. It is not yet the complete
local spool host. Track these five milestones rather than a completion percentage
or an unlimited sequence of examples; each may require several focused PRs.

1. **Interrupted completion.** Explicitly reconcile complete execution/custody
   evidence without retrying work. Result and acceptance handoff repair are
   supported; missing termination evidence stays fenced. Close with real write
   failures and refusal cases, not claims of universal crash recovery.
2. **Explicit state continuation.** Bind the existing state/checkpoint primitives
   to a release-bound spool attempt. Demonstrate selected state, stale-commit
   refusal and continuation in a fresh environment without inheriting scratch.
3. **Pause and suspension.** Qualify cooperative safe points, explicit resume and
   checkpoint/stop suspension for the native backend. Do not equate SIGSTOP or a
   process/VM image with durable business recovery. Unsupported modes stay false.
4. **Retention and capacity.** Expose retained references and leases, safely retire
   unreferenced products/input snapshots, and apply backpressure before storage
   exhaustion. Confirmed termination and state/downstream references gate cleanup.
5. **Release compatibility and final qualification.** Resolve declared dependencies
   to exact supported bindings, show update/permission differences and preserve old
   releases/state on rejected updates. Reuse the two-instance/two-silk acceptance,
   with explicit supported scope and unsupported cases, as the local completion
   gate. Publisher authentication and a public registry are not prerequisites.

Loom owns authoring, verification and delivery. Domain rules stay in the spool;
an-agent owns execution and custody. Further Weaver orchestration, GPU/GUI,
arbitrary hostile harnesses and additional VM backends are outside this local
headless milestone set and require their own qualification.

Preserve CLI/profile/tape compatibility. Unsupported enforcement must refuse,
not silently fall back. Development merges require the existing CI and acceptance
gates plus a user notification; main promotion remains user-triggered.
