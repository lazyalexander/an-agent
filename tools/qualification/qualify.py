#!/usr/bin/env python3
"""Developer-only frozen qualification queue. No runtime execution authority."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import time
import uuid
from candidate import freeze, verify
from store import lock, private_root, read, write


def status(root):
    rows = [read(p) for p in sorted((root / 'queue').glob('*.json'))]
    for row in rows:
        trials = root / 'runs' / row['id'] / row.get('active_phase', 'smoke') / 'trials.json'
        if trials.exists():
            try:
                data = read(trials)
                row['counts'] = {key: sum(r['status'] == key for r in data) for key in ('passed', 'failed', 'blocked')}
            except json.JSONDecodeError:
                row['observation'] = 'Report update in progress'
    return {'stop_requested': (root / 'stop.json').exists(), 'runs': rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    sub = parser.add_subparsers(dest='op', required=True)
    p = sub.add_parser('freeze'); p.add_argument('config', type=Path)
    p = sub.add_parser('enqueue'); p.add_argument('candidate'); p.add_argument('digest')
    p.add_argument('--phase', choices=('smoke', 'full'), default='smoke')
    p.add_argument('--case', action='append', default=[])
    p = sub.add_parser('reconcile'); p.add_argument('id')
    for name in ('start', 'worker', 'status', 'stop', 'dashboard'):
        sub.add_parser(name)
    args = parser.parse_args(); root = private_root(args.root)
    (root / 'queue').mkdir(mode=0o700, exist_ok=True)
    (root / 'runs').mkdir(mode=0o700, exist_ok=True)
    if args.op == 'freeze':
        result = freeze(root, args.config)
    elif args.op == 'enqueue':
        verify(root, args.candidate, args.digest)
        identity = uuid.uuid4().hex
        result = {'id': identity, 'candidate': args.candidate, 'manifest_sha256': args.digest,
                  'phase': args.phase, 'cases': args.case, 'state': 'queued', 'submitted': time.time()}
        with lock(root):
            write(root / 'queue' / (str(time.time_ns()) + '-' + identity + '.json'), result)
    elif args.op == 'start':
        with lock(root, 'worker'):
            (root / 'stop.json').unlink(missing_ok=True)
        with (root / 'worker.log').open('ab') as output:
            p = subprocess.Popen([sys.executable, '-B', str(Path(__file__).resolve()), '--root', str(root), 'worker'],
                                 stdin=subprocess.DEVNULL, stdout=output, stderr=output, start_new_session=True)
        result = {'worker_pid': p.pid}
    elif args.op == 'worker':
        from runner import serve
        serve(root); return
    elif args.op == 'stop':
        write(root / 'stop.json', {'requested': time.time()})
        result = {'stop_requested': True, 'current_case_interrupted': False}
    elif args.op == 'reconcile':
        from reconcile import reconcile
        result = reconcile(root, args.id)
    elif args.op == 'dashboard':
        from dashboard import serve
        serve(root); return
    else:
        result = status(root)
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
