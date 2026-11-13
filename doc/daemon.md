# macOS user service

The service is a local product entry point over native spools, not a second
execution host. It queues references to original `native_spool::supervisor`
reservations; those finite owners still perform admission, native workspace
execution, termination confirmation and artifact custody. Domain sequencing and
instructions remain in spool releases. No VM or container is required.

## Start and inspect

Select the built host by its real absolute path. Create a new service directory
under an existing private parent, using canonical paths (macOS `/var` and `/tmp`
are symlinks). The service root must belong to the logged-in user with mode 0700.

```sh
an-agent-host daemon init /absolute/private/service 2
an-agent-host daemon serve /absolute/private/service
an-agent-host daemon show /absolute/private/service
an-agent-host daemon request /absolute/private/service /absolute/request.json
an-agent-host daemon launchd /absolute/private/service /absolute/agent.plist
```

`serve` stays in the foreground for launchd. SIGTERM closes its admission socket;
original finite owners retain their deadlines and control records. The generated
LaunchAgent plist is not installed automatically. Install one selected service per
user with the operator's existing launchd workflow; never run this as root. An
OS-owned advisory lock prevents duplicate services and releases after process loss.
No privileged helper, root daemon or provider credential store is installed.

`endpoint.json` contains the loopback console URL. Its fragment token authorizes
read-only snapshot requests. The socket accepts only same-user callers, bounded
JSON lines and private store access. The HTTP surface provides no controls, CORS
access or arbitrary file serving. Host checks and authorization gate data reads.
This is a trusted same-user service, not isolation from malicious peer processes.

## Operations and approval

Each request has `op` and the selected fields below. Controls returning
`approval_sha256` are previews; resubmit the same request with `approve` set to that
exact digest. Changing inputs, runtime selection or scope invalidates approval.
Registration alone grants neither execution nor model disclosure.

| Operation | Fields | Effect |
| --- | --- | --- |
| `register` | `instance` | Register one existing, release-bound native spool instance |
| `submit` | `instance`, `run`, `seconds`, either `input` or `artifact: {catalog, revision}` | Preview/reserve an existing supervisor start and queue its reference |
| `cancel` | `id` | Record exact task cancellation and forward it to the original owner |
| `pause`, `resume`, `unload` | `instance` | Delegate the original lifecycle operation and approval |
| `suspend` | `instance`, `revision` | Select the exact committed checkpoint; no memory suspension |
| `accept`, `reclaim` | `instance`, `run` | Delegate original custody gates; never infer authority from the queue |
| `show` | none | Read lifecycle, task and resource projections |
| `task` | `id` | Inspect one original task without enumerating other spools or artifacts |
| `harness` | `task` | Preview/queue a finite external client with explicit content disclosure |
| `harness-result` | `id` | Read a completed proposal, never an accepted business artifact |

The bounded queue permits 1–8 concurrent tasks, one active queued attempt per
instance, at most 128 registered instances and 256 retained scheduling entries.
Selection is FIFO among eligible entries. Existing direct entry points keep their
own locking and do not become globally scheduled implicitly.

A durable claim is written before launching. Claimed work is never automatically
replayed, including after a service crash in the spawn window. On restart, the
service reads original completion evidence. An unresolved claim holds its budget
slot and is displayed as `running-or-unconfirmed`; missing receipts cannot be
converted into successful cancellation, accepted output or permission to reclaim.
A stop request and confirmed termination remain separate. Queue records contain
scheduling metadata, not a new authoritative execution ledger. Registry archival,
automatic state migration and automatic retries are not implemented.

<a id="native-teaching-applications"></a>

## Native clients and operator runtime selection

