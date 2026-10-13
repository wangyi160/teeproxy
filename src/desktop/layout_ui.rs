//! Native sidebar navigation and detail pages; all drawing stays on the UI thread.
#![allow(unsafe_op_in_unsafe_fn)]
use super::*;

pub(super) const NAV_FIRST: u16 = 501;
pub(super) const NAV_LAST: u16 = 506;
pub(super) const PAGE_TITLE: u16 = 509;
pub(super) const PAGE_DESCRIPTION: u16 = 510;
pub(super) const OPEN_SETTINGS: u16 = 511;
pub(super) const SERVICE_STATE: u16 = 512;
pub(super) const ROUTER_SUMMARY: u16 = 514;
pub(super) const MODELS_EMPTY: u16 = 515;
pub(super) const ACCOUNTS_EMPTY: u16 = 516;
pub(super) const MODEL_COUNT: u16 = 517;
pub(super) const POOL_COUNT: u16 = 518;
pub(super) const OVERVIEW: usize = 0;
pub(super) const MODEL_PAGE: usize = 1;
pub(super) const ACCOUNT_PAGE: usize = 2;
pub(super) const SETTINGS_PAGE: usize = 3;
pub(super) const LOG_PAGE: usize = 4;
pub(super) const ROUTE_PAGE: usize = 5;
const PAGES: [(&str, &str); 6] = [
    ("概览", "连接你的 Agent，查看本地代理的运行状态。"),
    (
        "模型列表",
        "先选择账号池，再使用该池的模型；切换运行中的池需先停止代理。",
    ),
    ("账号池", "在上方选择账号池，下方查看该池的账号及状态。"),
    ("连接设置", "配置上游 Router 与本地代理服务。"),
    ("运行记录", "查看连接验证、列表刷新和代理运行的详细记录。"),
    ("路由策略", "指定账号池，或按模型、倍率和实测速度自动匹配。"),
];

