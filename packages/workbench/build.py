"""Compile the standalone workbench with the installed TypeScript compiler."""
from pathlib import Path
from base64 import b64encode
import json
import subprocess
import tempfile

root = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory() as tmp:
    script = Path(tmp) / 'studio.js'
    subprocess.run(['tsc', '--strict', '--target', 'ES2022', '--lib', 'ES2022,DOM',
                    '--outDir', tmp, str(root / 'studio.ts')], check=True)
    page = (root / 'workbench.html').read_text()
    icon = (root.parents[1] / 'assets/icon.svg').read_bytes()
    page = page.replace('__APP_ICON__', 'data:image/svg+xml;base64,' + b64encode(icon).decode())
    page = page.replace('/* STYLES */', (root / 'workbench.css').read_text())
    translations = json.loads((root / 'zh-CN.json').read_text())
    payload = json.dumps(translations, ensure_ascii=True).replace('<', r'\u003c')
    page = page.replace('/* SCRIPT */', 'const translations = ' + payload + ';\n' + script.read_text())
    (root / 'index.html').write_text(page)
