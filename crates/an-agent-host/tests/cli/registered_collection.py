#!/usr/bin/env python3
"""Real headless registered publication, copy readers, checkpoint retention and exact deletion."""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
from host_qualification import require_native_workspace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    require_native_workspace()
    host, root = args.host.resolve(strict=True), args.evidence.resolve()
    root.mkdir(mode=0o700)
    log = root / 'commands.local.jsonl'

    def call(*argv, fail=None):
        result = subprocess.run([host, *map(str, argv)], text=True, capture_output=True, timeout=30)
        with log.open('a') as f:
            f.write(json.dumps(dict(args=list(map(str, argv)), code=result.returncode,
                                    stdout=result.stdout, stderr=result.stderr)) + '\n')
        if fail is not None:
            assert result.returncode and fail in result.stderr, (argv, result.stderr, result.stdout)
            return
        assert not result.returncode, (argv, result.stderr, result.stdout)
        return json.loads(result.stdout)

    def approve(*argv):
        return call(*argv, '--approve', call(*argv)['approval_sha256'])

    def produce(run, registered=True):
        approve('spool-start', producer, run, source, 15)
        flags = ['--registered'] if registered else []
        approve('spool-accept', producer, run, *flags)
        return call('spool-artifacts', producer, run)

    def select(report, output):
        return next(o['binding']['revision'] for o in report['outputs'] if o['path'] == output)

    def command(op, run, output, rev, *extra):
        return ['spool-managed', op, producer, run, output, rev, *extra]

    package = root / 'release'
    (package / 'silk').mkdir(parents=True)
    script = 'set -eu\ncat "$AN_SPOOL_INPUT" > bulk.txt\nprintf retained > state.txt\n'
    (package / 'silk/main.sh').write_text(script)
    (package / 'native-spool.json').write_text(json.dumps({
        'schema': 'an-native-spool-v3', 'name': 'collection', 'version': '1',
        'entry': 'silk/main.sh', 'files': {'silk/main.sh': hashlib.sha256(script.encode()).hexdigest()},
        'outputs': ['bulk.txt', 'state.txt'], 'binary_outputs': ['bulk.txt'],
        'events': {'consumes': [], 'produces': ['artifact.accepted']},
        'requirements': {'runtime': 'macos-native-process-v1', 'os': 'macos', 'arch': 'portable-source',
                         'interaction': 'headless', 'gpu': 'none', 'inputs': ['input.txt'],
                         'editable': [], 'dependencies': [], 'capabilities': ['read-input', 'write-declared-outputs']}}))
    producer, consumer = root / 'source', root / 'consumer'
    for instance in (producer, consumer):
        call('spool-load', package, instance)
    source = root / 'seed.txt'
    source.write_bytes(bytes(range(256)) * 4096)

    old = produce('legacy', False)
    legacy = select(old, 'bulk.txt')
    call(*command('delete', 'legacy', 'bulk.txt', legacy), fail='enrolled at first publication')
    call('spool-accept', producer, 'legacy', '--registered', fail='fresh per-run catalog')
    report = produce('registered')
    approve('spool-reclaim', producer, 'registered')
    a, b = select(report, 'state.txt'), select(report, 'bulk.txt')
    ca = lambda op, *rest: command(op, 'registered', 'state.txt', a, *rest)
    cb = lambda op, *rest: command(op, 'registered', 'bulk.txt', b, *rest)
    call(*cb('delete'), fail='close reader admission')
    call(*command('show', 'registered', 'bulk.txt', a), fail='exact version')
    assert next(o for o in report['outputs'] if o['path'] == 'bulk.txt')['binding']['encoding'] == 'bytes'
    call('artifact-checkout', report['catalog'], b, root / 'bypass', fail='registered reader')
    db = Path(report['catalog']) / 'revisions' / b / 'registered-custody.sqlite3'
    call('spool-start', consumer, 'raw-bypass', db, 15, fail='raw access')
    assert not (db.parent / 'content.txt').exists()
    profile = root / 'readonly.json'
    job = root / 'broad-root-job'
    profile.write_text(json.dumps({'schema': 'an-workspace-job-v2', 'executable': '/bin/bash',
        'args': ['-c', 'cat "$2" > /dev/null || exit 73; cat "$1" > /dev/null 2>&1 && exit 71; printf denied > result.txt', 'test', str(db), str(db.parent / 'revision.json')],
        'environment': {}, 'readonly': [str(db.parent)], 'inputs': [str(source)],
        'outputs': ['result.txt'], 'trusted_process_group': True, 'wall_seconds': 15}))
    assert approve('workspace-run', profile, job)['status'] == 'succeeded'
    assert (job / 'work/result.txt').read_text() == 'denied'


    seed_state = root / 'state-seed.txt'
    seed_state.write_text('initial')
    checkpoint = approve('spool-state-init', consumer, 'state.txt', seed_state)['state']['revision']
    selection = root / 'pins.json'
    selection.write_text(json.dumps({'schema': 'an-native-checkpoint-dependencies-v1', 'artifacts': [
        {'instance': str(producer), 'run': 'registered', 'output': 'state.txt', 'revision': a}]}))
    approve('spool-state-dependencies', consumer, checkpoint, selection)
    approve(*ca('close'))
    call(*ca('delete'), fail='checkpoint or unreleased')
    assert call('spool-state-show', consumer, checkpoint)['dependencies']['status'] == 'registered'

    copy = root / 'reader-copy.txt'
    approve(*cb('checkout', 'reader', copy))
    assert copy.read_bytes() == source.read_bytes()
    approve('spool-start', consumer, 'reads-copy', copy, 15)
    approve('spool-accept', consumer, 'reads-copy')
    late = cb('checkout', 'late', root / 'late.txt')
    stale_read = call(*late)['approval_sha256']
    approve(*cb('close'))
    call(*late, '--approve', stale_read, fail='admission is closed')
    assert not (root / 'late.txt').exists()
    call(*cb('checkout', 'late', root / 'late.txt'), fail='admission is closed')
    call(*cb('delete'), fail='checkpoint or unreleased')
    approve(*cb('release', 'reader'))
    sibling = call('spool-artifacts', producer, 'registered', '--output', 'state.txt')
    before = db.stat().st_size
    mirrors = db.parent / 'native-input-references'
    mirrors.mkdir()
    orphan = mirrors / 'orphan.json'
    orphan.write_text('{}')  # Disposable fixture: contradict the authoritative inventory.
    call(*cb('delete'), fail='additional source reference')
    orphan.unlink()
    digest = call(*cb('delete'))['approval_sha256']
    call(*cb('delete'), '--approve', '0' * 64, fail='approval mismatch')
    done = call(*cb('delete'), '--approve', digest)['receipt']
    assert done['revision'] == b and done['approval_sha256'] == digest
    assert done['logical_bytes'] == len(source.read_bytes())
    view = call(*cb('show'))
    assert view['phase'] == 'deleted' and view['receipt'] == done
    assert db.stat().st_size < before
    assert copy.read_bytes() == source.read_bytes()
    approve('spool-start', consumer, 'after-collection', copy, 15)
    approve('spool-accept', consumer, 'after-collection')
    call(*cb('delete'), '--approve', digest, fail='never replay')
    assert call('spool-artifacts', producer, 'registered', '--output', 'state.txt') == sibling
    call('spool-artifacts', producer, 'registered', '--output', 'bulk.txt', fail='collection recorded')
    assert call('spool-artifacts', producer, 'registered')['admission']['bulk.txt']['mode'] == 'deleted'
    assert call('spool-state-show', consumer, checkpoint)['dependencies']['status'] == 'registered'
    assert call('spool-artifacts', producer, 'legacy') == old

    # A successful copy whose completion write fails is still an outstanding reader.
    reader_run = 'unknown-reader'
    another = produce(reader_run)
    reader_rev = select(another, 'state.txt')
    reader_db = Path(another['catalog']) / 'revisions' / reader_rev / 'registered-custody.sqlite3'
    with sqlite3.connect(reader_db) as conn:
        conn.execute("""CREATE TRIGGER fail_copy BEFORE UPDATE OF ledger ON custody
            WHEN instr(NEW.ledger,'"phase":"copied"')>0
            BEGIN SELECT RAISE(ABORT,'copy completion interrupted'); END""")
    reader = lambda op, *args: command(op, reader_run, 'state.txt', reader_rev, *args)
    uncertain_copy = reader('checkout', 'lost', root / 'unconfirmed-copy.txt')
    copy_token = call(*uncertain_copy)['approval_sha256']
    call(*uncertain_copy, '--approve', copy_token, fail='copy completion interrupted')
    with sqlite3.connect(reader_db) as conn:
        conn.execute('DROP TRIGGER fail_copy')
    assert call(*reader('show'))['readers']['lost']['phase'] == 'opening'
    approve(*reader('close'))
    call(*reader('release', 'lost'), fail='original completed copy')
    call(*reader('delete'), fail='checkpoint or unreleased')
    assert (root / 'unconfirmed-copy.txt').read_text() == 'retained'

    evidence = {}
    for run in ('missing', 'interrupted'):
        product = produce(run)
        rev = select(product, 'bulk.txt')
        cmd = lambda op: command(op, run, 'bulk.txt', rev)
        approve(*cmd('close'))
        token = call(*cmd('delete'))['approval_sha256']
        backing = Path(product['catalog']) / 'revisions' / rev / 'registered-custody.sqlite3'
        # Disposable storage fault, outside product authority.
        with sqlite3.connect(backing) as conn:
            if run == 'missing':
                conn.execute('UPDATE custody SET payload=NULL WHERE id=1')
            else:
                conn.execute("""CREATE TRIGGER interrupt_receipt BEFORE UPDATE OF ledger ON custody
                    WHEN NEW.payload IS NULL BEGIN SELECT RAISE(ABORT,'interrupted receipt'); END""")
        if run == 'missing':
            call(*cmd('show'), fail='payload missing')
            call(*cmd('delete'), '--approve', token, fail='payload missing')
        else:
            call(*cmd('delete'), '--approve', token, fail='interrupted receipt')
            with sqlite3.connect(backing) as conn:
                conn.execute('DROP TRIGGER interrupt_receipt')
                assert conn.execute('SELECT length(payload) FROM custody').fetchone()[0] == len(source.read_bytes())
            uncertain = call(*cmd('show'))
            assert uncertain['phase'] == 'deletion-unconfirmed' and uncertain['receipt'] is None
            call(*cmd('delete'), '--approve', token, fail='never replay')
            assert call(*cmd('show')) == uncertain
        evidence[run] = 'no deletion receipt'
    (root / 'acceptance.json').write_text(json.dumps({
        'passed': True, 'checkpoint_pin_blocked': True, 'copy_reader_blocked_before_release': True,
        'deleted_revision': b, 'remaining_revision': a, 'logical_bytes_removed': done['logical_bytes'],
        'database_size_before': before, 'database_size_after': db.stat().st_size,
        'uncertain_reader_blocks_delete': True, 'broad_root_raw_read_denied': True,
        'faults': evidence, 'automatic_retries': 0, 'host_sha256': hashlib.sha256(host.read_bytes()).hexdigest()
    }, indent=2))


if __name__ == '__main__':
    main()
