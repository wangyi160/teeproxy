//! Account forms and lists are native child controls in the main window.
#![allow(unsafe_op_in_unsafe_fn)]
use super::*;
use account_client::{AccountClient, Failure};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) const PLATFORM: u16 = 1511;
const USERNAME: u16 = 1512;
const PASSWORD: u16 = 1513;
const NAME: u16 = 1514;
const SUBMIT: u16 = 1515;
const SWITCH: u16 = 1516;
const IDENTITY: u16 = 1518;
const LOGOUT: u16 = 1519;
const SUMMARY: u16 = 1521;
pub(super) const TABLE: u16 = 1522;
const DETAILS: u16 = 1523;
const FEEDBACK: u16 = 1524;
const NETWORK: u16 = 1529;
const AMOUNT: u16 = 1530;
const PAY: u16 = 1531;
const DETAIL: u16 = 1532;
const COPY: u16 = 1533;
const PREV: u16 = 1534;
const NEXT: u16 = 1535;
const CURRENT: u16 = 1537;
const NEW_PASSWORD: u16 = 1538;
const CHANGE_PASSWORD: u16 = 1539;
const COPY_AMOUNT: u16 = 1540;
const PAGE: u16 = 1541;
const BALANCE: u16 = 1542;
const GO_LOGIN: u16 = 1543;
const PAYMENT_TAB: u16 = 1544;
const JOURNAL_TAB: u16 = 1545;
const ACCOUNT_NAME: u16 = 1546;
const CONNECTION_STATE: u16 = 1547;
const USAGE_SUMMARY: u16 = 1548;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(super) enum Panel {
    #[default]
    Overview,
    Payments,
    Finance,
    Journal,
}
impl Panel {
    fn for_page(page: usize) -> Self {
        match page {
            PAYMENTS_PAGE => Self::Payments,
            FINANCE_PAGE => Self::Finance,
            _ => Self::Overview,
        }
    }
    fn route(self, cursor: &str, offset: u64) -> String {
        match self {
            Self::Payments => {
                if cursor.is_empty() {
                    "payments".into()
                } else {
                    format!("payments?before={cursor}")
                }
            }
            Self::Finance | Self::Journal => {
                format!("finance?offset={offset}&journal_offset={offset}")
            }
            _ => "me".into(),
        }
    }
    fn has_table(self) -> bool {
        self != Self::Overview
    }
}
#[derive(Clone, Copy)]
enum Group {
    Always,
    Guest,
    Register,
    Locked,
    Member,
    Panel(Panel),
    Table,
    Wallet,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Load,
    Login,
    Register,
    Logout,
    Password,
    Bind,
    Payment,
    CheckPayment,
    Receipt,
}
pub(super) struct Reply {
    action: Action,
    panel: Panel,
    page: usize,
    cursor: String,
    pub result: std::result::Result<Value, Failure>,
    session: Zeroizing<Vec<u8>>,
}
pub(super) struct State {
    client: Option<Arc<AccountClient>>,
    base: String,
    pub task: Option<JoinHandle<()>>,
    pub panel: Panel,
    register: bool,
    me: Value,
    view: Value,
    groups: HashMap<usize, Group>,
    rows: Vec<Value>,
    channels: Vec<Value>,
    page: usize,
    cursors: Vec<String>,
    detail: Value,
    copied: Zeroizing<String>,
    attempt: Option<(String, String)>,
    refreshed: std::time::Instant,
    refresh_pending: bool,
    connection_id: String,
    connection_ready: bool,
    session: Zeroizing<Vec<u8>>,
    bind_retry: bool,
    connection_problem: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            client: None,
            base: String::new(),
            task: None,
            panel: Panel::Overview,
            register: false,
            me: Value::Null,
            view: Value::Null,
            groups: HashMap::new(),
            rows: Vec::new(),
            channels: Vec::new(),
            page: 0,
            cursors: vec![String::new()],
            detail: Value::Null,
            copied: Zeroizing::new(String::new()),
            attempt: None,
            refreshed: std::time::Instant::now(),
            refresh_pending: false,
            connection_id: String::new(),
            connection_ready: false,
            session: Zeroizing::new(Vec::new()),
            bind_retry: false,
            connection_problem: String::new(),
        }
    }
}
impl State {
    pub fn connection_ready(&self) -> bool {
        self.connection_ready
    }
    pub fn active_tab(&self, id: u16) -> bool {
        self.logged_in()
            && matches!(
                (id, self.panel),
                (PAYMENT_TAB, Panel::Payments) | (JOURNAL_TAB, Panel::Journal)
            )
    }
    pub fn logged_in(&self) -> bool {
        self.me.pointer("/account/role").and_then(Value::as_str) == Some("user")
    }
    fn clear(&mut self) {
        self.me = Value::Null;
        self.view = Value::Null;
        self.rows.clear();
        self.channels.clear();
        self.detail = Value::Null;
        self.copied = Zeroizing::new(String::new());
        self.attempt = None;
        self.page = 0;
        self.cursors = vec![String::new()];
        self.refresh_pending = false;
        self.connection_id.clear();
        self.connection_ready = false;
        self.session.clear();
        self.bind_retry = false;
        self.connection_problem.clear();
    }
    pub fn visible(&self, hwnd: HWND) -> bool {
        let logged = self.logged_in();
        match self.groups.get(&(hwnd as usize)) {
            Some(Group::Guest) => !logged && self.panel == Panel::Overview,
            Some(Group::Register) => !logged && self.register && self.panel == Panel::Overview,
            Some(Group::Locked) => !logged && self.panel != Panel::Overview,
            Some(Group::Member) => logged,
            Some(Group::Panel(panel)) => logged && self.panel == *panel,
            Some(Group::Table) => logged && self.panel.has_table(),
            Some(Group::Wallet) => logged && matches!(self.panel, Panel::Payments | Panel::Journal),
            _ => true,
        }
    }
}
fn text(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| {
        if value.is_null() {
            "—".into()
        } else {
            value.to_string()
        }
    })
}
fn integer(value: &Value) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}
fn amount(value: &Value) -> String {
    let Some(value) = integer(value) else {
        return "—".into();
    };
    let fraction = format!("{:06}", value.unsigned_abs() % 1_000_000);
    let fraction = fraction.trim_end_matches('0');
    format!(
        "{}{}.{:0<2}",
        if value < 0 { "-" } else { "" },
        value.unsigned_abs() / 1_000_000,
        fraction
    )
}
fn money(value: &Value) -> String {
    if value.is_null() {
        "—".into()
    } else {
        format!("${}", amount(value))
    }
}
fn date(value: &Value) -> String {
    let Some(seconds) = integer(value).and_then(|v| u64::try_from(v).ok()) else {
        return "—".into();
    };
    catalog::checked_time(seconds).unwrap_or_else(|| "—".into())
}
fn status(value: &Value) -> String {
    match value.as_str().unwrap_or("") {
        "enabled" => "启用",
        "disabled" => "停用",
        "provisioning" => "待下发",
        "pending" => "待处理",
        "creating" => "订单确认中",
        "paid" => "到账待入账",
        "credited" => "已入账",
        "expired" => "已过期",
        "failed" => "失败",
        "settled" => "已核算",
        "approved" => "已开通",
        "rejected" => "已拒绝",
        _ => return text(value),
    }
    .into()
}
impl App {
    unsafe fn account_group(&mut self, start: usize, group: Group) {
        for c in &self.controls[start..] {
            self.account.groups.insert(c.hwnd as usize, group);
        }
    }
    pub(super) unsafe fn build_account(&mut self) -> Result<()> {
        self.building_page = Some(ACCOUNT_PAGE);
        let start = self.controls.len();
        self.add(FEEDBACK, w!("STATIC"), "", SS_LEFT, [260, -75, -312, 42], 2)?;
        self.account_group(start, Group::Always);
        let start = self.controls.len();
        self.label("用户名", [300, 180, 150, 24], 0)?;
        self.edit(USERNAME, "", [300, 214, 460, 42], 0, "")?;
        self.label("密码", [300, 282, 150, 24], 0)?;
        self.edit(PASSWORD, "", [300, 316, 460, 42], ES_PASSWORD as u32, "")?;
        self.button(SUBMIT, "登录", [300, 496, 160, 42], 0)?;
        self.button(SWITCH, "注册新账号", [480, 496, 180, 42], 0)?;
        self.account_group(start, Group::Guest);
        let start = self.controls.len();
        self.label("名称", [300, 384, 150, 24], 0)?;
        self.edit(NAME, "", [300, 418, 460, 42], 0, "")?;
        self.account_group(start, Group::Register);
        let start = self.controls.len();
        self.add(IDENTITY, w!("STATIC"), "", SS_LEFT, [260, 134, -590, 28], 3)?;

        self.account_group(start, Group::Member);
        let start = self.controls.len();
        self.label("请先登录", [268, 210, 300, 28], 3)?;
        self.button(GO_LOGIN, "前往登录", [268, 264, 150, 40], 0)?;
        self.account_group(start, Group::Locked);
        let start = self.controls.len();
        self.label("用户名", [268, 210, 132, 24], 0)?;
        self.add(SUMMARY, w!("STATIC"), "", SS_LEFT, [420, 210, -480, 28], 3)?;
        self.label("名称", [268, 262, 132, 24], 0)?;
        self.add(
            ACCOUNT_NAME,
            w!("STATIC"),
            "",
            SS_LEFT,
            [420, 262, -480, 26],
            0,
        )?;
        self.label("连接状态", [268, 314, 132, 24], 0)?;
        self.add(
            CONNECTION_STATE,
            w!("STATIC"),
            "",
            SS_LEFT,
            [420, 314, -480, 26],
            0,
        )?;
        self.button(LOGOUT, "退出登录", [268, 370, 132, 38], 0)?;
        self.account_group(start, Group::Panel(Panel::Overview));
        let start = self.controls.len();
        self.table(
            TABLE,
            &[("", 180), ("", 180), ("", 150), ("", 125), ("", 120)],
        )?;
        self.controls
            .iter_mut()
            .find(|c| c.id == TABLE)
            .unwrap()
            .rect = [260, 320, -312, -585];
        self.add(
            DETAILS,
            w!("EDIT"),
            "",
            WS_TABSTOP
                | WS_VSCROLL
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_READONLY as u32,
            [260, -242, -312, 108],
            2,
        )?;
        self.button(DETAIL, "查看", [260, -122, 96, 34], 0)?;
        self.button(COPY, "复制详情", [368, -122, 112, 34], 0)?;
        self.button(PREV, "上一页", [-252, -122, 96, 34], 0)?;
        self.button(NEXT, "下一页", [-144, -122, 96, 34], 0)?;
        self.add(PAGE, w!("STATIC"), "", SS_LEFT, [-250, 184, 210, 24], 2)?;
        self.account_group(start, Group::Table);
        let start = self.controls.len();
        self.button(PAYMENT_TAB, "充值订单", [260, 258, 124, 38], 0)?;
        self.button(JOURNAL_TAB, "余额变动", [396, 258, 124, 38], 0)?;
        self.account_group(start, Group::Wallet);
        let start = self.controls.len();
        self.add(BALANCE, w!("STATIC"), "", SS_LEFT, [260, 184, -590, 28], 3)?;
        self.account_group(start, Group::Wallet);
        let start = self.controls.len();
        self.add(
            NETWORK,
            w!("COMBOBOX"),
            "",
            WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
            [260, 218, 180, 250],
            0,
        )?;
        self.edit(AMOUNT, "10", [452, 212, 150, 38], 0, "USDT 金额")?;
        self.button(PAY, "创建充值订单", [614, 212, 150, 38], 0)?;
        self.button(COPY_AMOUNT, "复制付款金额", [492, -122, 150, 34], 0)?;
        self.account_group(start, Group::Panel(Panel::Payments));
        let start = self.controls.len();
        self.label("当前密码", [268, 466, 132, 24], 0)?;
        self.edit(CURRENT, "", [420, 456, -480, 40], ES_PASSWORD as u32, "")?;
        self.label("新密码", [268, 524, 132, 24], 0)?;
        self.edit(
            NEW_PASSWORD,
            "",
            [420, 514, -480, 40],
            ES_PASSWORD as u32,
            "",
        )?;
        self.button(CHANGE_PASSWORD, "修改密码", [420, 576, 160, 40], 0)?;
        self.account_group(start, Group::Panel(Panel::Overview));
        let start = self.controls.len();
        self.add(
            USAGE_SUMMARY,
            w!("STATIC"),
            "",
            SS_LEFT,
            [260, 218, -312, 60],
            3,
        )?;
        self.account_group(start, Group::Panel(Panel::Finance));
        for (id, max) in [
            (USERNAME, 64),
            (PASSWORD, 512),
            (NAME, 128),
            (CURRENT, 512),
            (NEW_PASSWORD, 512),
            (AMOUNT, 20),
        ] {
            SendMessageW(self.by_id[&id], EM_SETLIMITTEXT, max, 0);
        }
        self.building_page = None;
        Ok(())
    }
    unsafe fn account_busy(&self) {
        let ready = self.account.task.is_none();
        for control in &self.controls {
            if control.page == Some(ACCOUNT_PAGE) && control.id != FEEDBACK {
                EnableWindow(control.hwnd, ready as i32);
            }
        }
        if ready && self.account.logged_in() {
            EnableWindow(PREV.hwnd(self), (self.account.page > 0) as i32);
            let view = if self.account.panel == Panel::Journal {
                &self.account.view["journal"]
            } else {
                &self.account.view
            };
            EnableWindow(
                NEXT.hwnd(self),
                view["has_more"].as_bool().unwrap_or(false) as i32,
            );
            let configured = self.account.view.pointer("/config/enabled")
                == Some(&Value::Bool(true))
                && !self.account.channels.is_empty();
            EnableWindow(PAY.hwnd(self), configured as i32);
        }
    }
    pub(super) unsafe fn account_page_entered(&mut self) {
        self.account.panel = if self.page == PAYMENTS_PAGE && self.account.panel == Panel::Journal {
            Panel::Journal
        } else {
            Panel::for_page(self.page)
        };
        self.account.page = 0;
        self.account.cursors = vec![String::new()];
        self.account.view = Value::Null;
        self.account.detail = Value::Null;
        self.account.copied = Zeroizing::new(String::new());
        self.set(DETAILS, "");
        self.set(FEEDBACK, "");
        self.render_account();
        self.account.refresh_pending = self.account.logged_in();
        if self.account.refresh_pending
            && self.account.task.is_none()
            && let Err(e) = self.account_load(0, String::new())
        {
            self.set(FEEDBACK, &e.to_string());
        }
    }
    unsafe fn submit_account(
        &mut self,
        action: Action,
        method: &str,
        route: String,
        body: Value,
        pagination: (usize, String),
        operation: String,
    ) -> Result<()> {
        let (page, cursor) = pagination;
        ensure!(self.account.task.is_none(), "账户操作正在进行");
        let client = self.account.client.clone().context("请先登录")?;
        let sender = self.sender.clone();
        let method = method.to_owned();
        let panel = self.account.panel;
        self.account.task = Some(self.runtime.spawn(async move {
            let result = if action == Action::Load {
                client.load(&route).await
            } else {
                client.request(&method, &route, body, &operation).await
            };
            let _ = sender.send(Event::Account(Reply {
                action,
                panel,
                page,
                cursor,
                result,
                session: client.saved_session().await,
            }));
        }));
        self.set(FEEDBACK, "正在处理…");
        self.account_busy();
        Ok(())
    }
    unsafe fn account_load(&mut self, page: usize, cursor: String) -> Result<()> {
        self.account.refresh_pending = false;
        self.submit_account(
            Action::Load,
            "GET",
            self.account.panel.route(&cursor, (page as u64) * 50),
            json!({}),
            (page, cursor),
            String::new(),
        )
    }
    unsafe fn chosen_account_row(&self) -> Result<Value> {
        selected(TABLE.hwnd(self))
            .and_then(|i| self.account.rows.get(i))
            .cloned()
            .context("请先选择一条记录")
    }
    unsafe fn account_operation(&mut self, fingerprint: String) -> Result<String> {
        if self
            .account
            .attempt
            .as_ref()
            .is_none_or(|(old, _)| old != &fingerprint)
        {
            self.account.attempt = Some((fingerprint, account_client::operation_id()?));
        }
        Ok(self.account.attempt.as_ref().unwrap().1.clone())
    }
    pub(super) unsafe fn account_action(&mut self, id: u16) -> Result<()> {
        if id == GO_LOGIN {
            self.select_page(ACCOUNT_PAGE);
            return Ok(());
        }
        if self.account.task.is_some() {
            return Ok(());
        }
        if matches!(id, PAYMENT_TAB | JOURNAL_TAB) {
            self.account.panel = if id == PAYMENT_TAB {
                Panel::Payments
            } else {
                Panel::Journal
            };
            self.account_page_entered();
            return Ok(());
        }
        if id == SWITCH {
            self.account.register = !self.account.register;
            self.set(PASSWORD, "");
            self.set(
                SUBMIT,
                if self.account.register {
                    "注册"
                } else {
                    "登录"
                },
            );
            self.set(
                SWITCH,
                if self.account.register {
                    "已有账号，登录"
                } else {
                    "注册新账号"
                },
            );
            self.set(FEEDBACK, "");
            self.update_visibility();
            self.layout();
            return Ok(());
        }
        if id == SUBMIT {
            let base = account_client::platform(&self.text(PLATFORM))?;
            let username = self.text(USERNAME).trim().to_owned();
            ensure!(
                (3..=64).contains(&username.len())
                    && username
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c)),
                "用户名须为 3–64 位字母、数字、点、下划线或连字符"
            );
            let password = Zeroizing::new(self.text(PASSWORD));
            ensure!(!password.is_empty() && password.len() <= 512, "请填写密码");
            let registering = self.account.register;
            if registering {
                ensure!(password.chars().count() >= 12, "密码至少 12 个字符");
                ensure!(!self.text(NAME).trim().is_empty(), "请填写名称");
            }
            self.disconnect_identity()?;
            self.account.clear();
            self.account.base = base.clone();
            self.account.client = Some(Arc::new(AccountClient::new(&base)?));
            let body = if registering {
                json!({"username":username,"password":password.as_str(),"name":self.text(NAME).trim(),"role":"user"})
            } else {
                json!({"username":username,"password":password.as_str()})
            };
            self.set(PASSWORD, "");
            return self.submit_account(
                if registering {
                    Action::Register
                } else {
                    Action::Login
                },
                "POST",
                if registering { "register" } else { "login" }.into(),
                body,
                (0, String::new()),
                String::new(),
            );
        }
        ensure!(self.account.logged_in(), "请先登录");
        let action;
        let mut method = "GET";
        let route;
        let mut body = json!({});
        let mut operation = String::new();
        match id {
            LOGOUT => {
                self.disconnect_identity()?;
                self.account.clear();
                self.render_account();
                action = Action::Logout;
                method = "POST";
                route = "logout".into();
            }
            PREV => {
                if self.account.page > 0 {
                    let page = self.account.page - 1;
                    return self.account_load(page, self.account.cursors[page].clone());
                }
                return Ok(());
            }
            NEXT => {
                let view = if self.account.panel == Panel::Journal {
                    &self.account.view["journal"]
                } else {
                    &self.account.view
                };
                ensure!(view["has_more"] == true, "已到最后一页");
                let cursor = view["next_before"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| view["next_before"].as_u64().map(|v| v.to_string()))
                    .unwrap_or_default();
                return self.account_load(self.account.page + 1, cursor);
            }
            PAY => {
                ensure!(
                    self.account.view.pointer("/config/enabled") == Some(&Value::Bool(true)),
                    "充值尚未启用"
                );
                let channel = SendMessageW(NETWORK.hwnd(self), CB_GETCURSEL, 0, 0);
                let network = self
                    .account
                    .channels
                    .get(channel as usize)
                    .context("请选择充值网络")?["id"]
                    .as_str()
                    .context("充值网络无效")?
                    .to_owned();
                let amount = self.text(AMOUNT).trim().to_owned();
                ensure!(
                    !amount.is_empty() && amount.bytes().all(|v| v.is_ascii_digit() || v == b'.'),
                    "请填写正确的 USDT 金额"
                );
                operation = self.account_operation(format!("payment:{network}:{amount}"))?;
                action = Action::Payment;
                method = "POST";
                route = "payments".into();
                body = json!({"amount":amount,"network":network});
            }
            CHANGE_PASSWORD => {
                let current = Zeroizing::new(self.text(CURRENT));
                let next = Zeroizing::new(self.text(NEW_PASSWORD));
                ensure!(
                    !current.is_empty() && next.chars().count() >= 12,
                    "请填写当前密码，新密码至少 12 个字符"
                );
                body = json!({"current_password":current.as_str(),"new_password":next.as_str()});
                self.set(CURRENT, "");
                self.set(NEW_PASSWORD, "");
                action = Action::Password;
                method = "POST";
                route = "password".into();
            }
            COPY => {
                ensure!(!self.account.copied.is_empty(), "请先查看详情");
                copy_text(self.hwnd, &self.account.copied)?;
                self.set(FEEDBACK, "已复制");
                return Ok(());
            }
            COPY_AMOUNT => {
                ensure!(
                    self.account.detail["state"] == "pending",
                    "仅等待付款的订单可以复制付款金额"
                );
                copy_text(self.hwnd, &amount(&self.account.detail["payable_microusd"]))?;
                self.set(FEEDBACK, "付款金额已复制");
                return Ok(());
            }
            DETAIL => {
                let row = self.chosen_account_row()?;
                match self.account.panel {
                    Panel::Payments => {
                        action = Action::CheckPayment;
                        method = "POST";
                        route = format!("payments/{}/refresh", text(&row["id"]));
                    }
                    Panel::Finance => {
                        action = Action::Receipt;
                        route = format!("receipts/records/{}", text(&row["seq"]));
                    }
                    _ => {
                        self.account_selection_changed();
                        return Ok(());
                    }
                }
            }
            _ => return Ok(()),
        }
        self.submit_account(action, method, route, body, (0, String::new()), operation)
    }
    pub(super) unsafe fn account_selection_changed(&mut self) {
        let Ok(row) = self.chosen_account_row() else {
            self.set(DETAILS, "");
            self.account.copied = Zeroizing::new(String::new());
            return;
        };
        if self.account.panel == Panel::Payments {
            self.show_payment(row);
            return;
        }
        let detail = match self.account.panel {
            Panel::Journal => format!(
                "记录：{}\r\n金额：{}\r\n时间：{}",
                text(&row["id"]),
                money(&row["amount"]),
                date(&row["created_at"])
            ),
            _ => format!(
                "流水：{}\r\n模型：{} / {}\r\n账号池：{}\r\n金额：{}",
                text(&row["seq"]),
                text(&row["provider"]),
                text(&row["model"]),
                text(&row["pool"]),
                money(&row["charge_microusd"])
            ),
        };
        self.set(DETAILS, &detail);
        self.account.copied = Zeroizing::new(detail);
    }
    unsafe fn show_payment(&mut self, row: Value) {
        let payable = row["state"] == "pending"
            && row["receive_address"]
                .as_str()
                .is_some_and(|s| !s.is_empty());
        let detail = format!(
            "{} · {}\r\n付款金额：{} USDT　到账余额：{}\r\n收款地址：{}\r\n有效期至：{}\r\n订单：{}\r\n仅发送该网络的 USDT，按订单金额付款。",
            text(&row["network_name"]),
            status(&row["state"]),
            amount(&row["payable_microusd"]),
            money(&row["credit_microusd"]),
            if payable {
                text(&row["receive_address"])
            } else {
                "—".into()
            },
            date(&row["expires_at"]),
            text(&row["id"])
        );
        self.set(DETAILS, &detail);
        self.account.copied = Zeroizing::new(if payable {
            text(&row["receive_address"])
        } else {
            String::new()
        });
        self.account.detail = row;
    }
    pub(super) unsafe fn render_account(&mut self) {
        let logged = self.account.logged_in();
        self.set(
            IDENTITY,
            &if logged {
                text(&self.account.me["account"]["username"])
            } else {
                String::new()
            },
        );
        self.set(DETAILS, "");
        if logged {
            let balance = &self.account.me["balance"];
            let available = integer(&balance["ceiling_microusd"])
                .zip(integer(&balance["spent_microusd"]))
                .map(|(a, b)| Value::String((a - b).to_string()))
                .unwrap_or(Value::Null);
            self.set(BALANCE, &format!("可用余额　{}", money(&available)));
            self.set(SUMMARY, &text(&self.account.me["account"]["username"]));
            self.set(ACCOUNT_NAME, &text(&self.account.me["account"]["name"]));
            self.set(
                CONNECTION_STATE,
                if !self.account.connection_problem.is_empty() {
                    &self.account.connection_problem
                } else if self.router_key.is_empty() {
                    "正在绑定"
                } else if self.account.connection_ready {
                    "已绑定"
                } else {
                    "等待 Router 同步"
                },
            );
            let summary = &self.account.view["summary"];
            self.set(
                USAGE_SUMMARY,
                &format!(
                    "今日消费 {}    本月消费 {}\r\n调用 {} 次    待核算 {} 条",
                    money(&summary["today_microusd"]),
                    money(&summary["month_microusd"]),
                    text(&summary["requests"]),
                    text(&summary["pending"])
                ),
            );
        } else {
            self.set(SUMMARY, "");
            self.set(BALANCE, "");
            self.set(ACCOUNT_NAME, "");
            self.set(CONNECTION_STATE, "");
            self.set(USAGE_SUMMARY, "");
        }
        let panel = self.account.panel;
        let source = if !logged {
            Value::Null
        } else {
            match panel {
                Panel::Journal => self.account.view["journal"]["data"].clone(),
                _ => self.account.view["data"].clone(),
            }
        };
        self.account.rows = source.as_array().cloned().unwrap_or_default();
        let headings = match panel {
            Panel::Payments => ["时间", "网络", "付款金额 USDT", "状态", "订单"],
            Panel::Journal => ["时间", "类型", "金额 USD", "编号", ""],
            _ => ["时间", "模型 / 账号池", "金额 USD", "状态", "流水"],
        };
        for (i, label) in headings.iter().enumerate() {
            let mut label = wide(label);
            let column = LVCOLUMNW {
                mask: LVCF_TEXT | LVCF_WIDTH,
                cx: self.scale([174, 240, 130, 132, 100][i]),
                pszText: label.as_mut_ptr(),
                ..mem::zeroed()
            };
            SendMessageW(
                TABLE.hwnd(self),
                LVM_SETCOLUMNW,
                i,
                (&column as *const LVCOLUMNW) as isize,
            );
        }
        let rows: Vec<Vec<String>> = self
            .account
            .rows
            .iter()
            .map(|row| match panel {
                Panel::Payments => vec![
                    date(&row["created_at"]),
                    text(&row["network_name"]),
                    amount(
                        row.get("payable_microusd")
                            .filter(|v| !v.is_null())
                            .unwrap_or(&row["requested_microusd"]),
                    ),
                    status(&row["state"]),
                    text(&row["id"]),
                ],
                Panel::Journal => vec![
                    date(&row["created_at"]),
                    if row["kind"] == "credit" {
                        "余额增加".into()
                    } else {
                        "余额减少".into()
                    },
                    money(&row["amount"]),
                    text(&row["id"]),
                    String::new(),
                ],
                _ => vec![
                    date(&row["created_at"]),
                    format!("{} / {}", text(&row["model"]), text(&row["pool"])),
                    money(&row["charge_microusd"]),
                    if row["charge_microusd"].is_null() {
                        "待核算".into()
                    } else if ["estimated", "mixed"]
                        .contains(&row["usage_source"].as_str().unwrap_or(""))
                    {
                        "已计费 · 含估算".into()
                    } else {
                        "已核算".into()
                    },
                    text(&row["seq"]),
                ],
            })
            .collect();
        fill(TABLE.hwnd(self), &rows);
        self.set(
            PAGE,
            &format!("第 {} 页 · {} 条", self.account.page + 1, rows.len()),
        );
        self.set(
            DETAIL,
            if panel == Panel::Payments {
                "刷新订单"
            } else if panel == Panel::Finance {
                "消费凭据"
            } else {
                "查看"
            },
        );
        self.set(
            COPY,
            if panel == Panel::Payments {
                "复制地址"
            } else {
                "复制详情"
            },
        );
        if panel == Panel::Payments {
            let old = SendMessageW(NETWORK.hwnd(self), CB_GETCURSEL, 0, 0);
            let chosen = self
                .account
                .channels
                .get(old as usize)
                .map(|c| c["id"].clone());
            self.account.channels = self.account.view["config"]["channels"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            SendMessageW(NETWORK.hwnd(self), CB_RESETCONTENT, 0, 0);
            for c in &self.account.channels {
                SendMessageW(
                    NETWORK.hwnd(self),
                    CB_ADDSTRING,
                    0,
                    wide(&text(&c["name"])).as_ptr() as isize,
                );
            }
            let index = chosen
                .and_then(|c| self.account.channels.iter().position(|r| r["id"] == c))
                .unwrap_or(0);
            SendMessageW(NETWORK.hwnd(self), CB_SETCURSEL, index, 0);
        }
        self.update_visibility();
        self.account_busy();
        self.busy(self.task.is_some());
        self.layout();
    }
    pub(super) unsafe fn account_reply(&mut self, reply: Reply) {
        self.account.task.take();
        self.account.refreshed = std::time::Instant::now();
        let action = reply.action;
        self.account.session = reply.session;
        let value = match reply.result {
            Ok(value) => value,
            Err(error) => {
                if error.status == 401 || action == Action::Logout {
                    let _ = self.disconnect_identity();
                    self.account.clear();
                    self.account.client = None;
                    self.render_account();
                }
                if action == Action::Bind {
                    self.account.bind_retry =
                        error.status == 0 || error.status == 429 || error.status >= 500;
                    self.account.connection_problem = error.message.clone();
                    self.render_account();
                }
                self.set(FEEDBACK, &error.message);
                self.account_busy();
                self.finish_account_refresh();
                return;
            }
        };
        if action == Action::Payment {
            self.account.attempt = None;
        }
        // A page switch does not cancel a sent mutation. Complete it, but keep
        // its response out of the newly selected page and then read that page.
        if (reply.panel != self.account.panel || self.account.refresh_pending)
            && matches!(
                action,
                Action::Load | Action::Receipt | Action::Payment | Action::CheckPayment
            )
        {
            if action == Action::Load {
                self.account.me = value["me"].clone();
                self.update_connection_state();
            }
            self.render_account();
            self.finish_account_refresh();
            return;
        }
        let mut followup = false;
        match action {
            Action::Login | Action::Register => {
                self.account.me = value;
                self.account.bind_retry = true;
                self.account.register = false;
                self.account.refresh_pending =
                    is_account_page(self.page) && self.page != ACCOUNT_PAGE;
                let save = (|| -> Result<()> {
                    std::fs::create_dir_all(self.path.parent().unwrap())?;
                    std::fs::write(
                        self.path.parent().unwrap().join("account-platform.json"),
                        serde_json::to_vec(&self.account.base)?,
                    )?;
                    Ok(())
                })();
                self.render_account();
                self.set(
                    FEEDBACK,
                    if save.is_ok() {
                        "已登录，正在绑定连接"
                    } else {
                        "已登录，平台地址未能保存"
                    },
                );
            }
            Action::Bind => {
                let mut value = value;
                let key =
                    Zeroizing::new(value["credential"].take().as_str().unwrap_or("").to_owned());
                let result = self.bind_identity(&value, &key);
                if let Err(e) = result {
                    self.set(FEEDBACK, &e.to_string());
                } else {
                    self.set(
                        FEEDBACK,
                        if self.account.connection_ready {
                            "连接已就绪"
                        } else {
                            "等待 Router 同步"
                        },
                    );
                }
                self.account.bind_retry = self.router_key.is_empty();
                self.render_account();
            }
            Action::Load => {
                let payment_id = self.account.detail["id"].clone();
                self.account.me = value["me"].clone();
                self.update_connection_state();
                self.account.view = value["view"].clone();
                self.account.page = reply.page;
                self.account.cursors.truncate(reply.page + 1);
                if self.account.cursors.len() == reply.page {
                    self.account.cursors.push(reply.cursor);
                }
                self.account.detail = Value::Null;
                self.account.copied = Zeroizing::new(String::new());
                self.render_account();
                if self.account.panel == Panel::Payments
                    && let Some(row) = self
                        .account
                        .rows
                        .iter()
                        .find(|r| r["id"] == payment_id)
                        .cloned()
                {
                    self.show_payment(row);
                }
                self.set(
                    FEEDBACK,
                    if self.account.rows.is_empty() && self.account.panel.has_table() {
                        "暂无记录"
                    } else {
                        "已更新"
                    },
                );
            }
            Action::Logout | Action::Password => {
                let _ = self.disconnect_identity();
                self.account.clear();
                self.account.client = None;
                self.set(PASSWORD, "");
                self.set(CURRENT, "");
                self.set(NEW_PASSWORD, "");
                self.set(SUBMIT, "登录");
                self.set(SWITCH, "注册新账号");
                if is_account_page(self.page) {
                    self.select_page(ACCOUNT_PAGE);
                }
                self.render_account();
                self.set(
                    FEEDBACK,
                    if action == Action::Password {
                        "密码已修改，请重新登录"
                    } else {
                        "已退出登录"
                    },
                );
            }
            Action::Receipt => {
                let detail = serde_json::to_string_pretty(&value).unwrap_or_default();
                self.set(DETAILS, &detail);
                self.account.copied = Zeroizing::new(detail);
                self.set(COPY, "复制凭据 JSON");
                self.set(FEEDBACK, "凭据已读取");
            }
            Action::Payment | Action::CheckPayment => {
                self.show_payment(value);
                self.set(FEEDBACK, "订单已更新，请核对网络、金额和地址");
                if action == Action::Payment {
                    self.account.page = 0;
                    self.account.cursors = vec![String::new()];
                }
                followup = true;
            }
        }
        self.account_busy();
        if followup
            && is_account_page(self.page)
            && let Err(e) = self.account_load(
                self.account.page,
                self.account.cursors[self.account.page].clone(),
            )
        {
            self.set(FEEDBACK, &e.to_string());
        }
        if matches!(action, Action::Login | Action::Register)
            && let Err(e) = self.bind_account()
        {
            self.set(FEEDBACK, &e.to_string());
        }
        self.finish_account_refresh();
    }
    unsafe fn finish_account_refresh(&mut self) {
        if self.account.refresh_pending
            && self.account.task.is_none()
            && self.account.logged_in()
            && is_account_page(self.page)
            && let Err(error) = self.account_load(0, String::new())
        {
            self.set(FEEDBACK, &error.to_string());
        }
    }
    unsafe fn bind_account(&mut self) -> Result<()> {
        let device = identity::device(self.path.parent().unwrap())?;
        self.submit_account(
            Action::Bind,
            "POST",
            "connection".into(),
            json!({"device_id": device}),
            (0, String::new()),
            String::new(),
        )
    }
    unsafe fn bind_identity(&mut self, value: &Value, key: &str) -> Result<()> {
        ensure!(!key.is_empty() && key.len() <= 8192, "平台连接凭据无效");
        let router = value["router_url"]
            .as_str()
            .context("平台未配置 Router 地址")?;
        let url = reqwest::Url::parse(router)?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none(),
            "Router 地址无效"
        );
        self.router_key = Zeroizing::new(key.to_owned());
        self.account.connection_problem.clear();
        self.account.connection_id = value["connection_id"]
            .as_str()
            .context("连接身份无效")?
            .to_owned();
        self.account.connection_ready = value["state"] == "enabled";
        self.set(ROUTER, router);
        self.persist_identity()?;
        self.invalidate_catalog();
        self.invalidate_prices();
        self.update_summary();
        self.busy(self.task.is_some());
        Ok(())
    }
    unsafe fn persist_identity(&self) -> Result<()> {
        let session: Value = serde_json::from_slice(&self.account.session)?;
        // Only identity/session data is cached. Wallet and financial records
        // are always loaded from the platform for the current account.
        identity::save(
            self.path.parent().unwrap(),
            &json!({
                "version":1,"platform":self.account.base,"account":self.account.me["account"],
                "connection_id":self.account.connection_id,"credential":self.router_key.as_str(),
                "router_url":self.text(ROUTER),"session":session
            }),
        )
    }
    pub(super) unsafe fn restore_identity(&mut self) -> Result<()> {
        let Some(saved) = identity::load(self.path.parent().unwrap())? else {
            return Ok(());
        };
        ensure!(
            saved["version"] == 1 && saved["account"]["role"] == "user",
            "登录信息无效"
        );
        let base = account_client::platform(saved["platform"].as_str().context("平台地址无效")?)?;
        let client = AccountClient::restore(&base, &saved["session"])?;
        self.account.base = base.clone();
        self.set(PLATFORM, &base);
        self.account.client = Some(Arc::new(client));
        self.account.me = json!({"account":saved["account"]});
        self.router_key = Zeroizing::new(saved["credential"].as_str().unwrap_or("").to_owned());
        self.account.connection_id = saved["connection_id"].as_str().unwrap_or("").to_owned();
        self.set(ROUTER, saved["router_url"].as_str().unwrap_or(""));
        self.account.session = Zeroizing::new(serde_json::to_vec(&saved["session"])?);
        self.account.bind_retry = self.router_key.is_empty();
        // Verify the restored session before enabling this account's proxy.
        self.account.connection_ready = false;
        self.render_account();
        self.account_load(0, String::new())
    }
    unsafe fn update_connection_state(&mut self) {
        if self.account.connection_id.is_empty() {
            return;
        }
        let state = self.account.me["keys"]
            .as_array()
            .and_then(|keys| keys.iter().find(|k| k["id"] == self.account.connection_id))
            .map(|k| text(&k["state"]));
        self.account.connection_ready = state.as_deref() == Some("enabled");
        if state.is_none()
            || state.as_deref() == Some("disabled")
            || self.account.me.pointer("/balance/enabled") == Some(&Value::Bool(false))
        {
            self.stop();
            self.router_key = Zeroizing::new(String::new());
            self.account.bind_retry = false;
            self.account.connection_problem = "连接已停用".into();
            let _ = identity::clear(self.path.parent().unwrap());
        }
        self.busy(self.task.is_some());
    }
    unsafe fn disconnect_identity(&mut self) -> Result<()> {
        self.stop();
        self.router_key = Zeroizing::new(String::new());
        self.account.connection_id.clear();
        self.account.connection_ready = false;
        self.account.bind_retry = false;
        self.invalidate_catalog();
        self.invalidate_prices();
        self.update_summary();
        identity::clear(self.path.parent().unwrap())
    }
    pub(super) unsafe fn account_poll(&mut self) {
        if self.account.logged_in()
            && self.account.task.is_none()
            && self.account.bind_retry
            && self.account.refreshed.elapsed().as_secs() >= 15
            && let Err(e) = self.bind_account()
        {
            self.set(FEEDBACK, &e.to_string());
        }
        if (is_account_page(self.page) || !self.account.connection_ready)
            && IsWindowVisible(self.hwnd) != 0
            && self.account.logged_in()
            && self.account.task.is_none()
            && self.account.refreshed.elapsed().as_secs()
                >= if !self.account.connection_ready {
                    5
                } else if self.account.panel == Panel::Payments {
                    15
                } else {
                    300
                }
            && let Err(e) = self.account_load(
                self.account.page,
                self.account.cursors[self.account.page].clone(),
            )
        {
            self.set(FEEDBACK, &e.to_string());
            self.account.refreshed = std::time::Instant::now();
        }
    }
    pub(super) unsafe fn account_error(&self, error: &anyhow::Error) {
        self.set(FEEDBACK, &error.to_string());
    }
}
trait Handle {
    unsafe fn hwnd(self, app: &App) -> HWND;
}
impl Handle for u16 {
    unsafe fn hwnd(self, app: &App) -> HWND {
        app.by_id[&self]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe fn complete(app: &mut App) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while app.account.task.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "native account task timed out"
            );
            while let Ok(event) = app.receiver.try_recv() {
                if let Event::Account(reply) = event {
                    app.account_reply(reply);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    #[test]
    fn native_account_forms_load_data_and_clear_it_on_signout() {
        use axum::{
            Json, Router,
            http::HeaderMap,
            routing::{get, post},
        };
        unsafe {
            let root = std::env::temp_dir().join(format!(
                "teeproxy-native-account-{}",
                account_client::operation_id().unwrap()
            ));
            let mut app = App::new(root.join("desktop.json")).unwrap();
            let controls = INITCOMMONCONTROLSEX {
                dwSize: mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_STANDARD_CLASSES | ICC_LISTVIEW_CLASSES,
            };
            InitCommonControlsEx(&controls);
            app.hwnd = CreateWindowExW(
                0,
                w!("STATIC"),
                w!("native account test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                1100,
                800,
                ptr::null_mut(),
                ptr::null_mut(),
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            );
            assert!(!app.hwnd.is_null());
            app.build_shell(&Profile::default(), &Secrets::default())
                .unwrap();
            app.page = ACCOUNT_PAGE;
            let view = json!({"account":{"role":"user","username":"fixture-user","name":"Fixture"},"balance":{"enabled":true,"ceiling_microusd":20000000,"spent_microusd":500001},"keys":[{"id":"k_fixture","label":"Laptop","prefix":"fixture","state":"enabled"}],"applications":[],"merchants":[]});
            let login = view.clone();
            let me = view.clone();
            let payment_calls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
            let payment_posts = payment_calls.clone();
            let payment_state = Arc::new(std::sync::Mutex::new(Value::Null));
            let payment_get = payment_state.clone();
            let payment_post = payment_state.clone();
            let mock=Router::new()
                .route("/portal/api/login",post(move ||{let mut v=login.clone();v["csrf"]=json!("fixture-csrf");async move {([("set-cookie","teerouter_portal=fixture; HttpOnly")],Json(v))}}))
                .route("/portal/api/me",get(move |headers:HeaderMap|{let v=me.clone();async move {assert_eq!(headers["cookie"],"teerouter_portal=fixture");Json(v)}}))
                .route("/portal/api/connection",post(|headers:HeaderMap,Json(body):Json<Value>|async move {
                    assert_eq!(body.as_object().unwrap().len(), 1);
                    assert_eq!(body["device_id"].as_str().unwrap().len(), 48);
                    assert_eq!(headers["cookie"],"teerouter_portal=fixture");
                    assert_eq!(headers["x-csrf-token"],"fixture-csrf");
                    Json(json!({"credential":"fixture-private-router-credential","connection_id":"k_fixture","state":"enabled","router_url":"https://router.example.invalid"}))
                }))
                .route("/portal/api/finance", get(|| async { Json(json!({"summary":{"requests":1,"pending":0,"today_microusd":123,"month_microusd":123},"data":[{"seq":"1","model":"fixture","pool":"fixture-pool","charge_microusd":"123"}],"journal":{"data":[{"id":"2","kind":"credit","amount":"20000000"}],"has_more":false}})) }))
                .route("/portal/api/receipts/records/1", get(|| async { Json(json!({"proof":"signed-fixture"})) }))
                .route("/portal/api/payments",get(move || {
                    let order = payment_get.lock().unwrap().clone();
                    async move {Json(json!({"data":if order.is_null(){vec![]}else{vec![order]},"has_more":false,"config":{"enabled":true,"channels":[{"id":"tron","name":"TRON"}]}}))}
                }).post(move |headers:HeaderMap,Json(body):Json<Value>| {
                    let mut calls = payment_posts.lock().unwrap();
                    calls.push(headers["idempotency-key"].to_str().unwrap().to_owned());
                    assert_eq!(body,json!({"network":"tron","amount":"10"}));
                    let first = calls.len()==1;
                    let order=json!({"id":"fixture-order","state":"pending","network_name":"TRON","payable_microusd":"10000123","credit_microusd":"10000123","receive_address":"fixture-address"});
                    if !first {*payment_post.lock().unwrap()=order.clone();}
                    async move {if first {(axum::http::StatusCode::BAD_GATEWAY,Json(json!({"error":"temporary_failure"})))}else{(axum::http::StatusCode::OK,Json(order))}}
                }))
                .route("/portal/api/logout",post(||async{Json(json!({"ok":true}))}));
            let listener = app
                .runtime
                .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
                .unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let server = app.runtime.spawn(async {
                axum::serve(listener, mock).await.unwrap();
            });
            app.account.client = Some(Arc::new(AccountClient::test_client(base)));
            app.account.base = "https://example.invalid".into();
            app.submit_account(
                Action::Login,
                "POST",
                "login".into(),
                json!({"username":"fixture-user","password":"example-password"}),
                (0, String::new()),
                String::new(),
            )
            .unwrap();
            complete(&mut app);
            assert!(app.account.logged_in());
            assert!(!app.text(SUMMARY).contains("$19.499999"));
            assert!(app.text(BALANCE).contains("$19.499999"));
            assert!(!app.account.visible(USERNAME.hwnd(&app)));
            assert!(app.account.visible(IDENTITY.hwnd(&app)));
            assert_eq!(app.router_key.as_str(), "fixture-private-router-credential");
            assert!(app.account.connection_ready());
            assert!(!app.by_id.contains_key(&102));
            assert!(!app.by_id.contains_key(&1527));
            app.set(CLIENT_KEY, "local-api-key-never-uploaded");
            let saved = identity::load(&root).unwrap().unwrap();
            assert_eq!(saved["credential"], "fixture-private-router-credential");
            assert!(!saved.to_string().contains("local-api-key"));
            assert!(saved.get("balance").is_none());
            // Switching away while a read is in flight cannot fill the wrong page.
            app.select_page(FINANCE_PAGE);
            app.select_page(PAYMENTS_PAGE);
            complete(&mut app);
            assert_eq!(app.account.channels.len(), 1);
            assert!(app.account.rows.is_empty());
            assert_eq!(app.text(PAGE_TITLE), "余额与充值");
            assert!(app.account.visible(PAY.hwnd(&app)));
            let operation = app.account_operation("payment:tron:10".into()).unwrap();
            assert_eq!(
                operation,
                app.account_operation("payment:tron:10".into()).unwrap()
            );
            app.account_action(PAY).unwrap();
            complete(&mut app);
            assert_eq!(
                payment_calls.lock().unwrap().len(),
                1,
                "failed create must not auto-replay"
            );
            app.account_load(0, String::new()).unwrap();
            complete(&mut app);
            assert_eq!(
                operation,
                app.account_operation("payment:tron:10".into()).unwrap()
            );
            app.account_action(PAY).unwrap();
            complete(&mut app);
            assert_eq!(
                *payment_calls.lock().unwrap(),
                vec![operation.clone(), operation]
            );
            assert_eq!(app.account.rows.len(), 1);
            assert!(app.text(DETAILS).contains("10.000123"));
            assert_eq!(app.account.copied.as_str(), "fixture-address");
            app.select_page(ACCOUNT_PAGE);
            complete(&mut app);
            assert!(app.account.visible(CURRENT.hwnd(&app)));
            assert!(!app.account.visible(TABLE.hwnd(&app)));
            assert!(!app.account.visible(PAY.hwnd(&app)));
            assert_eq!(app.text(PAGE_TITLE), "我的账户");
            app.select_page(PAYMENTS_PAGE);
            complete(&mut app);
            app.account_action(JOURNAL_TAB).unwrap();
            complete(&mut app);
            assert_eq!(app.account.panel, Panel::Journal);
            assert_eq!(app.account.rows[0]["amount"], "20000000");
            assert!(!app.account.visible(PAY.hwnd(&app)));
            assert!(app.account.visible(BALANCE.hwnd(&app)));
            app.account_action(PAYMENT_TAB).unwrap();
            complete(&mut app);
            assert!(app.account.visible(PAY.hwnd(&app)));
            app.select_page(FINANCE_PAGE);
            complete(&mut app);
            let selection = LVITEMW {
                stateMask: LVIS_SELECTED,
                state: LVIS_SELECTED,
                ..mem::zeroed()
            };
            SendMessageW(
                TABLE.hwnd(&app),
                LVM_SETITEMSTATE,
                0,
                (&selection as *const LVITEMW) as isize,
            );
            app.account_action(DETAIL).unwrap();
            complete(&mut app);
            assert!(app.text(DETAILS).contains("signed-fixture"));
            let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel();
            app.shutdown = Some(shutdown);
            app.task = Some(app.runtime.spawn(std::future::pending::<()>()));
            app.running = true;
            app.account_action(LOGOUT).unwrap();
            assert!(app.stopping);
            assert!(shutdown_rx.try_recv().is_ok());
            assert!(app.router_key.is_empty());
            assert!(app.account.rows.is_empty());
            complete(&mut app);
            assert!(!app.account.logged_in());
            assert!(app.account.rows.is_empty());
            assert!(app.account.channels.is_empty());
            assert!(app.text(SUMMARY).is_empty());
            assert!(app.text(DETAILS).is_empty());
            assert!(app.account.visible(USERNAME.hwnd(&app)));
            assert!(!app.account.visible(TABLE.hwnd(&app)));
            app.select_page(FINANCE_PAGE);
            assert!(app.account.visible(GO_LOGIN.hwnd(&app)));
            assert!(!app.account.visible(USERNAME.hwnd(&app)));
            app.account_action(GO_LOGIN).unwrap();
            assert_eq!(app.page, ACCOUNT_PAGE);
            assert!(app.account.visible(USERNAME.hwnd(&app)));
            assert!(app.router_key.is_empty());
            assert!(identity::load(&root).unwrap().is_none());
            assert_eq!(app.text(CLIENT_KEY), "local-api-key-never-uploaded");
            assert!(!app.catalog_loaded);
            assert!(app.catalog.pools.is_empty());
            app.task.take().unwrap().abort();
            server.abort();
            DestroyWindow(app.hwnd);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn money_keeps_micro_precision_and_negative_balances() {
        assert_eq!(money(&json!("1000001")), "$1.000001");
        assert_eq!(money(&json!("-1")), "$-0.000001");
        assert_eq!(money(&Value::Null), "—");
    }
    #[test]
    fn logout_clears_all_private_account_views() {
        let mut state = State {
            me: json!({"account":{"role":"user"}}),
            view: json!({"balance":100}),
            rows: vec![json!({"private":true})],
            copied: Zeroizing::new("secret".into()),
            ..State::default()
        };
        state.clear();
        assert!(!state.logged_in());
        assert!(state.view.is_null());
        assert!(state.rows.is_empty());
        assert!(state.copied.is_empty());
    }
}
