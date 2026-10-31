# Native spool runtime and round control

## Release-bound native spool: reviewed headless profile

Status: implemented for reviewed local shell tools and explicitly selected
[native Python environments](native-spool.md). This is a bounded spool
facade over the existing workspace supervisor, snapshots and custody machinery.
Platform coverage is maintained in the [runtime matrix](platforms.md).
It is not an arbitrary agent sandbox, general dependency resolver or replacement
for trusted in-process `SpoolBeat` mounting. No core or Loom runtime dependency is
introduced. Domain sequencing remains in the entry silk.

Optional [explicit native state](native-state.md) binds a selected checkpoint to a
run and commits an accepted state output with an expected-version check. It uses
the same release; v3 can select independent data and state inputs. Ordinary runs stay stateless.

[Round-boundary control](native-spool.md) pauses admission after confirmed
settlement, pins a checkpoint for suspension and explicitly reopens admission.
It does not pause or restore a running process.

[Native storage](native-storage.md) separates work collection from private input
snapshot collection and formal state/artifact retention. It also adds native
workspace admission headroom without a runtime filesystem quota.

[Release review](native-state.md) compares exact verified package bytes and
requested contracts without authorizing an update. Candidate continuation uses
a fresh independently approved instance; old releases and state remain retained.

[Native workspace events](native-events.md) adds explicit v3 event interfaces and
host-owned artifact delivery. V1/v2 releases remain unchanged and do not acquire
event rights implicitly. Business sequencing stays inside silk/kit.

[Native Python and binary products](native-spool.md) adds operator-selected,
inventory-bound dependencies and explicit byte outputs to v3. V1/v2 and existing
shell packages retain their behavior; no package performs dependency installation.

### Contract and layout

Loom's `export-native-spool` exports `native-spool.json` and selected `silk/` and
`kit/` files. Any producer can emit this versioned JSON contract; the host validates
it independently. This new contract does not reinterpret existing `spool.yaml`,
`file-kit-v1`, `[silk]` or legacy `[kit]` formats.

The manifest has exactly `schema` (`an-native-spool-v1`), `name`, `version`,
`entry`, `files` (relative code path to SHA-256), and `outputs`. Code paths have
one role directory and an ASCII basename, unique across both roles. Entry must
be a declared silk. At most 12 code files, 1 MiB each and 8 MiB total are allowed.
Outputs are 1–16 distinct top-level text filenames, excluding `kit` and `tmp`.
Existing text-output and workspace inventory limits still apply. Name/version
labels do not authenticate a publisher, establish update lineage or imply SemVer.
The release digest hashes the exact manifest bytes and transitively binds code.
Unknown fields, unlisted files, symlinks, hardlinked payloads and mismatches refuse.

Loading exclusively creates a new instance directory and copies the verified
release plus separate kit working copies. No package code or install hook runs.
The instance UUID is distinct from the release digest. All files remain host-owned;
workloads cannot write the instance store, baseline, profile or permission data.

```text
INSTANCE/
  instance.json
  release/native-spool.json, release/silk/*, release/kit/*
  kit/*                         # edit between runs through host approval
  round-control.json            # optional admission mode and generation
  round-control/*               # immutable transition evidence and staging
  runs/RUN/
    profile.json, start.json, result.json
    accepted.json, reclaimed.json   # bound custody results when present
    job/                        # existing private workspace execution layout
  unloaded.json                 # terminal load lifecycle marker
```

A load is not a registration into the legacy embedded registry. This slice binds
one verified package to a new host instance directly. Reload uses a fresh instance
directory; existing instances and their evidence are not overwritten. Loading
under the source release directory is refused. At most 64 run names are allocated
per instance. A used run name is never automatically retried.

### Commands and explicit authority

```text
an-agent-host spool-load PACKAGE NEW_INSTANCE
an-agent-host spool-show INSTANCE
an-agent-host spool-reconcile-run INSTANCE RUN [--approve DIGEST]
an-agent-host spool-kit-update INSTANCE BASENAME SOURCE [--approve DIGEST]
an-agent-host spool-kit-reset INSTANCE BASENAME [--approve DIGEST]
an-agent-host spool-start INSTANCE RUN INPUT SECONDS [--approve DIGEST]
an-agent-host spool-supervisor start INSTANCE RUN INPUT SECONDS [--approve DIGEST]
an-agent-host spool-supervisor show INSTANCE RUN
an-agent-host spool-supervisor cancel INSTANCE RUN [--approve DIGEST]
an-agent-host spool-pause INSTANCE [--approve DIGEST]
an-agent-host spool-suspend INSTANCE STATE_REVISION [--approve DIGEST]
an-agent-host spool-resume INSTANCE [--approve DIGEST]
an-agent-host spool-accept INSTANCE RUN [--approve DIGEST]
an-agent-host spool-reclaim INSTANCE RUN [--approve DIGEST]
an-agent-host spool-unload INSTANCE [--approve DIGEST]
```