Applications can load the three mechanical releases as ordinary native Python
instances and use this same register/submit/accept path. An operator Python runtime
may additionally select bounded `readonly` roots (at most six), snapshot `inputs`
(at most eight), and `graphics: "macos-agx-metal-v1"`. These selections live in the
immutable instance runtime binding, never in portable release authority. Auxiliary
files become numbered `AN_SPOOL_AUX_0` inputs with unique snapshot names; their
bytes participate in the original preview and are rechecked before execution.
A Python release requesting `gpu: "metal"` refuses loading without that explicit
graphics selection. Only this selection allows the existing 900-second bound;
other native runs retain 300 seconds. Backend graphics policy remains unchanged.

For a downstream run, `submit.artifact` selects an already accepted revision;
`input` is omitted for new clients. A `run-artifact` permission binds the catalog reference, exact revision, receiving instance/run and maximum seconds. The host resolves the blob location and retains the existing claim/custody path. Legacy callers may still supply `input` with a path-based `run` grant; if both fields are supplied the host verifies they identify the same artifact. The supervisor
retains the revision and revalidates it at launch through the existing input
reader/claim path. A copied file is not silently treated as the original revision.
Queue loss, cancellation and uncertain completion keep their existing behavior.

An example-only teaching client is in an-spool's `examples/mechanical-animation/product`; it is not an installed product component. Shared client and optional page helpers stay in `examples/support`, outside production packages.
Its browser page previews each finite operation, obtains a narrowed task grant,
and submits through the existing task socket. Credentials live only for that call;
there is no application credential store or execution queue. Stage eligibility and
progress come from original task and custody projections. No automatic retries or
background stage advancement are introduced. The existing console remains
read-only; the application has a separate loopback control token and requires
same-origin POSTs. The final accepted OpenRaster handoff uses Shuttle's existing
Krita bridge and is a first-frame reference, not editable animation or part layers.

`native_daemon_inputs.py` checks bound auxiliary bytes, graphics refusal and an
accepted binary revision handed between original queued supervisors. The teaching
application's real steam/four-cylinder qualification remains a separate product
acceptance; this fixture does not claim Blender or Krita validation.

## Finite Codex harness

The initial adapter reuses the qualified Codex CLI 0.166.0 finite material profile.
`task` contains `label`, canonical `binary`, `binary_sha256`, `prompt`, `schema`,
`scope`, nullable `model` and nullable `mcp: {url, token}`. The selected executable
is hashed at approval and checked again before invocation. Native authentication
belongs to Codex; no credentials are copied into a spool or console snapshot.

The client owns model/tool loops and context. Shell, editing, plugins, native
agent delegation and unrelated tools are disabled. The only optional MCP tool is
`read_approved_material` on an explicitly selected loopback endpoint. A request
must already have application-owned disclosure authorization. This initial adapter
is for data proposals, not general-purpose coding, arbitrary remote harness
protocols or remote environment lifecycle. Remote inference through the selected
Codex client is distinct from local spool execution.

The same existing process-group supervisor defaults to a 180-second native client deadline
and confirms its local termination. This is not confirmation that a remote model
stopped or that provider usage is zero. Native events remain private; completed
proposals still require domain validation and the existing kit-edit/artifact gates.
The client version, scope and task reference replace Shuttle's duplicate driver.

## Embedded app-server ownership

The host may supervise a pinned Codex app-server as a replaceable child process.
The harness owns model turns and private conversation history. The host owns task
identity, grants, native execution settlement and accepted products. A model turn
completion never accepts an artifact or proves a remote request stopped.

The first profile creates a fresh ephemeral thread per host harness attempt.
Set `task.app_server=true` on the existing harness request. It is part of the
harness permission digest; legacy requests omit it and keep their old identity.
This profile requires task authorization and refuses external MCP configuration.
Only the explicit prompt and results of authorized host tools enter that thread.
Shell, editing, plugins, implicit project instructions and native subagent tools
are disabled. Host-mediated delegation opens another fresh thread under an
operator-issued descendant grant. The child receives an explicit task and no
parent conversation. Grant references remain host-owned; bearer secrets never
enter model context. Every tool dispatch rechecks the original task grant and
concrete resource capability. Delegation may only select an existing descendant
of that grant and cannot restore rights removed from the child.

