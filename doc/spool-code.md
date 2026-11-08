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