Editing, start, acceptance, reclaim and unload first return a plan and approval
digest. Start preview can persist a bounded profile but never launches. The start
approval binds instance identity, release, run name, selected kit/input hashes,
effective native policy and deadline. Reusing an approval after an input or kit
change fails. Loading and showing do not create execution permission.

The fixed runtime is `/bin/bash`; `entry` selects a script, not a host executable
or extra command-line grant. Dependencies are the existing reviewed macOS runtime
closure, not a hermetic bundled toolchain. Network remains denied. Silk sees:

- `AN_SPOOL_SILK_DIR`: read-only selected-code snapshot directory.
- `AN_SPOOL_KIT_DIR`: the same read-only directory, with the selected kit revisions.
- `AN_SPOOL_INPUT`: the one selected, fixed-version input file.
- Current directory: the writable work area for intermediates and output candidates.

Kit edits affect the loaded instance's working copy only. Start fixes those bytes
into a new execution snapshot. Running edits are refused by the instance operation
lock; there is no live self-edit protocol. A fresh load restores package baselines.
Generated work files remain scratch and cannot mutate canonical silk or the
selected code snapshot. This does not prove a compromised interpreter will obey
domain sequencing: arbitrary hostile native process trees remain unsupported.

### Lifecycle and artifact ownership

One active operation holds a per-instance lock. Independent instances use distinct
locks and work areas. SIGINT/SIGTERM to the running host CLI request whole-attempt
cancellation through the existing supervisor. No numeric-PID-only control API,
background service or durable cancellation mailbox is added.

Result envelopes bind instance, release, run, profile digest and the exact native
receipt. Custody rechecks those fields and the existing termination evidence.
Another instance's receipt cannot claim this instance's output. Products remain in
the existing immutable artifact catalog beneath the run; unloading does not delete
them. Downstream fixed checkout still uses `artifact-checkout CATALOG REVISION PATH`;
there is no live shared writable product directory.

Unload refuses active or unconfirmed attempts. It releases admission for new work,
not disk contents or formal product retention. Explicit per-run reclaim remains
separate and works before or after unload. It abandons work/resume rights only
following existing settlement/acceptance gates. The new `archived_record` read
checks collection intent, receipt digest and final marker before recognizing a
previously collected run; ordinary workspace admission remains fenced after GC.
Inputs and diagnostic evidence retain their existing policies. No automatic GC,
disk quota enforcement or indefinite-history promise is introduced.

A killed host, missing result, stale lock or partial publication remains visibly
uncertain and fenced. There is no automatic lock removal, restart or receipt repair.
The store is trusted against same-user tampering, as in existing host profiles.

### Qualified acceptance

`tests/native_spool.rs` exercises real native isolation, two silk steps, an edited
kit, prohibited silk/external writes, two concurrent instances, cancellation,
active-operation refusal, cross-instance receipt substitution, immutable product
checkout after collection/unload, duplicate-run refusal and fresh-load baseline
restoration. Adjacent unit tests cover nonexecuting/exclusive loading, changed
payloads, unlisted material and package attempts to request extra authority.

Loom separately runs a real CLI round trip against a pinned host revision. It
exports the actual example archive, edits the kit, executes, accepts, reclaims,
unloads and loads a fresh instance. Neither repository links the other's runtime.

### Current extensions and remaining scope

The [status page](status.md) owns the current P0/P1 plan. Explicit
checkpoint continuation, round-boundary pause/suspension, private snapshot
collection/headroom, event dispatch and receiver succession are implemented
bounded contracts, not future stages. The host proposal slot below remains a file
exchange; optional reviewed harness development belongs to Loom/Weaver, not a new
host model loop. Process-memory pause/restore, automatic state/release migration,
formal payload GC and stronger hostile-process containment remain unqualified.
Do not infer those guarantees from admission-control or backend capability flags.

