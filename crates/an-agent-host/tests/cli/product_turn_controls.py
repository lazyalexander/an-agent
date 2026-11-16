"""Task-owned fault injection through the existing product and daemon controls."""
import hashlib
import json
import subprocess
import time
import uuid


class TurnControls:
    def __init__(self, mode, host, service, evidence):
        self.mode = mode
        self.host = host
        self.service = service
        self.evidence = evidence
        self.result = None

    def observe(self, state, call, approve):
        if not self.mode or self.result is not None or not state.get('session'):
            return False
        job = self.service / 'jobs' / state['session']
        operation = state.get('operations', {}).get('extract')
        if not operation:
            return False
        directory = job / 'cocoon-jobs' / operation
        key = hashlib.sha256((operation + ':execute').encode()).hexdigest()[:32]
        marker = directory / 'operations' / (key + '.json')
        # Qualify the outstanding execution boundary, not VM preparation.
        if not marker.exists() or marker.with_suffix('.result.json').exists():
            return False
        started = time.monotonic()
        self.started = started
        if self.mode == 'steer':
            view = call({'op': 'task-view', 'id': state['session']})['session']
            assert view['turns']['active'] is not None, 'Original turn already completed'
            request = {'op': 'task-control', 'control': 'supplement', 'id': state['session'],
                       'execution': view['execution'], 'after_turn': view['turns']['completed'],
                       'context_revision': view['context']['snapshot']['revision'],
                       'command': str(uuid.uuid4()),
                       'instruction': 'Preserve all source facts, row order, EUR currency and formulas. Continue the original task.'}
            response = approve(request)
        elif self.mode == 'revoke-spool':
            context = json.loads((job / 'context-initial.json').read_text())
            grant = next(b['grant_ref']['id'] for b in context['spool_bindings'] if b['name'] == 'intermediate')
            request = {'op': 'revoke-grant', 'id': grant}
            path = self.evidence / 'revoke-request.json'
            def daemon(value):
                path.write_text(json.dumps(value))
                result = subprocess.run([str(self.host), 'daemon', 'request', str(self.service), str(path)],
                                        capture_output=True, text=True, timeout=15)
                assert result.returncode == 0, result.stderr
                return json.loads(result.stdout)
            preview = daemon(request)
            response = daemon(dict(request, approve=preview['approval_sha256']))
        else:
            response = call({'op': 'delivery-cancel', 'id': self.task})
        elapsed = (time.monotonic() - started) * 1000
        self.result = {'mode': self.mode, 'operation': operation, 'session': state['session'],
                       'boundary': 'execute-request-without-receipt', 'admission_ms': elapsed,
                       'response': response}
        (self.evidence / 'turn-control.json').write_text(json.dumps(self.result, indent=2))
        assert elapsed < 1500, ('Control admission exceeded 1500 ms', elapsed)
        return self.mode == 'cancel-program'

    def verify(self, state):
        if not self.mode:
            return
        assert self.result is not None, 'Required execution boundary was not observed'
        assert state['session'] == self.result['session'], 'Control replaced original session'
        if self.mode == 'revoke-spool':
            assert state['state'] == 'failed' and not state.get('products'), state
        if self.mode != 'steer':
            self.result['termination_seconds'] = time.monotonic() - self.started
            assert self.result['termination_seconds'] <= 30, 'Original execution termination exceeded 30 seconds'
        self.result['termination_confirmed'] = state['termination_confirmed']
        self.result['runtime_termination_confirmed'] = state['runtime_termination_confirmed']
        (self.evidence / 'turn-control.json').write_text(json.dumps(self.result, indent=2))
