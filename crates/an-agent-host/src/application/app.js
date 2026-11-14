'use strict';
const token=location.hash.slice(1)||sessionStorage.getItem('an-local-token')||'';
if(token)sessionStorage.setItem('an-local-token',token);
history.replaceState(null,'',location.pathname);
let strings={},language='zh-CN',upload,preview,selectedTask,taskView,controlPreview;
const $=id=>document.getElementById(id);
async function request(path,body,raw=false){const response=await fetch(path,{method:body?'POST':'GET',headers:{Authorization:`Bearer ${token}`,...(!raw&&body?{'Content-Type':'application/json'}:{})},body:body?(raw?body:JSON.stringify(body)):undefined});const value=await response.json();if(!response.ok)throw new Error(value.error||response.statusText);return value;}
function render(){document.documentElement.lang=language;document.querySelectorAll('[data-text]').forEach(e=>e.textContent=strings[language][e.dataset.text]);}
async function refresh(){const view=await request('/view');$('environments').textContent=view.environments.map(e=>`${e.manifest.name} ${e.manifest.version}`).join(' · ');const tasks=view.host.tasks||[];$('tasks').replaceChildren();
if(!tasks.length)$('tasks').textContent=strings[language].noTasks;
for(const task of tasks){const button=document.createElement('button');button.textContent=`${task.label||task.id} · ${task.outcome||task.state}`;button.onclick=()=>act(()=>selectTask(task.id));$('tasks').append(button);}
if(selectedTask)await selectTask(selectedTask);}
async function act(callback){$('error').textContent='';try{await callback();}catch(error){$('error').textContent=error.message;} }
$('language').onchange=e=>{language=e.target.value;render();};
$('inspect').onclick=()=>act(async()=>{preview=null;$('approve').hidden=true;const file=$('bundle').files[0];if(!file)throw new Error(strings[language].choose);$('status').textContent=strings[language].uploading;upload=(await request('/upload',file,true)).upload;preview=await request('/action',{op:'environment-preview',upload});$('preview').textContent=JSON.stringify(preview.manifest,null,2);$('approve').hidden=false;$('status').textContent='';});
$('approve').onclick=()=>act(async()=>{await request('/action',{op:'environment-import',upload,approve:preview.approval_sha256});$('approve').hidden=true;$('status').textContent=strings[language].imported;await refresh();});
act(async()=>{strings=await (await fetch('/strings.json')).json();render();await refresh();});

async function selectTask(id){
 if(selectedTask!==id){controlPreview=null;$('control-preview').hidden=true;}
 selectedTask=id;const result=await request('/action',{op:'task-view',id});
 if(selectedTask!==id)return;taskView=result;
 $('task-detail').hidden=false;
 $('task-state').textContent=`${taskView.task.label||id} · ${taskView.task.outcome||taskView.task.state}`;
 $('cancel').disabled=taskView.task.state==='settled';
 $('supplement').disabled=!taskView.session||taskView.task.state==='settled';
}
async function prepareControl(control){
 if(!taskView)return;
 const value={op:'task-control',control,id:selectedTask};
 if(control==='supplement')Object.assign(value,{instruction:$('instruction').value,command:crypto.randomUUID(),execution:taskView.session.execution,after_turn:taskView.session.turns.completed});
 const plan=await request('/action',value);controlPreview={...value,approve:plan.approval_sha256};
 $('control-description').textContent=`${strings[language][control]} · ${taskView.task.label||selectedTask}${control==='supplement'?'\n'+value.instruction:''}`;
 $('control-preview').hidden=false;
}
$('cancel').onclick=()=>act(()=>prepareControl('cancel'));
$('supplement').onclick=()=>act(()=>prepareControl('supplement'));
$('confirm-control').onclick=()=>act(async()=>{
 const original=controlPreview;controlPreview=null;$('control-preview').hidden=true;
 await request('/action',original);$('status').textContent=strings[language].sent;await refresh();
});
setInterval(()=>{if(strings[language])act(refresh);},3000);