Managed runs carry a `spool-owner.json` marker. Public standalone workspace run, acceptance, and reclamation commands reject these jobs; callers must use the owning spool commands so the instance operation lock and ownership checks remain in force. This is an API lifecycle fence, not protection against a host user modifying the control store.


### Optional kit proposal slot

The host now exposes a file exchange slot, not a model loop or plugin launcher:

```text
spool-kit-proposal-request INSTANCE KIT_BASENAME
spool-kit-proposal-apply INSTANCE PROPOSAL_JSON [--approve HASH]
spool-kit-proposal-revoke INSTANCE REQUEST_UUID
```

A request returns `request` and `request_sha256`. The selected request carries the
instance, release, kit path, original content hash and source, plus an epoch derived
from run and edit identities. Only declared kit basenames are accepted. An external
CLI adapter or authorized shuttle can read that exported data and return:

```json
{
  "schema": "an-weaver-proposal-v1",
  "request": "REQUEST_UUID",
  "request_sha256": "HASH_FROM_REQUEST",
  "kit": "kit/transform.sh",
  "content": "printf 'adapted:'; cat \"$1\"\n"
}
```

The operator imports the response. The host checks its scope, generation and bytes
under the instance lock, then previews a separate approval. A proposal is not a
capability token. Do not expose the unrestricted host CLI or its private control
store to an agent. A transport adapter is responsible for transferring only the
selected data; shuttle transport cannot grant authority.

Any run preparation or recorded kit edit invalidates previous requests, even if
an edit restores identical bytes. Revocation closes result admission, but explicitly
does not claim the external agent terminated. Unload/reload changes instance
identity. Requests are one-use, bounded to 128 directory entries; application closes
the request with a commit intent before editing. An interrupted import fails closed
and requires inspection rather than replay. No automatic cleanup is introduced.

`weaver_proposal_slot` is true in capabilities; `weaver` remains false because no
harness is launched or supervised. The real cross-repository test uses a deterministic
file proposal, not a model. Host tests also cover revoked/stale/foreign requests,
protected silk, changed proposal approval, duplicate application and fixed-version
artifact checkout into a second spool. This adds no new domain or model dependency.

### Readiness timing and CI

CI shows test compilation and execution as separate steps, without excluding any
workspace tests. The 64 MiB clone-handoff regression runs five mutation/cancellation cases. Its
readiness clock includes admission hashing and snapshot preparation; it is distinct
from the workload's 30-second execution deadline. The 120-second readiness watchdog
is a bounded test budget, not a product latency guarantee. Failure cancels and joins
the worker and reports its result. When `AN_AGENT_ACCEPTANCE_DIR` is set, per-case
ready/settled evidence is retained under `clone-handoff/` by the existing CI artifact
step. File size, fixed-version assertions, cancellation checks, and production
deadlines are unchanged. Preparation performance remains measurable follow-up work.

The canonical module is `native_spool::kit_proposal`. The old `weaver` module,
`spool-weaver-*` commands, `weaver_proposal_slot` capability, on-disk `weaver/`
tickets and v1 schemas remain compatible. `kit_proposal_slot` advertises the
canonical name. This slot receives edits; it does not prepare harness tasks,
review generated code or launch an agent. Those future roles use the Weaver name.

### Explicit native v2 requirements

The host also accepts `an-native-spool-v2`, with the v1 fields plus a required
`requirements` object. V1 with requirements and v2 without requirements refuse.
The initial admitted values are runtime `macos-native-process-v1`, OS `macos`,
architecture `portable-source`, interaction `headless`, GPU `none`, one portable
text input-slot name, no additional dependencies, and exactly the capability
requests `read-input` and `write-declared-outputs`. Editable paths must be unique
selected kit paths; unlike legacy v1, a kit payload is not implicitly editable.

Admission happens before instance creation and is checked again when the release
is read for execution. Unsupported target/GPU/application requirements, unknown
capabilities and unresolved dependencies refuse; they never select a weaker
backend. Show reports requested requirements, while start approval binds the
actual fixed workspace policy. Capability labels do not grant authority.
V2 is currently macOS-only. Linux packaging and future backend contracts must
not be confused with qualified execution. GPU CLI execution and shuttle-mediated
application interaction are separate capabilities; neither is implemented here.

### Discovering accepted artifact versions

`spool-artifacts INSTANCE RUN` is a read-only host query returning instance,
release, run, artifact catalog and fixed output revision bindings. It requires
confirmed settlement and matching native/workspace acceptance records, validates
the accepted inventory and each retained revision's bytes and provenance, and
never promotes scratch files. It remains available after scratch reclamation
and instance unload. Use existing artifact-checkout to materialize a selected
version for a downstream input. No mutable latest pointer is introduced.