Approval binds the selected adapter, executable, model, output schema and scope.
Cancellation and revocation reach the existing independent owner. The owner
confirms local process-group termination; unknown results remain held without
replay. Work already admitted to a separate spool retains its original owner
and must be cancelled explicitly. Tool requests return to the original daemon;
its ordinary task checks and queue serialize effects. The model sees one
`host_request` tool with a JSON `request` argument. Previews grant nothing and
mutations still require an exact approval plus a concrete capability. An optional
`grant` ID selects an existing descendant. Only a delegating parent can select
one; models cannot issue bearer credentials or administer grants. Request/response
hashes and outcomes are retained in private `tool-receipts.jsonl`.
The app-server uses a private client home with a reference to the native file
login. User config, project rules and private sessions are not copied. This
initial file-login profile does not qualify keyring-only authentication. Local
termination permits deletion of that private client home. Protocol errors,
missing receipts and unsupported server requests fail closed without replay.
Domain conversion belongs to packaged programs
and application adapters, not to this protocol bridge.

## Task-scoped permissions

The operator socket issues `grant` with a `scope` containing `task`, `subject`,
`expires_ms` (host Unix milliseconds), `delegate` and `permissions`. Preview and
exact approval bind that scope. The response returns a private bearer credential
`{id, secret}` and a durable grant binding. Store credentials in private application
memory or files. Activity and queue records retain only grant IDs and digests.
`subject` attributes work to a role; possession of the credential authenticates
requests. The existing same-user operator endpoint remains trusted administration.

Use `daemon task-request ROOT JSON` for delegated calls. It connects to `tasks.sock`
in the same service and requires `{credential, request}` on every call. Missing,
forged or stripped credentials fail. This endpoint exposes only granted operations
and the caller's own descendant jobs. Both sockets share the existing queue,
supervisors and custody. Workloads must remain excluded from operator sockets,
host stores and other credentials by the selected execution profile. The native
client and application adapters are still trusted same-user software.

Permissions use exact resources, with at most 64 entries per grant.

| Kind | Fields and enforcement |
| --- | --- |
| `run` | Canonical registered `instance`, `run`, canonical `input`, maximum `seconds`; original start approval fixes the selected bytes and backend policy |
| `run-artifact` | Canonical `instance`, `run`, canonical `catalog`, exact `revision`, maximum `seconds`; the host alone resolves storage paths |
| `harness` | `profile_sha256` from operator `harness-permission` with the selected `task`; binds executable/hash, model, schema, scope and finite MCP selection |
| `read-input` | Canonical `path` and `sha256`; `read-input` returns exactly matching bytes |
| `accept` | Registered `instance` and `run`; delegates original acceptance and permits recording proposal adoption for that run |
| `artifact` | Registered `instance`, `run`, `output`; `artifact-read` verifies original custody and returns bytes plus the selected inventory |

The byte-read operations are bounded to 512 KiB. Permission selection does not
interpret arbitrary prompt text or prove its provenance. The trusted application
owns disclosure and domain checks. Every harness submission still requires the
original exact prompt approval; the launch binding also includes its grant.

`derive-grant` on the task endpoint uses the same scope fields. Its permissions
must be a subset, its task must match, and expiry cannot increase. A native run
deadline can decrease. The parent must allow delegation. A child can remove that
ability. The host checks the entire immutable ancestor chain, bounded to 16 grants;
the service retains at most 256 grants. Child credentials are returned only to the
issuing caller. The caller explicitly passes them to the intended adapter.

`revoke-grant` with `id` closes the selected grant and descendants after exact
approval. Scoped callers can revoke their own grant or descendants. `cancel` for
a permission-bound job previews and revokes its grant, including other jobs using
that grant. Use distinct child grants for independently cancellable work. Original
workers observe revocation/expiry during execution even after daemon loss; queue
reconciliation forwards cancellation to the original owners. Status inspection
and further cancellation remain available after revocation. New work, proposal
retrieval and artifact acquisition fail. Existing accepted products remain in
their original custody. Local termination evidence still comes from the original
supervisors; remote inference termination remains unknown.

