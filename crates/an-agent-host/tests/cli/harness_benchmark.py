"""Real-provider benchmark cases using the existing daemon acceptance client."""
import json
import signal
import subprocess
import time
from pathlib import Path

from app_server_support import Driver, sha


def material(driver, name, text):
    path = driver.root / name
    path.write_text(text)
    request = {'op': 'read-input', 'path': str(path), 'sha256': sha(path), 'encoding': 'utf-8'}
    permission = {'kind': 'read-input', 'path': str(path), 'sha256': sha(path)}
    return request, permission


def launch(driver, args, prompt, rights, schema, turns=1):
    task = driver.task(args.codex.resolve(strict=True), prompt, scope={'case': args.scenario}, schema=schema)
    task.update(adapter='codex-app-server', turn_limit=turns, wall_seconds=600, model=args.model)
    task.pop('app_server')
    credential = driver.grant([driver.profile(task), *rights])
    job = driver.approve({'op': 'harness', 'task': task}, credential)['id']
    return job, credential


def wait_turn(driver, job, credential, number):
    deadline = time.monotonic() + 600
    marker = driver.service / 'jobs' / job / f'turn-result-{number}.json'
    while not marker.exists():
        view = driver.request({'op': 'task', 'id': job}, credential)
        if view['state'] == 'settled':
            raise AssertionError(('Original session ended before expected turn', view))
        if time.monotonic() >= deadline:
            raise AssertionError('Original turn did not settle within its deadline')
        time.sleep(.1)
    return driver.request({'op': 'harness-turn-result', 'id': job, 'turn': number}, credential)


def continue_turn(driver, job, credential, number, instruction):
    view = driver.request({'op': 'harness-connect', 'id': job}, credential)
    return driver.approve({'op': 'harness-continue', 'id': job, 'execution': view['execution'],
                           'after_turn': number, 'command': f'next-{number}', 'instruction': instruction}, credential)


def publish(driver, content, name='result.json'):
    source = driver.root / ('source-' + name)
    source.write_text(content)
    package = driver.root / ('package-' + name)
    (package / 'silk').mkdir(parents=True)
    entry = package / 'silk/main.sh'
    entry.write_text('set -eu\ncat "$AN_SPOOL_INPUT" > ' + name + '\n')
    (package / 'native-spool.json').write_text(json.dumps({
        'schema': 'an-native-spool-v3', 'name': 'benchmark-delivery', 'version': '1.0.0',
        'entry': 'silk/main.sh', 'files': {'silk/main.sh': sha(entry)}, 'outputs': [name],
        'events': {'consumes': [], 'produces': ['artifact.accepted']},
        'requirements': {'runtime': 'macos-native-process-v1', 'os': 'macos', 'arch': 'portable-source',
                         'interaction': 'headless', 'gpu': 'none', 'inputs': ['input'], 'editable': [],
                         'dependencies': [], 'capabilities': ['read-input', 'write-declared-outputs']}}))
    instance = driver.root / ('instance-' + name)
    driver.call('spool-load', package, instance)
    driver.request({'op': 'register', 'instance': str(instance)})
    resource = {'instance': str(instance), 'run': 'deliver'}
    credential = driver.grant([{'kind': 'run', **resource, 'input': str(source), 'seconds': 20},
                               {'kind': 'accept', **resource}])
    job = driver.approve({'op': 'submit', **resource, 'input': str(source), 'seconds': 20}, credential)['id']
    driver.wait(job, credential)
    driver.approve({'op': 'accept', **resource}, credential)
    inventory = driver.call('spool-artifacts', instance, 'deliver')
    assert inventory['outputs'][0]['binding']['content_sha256'] == sha(source)
    return {'sha256': sha(source), 'task': job, 'accepted': True}


