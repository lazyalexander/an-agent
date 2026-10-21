# Host responsibilities and attention boundaries

Status: execution and artifact handoff are implemented in the bounded profiles
linked below. Attention management is a design boundary, not a new runtime
service, graph schema, scheduler, or GNN dependency.

## Workflow package direction

A spool is intended to be a reusable workflow package: stages, CLI-first kits,
configuration, documentation, dependencies and requested capabilities. A shuttle
is an independently deployed application adapter, not a mandatory component of
every spool. The current embedded-body descriptor and registration contracts
remain unchanged; general multi-stage execution and remote VM execution are not
implemented by this document.

Keep package identity, run identity and stage-attempt identity separate. A run
binds immutable package/input identities and an approved policy. A stage consumes
scoped context references and accepted upstream artifacts; it produces a report,
context changes and candidate artifacts. The host validates and accepts those
outputs before making them downstream inputs. Failure, cancellation and unknown
outcomes are first-class results, not implicit permission to retry.

Context changes cannot grant capabilities or promote untrusted text into system
instructions. Connection declarations reference host-owned profiles; packages
must not carry live credentials. Reports and model claims are evidence to assess,
not authenticated proof of execution or domain correctness.

## Permission and scheduling boundaries

The model agent and spool payload are untrusted. The host authorization service,
execution enforcement and application-side capability gate are trusted computing
components. A supervising model may propose a plan or ask an operator for a
grant; it cannot edit policy or authorize itself.

The host owns workflow dependencies, admission, context disclosure, budgets,
artifact acceptance and retention decisions. SPACE (or another qualified backend)
owns placement, VM lifecycle, actual process termination and enforced resource,
filesystem and network restrictions. Do not move SPACE controllers into an-agent
or maintain a competing VM state machine. Record requested cancellation separately
from confirmed termination; a disconnected stream is not proof of completion.

Required backend capabilities are admission requirements. Unsupported or unknown
isolation must fail closed, never fall back to an unrestricted local shell.
macOS and Linux adapters need separate acceptance evidence; snapshots are not
assumed portable between them. See [SPACE handoff preparation](SPACE-HANDOFF.md).

Shuttles accept bounded application operations, recheck grants and object
revisions at application time, and return actual-effect receipts. They must not
expose arbitrary host execution as a shortcut around the VM. Receiving an artifact
does not authorize applying it to a user document. A VM snapshot cannot roll back
an external application, sent message or model request.

Fixed-kit execution and code-generation execution are separate proposed profiles.
The former binds reviewed entrypoints and dependencies and avoids general-purpose
interpreter/plugin entrypoints; the latter permits development only inside an
explicitly authorized sandbox. Neither may self-register or publish a new release,
alter trusted policy, acquire production credentials or expand shuttle access.
Writing code-shaped text is not a reliable security boundary: `noexec`, removal
of a compiler and denial of child processes do not prevent all interpreted code.
Static checks are bounded evidence; restrictions still apply to generated code.
These profiles are requirements, not newly implemented runtime modes.

## Ownership

| Component | Responsibility |
| --- | --- |
| Waver | Developer scaffolds, code-tool briefs, source checks, declarations, portable package evidence and delivery |
| Shuttle | Application-native events, narrowly authorized operations, revision checks and actual-effect receipts |
| Spool | Workflow definition, stage contracts, dependencies and requested capabilities; no self-grants |
| Kit | Developer-owned computation and intermediate artifacts, normally through a CLI |
| an-agent host | Concrete execution policy, model connections, scoped context exchange, process lifecycle, audit and artifact reception |
| Core | Existing admission and causal recording contracts; no application-specific attention model or packaging implementation |
| Execution backend | VM/process lifecycle and enforced resource, filesystem and network restrictions; unsupported profiles fail closed |

Waver and the host exchange explicit files and CLI results, not implementation
dependencies. Package checks cannot grant execution rights. The runtime does
not depend on Waver's analyzer or archive format. Kit invocation and code
generation remain separate operations; mature code tools keep their own operator
approval interfaces rather than becoming another runtime inside Waver.

## Three distinct decisions

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

## Current execution and handoff

- [Build/file jobs](FILE-JOBS.md) define isolated compilation, tests and accepted
  outputs. Passing tests does not prove domain correctness.
- [Process lifecycle](PROCESS-LIFECYCLE.md) owns cancellation, deadlines and
  cleanup, with no automatic retry.
- [Artifact context](CONTEXT-OUTPUT.md) exports an explicitly selected,
  receipt-checked view. It is deterministic byte selection, not attention policy.
- [Model CLI](MODEL-CLI.md) separately approves a host-owned model request.

A file kit's writable `work/output.txt`, accepted `artifact.txt`, diagnostic
streams and derived context packet are distinct products. Only the accepted
copy may be selected as `artifact`; stdout is not an implicit fallback. Export
preserves source identity and audience but grants neither model access nor
permission to overwrite the user's document. Waver only packages portable build
evidence; local plans, audit and context selections stay out of that package.

These are bounded local profiles, not authenticated remote attestations or
crash-atomic transactions. Retention remains manual. Native Linux fixed-file
execution is supported; isolated Linux builds and SPACE machine acceptance
remain deferred, and no automatic
application of results or cleanup is added by this boundary document.
