'use strict';
const token=location.hash.slice(1)||sessionStorage.getItem('an-local-token')||'';
if(token)sessionStorage.setItem('an-local-token',token);
history.replaceState(null,'',location.pathname);
let strings={},language='zh-CN',upload,preview,selectedTask,taskView,controlPreview;
const $=id=>document.getElementById(id);
async function request(path,body,raw=false){const response=await fetch(path,{method:body?'POST':'GET',headers:{Authorization:`Bearer ${token}`,...(!raw&&body?{'Content-Type':'application/json'}:{})},body:body?(raw?body:JSON.stringify(body)):undefined});const value=await response.json();if(!response.ok)throw new Error(value.error||response.statusText);return value;}
function stateText(value){return strings[language]["state_"+value]||value;}
function render(){document.documentElement.lang=language;document.querySelectorAll('[data-text]').forEach(e=>e.textContent=strings[language][e.dataset.text]);document.querySelectorAll("[data-aria]").forEach(e=>e.setAttribute("aria-label",strings[language][e.dataset.aria]));}
async function refresh(){const view=await request('/view');$('capacity').hidden=view.host.concurrency>=2;$('environments').textContent=view.environments.map(e=>`${e.manifest.name} ${e.manifest.version}`).join(' · ');const tasks=view.host.tasks||[];$('tasks').replaceChildren();
if(!tasks.length)$('tasks').textContent=strings[language].noTasks;
for(const task of tasks){const button=document.createElement('button');button.textContent=`${task.label||task.id} · ${stateText(task.outcome||task.state)}`;button.onclick=()=>act(()=>selectTask(task.id));$('tasks').append(button);}
if(selectedTask)await selectTask(selectedTask);await renderDeliveries(view);}
async function act(callback){$('error').textContent='';try{await callback();}catch(error){$('error').textContent=error.message;} }
$('language').onchange=e=>{language=e.target.value;render();};
$('inspect').onclick=()=>act(async()=>{preview=null;$('approve').hidden=true;const file=$('bundle').files[0];if(!file)throw new Error(strings[language].choose);$('status').textContent=strings[language].uploading;upload=(await request('/upload',file,true)).upload;preview=await request('/action',{op:'environment-preview',upload});$('preview').textContent=JSON.stringify(preview.manifest,null,2);$('approve').hidden=false;$('status').textContent='';});
$('approve').onclick=()=>act(async()=>{await request('/action',{op:'environment-import',upload,approve:preview.approval_sha256});$('approve').hidden=true;$('status').textContent=strings[language].imported;await refresh();});
act(async()=>{strings=await (await fetch('/strings.json')).json();render();$('delivery-instruction').value=strings[language].defaultInstruction;await refresh();});

