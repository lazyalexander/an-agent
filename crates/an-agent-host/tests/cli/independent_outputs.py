"""Exercise single-payload verification using two accepted products of one real attempt."""
from pathlib import Path


def qualify(call, approved, instance, name):
    query = ['spool-artifacts', instance, name]
    original = call(*query)
    state = call(*query, '--output', 'state.txt')
    report = call(*query, '--output', 'report.txt')
    assert state['payload_verified'] and report['inventory_metadata_verified']
    assert state['output'] in original['outputs'] and report['output'] in original['outputs']
    call(*query, '--output', 'unknown.txt', success=False)
    approved('spool-artifact-retire', instance, name, 'report.txt')
    baseline = call(*query)
    references = call(*query, '--references', 'report.txt')
    review = call(*query, '--review-retirement', 'report.txt')
    folder = Path(state['catalog']) / 'revisions' / state['output']['binding']['revision']
    content, manifest = folder / 'content.txt', folder / 'revision.json'
    withheld = folder / 'withheld.local'
    content.rename(withheld)
    try:
        # The target is still read and hashed. Do not confer any whole-run or deletion authority.
        assert call(*query, '--output', 'report.txt') == report
        assert call(*query, '--references', 'report.txt') == references
        assert call(*query, '--review-retirement', 'report.txt') == review
        assert not review['collection_authorized']
        call(*query, '--output', 'state.txt', success=False)
        call(*query, success=False)
        call('spool-artifact-retire', instance, name, 'report.txt', success=False)
        # A corrupt sibling manifest invalidates the inventory even for an intact target.
        metadata = manifest.read_bytes()
        try:
            manifest.write_bytes(b'{}')
            call(*query, '--output', 'report.txt', success=False)
        finally:
            manifest.write_bytes(metadata)
        data = withheld.read_bytes()
        content.write_bytes(bytes(b ^ 1 for b in data))
        call(*query, '--output', 'state.txt', success=False)
        assert call(*query, '--output', 'report.txt') == report
    finally:
        withheld.replace(content)
    assert call(*query) == baseline
