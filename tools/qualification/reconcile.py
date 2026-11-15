"""Read original receipts only; never restart claimed qualification work."""
from process import exited
from store import lock, read, write


def reconcile(root, identity):
    with lock(root, 'worker'), lock(root):
        matches = [(p, read(p)) for p in (root / 'queue').glob('*.json')]
        matches = [(p, r) for p, r in matches if r['id'] == identity]
        if len(matches) != 1:
            raise ValueError('Original queued identity not found')
        path, record = matches[0]
        if record['state'] not in ('running', 'unknown'):
            return record
        record.update(state='unknown', reason='Original evidence incomplete; no replay permitted')
        owners = [record.get('owner'), record.get('child_identity')]
        if not all(owners) or not all(exited(owner) for owner in owners):
            write(path, record); return record
        run = root / 'runs' / identity
        phases = ['smoke', 'full'] if record['phase'] == 'full' else [record['phase']]
        summaries = {}
        for phase in phases:
            directory = run / phase
            report = directory / 'summary.json'
            if not report.exists():
                write(path, record); return record
            summaries[phase] = read(report)
            for request in directory.glob('*/service/jobs/*/request.json'):
                completion = request.parent / 'completion.json'
                if not completion.exists():
                    write(path, record); return record
                result = read(completion)['result']
                if read(request).get('kind') == 'harness' and not result.get('termination_confirmed'):
                    write(path, record); return record
            for cleanup in directory.glob('*/cleanup.json'):
                if read(cleanup).get('unsettled'):
                    write(path, record); return record
        record.update(state='completed', reports=summaries, reconciled=True,
                      qualification='passed' if all(v['qualification'] == 'complete' for v in summaries.values()) else 'incomplete')
        record.pop('reason', None)
        write(path, record)
        return record
