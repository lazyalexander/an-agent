# Host CLI acceptance

Build the selected host binary once, then pass its explicit path. No runner
builds a binary, reads model credentials or invokes Loom. Python 3 standard
library is sufficient.

```sh
cargo build --locked -p an-agent-host --bin an-agent-host
python3 crates/an-agent-host/tests/cli/native_state.py \
  --host target/debug/an-agent-host --evidence /private/tmp/native-state-evidence
```

`native_state.py` requires the real macOS native backend; other platforms report
a skip, not successful runtime qualification. It tests explicit initialization,
fixed state input, successful accepted-output commit, stale conflicts, cancelled
candidate refusal, interrupted HEAD publication, orphan refusal, idempotent
commit confirmation, scratch reclamation and a fresh instance seeded explicitly
from a fixed checkpoint. The counter rules belong to this fixture, not the host.

The round-control scenario also qualifies explicit pause/resume, checkpoint-bound
suspension, stale approvals/proposals, lost-head fencing, interrupted control
publication, changed-kit/state refusal, an independent second instance and
scratch reclamation while suspended. Live execution and unknown start evidence
refuse control; the live test requires confirmed cancellation before proceeding.
The safe point is a completed, formally accepted round, not in-process signaling.
Optional `--legacy-host /path/to/pre-control/an-agent-host` verifies that an older
host reads the legacy instance before activation and refuses the upgraded instance
after pause and resume. Build each selected binary once and use stable copies;
never rebuild a shared executable while an external suite is invoking it.

Each command has a bounded wait and records stdout/stderr and its exit status.
On a client deadline the runner requests host cancellation; unknown termination
fails the suite and keeps evidence. It never automatically reruns an execution.
All fixture directories, receipts and command observations are retained, including
on failure. Operational evidence contains local paths; do not publish it raw.
