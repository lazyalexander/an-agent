"""Private evidence driver for the original daemon; no model loop or retry policy."""
import hashlib
import json
from pathlib import Path
import signal
import subprocess
import time


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class Driver:
    def __init__(self, host, root):
        self.host, self.root = Path(host).resolve(strict=True), Path(root).resolve()
        self.root.mkdir(mode=0o700)
        self.service = self.root / 'service'
        self.calls = []
        self.call('daemon', 'init', self.service, 3)
        self.log = (self.root / 'daemon.log').open('w')
        self.process = subprocess.Popen([str(self.host), 'daemon', 'serve', str(self.service)], stdout=self.log, stderr=self.log)
        deadline = time.monotonic() + 10
        while not (self.service / 'endpoint.json').exists():
            assert self.process.poll() is None and time.monotonic() < deadline
            time.sleep(.025)

    def call(self, *args, ok=True):
        result = subprocess.run([str(self.host), *map(str, args)], capture_output=True, text=True, timeout=30)
        assert (result.returncode == 0) == ok, (args, result.stdout, result.stderr)
        return json.loads(result.stdout) if ok else result.stderr

    def request(self, value, credential=None, ok=True):
        path = self.root / 'controller-request.json'
        path.write_text(json.dumps({'credential': credential, 'request': value} if credential else value))
        result = self.call('daemon', 'task-request' if credential else 'request', self.service, path, ok=ok)
        self.calls.append({'op': value['op'], 'ok': ok})
        return result

    def approve(self, value, credential=None):
        preview = self.request(value, credential)
        return self.request(dict(value, approve=preview['approval_sha256']), credential)

    def wait(self, job, credential, outcome='succeeded'):
        deadline = time.monotonic() + 210
        while time.monotonic() < deadline:
            view = self.request({'op': 'task', 'id': job}, credential)
            if view['state'] == 'settled':
                assert view['outcome'] == outcome, view
                return view
            time.sleep(.15)
        raise AssertionError('Unknown original attempt; retain evidence, never replay')

    def ready(self, job):
        deadline = time.monotonic() + 40
        marker = self.service / 'jobs' / job / 'native-events.jsonl'
        while not marker.exists() or 'turn/started' not in marker.read_text():
            assert time.monotonic() < deadline, 'No original turn readiness; retain without replay'
            time.sleep(.05)

    def profile(self, task):
        return self.request({'op': 'harness-permission', 'task': task})

    def grant(self, rights, subject='main', delegate=False, parent=None):
        scope = {'task': 'document-study', 'subject': subject, 'expires_ms': self.expires,
                 'delegate': delegate, 'permissions': rights}
        return self.approve({'op': 'derive-grant' if parent else 'grant', 'scope': scope}, parent)['credential']

    def task(self, binary, prompt, scope=None, schema=None):
        return {'app_server': True, 'label': 'document-study', 'binary': str(binary), 'binary_sha256': sha(binary),
                'prompt': prompt, 'scope': scope or {}, 'model': None, 'mcp': None,
                'schema': schema or {'type': 'object', 'properties': {'ok': {'type': 'boolean'}},
                                     'required': ['ok'], 'additionalProperties': False}}

    def close(self):
        if self.process.poll() is None:
            self.process.send_signal(signal.SIGTERM)
            self.process.wait(timeout=15)
        self.log.close()
