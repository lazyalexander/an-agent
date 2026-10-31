#!/usr/bin/env python3
"""Measure foreground supervisor loss and enforce uncertain-execution custody."""
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
    host = args.host.resolve(strict=True)
    root = args.evidence.resolve()
    root.mkdir(mode=0o700)
    calls, samples, children = [], [], []
    started = time.monotonic()

    def record():
        (root / 'commands.local.json').write_text(json.dumps(calls, indent=2))
        (root / 'processes.local.json').write_text(json.dumps(samples, indent=2))

    def spawn(*argv):
        child = subprocess.Popen([str(host), *map(str, argv)], text=True,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        children.append(child)
        return child

    def finish(child, success=True):
        out, err = child.communicate(timeout=25)
        calls.append({'args': child.args, 'exit': child.returncode, 'stdout': out, 'stderr': err})
        record()
        assert (child.returncode == 0) == success, calls[-1]
        return json.loads(out) if out else None

    def call(*argv, success=True):
        return finish(spawn(*argv), success)

    def approve(*argv):
        preview = call(*argv)
        return call(*argv, '--approve', preview['approval_sha256'])

    def group_snapshot(group):
        observed = subprocess.run(
            ['/bin/ps', '-axo', 'pid=,ppid=,pgid=,stat='], text=True,
            capture_output=True, check=True, timeout=5)
        rows = [line.split() for line in observed.stdout.splitlines()]
        members = [{'pid': int(row[0]), 'ppid': int(row[1]), 'pgid': int(row[2]),
                    'state': row[3]} for row in rows if int(row[2]) == group]
        samples.append({'elapsed_seconds': time.monotonic() - started, 'members': members})
        record()
        return members

    def wait_until(predicate, seconds=20):
        deadline = time.monotonic() + seconds
        while not predicate():
            assert time.monotonic() < deadline, 'bounded observation deadline exceeded'
            time.sleep(.05)

    package = root / 'release'
    (package / 'silk').mkdir(parents=True)
    # All workers have their own finite lifetime, even if the tested supervisor
    # dies. The observer never signals numeric orphan PIDs or removes work files.
    code = """set -eu
read -r mode < "$AN_SPOOL_INPUT"
if [ "$mode" = quick ]; then printf 'preserved\n' > report.txt; exit 0; fi
printf '%s\n' "$$" > leader.pid
worker() {
  label="$1"
  for tick in 1 2 3 4 5 6 7 8; do
    printf '%s\n' "$tick" >> "$label.heartbeat"
    sleep 1
  done
  printf done > "$label.done"
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
        'schema': 'an-native-spool-v2', 'name': 'supervisor-loss', 'version': '0.1.0',
        'entry': 'silk/main.sh', 'files': {'silk/main.sh': hashlib.sha256(code.encode()).hexdigest()},
        'outputs': ['report.txt'], 'requirements': {
            'runtime': 'macos-native-process-v1', 'os': 'macos', 'arch': 'portable-source',
            'interaction': 'headless', 'gpu': 'none', 'inputs': ['input.txt'],
            'editable': [], 'dependencies': [], 'capabilities': ['read-input', 'write-declared-outputs']}}))
    seed = root / 'input.txt'
    seed.write_text('quick\n')
    independent = root / 'independent'
    call('spool-load', package, independent)
    summary = {}
    try:
        for name, stop in [('cancelled', signal.SIGTERM), ('killed', signal.SIGKILL)]:
            instance = root / name
            call('spool-load', package, instance)
            seed.write_text('quick\n')
            approve('spool-start', instance, 'baseline', seed, 15)
            approve('spool-accept', instance, 'baseline')
            accepted = call('spool-artifacts', instance, 'baseline')
            seed.write_text('workers\n')
            preview = call('spool-start', instance, 'work', seed, 3)
            child = spawn('spool-start', instance, 'work', seed, 3,
                          '--approve', preview['approval_sha256'])
            work = instance / 'runs/work/job/work'
            wait_until(lambda: (work / 'workers.pid').exists()
                       and (work / 'alpha.heartbeat').exists() and (work / 'beta.heartbeat').exists())
            assert child.poll() is None
            group = int((work / 'leader.pid').read_text())
            workers = {int(pid) for pid in (work / 'workers.pid').read_text().split()}
            before = group_snapshot(group)
            assert workers.issubset({item['pid'] for item in before}), before
            killed_at = time.monotonic()
            child.send_signal(stop)
            result = finish(child, False)
            signal_to_exit = time.monotonic() - killed_at
            if name == 'cancelled':
                assert result['status'] == 'cancelled', result
                assert result['workspace']['termination_confirmed'] is True
                assert not group_snapshot(group)
                approve('spool-reclaim', instance, 'work')
                summary[name] = {'termination_confirmed': True,
                                 'cancel_to_receipt_seconds': signal_to_exit}
                continue

            assert child.returncode == -signal.SIGKILL
            live = group_snapshot(group)
            assert workers.issubset({item['pid'] for item in live}), live
            # Observe beyond the former owner's deadline before unrelated CLI
            # checks. Readiness, rather than launch timing, gates fault injection.
            wait_until(lambda: time.monotonic() - killed_at >= 4)
            past_deadline = group_snapshot(group)
            assert any(item['pid'] in workers for item in past_deadline), past_deadline
            view = call('spool-status', instance, 'work')
            assert not view['termination_confirmed'], view
            for command in ['spool-reclaim', 'spool-accept', 'spool-reconcile-run']:
                call(command, instance, 'work', success=False)
            assert work.exists()
            seed.write_text('quick\n')
            approve('spool-start', independent, 'unaffected', seed, 15)
            wait_until(lambda: not group_snapshot(group))
            assert (work / 'alpha.done').exists() and (work / 'beta.done').exists()
            assert (work / 'report.txt').read_text() == 'late-output\n'
            assert not call('spool-status', instance, 'work')['termination_confirmed']
            attempts = sorted(path.name for path in (instance / 'runs/work/job/attempts').iterdir())
            # Fault injection on this closed, disposable fixture only: remove
            # stale operation markers after observed exit to expose the deeper
            # evidence checks. This is not a product recovery procedure.
            locks = list(instance.rglob('host.lock'))
            assert locks
            for lock in locks:
                lock.unlink()
            for command in ['spool-reclaim', 'spool-accept', 'spool-reconcile-run']:
                call(command, instance, 'work', success=False)
            call('spool-unload', instance, success=False)
            call('spool-start', instance, 'work', seed, 3, success=False)
            assert attempts == sorted(path.name for path in (instance / 'runs/work/job/attempts').iterdir())
            assert work.exists() and not (instance / 'runs/work/accepted.json').exists()
            assert call('spool-artifacts', instance, 'baseline') == accepted
            summary[name] = {'termination_confirmed': False, 'workers_after_owner_exit': len(workers),
                             'workers_outlived_deadline': True, 'late_output_accepted': False,
                             'stale_lock_count': len(locks), 'evidence_refusal_without_locks': True,
                             'unknown_execution_retried': False, 'formal_output_preserved': True}
        (root / 'report.json').write_text(json.dumps({
            'passed': True, 'host_sha256': hashlib.sha256(host.read_bytes()).hexdigest(),
            'observed': summary, 'automatic_retries': 0, 'model_calls': 0,
            'decision': 'foreground owner loss needs an independent execution supervisor',
            'scope': 'trusted non-detaching native process group; externally sampled PIDs are not custody authority'
        }, indent=2))
    finally:
        for child in children:
            if child.poll() is None:
                child.send_signal(signal.SIGTERM)
                child.communicate(timeout=20)
        record()


if __name__ == '__main__':
    main()
