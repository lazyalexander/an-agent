#!/usr/bin/env python3
"""Multi-round real headless capacity maintenance without deleting formal products."""
import argparse
import hashlib
import json
from pathlib import Path
from host_qualification import require_native_workspace
import signal
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    options = parser.parse_args()
    require_native_workspace()
    host, root = options.host.resolve(strict=True), options.evidence.resolve()
    root.mkdir(mode=0o700)
    calls = []
    size, rounds = 8 * 1024 * 1024, 4

    def spawn(*args):
        return subprocess.Popen([str(host), *map(str, args)], text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def finish(process, success=True):
        try:
            out, err = process.communicate(timeout=45)
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

    def package(name, state=False):
        release = root / (name + "-release")
        (release / "silk").mkdir(parents=True)
        if state:
            code = ('set -eu\nawk \'{s+=$1} END {printf "%.0f\\n",s}\' '
                    '"$AN_SPOOL_INPUT" "$AN_SPOOL_STATE" > state.txt\n'
                    f'if [ "$(cat "$AN_SPOOL_STATE")" -ge {3 * size} ]; then\n'
                    'printf ready > ready.marker\nsleep 12\nfi\ncat state.txt > report.txt\n')
        else:
            code = 'set -eu\ncp "$AN_SPOOL_INPUT" scratch.bin\nwc -c < scratch.bin > data.txt\n'
        (release / "silk/main.sh").write_text(code)
        req = {"runtime": "macos-native-process-v1", "os": "macos", "arch": "portable-source",
            "interaction": "headless", "gpu": "none", "inputs": ["batch.txt"], "editable": [],
            "capabilities": ["read-input", "write-declared-outputs"], "dependencies": []}
        if state:
            req["state_input"] = "checkpoint.txt"
        (release / "native-spool.json").write_text(json.dumps({
            "schema": "an-native-spool-v3", "name": name, "version": "0.1.0", "entry": "silk/main.sh",
            "files": {"silk/main.sh": hashlib.sha256(code.encode()).hexdigest()},
            "outputs": ["state.txt", "report.txt"] if state else ["data.txt"], "requirements": req,
            "events": {"consumes": ["artifact.accepted"] if state else [], "produces": ["artifact.accepted"]}}))
        return release

    a, b, ws = (root / name for name in ["producer", "consumer", "events"])
    call("spool-load", package("producer"), a)
    call("spool-load", package("consumer", True), b)
    call("spool-events-init", ws)
    config = root / "route.json"
    config.write_text(json.dumps({"schema": "an-native-artifact-route-v1", "source": str(a),
        "output": "data.txt", "target": str(b), "result": "report.txt", "wall_seconds": 30}))
    approved("spool-event-subscribe", ws, "counts", config)
    seed = root / "zero.txt"
    seed.write_text("0\n")
    head = approved("spool-state-init", b, "state.txt", seed)["state"]["revision"]
    large = root / "external-input.txt"
    large.write_bytes(b"x" * size)
    large_hash = hashlib.sha256(large.read_bytes()).hexdigest()
    produced, consumed, heads = [], [], []

    def produce(index):
        run = f"round-{index}"
        approved("spool-start", a, run, large, 15)
        approved("spool-accept", a, run)
        event = approved("spool-event-publish", ws, a, run, "data.txt")["event"]["id"]
        produced.append(run)
        return event

    def prepare(index):
        event = produce(index)
        args = ["spool-event-deliver", ws, event, "counts", "--state", head]
        preview = call(*args)
        run = preview["plan"]["context"]["delivery"]
        consumed.append(run)
        return event, run, args, preview["approval_sha256"]

    def commit(event, run, previous):
        approved("spool-accept", b, run)
        next_head = approved("spool-state-commit", b, previous, run)["revision"]
        approved("spool-event-complete", ws, event, "counts")
        heads.append(next_head)
        return next_head

    def request(label, material, runs):
        path = root / (label + ".json")
        path.write_text(json.dumps({"schema": "an-native-private-collection-request-v1",
                                   "material": material, "runs": runs}))
        return path

    def collect(instance, label, material, runs):
        path = request(label, material, runs)
        preview = call("spool-storage-collect", instance, path)
        call("spool-storage-collect", instance, path, "--approve", "0" * 64, success=False)
        result = call("spool-storage-collect", instance, path, "--approve", preview["approval_sha256"])
        assert len(result["completed"]) == len(runs) and result["automatic_retries"] == 0
        call("spool-storage-collect", instance, path, success=False)  # Never skip/replay completed items.
        return path

    for index in range(rounds - 1):
        event, run, args, digest = prepare(index)
        call(*args, "--approve", digest)
        head = commit(event, run, head)
    event, run, args, digest = prepare(rounds - 1)
    invalid = request("invalid-selection", "work", [produced[0], "missing"])
    call("spool-storage-collect", a, invalid, success=False)
    assert (a / "runs" / produced[0] / "job/work/scratch.bin").exists()
    duplicate = request("duplicate-selection", "work", [produced[0], produced[0]])
    call("spool-storage-collect", a, duplicate, success=False)
    # Cancel the maintenance client during real preview validation, before any deletion.
    cancel_request = request("cancel-maintenance", "work", produced)
    cancel_preview = call("spool-storage-collect", a, cancel_request)
    maintenance = spawn("spool-storage-collect", a, cancel_request, "--approve", cancel_preview["approval_sha256"])
    try:
        deadline = time.monotonic() + 15
        while not (a / "host.lock").exists():
            if maintenance.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError("No maintenance validation boundary; no retry")
            time.sleep(.005)
        assert all(not (a / "runs" / name / "job/reclaim-intent.json").exists() for name in produced)
    finally:
        if maintenance.poll() is None:
            maintenance.send_signal(signal.SIGINT)
        stopped = finish(maintenance, False)
    assert stopped["status"] == "cancelled" and stopped["completed"] == []
    assert all((a / "runs" / name / "job/work/scratch.bin").exists() for name in produced)
    process = spawn(*args, "--approve", digest)
    try:
        deadline = time.monotonic() + 15
        while not (b / "runs" / run / "job/work/ready.marker").exists():
            if process.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError("No real downstream readiness; no retry")
            time.sleep(.02)
        before = call("spool-storage", a)
        assert sum(r["snapshot_logical_bytes"] for r in before["runs"]) >= size * rounds
        denied = request("running-denied", "work", [consumed[0], run])
        call("spool-storage-collect", b, denied, success=False)
        assert (b / "runs" / consumed[0] / "job/work").exists()
        assert process.poll() is None  # Start maintenance while downstream is actually running.
        collect(a, "source-work", "work", produced)
        collect(a, "source-inputs", "snapshots", produced)
        after = call("spool-storage", a)
        assert all(r["work_logical_bytes"] == 0 and r["snapshot_logical_bytes"] == 0 for r in after["runs"])
        removed = before["total_logical_bytes"] - after["total_logical_bytes"]
        assert removed >= size * (rounds * 2 - 1), (before, after)
        downstream_finished_during_maintenance = process.poll() is not None
        completed = finish(process)
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
            finish(process, False)
    assert completed["result"]["workspace"]["termination_confirmed"] is True
    head = commit(event, run, head)
    collect(b, "consumer-work", "work", consumed)
    collect(b, "consumer-inputs", "snapshots", consumed)

    # A fresh fifth attempt is cancelled, never published or committed.
    cancel_event, cancel_run, args, digest = prepare(rounds)
    process = spawn(*args, "--approve", digest)
    try:
        deadline = time.monotonic() + 15
        while not (b / "runs" / cancel_run / "job/work/ready.marker").exists():
            if process.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError("No real cancellation readiness; no retry")
            time.sleep(.02)
        denied = request("cancel-running-denied", "work", [cancel_run])
        call("spool-storage-collect", b, denied, success=False)
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
        cancelled = finish(process, False)
    assert cancelled["result"]["workspace"]["termination_confirmed"] is True
    call("spool-accept", b, cancel_run, success=False)
    collect(b, "cancel-work", "work", [cancel_run])
    collect(b, "cancel-inputs", "snapshots", [cancel_run])
    assert call("spool-state-show", b)["head"] == head
    call("spool-event-deliver", ws, cancel_event, "counts", "--state", head, success=False)
    collect(a, "source-tail-work", "work", [produced[-1]])
    # Lose only a known completed collection receipt. Do not infer permission to continue.
    first_attempt = next((a / "runs" / produced[0] / "job/attempts").iterdir())
    receipt = first_attempt / "snapshot-reclaimed.json"
    original = receipt.read_bytes()
    receipt.unlink()
    unknown = request("unknown-selection", "snapshots", [produced[0], produced[-1]])
    call("spool-storage-collect", a, unknown, success=False)
    tail_attempt = next((a / "runs" / produced[-1] / "job/attempts").iterdir())
    assert (tail_attempt / "snapshot/inputs").exists()
    receipt.write_bytes(original)
    collect(a, "source-tail-inputs", "snapshots", [produced[-1]])
    assert hashlib.sha256(large.read_bytes()).hexdigest() == large_hash
    for instance, names in [(a, produced), (b, consumed)]:
        for name in names:
            assert len(list((instance / "runs" / name / "job/attempts").iterdir())) == 1
    for index, revision in enumerate(heads):
        selected = call("spool-state-show", b, revision)["selected"]
        saved = root / f"state-{index}.txt"
        call("artifact-checkout", selected["catalog"], revision, saved)
        assert saved.read_text() == f"{(index + 1) * size}\n"
    for name in produced:
        products = call("spool-artifacts", a, name)
        output = root / (name + ".txt")
        call("artifact-checkout", products["catalog"], products["outputs"][0]["binding"]["revision"], output)
        assert int(output.read_text()) == size
    summary = {"schema": "native-capacity-maintenance-qualification-v1", "passed": True,
        "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(), "rounds": rounds,
        "input_bytes_per_round": size, "producer_before_logical_bytes": before["total_logical_bytes"],
        "producer_after_logical_bytes": after["total_logical_bytes"], "logical_bytes_removed": removed,
        "physical_bytes_saved": None, "downstream_running_at_collection_start": True,
        "downstream_finished_during_maintenance": downstream_finished_during_maintenance,
        "cancel_termination_confirmed": True, "maintenance_cancelled_before_deletion": True, "formal_products_and_checkpoints_retained": True,
        "external_input_preserved": True, "unknown_collection_retried": False,
        "automatic_retries": 0, "model_invoked": False}
    (root / "acceptance.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
