"""Prepare reviewed Container images using an existing local fork installation."""
import fcntl
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tarfile


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1048576), b''):
            h.update(block)
    return h.hexdigest()


def run(args):
    return subprocess.run(args, capture_output=True, text=True, timeout=300, env={'PATH': '/usr/bin:/bin'})


def checked(args):
    result = run(args)
    if result.returncode:
        raise ValueError(result.stderr.strip()[:500] or 'Container command failed')
    return result.stdout


def reference(path):
    with tarfile.open(path) as archive:
        entry = archive.getmember('index.json')
        if not entry.isfile() or entry.size > 65536:
            raise ValueError('Invalid OCI index')
        desc, = json.load(archive.extractfile(entry))['manifests']
    annotations = desc['annotations']
    tag = annotations['org.opencontainers.image.ref.name']
    if annotations.get('io.containerd.image.name', tag) != tag:
        raise ValueError('Conflicting OCI image names')
    if not re.fullmatch(r'sha256:[a-f0-9]{64}', desc['digest']):
        raise ValueError('Invalid OCI digest')
    if tag.startswith('-') or not tag.isascii() or any(c.isspace() for c in tag):
        raise ValueError('Invalid OCI reference')
    prefix, slash, last = tag.partition('@')[0].rpartition('/')
    return tag, prefix + slash + last.partition(':')[0] + '@' + desc['digest'], desc['digest']


def inspect(cli, ref, service_root):
    result = run([cli, 'image', 'inspect', ref])
    if result.returncode:
        if 'image not found' in result.stderr.lower():
            return None
        raise ValueError(result.stderr.strip()[:500])
    images = json.loads(result.stdout)
    if not images:
        # ImageInspect omits infrastructure tags. Read the active service's index.
        return json.loads((service_root/'state.json').read_bytes())[ref]['digest']
    image, = images
    return image['configuration']['descriptor']['digest']


def main():
    env = Path(sys.argv[1])
    manifest = json.loads((env/'runtime.json').read_bytes())
    components = manifest['components']
    expected = manifest['files'][components['container']]['sha256']
    store = Path.home()/'Library/Application Support/an-agent/container'
    for receipt in sorted(store.glob('*/installation.json')):
        inst = receipt.parent
        files = json.loads(receipt.read_bytes())['files']
        if files['bin/container']['sha256'] == expected:
            break
    else:
        raise ValueError('Matching Container installation required')
    expected_components = {
        'container_apiserver': 'bin/container-apiserver',
        'container_runtime_linux': 'libexec/container/plugins/container-runtime-linux/bin/container-runtime-linux',
        'container_network_vmnet': 'libexec/container/plugins/container-network-vmnet/bin/container-network-vmnet',
        'container_images': 'libexec/container/plugins/container-core-images/bin/container-core-images',
    }
    for key, name in expected_components.items():
        if manifest['files'][components[key]]['sha256'] != files[name]['sha256']:
            raise ValueError('Installed control components differ from the environment')
    cli = inst/'bin/container'
    apply = sys.argv[2] == 'apply'
    with (store/'.prepare.lock').open('a+') as lock:
        return prepare_locked(env, manifest, components, inst, files, cli, store, apply, lock)


def prepare_locked(env, manifest, components, inst, files, cli, store, apply, lock):
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        lock.seek(0)
        fields = lock.read(256).split()
        step = fields[1] if len(fields)==2 and fields[0]==env.name else ''
        return ['busy', step] if step in ('start','container_init','container_image') else ['busy']
    def progress(step):
        if step not in json.loads(sys.argv[3]):
            raise ValueError('Environment changed; review the required steps again')
        lock.seek(0)
        lock.truncate()
        lock.write(env.name+' '+step)
        lock.flush()
    if apply:
        for name, record in files.items():
            path = inst/name
            if path.is_symlink() or not path.resolve().is_relative_to(inst.resolve()) or sha(path) != record['sha256']:
                raise ValueError('Installed Container files changed')
    # Observe the original launchd owner before touching any shared service.
    label = f'gui/{os.getuid()}/com.apple.container.apiserver'
    service_root = None
    def active():
        nonlocal service_root
        r = run(['/bin/launchctl','print',label])
        if r.returncode:
            if 'could not find service' in r.stderr.lower():
                return False
            raise ValueError('Container service ownership unavailable')
        if ('program = '+str(inst/'bin/container-apiserver')) not in [line.strip() for line in r.stdout.splitlines()]:
            raise ValueError('Another Container installation owns the control service')
        plist, = [Path(line.strip()[7:]) for line in r.stdout.splitlines() if line.strip().startswith('path = ')]
        service_root = plist.parent.parent
        return True
    steps = []
    live = active()
    if not live:
        steps.append('start')
        if apply:
            progress('start')
            checked([cli,'system','start','--offline','--install-root',inst,'--app-root',store/'service'])
            live = active()
            if not live:
                raise ValueError('Container service did not start')
    for key in ('container_init','container_image'):
        archive = env/components[key]
        tag, ref, digest = reference(archive)
        if key == 'container_image' and json.loads((env/components['backend']).read_bytes())['image'] != ref:
            raise ValueError('Image differs from approved environment')
        existing = inspect(cli,ref,service_root) if live else None
        if existing == digest:
            continue
        if existing is not None:
            raise ValueError('Conflicting image digest')
        steps.append(key)
        if apply:
            progress(key)
            previous = inspect(cli,tag,service_root)
            if previous is None:
                checked([cli,'image','load','-i',archive])
            if inspect(cli,tag,service_root) != digest:
                raise ValueError('Conflicting image tag')
            checked([cli,'image','tag',tag,ref])
            if inspect(cli,ref,service_root) != digest:
                raise ValueError('Image digest registration failed')
    return [] if apply else steps


if __name__ == '__main__':
    try:
        print(json.dumps(main()))
    except Exception as error:
        sys.exit(str(error))