def control_case(driver, args):
    scenario = args.scenario.rsplit('.', 1)[-1]
    request, permission = material(driver, 'skill.txt', 'The current output target is 100. Supplemental instructions supersede this value.')
    schema = {'type': 'object', 'properties': {'value': {'type': 'integer'}},
              'required': ['value'], 'additionalProperties': False}
    prompt = ('Read this skill with host_request, then independently calculate the first 300 prime numbers '
              'and verify their sum before returning the skill target as value. ' + json.dumps(request))
    job, credential = launch(driver, args, prompt, [permission], schema)
    driver.ready(job)
    receipts = driver.service / 'jobs' / job / 'tool-receipts.jsonl'
    deadline = time.monotonic() + 90
    while not receipts.exists() or not receipts.read_text().strip():
        assert time.monotonic() < deadline
        time.sleep(.025)
    before = driver.request({'op': 'harness-connect', 'id': job}, credential)
    owner = (driver.service / 'jobs' / job / 'owner.json').read_bytes()
    if scenario == 'cancel':
        driver.approve({'op': 'cancel', 'id': job})
        driver.wait(job, credential, 'cancelled')
        result = json.loads((driver.service / 'jobs' / job / 'completion.json').read_text())['result']
        assert result['termination_confirmed'] and not result['proposal_available']
        assert not driver.request({'op': 'grant-status'}, credential)['active']
        return job, {'cancelled': True, 'local_release': True,
                     'remote_termination_confirmed': result['remote_termination_confirmed']}
    if scenario == 'daemon_reconnect':
        driver.process.send_signal(signal.SIGTERM)
        driver.process.wait(timeout=15)
        driver.process = subprocess.Popen([str(driver.host), 'daemon', 'serve', str(driver.service)], stdout=driver.log, stderr=driver.log)
        deadline = time.monotonic() + 10
        while not (driver.service / 'endpoint.json').exists():
            assert time.monotonic() < deadline
            time.sleep(.05)
        after = driver.request({'op': 'harness-connect', 'id': job}, credential)
        assert before['execution'] == after['execution']
        assert owner == (driver.service / 'jobs' / job / 'owner.json').read_bytes()
    command = {'op': 'harness-steer', 'id': job, 'execution': before['execution'], 'command': 'new-target',
               'instruction': 'Stop calculating primes. The final target is now 200. Return value=200 immediately.'}
    driver.approve(command, credential)
    driver.wait(job, credential)
    final = driver.request({'op': 'harness-result', 'id': job}, credential)
    assert final['proposal'] == {'value': 200}
    view = driver.request({'op': 'harness-connect', 'id': job}, credential)
    assert any(c['state'] == 'acknowledged' for c in view['commands'])
    if scenario == 'lost_receipt':
        ack = driver.service / 'jobs' / job / 'ack-new-target.json'
        ack.rename(driver.root / 'withheld-ack.json')
        view = driver.request({'op': 'harness-connect', 'id': job}, credential)
        assert any(c['state'] == 'unknown' for c in view['commands'])
        replay = driver.approve(command, credential)
        assert replay['replayed'] is False
        native = (driver.service / 'jobs' / job / 'native-events.jsonl').read_text()
        assert native.count('"id":"steer-new-target"') == 1
        return job, {'outcome': 'retain_unknown', 'original_resources_retained': True, 'replayed': False,
                     'fault_scope': 'withheld steering acknowledgement; application save effects remain unqualified'}
    return job, {'owner_retained': True, 'artifact': publish(driver, json.dumps(final['proposal']))}


def refusal_case(driver, args):
    scenario = args.scenario.rsplit('.', 1)[-1]
    request, permission = material(driver, 'requested-input.txt', 'Selected release information')
    if scenario == 'stale_release':
        rights = [permission]
        Path(request['path']).write_text('Replacement bytes after the selected revision')
    else:
        rights = []
    card, card_permission = material(driver, 'card.json', json.dumps({'requested_input': request}))
    schema = {'type': 'object', 'properties': {'status': {'type': 'string'}},
              'required': ['status'], 'additionalProperties': False}
    if scenario == 'missing_capability':
        Path(card['path']).write_text(json.dumps({'available': [{'input': 'table', 'output': 'flattened image PDF'}]}))
        card['sha256'] = sha(Path(card['path']))
        card_permission['sha256'] = card['sha256']
        prompt = ('Read the capability card. The user requires an editable XLSX workbook with formulas. '
                  'Return status=unsupported if the available capability cannot deliver it. ' + json.dumps(card))
        expected = 'unsupported'
    else:
        prompt = ('Read the card, then attempt its requested input through host_request using the exact identity. '
                  'If access fails, stop and return status=refused. ' + json.dumps(card))
        expected = 'refused'
    job, credential = launch(driver, args, prompt, [card_permission, *rights], schema)
    driver.wait(job, credential)
    final = driver.request({'op': 'harness-result', 'id': job}, credential)
    assert final['proposal']['status'] == expected
    records = [json.loads(line) for line in (driver.service / 'jobs' / job / 'tool-receipts.jsonl').read_text().splitlines()]
    if expected == 'refused':
        assert any(not r['succeeded'] for r in records)
    assert not list(driver.root.glob('instance-*'))
    return job, {'outcome': expected, 'unauthorized_effects': 0, 'failed_calls': sum(not r['succeeded'] for r in records)}


