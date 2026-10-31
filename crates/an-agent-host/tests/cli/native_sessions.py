#!/usr/bin/env python3
"""Real session ownership, pinned state, concurrent cancellation and no replay."""
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
    calls, children = [], []

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

    def package(name, consumer=False):
        folder = root / (name + '-release')
        (folder / 'silk').mkdir(parents=True)
        code = 'set -eu\n'
        if consumer:
            code += 'printf ready > ready.marker\nsleep 2\n'
            code += '''awk '{s += $1} END {print s}' "$AN_SPOOL_INPUT" "$AN_SPOOL_STATE" > report.txt\n'''
        else:
            code += 'cat "$AN_SPOOL_INPUT" > report.txt\n'
        (folder / 'silk/main.sh').write_text(code)
        requirements = {'runtime': 'macos-native-process-v1', 'os': 'macos',
                        'arch': 'portable-source', 'interaction': 'headless', 'gpu': 'none',
                        'inputs': ['input.txt'], 'editable': [],
                        'capabilities': ['read-input', 'write-declared-outputs'], 'dependencies': []}
        if consumer:
            requirements['state_input'] = 'checkpoint.txt'
        (folder / 'native-spool.json').write_text(json.dumps({
            'schema': 'an-native-spool-v3', 'name': name, 'version': '0.1.0',
            'entry': 'silk/main.sh', 'files': {'silk/main.sh': hashlib.sha256(code.encode()).hexdigest()},
            'outputs': ['report.txt'], 'requirements': requirements,
            'events': {'consumes': ['artifact.accepted'] if consumer else [],
                       'produces': ['artifact.accepted']}}))
        return folder

    try:
        ws, producer, seed = root / 'workspace', root / 'producer', root / 'seed.txt'
        call('spool-events-init', ws)
        call('spool-load', package('producer'), producer)
        events = []
        for i in range(2):
            seed.write_text(f'{i + 2}\n')
            approve('spool-start', producer, f'run-{i}', seed, 15)
            approve('spool-accept', producer, f'run-{i}')
            events.append(approve('spool-event-publish', ws, producer, f'run-{i}', 'report.txt')['event']['id'])
        seed.write_text('999\n')  # Published input is pinned, never this mutable path.
        requests, targets = {}, {}
        for name in ['alpha', 'beta', 'gamma', 'delta']:
            target = root / name
            targets[name] = target
            call('spool-load', package(name, True), target)
            state_file = root / (name + '-state.txt')
            state_file.write_text('10\n')
            revision = approve('spool-state-init', target, 'report.txt', state_file)['state']['revision']
            route = root / (name + '-route.json')
            route.write_text(json.dumps({'schema': 'an-native-artifact-route-v1',
                'source': str(producer), 'output': 'report.txt', 'target': str(target),
                'result': 'report.txt', 'wall_seconds': 15}))
            approve('spool-event-subscribe', ws, name, route)
            request = root / (name + '-request.json')
            request.write_text(json.dumps({'schema': 'an-native-event-dispatch-request-v1', 'name': name,
                'entries': [{'event': e, 'subscription': name, 'state_revision': revision} for e in events]}))
            requests[name] = request
            call('session-create', ws, name)
        a = call('session-run', ws, 'alpha', 'one', requests['alpha'])
        b = call('session-run', ws, 'beta', 'one', requests['beta'])
        changed = requests['beta'].read_bytes()
        value = json.loads(changed)
        value['entries'] = value['entries'][:1]
        requests['beta'].write_text(json.dumps(value))
        call('session-run', ws, 'beta', 'one', requests['beta'], '--approve', b['approval_sha256'], success=False)
        requests['beta'].write_bytes(changed)
        stale = call('session-control', ws, 'alpha', 'paused')
        approve('session-control', ws, 'alpha', 'paused')
        call('session-run', ws, 'alpha', 'one', requests['alpha'], '--approve', a['approval_sha256'], success=False)
        approve('session-control', ws, 'alpha', 'open')
        call('session-control', ws, 'alpha', 'paused', '--approve', stale['approval_sha256'], success=False)
        a = call('session-run', ws, 'alpha', 'one', requests['alpha'])
        # Existing workspace admission is intentionally fail-fast, not retried.
        lock = ws / 'host.lock'
        with lock.open('x') as stream:
            stream.write('qualification: another metadata mutation owns admission\n')
        try:
            call('session-run', ws, 'alpha', 'one', requests['alpha'], '--approve', a['approval_sha256'], success=False)
            assert not (ws / 'sessions/alpha/threads/one').exists()
        finally:
            lock.unlink()
        running = {}
        for name, preview in [('alpha', a), ('beta', b)]:
            running[name] = spawn('session-run', ws, name, 'one', requests[name], '--approve', preview['approval_sha256'])
            run = preview['plan']['dispatch']['plan']['entries'][0]['plan']['context']['delivery']
            marker = targets[name] / 'runs' / run / 'job/work/ready.marker'
            deadline = time.monotonic() + 15
            while not marker.exists():
                if running[name].poll() is not None:
                    finish(running[name])
                    raise AssertionError('exited before readiness')
                assert time.monotonic() < deadline, 'no actual readiness'
                time.sleep(.02)
        started = time.monotonic()
        approve('session-control', ws, 'alpha', 'cancelled')
        cancelled = finish(running['alpha'], success=False)
        latency = time.monotonic() - started
        assert cancelled['result']['status'] == 'cancelled', cancelled
        assert finish(running['beta'])['result']['status'] == 'succeeded'
        for name in ['alpha', 'beta']:
            view = call('session-show', ws, name)
            assert view['threads'][0]['settled'], view
            call('session-run', ws, name, 'one', requests[name], success=False)
            if name == 'beta':
                second = approve('session-run', ws, name, 'two', requests['delta'])
                assert second['result']['status'] == 'succeeded'
                assert len(call('session-show', ws, name)['threads']) == 2
            approve('session-control', ws, name, 'closed')
        run = b['plan']['dispatch']['plan']['entries'][0]['plan']['context']['delivery']
        approve('spool-accept', targets['beta'], run)
        outputs = call('spool-artifacts', targets['beta'], run)
        assert outputs  # Session closure did not destroy or claim the products.
        assert (targets['beta'] / 'runs' / run / 'job/work/report.txt').read_text().strip() == '12'
        # A paused thread drains its current entry and admits no next entry.
        g = call('session-run', ws, 'gamma', 'drain', requests['gamma'])
        child = spawn('session-run', ws, 'gamma', 'drain', requests['gamma'], '--approve', g['approval_sha256'])
        run = g['plan']['dispatch']['plan']['entries'][0]['plan']['context']['delivery']
        marker = targets['gamma'] / 'runs' / run / 'job/work/ready.marker'
        deadline = time.monotonic() + 15
        while not marker.exists():
            assert child.poll() is None and time.monotonic() < deadline
            time.sleep(.02)
        approve('session-control', ws, 'gamma', 'paused')
        result = finish(child)['result']
        assert result['attempted_entries'] == 1 and result['reason'] == 'stopped', result
        # Lost receipt is deliberately unknown even after actual successful work.
        receipt = ws / 'sessions/gamma/threads/drain/result.json'
        saved = receipt.read_bytes()
        receipt.unlink()
        assert not call('session-show', ws, 'gamma')['threads'][0]['settled']
        call('session-control', ws, 'gamma', 'closed', success=False)
        call('session-run', ws, 'gamma', 'drain', requests['gamma'], success=False)
        approve('session-control', ws, 'gamma', 'open')
        call('session-run', ws, 'gamma', 'replacement', requests['gamma'], success=False)
        receipt.write_bytes(saved)
        approve('session-control', ws, 'gamma', 'closed')
        (root / 'report.json').write_text(json.dumps({'passed': True, 'cancel_seconds': latency,
            'model_calls': 0, 'automatic_retries': 0, 'unknown_receipt': 'injected loss after real execution',
            'sessions': 4, 'admission': 'explicit sequential admission; overlapping execution; simultaneous admission may refuse', 'checks': ['independence', 'cancel', 'drain', 'stale-generation',
                'pinned-input-state', 'closed-product-retention', 'unknown-no-replay']}, indent=2))
    finally:
        for child in children:
            if child.poll() is None:
                child.send_signal(signal.SIGINT)
                out, err = child.communicate(timeout=15)
                calls.append({'args': child.args, 'exit': child.returncode, 'stdout': out, 'stderr': err, 'cleanup': True})
        (root / 'commands.local.json').write_text(json.dumps(calls, indent=2))


if __name__ == '__main__':
    main()
