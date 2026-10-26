# Finite coding-agent capabilities

`an-agent-tools` is an operator-started MCP stdio service. Transport uses the
official `rmcp` SDK; authority, native execution and artifact acceptance remain
in an-agent-host. Loom prepares projects and packages reviewed outputs; it does
not implement the broker, execution supervisor or a replacement coding agent.

## Trust boundary

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

## Operator contract

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
# Review selected releases, source hash, destination and limits first.
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

## Cancellation and delivery

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

## Verification

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