def reasoning_case(driver, args):
    reads, rights = [], []
    expected = []
    for index, (old, new, revoked) in enumerate([(80, 95, False), (80, 110, True), (90, 105, False)]):
        name = f'material-{index}'
        body = {'id': name, 'certificates': [
            {'id': f'{name}-A', 'date': '2025-01-01', 'maximum_celsius': old, 'revoked': False},
            {'id': f'{name}-B', 'date': '2026-01-01', 'maximum_celsius': new, 'revoked': revoked}]}
        request, permission = material(driver, name + '.json', json.dumps(body))
        reads.append(request)
        rights.append(permission)
        expected.append({'id': name, 'maximum_celsius': old if revoked else new, 'evidence': f'{name}-A' if revoked else f'{name}-B'})
    item = {'type': 'object', 'properties': {'id': {'type': 'string'}, 'maximum_celsius': {'type': 'integer'},
                                           'evidence': {'type': 'string'}},
            'required': ['id', 'maximum_celsius', 'evidence'], 'additionalProperties': False}
    schema = {'type': 'object', 'properties': {'materials': {'type': 'array', 'items': item}},
              'required': ['materials'], 'additionalProperties': False}
    job, credential = launch(driver, args,
        'Read all three independent dossiers. For each select the latest unrevoked certificate, preserve its Celsius limit '
        'and cite its exact certificate ID. Return in material ID order. ' + json.dumps(reads), rights, schema)
    driver.wait(job, credential)
    proposal = driver.request({'op': 'harness-result', 'id': job}, credential)['proposal']
    assert proposal['materials'] == expected, proposal
    return job, {'artifact': publish(driver, json.dumps(proposal)), 'independent_dossiers': 3}


