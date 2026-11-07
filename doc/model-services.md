# Model, context and coding-tool services

## Native harness ownership

Use existing coding clients for model/tool loops, shell tools, session assembly
and compaction. The [qualified direct MCP route](https://github.com/lazyalexander/an-loom/tree/9089b0283cc9077f51447e38b32f553049948d98/examples/finite-mcp)
uses the existing host service. Domain instructions, code and acceptance rules
stay in the spool project; the client owns its private context and tape.

The unused `an-agent-context` and `an-agent-factory` crates and the generic
`spools/agent_loop` sample are retired. No retained runtime package depended on
them. This removes the context sidecar/cache implementation, default Bash
constructor, client-style timeout/cancellation code and sample ReAct policy.
The kernel still accepts explicit tool constructors through `build_with` and
`spawn_with`; it does not implicitly install a shell. Managed work uses the
existing native spool/workspace execution profiles and their termination/custody
gates. A coding client's local shell has that client's policy, not a new host grant.

Existing Rust consumers of the removed crates must explicitly migrate or use
the [last maintained source](https://github.com/lazyalexander/an-agent/tree/69f66023c12a27e97b8a4804194272fc13c4b1f0).
Old tape, `ctx/` sidecars and published descriptors remain operator-retained
history; no conversion, compaction or replay is performed by retirement.
Legacy event/profile identities remain unchanged. The host's explicit context
packets, visibility/disclosure checks, model-call admission, finite MCP tools and
execution/artifact receipts remain separate supported services.

## Retired model transport and command

The host no longer ships `HttpModelClient`, `ModelConnection`, `ModelCallControl`,
`model_run`, the `host-model` descriptor, or the sample provider configuration.
Their only in-repository consumers were the model command and its own tests;
Loom and Shuttle have no dependency on them. CAD material review and missing
finish completion already use the same qualified finite Codex entry in Shuttle.
There is no replacement host model launcher, provider fallback, or credential store.
The removed `model` command refuses before reading inputs or creating output.

Existing library consumers must migrate explicitly or pin the
[last maintained development source](https://github.com/lazyalexander/an-agent/tree/ba12ad9722af6da41721637a66516031bc2928b0).
Use the native client's authentication, model protocol, context management and
cancellation. Review disclosure before passing domain-selected data to that
client; a host execution grant does not authorize model disclosure. Do not feed
host context packets to a client as trusted instructions or give it the whole
run store. Receive its result as untrusted bytes and validate it before separately
approving any host execution. There is no automatic migration of old profiles,
credentials, transcripts or pending attempts, and uncertain attempts are not retried.

The core `ModelClient` embedding interface, model admission/disclosure and usage
accounting remain available to application-owned adapters. This retirement does
not change core wire/event identities, native peer context delivery, execution
supervision, termination evidence, or artifact custody. The receipt-checked
`context-export ... model` selector remains read-only compatibility for historical
`an-model-receipt-v1` artifacts; it cannot call a provider.

## Explicit context

### Context wire contract

`an-context-v1` is one JSON object with exactly `schema`, `audience`, `source`,
`text`, and `sha256`. Audience is the destination agent UUID for a model import. SHA-256
covers the exact UTF-8 text bytes; source is a bounded attribution label.
The whole JSON packet must fit 4096 bytes. Unknown fields, wrong audience,
invalid source, changed text and invalid hashes are refused.

`context_exchange::import(control, agent_id, bytes)` appends one untrusted data
clip and returns its exact ID. Select that ID in `BeatStep::InvokeModel`; do not
select the newest arbitrary clip. It never writes environment/system guidance,
opens referenced files, shares other sessions, or executes returned instructions.
The normal core path provides admission, request/response evidence and usage.
Do not use an infallible output sink to claim a fallible file write succeeded.

`an-agent-host context-check AUDIENCE_UUID PACKET` validates without importing, granting
authority or calling a model. The legacy Waver `context-packet` command produced
this wire format; this is not a claim that current Loom exposes that command.
A digest binds bytes, not authorship, truth or confidence. Source labels are
claims. Re-import can create duplicate notes; there is no deduplication or
cross-thread authorization service. The host must approve each import.

For retained kit or historical model output, use the bounded export below.
It does not automatically import or disclose a packet. `tests/host_agent.rs`
checks that imported context stays out of system guidance and is cited by the
action. `tests/artifact_context.rs` preserves export/tamper checks for accepted
kit outputs and legacy model receipts without a provider or HTTP fixture.

## Retained output and model context are separate products

### Source review and choice

Reviewed the clean Codex source snapshot
`2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4`. These are design references,
not a runtime dependency or a copied Codex module.

| Codex mechanism | Host decision |
| --- | --- |
| [Context-window accounting](https://github.com/openai/codex/blob/2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4/codex-rs/core/src/session/context_window.rs): full-window cap is distinct from compaction thresholds | Keep byte bounds honest. Do not claim tokenizer-independent token accounting or introduce a conversation scheduler for one-shot calls. |
| [History output truncation](https://github.com/openai/codex/blob/2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4/codex-rs/core/src/context_manager/history.rs#L553-L574): budget the view and account for serialization | Adopt separate artifact and context views, including the existing serialized packet cap. |
| [Head/tail output retention](https://github.com/openai/codex/blob/2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4/codex-rs/core/src/unified_exec/head_tail_buffer.rs): retain beginning/end and count omissions | Adopt a small pure UTF-8 selection function over an already bounded artifact; do not change live pipe collection or output-limit outcomes. |
| [Retained-context ordering](https://github.com/openai/codex/blob/2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4/codex-rs/core/src/session/retained_context.rs): persistence and source ordering are explicit | Preserve receipt, artifact and derived-packet identities. Defer multi-turn history ordering until a host consumer needs it. |
| [Unified execution](https://github.com/openai/codex/blob/2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4/codex-rs/core/src/unified_exec/mod.rs) and [process lifecycle](https://github.com/openai/codex/blob/2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4/codex-rs/core/src/unified_exec/process.rs): approval, process management, cancellation, confirmed termination and bounded buffers | Keep platform confinement separate from shared supervision. Current five-second stdio jobs do not justify PTY/session pooling. Do not adopt policy-dependent unsandboxed retries. |

The concrete gap was output transfer: a kit retains up to 64 KiB per stream and
a historical model result up to 1 MiB, but an explicit context packet is capped at 4 KiB.
Manual copying either loses provenance, overfills the packet, or encourages
accidental full-history transfer. A deterministic export closes this gap without
new model calls, execution permissions, development-tool runtime coupling, or
core changes.

### Use

```sh
an-agent-host context-export RUN stdout AUDIENCE_UUID 1024 NEW_CONTEXT_DIRECTORY
an-agent-host context-check AUDIENCE_UUID NEW_CONTEXT_DIRECTORY/context.json
## Inspect the selected packet and explicitly authorize disclosure to your native client.
```

Choose exactly one source: `artifact`, `stdout`, `stderr`, or `model`. The last selects
`output.txt` from a successful historical model run; no new model run is launched. Kit failure outputs may be selected
for diagnosis; their original status is preserved in the source label.

For a file kit, use `context-export RUN artifact AUDIENCE_UUID 1024 NEW_OUTPUT`.
This selects only the host's accepted `artifact.txt`, never `work/output.txt`
or diagnostic stdout. The receipt must report success, exit zero and a reaped
process, and bind the fixed artifact path, exact byte count and SHA-256. A failed
run cannot promote a partial file to accepted context. Missing, oversized,
symlinked, hard-linked, changed or non-UTF-8 artifacts are refused before output
creation. Receipt files are also bounded regular files without links.

This selector adds no new context ABI or automatic model call. The existing
head/tail budget and explicit audience still apply; it is not semantic routing.

The text budget is 128–2048 **UTF-8 bytes**, including the omission marker, not
tokens. A short artifact passes unchanged. A longer artifact retains a prefix
and suffix with a conspicuous middle-omission marker. This is not a semantic
summary: important middle content may be absent. No relevance ranking is claimed.
JSON escaping must also fit the existing 4096-byte packet cap. An overflow is
rejected before writing; select a smaller budget rather than silently widening
the contract. Binary/non-UTF-8 artifacts are rejected, not repaired.

`context.json` uses the unchanged `an-context-v1` ABI. `selection.json` records:

- source run location, receipt digest, artifact identity and original outcome;
- exact half-open byte ranges and omitted byte count within the retained file;
- selection strategy, byte budget, audience and final packet digest.

The CLI prints only paths and small metadata, not the artifact contents. Files
remain private, the output directory must be new and outside the source run,
and exports never overwrite an earlier selection. Failed persistence may leave
partial evidence but is never reported as success.

### Trust and responsibility

The host reads only fixed filenames for recognized receipts, checks the retained
artifact hash, rejects static symlinks/hardlinks and bounds every read. It does not follow
arbitrary paths embedded in a receipt. The operator selects the private run and
destination audience. No run-store discovery, thread import, model call, shell
execution or grant occurs during export.

The run store is still trusted: changing both receipt and artifact can forge
their relationship. Hashes prove byte identity, not authorship, factual confidence
or authority. Selection metadata is local evidence, not an authenticated remote
attestation. A later receiver still validates the packet and explicitly approves
disclosure. Prompt injection is not solved by truncation or digest checks.

Omission counts concern the retained artifact, not the whole process transcript:
the execution output cap may already have stopped and truncated a noisy process.
No full-output recovery, replay or token-savings percentage is promised.

Development clients can target these packet/CLI contracts. Kit authors retain
responsibility for meaningful domain summaries. Core remains admission and audit;
selection and export are host responsibilities. No Codex dependency is added.

### Acceptance

Tests cover byte-budget bounds across Unicode boundaries, exact short outputs,
repeatability, source preservation, refusal of overwrite/source-directory writes,
artifact tampering, path selection, symlinks, non-UTF-8 and escaped-packet overflow.
A real controlled kit produces the receipt used for export. A retained legacy
model fixture checks bounded selection, audience, unchanged source bytes and
refusal of changed hash, failed outcome, linked files and oversized output.
No HTTP fixture, live provider claim or model-quality evaluation is implied.

File-profile acceptance additionally checks that the accepted product, not the
diagnostic stream or writable working copy, is exported. Failure, cancellation,
timeout, unreaped process, inconsistent size/path/hash and linked receipt or
artifact fixtures are rejected without creating a destination. This is local
receipt validation, not proof against a forged trusted run directory.

## Finite coding-agent capabilities

`an-agent-tools` is an operator-started MCP stdio service. Transport uses the
official `rmcp` SDK; authority, native execution and artifact acceptance remain
in an-agent-host. Loom prepares projects and packages reviewed outputs; it does
not implement the broker, execution supervisor or a replacement coding agent.

### Trust boundary

The third-party CLI and its model connection are trusted operator software.
The model's requests and proposed source are untrusted data. A qualified client
profile must enforce its tool allowlist at execution, not just hide tools in the
prompt. This service does not contain a malicious CLI executable, intercept its
model connection, or prevent native plugins from accessing the user's files.
Do not use the reviewed workspace job profile as an untrusted-agent sandbox.

Kits run through the existing native file-kit sandbox with scrubbed environment,
bounded input/output and host lifecycle supervision. Their filesystem/network
authority does not come from the coding client's permissions. A proposal is
written as data and never compiled by a tool call. A separate operator review
and development grant are required before build/test and packaging.

Client credentials stay in the client's original authentication service. Never
copy or link credentials into a project, capability session or build directory.
Agent instructions, skills and MCP configurations are executable inputs to review.
Prompts and repository instructions cannot enlarge a host grant.

### Operator contract

Create a private, operator-owned JSON file outside agent-editable directories:

```json
{
  "schema": "an-agent-tools-v1",
  "store": "/operator/selected/store",
  "session": "/operator/new-session",
  "source": "/operator/selected/main.rs",
  "kits": {"sort": "FULL_64_CHARACTER_RELEASE_SHA256"},
  "max_calls": 16
}
```

```sh
an-agent-tools grant.local.json
## Review selected releases, source hash, destination and limits first.
an-agent-tools grant.local.json --approve REVIEWED_APPROVAL_SHA256
```

Preview creates no session. Approval binds the file bytes, selected source,
release checks and destinations. The session must be new and outside the store;
the grant must be outside the session. Profiles and stores assume trusted local
ownership, not protection against concurrent hostile same-user mutation.

Only three tools exist:

| Tool | Input | Result |
| --- | --- | --- |
| `read_source` | Invocation identity | Selected UTF-8 source and digest |
| `propose_source` | Identity and text | Candidate reference/digest; accepted=false, executed=false |
| `run_kit` | Identity, granted handle, text | Native status, termination evidence, receipt digest and publication outcome; artifact reference and text only after commit |

Closed request schemas reject extra paths, commands and capability fields.
There are at most eight handles and 32 submitted calls, 64 KiB source/proposal,
8 KiB kit input and a 512 KiB decoded request envelope. The transport remains a
trusted-local-client service, not an Internet-facing resource-exhaustion boundary.
Invalid requests receive bounded denial evidence; each admitted invocation gets
an exclusive intent before effects. Reusing an identity is refused. A pending or
uncertain submission is reconciled from its ledger, never automatically retried.

### Cancellation and delivery

MCP cancellation, input EOF, transport read error and process SIGINT/SIGTERM signal
active native work to stop. The broker waits for native supervision to settle;
it does not equate a client disappearing with a terminated kit. EOF is observed
before the SDK drains active handlers. A cancelled MCP request may have no tool
response: inspect the durable native receipt and `calls/IDENTITY/result.json`.

Native success is necessary but not sufficient for artifact admission. Each call
has one atomic decision shared by MCP cancellation, transport-drop cancellation,
EOF/shutdown and the publisher:

- Cancellation wins the transition from open: import never starts. A native run
  that already succeeded retains `execution_status=succeeded`, while the tool
  result reports `status=cancelled` and `publication.status=cancelled-before-commit`.
  No artifact or output text is delivered by that call.
- Publication wins the transition from open to committing: subsequent cancellation
  is too late to prevent this import. Before import, the host writes
  `calls/IDENTITY/publication-intent.json`, binding the native run ID and receipt
  digest. Successful import reports `publication.status=committed`, the immutable
  revision and text. The `cancellation=too-late-after-commit-start` field describes
  this policy; it does not claim a late cancellation actually occurred.

A cancelled or timed-out native run never reaches the publication decision and
reports `publication.status=not-attempted`. Existing termination and integrity
checks inside artifact import remain mandatory. The atomic transition orders
host-observed cancellation against admission; a notification still in transit
cannot undo an admitted commit. It is not a transaction across a client response
and filesystem writes.

An import/result failure after admission remains `refused-or-uncertain`; a retained
publication intent is reported as `publication.status=uncertain`. The intent proves
admission, not successful commit. Inspect the native evidence and immutable catalog;
never retry automatically or delete a published revision to simulate rollback.
The same decision cannot reopen after failure. A failed intent write prevents import.
A disconnected client may miss the response even after commit: the durable call
result is authoritative when present. Result publication happens after capability
ownership is released. If the host itself
is forcibly killed, evidence may remain uncertain; this is not an exactly-once
remote execution protocol or recovery service. A remote/GPU adapter must provide
its own actual termination and late-result fencing before claiming equivalence.

### Verification

`file_runtime::agent_tools` drives the actual MCP binary and existing native
kernel path. It verifies tool discovery, source proposals without execution,
forbidden paths/handles, successful artifact checkout, duplicate refusal,
cancellation and disconnect cleanup. It runs in the macOS/Linux native CI suite.
Deterministic publication tests also force each side of the race to win using
channels and real catalog writes, including cancellation after the old flag-check
window and a lost result after commit. They check no publication on cancellation,
retained revisions after late cancellation, intent write failures and no retries.
The paid Kimi development/cancellation acceptance is maintained in Loom; native
agent tape stays client-managed. No provider credential is required in CI.
