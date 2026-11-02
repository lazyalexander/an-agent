"""Real event consumer: A is the fixed business input and B is its parent state."""
import json


def qualify(call, approved, workspace, producer, receiver, run, checkpoint, inputs):
    data, parent = [c['reference'] for c in inputs]
    revision = data['binding']['revision']
    request = receiver.parent / 'checkpoint-pins.json'
    request.write_text(json.dumps({'schema': 'an-native-checkpoint-dependencies-v1', 'artifacts': [
        {'instance': str(producer), 'run': 'first', 'output': 'batch.txt', 'revision': revision}]}))
    declaration = approved('spool-state-dependencies', receiver, checkpoint, request)['dependencies']
    release = ['spool-input-release', receiver, run, '--delivery', workspace, 'batches']
    call(*release, success=False)  # A positive pin alone never unlocks release.
    coverage = {'schema': 'an-checkpoint-exhaustive-input-coverage-v1', 'checkpoint': checkpoint,
                'declaration_sha256': declaration['approval_sha256'], 'inputs': [
                    {'catalog': ref['catalog'], 'revision': ref['binding']['revision'], 'disposition': use}
                    for ref, use in [(data, 'checkpoint-pin'), (parent, 'not-required')]]}
    file = receiver.parent / 'coverage.json'
    command = [*release, '--coverage', file]
    file.write_text(json.dumps(coverage))
    preview = call(*command)
    digest = preview['approval_sha256']
    for change in [dict(inputs=coverage['inputs'][:1]), dict(inputs=coverage['inputs'][::-1]),
                   dict(checkpoint=parent['binding']['revision']), dict(declaration_sha256='0' * 64),
                   dict(inputs=[dict(coverage['inputs'][0], disposition='not-required'), coverage['inputs'][1]])]:
        file.write_text(json.dumps(dict(coverage, **change)))
        call(*command, '--approve', digest, success=False)
    file.write_text(json.dumps(coverage))
    # A lost original acknowledgement or commit record cannot be repaired by release.
    state = call('spool-state-show', receiver, checkpoint)['selected']['binding']
    for record in (workspace / 'deliveries' / run / 'ack.json',
                   receiver / 'checkpoints' / ('commit-' + state['source']['approval_sha256'] + '.json'),
                   receiver / 'checkpoints/dependencies' / checkpoint / 'complete.json'):
        saved = record.read_bytes()
        try:
            record.unlink()
            call(*command, '--approve', digest, success=False)
            assert call('spool-input-references', receiver, run)['claims'] == inputs
            assert not (receiver / 'runs' / run / 'input-release-intent.json').exists()
        finally:
            record.write_bytes(saved)
    done = call(*command, '--approve', digest)
    assert done['status'] == 'succeeded' and not done['payloads_deleted']
    assert call(*command)['already_released']
    claims = call('spool-input-references', receiver, run)['claims']
    assert len(claims) == 2 and all(c['release']['approval_sha256'] == digest for c in claims)
    pins = call('spool-state-show', receiver, checkpoint)['dependencies']['claims']
    assert len(pins) == 1 and pins[0]['reference']['binding']['revision'] == revision
    assert pins[0]['release'] is None
    observed = call('spool-artifacts', producer, 'first', '--references', 'batch.txt')
    assert pins[0]['reference'] in [c['reference'] for c in observed['claims'] if c['release'] is None]
    assert not observed['collection_authorized']
    complete = receiver / 'runs' / run / 'input-released.json'
    saved = complete.read_bytes()
    try:
        complete.unlink()
        call(*command, '--approve', digest, success=False)
        assert not complete.exists() and call('spool-input-references', receiver, run)['claims'] == claims
        assert len(list((receiver / 'runs' / run / 'job/attempts').iterdir())) == 1
    finally:
        complete.write_bytes(saved)


def contradictory_policy(call, approved, workspace, producer, receiver, parent, event, batch, calls):
    # Both the operator's coverage and the registered A pin agree. The spool's
    # accepted policy for this batch disagrees, so neither attempt claim can move.
    args = ['spool-event-deliver', workspace, event['id'], 'batches', '--state', parent]
    run = call(*args)['plan']['context']['delivery']
    approved(*args)
    approved('spool-accept', receiver, run)
    revision = approved('spool-state-commit', receiver, parent, run)['revision']
    approved('spool-event-complete', workspace, event['id'], 'batches')
    approved('spool-reclaim', receiver, run)
    approved('spool-reclaim-snapshots', receiver, run)
    file = receiver.parent / 'audit-declaration.json'
    file.write_text(json.dumps({'schema': 'an-native-checkpoint-dependencies-v1', 'artifacts': [
        {'instance': str(producer), 'run': 'newer', 'output': 'batch.txt',
         'revision': batch['outputs'][0]['binding']['revision']}]}))
    pins = approved('spool-state-dependencies', receiver, revision, file)['dependencies']
    claims = call('spool-input-references', receiver, run)['claims']
    file.write_text(json.dumps({'schema': 'an-checkpoint-exhaustive-input-coverage-v1',
        'checkpoint': revision, 'declaration_sha256': pins['approval_sha256'],
        'inputs': [{'catalog': c['reference']['catalog'], 'revision': c['reference']['binding']['revision'],
                    'disposition': use} for c, use in zip(claims, ['checkpoint-pin', 'not-required'])]}))
    call('spool-input-release', receiver, run, '--delivery', workspace, 'batches', '--coverage', file,
         success=False)
    assert "operator disposition conflicts with accepted spool policy" in calls[-1]["stderr"]
    assert len(claims) == 2 and all(c['release'] is None for c in claims)
    assert call('spool-input-references', receiver, run)['claims'] == claims
    assert not (receiver / 'runs' / run / 'input-release-intent.json').exists()
