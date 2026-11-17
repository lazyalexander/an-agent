"""Install verified fork products without registering or controlling shared services."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

from container_build import digest, output, validate


PLUGINS = {
    'container-runtime-linux': ('RuntimeLinux', 'container-runtime-linux'),
    'container-network-vmnet': ('NetworkVmnet', 'container-network-vmnet'),
    'container-core-images': ('CoreImages', 'container-core-images'),
    'container-core-clusters': ('CoreClusters', 'container-core-clusters'),
    'machine-apiserver': ('MachineAPIServer', 'machine-apiserver'),
    'cluster': ('Cluster', 'cluster'),
    'cluster-hoststore': ('ClusterHostStore', 'cluster-hoststore'),
    'container-build': ('ContainerBuild', 'build'),
    'container-builder': ('ContainerBuilder', 'builder'),
}


def copy(source, target):
    if source.is_symlink() or not source.is_file():
        raise ValueError('Expected a regular installation input')
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)


def install(source, build_record):
    record = json.loads(build_record.read_bytes())
    validate(source, record['source'])
    if record['schema'] != 'an-container-build-v1':
        raise ValueError('Unsupported build evidence')
    build = Path(output(source, '/usr/bin/swift', 'build', '-c', 'release', '--show-bin-path'))
    for name, entry in record['binaries'].items():
        if name not in {'container', 'container-apiserver', *PLUGINS}:
            raise ValueError('Unexpected build product')
        if digest(build / name) != entry['sha256']:
            raise ValueError('Build product changed: ' + name)
    if set(record['binaries']) != {'container', 'container-apiserver', *PLUGINS}:
        raise ValueError('Incomplete build products')
    parent = Path.home() / 'Library/Application Support/an-agent/container'
    for path in [parent, *parent.parents]:
        if path.is_symlink():
            raise ValueError('Installation ancestor is a symlink')
    parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if parent.stat().st_uid != os.getuid() or parent.stat().st_mode & 0o077:
        raise ValueError('Installation store must be private and owned by this user')
    identity = digest(build_record)
    target = parent / identity
    temporary = parent / ('.pending-' + identity)
    if target.exists():
        raise ValueError('Installation already exists; verify the original manifest')
    temporary.mkdir(mode=0o700)
    for name in record['binaries']:
        if name in PLUGINS:
            source_plugin, plugin = PLUGINS[name]
            directory = temporary / 'libexec/container/plugins' / plugin
            executable = directory / 'bin' / plugin
            copy(source / 'Sources/Plugins' / source_plugin / 'config.toml', directory / 'config.toml')
        else:
            executable = temporary / 'bin' / name
        copy(build / name, executable)
        if digest(executable) != record['binaries'][name]['sha256']:
            raise ValueError('Build product changed while copying')
        executable.chmod(0o500)
        command = ['/usr/bin/codesign', '--force', '--sign', '-', '--timestamp=none',
                   '--identifier', 'com.apple.container.' + ('cli' if name == 'container' else name.removeprefix('container-'))]
        if name in ('container-runtime-linux', 'container-network-vmnet'):
            command += ['--entitlements', str(source / 'signing' / (name + '.entitlements'))]
        subprocess.run([*command, str(executable)], check=True)
        subprocess.run(['/usr/bin/codesign', '--verify', '--strict', str(executable)], check=True)
    for plugin, resource in [('MachineAPIServer', 'init'), ('Cluster', 'kindnet.yaml')]:
        destination = 'machine-apiserver' if plugin == 'MachineAPIServer' else 'cluster'
        copy(source / 'Sources/Plugins' / plugin / 'Resources' / resource,
             temporary / 'libexec/container/plugins' / destination / 'resources' / resource)
        if resource == 'init':
            (temporary / 'libexec/container/plugins' / destination / 'resources' / resource).chmod(0o500)
    for name in ('LICENSE', 'NOTICE.md', 'Package.resolved'):
        copy(source / name, temporary / 'provenance' / name)
    copy(build_record, temporary / 'provenance/build.json')
    files = {str(path.relative_to(temporary)): {'sha256': digest(path), 'bytes': path.stat().st_size}
             for path in sorted(temporary.rglob('*')) if path.is_file()}
    (temporary / 'installation.json').write_text(json.dumps({
        'schema': 'an-container-installation-v1', 'build_sha256': identity,
        'files': files, 'services_started': False, 'runtime_qualified': False}, indent=2) + '\n')
    for path in temporary.rglob('*'):
        if path.is_file():
            with path.open('rb') as stream:
                os.fsync(stream.fileno())
    temporary.rename(target)
    descriptor = os.open(parent, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    return target


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('build_record', type=Path)
    args = parser.parse_args()
    print(install(args.source.resolve(), args.build_record))
