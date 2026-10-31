#!/usr/bin/env python3
"""Real concurrent observers and exact revocations; no retries or timing workaround."""
import argparse, hashlib, json, os, signal, subprocess, time
from pathlib import Path
from host_qualification import require_native_workspace
parser = argparse.ArgumentParser(description='Concurrent observation/control; each mutation submitted once.')
parser.add_argument('--host', type=Path, required=True)
parser.add_argument('--fixture', type=Path, required=True, help='Completed native_events.py evidence')
parser.add_argument('--evidence', type=Path, required=True)
options = parser.parse_args()
require_native_workspace()
host = options.host.resolve(strict=True)
root = options.evidence.resolve()
root.mkdir(mode=448)
fixture = options.fixture.resolve(strict=True)
producer = fixture / 'a'
consumer = fixture / 'b'
assert json.loads((fixture / 'acceptance.json').read_text())['passed'] is True
env = dict(os.environ, AN_AGENT_HOST_TIMING='1')
rows = []

def call(*args):
    start = time.monotonic_ns()
    process = subprocess.run([str(host), *map(str, args)], capture_output=True, text=True, env=env, timeout=40)
    record = dict(args=list(map(str, args)), code=process.returncode, stdout=process.stdout, stderr=process.stderr, start_ns=start, elapsed_ms=(time.monotonic_ns() - start) / 1000000.0)
    rows.append(record)
    with (root / 'commands.jsonl').open('a') as f:
        f.write(json.dumps(record) + '\n')
    return record

def checked(*args):
    record = call(*args)
    assert record['code'] == 0, record['stderr']
    return json.loads(record['stdout'])

def approve(*args):
    process = checked(*args)
    return checked(*args, '--approve', process['approval_sha256'])
runs = ['first', 'second']
for i in range(6):
    name = f'lock-evidence-{i}'
    approve('spool-start', producer, name, fixture / 'seed.txt', '15')
    approve('spool-accept', producer, name)
    runs.append(name)
summary = []
for events in [2, 8]:
    for watchers in [0, 1, 4]:
        for rep in range(2):
            case = root / f'e{events}-w{watchers}-r{rep}'
            case.mkdir()
            space = case / 'space'
            checked('spool-events-init', space)
            route = case / 'route.json'
            route.write_text(json.dumps(dict(schema='an-native-artifact-route-v1', source=str(producer), output='report.txt', target=str(consumer), result='report.txt', wall_seconds=30)))
            for i in range(12):
                approve('spool-event-subscribe', space, f'r{i}', route)
            for run in runs[:events]:
                approve('spool-event-publish', space, producer, run, 'report.txt')
            plans = [checked('spool-event-revoke', space, f'r{i}')['approval_sha256'] for i in range(12)]
            observers = []
            handles = []
            projection = space / 'facts/env.md'
            initial = (projection.stat().st_ino, projection.stat().st_mtime_ns, projection.read_bytes())
            try:
                for i in range(watchers):
                    out = (case / f'watch-{i}.jsonl').open('w')
                    err = (case / f'watch-{i}.stderr').open('w')
                    handles.extend([out, err])
                    observers.append(subprocess.Popen([str(host), 'spool-events-watch', str(space), '60', '100'], stdout=out, stderr=err, text=True, env=env))
                if watchers:
                    deadline = time.monotonic() + 20
                    while any((process.poll() is None and (not (case / f'watch-{i}.jsonl').stat().st_size) for (i, process) in enumerate(observers))):
                        if time.monotonic() > deadline:
                            raise RuntimeError('observer readiness timeout; no retry')
                        time.sleep(0.005)
                assert initial == (projection.stat().st_ino, projection.stat().st_mtime_ns, projection.read_bytes()), 'observation wrote projection'
                before = len(rows)
                for (i, sha) in enumerate(plans):
                    call('spool-event-revoke', space, f'r{i}', '--approve', sha)
                mutations = rows[before:]
            finally:
                for process in observers:
                    if process.poll() is None:
                        process.send_signal(signal.SIGINT)
                for process in observers:
                    process.wait(timeout=15)
                for f in handles:
                    f.close()
            outcomes = []
            for record in mutations:
                prior = json.loads(next((x['stdout'] for x in reversed(rows[:before]) if x['args'] == record['args'][:3])))
                exists = (space / 'revocations' / f"{prior['plan']['subscription']}.json").exists()
                outcomes.append(dict(code=record['code'], persisted=exists, elapsed_ms=record['elapsed_ms']))
            result = dict(events=events, routes=12, watchers=watchers, rep=rep, approvals=outcomes, watch_codes=[process.returncode for process in observers], evidence=str(case))
            summary.append(result)
            (root / 'summary.json').write_text(json.dumps(dict(host_sha256=hashlib.sha256(host.read_bytes()).hexdigest(), automatic_retries=0, samples=summary), indent=2))
            print(json.dumps({k: v for (k, v) in result.items() if k not in ['approvals', 'evidence']}), 'refused', sum((x['code'] != 0 for x in outcomes)), flush=True)
for sample in summary:
    assert all((a['code'] == 0 and a['persisted'] for a in sample['approvals'])), sample
    assert all((code == 0 for code in sample['watch_codes'])), sample
    for err in Path(sample['evidence']).glob('watch-*.stderr'):
        metrics = [json.loads(line.split('=', 1)[1]) for line in err.read_text().splitlines() if line.startswith('AN_AGENT_TIMING=')]
        assert not any((m['stage'] in ['lock_acquire', 'lock_hold', 'write_sync'] for m in metrics)), err
(root / 'acceptance.json').write_text(json.dumps(dict(passed=True, host_sha256=hashlib.sha256(host.read_bytes()).hexdigest(), approved_revocations=144, automatic_retries=0, observation_writes=False, all_observers_survived=True), indent=2))