Loom continues to declare outputs in the release and owns no runtime catalog.
The command works for both native v1 and v2. Missing acceptance, uncertain
termination, foreign records and changed content refuse. An existing filename or
a successful exit alone does not establish a formal artifact.

### Run monitoring

`spool-status INSTANCE RUN` is an observation-only query. It returns a stable
phase (`prepared`, `unconfirmed`, `running-or-unconfirmed`, `settled`, `accepted`,
`reclaimed`), execution outcome, termination confirmation and separate artifact
status. Missing, inconsistent or racing settlement remains uncertain. Collection
requires verified workspace evidence; an outer marker alone cannot establish it.
Accepted output bindings use the same evidence checks as spool-artifacts and survive unload.
Partial acceptance is reported as unconfirmed rather than a clean pending state.

The query never acquires, writes or clears the instance execution lock, including
when verifying accepted outputs. A busy or stale lock alone does not make valid
artifact evidence unconfirmed. The query never infers liveness from a PID or a
lock file. A missing receipt cannot
distinguish running work from interrupted bookkeeping. All mutation/custody
commands independently revalidate and retain their existing locks and approvals.
The response is a bounded observation, not an atomic scheduling decision.
Headless CPU is qualified first. Common scheduling/monitoring identities may be
reused for future GPU/application backends, whose actual supported lifecycle and
external settlement must be separately qualified. No pause or suspend support is
implied by these state names.

### Reconciling an interrupted acceptance handoff

If workspace acceptance completed but recording the instance-bound accepted.json
failed, status reports unconfirmed custody and ordinary acceptance is not retried.
`spool-reconcile-accept INSTANCE RUN [--approve HASH]` previews a repair, validates
confirmed settlement, complete workspace acceptance, exact output inventory and
all retained versions/provenance, then writes only the missing instance record.
A matching existing record is an explicitly verified no-op. Conflicting, partial,
foreign or changed evidence refuses; existing records are never overwritten.

The approval binds the complete recovered acceptance binding. No workload is
restarted and no artifact is republished. Query and downstream checkout work again
using the original revisions. This does not recover an incomplete workspace
acceptance or missing execution result, clear uncertain termination, or infer an
external worker stopped. Those states remain fenced for later reconciliation.
The filesystem failure acceptance test exercises actual publication followed by
a failed facade write; it is not a claim of arbitrary power-loss durability.


### Restoring an editable kit baseline

`spool-kit-reset` restores one declared editable working copy from the verified
release baseline without changing instance identity, retained products or other
kits. Preview and approval use the existing edit plan and its before/after
digests. It shares the same instance lock, editable-path checks and edit receipt
as update-kit; active execution and unloaded instances refuse. Protected silk or
noneditable v2 kit paths remain forbidden. The immutable release is not modified.

When reset changes kit bytes, earlier start approvals for different bytes are
invalid. Every approved reset records an edit generation, so old proposals stay
stale even after an edit-away/edit-back cycle. It grants no execution. Inspect and separately approve the
next start. Existing load/unload/fresh-directory baseline restoration still works.


### Reconciling a missing execution result

`spool-reconcile-run INSTANCE RUN [--approve DIGEST]` explicitly repairs a missing
outer result.json when one intact workspace attempt already confirms process
termination. It verifies instance/release/run ownership, the original start plan,
profile identity, workspace intent, receipt, execution mapping and snapshot bytes.
It writes only the derived result record after approval. Matching existing records
are a verified no-op; conflicting records are never overwritten.

Attempt discovery requires exactly one canonical UUID directory. A regular
`.DS_Store` file is left untouched and listed in the repair approval's
`ignored_attempt_entries`; it is not an attempt. Other unexpected entries, links,
noncanonical UUID names and a directory named `.DS_Store` refuse with a diagnostic
naming the entry. Multiple real attempts remain ambiguous and refuse. No blanket
metadata or editor-file exclusion is applied.

This does not launch, retry, accept output, clear locks or infer termination from
a PID. Missing/uncertain receipts, unstarted attempts, multiple attempts, changed
snapshots, foreign bindings and reclaimed workspaces refuse. Preserve those
states for investigation. A repaired successful run still requires explicit
spool-accept; a failed/cancelled run remains ineligible for acceptance. Normal
reclamation/unload become available only under their existing evidence checks.

