# Architecture and vocabulary

The goal is natural assistance inside existing applications. Users work on their
documents, models and canvases; host integrations supply bounded context and
request useful work. Text interaction remains available when intent or authority
needs clarification. Domain algorithms do not belong in the kernel.

## Responsibilities

| Component | Responsibility |
| --- | --- |
| an-agent host | Admission and grants, model/context services, execution identity, lifecycle coordination, committed state, accepted artifacts, delivery and retention evidence |
| Core | Existing event/tape, identity and admission/control primitives; not a domain pipeline engine or VM manager |
| Spool release | Immutable domain code, internal silks, skills, dependencies, state formats and domain acceptance rules |
| Spool instance | A release-bound business identity with committed state |
| Spool execution attempt | The target unit of execution isolation, authority and lifecycle |
| Silk | Release-bound core domain code and pipeline skeleton; read-only during an attempt |
| Kit | Adaptable internal glue; packaged baselines are immutable, explicitly editable working copies are scratch by default |
| Shuttle | An authorized bridge into an external application, with modification preconditions and observed results |
| Execution backend | Actual isolation and process/environment control; OpenShell is one optional implementation |
| an-loom | Developer workbench for authoring, code-tool assistance, checks and packaging; never runtime authorization |

```mermaid
flowchart TB
    Developer[Domain developer and existing code tools] --> Loom[Loom: develop, validate, package]
    Loom --> Release[Immutable spool release]
    Release --> Host[an-agent host: admission and effective authority]
    Host --> Backend[Replaceable backend: Native or OpenShell]
    Backend --> Attempt[Spool execution attempt]
    Attempt --> A[Internal silk A]
    Attempt --> B[Internal silk B]
    A --> K[Kit working copy: bounded glue]
    K --> B
    Attempt --> Output[Candidate state and products]
    Output --> Acceptance[Host settlement and acceptance]
    Acceptance --> Custody[Committed state and artifact revisions]
    Attempt --> Request[External capability request]
    Request --> Host
    Host --> Shuttle[Authorized shuttle]
    Shuttle --> App[Existing application and editable document]
    App --> Receipt[Observed effect receipt]
    Receipt --> Host
```

This is the target responsibility model. Existing single-entry profiles cover
only a bounded specialization. In-process `SpoolBeat` bodies remain trusted
embedding code; mounting a declaration does not place that code in isolation.
The native facade qualifies bounded multi-silk CLI execution; arbitrary agent
containment and a general multi-backend spool ABI remain unimplemented.

## Local execution strategy

Spool interaction is workspace-event routing, not a host business workflow graph.
Silk/kit owns domain branches, loops and delegation decisions. The host owns
subscriptions, admission, attempt supervision and fixed-version delivery evidence.
The [native event slice](native-events.md) connects accepted
artifact facts to separately approved native attempts through explicit foreground
dispatch. It adds no graph DSL, background scheduler or low-latency channel.
The [native Python extension](native-spool.md) binds an
operator-selected environment and explicit binary products to that same boundary.
Spool packages declare requests; local runtime selection and grants stay host-owned.
Subagents use the same authority boundary; shuttles remain external-application
bridges. Future high-frequency data needs a separately granted channel rather
than forcing every sample into the durable workspace log or tape.

Own a lightweight spool execution manager, not a new container implementation.
Reuse the native backend first, with independently owned concurrent attempts.
Future container or microVM adapters provide enforcement and lifecycle evidence;
the host continues to own grants, accepted versions and inter-spool delivery.
Release identity, business state, live process state and filesystem backups are
distinct. A restart selects explicit inputs/state and creates a new attempt.

## Dependency and trust direction

Runtime crates do not depend on Loom or a domain workflow example. Domain
sequencing and checks stay in the spool. The host does not need to interpret
business state to retain and version it. Backend-specific policy/state is
translated by host adapters; it does not enter the pure semantic crate.

The spool requests capabilities; the host grants them. Package instructions,
source-analysis reports, generated YAML and model findings cannot create grants.
A silk cannot select a weaker backend or write directly to the formal catalog.
Backend support must be qualified for its platform, version and effective policy.
There is no silent weaker fallback when requirements cannot be enforced.

The [spool code contract](spool-code.md) distinguishes stable silks from adaptable
kits. This is a target contract, not new mutable-code enforcement. Legacy `file-kit-v1`, CLI
commands, `kit.*` events, paths and compatibility APIs retain their spelling.
Historical Silk communication protocol/Silksong records are a different schema,
not executable silks; no history is rewritten by the naming change.

For the detailed audit and migration, see [the spool boundary](architecture.md).

