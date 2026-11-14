'use strict';
const token=location.hash.slice(1)||sessionStorage.getItem('an-local-token')||'';
if(token)sessionStorage.setItem('an-local-token',token);
history.replaceState(null,'',location.pathname);
let strings={},language='zh-CN',upload,preview;
const $=id=>document.getElementById(id);
async function request(path,body,raw=false){const response=await fetch(path,{method:body?'POST':'GET',headers:{Authorization:`Bearer ${token}`,...(!raw&&body?{'Content-Type':'application/json'}:{})},body:body?(raw?body:JSON.stringify(body)):undefined});const value=await response.json();if(!response.ok)throw new Error(value.error||response.statusText);return value;}
function render(){document.documentElement.lang=language;document.querySelectorAll('[data-text]').forEach(e=>e.textContent=strings[language][e.dataset.text]);}
async function refresh(){const view=await request('/view');$('environments').textContent=view.environments.map(e=>`${e.manifest.name} ${e.manifest.version}`).join(' · ');$('tasks').textContent=(view.host.tasks||[]).map(t=>`${t.label||t.id}: ${t.outcome||t.state}`).join('\n');}
async function act(callback){$('error').textContent='';try{await callback();}catch(error){$('error').textContent=error.message;} }
$('language').onchange=e=>{language=e.target.value;render();};
$('inspect').onclick=()=>act(async()=>{preview=null;$('approve').hidden=true;const file=$('bundle').files[0];if(!file)throw new Error(strings[language].choose);$('status').textContent=strings[language].uploading;upload=(await request('/upload',file,true)).upload;preview=await request('/action',{op:'environment-preview',upload});$('preview').textContent=JSON.stringify(preview.manifest,null,2);$('approve').hidden=false;$('status').textContent='';});
$('approve').onclick=()=>act(async()=>{await request('/action',{op:'environment-import',upload,approve:preview.approval_sha256});$('approve').hidden=true;$('status').textContent=strings[language].imported;await refresh();});
act(async()=>{strings=await (await fetch('/strings.json')).json();render();await refresh();});
