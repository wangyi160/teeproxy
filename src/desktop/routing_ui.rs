#![allow(unsafe_op_in_unsafe_fn)]
use super::*;
use crate::routing::{Policy, Priority};

pub(super) const ROUTE_MODE: u16 = 601;
pub(super) const ROUTE_POOL: u16 = 602;
pub(super) const ROUTE_PROVIDER: u16 = 603;
pub(super) const ROUTE_MODEL: u16 = 604;
pub(super) const ROUTE_MAX: u16 = 605;
pub(super) const ROUTE_PRIORITY: u16 = 606;
pub(super) const ROUTE_SAVE: u16 = 607;
pub(super) const ROUTE_COPY: u16 = 608;
pub(super) const ROUTE_CURRENT: u16 = 609;
pub(super) const ROUTE_OVERVIEW: u16 = 610;
const PROVIDERS: [&str; 5] = ["", "openai", "claude", "grok", "workbuddy"];
const FOLLOW_MODEL: &str = "跟随 Agent 请求";

impl App {
    unsafe fn route_combo(
        &mut self,
        id: u16,
        rect: [i32; 4],
        values: &[&str],
        index: usize,
        editable: bool,
    ) -> Result<()> {
        let hwnd = self.add(
            id,
            w!("COMBOBOX"),
            "",
            WS_TABSTOP
                | WS_VSCROLL
                | if editable {
                    CBS_DROPDOWN as u32
                } else {
                    CBS_DROPDOWNLIST as u32
                },
            rect,
            0,
        )?;
        for text in values {
            SendMessageW(hwnd, CB_ADDSTRING, 0, wide(text).as_ptr() as isize);
        }
        SendMessageW(hwnd, CB_SETCURSEL, index, 0);
        Ok(())
    }
    pub(super) unsafe fn build_routing(&mut self, profile: &Profile) -> Result<()> {
        self.building_page = Some(ROUTE_PAGE);
        self.label("选择使用方式", [268, 150, -316, 26], 3)?;
        self.label("路由模式", [268, 204, 130, 24], 0)?;
        self.route_combo(
            ROUTE_MODE,
            [412, 198, -472, 160],
            &["指定账号池", "自动匹配账号池"],
            profile.routing.automatic as usize,
            false,
        )?;
        self.label("指定账号池", [268, 258, 130, 24], 0)?;
        self.route_combo(ROUTE_POOL, [412, 252, -472, 240], &[], 0, false)?;
        SendMessageW(
            self.by_id[&ROUTE_POOL],
            CB_SETCUEBANNER,
            0,
            wide("刷新列表后选择账号池").as_ptr() as isize,
        );
        self.label("模型大类", [268, 312, 130, 24], 0)?;
        self.route_combo(
            ROUTE_PROVIDER,
            [412, 306, -472, 220],
            &[
                "全部官方服务",
                "OpenAI",
                "Claude",
                "Grok",
                "Workbuddy（测试）",
            ],
            PROVIDERS
                .iter()
                .position(|p| *p == profile.routing.provider)
                .unwrap_or(0),
            false,
        )?;
        self.label("模型小类", [268, 366, 130, 24], 0)?;
        self.route_combo(ROUTE_MODEL, [412, 360, -472, 240], &[FOLLOW_MODEL], 0, true)?;
        if !profile.routing.model.is_empty() {
            self.set(ROUTE_MODEL, &profile.routing.model);
        }
        self.label("倍率上限", [268, 420, 130, 24], 0)?;
        self.edit(
            ROUTE_MAX,
            &crate::routing::format_multiplier(profile.routing.max_multiplier_millionths),
            [412, 408, 150, 38],
            0,
            "例如 0.5",
        )?;
        self.label("× 官方价格", [580, 420, 200, 24], 2)?;
        self.label("匹配策略", [268, 474, 130, 24], 0)?;
        self.route_combo(
            ROUTE_PRIORITY,
            [412, 468, -472, 160],
            &["费率优先", "速率优先（实测）"],
            (profile.routing.priority == Priority::Speed) as usize,
            false,
        )?;
        self.label(
            "指定模型时，Agent 也需使用相同模型 ID；留空则跟随请求。",
            [268, 526, -316, 24],
            2,
        )?;
        self.label(
            "速率按真实流式响应测速；无样本时轮流积累，不额外发送测试请求。",
            [268, 556, -316, 24],
            2,
        )?;
        self.label(
            "无符合条件的池会报错；保存后下次启动生效。",
            [268, 586, -316, 24],
            2,
        )?;
        self.button(ROUTE_SAVE, "保存路由设置", [244, 638, 156, 38], 0)?;
        self.button(ROUTE_COPY, "复制指定模型", [416, 638, 156, 38], 0)?;
        self.add(
            ROUTE_CURRENT,
            w!("STATIC"),
            "",
            SS_LEFT,
            [260, 690, -312, 26],
            2,
        )?;
        self.refresh_route_pools();
        self.update_route_controls(false);
        Ok(())
    }
    pub(super) unsafe fn routing_policy(&self) -> Result<Policy> {
        let model = self.text(ROUTE_MODEL);
        let provider = SendMessageW(self.by_id[&ROUTE_PROVIDER], CB_GETCURSEL, 0, 0);
        let policy = Policy {
            automatic: SendMessageW(self.by_id[&ROUTE_MODE], CB_GETCURSEL, 0, 0) == 1,
            provider: PROVIDERS.get(provider as usize).unwrap_or(&"").to_string(),
            model: if model == FOLLOW_MODEL {
                String::new()
            } else {
                model.trim().to_owned()
            },
            max_multiplier_millionths: crate::routing::parse_multiplier(&self.text(ROUTE_MAX))?,
            priority: if SendMessageW(self.by_id[&ROUTE_PRIORITY], CB_GETCURSEL, 0, 0) == 1 {
                Priority::Speed
            } else {
                Priority::Price
            },
        };
        policy.validate()?;
        Ok(policy)
    }
    pub(super) unsafe fn update_route_controls(&self, busy: bool) {
        let automatic = SendMessageW(self.by_id[&ROUTE_MODE], CB_GETCURSEL, 0, 0) == 1;
        for id in [ROUTE_PROVIDER, ROUTE_MODEL, ROUTE_MAX, ROUTE_PRIORITY] {
            EnableWindow(self.by_id[&id], (!busy && automatic) as i32);
        }
        EnableWindow(self.by_id[&ROUTE_MODE], (!busy) as i32);
        EnableWindow(
            self.by_id[&ROUTE_POOL],
            (!busy && !automatic && !self.catalog.pools.is_empty()) as i32,
        );
        EnableWindow(self.by_id[&ROUTE_SAVE], (!busy) as i32);
    }
    pub(super) unsafe fn refresh_route_pools(&mut self) {
        if !self.by_id.contains_key(&ROUTE_POOL) {
            return;
        }
        let configured = self.text(POOL);
        let hwnd = self.by_id[&ROUTE_POOL];
        SendMessageW(hwnd, CB_RESETCONTENT, 0, 0);
        for pool in &self.catalog.pools {
            SendMessageW(hwnd, CB_ADDSTRING, 0, wide(&pool.label).as_ptr() as isize);
        }
        SendMessageW(
            hwnd,
            CB_SETCURSEL,
            self.catalog
                .pools
                .iter()
                .position(|p| p.id == configured)
                .unwrap_or(usize::MAX),
            0,
        );
        self.refresh_route_models();
        self.update_route_controls(self.task.is_some() || self.inventory_task.is_some());
        self.update_route_summary();
    }
    unsafe fn refresh_route_models(&self) {
        let current = self.text(ROUTE_MODEL);
        let provider = SendMessageW(self.by_id[&ROUTE_PROVIDER], CB_GETCURSEL, 0, 0);
        let matches = |name: &str| match provider {
            0 => ["OpenAI", "Claude", "Grok"].contains(&name),
            1 => name == "OpenAI",
            2 => name == "Claude",
            3 => name == "Grok",
            4 => name == "Workbuddy（测试）",
            _ => false,
        };
        let models: std::collections::BTreeSet<_> = self
            .catalog
            .models
            .iter()
            .filter(|m| matches(&m.provider))
            .map(|m| m.id.as_str())
            .collect();
        let hwnd = self.by_id[&ROUTE_MODEL];
        SendMessageW(hwnd, CB_RESETCONTENT, 0, 0);
        SendMessageW(hwnd, CB_ADDSTRING, 0, wide(FOLLOW_MODEL).as_ptr() as isize);
        for model in models {
            SendMessageW(hwnd, CB_ADDSTRING, 0, wide(model).as_ptr() as isize);
        }
        self.set(
            ROUTE_MODEL,
            if current.is_empty() {
                FOLLOW_MODEL
            } else {
                &current
            },
        );
    }
    pub(super) unsafe fn update_route_summary(&self) {
        if !self.by_id.contains_key(&ROUTE_MODE) {
            return;
        }
        let automatic = SendMessageW(self.by_id[&ROUTE_MODE], CB_GETCURSEL, 0, 0) == 1;
        let summary = if automatic {
            format!(
                "自动匹配 · {} · {} · ≤ {}×",
                self.text(ROUTE_PROVIDER),
                self.text(ROUTE_PRIORITY),
                self.text(ROUTE_MAX)
            )
        } else {
            let id = self.text(POOL);
            format!(
                "指定账号池 · {}",
                self.catalog
                    .pools
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.label.as_str())
                    .unwrap_or(if id.is_empty() { "尚未选择" } else { &id })
            )
        };
        self.set(ROUTE_OVERVIEW, &summary);
        if !self.running {
            self.set(ROUTE_CURRENT, &format!("下次启动：{summary}"));
        } else if !automatic {
            self.set(ROUTE_CURRENT, &format!("运行中：{summary}"));
        }
    }
    pub(super) unsafe fn routing_action(&mut self, id: u16) -> Result<()> {
        match id {
            ROUTE_COPY => {
                let model = self.routing_policy()?.model;
                ensure!(
                    !model.is_empty(),
                    "跟随请求模式无需复制；请在 Agent 中指定模型，或先选择具体模型"
                );
                copy_text(self.hwnd, &model)?;
                self.append(&format!("已复制模型 {model}。"));
            }
            ROUTE_SAVE => self.action(SAVE)?,
            ROUTE_MODE | ROUTE_POOL | ROUTE_PROVIDER | ROUTE_PRIORITY | ROUTE_MODEL => {
                ensure!(
                    self.task.is_none() && self.inventory_task.is_none(),
                    "请先停止代理再修改路由"
                );
                if id == ROUTE_POOL {
                    let index = SendMessageW(self.by_id[&ROUTE_POOL], CB_GETCURSEL, 0, 0);
                    if let Some(pool) = self.catalog.pools.get(index as usize) {
                        self.set(POOL, &pool.id);
                    }
                }
                if id == ROUTE_PROVIDER {
                    self.set(ROUTE_MODEL, FOLLOW_MODEL);
                    self.refresh_route_models();
                }
                self.update_route_controls(false);
                self.update_route_summary();
            }
            _ => (),
        }
        Ok(())
    }
}
