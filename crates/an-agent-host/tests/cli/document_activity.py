#!/usr/bin/env python3
"""One original main session and original custody for a synthetic PDF/MD/XLSX case.

Application adapters own PDF extraction and workbook authoring. The host retains
delegation, approval, activity evidence, native publication and artifact custody.
Requires pypdf and either XlsxWriter or an explicitly selected builder.
"""
import argparse
import hashlib
import json
import os
import re
from io import BytesIO
from pathlib import Path
import signal
import subprocess
import time
from zipfile import ZipFile
from xml.etree import ElementTree

from pypdf import PdfReader
from host_qualification import require_native_workspace
from document_benchmark import knowledge, write_workbook, expected_rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('host', 'evidence', 'pdf', 'codex'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--node', type=Path)
    parser.add_argument('--builder', type=Path)
    parser.add_argument('--scenario', default='document_delivery.table_exact')
    parser.add_argument('--model', default='gpt-6.1-sol')
    parser.add_argument('--shuttle-client', type=Path)
    parser.add_argument('--app-server', action='store_true')
    qualify(parser.parse_args())


def qualify(args):
    require_native_workspace()
    started = time.monotonic()
    host, pdf, codex = [p.resolve(strict=True) for p in (args.host, args.pdf, args.codex)]
    node = args.node.resolve(strict=True) if args.node else None
    builder = args.builder.resolve(strict=True) if args.builder else None
    if (builder or args.shuttle_client) and node is None:
        raise ValueError('--node is required for the selected application adapter')
    root = args.evidence.resolve()
    root.mkdir(mode=0o700)
    service = root / 'service'
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    jobs = []
    phases = {}
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
        deadline = time.monotonic() + 600
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
            'schema': 'an-native-spool-v3', 'name': materials['selected'], 'version': '0.1.0',
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

    def turn_result(task_id, number, credential):
        deadline = time.monotonic() + 600
        marker = service / 'jobs' / task_id / f'turn-result-{number}.json'
        while not marker.exists():
            view = request({'op': 'task', 'id': task_id}, credential)
            if view['state'] == 'settled':
                raise AssertionError(('main session ended before its next turn', view))
            if time.monotonic() >= deadline:
                raise AssertionError('original turn deadline exceeded; retain evidence')
            time.sleep(.1)
        return request({'op': 'harness-turn-result', 'id': task_id, 'turn': number}, credential)

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
        materials = knowledge(root, args.scenario)
        instances = {name: prepare(name) for name in sources}
        item = {'type': 'object', 'properties': {'item': {'type': 'string'},
                'quantity': {'type': 'integer'}, 'unit_price': {'type': 'number'}},
                'required': ['item', 'quantity', 'unit_price'], 'additionalProperties': False}
        schema = {'type': 'object', 'properties': {'markdown': {'type': 'string'},
                  'rows': {'type': 'array', 'items': item}, 'selected': {'type': 'string'}},
                  'required': ['markdown', 'rows', 'selected'], 'additionalProperties': False}
        ambiguous = args.scenario.endswith('table_ambiguous')
        turn_limit = 3 if ambiguous else 2
        selected = {'adapter': 'codex-app-server', 'turn_limit': turn_limit, 'wall_seconds': 600,
                    'label': 'document-main', 'binary': str(codex), 'binary_sha256': sha(codex),
                    'prompt': '', 'schema': schema, 'scope': {'pdf_sha256': sha(pdf), 'case': args.scenario},
                    'model': args.model, 'mcp': None}
        profile = request({'op': 'harness-permission', 'task': selected})
        permissions = {}
        for name, source in sources.items():
            resource = {'instance': str(instances[name]), 'run': 'deliver'}
            permissions[name] = [
                {'kind': 'run', **resource, 'input': str(source), 'seconds': 20},
                {'kind': 'accept', **resource},
                {'kind': 'artifact', **resource, 'output': name}]
        scope = {'task': sha(pdf), 'subject': 'document-shuttle', 'expires_ms': int(time.time()*1000)+900_000,
                 'delegate': True, 'permissions': [profile, *materials['permissions']] + sum(permissions.values(), [])}
        parent_credential = approve({'op': 'grant', 'scope': scope})['credential']
        def child(subject, rights):
            return approve({'op': 'derive-grant', 'scope': {**scope, 'subject': subject,
                'delegate': False, 'permissions': rights}}, parent_credential)['credential']
        main_credential = child('document-main', scope['permissions'])
        observer = child('unrelated-observer', [])
        admitted = time.monotonic()
        original, accepted_pdf = publish('source.pdf', pdf, parent_credential)
        pdf_data = request({'op': 'artifact-read', 'instance': str(original), 'run': 'deliver', 'output': 'source.pdf'}, main_credential)
        text = '\n'.join(p.extract_text() for p in PdfReader(BytesIO(bytes(pdf_data['bytes']))).pages)
        first_prompt = (
            'Convert the supplied PDF text to Markdown. Discover the appropriate document capability '
            'by reading the index and its matching contract and skill through host_request. '
            'read-input supports encoding="utf-8" and requires the listed path and sha256. '
            'Preserve every item, quantity and price in a pipe table with columns Item, Quantity, Unit price. '
            'Apply the selected skill unit rules to produce EUR values. If facts are missing, return questions in markdown '
            'with empty rows and wait for clarification before producing the table. '
            'Return markdown, rows=[] and selected equal to the chosen capability ID. '
            'The second turn will provide the accepted Markdown for typed extraction.\nIndex request: '
            + json.dumps(materials['index']) + '\nPDF text:\n' + text)
        main_id = approve({'op': 'harness', 'task': dict(selected, prompt=first_prompt)}, main_credential)['id']
        first = turn_result(main_id, 1, main_credential)
        turns = [first['turn']]
        connect = request({'op': 'harness-connect', 'id': main_id}, main_credential)
        owner = (service / 'jobs' / main_id / 'owner.json').read_bytes()
        proposal = first['proposal']
        assert proposal['selected'] == materials['selected'], proposal
        markdown_turn = 1
        if ambiguous:
            assert '?' in proposal['markdown'] and not proposal['rows'], proposal
            approve({'op': 'harness-continue', 'id': main_id, 'execution': connect['execution'],
                     'after_turn': 1, 'command': 'clarification',
                     'instruction': 'The currency is EUR and the Bolt quantity is 12. Produce the requested Markdown now.'}, main_credential)
            clarified = turn_result(main_id, 2, main_credential)
            assert clarified['thread'] == first['thread']
            turns.append(clarified['turn'])
            proposal = clarified['proposal']
            markdown_turn = 2
        markdown.write_text(proposal['markdown'])
        assert all(word in proposal['markdown'] for word in ('Bolt', 'Plate', 'Bearing', '|'))
        expected = expected_rows(args.scenario)
        markdown_rows = []
        for line in proposal['markdown'].splitlines():
            cells = [cell.strip().strip('*') for cell in line.strip().strip('|').split('|')]
            if cells and cells[0] in [row[0] for row in expected]:
                assert len(cells) >= 3, cells
                numeric = lambda cell: float(re.sub(r'\s|EUR|€', '', cell))
                markdown_rows.append((cells[0], numeric(cells[1]), numeric(cells[2])))
        assert markdown_rows == expected, markdown_rows
        phases['markdown_ready_ms'] = round((time.monotonic() - started) * 1000)
        md_instance, accepted_md = publish('document.md', markdown, main_credential)
        read_md = {'op': 'artifact-read', 'instance': str(md_instance), 'run': 'deliver',
                   'output': 'document.md', 'encoding': 'utf-8'}
        instruction = ('Read the accepted Markdown using host_request, then extract its table into typed rows. '
                       'Use the workbook skill. Return markdown="", rows with all original values, and '
                       'the same selected capability ID. Markdown request: ' + json.dumps(read_md)
                       + '\nWorkbook skill request: ' + json.dumps(materials['sheet']))
        approve({'op': 'harness-continue', 'id': main_id, 'execution': connect['execution'],
                 'after_turn': markdown_turn, 'command': 'extract-accepted-markdown', 'instruction': instruction}, main_credential)
        wait(main_id, main_credential)
        final = request({'op': 'harness-result', 'id': main_id}, main_credential)
        second = request({'op': 'harness-turn-result', 'id': main_id, 'turn': turn_limit}, main_credential)
        turns.append(second['turn'])
        assert first['thread'] == second['thread'] and first['turn'] != second['turn']
        assert owner == (service / 'jobs' / main_id / 'owner.json').read_bytes()
        rows = final['proposal']
        assert [(r['item'], r['quantity'], r['unit_price']) for r in rows['rows']] == expected
        rows['source_sha256'] = sha(accepted_md)
        data = root / 'rows.json'
        data.write_text(json.dumps(rows))
        if builder:
            subprocess.run([str(node), str(builder), str(data), str(root / 'workbook')], check=True, timeout=90)
        else:
            write_workbook(rows, workbook)
        sheet_instance, accepted_sheet = publish('result.xlsx', workbook, main_credential)
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
                assert cells['D' + str(row)].findtext('m:f', namespaces=ns).lstrip('=') == f'B{row}*C{row}'
            assert value('D10') == sum(q * p for _, q, p in expected)
            assert cells['D10'].findtext('m:f', namespaces=ns).lstrip('=') == 'SUM(D6:D8)'
        approve({'op': 'record-adoption', 'id': main_id, 'instance': str(sheet_instance), 'run': 'deliver'}, main_credential)
        delivery_ms = round((time.monotonic() - admitted) * 1000)
        for name, value, credential in [
            ('foreign result denied', {'op': 'harness-result', 'id': main_id}, observer),
            ('child escalation denied', {'op': 'derive-grant', 'scope': scope}, main_credential),
            ('operator controls denied', {'op': 'show'}, main_credential),
        ]:
            request(value, credential, success=False)
            rejections.append(name)
        activities = [request({'op': 'activity', 'id': task_id}, parent_credential) for task_id in jobs]
        activity = next(v for v in activities if v['task']['id'] == main_id)
        metrics = activity['harness']['metrics']
        assert metrics['usage']['available'] and metrics['usage']['total_tokens'] > 0, metrics
        assert metrics['usage']['sessions'] == 1
        hashes = set(materials['knowledge_hashes'])
        reads = [r for r in metrics['disclosure']['resources'] if r['content_sha256'] in hashes]
        unique = {r['content_sha256']: r['content_bytes'] for r in reads}
        metrics['knowledge'] = {'reads': len(reads), 'total_bytes': sum(r['content_bytes'] for r in reads),
                                'unique_bytes': sum(unique.values()), 'classification': 'frozen knowledge content hashes',
                                'irrelevant_bytes': sum(r['content_bytes'] for r in reads if r['content_sha256'] in materials['irrelevant_hashes'])}
        assert set(materials['required_hashes']).issubset(unique), metrics
        approve({'op': 'revoke-grant', 'id': parent_credential['id']}, parent_credential)
        request({'op': 'harness-result', 'id': main_id}, main_credential, success=False)
        assert call('spool-artifacts', sheet_instance, 'deliver')['outputs'][0]['binding']['content_sha256'] == sha(accepted_sheet)
        (root / 'activities.json').write_text(json.dumps(activities, indent=2))
        report = {'passed': True, 'real_codex': True, 'scenario': args.scenario,
            'host_sha256': sha(host), 'codex_sha256': sha(codex), 'model': args.model,
            'agents': [main_id], 'thread': first['thread'], 'turns': turns, 'tasks': jobs,
            'expected_rows': expected, 'total_eur': sum(q * p for _, q, p in expected), 'source_pdf_sha256': sha(pdf),
            'markdown_sha256': sha(accepted_md), 'xlsx_sha256': sha(accepted_sheet),
            'elapsed_ms': round((time.monotonic() - started) * 1000), 'phases': phases, 'metrics': metrics,
            'admission_to_delivery_ms': delivery_ms, 'preparation_ms': round((admitted - started) * 1000),
            'task_permissions': {'rejections': rejections, 'parent_revocation_reaches_children': True,
                'accepted_products_survive_revocation': True, 'shuttle_client': bool(args.shuttle_client)},
            'authoring': 'application library; native host publication and custody',
            'single_main_session': True, 'expected_clarifications': int(ambiguous), 'automatic_retries': 0}
        (root / 'report.json').write_text(json.dumps(report, indent=2))
        return report
    finally:
        if parent_credential and daemon.poll() is None:
            approve({'op': 'revoke-grant', 'id': parent_credential['id']}, parent_credential)
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                pending = [task for task in request({'op': 'show'})['tasks'] if task['state'] != 'settled']
                if not pending:
                    break
                time.sleep(.1)
            retained = {}
            for path in (service / 'jobs').glob('*/request.json'):
                if json.loads(path.read_text()).get('kind') == 'harness':
                    retained[path.parent.name] = request({'op': 'activity', 'id': path.parent.name})['harness']['metrics']
            (root / 'retained-metrics.json').write_text(json.dumps(retained, indent=2))
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            daemon.wait(timeout=10)


if __name__ == '__main__':
    main()