impl App {
    pub(super) unsafe fn build_shell(
        &mut self,
        profile: &Profile,
        secrets: &Secrets,
    ) -> Result<()> {
        self.building_page = None;
        self.label("teeproxy", [64, 27, 130, 30], 3)?;
        self.label("安全连接 · 本地代理", [24, 68, 172, 22], 2)?;
        for (page, (title, _)) in PAGES.iter().enumerate() {
            self.button(
                NAV_FIRST + page as u16,
                title,
                [16, 128 + page as i32 * 54, 176, 44],
                0,
            )?;
        }
        self.add(
            SERVICE_STATE,
            w!("STATIC"),
            "代理已停止",
            SS_LEFT,
            [24, -110, 168, 24],
            4,
        )?;
        self.label("关闭窗口后在托盘运行", [24, -78, 172, 20], 2)?;
        self.label(
            concat!("v", env!("CARGO_PKG_VERSION")),
            [24, -40, 172, 20],
            2,
        )?;
        self.add(
            PAGE_TITLE,
            w!("STATIC"),
            "概览",
            SS_LEFT,
            [244, 32, -452, 42],
            1,
        )?;
        self.add(
            PAGE_DESCRIPTION,
            w!("STATIC"),
            "",
            SS_LEFT,
            [244, 82, -284, 26],
            2,
        )?;
        self.button(START, "启动代理", [-168, 34, 132, 38], 0)?;
        self.add(
            STATUS,
            w!("STATIC"),
            "已停止",
            SS_LEFT,
            [244, -40, -280, 24],
            2,
        )?;
        self.button(REFRESH, "刷新列表", [-156, 130, 120, 36], 0)?;
        self.add(
            CATALOG_STATUS,
            w!("STATIC"),
            "尚未读取列表",
            SS_LEFT,
            [244, -74, -280, 22],
            2,
        )?;

        self.building_page = Some(OVERVIEW);
        self.label("本地接入地址", [268, 150, 360, 26], 3)?;
        self.edit(
            URL,
            &format!("http://127.0.0.1:{}/v1", profile.port),
            [268, 198, -460, 40],
            ES_READONLY as u32,
            "",
        )?;
        self.button(COPY, "复制地址", [-168, 198, 108, 40], 0)?;
        self.label(
            "将此地址填入 Agent 的 API 地址（Base URL）。",
            [268, 258, -316, 22],
            2,
        )?;
        self.label("上游连接", [268, 342, 330, 26], 3)?;
        self.add(
            ROUTER_SUMMARY,
            w!("STATIC"),
            "",
            SS_LEFT,
            [268, 392, -316, 26],
            0,
        )?;
        self.add(
            ROUTE_OVERVIEW,
            w!("STATIC"),
            "尚未选择账号池",
            SS_LEFT,
            [268, 432, -316, 22],
            2,
        )?;
        self.button(OPEN_SETTINGS, "连接设置", [-192, 336, 132, 38], 0)?;
        self.label("池内模型总数", [268, 518, 220, 24], 2)?;
        self.add(
            MODEL_COUNT,
            w!("STATIC"),
            "未读取",
            SS_LEFT,
            [268, 552, 220, 34],
            3,
        )?;
        self.label("上游账号池", [576, 518, 220, 24], 2)?;
        self.add(
            POOL_COUNT,
            w!("STATIC"),
            "未读取",
            SS_LEFT,
            [576, 552, 220, 34],
            3,
        )?;

        self.build_inventory()?;
        self.building_page = Some(SETTINGS_PAGE);
        self.label("上游 Router", [268, 142, 400, 26], 3)?;
        self.label("Router 地址", [268, 195, 132, 24], 0)?;
        self.edit(
            ROUTER,
            &profile.router,
            [412, 186, -472, 38],
            0,
            "https://router.example.com:8443",
        )?;
        self.label("业务密钥", [268, 249, 132, 24], 0)?;
        self.edit(
            ROUTER_KEY,
            &secrets.router_key,
            [412, 240, -596, 38],
            ES_PASSWORD as u32,
            "填写 Router key",
        )?;
        self.button(
            SHOW_KEYS,
            "显示密钥",
            [-166, 246, 106, 26],
            BS_AUTOCHECKBOX as u32,
        )?;
        self.label("TDX 批准策略", [268, 303, 136, 24], 0)?;
        self.edit(
            TDX,
            &profile.tdx_policy,
            [412, 294, -584, 38],
            0,
            "选择批准策略文件",
        )?;
        self.button(BROWSE_TDX, "选择文件", [-156, 294, 96, 38], 0)?;
        self.label("SNP 批准策略", [268, 357, 136, 24], 0)?;
        self.edit(
            SNP,
            &profile.snp_policy,
            [412, 348, -584, 38],
            0,
            "使用 AMD SEV-SNP 时填写",
        )?;
        self.button(BROWSE_SNP, "选择文件", [-156, 348, 96, 38], 0)?;
        self.label(
            "按 Router 的运行环境选择 TDX 或 SNP 策略。",
            [412, 402, -472, 22],
            2,
        )?;

        self.label("本地服务", [268, 468, 400, 26], 3)?;
        self.label("监听端口", [268, 520, 132, 24], 0)?;
        self.edit(
            PORT,
            &profile.port.to_string(),
            [412, 511, 110, 38],
            ES_NUMBER as u32,
            "8899",
        )?;
        self.label("账号池", [546, 520, 72, 24], 0)?;
        self.edit(
            POOL,
            &profile.pool,
            [624, 511, -684, 38],
            ES_READONLY as u32,
            "在路由策略页选择",
        )?;
        self.label("本地密钥", [268, 574, 132, 24], 0)?;
        self.edit(
            CLIENT_KEY,
            &secrets.client_key,
            [412, 565, -472, 38],
            ES_PASSWORD as u32,
            "可选；设置后同时填入 Agent",
        )?;
        self.label(
            "密钥使用当前 Windows 用户加密保存。",
            [268, 623, -316, 22],
            2,
        )?;
        self.button(SAVE, "保存设置", [244, 674, 132, 38], 0)?;
        self.button(VERIFY, "验证连接", [392, 674, 132, 38], 0)?;

        self.building_page = Some(LOG_PAGE);
        self.add(
            LOG,
            w!("EDIT"),
            "",
            WS_TABSTOP
                | WS_VSCROLL
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_READONLY as u32,
            [260, 144, -312, -208],
            2,
        )?;
        self.building_page = None;
        self.build_routing(profile)?;
        self.building_page = None;
        self.update_summary();
        Ok(())
    }

