#!/usr/bin/env python3
"""Real data/checkpoint selection, stale commits and cancellation retention."""
from stateful_input_release import qualify as qualify_release, contradictory_policy
import argparse
import hashlib
import json
from pathlib import Path
from host_qualification import require_native_workspace
import signal
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    options = parser.parse_args()
    require_native_workspace()
    root, host = options.evidence.resolve(), options.host.resolve(strict=True)
    root.mkdir(mode=0o700)
    calls = []

    def spawn(*args):
        return subprocess.Popen([str(host), *map(str, args)], text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def finish(process, success=True):
        try:
            out, err = process.communicate(timeout=30)
        except subprocess.TimeoutExpired as error:
            process.send_signal(signal.SIGINT)
            try:
                process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                raise RuntimeError(f"Termination unknown for {process.pid}; retain {root}") from error
            raise RuntimeError("Client deadline exceeded; no retry") from error
        calls.append({"args": process.args, "exit": process.returncode, "stdout": out, "stderr": err})
        (root / "commands.local.json").write_text(json.dumps(calls, indent=2))
        assert (process.returncode == 0) == success, (process.args, out, err)
        return json.loads(out) if out else None

    def call(*args, success=True):
        return finish(spawn(*args), success)

    def approved(*args):
        preview = call(*args)
        return call(*args, "--approve", preview["approval_sha256"])

    def package(name, files, outputs, state_slot=False):
        target = root / (name + "-release")
        target.mkdir()
        for path, code in files.items():
            (target / path).parent.mkdir(exist_ok=True)
            (target / path).write_text(code)
        req = {"runtime": "macos-native-process-v1", "os": "macos", "arch": "portable-source",
               "interaction": "headless", "gpu": "none", "inputs": ["batch.txt"], "editable": [],
               "capabilities": ["read-input", "write-declared-outputs"], "dependencies": []}
        if state_slot:
            req["state_input"] = "checkpoint.txt"
        (target / "native-spool.json").write_text(json.dumps({
            "schema": "an-native-spool-v3", "name": name, "version": "0.1.0", "entry": next(p for p in files if p.startswith("silk/")),
            "files": {path: hashlib.sha256(code.encode()).hexdigest() for path, code in files.items()},
            "outputs": outputs, "requirements": req,
            "events": {"consumes": ["artifact.accepted"] if state_slot else [], "produces": ["artifact.accepted"]}}))
        return target

    source = package("source", {"silk/main.sh": 'set -eu\ncat "$AN_SPOOL_INPUT" > batch.txt\n'}, ["batch.txt"])
    policy = {"schema": "an-native-checkpoint-managed-input-policy-v1", "state_output": "state.txt",
              "data": "checkpoint-pin", "parent_state": "not-required"}
    # The counter retains its source batch as provenance. The audit batch (100)
    # also needs its parent state, so an operator cannot choose to discard it.
    emit_policy = ("if [ \"$(cat \"$1\")\" = 100 ]; then\n" +
                   "echo '" + json.dumps(dict(policy, parent_state="checkpoint-pin")) + "'\nelse\n" +
                   "echo '" + json.dumps(policy) + "'\nfi > checkpoint-input-policy.json\n")
    target = package("counter", {
        "silk/main.sh": 'set -eu\n/bin/bash "$AN_SPOOL_KIT_DIR/step.sh" "$AN_SPOOL_INPUT" "$AN_SPOOL_STATE"\n',
        "kit/step.sh": 'set -eu\nif [ "$(cat "$1")" = hold ]; then\n'
                       '  printf "999\\n" > state.txt\nprintf ready > ready.marker\nsleep 60\nfi\n'
                       'awk \'/^[0-9]+$/ {sum += $1; next} {exit 2} END {print sum}\' "$2" "$1" > state.txt\n'
                       'cat state.txt > report.txt\n' + emit_policy,
    }, ["state.txt", "report.txt", "checkpoint-input-policy.json"], True)
    a, b, ws = root / "a", root / "b", root / "workspace"
    call("spool-load", source, a)
    call("spool-load", target, b)
    call("spool-events-init", ws)
    seed = root / "seed.txt"
    seed.write_text("10\n")
    initial = approved("spool-state-init", b, "state.txt", seed)["state"]["revision"]

    def produce(name, text):
        seed.write_text(text)
        approved("spool-start", a, name, seed, 15)
        approved("spool-accept", a, name)
        artifact = call("spool-artifacts", a, name)
        event = approved("spool-event-publish", ws, a, name, "batch.txt")["event"]
        return artifact, event

    first, event = produce("first", "2\n3\n")
    newer, audit_event = produce("newer", "100\n")
    old = first["outputs"][0]["binding"]["revision"]
    new = newer["outputs"][0]["binding"]["revision"]
    request = root / "selection.local.json"

    def select(artifact, state):
        request.write_text(json.dumps({"schema": "an-native-data-state-v1",
            "data": {"catalog": artifact["catalog"], "revision": artifact["outputs"][0]["binding"]["revision"]},
            "state_revision": state}))

    call("spool-start", b, "missing", seed, 15, success=False)
    call("spool-start-state", b, "legacy", initial, 15, success=False)
    assert not (b / "runs/missing").exists() and not (b / "runs/legacy").exists()
    select(first, initial)
    preview = call("spool-start-data-state", b, "direct", request, 15)
    assert preview["plan"]["data_input"]["binding"]["revision"] == old
    assert preview["plan"]["state_input"]["binding"]["revision"] == initial
    assert len(preview["plan"]["workspace"]["artifact_inputs"]) == 2
    select(newer, initial)
    call("spool-start-data-state", b, "direct", request, 15, "--approve", preview["approval_sha256"], success=False)
    assert not (b / "runs/direct/job").exists()
    select(first, initial)
    direct = call("spool-start-data-state", b, "direct", request, 15, "--approve", preview["approval_sha256"])
    assert direct["status"] == "succeeded" and direct["workspace"]["termination_confirmed"] is True
    approved("spool-accept", b, "direct")
    select(newer, initial)
    approved("spool-start-data-state", b, "stale", request, 15)
    approved("spool-accept", b, "stale")
    stale_preview = call("spool-state-commit", b, initial, "stale")
    for name in ["direct", "stale"]:
        approved("spool-reclaim", b, name)
        approved("spool-reclaim-snapshots", b, name)
    current = approved("spool-state-commit", b, initial, "direct")["revision"]
    conflict = call("spool-state-commit", b, initial, "stale", "--approve", stale_preview["approval_sha256"], success=False)
    assert conflict["status"] == "conflict" and conflict["automatic_retries"] == 0
    assert call("spool-state-show", b)["head"] == current

    route = root / "route.local.json"
    route.write_text(json.dumps({"schema": "an-native-artifact-route-v1", "source": str(a),
        "output": "batch.txt", "target": str(b), "result": "report.txt", "wall_seconds": 30}))
    approved("spool-event-subscribe", ws, "batches", route)
    call("spool-event-deliver", ws, event["id"], "batches", success=False)
    event_args = ["spool-event-deliver", ws, event["id"], "batches", "--state", current]
    preview = call(*event_args)
    run = preview["plan"]["context"]["delivery"]
    call("spool-event-deliver", ws, event["id"], "batches", "--state", initial,
         "--approve", preview["approval_sha256"], success=False)
    assert not (ws / "deliveries" / run).exists()
    delivered = call(*event_args, "--approve", preview["approval_sha256"])
    assert delivered["status"] == "succeeded" and delivered["consumed"] is False
    approved("spool-accept", b, run)
    head = approved("spool-state-commit", b, current, run)["revision"]
    approved("spool-event-complete", ws, event["id"], "batches")
    call("spool-event-deliver", ws, event["id"], "batches", "--state", head, success=False)
    assert call(*event_args)["disposition"] == "already-consumed"
    result_path = b / "runs" / run / "result.json"
    saved = result_path.read_bytes()
    try:
        result_path.unlink()
        uncertain = call(*event_args, success=False)
        assert uncertain["status"] == "uncertain" and uncertain["automatic_retries"] == 0
        assert len(list((b / "runs" / run / "job/attempts").iterdir())) == 1
    finally:
        result_path.write_bytes(saved)
    approved("spool-reclaim", b, run)
    approved("spool-reclaim-snapshots", b, run)

    state_claims = call("spool-input-references", b, run)["claims"]
    assert len(state_claims) == 2 and all(c["release"] is None for c in state_claims)
    call("spool-input-release", b, run, "--delivery", ws, "batches", success=False)
    assert "stateful inputs stay retained" in calls[-1]["stderr"]
    assert call("spool-input-references", b, run)["claims"] == state_claims
    assert call("spool-state-show", b)["head"] == head
    assert not (b / "runs" / run / "input-release-intent.json").exists()

    qualify_release(call, approved, ws, a, b, run, head, state_claims)

    _, holding = produce("holding", "hold\n")
    hold_args = ["spool-event-deliver", ws, holding["id"], "batches", "--state", head]
    preview = call(*hold_args)
    hold_run = preview["plan"]["context"]["delivery"]
    process = spawn(*hold_args, "--approve", preview["approval_sha256"])
    try:
        deadline = time.monotonic() + 15
        while not (b / "runs" / hold_run / "job/work/ready.marker").exists():
            if process.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError("Readiness not established; no retry")
            time.sleep(.02)
        call("spool-reclaim", b, hold_run, success=False)
        call("spool-reclaim-snapshots", b, hold_run, success=False)
        assert not (b / "runs" / hold_run / "reclaimed.json").exists()
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
        cancelled = finish(process, success=False)
    assert cancelled["status"] == "cancelled" and cancelled["result"]["workspace"]["termination_confirmed"] is True
    call("spool-accept", b, hold_run, success=False)
    call("spool-state-commit", b, head, hold_run, success=False)
    call("spool-event-complete", ws, holding["id"], "batches", success=False)
    assert call("spool-state-show", b)["head"] == head
    approved("spool-reclaim", b, hold_run)
    approved("spool-reclaim-snapshots", b, hold_run)
    assert not (b / "runs" / hold_run / "job/work").exists()
    assert not (ws / "deliveries" / hold_run / "ack.json").exists()
    assert call(*hold_args, success=False)["automatic_retries"] == 0

    for name, expected in [("direct", "15\n"), ("stale", "110\n"), (run, "20\n")]:
        artifacts = call("spool-artifacts", b, name)
        report = next(p for p in artifacts["outputs"] if p["path"] == "report.txt")
        out = root / (name + ".txt")
        call("artifact-checkout", artifacts["catalog"], report["binding"]["revision"], out)
        assert out.read_text() == expected
    retained = root / "batch-retained.txt"
    call("artifact-checkout", first["catalog"], old, retained)
    assert retained.read_text() == "2\n3\n"
    selected = call("spool-state-show", b, head)["selected"]
    restored = root / "state-retained.txt"
    call("artifact-checkout", selected["catalog"], head, restored)
    assert restored.read_text() == "20\n"
    # The same contract also names both files under the qualified Python runtime.
    python_source = package("python-state", {"silk/main.py":
        'import os\nfrom pathlib import Path\n'
        'data=Path(os.environ["AN_SPOOL_INPUT"]).read_text()\n'
        'state=Path(os.environ["AN_SPOOL_STATE"]).read_text()\n'
        'result=str(int(state)+sum(map(int,data.splitlines())))+"\\n"\n'
        'Path("state.txt").write_text(result)\nPath("report.txt").write_text(result)\n'},
        ["state.txt", "report.txt"], True)
    manifest_path = python_source / "native-spool.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["requirements"]["runtime"] = "macos-native-python-v1"
    manifest_path.write_text(json.dumps(manifest))
    site = root / "site"
    site.mkdir()
    exe = Path(sys.executable).resolve(strict=True)
    runtime = root / "runtime.local.json"
    runtime.write_text(json.dumps({"schema": "an-native-python-runtime-v1", "executable": str(exe),
        "executable_sha256": hashlib.sha256(exe.read_bytes()).hexdigest(),
        "stdlib": str(Path(sys.base_prefix).resolve()), "site": str(site),
        "site_sha256": hashlib.sha256(b"{}").hexdigest(), "dependencies": []}))
    p = root / "python-instance"
    call("spool-load", python_source, p, "--runtime", runtime)
    seed_state = approved("spool-state-init", p, "state.txt", restored)["state"]["revision"]
    select(first, seed_state)
    approved("spool-start-data-state", p, "resume", request, 15)
    approved("spool-accept", p, "resume")
    new_head = approved("spool-state-commit", p, seed_state, "resume")["revision"]
    py_state = call("spool-state-show", p)["selected"]
    continued = root / "python-state-retained.txt"
    call("artifact-checkout", py_state["catalog"], new_head, continued)
    assert continued.read_text() == "25\n"
    assert call("spool-state-show", b)["head"] == head
    contradictory_policy(call, approved, ws, a, b, head, audit_event, newer, calls)
    summary = {"schema": "native-data-state-qualification-v1", "passed": True,
        "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(), "data_revision": old,
        "newer_data_revision": new, "initial_state": initial, "committed_state": head,
        "stale_commit_refused": True, "unknown_delivery_retried": False,
        "cancel_termination_confirmed": True, "retained_after_collection": True, "automatic_retries": 0}
    summary["stateful_release_verified"] = {"released_attempt_inputs": 2, "retained_checkpoint_inputs": 1}
    summary["python_data_state_qualified"] = True
    (root / "acceptance.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