## Implementation boundary audit

The runtime dependency direction matches the intended ownership: runtime Cargo
manifests do not depend on Loom, and Loom does not link runtime crates. Domain
algorithms remain in release payloads/examples. A generated declaration requests
capabilities; it cannot grant them. Registration publishes selected bytes without
activating an execution.

The host execution facade keeps its existing public functions and CLI:

| Module | Owns | Must not own |
| --- | --- | --- |
| `execution/deployment.rs` | Deployment validation and immutable registration | Execution authorization or domain compilation |
| `execution/mod.rs` | Public entrypoints and explicit backend selection | Inline backend implementation or packaging policy |
| `execution/attempt.rs` | Input selection, allocation lock, operation identity, backend result reconciliation, output acceptance and receipt | Domain algorithms or backend isolation mechanisms |
| `execution/admission.rs` | Kernel request/reply validation and final tape/audit settlement for the same attempt | Process spawning or artifact acceptance |
| `execution/backend/` | Native/OpenShell translation to the existing execution contract | Release assembly or independent grants |
| `settled_run`, `instance`, `artifact_versions`, `retention` | Evidence gates, committed state, product custody and collection | Interpretation of domain state or automatic recovery of external effects |

The attempt carries its admitted identity through receipt production into audit
settlement. Ordering is unchanged: selected input and plan, kernel admission,
operation intent, executable publication, backend result reconciliation, candidate
acceptance, receipt, audit and final settlement seal. A partial finish is not a
settled run. Shared allocation locking still ends before independent execution;
this refactor adds no global lifecycle lock, retries or weaker fallback.

Loom's host adapter is deliberately coupled to qualified CLI/receipt formats.
That is a development client dependency, not runtime ownership. Offline source
inspection and build-receipt validation are not sandbox enforcement. The host
still decides whether the received release and requested execution are admissible.

### Limits retained by this refactor

- Trusted in-process `SpoolBeat` embedding and isolated CLI attempts remain distinct
  paths. A declaration does not sandbox an in-process body.
- Legacy deployments still support bounded one-program profiles. The native
  facade adds declared silk/kit snapshots and approved between-run edits, not a
  general mutable-code or hostile-agent execution contract.
- Loom's host CLI transport is trusted operator tooling, not a sandbox for a
  model-selected executable. Its response-size check occurs after capture.
- No wire format, legacy name, CLI, permission scope or domain acceptance rule is
  migrated here. A new profile must earn integration evidence before broadening
  the interfaces; a generic plugin/control-plane framework is not a prerequisite.

## Attention is not authority


| Question | Authority and mechanism |
| --- | --- |
| Does a known dependency need updating? | Explicit application dependencies, input revisions and invalidation rules |
| Is optional assistance worth proposing now, and with which context? | A replaceable, bounded rule or learned policy, returning a candidate request |
| May that request execute or disclose data? | Host admission and grants, checked again at the actual execution/model boundary |

A learned relevance score must not suppress required dependency invalidation,
audit evidence, cancellation or enforcement. Conversely, a dependency change
does not authorize immediate expensive work: the host may coalesce it, defer it
or reject work against an obsolete revision. Application-local preview loops
remain separate from slower agent reasoning.

Attention selects candidates; it does not grant capabilities, rewrite roles,
silently move context between audiences, choose secrets or override a denial.
Relevance, factual confidence, provenance and permission are different concepts.
High relevance cannot turn untrusted source text into instructions.

## Requirements for a future attention consumer

Use native object identities and revisions when applications provide them.
Names, paths, aliases and text-extracted entities are adapter observations, not
authoritative object identity. Model relevance for an object at a particular
event/revision; do not treat the last label as a permanent property of an object.

A candidate must remain traceable to its observations, input revisions, policy
identity and intended audience. Execution still needs its own approved plan.
Do not let an optional policy erase the evidence of a skipped or superseded
request, or mutate already recorded facts. Keep raw high-frequency UI samples
at the application edge; only appropriate bounded facts cross the host boundary.

Start with explicit subscriptions, coalescing and bounded rules when a real
cross-application consumer needs them. Compare any learned policy against those
baselines using useful accepted work, missed updates, false interruptions,
staleness, latency and total model/context cost. Trigger-stage latency is not
end-to-end task latency or a token-savings guarantee.

[Temporal-graph triggering and routing](https://arxiv.org/abs/2605.30152)
is a design reference for separating attention from generation. No implementation
or benchmark result from that paper is incorporated here. Do not introduce a
GNN or a generic workflow framework before independent consumers justify it.
