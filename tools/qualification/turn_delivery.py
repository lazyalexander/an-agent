#!/usr/bin/env python3
"""Audit retained product document acceptance without launching or replaying work."""
import argparse
from decimal import Decimal
import hashlib
import json
from pathlib import Path
from xml.etree import ElementTree
from zipfile import ZipFile


def read(path):
    return json.loads(path.read_text())


def snapshot(value):
    body = dict(value)
    revision = body.pop('revision')
    encoded = json.dumps(body, sort_keys=True, ensure_ascii=False, separators=(',', ':')).encode()
    assert hashlib.sha256(encoded).hexdigest() == revision, 'Context digest mismatch'
    assert value['schema'] == 'an-turn-context-v1'
    assert value['limits']['reset'] is False
    return revision


def workbook(path, quantity):
    ns = {'s': 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
    with ZipFile(path) as archive:
        sheet = ElementTree.fromstring(archive.read('xl/worksheets/sheet1.xml'))
        cells = {cell.attrib['r']: cell for cell in sheet.findall('.//s:sheetData/s:row/s:c', ns)}
        strings = []
        if 'xl/sharedStrings.xml' in archive.namelist():
            strings = [''.join(node.itertext()) for node in
                       ElementTree.fromstring(archive.read('xl/sharedStrings.xml'))]
        def value(address):
            cell = cells[address]
            if cell.get('t') == 'inlineStr':
                return ''.join(cell.find('s:is', ns).itertext())
            text = cell.findtext('s:v', namespaces=ns)
            return strings[int(text)] if cell.get('t') == 's' else text
        expected = [('Bolt', quantity, '0.50'), ('Plate', 3, '8.00'), ('Bearing', 2, '4.25')]
        for row, (item, count, price) in enumerate(expected, 2):
            assert value(f'A{row}') == item
            assert Decimal(value(f'B{row}')) == count
            assert Decimal(value(f'C{row}')) == Decimal(price)
            assert Decimal(value(f'D{row}')) == count * Decimal(price)
            assert cells[f'D{row}'].findtext('s:f', namespaces=ns) == f'B{row}*C{row}'
        assert cells['D5'].findtext('s:f', namespaces=ns) == 'SUM(D2:D4)'
        assert Decimal(value('D5')) == Decimal(quantity) / 2 + Decimal('32.50')


def audit(service, evidence, require_summary=False):
    report = read(evidence / 'report.json')
    assert report['passed'] and report['real_model'] and not report['cancelled']
    state = read(service / 'application/tasks' / report['task'] / 'state.json')
    assert state['state'] == 'succeeded'
    assert state['termination_confirmed'] and state['runtime_termination_confirmed']
    job = service / 'jobs' / state['session']
    session = read(job / 'session.json')
    contexts = [read(job / 'context-initial.json')]
    commands = sorted((read(p) for p in (job / 'commands').glob('*.json')), key=lambda v: v['sequence'])
    contexts.extend(command['context'] for command in commands)
    previous = None
    for context in contexts:
        assert context['parent'] == previous, 'Broken context ancestry'
        assert context['execution'] == session['execution']
        assert context['task'] == state['session']
        previous = snapshot(context)
    for command in commands:
        ack = read(job / ('ack-' + command['plan']['command'] + '.json'))
        assert ack['state'] == 'acknowledged'
    results = [read(p) for p in job.glob('turn-result-*.json')]
    assert len(results) == (3 if report['clarification'] else 2)
    assert {result['thread'] for result in results} == {session['thread']}
    assert all(result['artifact_accepted'] is False for result in results)
    accepted = read(job / 'commands/accepted-intermediate.json')['context']
    intermediate = next(b['instance'] for b in contexts[0]['spool_bindings'] if b['name'] == 'intermediate')
    assert any(item['instance'] == intermediate and item['output'] == 'markdown.md' and item['artifact_accepted']
               for item in accepted['resources']['accepted_artifacts'])
    if require_summary:
        for command in commands:
            if command['plan']['operation'] == 'continue-harness':
                summary = command['context']['summary']
                assert summary and summary['source'] == 'model-proposal', 'Missing model summary'
                assert summary['context_revision'] == command['context']['parent']
                assert summary['artifact_accepted'] is False and summary['grants_authority'] is False
        proposals = [read(path) for path in (job / 'context-proposals').glob('*.json')]
        assert {p['turn'] for p in proposals} == {r['number'] for r in results}
        assert any(p['context_revision'] == contexts[-1]['revision'] for p in proposals)
    bindings = {b['name']: b['grant_ref']['id'] for b in contexts[0]['spool_bindings']}
    assert len(set(bindings.values())) == 2
    receipts = [json.loads(line) for line in (job / 'tool-receipts.jsonl').read_text().splitlines()]
    primary = {receipt['grant'] for receipt in receipts}
    assert len(primary) == 1 and not primary.intersection(bindings.values())
    revisions = {context['revision'] for context in contexts}
    assert all(receipt['context_revision'] in revisions for receipt in receipts)
    for name, grant in bindings.items():
        assert any(r['succeeded'] and r['effective_grant'] == grant for r in receipts)
        plan = read(Path(state['instances'][name]) / 'runs/deliver/supervisor/request.json')['plan']
        assert plan['task_authorization']['id'] in primary
        assert {guard['id'] for guard in plan['authority_guards']} == {grant}
    for name, digest in report['products'].items():
        assert hashlib.sha256((evidence / name).read_bytes()).hexdigest() == digest
    workbook(evidence / 'result.xlsx', 13 if report.get('source_revision') else 12)
    return {'passed': True, 'single_thread': True, 'context_chain': len(contexts),
            'separate_spool_grants': True, 'dual_supervisor_guards': True,
            'exact_workbook_rows': True, 'summary_required': require_summary,
            'summary_present': bool(accepted['summary']), 'host_sha256': report['host_sha256']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--service', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    parser.add_argument('--require-summary', action='store_true')
    args = parser.parse_args()
    result = audit(args.service.resolve(strict=True), args.evidence.resolve(strict=True), args.require_summary)
    (args.evidence / 'turn-audit.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result))


if __name__ == '__main__':
    main()
