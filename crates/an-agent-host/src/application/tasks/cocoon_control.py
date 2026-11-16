"""Keep Cocoon failures attributable without exposing private diagnostics to the model."""
import uuid


def call(direct, save, root, operation, grant='', request_id=None):
    identity = request_id or str(uuid.uuid4())
    request = {'request': identity, 'caller': 'product-task', 'credential': '',
               'grant': grant, 'operation': operation}
    path = root / 'cocoon-request.json'
    save(path, request)
    # The local Cocoon CLI supplies the management credential; never send it to a model.
    response = direct('api', path, abortable=operation['operation'] == 'image_mutation')
    if response.get('request') != identity or response.get('caller') != 'product-task':
        raise RuntimeError('Cocoon response identity mismatch; inspect original operation')
    result = response['result']
    if 'Ok' in result:
        return result['Ok']
    directory = root / 'cocoon-errors'
    directory.mkdir(mode=0o700, exist_ok=True)
    save(directory / (identity + '.json'), {'request': identity,
         'operation': operation['operation'], 'result': result})
    if result.get('Err') == 'host protection backpressure rejects new execution':
        raise RuntimeError('Cocoon host memory protection blocked execution; protection remains enabled')
    raise RuntimeError('Cocoon refused ' + operation['operation'] + '; inspect private error record ' + identity)
