#![allow(unsafe_op_in_unsafe_fn)]
use super::*;

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
            (MODELS, &[250, 110, 160, 230][..]),
            (ACCOUNTS, &[108, 112, 112, 145, 96, 82, 94, 66][..]),
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
        self.building_page = Some(MODEL_PAGE);
        self.edit(
            SEARCH,
            "",
            [244, 130, -568, 36],
            0,
            "搜索模型、供应商或账号池",
        )?;
        self.button(COPY_MODEL, "复制模型 ID", [-308, 130, 136, 36], 0)?;
        self.table(
            MODELS,
            &[
                ("模型 ID", 250),
                ("供应商", 110),
                ("账号池", 160),
                ("接口", 230),
            ],
        )?;
        let empty = self.add(
            MODELS_EMPTY,
            w!("STATIC"),
            "尚未加载模型\r\n\r\n请先完成连接设置，然后点击刷新列表。",
            windows_sys::Win32::System::SystemServices::SS_CENTER,
            [284, 332, -360, 100],
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
        self.building_page = Some(ACCOUNT_PAGE);
        self.button(USE_POOL, "使用所选账号池", [244, 130, 168, 36], 0)?;
        self.table(
            ACCOUNTS,
            &[
                ("账号池", 108),
                ("价格倍率", 112),
                ("商户", 112),
                ("账号", 145),
                ("供应商", 96),
                ("状态", 82),
                ("模型目录", 94),
                ("模型数", 66),
            ],
        )?;
        let empty = self.add(
            ACCOUNTS_EMPTY,
            w!("STATIC"),
            "尚未加载账号池\r\n\r\n验证连接后，可查看上游账号及可用状态。",
            windows_sys::Win32::System::SystemServices::SS_CENTER,
            [284, 332, -360, 100],
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
        self.building_page = None;
        Ok(())
    }
    unsafe fn table(&mut self, id: u16, columns: &[(&str, i32)]) -> Result<()> {
        let hwnd = self.add(
            id,
            WC_LISTVIEWW,
            "",
            WS_TABSTOP | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
            [260, 198, -312, -300],
            0,
        )?;
        SendMessageW(
            hwnd,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize,
        );
        SetWindowTheme(hwnd, w!("Explorer"), ptr::null());
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
                    a.pool_name.as_str(),
                    a.multiplier.as_str(),
                    a.merchant.as_str(),
                    a.id.as_str(),
                    a.provider.as_str(),
                    a.state.as_str(),
                    a.catalog.as_str(),
                    a.models.as_str(),
                ]
            })
            .collect::<Vec<_>>();
        fill(self.by_id[&ACCOUNTS], &accounts);
        self.set(
            MODELS_EMPTY,
            if self.catalog_loaded {
                "没有匹配的模型\r\n\r\n调整搜索条件，或刷新当前可访问的模型。"
            } else {
                "尚未加载模型\r\n\r\n请先完成连接设置，然后点击刷新列表。"
            },
        );
        self.set(
            ACCOUNTS_EMPTY,
            if self.catalog_loaded {
                "没有可见的账号池\r\n\r\n当前业务密钥没有可显示的上游账号。"
            } else {
                "尚未加载账号池\r\n\r\n验证连接后，可查看上游账号及可用状态。"
            },
        );
        self.update_summary();
        self.update_visibility();
    }
    pub(super) unsafe fn invalidate_catalog(&mut self) {
        self.catalog = Default::default();
        self.catalog_loaded = false;
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
                self.select_page(SETTINGS_PAGE);
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
                    375
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
                assert_eq!(state.page, SETTINGS_PAGE);
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
