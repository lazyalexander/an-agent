# Retained output and model context are separate products

## Source review and choice

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
a model result up to 1 MiB, but an explicit context packet is capped at 4 KiB.
Manual copying either loses provenance, overfills the packet, or encourages
accidental full-history transfer. A deterministic export closes this gap without
new model calls, execution permissions, Waver coupling, or core changes.

## Use

```sh
an-agent-host context-export RUN stdout THREAD_UUID 1024 NEW_CONTEXT_DIRECTORY
an-agent-host context-check THREAD_UUID NEW_CONTEXT_DIRECTORY/context.json
an-agent-host model PROFILE NEW_CONTEXT_DIRECTORY/context.json NEW_MODEL_OUTPUT
# Inspect the preview, then approve its exact digest using the existing model CLI.
```

Choose exactly one source: `artifact`, `stdout`, `stderr`, or `model`. The last selects
`output.txt` from a successful model run. Kit failure outputs may be selected
for diagnosis; their original status is preserved in the source label.

For a file kit, use `context-export RUN artifact THREAD_UUID 1024 NEW_OUTPUT`.
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

## Trust and responsibility

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

Waver can continue generating the same packet/CLI contracts. Kit authors retain
responsibility for meaningful domain summaries. Core remains admission and audit;
selection and export are host responsibilities. No Codex dependency is added.

## Acceptance

Tests cover byte-budget bounds across Unicode boundaries, exact short outputs,
repeatability, source preservation, refusal of overwrite/source-directory writes,
artifact tampering, path selection, symlinks, non-UTF-8 and escaped-packet overflow.
A real controlled kit produces the receipt used for export. The model CLI test
preserves a 5000-byte response, exports a bounded view, and submits only that view
through a second explicitly approved HTTP-fixture request. No live provider claim
or model-quality evaluation is implied.

File-profile acceptance additionally checks that the accepted product, not the
diagnostic stream or writable working copy, is exported. Failure, cancellation,
timeout, unreaped process, inconsistent size/path/hash and linked receipt or
artifact fixtures are rejected without creating a destination. This is local
receipt validation, not proof against a forged trusted run directory.
