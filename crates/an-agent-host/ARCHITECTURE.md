# Host responsibilities and attention boundaries

Status: execution and artifact handoff are implemented in the bounded profiles
linked below. Attention management is a design boundary, not a new runtime
service, graph schema, scheduler, or GNN dependency.

## Ownership

| Component | Responsibility |
| --- | --- |
| Waver | Developer scaffolds, code-tool briefs, source checks, declarations, portable package evidence and delivery |
| Shuttle | Application-native events and translation between application objects and kit inputs/results |
| Spool / kit | Developer-owned domain computation and optional assistance/context-selection policy |
| an-agent host | Concrete execution policy, model connections, scoped context exchange, process lifecycle, audit and artifact reception |
| Core | Existing admission and causal recording contracts; no application-specific attention model or packaging implementation |
| OS backend | Enforced process, filesystem and network restrictions; unsupported profiles fail closed |

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
crash-atomic transactions. Retention remains manual, native Linux build/file
execution and SPACE machine acceptance remain deferred, and no automatic
application of results or cleanup is added by this boundary document.
