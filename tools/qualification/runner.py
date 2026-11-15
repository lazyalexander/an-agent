"""One background qualification owner; claimed runs are never replayed."""
import os
from pathlib import Path
import subprocess
import sys
import time
from candidate import verify
from store import lock, read, write
from process import identity


def execute(root, record, path):
    candidate, manifest = verify(root, record['candidate'], record['manifest_sha256'])
    record.update(state='running', started=time.time(), owner=identity(os.getpid()))
    write(path, record)  # Durable claim precedes spawn.
    run = root / 'runs' / record['id']
    run.mkdir(mode=0o700)
    phases = ['smoke', 'full'] if record['phase'] == 'full' else [record['phase']]
    environment = dict(os.environ, PYTHONDONTWRITEBYTECODE='1',
                       PYTHONPATH=str(candidate / 'python_site') + ':' + str(candidate / 'mechanical_site'))
    for phase in phases:
        verify(root, record['candidate'], record['manifest_sha256'])
        record['active_phase'] = phase
        write(path, record)
        command = [sys.executable, '-B', str(candidate / 'host/crates/an-agent-host/tests/cli/single_agent_benchmark.py'),
                   '--host', str(candidate / 'host_binary'), '--codex', str(candidate / 'codex'),
                   '--spool', str(candidate / 'spool'), '--shuttle', str(candidate / 'shuttle'),
                   '--mechanical-fixtures', str(candidate / 'mechanical_fixtures'),
                   '--mechanical-site', str(candidate / 'mechanical_site'), '--model', manifest['model'],
                   '--phase', phase, '--evidence', str(run / phase), '--archive', str(run / (phase + '-archive'))]
        for case in record['cases']:
            command += ['--case', case]
        with (run / (phase + '.log')).open('wb') as output:
            process = subprocess.Popen(command, env=environment, stdout=output, stderr=output)
            record['child_pid'] = process.pid
            record['child_identity'] = identity(process.pid)
            write(path, record)
            code = process.wait()
        report = run / phase / 'summary.json'
        if code or not report.exists():
            record.update(state='unknown', reason='Driver exited without a verified complete report')
            write(path, record)
            return
        summary = read(report)
        record.setdefault('reports', {})[phase] = summary
        if summary['overall']['failed'] or summary['overall']['blocked']:
            record.update(state='completed', qualification='incomplete', finished=time.time())
            write(path, record)
            return
        if (root / 'stop.json').exists() and phase != phases[-1]:
            record.update(state='completed', qualification='incomplete', reason='Stopped before next phase')
            write(path, record)
            return
    record.update(state='completed', qualification='passed', finished=time.time())
    write(path, record)


def serve(root):
    with lock(root, 'worker'):
        write(root / 'worker.json', {'pid': os.getpid(), 'started': time.time()})
        while not (root / 'stop.json').exists():
            with lock(root):
                paths = sorted((root / 'queue').glob('*.json'))
                records = [(p, read(p)) for p in paths]
                if any(r['state'] in ('running', 'unknown') for _, r in records):
                    return  # Original owner must be reconciled explicitly.
                selected = next(((p, r) for p, r in records if r['state'] == 'queued'), None)
                if selected:
                    path, record = selected
                    record['state'] = 'running'
                    write(path, record)
            if not selected:
                time.sleep(1)
                continue
            try:
                execute(root, record, path)
            except Exception as error:
                record.update(state='unknown', reason=str(error))
                write(path, record)
                return
