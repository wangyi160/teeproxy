//! Native checkout cards and local QR rendering. No web view or external QR service.
#![allow(unsafe_op_in_unsafe_fn)]
use super::*;
use crate::desktop::layout_ui::{draw_text, fill_rect, rounded};

pub(super) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn payment_payable(row: &Value, now: u64) -> bool {
    row["state"] == "pending"
        && integer(&row["expires_at"]).is_some_and(|expiry| expiry > i128::from(now))
        && integer(&row["payable_microusd"]).is_some_and(|value| value > 0)
        && row["receive_address"].as_str().is_some_and(|address| {
            !address.is_empty()
                && address.len() <= 128
                && address
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b":_-".contains(&c))
        })
}

fn cards(width: i32, height: i32) -> [[i32; 4]; 4] {
    let content = width - 280;
    let history = (content * 2 / 5).clamp(270, 380);
    [
        [244, 128, 210, 130],
        [470, 128, width - 506, 130],
        [244, 278, history, height - 378],
        [260 + history, 278, content - history - 16, height - 378],
    ]
}

impl App {
    pub(in crate::desktop) fn payment_page(&self) -> bool {
        self.page == PAYMENTS_PAGE
            && self.account.logged_in()
            && self.account.panel == Panel::Payments
    }

    pub(in crate::desktop) fn payment_rect(
        &self,
        id: u16,
        width: i32,
        height: i32,
    ) -> Option<[i32; 4]> {
        if !self.payment_page() {
            return None;
        }
        let [_, form, history, checkout] = cards(width, height);
        let [hx, hy, hw, hh] = history;
        let [cx, cy, cw, ch] = checkout;
        Some(match id {
            IDENTITY => [264, 224, 170, 22],
            BALANCE => [264, 180, 174, 30],
            NETWORK => [form[0] + 20, 204, form[2] - 316, 250],
            AMOUNT => [width - 292, 210, 84, 22],
            PAY => [width - 184, 200, 128, 38],
            TABLE => [hx + 16, hy + 48, hw - 32, hh - 108],
            PAGE => [hx + 116, hy + 20, hw - 136, 20],
            PREV => [hx + 16, hy + hh - 48, (hw - 44) / 2, 32],
            NEXT => [hx + 28 + (hw - 44) / 2, hy + hh - 48, (hw - 44) / 2, 32],
            COPY => [cx + 20, cy + ch - 50, (cw - 52) / 2, 32],
            COPY_AMOUNT => [cx + 32 + (cw - 52) / 2, cy + ch - 50, (cw - 52) / 2, 32],
            _ => return None,
        })
    }

    pub(in crate::desktop) fn payment_frame(&self, hwnd: HWND, width: i32) -> Option<[i32; 4]> {
        (self.payment_page() && self.by_id.get(&AMOUNT) == Some(&hwnd)).then_some([
            width - 302,
            200,
            104,
            40,
        ])
    }

    pub(in crate::desktop) fn payment_hidden(&self, id: u16) -> bool {
        self.payment_page() && matches!(id, DETAILS | DETAIL)
    }

    pub(in crate::desktop) fn payment_white_control(&self, id: u16) -> bool {
        self.payment_page()
            && matches!(
                id,
                IDENTITY
                    | BALANCE
                    | NETWORK
                    | AMOUNT
                    | PAY
                    | PAGE
                    | PREV
                    | NEXT
                    | COPY
                    | COPY_AMOUNT
            )
    }

    pub(in crate::desktop) unsafe fn payment_columns(&self, width: i32, height: i32) {
        if !self.payment_page() {
            return;
        }
        let available = cards(width, height)[2][2] - 38;
        let network = available / 3;
        for (index, size) in [0, network, available - network - 72, 72, 0]
            .into_iter()
            .enumerate()
        {
            SendMessageW(
                TABLE.hwnd(self),
                LVM_SETCOLUMNWIDTH,
                index,
                self.scale(size) as isize,
            );
        }
    }

    pub(in crate::desktop) unsafe fn payment_buttons(&self) {
        if !self.payment_page() {
            return;
        }
        let payable = payment_payable(&self.account.detail, unix_now());
        EnableWindow(COPY.hwnd(self), payable as i32);
        EnableWindow(COPY_AMOUNT.hwnd(self), payable as i32);
    }

    pub(in crate::desktop) unsafe fn payment_tick(&mut self) {
        if !self.payment_page() || self.account.detail.is_null() || IsWindowVisible(self.hwnd) == 0
        {
            return;
        }
        let now = unix_now();
        if self.account.payment_second == now {
            return;
        }
        self.account.payment_second = now;
        self.payment_buttons();
        let mut client = RECT::default();
        GetClientRect(self.hwnd, &mut client);
        let width = client.right * 96 / self.dpi as i32;
        let height = client.bottom * 96 / self.dpi as i32;
        let rect = self.scaled_rect(cards(width, height)[3], width, height);
        InvalidateRect(self.hwnd, &rect, 0);
    }

