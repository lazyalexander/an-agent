#!/usr/bin/env python3
"""Real native spool state acceptance against one explicitly built host binary."""
import argparse
import hashlib
import json
import platform
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest


PARSER = argparse.ArgumentParser(description=__doc__)
PARSER.add_argument("--host", required=True, type=Path)
PARSER.add_argument("--evidence", type=Path)
OPTIONS = None


class NativeState(unittest.TestCase):
    def setUp(self):
        parent = OPTIONS.evidence
        if parent:
            parent.mkdir(parents=True, exist_ok=True)
        self.base = Path(tempfile.mkdtemp(prefix="native-state-", dir=parent)).resolve()
        self.host = OPTIONS.host.resolve(strict=True)
        self.calls = []
        print(f"Retained evidence: {self.base}", flush=True)

    def tearDown(self):
        # Never remove execution evidence, including when termination is unknown.
        (self.base / "calls.json").write_text(json.dumps(self.calls, indent=2))
        (self.base / "environment.json").write_text(json.dumps({
            "platform": platform.platform(), "machine": platform.machine(),
            "host_sha256": hashlib.sha256(self.host.read_bytes()).hexdigest(),
        }, indent=2))

    def finish(self, process, args, success=True):
        try:
            stdout, stderr = process.communicate(timeout=20)
        except subprocess.TimeoutExpired:
            process.send_signal(signal.SIGINT)
            try:
                stdout, stderr = process.communicate(timeout=10)
            except subprocess.TimeoutExpired as error:
                raise AssertionError(
                    f"Host {process.pid} termination unknown; evidence retained; no retry"
                ) from error
            self.fail(f"Command deadline exceeded: {args}; {stderr}")
        self.calls.append({"args": list(map(str, args)), "code": process.returncode,
                           "stdout": stdout, "stderr": stderr})
        self.assertEqual(process.returncode == 0, success, stderr or stdout)
        return json.loads(stdout) if stdout else None

    def call(self, *args, success=True):
        process = subprocess.Popen([str(self.host), *map(str, args)],
                                   text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        return self.finish(process, args, success)

    def approved(self, *args):
        preview = self.call(*args)
        return self.call(*args, "--approve", preview["approval_sha256"])

    def package(self):
        package = self.base / "package"
        (package / "silk").mkdir(parents=True)
        (package / "kit").mkdir()
        files = {
            "silk/main.sh": 'set -eu\n/bin/bash "$AN_SPOOL_KIT_DIR/step.sh" "$AN_SPOOL_INPUT"\n',
            "kit/step.sh": 'set -eu\ntest ! -e internal.tmp\nvalue=$(cat "$1")\n'
                           'printf "%s\\n" "$((value + 1))" > state.txt\n'
                           'printf "count=%s\\n" "$((value + 1))" > report.txt\n'
                           'printf scratch > internal.tmp\n',
        }
        for name, content in files.items():
            (package / name).write_text(content)
        (package / "native-spool.json").write_text(json.dumps({
            "schema": "an-native-spool-v2", "name": "counter", "version": "1.0.0",
            "entry": "silk/main.sh", "files": {
                name: hashlib.sha256(content.encode()).hexdigest()
                for name, content in files.items()
            }, "outputs": ["state.txt", "report.txt"], "requirements": {
                "runtime": "macos-native-process-v1", "os": "macos",
                "arch": "portable-source", "interaction": "headless", "gpu": "none",
                "inputs": ["state"], "editable": ["kit/step.sh"],
                "capabilities": ["read-input", "write-declared-outputs"], "dependencies": [],
            },
        }))
        return package

    def current(self, root):
        return self.call("spool-state-show", root)

    def run_state(self, root, run, revision):
        result = self.approved("spool-start-state", root, run, revision, "10")
        self.assertEqual(result["status"], "succeeded")
        self.assertTrue(result["workspace"]["termination_confirmed"])
        return result["workspace"]["attempt"]

    @unittest.skipUnless(platform.system() == "Darwin", "real macOS native backend required")
    def test_explicit_commit_conflict_recovery_and_fresh_environment(self):
        package = self.package()
        root = self.base / "instance"
        self.call("spool-load", package, root)
        seed = self.base / "seed.txt"
        seed.write_text("0\n")
        self.call("spool-state-init", root, "undeclared.txt", seed, success=False)
        preview = self.call("spool-state-init", root, "state.txt", seed)
        self.assertFalse((root / "checkpoints").exists())
        seed.write_text("9\n")
        self.call("spool-state-init", root, "state.txt", seed, "--approve",
                  preview["approval_sha256"], success=False)
        seed.write_text("0\n")
        first = self.call("spool-state-init", root, "state.txt", seed,
                          "--approve", preview["approval_sha256"])["state"]["revision"]
        self.call("spool-state-init", root, "state.txt", seed, success=False)
        self.assertEqual(self.current(root)["head"], first)
        # Identical bytes through the ordinary input route do not confer state authority.
        self.approved("spool-start", root, "ordinary", seed, "10")
        self.approved("spool-accept", root, "ordinary")
        self.call("spool-state-commit", root, first, "ordinary", success=False)
        content = Path(self.current(root)["selected"]["path"])
        plan = self.call("spool-start-state", root, "a", first, "10")
        content.write_text("999\n")
        self.call("spool-start-state", root, "a", first, "10", "--approve",
                  plan["approval_sha256"], success=False)
        self.assertFalse((root / "runs/a/job").exists())
        content.write_text("0\n")
        attempt_a = self.run_state(root, "a", first)
        attempt_b = self.run_state(root, "b", first)
        self.assertNotEqual(attempt_a, attempt_b)
        self.assertEqual(self.current(root)["head"], first)
        self.call("spool-state-commit", root, first, "a", success=False)
        self.approved("spool-accept", root, "a")
        self.approved("spool-accept", root, "b")
        stale = self.call("spool-state-commit", root, first, "b")
        commit = self.call("spool-state-commit", root, first, "a")
        self.call("spool-state-commit", root, first, "a", "--approve", "0" * 64, success=False)
        # A real filesystem failure after immutable publication but before HEAD rename.
        staging = root / "checkpoints" / f'HEAD-{commit["approval_sha256"]}.pending'
        staging.write_text("conflicting staging bytes")
        self.call("spool-state-commit", root, first, "a", "--approve",
                  commit["approval_sha256"], success=False)
        self.assertEqual(self.current(root)["head"], first)
        orphan = next(p.name for p in (root / "checkpoints/catalog/revisions").iterdir()
                      if p.name != first)
        self.call("spool-state-show", root, orphan, success=False)
        staging.unlink()
        # Mutating scratch after acceptance cannot replace the accepted state bytes.
        (root / "runs/a/job/work/state.txt").write_text("777\n")
        self.approved("spool-reclaim", root, "a")
        second = self.call("spool-state-commit", root, first, "a", "--approve",
                           commit["approval_sha256"])["revision"]
        self.assertEqual(Path(self.current(root)["selected"]["path"]).read_text(), "1\n")
        replay = self.call("spool-state-commit", root, first, "a", "--approve",
                           commit["approval_sha256"])
        self.assertTrue(replay["already_committed"])
        self.assertEqual(replay["revision"], second)
        conflict = self.call("spool-state-commit", root, first, "b", "--approve",
                             stale["approval_sha256"], success=False)
        self.assertEqual(conflict["status"], "conflict")
        self.assertEqual(conflict["automatic_retries"], 0)
        self.approved("spool-reclaim", root, "b")
        self.assertEqual(self.call("spool-state-show", root, first)["selected"]["binding"]["revision"], first)
        self.assertFalse((root / "runs/a/job/work").exists())
        self.run_state(root, "next", second)
        self.approved("spool-accept", root, "next")
        third = self.approved("spool-state-commit", root, second, "next")["revision"]
        self.assertEqual(Path(self.current(root)["selected"]["path"]).read_text(), "2\n")

        # A cancelled run with an apparent candidate must never advance state.
        edit = self.base / "hold.sh"
        edit.write_text('set -eu\nprintf "999\\n" > state.txt\nprintf ready > ready\nsleep 30\n')
        self.approved("spool-kit-update", root, "step.sh", edit)
        cancel_args = ["spool-start-state", root, "cancel", third, "10"]
        cancel_plan = self.call(*cancel_args)
        process = subprocess.Popen([str(self.host), *map(str, cancel_args), "--approve",
                                    cancel_plan["approval_sha256"]], text=True,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        deadline = time.monotonic() + 8
        while not (root / "runs/cancel/job/work/ready").exists():
            if process.poll() is not None or time.monotonic() > deadline:
                process.send_signal(signal.SIGINT) if process.poll() is None else None
                self.finish(process, cancel_args, success=False)
                self.fail("Cancellation witness never became ready")
            time.sleep(0.02)
        self.call("spool-state-commit", root, third, "cancel", success=False)
        process.send_signal(signal.SIGTERM)
        cancelled = self.finish(process, cancel_args, success=False)
        self.assertEqual(cancelled["status"], "cancelled")
        self.assertTrue(cancelled["workspace"]["termination_confirmed"])
        self.call("spool-state-commit", root, third, "cancel", success=False)
        self.call("spool-accept", root, "cancel", success=False)
        self.approved("spool-reclaim", root, "cancel")
        self.assertEqual(self.current(root)["head"], third)

        # New instance, restored kit baseline, explicitly seeded from one fixed checkpoint.
        snapshot = self.current(root)["selected"]
        exported = self.base / "saved-state.txt"
        self.call("artifact-checkout", snapshot["catalog"], third, exported)
        self.approved("spool-unload", root)
        fresh = self.base / "fresh-instance"
        self.call("spool-load", package, fresh)
        self.call("spool-start-state", fresh, "foreign", third, "10", success=False)
        seeded = self.approved("spool-state-init", fresh, "state.txt", exported)["state"]["revision"]
        self.assertNotEqual(seeded, third)
        self.run_state(fresh, "continued", seeded)
        self.approved("spool-accept", fresh, "continued")
        self.approved("spool-state-commit", fresh, seeded, "continued")
        self.assertEqual(Path(self.current(fresh)["selected"]["path"]).read_text(), "3\n")
        self.assertEqual(self.current(root)["head"], third)
        self.assertEqual(len(list((root / "runs/a/job/attempts").iterdir())), 1)


if __name__ == "__main__":
    OPTIONS, remaining = PARSER.parse_known_args()
    unittest.main(argv=[__file__, *remaining])
