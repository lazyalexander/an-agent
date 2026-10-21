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

## Changes and limits

- Run each real-execution integration test in its own test process, following
  the isolation approach used by the reference Codex PTY tests. Both operations
  now invoke the CLI; the cancelled run and explicit next run have distinct
  receipts, and the next output, reaping and lock release are checked.
  Isolating only the signal test was insufficient: the same error moved to the
  ordinary execution test in stress iteration 3. Isolation covers the entire
  execution test group, not just the originally failing assertion.
- Preserve the next receipt and stderr in assertion output before fixtures are
  removed. A failed assertion must explain `refused`, not merely print a status.
- Add a deterministic inherited-writer regression with a pre-exec pipe barrier,
  plus a real missing-interpreter refusal test covering retained input, failure audit,
  unlocked store, no process claim and exactly one invocation.
- Persist additive `error` evidence for refusals: stage, message, optional OS
  code and I/O kind, and `retry_attempted: false`. `spawn-or-pre-exec` deliberately
  does not distinguish exec from a failing pre-exec policy syscall: Rust's
  process API does not provide that distinction. Errors without an OS errno
  keep `os_code: null`.

There is no automatic retry, sleep workaround, relaxed sandbox rule, suppressed
failure or cancellation-state reset. Test isolation removes this harness's
writer/fork interference; it is not a general cure for arbitrary embedded host
threads forking while executable files are being written. A real host refusal
remains fail-closed and inspectable. Do not infer that every future `refused`
has this cause; inspect its own receipt first.

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
