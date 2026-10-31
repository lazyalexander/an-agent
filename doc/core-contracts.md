# Core identity and recording contracts

## Delivery authority and landing evidence

Delivery admission already binds the registration digest and generation, agent
incarnation and mounted spool instance. Publication now revalidates all four.
Changing registration and then restoring identical bytes does not restore an old
grant: its generation is different. An identical consecutive registration keeps
its generation and does not invalidate a reply.

### Publication boundary

After a beat body returns, or an agent body yields its final utterance, validation
and the spool note/intent admission share the existing supervision mutex with
registration changes, unmount and release. The check and publication are one
critical section. The same fence covers a halt note. No user body, hook, model
call or external callback executes under this critical section. Local tape I/O
remains inside it to preserve ordering.

A stale delivery returns `ReviewStale` and writes `delivery_stale` audit evidence;
it is not a body failure and does not automatically unmount, retry or reschedule
the body. Earlier valid model observations remain retained. After hooks may run
arbitrary host code, so offer/dispatch validate a reply again after those hooks.
A note published before revocation remains history; a later check cannot erase it.

### Host boundary

`SpoolReply` retains private admission facts. Hosts call `validate_reply` before
executing an intent. The included pump checks each Utter and Raise, and the CLI,
build and model-output paths check before starting their admitted action or
publishing the response artifact. The facts are in-memory authorization context,
not a signed transferable token or a new persistent replay format. Callers must
obtain replies through delivery rather than construct struct literals.

`applied` records a host claim about something already landed; it is not the API
that performs the external effect. It revalidates under the supervision lock.
Current claims keep the `applied` tag. A stale claim writes `applied_stale` with
its original facts, spool-note reference, host note and reason, then returns an
error. Evidence write failures propagate; no automatic retry or success claim is
synthesized. The pump records how many intents completed when revocation stops a
partially executed reply.

A preflight check is not an atomic transaction with an arbitrary application.
Revocation can occur after a check or during a sink callback. Already completed
effects cannot be rolled back by core: the host must preserve/report them, stop
remaining work and use its own revision checks or transaction boundary where
needed. This slice does not introduce application-specific rollback, a global
external-operation lock or cross-process workspace coordination.

### Recovery and verification

A reopened session has a new incarnation, and remounting has a new mount ID.
Retained old replies cannot be confirmed against those new identities, even if
registration bytes are unchanged. Fresh delivery is required; old tape history
and model blobs remain readable and are not rewritten into new grants.

Tests cover a blocked beat during registration replacement, replacement after
model completion, after-hook revocation, identical versus ABA registration,
missing facts, reopen/remount, failed landing-evidence writes, and a real host
pump whose sink revokes authority between an Utter and a Raise. The latter keeps
the visible effect and a partial stale claim while refusing the remaining Raise.

## Tape writer ownership

`JsonlStore::open` acquires a nonblocking exclusive standard-library file lock
on the tape before returning. Ownership lasts until the store is dropped. A
second independent handle returns `StoreError::WriterBusy` before scanning,
reserving a sequence, appending or repairing. Other lock errors propagate; there
is no unlocked fallback or automatic retry. No new runtime dependency is needed.

The lock lives on the open tape file, not a removable lock marker. Symlink and
hard-link aliases refer to the same locked file. Reads, appends and truncation
use that same descriptor. The existing instance mutex still serializes operations
through a shared `Arc<JsonlStore>`. Separate tapes remain independent.

### Recovery and failure semantics

Opening now creates the tape immediately, even before its first event. Successful
open does not validate or repair event contents; the first read/append does so
under exclusive ownership. The event schema and legacy-line parsing are unchanged.

`read_all` is an owner operation with repair authority, not a read-only observer.
There is no independent read-only store API in this slice. Live inspection should
use the owning store/control's event API. Session resume and recorder recovery
must release the old owner first; they may not open a second repair-capable handle.
Any retained agent/store references continue to own the lock after a seat release.

Append errors invalidate the cached next sequence. A complete record retained
after a sync error consumes its sequence on the next explicit operation or reopen;
a partial final line follows the existing quarantine path. The API does not retry
an action or promise that an append error means nothing was recorded.

Repair synchronizes the quarantine evidence before truncating the tape. Failure
before truncation can duplicate evidence on a later recovery, but must not lose
it. Failure after truncation can leave an uncertain sync result; reopening scans
the actual retained tape. Failed reads/repairs also invalidate the sequence cache
so a subsequent append cannot bypass unfinished repair. Errors never release a
live store's ownership. Process exit releases the OS lock without stale-marker
cleanup; the next owner resumes from the retained events.

### Boundary and validation

This is cooperative host-owned storage coordination, not a sandbox against a
process that ignores advisory locks or unlinks/replaces the owned tape. Do not
replace or remove tape files while their owners are alive. Network filesystems
are not qualified; unsupported locking fails closed. The tested targets are
native macOS and Linux, not every filesystem or OS. No hardware power-loss
qualification is claimed.

Tests retain the existing shared-Arc concurrency and legacy/torn-tail coverage,
and add independent handles/threads, path aliases, a real competing
`AgentControl` resume, append/repair fault points with and without reopen, and
separate-process contention. A process is killed while holding a partially written
tail; competitors must leave it untouched, and a fresh process must quarantine
it and continue the sequence. The host process-supervision test module launches only its own test executable.
Core owns the lock and in-process fault tests; the existing host boundary owns
child creation and cleanup. Architecture allowances and baselines are unchanged.

## Environment input identity

`Workspace` stores host facts. An environment generation in `log.jsonl` selects
immutable Markdown bytes at `env/<sha256>`. `env.md` is a replaceable projection
for inspection, not a source of model input. An external editor must submit its
text through `put_env`; editing `env.md` does not create a generation and its
contents are replaced on reopen.

### Write and recovery contract

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

### Read contract

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

### Evidence

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
