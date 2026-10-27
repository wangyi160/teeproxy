#![allow(unsafe_op_in_unsafe_fn)]
use super::*;

pub(super) const MODELS: u16 = 302;
pub(super) const ACCOUNTS: u16 = 303;
pub(super) const SEARCH: u16 = 304;
pub(super) const COPY_MODEL: u16 = 305;
pub(super) const USE_POOL: u16 = 306;
pub(super) const REFRESH: u16 = 307;
pub(super) const CATALOG_STATUS: u16 = 308;
pub(super) const POOLS: u16 = 312;
pub(super) const POOLS_LOADING: u16 = 313;
pub(super) const ACCOUNTS_TITLE: u16 = 314;
pub(super) const POOLS_TITLE: u16 = 315;
pub(super) const MODELS_TITLE: u16 = 316;

pub(super) fn inventory_cards(width: i32, height: i32) -> [[i32; 4]; 3] {
    let left = (width - 296) * 44 / 100;
    [
        [244, 186, width - 280, 222],
        [244, 428, left, height - 516],
        [260 + left, 428, width - 296 - left, height - 516],
    ]
}

pub(super) fn inventory_rect(id: u16, width: i32, height: i32) -> Option<[i32; 4]> {
    let [pools, accounts, models] = inventory_cards(width, height);
    let card = match id {
        POOLS_LOADING => pools,
        ACCOUNTS | ACCOUNTS_TITLE | ACCOUNTS_LOADING => accounts,
        MODELS | MODELS_TITLE | MODELS_LOADING => models,
        _ => return None,
    };
    Some(match id {
        ACCOUNTS_TITLE | MODELS_TITLE => [card[0] + 16, card[1] + 14, card[2] - 32, 24],
        POOLS_LOADING | ACCOUNTS_LOADING | MODELS_LOADING => [
            card[0] + (card[2] - 48) / 2,
            card[1] + 48 + (card[3] - 112) / 2,
            48,
            48,
        ],
        _ => [card[0] + 16, card[1] + 50, card[2] - 32, card[3] - 66],
    })
}

