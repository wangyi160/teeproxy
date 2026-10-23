const originalAccountAction=workspace.action;
workspace.action=async(action,id,control)=>{
  if(action==='request-access'){await api('access','POST',{merchant_id:id});await refresh();toast('申请已提交');return;}
  if(action==='use-local-key'){await api('use-key','POST',{id});toast('API key 已保存至 teeproxy');return;}
  return originalAccountAction(action,id,control);
};
workspace.views.access=()=>`<div class="heading"><h1>服务商授权</h1></div><section class="panel">${table(['商户','授权状态','操作'],state.merchants.map(m=>{const pending=state.applications.some(a=>a.kind==='access'&&a.subject===m.id&&a.status==='pending');return `<tr><td>${esc(m.name)}</td><td>${m.authorized?'已授权':pending?'待审核':'未授权'}</td><td>${!m.authorized&&!pending?button('申请使用','request-access',m.id):''}</td></tr>`;}))}</section>`;
const keyNav=document.querySelector('[data-page=keys]');
if(keyNav){const a=document.createElement('a');a.dataset.page='access';a.href='/account#access';a.textContent='服务商授权';keyNav.after(a);}
