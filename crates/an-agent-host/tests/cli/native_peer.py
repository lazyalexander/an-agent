#!/usr/bin/env python3
"""Real split-phase peers: fixed inputs, idle waits, generation fences and no replay."""
import argparse
import hashlib
import json
from pathlib import Path
import signal
import subprocess
import time
from host_qualification import require_native_workspace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    require_native_workspace()
    host, root = args.host.resolve(strict=True), args.evidence.resolve()
    root.mkdir(mode=0o700)
    children, calls = [], []

    def spawn(*argv):
        child = subprocess.Popen([str(host), *map(str, argv)], text=True,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        children.append(child)
        return child

    def finish(child, success=True):
        out, err = child.communicate(timeout=40)
        calls.append({'args': child.args, 'exit': child.returncode, 'stdout': out, 'stderr': err})
        (root / 'commands.local.json').write_text(json.dumps(calls, indent=2))
        assert (child.returncode == 0) == success, (child.args, out, err)
        return json.loads(out) if out else None

    def call(*argv, success=True):
        return finish(spawn(*argv), success)

    def approve(*argv):
        preview = call(*argv)
        return call(*argv, '--approve', preview['approval_sha256'])

    def package(name, receiver=False):
        directory = root / (name + '-release')
        (directory / 'silk').mkdir(parents=True)
        code = 'set -eu\nread -r value < "$AN_SPOOL_INPUT"\nprintf ready > ready.marker\n'
        wait_value = 'hold' if receiver else 'local-hold'
        code += f'if [ "$value" = {wait_value} ]; then sleep 60; fi\n'
        code += "printf 'reply:%s\\n' \"$value\" > report.txt\n" if receiver else 'cat "$AN_SPOOL_INPUT" > report.txt\n'
        (directory / 'silk/main.sh').write_text(code)
        (directory / 'native-spool.json').write_text(json.dumps({
            'schema': 'an-native-spool-v2', 'name': name, 'version': '0.1.0',
            'entry': 'silk/main.sh', 'files': {'silk/main.sh': hashlib.sha256(code.encode()).hexdigest()},
            'outputs': ['report.txt'], 'requirements': {
                'runtime': 'macos-native-process-v1', 'os': 'macos', 'arch': 'portable-source',
                'interaction': 'headless', 'gpu': 'none', 'inputs': ['input.txt'],
                'editable': [], 'dependencies': [], 'capabilities': ['read-input', 'write-declared-outputs']}}))
        return directory

    def ready(instance, run, child):
        marker = instance / 'runs' / run / 'job/work/ready.marker'
        deadline = time.monotonic() + 15
        while not marker.exists():
            assert child.poll() is None and time.monotonic() < deadline, 'no real readiness'
            time.sleep(.01)

    try:
        a, b = root / 'a', root / 'b'
        call('spool-load', package('sender'), a)
        call('spool-load', package('receiver', True), b)
        data = root / 'input.txt'
        data.write_text('2\n')
        approve('spool-start', a, 'produce', data, 15)
        approve('spool-accept', a, 'produce')
        data.write_text('999\n')
        spec = root / 'request.json'
        spec.write_text(json.dumps({'schema': 'an-native-peer-request-v1', 'source_run': 'produce',
            'source_output': 'report.txt', 'target': str(b), 'reply_output': 'report.txt', 'wall_seconds': 20}))

        approve('spool-peer', 'request', a, 'normal', spec)
        call('spool-peer', 'request', a, 'normal', spec, success=False)
        view = call('spool-peer', 'show', a, 'normal')
        assert view['waiting_for_reply'] and not view['waiting_process_created']
        assert not (a / 'host.lock').exists() and not list((b / 'runs').iterdir())
        # A's wait holds no process/lock: real independent work can occupy A while
        # the receiver runs. No scheduler fairness or arbitrary blocking IPC claim.
        hold = root / 'hold.txt'
        hold.write_text('local-hold\n')
        preview = call('spool-start', a, 'independent', hold, 20)
        other = spawn('spool-start', a, 'independent', hold, 20, '--approve', preview['approval_sha256'])
        ready(a, 'independent', other)
        response = approve('spool-peer', 'run', a, 'normal', 'request')
        assert response['status'] == 'succeeded' and other.poll() is None
        other.send_signal(signal.SIGINT)
        cancelled = finish(other, False)
        assert cancelled['workspace']['termination_confirmed'] is True
        approve('spool-accept', b, response['run'])
        approve('spool-peer', 'reply', a, 'normal')
        reply_path = a / 'peers/normal/reply.json'
        original = reply_path.read_bytes()
        changed = json.loads(original)
        changed['plan']['request'] = 'wrong-recipient'
        reply_path.write_text(json.dumps(changed))
        assert call('spool-peer', 'show', a, 'normal')['phase'] == 'reply-unconfirmed'
        call('spool-peer', 'run', a, 'normal', 'response', success=False)
        reply_path.write_bytes(original)
        assert call('spool-peer', 'show', a, 'normal')['phase'] == 'response-ready'
        final = approve('spool-peer', 'run', a, 'normal', 'response')
        approve('spool-accept', a, final['run'])
        assert (a / 'runs' / final['run'] / 'job/work/report.txt').read_text() == 'reply:2\n'
        assert call('spool-input-references', a, final['run'])['disposition'] == 'registered'
        call('spool-peer', 'run', a, 'normal', 'request', success=False)
        call('spool-peer', 'run', a, 'normal', 'response', success=False)

        # Retained replies cannot cross an endpoint generation change.
        approve('spool-peer', 'request', a, 'stale', spec)
        stale = approve('spool-peer', 'run', a, 'stale', 'request')
        approve('spool-accept', b, stale['run'])
        approve('spool-peer', 'reply', a, 'stale')
        old_preview = call('spool-peer', 'run', a, 'stale', 'response')
        approve('spool-pause', a)
        approve('spool-resume', a)
        call('spool-peer', 'run', a, 'stale', 'response', '--approve', old_preview['approval_sha256'], success=False)
        assert not call('spool-peer', 'show', a, 'stale')['endpoints_current']
        assert reply_path.read_bytes() == original

        # Explicit fresh request succeeds after rotation; old requests never rerun.
        approve('spool-peer', 'request', a, 'fresh', spec)
        fresh = approve('spool-peer', 'run', a, 'fresh', 'request')
        approve('spool-accept', b, fresh['run'])
        approve('spool-peer', 'reply', a, 'fresh')
        approve('spool-peer', 'cancel', a, 'fresh')
        call('spool-peer', 'run', a, 'fresh', 'response', success=False)
        assert call('spool-peer', 'show', a, 'fresh')['stages']['request']['settled']
        assert call('spool-artifacts', b, fresh['run'])['outputs']

        # Lost facade completion after real work remains unknown, never replayable.
        approve('spool-peer', 'request', a, 'lost', spec)
        done = approve('spool-peer', 'run', a, 'lost', 'request')
        approve('spool-accept', b, done['run'])
        receipt = a / 'peers/lost/request-result.json'
        saved = receipt.read_bytes()
        receipt.unlink()
        assert call('spool-peer', 'show', a, 'lost')['stages']['request']['outcome'] == 'unknown'
        call('spool-peer', 'reply', a, 'lost', success=False)
        call('spool-peer', 'run', a, 'lost', 'request', success=False)
        intent = a / 'peers/lost/request-start.json'
        original = intent.read_bytes()
        intent.unlink()
        # Both peer receipts are gone, but the native job still fences admission.
        assert call('spool-peer', 'show', a, 'lost')['stages']['request']['outcome'] == 'unknown'
        call('spool-peer', 'run', a, 'lost', 'request', success=False)
        receipt.write_bytes(saved)
        assert call('spool-peer', 'show', a, 'lost')['stages']['request']['outcome'] == 'unknown'
        intent.write_bytes(original)

        # Cancelling before receiver admission creates no process and no new run.
        count = len(list((b / 'runs').iterdir()))
        approve('spool-peer', 'request', a, 'revoked', spec)
        admitted = call('spool-peer', 'run', a, 'revoked', 'request')
        approve('spool-peer', 'cancel', a, 'revoked')
        call('spool-peer', 'run', a, 'revoked', 'request', '--approve', admitted['approval_sha256'], success=False)
        # Previews may stage a run plan; they may not create a job or an attempt.
        staged = b / 'runs' / admitted['plan']['run']
        assert not (staged / 'job').exists()
        assert len(list((b / 'runs').iterdir())) == count + 1

        # Revoking a live request closes delivery, not the independently owned
        # receiver process. Its normal supervisor must confirm actual termination.
        hold.write_text('hold\n')
        approve('spool-start', a, 'produce-hold', hold, 15)
        approve('spool-accept', a, 'produce-hold')
        value = json.loads(spec.read_text())
        value['source_run'] = 'produce-hold'
        spec.write_text(json.dumps(value))
        approve('spool-peer', 'request', a, 'live', spec)
        preview = call('spool-peer', 'run', a, 'live', 'request')
        running = spawn('spool-peer', 'run', a, 'live', 'request', '--approve', preview['approval_sha256'])
        run = preview['plan']['run']
        ready(b, run, running)
        approve('spool-peer', 'cancel', a, 'live')
        assert running.poll() is None
        call('spool-reclaim', b, run, success=False)
        assert not (b / 'runs' / run / 'reclaimed.json').exists()
        running.send_signal(signal.SIGINT)
        stopped = finish(running, False)
        assert stopped['workspace']['termination_confirmed'] is True
        call('spool-peer', 'reply', a, 'live', success=False)
        approve('spool-reclaim', b, run)
        assert call('spool-artifacts', a, 'produce-hold')['outputs']
        (root / 'report.json').write_text(json.dumps({'passed': True,
            'host_sha256': hashlib.sha256(host.read_bytes()).hexdigest(),
            'fixed_reply': 'reply:2', 'generation_fence': True, 'unknown_replayed': False,
            'sender_busy_receiver_progress': True, 'wait_processes': 0,
            'automatic_retries': 0, 'model_calls': 0,
            'scope': 'split-phase artifact protocol; no live IPC or scheduler fairness claim'}, indent=2))
    finally:
        for child in children:
            if child.poll() is None:
                child.send_signal(signal.SIGINT)
                out, err = child.communicate(timeout=15)
                calls.append({'args': child.args, 'exit': child.returncode,
                              'stdout': out, 'stderr': err, 'cleanup': True})
        (root / 'commands.local.json').write_text(json.dumps(calls, indent=2))


if __name__ == '__main__':
    main()
