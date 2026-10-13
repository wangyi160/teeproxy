#![allow(unsafe_op_in_unsafe_fn)]
use super::*;
use crate::routing::{Policy, Priority};

pub(super) const ROUTE_MODE: u16 = 601;
pub(super) const ROUTE_POOL: u16 = 602;
pub(super) const ROUTE_MAX: u16 = 605;
pub(super) const ROUTE_PRIORITY: u16 = 606;
pub(super) const ROUTE_SAVE: u16 = 607;
pub(super) const ROUTE_CURRENT: u16 = 609;
pub(super) const ROUTE_OVERVIEW: u16 = 610;

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
        self.label("倍率上限", [268, 312, 130, 24], 0)?;
        self.edit(
            ROUTE_MAX,
            &crate::routing::format_multiplier(profile.routing.max_multiplier_millionths),
            [412, 300, 150, 38],
            0,
            "例如 0.5",
        )?;
        self.label("× 官方价格", [580, 312, 200, 24], 2)?;
        self.label("匹配策略", [268, 366, 130, 24], 0)?;
        self.route_combo(
            ROUTE_PRIORITY,
            [412, 360, -472, 160],
            &["费率优先", "速率优先（实测）"],
            (profile.routing.priority == Priority::Speed) as usize,
            false,
        )?;
        self.label(
            "规则随请求发送，由 Router 匹配 Agent 的模型并检查倍率上限。",
            [268, 418, -316, 24],
            2,
        )?;
        self.label(
            "速率由 Router 测量首字时间和生成速度；无样本时从实际请求积累。",
            [268, 448, -316, 24],
            2,
        )?;
        self.label(
            "保存后新请求立即生效；正在进行的请求继续使用原规则。",
            [268, 478, -316, 24],
            2,
        )?;
        self.button(ROUTE_SAVE, "保存路由设置", [244, 530, 156, 38], 0)?;
        self.add(
            ROUTE_CURRENT,
            w!("STATIC"),
            "",
            SS_LEFT,
            [260, 582, -312, 26],
            2,
        )?;
        self.refresh_route_pools();
        self.update_route_controls(false);
        Ok(())
    }
    pub(super) unsafe fn routing_policy(&self) -> Result<Policy> {
        let policy = Policy {
            automatic: SendMessageW(self.by_id[&ROUTE_MODE], CB_GETCURSEL, 0, 0) == 1,
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
        let busy = self.stopping || (busy && !self.running);
        let automatic = SendMessageW(self.by_id[&ROUTE_MODE], CB_GETCURSEL, 0, 0) == 1;
        EnableWindow(self.by_id[&ROUTE_PRIORITY], (!busy && automatic) as i32);
        EnableWindow(self.by_id[&ROUTE_MODE], (!busy) as i32);
        EnableWindow(self.by_id[&ROUTE_MAX], (!busy) as i32);
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
        self.update_route_controls(self.task.is_some() || self.inventory_task.is_some());
        self.update_route_summary();
    }
    pub(super) unsafe fn update_route_summary(&self) {
        if !self.by_id.contains_key(&ROUTE_MODE) {
            return;
        }
        let automatic = SendMessageW(self.by_id[&ROUTE_MODE], CB_GETCURSEL, 0, 0) == 1;
        let summary = if automatic {
            format!(
                "自动匹配 · {} · ≤ {}×",
                self.text(ROUTE_PRIORITY),
                self.text(ROUTE_MAX)
            )
        } else {
            let id = self.text(POOL);
            format!(
                "指定账号池 · {} · ≤ {}×",
                self.catalog
                    .pools
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.label.as_str())
                    .unwrap_or(if id.is_empty() { "尚未选择" } else { &id }),
                self.text(ROUTE_MAX)
            )
        };
        if let Some(proxy) = &self.live_proxy {
            let active = proxy.routing_rules();
            let active_summary = format!(
                "{} · ≤ {}×",
                if active.policy.automatic {
                    format!(
                        "自动匹配 · {}",
                        if active.policy.priority == Priority::Speed {
                            "速率优先（实测）"
                        } else {
                            "费率优先"
                        }
                    )
                } else {
                    let id = active.pool.as_deref().unwrap_or("尚未选择");
                    format!(
                        "指定账号池 · {}",
                        self.catalog
                            .pools
                            .iter()
                            .find(|p| p.id == id)
                            .map(|p| p.label.as_str())
                            .unwrap_or(id)
                    )
                },
                crate::routing::format_multiplier(active.policy.max_multiplier_millionths)
            );
            self.set(ROUTE_OVERVIEW, &active_summary);
            self.set(
                ROUTE_CURRENT,
                &if self.route_matches_active() {
                    format!("已生效：{active_summary}")
                } else {
                    format!("未保存：{summary}")
                },
            );
        } else {
            self.set(ROUTE_OVERVIEW, &summary);
            self.set(ROUTE_CURRENT, &format!("下次启动：{summary}"));
        }
    }
    pub(super) unsafe fn route_matches_active(&self) -> bool {
        self.live_proxy.as_ref().is_some_and(|proxy| {
            let active = proxy.routing_rules();
            self.routing_policy()
                .is_ok_and(|draft| draft == active.policy)
                && (active.policy.automatic
                    || self.text(POOL).trim() == active.pool.as_deref().unwrap_or(""))
        })
    }
    pub(super) unsafe fn save_profile(
        &mut self,
        profile: &Profile,
        secrets: &Secrets,
    ) -> Result<()> {
        ensure!(
            !self.stopping && (self.task.is_none() || self.running),
            "请等待当前操作完成"
        );
        let config = profile.config(secrets, self.path.parent().unwrap())?;
        let rules = crate::routing::Rules::new(config.routing, config.pool)?;
        if self.running {
            ensure!(self.live_proxy.is_some(), "代理尚未就绪");
            ensure!(
                rules.policy.automatic || rules.pool.is_some(),
                "请选择账号池"
            );
        }
        // Persist first: invalid input or a failed disk write never changes live rules.
        settings::save(&self.path, profile, secrets)?;
        if let Some(proxy) = &self.live_proxy {
            proxy.replace_routing(rules);
            self.append("路由规则已生效：新请求使用新规则，正在进行的请求继续使用原规则。");
        }
        Ok(())
    }
    pub(super) unsafe fn routing_action(&mut self, id: u16) -> Result<()> {
        match id {
            ROUTE_SAVE => self.action(SAVE)?,
            ROUTE_MODE | ROUTE_POOL | ROUTE_PRIORITY => {
                ensure!(
                    !self.stopping && (self.task.is_none() || self.running),
                    "请等待当前操作完成"
                );
                if id == ROUTE_POOL {
                    let index = SendMessageW(self.by_id[&ROUTE_POOL], CB_GETCURSEL, 0, 0);
                    if let Some(pool) = self.catalog.pools.get(index as usize) {
                        self.set(POOL, &pool.id);
                    }
                }
                self.update_route_controls(false);
                self.update_route_summary();
            }
            _ => (),
        }
        Ok(())
    }
}
