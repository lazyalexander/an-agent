#!/usr/bin/env python3
"""Real finite event dispatch: pinned state, drain-stop, cancel and no replay."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import signal
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--legacy-host", type=Path)
    args = parser.parse_args()
    if platform.system() != "Darwin":
        raise SystemExit("Real macOS enforcement required; no fallback")
    host, root = args.host.resolve(strict=True), args.evidence.resolve()
    root.mkdir(mode=0o700)
    legacy = args.legacy_host.resolve(strict=True) if args.legacy_host else None
    calls = []

    def spawn(*args):
        return subprocess.Popen([str(host), *map(str, args)], text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def finish(process, success=True):
        try:
            out, err = process.communicate(timeout=35)
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

    def package(name, state=False, hold=0):
        release = root / (name + "-release")
        (release / "silk").mkdir(parents=True)
        code = 'set -eu\n'
        if state:
            code += f'printf ready > ready.marker\nsleep {hold}\n'
            code += 'awk \'{s += $1} END {print s}\' "$AN_SPOOL_INPUT" "$AN_SPOOL_STATE" > state.txt\n'
            code += 'cat state.txt > report.txt\n'
        else:
            code += 'cat "$AN_SPOOL_INPUT" > report.txt\n'
        (release / "silk/main.sh").write_text(code)
        requirements = {"runtime": "macos-native-process-v1", "os": "macos", "arch": "portable-source",
                        "interaction": "headless", "gpu": "none", "inputs": ["batch.txt"], "editable": [],
                        "capabilities": ["read-input", "write-declared-outputs"], "dependencies": []}
        if state:
            requirements["state_input"] = "checkpoint.txt"
        (release / "native-spool.json").write_text(json.dumps({
            "schema": "an-native-spool-v3", "name": name, "version": "0.1.0", "entry": "silk/main.sh",
            "files": {"silk/main.sh": hashlib.sha256(code.encode()).hexdigest()},
            "outputs": ["state.txt", "report.txt"] if state else ["report.txt"],
            "requirements": requirements,
            "events": {"consumes": ["artifact.accepted"] if state else [], "produces": ["artifact.accepted"]}}))
        return release

    source, ws = root / "source", root / "workspace"
    call("spool-load", package("producer"), source)
    call("spool-events-init", ws)
    seed = root / "seed.txt"
    events = []
    for index, value in enumerate(["2\n", "3\n", "100\n"]):
        seed.write_text(value)
        name = f"source-{index}"
        approved("spool-start", source, name, seed, 15)
        approved("spool-accept", source, name)
        events.append(approved("spool-event-publish", ws, source, name, "report.txt")["event"]["id"])

    def consumer(name, hold=0):
        target = root / name
        call("spool-load", package(name, True, hold), target)
        seed.write_text("10\n")
        state = approved("spool-state-init", target, "state.txt", seed)["state"]["revision"]
        config = root / (name + "-route.json")
        config.write_text(json.dumps({"schema": "an-native-artifact-route-v1", "source": str(source),
            "output": "report.txt", "target": str(target), "result": "report.txt", "wall_seconds": 30}))
        approved("spool-event-subscribe", ws, name, config)
        return target, state

    def request(name, route, revision):
        path = root / (name + ".json")
        value = {"schema": "an-native-event-dispatch-request-v1", "name": name,
                 "entries": [{"event": event, "subscription": route, "state_revision": revision}
                             for event in events[:2]]}
        path.write_text(json.dumps(value))
        return path

    def runs(preview):
        return [entry["plan"]["context"]["delivery"] for entry in preview["plan"]["entries"]]

    def wait_ready(process, target, run):
        deadline = time.monotonic() + 15
        marker = target / "runs" / run / "job/work/ready.marker"
        while not marker.exists():
            if process.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError("No actual readiness; no retry")
            time.sleep(.02)

    normal, initial = consumer("normal")
    req = request("normal-batch", "normal", initial)
    preview = call("spool-events-dispatch", ws, req)
    ids = runs(preview)
    if legacy:
        subprocess.run([str(legacy), "spool-events-show", str(ws)], capture_output=True, check=True)
        old_preview = subprocess.run([str(legacy), "spool-events-dispatch", str(ws), str(req)],
                                     capture_output=True, text=True, check=True)
        assert json.loads(old_preview.stdout)["approval_sha256"] == preview["approval_sha256"]
    pause_preview = call("spool-event-pause", ws, "normal")
    assert json.loads((ws / "native-events.json").read_text())["schema"] == "an-native-event-workspace-v1"
    call("spool-event-pause", ws, "normal", "--approve", "0" * 64, success=False)
    call("spool-event-pause", ws, "normal", "--approve", pause_preview["approval_sha256"])
    assert json.loads((ws / "native-events.json").read_text())["schema"] == "an-native-event-workspace-v2"
    if legacy:
        refused = subprocess.run([str(legacy), "spool-events-show", str(ws)], capture_output=True, text=True)
        assert refused.returncode != 0 and "unsupported native event workspace" in refused.stderr
    row = call("spool-events-pending", ws)["deliveries"][0]
    assert row["phase"] == "blocked" and row["subscription_control"]["mode"] == "paused"
    call("spool-events-dispatch", ws, req, "--approve", preview["approval_sha256"], success=False)
    approved("spool-event-resume", ws, "normal")
    call("spool-event-pause", ws, "normal", "--approve", pause_preview["approval_sha256"], success=False)
    call("spool-events-dispatch", ws, req, "--approve", preview["approval_sha256"], success=False)
    entry = preview["plan"]["entries"][0]
    call("spool-event-deliver", ws, events[0], "normal", "--state", initial,
         "--approve", entry["approval_sha256"], success=False)
    assert all(not (normal / "runs" / run / "job").exists() for run in ids)
    preview = call("spool-events-dispatch", ws, req)
    assert preview["plan"]["entries"][0]["plan"]["subscription_control"]["generation"] == 2
    assert ids == runs(preview)  # Control never creates a second delivery identity.
    assert not (ws / "dispatches").exists()
    assert all(not (normal / "runs" / run / "job").exists() for run in ids)
    call("spool-events-dispatch", ws, req, "--approve", "0" * 64, success=False)
    assert not (ws / "dispatches").exists()
    changed = json.loads(req.read_text())
    changed["entries"][1]["state_revision"] = "0" * 64
    req.write_text(json.dumps(changed))
    call("spool-events-dispatch", ws, req, "--approve", preview["approval_sha256"], success=False)
    req = request("normal-batch", "normal", initial)
    result = call("spool-events-dispatch", ws, req, "--approve", preview["approval_sha256"])
    assert result["reason"] == "completed" and result["attempted_entries"] == 2
    for run, expected in zip(ids, ["12\n", "13\n"]):
        assert (normal / "runs" / run / "job/work/report.txt").read_text() == expected
        approved("spool-accept", normal, run)
    # State was pinned to 10 for both entries, never implicitly threaded from the first.
    current = approved("spool-state-commit", normal, initial, ids[0])["revision"]
    stale = call("spool-state-commit", normal, initial, ids[1], success=False)
    assert stale["status"] == "conflict"
    assert call("spool-state-show", normal)["head"] == current
    repeated = call("spool-events-dispatch", ws, req, "--approve", preview["approval_sha256"])
    assert repeated["executed"] is False and len(repeated["entries"]) == 2
    for run in ids:
        assert len(list((normal / "runs" / run / "job/attempts").iterdir())) == 1
    evidence = ws / "dispatches/normal-batch/entry-0.json"
    receipt_bytes = evidence.read_bytes()
    evidence.unlink()
    assert call("spool-events-dispatch-show", ws, "normal-batch", success=False)["reason"] == "incomplete-entry-evidence"
    call("spool-events-dispatch", ws, req, success=False)
    evidence.write_bytes(receipt_bytes)
    summary_file = ws / "dispatches/normal-batch/result.json"
    original_summary = summary_file.read_bytes()
    summary_file.unlink()
    call("spool-events-dispatch", ws, req, success=False)
    repair = call("spool-events-dispatch-reconcile", ws, "normal-batch")
    assert not summary_file.exists()
    call("spool-events-dispatch-reconcile", ws, "normal-batch", "--approve", "0" * 64, success=False)
    saved_entry = (ws / "dispatches/normal-batch/entry-0.json").read_bytes()
    (ws / "dispatches/normal-batch/entry-0.json").unlink()
    call("spool-events-dispatch-reconcile", ws, "normal-batch", success=False)
    (ws / "dispatches/normal-batch/entry-0.json").write_bytes(saved_entry)
    repaired = call("spool-events-dispatch-reconcile", ws, "normal-batch", "--approve", repair["approval_sha256"])
    assert repaired["executed"] is False and repaired["summary_reconstructed"] is True
    assert json.loads(summary_file.read_bytes()) == json.loads(original_summary)
    assert call("spool-events-dispatch-reconcile", ws, "normal-batch")["already_recorded"] is True
    for run in ids:
        assert len(list((normal / "runs" / run / "job/attempts").iterdir())) == 1
    # A fresh batch name does not bypass event/subscription's single-admission fence.
    duplicate = request("duplicate", "normal", initial)
    call("spool-events-dispatch", ws, duplicate, success=False)

    stopped, state = consumer("stopped", 3)
    req = request("stop-batch", "stopped", state)
    preview = call("spool-events-dispatch", ws, req)
    ids = runs(preview)
    process = spawn("spool-events-dispatch", ws, req, "--approve", preview["approval_sha256"])
    try:
        wait_ready(process, stopped, ids[0])
        process.send_signal(signal.SIGTERM)  # Drain, not cancellation.
        result = finish(process)
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
            finish(process, False)
    call("spool-events-dispatch-reconcile", ws, "stop-batch", success=False)
    assert result["reason"] == "stopped" and result["attempted_entries"] == 1
    assert call("spool-status", stopped, ids[0])["execution_status"] == "succeeded"
    assert not (stopped / "runs" / ids[1] / "job").exists()
    assert call("spool-events-dispatch", ws, req)["executed"] is False
    assert not (stopped / "runs" / ids[1] / "job").exists()

    # A real host ledger write failure after execution cannot admit the next entry.
    faulted, state = consumer("faulted", 3)
    failed_req = request("fault-batch", "faulted", state)
    failed_preview = call("spool-events-dispatch", ws, failed_req)
    failed_ids = runs(failed_preview)
    process = spawn("spool-events-dispatch", ws, failed_req, "--approve", failed_preview["approval_sha256"])
    fault = ws / "dispatches/fault-batch/entry-0.json"
    try:
        wait_ready(process, faulted, failed_ids[0])
        fault.mkdir()
        finish(process, False)
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
            finish(process, False)
    assert not (faulted / "runs" / failed_ids[1] / "job").exists()
    call("spool-events-dispatch", ws, failed_req, success=False)
    assert len(list((faulted / "runs" / failed_ids[0] / "job/attempts").iterdir())) == 1
    fault.rmdir()  # Remove only the injected obstruction; do not invent a receipt.
    assert call("spool-events-dispatch-show", ws, "fault-batch", success=False)["status"] == "uncertain"
    assert not (faulted / "runs" / failed_ids[1] / "job").exists()

    # Pausing an admitted route does not cancel its current entry or block custody.
    gated, state = consumer("gated", 3)
    gated_req = request("gated-batch", "gated", state)
    gated_preview = call("spool-events-dispatch", ws, gated_req)
    gated_ids = runs(gated_preview)
    process = spawn("spool-events-dispatch", ws, gated_req, "--approve", gated_preview["approval_sha256"])
    try:
        wait_ready(process, gated, gated_ids[0])
        approved("spool-event-pause", ws, "gated")
        gated_result = finish(process, False)
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
            finish(process, False)
    assert gated_result["attempted_entries"] == 2  # Second admission is refused, not launched.
    assert call("spool-status", gated, gated_ids[0])["execution_status"] == "succeeded"
    assert not (gated / "runs" / gated_ids[1] / "job").exists()
    approved("spool-accept", gated, gated_ids[0])
    approved("spool-state-commit", gated, state, gated_ids[0])
    approved("spool-event-complete", ws, events[0], "gated")
    approved("spool-event-resume", ws, "gated")
    call("spool-events-dispatch", ws, gated_req, success=False)
    assert not (gated / "runs" / gated_ids[1] / "job").exists()
    # A fresh approval is required for known unadmitted work after resume.
    remaining = json.loads(gated_req.read_text())
    remaining["name"] = "gated-remaining"
    remaining["entries"] = remaining["entries"][1:]
    remaining_path = root / "gated-remaining.json"
    remaining_path.write_text(json.dumps(remaining))
    assert approved("spool-events-dispatch", ws, remaining_path)["reason"] == "completed"
    # Revocation is permanent; reversible controls cannot resurrect the route.
    approved("spool-event-revoke", ws, "gated")
    call("spool-event-pause", ws, "gated", success=False)
    call("spool-event-resume", ws, "gated", success=False)

    invalidated, _ = consumer("invalidated")
    approved("spool-event-pause", ws, "invalidated")
    approved("spool-pause", invalidated)
    approved("spool-resume", invalidated)
    call("spool-event-resume", ws, "invalidated", success=False)
    control = next(row["control"] for row in call("spool-events-show", ws)["subscriptions"]
                   if row["name"] == "invalidated")
    assert control["mode"] == "paused" and control["generation"] == 1

    cancelled, state = consumer("cancelled", 20)
    req = request("cancel-batch", "cancelled", state)
    preview = call("spool-events-dispatch", ws, req)
    ids = runs(preview)
    process = spawn("spool-events-dispatch", ws, req, "--approve", preview["approval_sha256"])
    try:
        wait_ready(process, cancelled, ids[0])
        call("spool-reclaim", cancelled, ids[0], success=False)
        call("spool-reclaim-snapshots", cancelled, ids[0], success=False)
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
        result = finish(process, False)
    assert result["status"] == "cancelled" and result["attempted_entries"] == 1
    observed = call("spool-status", cancelled, ids[0])
    assert observed["termination_confirmed"] is True and observed["execution_status"] == "cancelled"
    assert not (cancelled / "runs" / ids[1] / "job").exists()
    call("spool-accept", cancelled, ids[0], success=False)
    assert call("spool-state-show", cancelled)["head"] == state
    approved("spool-reclaim", cancelled, ids[0])
    approved("spool-reclaim-snapshots", cancelled, ids[0])
    assert not (cancelled / "runs" / ids[0] / "job/work").exists()

    # Simulated report loss after known cancellation, not arbitrary power-loss proof.
    batch_result = ws / "dispatches/cancel-batch/result.json"
    batch_bytes = batch_result.read_bytes()
    batch_result.unlink()
    missing = call("spool-events-dispatch", ws, req, success=False)
    assert missing["status"] == "uncertain" and missing["automatic_retries"] == 0
    assert not (cancelled / "runs" / ids[1] / "job").exists()
    assert len(list((cancelled / "runs" / ids[0] / "job/attempts").iterdir())) == 1
    batch_result.write_bytes(batch_bytes)
    # An incomplete durable intent is also fenced, not treated as an absent batch.
    incomplete = request("incomplete", "cancelled", state)
    (ws / "dispatches/incomplete").mkdir()
    unknown = call("spool-events-dispatch-show", ws, "incomplete", success=False)
    assert unknown["reason"] == "incomplete-intent"
    call("spool-events-dispatch", ws, incomplete, success=False)
    assert not (cancelled / "runs" / ids[1] / "job").exists()
    summary = {"schema": "native-event-dispatch-qualification-v1", "passed": True,
        "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(), "fixed_state": True,
        "stale_commit_refused": True, "stop_drained_one_entry": True,
        "cancel_termination_confirmed": True, "unknown_retried": False, "real_ledger_write_failure_fenced": True,
        "automatic_retries": 0, "model_invoked": False, "subscription_control_qualified": True, "summary_reconstruction_qualified": True,
        "old_approval_refused_after_resume": True, "legacy_host_refusal_checked": legacy is not None}
    (root / "acceptance.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
