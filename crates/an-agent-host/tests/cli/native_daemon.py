#!/usr/bin/env python3
"""Real local service queue, restart, scoped cancellation and original custody."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time
import urllib.error
import urllib.request
from host_qualification import require_native_workspace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    require_native_workspace()
    host, root = args.host.resolve(strict=True), args.evidence.resolve()
    root.mkdir(mode=0o700)
    service = root / 'service'
    daemon = None
    calls = []

    def call(*argv, success=True):
        result = subprocess.run([str(host), *map(str, argv)], capture_output=True, text=True, timeout=25)
        calls.append({'args': list(map(str, argv)), 'exit': result.returncode, 'stderr': result.stderr})
        assert (result.returncode == 0) == success, (argv, result.stdout, result.stderr)
        return json.loads(result.stdout) if result.stdout else None

    def request(value, success=True):
        path = root / 'request.json'
        path.write_text(json.dumps(value))
        return call('daemon', 'request', service, path, success=success)

    def approved(value):
        preview = request(value)
        return request({**value, 'approve': preview['approval_sha256']})

    def until(read, seconds=25):
        deadline = time.monotonic() + seconds
        while True:
            value = read()
            if value:
                return value
            assert time.monotonic() < deadline, 'observation deadline exceeded'
            time.sleep(.05)

    def start():
        with (root / 'service.log').open('ab') as log:
            child = subprocess.Popen([str(host), 'daemon', 'serve', str(service)], stdout=log, stderr=log)
        until(lambda: (service / 'endpoint.json').exists() and child.poll() is None
              and json.loads((service / 'endpoint.json').read_text())['pid'] == child.pid)
        return child

    def show():
        return call('daemon', 'show', service)

    def task(task_id):
        return next(row for row in show()['tasks'] if row['id'] == task_id)

    package = root / 'release'
    (package / 'silk').mkdir(parents=True)
    code = '''set -eu
read -r mode < "$AN_SPOOL_INPUT"
printf 'started\n' > started.txt
if [ "$mode" = slow ]; then sleep 6; fi
printf 'preserved\n' > report.txt
'''
    (package / 'silk/main.sh').write_text(code)
    (package / 'native-spool.json').write_text(json.dumps({
        'schema': 'an-native-spool-v2', 'name': 'daemon-fixture', 'version': '0.1.0',
        'entry': 'silk/main.sh', 'files': {'silk/main.sh': hashlib.sha256(code.encode()).hexdigest()},
        'outputs': ['report.txt'], 'requirements': {
            'runtime': 'macos-native-process-v1', 'os': 'macos', 'arch': 'portable-source',
            'interaction': 'headless', 'gpu': 'none', 'inputs': ['input.txt'],
            'editable': [], 'dependencies': [], 'capabilities': ['read-input', 'write-declared-outputs']}}))
    slow, quick = root / 'slow.txt', root / 'quick.txt'
    slow.write_text('slow\n')
    quick.write_text('quick\n')
    a, b = root / 'a', root / 'b'
    call('spool-load', package, a)
    call('spool-load', package, b)
    call('daemon', 'init', service, 1)
    call('daemon', 'launchd', service, root / 'agent.plist')
    subprocess.run(['/usr/bin/plutil', '-lint', str(root / 'agent.plist')], check=True, capture_output=True)
    try:
        daemon = start()
        call('daemon', 'serve', service, success=False)
        for instance in [a, b]:
            request({'op': 'register', 'instance': str(instance)})
        first = {'op': 'submit', 'instance': str(a), 'run': 'first', 'input': str(slow), 'seconds': 20}
        preview = request(first)
        request({**first, 'seconds': 19, 'approve': preview['approval_sha256']}, success=False)
        one = request({**first, 'approve': preview['approval_sha256']})['id']
        until(lambda: (a / 'runs/first/job/work/started.txt').exists())
        until(lambda: task(one)['resources']['available'])
        two = approved({'op': 'submit', 'instance': str(b), 'run': 'second', 'input': str(quick), 'seconds': 20})['id']
        assert task(two)['state'] == 'queued'
        request(first, success=False)
        approved({'op': 'cancel', 'id': two})
        assert task(two)['cancellation_requested']
        assert not (b / 'runs/second/job').exists()
        before = len(list((a / 'runs/first/job/attempts').iterdir()))
        daemon.kill()
        daemon.wait(timeout=5)
        daemon = start()
        until(lambda: task(one)['state'] == 'settled')
        until(lambda: task(two)['state'] == 'settled')
        assert task(one)['outcome'] == 'succeeded'
        assert task(two)['outcome'] == 'cancelled'
        assert before == len(list((a / 'runs/first/job/attempts').iterdir())) == 1
        assert not (b / 'runs/second/job').exists()
        assert task(one)['native']['artifact_status'] == 'awaiting-acceptance'
        accepted = approved({'op': 'accept', 'instance': str(a), 'run': 'first'})
        assert accepted['status'] == 'succeeded'
        assert task(one)['native']['artifact_status'] == 'accepted'
        inventory = call('spool-artifacts', a, 'first')
        approved({'op': 'pause', 'instance': str(a)})
        approved({'op': 'resume', 'instance': str(a)})
        endpoint = json.loads((service / 'endpoint.json').read_text())['url']
        origin, token = endpoint.split('/#')
        with urllib.request.urlopen(origin + '/') as response:
            assert '工作台' in response.read().decode()
        try:
            urllib.request.urlopen(origin + '/snapshot')
            raise AssertionError('unauthorized snapshot accepted')
        except urllib.error.HTTPError as error:
            assert error.code == 403
        req = urllib.request.Request(origin + '/snapshot', headers={'Authorization': 'Bearer ' + token})
        with urllib.request.urlopen(req) as response:
            assert len(json.load(response)['instances']) == 2
        # An approved harness executable is a test fixture, not provider evidence.
        fake = root / 'codex-fixture'
        fake.write_text('''#!/bin/bash
set -eu
if [ "$1" = --version ]; then echo 'codex-cli 0.166.0'; exit 0; fi
out=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = --output-last-message ]; then out="$2"; shift; fi
  if [ "$1" = slow-harness ]; then sleep 8; fi
  shift
done
printf '{"finish":"steel"}' > "$out"
printf '{"type":"turn.completed"}\\n'
''')
        fake.chmod(0o700)
        base = {'label': 'finite fixture', 'binary': str(fake), 'binary_sha256': hashlib.sha256(fake.read_bytes()).hexdigest(),
                'prompt': 'complete', 'scope': {'parts': ['grip']}, 'schema': {'type': 'object'}, 'model': None, 'mcp': None}
        job = approved({'op': 'harness', 'task': base})['id']
        until(lambda: task(job)['state'] == 'settled')
        decision = request({'op': 'harness-result', 'id': job})
        assert decision['proposal'] == {'finish': 'steel'}
        assert decision['result']['artifact_accepted'] is False
        job = approved({'op': 'harness', 'task': {**base, 'prompt': 'slow-harness'}})['id']
        until(lambda: (service / 'jobs' / job / 'owner.json').exists())
        approved({'op': 'cancel', 'id': job})
        until(lambda: task(job)['state'] == 'settled')
        assert task(job)['outcome'] == 'cancelled'
        request({'op': 'harness-result', 'id': job}, success=False)
        assert call('spool-artifacts', a, 'first') == inventory
        approved({'op': 'unload', 'instance': str(a)})
        assert not call('spool-show', a)['loaded']
        # Losing the original owner never frees a slot or permits replay/custody.
        c = root / 'c'
        call('spool-load', package, c)
        request({'op': 'register', 'instance': str(c)})
        lost = approved({'op': 'submit', 'instance': str(b), 'run': 'owner-lost', 'input': str(slow), 'seconds': 20})['id']
        until(lambda: (b / 'runs/owner-lost/job/work/started.txt').exists())
        owner = json.loads((b / 'runs/owner-lost/supervisor/owner.json').read_text())
        os.kill(owner['pid'], signal.SIGKILL)
        blocked = approved({'op': 'submit', 'instance': str(c), 'run': 'blocked', 'input': str(quick), 'seconds': 20})['id']
        time.sleep(7)  # The finite fixture expires; no borrowed process is signalled.
        assert task(lost)['state'] == 'running-or-unconfirmed'
        assert task(blocked)['state'] == 'queued'
        assert not (c / 'runs/blocked/job').exists()
        call('spool-accept', b, 'owner-lost', success=False)
        assert not (b / 'runs/owner-lost/supervisor/completion.json').exists()
        assert call('spool-artifacts', a, 'first') == inventory
        (root / 'report.json').write_text(json.dumps({'passed': True,
            'host_sha256': hashlib.sha256(host.read_bytes()).hexdigest(),
            'queue_bound': 1, 'restart_replayed': False, 'scoped_prelaunch_cancel': True,
            'original_artifact_custody': True, 'harness_fixture_only': True,
            'provider_qualified': False, 'read_only_console_authorization': True,
            'resources_observed': True, 'unknown_owner_holds_budget': True, 'automatic_retries': 0}, indent=2))
    finally:
        if daemon and daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            daemon.wait(timeout=10)
        (root / 'commands.local.json').write_text(json.dumps(calls, indent=2))


if __name__ == '__main__':
    main()
