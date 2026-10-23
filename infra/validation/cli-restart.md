# Cancellation followed by launch refusal

## Evidence and diagnosis

An earlier Waver Linux handoff CI run (`39645358759`, host revision
`bcbc4004ce2e505c50b5ecd2df9aab81ffa61d10`) failed after the signal test had
already verified cancellation, child reaping, no applied event and lock removal.
Its next in-process call returned `refused`. The original assertion discarded
the run's diagnostic, so that historical log alone cannot identify the errno.

On 2026-10-22, the unchanged runtime with an evidence-only assertion reproduced
the same next-run failure on native ARM Linux at parallel iteration 109:

```text
stderr: Text file busy (os error 26)
left: String("refused")
right: "succeeded"
```

The lifecycle test alone passed 150 iterations. A synchronized regression now
demonstrates the mechanism: a sibling forks while an executable snapshot is
open for writing; the parent closes its writer, but the sibling retains its copy
before exec. Linux refuses another exec of that inode with `ETXTBSY`. CLOEXEC
does not close descriptors until exec. This is not evidence of a stale
cancellation token, unreaped cancelled child, leftover store lock or permission
bypass. The historical CI errno remains unknown; the local reproduction makes
descriptor inheritance the supported explanation rather than a guessed retry.

## Product fix, not test isolation

An initial test-only mitigation isolated the execution fixtures. Isolating only
the signal test moved the same error to ordinary execution at stress iteration
3. Isolating the entire test group stabilized the harness but did **not** fix
the embedding process. That mitigation has been removed, including the isolated
writer probe. Green isolated tests were not proof of production correctness.

The product publisher is now used by registration, per-run snapshots and macOS
build-output acceptance:

1. The host writes bytes to a private `source.data` inode that is never executed,
   linked or renamed into an executable. A host fork may inherit that writer
   without retaining a writer on the executable inode.
2. Fixed `/bin/cp` creates a separate candidate inode in a new mode-0700 staging
   directory. Absolute arguments and an empty environment are used, with no
   shell, model-selected helper, build hook or project code execution.
3. The existing process supervisor bounds the helper to ten seconds, handles
   cancellation, and confirms exit/reaping. The host never opens the candidate
   for writing. This trusted system utility must not delegate writes to a
   surviving descendant; helper selection is not configurable by a spool.
4. The host verifies exact admitted bytes, sets mode 0500 and syncs through a
   read-only descriptor. A same-filesystem hard link publishes atomically
   without replacing an existing name. Named staging files are removed after
   the helper is supervised; the published destination is never cleaned up by
   the staging guard.
5. Only then may the sandbox launch the kit. A helper failure, timeout,
   cancellation or hash mismatch cannot launch a kit or produce an applied
   event. There is no in-process executable-copy fallback or automatic retry.

The run preview declares the helper and its separate deadline. Receipts record
publication evidence, helper PID/reaping and the kit's **separate** PID/reaping.
A helper PID is never reported as a started kit. Post-cancel CLI tests wait for
the actual kit, not the first child (which may now be the trusted publisher).
Build acceptance retains `publication.json` separately from compiler/test stages.

## Regression coverage

- Three independent host stores register and run in threads of one embedding
  process, while another thread repeatedly forks with a deliberately delayed
  pre-exec callback. This widens the inheritance window as fault injection,
  not as a product workaround. The real AgentControl and native sandbox paths
  execute 36 separately identified kit invocations per test, checking exact
  output, no retry, publication evidence, child reaping and store-lock release.
- All ordinary execution and cancellation tests run without process-isolation
  wrappers or forced single-threaded test execution. Both signal cancellation
  and the subsequent explicit operation use the real CLI.
- Preserve the next receipt and stderr in assertion output before fixtures are
  removed. A failed assertion must explain `refused`, not merely print a status.
- Add a deterministic inherited-writer regression with a pre-exec pipe barrier,
  a missing-interpreter refusal test and publication no-overwrite, cancellation,
  symlink and racing-publisher tests. Raw unsafe publication still demonstrates
  the Linux failure; the product publisher must execute successfully afterward.
- Persist additive `error` evidence for refusals: stage, message, optional OS
  code and I/O kind, and `retry_attempted: false`. `spawn-or-pre-exec` deliberately
  does not distinguish exec from a failing pre-exec policy syscall: Rust's
  process API does not provide that distinction. Errors without an OS errno
  keep `os_code: null`.

## Scope and remaining limits

This closes the inherited-writer window for **host-owned publication**, not
arbitrary executable files created or modified by third-party host code. A
trusted same-user process can still chmod/rewrite stored programs; the private
host-store trust assumption has not changed. Hashes identify admitted bytes,
not trusted code. Do not bypass the publisher by writing an executable directly.

This is not remote attestation, a power-loss-atomic transaction for the whole
store, or new Windows support. Fixed `/bin/cp` and a filesystem supporting hard
links are requirements; unavailable operations fail closed. Real sandbox
refusals remain inspectable and never retried automatically.

macOS is the primary product/acceptance platform for this iteration. Keep
existing native Linux code and CI regression coverage; do not expand Linux or
SPACE deployment scope. Common context, declaration and evidence formats stay
independent of Seatbelt. Linux-specific policy remains in its existing backend.

## Required CI

Repository rulesets require GitHub Actions checks with no bypass actors and
up-to-date branches:

| Repository / target | Required checks |
| --- | --- |
| an-agent: `dev-advance`, `main` | `native-host (ubuntu-latest)`, `native-host (macos-latest)` |
| an-waver: `main` | `test`, `host-handoff (ubuntu-latest)`, `host-handoff (macos-latest)` |

These are server-side settings, not guarantees provided by a YAML filename.
Renaming a job requires updating the gate in the same change. Existing branch
rules are preserved. CI checks do not replace maintainer review or sandbox
acceptance on a specific deployment. Waver pins the reviewed host revision;
it does not implement the host runtime or depend on Lima.