Real filesystem-failure tests complete successful and failed workloads while a
directory blocks the final result write. They then verify explicit repair, no
additional attempt or publication, negative evidence cases and normal custody.
This is not arbitrary crash recovery or a power-loss durability claim.

## Native control at round boundaries

The first control slice uses a completed spool attempt as its safe point. It
pauses admission between attempts; it does not stop a running process. The spool
author decides how domain work reaches a committed checkpoint. The host verifies
termination, accepted outputs and fixed identities, without interpreting state.
Loom packages the same v1/v2 release; no new release schema or backend is needed.

```text
an-agent-host spool-pause INSTANCE [--approve HASH]
an-agent-host spool-suspend INSTANCE STATE_REVISION [--approve HASH]
an-agent-host spool-resume INSTANCE [--approve HASH]
```

Every transition has a preview and separate approval. A live operation, stale
lock, unknown termination or successful run lacking formal acceptance refuses.
Prepared plans do not count as execution; an interrupted start with no completed
workspace evidence does. Nothing clears a stale lock or retries an execution.

Pause fences new starts, kit/proposal application and state mutations. Inspection,
proposal revocation and existing guarded artifact custody remain available.
Suspension additionally pins the current committed text checkpoint and effective
kit hashes. It retains state, release and evidence; scratch can be explicitly
reclaimed through the existing command after settlement and acceptance.

Resume verifies the saved release, kits, checkpoint and completed-run evidence
before reopening admission. It launches nothing and carries no old execution
approval forward. Continuation still requires an explicit `spool-start-state`
selection and fresh approval. A domain may instead begin separately authorized
new work. No state, program counter, heap or hidden scratch is injected implicitly.

Control generations prevent old start and transition approvals from becoming
valid again after a pause/resume cycle. A transition publishes immutable evidence
before atomically changing the control head. An interrupted publication keeps the
previous mode; explicit repetition of the same approved transition may finish its
publication. Confirmation of an already published transition is a no-op. An
incompatible staged record refuses for inspection, without overwriting history.
This does not establish sudden-power-loss recovery or hostile host-store safety.

`spool-show` reports the mode and generation separately from a run's outcome.
Existing `spool-status` continues to describe execution and custody, independently
of whether new work is paused. Unload remains terminal, preserves checkpoints and
control evidence, and is allowed from a settled paused/suspended instance. A fresh
load uses the existing explicit fixed-state checkout/seed path and restores kit
baselines; it is not automatic instance or process restoration.

Existing instances without a control head begin at active generation zero. The
first approved transition installs the baseline without changing release identity.
Before publishing any stopped admission mode, it atomically upgrades the private
instance record to `an-native-spool-instance-v2`. Older hosts reject that record
instead of silently ignoring control; hosts supporting this feature continue to
read legacy v1 instances. This is an instance-format floor, not a release-format
change or protection against an operator deliberately rewriting the private store.
Missing or inconsistent control evidence after installation fences admission.
The generic process pause/suspend capability flags remain false. Separate
`pause_between_attempts` and `suspend_checkpoint` flags describe this macOS slice.
In-flight cooperative signaling, transparent process suspension, VM snapshots,
GPU/GUI backends and automatic storage collection remain unqualified.

## Operator-selected native Python and binary products

Status: qualified for reviewed, CPU-only, headless tools; platform coverage is
maintained in the [runtime matrix](platforms.md). This extends the
[native v3 event profile](native-events.md) through existing workspace execution
and binary artifact custody. It adds no core dependency, environment installer,
package resolver, agent harness, domain parser or extra isolation per silk.

### Portable requests and local selection

A native v3 release may declare `binary_outputs`, a unique subset of `outputs`.
Omission preserves text output behavior. Each binary output is accepted as opaque
bytes through the existing artifact-v2 contract: 8 MiB per file, 16 MiB combined
outputs; text retains its UTF-8/64 KiB bound. Format, formulas, ZIP expansion and
domain checks belong to the consumer. A file extension does not select encoding.
An older v3 host rejects the new field instead of silently accepting it as text.
V1/v2 bytes and behavior remain unchanged; they cannot declare binary outputs.

