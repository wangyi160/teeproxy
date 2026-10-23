"use strict";
let paymentData=null,paymentSelected=null,paymentError='',paymentBusy=false,paymentAttempt=null;
let paymentBefore='',paymentPages=[],paymentNetwork='tron';
const paymentNames={creating:'订单确认中',pending:'等待付款',paid:'已到账 · 正在入账',credited:'已入账',expired:'已过期',failed:'创建失败'};
const tokenAmount=n=>{const v=BigInt(n||0);return (v/1000000n)+'.'+(v%1000000n).toString().padStart(6,'0').replace(/0+$/,'').padEnd(2,'0');};
async function loadPayments(){try{paymentData=await api('payments'+(paymentBefore?'?before='+encodeURIComponent(paymentBefore):''));paymentError='';if(paymentSelected){const r=paymentData.data.find(r=>r.id===paymentSelected.id);paymentSelected=r||await api('payments/'+encodeURIComponent(paymentSelected.id));}}catch(e){paymentError=e.message;}}
function rechargeView(){
  const c=paymentData?.config;
  let html='<section class="panel recharge-panel"><div class="heading"><div><h2>USDT 充值</h2><p>1 USDT = 1 USD · 平台手续费 0</p></div></div>';
  if(paymentError)html+=`<div class="notice warn">${esc(paymentError)}</div>`;
  if(!c)return html+'<p class="muted">正在读取充值服务…</p></section>';
  if(!c.enabled)html+='<p class="muted">USDT 收款配置尚未启用。</p>';
  else html+=`<form id="recharge-form" class="finance-filters"><label>充值网络<select name="network">${(c.channels||[]).map(n=>`<option value="${esc(n.id)}" ${paymentNetwork===n.id?'selected':''}>${esc(n.name)} · ${esc(n.asset)}</option>`).join('')}</select></label><label>充值金额 · USDT<input name="amount" type="number" min="${tokenAmount(c.minimum_microusd)}" max="${tokenAmount(c.maximum_microusd)}" step="0.01" value="10" required inputmode="decimal"></label><button class="primary" ${paymentBusy?'disabled':''}>创建充值订单</button><span class="hint">${tokenAmount(c.minimum_microusd)}–${tokenAmount(c.maximum_microusd)} USDT</span></form>`;
  if(paymentSelected){const r=paymentSelected,payable=r.payable_microusd;
    html+=`<div class="recharge-checkout"><div>${r.state==='pending'&&r.receive_address?`<img width="176" height="176" alt="${esc(r.network_name)} 收款地址二维码" src="/account-api/payments/${encodeURIComponent(r.id)}/qr.svg">`:''}</div><div class="recharge-details"><span class="badge">${esc(paymentNames[r.state]||r.state)}</span><h3>${payable?tokenAmount(payable)+' USDT':'正在确认订单'}</h3>${payable?`<p>到账余额：<strong>${financeMoney(r.credit_microusd)}</strong></p>`:''}${r.state==='pending'&&r.receive_address?`<label>收款地址 · ${esc(r.network_name)}<code class="recharge-address">${esc(r.receive_address)}</code></label><div class="actions"><button data-action="payment-copy-address">复制地址</button><button data-action="payment-copy-amount">复制金额</button></div><p class="hint">仅发送 ${esc(r.network_name)} 网络 ${esc(r.asset)}，请按订单金额转账。二维码包含收款地址。</p>`:''}<p class="hint">网络：${esc(r.network_name)} · ${esc(r.asset)}<br>代币合约：<span class="recharge-address">${esc(r.contract)}</span></p><p class="hint">订单：${esc(r.id)}<br>有效期至：${date(r.expires_at)}</p>${r.transaction_id?`<a href="${esc(r.transaction_url)}" target="_blank" rel="noopener noreferrer">查看链上交易 ↗</a>`:''}${r.error?'<p class="notice warn">'+esc(r.error==='credit_pending'?'资金已确认，余额入账待处理':r.error==='payment_order_unconfirmed'?'创建结果尚未确认，请稍后刷新':'到账查询暂时不可用，系统将重试')+'</p>':''}<button data-action="payment-check" data-id="${esc(r.id)}">刷新状态</button></div></div>`;
  }
  html+='</section>';
  return html+`<section class="panel"><h2>充值订单</h2>${table(['创建时间','充值网络','充值金额','状态','操作'],paymentData.data.map(r=>`<tr><td>${date(r.created_at)}<span class="sub">${esc(r.id)}</span></td><td>${esc(r.network_name)}</td><td>${r.payable_microusd?tokenAmount(r.payable_microusd):tokenAmount(r.requested_microusd)} USDT</td><td>${esc(paymentNames[r.state]||r.state)}</td><td><button data-action="payment-detail" data-id="${esc(r.id)}">查看</button></td></tr>`),'暂无充值订单。')}<div class="actions"><button data-action="payment-prev" ${paymentPages.length?'':'disabled'}>上一页</button><button data-action="payment-next" ${paymentData.has_more?'':'disabled'}>下一页</button></div></section>`;
}
function bindPayments(){const form=$('#recharge-form');if(!form)return;form.elements.network.onchange=()=>{paymentNetwork=form.elements.network.value;};form.onsubmit=async event=>{
  event.preventDefault();if(paymentBusy)return;const data=new FormData(form),amount=data.get('amount'),network=data.get('network');paymentNetwork=network;
  if(!paymentAttempt||paymentAttempt.amount!==amount||paymentAttempt.network!==network)paymentAttempt={amount,network,id:newOperationId()};
  paymentBusy=true;form.querySelector('button').disabled=true;
  try{paymentSelected=await api('payments','POST',{amount,network},paymentAttempt.id);paymentAttempt=null;paymentBefore='';paymentPages=[];await loadPayments();render();}
  catch(e){toast(e.message);}finally{paymentBusy=false;const b=$('#recharge-form button');if(b)b.disabled=false;}
};}
async function paymentAction(action,id){
  if(action==='payment-next'&&paymentData.next_before){paymentPages.push(paymentBefore);paymentBefore=paymentData.next_before;await loadPayments();render();}
  if(action==='payment-prev'&&paymentPages.length){paymentBefore=paymentPages.pop();await loadPayments();render();}
  if(action==='payment-detail'){paymentSelected=await api('payments/'+encodeURIComponent(id));render();}
  if(action==='payment-check'){paymentSelected=await api('payments/'+encodeURIComponent(id)+'/refresh','POST',{});await loadPayments();render();}
  if(action==='payment-copy-address'){await copyToClipboard(paymentSelected.receive_address);toast('地址已复制');}
  if(action==='payment-copy-amount'){await copyToClipboard(tokenAmount(paymentSelected.payable_microusd));toast('金额已复制');}
}
setInterval(async()=>{
  if(typeof page==='undefined'||page!=='finance'||document.hidden||paymentBusy||!paymentData?.data.some(r=>['creating','pending','paid'].includes(r.state)))return;
  const old=JSON.stringify(paymentData);await loadPayments();
  if(old!==JSON.stringify(paymentData)&&!document.activeElement?.closest('form')){await loadFinance(false);render();}
},5000);
