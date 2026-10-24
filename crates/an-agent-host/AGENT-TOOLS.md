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
| `run_kit` | Identity, granted handle, text | Native status, termination evidence, receipt digest; accepted artifact reference and text only on success |

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

Only a successful, uncancelled result enters the existing immutable artifact
store. Partial output from a cancelled or timed-out kit is not accepted. Result
publication happens after capability ownership is released. If the host itself
is forcibly killed, evidence may remain uncertain; this is not an exactly-once
remote execution protocol or recovery service. A remote/GPU adapter must provide
its own actual termination and late-result fencing before claiming equivalence.

## Verification

`file_runtime::agent_tools` drives the actual MCP binary and existing native
kernel path. It verifies tool discovery, source proposals without execution,
forbidden paths/handles, successful artifact checkout, duplicate refusal,
cancellation and disconnect cleanup. It runs in the macOS/Linux native CI suite.
The paid Kimi development/cancellation acceptance is maintained in Loom; native
agent tape stays client-managed. No provider credential is required in CI.
