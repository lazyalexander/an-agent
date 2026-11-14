# Spool content labels: silk and kit

Status: agreed definition, revision 3. Labels describe provenance and lifetime,
not code responsibilities, language, file extension, directory or authority.
Existing qualified profiles remain the execution limit. This revision does not
implement arbitrary runtime editing or a general overlay filesystem.

## Immutable baseline and mutable derivatives

| Content | Label | Rule |
| --- | --- | --- |
| Every selected release file, including programs, configuration, skills and AGENTS.md | silk | Immutable baseline; change through reviewed development and a new release |
| An agent-modified working copy of any baseline file | kit | Separate derivative; never overwrite the selected release |
| New files, generated programs and other uncommitted work | kit | Attempt-owned by default; not automatically retained or executable |
| Accepted artifacts and committed state | Separate custody records | Explicit acceptance/commit and retention; never delete with scratch |

A file under a legacy `kit/` directory is still silk inside the release. Its
unchanged working copy retains the silk label. Once modified it is kit, including
after reset; the modification record and baseline match are reported separately.
Reset restores bytes; it does not turn a mutable working copy into a release.
File kind (program, configuration, instructions, data) is independent of the label.
A program does not become silk merely because it implements core domain logic.

## Editing and instruction protection

Programs and configuration may be adapted only in separately authorized working
copies, within the selected host profile's supported editable paths. The baseline
remains available. Record the owning instance/attempt, logical path, source release,
base digest when present, and derivative digest. A new file has no baseline digest.

Skill instructions, SKILL.md, agent.md and AGENTS.md are protected during attempts.
Updating the instruction set requires reviewed development and a new release.
New instruction-looking files in scratch remain inert kit data: they cannot be
loaded as instructions, widen the assignment or replace the selected instructions.
Protect instruction resources regardless of their directory or casing. A renamed
instruction bundle is not editable configuration merely because of its extension.

Kit is a label, never a permission. Labeling a file kit does not grant editing,
execution, dependency installation, network access or access to another attempt.
Configuration edits that affect entrypoints, dependencies, authority or host policy
require fresh validation/approval; package instructions cannot grant host rights.
Before execution, bind fixed selected bytes and their effective authority using a
qualified host route. Do not run code while it is being changed.

## Settlement, reset and collection

Unretained kit is eligible for reset or scratch collection only after confirmed
whole-attempt settlement. Unknown termination fences cleanup and automatic retry.
Reset affects local working bytes, not external effects already performed.
No background collector or automatic checkpoint inheritance is implied.

Retain a useful kit explicitly as fixed bytes and an admitted input/dependency
for later work. Artifact custody is not execution authority. Promotion to silk
requires review, checks and a new immutable release; changing a label is not a
release operation. Accepted products and committed state use their own retention
and reference rules and survive ordinary kit/scratch collection.

## Ownership and compatibility

The domain spool carries knowledge, instructions, programs and business acceptance
meaning. Loom prepares bounded tasks, records protected/editable paths, checks
candidates and packages selected bytes. The host admits execution, enforces grants,
settles attempts and owns state/artifact/scratch custody. Backends provide isolation
and termination evidence; Shuttle mediates authorized application interaction.
Mature coding harnesses own their editing/model loop and native context.

Existing `[silk]` / `[kit]` fields, `silk/` / `kit/` paths, `Kit` aliases,
`file-kit-v1`, `build-kit`, `kit.*` events and historical Silk communication records
keep their wire and parsing meaning. They are compatibility bindings, not the label
definition. Old release identities and historical receipts are never rewritten.

The current native profile supports fixed selected files and approved between-run
working-copy updates through legacy `kit/` slots. That restriction is a profile
limit, not a claim that only glue can become kit. Arbitrary-path overlays, adding
executable files or configuration slots, and runtime self-editing need a separately
qualified host route. A new-file kit label alone must not bypass that restriction.

## Implemented observation and next qualification

Host instance inspection reports release files as silk, unchanged working copies as silk and modified copies as
kit, and unregistered generated work as kit by default. It retains the original
`kits` digest map for compatibility. These observations grant no authority and do
not replace artifact/state custody records.
Loom task inventories label all baseline files silk; received candidates are kit
with baseline provenance. Checked publication produces a new silk release.

