#![allow(unsafe_op_in_unsafe_fn)]
use super::*;

pub(super) const MODELS: u16 = 302;
pub(super) const ACCOUNTS: u16 = 303;
pub(super) const SEARCH: u16 = 304;
pub(super) const COPY_MODEL: u16 = 305;
pub(super) const USE_POOL: u16 = 306;
pub(super) const REFRESH: u16 = 307;
pub(super) const CATALOG_STATUS: u16 = 308;
pub(super) const POOL_DETAILS: u16 = 310;
pub(super) const POOLS: u16 = 312;
pub(super) const POOLS_EMPTY: u16 = 313;
pub(super) const ACCOUNTS_TITLE: u16 = 314;
pub(super) const POOLS_TITLE: u16 = 315;
pub(super) const MODELS_TITLE: u16 = 316;

pub(super) fn inventory_cards(width: i32, height: i32) -> [[i32; 4]; 3] {
    let left = (width - 296) * 44 / 100;
    [
        [244, 186, width - 280, 222],
        [244, 456, left, height - 544],
        [260 + left, 456, width - 296 - left, height - 544],
    ]
}

pub(super) fn inventory_rect(id: u16, width: i32, height: i32) -> Option<[i32; 4]> {
    let [_, accounts, models] = inventory_cards(width, height);
    let card = match id {
        ACCOUNTS | ACCOUNTS_TITLE | ACCOUNTS_EMPTY => accounts,
        MODELS | MODELS_TITLE | MODELS_EMPTY => models,
        _ => return None,
    };
    Some(match id {
        ACCOUNTS_TITLE | MODELS_TITLE => [card[0] + 16, 470, card[2] - 32, 24],
        ACCOUNTS_EMPTY | MODELS_EMPTY => [card[0] + 24, 556, card[2] - 48, 100],
        _ => [card[0] + 16, 506, card[2] - 32, height - 610],
    })
}