impl App {
    pub(super) unsafe fn redraw_latencies(&self) {
        // Update cells in place so expiration never resets selection or scroll.
        for (row, pool) in self.catalog.pools.iter().enumerate() {
            latency_cell(self.by_id[&POOLS], row, 4, &pool.latency.display());
        }
        for (row, account) in self
            .catalog
            .accounts
            .iter()
            .filter(|a| Some(&a.pool) == self.account_pool_id.as_ref())
            .enumerate()
        {
            latency_cell(self.by_id[&ACCOUNTS], row, 2, &account.latency.display());
        }
    }
    pub(super) unsafe fn scale_inventory_columns(&self) {
        for (id, widths) in [
            (MODELS, &[282, 124][..]),
            (POOLS, &[120, 100, 84, 72, 120, 100, 100, 132][..]),
            (ACCOUNTS, &[104, 72, 100, 132, 78, 64][..]),
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
            MODELS_TITLE,
            w!("STATIC"),
            "模型",
            SS_LEFT,
            [260, 470, 300, 24],
            4,
        )?;
        self.table(MODELS, &[("模型 ID", 282), ("接口", 124)])?;
        let empty = self.add(
            MODELS_LOADING,
            w!("STATIC"),
            "加载中",
            windows_sys::Win32::System::SystemServices::SS_OWNERDRAW,
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
                ("账号池", 120),
                ("商户", 100),
                ("价格倍率", 84),
                ("账号数量", 72),
                ("最低链路延迟", 120),
                ("模型提供商", 100),
                ("状态", 100),
                ("测速时间", 132),
            ],
        )?;
        self.add(
            ACCOUNTS_TITLE,
            w!("STATIC"),
            "账号",
            SS_LEFT,
            [260, 426, -312, 24],
            4,
        )?;
        self.table(
            ACCOUNTS,
            &[
                ("账号", 104),
                ("状态", 72),
                ("链路延迟", 100),
                ("测速时间", 132),
                ("模型目录", 78),
                ("模型数", 64),
            ],
        )?;
        let empty = self.add(
            POOLS_LOADING,
            w!("STATIC"),
            "加载中",
            windows_sys::Win32::System::SystemServices::SS_OWNERDRAW,
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
            ACCOUNTS_LOADING,
            w!("STATIC"),
            "加载中",
            windows_sys::Win32::System::SystemServices::SS_OWNERDRAW,
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
        let routing_selection = self
            .catalog
            .pools
            .get(SendMessageW(self.by_id[&ROUTE_POOL], CB_GETCURSEL, 0, 0) as usize)
            .map(|p| p.id.clone());
        let preferred = self.account_pool_id.clone().or_else(|| {
            let id = self.configured_pool.clone();
            (!id.is_empty()).then_some(id)
        });
        self.catalog = catalog;
        self.catalog_loaded = true;
        // Preserve by full ID, never silently switch merchants when a pool disappears.
        self.render_pools(preferred.as_deref());
        self.refresh_route_pools();
        if let Some(id) = routing_selection {
            // Periodic refresh must not discard an unsaved routing selection.
            let selected = self
                .catalog
                .pools
                .iter()
                .position(|p| p.id == id)
                .unwrap_or(usize::MAX);
            SendMessageW(self.by_id[&ROUTE_POOL], CB_SETCURSEL, selected, 0);
            self.update_route_summary();
        }
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
                    p.latency.display(),
                    self.pool_providers(p),
                    p.state.clone(),
                    p.latency.checked.clone(),
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
                    a.id.clone(),
                    a.state.clone(),
                    a.latency.display(),
                    a.latency.checked.clone(),
                    a.catalog.clone(),
                    a.models.clone(),
                ]
            })
            .collect::<Vec<_>>();
        fill(self.by_id[&ACCOUNTS], &accounts);
        self.set(
            ACCOUNTS_TITLE,
            &if let Some(pool) = self.account_pool() {
                format!("账号 · {} 个", pool.account_count)
            } else {
                "账号".into()
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
            MODELS_TITLE,
            &format!("模型 · {} 个", self.model_rows.len()),
        );
        self.update_summary();
        self.update_visibility();
    }
    pub(super) unsafe fn invalidate_catalog(&mut self) {
        self.management_catalog = None;
        self.catalog = Default::default();
        self.catalog_loaded = false;
        self.render_pools(None);
        self.refresh_route_pools();
        self.render_catalog();
        self.inventory_refresh_pending = matches!(self.page, OVERVIEW | INVENTORY_PAGE);
        self.set(CATALOG_STATUS, "");
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
    pub(super) unsafe fn refresh_inventory_if_due(&mut self) {
        if !matches!(self.page, OVERVIEW | INVENTORY_PAGE)
            || !self.account.logged_in()
            || self.router_key.is_empty()
            || self.inventory_task.is_some()
            || self.stopping
            || (self.task.is_some() && !self.running)
            || (!self.inventory_refresh_pending
                && self.inventory_refreshed.elapsed().as_secs() < 300)
        {
            return;
        }
        // Queue page entry behind another task or login, but do not spin on a failed fetch.
        self.inventory_refresh_pending = false;
        self.inventory_refreshed = std::time::Instant::now();
        if let Err(error) = self.refresh_catalog() {
            self.set(CATALOG_STATUS, "刷新失败");
            self.action_failed(REFRESH, &error);
        }
    }
    pub(super) unsafe fn refresh_catalog(&mut self) -> Result<()> {
        if self.inventory_task.is_some() || self.stopping || (self.task.is_some() && !self.running)
        {
            return Ok(());
        }
        let cache = self.management_catalog()?;
        self.inventory_generation += 1;
        self.inventory_refresh_pending = false;
        self.inventory_refreshed = std::time::Instant::now();
        self.inventory_finished_observed = false;
        let generation = self.inventory_generation;
        let sender = self.sender.clone();
        self.inventory_task = Some(self.runtime.spawn(async move {
            let result = async {
                let value = cache.load(true).await?;
                let mut catalog = catalog::parse(&value["models"], &value["pools"])?;
                catalog.stale = value["status"] == "stale";
                Ok(catalog)
            }.await.map_err(|e: anyhow::Error| e.to_string());
            let _ = sender.send(Event::Inventory(generation, result));
        }));
        self.set(CATALOG_STATUS, "");
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
unsafe fn latency_cell(hwnd: HWND, row: usize, column: i32, text: &str) {
    let mut text = wide(text);
    let item = LVITEMW {
        iSubItem: column,
        pszText: text.as_mut_ptr(),
        ..mem::zeroed()
    };
    SendMessageW(
        hwnd,
        LVM_SETITEMTEXTW,
        row,
        (&item as *const LVITEMW) as isize,
    );
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
                        Secrets { client_key: "test-local-client-key".into(), router_key: String::new(), router_access: String::new() },
                    )))
                    .unwrap();
                // Platform prices render without a local inventory or Router connection.
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
                    {"id":"pool-b","accounts":[{"id":"account-b","enabled":true,"available":true}]},
                    {"id":"pool-empty","accounts":[]}
                ]});
                let probe = serde_json::json!({"status":"measured","latency_ms":170.25,
                    "checked_at":1792000000,"expires_at":4102444800u64});
                pools["data"][0]["proxy_latency"] = probe.clone();
                pools["data"][0]["accounts"][0]["proxy_latency"] = probe;
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
                assert_eq!(SendMessageW(state.by_id[&POOLS], LVM_GETITEMCOUNT, 0, 0), 2);
                assert_eq!(cell_text(state.by_id[&POOLS], 0, 4), "170.2 ms");
                assert_eq!(cell_text(state.by_id[&POOLS], 0, 3), "1");
                assert_eq!(cell_text(state.by_id[&POOLS], 1, 3), "1");
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
                assert_eq!(cell_text(state.by_id[&ACCOUNTS], 0, 1), "可用");
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
                let mut config=profile.config(&secrets, state.path.parent().unwrap()).unwrap();
                config.user_signer=Some(crate::user_auth::fixture());
                let proxy = Proxy::new(
                    config,
                    client::Trust::default(),
                    "test-key",
                    Some(&secrets.client_key),
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
                    1
                );
                assert_eq!(cell_text(state.by_id[&ACCOUNTS], 0, 0), "account-a");
                assert_eq!(cell_text(state.by_id[&ACCOUNTS], 0, 2), "170.2 ms");
                assert_eq!(state.model_pool().unwrap().id, "pool-a");
                assert_eq!(state.model_rows.len(), 2);
                assert!(state.text(SEARCH).is_empty());
                // Viewing a different pool never changes the saved routing rule.
                assert_eq!(state.configured_pool, "pool-b");
                assert_eq!(settings::load(&state.path).unwrap().0.pool, "pool-b");
                // Refresh may reorder rows; preserve the selected pool by full ID.
                let draft_pool = state
                    .catalog
                    .pools
                    .iter()
                    .position(|p| p.id == "pool-a")
                    .unwrap();
                SendMessageW(state.by_id[&ROUTE_POOL], CB_SETCURSEL, draft_pool, 0);
                pools["data"][0]["pool_name"] = serde_json::json!("second");
                pools["data"][1]["pool_name"] = serde_json::json!("first");
                state.replace_catalog(catalog::parse(&models, &pools).unwrap());
                assert_eq!(state.account_pool().unwrap().id, "pool-a");
                assert_eq!(selected(state.by_id[&POOLS]), Some(1));
                let selected_route =
                    SendMessageW(state.by_id[&ROUTE_POOL], CB_GETCURSEL, 0, 0) as usize;
                assert_eq!(state.catalog.pools[selected_route].id, "pool-a");
                assert_eq!(settings::load(&state.path).unwrap().0.pool, "pool-b");
                assert_eq!(
                    SendMessageW(state.by_id[&ACCOUNTS], LVM_GETITEMCOUNT, 0, 0),
                    1
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
                        &serde_json::json!({"data":[{"id":"pool-b","accounts":[{"id":"account-b","enabled":true,"available":true}]}]}),
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
                assert_eq!(state.account_pool().unwrap().account_count, 1);
                assert_eq!(
                    SendMessageW(state.by_id[&ACCOUNTS], LVM_GETITEMCOUNT, 0, 0),
                    1
                );
                let usable = mem::take(&mut state.catalog);
                // Expiry removes the selected pool and its account/model panes,
                // without silently changing the persisted fixed-pool rule.
                state.replace_catalog(catalog::parse(
                    &models,
                    &serde_json::json!({"data":[{"id":"pool-b","accounts":[{"id":"account-b","enabled":true,"available":false}]}]}),
                ).unwrap());
                assert!(state.account_pool().is_none());
                assert!(state.model_rows.is_empty());
                assert!(state.catalog.accounts.is_empty());
                assert_eq!(state.configured_pool, "pool-a");
                state.replace_catalog(usable);
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
                // A denial discovered by the UI must stop the service and
                // clear private rows, including while its window is hidden.
                let denied = crate::catalog::Cache::new(|| Box::pin(async {
                    Err(crate::catalog::AccessRevoked.into())
                }));
                state.management_catalog = Some(denied.clone());
                let (shutdown, mut stopped) = tokio::sync::oneshot::channel();
                state.shutdown = Some(shutdown);
                state.task = Some(state.runtime.spawn(std::future::pending::<()>()));
                state.running = true;
                assert!(state.runtime.block_on(denied.load(true)).is_err());
                state.poll();
                assert_eq!(stopped.try_recv(), Ok(()));
                assert!(state.stopping);
                assert!(state.management_catalog.is_none());
                assert!(state.catalog.models.is_empty());
                assert!(!state.inventory_refresh_pending);
                state.task.take().unwrap().abort();
                state.running = false;
                state.stopping = false;
                state.tray.take();
                DestroyWindow(hwnd);
                for font in state.fonts {
                    DeleteObject(font);
                }
            }
            UnregisterClassW(class.lpszClassName, class.hInstance);
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(directory.join("account-platform.json")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
