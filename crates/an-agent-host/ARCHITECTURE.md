# Host responsibilities and attention boundaries

Status: execution and artifact handoff are implemented in the bounded profiles
linked below. Attention management is a design boundary, not a new runtime
service, graph schema, scheduler, or GNN dependency.

## Workflow package direction

A spool is a carrier of domain knowledge: workflow decisions, domain rules,
skills, algorithms and acceptance criteria belong in its package and kits.
The host must not absorb those rules. It provides execution authority, generic
integrity checks, version provenance and retention mechanisms. Waver helps
developers produce packages without becoming coupled to a particular domain.

### Execution history, package releases and artifact revisions

These are distinct assets, not three implementations of one version system.
Global tape is primarily diagnostic and causal evidence. A spool may explicitly
use its own log as recovery material; neither logs nor base images promise full
execution recovery. Disk snapshots and memory/device checkpoints have different
backend-specific guarantees, and cannot restore external side effects.

The host owns accepted artifact identities, lineage, access and retention.
A spool defines output meaning and domain validation. A backend owns isolation
and environment state; its writable disk is not the authoritative artifact
catalog or an independent backup. Spools may be business black boxes, but their
inputs, permissions, outputs and termination remain explicit boundary contracts.

An accepted revision is immutable. A human edits a separate working copy and
explicitly submits a saved checkpoint as a new revision with a parent reference.
Human-edit provenance is an operator declaration, not authenticated authorship.
Downstream runs bind an exact revision and byte digest; they never follow a
mutable latest pointer. Later edits neither replace running inputs nor authorize
reruns. Divergent edits may share a parent. Domain-specific merging belongs to
the spool or application, not a generic last-writer-wins host policy.

The initial artifact revision profile is bounded UTF-8 file data, not a general
document repository. It preserves accepted run output and explicit edits outside
the VM, verifies identity on every read, and binds selected bytes into the
existing controlled execution path. There is no automatic watching, merge,
garbage collection, VM snapshot, or independent backup claim. An operator must
save a coherent file and approve its byte digest before importing an edit.
Existing native input/output limits still apply.

A spool is intended to be a reusable workflow package: stages, CLI-first kits,
configuration, documentation, dependencies and requested capabilities. A shuttle
is an independently deployed application adapter, not a mandatory component of
every spool. The current embedded-body descriptor and registration contracts
remain unchanged; general multi-stage execution is not implemented by this
document. The optional OpenShell profile implements bounded VM file-kit execution,
not a general workflow scheduler.

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

### Internal execution backend contract

`execution/backend.rs` defines the host-owned `ExecutionBackend` contract.
Native and OpenShell implement validation, plan resolution, bounded execution,
and compatibility receipt decoration. Both enter through the same registration,
AgentControl admission, executable publication and host settlement path. Public
entrypoints, CLI selection, preview fields and existing receipt fields remain
compatible. This is not a stable third-party plugin API yet.

```text
caller -> host admission -> operation intent -> executable publication
                                  |
                           ExecutionBackend
                           /              \
                   Native supervisor   OpenShell lifecycle
                   Seatbelt/Landlock   qualified VM driver
                           \              /
                      typed outcome + candidate
                                  |
                 host reconciliation -> artifact validation
                                  |
                       receipt + tape + accepted product
```

The host persists `operation.json` before publication/submission. It separates
the run ID from a fresh operation ID. An additive `receipt.operation` records
intent, returned observation and cancellation request independently of the
legacy status. Successful outcomes require matching operation identity and
confirmed termination. A VM outcome additionally requires the same non-missing
environment ID and runtime generation at execution and settlement. Native
execution has no VM generation; a reaped child is its existing termination
evidence, not a memory snapshot or VM lifecycle claim.

OpenShell retains its own ownership checks and stop-confirmation sequence. It
returns typed initial/settled identities; host decisions do not parse diagnostic
log text. A mismatch is refused before artifact publication, while stdout,
stderr, process observations and reconciliation error remain inspectable. An
unconfirmed termination stays unconfirmed, not success. Candidate bytes are
bounded data, not permission to write a host path. Native file acceptance keeps
the existing inode/regular-file checks; only the host writes `artifact.txt`.

Cancellation uses the existing request token and backend-specific cleanup.
Cancellation request is not termination confirmation. The current late-cancel
window for OpenShell and native supervisor behavior remain unchanged. No
automatic retry, fallback, restart or adoption of a late result is added.
Publication failures remain separate from backend observations. A missing
receipt after a crash is an unsettled operation requiring investigation, not a
recoverable job queue. Writes retain the existing durability semantics; the new
intent file does not promise power-loss durability or crash reconciliation.

This borrows [SPACE's separation of intent, runtime evidence and guarded completion](https://github.com/perplexityai/SPACE/blob/57c7c0c47a73373a974c5b542d180599795465da/crates/libs/placement/machine/README.md),
not its control plane or placement machinery. Operation IDs correlate trusted
adapter returns; they are not cryptographic remote execution attestations. There is no
distributed fence or compare-and-swap claim: execution is synchronous and the
existing whole-store lock remains held. A future concurrent/remote scheduler
would need persistent claims and guarded commits before relaxing that lock.
Pause, suspension, backup and another VM backend are outside this contract's
current supported operations. Adding one must not turn unsupported capabilities
into native fallbacks or equate disk restart with memory resume.

Acceptance for this boundary: existing native process/file-kit regressions;
shared-contract tests for stale operations, replaced generations, missing
termination evidence and bounded candidates; real registration/admission fault
injection proving rejected candidates never become artifacts while diagnostics
remain available. The opt-in `openshell_live` test also compares operation IDs
and initial/settled VM identities for success, timeout and cancellation.
The macOS OpenShell 0.1.5 real run on 2026-10-23 passed all three paths in 56.92s;
all three confirmed stop, and only success produced an accepted artifact.
This does not qualify crash recovery, another VM backend or distributed fencing.

The model agent and spool payload are untrusted. The host authorization service,
execution enforcement and application-side capability gate are trusted computing
components. A supervising model may propose a plan or ask an operator for a
grant; it cannot edit policy or authorize itself.

The host owns workflow dependencies, admission, context disclosure, budgets,
artifact acceptance and retention decisions. A qualified execution backend owns
VM lifecycle, actual process termination and enforced resource, filesystem and
network restrictions. Prioritize local execution; do not require a remote
orchestration platform or copy its controllers into an-agent. The host records
intent and evidence rather than maintaining a competing VM state machine.
Record requested cancellation separately from confirmed termination; a
disconnected stream is not proof of completion.

Required backend capabilities are admission requirements. Unsupported or unknown
isolation must fail closed, never fall back to an unrestricted local shell.
macOS and Linux adapters need separate acceptance evidence; snapshots are not
assumed portable between them. See [local VM capabilities and limits](LOCAL-VM.md).

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
| Spool | Domain knowledge, workflow rules, skills, stage contracts and domain acceptance; declares dependencies and capabilities, never self-grants |
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
execution is supported; isolated Linux builds, VM pause/suspend and snapshot
recovery remain deferred. No automatic application of results or cleanup is
added by this boundary document.
