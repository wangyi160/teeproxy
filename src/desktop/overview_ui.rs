//! Overview groups service, account, Agent setup, routing and inventory by purpose.
#![allow(unsafe_op_in_unsafe_fn)]
use super::*;

const PROXY_STATE: u16 = 571;
const ACCOUNT_NAME: u16 = 572;
const BALANCE: u16 = 573;
const ROUTE_TARGET: u16 = 574;
const ROUTE_LIMIT: u16 = 575;
const RESOURCE_STATE: u16 = 576;
pub(super) const OPEN_ACCOUNT: u16 = 580;
pub(super) const OPEN_PAYMENTS: u16 = 581;
pub(super) const OPEN_ROUTING: u16 = 582;
pub(super) const OPEN_INVENTORY: u16 = 583;
pub(super) const OPEN_LOG: u16 = 584;

pub(super) fn overview_link(id: u16) -> bool {
    matches!(
        id,
        OPEN_SETTINGS | OPEN_ACCOUNT | OPEN_PAYMENTS | OPEN_ROUTING | OPEN_INVENTORY | OPEN_LOG
    )
}

pub(super) fn overview_cards(width: i32) -> [[i32; 4]; 6] {
    let available = width - 280;
    let metric = (available - 32) / 3;
    let route = (available - 16) * 56 / 100;
    [
        [244, 128, metric, 112],
        [260 + metric, 128, metric, 112],
        [276 + metric * 2, 128, available - 32 - metric * 2, 112],
        [244, 256, available, 176],
        [244, 448, route, 240],
        [260 + route, 448, available - route - 16, 240],
    ]
}

pub(super) fn overview_rect(id: u16, width: i32) -> Option<[i32; 4]> {
    let [service, account, balance, _, route, resources] = overview_cards(width);
    let metric = |card: [i32; 4]| [card[0] + 20, card[1] + 56, card[2] - 40, 36];
    let link = |card: [i32; 4], w| [card[0] + card[2] - w - 12, card[1] + 12, w, 30];
    Some(match id {
        PROXY_STATE => metric(service),
        ACCOUNT_NAME => metric(account),
        BALANCE => metric(balance),
        OPEN_LOG => link(service, 52),
        OPEN_ACCOUNT => link(account, 52),
        OPEN_PAYMENTS => link(balance, 52),
        OPEN_ROUTING => link(route, 52),
        OPEN_INVENTORY => link(resources, 80),
        ROUTE_OVERVIEW => [route[0] + 120, route[1] + 80, route[2] - 140, 26],
        ROUTE_TARGET => [route[0] + 120, route[1] + 124, route[2] - 140, 26],
        ROUTE_LIMIT => [route[0] + 120, route[1] + 168, route[2] - 140, 26],
        POOL_COUNT => [
            resources[0] + 24,
            resources[1] + 76,
            resources[2] / 2 - 40,
            42,
        ],
        MODEL_COUNT => [
            resources[0] + resources[2] / 2 + 12,
            resources[1] + 76,
            resources[2] / 2 - 36,
            42,
        ],
        RESOURCE_STATE => [resources[0] + 24, resources[1] + 184, resources[2] - 48, 22],
        _ => return None,
    })
}

impl App {
    pub(super) unsafe fn build_overview(&mut self, profile: &Profile) -> Result<()> {
        self.building_page = Some(OVERVIEW);
        for (id, font) in [
            (PROXY_STATE, 3),
            (ACCOUNT_NAME, 3),
            (BALANCE, 3),
            (ROUTE_OVERVIEW, 4),
            (ROUTE_TARGET, 0),
            (ROUTE_LIMIT, 0),
            (POOL_COUNT, 1),
            (MODEL_COUNT, 1),
            (RESOURCE_STATE, 2),
        ] {
            self.add(
                id,
                w!("STATIC"),
                "",
                SS_LEFT | windows_sys::Win32::System::SystemServices::SS_ENDELLIPSIS,
                overview_rect(id, 1100).unwrap(),
                font,
            )?;
        }
        for (id, label) in [
            (OPEN_LOG, "日志"),
            (OPEN_ACCOUNT, "管理"),
            (OPEN_PAYMENTS, "充值"),
            (OPEN_ROUTING, "修改"),
            (OPEN_INVENTORY, "查看全部"),
        ] {
            self.button(id, label, overview_rect(id, 1100).unwrap(), 0)?;
        }
        self.button(OPEN_SETTINGS, "接入设置", [-148, 270, 96, 32], 0)?;
        self.edit(
            URL,
            &profile
                .listen_ip
                .trim()
                .parse::<std::net::IpAddr>()
                .map(|ip| crate::config::local_api_url((ip, profile.port).into()))
                .unwrap_or_default(),
            [264, 350, -456, 42],
            ES_READONLY as u32,
            "",
        )?;
        self.button(COPY, "复制地址", [-176, 350, 120, 42], 0)?;
        Ok(())
    }

