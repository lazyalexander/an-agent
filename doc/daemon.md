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
| `submit` | `instance`, `run`, `input`, `seconds` | Preview/reserve an existing supervisor start and queue its reference |
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

The same existing process-group supervisor bounds the native client to 180 seconds
and confirms its local termination. This is not confirmation that a remote model
stopped or that provider usage is zero. Native events remain private; completed
proposals still require domain validation and the existing kit-edit/artifact gates.
The client version, scope and task reference replace Shuttle's duplicate driver.

## Observation and acceptance

The console projects original instance, round-control, run, custody and kit labels.
It displays task order, cancellation, uncertainty and outcomes separately from
artifact acceptance. OS samples include the service and birth-identity checked
live task-owner descendants. Summed resident sizes may include shared pages; the
sample excludes exited workers, reparented processes and shared CAD/Krita apps.
No unavailable metric is reported as zero or used to authorize process control.
Model billing, historical resource accounting and automatic resource enforcement
are not inferred from these samples. The console is currently read-only.

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
