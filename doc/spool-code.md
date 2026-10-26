# Spool, silk and kit code contract

Status: agreed target contract. This document changes vocabulary and authoring
rules, not executable schemas or enforcement. Existing qualified profiles remain
the limit of implementation; a general mutable-kit runtime is not implemented.

## Roles and mutability

| Object | Role | Modification boundary |
| --- | --- | --- |
| Spool release | Immutable domain package containing its pipeline, code, instructions, configuration and acceptance contracts | Selected published bytes never change in place; changes produce a new release identity |
| Spool attempt | Host-owned execution, authority and lifecycle boundary | Explicit inputs and state; no implicit inheritance from prior scratch |
| Silk | Core domain implementation and pipeline skeleton, including stable step contracts and domain acceptance rules | Read-only within the selected release and attempt; development changes require validation and a new release |
| Kit | Internal glue for adaptation, conversion and connecting steps | An agent may modify explicitly editable working copies within the owning spool's grant |

These are responsibility classes, not language, file-extension or directory-name
rules. A silk may invoke an agent or an existing CLI. A kit may be a generated
script or a packaged component. Neither receives a separate VM or grant by default.
A component that decides authoritative acceptance, protected sequencing or other
core domain invariants must be treated as silk even if its author calls it glue.
Host permissions and host acceptance cannot be redefined by either class.

## Kit origins and lifetime

- **Packaged kit:** its baseline bytes are immutable parts of the spool release.
  Adaptation edits a separate working copy, never the package or a shared baseline.
- **Temporary kit:** generated during work, normally treated as scratch files.
  Being executable or useful does not automatically make it state or an artifact.

Both may be repaired by an agent when they fail or drift from the assigned goal,
within explicit editable paths and task constraints. A kit cannot edit silk,
protected checks, the effective host policy or its own authorization. If a change
requires those edits, request a separately authorized development task/new release.
Silk is not immutable forever: its development source can evolve under that route.

Modification does not authorize execution. Before an execution uses changed code,
bind a fixed revision/digest and the effective grant using a qualified host route.
Do not write an executable while launching it or treat a hash as proof of safety.
Existing selected-file snapshots do not freeze all executables or dependencies;
unsupported enforcement remains a disclosed gap, not permission for self-update.

## Retention and promotion

Unregistered temporary kits and working copies are eligible for scratch collection
only after whole-attempt settlement is confirmed. They are not deleted merely
because one subprocess exited. Uncertain termination fences collection. Partial
results are not automatically promoted, inherited by the next attempt or retried.
There is no new periodic garbage collector implied by this contract.

If a later attempt needs an adapted kit, explicitly retain its exact bytes and
reference them as an admitted input/checkpoint dependency. Declared input formats
and the next grant must allow it. The host preserves selected bytes and references;
the spool determines whether they are sufficient for business continuation.
An accepted artifact may contain code as data; custody alone never grants execution.

Promote reusable changes through reviewed development, checks and packaging into
a new immutable spool release. Keep the original release available according to
its retention policy. Do not redefine release identity from a mutable workspace.

## Division of responsibility

| Owner | Responsibility |
| --- | --- |
| Domain spool/developer | Silk invariants, kit interfaces, allowed adaptation points, domain tests and acceptance meaning |
| Loom | Prepare bounded code-agent tasks with editable/protected paths; coordinate checks; package explicitly selected bytes and preserve provenance |
| an-agent host | Admit execution, enforce authority through a qualified backend, bind selected versions, observe external effects, settle attempts and manage retained state/artifacts/scratch |
| Backend | Enforce the admitted filesystem/process/environment boundary and provide termination evidence |
| Shuttle | Mediate separately authorized external application interaction |

Loom is not a runtime policy engine or a replacement coding agent. an-agent does
not decide domain algorithms or automatically rewrite a failing kit. Mature coding
tools may perform the edits; their own context/tape remains theirs unless a separate
integration explicitly provides host tracing. Domain glue never belongs in core.

## Compatibility and next validation

Legacy `[kit]` authoring, `Kit` aliases, `kit/` paths, `file-kit-v1`, `build-kit` and
`kit.*` events keep their existing parsing and wire meaning. `[kit]` remains an
alias for the existing `[silk]` declaration, not a declaration of editable glue.
The new conceptual kit role adds no manifest field in this documentation change.
Historical Silk/Silksong communication records retain their original schema.
Never infer edit permissions from a legacy token or rewrite old releases/tape.

The next implementation slice should use one lightweight spool: keep its silk
read-only, adapt one kit working copy, execute the selected fixed bytes, retain
only declared outputs, and reclaim unretained copies after confirmed settlement.
It must also reject attempted silk/policy edits and preserve immutable release
bytes. Reuse existing host and Loom contracts; do not introduce a generic pipeline
engine, a new backend or blanket code-execution authority to make the example pass.
