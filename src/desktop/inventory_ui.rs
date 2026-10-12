#![allow(unsafe_op_in_unsafe_fn)]
use super::*;

pub(super) const TABS: u16 = 301;
pub(super) const MODELS: u16 = 302;
pub(super) const ACCOUNTS: u16 = 303;
pub(super) const SEARCH: u16 = 304;
pub(super) const COPY_MODEL: u16 = 305;
pub(super) const USE_POOL: u16 = 306;
pub(super) const REFRESH: u16 = 307;
pub(super) const CATALOG_STATUS: u16 = 308;

impl App {
    pub(super) unsafe fn scale_inventory_columns(&self) {
        for (id, widths) in [
            (MODELS, &[300, 130, 190, 310][..]),
            (ACCOUNTS, &[170, 220, 130, 110, 120, 90][..]),
        ] {
            if let Some(hwnd) = self.by_id.get(&id) {
                for (column, width) in widths.iter().enumerate() {
                    SendMessageW(
                        *hwnd,
                        LVM_SETCOLUMNWIDTH,
                        column,
                        self.scale(*width) as isize,
                    );
                }
            }
        }
    }
    pub(super) unsafe fn build_inventory(&mut self) -> Result<()> {
        self.building_page = None;
        let tabs = self.add(TABS, WC_TABCONTROLW, "", WS_TABSTOP, [24, 216, -48, 32], 0)?;
        for (index, label) in ["模型", "账号池", "连接设置", "运行记录"]
            .into_iter()
            .enumerate()
        {
            let mut label = wide(label);
            let item = TCITEMW {
                mask: TCIF_TEXT,
                pszText: label.as_mut_ptr(),
                ..mem::zeroed()
            };
            SendMessageW(
                tabs,
                TCM_INSERTITEMW,
                index,
                (&item as *const TCITEMW) as isize,
            );
        }
        self.button(REFRESH, "刷新模型和账号", [512, 164, 156, 34], 0)?;
        self.add(
            CATALOG_STATUS,
            w!("STATIC"),
            "尚未读取列表",
            SS_LEFT,
            [684, 171, -708, 22],
            2,
        )?;
        self.label(
            "最小化或关闭窗口后留在系统托盘；右键托盘图标可打开、停止或退出。",
            [24, -32, -48, 22],
            2,
        )?;
        self.building_page = Some(0);
        self.label("筛选模型", [24, 271, 76, 24], 0)?;
        self.edit(
            SEARCH,
            "",
            [104, 267, 360, 30],
            0,
            "模型 ID、供应商或账号池",
        )?;
        self.button(COPY_MODEL, "复制模型 ID", [-160, 267, 136, 30], 0)?;
        self.table(
            MODELS,
            &[
                ("模型 ID", 300),
                ("供应商", 130),
                ("账号池", 190),
                ("接口", 310),
            ],
        )?;
        self.building_page = Some(1);
        self.label(
            "当前 key 可访问的上游账号；状态来自 router。",
            [24, 271, -230, 24],
            0,
        )?;
        self.button(USE_POOL, "使用所选账号池", [-188, 267, 164, 30], 0)?;
        self.table(
            ACCOUNTS,
            &[
                ("账号池", 170),
                ("账号", 220),
                ("供应商", 130),
                ("状态", 110),
                ("模型目录", 120),
                ("模型数", 90),
            ],
        )?;
        self.building_page = None;
        Ok(())
    }
    unsafe fn table(&mut self, id: u16, columns: &[(&str, i32)]) -> Result<()> {
        let hwnd = self.add(
            id,
            WC_LISTVIEWW,
            "",
            WS_TABSTOP | WS_BORDER | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
            [24, 312, -48, -364],
            0,
        )?;
        SendMessageW(
            hwnd,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_GRIDLINES) as isize,
        );
        for (index, (label, width)) in columns.iter().enumerate() {
            let mut label = wide(label);
            let column = LVCOLUMNW {
                mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
                cx: self.scale(*width),
                pszText: label.as_mut_ptr(),
                iSubItem: index as i32,
                ..mem::zeroed()
            };
            SendMessageW(
                hwnd,
                LVM_INSERTCOLUMNW,
                index,
                (&column as *const LVCOLUMNW) as isize,
            );
        }
        Ok(())
    }
    pub(super) unsafe fn select_page(&mut self, page: usize) {
        self.page = page.min(3);
        SendMessageW(self.by_id[&TABS], TCM_SETCURSEL, self.page, 0);
        for control in &self.controls {
            ShowWindow(
                control.hwnd,
                if control.page.is_none_or(|p| p == self.page) {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
        }
        self.layout();
        // Tab navigation remains restricted to the visible page's native controls.
        SetFocus(self.by_id[&TABS]);
    }
    pub(super) unsafe fn render_catalog(&mut self) {
        let filter = self.text(SEARCH).to_lowercase();
        self.model_rows = self
            .catalog
            .models
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                format!("{} {} {}", m.id, m.provider, m.pools)
                    .to_lowercase()
                    .contains(&filter)
            })
            .map(|(i, _)| i)
            .collect();
        let models = self
            .model_rows
            .iter()
            .map(|index| {
                let row = &self.catalog.models[*index];
                vec![
                    row.id.as_str(),
                    row.provider.as_str(),
                    row.pools.as_str(),
                    row.protocols.as_str(),
                ]
            })
            .collect::<Vec<_>>();
        fill(self.by_id[&MODELS], &models);
        let accounts = self
            .catalog
            .accounts
            .iter()
            .map(|a| {
                vec![
                    a.pool.as_str(),
                    a.id.as_str(),
                    a.provider.as_str(),
                    a.state.as_str(),
                    a.catalog.as_str(),
                    a.models.as_str(),
                ]
            })
            .collect::<Vec<_>>();
        fill(self.by_id[&ACCOUNTS], &accounts);
    }
    pub(super) unsafe fn invalidate_catalog(&mut self) {
        self.catalog = Default::default();
        self.render_catalog();
        self.set(CATALOG_STATUS, "配置已修改 · 请刷新列表");
    }
    pub(super) unsafe fn refresh_catalog(&mut self) -> Result<()> {
        if self.inventory_task.is_some() || self.stopping || (self.task.is_some() && !self.running)
        {
            return Ok(());
        }
        let (profile, secrets) = self.snapshot()?;
        let config = profile.config(&secrets, self.path.parent().unwrap())?;
        ensure!(
            !secrets.router_key.is_empty(),
            "请先在连接设置填写 Router key"
        );
        self.inventory_generation += 1;
        self.inventory_finished_observed = false;
        let generation = self.inventory_generation;
        let sender = self.sender.clone();
        self.inventory_task = Some(self.runtime.spawn(async move {
            let result = catalog::fetch(&config, &secrets.router_key)
                .await
                .map_err(|e| redact(&format!("{e:#}"), &secrets));
            let _ = sender.send(Event::Inventory(generation, result));
        }));
        self.set(CATALOG_STATUS, "正在验证并刷新列表…");
        self.busy(self.task.is_some());
        Ok(())
    }
    pub(super) unsafe fn inventory_action(&mut self, id: u16) -> Result<()> {
        match id {
            REFRESH => self.refresh_catalog()?,
            COPY_MODEL => {
                let index = selected(self.by_id[&MODELS])
                    .and_then(|i| self.model_rows.get(i))
                    .context("请先选择一个模型")?;
                copy_text(self.hwnd, &self.catalog.models[*index].id)?;
                self.append("模型 ID 已复制，可填入 agent 的模型配置。");
            }
            USE_POOL => {
                ensure!(
                    self.task.is_none() && self.inventory_task.is_none(),
                    "请先停止网关，再切换账号池"
                );
                let row = selected(self.by_id[&ACCOUNTS])
                    .and_then(|i| self.catalog.accounts.get(i))
                    .context("请先选择一个账号")?;
                let pool = row.pool.clone();
                self.set(POOL, &pool);
                self.append(&format!(
                    "已选择账号池 {pool}；下次启动生效，保存配置可长期保留。"
                ));
                self.select_page(2);
            }
            _ => {}
        }
        Ok(())
    }
}
unsafe fn selected(hwnd: HWND) -> Option<usize> {
    let index = SendMessageW(hwnd, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize);
    (index >= 0).then_some(index as usize)
}
unsafe fn fill(hwnd: HWND, rows: &[Vec<&str>]) {
    // WM_SETREDRAW(TRUE) sets WS_VISIBLE, so never use it on a hidden page.
    let shown = GetWindowLongPtrW(hwnd, GWL_STYLE) & WS_VISIBLE as isize != 0;
    if shown {
        SendMessageW(hwnd, WM_SETREDRAW, 0, 0);
    }
    SendMessageW(hwnd, LVM_DELETEALLITEMS, 0, 0);
    for (index, row) in rows.iter().enumerate() {
        for (column, text) in row.iter().enumerate() {
            let mut text = wide(text);
            let item = LVITEMW {
                mask: LVIF_TEXT,
                iItem: index as i32,
                iSubItem: column as i32,
                pszText: text.as_mut_ptr(),
                ..mem::zeroed()
            };
            SendMessageW(
                hwnd,
                if column == 0 {
                    LVM_INSERTITEMW
                } else {
                    LVM_SETITEMW
                },
                0,
                (&item as *const LVITEMW) as isize,
            );
        }
    }
    if shown {
        SendMessageW(hwnd, WM_SETREDRAW, 1, 0);
    }
    InvalidateRect(hwnd, ptr::null(), 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_inventory_renders_filters_and_selects_the_pool() {
        // Synthetic rows are injected only in this unit test, never via a runtime option.
        let app = RefCell::new(
            App::new(std::env::temp_dir().join("teeproxy-inventory-unit-unused.json")).unwrap(),
        );
        unsafe {
            let controls = INITCOMMONCONTROLSEX {
                dwSize: mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                // Rust's library test executable does not embed the GUI's v6 manifest.
                // Standard EDIT/BUTTON classes already exist; load the tables and tabs.
                dwICC: ICC_LISTVIEW_CLASSES | ICC_TAB_CLASSES,
            };
            assert_ne!(InitCommonControlsEx(&controls), 0);
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: GetModuleHandleW(ptr::null()),
                lpszClassName: w!("teeproxy.InventoryTest"),
                ..mem::zeroed()
            };
            assert_ne!(RegisterClassW(&class), 0);
            let hwnd = CreateWindowExW(
                0,
                class.lpszClassName,
                w!("Inventory unit test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                1020,
                720,
                ptr::null_mut(),
                ptr::null_mut(),
                class.hInstance,
                (&app as *const RefCell<App>).cast(),
            );
            assert!(!hwnd.is_null());
            {
                let mut state = app.borrow_mut();
                state.hwnd = hwnd;
                state
                    .build(Ok((Profile::default(), Secrets::default())))
                    .unwrap();
                state.catalog = catalog::parse(
                    &serde_json::json!({"data":[{"id":"alpha","router_pools":["pool-a"]},{"id":"beta","router_pools":["pool-b"]}]}),
                    &serde_json::json!({"data":[{"id":"pool-b","accounts":[{"id":"account-b","enabled":true,"available":false}]}]})
                ).unwrap();
                state.render_catalog();
                state.dpi = 144;
                state.refresh_fonts();
                assert_eq!(
                    SendMessageW(state.by_id[&MODELS], LVM_GETCOLUMNWIDTH, 0, 0),
                    450
                );
                state.dpi = 96;
                state.refresh_fonts();
                assert_eq!(
                    GetWindowLongPtrW(state.by_id[&MODELS], GWL_STYLE) & WS_VISIBLE as isize,
                    0
                );
                assert_eq!(
                    GetWindowLongPtrW(state.by_id[&ACCOUNTS], GWL_STYLE) & WS_VISIBLE as isize,
                    0
                );
                assert_eq!(
                    SendMessageW(state.by_id[&MODELS], LVM_GETITEMCOUNT, 0, 0),
                    2
                );
                assert_eq!(
                    SendMessageW(state.by_id[&ACCOUNTS], LVM_GETITEMCOUNT, 0, 0),
                    1
                );
                state.set(SEARCH, "POOL-B");
                state.render_catalog();
                assert_eq!(
                    SendMessageW(state.by_id[&MODELS], LVM_GETITEMCOUNT, 0, 0),
                    1
                );
                assert_eq!(state.catalog.models[state.model_rows[0]].id, "beta");
                let selection = LVITEMW {
                    state: LVIS_SELECTED,
                    stateMask: LVIS_SELECTED,
                    ..mem::zeroed()
                };
                SendMessageW(
                    state.by_id[&ACCOUNTS],
                    LVM_SETITEMSTATE,
                    0,
                    (&selection as *const LVITEMW) as isize,
                );
                state.inventory_action(USE_POOL).unwrap();
                assert_eq!(state.text(POOL), "pool-b");
                assert_eq!(state.page, 2);
                let stale_catalog = mem::take(&mut state.catalog);
                state.invalidate_catalog();
                state.inventory_generation = 10;
                state.inventory_task = Some(state.runtime.spawn(std::future::pending::<()>()));
                state.busy(false);
                assert_eq!(IsWindowEnabled(state.by_id[&ROUTER]), 0);
                state.stop();
                state
                    .sender
                    .send(Event::Inventory(10, Ok(stale_catalog)))
                    .unwrap();
                state.poll();
                assert_ne!(IsWindowEnabled(state.by_id[&ROUTER]), 0);
                assert_eq!(
                    SendMessageW(state.by_id[&MODELS], LVM_GETITEMCOUNT, 0, 0),
                    0
                );
                assert_eq!(
                    SendMessageW(state.by_id[&ACCOUNTS], LVM_GETITEMCOUNT, 0, 0),
                    0
                );
                state.tray.take();
                DestroyWindow(hwnd);
                for font in state.fonts {
                    DeleteObject(font);
                }
            }
            UnregisterClassW(class.lpszClassName, class.hInstance);
        }
    }
}