`native_task_permissions.py` qualifies credential refusals, attenuation, cross-task
access, changed inputs, native filesystem denial, cancellation, persistent parent
revocation and worker expiry during daemon loss. `document_activity.py` qualifies
two real Codex agents with separate child grants, accepted PDF/Markdown/XLSX
handoff, cancellation and retained products. Pass `--shuttle-client` with the
compiled CAD daemon client to exercise its capability transport.

## Observation and acceptance

The console projects original instance, round-control, run, custody and kit labels.
It displays task order, cancellation, uncertainty and outcomes separately from
artifact acceptance. OS samples include the service and birth-identity checked
live task-owner descendants. Summed resident sizes may include shared pages; the
sample excludes exited workers, reparented processes and shared CAD/Krita apps.
No unavailable metric is reported as zero or used to authorize process control.
Model billing, historical resource accounting and automatic resource enforcement
are not inferred from these samples. The console is currently read-only. English is the default UI language; its language button selects the retained Simplified Chinese companion without changing authority or task records.

Run `tests/cli/native_daemon.py` with explicit `--host` and a fresh `--evidence`
directory on macOS. It covers duplicate service exclusion, bound approvals,
FIFO/concurrency, service loss without replay, queued cancellation, original
custody, lifecycle controls, external-client cancellation, resource observation and
console authorization. Its local harness fixture is not provider evidence.
CAD→Krita and real Codex/MCP tests remain separately required in Shuttle before
promoting a mature spool. Unsupported platforms refuse service execution.

## Async delegation

`delegate` takes `parent` and the existing finite harness `task`, returning a task
ID immediately after exact disclosure approval. `parent` is a correlation label,
not an authorization grant or isolation boundary. Same-user access rules apply.
`update-delegation` takes `id`, `strategy: "enqueue"` and `instruction`. It retains
the original harness, scope, schema and MCP binding, appends the instruction to
its prompt and requires a new approval. This is a new finite attempt, not a live
session resume: predecessor output is not automatically injected. The FIFO queue
starts it only after confirmed predecessor success. Failed, cancelled or unknown
predecessors leave it queued; explicitly cancel it to settle it without running.
Interrupt/replay and autonomous recursive spawning are not supported.

Use existing `task` and `cancel` operations. `delegation-inbox` takes `parent` and
returns durable task projections and completed proposals; repeated reads and
service restarts do not consume messages or replay execution. This minimum uses
pull delivery: automatic orchestrator wake-up and inbox acknowledgements are not
implemented. Proposals remain untrusted data, not accepted artifacts. Existing
queue limits bound all delegations; there is no second scheduler or model loop.

## Agent activity and adoption

`activity` with `id` returns a bounded view over retained request, claim, owner,
cancellation, completion and proposal records. Each source has a digest; record
categories are not a reconstructed wall-clock timeline. Native spool custody
continues to come from the original status/acceptance facade. The console's task
detail uses the same authenticated read-only activity view.

External Codex JSONL events are classified into fixed event/action categories,
with original line numbers and the retained source digest. At most 128 events
are returned; omitted and malformed lines are counted. Raw prompts, arguments,
messages, URLs and tool output are never copied into this view. These are harness
reports, not host-confirmed filesystem changes or authority. Missing events mean
unknown visibility. Original private events remain the source, with no duplicate
activity ledger.

`record-adoption` with `id`, registered `instance` and `run` previews an explicit
operator association between a confirmed proposal digest and original formally
accepted artifact custody. Exact approval writes one retained association under
the task. It cannot accept an artifact, change custody or prove semantic
equivalence. An unaccepted/cancelled run cannot be linked. The retained record
survives service restart and is included in activity queries.