def work_case(driver, args):
    parallel = args.scenario.endswith('parallel_tools')
    count = 3 if parallel else 2
    stages = []
    for index in range(count):
        package = driver.root / f'package-{index}'
        (package / 'silk').mkdir(parents=True)
        entry = package / 'silk/main.sh'
        entry.write_text('set -eu\ndate +%s > timing.txt\nsleep 2\n'
                         'awk \'{print $1 * 2}\' "$AN_SPOOL_INPUT" > result.txt\ndate +%s >> timing.txt\n')
        (package / 'native-spool.json').write_text(json.dumps({
            'schema': 'an-native-spool-v3', 'name': 'numeric-step', 'version': '1.0.0',
            'entry': 'silk/main.sh', 'files': {'silk/main.sh': sha(entry)}, 'outputs': ['result.txt', 'timing.txt'],
            'events': {'consumes': [], 'produces': ['artifact.accepted']},
            'requirements': {'runtime': 'macos-native-process-v1', 'os': 'macos', 'arch': 'portable-source',
                'interaction': 'headless', 'gpu': 'none', 'inputs': ['input'], 'editable': [],
                'dependencies': [], 'capabilities': ['read-input', 'write-declared-outputs']}}))
        instance = driver.root / f'instance-{index}'
        driver.call('spool-load', package, instance)
        driver.request({'op': 'register', 'instance': str(instance)})
        source = driver.root / f'number-{index}.txt'
        source.write_text(str([5, 7, 11][index]))
        stages.append({'id': f'stage-{index}', 'instance': str(instance), 'source': str(source)})
    request, permission = material(driver, 'program-contract.json', json.dumps({
        'stages': stages, 'operation': 'multiply input by two',
        'dependencies': 'independent' if parallel else 'stage-1 consumes accepted stage-0 output',
        'outputs': 'editable text numbers'}))
    schema = {'type': 'object', 'properties': {
        'order': {'type': 'array', 'items': {'type': 'string'}},
        'results': {'type': 'array', 'items': {'type': 'integer'}}},
        'required': ['order', 'results'], 'additionalProperties': False}
    job, credential = launch(driver, args,
        'Read the program contract and choose the stage execution order. Return results=[] for now. '
        'The application will execute the selected programs through the original daemon and return accepted outputs. '
        + json.dumps(request), [permission], schema, turns=2)
    first = wait_turn(driver, job, credential, 1)
    order = first['proposal']['order']
    by_id = {stage['id']: stage for stage in stages}
    assert len(order) == len(stages) and set(order) == set(by_id), order
    if not parallel:
        assert order == list(by_id), 'Dependent stages must follow accepted-input order'
    stages = [by_id[name] for name in order]
    pending, values, intervals = [], [], []
    previous = None

    def submit(stage, artifact):
        resource = {'instance': stage['instance'], 'run': 'transform'}
        permission = {'kind': 'run', **resource, 'input': stage['source'], 'seconds': 20}
        command = {'op': 'submit', **resource, 'input': stage['source'], 'seconds': 20}
        if artifact:
            permission.pop('input')
            permission.update(kind='run-artifact', **artifact)
            command.pop('input')
            command['artifact'] = artifact
        grant = driver.grant([permission, {'kind': 'accept', **resource}])
        started = time.monotonic()
        task = driver.approve(command, grant)['id']
        return stage, resource, grant, task, started

    def accept(pending):
        stage, resource, grant, task, started = pending
        driver.wait(task, grant)
        ended = time.monotonic()
        driver.approve({'op': 'accept', **resource}, grant)
        inventory = driver.call('spool-artifacts', stage['instance'], 'transform')
        output = next(output for output in inventory['outputs'] if output['path'] == 'result.txt')
        reference = {'catalog': inventory['catalog'], 'revision': output['binding']['revision']}
        target = driver.root / (stage['id'] + '-accepted.txt')
        driver.call('artifact-checkout', reference['catalog'], reference['revision'], target)
        values.append(int(target.read_text().strip()))
        timing = next(output for output in inventory['outputs'] if output['path'] == 'timing.txt')
        timing_path = driver.root / (stage['id'] + '-timing.txt')
        driver.call('artifact-checkout', inventory['catalog'], timing['binding']['revision'], timing_path)
        begin, end = map(int, timing_path.read_text().splitlines())
        intervals.append({'task': task, 'stage': stage['id'], 'submitted_seconds': started, 'observed_settled_seconds': ended,
                          'program_start_epoch_seconds': begin, 'program_end_epoch_seconds': end,
                          'timestamp_resolution_seconds': 1})
        if not parallel and len(values) > 1:
            refs = driver.call('spool-input-references', stage['instance'], 'transform')
            assert any(c['reference']['binding']['revision'] == previous['revision'] for c in refs['claims'])
        return reference

    if parallel:
        pending = [submit(stage, None) for stage in stages]
        for item in pending:
            accept(item)
    else:
        for stage in stages:
            previous = accept(submit(stage, previous))
    expected = [int(Path(stage['source']).read_text()) * 2 for stage in stages] if parallel else [10, 20]
    assert values == expected, values
    continue_turn(driver, job, credential, 1,
                  'These are the real accepted outputs in stage order. Preserve the order and return their values. '
                  + json.dumps(values))
    driver.wait(job, credential)
    result = driver.request({'op': 'harness-result', 'id': job}, credential)['proposal']
    assert len(result['order']) == len(result['results']) == len(stages)
    assert dict(zip(result['order'], result['results'])) == dict(zip(order, values)), result
    # Integer program timestamps provide a conservative common-running interval.
    interior = [(item['program_start_epoch_seconds'] + 1, item['program_end_epoch_seconds']) for item in intervals]
    peak = max(sum(begin <= point < end for begin, end in interior) for point, _ in interior)
    return job, {'accepted_values': values, 'program_intervals': intervals,
                 'confirmed_parallel_programs_lower_bound': peak, 'daemon_slots': 3,
                 'parallel_submission': parallel, 'revision_bound_handoff': not parallel}


def run(args):
    started = time.monotonic()
    driver = Driver(args.host, args.evidence)
    driver.expires = int(time.time() * 1000) + 900000
    job = None
    try:
        family = args.scenario.split('.')[0]
        if family == 'background_control' or args.scenario.endswith('lost_receipt'):
            job, detail = control_case(driver, args)
        elif family == 'boundary_faults' or args.scenario.endswith('missing_capability'):
            job, detail = refusal_case(driver, args)
        elif args.scenario.endswith('parallel_reasoning'):
            job, detail = reasoning_case(driver, args)
        elif family == 'work_shape':
            job, detail = work_case(driver, args)
        else:
            raise ValueError('This case requires its domain-specific acceptance entrypoint')
        activity = driver.request({'op': 'activity', 'id': job})
        report = {'passed': True, 'scenario': args.scenario, 'real_codex': True,
                  'host_sha256': sha(driver.host), 'codex_sha256': sha(args.codex), 'model': args.model,
                  'agents': [job], 'single_main_session': True, 'metrics': activity['harness']['metrics'],
                  'elapsed_ms': round((time.monotonic() - started) * 1000), 'details': detail,
                  'admission_to_outcome_ms': round((time.monotonic() - driver.first_admitted) * 1000),
                  'preparation_ms': round((driver.first_admitted - started) * 1000),
                  'automatic_retries': 0}
        (driver.root / 'report.json').write_text(json.dumps(report, indent=2))
        return report
    finally:
        driver.close()

