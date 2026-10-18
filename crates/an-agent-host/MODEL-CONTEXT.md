# Model connections and explicit context

This is a host-library slice, not a new model gateway or autonomous agent.
Core is unchanged. Waver produces portable inputs; it does not retain model
credentials, install connections, manage sessions, or perform inference.

## Connection ownership

The trusted embedding host constructs `ModelConnection::new(spec, secret)` and
installs it with `AgentControl::set_model_client`. Use `connection.spec().clone()`
on the card: the admitted endpoint and model must be the actual wire identity.
The connection refuses changes to endpoint, model, or provider options before
sending bytes. Secrets come from the embedding application's resolver, not a
spool declaration or context packet. Do not put secrets in `extra_body`.

HTTPS is required except for literal loopback HTTP development endpoints.
Redirects and implicit environment proxies are disabled. HTTP error bodies are
not copied to tape; responses are bounded to 1 MiB. Input messages are bounded
to 64 messages / 64 KiB of content. Usage remains mandatory. This adapter is
text chat-completions only: no streaming, image, audio, video, or tool-call loop.
Provider-specific adapters may preserve richer native protocols later.

The legacy `HttpModelClient` remains a low-level transport with no endpoint
binding. Prefer `ModelConnection` for credentialed use. Its transport now also
rejects redirects, ignores environment proxy settings, bounds responses and
redacts HTTP failure bodies. Deployments requiring a proxy need an explicit
future adapter; there is no hidden environment fallback.

An endpoint binding is application policy, not a network namespace, DNS pin,
provider honesty proof or protection against malicious code inside the host.
The caller still authorizes disclosure and spending. No retries are automatic.

## Context wire contract

`an-context-v1` is one JSON object with exactly `schema`, `audience`, `source`,
`text`, and `sha256`. Audience is the destination runtime thread UUID. SHA-256
covers the exact UTF-8 text bytes; source is a bounded attribution label.
The whole JSON packet must fit 4096 bytes. Unknown fields, wrong audience,
invalid source, changed text and invalid hashes are refused.

`context_exchange::import(control, thread, bytes)` appends one untrusted data
clip and returns its exact ID. Select that ID in `BeatStep::InvokeModel`; do not
select the newest arbitrary clip. It never writes environment/system guidance,
opens referenced files, shares other sessions, or executes returned instructions.
The normal core path provides admission, request/response evidence and usage.
Do not use an infallible output sink to claim a fallible file write succeeded.

`an-agent-host context-check THREAD PACKET` validates without importing, granting
authority or calling a model. Waver's `context-packet` produces this wire format.
The [one-shot model CLI](MODEL-CLI.md) now runs explicitly approved requests
through this host library. A digest binds bytes, not authorship, truth or confidence. Source labels
are claims. Re-import can create duplicate notes; there is no deduplication or
cross-thread authorization service. The host must approve each import.

## Acceptance

`tests/model_client.rs` runs an actual local HTTP fixture through AgentControl,
a declared policy body, a bound credentialed connection and selected context.
It verifies unrelated notes/events are not sent and the secret is absent from
tape. Additional tests reject identity changes and malformed endpoints, preserve
usage validation and redact failed responses. `tests/host_agent.rs` verifies
the imported context stays out of the system message and is cited by the action.
Waver's pinned cross-repository test checks packet compatibility and audience
rejection through the real CLI. These are not live vendor-model quality tests.

Next: named profile management, stronger spending controls and authenticated
remote approval if needed. Do
not grant network/model access to the existing stdio-only kit profile to get it.