The opt-in `document_activity.py` CLI qualification uses a synthetic PDF table,
two real finite Codex delegations and an operator-selected mature workbook
builder. PDF extraction and workbook authoring stay in application adapters;
original native spools publish the exact PDF, Markdown and XLSX bytes and retain
their custody. Approved adoption links connect the two proposals to accepted
Markdown/XLSX artifacts. Exported OOXML values and formulas are independently
checked. This is one text-PDF case, not OCR or native Excel qualification.

The document case takes `--host`, fresh `--evidence`, synthetic `--pdf`, qualified
`--codex`, `--node` and application-owned `--builder` paths. The controller needs
`pypdf`. The builder receives typed rows JSON and an output directory and writes
`result.xlsx`: source rows A6:C8, amount formulas D6:D8 and total D10. It never
installs tools or provisions services. The fixture checks Bolt (12 at 0.50),
Plate (3 at 8.00), Bearing (2 at 4.25), total EUR 38.50.

## Harness adapters and reconnect

`HarnessAdapter` supplies execution, capability discovery and normalized event
translation behind the existing daemon queue and independent task owner. Select
`task.adapter="codex-app-server"` for interactive Codex. `codex-exec` preserves the
finite profile. Existing omitted selectors and `app_server=true` retain their
serialized identities and permission profiles. Conflicting selectors are rejected.
Both Codex profiles remain pinned to CLI 0.166.0 and the approved executable hash.

Call `harness-connect` with `id` and optional `cursor` through the original task
endpoint. It returns the original execution handle, capability flags, task state,
normalized events, the next cursor and steering receipts. Reconnecting uses retained
records and the original worker. It survives a controller disconnect or daemon
restart while that worker stays alive. `resume_execution=false` explicitly describes
worker/provider loss. Unknown executions keep their original IDs without replay.
Cancelled credentials retain scoped observation and cancellation access.

Call `harness-steer` with `id`, `execution`, a unique `command` and `instruction`.
Preview and exact approval bind the instruction digest to the original execution.
The selected active turn is also bound. Idle sessions require an explicit continuation.
Commands arriving after their selected turn ends are rejected.
The caller must own the task and retain its harness capability. The worker rechecks
the original task grant before delivery. Up to 32 commands are ordered by admission,
with instructions bounded to 64 KiB. Reusing an identical command ID reconciles its
receipt. Reusing it for different content fails. The durable sent marker precedes
transport, so a crash between marker and acknowledgement remains unknown.
`queued` describes local admission. `acknowledged` records provider acceptance into
the same turn. It does not prove the requested change appeared in the result.

