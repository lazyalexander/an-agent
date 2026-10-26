# Environment input identity

`Workspace` stores host facts. An environment generation in `log.jsonl` selects
immutable Markdown bytes at `env/<sha256>`. `env.md` is a replaceable projection
for inspection, not a source of model input. An external editor must submit its
text through `put_env`; editing `env.md` does not create a generation and its
contents are replaced on reopen.

## Write and recovery contract

1. Write new bytes to a unique pending file, synchronize it, atomically publish
   the hash path without replacing an existing blob, and synchronize the blob
   directory on Unix. An existing blob must verify before it can be reused.
2. Append the generation record and synchronize the log. Successful log sync is
   the acknowledged generation commit point.
3. Build a unique projection file from the selected committed blob, synchronize
   it, rename it to `env.md`, and synchronize the workspace directory on Unix.

Projection errors return `EnvProjection` with the selected generation ID. The
log may already contain the new generation, so an error does not imply rollback.
`cite` can still return that generation from its verified blob. Reopening rebuilds
the projection without appending another generation. Repeating an identical named
write also verifies the blob, synchronizes the retained log and repairs the
current projection without duplicating a generation or rewinding to an older name.

A log write/sync failure has an uncertain commit outcome. A complete retained
record may be recovered; missing or incomplete records must not be inferred from
`env.md` or orphan blobs. Reopen synchronizes the retained log before returning.
An unterminated or malformed log is refused and left untouched for inspection.
There is no automatic truncation, rollback, retry or orphan-file collection.

## Read contract

`cite` selects the latest environment generation from the log, reads its blob,
verifies SHA-256 and decodes UTF-8. Missing hashes, invalid paths, missing or
unreadable blobs, digest mismatches and invalid UTF-8 reject the citation. It
never substitutes an empty string for a failed read. With no environment
generation, the citation has no environment ID/hash and intentionally empty text.
`env_bytes` also verifies the requested blob's hash, including historical blobs.

Only an empty workspace may initialize a new log. A nonempty workspace with a
missing log is refused; opening it cannot silently erase its generation history.
Old newline-terminated records remain readable, including records without `name`.
No log or blob migration is performed. Model input assembly already propagates
citation errors, so invalid environment evidence prevents a provider call.

The workspace remains host-owned. Its existing mutex coordinates one instance;
this slice does not add cross-process writer exclusion or protect against a
hostile process rewriting the ledger. Immutable publication means this API never
overwrites an existing blob; citations independently verify the bytes read.
Config and registration payload contracts are unchanged.

## Evidence

`workspace/env_tests.rs` covers the former live-file-before-log crash state,
missing/tampered projections, orphan pending files, corrupt/missing/unreadable
blobs, invalid metadata/UTF-8, missing/torn logs, named-write idempotency, and
reopening from retained files. Eleven per-instance test-only I/O fault points
exercise blob write/sync/publication, directory sync, log open/write/sync and
projection write/sync/rename. Each failure must leave a matching citation or a
refusal; reopening must recover from the retained authoritative files.

A real `AgentControl` regression proves corrupt environment bytes prevent model
invocation. These are deterministic I/O failure and persisted-state recovery
tests, not a hardware power-loss qualification. Filesystem durability still
depends on the platform honoring synchronization operations.
