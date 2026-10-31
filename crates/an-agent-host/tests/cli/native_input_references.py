#!/usr/bin/env python3
"""Native input claim ownership and bounded release after real cancellation."""
import argparse
import hashlib
import json
from pathlib import Path
from host_qualification import require_native_workspace
import signal
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--host', type=Path, required=True)
parser.add_argument('--fixture', type=Path, required=True)
parser.add_argument('--evidence', type=Path, required=True)
args = parser.parse_args()
require_native_workspace()
host = args.host.resolve(strict=True)
fixture = args.fixture.resolve(strict=True)
root = args.evidence.resolve()
root.mkdir(mode=0o700)
assert json.loads((fixture / 'acceptance.json').read_text())['passed']
commands = []


def finish(process, success=True):
    try:
        out, err = process.communicate(timeout=40)
    except subprocess.TimeoutExpired as error:
        process.send_signal(signal.SIGINT)
        process.communicate(timeout=15)
        raise RuntimeError('deadline exceeded; retain evidence, no retry') from error
    commands.append(dict(args=process.args, code=process.returncode, stdout=out, stderr=err))
    (root / 'commands.json').write_text(json.dumps(commands, indent=2))
    assert (process.returncode == 0) == success, (out, err)
    return json.loads(out) if out else None


def call(*command, success=True):
    return finish(subprocess.Popen([str(host), *map(str, command)], text=True,
                  stdout=subprocess.PIPE, stderr=subprocess.PIPE), success)


def approve(*command):
    preview = call(*command)
    return call(*command, '--approve', preview['approval_sha256'])


producer = fixture / 'a'
succeeded = fixture / 'b'
original = call('spool-input-references', succeeded, 'direct')
assert len(original['claims']) == 2
assert all(c['release'] is None for c in original['claims'])
call('spool-input-release', succeeded, 'direct', success=False)
package = root / 'release'
(package / 'silk').mkdir(parents=True)
code = 'set -eu\nprintf ready > ready.marker\nsleep 25\ncat "$AN_SPOOL_STATE" > state.txt\n'
(package / 'silk/main.sh').write_text(code)
manifest = json.loads((fixture / 'counter-release/native-spool.json').read_text())
manifest.update(name='holding-references', entry='silk/main.sh', files={
    'silk/main.sh': hashlib.sha256(code.encode()).hexdigest()}, outputs=['state.txt'])
manifest['requirements']['editable'] = []
(package / 'native-spool.json').write_text(json.dumps(manifest))
consumer = root / 'consumer'
call('spool-load', package, consumer)
seed = root / 'seed.txt'
seed.write_text('0\n')
state = approve('spool-state-init', consumer, 'state.txt', seed)['state']['revision']
source = call('spool-artifacts', producer, 'first')


def incoming():
    view = call('spool-artifacts', producer, 'first', '--references', 'batch.txt')
    assert view['binding'] == original['claims'][0]['reference']['binding']
    for key in ('revision', 'content_sha256', 'bytes', 'parent'):
        assert view['binding'][key] == source['outputs'][0]['binding'][key]
    assert view['observation_only'] and not view['atomic_snapshot']
    assert not view['reference_census_complete'] and not view['collection_authorized']
    assert not view['consumer_receipts_checked']
    return view


call('spool-artifacts', producer, 'first', '--references', 'unknown.txt', success=False)
assert incoming()['registered_claims'] >= 1
request = root / 'input.json'
request.write_text(json.dumps(dict(schema='an-native-data-state-v1', data={
    'catalog': source['catalog'], 'revision': source['outputs'][0]['binding']['revision']},
    state_revision=state)))
preview = call('spool-start-data-state', consumer, 'held', request, 30)
assert not (consumer / 'runs/held/input-references.json').exists()
# Fail the second source claim after the first one was durably acquired. This
# explicit fault injection must fence the run, not silently retry or remove pins.
partial = call('spool-start-data-state', consumer, 'partial', request, 30)
state_catalog = Path(partial['plan']['state_input']['catalog'])
lock = state_catalog / 'host.lock'
with lock.open('x') as handle:
    handle.write('fixture: second catalog held during reference acquisition\n')
try:
    call('spool-start-data-state', consumer, 'partial', request, 30,
         '--approve', partial['approval_sha256'], success=False)
finally:
    lock.unlink()
partial_dir = consumer / 'runs/partial'
assert (partial_dir / 'input-references.json').is_file()
assert not (partial_dir / 'input-references-registered.json').exists()
assert not (partial_dir / 'start.json').exists() and not (partial_dir / 'job').exists()
source_claims = Path(source['catalog']) / 'revisions' / source['outputs'][0]['binding']['revision'] / 'native-input-references'
retained = {p.name: p.read_bytes() for p in source_claims.iterdir()}
assert any(json.loads(v)['owner']['run'] == 'partial' for v in retained.values())
assert any(c['reference']['owner']['run'] == 'partial'
           and c['observation'] == 'claim_without_source_release' for c in incoming()['claims'])
call('spool-input-references', consumer, 'partial', success=False)
call('spool-input-release', consumer, 'partial', success=False)
call('spool-start-data-state', consumer, 'partial', request, 30,
     '--approve', partial['approval_sha256'], success=False)
