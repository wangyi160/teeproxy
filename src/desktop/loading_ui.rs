//! Shared native loading indicators; animation follows the actual async tasks.
#![allow(unsafe_op_in_unsafe_fn)]
use super::*;

pub(super) const PAGE_LOADING: u16 = 569;
pub(super) const ACCOUNT_LOADING: u16 = 570;

impl App {
    pub(super) unsafe fn build_loading(&mut self) -> Result<()> {
        self.building_page = None;
        for (id, rect) in [
            (PAGE_LOADING, [-216, 34, 38, 38]),
            (ACCOUNT_LOADING, [260, 320, 48, 48]),
        ] {
            let hwnd = self.add(
                id,
                w!("STATIC"),
                "加载中",
                windows_sys::Win32::System::SystemServices::SS_OWNERDRAW,
                rect,
                0,
            )?;
            SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        Ok(())
    }

    pub(super) fn page_loading(&self) -> bool {
        (self.task.is_some() && (!self.running || self.stopping))
            || (self.page == OVERVIEW
                && (self.account.task.is_some() || self.inventory_task.is_some()))
            || if is_account_page(self.page) {
                self.account.task.is_some()
            } else if matches!(
                self.page,
                OVERVIEW | INVENTORY_PAGE | ROUTE_PAGE | PRICE_PAGE
            ) {
                self.inventory_task.is_some()
                    || (self.account.task.is_some() && !self.account.connection_ready())
            } else {
                false
            }
    }

    pub(super) fn account_table_loading(&self) -> bool {
        matches!(self.page, PAYMENTS_PAGE | FINANCE_PAGE) && self.account.table_loading()
    }

    pub(super) fn loading_rect(&self, id: u16, width: i32, height: i32) -> Option<[i32; 4]> {
        let [x, y, w, h] = match id {
            PRICE_LOADING => [260, 236, width - 312, height - 510],
            ACCOUNT_LOADING => self
                .withdrawal_rect(account_ui::TABLE, width, height)
                .or_else(|| self.payment_rect(account_ui::TABLE, width, height))
                .unwrap_or([260, 320, width - 312, height - 585]),
            _ => return None,
        };
        // Center in the list body, below the column header.
        Some([x + (w - 48) / 2, y + 26 + (h - 26 - 48) / 2, 48, 48])
    }

    pub(super) unsafe fn loading_tick(&self) {
        if IsWindowVisible(self.hwnd) == 0 {
            return;
        }
        for id in [
            PAGE_LOADING,
            ACCOUNT_LOADING,
            PRICE_LOADING,
            POOLS_LOADING,
            ACCOUNTS_LOADING,
            MODELS_LOADING,
            REFRESH,
        ] {
            if id == REFRESH && self.inventory_task.is_none() {
                continue;
            }
            if let Some(hwnd) = self.by_id.get(&id)
                && IsWindowVisible(*hwnd) != 0
            {
                InvalidateRect(*hwnd, ptr::null(), 0);
            }
        }
    }

    pub(super) unsafe fn draw_loading(&self, item: &DRAWITEMSTRUCT) {
        let saved = SaveDC(item.hDC);
        let header = item.CtlID as u16 == PAGE_LOADING;
        layout_ui::fill_rect(
            item.hDC,
            &item.rcItem,
            if header {
                rgb(248, 250, 253)
            } else {
                rgb(255, 255, 255)
            },
        );
        self.draw_loading_spinner(item.hDC, &item.rcItem, header);
        RestoreDC(item.hDC, saved);
    }

    pub(super) unsafe fn draw_loading_spinner(&self, dc: HDC, rect: &RECT, compact: bool) {
        let phase = (self.inventory_refreshed.elapsed().as_millis() / 100 % 12) as u32;
        let radius = f64::from(self.scale(if compact { 7 } else { 12 }));
        let dot = self.scale(if compact { 3 } else { 5 });
        let center_x = f64::from(rect.left + rect.right) / 2.0;
        let center_y = f64::from(rect.top + rect.bottom) / 2.0;
        SelectObject(dc, GetStockObject(DC_BRUSH));
        SelectObject(dc, GetStockObject(NULL_PEN));
        for index in 0..12 {
            let age = (phase + 12 - index) % 12;
            let strength = 48 + (11 - age) * 18;
            let channel = |value: u32| 255 - (255 - value) * strength / 255;
            let color = rgb(channel(47), channel(93), channel(232));
            SetDCBrushColor(dc, color);
            let angle = f64::from(index) * std::f64::consts::TAU / 12.0;
            let x = (center_x + radius * angle.sin()).round() as i32 - dot / 2;
            let y = (center_y - radius * angle.cos()).round() as i32 - dot / 2;
            if compact {
                // GDI can omit the interior of very small penless ellipses.
                super::layout_ui::fill_rect(
                    dc,
                    &RECT {
                        left: x,
                        top: y,
                        right: x + dot,
                        bottom: y + dot,
                    },
                    color,
                );
            } else {
                Ellipse(dc, x, y, x + dot, y + dot);
            }
        }
    }
}
