# Execution and authority

A managed production attempt must identify its owning spool release, selected
inputs/state, effective grant and backend. Internal silks run within that
boundary. A silk call is not a default request for another VM. Developer tools
may invoke standalone programs, but that alone is not a managed spool execution.

## Host-owned flow

1. Validate immutable release/input identities and the requested operation.
2. Assess declarations and relevant current facts without trusting them as grants.
3. Resolve an operator-authorized plan and an adequate execution profile.
4. Record operation intent before submission. Distinguish host operation identity
   from backend environment identity and runtime generation.
5. Execute inside the selected boundary. Internal domain sequencing remains in
   the spool; external capabilities require the host-authorized interface.
6. Reconcile actual results, cancellation state, environment identity and terminal
   settlement. Validate declared candidate outputs before publication.
7. Commit official outputs/state through their supported host contract and retain
   evidence. Reclaim eligible scratch only after the required settlement checks.

The existing host backend contract implements part of this flow for Native and
OpenShell single-entry deployments. It is internal, not a stable general plugin
API. Reviewed workspace jobs are a separate trusted operator profile; they do
not yet carry full production spool authority or hostile-agent containment.

## Semantic assessment and hard limits

The semantic plane detects and reports findings. It does not schedule work,
allocate VMs, cancel execution or contact a supervising agent. The host/control
layer decides what to do with findings and requests review when necessary.
Execution boundaries enforce permissions independently of model judgment.
A declaration, passing policy check or recorded receipt is not itself enforcement.

## Cancellation, failure and uncertainty

A cancellation request, CLI exit and confirmed environment termination are
separate observations. A silk subprocess exiting does not prove that sibling or
escaped workers stopped. The target whole-spool boundary must account for all
owned execution before settlement or cleanup is declared complete.

Current profiles have different qualified scopes. Native reviewed workflows
require cooperating workers that do not daemonize or escape their process group.
OpenShell additionally requires stop confirmation for the same owned environment
and generation. Neither should be generalized to unsupported workloads.

Unknown submission or termination is retained for reconciliation, not automatically
retried. A timed-out/cancelled attempt cannot publish a late result as success.
Host crashes, external effects and remote workers require evidence beyond the
local process exit. Do not infer durable crash recovery from an operation journal.

## Backend and application boundaries

OpenShell is optional; Native profiles use their existing OS-specific restrictions.
Isolation, credentials, networking, quotas, pause and snapshots are independent
capabilities. Upstream backend support is not proof that our adapter exposes or
has qualified it. Do not add one backend per internal silk.

User applications remain outside the spool environment unless explicitly placed
inside it. A shuttle mediates authorized external reads/changes with selected
versions and actual-effect receipts. Filesystem backup cannot undo a message,
remote API call or arbitrary GUI operation.

Detailed contracts: [host execution](../crates/an-agent-host/README.md),
[security layers](../crates/an-agent-host/SECURITY-LAYERS.md),
[process ownership](../crates/an-agent-host/PROCESS-LIFECYCLE.md),
[OpenShell](../crates/an-agent-host/OPENSHELL.md).
