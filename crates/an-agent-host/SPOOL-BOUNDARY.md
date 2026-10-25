# Spool execution boundary and internal silks

Status: accepted architecture; existing bounded execution profiles remain in
place. This document does not claim a multi-silk runtime or change any grant.

## Objects and authority

A spool release is an immutable domain package: workflow code, internal silks,
skills, configuration, dependency bindings and domain validation. A spool instance
binds a release and committed business state. A spool execution attempt is the
host-owned unit of execution isolation, resource ownership and lifecycle.
Several silks can run inside that one attempt's granted environment.

A silk is an internal callable computation, implemented by a script, binary,
existing tool or agent. It does not independently acquire a host identity, grant,
VM, credential or artifact catalog. A reusable silk may be shared in development,
but managed execution must bind its selected bytes to the owning spool release.
Independent CLI use remains possible outside an-agent and carries no host guarantee.

The host admits a spool attempt and selects its backend. Silks inherit at most
the attempt's effective authority. An inner call does not create a new sandbox
by default. Isolation between mutually distrusting silks requires an explicit,
separately authorized boundary; it is not implied by their names or call count.
Separate spool attempts must not share writable execution environments implicitly.

```mermaid
flowchart TB
    Loom[Loom: develop, validate, package] --> Release[Immutable spool release]
    Release --> Host[Host: admission, grants, attempt identity]
    Host --> Adapter[Replaceable execution backend adapter]
    Adapter --> Boundary[Spool attempt: one granted execution environment]
    Boundary --> A[Internal silk: CLI]
    Boundary --> B[Internal silk: agent or existing tool]
    A --> B
    Boundary --> Candidate[Candidate state and declared outputs]
    Candidate --> Accept[Host reconciliation and acceptance]
    Accept --> Store[Committed state and immutable artifacts]
    Boundary --> Request[External capability request]
    Request --> Host
    Host --> Shuttle[Authorized application shuttle]
```

OpenShell is an optional backend below host execution authority, not a mandatory
runtime, a semantic dependency or a per-silk wrapper. Native enforcement remains
available within its qualified profiles. No unavailable or insufficient backend
may silently fall back to weaker execution. A VM label does not establish all
filesystem, network, credential or termination guarantees.

Semantic assessment receives transport-neutral facts and returns findings. It
neither imports OpenShell nor owns scheduling, cancellation or authorization.
The host evaluates findings and enforces hard ceilings; backend adapters translate
policy and observations, never domain meaning or publication authority.

## State and lifetime

The lifecycle belongs to the spool attempt, not to each silk subprocess. A silk
exit does not prove that the environment's other workers have stopped. Cancellation
and timeout concern the whole owned attempt. An unconfirmed termination fences
publication, continuation and reclamation. A stale environment generation cannot
submit results for a replacement attempt.

The host retains committed instance state, official artifacts and boundary
receipts outside the disposable environment. Internal files are scratch unless
explicitly retained for recovery. Stopping or replacing the environment does not
change committed state, restore process memory or reverse external effects.
A future commit during a live attempt requires its own coherent-checkpoint
contract; the current profiles still require successful terminal settlement.
External application changes stay behind host-authorized shuttles with revision
preconditions and actual-effect evidence.

## Implementation audit

| Existing surface | Actual scope | Gap to the target |
| --- | --- | --- |
| `execution/backend.rs` and `environment.rs` | Native/OpenShell delegation, operation identity, generation reconciliation and host-owned acceptance | Contract is internal and supports bounded one-program deployments, not an internal silk dispatcher |
| Embedded `SpoolBeat` bodies | Existing in-process host integration | No execution isolation is implied by mounting a body; retain as a trusted embedding surface, not an untrusted spool runner |
| Native stdio/file profiles | Release-bound entry executable, platform restrictions and process supervision | Do not widen grants to support arbitrary agents or internal workers |
| OpenShell file profile | One owned sandbox per invocation; stop and identity confirmation before accepting one candidate | Treat as a single-entry spool-attempt specialization, not a new VM per internal silk; shared internal execution is not implemented |
| Reviewed workspace jobs | Operator-selected trusted workflow, scratch and binary output custody | Separate acceptance/development surface; not a production spool authority or hostile-agent boundary |
| Model and application services | Separate host-controlled services and shuttle responsibilities | Do not inherit authority from an environment label or an internal silk request |

## Incremental migration

1. Adopt spool/silk terminology in architecture and new authoring output. Preserve
   existing CLI/wire spellings and immutable releases; do not rewrite history.
2. Make every production execution entry explicitly bind an owning spool release,
   attempt, input/state versions and effective operator grant. Keep operator-only
   development/workspace jobs visibly separate until they satisfy this contract.
3. Define the smallest common plan/settlement fields from those real paths. Bind
   selected backend capabilities to platform/version/profile evidence. Unsupported
   requests are rejected; semantic advice does not choose a weaker backend.
4. Qualify one spool with two internal silk steps in one owned environment. Verify
   release/input binding, no extra environment per call, whole-attempt cancellation,
   surviving descendants, stale-generation refusal and unchanged artifact custody.
   Keep business sequencing inside the spool, not in a generic host DAG engine.
5. Only then consider persistent environments or another backend. Pause, memory
   suspension, snapshot backup, crash reconciliation and stronger process-tree
   containment each require separate evidence. No new VM controller is implied.

## Naming compatibility

The former callable `kit` is now a `silk`. Existing `file-kit-v1`, `build-kit`,
`kit.request`/`kit.execute`/completion events, source paths and Rust API spellings
are compatibility tokens until separately migrated with versioned acceptance.
They do not define an independent execution boundary.

Historical Silk protocol and Silksong records refer to the old communication
vocabulary. They are not executable silks and must not be decoded or relabelled
as such. Future communication contracts need distinct names or explicit schema
identities; this change does not migrate that protocol or rewrite its tape.
