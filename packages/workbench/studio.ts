// Local workspace UI. A host may supply its catalog and initial records before startup.
type Language = 'zh-CN' | 'en';
const languageKey='an-agent.workbench.language';
function initialLanguage():Language {
  try {
    const saved=localStorage.getItem(languageKey);
    if(saved==='zh-CN'||saved==='en')return saved;
  } catch { /* Continue with browser preferences. */ }
  for(const tag of navigator.languages?.length?navigator.languages:[navigator.language]){
    if(/^zh(?:-|$)/i.test(tag))return 'zh-CN';
    if(/^en(?:-|$)/i.test(tag))return 'en';
  }
  return 'en';
}
let language=initialLanguage();
const t=(zh:string,en:string)=>language==='en'?en:zh;
interface Manifest {id:string; version?:string; name:string; category:string; description:string; input:string; output:string; glyph:string; theme:string}
interface Artifact {file:string; spool:string; revision:string; agent:string}
interface Receipt {file:string; revision:string; from:string; to:string}
interface Usage {agent:string; input:number; output:number}
interface Workspace {id:string; name:string; intent:string; installed:string[]; artifacts:Artifact[]; receipts:Receipt[]; usage:Usage[]; integration:'pending'|'done'|'none'}
interface Store {version:1; active:string; spaces:Workspace[]}
interface WorkbenchSource {
  storageKey:string;
  catalog:Manifest[];
  initialState():Store;
  request?(workspace:Workspace):{spool:string;reason:string}|undefined;
  afterLoad?(workspace:Workspace,spool:string):void;
}
interface Window {workbenchSource?:WorkbenchSource}
const source=window.workbenchSource;
const key=source?.storageKey??'an-agent.workbench.v1';
const catalog=source?.catalog??[];
const byId=(id:string)=>catalog.find(item=>item.id===id)!;
const spoolName=(id:string)=>id==='master'?'Master Agent':byId(id).name;
const format=(n:number)=>n.toLocaleString(language);
function blank(id:string,name:string,intent:string,installed:string[]):Workspace{
  return {id,name,intent,installed,artifacts:[],receipts:[],usage:[],integration:'none'};
}
function valid(w:Workspace):boolean{
  const text=(x:unknown)=>typeof x==='string';
  const known=(id:string)=>catalog.some(m=>m.id===id);
  const endpoint=(id:string)=>id==='master'||known(id);
  return !!w && [w.id,w.name,w.intent].every(text) && Array.isArray(w.installed) && w.installed.every(known)
    && Array.isArray(w.artifacts) && w.artifacts.every(a=>a&&known(a.spool)&&[a.file,a.revision,a.agent].every(text))
    && Array.isArray(w.receipts) && w.receipts.every(r=>r&&endpoint(r.from)&&endpoint(r.to)&&[r.file,r.revision].every(text))
    && Array.isArray(w.usage) && w.usage.every(u=>u&&text(u.agent)&&[u.input,u.output].every(n=>Number.isSafeInteger(n)&&n>=0))
    && ['pending','done','none'].includes(w.integration);
}
function restore():Store{
  try{
    const data=JSON.parse(localStorage.getItem(key)??'null');
    if(data?.version===1 && Array.isArray(data.spaces) && data.spaces.length && data.spaces.every(valid) && data.spaces.some((w:Workspace)=>w.id===data.active))return data;
  }catch{/* Storage may be unavailable for local HTML. */}
  return source?.initialState()??{version:1,active:'',spaces:[]};
}
const store=restore();
const ws=()=>store.spaces.find(w=>w.id===store.active)!;
function save(){
  try{localStorage.setItem(key,JSON.stringify(store));}
  catch{get('storage-note').textContent=t('本窗口可继续使用，当前浏览器无法保存本地工作区。','Browser storage is unavailable. Changes last for this session.');}
}
function load(w:Workspace,id:string){
  if(w.installed.includes(id))return;
  w.installed.push(id);
  source?.afterLoad?.(w,id);
}
function get(id:string){return document.getElementById(id)!;}
function node<K extends keyof HTMLElementTagNameMap>(tag:K,cls='',text=''){
  const el=document.createElement(tag);
  if(cls)el.className=cls;
  if(text)el.textContent=text;
  return el;
}
function button(text:string,cls:string,fn:()=>void){
  const b=node('button',cls,text);b.type='button';b.onclick=fn;return b;
}
function tile(m:Manifest){const t=node('span','tile '+m.theme,m.glyph);t.setAttribute('aria-hidden','true');return t;}
function datum(a:string,b:string){
  const d=node('div','datum');d.append(node('span','',a),node('span','',b));return d;
}
function heading(title:string,action?:HTMLElement){
  const h=node('div','section-head');h.append(node('h2','',title));if(action)h.append(action);return h;
}
function record(title:string,detail:string){
  const d=node('details');d.append(node('summary','',title),node('p','',detail));return d;
}
const main=get('content'), sheet=get('sheet') as HTMLDialogElement,body=get('sheet-body');
type Page='space'|'market';
type Tab='overview'|'artifacts'|'transfers';
let page:Page='space',tab:Tab='overview';
function modal(context:string,title:string){
  get('sheet-context').textContent=context;
  const heading=node('h2','',title); heading.id='sheet-title';
  body.replaceChildren(heading);
  if(!sheet.open)sheet.showModal();
}
get('close').onclick=()=>sheet.close();
sheet.onclick=e=>{if(e.target===sheet){const r=sheet.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)sheet.close();}};
function total(w:Workspace){return w.usage.reduce((s,u)=>s+u.input+u.output,0);}
function usageView(){
  const w=ws();
  modal(w.name,t('Token 用量','Token usage'));
  if(!w.usage.length){body.append(node('p','empty',t('暂无用量记录。','No usage recorded yet.')));return;}
  const tokens=total(w),input=w.usage.reduce((s,u)=>s+u.input,0);
  body.append(node('p','sub',t('累计用量','Total usage')),node('div','number',format(tokens)));
  const bar=node('div','meter'),part=node('i');part.style.width=(100*input/(tokens||1))+'%';bar.append(part);
  body.append(bar,datum(t('输入','Input'),format(input)),datum(t('输出','Output'),format(tokens-input)),heading(t('按 Agent','By agent')));
  for(const agent of new Set(w.usage.map(u=>u.agent))){
    const records=w.usage.filter(u=>u.agent===agent);
    body.append(datum(agent,format(records.reduce((s,u)=>s+u.input+u.output,0))));
  }
}
function selectedCount(n:number){return t('已选 '+n+' 个',n+' selected');}
function createView(){
  modal('Workspace',t('创建工作空间','New workspace'));
  const form=node('form');
  const name=node('input','field');name.id='new-name';name.required=true;name.maxLength=64;name.placeholder=t('工作区名称','Workspace name');
  const intent=node('textarea','field');intent.id='new-intent';intent.maxLength=1200;intent.placeholder=t('希望 Master Agent 完成什么？','What should the master agent deliver?');intent.rows=2;
  function label(text:string,id:string){const l=node('label','form-label',text);l.htmlFor=id;return l;}
  form.append(label(t('名称','Name'),name.id),name,label(t('目标（可选）','Goal (optional)'),intent.id),intent,node('p','form-label',t('选择 Spool','Choose Spools')));
  if(!catalog.length)form.append(node('p','sub',t('当前没有可用的 Spool，可先创建工作区。','No Spools are available yet. You can still create this workspace.')));
  const choices=catalog.map(m=>{
    const l=node('label','choice'),input=node('input');input.type='checkbox';input.value=m.id;
    const copy=node('span','row-text');
    copy.append(node('strong','',m.name),node('small','',m.input+' → '+m.output));
    l.append(tile(m),copy,input);form.append(l);return input;
  });
  const footer=node('div','footer-actions'),count=node('span','sub',selectedCount(0));
  form.onchange=()=>count.textContent=selectedCount(choices.filter(c=>c.checked).length);
  const submit=node('button','primary',t('创建 Workspace','Create workspace'));submit.type='submit';footer.append(count,submit);form.append(footer);
  form.onsubmit=e=>{
    e.preventDefault();
    const title=name.value.trim();
    if(!title||store.spaces.some(w=>w.name===title)){name.setCustomValidity(title?t('已有同名工作区','A workspace with this name already exists.'):t('请输入工作区名称','Enter a workspace name.'));name.reportValidity();return;}
    const w=blank(crypto.randomUUID(),title,intent.value.trim(),choices.filter(c=>c.checked).map(c=>c.value));
    store.spaces.push(w);store.active=w.id;page='space';tab='overview';save();sheet.close();render();
  };
  name.oninput=()=>name.setCustomValidity('');
  body.append(form);name.focus();
}
function artifactList(items:Artifact[]){
  const box=node('div','panel');
  if(!items.length)box.append(node('span','sub',t('暂无已验收的产物。','No accepted artifacts yet.')));
  for(const a of items)box.append(record(a.file,spoolName(a.spool)+' · '+a.agent+t(' · 已验收 · ',' · Accepted · ')+a.revision));
  return box;
}
function receiptList(items:Receipt[]){
  const box=node('div','panel');
  if(!items.length)box.append(node('span','sub',t('暂无传递记录。','No transfers yet.')));
  for(const r of items)box.append(record(r.file+'　·　'+spoolName(r.from)+' → '+spoolName(r.to),t('已确认消费 · ','Consumption confirmed · ')+r.revision));
  return box;
}
function spoolView(id:string,request=false){
  const w=ws(),m=byId(id),installed=w.installed.includes(id);
  modal(w.name+'　/　'+(request?t('加载申请','Load request'):'Spool'),m.name);
  body.append(node('p','intro',m.description),datum(t('输入','Input'),m.input),datum(t('输出','Output'),m.output));
  if(m.version)body.append(datum(t('版本','Version'),m.version));
  if(request)body.append(datum(t('申请方','Requested by'),'Master Agent'));
  if(installed){
    const products=w.artifacts.filter(a=>a.spool===id);
    body.append(datum(t('运行状态','Status'),t('已加载 · 空闲','Loaded · Idle')),datum(t('最近使用','Last used by'),products.length?products.at(-1)!.agent:t('尚未运行','No runs yet')),heading(t('产物','Artifacts')),artifactList(products),heading(t('传递','Transfers')),receiptList(w.receipts.filter(r=>r.from===id||r.to===id)));
  }
  const f=node('div','footer-actions');
  f.append(node('span','sub',installed?t('移除后保留产物与记录','Artifacts and history are kept after removal.'):t('仅对当前 Workspace 生效','Applies to this workspace only.')));
  f.append(button(installed?t('移除 Spool','Remove Spool'):request?t('批准并加载','Approve and load'):t('加入工作区','Add to workspace'),installed?'secondary':'primary',()=>{
    if(installed)w.installed=w.installed.filter(x=>x!==id);else load(w,id);
    save();sheet.close();render();
  }));
  body.append(f);
}
let marketDraft={selected:new Set<string>(),category:'',search:''};
function market(){
  marketDraft={selected:new Set(ws().installed),category:'',search:''};
  page='market';render();
}
function marketView(){
  const w=ws();
  main.append(node('div','eyebrow',t('资源库','Library')),node('h1','','Spool Market'),node('p','intro',t('为「'+w.name+'」选择可用能力。','Choose capabilities for '+w.name+'.')));
  if(!catalog.length){
    main.append(node('p','empty',t('尚无可用的 Spool。','No Spools available yet.')),
      button(t('返回工作区','Back to workspace'),'secondary',()=>{page='space';render();}));
    return;
  }
  const search=node('input','search');search.type='search';search.placeholder=t('搜索 Spool、输入或输出','Search Spools, inputs or outputs');search.setAttribute('aria-label',search.placeholder);
  const grid=node('div','market-grid'), filters=node('div','filters');
  const selected=marketDraft.selected;
  let category=marketDraft.category;
  search.value=marketDraft.search;
  const footer=node('div','market-footer'),summary=node('span','sub');
  const back=button(t('返回工作区','Back to workspace'),'secondary',()=>{page='space';render();});
  const apply=button(t('保存选择','Save selection'),'primary',()=>{
    w.installed=w.installed.filter(id=>selected.has(id));
    for(const id of selected) if(!w.installed.includes(id))load(w,id);
    save();page='space';tab='overview';render();
  });
  function update(){
    const changed=w.installed.length!==selected.size||w.installed.some(id=>!selected.has(id));
    summary.textContent=selectedCount(selected.size);
    back.textContent=changed?t('放弃更改','Discard changes'):t('返回工作区','Back to workspace');
    apply.disabled=!changed;
  }
  function results(){
    grid.replaceChildren();const q=search.value.trim().toLowerCase();
    for(const m of catalog.filter(m=>(!category||m.theme===category)&&[m.name,m.category,m.description,m.input,m.output].join(' ').toLowerCase().includes(q))){
      const card=node('article','product '+m.theme);
      const head=node('div','product-head'),select=button('','pick',()=>{
        if(selected.has(m.id))selected.delete(m.id);else selected.add(m.id);
        refresh();update();
      });
      function refresh(){
        const on=selected.has(m.id);
        select.textContent=on?t('✓ 已选','✓ Selected'):t('＋ 选择','+ Select');
        select.setAttribute('aria-label',m.name+' · '+select.textContent);
        select.setAttribute('aria-pressed',String(on));
        card.classList.toggle('selected',on);
      }
      refresh();head.append(tile(m),select);
      const spec=record(t('能力声明','Capability manifest'),m.input+' → '+m.output+(m.version?' · v'+m.version:''));
      card.append(head,node('h3','',m.name),node('p','',m.description),spec);
      grid.append(card);
    }
    if(!grid.children.length)grid.append(node('p','empty',t('没有匹配的 Spool。','No matching Spools.')));
  }
  for(const [id,label] of [['',t('全部','All')],...catalog.map(m=>[m.theme,m.category])]){
    const chip=button(label,'filter',()=>{
      marketDraft.category=category=id;
      for(const item of Array.from(filters.children))item.setAttribute('aria-pressed',String(item===chip));
      results();
    });
    chip.setAttribute('aria-pressed',String(id===category));filters.append(chip);
  }
  search.oninput=()=>{marketDraft.search=search.value;results();};
  footer.append(summary,back,apply);
  main.append(search,filters,grid,footer);update();results();
}
function overview(w:Workspace){
  const request=source?.request?.(w);
  if(request && catalog.some(m=>m.id===request.spool)){
    const candidate=byId(request.spool);
    const panel=node('article','panel pending');
    const message=node('div','request-copy');
    message.append(node('h3','',t('允许加载「'+candidate.name+'」？','Load '+candidate.name+'?')),node('p','',request.reason));
    panel.append(tile(candidate),message,button(t('审阅申请','Review request'),'secondary',()=>spoolView(candidate.id,true)));
    main.append(panel);
  }
  main.append(heading(t('已加载的 Spool · ','Loaded Spools · ')+w.installed.length,button(t('选择 Spool　＋','Choose Spools +'),'link',market)));
  const list=node('div','list');
  if(!w.installed.length)list.append(node('p','empty',t('尚未加载 Spool。前往 Market 为这条流水线选择能力。','No Spools loaded. Choose capabilities for this pipeline in the Market.')));
  for(const id of w.installed){
    const m=byId(id),a=w.artifacts.filter(a=>a.spool===id),row=button('','row',()=>spoolView(id)),text=node('span','row-text');
    text.append(node('strong','',m.name),node('small','',a.length?a.at(-1)!.agent+' · '+t(a.length+' 件产物',a.length+(a.length===1?' artifact':' artifacts')):t('尚未运行','No runs yet')));
    row.append(tile(m),text,node('span','badge',t('空闲','Idle')),node('span','chevron','›'));list.append(row);
  }
  main.append(list);
}
function workspaceView(){
  const w=ws(),hero=node('header','hero'),copy=node('div','hero-copy');
  const status=node('span','master');status.append(node('i','dot'),document.createTextNode('Master Agent · '+(w.integration==='pending'?t('等待加载批准','Awaiting approval'):w.integration==='done'?t('已完成交付','Delivered'):t('就绪','Ready'))));
  copy.append(node('div','eyebrow','Workspace'),node('h1','',w.name));
  if(w.intent)copy.append(node('p','',w.intent));
  copy.append(status);hero.append(copy,(get('hero-art') as HTMLTemplateElement).content.cloneNode(true));main.append(hero);
  const tabs=node('div','segment');
  for(const [name,label] of [['overview',t('概览','Overview')],['artifacts',t('产物','Artifacts')],['transfers',t('传递','Transfers')]] as const){
    const b=button(label,tab===name?'active':'',()=>{tab=name;render();main.querySelector<HTMLButtonElement>('.segment .active')?.focus();});
    b.setAttribute('aria-pressed',String(tab===name));tabs.append(b);
  }
  main.append(tabs);
  if(tab==='overview')overview(w);
  if(tab==='artifacts')main.append(heading(t('已验收产物','Accepted artifacts')),artifactList(w.artifacts));
  if(tab==='transfers')main.append(heading(t('产物传递','Artifact transfers')),receiptList(w.receipts));
}
function render(){
  document.documentElement.lang=language;
  document.title=t('an-agent · 工作台','an-agent · Workbench');
  get('space-label').textContent=t('工作空间','Workspaces');
  get('new-label').textContent=t('新建工作空间','New workspace');
  get('library-label').textContent=t('资源库','Library');
  get('demo-label').textContent=source?t('本地演示','Local demo'):t('本地工作台','Local workbench');
  get('create').title=t('创建工作空间','Create workspace');
  get('create').setAttribute('aria-label',get('create').title);
  get('spaces').setAttribute('aria-label',t('选择工作空间','Choose workspace'));
  get('usage').setAttribute('aria-label',t('查看当前工作区 Token 用量','View workspace token usage'));
  get('close').setAttribute('aria-label',t('关闭','Close'));
  if(get('storage-note').textContent)get('storage-note').textContent=t('本窗口可继续使用，当前浏览器无法保存本地工作区。','Browser storage is unavailable. Changes last for this session.');
  (get('language') as HTMLSelectElement).value=language;
  const w=ws(),spaces=get('spaces');spaces.replaceChildren();
  (get('market-nav') as HTMLButtonElement).disabled=!w;
  (get('usage') as HTMLButtonElement).disabled=!w;
  if(!w){
    get('breadcrumb').textContent=t('工作台','Workbench');
    get('usage-value').textContent=t('Token 用量 · 0','Token usage · 0');
    main.replaceChildren(node('h1','',t('创建第一个工作空间','Create your first workspace')),
      node('p','intro',t('为流水线设定目标，再选择可用的 Spool。','Set a goal for your pipeline and choose the Spools it can use.')),
      button(t('创建工作空间','New workspace'),'primary welcome-action',createView));
    return;
  }
  for(const space of store.spaces){
    const active=space.id===w.id;
    const b=button('','nav-item'+(active?' active':''),()=>{store.active=space.id;page='space';tab='overview';save();render();});
    b.append(node('i','space-mark'),node('span','',space.name));b.setAttribute('aria-pressed',String(active));spaces.append(b);
  }
  get('market-nav').classList.toggle('active',page==='market');
  get('market-nav').setAttribute('aria-pressed',String(page==='market'));
  get('breadcrumb').textContent=w.name+'　/　'+(page==='market'?'Spool Market':t('工作台','Workbench'));
  get('usage-value').textContent=t('Token 用量　','Token usage · ')+format(total(w));
  main.replaceChildren();
  if(page==='market')marketView();else workspaceView();
}
get('language').onchange=e=>{
  language=(e.currentTarget as HTMLSelectElement).value==='en'?'en':'zh-CN';
  try { localStorage.setItem(languageKey,language); } catch { /* Session-only preference. */ }
  render();
};
get('usage').onclick=usageView;
get('create').onclick=createView;
get('create-wide').onclick=createView;
get('market-nav').onclick=market;
render();
