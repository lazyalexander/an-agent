#!/usr/bin/env python3
"""Real macOS artifact-event delivery, cancellation and no-retry qualification."""
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
    options = parser.parse_args()
    if platform.system() != "Darwin":
        raise SystemExit("Real macOS native enforcement required; no fallback")
    root = options.evidence.resolve()
    root.mkdir(mode=0o700)
    host = options.host.resolve(strict=True)
    calls = []

    def finish(process, args, success=True):
        try:
            stdout, stderr = process.communicate(timeout=35)
        except subprocess.TimeoutExpired as error:
            process.send_signal(signal.SIGINT)
            try:
                stdout, stderr = process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                raise RuntimeError(f"Termination unknown for {process.pid}; retain {root}") from error
            raise RuntimeError(f"Deadline exceeded: {args}; no retry") from error
        calls.append({"args": list(map(str, args)), "code": process.returncode,
                      "stdout": stdout, "stderr": stderr})
        (root / "commands.local.json").write_text(json.dumps(calls, indent=2))
        assert (process.returncode == 0) == success, stderr or stdout
        return json.loads(stdout) if stdout else None

    def call(*args, success=True):
        process = subprocess.Popen([str(host), *map(str, args)], text=True,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        return finish(process, args, success)

    def approved(*args, success=True):
        preview = call(*args)
        return call(*args, "--approve", preview["approval_sha256"], success=success)

    def package(name, transform):
        target = root / name
        (target / "silk").mkdir(parents=True)
        (target / "kit").mkdir()
        files = {"silk/main.sh": 'set -eu\n/bin/bash "$AN_SPOOL_KIT_DIR/transform.sh" "$AN_SPOOL_INPUT" > intermediate.txt\n'
                                     '/bin/bash "$AN_SPOOL_SILK_DIR/report.sh" intermediate.txt > report.txt\n',
                 "silk/report.sh": 'set -eu\ncat "$1"\n', "kit/transform.sh": transform}
        for path, content in files.items():
            (target / path).write_text(content)
        manifest = {"schema": "an-native-spool-v3", "name": name, "version": "1.0.0",
                    "entry": "silk/main.sh", "files": {
                        path: hashlib.sha256(content.encode()).hexdigest() for path, content in files.items()},
                    "outputs": ["report.txt"], "requirements": {
                        "runtime": "macos-native-process-v1", "os": "macos", "arch": "portable-source",
                        "interaction": "headless", "gpu": "none", "inputs": ["input"],
                        "editable": ["kit/transform.sh"],
                        "capabilities": ["read-input", "write-declared-outputs"], "dependencies": []},
                    "events": {"consumes": ["artifact.accepted"], "produces": ["artifact.accepted"]}}
        (target / "native-spool.json").write_text(json.dumps(manifest))
        return target

    def route(space, name, source, target):
        config = root / f"route-{name}.json"
        config.write_text(json.dumps({"schema": "an-native-artifact-route-v1", "source": str(source),
                                     "output": "report.txt", "target": str(target),
                                     "result": "report.txt", "wall_seconds": 30}))
        return approved("spool-event-subscribe", space, name, config)

    source_package = package("producer", 'set -eu\ntr "[:lower:]" "[:upper:]" < "$1"\n')
    target_package = package("consumer", 'set -eu\nprintf "B:"\ncat "$1"\n')
    a, b = root / "a", root / "b"
    call("spool-load", source_package, a)
    call("spool-load", target_package, b)
    space = root / "workspace"
    call("spool-events-init", space)
    route(space, "main", a, b)
    seed = root / "seed.txt"
    seed.write_text("first\n")
    approved("spool-start", a, "first", seed, "15")
    call("spool-event-publish", space, a, "first", "report.txt", success=False)
    approved("spool-accept", a, "first")
    event = approved("spool-event-publish", space, a, "first", "report.txt")["event"]
    event_id = event["id"]
    assert call("spool-event-publish", space, a, "first", "report.txt")["already_published"] is True
    call("spool-event-deliver", space, event_id, "absent", success=False)
    preview = call("spool-event-deliver", space, event_id, "main")
    run = preview["plan"]["context"]["delivery"]
    assert not (b / "runs" / run / "job").exists()
    assert not (space / "deliveries" / run).exists()
    call("spool-event-deliver", space, event_id, "main", "--approve", "0" * 64, success=False)
    assert not (space / "deliveries" / run).exists()
    seed.write_text("later\n")
    approved("spool-start", a, "second", seed, "15")
    approved("spool-accept", a, "second")
    second = approved("spool-event-publish", space, a, "second", "report.txt")["event"]["id"]
    result = call("spool-event-deliver", space, event_id, "main", "--approve", preview["approval_sha256"])
    assert result["result"]["workspace"]["termination_confirmed"] is True
    assert result["consumed"] is False
    assert (b / "runs" / run / "job/work/report.txt").read_text() == "B:FIRST\n"
    attempted = b / "runs" / run / "job/attempts"
    assert len(list(attempted.iterdir())) == 1
    uncertain = call("spool-event-deliver", space, event_id, "main", success=False)
    assert uncertain["executed"] is False and uncertain["automatic_retries"] == 0
    call("spool-event-complete", space, event_id, "main", success=False)
    result_file = b / "runs" / run / "result.json"
    result_bytes = result_file.read_bytes()
    result_file.unlink()
    unknown = call("spool-event-deliver", space, event_id, "main", "--approve", preview["approval_sha256"], success=False)
    assert unknown["status"] == "uncertain" and unknown["automatic_retries"] == 0
    assert len(list(attempted.iterdir())) == 1
    result_file.write_bytes(result_bytes)
    approved("spool-accept", b, run)
    approved("spool-pause", b)  # Admission pause must not block settling old consumption.
    completed = approved("spool-event-complete", space, event_id, "main")
    approved("spool-resume", b)
    assert completed["receipt"]["result_event"]["event"]["cause"]["event"] == event_id
    assert call("spool-event-deliver", space, event_id, "main")["disposition"] == "already-consumed"
    records = call("spool-events-show", space)["records"]
    ack = space / "deliveries" / run / "ack.json"
    ack.unlink()  # Lost confirmation after complete consumption, not a process retry.
    call("spool-event-deliver", space, event_id, "main", success=False)
    approved("spool-event-complete", space, event_id, "main")
    assert call("spool-events-show", space)["records"] == records
    assert len(list(attempted.iterdir())) == 1
    for instance, name in [(a, "first"), (b, run)]:
        approved("spool-reclaim", instance, name)
        approved("spool-reclaim-snapshots", instance, name)
    assert call("spool-event-deliver", space, event_id, "main")["executed"] is False
    route(space, "generation", a, b)
    stale = call("spool-event-deliver", space, second, "generation")
    approved("spool-pause", b)
    approved("spool-resume", b)
    call("spool-event-deliver", space, second, "generation", "--approve", stale["approval_sha256"], success=False)
    assert not (space / "deliveries" / stale["plan"]["context"]["delivery"]).exists()
    approved("spool-event-revoke", space, "main")
    call("spool-event-deliver", space, second, "main", success=False)
    assert call("spool-event-deliver", space, event_id, "main")["executed"] is False
    foreign = root / "foreign-workspace"
    call("spool-events-init", foreign)
    call("spool-event-deliver", foreign, event_id, "main", success=False)

    # Actual cancellation holds the instance lock, but not the workspace lock.
    holding_package = package("holding", 'set -eu\nprintf ready > ready.marker\nsleep 25\ncat "$1"\n')
    holding = root / "holding-instance"
    call("spool-load", holding_package, holding)
    route(space, "closed", a, holding)
    denied = call("spool-event-deliver", space, second, "closed")
    approved("spool-event-revoke", space, "closed")
    call("spool-event-deliver", space, second, "closed", "--approve", denied["approval_sha256"], success=False)
    assert not (space / "deliveries" / denied["plan"]["context"]["delivery"]).exists()
    route(space, "hold", a, holding)
    plan = call("spool-event-deliver", space, second, "hold")
    hold_run = plan["plan"]["context"]["delivery"]
    args = ["spool-event-deliver", space, second, "hold", "--approve", plan["approval_sha256"]]
    process = subprocess.Popen([str(host), *map(str, args)], text=True,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    deadline = time.monotonic() + 20
    marker = holding / "runs" / hold_run / "job/work/ready.marker"
    try:
        while not marker.exists():
            if process.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError("No real cancellation readiness; no retry")
            time.sleep(0.02)
        call("spool-events-show", space)
        during = call("spool-event-deliver", space, second, "hold", success=False)
        assert during["executed"] is False
        approved("spool-event-revoke", space, "hold")
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
        cancelled = finish(process, args, success=False)
    assert cancelled["status"] == "cancelled"
    assert cancelled["result"]["workspace"]["termination_confirmed"] is True
    call("spool-accept", holding, hold_run, success=False)
    call("spool-event-complete", space, second, "hold", success=False)
    assert not (space / "deliveries" / hold_run / "ack.json").exists()
    assert len(list((holding / "runs" / hold_run / "job/attempts").iterdir())) == 1

    # Confirmed execution with lost facade evidence stays fenced, never reexecuted.
    result_file = holding / "runs" / hold_run / "result.json"
    original = result_file.read_bytes()
    result_file.unlink()
    unknown = call("spool-event-deliver", space, second, "hold", success=False)
    assert unknown["status"] == "uncertain" and unknown["automatic_retries"] == 0
    assert len(list((holding / "runs" / hold_run / "job/attempts").iterdir())) == 1
    result_file.write_bytes(original)
    # Reload at the same location is a distinct instance, not a subscription successor.
    b.rename(root / "retained-b")
    call("spool-load", target_package, b)
    call("spool-event-deliver", space, second, "generation", success=False)
    assert list((b / "runs").iterdir()) == []
    summary = {"schema": "native-event-qualification-v1", "passed": True,
               "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(),
               "platform": platform.platform(), "event": event_id, "second_event": second,
               "attempts_for_first_delivery": 1, "attempts_for_cancelled_delivery": 1,
               "lost_ack_repaired_without_execution": True, "execution_retried": False,
               "model_invoked": False, "background_dispatch": False}
    (root / "acceptance.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
