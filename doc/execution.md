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

## Concurrent native attempts

The host owns lightweight attempt coordination; it does not implement a new
container runtime. Native execution is the first local backend. Container or
microVM support remains an independently qualified backend choice, not a reason
to move artifact custody or domain sequencing into that backend.

The fixed native file profile now allows independent attempts in the same host
store. The store lock protects release resolution, input snapshots and exclusive
run-directory allocation, then is released before admission and process launch.
Each attempt owns its directory, AgentControl thread, operation identity, child
and cancellation flag. Short shared metadata operations still fail closed on
lock contention; there is no new queue, stale-lock repair or automatic retry.
A running child no longer holds that lock for its entire lifetime.

Settlement evidence, not absence of the store lock, gates cleanup. A new attempt
is an explicit invocation with a new identity, not resumption of cancelled memory.
Business state is selected explicitly. OS threads and process memory belong to
the backend; no portable thread snapshot is promised. Accepted file revisions
carry data between attempts, with small control notifications remaining separate.

The real CLI regression in `tests/file_runtime/concurrent.rs` starts two named
spool releases in one store, cancels one through SIGTERM, reclaims its confirmed
cancelled work and starts a fresh attempt while its peer continues. The peer pins
v1, finishes after v2 publication, and retains v1 output after scratch collection.
This qualifies the existing single-entry file ABI, not arbitrary multi-silk
process trees, hostile same-user processes, persistent environments or VM backup.
Loom still authors, checks and packages releases; it owns none of these runtime
locks, grants, cancellation or retention decisions.

For reproducible local evidence:

```sh
AN_AGENT_ACCEPTANCE_DIR=/absolute/path/to/evidence \
  cargo test -p an-agent-host --test file_runtime concurrent -- --nocapture
```

The report includes relative event times, full plans, operation IDs, child PIDs,
receipts, settlement/audit records, revision bindings and reclamation results.
These synthetic test records can contain local paths: inspect before sharing.
CI retains the report per OS; timing is diagnostic evidence, not a benchmark.

## One reviewed body with two silks

The qualified reviewed-body path runs one coordinator and two internal silk
processes through the existing reviewed workspace profile
([platform qualification](platforms.md)). One process group, attempt and
cancellation owner cover the whole round. A producer hands off an internal file;
a consumer offers a candidate, and only whole-round settlement can authorize host
copy/verification and publication. A zero-exit leader with abandoned members is
failed, not successful after forced cleanup. The release-bound native facade also
qualifies two internal silk steps; neither path grants a separate environment per
silk or moves domain sequencing into the host.

The [implementation and stress plan](../crates/an-agent-host/MULTI-SILK-PLAN.md)
separates current regression/soak evidence from future saturation and crash-fault
tests. This is not a general registered multi-silk deployment or a hostile-code
sandbox. Reuse the current authority boundary before designing another package
ABI; Loom remains responsible for development and packaging only.
