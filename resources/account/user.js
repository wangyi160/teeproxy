"use strict";
function userOverview(){
  const balance=state.balance,application=state.applications.find(a=>a.kind==='user');
  return `<div class="heading"><div><div class="eyebrow">USER CENTER</div><h1>${esc(state.account.name)}</h1><p>查看接入状态、余额与平台密钥。</p></div></div>
    ${financeHome('user')}
    <div class="cards"><div class="card"><small>用户开通状态</small><strong>${badge(application?.status||'pending')}</strong></div><div class="card"><small>累计消费 · USD</small><strong>${balance?money(balance.spent_microusd):'—'}</strong></div><div class="card"><small>启用的 API key</small><strong>${state.keys.filter(k=>k.state==='enabled').length}</strong></div></div>
    ${balance&&balance.ceiling_microusd<=balance.spent_microusd?'<div class="notice warn">当前余额不足，请在余额与充值页面查看账务和充值状态。</div>':''}
    ${balance&&!balance.enabled?'<div class="notice warn">模型服务已被平台停用，请联系管理员。</div>':''}
    ${state.account.user_id&&!state.user_active?'<div class="notice warn">平台用户已被删除，请联系管理员。</div>':''}
    ${state.sync_error?'<div class="notice warn">Router 同步暂未完成，以下为平台已保存的状态。</div>':''}
    ${applications()}<section class="panel"><h2>下一步</h2><p class="muted">开通后创建 API key，按照连接指南配置 teeproxy，即可通过本地 API 调用已获授权的模型。</p><a class="button-link" href="/account#connection">查看连接指南</a></section>`;
}
function userKeys(){
  return `<div class="heading"><div><h1>API keys</h1><p>密钥属于当前用户，共享用户余额和并发上限。</p></div>${state.user_active?button('创建 API key','new-key','','primary'):''}</div><section class="panel">${table(['名称','密钥前缀','状态','创建时间','操作'],state.keys.map(k=>`<tr><td>${esc(k.label)}</td><td><code>${esc(k.prefix)}…</code></td><td>${badge(k.state)}</td><td>${date(k.created_at)}</td><td>${button('查看密钥','reveal',k.id,'link')}${k.state==='enabled'?button('停用','revoke',k.id,'link danger'):''}</td></tr>`),'开通后可以创建 API key。')}</section><p class="muted">密钥启用受余额、平台授权、商户限额和路由规则约束。</p>`;
}
function userConnection(){
  return `<div class="heading"><div><h1>连接指南</h1><p>通过 teeproxy 使用模型服务。</p></div></div>${downloads('user')}
    <section class="panel"><h2>Router 地址</h2><pre>${esc(state.router_url)}</pre>${button('复制 Router 地址','copy-router')}
    <ol class="steps"><li>打开 teeproxy，填入上面的 Router 地址和自己的平台 API key。</li><li>加载经过独立审核的 TEE 信任策略，完成 TEE 验证后再建立连接。</li><li>在 teeproxy 中选择路由策略并启动本地代理。</li><li>将 agent 的 API 地址设置为 teeproxy 显示的本地地址，API key 填写 teeproxy 的本地访问密钥。</li></ol></section>
    <section class="panel"><h2>Agent 地址示例</h2><pre>http://127.0.0.1:8899/v1</pre><p class="muted">以 teeproxy 界面实际显示的端口和本地密钥为准。平台 key 配在 teeproxy 的 Router 连接中。</p></section>`;
}
const workspace={
  role:'user',views:{overview:userOverview,finance:financeView,keys:userKeys,connection:userConnection,receipts:ownedReceiptsView},
  load:currentPage=>currentPage==='receipts'?loadOwnedReceipts(false):['overview','finance'].includes(currentPage)?loadFinance(false):null,
  afterRender:currentPage=>{bindFinance(currentPage);bindOwnedReceipts(currentPage);},
  async action(action,id,control){
    if(action.startsWith('receipt-'))return ownedReceiptAction(action,id);
    if(action.startsWith('payment-'))return paymentAction(action,id);
    if(action.startsWith('finance-'))return financeAction(action,id,control);
    if(action==='new-key')modal('创建 API key','<label>名称<input name="label" required maxlength="128" placeholder="例如：工作电脑"></label>',async(form,operationId)=>{
      await api('keys','POST',{label:form.get('label')},operationId);toast('密钥已保存，可在列表查看；下发状态以列表为准');
    });
    if(action==='reveal'){
      const result=await api(`keys/${encodeURIComponent(id)}/reveal`,'POST',{});
      modal('API key',`<pre>${esc(result.key)}</pre>${button('应用至 teeproxy','use-local-key',id,'primary')}`,null);
    }
    if(action==='revoke')modal('停用 API key','<p>停用后该密钥将不能继续调用模型服务。</p>',async()=>{
      const result=await api(`keys/${encodeURIComponent(id)}`,'DELETE');toast(result.state==='pending'?'停用已保存，等待 Router 确认':'密钥已停用');
    });
  }
};