    pub(super) unsafe fn update_overview(&self) {
        if !self.by_id.contains_key(&PROXY_STATE) {
            return;
        }
        self.set(
            PROXY_STATE,
            if self.stopping {
                "正在停止"
            } else if self.running {
                "运行中"
            } else if self.task.is_some() {
                "连接中"
            } else {
                "已停止"
            },
        );
        let (username, balance) = self.account.overview_summary();
        self.set(ACCOUNT_NAME, &username);
        self.set(BALANCE, &balance);
        let active = self.live_proxy.as_ref().map(|proxy| proxy.routing_rules());
        let policy = active
            .as_ref()
            .map(|r| r.policy.clone())
            .or_else(|| self.routing_policy().ok());
        if let Some(policy) = policy {
            self.set(
                ROUTE_OVERVIEW,
                if policy.automatic {
                    "自动匹配"
                } else {
                    "指定账号池"
                },
            );
            let pool = active
                .as_ref()
                .and_then(|r| r.pool.as_deref())
                .unwrap_or(&self.configured_pool);
            self.set(
                ROUTE_TARGET,
                if policy.automatic {
                    if policy.priority == crate::routing::Priority::Speed {
                        "低延迟优先"
                    } else {
                        "费率优先"
                    }
                } else {
                    self.catalog
                        .pools
                        .iter()
                        .find(|p| p.id == pool)
                        .map(|p| p.name.as_str())
                        .unwrap_or(if pool.is_empty() { "—" } else { pool })
                },
            );
            self.set(
                ROUTE_LIMIT,
                &format!(
                    "{}×",
                    crate::routing::format_multiplier(policy.max_multiplier_millionths)
                ),
            );
        } else {
            for id in [ROUTE_OVERVIEW, ROUTE_TARGET, ROUTE_LIMIT] {
                self.set(id, "—");
            }
        }
        self.set(
            POOL_COUNT,
            &if self.catalog_loaded {
                self.catalog.pools.len().to_string()
            } else {
                "—".into()
            },
        );
        self.set(
            MODEL_COUNT,
            &if self.catalog_loaded {
                self.catalog
                    .models
                    .iter()
                    .map(|model| &model.id)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    .to_string()
            } else {
                "—".into()
            },
        );
        self.set(
            RESOURCE_STATE,
            if !self.account.logged_in() {
                "未登录"
            } else if self.router_key.is_empty() {
                "连接中"
            } else if self.text(CATALOG_STATUS) == "刷新失败" {
                "目录刷新失败"
            } else {
                ""
            },
        );
        if self.page == OVERVIEW {
            InvalidateRect(self.hwnd, ptr::null(), 0);
        }
    }

    pub(super) unsafe fn paint_overview(&self, dc: HDC, width: i32, height: i32) {
        if self.page != OVERVIEW {
            return;
        }
        let [service, account, balance, agent, route, resources] = overview_cards(width);
        let label = |text: &str, rect, font, color| {
            layout_ui::draw_text(
                dc,
                text,
                self.fonts[font],
                self.scaled_rect(rect, width, height),
                color,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            )
        };
        let muted = rgb(106, 119, 139);
        let ink = rgb(28, 41, 61);
        for (card, title) in [
            (service, "本地代理"),
            (account, "当前账户"),
            (balance, "可用余额 · USD"),
        ] {
            label(
                title,
                [card[0] + 20, card[1] + 18, card[2] - 82, 24],
                2,
                muted,
            );
        }
        for (card, title) in [
            (agent, "Agent 接入"),
            (route, "路由策略"),
            (resources, "可用资源"),
        ] {
            label(
                title,
                [card[0] + 20, card[1] + 18, card[2] - 120, 26],
                3,
                ink,
            );
        }
        label("API 地址 / Base URL", [264, 314, 320, 24], 2, muted);
        let automatic = self
            .live_proxy
            .as_ref()
            .map(|proxy| proxy.routing_rules().policy.automatic)
            .unwrap_or(SendMessageW(self.by_id[&ROUTE_MODE], CB_GETCURSEL, 0, 0) == 1);
        for (offset, title) in [
            (80, "路由模式"),
            (
                124,
                if automatic {
                    "匹配策略"
                } else {
                    "账号池"
                },
            ),
            (168, "倍率上限"),
        ] {
            label(title, [route[0] + 20, route[1] + offset, 94, 24], 2, muted);
        }
        label(
            "账号池",
            [resources[0] + 24, resources[1] + 128, 100, 24],
            2,
            muted,
        );
        label(
            "模型",
            [
                resources[0] + resources[2] / 2 + 12,
                resources[1] + 128,
                100,
                24,
            ],
            2,
            muted,
        );
    }
}
