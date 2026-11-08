#!/usr/bin/env python3
"""Real finite delegation and original custody for a synthetic PDF/MD/XLSX case.

Application adapters own PDF extraction and workbook authoring. The host retains
delegation, approval, activity evidence, native publication and artifact custody.
Requires pypdf in the controller environment and an explicitly selected builder.
"""
import argparse
import hashlib
import json
from pathlib import Path
import signal
import subprocess
import time
from zipfile import ZipFile
from xml.etree import ElementTree

from pypdf import PdfReader
from host_qualification import require_native_workspace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('host', 'evidence', 'pdf', 'codex', 'node', 'builder'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    require_native_workspace()
    host, pdf, codex, node, builder = [p.resolve(strict=True) for p in
                                     (args.host, args.pdf, args.codex, args.node, args.builder)]
    root = args.evidence.resolve()
    root.mkdir(mode=0o700)
    service = root / 'service'
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    jobs = []

    def call(*argv):
        result = subprocess.run([str(host), *map(str, argv)], capture_output=True,
                                text=True, timeout=30)
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)

    def request(value):
        path = root / 'request.json'
        path.write_text(json.dumps(value))
        return call('daemon', 'request', service, path)

    def approve(value):
        preview = request(value)
        return request({**value, 'approve': preview['approval_sha256']})

    def wait(task_id):
        deadline = time.monotonic() + 200
        while time.monotonic() < deadline:
            view = request({'op': 'task', 'id': task_id})
            if view['state'] == 'settled':
                assert view['outcome'] == 'succeeded', view
                jobs.append(task_id)
                return
            time.sleep(.1)
        raise AssertionError('unknown task outcome; inspect original, never replay')

    def publish(name, source):
        package = root / ('package-' + name)
        (package / 'silk').mkdir(parents=True)
        code = 'set -eu\ncat "$AN_SPOOL_INPUT" > ' + name + '\n'
        (package / 'silk/main.sh').write_text(code)
        (package / 'native-spool.json').write_text(json.dumps({
            'schema': 'an-native-spool-v3', 'name': 'document-delivery', 'version': '0.1.0',
            'events': {'consumes': [], 'produces': ['artifact.accepted']},
            'entry': 'silk/main.sh', 'files': {'silk/main.sh': hashlib.sha256(code.encode()).hexdigest()},
            'outputs': [name], 'binary_outputs': [name] if name.endswith(('.pdf', '.xlsx')) else [], 'requirements': {'runtime': 'macos-native-process-v1', 'os': 'macos',
            'arch': 'portable-source', 'interaction': 'headless', 'gpu': 'none',
            'inputs': ['input'], 'editable': [], 'dependencies': [],
            'capabilities': ['read-input', 'write-declared-outputs']}}))
        instance = root / ('instance-' + name)
        call('spool-load', package, instance)
        request({'op': 'register', 'instance': str(instance)})
        task_id = approve({'op': 'submit', 'instance': str(instance), 'run': 'deliver',
                           'input': str(source), 'seconds': 20})['id']
        wait(task_id)
        approve({'op': 'accept', 'instance': str(instance), 'run': 'deliver'})
        custody = call('spool-artifacts', instance, 'deliver')
        output = instance / 'runs/deliver/job/work' / name
        assert sha(output) == sha(source)
        assert custody['outputs'][0]['binding']['content_sha256'] == sha(output)
        return instance, output

    def delegate(label, prompt, schema, scope):
        value = {'op': 'delegate', 'parent': sha(pdf), 'task': {
            'label': label, 'binary': str(codex), 'binary_sha256': sha(codex),
            'prompt': prompt, 'schema': schema, 'scope': scope, 'model': None, 'mcp': None}}
        task_id = approve(value)['id']
        wait(task_id)
        inbox = request({'op': 'delegation-inbox', 'parent': sha(pdf)})
        message = next(v for v in inbox['messages'] if v['id'] == task_id)
        return task_id, message['proposal']

    call('daemon', 'init', service, 2)
    with (root / 'service.log').open('w') as log:
        daemon = subprocess.Popen([str(host), 'daemon', 'serve', str(service)], stdout=log, stderr=log)
    try:
        for _ in range(200):
            if (service / 'endpoint.json').exists():
                break
            assert daemon.poll() is None, 'service failed'
            time.sleep(.05)
        original, accepted_pdf = publish('source.pdf', pdf)
        text = '\n'.join(p.extract_text() for p in PdfReader(accepted_pdf).pages)
        assert all(word in text for word in ('Bolt', 'Plate', 'Bearing'))
        first, proposal = delegate('pdf-markdown-agent',
            'Convert this synthetic PDF text into Markdown. Preserve every item, quantity and unit price. '
            'Use a pipe table with columns Item, Quantity, Unit price. Return only the schema.\n' + text,
            {'type': 'object', 'properties': {'markdown': {'type': 'string'}},
             'required': ['markdown'], 'additionalProperties': False}, {'pdf_sha256': sha(pdf)})
        markdown = root / 'document.md'
        markdown.write_text(proposal['markdown'])
        assert all(word in proposal['markdown'] for word in ('Bolt', 'Plate', 'Bearing', '|'))
        md_instance, accepted_md = publish('document.md', markdown)
        approve({'op': 'record-adoption', 'id': first, 'instance': str(md_instance), 'run': 'deliver'})
        item = {'type': 'object', 'properties': {'item': {'type': 'string'},
                'quantity': {'type': 'integer'}, 'unit_price': {'type': 'number'}},
                'required': ['item', 'quantity', 'unit_price'], 'additionalProperties': False}
        second, rows = delegate('markdown-sheet-agent',
            'Extract the table into typed rows, preserving item names, quantities and unit prices.\n' + accepted_md.read_text(),
            {'type': 'object', 'properties': {'rows': {'type': 'array', 'items': item}},
             'required': ['rows'], 'additionalProperties': False}, {'markdown_sha256': sha(accepted_md)})
        expected = [('Bolt', 12, .5), ('Plate', 3, 8), ('Bearing', 2, 4.25)]
        assert [(r['item'], r['quantity'], r['unit_price']) for r in rows['rows']] == expected
        rows['source_sha256'] = sha(accepted_md)
        data = root / 'rows.json'
        data.write_text(json.dumps(rows))
        # Application-owned mature authoring utility, never another agent driver.
        subprocess.run([str(node), str(builder), str(data), str(root / 'workbook')], check=True, timeout=90)
        sheet_instance, accepted_sheet = publish('result.xlsx', root / 'workbook/result.xlsx')
        # Independently inspect exported OOXML, not the builder's own assertions.
        with ZipFile(accepted_sheet) as archive:
            ns = {'m': 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
            strings = []
            if 'xl/sharedStrings.xml' in archive.namelist():
                tree = ElementTree.fromstring(archive.read('xl/sharedStrings.xml'))
                strings = [''.join(v.itertext()) for v in tree.findall('m:si', ns)]
            tree = ElementTree.fromstring(archive.read('xl/worksheets/sheet1.xml'))
            cells = {v.get('r'): v for v in tree.findall('.//m:c', ns)}
            def value(address):
                cell = cells[address]
                assert cell.get('t') != 'e', address
                raw = cell.findtext('m:v', namespaces=ns)
                if cell.get('t') == 's':
                    return strings[int(raw)]
                if cell.get('t') == 'str':
                    return raw
                if cell.get('t') == 'inlineStr':
                    return ''.join(cell.find('m:is', ns).itertext())
                return float(raw)
            for row, (name, quantity, price) in enumerate(expected, 6):
                assert (value('A' + str(row)), value('B' + str(row)), value('C' + str(row))) == (name, quantity, price)
                assert value('D' + str(row)) == quantity * price
            assert value('D10') == 38.5
            assert cells['D10'].findtext('m:f', namespaces=ns).lstrip('=') == 'SUM(D6:D8)'
        approve({'op': 'record-adoption', 'id': second, 'instance': str(sheet_instance), 'run': 'deliver'})
        activities = [request({'op': 'activity', 'id': task_id}) for task_id in jobs]
        assert len(activities) == 5
        for task_id in (first, second):
            activity = next(v for v in activities if v['task']['id'] == task_id)
            assert activity['adoption']['plan']['grants_authority'] is False
            assert activity['harness']['visibility'] == 'reported-events-only'
        (root / 'activities.json').write_text(json.dumps(activities, indent=2))
        (root / 'report.json').write_text(json.dumps({'passed': True, 'real_codex': True,
            'host_sha256': sha(host), 'agents': [first, second], 'tasks': jobs,
            'expected_rows': expected, 'total_eur': 38.5, 'source_pdf_sha256': sha(pdf),
            'markdown_sha256': sha(accepted_md), 'xlsx_sha256': sha(accepted_sheet),
            'authoring': 'external application adapter; native host verifies publication and custody',
            'hidden_harness_actions': 'unknown', 'activity': 'retained source references, not a second ledger'}, indent=2))
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            daemon.wait(timeout=10)


if __name__ == '__main__':
    main()