    pub(super) unsafe fn update_summary(&self) {
        self.update_route_summary();
        let router = self.text(ROUTER);
        self.set(
            ROUTER_SUMMARY,
            if router.is_empty() {
                "尚未配置 Router，请先完成连接设置。"
            } else {
                &router
            },
        );
        self.set(
            MODEL_COUNT,
            &if self.catalog_loaded {
                self.catalog.models.len().to_string()
            } else {
                "未读取".into()
            },
        );
        self.set(
            POOL_COUNT,
            &if self.catalog_loaded {
                self.catalog.pools.len().to_string()
            } else {
                "未读取".into()
            },
        );
    }

    pub(super) unsafe fn update_visibility(&self) {
        for control in &self.controls {
            let visible = match control.id {
                REFRESH => matches!(self.page, MODEL_PAGE | ACCOUNT_PAGE | ROUTE_PAGE),
                CATALOG_STATUS => matches!(self.page, MODEL_PAGE | ACCOUNT_PAGE),
                MODELS_EMPTY => self.page == MODEL_PAGE && self.model_rows.is_empty(),
                POOLS_EMPTY => self.page == ACCOUNT_PAGE && self.catalog.pools.is_empty(),
                ACCOUNTS_EMPTY => {
                    self.page == ACCOUNT_PAGE
                        && self.account_pool().is_none_or(|p| p.account_count == 0)
                }
                _ => control.page.is_none_or(|page| page == self.page),
            };
            ShowWindow(control.hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
    }

    pub(super) unsafe fn select_page(&mut self, page: usize) {
        self.page = page.min(ROUTE_PAGE);
        let (title, description) = PAGES[self.page];
        self.set(PAGE_TITLE, title);
        self.set(PAGE_DESCRIPTION, description);
        self.update_visibility();
        self.layout();
        InvalidateRect(self.hwnd, ptr::null(), 1);
        for id in NAV_FIRST..=NAV_LAST {
            InvalidateRect(self.by_id[&id], ptr::null(), 1);
        }
        SetFocus(self.by_id[&(NAV_FIRST + self.page as u16)]);
    }

    pub(super) unsafe fn control_background(&self, hwnd: HWND) -> u32 {
        if let Some(c) = self.controls.iter().find(|c| c.hwnd == hwnd) {
            if c.rect[0] >= 0 && c.rect[0] < 208 {
                return rgb(241, 244, 249);
            }
            if c.page.is_some() && !matches!(c.id, SEARCH | COPY_MODEL | USE_POOL | SAVE | VERIFY) {
                return rgb(255, 255, 255);
            }
        }
        rgb(248, 250, 253)
    }

    pub(super) unsafe fn paint_surface(&self, dc: HDC) {
        let saved = SaveDC(dc);
        let mut client = RECT::default();
        GetClientRect(self.hwnd, &mut client);
        let width = client.right * 96 / self.dpi as i32;
        let height = client.bottom * 96 / self.dpi as i32;
        fill_rect(dc, &client, rgb(248, 250, 253));
        let sidebar = RECT {
            right: self.scale(208),
            ..client
        };
        fill_rect(dc, &sidebar, rgb(241, 244, 249));
        let divider = RECT {
            left: self.scale(207),
            right: self.scale(208),
            ..client
        };
        fill_rect(dc, &divider, rgb(226, 232, 240));
        let logo = self.scaled_rect([24, 28, 28, 28], width, height);
        rounded(dc, &logo, self.scale(7), rgb(47, 93, 232), rgb(47, 93, 232));
        draw_text(
            dc,
            "T",
            self.fonts[4],
            logo,
            rgb(255, 255, 255),
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        let cards: Vec<[i32; 4]> = match self.page {
            OVERVIEW => vec![
                [244, 128, -280, 176],
                [244, 320, -280, 160],
                [244, 496, -280, 116],
            ],
            SETTINGS_PAGE => vec![[244, 120, -280, 316], [244, 448, -280, 204]],
            MODEL_PAGE => vec![[244, 230, -280, -318]],
            ACCOUNT_PAGE => vec![[244, 186, -280, 210], [244, 412, -280, -500]],
            LOG_PAGE => vec![[244, 128, -280, -184]],
            ROUTE_PAGE => vec![[244, 128, -280, 490], [244, 680, -280, 42]],
            _ => Vec::new(),
        };
        for card in cards {
            rounded(
                dc,
                &self.scaled_rect(card, width, height),
                self.scale(10),
                rgb(255, 255, 255),
                rgb(225, 231, 240),
            );
        }
        for (rect, page, hwnd) in &self.input_frames {
            if page.is_none_or(|page| page == self.page) {
                rounded(
                    dc,
                    &self.scaled_rect(*rect, width, height),
                    self.scale(6),
                    rgb(255, 255, 255),
                    if GetFocus() == *hwnd {
                        rgb(47, 93, 232)
                    } else {
                        rgb(210, 219, 232)
                    },
                );
            }
        }
        RestoreDC(dc, saved);
    }

    unsafe fn scaled_rect(&self, rect: [i32; 4], width: i32, height: i32) -> RECT {
        let [x, y, w, h] = rect;
        let x = if x < 0 { width + x } else { x };
        let y = if y < 0 { height + y } else { y };
        let w = if w < 0 { width + w } else { w };
        let h = if h < 0 { height + h } else { h };
        RECT {
            left: self.scale(x),
            top: self.scale(y),
            right: self.scale(x + w),
            bottom: self.scale(y + h),
        }
    }

    pub(super) unsafe fn draw_button(&self, item: &DRAWITEMSTRUCT) {
        let dc = item.hDC;
        let saved = SaveDC(dc);
        let id = item.CtlID as u16;
        let nav = (NAV_FIRST..=NAV_LAST).contains(&id);
        let active = nav && (id - NAV_FIRST) as usize == self.page;
        let disabled = item.itemState & ODS_DISABLED != 0;
        let pressed = item.itemState & ODS_SELECTED != 0;
        let primary = matches!(id, START | SAVE);
        let color = if disabled {
            rgb(153, 163, 178)
        } else if active {
            rgb(39, 79, 202)
        } else if primary {
            rgb(255, 255, 255)
        } else {
            rgb(48, 61, 82)
        };
        let background = self.control_background(item.hwndItem);
        fill_rect(dc, &item.rcItem, background);
        let fill = if disabled {
            rgb(236, 240, 245)
        } else if active {
            rgb(225, 234, 253)
        } else if primary {
            if pressed {
                rgb(32, 67, 183)
            } else {
                rgb(47, 93, 232)
            }
        } else if pressed {
            rgb(231, 237, 246)
        } else if nav {
            background
        } else {
            rgb(255, 255, 255)
        };
        let border = if nav || primary || disabled {
            fill
        } else {
            rgb(213, 221, 234)
        };
        rounded(dc, &item.rcItem, self.scale(7), fill, border);
        let mut rect = item.rcItem;
        let flags = if nav {
            let symbols = [
                "\u{e80f}", "\u{e8a5}", "\u{e716}", "\u{e713}", "\u{e81c}", "\u{e8ab}",
            ];
            let mut icon = rect;
            icon.left += self.scale(16);
            icon.right = icon.left + self.scale(22);
            draw_text(
                dc,
                symbols[(id - NAV_FIRST) as usize],
                self.fonts[5],
                icon,
                color,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            rect.left += self.scale(54);
            DT_LEFT | DT_VCENTER | DT_SINGLELINE
        } else {
            DT_CENTER | DT_VCENTER | DT_SINGLELINE
        };
        draw_text(
            dc,
            &self.text(id),
            self.fonts[if active || primary { 4 } else { 0 }],
            rect,
            color,
            flags,
        );
        if item.itemState & (ODS_FOCUS | ODS_NOFOCUSRECT) == ODS_FOCUS {
            let mut focus = item.rcItem;
            InflateRect(&mut focus, -self.scale(4), -self.scale(4));
            DrawFocusRect(dc, &focus);
        }
        RestoreDC(dc, saved);
    }
}

unsafe fn fill_rect(dc: HDC, rect: &RECT, color: u32) {
    SetDCBrushColor(dc, color);
    FillRect(dc, rect, GetStockObject(DC_BRUSH) as HBRUSH);
}
unsafe fn rounded(dc: HDC, rect: &RECT, radius: i32, fill: u32, border: u32) {
    SelectObject(dc, GetStockObject(DC_BRUSH));
    SelectObject(dc, GetStockObject(DC_PEN));
    SetDCBrushColor(dc, fill);
    SetDCPenColor(dc, border);
    RoundRect(
        dc,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        radius,
        radius,
    );
}
unsafe fn draw_text(dc: HDC, value: &str, font: HFONT, mut rect: RECT, color: u32, flags: u32) {
    SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, color);
    let text = wide(value);
    DrawTextW(
        dc,
        text.as_ptr(),
        (text.len() - 1) as i32,
        &mut rect,
        flags | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
}
