"""Bind immutable spool releases and independent grants to a product session."""


def bind(client, task, scope, instances, specs, sha):
    bindings = []
    credentials = {}
    for name, instance in instances.items():
        selected = ('extract', 'intermediate') if name == 'intermediate' else ('final',)
        jobs = [{'spec': str(specs[key]), 'sha256': sha(specs[key])} for key in selected]
        permissions = []
        for permission in scope['permissions']:
            kind = permission['kind']
            if kind in ('read-input', 'artifact'):
                permissions.append(permission)
            elif permission.get('instance') == str(instance):
                permissions.append(permission)
            elif kind == 'cocoon-job' and permission['spec_sha256'] in {j['sha256'] for j in jobs}:
                permissions.append(permission)
        grant = client.approve({'op': 'grant', 'scope': dict(scope,
            subject='spool:' + name, delegate=False, permissions=permissions)})['credential']
        credentials[name] = grant
        release = client.cli('spool-show', instance)['release']
        bindings.append({'name': name, 'instance': str(instance), 'release': release,
                         'grant': grant['id'], 'cocoon_jobs': jobs})
    scope['permissions'] = [p for p in scope['permissions']
                            if p['kind'] not in ('run', 'run-artifact', 'run-candidate', 'run-job-output', 'accept')]
    task['scope']['turn_context'] = {'schema': 'an-turn-context-v1', 'spool_bindings': bindings}
    task['prompt'] = instructions(task['prompt'])
    scope['permissions'].insert(0, client.request({'op': 'harness-permission', 'task': task}))
    return credentials


def instructions(text):
    return ('Use the host turn context revision on every host_request except turn-context. '
            'Read turn-context when necessary. For cocoon-job select spool_binding=intermediate '
            'for extract/intermediate and spool_binding=final for final. For artifact-read select '
            'the consuming spool binding (final when preparing Excel). These bindings confer no '
            'extra permission; host checks remain authoritative.\n' + text)
