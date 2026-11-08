#!/usr/bin/env python3
"""Task capabilities on the original daemon, native supervisor and finite harness."""
import argparse
import hashlib
import json
import signal
import subprocess
import time
from pathlib import Path
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
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    denied = []

    def call(*argv, success=True):
        result = subprocess.run([str(host), *map(str, argv)], capture_output=True, text=True, timeout=30)
        assert (result.returncode == 0) == success, (argv, result.stdout, result.stderr)
        return json.loads(result.stdout) if success else result.stderr

    def request(value, credential=None, success=True, task_endpoint=False):
        path = root / 'request.json'
        path.write_text(json.dumps({'credential': credential, 'request': value} if credential else value))
        return call('daemon', 'task-request' if credential or task_endpoint else 'request',
                    service, path, success=success)

    def approve(value, credential=None):
        preview = request(value, credential)
        return request({**value, 'approve': preview['approval_sha256']}, credential)

    def reject(name, value, credential=None, task_endpoint=False):
        error = request(value, credential, success=False, task_endpoint=task_endpoint)
        denied.append({'case': name, 'error': error.strip()})

    def until(read, seconds=30):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            value = read()
            if value:
                return value
            time.sleep(.05)
        raise AssertionError('original task evidence unavailable; no replay')

    def start():
        with (root / 'service.log').open('ab') as log:
            child = subprocess.Popen([str(host), 'daemon', 'serve', str(service)], stdout=log, stderr=log)
        def ready():
            try:
                return json.loads((service / 'endpoint.json').read_text())['pid'] == child.pid
            except FileNotFoundError:
                return False
        until(ready)
        return child

    def wait(task_id, credential=None):
        view = until(lambda: (v if (v := request({'op': 'task', 'id': task_id}, credential))['state'] == 'settled' else None))
        return view['outcome']

    package = root / 'release'
    (package / 'silk').mkdir(parents=True)
    secret = root / 'ungranted.txt'
    secret.write_text('HOST-PRIVATE-SENTINEL')
    code = '''set -eu
read -r mode < "$AN_SPOOL_INPUT"
printf started > started.txt
if [ "$mode" = slow ]; then sleep 20; fi
if [ "$mode" != slow ]; then
  if cat "$mode" > stolen.txt 2> denied.txt; then exit 91; fi
  if printf overwritten > "$mode" 2>/dev/null; then exit 92; fi
fi
printf preserved > report.txt
'''
    (package / 'silk/main.sh').write_text(code)
    (package / 'native-spool.json').write_text(json.dumps({
        'schema': 'an-native-spool-v2', 'name': 'task-permissions', 'version': '0.1.0',
        'entry': 'silk/main.sh', 'files': {'silk/main.sh': hashlib.sha256(code.encode()).hexdigest()},
        'outputs': ['report.txt'], 'requirements': {'runtime': 'macos-native-process-v1',
        'os': 'macos', 'arch': 'portable-source', 'interaction': 'headless', 'gpu': 'none',
        'inputs': ['input'], 'editable': [], 'dependencies': [],
        'capabilities': ['read-input', 'write-declared-outputs']}}))
    source, slow = root / 'input.txt', root / 'slow.txt'
    source.write_text(str(secret) + '\n')
    slow.write_text('slow\n')
    instance = root / 'instance'
    call('spool-load', package, instance)
    fake = root / 'codex-fixture'
    fake.write_text('''#!/bin/bash
set -eu
if [ "$1" = --version ]; then echo 'codex-cli 0.166.0'; exit 0; fi
printf started > started.txt
out=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = --output-last-message ]; then out="$2"; shift; fi
  case "$1" in *slow-harness*) sleep 20;; esac
  shift
done
printf '{"ok":true}' > "$out"
printf '{"type":"turn.completed"}\\n'
''')
    fake.chmod(0o700)
    task = {'label': 'permission fixture', 'binary': str(fake), 'binary_sha256': sha(fake),
            'prompt': 'complete', 'scope': {'document': 'selected'}, 'schema': {'type': 'object'},
            'model': None, 'mcp': None}
    native = {'op': 'submit', 'instance': str(instance), 'run': 'publish', 'input': str(source), 'seconds': 30}
    call('daemon', 'init', service, 2)
    try:
        daemon = start()
        request({'op': 'register', 'instance': str(instance)})
        harness = request({'op': 'harness-permission', 'task': task})
        run_permission = {'kind': 'run', **{k: v for k, v in native.items() if k != 'op'}}
        artifact = {'kind': 'artifact', 'instance': str(instance), 'run': 'publish', 'output': 'report.txt'}
        accept = {'kind': 'accept', 'instance': str(instance), 'run': 'publish'}
        read = {'kind': 'read-input', 'path': str(source), 'sha256': sha(source)}
        scope = {'task': 'document-task', 'subject': 'shuttle', 'expires_ms': int(time.time()*1000)+120_000,
                 'delegate': True, 'permissions': [harness, run_permission, artifact, accept, read]}
        parent = approve({'op': 'grant', 'scope': scope})['credential']
        child_scope = {**scope, 'subject': 'sub-agent', 'delegate': False,
                       'permissions': [harness, {**run_permission, 'seconds': 20}]}
        child = approve({'op': 'derive-grant', 'scope': child_scope}, parent)['credential']
        reject('missing credential', {'op': 'show'}, task_endpoint=True)
        reject('forged credential', {'op': 'grant-status'}, {**child, 'secret': '0'*64})
        reject('operator endpoint operation', {'op': 'grant', 'scope': scope}, child)
        reject('delegate after removing delegation permission', {'op': 'derive-grant', 'scope': child_scope}, child)
        reject('widen child deadline', {'op': 'derive-grant', 'scope': {**child_scope, 'expires_ms': scope['expires_ms']+1}}, parent)
        reject('new resource in child', {'op': 'derive-grant', 'scope': {**child_scope,
               'permissions': [*child_scope['permissions'], {'kind': 'read-input', 'path': str(secret), 'sha256': sha(secret)}]}}, parent)
        reject('cross task delegation', {'op': 'delegate', 'parent': 'another-task', 'task': task}, child)
        reject('harness scope widening', {'op': 'delegate', 'parent': scope['task'],
               'task': {**task, 'scope': {'document': 'another'}}}, child)
        reject('read outside grant', {'op': 'read-input', 'path': str(secret), 'sha256': sha(secret)}, parent)
        reject('removed read permission', {'op': 'read-input', 'path': str(source), 'sha256': sha(source)}, child)
        assert bytes(request({'op': 'read-input', 'path': str(source), 'sha256': sha(source)}, parent)['bytes']) == source.read_bytes()
        original = source.read_bytes()
        source.write_text('changed')
        reject('changed input bytes', {'op': 'read-input', 'path': str(source), 'sha256': hashlib.sha256(original).hexdigest()}, parent)
        source.write_bytes(original)
        reject('widen native deadline', native, child)
        reject('substitute input path', {**native, 'seconds': 20, 'input': str(secret)}, child)
        submitted = {**native, 'seconds': 20}
        preview = request(submitted, child)
        reject('approval transplanted across grants', {**submitted, 'approve': preview['approval_sha256']}, parent)
        published = approve(submitted, child)['id']
        assert wait(published, child) == 'succeeded'
        assert secret.read_text() == 'HOST-PRIVATE-SENTINEL'
        reject('unaccepted artifact', {'op': 'artifact-read', **{k: v for k, v in artifact.items() if k != 'kind'}}, parent)
        reject('accept removed from child', {'op': 'accept', 'instance': str(instance), 'run': 'publish'}, child)
        approve({'op': 'accept', 'instance': str(instance), 'run': 'publish'}, parent)
        received = request({'op': 'artifact-read', **{k: v for k, v in artifact.items() if k != 'kind'}}, parent)
        assert bytes(received['bytes']) == b'preserved'
        custody = call('spool-artifacts', instance, 'publish')
        foreign = approve({'op': 'harness', 'task': task})['id']
        reject('unowned task', {'op': 'task', 'id': foreign}, child)
        assert wait(foreign) == 'succeeded'
        running = approve({'op': 'delegate', 'parent': scope['task'], 'task': {**task, 'prompt': 'slow-harness'}}, child)['id']
        until(lambda: (service / 'jobs' / running / 'work/started.txt').exists())
        approve({'op': 'cancel', 'id': running}, parent)
        assert wait(running, child) == 'cancelled'
        assert request({'op': 'grant-status'}, child)['active'] is False
        reject('cancelled grant cannot resubmit', {'op': 'delegate', 'parent': scope['task'], 'task': task}, child)
        reject('cancelled grant cannot return proposal', {'op': 'harness-result', 'id': running}, child)
        reject('operator follow-up retains revoked child authority', {'op': 'update-delegation',
               'id': running, 'strategy': 'enqueue', 'instruction': 'continue'})
        sibling = approve({'op': 'derive-grant', 'scope': child_scope}, parent)['credential']
        survivor = approve({'op': 'delegate', 'parent': scope['task'], 'task': task}, sibling)['id']
        assert wait(survivor, sibling) == 'succeeded'
        follow = approve({'op': 'update-delegation', 'id': survivor, 'strategy': 'enqueue',
                          'instruction': 'keep the same result'}, parent)['id']
        assert wait(follow, sibling) == 'succeeded'
        assert request({'op': 'task', 'id': follow}, parent)['authorization']['grant']['id'] == sibling['id']
        reject('sibling task visibility', {'op': 'task', 'id': survivor}, child)
        approve({'op': 'revoke-grant', 'id': parent['id']}, parent)
        reject('parent revocation reaches child', {'op': 'harness-result', 'id': survivor}, sibling)
        assert call('spool-artifacts', instance, 'publish') == custody
        daemon.terminate()
        daemon.wait(timeout=10)
        daemon = start()
        reject('revocation persists after restart', {'op': 'delegate', 'parent': scope['task'], 'task': task}, sibling)
        assert request({'op': 'task', 'id': survivor}, sibling)['outcome'] == 'succeeded'
        # Original native and harness owners observe expiry even while the daemon is down.
        expiring_run = {**native, 'run': 'expires', 'input': str(slow)}
        expiring_scope = {**scope, 'task': 'expiring', 'expires_ms': int(time.time()*1000)+7_000,
                          'permissions': [harness, {'kind': 'run', **{k: v for k, v in expiring_run.items() if k != 'op'}}]}
        expiring = approve({'op': 'grant', 'scope': expiring_scope})['credential']
        local = approve(expiring_run, expiring)['id']
        model = approve({'op': 'delegate', 'parent': 'expiring', 'task': {**task, 'prompt': 'slow-harness'}}, expiring)['id']
        until(lambda: (instance / 'runs/expires/job/work/started.txt').exists())
        until(lambda: (service / 'jobs' / model / 'work/started.txt').exists())
        daemon.terminate()
        daemon.wait(timeout=10)
        until(lambda: (instance / 'runs/expires/supervisor/completion.json').exists(), seconds=12)
        until(lambda: (service / 'jobs' / model / 'completion.json').exists(), seconds=12)
        daemon = start()
        assert wait(local, expiring) == 'cancelled'
        assert wait(model, expiring) == 'cancelled'
        call('spool-accept', instance, 'expires', success=False)
        assert call('spool-artifacts', instance, 'publish') == custody
        (root / 'report.json').write_text(json.dumps({'passed': True, 'host_sha256': sha(host),
            'rejections': denied, 'runtime_outside_read_write_denied': True,
            'cancel_revokes_child_grant': True, 'sibling_unaffected': True,
            'parent_revocation_persistent': True, 'daemon_loss_expiry_cancels_original_owners': True,
            'accepted_custody_preserved': True, 'harness_fixture_only': True}, indent=2))
    finally:
        if daemon and daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            daemon.wait(timeout=10)


if __name__ == '__main__':
    main()
