# Testing and cross-repository verification

This contract guides test placement and selective migration. It does not change
runtime behavior, move existing tests or replace current CI commands. Repository
verification remains defined in [AGENTS.md](../AGENTS.md) and the workflows.

## Ownership and language

| Scope | Owner and preferred implementation |
| --- | --- |
| Internal invariants, parsing, identity checks, private fault windows | Rust tests beside the owning module or in existing integration targets |
| Real CLI composition, process cancellation, filesystem custody and recovery | Host acceptance; prefer Python for new black-box suites using built binaries |
| Authoring, declarations, frozen candidates, packaging and evidence reception | Loom tests; Rust for internal contracts, Python for external CLI composition |
| Loom release loaded and executed by the host | Narrow paired acceptance, using explicit Loom and host revisions |
| Browser or GUI integration | Adapter-owned tests; TS when it fits the existing browser tooling |
| Domain correctness | Spool/example-owned tests, not host or Loom business branches |

Language is a testing choice, not a runtime dependency requirement. Keep mature
Rust tests when moving them offers no measured benefit. Do not expose a new
production API merely to let an external test trigger an internal fault window.
Host stress and failure-window matrices belong in an-agent; Loom should verify
its handoff contract without duplicating that full matrix. Existing coverage
must remain until an equivalent replacement is demonstrated.

## Placement and execution

Keep private Rust tests adjacent to their modules under `cfg(test)` and public
Rust suites in the owning crate's `tests/` tree. Future Python CLI suites should
live under that crate's `tests/cli/`, with a local README listing prerequisites,
binary arguments, commands and supported platforms. This is a placement rule,
not a claim that these suites have already been migrated.

Build product binaries in an explicit preparation step, once per selected
revision, target and build configuration. Pass their paths to external tests;
do not use `cargo run` or rebuild the workspace inside each scenario. A test
whose purpose is to verify a build remains an explicit exception. Cargo's
`test --no-run` followed by `test` normally reuses test binaries; these are not
inherently two full compilations.

For paired acceptance, record both source revisions, binary digests, target OS
and architecture, and relevant profile/contract identities. Historical suites
may pin different host revisions; do not silently reuse one binary across them.
A cache is only an optimization, never evidence of matching inputs.

Each external runner must bound waits, preserve failure output, and clean up
only processes and files it owns. A client timeout does not establish execution
termination. Retain evidence and refuse destructive cleanup when termination is
unknown; do not retry the operation automatically. Exercise real enforcement
before claiming it, and label simulated receipts or faults explicitly.

## Branch-owned run evidence

Every feature PR carries a reviewed acceptance summary tied to the tested source
revision or patch digest and a frozen executable hash. Include the environment,
scenario outcomes, denied operations, cancellation/reconnect observations,
artifact checks and unresolved limitations. Identify deterministic fixtures and
real-provider runs explicitly. Record accepted product hashes when relevant.

Keep per-run logs, private conversations, receipts and generated files under the
feature worktree's ignored `lab/` area. The branch owner retains them independently
of integration. The PR holds the durable summary and any reviewed CI-artifact
links, including their retention period. Per-run evidence is excluded from both
`dev-advance` and `main`; reusable regression suites and contract documentation
travel through the ordinary PR. Preserve summaries when pruning merged heads.

## Migration acceptance

For each selected suite, document old and new entrypoints and map its assertions
and fault cases. Preserve cancellation, stale-result rejection, version pinning,
termination confirmation and artifact-retention checks where applicable. Keep
required CI coverage and supported-platform checks; unavailable prerequisites
are not a passing acceptance result. Coordinate any entrypoint replacement with
CI and downstream callers in the same reviewed change.

Run both versions for the migration comparison, then remove duplicate coverage
only when equivalence is established. Do not weaken gates, raise deadlines or
hide races to obtain a faster result. Standard Cargo checks remain required;
run affected external suites separately and report skipped real acceptance.

Measure dependency/product compilation, test compilation/linking and execution
separately where tooling permits. Record runner/platform, revisions, toolchain,
cache state and repeated measurements; distinguish cold from warm runs. Python
can remove Rust test compilation but cannot remove product compilation. Do not
claim a speedup from a single combined CI duration or a changed runner.


