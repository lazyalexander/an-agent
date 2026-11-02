"""Checkpoint pin qualification shared by the real supervised-state CLI cases."""
import json


def declare(call, approved, source, report, instance, revision):
    request = instance.parent / (instance.name + '-dependencies.json')
    artifact = {'instance': str(source), 'run': 'baseline', 'output': 'report.txt',
                'revision': report['outputs'][0]['binding']['revision']}
    document = {'schema': 'an-native-checkpoint-dependencies-v1', 'artifacts': [artifact]}
    request.write_text(json.dumps(document))
    command = ('spool-state-dependencies', instance, revision, request)
    preview = call(*command)
    document['artifacts'][0]['revision'] = '0' * 64
    request.write_text(json.dumps(document))
    call(*command, '--approve', preview['approval_sha256'], success=False)
    assert not (instance / 'checkpoints/dependencies' / revision).exists()
    document['artifacts'][0]['revision'] = report['outputs'][0]['binding']['revision']
    document['artifacts'].append(dict(document['artifacts'][0]))
    request.write_text(json.dumps(document))
    call(*command, success=False)
    document['artifacts'].pop()
    request.write_text(json.dumps(document))
    accepted = approved(*command)
    assert accepted['dependencies']['status'] == 'registered'
    call(*command, success=False)  # A declaration is immutable, not an execution retry.
    return request


def retained(call, source, instance, revision):
    state = call('spool-state-show', instance, revision)
    dependencies = state['dependencies']
    assert dependencies['status'] == 'registered'
    assert not dependencies['collection_authorized']
    claim, = dependencies['claims']
    assert claim['release'] is None
    assert claim['reference']['owner']['checkpoint_revision'] == revision
    observation = call('spool-artifacts', source, 'baseline', '--references', 'report.txt')
    # The producer view may also contain independent attempt inputs.
    assert any(item['reference'] == claim['reference'] and item['release'] is None
               for item in observation['claims'])
    assert not observation['collection_authorized']


def incomplete(call, instance, revision, request, operation, selected):
    receipt = instance / 'checkpoints/dependencies' / revision / 'complete.json'
    original = receipt.read_bytes()
    receipt.unlink()  # Disposable fixture: simulate lost registration completion.
    call('spool-state-show', instance, revision, success=False)
    call('spool-state-dependencies', instance, revision, request, success=False)
    call('spool-' + operation, instance, 'unconfirmed-dependency', selected, 15, success=False)
    assert not (instance / 'runs/unconfirmed-dependency/job').exists()
    # Restore fixture evidence only; no product repair or implicit retry exists.
    receipt.write_bytes(original)
