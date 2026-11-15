"""Loopback-only, token-authenticated, read-only qualification projection."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import secrets
from store import write

PAGE = '''<!doctype html><meta charset="utf-8"><title>Qualification</title>
<style>body{font:16px system-ui;max-width:1000px;margin:3rem auto;background:#f5f7fa;color:#17212b}button{padding:.6rem}pre{background:white;padding:1rem;white-space:pre-wrap}</style>
<button id="language">中文 / English</button><h1 id="title"></h1><p id="connection"></p><pre id="runs"></pre>
<script>
const token=location.hash.slice(1);history.replaceState(null,'',location.pathname);let zh=true;
const title=document.querySelector('#title'),connection=document.querySelector('#connection'),runs=document.querySelector('#runs');
document.querySelector('#language').onclick=()=>{zh=!zh;refresh()};
async function refresh(){title.textContent=zh?'后台验收':'Background qualification';try{let r=await fetch('/status',{headers:{Authorization:'Bearer '+token}});if(!r.ok)throw Error();let v=await r.json();connection.textContent=zh?'已连接 · 只读':'Connected · read only';runs.textContent=JSON.stringify(v,null,2)}catch(e){connection.textContent=zh?'连接中断，任务状态未知':'Disconnected; task status unknown'}}refresh();setInterval(refresh,2000);
</script>'''


def serve(root):
    from qualify import status
    token = secrets.token_urlsafe(32)
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            authority = '%s:%s' % self.server.server_address
            if self.headers.get('Host') != authority or self.headers.get('Origin') not in (None, 'http://' + authority):
                self.send_error(403); return
            if self.path == '/':
                body, content = PAGE.encode(), 'text/html; charset=utf-8'
            elif self.path == '/status' and self.headers.get('Authorization') == 'Bearer ' + token:
                view = status(root)
                # Allowlist only public projection fields, not errors, paths or raw reports.
                fields = ('id', 'candidate', 'phase', 'state', 'qualification', 'active_phase', 'counts', 'submitted', 'finished')
                view['runs'] = [{k: r[k] for k in fields if k in r} for r in view['runs']]
                body, content = json.dumps(view).encode(), 'application/json'
            else:
                self.send_error(403); return
            self.send_response(200); self.send_header('Content-Type', content)
            self.send_header('Cache-Control', 'no-store'); self.send_header('X-Content-Type-Options', 'nosniff')
            self.send_header('Content-Length', str(len(body))); self.end_headers(); self.wfile.write(body)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    url = 'http://127.0.0.1:%s/#%s' % (server.server_port, token)
    write(root / 'dashboard.json', {'url': url})
    print(json.dumps({'url': url}), flush=True)
    server.serve_forever()
