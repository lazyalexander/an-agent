"""SPACE transport only. The Rust host owns admission and artifact acceptance."""
import base64, hashlib, importlib.util, io, json, os, sys, uuid
from pathlib import Path

profile, directory, step = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
p = json.loads(profile.read_text())
source = Path(p['client'])
assert hashlib.sha256(source.read_bytes()).hexdigest() == p['client_sha256']
spec = importlib.util.spec_from_file_location('space_official', source)
s = importlib.util.module_from_spec(spec); spec.loader.exec_module(s)
os.environ.update(SPACE_API_URL=p['api_url'], SPACE_DATA_PLANE_URL=p['data_plane_url'],
                  SPACE_TEMPLATE=p['template'], SPACE_API_TOKEN=Path(p['token_file']).read_text().strip())
c = s.configuration()
original = s.api
def api(config, method, path, body=None, key=None, timeout=None):
    return original(config, method, path, body, key, p['control_seconds'] if timeout is None else timeout)
s.api = api
s.REQUEST_TIMEOUT = p['control_seconds']; s.STREAM_TIMEOUT = p['control_seconds']
run = directory / 'space-control'
state = json.loads((run / 'state.json').read_text()) if run.exists() else {'requests': {}, 'transcripts': []}
request = json.loads((directory / 'space-request.json').read_text())
def save(): s.save(run, state)
def owned():
    value = api(c, 'GET', '/v1/sandboxes/' + state['sandbox_id'])
    assert value['metadata'].get('an_agent_operation') == request['operation_id']
    assert value['metadata'].get('an_agent_run') == request['run_id']
    name, digest = p['template'].split('@', 1)
    assert value['template_digest'] == digest and value['template'] == name
    if 'identity' in state:
        assert all(value[k] == state['identity'][k] for k in ('id', 'created_at', 'template_digest'))
    return value
if step == 'create':
    run.mkdir(mode=0o700)
    save()
    value = s.mutation(c, run, state, 'create', 'POST', '/v1/sandboxes', {
        'template': p['template'], 'runtime': 'fc',
        'metadata': {'an_agent_run': request['run_id'], 'an_agent_operation': request['operation_id']},
        'resources': {'vcpu': 1, 'memory_mib': 1024},
        'network_policy': {'version': 'v1', 'egress': {'default_action': 'deny'}}})
    s.remember_ids(run, state, value, [('sandbox_id','sandbox_id','sbx_'),('operation_id','operation_id','op_')])
elif step == 'ready':
    s.wait_for_state(c, '/v1/operations/' + state['operation_id'], 'succeeded')
    s.wait_for_state(c, '/v1/sandboxes/' + state['sandbox_id'], 'running')
    state['identity'] = owned(); save()
    value = api(c, 'GET', '/v1/sandboxes/' + state['sandbox_id'] + '/processes?limit=1')
    assert isinstance(value.get('data'), list)
elif step == 'run':
    owned()
    bootstrap = (directory / 'space-guest.py').read_text()
    payload = {'program':base64.b64encode((directory/'program').read_bytes()).decode(),
               'input':base64.b64encode((directory/'input.txt').read_bytes()).decode()}
    s.COMMAND = {'command':['/usr/bin/python3','-c',bootstrap,base64.b64encode(json.dumps(payload).encode()).decode()],
                 'cwd':'/tmp','user':'root','timeout':'20s'}
    class Limited(io.BytesIO):
        def write(self, data):
            if self.tell() + len(data) > 262144: raise ValueError('bounded guest output exceeded')
            return super().write(data)
    out, err = Limited(), Limited(); consume = s.consume_events
    s.consume_events = lambda response, transcript, _out, _err: consume(response, transcript, out, err)
    s.run_command(c, run, state)
    value = json.loads(out.getvalue())
    assert value['exit_code'] == 0
    candidate = base64.b64decode(value['candidate'], validate=True)
    assert len(candidate) <= 65536
    with (directory/'space-candidate').open('xb') as f: f.write(candidate)
elif step == 'delete':
    if 'sandbox_id' not in state:
        raise ValueError('submission unresolved; no guessed cleanup target')
    owned()
    # Storage exhaustion must not prevent a single owned cleanup request.
    # If durable proof cannot be written, the host still refuses acceptance.
    assert 'delete' not in state['requests'], 'delete already requested; inspect before replay'
    key = 'an-agent-delete-' + uuid.uuid4().hex
    path = '/v1/sandboxes/' + state['sandbox_id']
    state['requests']['delete'] = {'method':'DELETE','path':path,'body':None,'key':key}
    try: save()
    except OSError: pass
    value = api(c, 'DELETE', path, key=key)
    state['delete_operation_id'] = value['operation_id']
    s.require_id(state, 'delete_operation_id', 'op_')
    try: save()
    except OSError: pass
    s.wait_for_state(c, '/v1/operations/' + state['delete_operation_id'], 'succeeded')
    operation = api(c, 'GET', '/v1/operations/' + state['delete_operation_id'])
    assert operation['state'] == 'succeeded' and operation['kind'] == 'delete_sandbox'
    assert operation['target_type'] == 'sandbox' and operation['target_id'] == state['sandbox_id']
    state['deleted'] = True; save()
else:
    raise ValueError('unknown transport step')