Next qualify a directory-independent overlay with explicit new-file admission:
keep instruction resources immutable, bind configuration changes and new executable
bytes to fresh approval, prove baseline preservation/reset, retain declared products
and reclaim remaining kit only after confirmed settlement. Reuse current host and
Loom contracts; do not add a generic workflow engine or infer grants from labels.

## Proposed progressive disclosure for the single-main baseline

The following is a benchmark design for information exposed by spools. Its initial
implementation belongs in fixture metadata and existing discovery entrypoints.
Runtime manifests retain their current accepted fields. Several parsers reject
unknown fields, so adding these ideas to a release manifest requires a separately
reviewed schema change. Measure the proposed fields before adopting them as a
public contract. [Benchmark protocol](testing.md#single-main-agent-benchmark-v1)

### Information needed at each decision

| Layer | Exposed information | When needed |
| --- | --- | --- |
| Index card | Release-bound identity, short capability description, input/output kinds and a detailed-contract reference | Discover plausible capabilities |
| Detailed contract | Exact schemas, prerequisites, declared effects, resource requirements, entrypoint and acceptance references | Check fit and prepare a bounded invocation |
| Selected knowledge | Release-bound skills, domain rules and supporting material needed for the current step | Resolve domain decisions or adapt authorized working files |

Keep index cards small and measure their actual serialized size. Try different
description lengths in the benchmark before imposing a permanent token budget.
Reuse existing identity, description and requirements fields. Link the card and
knowledge resources to the selected release digest so the host can detect stale
references. A loaded skill remains subject to the instruction-protection and kit
rules above.

The host attaches current availability, compatible execution profile and grant
eligibility to discovery results. Availability and issued authority can change
between discovery and execution. Recheck them at invocation. A spool's declaration
of requested access supplies input to authorization. Methods receive the existing
host-verifiable grant reference, narrowed to the task and operation.

### Initial execution choices

The main agent can choose a declared deterministic program when the inputs and
preconditions are satisfied. It can load selected knowledge into its current
session when the task needs domain interpretation or an authorized adaptation.
It can request missing information or report an unsupported capability. Programs
may overlap when the host permits their resource usage.

The host validates each chosen invocation and owns admission, resource scheduling
and settlement. The benchmark records the choice and its eventual outcome. This
lets us measure whether the available information improves early decisions.
Architecture changes remain subject to comparative evaluation.

### Candidate hints to evaluate

| Information | Decision it can inform | Validation and accounting |
| --- | --- | --- |
| Required inputs, produced outputs and dependencies | Whether the task is ready and which steps depend on earlier results | Verify schemas and selected artifact revisions |
| Deterministic entrypoint and knowledge references | Direct program execution or knowledge loading | Verify release binding and callable availability |
| Independently checkable subresults | Whether work can be decomposed | Test independence against domain oracles |
| Shared files, app documents and exclusive resources | Whether program executions can overlap | Enforce conflicts in the host and Shuttle |
| Effects, idempotency, cancellation and recovery capabilities | How to handle interruption or an uncertain outcome | Require qualified backend/adapter evidence |
| Knowledge volume and estimated context demand | Which resources to disclose and in what order | Record bytes or tokenizer/version, then measure actual disclosure |
| Observed duration, usage and success by task shape | Whether a previous execution mode is promising | Host-owned measurements with sample count, environment, model and version |

Treat all spool-supplied hints as advisory. Unknown values stay explicit.
Only the host's observations establish current capability or historical cost.
Resolve uncertainty using the original operation identity and existing lifecycle
rules. Descriptions of recovery or cancellation cannot authorize cleanup.

Start with identity, purpose, I/O, requirements and knowledge references, deriving
them from the existing release where possible. Add hints only when an observed
selection or scheduling failure demonstrates missing information. Evaluate the
catalog-size, context-pressure, work-shape and unseen-spool cases against eager
full-context loading. The same accepted result must be obtainable through each
tested discovery configuration.

Record the selected card/revision, eligibility checks, chosen execution mode,
resources disclosed and final result. Use these records to refine the fields and
selection policy. A spool's preferred agent topology would introduce an additional
policy dependency, so the initial proposal leaves that decision to measured host
policy and explicit comparative experiments.
