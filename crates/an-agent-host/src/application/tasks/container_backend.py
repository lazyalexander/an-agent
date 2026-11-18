"""Run one approved document task against a provisioned Container installation."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import time
import uuid


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def image_identity(archive):
    with tarfile.open(archive, 'r:*') as image:
        entry = image.getmember('index.json')
        if not entry.isfile() or entry.size > 65536:
            raise ValueError('Invalid offline image index')
        descriptor, = json.load(image.extractfile(entry))['manifests']
    name = descriptor['annotations']['org.opencontainers.image.ref.name']
    repository = name.partition('@')[0]
    prefix, slash, final = repository.rpartition('/')
    return prefix + slash + final.partition(':')[0] + '@' + descriptor['digest']


class ContainerBackend:
    def __init__(self, environment, components, config, root, service, client,
                 save, read, check, update, wait_control, control):
        self.paths = {key: environment / path for key, path in components.items()}
        self.root, self.save, self.check, self.update = root, save, check, update
        self.cli = self.paths['container']
        self.declaration = read(self.paths['backend'])
        self.vm = str(uuid.uuid4())
        self.attempt = str(uuid.uuid4())

    def call(self, *args):
        result = subprocess.run([self.cli, *map(str, args)], check=True, capture_output=True,
                                timeout=120, env={'PATH': '/usr/bin:/bin'})
        return result.stdout

    def inspect(self):
        record, = json.loads(self.call('inspect', self.vm))
        cfg = record['configuration']
        if record['id'] != self.vm or cfg['labels']['an-agent.attempt'] != self.attempt:
            raise RuntimeError('Original container identity changed')
        return record

    def prepare(self):
        self.check()
        server = subprocess.check_output(['/bin/launchctl', 'print',
            f'gui/{os.getuid()}/com.apple.container.apiserver'], text=True)
        paths = [line.strip()[10:] for line in server.splitlines() if line.strip().startswith('program = ')]
        if len(paths) != 1 or digest(Path(paths[0])) != digest(self.paths['container_apiserver']):
            raise RuntimeError('Start the matching offline Container control service')
        image = self.declaration['image']
        init = image_identity(self.paths['container_init'])
        if image_identity(self.paths['container_image']) != image:
            raise ValueError('Selected offline image differs from task declaration')
        for ref in (image, init):
            result, = json.loads(self.call('image', 'inspect', ref))
            if result['configuration']['descriptor']['digest'] != ref.split('@')[1]:
                raise RuntimeError('Provision the approved offline image before task execution')
        self.check()
        self.save(self.root/'setup.json', {'vm': self.vm, 'attempt': self.attempt, 'image': image})
        self.save(self.root/'start-intent.json', {'vm': self.vm, 'attempt': self.attempt})
        self.update(vm=self.vm)
        self.call('run', '-d', '--name', self.vm, '--network', 'none', '--no-dns',
                  '--cap-drop', 'ALL', '--cpus', self.declaration['cpus'],
                  '--memory', str(self.declaration['memory_mib'])+'M',
                  '--writable-layer-bytes', self.declaration['storage_bytes'],
                  '--label', 'an-agent.attempt='+self.attempt, '--label', 'an-agent.generation=1',
                  '--kernel', self.paths['container_kernel'], '--init-image', init,
                  '--entrypoint', '/bin/sleep', image, '900')
        record = self.inspect()
        cfg = record['configuration']
        resources = cfg['resources']
        if (record['status']['state'] != 'running'
                or resources['cpus'] != self.declaration['cpus']
                or resources['memoryInBytes'] != self.declaration['memory_mib'] * 1048576
                or resources['storage'] != self.declaration['storage_bytes']
                or cfg['image']['descriptor']['digest'] != image.split('@')[1]
                or any(cfg[k] != [] for k in ('networks','mounts','publishedPorts','publishedSockets'))):
            raise RuntimeError('Running container differs from approved task')
        self.call('exec', self.vm, '/bin/mkdir', '-p', '/workspace')
        ledger = self.root/'container-operations'
        ledger.mkdir(mode=0o700)
        path = self.root/'transport.json'
        self.save(path, {'vm': self.vm, 'generation': 1, 'caller': 'product-task',
            'apple_container': {'cli': str(self.cli), 'cli_sha256': digest(self.cli),
            'ledger': str(ledger), 'image': image, 'attempt': self.attempt,
            'created_date': cfg['creationDate'], 'started_date': record['status']['startedDate'],
            'expires_unix': int(time.time())+900}})
        return path

    def cleanup(self, state, credential, vm_bound, setup_bound, owners_confirmed, requested_outcome):
        try:
            if (self.root/'start-intent.json').exists():
                record = self.inspect()
                if record['status']['state'] == 'running':
                    self.call('stop', '--time', '1', self.vm)
                if self.inspect()['status']['state'] != 'stopped':
                    raise RuntimeError('Original container still active')
                if not owners_confirmed:
                    raise RuntimeError('Original job settlement pending')
                self.call('delete', self.vm)
                self.save(self.root/'container-reclaimed.json', {'vm': self.vm, 'attempt': self.attempt})
            if owners_confirmed and state['state']=='stopping':
                self.update(state=requested_outcome, termination_confirmed=True,
                            runtime_termination_confirmed=True)
        except Exception:
            self.update(state='unknown', error='Original container cleanup requires reconciliation')