async function selectTask(id){
 if(selectedTask!==id){controlPreview=null;$('control-preview').hidden=true;}
 selectedTask=id;const result=await request('/action',{op:'task-view',id});
 if(selectedTask!==id)return;taskView=result;
 $('task-detail').hidden=false;
 $('task-state').textContent=`${taskView.task.label||id} · ${stateText(taskView.task.outcome||taskView.task.state)}`;
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

let deliveryPlan,deliveryId;
async function renderDeliveries(view){
 const select=$('delivery-environment'),selected=select.value;select.replaceChildren();
 for(const environment of view.environments){if(!environment.manifest.components.delivery)continue;const option=document.createElement('option');option.value=environment.id;option.textContent=`${environment.manifest.name} ${environment.manifest.version}`;select.append(option);}
 if([...select.options].some(o=>o.value===selected))select.value=selected;
 $('deliveries').replaceChildren();
 for(const delivery of view.deliveries||[]){const button=document.createElement('button');button.textContent=`${delivery.id.slice(0,8)} · ${stateText(delivery.state.state)}`;button.onclick=()=>act(()=>showDelivery(delivery.id));$('deliveries').append(button);}
 if(deliveryId)await showDelivery(deliveryId);
}
$('delivery-preview').onclick=()=>act(async()=>{
 const file=$('delivery-file').files[0],environment=$('delivery-environment').value;
 if(!environment)throw new Error(strings[language].noEnvironment);
 if(!file)throw new Error(language==='en'?'Select a PDF':'请选择 PDF');
 if(file.size>524288)throw new Error(language==='en'?'PDF limit: 512 KiB':'PDF 大小上限为 512 KiB');
 const upload=(await request('/upload',file,true)).upload;
 const value={op:'delivery-preview',id:crypto.randomUUID(),environment,upload,instruction:$('delivery-instruction').value};
 const checked=await request('/action',value);deliveryPlan={...value,op:'delivery-create',approve:checked.approval_sha256};
 $('delivery-description').textContent=`${file.name} · ${checked.plan.model} · ${checked.plan.wall_seconds}s\n${strings[language].scopeNotice}`;
 $('delivery-plan').hidden=false;
});
$('delivery-confirm').onclick=()=>act(async()=>{
 $('delivery-confirm').disabled=true;
 try{const result=await request('/action',deliveryPlan);$('delivery-plan').hidden=true;await showDelivery(result.id);}
 finally{$('delivery-confirm').disabled=false;}
});
async function showDelivery(id){
 if(deliveryId!==id)$('clarification-form').replaceChildren();deliveryId=id;
 const value=await request('/action',{op:'delivery-view',id});if(deliveryId!==id)return;
 const state=value.state;$('delivery-detail').hidden=false;$('delivery-state').textContent=stateText(state.state)+(state.error?' · '+state.error:'');
 $('delivery-reconcile').hidden=state.state!=='unknown';
 $('delivery-cancel').disabled=['succeeded','cancelled','failed'].includes(state.state);
 const form=$('clarification-form');
 if(state.state==='waiting-clarification'&&!form.children.length){
  for(const field of state.clarifications){const label=document.createElement('label');label.textContent=(field.label[language]||field.label.en)+(field.source_label?' · '+field.source_label:'');const input=document.createElement(field.type==='enum'?'select':'input');input.dataset.question=field.key;input.required=true;if(field.type==='enum')for(const choice of field.choices){const option=document.createElement('option');option.value=choice;option.textContent=choice;input.append(option);}else{input.type='number';input.min=field.minimum;input.max=field.maximum;}label.append(input);form.append(label,document.createElement('br'));}
 }
 $('delivery-answer').hidden=state.state!=='waiting-clarification';if(state.state!=='waiting-clarification')form.replaceChildren();
 $('delivery-products').replaceChildren();
 if(state.state==='succeeded')for(const stage of ['intermediate','final']){
  const button=document.createElement('button');button.textContent=strings[language].download+' '+(stage==='intermediate'?'Markdown':'Excel');
  button.onclick=()=>act(async()=>{const artifact=await request('/action',{op:'delivery-artifact',id,stage});const bytes=Uint8Array.from(artifact.bytes);const url=URL.createObjectURL(new Blob([bytes]));const link=document.createElement('a');link.href=url;link.download=stage==='intermediate'?'result.md':'result.xlsx';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});$('delivery-products').append(button);
 }
}
$('delivery-answer').onclick=()=>act(async()=>{
 const answers={};$('clarification-form').querySelectorAll('input,select').forEach(input=>{answers[input.dataset.question]=input.dataset.question==='currency'?input.value:Number(input.value);});
 const value={op:'delivery-clarify',id:deliveryId,answers};const preview=await request('/action',value);await request('/action',{...value,approve:preview.approval_sha256});await showDelivery(deliveryId);
});
$('delivery-cancel').onclick=()=>act(async()=>{await request('/action',{op:'delivery-cancel',id:deliveryId});await showDelivery(deliveryId);});

let capacityPlan;
$('capacity').onclick=()=>act(async()=>{const value={op:'delivery-capacity',command:crypto.randomUUID()};const preview=await request('/action',value);if(preview.ready)return;capacityPlan={...value,approve:preview.approval_sha256};$('capacity-description').textContent=strings[language].capacityNotice;$('capacity-confirm').hidden=false;});
$('capacity-confirm').onclick=()=>act(async()=>{await request('/action',capacityPlan);$('capacity-confirm').hidden=true;$('capacity-description').textContent='';await refresh();});

$('delivery-reconcile').onclick=()=>act(async()=>{await request('/action',{op:'delivery-reconcile',id:deliveryId});await showDelivery(deliveryId);});
