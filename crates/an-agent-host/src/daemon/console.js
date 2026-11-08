// Read-only projection. Untrusted labels and diagnostics never become HTML.
const token=location.hash.slice(1);history.replaceState(null,'',location.pathname);
const node=id=>document.getElementById(id);
const labels={'queued':'排队中','running-or-unconfirmed':'执行中或待核实','settled':'已结算','active':'可接单','paused':'已暂停接单','suspended':'已休眠','accepted':'已验收','awaiting-acceptance':'待验收','not-accepted':'未验收','unconfirmed':'待核实','succeeded':'成功','cancelled':'已取消','failed':'失败','timed_out':'超时'};
const label=value=>labels[value]??value??'未知';
function cell(row,text,className=''){const td=document.createElement('td');td.textContent=text;td.className=className;row.append(td);return td;}
async function refresh(){try{
  const response=await fetch('/snapshot',{headers:{Authorization:'Bearer '+token}});
  if(!response.ok)throw Error('无法读取工作台，请通过服务入口重新打开');
  const data=await response.json();node('connection').textContent='本机已连接 · '+new Date().toLocaleTimeString();
  node('loaded').textContent=data.instances.filter(i=>i.view.loaded===true).length;
  node('queued').textContent=data.tasks.filter(t=>t.state==='queued').length;
  node('active').textContent=data.tasks.filter(t=>t.state==='running-or-unconfirmed').length;
  node('memory').textContent=data.service_resources.available?(data.service_resources.resident_bytes/1048576).toFixed(1)+' MB':'未知';
  node('instances').replaceChildren();for(const item of data.instances){const row=document.createElement('tr');cell(row,(item.view.name??'未知')+' / '+(item.view.version??'—'));cell(row,item.view.error?'待核实':item.view.loaded?label(item.view.round_control?.mode):'已卸载');cell(row,item.registration.path,'path');node('instances').append(row);}
  node('tasks').replaceChildren();for(const task of data.tasks){const row=document.createElement('tr');cell(row,task.label??task.run??task.id);cell(row,label(task.state)+(task.cancellation_requested?' · 已请求取消':'')+(task.outcome?' · '+label(task.outcome):''));cell(row,label(task.native?.artifact_status));cell(row,task.resources?.available?((task.resources.cpu_user_ns+task.resources.cpu_system_ns)/1e9).toFixed(1)+' s / '+(task.resources.resident_bytes/1048576).toFixed(1)+' MB':'未知');const button=document.createElement('button');button.textContent='查看';button.onclick=()=>{node('detail').textContent=JSON.stringify(task,null,2);};cell(row,'').append(button);node('tasks').append(row);}
}catch(error){node('connection').textContent=error.message;node('connection').className='error';}}
refresh();setInterval(refresh,2000);
