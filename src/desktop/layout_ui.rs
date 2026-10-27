//! Native sidebar navigation and detail pages; all drawing stays on the UI thread.
#![allow(unsafe_op_in_unsafe_fn)]
use super::*;

pub(super) const NAV_FIRST: u16 = 501;
pub(super) const NAV_LAST: u16 = NAV_FIRST + PAGES.len() as u16 - 1;
pub(super) const PAGE_TITLE: u16 = 559;
pub(super) const PAGE_DESCRIPTION: u16 = 560;
pub(super) const OPEN_SETTINGS: u16 = 561;
pub(super) const SERVICE_STATE: u16 = 562;
pub(super) const MODELS_LOADING: u16 = 565;
pub(super) const ACCOUNTS_LOADING: u16 = 566;
pub(super) const MODEL_COUNT: u16 = 567;
pub(super) const POOL_COUNT: u16 = 568;
pub(super) const OVERVIEW: usize = 0;
pub(super) const INVENTORY_PAGE: usize = 1;
pub(super) const SETTINGS_PAGE: usize = 2;
pub(super) const LOG_PAGE: usize = 3;
pub(super) const ROUTE_PAGE: usize = 4;
pub(super) const PRICE_PAGE: usize = 5;
pub(super) const ACCOUNT_PAGE: usize = 6;
pub(super) const PAYMENTS_PAGE: usize = 7;
pub(super) const FINANCE_PAGE: usize = 8;
pub(super) const TRUST_PAGE: usize = 9;
pub(super) fn is_account_page(page: usize) -> bool {
    (ACCOUNT_PAGE..=FINANCE_PAGE).contains(&page)
}
const PAGES: [(&str, &str); 10] = [
    ("概览", ""),
    ("账号池与模型", ""),
    ("Agent 接入", ""),
    ("运行记录", ""),
    ("路由策略", ""),
    ("模型价格", ""),
    ("我的账户", ""),
    ("余额与充值", ""),
    ("消费明细", ""),
    ("连接与验证", ""),
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
        self.label("本地代理", [32, 104, 144, 20], 2)?;
        self.label("平台账户", [32, 344, 144, 20], 2)?;
        self.label("设置", [32, 496, 144, 20], 2)?;
        for (page, (title, _)) in PAGES.iter().enumerate() {
            self.button(
                NAV_FIRST + page as u16,
                title,
                [
                    16,
                    if page < ACCOUNT_PAGE {
                        128 + page as i32 * 34
                    } else if page == TRUST_PAGE {
                        524
                    } else {
                        370 + (page - ACCOUNT_PAGE) as i32 * 34
                    },
                    176,
                    32,
                ],
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
            "",
            SS_LEFT,
            [244, -74, -280, 22],
            2,
        )?;

        self.build_overview(profile)?;

        self.build_inventory()?;
        self.building_page = Some(SETTINGS_PAGE);
        self.label("本地服务", [268, 150, 400, 26], 3)?;
        self.label("监听 IP", [268, 214, 132, 24], 0)?;
        self.edit(LISTEN_IP, &profile.listen_ip, [412, 204, -664, 40], 0, "")?;
        self.label("端口", [-236, 214, 64, 24], 0)?;
        self.edit(
            PORT,
            &profile.port.to_string(),
            [-156, 204, 96, 40],
            ES_NUMBER as u32,
            "",
        )?;
        self.label("API key", [268, 284, 132, 24], 0)?;
        self.edit(CLIENT_KEY, &secrets.client_key, [412, 274, -472, 40], 0, "")?;
        self.button(GENERATE_KEY, "生成 API key", [412, 334, 160, 38], 0)?;
        self.button(COPY_KEY, "复制 API key", [584, 334, 160, 38], 0)?;
        self.button(SAVE, "保存", [268, 422, 132, 40], 0)?;

        self.building_page = Some(TRUST_PAGE);
        self.label("平台地址", [268, 160, 140, 24], 0)?;
        let platform = std::fs::read(self.path.parent().unwrap().join("account-platform.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<String>(&b).ok())
            .unwrap_or_else(|| "https://teerouter.org".into());
        self.edit(account_ui::PLATFORM, &platform, [420, 150, -480, 40], 0, "")?;
        self.label("Router 地址", [268, 228, 140, 24], 0)?;
        self.edit(ROUTER, &profile.router, [420, 218, -480, 40], 0, "")?;
        self.label("TDX 信任策略", [268, 314, 140, 24], 0)?;
        self.edit(TDX, &profile.tdx_policy, [420, 304, -592, 40], 0, "")?;
        self.button(BROWSE_TDX, "浏览文件", [-156, 304, 96, 40], 0)?;
        self.label("SNP 信任策略", [268, 382, 140, 24], 0)?;
        self.edit(SNP, &profile.snp_policy, [420, 372, -592, 40], 0, "")?;
        self.button(BROWSE_SNP, "浏览文件", [-156, 372, 96, 40], 0)?;
        self.button(SAVE_TRUST, "保存", [268, 456, 132, 40], 0)?;
        self.button(VERIFY, "验证连接", [416, 456, 132, 40], 0)?;

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
        self.build_pricing()?;
        self.build_account()?;
        self.build_loading()?;
        self.update_summary();
        Ok(())
    }

    pub(super) unsafe fn update_summary(&self) {
        self.update_route_summary();
    }

    pub(super) unsafe fn update_visibility(&self) {
        for control in &self.controls {
            let visible = match control.id {
                REFRESH => matches!(self.page, INVENTORY_PAGE | ROUTE_PAGE | PRICE_PAGE),
                PAGE_LOADING => self.page_loading(),
                ACCOUNT_LOADING => self.account_table_loading(),
                PRICE_LOADING => {
                    self.page == PRICE_PAGE
                        && self.inventory_task.is_some()
                        && self.price_rows.is_empty()
                }
                CATALOG_STATUS => {
                    self.page == INVENTORY_PAGE && !self.text(CATALOG_STATUS).is_empty()
                }
                MODELS_LOADING => {
                    self.page == INVENTORY_PAGE
                        && self.inventory_task.is_some()
                        && self.model_rows.is_empty()
                }
                POOLS_LOADING => {
                    self.page == INVENTORY_PAGE
                        && self.inventory_task.is_some()
                        && self.catalog.pools.is_empty()
                }
                ACCOUNTS_LOADING => {
                    self.page == INVENTORY_PAGE
                        && self.inventory_task.is_some()
                        && self.account_pool().is_none_or(|p| p.account_count == 0)
                }
                _ => control.page.is_none_or(|page| {
                    if page == ACCOUNT_PAGE {
                        is_account_page(self.page)
                    } else {
                        page == self.page
                    }
                }),
            };
            let visible = visible
                && (control.page != Some(ACCOUNT_PAGE) || self.account.visible(control.hwnd))
                && !self.payment_hidden(control.id);
            ShowWindow(control.hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
    }

    pub(super) unsafe fn select_page(&mut self, page: usize) {
        self.page = page.min(PAGES.len() - 1);
        self.set(
            REFRESH,
            if self.page == PRICE_PAGE {
                "刷新价格"
            } else {
                "刷新列表"
            },
        );
        let (title, description) = PAGES[self.page];
        self.set(PAGE_TITLE, title);
        self.set(PAGE_DESCRIPTION, description);
        if is_account_page(self.page) || self.page == OVERVIEW {
            self.account_page_entered();
        }
        self.inventory_refresh_pending = matches!(self.page, OVERVIEW | INVENTORY_PAGE);
        self.prices_refresh_pending = self.page == PRICE_PAGE;
        if matches!(self.page, OVERVIEW | INVENTORY_PAGE) {
            if !self.account.logged_in() {
                self.set(CATALOG_STATUS, "未登录");
            } else if self.router_key.is_empty() {
                self.set(CATALOG_STATUS, "连接中");
            }
            self.refresh_inventory_if_due();
        }
        if self.page == OVERVIEW {
            self.update_overview();
        }
        if self.page == PRICE_PAGE {
            self.refresh_prices_if_due();
        }
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
            if self.payment_white_control(c.id) {
                return rgb(255, 255, 255);
            }
            if c.rect[0] >= 0 && c.rect[0] < 208 {
                return rgb(241, 244, 249);
            }
            if c.page == Some(ACCOUNT_PAGE)
                && !self.input_frames.iter().any(|(_, _, input)| *input == hwnd)
            {
                return rgb(248, 250, 253);
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
            OVERVIEW => overview_cards(width).to_vec(),
            SETTINGS_PAGE => vec![[244, 128, -280, 268]],
            TRUST_PAGE => vec![[244, 128, -280, 300]],
            INVENTORY_PAGE => inventory_cards(width, height).to_vec(),
            LOG_PAGE => vec![[244, 128, -280, -184]],
            ROUTE_PAGE => vec![[244, 128, -280, 382], [244, 572, -280, 42]],
            PRICE_PAGE => vec![[244, 174, -280, -258]],
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
        self.paint_overview(dc, width, height);
        self.paint_payment(dc, width, height);
        for (rect, page, hwnd) in &self.input_frames {
            if page.is_none_or(|page| {
                if page == ACCOUNT_PAGE {
                    is_account_page(self.page)
                } else {
                    page == self.page
                }
            }) && GetWindowLongPtrW(*hwnd, GWL_STYLE) & WS_VISIBLE as isize != 0
            {
                rounded(
                    dc,
                    &self.scaled_rect(
                        self.payment_frame(*hwnd, width).unwrap_or(*rect),
                        width,
                        height,
                    ),
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

    pub(super) unsafe fn scaled_rect(&self, rect: [i32; 4], width: i32, height: i32) -> RECT {
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
        let primary = matches!(id, START | SAVE | SAVE_TRUST | 1515 | 1531 | 1539);
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
        if overview_link(id) {
            draw_text(
                dc,
                &self.text(id),
                self.fonts[2],
                item.rcItem,
                if pressed {
                    rgb(32, 67, 183)
                } else {
                    rgb(47, 93, 232)
                },
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            if item.itemState & ODS_FOCUS != 0 {
                DrawFocusRect(dc, &item.rcItem);
            }
            RestoreDC(dc, saved);
            return;
        }
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
        if id == REFRESH && self.inventory_task.is_some() {
            self.draw_loading_spinner(dc, &item.rcItem, true);
            RestoreDC(dc, saved);
            return;
        }
        let mut rect = item.rcItem;
        let flags = if nav {
            let symbols: [&str; PAGES.len()] = [
                "\u{e80f}", "\u{e716}", "\u{e713}", "\u{e81c}", "\u{e8ab}", "\u{e8d4}", "\u{e77b}",
                "\u{e8c7}", "\u{e9d9}", "\u{e72e}",
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

pub(super) unsafe fn fill_rect(dc: HDC, rect: &RECT, color: u32) {
    SetDCBrushColor(dc, color);
    FillRect(dc, rect, GetStockObject(DC_BRUSH) as HBRUSH);
}
pub(super) unsafe fn rounded(dc: HDC, rect: &RECT, radius: i32, fill: u32, border: u32) {
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
pub(super) unsafe fn draw_text(
    dc: HDC,
    value: &str,
    font: HFONT,
    mut rect: RECT,
    color: u32,
    flags: u32,
) {
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
