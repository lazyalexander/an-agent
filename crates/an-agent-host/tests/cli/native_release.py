#!/usr/bin/env python3
"""Real release review and independent continuation; no in-place upgrade or retry."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import shutil
import signal
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


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

    def call(*args, success=True):
        process = subprocess.Popen([str(host), *map(str, args)], text=True,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            stdout, stderr = process.communicate(timeout=30)
        except subprocess.TimeoutExpired as error:
            process.send_signal(signal.SIGINT)
            try:
                stdout, stderr = process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                raise RuntimeError(f"Termination unknown for {process.pid}; retain {root}") from error
            raise RuntimeError(f"Command deadline exceeded: {args}; no retry") from error
        calls.append({"args": list(map(str, args)), "code": process.returncode,
                      "stdout": stdout, "stderr": stderr})
        (root / "commands.local.json").write_text(json.dumps(calls, indent=2))
        assert (process.returncode == 0) == success, stderr or stdout
        return json.loads(stdout) if stdout else None

    def approved(*args, success=True):
        plan = call(*args)
        return call(*args, "--approve", plan["approval_sha256"], success=success)

    def package(name, code):
        path = root / name
        (path / "silk").mkdir(parents=True)
        (path / "kit").mkdir()
        (path / "silk/main.sh").write_text(
            'set -eu\n/bin/bash "$AN_SPOOL_KIT_DIR/step.sh" "$AN_SPOOL_INPUT"\n')
        (path / "kit/step.sh").write_text(code)
        manifest = {"schema": "an-native-spool-v2", "name": "counter", "version": "1.0.0",
                    "entry": "silk/main.sh", "files": {
                        name: digest(path / name) for name in ["silk/main.sh", "kit/step.sh"]},
                    "outputs": ["state.txt", "report.txt"], "requirements": {
                        "runtime": "macos-native-process-v1", "os": "macos", "arch": "portable-source",
                        "interaction": "headless", "gpu": "none", "inputs": ["state"],
                        "editable": ["kit/step.sh"],
                        "capabilities": ["read-input", "write-declared-outputs"], "dependencies": []}}
        (path / "native-spool.json").write_text(json.dumps(manifest))
        return path

    def counter(step):
        return ('set -eu\nvalue=$(cat "$1")\n'
                f'printf "%s\\n" "$((value + {step}))" > state.txt\n'
                f'printf "count=%s\\n" "$((value + {step}))" > report.txt\n')

    old = package("old", counter(1))
    candidate = package("candidate", counter(2))
    broken = package("broken", 'printf "partial\\n" > state.txt\nexit 23\n')
    instance = root / "old-instance"
    loaded = call("spool-load", old, instance)
    seed = root / "seed.txt"
    seed.write_text("0\n")
    initial = approved("spool-state-init", instance, "state.txt", seed)["state"]["revision"]
    approved("spool-start-state", instance, "first", initial, "10")
    approved("spool-accept", instance, "first")
    current = approved("spool-state-commit", instance, initial, "first")["revision"]
    checkpoint = call("spool-state-show", instance)["selected"]
    saved = root / "fixed-state.txt"
    call("artifact-checkout", checkpoint["catalog"], current, saved)
    assert saved.read_text() == "1\n"
    state_bytes = Path(checkpoint["path"]).read_bytes()
    old_release = digest(instance / "release/native-spool.json")
    prior_approval = call("spool-start-state", instance, "next", current, "10")["approval_sha256"]
    report = call("spool-release-diff", instance / "release", candidate)
    assert report["same_release"] is False and report["reused_coordinates"] is True
    assert len(report["changes"]["code"]) == 1
    assert report["candidate"]["runtime_supported"] is True
    assert report["execution_granted"] is False and report["successor_authorized"] is False
    assert not (candidate / "state.txt").exists()

    unsupported = root / "unsupported"
    shutil.copytree(candidate, unsupported)
    manifest = unsupported / "native-spool.json"
    value = json.loads(manifest.read_text())
    value["requirements"]["capabilities"].append("network")
    value["requirements"]["dependencies"].append("unresolved-tool")
    manifest.write_text(json.dumps(value))
    rejected = call("spool-release-diff", old, unsupported)
    assert rejected["candidate"]["runtime_supported"] is False
    assert {change["field"] for change in rejected["changes"]["requirements"]} == {"capabilities", "dependencies"}
    call("spool-load", unsupported, root / "unsupported-instance", success=False)
    assert not (root / "unsupported-instance").exists()

    histories = []
    for source, name, succeeds in [(candidate, "new", True), (broken, "broken", False)]:
        target = root / f"{name}-instance"
        new = call("spool-load", source, target)
        assert new["instance"] != loaded["instance"] and new["release"] != loaded["release"]
        assert not (target / "checkpoints").exists()
        call("spool-start-state", target, "foreign", current, "10", success=False)
        revision = approved("spool-state-init", target, "state.txt", saved)["state"]["revision"]
        call("spool-start-state", target, "stale", revision, "10", "--approve", prior_approval, success=False)
        assert not (target / "runs/stale/job").exists()
        result = approved("spool-start-state", target, "only", revision, "10", success=succeeds)
        assert result["workspace"]["termination_confirmed"] is True
        if succeeds:
            approved("spool-accept", target, "only")
            committed = approved("spool-state-commit", target, revision, "only")["revision"]
            assert Path(call("spool-state-show", target)["selected"]["path"]).read_text() == "3\n"
            histories.append(committed)
        else:
            call("spool-accept", target, "only", success=False)
            call("spool-state-commit", target, revision, "only", success=False)
            assert call("spool-state-show", target)["head"] == revision
            assert len(list((target / "runs/only/job/attempts").iterdir())) == 1
        assert call("spool-state-show", instance)["head"] == current
        assert Path(checkpoint["path"]).read_bytes() == state_bytes
        assert digest(instance / "release/native-spool.json") == old_release

    # The old reviewed approval remains valid only on its own untouched instance.
    call("spool-start-state", instance, "next", current, "10", "--approve", prior_approval)
    approved("spool-accept", instance, "next")
    approved("spool-state-commit", instance, current, "next")
    assert Path(call("spool-state-show", instance)["selected"]["path"]).read_text() == "2\n"
    summary = {"schema": "native-release-qualification-v1", "passed": True,
               "platform": platform.platform(), "host_sha256": digest(host),
               "old_release": loaded["release"], "candidate_release": report["candidate"]["release"],
               "new_state_revisions": histories, "execution_retried": False,
               "in_place_upgrade": False, "successor_authorized": False}
    (root / "acceptance.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
