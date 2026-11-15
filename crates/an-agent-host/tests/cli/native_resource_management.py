#!/usr/bin/env python3
"""Reclaim selected, host-accepted task copies without changing product custody."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time
import urllib.request
import uuid
from host_qualification import require_native_workspace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('host', 'service', 'evidence'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--task', action='append', required=True)
    parser.add_argument('--retire-image', required=True)
    args = parser.parse_args(); require_native_workspace()
    root = args.evidence.resolve(); root.mkdir(mode=0o700)
    service = args.service.resolve(strict=True)
    assert not (service / 'endpoint.json').exists(), 'Stop the owned prior service before this acceptance'
    endpoint = service / 'application/endpoint.json'
    previous = json.loads(endpoint.read_text())['url'] if endpoint.exists() else None
    log = (root / 'service.log').open('w')
    page = subprocess.Popen([str(args.host.resolve(strict=True)), 'app', 'serve', str(service)], stdout=log, stderr=log)
    try:
        endpoint = service / 'application/endpoint.json'
        deadline = time.monotonic() + 20
        while True:
            assert page.poll() is None and time.monotonic() < deadline
            try:
                url = json.loads(endpoint.read_text())['url']
                if url != previous:
                    base, token = url.split('/#'); break
            except (FileNotFoundError, json.JSONDecodeError): time.sleep(.025)
        def call(value):
            request = urllib.request.Request(base + '/action', data=json.dumps(value).encode(), headers={
                'Authorization': 'Bearer ' + token, 'Origin': base, 'Content-Type': 'application/json'})
            with urllib.request.urlopen(request, timeout=300) as response: return json.load(response)
        before = {}
        for task in args.task:
            before[task] = {stage: call({'op': 'delivery-artifact', 'id': task, 'stage': stage})['content_sha256']
                            for stage in ('intermediate', 'final')}
        plan = json.loads((service / 'application/tasks' / args.task[0] / 'plan.json').read_text())
        environment = plan['environment_digest']
        def inventory(): return call({'op': 'image-inventory', 'environment': environment})
        def cleanup(action, resource):
            value = {'op': 'image-preview', 'id': str(uuid.uuid4()), 'environment': environment,
                     'action': action, 'resource': resource}
            preview = call(value)
            request = dict(value, op='image-submit', approve=preview['approval_sha256'])
            call(request)
            deadline = time.monotonic() + 120
            while True:
                # The public view queries the original receipt. Never resubmit a deletion.
                view_request = urllib.request.Request(base + '/view', headers={'Authorization': 'Bearer ' + token})
                with urllib.request.urlopen(view_request, timeout=300) as response: view = json.load(response)
                record = next(item for item in view['images'] if item['id'] == value['id'])
                if record['state'] in ('succeeded', 'failed', 'cancelled', 'unknown'): break
                assert time.monotonic() < deadline; time.sleep(.1)
            assert record['state'] == 'succeeded', record
            assert call(request) == record
            return record
        initial = inventory(); assert initial['supported']
        records = []
        for task in args.task:
            setup = json.loads((service / 'application/tasks' / task / 'setup.json').read_text())
            records.append(cleanup('reclaim_vm', setup['vm']))
        records.append(cleanup('retire', args.retire_image))
        final = inventory()
        assert next(item for item in final['resources'] if item['id'] == args.retire_image)['retired']
        for task in args.task:
            for stage, expected in before[task].items():
                artifact = call({'op': 'delivery-artifact', 'id': task, 'stage': stage})
                assert artifact['content_sha256'] == expected
                assert hashlib.sha256(bytes(artifact['bytes'])).hexdigest() == expected
        (root / 'report.json').write_text(json.dumps({'passed': True, 'original_operations': records,
            'task_count': len(args.task), 'accepted_products_unchanged': True, 'inventory_before': initial,
            'inventory_after': final}, indent=2))
    finally:
        page.terminate(); page.wait(timeout=15)
        endpoint = service / 'endpoint.json'
        if endpoint.exists():
            pid = json.loads(endpoint.read_text())['pid']; os.kill(pid, signal.SIGTERM)
            deadline = time.monotonic() + 15
            while endpoint.exists() and time.monotonic() < deadline: time.sleep(.025)
            assert not endpoint.exists(), 'Original daemon stop unconfirmed'
        log.close()


if __name__ == '__main__': main()