## CI dependency caching

Host CI installs the toolchain selected by the repository's `rust-toolchain.toml`
through rustup, including its declared components. Do not install a separate
moving `stable` version before selecting the pinned version. The ephemeral hosted
runner makes that version its default (including fixtures invoking rustc outside
the repository) and removes other installed toolchains for this job only. The
cache action fingerprints all installed compilers; otherwise unused preinstalled
stable-version differences can invalidate a cache for identical source. This
workflow does not change developer machines.

The pinned [rust-cache action](https://github.com/Swatinem/rust-cache) covers both
`. -> target` and `tools/archcheck -> target` in one cache operation. These remain
independent Cargo workspaces and build directories. The action keys dependencies
by compiler and Cargo configuration/lockfiles; our additional key separates
runner OS and architecture. The `host-deps-v1` prefix provides an explicit cache
namespace. Default dependency-only caching keeps project compilation and all
checks active; cached binaries are never treated as passed acceptance evidence.
The action disables Cargo incremental compilation for this CI cache strategy.

Registry/git dependencies and dependency build outputs are cached, not lifecycle
receipts, workspace products, credentials or test results. Installed Cargo
binaries are excluded. Cache restoration cannot skip formatting, compilation,
workspace tests, Clippy, architecture-tool tests/lint or the architecture gate.
Existing job names, OS matrix, timeout and evidence upload remain unchanged.
GitHub's normal cache scope applies; a PR cache is not a new runtime trust grant.

To measure a cache change, first run the PR in a fresh namespace and confirm a
miss in its cache logs. After that run finishes successfully and saves the cache,
rerun the same commit and record the exact-hit observation. Compare per-OS job
and step durations including cache restore/save, not only compilation. A completed
CI rerun is a fresh test run, not a retry of an unknown product operation. Do not
flush unrelated caches or increase timeouts to improve the comparison. Preserve
run/attempt links in the PR and distinguish runner variance from established
improvements. Cache misses remain fully supported and must pass the same gates.

For cross-repository acceptance, cache each qualification job's selected host
checkout under that job's identity and retain its pinned revision. Different
historical host releases are distinct build inputs even when they expose similar
CLI commands. Do not share one host executable across those jobs to save time.
A combined dependency cache may cover multiple target directories, but never
cache accepted products or replace receipt verification with a cache hit.

## Historical qualification and retained decisions

Completed multi-silk plans and handoff pressure reports are retained in
[the pre-consolidation tree](https://github.com/lazyalexander/an-agent/tree/b44b3c538eb9428975d38e7cbc3669ddf9b0f827/crates/an-agent-host).
They are historical measurements, not current support promises or new work items.
The durable decisions are unchanged: hash immutable inputs before the catalog
commit lock, keep copying/publication inside it, treat contention as explicit
refusal, and require whole-group termination rather than leader exit. A retained
input clone is an independent version, not a live mount or permission to delete
its source. No measurement is a universal latency or throughput SLA.

Current reproducible CLI scenarios and their arguments remain in the
[acceptance entry-point README](../crates/an-agent-host/tests/cli/README.md).
Optional Linux recipes remain in [qualification](qualification.md), separate from
[qualified platform support](platforms.md). Raw evidence stays private.

## Single-main-agent benchmark v1

The executable scenario inventory uses the original daemon acceptance entrypoints.
Each run freezes inputs, source inventories, executable hashes and environment.
Run-specific evidence follows the branch-owned rule above. The inventory includes
the currently blocked Krita document-bound save case.
[Scenario inventory](../crates/an-agent-host/tests/benchmarks/single_agent_v1.json)

### Baseline and scope

Use one main agent session for an entire task, including transitions between
spools. Load spool descriptions, contracts and selected knowledge progressively.
Keep the external harness responsible for the model loop. The host retains
scheduling, task grants, lifecycle, activity records and artifact acceptance.
Record the main session identity across steering and reconnection. A hidden
model-driven child invalidates the single-main baseline classification.

The main agent may invoke deterministic programs concurrently, wait for background
tools, and resume after their results. Track those programs separately from model
agents. Freeze the tools available to the model, including any harness delegation
tools. Every model call contributing to the task belongs in its usage accounting.
Multiple independent user sessions are a separate concurrency experiment.

### Six metric groups

| Metric | Measurement |
| --- | --- |
| Accepted delivery | Positive tasks passing independent domain checks, host acceptance and required release checks divided by all started positive trials. Also report tasks succeeding in all three repetitions. |
| Cost per accepted delivery | Sum of usage across every positive trial in a family, including failed attempts and repairs, divided by accepted deliveries. Also report per-trial usage and control-case usage separately. |
| End-to-end latency | Time from host admission to accepted delivery or the expected control outcome. Report queue, discovery, model, program, human-wait and acceptance spans, with overlap identified. |
| Human assistance | Unplanned corrections and manual recovery count and active time per task. Predeclared approvals and scripted clarification have separate counts. |
| Control integrity | Count unauthorized effects, wrong-revision acceptance, replay of uncertain effects and premature reclamation. Report expected refusal, cancellation and recovery success separately. |
| Discovery and context efficiency | First eligible selection and final valid selection rates, time to first correct action, discovery calls, unique and repeated bytes disclosed, and observed token usage. |

A positive trial expects delivery or clarification followed by delivery. Expected
refusal, cancellation, retained uncertainty and unsupported capability each have
their own outcome denominator. Include timeouts and unsuccessful repairs in the
positive denominator. With zero accepted deliveries, cost per delivery is
undefined and the total incurred cost remains visible. Report started, blocked,
invalid and completed counts so missing coverage stays visible.

Grade the resulting files and application state with independent checks. Use
activity traces to diagnose causes. This outcome-based grading follows existing
agent evaluation practice. [Anthropic evaluation guide](https://www.anthropic.com/research/demystifying-evals-for-ai-agents)

Report both trial success and the fraction of positive cases passing every
repetition. Repeated success exposes consistency problems that a lucky successful
attempt can hide. The repeated-success measure is inspired by tau-bench's
reliability evaluation. [Tau-bench paper](https://arxiv.org/abs/2406.12045)

For token accounting, retain provider-native counters and their semantics. When
cached input is a subset of input, count it once. Apply the same rule to reasoning
tokens included in output. Deduplicate cumulative usage snapshots by execution,
turn and attempt identity. Charge failures, steering, context compression and
repair calls when reported. Mark missing counters `null` and report coverage.
Convert to money only with a recorded pricing version and billing model.

Measure model-call wall time when available. A provider's internal reasoning time
may remain unknown. Do not infer it by subtracting program time from total time.
Keep machine-active and user-visible latency, planned human wait, app startup and
tool execution distinct. Record observed cache state. Label uncontrollable
provider caching unknown. Latency summaries include successful and unsuccessful
trials separately, plus the deadline-censored count.

Discovery checks accept any capability satisfying the frozen contract. They must
allow equivalent spools and legitimate clarification. Track unnecessary knowledge
loads and wrong-release instructions independently of final correctness. Byte
counts measure disclosure volume. Token estimates carry the tokenizer/version
and remain distinct from provider-billed tokens.

### Case matrix and initial run size

Start with eight families and three cases per family. One repetition of the first
case in each family is an eight-trial instrumentation smoke. The initial full
baseline uses three repetitions of all 24 cases, totaling 72 trials. These counts
are initial planning defaults. Report per-case and per-family results before any
aggregate. Three repeats support diagnosis and descriptive consistency checks.
Tail-latency or architecture-superiority claims need a separately sized comparison
with uncertainty estimates.

| Family | Three cases | Principal observation |
| --- | --- | --- |
| Document delivery | Exact PDF table, revised/wrapped table, scripted clarification | Markdown and editable workbook values, formulas, revisions and custody |
| Spool selection | Catalog sizes 4, 16 and 64 with overlapping descriptions | Correct selection and growing discovery cost |
| Context pressure | Same task with 0, 16 and 64 KiB of unrelated knowledge | Selective loading, constraint retention and obsolete-release rejection |
| Background control | Supplemental instruction, daemon reconnect, cancellation | Original task identity, current instruction and resource release |
| Work shape | Independent programs, independent reasoning dossiers, dependent steps | Program parallelism versus reasoning critical path |
| Boundary faults | Unauthorized input, stale release, lost effect receipt | Refusal, provenance and uncertainty retention |
| Mechanical delivery | Steam chain, inline-four reuse, Krita target mismatch | Real domain quality, editable products and document-bound effects |
| New capabilities | Unseen spool identifier, schema variation, missing capability | Contract-based transfer and explicit unsupported outcome |

Document fixtures use synthetic inputs with exact expected rows and formulas.
The initial exact table totals EUR 38.50, and its changed-quantity revision totals
EUR 39.00. Freeze ambiguous inputs and scripted replies together. Mechanical
fixtures reuse an-spool domain checks and fix resolution, frame count, application
versions, layer requirements and visual tolerances before running. Record visual
review separately when a criterion needs human judgment.

The context-pressure sweep measures growing irrelevant material. Add a separate
required-context sweep if evaluating actual context-window saturation. Keep the
necessary facts and difficulty identical in the initial disclosure comparison.

### Trial preparation and evidence

Before a scored run, freeze source revisions, executables, spool releases, inputs,
oracles, catalog order, model and harness settings, grants, deadlines and token
budgets. A case recipe becomes executable only after its input and oracle digests
are recorded. Record fault triggers as observable events. Use existing test seams
or controller-owned fixtures for fault injection. Keep oracle answers outside the
agent's authorized workspace and instructions. The task must still disclose every
requirement needed to produce a valid result.

Calibrate deadlines and budgets with unscored smoke runs, then freeze them for all
compared variants. Preserve fresh task state across trials and counterbalance
execution order. Keep machine concurrency, app startup state and cache observations
in the record. A timed-out operation retains its original identity until settled.
An independent subsequent trial needs settled resources or an isolated allocation.

Reserve unseen fixture variations before tuning descriptions or prompts. Keep
development and held-out results separately labeled. If a platform cannot execute
a required case, record it as blocked and report reduced coverage. A suite with
blocked cases has partial qualification. Controller or grader defects invalidate
the affected comparison and require an explicit correction record.

Each trial record includes case/repetition identity, all session and operation
identities, observed effects, artifact hashes, oracle decisions, instructions and
grant revisions, disclosure events, stage timestamps, usage counters, human
interventions, resource-release evidence and unresolved state. Retain the provider
events actually exposed and leave private model reasoning unobserved.
Publish reviewed summaries through the existing branch evidence process.

### Reuse and instrumentation gaps

Document acceptance uses one original Codex thread across two turns, with a third
turn for the scripted clarification. The main agent reads release-bound knowledge
and the host-accepted Markdown. Application libraries handle PDF extraction and
workbook authoring. Independent Markdown and OOXML checks verify rows and formulas,
and the original host owns publication, acceptance and revocation.
[Document acceptance](../crates/an-agent-host/tests/cli/document_activity.py)

The HarnessAdapter suite uses a deterministic protocol fixture with two owners.
Reuse its identity, steering, cursor and cancellation assertions. Real-provider
benchmark runs need the same observables on the one-main task.
[HarnessAdapter acceptance](../crates/an-agent-host/tests/cli/native_harness_adapter.py)

Reuse existing grant-refusal and revocation checks, and correlate them with the
real task's effects. Pin an-spool's mechanical acceptance revision for the domain
chain. Host test code owns the orchestration measurements and generic invariants.
Spool tests own domain correctness.
[Permission acceptance](../crates/an-agent-host/tests/cli/native_task_permissions.py)

`activity.harness.metrics` reads original provider events and authorized tool receipts.
Cumulative session usage is deduplicated and checked for regressions. Authorized
UTF-8 reads record content digests, byte counts and host dispatch duration. Knowledge
classification uses frozen content hashes. A host response records disclosure,
with model consumption left unknown. Provider-internal reasoning time remains unknown.

Run `single_agent_benchmark.py` with `--phase smoke` and then `--phase full`, supplying
`--host`, `--codex`, `--evidence`, `--archive`, `--spool`, `--shuttle`,
`--mechanical-fixtures` and `--mechanical-site`. Use short evidence paths on macOS
for local sockets and an ignored branch-local archive. The controller needs pypdf,
XlsxWriter and the selected mechanical runtime dependencies. The host enforces wall
deadlines. The token threshold is checked after a run. Frozen reports identify this
limitation and preserve incurred usage on failures.

The lost-receipt case currently withholds a steering acknowledgement and verifies
unknown command reconciliation without replay. Application save effects need their
own qualification. The selected Krita bridge addresses documents by canonical path,
so changing window focus alone does not establish a target mismatch. Its host-grant-
bound save operation is missing. Record that case as blocked in every repetition.
Mechanical checks cover geometry, motion, video dimensions and frames, editable
Blender delivery and the ORA first-frame reference. Visual style and Krita GUI
save qualification remain separately reported.

### Evidence for changing architecture

First compare progressive disclosure against eager loading using the same main
agent, tools, model, grants and fixtures. Also test whether concise descriptions,
correct knowledge indexing or deterministic program parallelism resolve observed
problems. Classify tool faults, missing domain knowledge and resource contention
before attributing failure to agent topology.

A repeated weakness on distinct frozen inputs is the initial trigger for a bounded
alternative-architecture experiment. Record the hypothesis and the case family
it addresses. Choose the candidate from the diagnosed cause, such as independently
solvable reasoning work or incompatible context requirements. Test the candidate
on paired development and held-out inputs with the same quality checks.

Predeclare acceptable quality loss, required latency/cost improvement and the
comparison budget after observing the baseline. Compare both equal-budget quality
and cost/time at matched quality. Count coordinator and worker usage together.
Changes to models, tools or grants require separately labeled comparisons.

Adoption needs an observed benefit beyond trial variation, complete relevant
telemetry, zero observed control violations and an explicit accounting of added
maintenance, dependencies and recovery paths. A latency win during rendering
alone provides no evidence that more model agents would help. Keep the single-main
path as the default until the evidence supports a specific change.

### Cocoon candidate custody

`native_cocoon_session.py --host HOST --evidence PRIVATE_DIR` uses protocol
fixtures plus the real native spool supervisor and acceptance path. It verifies
binary chunk bounds, exact task capabilities, source/digest mismatch rejection,
formal-output denial before acceptance, and a continuation on the original owner
that reconciles a lost chunk receipt before reading the accepted artifact.
`--cancel-validation` instead cancels the producer while the native validator is
running, verifies termination with the parent grant still active, and rejects late
acceptance and candidate reuse. `--cocoon-profile PRIVATE_JSON --cocoon-attempt UUID`
selects an already authorized live VM for the same test. The operator owns VM
provisioning and final stop/reconciliation. Never commit profiles or evidence.
The main agent remains a deterministic protocol fixture even in live VM mode;
these checks do not qualify real model behavior or PDF/Excel domain correctness.

`native_cocoon_documents.py` accepts explicit `--host`, `--profile`, `--domain`
(an-spool documents package), `--python-runtime`, `--dependency-site` and a new
short `--evidence` directory. The VM must already contain the operator-reviewed
Python runtime and pinned document wheels. Omit `--codex` for the deterministic
primary protocol fixture; supply the qualified Codex binary for real model
acceptance. `--clarification` adds a third turn with a separately authorized user
answer input. `--lost-receipt` removes a completed host execute receipt and requires
recovery by the original request ID. `--cancel-before-continuation` verifies that
cancelling the original primary owner stops its VM and denies late acceptance.
The default chain invokes three fixed guest programs, accepts Markdown and XLSX
through original native spools, and independently checks source rows, formulas,
cached EUR totals and Markdown provenance. Raw reports and runtime images remain
private. Native custody does not establish GUI save/visual acceptance or final
provider billing/remote-provider termination.
`--restart-controller` restarts only the daemon between accepted Markdown and
continuation, checking the same primary execution/owner and retained job records.
`--cancel-during-program` uses a fixed slow program to check control observation,
steering admission and cancellation latency while guest execution is blocked;
it then requires local and VM termination with no proposed output. This fixture
measures control responsiveness, not real-model output or render performance.