The Codex bridge maps steering to `turn/steer` with the original `expectedTurnId`.
Cancellation continues through the existing revocable grant and supervised process
group. Local release evidence gates settlement. Remote provider termination remains
unconfirmed. Final structured output remains a proposal until the existing spool
publication and acceptance path accepts a concrete product. The adapter owns no
scheduler, model loop, permission issuer or artifact store. Protocol details follow
the pinned binary schema and [Codex app-server documentation](https://learn.chatgpt.com/docs/app-server).

A future modified DeepSeek adapter must implement this contract, declare supported
capabilities, map its native session identities privately and pass the same scoped
steering, cancellation, reconnect and artifact-custody checks. Unsupported capabilities
must be rejected explicitly. Adding an adapter must preserve the existing host job,
grant lineage, cancellation owner and acceptance path.

`native_harness_adapter.py` verifies two background owners, steering acknowledgements,
stale handles, sibling denial, duplicate command rejection, isolated cancellation
and daemon restart with unchanged owners using deterministic protocol processes.
`document_activity.py --app-server` exercises real Codex proposals through the
existing PDF, Markdown and Excel custody path using explicitly selected inputs and
an application-owned workbook builder. Real-provider evidence is retained locally.

The standalone workspace shell is built with `python3 packages/workbench/build.py`.
Its TypeScript and HTML source use English. Simplified Chinese interface messages
are maintained in `packages/workbench/zh-CN.json` and embedded during the build.
User-authored workspace names, goals and records retain their original language.

### Bounded turns and measurements

Interactive tasks may declare `turn_limit` from 1 to 8 and `wall_seconds` from 1 to
3600. Defaults remain one turn and 180 seconds. Explicit values enter the task's
harness permission digest. The original owner, execution handle, thread, schema,
grants and supervisor span all declared turns. Finite exec supports one turn.

`harness-turn-result` takes `id` and a one-based `turn`. It returns an immutable
completed-turn proposal with `artifact_accepted=false` and
`termination_confirmed=false`. `harness-continue` takes `id`, `execution`,
`after_turn`, a unique `command` and `instruction`. Exact approval binds the
instruction and completed turn. One continuation is admitted for each completed
turn within the original limit. Identical commands reconcile their original
receipts. The sent marker precedes transport. Unknown commands retain original
identities. Cancellation and grant revocation also terminate an owner waiting
between turns. Final `harness-result` still requires original task settlement.

`activity.harness.metrics` projects numeric cumulative provider counters and
authorized resource disclosures from existing retained records. Cached input is
part of input, and reasoning output is part of output. Repeated cumulative updates
are deduplicated per session. Invalid or regressing counters reduce coverage.
Missing values remain `null`. Resource receipts expose digests, byte counts and
host dispatch timing. Actual model consumption remains unknown. `read-input` and
`artifact-read` accept optional `encoding="utf-8"` through the same authorization
checks. The default response retains the existing byte representation.

### Host task-session boundary

The internal task-session layer owns continuation admission, command reconciliation,
observation and authorized tool dispatch. Adapter capabilities are typed; steering
and continuation are independent capabilities. Codex native event and usage parsing
remain in the Codex adapter. Existing `harness-*` operations, persisted identities
and permission digests remain compatible. This extraction does not introduce a new
scheduler or authorize guest/provider reports to accept artifacts.

## Task-session Cocoon transport

The task-session middle layer accepts `cocoon-call`, `cocoon-status` and
`cocoon-reconcile` through the existing `host_request` tool. This is a bounded
transport foundation, not a qualified spool execution backend or artifact importer.
It neither starts VMs nor accepts exported files into artifact custody.

A trusted operator supplies a private JSON profile (mode 0600) with `socket`,
`grant`, `caller`, `vm` and `generation`. The profile contains only a work grant,
not the Cocoon management credential. The model receives the
profile path and SHA-256, never its contents. A task grant must include
`{"kind":"cocoon","profile_sha256":"…","operation_sha256":"…"}`; the latter
binds the serialized Cocoon operation exactly. Issue this capability through the
existing operator preview/approval flow. Do not grant the model profile read access.
Cocoon independently checks its own work grant on every request.

A `cocoon-call` request includes `profile`, `profile_sha256`, `operation_id` (a UUID)
and `operation`. Allowed operations are show, context, stop, guest and
export. Guest/export identities must match the profile VM, caller and generation.
Management operations, including grant issuance and VM creation, are unavailable.
The host creates a separate Cocoon request UUID and durably records its binding
before transport. Repeated identical local operation IDs observe the original
record; changed content is rejected. Transport loss remains unknown without replay.
Records belong to the original task session, including when a grant serves several
sessions. Credentials and guest payloads are omitted from intent records.

`cocoon-reconcile` takes the same profile, operation identity and exact-operation
capability. It queries Cocoon's original request ID and retains a confirmed success;
it never resubmits the operation. `cocoon-status` requires only the operation ID
inside the tool. Controllers may also use the authenticated task endpoint with
`id` and `operation_id`, including after task-grant revocation, to observe their
own records. This is local receipt observation, not renewed execution authority.
`harness-connect` also returns the session’s retained `cocoon_operations` receipts.
Every response keeps `artifact_accepted=false`. `observed` means a correlated
backend reply exists; VM termination and product acceptance require separate checks.
