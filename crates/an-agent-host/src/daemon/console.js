// English is the default; localization changes presentation only.
const zh = localStorage.getItem('an-agent-language') === 'zh-CN';
const t = (en, cn) => zh ? cn : en;
document.documentElement.lang = zh ? 'zh-CN' : 'en';
if (zh) document.querySelectorAll('[data-zh]').forEach(el => { el.textContent = el.dataset.zh; });
const languageButton = document.getElementById('language');
languageButton.textContent = zh ? 'English' : '简体中文';
languageButton.onclick = () => {
    localStorage.setItem('an-agent-language', zh ? 'en' : 'zh-CN');
    location.hash = token;
    location.reload();
};
// Read-only projection. Untrusted labels and diagnostics never become HTML.
const token=location.hash.slice(1);history.replaceState(null,'',location.pathname);
const node=id=>document.getElementById(id);
const labels={'queued':t('Queued', '排队中'),'running-or-unconfirmed':t('Running or unconfirmed', '执行中或待核实'),'settled':t('Settled', '已结算'),'active':t('Ready', '可接单'),'paused':t('Paused', '已暂停接单'),'suspended':t('Suspended', '已休眠'),'accepted':t('Accepted', '已验收'),'awaiting-acceptance':t('Awaiting acceptance', '待验收'),'not-accepted':t('Not accepted', '未验收'),'unconfirmed':t('Unconfirmed', '待核实'),'succeeded':t('Succeeded', '成功'),'cancelled':t('Cancelled', '已取消'),'failed':t('Failed', '失败'),'timed_out':t('Timed out', '超时')};
const label=value=>labels[value]??value??t('Unknown', '未知');
function cell(row,text,className=''){const td=document.createElement('td');td.textContent=text;td.className=className;row.append(td);return td;}
async function refresh(){try{
  const response=await fetch('/snapshot',{headers:{Authorization:'Bearer '+token}});
  if(!response.ok)throw Error(t('Cannot read the workbench. Reopen it from the service entry.', '无法读取工作台，请通过服务入口重新打开'));
  const data=await response.json();node('connection').textContent=t('Connected locally · ', '本机已连接 · ')+new Date().toLocaleTimeString();
  node('loaded').textContent=data.instances.filter(i=>i.view.loaded===true).length;
  node('queued').textContent=data.tasks.filter(t=>t.state==='queued').length;
  node('active').textContent=data.tasks.filter(t=>t.state==='running-or-unconfirmed').length;
  node('memory').textContent=data.service_resources.available?(data.service_resources.resident_bytes/1048576).toFixed(1)+' MB':t('Unknown', '未知');
  node('instances').replaceChildren();for(const item of data.instances){const row=document.createElement('tr');cell(row,(item.view.name??t('Unknown', '未知'))+' / '+(item.view.version??'—'));cell(row,item.view.error?t('Unconfirmed', '待核实'):item.view.loaded?label(item.view.round_control?.mode):t('Unloaded', '已卸载'));cell(row,item.registration.path,'path');node('instances').append(row);}
  node('tasks').replaceChildren();for(const task of data.tasks){const row=document.createElement('tr');cell(row,task.label??task.run??task.id);cell(row,label(task.state)+(task.cancellation_requested?t(' · Cancellation requested', ' · 已请求取消'):'')+(task.outcome?' · '+label(task.outcome):''));cell(row,label(task.native?.artifact_status));cell(row,task.resources?.available?((task.resources.cpu_user_ns+task.resources.cpu_system_ns)/1e9).toFixed(1)+' s / '+(task.resources.resident_bytes/1048576).toFixed(1)+' MB':t('Unknown', '未知'));const button=document.createElement('button');button.textContent=t('Inspect', '查看');button.onclick=async()=>{try{const response=await fetch('/activity/'+encodeURIComponent(task.id),{headers:{Authorization:'Bearer '+token}});if(!response.ok)throw Error(t('Activity is unavailable', '活动记录暂不可用'));node('detail').textContent=JSON.stringify(await response.json(),null,2);}catch(error){node('detail').textContent=error.message;}};cell(row,'').append(button);node('tasks').append(row);}
}catch(error){node('connection').textContent=error.message;node('connection').className='error';}}
refresh();setInterval(refresh,2000);
