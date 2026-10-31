#!/usr/bin/env python3
"""Explicit old-state selection survives detached client loss and HEAD advancement."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time
from host_qualification import require_native_workspace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    parser.add_argument('--combined', action='store_true', help='Also bind an accepted business artifact')
    args = parser.parse_args()
    require_native_workspace()
    host, root = args.host.resolve(strict=True), args.evidence.resolve()
    root.mkdir(mode=0o700)
    calls, children, cases = [], [], {}

    def spawn(*argv):
        child = subprocess.Popen([str(host), *map(str, argv)], text=True,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        children.append(child)
        return child

    def finish(child, success=True):
        out, err = child.communicate(timeout=25)
        calls.append({'args': child.args, 'exit': child.returncode, 'stdout': out, 'stderr': err})
        (root / 'commands.local.json').write_text(json.dumps(calls, indent=2))
        assert (child.returncode == 0) == success, calls[-1]
        return json.loads(out) if out else None

    def call(*argv, success=True):
        return finish(spawn(*argv), success)

    def approved(*argv):
        preview = call(*argv)
        return call(*argv, '--approve', preview['approval_sha256'])

    def until(predicate, seconds=20):
        deadline = time.monotonic() + seconds
        while not predicate():
            assert time.monotonic() < deadline, 'observation deadline exceeded'
            time.sleep(.025)

    def group_members(group):
        ps = subprocess.run(['/bin/ps', '-axo', 'pid=,ppid=,pgid=,stat='],
                            check=True, capture_output=True, text=True, timeout=5)
        members = [line.split() for line in ps.stdout.splitlines() if int(line.split()[2]) == group]
        with (root / 'groups.local.jsonl').open('a') as stream:
            stream.write(json.dumps({'group': group, 'members': members}) + '\n')
        return members

    package = root / 'release'
    (package / 'silk').mkdir(parents=True)
    code = """set -eu
read -r mode < "$AN_SPOOL_INPUT"
if [ -n "${AN_SPOOL_STATE:-}" ]; then
  cat "$AN_SPOOL_INPUT" > observed-data
  cat "$AN_SPOOL_STATE" > observed-state
  [ "$mode" = preserved ] || exit 92
  read -r mode < "$AN_SPOOL_STATE"