    pub(in crate::desktop) unsafe fn paint_payment(&self, dc: HDC, width: i32, height: i32) {
        if !self.payment_page() {
            return;
        }
        let surfaces = cards(width, height);
        for rect in surfaces {
            rounded(
                dc,
                &self.scaled_rect(rect, width, height),
                self.scale(12),
                rgb(255, 255, 255),
                rgb(225, 231, 240),
            );
        }
        let label = |value: &str, rect: [i32; 4], font: usize, color: u32| {
            draw_text(
                dc,
                value,
                self.fonts[font],
                self.scaled_rect(rect, width, height),
                color,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
        };
        let ink = rgb(28, 41, 61);
        let muted = rgb(106, 119, 139);
        label("可用余额 · USD", [264, 148, 170, 24], 2, muted);
        label("USDT 充值", [490, 146, 160, 24], 3, ink);
        label("支付网络", [490, 178, 160, 20], 2, muted);
        label("充值金额", [width - 302, 178, 104, 20], 2, muted);
        label("充值记录", [260, surfaces[2][1] + 18, 100, 24], 4, ink);
        let [x, y, w, h] = surfaces[3];
        let row = &self.account.detail;
        if row.is_null() {
            label("付款详情", [x + 20, y + 18, w - 40, 26], 3, ink);
            draw_text(
                dc,
                "选择或创建充值订单",
                self.fonts[0],
                self.scaled_rect([x + 20, y + 76, w - 40, h - 116], width, height),
                muted,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            return;
        }
        let now = unix_now();
        let payable = payment_payable(row, now);
        let expired = row["state"] == "expired"
            || (row["state"] == "pending"
                && integer(&row["expires_at"]).is_some_and(|t| t <= i128::from(now)));
        let (state, color, bg) = if row["state"] == "credited" {
            ("已入账", rgb(28, 126, 85), rgb(230, 246, 237))
        } else if expired {
            ("已过期", muted, rgb(241, 244, 249))
        } else if payable {
            ("等待付款", rgb(39, 79, 202), rgb(234, 240, 255))
        } else if row["state"] == "paid" {
            ("正在入账", rgb(166, 110, 24), rgb(255, 245, 224))
        } else if row["state"] == "failed" {
            ("订单失败", rgb(170, 40, 40), rgb(255, 237, 237))
        } else {
            ("确认中", muted, rgb(241, 244, 249))
        };
        label("充值订单", [x + 20, y + 18, w - 140, 26], 3, ink);
        let badge = self.scaled_rect([x + w - 110, y + 18, 90, 28], width, height);
        rounded(dc, &badge, self.scale(12), bg, bg);
        draw_text(
            dc,
            state,
            self.fonts[4],
            badge,
            color,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        let side = (h - 184).clamp(144, 192);
        let qr_rect = self.scaled_rect([x + 20, y + 58, side, side], width, height);
        rounded(
            dc,
            &qr_rect,
            self.scale(8),
            rgb(255, 255, 255),
            rgb(231, 236, 244),
        );
        if payable && let Some(qr) = &self.account.payment_qr {
            // Whole device pixels with four quiet modules on each side; no resampling.
            let unit = ((qr_rect.right - qr_rect.left - self.scale(12)) / (qr.size() + 8)).max(1);
            let size = unit * (qr.size() + 8);
            let left = qr_rect.left + (qr_rect.right - qr_rect.left - size) / 2 + 4 * unit;
            let top = qr_rect.top + (qr_rect.bottom - qr_rect.top - size) / 2 + 4 * unit;
            for qy in 0..qr.size() {
                for qx in 0..qr.size() {
                    if qr.get_module(qx, qy) {
                        fill_rect(
                            dc,
                            &RECT {
                                left: left + qx * unit,
                                top: top + qy * unit,
                                right: left + (qx + 1) * unit,
                                bottom: top + (qy + 1) * unit,
                            },
                            rgb(0, 0, 0),
                        );
                    }
                }
            }
        } else {
            draw_text(
                dc,
                if row["state"] == "credited" {
                    "✓"
                } else {
                    state
                },
                self.fonts[3],
                qr_rect,
                color,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
        let right = x + side + 40;
        let remaining = w - side - 60;
        label("支付网络", [right, y + 60, remaining, 20], 2, muted);
        label(
            &text(&row["network_name"]),
            [right, y + 82, remaining, 26],
            4,
            ink,
        );
        label("应付金额 · USDT", [right, y + 118, remaining, 20], 2, muted);
        let payment_amount = amount(&row["payable_microusd"]);
        label(
            &payment_amount,
            [right, y + 140, remaining, 34],
            if payment_amount.len() <= 8 && remaining >= 150 {
                1
            } else {
                3
            },
            ink,
        );
        let timer = if payable {
            let seconds = (integer(&row["expires_at"]).unwrap() - i128::from(now)) as u64;
            format!("剩余 {:02}:{:02}", seconds / 60, seconds % 60)
        } else if row["state"] == "credited" {
            format!("到账 {}", money(&row["credit_microusd"]))
        } else {
            date(&row["created_at"])
        };
        label(&timer, [right, y + 184, remaining, 24], 2, color);
        let address_y = y + side + 70;
        label(
            if payable {
                "收款地址 · 仅限该网络的 USDT"
            } else {
                "订单编号"
            },
            [x + 20, address_y, w - 40, 20],
            2,
            muted,
        );
        let value = if payable {
            text(&row["receive_address"])
        } else {
            text(&row["id"])
        };
        draw_text(
            dc,
            &value,
            self.fonts[2],
            self.scaled_rect([x + 20, address_y + 24, w - 40, 34], width, height),
            ink,
            DT_LEFT | DT_WORDBREAK,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_or_unconfirmed_order_never_exposes_a_payable_qr() {
        let mut row = json!({"state":"pending","expires_at":101,"payable_microusd":"1000010","receive_address":"0xe027c39c24499527f5fce7f2a318eb037d85c26f"});
        assert!(payment_payable(&row, 100));
        assert!(!payment_payable(&row, 101));
        for state in ["creating", "expired", "paid", "credited", "failed"] {
            row["state"] = json!(state);
            assert!(!payment_payable(&row, 100));
        }
        row["state"] = json!("pending");
        row["receive_address"] = json!("");
        assert!(!payment_payable(&row, 100));
    }
}
