#!/usr/bin/env python3
"""Run the frozen single-main benchmark through existing host acceptance entrypoints."""
import argparse
import copy
import hashlib
import importlib.metadata
import json
import platform
import shutil
import statistics
import subprocess
import sys
import time
import traceback
from pathlib import Path

import document_activity
import harness_benchmark
import mechanical_benchmark
from app_server_support import sha
from document_benchmark import make_pdf
from host_qualification import require_native_workspace


def usage_covered(metric):
    return bool(metric['available'] and metric['invalid_records'] == 0 and metric['total_tokens'] is not None)


def group_summary(trials):
    positive = [trial for trial in trials if trial['expected_outcome'] in ('deliver', 'clarify_then_deliver')]
    delivered = [trial for trial in positive if trial['status'] == 'passed']
    measured = [trial for trial in trials if trial['usage_covered']]
    positive_measured = [trial for trial in positive if trial['usage_covered']]
    tokens = {key: sum(trial['result']['metrics']['usage'][key] for trial in measured)
              if measured and all(trial['result']['metrics']['usage'][key] is not None for trial in measured) else None
              for key in ('input_tokens', 'cached_input_tokens', 'output_tokens', 'reasoning_output_tokens', 'total_tokens')}
    positive_tokens = sum(trial['result']['metrics']['usage']['total_tokens'] for trial in positive_measured)
    outcomes = [trial['result'].get('admission_to_delivery_ms', trial['result'].get('admission_to_outcome_ms'))
                for trial in trials if trial['status'] == 'passed']
    outcomes = [value for value in outcomes if value is not None]
    return {'trials': len(trials), 'passed': sum(t['status'] == 'passed' for t in trials),
        'failed': sum(t['status'] == 'failed' for t in trials), 'blocked': sum(t['status'] == 'blocked' for t in trials),
        'positive_trials': len(positive), 'delivered': len(delivered),
        'all_repetitions_passed': all(t['status'] == 'passed' for t in trials),
        'admission_to_outcome_ms': {'samples': len(outcomes), 'median': statistics.median(outcomes) if outcomes else None,
                                  'max': max(outcomes) if outcomes else None},
        'usage_coverage': f'{len(measured)}/{len(trials)}', 'measured_tokens': tokens,
        'positive_measured_total_tokens': positive_tokens,
        'control_measured_total_tokens': tokens['total_tokens'] - positive_tokens if measured else None,
        'failed_wall_ms': [t['wall_ms'] for t in trials if t['status'] == 'failed'],
        'tokens_per_accepted_delivery': positive_tokens / len(delivered)
            if delivered and len(positive_measured) == len(positive) else None,
        'cost_note': 'Provider input includes cache. Reasoning is included in output. Missing counters invalidate comparisons.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('host', 'codex', 'evidence', 'archive', 'spool', 'shuttle', 'mechanical-fixtures', 'mechanical-site'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--phase', choices=['smoke', 'full'], required=True)
    parser.add_argument('--model', default='gpt-6.1-sol')
    parser.add_argument('--case', action='append', dest='selected_cases')
    parser.add_argument('--token-budget', type=int, default=250000)
    args = parser.parse_args()
    require_native_workspace()
    args.evidence = args.evidence.resolve()
    args.evidence.mkdir(mode=0o700)
    args.archive = args.archive.resolve()
    args.archive.mkdir(mode=0o700)
    for name in ('host', 'codex', 'spool', 'shuttle', 'mechanical_fixtures', 'mechanical_site'):
        setattr(args, name, getattr(args, name).resolve(strict=True))
    spec_path = Path(__file__).resolve().parents[1] / 'benchmarks/single_agent_v1.json'
    spec = json.loads(spec_path.read_text())
    cases = [family['cases'][0] for family in spec['case_families']] if args.phase == 'smoke' else [
        case for family in spec['case_families'] for case in family['cases']]
    if args.selected_cases:
        cases = [case for family in spec['case_families'] for case in family['cases'] if case['id'] in args.selected_cases]
        assert {case['id'] for case in cases} == set(args.selected_cases)
    repeats = 1 if args.phase == 'smoke' else spec['protocol']['repetitions_per_case']
    repository = Path(__file__).resolve().parents[4]
    source_paths = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=repository).decode().split('\0')
    source_inventory = {name: sha(repository / name) for name in sorted(set(source_paths)) if name and (repository/name).is_file()}
    release_index = json.loads((args.spool / 'teaching-releases.json').read_text())
    release_paths = ['teaching-releases.json', 'spools/cad-paint/releases/an-loom-cad-paint-spool-0.1.0.tgz',
                     *[release['distribution'] for release in release_index['releases']]]
    frozen = {'schema': 'an-single-main-benchmark-run-v1', 'phase': args.phase, 'cases': [c['id'] for c in cases],
              'repetitions': repeats, 'spec_sha256': sha(spec_path), 'host_sha256': sha(args.host),
              'codex_sha256': sha(args.codex), 'model': args.model, 'source_files': source_inventory,
              'source_inventory_sha256': hashlib.sha256(json.dumps(source_inventory, sort_keys=True).encode()).hexdigest(),
              'source_commits': {name: subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=path).decode().strip()
                                 for name, path in [('host', repository), ('spool', args.spool), ('shuttle', args.shuttle)]},
              'release_files': {name: sha(args.spool / name) for name in release_paths},
              'application_manifests': {str(p): sha(p) for p in [Path('/Applications/Blender.app/Contents/Info.plist'),
                                                                Path('/Applications/FreeCAD.app/Contents/Info.plist')]},
              'platform': platform.platform(), 'machine': platform.machine(), 'concurrency': 1,
              'daemon_slots': {'document': 2, 'other_families': 3},
              'token_budget': args.token_budget, 'token_budget_enforcement': 'post-run comparison threshold; wall deadline enforced by host',
              'provider_cache_control': 'unknown', 'reasoning_setting': 'provider default, unchanged across trials',
              'python': sys.version, 'libraries': {name: importlib.metadata.version(name) for name in ('pypdf', 'XlsxWriter', 'numpy', 'Pillow')},
              'spool_source_sha256': {str(p.relative_to(args.spool)): sha(p) for p in sorted(args.spool.glob('spools/**/source/*')) if p.is_file()},
              'pack_sha256': sha(args.spool / 'examples/mechanical-animation/product/pack.py'),
              'mechanical_inputs': {str(p.relative_to(args.mechanical_fixtures)): sha(p)
                  for case in ('steam', 'inline4') for p in sorted((args.mechanical_fixtures / (case + '-inputs')).iterdir()) if p.is_file()},
              'deadlines_seconds': {'document': 600, 'control': 600, 'mechanical': 1800},
              'mechanical_site_sha256': mechanical_benchmark.site_digest(args.mechanical_site)}
    (args.evidence/'frozen.json').write_text(json.dumps(frozen, indent=2))
    shutil.copy2(args.evidence/'frozen.json', args.archive/'frozen.json')
    records = []
    started = time.monotonic()
    for repetition in range(1, repeats + 1):
        # Rotate case order while preserving a serial machine workload.
        ordered = cases[repetition - 1:] + cases[:repetition - 1]
        for case in ordered:
            index = len(records) + 1
            current = copy.copy(args)
            current.scenario = case['id']
            current.evidence = args.evidence / f'{index:02d}'
            current.node = current.builder = current.shuttle_client = None
            current.app_server = True
            start = time.monotonic()
            record = {'case': case['id'], 'repetition': repetition, 'expected_outcome': case['expected_outcome'],
                      'evidence': str(current.evidence), 'status': 'failed'}
            print(json.dumps({'event': 'started', 'case': case['id'], 'repetition': repetition}), flush=True)
            try:
                family = case['id'].split('.')[0]
                if family == 'mechanical_delivery':
                    result = mechanical_benchmark.run(current)
                elif family in ('background_control', 'work_shape', 'boundary_faults') or case['id'].endswith('missing_capability'):
                    result = harness_benchmark.run(current)
                else:
                    current.pdf = args.evidence / f'{index:02d}.pdf'
                    make_pdf(current.pdf, case['id'])
                    record['input_sha256'] = sha(current.pdf)
                    result = document_activity.qualify(current)
                harness_jobs = [path.parent.name for path in (current.evidence / 'service/jobs').glob('*/request.json')
                                if json.loads(path.read_text()).get('kind') == 'harness']
                assert len(harness_jobs) == 1 and harness_jobs == result['agents'], harness_jobs
                record['observed_harness_jobs'] = harness_jobs
                usage = result['metrics']['usage']
                within_budget = None if usage['total_tokens'] is None else usage['total_tokens'] <= args.token_budget
                record.update(status='passed' if result['passed'] and within_budget is not False else 'failed',
                              result=result, token_budget_met=within_budget,
                              usage_covered=usage_covered(usage))
            except Exception as error:
                current.evidence.mkdir(exist_ok=True)
                (current.evidence/'failure.txt').write_text(traceback.format_exc())
                record['error'] = str(error)
                if isinstance(error, mechanical_benchmark.CapabilityUnavailable):
                    record['status'] = 'blocked'
                record['usage_covered'] = False
                # Retained host records can still explain a failed trial's cost.
                jobs = current.evidence/'service/jobs'
                for path in jobs.glob('*/request.json'):
                    if json.loads(path.read_text()).get('kind') == 'harness':
                        record.setdefault('original_jobs', []).append(path.parent.name)
                record['metrics_pending_from_retained_records'] = bool(record.get('original_jobs'))
                retained = current.evidence/'retained-metrics.json'
                if retained.exists():
                    metrics = json.loads(retained.read_text())
                    if len(metrics) == 1 and 'usage' in next(iter(metrics.values())):
                        metric = next(iter(metrics.values()))
                        record['result'] = {'metrics': metric}
                        record['usage_covered'] = usage_covered(metric['usage'])
                        record['metrics_pending_from_retained_records'] = False
            record['wall_ms'] = round((time.monotonic()-start)*1000)
            records.append(record)
            (args.evidence/'trials.json').write_text(json.dumps(records, indent=2))
            shutil.copytree(current.evidence, args.archive/f'{index:02d}', symlinks=True,
                            ignore=shutil.ignore_patterns('client-home'))
            shutil.copy2(args.evidence/'trials.json', args.archive/'trials.json')
            print(json.dumps({'event': 'finished', 'case': case['id'], 'repetition': repetition,
                              'status': record['status'], 'wall_ms': record['wall_ms'],
                              'error': record.get('error')}), flush=True)
    families = {}
    for family in spec['case_families']:
        trials = [record for record in records if record['case'].startswith(family['id'] + '.')]
        if not trials:
            continue
        families[family['id']] = group_summary(trials)
    summary = {'phase': args.phase, 'trials': len(records), 'passed': sum(t['status']=='passed' for t in records),
               'elapsed_seconds': round(time.monotonic()-started, 1), 'families': families,
               'cases': {case['id']: group_summary([r for r in records if r['case'] == case['id']]) for case in cases},
               'overall': group_summary(records),
               'source_inventory_sha256': frozen['source_inventory_sha256'],
               'qualification': 'complete' if all(t['status']=='passed' for t in records) else 'incomplete'}
    (args.evidence/'summary.json').write_text(json.dumps(summary, indent=2))
    shutil.copy2(args.evidence/'summary.json', args.archive/'summary.json')
    print(json.dumps(summary), flush=True)


if __name__ == '__main__':
    main()

