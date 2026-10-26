# External input clones: fixed versions without a full payload copy

## Scope and decision

Reuse macOS `fclonefileat` inside the existing attempt snapshot preparation.
No new input mode, spool API, global catalog, backend or Loom dependency is added.
Ordinary selected inputs may avoid a full payload write on a compatible filesystem.
This is copy-on-write snapshotting, not a mutable-path mount, hard link, zero-read
hashing or a guarantee of zero physical I/O. Existing 64 MiB/file and 100 MiB/plan
bounds remain unchanged. Pinned artifact inputs retain their validated catalog path.

The [Apple syscall contract](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/clonefile.2)
describes separate inode attributes and shared data blocks, private subsequent
writes, exclusive destination creation and filesystem limitations. Writes after
cloning can still exhaust disk capacity. Clone support does not make an untrusted
native process equivalent to a VM.

## Version and authority boundaries

1. Existing approval identifies selected bytes by digest.
2. Open the source without following its final symlink, pin the descriptor, and
   check regular-file type, single link and size. The source descriptor avoids
   substituting a different inode between opening and cloning.
3. Exclusively clone into the existing host-owned attempt snapshot directory.
   Do not copy source ownership or request source ACL inheritance.
4. Verify the destination against the approved digest, apply read-only mode and
   sync it. Only then publish the resolved execution manifest and launch.
5. The workload receives only snapshot placeholders. Original selected paths
   remain denied. Acceptance rechecks persisted snapshot and manifest evidence.

Source changes during preparation either leave exactly the approved snapshot or
cause preparation to fail; a successful clone alone is insufficient. Later writes,
truncation, pathname replacement and unlink of the source cannot change the
accepted snapshot inode. This does not provide a transaction across independently
changing input files or protect against a malicious same-user host administrator.

Only `EXDEV` and `ENOTSUP` fall back to the existing bounded copy. Other errors
remain errors; a pre-existing destination is never overwritten. Linux retains the
existing copy implementation and the workspace execution profile remains macOS-only.
There is no new Linux reflink or automatic execution retry in this change.

## Cancellation and retention

Cancellation is checked before preparation, between files, after the clone and
before manifest publication/launch. The filesystem syscall is synchronous and is
not claimed to be preemptible. Existing bounded reads and hash checks remain;
this is not a new cancellation latency guarantee. Failed/cancelled preparation
keeps the existing explicit not-started settlement path and never promotes a
partial snapshot as an accepted product.

During execution, the existing job lock and termination evidence prevent collection.
Unknown termination refuses collection and output acceptance. After confirmed
termination, explicit reclaim removes work scratch; successful results must first
be accepted. **Input snapshots remain attempt evidence**, as before. No snapshot
garbage collector or new retention policy is introduced. Removing a clone reference
does not delete the original; deleting the original does not free blocks still
referenced by the retained clone. Capacity savings can diminish as sources change.

## Local evidence (2026-10-26)

The real macOS host test writes every byte of a 64 MiB input. Five cases exercise
in-place overwrite, truncation, pathname replacement, unlink and cancellation.
The workload waits while the source is mutated, then hashes the snapshot itself.
Checks include distinct inodes, fixed digest, refusal to collect while running,
refusal on missing termination confirmation, accepted output checkout after work
reclamation, and retained input evidence. A cancelled attempt cannot publish.
Additional unit coverage refuses links, oversized inputs, existing destinations,
changed prelaunch input hashes and missing/substituted manifests.

An independent local filesystem probe writes complete 64 MiB and 1 GiB files,
checks allocated source blocks, calls the same syscall and compares it with a
streaming full copy. Each size has three observations on warm local storage:

| Size | Clone syscall ms | Full copy + sync ms | Initial SHA-256 read ms |
| --- | --- | --- | --- |
| 64 MiB | 0.103–0.135 | 8.299–12.671 | 23.707 |
| 1 GiB | 0.118–0.135 | 102.648–264.341 | 369.025 |

These are not end-to-end host timings, statistically isolated benchmarks or SLAs.
The clone column excludes later hashing, chmod and fsync; the copy column includes
streamed reads/writes and fsync. Development tests use a debug binary and should
not be compared to these Python/OpenSSL hash timings. The 1 GiB probe qualifies
filesystem behavior only, not host admission at that size. It also checks digest
stability after overwrite, truncation and source unlink.

The opt-in `AN_AGENT_HOST_TIMING=1` diagnostic emits `snapshot_clone` with
`cloned`, `copy_fallback` or `error`. Successful clone records mean no explicit
payload copy was performed on that path, not that the operating system performed
no I/O. Raw logs include local paths and stay in private observation output.
The observer script stays in ignored `lab/input-clones/`; product regressions live
in `tests/workspace_job/clone_handoff.rs` and adjacent snapshot unit tests.

Cross-volume/unsupported-filesystem fallback is specified but not qualified on a
second local volume in this slice. Disk exhaustion, crash durability and hostile
process escape are not qualified by these measurements. Preserve the existing
fixed-version and termination gates before broadening any of those claims.