fi
if [ "$mode" = changed ]; then printf 'changed-data\\n' > report.txt; exit 0; fi
case "$PWD" in */advance/job/work) printf "quick\n" > report.txt; exit 0;; esac
if [ "$mode" = quick ]; then printf 'preserved\n' > report.txt; exit 0; fi
printf '%s\n' "$$" > leader.pid
worker() {
  for tick in 1 2 3 4 5 6 7 8; do
    printf '%s\n' "$tick" >> "$1.heartbeat"
    sleep 1
  done
}
worker alpha &
first=$!
worker beta &
second=$!
printf '%s\n%s\n' "$first" "$second" > workers.pid
wait "$first"
wait "$second"
printf 'late-output\n' > report.txt
"""
    (package / 'silk/main.sh').write_text(code)
    (package / 'native-spool.json').write_text(json.dumps({
        'schema': 'an-native-spool-v2', 'name': 'supervised-workers', 'version': '0.1.0',
        'entry': 'silk/main.sh', 'files': {'silk/main.sh': hashlib.sha256(code.encode()).hexdigest()},
        'outputs': ['report.txt'], 'requirements': {
            'runtime': 'macos-native-process-v1', 'os': 'macos', 'arch': 'portable-source',
            'interaction': 'headless', 'gpu': 'none', 'inputs': ['input.txt'],
            'editable': [], 'dependencies': [], 'capabilities': ['read-input', 'write-declared-outputs']}}))
    quick, work_input = root / 'quick.txt', root / 'workers.txt'
    quick.write_text('quick\n')
    work_input.write_text('workers\n')
    a, b = root / 'a', root / 'b'
    call('spool-load', package, a)
    call('spool-load', package, b)
    state_package = package
    if args.combined:
        state_package = root / 'combined-release'
        (state_package / 'silk').mkdir(parents=True)
        (state_package / 'silk/main.sh').write_text(code)
        manifest = json.loads((package / 'native-spool.json').read_text())
        manifest['schema'] = 'an-native-spool-v3'
        manifest['events'] = {'consumes': [], 'produces': ['artifact.accepted']}
        manifest['requirements']['state_input'] = 'checkpoint.txt'
        (state_package / 'native-spool.json').write_text(json.dumps(manifest))
    try:
        normal = approved('spool-supervisor', 'start', a, 'baseline', quick, 15)
        assert normal['status'] == 'succeeded'
        approved('spool-accept', a, 'baseline')
        original = call('spool-artifacts', a, 'baseline')
        if args.combined:
            changed = root / 'changed.txt'
            changed.write_text('changed\n')
            approved('spool-start', a, 'replacement', changed, 15)
            approved('spool-accept', a, 'replacement')
            replacement = call('spool-artifacts', a, 'replacement')

        def select(path, version, state):
            path.write_text(json.dumps({'schema': 'an-native-data-state-v1',
                'data': {'catalog': version['catalog'], 'revision': version['outputs'][0]['binding']['revision']},
                'state_revision': state}))

        old_cancel = call('spool-supervisor', 'cancel', a, 'baseline')['approval_sha256']
        for name, seconds in [('deadline', 3), ('cancel', 20), ('client-cancel', 20), ('owner-lost', 20)]:
            instance = root / (name + '-state')
            call('spool-load', state_package, instance)
            revision = approved('spool-state-init', instance, 'report.txt', work_input)['state']['revision']
            selection = root / (name + '-selection.json')
            operation, selected = 'start-state', revision
            if args.combined:
                select(selection, original, revision)
                operation, selected = 'start-data-state', selection
            approved('spool-' + operation, instance, 'advance', selected, 15)
            approved('spool-accept', instance, 'advance')
            newer = approved('spool-state-commit', instance, revision, 'advance')['revision']
            assert newer != revision
            checkpoint = call('spool-state-show', instance, revision)['selected']
            preview = call('spool-supervisor', operation, instance, name, selected, seconds)
            if args.combined:
                select(selection, replacement, newer)
                call('spool-supervisor', operation, instance, name, selected, seconds,
                     '--approve', preview['approval_sha256'], success=False)
                assert not (instance / 'runs' / name / 'supervisor').exists()
                select(selection, original, revision)
            client = spawn('spool-supervisor', operation, instance, name, selected, seconds,
                           '--approve', preview['approval_sha256'])
            run = instance / 'runs' / name
            work = run / 'job/work'
            until(lambda: (work / 'workers.pid').exists()
                  and (work / 'alpha.heartbeat').exists() and (work / 'beta.heartbeat').exists())
            if args.combined:
                select(selection, replacement, newer)
                assert (work / 'observed-data').read_text() == 'preserved\n'
                assert (work / 'observed-state').read_text() == 'workers\n'
                start = json.loads((run / 'start.json').read_text())
                assert start['data_input']['binding']['revision'] == original['outputs'][0]['binding']['revision']
                assert start['state_input']['binding']['revision'] == revision
            owner = json.loads((run / 'supervisor/owner.json').read_text())
            group = int((work / 'leader.pid').read_text())
            members = group_members(group)
            assert len(members) >= 3
            call('spool-supervisor', 'worker', instance, name, preview['approval_sha256'], success=False)
            assert not call('spool-status', instance, name)['termination_confirmed']
            call('spool-reclaim', instance, name, success=False)
            call('spool-supervisor', 'cancel', instance, name, '--approve', old_cancel, success=False)
            changed_at = time.monotonic()
            if name == 'owner-lost':
                # Kill only the supervisor created by this fixture. Finite
                # workers expire independently; observation never adopts a PID.
                os.kill(owner['pid'], signal.SIGKILL)
                finish(client, False)
                until(lambda: not group_members(group))
                assert not call('spool-status', instance, name)['termination_confirmed']
                assert not (run / 'supervisor/completion.json').exists()
                for marker in instance.rglob('host.lock'):
                    marker.unlink()  # Disposable, stopped fixture fault injection.
                call('spool-supervisor', operation, instance, name, selected, seconds, success=False)
                call('spool-' + operation, instance, name, selected, seconds, success=False)
                call('spool-supervisor', 'worker', instance, name, preview['approval_sha256'], success=False)
                call('spool-reclaim', instance, name, success=False)
                call('spool-accept', instance, name, success=False)
                assert work.exists()
                assert call('spool-state-show', instance, revision)['selected'] == checkpoint
                assert len(list((run / 'job/attempts').iterdir())) == 1
                cases[name] = {'termination_confirmed': False, 'replayed': False, 'collected': False}
                continue

            if name == 'client-cancel':
                client.send_signal(signal.SIGTERM)
                result = finish(client, False)
                assert result['status'] == 'cancelled', result
            else:
                client.kill()
                finish(client, False)
                assert client.returncode == -signal.SIGKILL
            if name == 'cancel':
                approved('spool-supervisor', 'cancel', instance, name)
            elif name == 'deadline':
                independent = approved('spool-supervisor', 'start', b, 'independent', quick, 15)
                assert independent['status'] == 'succeeded'
            until(lambda: (run / 'supervisor/completion.json').exists())
            view = call('spool-supervisor', 'show', instance, name)
            expected = 'timed_out' if name == 'deadline' else 'cancelled'
            assert view['native']['execution_status'] == expected, view
            assert view['native']['termination_confirmed'], view
            assert not group_members(group)
            call('spool-accept', instance, name, success=False)
            approved('spool-reclaim', instance, name)
            assert call('spool-state-show', instance)['head'] == newer
            assert call('spool-state-show', instance, revision)['selected'] == checkpoint
            call('spool-supervisor', operation, instance, name, selected, seconds, success=False)
            cases[name] = {'outcome': expected, 'termination_confirmed': True,
                           'signal_to_observation_seconds': time.monotonic() - changed_at}
        # A torn prelaunch reservation is uncertain, not a fresh run.
        c = root / 'torn'
        call('spool-load', package, c)
        call('spool-supervisor', 'start', c, 'reserved', quick, 15)
        reservation = c / 'runs/reserved/supervisor'
        reservation.mkdir()
        assert call('spool-status', c, 'reserved')['phase'] == 'supervisor-reserved-or-unconfirmed'
        call('spool-start', c, 'reserved', quick, 15, success=False)
        call('spool-supervisor', 'start', c, 'reserved', quick, 15, success=False)
        call('spool-unload', c, success=False)
        assert not (c / 'runs/reserved/job').exists()
        assert call('spool-artifacts', a, 'baseline') == original
        (root / 'report.json').write_text(json.dumps({
            'passed': True, 'host_sha256': hashlib.sha256(host.read_bytes()).hexdigest(),
            'cases': cases, 'formal_output_preserved': True, 'automatic_retries': 0,
            'model_calls': 0, 'scope': 'explicit old state with newer HEAD; independent supervision', 'fixed_old_state': True, 'combined_data_state': args.combined
        }, indent=2))
    finally:
        for child in children:
            if child.poll() is None:
                child.send_signal(signal.SIGTERM)
                child.communicate(timeout=25)


if __name__ == '__main__':
    main()
