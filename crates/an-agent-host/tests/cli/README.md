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

Each command has a bounded wait and records stdout/stderr and its exit status.
On a client deadline the runner requests host cancellation; unknown termination
fails the suite and keeps evidence. It never automatically reruns an execution.
All fixture directories, receipts and command observations are retained, including
on failure. Operational evidence contains local paths; do not publish it raw.