assert {p.name: p.read_bytes() for p in source_claims.iterdir()} == retained
assert not (partial_dir / 'job').exists()
command = ['spool-start-data-state', str(consumer), 'held', str(request), '30',
           '--approve', preview['approval_sha256']]
process = subprocess.Popen([str(host), *command], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
try:
    deadline = time.monotonic() + 20
    while not (consumer / 'runs/held/job/work/ready.marker').exists():
        if process.poll() is not None or time.monotonic() > deadline:
            raise RuntimeError('real readiness missing; no retry')
        time.sleep(.02)
    active = call('spool-input-references', consumer, 'held')
    assert len(active['claims']) == 2 and all(c['release'] is None for c in active['claims'])
    assert active['plan']['owner'] != original['plan']['owner']
    assert active['claims'][0]['reference']['binding'] == original['claims'][0]['reference']['binding']
    # Source observation does not wait for producer/catalog control locks or read
    # the running consumer's private control state. It grants no collection.
    with (producer / 'host.lock').open('x') as producer_lock, \
            (Path(source['catalog']) / 'host.lock').open('x') as catalog_lock:
        producer_lock.write('fixture: producer mutation lock\n')
        catalog_lock.write('fixture: source mutation lock\n')
        try:
            view = incoming()
            assert any(c['reference']['owner']['run'] == 'held'
                       and c['observation'] == 'claim_without_source_release' for c in view['claims'])
        finally:
            (producer / 'host.lock').unlink()
            (Path(source['catalog']) / 'host.lock').unlink()
    call('spool-input-release', consumer, 'held', success=False)
    approve('spool-artifact-retire', producer, 'first', 'batch.txt')
finally:
    if process.poll() is None:
        process.send_signal(signal.SIGINT)
    cancelled = finish(process, False)
assert cancelled['status'] == 'cancelled' and cancelled['workspace']['termination_confirmed']
call('spool-input-release', consumer, 'held', success=False)
approve('spool-reclaim', consumer, 'held')
call('spool-input-release', consumer, 'held', success=False)
approve('spool-reclaim-snapshots', consumer, 'held')
result = consumer / 'runs/held/result.json'
saved = result.read_bytes()
result.unlink()
call('spool-input-release', consumer, 'held', success=False)
assert all(c['release'] is None for c in call('spool-input-references', consumer, 'held')['claims'])
result.write_bytes(saved)
preview = call('spool-input-release', consumer, 'held')
call('spool-input-release', consumer, 'held', '--approve', '0' * 64, success=False)
assert not (consumer / 'runs/held/input-release-intent.json').exists()
released = call('spool-input-release', consumer, 'held', '--approve', preview['approval_sha256'])
assert released['status'] == 'succeeded' and released['payloads_deleted'] is False
assert call('spool-input-release', consumer, 'held')['already_released'] is True
claims = call('spool-input-references', consumer, 'held')['claims']
assert all(c['release']['approval_sha256'] == preview['approval_sha256'] for c in claims)
assert all(c['release'] is None for c in call('spool-input-references', succeeded, 'direct')['claims'])
released_view = incoming()
assert any(c['reference']['owner']['run'] == 'held'
           and c['observation'] == 'source_release_recorded' for c in released_view['claims'])
assert any(c['reference']['owner']['run'] == 'direct'
           and c['observation'] == 'claim_without_source_release' for c in released_view['claims'])
for claim in claims:
    ref = claim['reference']
    payload = Path(ref['catalog']) / 'revisions' / ref['binding']['revision'] / 'content.txt'
    assert hashlib.sha256(payload.read_bytes()).hexdigest() == ref['binding']['content_sha256']
complete = consumer / 'runs/held/input-released.json'
saved = complete.read_bytes()
complete.unlink()
call('spool-input-release', consumer, 'held', '--approve', preview['approval_sha256'], success=False)
assert call('spool-input-references', consumer, 'held')['claims'] == claims
assert not complete.exists()
assert incoming() == released_view  # A source marker cannot prove consumer completion.
complete.write_bytes(saved)
assert call('spool-input-release', consumer, 'held')['already_released'] is True
partial_claim = next(p for p in source_claims.glob('*.json')
                     if json.loads(p.read_text()).get('owner', {}).get('run') == 'partial')
saved_claim = partial_claim.read_bytes()
partial_claim.write_bytes(b'{torn')
call('spool-artifacts', producer, 'first', '--references', 'batch.txt', success=False)
assert partial_claim.read_bytes() == b'{torn'
partial_claim.write_bytes(saved_claim)
assert incoming() == released_view
summary = dict(passed=True, host_sha256=hashlib.sha256(host.read_bytes()).hexdigest(),
               cancelled_references_released=True, successful_references_retained=True,
               partial_acquisition_retained=True, lost_confirmation_not_retried=True,
               source_observation_without_control_locks=True, source_markers_not_consumer_completion=True,
               malformed_source_claim_refused=True, payloads_deleted=0, automatic_retries=0)
(root / 'acceptance.json').write_text(json.dumps(summary, indent=2))
print(json.dumps(summary))
