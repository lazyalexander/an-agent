"""Mechanical domain qualification through released spools and the existing daemon."""
import importlib.util
import json
import subprocess
import sys
import time
from pathlib import Path
from zipfile import ZipFile

from app_server_support import Driver, sha
from harness_benchmark import material, wait_turn, continue_turn

STAGES = ('mechanism', 'appearance', 'mechanical-film')


class CapabilityUnavailable(RuntimeError):
    """A required application operation has no qualified adapter."""


def load_pack(spool):
    path = spool / 'examples/mechanical-animation/product/pack.py'
    spec = importlib.util.spec_from_file_location('mechanical_pack', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def site_digest(site):
    import hashlib
    inventory = {p.relative_to(site).as_posix(): sha(p) for p in sorted(site.rglob('*')) if p.is_file()}
    assert not any(p.is_symlink() for p in site.rglob('*'))
    return hashlib.sha256(json.dumps(inventory, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def run(args):
    if args.scenario.endswith('krita_target_mismatch'):
        args.evidence.mkdir(mode=0o700)
        files = ['packages/krita/document.ts', 'packages/krita/accepted.ts', 'packages/krita/live.ts',
                 'packages/krita/plugin/README.md', 'packages/krita/plugin/an_shuttle_live/core.py',
                 'packages/krita/plugin/an_shuttle_live/krita_doc.py']
        audit = {'status': 'blocked', 'model_started': False, 'effects_started': False,
                 'sources': {name: sha(args.shuttle / name) for name in files if (args.shuttle / name).is_file()},
                 'reason': 'A host-grant-bound selected-document save is absent from the selected Shuttle surface.',
                 'oracle_revision': 'The published bridge opens an accepted ORA and leaves GUI save operator-owned. The development live bridge uses a canonical path, so focus change alone is insufficient.'}
        (args.evidence / 'capability-gap.json').write_text(json.dumps(audit, indent=2))
        raise CapabilityUnavailable(audit['reason'])
    started = time.monotonic()
    driver = Driver(args.host, args.evidence)
    driver.expires = int(time.time() * 1000) + 3600000
    case = 'steam' if args.scenario.endswith('steam_chain') else 'inline4'
    try:
        packages = load_pack(args.spool).build(driver.root / 'packages')
        fixture = args.mechanical_fixtures / (case + '-inputs')
        site = args.mechanical_site.resolve(strict=True)
        python = Path(sys.executable).resolve(strict=True)
        stdlib = Path(sys.base_prefix).resolve(strict=True)
        selected_site_hash = site_digest(site)
        stages, rights, requests = {}, [], {}
        for name in STAGES:
            manifest = json.loads((packages / name / 'native-spool.json').read_text())
            runtime = {'schema': 'an-native-python-runtime-v1', 'executable': str(python),
                'executable_sha256': sha(python), 'stdlib': str(stdlib), 'site': str(site),
                'site_sha256': selected_site_hash, 'dependencies': manifest['requirements']['dependencies'],
                'readonly': ['/opt/homebrew', '/Applications/FreeCAD.app', '/Applications/Blender.app'],
                'inputs': [str((fixture / file).resolve(strict=True)) for file in
                           ('config.json', 'scene.blend', 'atlas.png', 'background.png')] if name == 'appearance' else []}
            if name == 'mechanical-film':
                runtime['graphics'] = 'macos-agx-metal-v1'
            selection = driver.root / (name + '-runtime.json')
            selection.write_text(json.dumps(runtime))
            instance = driver.root / name
            driver.call('spool-load', packages / name, instance, '--runtime', selection)
            driver.request({'op': 'register', 'instance': str(instance)})
            stages[name] = {'instance': str(instance), 'run': name, 'seconds': 900 if name == 'mechanical-film' else 300}
            request, permission = material(driver, name + '-knowledge.txt',
                (packages / name / 'silk/AGENTS.md').read_text()
                + '\nCurrent invocation contract\n' + json.dumps({'name': name, 'outputs': manifest['outputs'],
                    'depends_on': STAGES[STAGES.index(name)-1] if name != 'mechanism' else None}))
            requests[name] = request
            rights.append(permission)
        seed = driver.root / 'case.json'
        seed.write_text(json.dumps({'case': case}))
        schema = {'type': 'object', 'properties': {'stage': {'type': 'string'}, 'ready': {'type': 'boolean'}},
                  'required': ['stage', 'ready'], 'additionalProperties': False}
        prompt = ('Plan the mechanical teaching chain for ' + case + '. The ordered capabilities are '
                  'mechanism, appearance and mechanical-film. Read the current stage knowledge using host_request '
                  'and select the next stage. The application executes your reviewed choice through native spools '
                  'and returns accepted revisions. First stage knowledge: ' + json.dumps(requests['mechanism']))
        task = driver.task(args.codex.resolve(strict=True), prompt, scope={'case': args.scenario}, schema=schema)
        task.pop('app_server')
        task.update(adapter='codex-app-server', turn_limit=4, wall_seconds=1800, model=args.model)
        credential = driver.grant([driver.profile(task), *rights])
        main = driver.approve({'op': 'harness', 'task': task}, credential)['id']
        previous, results, thread, turns = None, {}, None, []
        for index, name in enumerate(STAGES, 1):
            choice = wait_turn(driver, main, credential, index)
            thread = thread or choice['thread']
            assert choice['thread'] == thread and choice['proposal'] == {'stage': name, 'ready': True}, choice
            turns.append(choice['turn'])
            selected = stages[name]
            resource = {'instance': selected['instance'], 'run': selected['run']}
            permission = {'kind': 'run', **resource, 'seconds': selected['seconds'], 'input': str(seed)}
            request = {'op': 'submit', **resource, 'seconds': selected['seconds'], 'input': str(seed)}
            if previous:
                permission.pop('input')
                permission.update(kind='run-artifact', **previous)
                request.pop('input')
                request['artifact'] = previous
            grant = driver.grant([permission, {'kind': 'accept', **resource}])
            before = time.monotonic()
            job = driver.approve(request, grant)['id']
            deadline = time.monotonic() + selected['seconds'] + 60
            while True:
                view = driver.request({'op': 'task', 'id': job}, grant)
                if view['state'] == 'settled':
                    assert view['outcome'] == 'succeeded', view
                    break
                assert time.monotonic() < deadline, 'Original mechanical task remains unknown'
                time.sleep(.5)
            driver.approve({'op': 'accept', **resource}, grant)
            if previous:
                references = driver.call('spool-input-references', selected['instance'], selected['run'])
                assert any(c['reference']['binding']['revision'] == previous['revision'] for c in references['claims'])
            inventory = driver.call('spool-artifacts', selected['instance'], selected['run'])
            exported = {}
            for output in inventory['outputs']:
                path = driver.root / (name + '-' + output['path'])
                driver.call('artifact-checkout', inventory['catalog'], output['binding']['revision'], path)
                assert sha(path) == output['binding']['content_sha256']
                exported[output['path']] = {'path': str(path), 'sha256': sha(path), 'revision': output['binding']['revision']}
            source_name = 'geometry.zip' if name == 'mechanism' else 'appearance.zip' if name == 'appearance' else 'painting.ora'
            previous = {'catalog': inventory['catalog'], 'revision': exported[source_name]['revision']}
            report = json.loads(Path(exported['report.json']['path']).read_text())
            if name == 'mechanism':
                assert report['source_report']['passed']
            else:
                assert report['passed']
            results[name] = {'task': job, 'accepted': True, 'elapsed_ms': round((time.monotonic()-before)*1000),
                             'outputs': exported, 'report': report}
            if index < len(STAGES):
                next_stage = STAGES[index]
                instruction = ('The previous stage is accepted. Read the next stage knowledge and select '
                               + next_stage + '. Previous accepted revision: ' + json.dumps(previous)
                               + '\nKnowledge request: ' + json.dumps(requests[next_stage]))
            else:
                instruction = 'All three stages have been accepted. Return stage=complete and ready=true. ' + json.dumps(
                    {name: {'accepted': value['accepted'], 'output_hashes': {k: v['sha256'] for k, v in value['outputs'].items()}}
                     for name, value in results.items()})
            continue_turn(driver, main, credential, index, instruction)
        driver.wait(main, credential)
        final = driver.request({'op': 'harness-result', 'id': main}, credential)['proposal']
        assert final == {'stage': 'complete', 'ready': True}
        fourth = driver.request({'op': 'harness-turn-result', 'id': main, 'turn': 4}, credential)
        assert fourth['thread'] == thread
        turns.append(fourth['turn'])
        outputs = results['mechanical-film']['outputs']
        config = json.loads((fixture / 'config.json').read_text())
        probe = subprocess.run([config['tools']['ffprobe'], '-v', 'error', '-count_frames', '-select_streams', 'v:0',
            '-show_entries', 'stream=width,height,nb_read_frames', '-of', 'json', outputs['film.mp4']['path']],
            capture_output=True, text=True, check=True, timeout=90)
        stream = json.loads(probe.stdout)['streams'][0]
        render = config['render']
        assert (stream['width'], stream['height']) == (render['width'], render['height'])
        assert int(stream['nb_read_frames']) == render['frames'] * config['export']['fps'] // render['fps']
        assert Path(outputs['editable.blend']['path']).stat().st_size > 1000
        with ZipFile(outputs['painting.ora']['path']) as archive:
            assert {'stack.xml', 'mergedimage.png'}.issubset(archive.namelist())
        activity = driver.request({'op': 'activity', 'id': main}, credential)
        result = {'passed': True, 'scenario': args.scenario, 'real_codex': True,
            'host_sha256': sha(driver.host), 'codex_sha256': sha(args.codex), 'model': args.model,
            'agents': [main], 'thread': thread, 'turns': turns, 'single_main_session': True,
            'elapsed_ms': round((time.monotonic()-started)*1000), 'metrics': activity['harness']['metrics'],
            'admission_to_delivery_ms': round((time.monotonic()-driver.first_admitted)*1000),
            'preparation_ms': round((driver.first_admitted-started)*1000),
            'stages': results, 'video': stream, 'editable_animation': True,
            'ora_scope': 'first-frame reference; editable mechanical animation is in the Blender file',
            'automatic_retries': 0}
        (driver.root / 'report.json').write_text(json.dumps(result, indent=2))
        return result
    finally:
        driver.close()