Requirements may select `macos-native-python-v1` only in v3, with the existing
macOS, portable-source, headless, GPU-none and exact capability request set. One
file input remains supported. Its declared slot name becomes the snapshot name,
so a consumer can receive `source.xlsx` from an artifact stored as `content.bin`.
Code basenames and the input name must be distinct. Optional v3 `state_input`
adds a separately selected checkpoint through the [data/state contract](native-state.md#independent-business-data-and-state).
It receives its own snapshot name and AN_SPOOL_STATE path. There is no
extension-based dispatch or implicit state/input merging.

The release carries bounded unique dependency labels, not executable paths,
installation hooks or grants. The operator separately selects a compatible
environment:

```text
an-agent-host spool-load DELIVERY NEW_INSTANCE --runtime RUNTIME_JSON
```

The strict operator JSON has these fields:

```json
{
  "schema": "an-native-python-runtime-v1",
  "executable": "/selected/python/bin/python3",
  "executable_sha256": "EXACT_SHA256",
  "stdlib": "/selected/python",
  "site": "/selected/venv/lib/python3.12/site-packages",
  "site_sha256": "EXACT_INVENTORY_SHA256",
  "dependencies": ["example-1.0.0"]
}
```

Paths must already be canonical absolute selections. `stdlib` and `site` are
separate directories, outside both the release and instance; root-directory,
symlink and nested selections refuse. Dependency labels must match the release
exactly. Shell releases reject an unsolicited runtime; Python releases refuse a
missing binding before creating an instance. Load copies the selected JSON to
read-only `runtime.json`, binds its exact digest in `instance.json`, and executes
no interpreter, project hook or installer. Start still requires its own approval.
Loom never supplies or approves this machine-local binding.

The site inventory is SHA-256 of compact UTF-8 JSON mapping sorted relative POSIX
file paths to SHA-256 content digests. All files, including bytecode, are included.
Symlinks, hardlinks, nonregular files, more than 8,192 entries, depth over 16,
files over 20 MiB or total bytes over 256 MiB refuse. The host does not import
packages while checking them. Labels are operator assertions; neither their
equality nor content hashes authenticate the package publisher or certify safety.

### Execution and acceptance

The existing workspace supervisor runs the selected interpreter with `-S -B`,
attempt-private code/data snapshots and no ambient environment. `PYTHONPATH`
names only the selected snapshot and site directory. User-site initialization,
`.pth` startup processing and bytecode writes are disabled. `HOME` and `TMPDIR`
point into work scratch; selected runtime roots are read-only and networking
remains denied by the reviewed native policy. The operator binding digest is
included in the approved execution environment.

The interpreter digest and dependency inventory are rechecked at load, while
planning an approved start, and before native artifact acceptance. Changed
dependencies refuse instead of silently changing the environment, launching a
replacement attempt or accepting its outputs. Restore/reselect a valid
environment explicitly; an unknown execution is never automatically retried.
The standard library and system dynamic libraries remain operator-owned platform
dependencies, not immutable packaged snapshots. Pre/post checks are not a proof
against a hostile concurrent same-user change-and-restore attack. This is the
existing reviewed-tools boundary, not arbitrary hostile-code containment.

Native artifact discovery and collection depend on retained execution/custody
evidence, not the continuing installation of Python. Accepted binary artifacts
and consumption confirmations remain readable after work/snapshot collection,
instance unload or loss of the runtime. Unaccepted successful work remains
retained until its acceptance boundary can be resolved.

Binary events use the same exact full revision binding as text events. A delivery
selects `content.bin` for verified byte revisions and carries the revision into
workspace input provenance. It never follows a mutable latest pointer. Explicit
acceptance and consumption completion remain separate from successful execution.

### Qualification

```sh
python3 -B crates/an-agent-host/tests/cli/native_binary.py \
  --host /selected/an-agent-host --evidence /private/tmp/native-binary-NEW
```

The real macOS suite checks missing/mismatched runtime refusal, changed dependency
refusal before start and acceptance, read-only runtime enforcement, opaque byte
handoff after a newer publication, confirmed cancellation, active collection and
cancelled-output acceptance refusal, and retained products/duplicate confirmation
without Python.
It records commands and evidence privately and never rebuilds its selected binary.
Loom separately packages the existing log-report code and qualifies real editable
XLSX/PPTX products. No GUI, model, GPU or new backend is claimed.

For client-independent execution of a native file-input attempt, use
`spool-supervisor start`. Its separate worker retains the original deadline and
process ownership after abrupt client exit. The [supervision contract](session-and-retention.md#independent-attempt-supervisor)
defines durable launch/cancellation, uncertain-result retention and the narrower
migration scope. Ordinary `spool-start` retains its foreground behavior.
