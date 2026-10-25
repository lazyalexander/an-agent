# Implementation status and next steps

This page describes the bounded implementation, not all capabilities suggested
by the architecture diagram. An example's success does not widen a runtime grant.

| Area | Current evidence and limitation |
| --- | --- |
| Embedded agents | Existing event/admission/tape and supervision paths; in-process bodies are trusted code, not isolated workloads |
| Native deployment | Registered hash-bound stdio/file profiles with platform-specific enforcement and receipts; not arbitrary code-agent execution |
| Optional OpenShell | Qualified single-program file execution, owned environment/identity checks, cancellation/timeout settlement and guarded output acceptance |
| Reviewed workspace jobs | Trusted native macOS workflow execution, retained engine scratch and selected binary artifact custody; not a general production spool ABI |
| Document handoff | Independent example acceptance for saved XLSX versions and fixed downstream PPT consumption; not a built-in Office application |
| State and artifacts | Bounded revision, instance commit, checkpoint/delivery and reclamation contracts; no automatic domain merge or complete rollback |
| Semantics | Pure claims/assessments and existing admission adapters; not comprehensive static program analysis or a security boundary |
| Multi-silk environment | Agreed target; not implemented as a general spool runtime |
| Pause/suspend/backup | Backend-specific capabilities to qualify; not general host guarantees |

macOS remains the primary local platform. Existing native Linux implementation
and CI are retained. This documentation does not add a new platform or backend.

## Next bounded work

1. Bind production execution entrances to the owning spool release/attempt,
   selected silk implementation, input/state versions and effective host grant.
   Keep operator development/workspace surfaces explicitly separate.
2. Reconcile existing execution paths with common identity, settlement and
   capability evidence. Reuse current acceptance and retention mechanisms.
3. Exercise two internal silks in one spool environment: no inner VM allocation,
   whole-attempt cancellation, remaining-worker termination, stale-generation
   refusal and accepted-output survival after scratch cleanup.
4. Add persistent environments or another backend only for a demonstrated need.
   Keep business sequencing in spool code and the developer workbench in Loom.

Preserve CLI/profile/tape compatibility. Unsupported enforcement must refuse,
not silently fall back. Changes land through reviewed PRs with the existing
cross-platform CI and architecture gates; the maintainer retains merge control.
