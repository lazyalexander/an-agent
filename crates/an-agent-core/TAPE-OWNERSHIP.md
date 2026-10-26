# Tape writer ownership

`JsonlStore::open` acquires a nonblocking exclusive standard-library file lock
on the tape before returning. Ownership lasts until the store is dropped. A
second independent handle returns `StoreError::WriterBusy` before scanning,
reserving a sequence, appending or repairing. Other lock errors propagate; there
is no unlocked fallback or automatic retry. No new runtime dependency is needed.

The lock lives on the open tape file, not a removable lock marker. Symlink and
hard-link aliases refer to the same locked file. Reads, appends and truncation
use that same descriptor. The existing instance mutex still serializes operations
through a shared `Arc<JsonlStore>`. Separate tapes remain independent.

## Recovery and failure semantics

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

## Boundary and validation

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