impl App {
    pub(super) unsafe fn scale_inventory_columns(&self) {
        for (id, widths) in [
            (MODELS, &[282, 124][..]),
            (POOLS, &[144, 120, 104, 84, 152, 124][..]),
            (ACCOUNTS, &[104, 72, 78, 64][..]),
            (PRICES, &[220, 96, 210, 88, 88, 88][..]),
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
        self.building_page = Some(INVENTORY_PAGE);
        self.button(USE_POOL, "使用所选账号池", [244, 130, 156, 36], 0)?;
        self.edit(SEARCH, "", [416, 130, -820, 36], 0, "搜索所选池的模型")?;
        self.button(COPY_MODEL, "使用此池并复制模型", [-388, 130, 216, 36], 0)?;
        self.add(
            POOL_DETAILS,
            w!("STATIC"),
            "请选择账号池",
            SS_LEFT,
            [244, 420, -280, 24],
            2,
        )?;
        self.add(
            MODELS_TITLE,
            w!("STATIC"),
            "模型 · 请先选择账号池",
            SS_LEFT,
            [260, 470, 300, 24],
            4,
        )?;
        self.table(MODELS, &[("模型 ID", 282), ("接口", 124)])?;
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
        self.add(
            POOLS_TITLE,
            w!("STATIC"),
            "账号池",
            SS_LEFT,
            [260, 200, -312, 24],
            2,
        )?;
        self.table(
            POOLS,
            &[
                ("账号池", 144),
                ("商户", 120),
                ("价格倍率", 104),
                ("账号数量", 84),
                ("模型提供商", 152),
                ("状态", 124),
            ],
        )?;
        self.add(
            ACCOUNTS_TITLE,
            w!("STATIC"),
            "账号 · 请先选择账号池",
            SS_LEFT,
            [260, 426, -312, 24],
            4,
        )?;
        self.table(
            ACCOUNTS,
            &[
                ("账号", 104),
                ("状态", 72),
                ("模型目录", 78),
                ("模型数", 64),
            ],
        )?;
        let empty = self.add(
            POOLS_EMPTY,
            w!("STATIC"),
            "尚未加载账号池\r\n\r\n请完成连接设置，然后点击刷新列表。",
            windows_sys::Win32::System::SystemServices::SS_CENTER,
            [284, 280, -360, 80],
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
        let empty = self.add(
            ACCOUNTS_EMPTY,
            w!("STATIC"),
            "请选择上方的账号池\r\n\r\n此处只显示所选池中的账号。",
            windows_sys::Win32::System::SystemServices::SS_CENTER,
            [284, 518, -360, 80],
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
    pub(super) unsafe fn table(&mut self, id: u16, columns: &[(&str, i32)]) -> Result<()> {
        let hwnd = self.add(
            id,
            WC_LISTVIEWW,
            "",
            WS_TABSTOP | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
            match id {
                POOLS => [260, 234, -312, 158],
                PRICES => [260, 236, -312, -510],
                _ => [260, 460, -312, -560],
            },
            0,
        )?;
        SendMessageW(
            hwnd,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP) as isize,
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
    pub(super) unsafe fn model_pool(&self) -> Option<&catalog::Pool> {
        self.account_pool()
    }
    pub(super) unsafe fn replace_catalog(&mut self, catalog: catalog::Catalog) {
        let preferred = self.account_pool_id.clone().or_else(|| {
            let id = self.configured_pool.clone();
            (!id.is_empty()).then_some(id)
        });
        self.catalog = catalog;
        self.catalog_loaded = true;
        // Preserve by full ID, never silently switch merchants when a pool disappears.
        self.render_pools(preferred.as_deref());
        self.refresh_route_pools();
        self.render_catalog();
    }
    pub(super) fn account_pool(&self) -> Option<&catalog::Pool> {
        self.catalog
            .pools
            .iter()
            .find(|p| Some(&p.id) == self.account_pool_id.as_ref())
    }
    fn pool_providers(&self, pool: &catalog::Pool) -> String {
        if !pool.provider.is_empty() {
            return pool.provider.clone();
        }
        self.catalog
            .models
            .iter()
            .filter(|m| m.pool == pool.id)
            .map(|m| m.provider.as_str())
            .chain(
                self.catalog
                    .accounts
                    .iter()
                    .filter(|a| a.pool == pool.id)
                    .map(|a| a.provider.as_str()),
            )
            .filter(|p| !p.is_empty())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(" / ")
    }
    unsafe fn render_pools(&mut self, preferred: Option<&str>) {
        let rows = self
            .catalog
            .pools
            .iter()
            .map(|p| {
                vec![
                    p.name.clone(),
                    p.merchant.clone(),
                    p.multiplier.clone(),
                    p.account_count.to_string(),
                    p.provider.clone(),
                    p.state.clone(),
                ]
            })
            .collect::<Vec<_>>();
        fill(self.by_id[&POOLS], &rows);
        let index = if let Some(id) = preferred {
            self.catalog.pools.iter().position(|p| p.id == id)
        } else {
            (!self.catalog.pools.is_empty()).then_some(0)
        };
        self.account_pool_id = index.map(|i| self.catalog.pools[i].id.clone());
        if let Some(index) = index {
            let selection = LVITEMW {
                state: LVIS_SELECTED | LVIS_FOCUSED,
                stateMask: LVIS_SELECTED | LVIS_FOCUSED,
                ..mem::zeroed()
            };
            SendMessageW(
                self.by_id[&POOLS],
                LVM_SETITEMSTATE,
                index,
                (&selection as *const LVITEMW) as isize,
            );
            SendMessageW(self.by_id[&POOLS], LVM_ENSUREVISIBLE, index, 0);
        }
        self.set(
            POOLS_TITLE,
            &format!("账号池 · {} 个", self.catalog.pools.len()),
        );
        self.set(
            POOLS_EMPTY,
            if self.catalog_loaded {
                "没有可见的账号池\r\n\r\n当前业务密钥没有可显示的账号池。"
            } else {
                "尚未加载账号池\r\n\r\n请完成连接设置，然后点击刷新列表。"
            },
        );
        self.render_accounts();
    }
    pub(super) unsafe fn pool_selection_changed(&mut self) {
        self.account_pool_id = selected(self.by_id[&POOLS])
            .and_then(|i| self.catalog.pools.get(i))
            .map(|p| p.id.clone());
        self.render_accounts();
        self.set(SEARCH, "");
        self.render_catalog();
    }
    unsafe fn render_accounts(&self) {
        let accounts = self
            .catalog
            .accounts
            .iter()
            .filter(|a| Some(&a.pool) == self.account_pool_id.as_ref())
            .map(|a| {
                vec![
                    a.id.as_str(),
                    a.state.as_str(),
                    a.catalog.as_str(),
                    a.models.as_str(),
                ]
            })
            .collect::<Vec<_>>();
        fill(self.by_id[&ACCOUNTS], &accounts);
        self.set(
            ACCOUNTS_TITLE,
            &if let Some(pool) = self.account_pool() {
                format!("账号 · {} 个", pool.account_count)
            } else {
                "账号 · 请先选择账号池".into()
            },
        );
        self.set(
            ACCOUNTS_EMPTY,
            if self.account_pool().is_some() {
                "此账号池暂无账号"
            } else {
                "请选择上方的账号池\r\n\r\n此处只显示所选池中的账号。"
            },
        );
    }
    pub(super) unsafe fn render_catalog(&mut self) {
        let filter = self.text(SEARCH).to_lowercase();
        let pool = self.model_pool().map(|p| p.id.clone());
        self.model_rows = self
            .catalog
            .models
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                Some(&m.pool) == pool.as_ref() && m.id.to_lowercase().contains(&filter)
            })
            .map(|(i, _)| i)
            .collect();
        let models = self
            .model_rows
            .iter()
            .map(|index| {
                let row = &self.catalog.models[*index];
                vec![row.id.as_str(), row.protocols.as_str()]
            })
            .collect::<Vec<_>>();
        fill(self.by_id[&MODELS], &models);
        self.set(
            MODELS_EMPTY,
            &if self.catalog_loaded {
                if let Some(pool) = self.model_pool() {
                    if filter.is_empty() {
                        format!(
                            "此账号池暂无可见模型\r\n\r\n{} · {}。请查看左侧账号状态。",
                            pool.name, pool.state
                        )
                    } else {
                        "此账号池没有匹配的模型\r\n\r\n清除搜索条件，或选择其他账号池。".into()
                    }
                } else {
                    "请选择账号池\r\n\r\n原选择的池若已不可见，请手动选择其他池。".into()
                }
            } else {
                "尚未加载模型\r\n\r\n请先完成连接设置，然后点击刷新列表。".into()
            },
        );
        self.set(
            MODELS_TITLE,
            &format!("模型 · {} 个", self.model_rows.len()),
        );
        self.set(
            POOL_DETAILS,
            &if let Some(pool) = self.model_pool() {
                format!(
                    "{} · {} · {} · 倍率 {} · {} · {} / {} 个模型",
                    pool.name,
                    pool.merchant,
                    self.pool_providers(pool),
                    pool.multiplier,
                    pool.state,
                    self.model_rows.len(),
                    self.catalog
                        .models
                        .iter()
                        .filter(|m| m.pool == pool.id)
                        .count()
                )
            } else {
                "请选择账号池".into()
            },
        );
        self.update_summary();
        self.update_visibility();
    }
    pub(super) unsafe fn invalidate_catalog(&mut self) {
        self.catalog = Default::default();
        self.catalog_loaded = false;
        self.render_pools(None);
        self.refresh_route_pools();
        self.render_catalog();
        self.set(CATALOG_STATUS, "配置已修改 · 请刷新列表");
    }
    unsafe fn select_inventory_pool(&mut self, pool: &str) -> Result<()> {
        ensure!(
            self.inventory_task.is_none()
                && !self.stopping
                && (self.task.is_none() || self.running),
            "请等待当前操作完成后再切换账号池"
        );
        let (mut profile, secrets) = self.snapshot()?;
        profile.pool = pool.into();
        profile.routing.automatic = false;
        self.save_profile(&profile, &secrets)?;
        self.configured_pool = pool.into();
        SendMessageW(self.by_id[&ROUTE_MODE], CB_SETCURSEL, 0, 0);
        self.refresh_route_pools();
        self.update_summary();
        Ok(())
    }
    unsafe fn use_model(&mut self, index: usize) -> Result<String> {
        let model = self
            .catalog
            .models
            .get(index)
            .context("请刷新模型列表后重试")?;
        ensure!(
            self.model_pool().is_some_and(|p| p.id == model.pool),
            "模型不属于当前选择的账号池"
        );
        let (pool, id) = (model.pool.clone(), model.id.clone());
        self.select_inventory_pool(&pool)?;
        Ok(id)
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
                let model = self.use_model(*index)?;
                copy_text(self.hwnd, &model)?;
                let pool = self.model_pool().unwrap();
                let message = format!(
                    "已保存账号池 {}，已复制模型 {}{}",
                    pool.name,
                    model,
                    if self.running {
                        "；新请求立即生效。"
                    } else {
                        "；启动代理后生效。"
                    }
                );
                self.set(CATALOG_STATUS, &message);
                self.append(&message);
            }
            USE_POOL => {
                let pool = self
                    .account_pool()
                    .context("请先选择一个账号池")?
                    .id
                    .clone();
                self.select_inventory_pool(&pool)?;
                let message = format!(
                    "已保存账号池 {pool}；{}。",
                    if self.running {
                        "新请求立即生效"
                    } else {
                        "下次启动生效"
                    }
                );
                self.append(&message);
                self.set(CATALOG_STATUS, &message);
            }
            _ => {}
        }
        Ok(())
    }
}
pub(super) unsafe fn selected(hwnd: HWND) -> Option<usize> {
    let index = SendMessageW(hwnd, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize);
    (index >= 0).then_some(index as usize)
}
pub(super) unsafe fn fill<T: AsRef<str>>(hwnd: HWND, rows: &[Vec<T>]) {
    // WM_SETREDRAW(TRUE) sets WS_VISIBLE, so never use it on a hidden page.
    let shown = GetWindowLongPtrW(hwnd, GWL_STYLE) & WS_VISIBLE as isize != 0;
    if shown {
        SendMessageW(hwnd, WM_SETREDRAW, 0, 0);
    }
    SendMessageW(hwnd, LVM_DELETEALLITEMS, 0, 0);
    for (index, row) in rows.iter().enumerate() {
        for (column, text) in row.iter().enumerate() {
            let mut text = wide(text.as_ref());
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

    unsafe fn choose_pool(state: &mut App, index: usize) {
        let selection = LVITEMW {
            state: LVIS_SELECTED | LVIS_FOCUSED,
            stateMask: LVIS_SELECTED | LVIS_FOCUSED,
            ..mem::zeroed()
        };
        SendMessageW(
            state.by_id[&POOLS],
            LVM_SETITEMSTATE,
            index,
            (&selection as *const LVITEMW) as isize,
        );
        state.pool_selection_changed();
    }

    unsafe fn cell_text(hwnd: HWND, row: usize, column: i32) -> String {
        let mut buffer = [0u16; 256];
        let mut item = LVITEMW {
            iSubItem: column,
            pszText: buffer.as_mut_ptr(),
            cchTextMax: buffer.len() as i32,
            ..mem::zeroed()
        };
        let length = SendMessageW(
            hwnd,
            LVM_GETITEMTEXTW,
            row,
            (&mut item as *mut LVITEMW) as isize,
        );
        String::from_utf16_lossy(&buffer[..length as usize])
    }

    #[test]
    fn native_inventory_renders_filters_and_selects_the_pool() {
        // Synthetic rows are injected only in this unit test, never via a runtime option.
        let directory = std::env::temp_dir().join(format!(
            "teeproxy-inventory-{}",
            crate::protocol::random_hex().unwrap()
        ));
        let path = directory.join("desktop.json");
        let app = RefCell::new(App::new(path.clone()).unwrap());
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
                    .build(Ok((
                        Profile {
                            router: "https://router.example".into(),
                            pool: "pool-b".into(),
                            ..Profile::default()
                        },
                        Secrets::default(),
                    )))
                    .unwrap();
                // The price page must work before any account or pool exists.
                let initial_page = state.page;
                state.price_book = pricing::parse(&pricing::tests::fixture()).unwrap();
                state.prices_loaded = true;
                state.render_prices();
                state.select_page(PRICE_PAGE);
                assert!(state.catalog.pools.is_empty());
                assert_eq!(
                    SendMessageW(state.by_id[&PRICES], LVM_GETITEMCOUNT, 0, 0),
                    4
                );
                assert_eq!(cell_text(state.by_id[&PRICES], 0, 3), "1.25");
                assert!(state.text(PRICE_DETAILS).contains("官方来源"));
                assert_ne!(
                    GetWindowLongPtrW(state.by_id[&PRICES], GWL_STYLE) & WS_VISIBLE as isize,
                    0
                );
                assert_eq!(
                    GetWindowLongPtrW(state.by_id[&MODELS], GWL_STYLE) & WS_VISIBLE as isize,
                    0
                );
                SendMessageW(state.by_id[&PRICE_PROVIDER], CB_SETCURSEL, 3, 0);
                state.render_prices();
                assert_eq!(
                    SendMessageW(state.by_id[&PRICES], LVM_GETITEMCOUNT, 0, 0),
                    2
                );
                assert!(cell_text(state.by_id[&PRICES], 1, 2).contains("≥ 200000"));
                state.set(PRICE_SEARCH, "no-match");
                state.render_prices();
                assert!(state.price_rows.is_empty());
                state.set(PRICE_SEARCH, "");
                state.select_page(initial_page);
                let models = serde_json::json!({"data":[{"id":"chatgpt/alpha","router_provider":"openai","router_model":"alpha","router_pools":["pool-a"]},{"id":"chatgpt/beta","router_provider":"openai","router_model":"beta","router_pools":["pool-a","pool-b"]}]});
                let mut pools = serde_json::json!({"data":[
                    {"id":"pool-a","accounts":[{"id":"account-a","enabled":true,"available":true},{"id":"account-b","enabled":false,"available":false}]},
                    {"id":"pool-b","accounts":[{"id":"account-b","enabled":true,"available":false}]},
                    {"id":"pool-empty","accounts":[]}
                ]});
                let catalog = catalog::parse(&models, &pools).unwrap();
                state.replace_catalog(catalog);
                SendMessageW(state.by_id[&ROUTE_MODE], CB_SETCURSEL, 1, 0);
                state.routing_action(ROUTE_MODE).unwrap();
                state.set(ROUTE_MAX, "0.25");
                state.action(ROUTE_SAVE).unwrap();
                let saved_policy = settings::load(&state.path).unwrap().0.routing;
                assert!(saved_policy.automatic);
                assert_eq!(saved_policy.max_multiplier_millionths, 250000);
                assert_eq!(state.model_pool().unwrap().id, "pool-b");
                assert_eq!(state.account_pool().unwrap().id, "pool-b");
                assert_eq!(SendMessageW(state.by_id[&POOLS], LVM_GETITEMCOUNT, 0, 0), 3);
                assert_eq!(cell_text(state.by_id[&POOLS], 0, 3), "2");
                assert_eq!(cell_text(state.by_id[&POOLS], 1, 3), "1");
                assert_eq!(cell_text(state.by_id[&POOLS], 2, 3), "0");
                state.dpi = 144;
                state.refresh_fonts();
                assert_eq!(
                    SendMessageW(state.by_id[&MODELS], LVM_GETCOLUMNWIDTH, 0, 0),
                    423
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
                    1
                );
                assert_eq!(
                    SendMessageW(state.by_id[&ACCOUNTS], LVM_GETITEMCOUNT, 0, 0),
                    1
                );
                assert_eq!(cell_text(state.by_id[&ACCOUNTS], 0, 0), "account-b");
                assert_eq!(cell_text(state.by_id[&ACCOUNTS], 0, 1), "暂不可用");
                state.set(SEARCH, "BETA");
                state.render_catalog();
                assert_eq!(
                    SendMessageW(state.by_id[&MODELS], LVM_GETITEMCOUNT, 0, 0),
                    1
                );
                assert_eq!(state.catalog.models[state.model_rows[0]].id, "chatgpt/beta");
                let index = state.model_rows[0];
                assert_eq!(state.use_model(index).unwrap(), "chatgpt/beta");
                assert!(!settings::load(&state.path).unwrap().0.routing.automatic);
                assert_eq!(
                    settings::load(&state.path)
                        .unwrap()
                        .0
                        .routing
                        .max_multiplier_millionths,
                    250000
                );
                assert_ne!(IsWindowEnabled(state.by_id[&ROUTE_MAX]), 0);
                assert_eq!(IsWindowEnabled(state.by_id[&ROUTE_PRIORITY]), 0);
                assert_eq!(settings::load(&state.path).unwrap().0.pool, "pool-b");
                let (profile, secrets) = state.snapshot().unwrap();
                let proxy = Proxy::new(
                    profile
                        .config(&secrets, state.path.parent().unwrap())
                        .unwrap(),
                    client::Trust::default(),
                    "test-key",
                    None,
                )
                .unwrap();
                state.task = Some(state.runtime.spawn(std::future::pending::<()>()));
                state.running = true;
                state.live_proxy = Some(proxy.clone());
                state.busy(true);
                assert_ne!(IsWindowEnabled(state.by_id[&ROUTE_MODE]), 0);
                assert_ne!(IsWindowEnabled(state.by_id[&ROUTE_MAX]), 0);
                assert_ne!(IsWindowEnabled(state.by_id[&ROUTE_SAVE]), 0);
                assert_ne!(IsWindowEnabled(state.by_id[&USE_POOL]), 0);
                assert_eq!(IsWindowEnabled(state.by_id[&ROUTER]), 0);
                let original_rules = proxy.routing_rules();
                let original_file = std::fs::read(&state.path).unwrap();
                SendMessageW(state.by_id[&ROUTE_MODE], CB_SETCURSEL, 1, 0);
                state.routing_action(ROUTE_MODE).unwrap();
                assert!(state.text(ROUTE_CURRENT).starts_with("未保存"));
                assert_eq!(proxy.routing_rules(), original_rules);
                state.set(ROUTE_MAX, "NaN");
                assert!(state.action(ROUTE_SAVE).is_err());
                assert_eq!(proxy.routing_rules(), original_rules);
                assert_eq!(std::fs::read(&state.path).unwrap(), original_file);
                state.set(ROUTE_MAX, "0.2");
                let saved_path = state.path.clone();
                state.path = saved_path.join("cannot-create-under-file.json");
                assert!(state.action(ROUTE_SAVE).is_err());
                assert_eq!(proxy.routing_rules(), original_rules);
                state.path = saved_path;
                assert_eq!(std::fs::read(&state.path).unwrap(), original_file);
                state.action(ROUTE_SAVE).unwrap();
                assert!(state.text(ROUTE_CURRENT).starts_with("已生效"));
                assert!(proxy.routing_rules().policy.automatic);
                assert_eq!(
                    proxy.routing_rules().policy.max_multiplier_millionths,
                    200000
                );
                assert_eq!(
                    settings::load(&state.path).unwrap().0.routing,
                    proxy.routing_rules().policy
                );
                choose_pool(&mut state, 0);
                assert_eq!(state.model_rows.len(), 2);
                let index = state.model_rows[0];
                state.use_model(index).unwrap();
                assert_eq!(state.configured_pool, "pool-a");
                assert_eq!(settings::load(&state.path).unwrap().0.pool, "pool-a");
                assert_eq!(proxy.routing_rules().pool.as_deref(), Some("pool-a"));
                assert!(!proxy.routing_rules().policy.automatic);
                assert_eq!(
                    proxy.routing_rules().policy.max_multiplier_millionths,
                    200000
                );
                choose_pool(&mut state, 1);
                let index = state.model_rows[0];
                assert_eq!(state.use_model(index).unwrap(), "chatgpt/beta");
                state.inventory_action(USE_POOL).unwrap();
                assert_eq!(proxy.routing_rules().pool.as_deref(), Some("pool-b"));
                assert_eq!(settings::load(&state.path).unwrap().0.pool, "pool-b");
                assert_eq!(state.use_model(index).unwrap(), "chatgpt/beta");
                state.task.take().unwrap().abort();
                state.running = false;
                state.live_proxy = None;
                let selection = LVITEMW {
                    state: LVIS_SELECTED,
                    stateMask: LVIS_SELECTED,
                    ..mem::zeroed()
                };
                // Deliver real list-view notifications without holding the UI borrow.
                let pool_list = state.by_id[&POOLS];
                drop(state);
                SendMessageW(
                    pool_list,
                    LVM_SETITEMSTATE,
                    0,
                    (&selection as *const LVITEMW) as isize,
                );
                let mut state = app.borrow_mut();
                assert_eq!(state.account_pool().unwrap().id, "pool-a");
                assert_eq!(
                    SendMessageW(state.by_id[&ACCOUNTS], LVM_GETITEMCOUNT, 0, 0),
                    2
                );
                assert_eq!(cell_text(state.by_id[&ACCOUNTS], 0, 0), "account-a");
                assert_eq!(cell_text(state.by_id[&ACCOUNTS], 1, 1), "已停用");
                assert_eq!(state.model_pool().unwrap().id, "pool-a");
                assert_eq!(state.model_rows.len(), 2);
                assert!(state.text(SEARCH).is_empty());
                // Viewing a different pool never changes the saved routing rule.
                assert_eq!(state.configured_pool, "pool-b");
                assert_eq!(settings::load(&state.path).unwrap().0.pool, "pool-b");
                // Refresh may reorder rows; preserve the selected pool by full ID.
                pools["data"][0]["pool_name"] = serde_json::json!("second");
                pools["data"][1]["pool_name"] = serde_json::json!("first");
                state.replace_catalog(catalog::parse(&models, &pools).unwrap());
                assert_eq!(state.account_pool().unwrap().id, "pool-a");
                assert_eq!(selected(state.by_id[&POOLS]), Some(2));
                assert_eq!(
                    SendMessageW(state.by_id[&ACCOUNTS], LVM_GETITEMCOUNT, 0, 0),
                    2
                );
                state.select_page(INVENTORY_PAGE);
                for id in [POOLS, ACCOUNTS, MODELS] {
                    assert_ne!(
                        GetWindowLongPtrW(state.by_id[&id], GWL_STYLE) & WS_VISIBLE as isize,
                        0
                    );
                }
                assert_eq!(state.model_pool().unwrap().id, "pool-a");
                state.inventory_action(USE_POOL).unwrap();
                assert_eq!(state.configured_pool, "pool-a");
                assert_eq!(state.page, INVENTORY_PAGE);
                assert_eq!(settings::load(&state.path).unwrap().0.pool, "pool-a");
                state.replace_catalog(
                    catalog::parse(
                        &serde_json::json!({"data":[{"id":"chatgpt/beta","router_provider":"openai","router_model":"beta","router_pools":["pool-b"]}]}),
                        &serde_json::json!({"data":[{"id":"pool-b","accounts":[]}]}),
                    )
                    .unwrap(),
                );
                assert!(state.model_pool().is_none());
                assert!(state.model_rows.is_empty());
                assert!(state.account_pool().is_none());
                assert_eq!(
                    SendMessageW(state.by_id[&ACCOUNTS], LVM_GETITEMCOUNT, 0, 0),
                    0
                );
                drop(state);
                SendMessageW(
                    pool_list,
                    LVM_SETITEMSTATE,
                    0,
                    (&selection as *const LVITEMW) as isize,
                );
                let mut state = app.borrow_mut();
                assert_eq!(state.account_pool().unwrap().id, "pool-b");
                assert_eq!(state.account_pool().unwrap().account_count, 0);
                assert_eq!(
                    SendMessageW(state.by_id[&ACCOUNTS], LVM_GETITEMCOUNT, 0, 0),
                    0
                );
                assert_eq!(state.text(ACCOUNTS_EMPTY), "此账号池暂无账号");
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
                assert_eq!(SendMessageW(state.by_id[&POOLS], LVM_GETITEMCOUNT, 0, 0), 0);
                state.tray.take();
                DestroyWindow(hwnd);
                for font in state.fonts {
                    DeleteObject(font);
                }
            }
            UnregisterClassW(class.lpszClassName, class.hInstance);
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
