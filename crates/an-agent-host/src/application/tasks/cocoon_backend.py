"""Legacy backend lifecycle; the controller owns task turns and delivery."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time
import uuid


class CocoonBackend:

    def __init__(self, environment, components, config, root, service, client, save, read, check, update, wait_control, control):
        (self.environment, self.components, self.config) = (environment, components, config)
        (self.root, self.service, self.client) = (root, service, client)
        (self.save, self.read, self.check, self.update) = (save, read, check, update)
        (self.wait_control, self.control) = (wait_control, control)
        self.vm = self.process = self.work = self.transport = None
        self.cocoon_state = Path(config.get('cocoon_state') or root / 'cocoon')

    def direct(self, *args, abortable=False):
        command = [str(self.environment / self.components['cocoon']), 'host', str(self.cocoon_state), *map(str, args)]
        if abortable:
            with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as process:
                while True:
                    try:
                        (stdout, stderr) = process.communicate(timeout=0.1)
                        if process.returncode:
                            raise RuntimeError('Cocoon control failed; inspect original request')
                        return json.loads(stdout)
                    except subprocess.TimeoutExpired:
                        try:
                            self.check()
                        except BaseException:
                            try:
                                subprocess.run(command[:3] + ['image-cancel', str(self.root / 'image-branch-query.json')], capture_output=True, timeout=10)
                            finally:
                                process.terminate()
                                process.communicate(timeout=5)
                            raise
        result = subprocess.run([str(self.environment / self.components['cocoon']), 'host', str(self.cocoon_state), *map(str, args)], capture_output=True, text=True, timeout=180)
        if result.returncode:
            raise RuntimeError('Cocoon control failed; inspect private records')
        return json.loads(result.stdout)

    def api(self, operation, grant='', request_id=None):
        return self.control['call'](self.direct, self.save, self.root, operation, grant, request_id)

    def prepare(self):
        self.check()
        if not self.config.get('cocoon_state'):
            image = self.direct('image', 'import', self.environment / self.components['kernel'], self.environment / self.components['initrd'], self.environment / self.components['root_image'], 'Reviewed offline runtime')['image']
        else:
            self.cocoon_state.parent.mkdir(parents=True, exist_ok=True, mode=448)
            self.direct('image-init')
            if self.config.get('image'):
                image = self.config['image']['image']
                current = self.direct('image-info', image)
                if current['approved'] is not True or current['sha256'] != self.config['image']['sha256']:
                    raise ValueError('Approved image changed')
            else:
                image = self.direct('image', 'import', self.environment / self.components['kernel'], self.environment / self.components['initrd'], self.environment / self.components['root_image'], 'Reviewed offline runtime')['image']
        self.vm = str(uuid.uuid4())
        self.save(self.root / 'setup.json', {'vm': self.vm, 'image': image, 'owner': 'product-task'})
        self.update(vm=self.vm)
        self.save(self.root / 'runtime-launch.json', {'automatic_retries': 0})
        with (self.root / 'cocoon.log').open('w') as log:
            self.process = subprocess.Popen([str(self.environment / self.components['cocoon']), 'host', str(self.cocoon_state), 'serve', str(self.environment / self.components['runner'])], stdout=log, stderr=log)
        self.client.cli('app', 'task-runtime-owner', self.service, self.root.name, self.process.pid)
        socket = Path('/tmp') / ('cocoon-host-' + hashlib.sha256(os.fsencode(self.cocoon_state)).hexdigest() + '.sock')
        self.wait_control(socket, self.process, lambda : self.api({'operation': 'protection_status'}), self.check)
        self.check()
        if self.config.get('image'):
            request_id = str(uuid.uuid4())
            self.save(self.root / 'image-branch-query.json', {'request': request_id, 'caller': 'product-task', 'credential': (self.cocoon_state / 'api-credential').read_text()})
            self.save(self.root / 'image-branch-intent.json', {'request': request_id, 'vm': self.vm, 'image': image})
            result = self.api({'operation': 'image_mutation', 'action': {'operation': 'branch', 'image': image, 'sha256': self.config['image']['sha256'], 'vm': self.vm, 'owner': 'product-task:' + self.root.name}}, request_id=request_id)
            self.save(self.root / 'image-branch-result.json', result)
            if result['state'] != 'succeeded':
                raise RuntimeError('Original image branch requires reconciliation')
        else:
            self.direct('vm', 'create', self.vm, image, 'product-task', 'retain')
        self.check()
        start_id = str(uuid.uuid4())
        self.save(self.root / 'start-intent.json', {'request': start_id, 'vm': self.vm})
        active = self.api({'operation': 'start', 'vm': self.vm}, request_id=start_id)
        self.save(self.root / 'start-result.json', active)
        self.work = self.api({'operation': 'issue_grant', 'subject': 'product-task', 'vm': self.vm, 'actions': ['inspect', 'guest', 'export', 'stop'], 'expires_unix': int(time.time()) + 900, 'max_export_bytes': 4194304})
        self.transport = self.root / 'transport.json'
        self.save(self.transport, {'socket': str(socket), 'grant': self.work['token'], 'caller': 'product-task', 'vm': self.vm, 'generation': active['generation']})
        return self.transport

    def cleanup(self, state, credential, vm_bound, setup_bound, owners_confirmed, requested_outcome):
        vm_confirmed = self.vm is None
        if self.vm:
            try:
                initial = self.direct('vm', 'show', self.vm)
                if not vm_bound and initial.get('state') == 'prepared' and ('active_lease' in initial) and (initial['active_lease'] is None) and ('active_attempt' in initial) and (initial['active_attempt'] is None):
                    observed = initial
                elif vm_bound:
                    observed = self.client.request({'op': 'cocoon-release', 'id': state['session']}, credential)
                    if not observed or not observed.get('termination_confirmed'):
                        raise RuntimeError('Original VM release is unconfirmed')
                elif setup_bound:
                    observed = self.client.cli('app', 'task-release-setup', self.service, self.root.name)
                    if not observed.get('termination_confirmed'):
                        raise RuntimeError('Original setup release is unconfirmed')
                elif self.work:
                    stop_id = str(uuid.uuid4())
                    self.save(self.root / 'controller-stop.json', {'request': stop_id, 'vm': self.vm})
                    observed = self.api({'operation': 'stop', 'vm': self.vm}, self.work['token'], stop_id)
                else:
                    observed = self.direct('vm', 'stop', self.vm)
                self.save(self.root / 'vm-release.json', observed)
                observed = self.direct('vm', 'show', self.vm)
                vm_confirmed = observed.get('active_lease') is None and (observed.get('state') in ('stopped', 'recovery_required') or (observed.get('state') == 'prepared' and observed.get('active_attempt') is None))
                if observed.get('active_lease') is not None or not vm_confirmed:
                    self.update(state='unknown', error='VM release not confirmed')
            except Exception:
                if not (self.root / 'start-intent.json').exists():
                    vm_confirmed = True
                else:
                    self.update(state='unknown', error='VM release not confirmed')
        if self.process is not None and vm_confirmed and owners_confirmed:
            self.process.terminate()
            try:
                self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                vm_confirmed = False
        runtime_confirmed = self.process is None or self.process.poll() is not None
        if runtime_confirmed:
            try:
                if not self.client.cli('app', 'task-image-settle', self.service, self.root.name)['settled']:
                    owners_confirmed = False
            except Exception:
                owners_confirmed = False
        if not owners_confirmed:
            self.update(state='unknown', error='Original operation settlement unconfirmed')
        if vm_confirmed and owners_confirmed and runtime_confirmed and (state['state'] == 'stopping'):
            self.update(state=requested_outcome, termination_confirmed=True, runtime_termination_confirmed=True)
        elif not runtime_confirmed:
            self.update(state='unknown', error='Control service release not confirmed')
