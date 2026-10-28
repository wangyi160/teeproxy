//! User refunds use the platform's shared withdrawal policy and manual queue.
#![allow(unsafe_op_in_unsafe_fn)]
use super::*;

pub(super) const DEPOSIT_TAB:u16=1554;
pub(super) const WITHDRAW_TAB:u16=1555;
const W_NETWORK:u16=1556;
const W_AMOUNT:u16=1557;
const W_ADDRESS:u16=1558;
const W_SUBMIT:u16=1559;
const W_CANCEL:u16=1560;
const W_QUOTE:u16=1561;
const W_BALANCE:u16=1562;

pub(super) fn withdrawal_status(row:&Value)->String {
    match row["state"].as_str().unwrap_or("") {
        "pending"=>"待处理","processing"=>"打款处理中","paid"=>"已打款","rejected"=>"已拒绝","cancelled"=>"已取消",_=>"—"
    }.into()
}
fn micro(value:&str)->Result<i128>{
    let parts:Vec<_>=value.trim().split('.').collect();
    ensure!((1..=2).contains(&parts.len())&&!parts[0].is_empty()&&parts.iter().all(|s|s.bytes().all(|b|b.is_ascii_digit()))&&parts.get(1).is_none_or(|s|!s.is_empty()&&s.len()<=6),"金额最多保留 6 位小数");
    let whole:i128=parts[0].parse().context("金额无效")?;
    ensure!(whole<=1_000_000,"金额超出范围");
    let fraction=parts.get(1).copied().unwrap_or("");
    let amount=whole*1_000_000+format!("{fraction:0<6}").parse::<i128>()?;
    ensure!(amount>0,"金额必须大于 0");Ok(amount)
}
fn quote(policy:&Value,input:&str)->Result<(i128,i128,i128)>{
    let amount=micro(input)?;
    let minimum=integer(&policy["minimum_microusd"]).context("正在读取提现规则")?;
    ensure!(amount>=minimum,"最低提现 {} USDT",super::amount(&Value::String(minimum.to_string())));
    let tier=policy["tiers"].as_array().context("提现规则无效")?.iter().find(|t|t["to_microusd"].is_null()||integer(&t["to_microusd"]).is_some_and(|v|amount<v)).context("提现规则无效")?;
    let rate=integer(&tier["rate_bps"]).context("提现规则无效")?;
    let fee=(amount*rate+5000)/10000;Ok((amount,fee,rate))
}
impl App {
    pub(super) unsafe fn build_withdrawal(&mut self)->Result<()>{
        let start=self.controls.len();
        self.button(DEPOSIT_TAB,"充值",[-268,80,100,32],0)?;
        self.button(WITHDRAW_TAB,"提现",[-156,80,100,32],0)?;
        self.account_group(start,Group::Funds);
        let start=self.controls.len();
        self.add(W_BALANCE,w!("STATIC"),"",SS_LEFT,[260,184,-312,28],3)?;
        self.label("提现网络",[260,222,180,24],0)?;
        self.add(W_NETWORK,w!("COMBOBOX"),"",WS_TABSTOP|CBS_DROPDOWNLIST as u32|WS_VSCROLL,[260,250,204,250],0)?;
        self.label("提现金额 · USDT",[484,222,190,24],0)?;
        self.edit(W_AMOUNT,"",[484,246,150,36],0,"")?;
        self.label("收款地址",[260,298,180,24],0)?;
        self.edit(W_ADDRESS,"",[260,330,-500,36],0,"")?;
        self.button(W_SUBMIT,"申请提现",[-218,330,166,36],0)?;
        self.add(W_QUOTE,w!("STATIC"),"",SS_LEFT,[260,376,-312,44],0)?;
        self.button(W_CANCEL,"取消申请",[492,-122,120,34],0)?;
        self.account_group(start,Group::Panel(Panel::Withdrawals));
        SendMessageW(W_AMOUNT.hwnd(self),EM_SETLIMITTEXT,20,0);
        SendMessageW(W_ADDRESS.hwnd(self),EM_SETLIMITTEXT,128,0);Ok(())
    }
    pub(in crate::desktop) fn withdrawal_rect(&self,id:u16,width:i32,height:i32)->Option<[i32;4]>{
        if self.page!=PAYMENTS_PAGE||self.account.panel!=Panel::Withdrawals{return None;}
        match id {
            TABLE=>Some([260,432,width-312,(height-696).max(60)]),
            PAGE=>Some([width-250,408,210,24]),
            _=>None,
        }
    }
    pub(super) unsafe fn withdrawal_buttons(&self){
        let ready=self.account.task.is_none();
        for id in [W_NETWORK,W_AMOUNT,W_ADDRESS,W_SUBMIT,W_CANCEL]{EnableWindow(id.hwnd(self),ready as i32);}
        if self.account.panel==Panel::Withdrawals{
            EnableWindow(W_SUBMIT.hwnd(self),(ready&&self.account.view["can_apply"]==true) as i32);
            EnableWindow(W_CANCEL.hwnd(self),(ready&&self.chosen_account_row().is_ok_and(|r|r["state"]=="pending")) as i32);
            EnableWindow(COPY.hwnd(self),1);
        }
    }
    pub(super) unsafe fn render_withdrawal(&mut self){
        self.set(DEPOSIT_TAB,if self.account.panel==Panel::Payments{"● 充值"}else{"充值"});
        self.set(WITHDRAW_TAB,if self.account.panel==Panel::Withdrawals{"● 提现"}else{"提现"});
        if self.account.panel!=Panel::Withdrawals{return;}
        let view=&self.account.view;
        self.set(W_BALANCE,&format!("可用余额 {}    提现冻结 {}",money(&view["balance"]["available_microusd"]),money(&view["balance"]["reserved_microusd"])));
        let old=SendMessageW(W_NETWORK.hwnd(self),CB_GETCURSEL,0,0);
        SendMessageW(W_NETWORK.hwnd(self),CB_RESETCONTENT,0,0);
        for c in view["channels"].as_array().into_iter().flatten(){SendMessageW(W_NETWORK.hwnd(self),CB_ADDSTRING,0,wide(&text(&c["name"])).as_ptr() as isize);}
        SendMessageW(W_NETWORK.hwnd(self),CB_SETCURSEL,old.max(0) as usize,0);
        self.set(DETAIL,"查看详情");self.set(COPY,"复制详情");
        self.withdrawal_amount_changed();
        self.set(DETAILS,"");
    }
    pub(in crate::desktop) unsafe fn withdrawal_amount_changed(&self){
        if self.account.panel!=Panel::Withdrawals{return;}
        let policy=&self.account.view["fee_policy"];
        if policy.is_null(){self.set(W_QUOTE,"");return;}
        let detail=if self.text(W_AMOUNT).trim().is_empty(){String::new()}else{
            match quote(policy,&self.text(W_AMOUNT)){
                Ok((total,fee,rate))=>format!("手续费 {}% · {} USDT    到账 {} USDT",rate as f64/100.0,amount(&json!(fee.to_string())),amount(&json!((total-fee).to_string()))),
                Err(error)=>error.to_string(),
            }
        };
        self.set(W_QUOTE,&format!("最低提现 {} USDT\r\n{}",amount(&policy["minimum_microusd"]),detail));
    }
    pub(super) unsafe fn show_withdrawal(&mut self,row:&Value){
        self.account.detail=row.clone();
        let detail=format!("申请 #{} · {} · {}\r\n申请 {} USDT    手续费 {} USDT    到账 {} USDT\r\n收款地址：{}\r\n交易哈希：{}    {}",
            text(&row["id"]),withdrawal_status(row),text(&row["network_name"]),amount(&row["amount_microusd"]),amount(&row["fee_microusd"]),amount(&row["payout_microusd"]),text(&row["address"]),text(&row["transaction_id"]),row["note"].as_str().unwrap_or(""));
        self.set(DETAILS,&detail);self.account.copied=Zeroizing::new(detail);self.withdrawal_buttons();
    }
    pub(super) unsafe fn withdrawal_action(&mut self,id:u16)->Result<bool>{
        if matches!(id,DEPOSIT_TAB|WITHDRAW_TAB){
            self.account.panel=if id==DEPOSIT_TAB{Panel::Payments}else{Panel::Withdrawals};
            self.account.page=0;self.account.cursors=vec![String::new()];self.account.view=Value::Null;
            self.account.detail=Value::Null;self.account.payment_qr=None;self.account.copied.clear();self.set(DETAILS,"");
            self.render_account();self.account_load(0,String::new())?;return Ok(true);
        }
        if self.account.panel!=Panel::Withdrawals{return Ok(false);}
        if id==W_SUBMIT{
            let (total,fee,rate)=quote(&self.account.view["fee_policy"],&self.text(W_AMOUNT))?;
            let display=|v:i128|amount(&Value::String(v.to_string()));
            self.set(W_QUOTE,&format!("手续费 {}% · {} USDT    到账 {} USDT",rate as f64/100.0,display(fee),display(total-fee)));
            ensure!(integer(&self.account.view["balance"]["available_microusd"]).is_some_and(|v|total<=v),"可用余额不足");
            let index=SendMessageW(W_NETWORK.hwnd(self),CB_GETCURSEL,0,0);
            let channel=self.account.view["channels"].as_array().and_then(|v|v.get(index as usize)).context("请选择提现网络")?;
            let network=channel["id"].as_str().context("请选择提现网络")?.to_owned();
            let address=self.text(W_ADDRESS).trim().to_owned();ensure!(!address.is_empty(),"请填写收款地址");
            let body=json!({"amount":display(total),"network":network,"address":address,"policy_version":self.account.view["fee_policy"]["version"]});
            let confirm=format!("网络：{}\n收款地址：{}\n\n申请 {} USDT\n手续费 {} USDT\n实际到账 {} USDT",text(&channel["name"]),address,display(total),display(fee),display(total-fee));
            if MessageBoxW(self.hwnd,wide(&confirm).as_ptr(),w!("确认提现"),MB_OKCANCEL|MB_ICONQUESTION)!=IDOK{return Ok(true);}
            let operation=self.account_operation(format!("withdrawal:{body}"))?;
            self.submit_account(Action::Withdrawal,"POST","withdrawals".into(),body,(0,String::new()),operation)?;return Ok(true);
        }
        if id==W_CANCEL{
            let row=self.chosen_account_row()?;ensure!(row["state"]=="pending","仅待处理的申请可取消");
            let operation=self.account_operation(format!("cancel-withdrawal:{}:{}",row["id"],row["version"]))?;
            self.submit_account(Action::CancelWithdrawal,"POST",format!("withdrawals/{}/cancel",text(&row["id"])),json!({"version":row["version"]}),(0,String::new()),operation)?;
            return Ok(true);
        }
        Ok(false)
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn exact_fee_boundaries_and_invalid_amounts(){
        let p=json!({"minimum_microusd":"10000000","tiers":[{"to_microusd":"100000000","rate_bps":200},{"to_microusd":null,"rate_bps":100}]});
        assert!(quote(&p,"9.999999").is_err());assert_eq!(quote(&p,"99.999999").unwrap(),(99999999,2000000,200));
        assert_eq!(quote(&p,"100").unwrap(),(100000000,1000000,100));
        for value in ["1e3","-1","NaN","1.0000001","1.","0",""]{assert!(micro(value).is_err());}
    }
}
