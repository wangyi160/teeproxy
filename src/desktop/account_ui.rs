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
const RELOAD: u16 = 1520;
const SUMMARY: u16 = 1521;
pub(super) const TABLE: u16 = 1522;
const DETAILS: u16 = 1523;
const FEEDBACK: u16 = 1524;
const LABEL: u16 = 1525;
const CREATE_KEY: u16 = 1526;
const APPLY_KEY: u16 = 1527;
const REVOKE_KEY: u16 = 1528;
const NETWORK: u16 = 1529;
const AMOUNT: u16 = 1530;
const PAY: u16 = 1531;
const DETAIL: u16 = 1532;
const COPY: u16 = 1533;
const PREV: u16 = 1534;
const NEXT: u16 = 1535;
const ACCESS: u16 = 1536;
const CURRENT: u16 = 1537;
const NEW_PASSWORD: u16 = 1538;
const CHANGE_PASSWORD: u16 = 1539;
const COPY_AMOUNT: u16 = 1540;
const PAGE: u16 = 1541;
const BALANCE: u16 = 1542;
const GO_LOGIN: u16 = 1543;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Panel {
    #[default]
    Overview,
    Keys,
    Payments,
    Finance,
    Receipts,
    Access,
    Security,
    Journal,
}
impl Panel {
    fn for_page(page: usize) -> Self {
        match page {
            KEYS_PAGE => Self::Keys,
            PAYMENTS_PAGE => Self::Payments,
            FINANCE_PAGE => Self::Finance,
            RECEIPTS_PAGE => Self::Receipts,
            ACCESS_PAGE => Self::Access,
            SECURITY_PAGE => Self::Security,
            JOURNAL_PAGE => Self::Journal,
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
            Self::Receipts => {
                if cursor.is_empty() {
                    "receipts/mine".into()
                } else {
                    format!("receipts/mine?before={cursor}")
                }
            }
            _ => "me".into(),
        }
    }
    fn has_table(self) -> bool {
        !matches!(self, Self::Overview | Self::Security)
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
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Load,
    Login,
    Register,
    Logout,
    Password,
    NewKey,
    Apply,
    Revoke,
    Payment,
    CheckPayment,
    Receipt,
    Access,
}
pub(super) struct Reply {
    action: Action,
    panel: Panel,
    page: usize,
    cursor: String,
    pub result: std::result::Result<Value, Failure>,
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
        }
    }
}
impl State {
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
        let platform = std::fs::read(self.path.parent().unwrap().join("account-platform.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<String>(&b).ok())
            .unwrap_or_else(|| "https://teerouter.org".into());
        self.label("管理平台", [268, 148, 150, 24], 0)?;
        self.edit(PLATFORM, &platform, [268, 178, 460, 40], 0, "")?;
        self.label("用户名", [268, 238, 150, 24], 0)?;
        self.edit(USERNAME, "", [268, 268, 460, 40], 0, "")?;
        self.label("密码", [268, 328, 150, 24], 0)?;
        self.edit(PASSWORD, "", [268, 358, 460, 40], ES_PASSWORD as u32, "")?;
        self.button(SUBMIT, "登录", [268, 530, 160, 40], 0)?;
        self.button(SWITCH, "注册新账号", [448, 530, 180, 40], 0)?;
        self.account_group(start, Group::Guest);
        let start = self.controls.len();
        self.label("名称", [268, 418, 150, 24], 0)?;
        self.edit(NAME, "", [268, 448, 460, 40], 0, "")?;
        self.account_group(start, Group::Register);
        let start = self.controls.len();
        self.add(IDENTITY, w!("STATIC"), "", SS_LEFT, [260, 134, -590, 28], 3)?;
        self.button(RELOAD, "刷新", [-240, 128, 88, 36], 0)?;
        self.button(LOGOUT, "退出登录", [-140, 128, 104, 36], 0)?;
        self.account_group(start, Group::Member);
        let start = self.controls.len();
        self.label("请先登录", [268, 210, 300, 28], 3)?;
        self.button(GO_LOGIN, "前往登录", [268, 264, 150, 40], 0)?;
        self.account_group(start, Group::Locked);
        let start = self.controls.len();
        self.add(
            SUMMARY,
            w!("EDIT"),
            "",
            ES_MULTILINE as u32 | ES_READONLY as u32,
            [268, 250, -328, 300],
            3,
        )?;
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
            .rect = [260, 290, -312, -555];
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
        self.edit(LABEL, "", [260, 238, 210, 38], 0, "API key 名称")?;
        self.button(CREATE_KEY, "创建", [482, 238, 90, 38], 0)?;
        self.button(APPLY_KEY, "应用 API key", [584, 238, 150, 38], 0)?;
        self.button(REVOKE_KEY, "停用", [746, 238, 90, 38], 0)?;
        self.account_group(start, Group::Panel(Panel::Keys));
        let start = self.controls.len();
        self.add(BALANCE, w!("STATIC"), "", SS_LEFT, [260, 184, -590, 28], 3)?;
        self.add(
            NETWORK,
            w!("COMBOBOX"),
            "",
            WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
            [260, 238, 180, 250],
            0,
        )?;
        self.edit(AMOUNT, "10", [452, 232, 150, 38], 0, "USDT 金额")?;
        self.button(PAY, "创建充值订单", [614, 232, 150, 38], 0)?;
        self.button(COPY_AMOUNT, "复制付款金额", [492, -122, 150, 34], 0)?;
        self.account_group(start, Group::Panel(Panel::Payments));
        let start = self.controls.len();
        self.button(ACCESS, "申请使用所选商户", [260, 238, 190, 38], 0)?;
        self.account_group(start, Group::Panel(Panel::Access));
        let start = self.controls.len();
        self.label("当前密码", [268, 250, 160, 24], 0)?;
        self.edit(CURRENT, "", [268, 282, 460, 40], ES_PASSWORD as u32, "")?;
        self.label("新密码", [268, 350, 160, 24], 0)?;
        self.edit(
            NEW_PASSWORD,
            "",
            [268, 382, 460, 40],
            ES_PASSWORD as u32,
            "",
        )?;
        self.button(CHANGE_PASSWORD, "修改密码", [268, 460, 160, 40], 0)?;
        self.account_group(start, Group::Panel(Panel::Security));
        for (id, max) in [
            (USERNAME, 64),
            (PASSWORD, 512),
            (NAME, 128),
            (CURRENT, 512),
            (NEW_PASSWORD, 512),
            (LABEL, 128),
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
        self.account.panel = Panel::for_page(self.page);
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
                client.load(&route, panel == Panel::Keys).await
            } else {
                client.request(&method, &route, body, &operation).await
            };
            let _ = sender.send(Event::Account(Reply {
                action,
                panel,
                page,
                cursor,
                result,
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
            RELOAD => {
                return self.account_load(
                    self.account.page,
                    self.account.cursors[self.account.page].clone(),
                );
            }
            LOGOUT => {
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
            CREATE_KEY => {
                let label = self.text(LABEL).trim().to_owned();
                ensure!(!label.is_empty(), "请填写 API key 名称");
                operation = self.account_operation(format!("key:{label}"))?;
                action = Action::NewKey;
                method = "POST";
                route = "keys".into();
                body = json!({"label":label});
            }
            APPLY_KEY | REVOKE_KEY => {
                let row = self.chosen_account_row()?;
                let key = text(&row["id"]);
                if id == APPLY_KEY {
                    ensure!(row["state"] == "enabled", "请等待 API key 启用后再应用");
                    ensure!(
                        self.task.is_none() && self.inventory_task.is_none(),
                        "请先停止代理，再切换业务 API key"
                    );
                    action = Action::Apply;
                    method = "POST";
                    route = format!("keys/{key}/reveal");
                } else {
                    if MessageBoxW(
                        self.hwnd,
                        wide("停用后，该 API key 将无法继续请求模型。确认停用？").as_ptr(),
                        wide("停用 API key").as_ptr(),
                        MB_YESNO | MB_ICONQUESTION,
                    ) != IDYES
                    {
                        return Ok(());
                    }
                    action = Action::Revoke;
                    method = "DELETE";
                    route = format!("keys/{key}");
                }
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
            ACCESS => {
                let row = self.chosen_account_row()?;
                ensure!(row["authorized"] != true, "已经获得该商户授权");
                action = Action::Access;
                method = "POST";
                route = "access".into();
                body = json!({"merchant_id":row["id"]});
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
                    Panel::Receipts => {
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
            Panel::Keys => format!(
                "名称：{}\r\n状态：{}\r\nAPI key：{}\r\n创建时间：{}",
                text(&row["label"]),
                status(&row["state"]),
                text(
                    row.get("key")
                        .filter(|v| !v.is_null())
                        .unwrap_or(&row["key_error"])
                ),
                date(&row["created_at"])
            ),
            Panel::Access => format!(
                "商户：{}\r\n状态：{}",
                text(&row["name"]),
                if row["authorized"] == true {
                    "已授权"
                } else {
                    "未授权"
                }
            ),
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
        self.account.copied = Zeroizing::new(if self.account.panel == Panel::Keys {
            row["key"].as_str().unwrap_or("").to_owned()
        } else {
            detail
        });
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
            let apps = self.account.me["applications"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let application = apps.iter().find(|a| a["kind"] == "user");
            self.set(BALANCE, &format!("可用余额　{}", money(&available)));
            self.set(
                SUMMARY,
                &format!(
                    "名称　{}\r\n\r\n开通状态　{}\r\n\r\n管理平台　{}",
                    text(&self.account.me["account"]["name"]),
                    application
                        .map(|a| status(&a["status"]))
                        .unwrap_or_else(|| "—".into()),
                    self.account.base,
                ),
            );
        } else {
            self.set(SUMMARY, "");
            self.set(BALANCE, "");
        }
        let panel = self.account.panel;
        let source = if !logged {
            Value::Null
        } else {
            match panel {
                Panel::Access => self.account.me["merchants"].clone(),
                Panel::Journal => self.account.view["journal"]["data"].clone(),
                _ => self.account.view["data"].clone(),
            }
        };
        self.account.rows = source.as_array().cloned().unwrap_or_default();
        let headings = match panel {
            Panel::Keys => ["名称", "API key", "状态", "创建时间", ""],
            Panel::Payments => ["时间", "网络", "付款金额 USDT", "状态", "订单"],
            Panel::Access => ["商户", "状态", "", "", ""],
            Panel::Journal => ["时间", "类型", "金额 USD", "编号", ""],
            _ => ["时间", "模型 / 账号池", "金额 USD", "状态", "流水"],
        };
        for (i, label) in headings.iter().enumerate() {
            let mut label = wide(label);
            let column = LVCOLUMNW {
                mask: LVCF_TEXT | LVCF_WIDTH,
                cx: self.scale(if panel == Panel::Keys {
                    [120, 180, 80, 150, 0][i]
                } else {
                    [180, 180, 150, 125, 120][i]
                }),
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
                Panel::Keys => vec![
                    text(&row["label"]),
                    text(
                        row.get("key")
                            .filter(|v| !v.is_null())
                            .unwrap_or(&row["key_error"]),
                    ),
                    status(&row["state"]),
                    date(&row["created_at"]),
                    String::new(),
                ],
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
                Panel::Access => vec![
                    text(&row["name"]),
                    if row["authorized"] == true {
                        "已授权".into()
                    } else if self.account.me["applications"].as_array().is_some_and(|a| {
                        a.iter().any(|a| {
                            a["kind"] == "access"
                                && a["subject"] == row["id"]
                                && a["status"] == "pending"
                        })
                    }) {
                        "待审核".into()
                    } else {
                        "未授权".into()
                    },
                    String::new(),
                    String::new(),
                    String::new(),
                ],
                Panel::Journal => vec![
                    date(&row["created_at"]),
                    if row["kind"] == "credit" {
                        "充值".into()
                    } else {
                        "退款 / 调整".into()
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
        if panel == Panel::Keys {
            SendMessageW(
                TABLE.hwnd(self),
                LVM_SETCOLUMNWIDTH,
                1,
                LVSCW_AUTOSIZE as isize,
            );
            let width = SendMessageW(TABLE.hwnd(self), LVM_GETCOLUMNWIDTH, 1, 0);
            if width < self.scale(180) as isize {
                SendMessageW(
                    TABLE.hwnd(self),
                    LVM_SETCOLUMNWIDTH,
                    1,
                    self.scale(180) as isize,
                );
            }
        }
        self.set(
            PAGE,
            &format!("第 {} 页 · {} 条", self.account.page + 1, rows.len()),
        );
        self.set(
            DETAIL,
            if panel == Panel::Payments {
                "刷新订单"
            } else {
                "查看"
            },
        );
        self.set(
            COPY,
            if panel == Panel::Payments {
                "复制地址"
            } else if panel == Panel::Keys {
                "复制 API key"
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
        self.layout();
    }
    pub(super) unsafe fn account_reply(&mut self, reply: Reply) {
        self.account.task.take();
        self.account.refreshed = std::time::Instant::now();
        let action = reply.action;
        let value = match reply.result {
            Ok(value) => value,
            Err(error) => {
                if error.status == 401 || action == Action::Logout {
                    self.account.clear();
                    self.account.client = None;
                    self.render_account();
                }
                self.set(FEEDBACK, &error.message);
                self.account_busy();
                self.finish_account_refresh();
                return;
            }
        };
        if matches!(action, Action::NewKey | Action::Payment) {
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
            }
            self.render_account();
            self.finish_account_refresh();
            return;
        }
        let mut followup = false;
        match action {
            Action::Login | Action::Register => {
                self.account.me = value;
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
                        "已登录"
                    } else {
                        "已登录，平台地址未能保存"
                    },
                );
            }
            Action::Load => {
                let payment_id = self.account.detail["id"].clone();
                self.account.me = value["me"].clone();
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
            Action::Apply => {
                let mut value = value;
                let key = Zeroizing::new(match value["key"].take() {
                    Value::String(key) => key,
                    _ => String::new(),
                });
                let result = self.apply_account_key(&key);
                self.set(
                    FEEDBACK,
                    &match result {
                        Ok(()) => "API key 已应用并保存".into(),
                        Err(e) => e.to_string(),
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
            _ => {
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
    unsafe fn apply_account_key(&mut self, key: &str) -> Result<()> {
        ensure!(!key.is_empty(), "平台返回的 API key 无效");
        ensure!(
            self.task.is_none() && self.inventory_task.is_none(),
            "请先停止代理，再应用 API key"
        );
        let (mut profile, mut secrets) = settings::load(&self.path)?;
        if profile.router.is_empty() {
            profile.router = "https://router.teerouter.org".into();
        }
        secrets.router_key = key.to_owned();
        settings::save(&self.path, &profile, &secrets)?;
        self.set(ROUTER_KEY, key);
        if self.text(ROUTER).is_empty() {
            self.set(ROUTER, &profile.router);
        }
        self.invalidate_catalog();
        self.invalidate_prices();
        Ok(())
    }
    pub(super) unsafe fn account_poll(&mut self) {
        if is_account_page(self.page)
            && IsWindowVisible(self.hwnd) != 0
            && self.account.logged_in()
            && self.account.task.is_none()
            && self.account.refreshed.elapsed().as_secs()
                >= if self.account.panel == Panel::Payments {
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
            let view = json!({"account":{"role":"user","username":"fixture-user","name":"Fixture"},"balance":{"ceiling_microusd":20000000,"spent_microusd":500001},"keys":[{"id":"k_fixture","label":"Laptop","prefix":"fixture","state":"enabled"}],"applications":[],"merchants":[]});
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
                .route("/portal/api/keys/k_fixture/reveal",post(|headers:HeaderMap|async move {
                    assert_eq!(headers["cookie"],"teerouter_portal=fixture");
                    assert_eq!(headers["x-csrf-token"],"fixture-csrf");
                    Json(json!({"key":"fixture-full-api-key-with-all-characters"}))
                }))
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
            app.select_page(KEYS_PAGE);
            complete(&mut app);
            assert_eq!(app.account.rows.len(), 1);
            assert_eq!(app.account.rows[0]["label"], "Laptop");
            assert_eq!(
                app.account.rows[0]["key"],
                "fixture-full-api-key-with-all-characters"
            );
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
            app.account_selection_changed();
            assert!(
                app.text(DETAILS)
                    .contains("API key：fixture-full-api-key-with-all-characters")
            );
            assert_eq!(
                app.account.copied.as_str(),
                "fixture-full-api-key-with-all-characters"
            );
            assert_eq!(app.text(COPY), "复制 API key");
            assert!(!app.account.visible(PAY.hwnd(&app)));
            // Switch before the previous fetch completes. Its key list must not
            // be rendered under the payments heading when the response arrives.
            app.select_page(KEYS_PAGE);
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
            app.select_page(SECURITY_PAGE);
            complete(&mut app);
            assert!(app.account.visible(CURRENT.hwnd(&app)));
            assert!(!app.account.visible(TABLE.hwnd(&app)));
            assert!(!app.account.visible(PAY.hwnd(&app)));
            assert_eq!(app.text(PAGE_TITLE), "账号安全");
            app.account_action(LOGOUT).unwrap();
            complete(&mut app);
            assert!(!app.account.logged_in());
            assert!(app.account.rows.is_empty());
            assert!(app.account.channels.is_empty());
            assert!(app.text(SUMMARY).is_empty());
            assert!(app.text(DETAILS).is_empty());
            assert!(app.account.visible(USERNAME.hwnd(&app)));
            assert!(!app.account.visible(TABLE.hwnd(&app)));
            app.select_page(RECEIPTS_PAGE);
            assert!(app.account.visible(GO_LOGIN.hwnd(&app)));
            assert!(!app.account.visible(USERNAME.hwnd(&app)));
            app.account_action(GO_LOGIN).unwrap();
            assert_eq!(app.page, ACCOUNT_PAGE);
            assert!(app.account.visible(USERNAME.hwnd(&app)));
            server.abort();
            DestroyWindow(app.hwnd);
            std::fs::remove_file(root.join("account-platform.json")).unwrap();
            std::fs::remove_dir(root).unwrap();
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
