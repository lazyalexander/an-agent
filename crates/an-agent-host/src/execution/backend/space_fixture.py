"""Local protocol fault fixture, not a SPACE provider or guest qualification."""
import base64, json, os, sys, time
from pathlib import Path
root=Path(sys.argv[2])
def configuration(): return {'token':os.environ['SPACE_API_TOKEN']}
def api(config, method, path, body=None, key=None, timeout=None):
    if method=='DELETE':
        if config['token']=='unknown-delete': raise RuntimeError('unknown deletion')
        (root/'delete-reached').write_text('reached')
        return {'operation_id':'op_delete'}
    if path.startswith('/v1/operations/'):
        return {'state':'succeeded','kind':'delete_sandbox','target_type':'sandbox','target_id':'sbx_foreign' if config['token']=='wrong-delete' else 'sbx_fixture'}
    r=json.loads((root/'space-request.json').read_text())
    return {'id':'sbx_fixture','created_at':'generation-2' if config['token']=='changed' and (root/'space-control/state.json').exists() and 'identity' in json.loads((root/'space-control/state.json').read_text()) else 'generation-1','template':'seed',
        'template_digest':os.environ['SPACE_TEMPLATE'].split('@')[1],
        'metadata':{'an_agent_run':r['run_id'],'an_agent_operation': 'foreign' if config['token']=='foreign' else r['operation_id']},'data':[]}
def require_id(state, name, prefix):
    assert state[name].startswith(prefix)
    return state[name]
def save(directory,state):
    if os.environ['SPACE_API_TOKEN']=='journal-failed' and 'delete' in state['requests']:
        raise OSError('simulated journal exhaustion')
    (directory/'state.json').write_text(json.dumps(state))
def mutation(config,directory,state,name,method,path,body=None):
    assert name not in state['requests']
    state['requests'][name]={'body':body}; save(directory,state)
    if name=='create':
        assert body['network_policy']['egress']['default_action']=='deny'
        assert body['metadata']['an_agent_operation']==json.loads((root/'operation.json').read_text())['operation_id']
        if config['token']=='uncertain': raise RuntimeError('unknown create response')
        return {'sandbox_id':'sbx_fixture','operation_id':'op_create'}
    if config['token']=='unknown-delete': raise RuntimeError('unknown deletion')
    return {'operation_id':'op_delete'}
def remember_ids(directory,state,result,fields):
    for source,target,prefix in fields:
        assert result[source].startswith(prefix);state[target]=result[source]
    save(directory,state)
def wait_for_state(*args): pass
def consume_events(response,transcript,stdout,stderr):
    stdout.write(json.dumps({'exit_code':0,'candidate':base64.b64encode(b'x'*65537 if os.environ['SPACE_API_TOKEN']=='oversize' else b'accepted').decode()}).encode())
def run_command(config,directory,state):
    if config['token']=='cancel':
        (root/'run-started').write_text('started'); time.sleep(60)
    if config['token']=='failed': raise RuntimeError('guest failed')
    consume_events(None,None,None,None)
