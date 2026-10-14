#![allow(unsafe_op_in_unsafe_fn)]
use super::*;

pub(super) const PRICES: u16 = 701;
pub(super) const PRICE_PROVIDER: u16 = 702;
pub(super) const PRICE_SEARCH: u16 = 703;
pub(super) const PRICE_DETAILS: u16 = 704;
pub(super) const PRICE_STATUS: u16 = 705;
pub(super) const PRICE_EMPTY: u16 = 706;
pub(super) const PRICE_COUNT: u16 = 707;

impl App {
    pub(super) unsafe fn build_pricing(&mut self) -> Result<()> {
        self.building_page = Some(PRICE_PAGE);
        let provider = self.add(
            PRICE_PROVIDER,
            w!("COMBOBOX"),
            "",
            WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
            [244, 130, 160, 230],
            0,
        )?;
        for title in ["全部提供商", "ChatGPT", "Claude", "Grok"] {
            SendMessageW(provider, CB_ADDSTRING, 0, wide(title).as_ptr() as isize);
        }
        SendMessageW(provider, CB_SETCURSEL, 0, 0);
        self.edit(PRICE_SEARCH, "", [420, 130, -592, 36], 0, "搜索官方模型")?;
        self.label(
            "美元 / 百万 Token · 官方基准价，实际费用另乘账号池倍率",
            [260, 184, -312, 24],
            2,
        )?;
        self.add(
            PRICE_COUNT,
            w!("STATIC"),
            "完整官方价格簿 · 独立于账号池",
            SS_LEFT,
            [260, 212, -312, 22],
            2,
        )?;
        self.table(
            PRICES,
            &[
                ("模型", 220),
                ("提供商", 96),
                ("计价条件", 210),
                ("输入 $", 88),
                ("输出 $", 88),
                ("缓存读 $", 88),
            ],
        )?;
        let empty = self.add(
            PRICE_EMPTY,
            w!("STATIC"),
            "尚未读取官方价格\r\n\r\n完成连接设置后，点击“刷新价格”。",
            SS_LEFT,
            [292, 278, -376, 100],
            0,
        )?;
        SetWindowPos(
            empty,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        self.label("价格详情与来源", [260, -254, -312, 24], 4)?;
        self.add(
            PRICE_DETAILS,
            w!("EDIT"),
            "选择一行查看缓存写入价格、更新时间与官方来源。",
            WS_TABSTOP
                | WS_VSCROLL
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_READONLY as u32,
            [260, -220, -312, 128],
            2,
        )?;
        self.add(
            PRICE_STATUS,
            w!("STATIC"),
            "价格显示不代表当前账号池可调用该模型。",
            SS_LEFT,
            [244, -74, -280, 22],
            2,
        )?;
        self.building_page = None;
        Ok(())
    }
    pub(super) unsafe fn render_prices(&mut self) {
        let provider = SendMessageW(self.by_id[&PRICE_PROVIDER], CB_GETCURSEL, 0, 0);
        let provider = match provider {
            1 => "ChatGPT",
            2 => "Claude",
            3 => "Grok",
            _ => "",
        };
        let search = self.text(PRICE_SEARCH).trim().to_lowercase();
        self.price_rows = self
            .price_book
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                (provider.is_empty() || r.provider == provider)
                    && (search.is_empty() || r.model.to_lowercase().contains(&search))
            })
            .map(|(i, _)| i)
            .collect();
        let rows: Vec<_> = self
            .price_rows
            .iter()
            .map(|i| {
                let r = &self.price_book.rows[*i];
                vec![
                    r.model.as_str(),
                    r.provider,
                    &r.condition,
                    &r.input,
                    &r.output,
                    &r.cache_read,
                ]
            })
            .collect();
        fill(self.by_id[&PRICES], &rows);
        self.set(
            PRICE_COUNT,
            &if self.prices_loaded {
                format!(
                    "价格簿 v{} · 全部 {} 个模型 · 当前 {} 个计价档位",
                    self.price_book.revision,
                    self.price_book.models,
                    rows.len()
                )
            } else {
                "完整官方价格簿 · 独立于账号池".into()
            },
        );
        self.set(
            PRICE_EMPTY,
            if self.prices_loaded {
                if self.price_book.rows.is_empty() {
                    "Router 尚未发布官方价格。"
                } else {
                    "没有匹配的官方模型\r\n\r\n请修改搜索或提供商筛选。"
                }
            } else {
                "尚未读取官方价格\r\n\r\n完成连接设置后，点击“刷新价格”。"
            },
        );
        if !rows.is_empty() {
            let state = LVITEMW {
                state: LVIS_SELECTED | LVIS_FOCUSED,
                stateMask: LVIS_SELECTED | LVIS_FOCUSED,
                ..mem::zeroed()
            };
            SendMessageW(
                self.by_id[&PRICES],
                LVM_SETITEMSTATE,
                0,
                (&state as *const LVITEMW) as isize,
            );
        }
        self.price_selection_changed();
        self.update_visibility();
    }
    pub(super) unsafe fn price_selection_changed(&self) {
        let row = selected(self.by_id[&PRICES])
            .and_then(|i| self.price_rows.get(i))
            .and_then(|i| self.price_book.rows.get(*i));
        self.set(
            PRICE_DETAILS,
            &row.map(|r| {
                format!(
                    "{} / {} · {}\r\n{}",
                    r.provider, r.model, r.condition, r.details
                )
            })
            .unwrap_or_else(|| "选择一行查看缓存写入价格、更新时间与官方来源。".into()),
        );
    }
    pub(super) unsafe fn invalidate_prices(&mut self) {
        self.price_book = Default::default();
        self.prices_loaded = false;
        self.render_prices();
        self.set(PRICE_STATUS, "连接设置已修改，请刷新官方价格。");
    }
    pub(super) unsafe fn refresh_prices(&mut self) -> Result<()> {
        if self.inventory_task.is_some() || self.stopping || (self.task.is_some() && !self.running)
        {
            return Ok(());
        }
        let (profile, secrets) = self.snapshot()?;
        ensure!(
            !secrets.router_key.is_empty(),
            "请先在连接设置填写 Router key"
        );
        let config = profile.config(&secrets, self.path.parent().unwrap())?;
        self.inventory_generation += 1;
        self.inventory_finished_observed = false;
        let generation = self.inventory_generation;
        let sender = self.sender.clone();
        self.inventory_task = Some(self.runtime.spawn(async move {
            let result = pricing::fetch(&config, &secrets.router_key)
                .await
                .map_err(|e| redact(&format!("{e:#}"), &secrets));
            let _ = sender.send(Event::Prices(generation, result));
        }));
        self.set(PRICE_STATUS, "正在通过 TEE 验证通道读取完整官方价格…");
        self.busy(self.task.is_some());
        Ok(())
    }
}
