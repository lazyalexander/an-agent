"""Real macOS CLI custody and cancellation; standard library only."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest


class Handoff(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if sys.platform != 'darwin':
            raise RuntimeError('This acceptance requires macOS enforcement')
        cls.host = str(Path(os.environ['AN_AGENT_HOST_BIN']).resolve(strict=True))

    def setUp(self):
        evidence = os.environ.get('AN_AGENT_ACCEPTANCE_DIR')
        if evidence:
            Path(evidence).mkdir(parents=True, exist_ok=True)
        self.base = Path(tempfile.mkdtemp(prefix='cli-handoff-', dir=evidence)).resolve()
        self.events = self.base / 'commands.jsonl'

    def cli(self, *args, timing=False, ok=True):
        env = os.environ.copy()
        env.pop('AN_AGENT_HOST_TIMING', None)
        if timing:
            env['AN_AGENT_HOST_TIMING'] = '1'
        started = time.monotonic()
        result = subprocess.run([self.host, *map(str, args)], env=env,
                                capture_output=True, text=True, timeout=40)
        with self.events.open('a') as stream:
            stream.write(json.dumps(dict(args=list(map(str, args)),
                seconds=time.monotonic()-started, code=result.returncode,
                stdout=result.stdout, stderr=result.stderr)) + '\n')
        self.assertEqual(result.returncode == 0, ok, result.stderr)
        return result

    def approved(self, *args):
        preview = json.loads(self.cli(*args).stdout)
        return json.loads(self.cli(*args, '--approve', preview['approval_sha256']).stdout)

    def fixture(self, base, script, deadline):
        base.mkdir(exist_ok=True)
        selected = base / 'selected'
        selected.mkdir()
        source = selected / 'input.txt'
        source.write_text('immutable')
        profile = base / 'profile.json'
        spec = dict(schema='an-workspace-job-v2', executable='/bin/bash',
                    args=['-c', script, 'fixture', str(base/'forbidden.txt')],
                    readonly=[], inputs=[str(source)], environment={},
                    outputs=['output.txt'], wall_seconds=deadline, trusted_process_group=True)
        profile.write_text(json.dumps(spec))
        return profile, base/'job', spec

    @staticmethod
    def traces(result):
        return [json.loads(line.removeprefix('AN_AGENT_TIMING='))
                for line in result.stderr.splitlines() if line.startswith('AN_AGENT_TIMING=')]

    def test_large_binary_custody_keeps_limits_and_timing_cannot_grant_a_lock(self):
        profile, root, spec = self.fixture(self.base, 'unused', 20)
        source = self.base/'selected/large.bin'
        source.write_bytes(bytes(8*1024*1024))
        spec.update(args=['-c', 'cp "$1" a.bin; cp "$1" b.bin', 'fixture', '{input:0}'],
                    inputs=[str(source)], outputs=['a.bin', 'b.bin'], binary_outputs=['a.bin', 'b.bin'])
        profile.write_text(json.dumps(spec))
        receipt = self.approved('workspace-run', profile, root)
        self.assertEqual(receipt['status'], 'succeeded')
        self.assertTrue(receipt['termination_confirmed'])
        attempt = receipt['attempt']
        args = ('workspace-accept', root, attempt)
        (root/'host.lock').write_text('test-owned competing operation')
        blocked = self.cli(*args, timing=True, ok=False)
        self.assertTrue(any(t['stage']=='lock_acquire' and t['outcome']=='conflict' for t in self.traces(blocked)))
        self.assertFalse((root/'artifacts').exists())
        (root/'host.lock').unlink()
        quiet = self.cli(*args)
        self.assertNotIn('AN_AGENT_TIMING=', quiet.stderr)
        committed = self.cli(*args, '--approve', json.loads(quiet.stdout)['approval_sha256'], timing=True)
        traces = self.traces(committed)
        self.assertTrue(any(t['stage']=='lock_hold' and t['outcome']=='released' for t in traces))
        self.assertTrue(any(t['stage']=='write_sync' and t['bytes']==8*1024*1024 for t in traces))
        accepted = json.loads(committed.stdout)
        self.approved('workspace-reclaim', root, attempt)
        self.assertFalse((root/'work').exists())
        for i, output in enumerate(accepted['outputs']):
            target = self.base/f'retained-{i}'
            self.cli('artifact-checkout', root/'artifacts', output['binding']['revision'], target)
            data = target.read_bytes()
            self.assertEqual(len(data), 8*1024*1024)
            self.assertEqual(data, bytes(len(data)))
        # Remove only known bulky test data after confirmed termination and checks.
        source.unlink()
        for target in self.base.glob('retained-*'):
            target.unlink()
        shutil.rmtree(root/'artifacts')

    def test_cancellation_during_silk_forking_does_not_wait_for_inherited_pipes(self):
        for iteration in range(32):
            with self.subTest(iteration=iteration):
                base = self.base/str(iteration)
                profile, root, _ = self.fixture(base,
                    'printf ready > ready; for n in {1..16}; do /bin/sleep 30 & done; wait', 10)
                start = time.monotonic()
                preview = json.loads(self.cli('workspace-run', profile, root).stdout)
                # File-backed output cannot block a child on a full harness pipe.
                with (base/'stdout').open('w') as out, (base/'stderr').open('w') as err:
                    child = subprocess.Popen([self.host, 'workspace-run', str(profile), str(root),
                        '--approve', preview['approval_sha256']], stdout=out, stderr=err)
                    try:
                        while not (root/'work/ready').exists() and time.monotonic()-start < 5:
                            self.assertIsNone(child.poll(), 'host exited before readiness')
                            time.sleep(.001)
                        self.assertTrue((root/'work/ready').exists())
                        child.send_signal(signal.SIGINT)
                        child.wait(timeout=max(.001, 5-(time.monotonic()-start)))
                    finally:
                        if child.poll() is None:
                            child.send_signal(signal.SIGTERM)
                            try:
                                child.wait(timeout=15)
                            except subprocess.TimeoutExpired:
                                child.kill()
                                child.wait(timeout=5)
                            # No workspace deletion: killing the client proves no group settlement.
                self.assertEqual(child.returncode, 1, (base/'stderr').read_text())
                self.assertIn('execution did not succeed', (base/'stderr').read_text())
                receipt = json.loads((base/'stdout').read_text())
                self.assertLess(time.monotonic()-start, 5, receipt)
                self.assertEqual(receipt['status'], 'cancelled')
                self.assertTrue(receipt['termination_confirmed'])
                self.assertTrue(receipt['group_observation']['empty'])
                attempt = receipt['attempt']
                self.cli('workspace-accept', root, attempt, ok=False)
                self.approved('workspace-reclaim', root, attempt)
                self.assertFalse((root/'work').exists())


if __name__ == '__main__':
    unittest.main()
