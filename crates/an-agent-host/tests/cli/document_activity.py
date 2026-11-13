#!/usr/bin/env python3
"""Real finite delegation and original custody for a synthetic PDF/MD/XLSX case.

Application adapters own PDF extraction and workbook authoring. The host retains
delegation, approval, activity evidence, native publication and artifact custody.
Requires pypdf in the controller environment and an explicitly selected builder.
"""
import argparse
import hashlib
import json
import os
from io import BytesIO
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
    parser.add_argument('--shuttle-client', type=Path)
    parser.add_argument('--app-server', action='store_true')
    args = parser.parse_args()
    require_native_workspace()
    host, pdf, codex, node, builder = [p.resolve(strict=True) for p in
                                     (args.host, args.pdf, args.codex, args.node, args.builder)]
    root = args.evidence.resolve()
    root.mkdir(mode=0o700)
    service = root / 'service'
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    jobs = []
    rejections = []
    parent_credential = None

    def call(*argv, success=True):
        result = subprocess.run([str(host), *map(str, argv)], capture_output=True,
                                text=True, timeout=30)
        assert (result.returncode == 0) == success, result.stderr
        return json.loads(result.stdout) if success else result.stderr

    def request(value, credential=None, success=True):
        path = root / 'request.json'
        path.write_text(json.dumps({'credential': credential, 'request': value} if credential else value))
        if credential and args.shuttle_client:
            script = '''import {readFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const {daemonTaskRequest} = await import(pathToFileURL(process.argv[1]).href);
const {credential, request} = JSON.parse(await readFile(process.argv[2], 'utf8'));
try { console.log(JSON.stringify(await daemonTaskRequest(credential, request))); }
catch (error) { console.error(error.message); process.exitCode = 1; }
'''
            result = subprocess.run([str(node), '--input-type=module', '-e', script,
                str(args.shuttle_client.resolve(strict=True)), str(path)], capture_output=True, text=True,
                timeout=40, env={**os.environ, 'AN_AGENT_DAEMON_ROOT': str(service), 'AN_AGENT_HOST_BIN': str(host)})
            assert (result.returncode == 0) == success, result.stderr
            return json.loads(result.stdout) if success else result.stderr
        return call('daemon', 'task-request' if credential else 'request', service, path, success=success)

    def approve(value, credential=None):
        preview = request(value, credential)
        return request({**value, 'approve': preview['approval_sha256']}, credential)

    def wait(task_id, credential, outcome='succeeded'):
        deadline = time.monotonic() + 200
        while time.monotonic() < deadline:
            view = request({'op': 'task', 'id': task_id}, credential)
            if view['state'] == 'settled':
                assert view['outcome'] == outcome, view
                if outcome == 'succeeded':
                    jobs.append(task_id)
                return
            time.sleep(.1)
        raise AssertionError('unknown task outcome; inspect original, never replay')

    def prepare(name):
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
        return instance

    def publish(name, source, credential):
        instance = instances[name]
        task_id = approve({'op': 'submit', 'instance': str(instance), 'run': 'deliver',
                           'input': str(source), 'seconds': 20}, credential)['id']
        wait(task_id, credential)
        approve({'op': 'accept', 'instance': str(instance), 'run': 'deliver'}, credential)
        received = request({'op': 'artifact-read', 'instance': str(instance), 'run': 'deliver', 'output': name}, credential)
        output = root / ('accepted-' + name)
        output.write_bytes(bytes(received['bytes']))
        assert sha(output) == sha(source)
        assert received['inventory']['output']['binding']['content_sha256'] == sha(output)
        return instance, output

    def selected(label, prompt, schema, scope):
        return {'app_server': args.app_server, 'label': label, 'binary': str(codex), 'binary_sha256': sha(codex),
                'prompt': prompt, 'schema': schema, 'scope': scope, 'model': None, 'mcp': None}

    def delegate(label, prompt, schema, scope, credential):
        value = {'op': 'delegate', 'parent': sha(pdf), 'task': {
            'app_server': args.app_server, 'label': label, 'binary': str(codex), 'binary_sha256': sha(codex),
            'prompt': prompt, 'schema': schema, 'scope': scope, 'model': None, 'mcp': None}}
        task_id = approve(value, credential)['id']
        wait(task_id, credential)
        inbox = request({'op': 'delegation-inbox', 'parent': sha(pdf)}, credential)
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
        markdown = root / 'document.md'
        markdown.touch()
        (root / 'workbook').mkdir()
        workbook = root / 'workbook/result.xlsx'
        workbook.touch()
        sources = {'source.pdf': pdf, 'document.md': markdown, 'result.xlsx': workbook}
        instances = {name: prepare(name) for name in sources}
        markdown_schema = {'type': 'object', 'properties': {'markdown': {'type': 'string'}},
                           'required': ['markdown'], 'additionalProperties': False}
        item = {'type': 'object', 'properties': {'item': {'type': 'string'},
                'quantity': {'type': 'integer'}, 'unit_price': {'type': 'number'}},
                'required': ['item', 'quantity', 'unit_price'], 'additionalProperties': False}
        rows_schema = {'type': 'object', 'properties': {'rows': {'type': 'array', 'items': item}},
                       'required': ['rows'], 'additionalProperties': False}
        pdf_scope = {'pdf_sha256': sha(pdf)}
        sheet_scope = {'pdf_sha256': sha(pdf), 'stage': 'markdown-to-sheet'}
        profiles = [request({'op': 'harness-permission', 'task': selected('document-agent', '', schema, scope)})
                    for schema, scope in [(markdown_schema, pdf_scope), (rows_schema, sheet_scope)]]
        permissions = {}
        for name, source in sources.items():
            resource = {'instance': str(instances[name]), 'run': 'deliver'}
            permissions[name] = [
                {'kind': 'run', **resource, 'input': str(source), 'seconds': 20},
                {'kind': 'accept', **resource},
                {'kind': 'artifact', **resource, 'output': name}]
        scope = {'task': sha(pdf), 'subject': 'document-shuttle', 'expires_ms': int(time.time()*1000)+900_000,
                 'delegate': True, 'permissions': profiles + sum(permissions.values(), [])}
        parent_credential = approve({'op': 'grant', 'scope': scope})['credential']
        def child(subject, rights):
            return approve({'op': 'derive-grant', 'scope': {**scope, 'subject': subject,
                'delegate': False, 'permissions': rights}}, parent_credential)['credential']
        first_credential = child('pdf-markdown-agent', [profiles[0], permissions['source.pdf'][2], *permissions['document.md']])
        second_credential = child('markdown-sheet-agent', [profiles[1], permissions['document.md'][2], *permissions['result.xlsx']])
        original, accepted_pdf = publish('source.pdf', pdf, parent_credential)
        pdf_data = request({'op': 'artifact-read', 'instance': str(original), 'run': 'deliver', 'output': 'source.pdf'}, first_credential)
        text = '\n'.join(p.extract_text() for p in PdfReader(BytesIO(bytes(pdf_data['bytes']))).pages)
        assert all(word in text for word in ('Bolt', 'Plate', 'Bearing'))
        first, proposal = delegate('pdf-markdown-agent',
            'Convert this synthetic PDF text into Markdown. Preserve every item, quantity and unit price. '
            'Use a pipe table with columns Item, Quantity, Unit price. Return only the schema.\n' + text,
            markdown_schema, pdf_scope, first_credential)
        markdown.write_text(proposal['markdown'])
        assert all(word in proposal['markdown'] for word in ('Bolt', 'Plate', 'Bearing', '|'))
        md_instance, accepted_md = publish('document.md', markdown, first_credential)
        approve({'op': 'record-adoption', 'id': first, 'instance': str(md_instance), 'run': 'deliver'}, first_credential)
        read_md = request({'op': 'artifact-read', 'instance': str(md_instance), 'run': 'deliver', 'output': 'document.md'}, second_credential)
        second, rows = delegate('markdown-sheet-agent',
            'Extract the table into typed rows, preserving item names, quantities and unit prices.\n' + bytes(read_md['bytes']).decode(),
            rows_schema, sheet_scope, second_credential)
        expected = [('Bolt', 12, .5), ('Plate', 3, 8), ('Bearing', 2, 4.25)]
        assert [(r['item'], r['quantity'], r['unit_price']) for r in rows['rows']] == expected
        rows['source_sha256'] = sha(accepted_md)
        data = root / 'rows.json'
        data.write_text(json.dumps(rows))
        # Application-owned mature authoring utility, never another agent driver.
        subprocess.run([str(node), str(builder), str(data), str(root / 'workbook')], check=True, timeout=90)
        sheet_instance, accepted_sheet = publish('result.xlsx', workbook, second_credential)
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
        approve({'op': 'record-adoption', 'id': second, 'instance': str(sheet_instance), 'run': 'deliver'}, second_credential)
        for name, value, credential in [
            ('PDF access removed from sheet agent', {'op': 'artifact-read', 'instance': str(original), 'run': 'deliver', 'output': 'source.pdf'}, second_credential),
            ('sibling result denied', {'op': 'harness-result', 'id': first}, second_credential),
            ('child escalation denied', {'op': 'derive-grant', 'scope': scope}, first_credential),
            ('operator controls denied', {'op': 'show'}, first_credential),
        ]:
            request(value, credential, success=False)
            rejections.append(name)
        cancelled_credential = child('cancelled-document-agent', [profiles[0]])
        cancelled = approve({'op': 'delegate', 'parent': sha(pdf), 'task': selected('cancelled-document-agent',
            'Return a short Markdown table for the cancelled synthetic test.', markdown_schema, pdf_scope)}, cancelled_credential)['id']
        approve({'op': 'cancel', 'id': cancelled}, parent_credential)
        wait(cancelled, cancelled_credential, outcome='cancelled')
        request({'op': 'harness-result', 'id': cancelled}, cancelled_credential, success=False)
        assert request({'op': 'grant-status'}, cancelled_credential)['active'] is False
        activities = [request({'op': 'activity', 'id': task_id}, parent_credential) for task_id in jobs]
        assert len(activities) == 5
        for task_id in (first, second):
            activity = next(v for v in activities if v['task']['id'] == task_id)
            assert activity['adoption']['plan']['grants_authority'] is False
            assert activity['harness']['visibility'] == 'reported-events-only'
            assert activity['task']['authorization']['active'] is True
        approve({'op': 'revoke-grant', 'id': parent_credential['id']}, parent_credential)
        request({'op': 'harness-result', 'id': second}, second_credential, success=False)
        assert call('spool-artifacts', sheet_instance, 'deliver')['outputs'][0]['binding']['content_sha256'] == sha(accepted_sheet)
        (root / 'activities.json').write_text(json.dumps(activities, indent=2))
        (root / 'report.json').write_text(json.dumps({'passed': True, 'real_codex': True,
            'host_sha256': sha(host), 'agents': [first, second], 'tasks': jobs,
            'expected_rows': expected, 'total_eur': 38.5, 'source_pdf_sha256': sha(pdf),
            'markdown_sha256': sha(accepted_md), 'xlsx_sha256': sha(accepted_sheet),
            'task_permissions': {'rejections': rejections, 'cancelled_task': cancelled,
                'cancel_revoked_authority': True, 'parent_revocation_reaches_children': True,
                'accepted_products_survive_revocation': True, 'shuttle_client': bool(args.shuttle_client)},
            'authoring': 'external application adapter; native host verifies publication and custody',
            'hidden_harness_actions': 'unknown', 'activity': 'retained source references, not a second ledger'}, indent=2))
    finally:
        if parent_credential and daemon.poll() is None:
            approve({'op': 'revoke-grant', 'id': parent_credential['id']}, parent_credential)
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            daemon.wait(timeout=10)


if __name__ == '__main__':
    main()
