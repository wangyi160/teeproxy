//! Native Win32 frontend. All HWNDs and UI state stay on the message-loop thread.
//! Workers send owned messages; no window pointer crosses a thread boundary.
#![allow(unsafe_op_in_unsafe_fn)]
pub(crate) mod catalog;
mod inventory_ui;
pub mod settings;
mod tray;
use inventory_ui::*;

use crate::{client, proxy::Proxy};
use anyhow::{Context, Result, ensure};
use settings::{Profile, Secrets};
use std::{cell::RefCell, collections::HashMap, mem, path::PathBuf, ptr, sync::mpsc};
use tokio::{runtime::Runtime, task::JoinHandle};
use windows_sys::Win32::System::SystemServices::SS_LEFT;
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*},
        UI::{
            Controls::{Dialogs::*, *},
            HiDpi::*,
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
    core::w,
};
use zeroize::Zeroizing;

const ROUTER: u16 = 101;
const ROUTER_KEY: u16 = 102;
const PORT: u16 = 103;
const POOL: u16 = 104;
const TDX: u16 = 105;
const SNP: u16 = 106;
const CLIENT_KEY: u16 = 107;
const URL: u16 = 108;
const STATUS: u16 = 109;
const LOG: u16 = 110;
const SAVE: u16 = 201;
const VERIFY: u16 = 202;
const START: u16 = 203;
const STOP: u16 = 204;
const COPY: u16 = 205;
const BROWSE_TDX: u16 = 206;
const BROWSE_SNP: u16 = 207;
const SHOW_KEYS: u16 = 208;
const FIELDS: [u16; 10] = [
    ROUTER, ROUTER_KEY, PORT, POOL, TDX, SNP, CLIENT_KEY, BROWSE_TDX, BROWSE_SNP, SAVE,
];

enum Event {
    Started(u64, String),
    Verified(u64, bool, Vec<String>),
    Failed(u64, String),
    Stopped(u64),
    Inventory(u64, std::result::Result<catalog::Catalog, String>),
}
struct Control {
    hwnd: HWND,
    rect: [i32; 4],
    font: usize,
    page: Option<usize>,
}
struct App {
    hwnd: HWND,
    controls: Vec<Control>,
    by_id: HashMap<u16, HWND>,
    fonts: [HFONT; 3],
    dpi: u32,
    path: PathBuf,
    runtime: Runtime,
    sender: mpsc::Sender<Event>,
    receiver: mpsc::Receiver<Event>,
    task: Option<JoinHandle<()>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    stopping: bool,
    finished_observed: bool,
    generation: u64,
    running: bool,
    status_color: u32,
    log: Vec<String>,
    building_page: Option<usize>,
    page: usize,
    catalog: catalog::Catalog,
    model_rows: Vec<usize>,
    inventory_task: Option<JoinHandle<()>>,
    inventory_generation: u64,
    inventory_finished_observed: bool,
    tray: Option<tray::Tray>,
}

impl App {
    fn new(path: PathBuf) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let (sender, receiver) = mpsc::channel();
        Ok(Self {
            hwnd: ptr::null_mut(),
            controls: Vec::new(),
            by_id: HashMap::new(),
            fonts: [ptr::null_mut(); 3],
            dpi: 96,
            path,
            runtime,
            sender,
            receiver,
            task: None,
            shutdown: None,
            stopping: false,
            finished_observed: false,
            generation: 0,
            running: false,
            status_color: rgb(90, 99, 112),
            log: Vec::new(),
            building_page: None,
            page: 2,
            catalog: Default::default(),
            model_rows: Vec::new(),
            inventory_task: None,
            inventory_generation: 0,
            inventory_finished_observed: false,
            tray: None,
        })
    }
}

pub fn run(path: Option<PathBuf>) -> Result<()> {
    let path = path.map(Ok).unwrap_or_else(settings::default_path)?;
    let path = std::path::absolute(path)?;
    let loaded = settings::load(&path);
    let app = RefCell::new(App::new(path)?);
    // The RefCell outlives the window and is accessed only on this UI thread.
    // try_borrow_mut in wndproc also handles synchronous Win32 reentrancy.
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let controls = INITCOMMONCONTROLSEX {
            dwSize: mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES | ICC_LISTVIEW_CLASSES | ICC_TAB_CLASSES,
        };
        InitCommonControlsEx(&controls);
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            hIcon: LoadIconW(instance, 101usize as *const u16),
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            lpszClassName: w!("teeproxy.NativeWindow"),
            ..mem::zeroed()
        };
        ensure!(RegisterClassW(&class) != 0, "无法注册桌面窗口");
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.lpszClassName,
            w!("teeproxy"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            860,
            720,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            (&app as *const RefCell<App>).cast(),
        );
        ensure!(!hwnd.is_null(), "无法创建桌面窗口");
        let built = {
            let mut state = app.borrow_mut();
            state.hwnd = hwnd;
            state.dpi = GetDpiForWindow(hwnd).max(96);
            state.build(loaded)
        };
        if let Err(error) = built {
            DestroyWindow(hwnd);
            return Err(error);
        }
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        let mut message: MSG = mem::zeroed();
        loop {
            let result = GetMessageW(&mut message, ptr::null_mut(), 0, 0);
            if result == 0 {
                break;
            }
            if result == -1 {
                DestroyWindow(hwnd);
                anyhow::bail!("桌面消息循环失败");
            }
            if IsDialogMessageW(hwnd, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    let mut app = app.into_inner();
    if let Some(task) = app.task.take() {
        task.abort();
    }
    if let Some(task) = app.inventory_task.take() {
        task.abort();
    }
    app.tray.take();
    unsafe {
        for font in app.fonts {
            if !font.is_null() {
                DeleteObject(font);
            }
        }
    }
    app.runtime
        .shutdown_timeout(std::time::Duration::from_secs(2));
    Ok(())
}

impl App {
    unsafe fn add(
        &mut self,
        id: u16,
        class: *const u16,
        text: &str,
        style: u32,
        rect: [i32; 4],
        font: usize,
    ) -> Result<HWND> {
        let hwnd = CreateWindowExW(
            0,
            class,
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            1,
            1,
            self.hwnd,
            id as usize as HMENU,
            GetModuleHandleW(ptr::null()),
            ptr::null(),
        );
        ensure!(!hwnd.is_null(), "无法创建桌面控件");
        self.controls.push(Control {
            hwnd,
            rect,
            font,
            page: self.building_page,
        });
        if id != 0 {
            self.by_id.insert(id, hwnd);
        }
        Ok(hwnd)
    }
    unsafe fn label(&mut self, text: &str, rect: [i32; 4], font: usize) -> Result<()> {
        self.add(0, w!("STATIC"), text, SS_LEFT, rect, font)?;
        Ok(())
    }
    unsafe fn edit(
        &mut self,
        id: u16,
        value: &str,
        rect: [i32; 4],
        extra: u32,
        cue: &str,
    ) -> Result<()> {
        let hwnd = self.add(
            id,
            w!("EDIT"),
            value,
            WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32 | extra,
            rect,
            0,
        )?;
        SendMessageW(
            hwnd,
            EM_SETLIMITTEXT,
            if id == ROUTER_KEY {
                8192
            } else if id == PORT {
                5
            } else if id == CLIENT_KEY {
                256
            } else {
                32700
            },
            0,
        );
        if !cue.is_empty() {
            SendMessageW(hwnd, EM_SETCUEBANNER, 0, wide(cue).as_ptr() as isize);
        }
        Ok(())
    }
    unsafe fn button(&mut self, id: u16, title: &str, rect: [i32; 4], extra: u32) -> Result<()> {
        self.add(id, w!("BUTTON"), title, WS_TABSTOP | extra, rect, 0)?;
        Ok(())
    }
    unsafe fn build(&mut self, loaded: Result<(Profile, Secrets)>) -> Result<()> {
        let (profile, secrets, error) = match loaded {
            Ok((p, s)) => (p, s, None),
            Err(error) => (
                Profile::default(),
                Secrets::default(),
                Some(error.to_string()),
            ),
        };
        self.label("teeproxy", [24, 18, 400, 38], 1)?;
        self.label("本地 API 网关 · 独立验证 TEE 上游", [25, 58, 600, 22], 2)?;
        self.add(
            STATUS,
            w!("STATIC"),
            "已停止",
            SS_LEFT,
            [24, 89, 600, 24],
            0,
        )?;
        self.label("Agent API 地址", [24, 123, 130, 24], 0)?;
        self.edit(
            URL,
            &format!("http://127.0.0.1:{}/v1", profile.port),
            [160, 119, -284, 30],
            ES_READONLY as u32,
            "",
        )?;
        self.button(COPY, "复制地址", [-120, 119, 96, 30], 0)?;
        self.build_inventory()?;
        self.building_page = Some(2);
        self.label("Router 地址", [24, 263, 132, 24], 0)?;
        self.edit(
            ROUTER,
            &profile.router,
            [160, 258, -184, 30],
            0,
            "https://your-router.example.com:8443",
        )?;
        self.label("Router key", [24, 303, 132, 24], 0)?;
        self.edit(
            ROUTER_KEY,
            &secrets.router_key,
            [160, 298, -296, 30],
            ES_PASSWORD as u32,
            "上游 router 的业务密钥",
        )?;
        self.button(
            SHOW_KEYS,
            "显示密钥",
            [-128, 301, 104, 26],
            BS_AUTOCHECKBOX as u32,
        )?;
        self.label("本地端口", [24, 343, 132, 24], 0)?;
        self.edit(
            PORT,
            &profile.port.to_string(),
            [160, 338, 100, 30],
            ES_NUMBER as u32,
            "8899",
        )?;
        self.label("账号池（可选）", [286, 343, 125, 24], 0)?;
        self.edit(
            POOL,
            &profile.pool,
            [420, 338, -444, 30],
            0,
            "使用 router 默认池",
        )?;
        self.label("TDX 批准策略", [24, 383, 132, 24], 0)?;
        self.edit(
            TDX,
            &profile.tdx_policy,
            [160, 378, -284, 30],
            0,
            "选择审核后的 JSON 策略",
        )?;
        self.button(BROWSE_TDX, "选择…", [-120, 378, 96, 30], 0)?;
        self.label("SNP 批准策略", [24, 423, 132, 24], 0)?;
        self.edit(
            SNP,
            &profile.snp_policy,
            [160, 418, -284, 30],
            0,
            "使用 AMD SEV-SNP 时填写",
        )?;
        self.button(BROWSE_SNP, "选择…", [-120, 418, 96, 30], 0)?;
        self.label("本地 key（可选）", [24, 463, 132, 24], 0)?;
        self.edit(
            CLIENT_KEY,
            &secrets.client_key,
            [160, 458, -184, 30],
            ES_PASSWORD as u32,
            "留空则本机请求无需密钥；设置后同步填入 agent",
        )?;
        self.label(
            "密钥通过 Windows 用户加密保存。运行时配置锁定；修改上游前请先停止。",
            [160, 495, -184, 24],
            2,
        )?;
        self.building_page = None;
        self.button(SAVE, "保存配置", [24, 164, 110, 34], 0)?;
        self.button(VERIFY, "验证上游", [146, 164, 110, 34], 0)?;
        self.button(
            START,
            "启动网关",
            [268, 164, 110, 34],
            BS_DEFPUSHBUTTON as u32,
        )?;
        self.button(STOP, "停止", [390, 164, 110, 34], 0)?;
        self.building_page = Some(3);
        self.add(
            LOG,
            w!("EDIT"),
            "",
            WS_TABSTOP
                | WS_BORDER
                | WS_VSCROLL
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_READONLY as u32,
            [24, 264, -48, -312],
            2,
        )?;
        self.building_page = None;
        self.refresh_fonts();
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: self.scale(1020),
            bottom: self.scale(720),
        };
        AdjustWindowRectExForDpi(
            &mut rect,
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            0,
            WS_EX_CONTROLPARENT,
            self.dpi,
        );
        SetWindowPos(
            self.hwnd,
            ptr::null_mut(),
            0,
            0,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOMOVE | SWP_NOZORDER,
        );
        self.layout();
        self.busy(false);
        self.select_page(if profile.router.is_empty() { 2 } else { 0 });
        self.tray = Some(tray::Tray::new(self.hwnd));
        if !self.tray.as_ref().is_some_and(|tray| tray.installed) {
            self.append("系统托盘注册失败；窗口保持任务栏显示，关闭窗口将退出。");
        }
        self.append(&format!("配置文件：{}", self.path.display()));
        self.append("填写 HTTPS 上游和批准策略后，验证上游或启动网关。每条业务连接都独立校验。");
        #[cfg(not(feature = "snp-verifier"))]
        self.append("此构建包含 TDX 验证。AMD SNP 需要启用 snp-verifier 的构建。");
        if let Some(error) = error {
            self.status("配置读取失败，请检查运行记录", rgb(170, 40, 40));
            self.append(&error);
        }
        SetTimer(self.hwnd, 1, 100, None);
        SetFocus(self.by_id[&TABS]);
        Ok(())
    }
    fn scale(&self, value: i32) -> i32 {
        (value as i64 * self.dpi as i64 / 96) as i32
    }
    unsafe fn refresh_fonts(&mut self) {
        self.scale_inventory_columns();
        let old = self.fonts;
        for (index, (size, weight)) in [(14, 400), (28, 600), (12, 400)].into_iter().enumerate() {
            self.fonts[index] = CreateFontW(
                -self.scale(size),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                w!("Microsoft YaHei UI"),
            );
        }
        for control in &self.controls {
            SendMessageW(
                control.hwnd,
                WM_SETFONT,
                self.fonts[control.font] as usize,
                1,
            );
        }
        for font in old {
            if !font.is_null() {
                DeleteObject(font);
            }
        }
    }
    unsafe fn layout(&self) {
        let mut client: RECT = mem::zeroed();
        GetClientRect(self.hwnd, &mut client);
        let width = client.right * 96 / self.dpi as i32;
        let height = client.bottom * 96 / self.dpi as i32;
        for control in &self.controls {
            let [x, y, w, h] = control.rect;
            MoveWindow(
                control.hwnd,
                self.scale(if x < 0 { width + x } else { x }),
                self.scale(if y < 0 { height + y } else { y }),
                self.scale(if w < 0 { width + w } else { w }).max(1),
                self.scale(if h < 0 { height + h } else { h }).max(1),
                1,
            );
        }
    }
    unsafe fn text(&self, id: u16) -> String {
        let hwnd = self.by_id[&id];
        let length = GetWindowTextLengthW(hwnd).clamp(0, 32700) as usize;
        let mut buffer = Zeroizing::new(vec![0u16; length + 1]);
        let read = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32).max(0) as usize;
        String::from_utf16_lossy(&buffer[..read])
    }
    unsafe fn set(&self, id: u16, text: &str) {
        SetWindowTextW(self.by_id[&id], wide(text).as_ptr());
    }
    unsafe fn status(&mut self, text: &str, color: u32) {
        self.status_color = color;
        if let Some(tray) = &self.tray {
            tray.status(text);
        }
        self.set(STATUS, text);
        InvalidateRect(self.by_id[&STATUS], ptr::null(), 1);
    }
    unsafe fn append(&mut self, message: &str) {
        self.log
            .push(message.replace('\r', "").replace('\n', " · "));
        while self.log.len() > 100 {
            self.log.remove(0);
        }
        let text = self.log.join("\r\n");
        self.set(LOG, &text);
        SendMessageW(self.by_id[&LOG], EM_SETSEL, text.encode_utf16().count(), -1);
        SendMessageW(self.by_id[&LOG], EM_SCROLLCARET, 0, 0);
    }
    unsafe fn snapshot(&self) -> Result<(Profile, Secrets)> {
        let port = self
            .text(PORT)
            .parse::<u16>()
            .context("本地端口必须介于 1 和 65535 之间")?;
        let profile = Profile {
            router: self.text(ROUTER),
            port,
            tdx_policy: self.text(TDX),
            snp_policy: self.text(SNP),
            pool: self.text(POOL),
        };
        let secrets = Secrets {
            router_key: self.text(ROUTER_KEY),
            client_key: self.text(CLIENT_KEY),
        };
        profile.config(&secrets, self.path.parent().unwrap())?;
        Ok((profile, secrets))
    }
    unsafe fn busy(&self, value: bool) {
        let value = value || self.inventory_task.is_some();
        EnableWindow(
            self.by_id[&REFRESH],
            (self.inventory_task.is_none()
                && !self.stopping
                && (self.task.is_none() || self.running)) as i32,
        );
        EnableWindow(self.by_id[&USE_POOL], (!value) as i32);
        for id in FIELDS {
            EnableWindow(self.by_id[&id], (!value) as i32);
        }
        for id in [VERIFY, START] {
            EnableWindow(self.by_id[&id], (!value) as i32);
        }
        EnableWindow(self.by_id[&STOP], value as i32);
    }
    unsafe fn action(&mut self, id: u16) -> Result<()> {
        match id {
            SAVE => {
                let (profile, secrets) = self.snapshot()?;
                settings::save(&self.path, &profile, &secrets)?;
                self.append("配置已保存，密钥已使用当前 Windows 用户加密。");
            }
            VERIFY | START if self.task.is_none() && self.inventory_task.is_none() => {
                let (profile, secrets) = self.snapshot()?;
                let config = profile.config(&secrets, self.path.parent().unwrap())?;
                self.generation += 1;
                self.finished_observed = false;
                let generation = self.generation;
                let sender = self.sender.clone();
                let (shutdown, shutdown_rx) = tokio::sync::oneshot::channel();
                self.shutdown = Some(shutdown);
                self.busy(true);
                self.status(
                    if id == VERIFY {
                        "正在验证上游…"
                    } else {
                        "正在启动…"
                    },
                    rgb(30, 94, 166),
                );
                self.task = Some(self.runtime.spawn(async move {
                    let result: Result<()> = async {
                        let trust = config.trust()?;
                        if id == VERIFY {
                            let attempt = tokio::select! {
                                _ = shutdown_rx => { let _ = sender.send(Event::Stopped(generation)); return Ok(()); },
                                attempt = client::verify(&config.router, &trust) => attempt,
                            };
                            let mut lines: Vec<String> = attempt
                                .report
                                .checks
                                .iter()
                                .map(|check| format!("{} · {}", check.status, check.name))
                                .collect();
                            lines.push(redact(&attempt.report.message, &secrets));
                            let _ =
                                sender.send(Event::Verified(generation, attempt.report.ok, lines));
                        } else {
                            let address = config.listen;
                            let local_key = (!secrets.client_key.is_empty())
                                .then_some(secrets.client_key.as_str());
                            let proxy = Proxy::new(config, trust, &secrets.router_key, local_key)?;
                            let listener = tokio::net::TcpListener::bind(address)
                                .await
                                .context("本地端口无法监听，可能已被占用")?;
                            let _ = sender.send(Event::Started(
                                generation,
                                format!("http://{}/v1", listener.local_addr()?),
                            ));
                            proxy.serve_until(listener, async { let _ = shutdown_rx.await; }).await?;
                            let _ = sender.send(Event::Stopped(generation));
                        }
                        Ok(())
                    }
                    .await;
                    if let Err(error) = result {
                        let _ = sender.send(Event::Failed(
                            generation,
                            redact(&format!("{error:#}"), &secrets),
                        ));
                    }
                }));
            }
            STOP => self.stop(),
            REFRESH | COPY_MODEL | USE_POOL => self.inventory_action(id)?,
            tray::OPEN => tray::restore(self.hwnd),
            tray::TOGGLE => {
                if self.task.is_some() || self.inventory_task.is_some() {
                    self.stop();
                } else {
                    self.action(START)?;
                }
            }
            tray::EXIT => self.quit(),
            COPY => {
                copy_text(self.hwnd, &self.text(URL))?;
                self.append("已复制 Agent API 地址。");
            }
            BROWSE_TDX | BROWSE_SNP => {
                if let Some(path) = choose_file(self.hwnd) {
                    self.set(if id == BROWSE_TDX { TDX } else { SNP }, &path);
                }
            }
            SHOW_KEYS => {
                let show =
                    SendMessageW(self.by_id[&SHOW_KEYS], BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
                for id in [ROUTER_KEY, CLIENT_KEY] {
                    SendMessageW(
                        self.by_id[&id],
                        EM_SETPASSWORDCHAR,
                        if show { 0 } else { 0x25cf },
                        0,
                    );
                    InvalidateRect(self.by_id[&id], ptr::null(), 1);
                }
            }
            _ => {}
        }
        Ok(())
    }
    unsafe fn stop(&mut self) {
        if let Some(task) = self.inventory_task.take() {
            task.abort();
            self.inventory_generation += 1;
            self.set(CATALOG_STATUS, "刷新已取消 · 可重新刷新");
            self.busy(self.task.is_some());
        }
        if self.task.is_none() || self.stopping {
            return;
        }
        self.stopping = true;
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        EnableWindow(self.by_id[&STOP], 0);
        self.status("正在停止并关闭连接…", rgb(90, 99, 112));
    }
    unsafe fn quit(&mut self) {
        self.stop();
        if let Some(tray) = &mut self.tray {
            tray.remove();
        }
        DestroyWindow(self.hwnd);
    }
    unsafe fn hide_to_tray(&mut self) -> bool {
        if self.tray.as_ref().is_some_and(|tray| tray.installed) {
            ShowWindow(self.hwnd, SW_HIDE);
            true
        } else {
            false
        }
    }
    unsafe fn poll(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Inventory(generation, result) if generation == self.inventory_generation => {
                    self.inventory_task.take();
                    match result {
                        Ok(catalog) => {
                            self.catalog = catalog;
                            self.render_catalog();
                            self.set(
                                CATALOG_STATUS,
                                &format!(
                                    "{} 个模型 · {} 个账号池",
                                    self.catalog.models.len(),
                                    self.catalog.pools
                                ),
                            );
                            self.append("已通过 TEE 验证并刷新模型、账号池及账号状态。");
                        }
                        Err(error) => {
                            self.set(CATALOG_STATUS, "刷新失败 · 请查看运行记录");
                            self.append(&error);
                        }
                    }
                    self.busy(self.task.is_some());
                }
                Event::Started(generation, url)
                    if generation == self.generation && !self.stopping =>
                {
                    self.running = true;
                    self.set(URL, &url);
                    self.status("运行中 · 每条上游连接独立验证", rgb(20, 117, 73));
                    self.append(&format!("网关已启动：{url}"));
                    self.busy(true);
                    if let Err(error) = self.refresh_catalog() {
                        self.append(&error.to_string());
                    }
                }
                Event::Verified(generation, ok, lines) if generation == self.generation => {
                    self.task.take();
                    self.shutdown.take();
                    self.stopping = false;
                    self.busy(false);
                    self.status(
                        if ok {
                            "上游验证通过 · 网关尚未启动"
                        } else {
                            "上游验证未通过"
                        },
                        if ok {
                            rgb(20, 117, 73)
                        } else {
                            rgb(170, 40, 40)
                        },
                    );
                    for line in lines {
                        self.append(&line);
                    }
                }
                Event::Failed(generation, message) if generation == self.generation => {
                    self.task.take();
                    self.shutdown.take();
                    self.stopping = false;
                    self.running = false;
                    self.busy(false);
                    self.status("操作失败，请检查运行记录", rgb(170, 40, 40));
                    self.append(&message);
                }
                Event::Stopped(generation) if generation == self.generation => {
                    self.task.take();
                    self.shutdown.take();
                    self.running = false;
                    self.stopping = false;
                    self.busy(false);
                    self.status("已停止", rgb(90, 99, 112));
                    self.append("网关已停止或验证已取消，现有连接已关闭。");
                }
                _ => {}
            }
        }
        if self.task.as_ref().is_some_and(|task| task.is_finished()) {
            // Wait one tick for a completion event racing with try_recv().
            if self.finished_observed {
                self.task.take();
                self.shutdown.take();
                self.running = false;
                self.stopping = false;
                self.busy(false);
                self.status("后台任务意外结束，请重新启动", rgb(170, 40, 40));
                self.append("后台任务未返回完成状态。");
            } else {
                self.finished_observed = true;
            }
        }
        if self
            .inventory_task
            .as_ref()
            .is_some_and(|task| task.is_finished())
        {
            if self.inventory_finished_observed {
                self.inventory_task.take();
                self.set(CATALOG_STATUS, "刷新意外结束 · 可重新刷新");
                self.append("列表刷新任务未返回完成状态。");
                self.busy(self.task.is_some());
            } else {
                self.inventory_finished_observed = true;
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lp as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    if message == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        return DefWindowProcW(hwnd, message, wp, lp);
    }
    if message == WM_DESTROY {
        KillTimer(hwnd, 1);
        PostQuitMessage(0);
        return 0;
    }
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<App>;
    if !pointer.is_null()
        && let Ok(mut app) = (&*pointer).try_borrow_mut()
    {
        if app
            .tray
            .as_ref()
            .is_some_and(|tray| tray.restart != 0 && message == tray.restart)
        {
            if let Some(tray) = &mut app.tray {
                tray.recreate();
            }
            if !app.tray.as_ref().is_some_and(|tray| tray.installed) {
                tray::unavailable(&mut app);
            }
            return 0;
        }
        match message {
            tray::MESSAGE => {
                let notification = (lp as u32) & 0xffff;
                let action = if matches!(
                    notification,
                    windows_sys::Win32::UI::Shell::NIN_SELECT | tray::KEY_SELECT | WM_LBUTTONDBLCLK
                ) {
                    tray::OPEN
                } else if matches!(notification, WM_CONTEXTMENU | WM_RBUTTONUP) {
                    app.tray.as_ref().map_or(0, |tray| {
                        tray.menu(app.task.is_some() || app.inventory_task.is_some())
                    })
                } else {
                    0
                };
                if let Err(error) = app.action(action) {
                    app.append(&error.to_string());
                    tray::restore(hwnd);
                }
                return 0;
            }
            WM_NOTIFY => {
                let notification = &*(lp as *const NMHDR);
                if notification.idFrom == TABS as usize && notification.code == TCN_SELCHANGE {
                    let page = SendMessageW(app.by_id[&TABS], TCM_GETCURSEL, 0, 0).max(0) as usize;
                    app.select_page(page);
                }
                return 0;
            }
            WM_COMMAND => {
                let id = (wp & 0xffff) as u16;
                let notification = (wp >> 16) as u16;
                if id == SEARCH && notification == EN_CHANGE as u16 {
                    app.render_catalog();
                    return 0;
                }
                if FIELDS.contains(&id)
                    && notification == EN_CHANGE as u16
                    && app.task.is_none()
                    && app.inventory_task.is_none()
                    && app.by_id.contains_key(&URL)
                {
                    if id == PORT
                        && let Ok(port) = app.text(PORT).parse::<u16>()
                    {
                        app.set(URL, &format!("http://127.0.0.1:{port}/v1"));
                    }
                    if matches!(id, ROUTER | ROUTER_KEY | TDX | SNP) {
                        app.invalidate_catalog();
                    }
                    app.status("配置已修改 · 请验证或启动", rgb(90, 99, 112));
                } else if notification == BN_CLICKED as u16
                    && (app.by_id.contains_key(&id)
                        || matches!(id, tray::OPEN | tray::TOGGLE | tray::EXIT))
                    && let Err(error) = app.action(id)
                {
                    app.status("操作失败，请检查运行记录", rgb(170, 40, 40));
                    app.append(&format!("{error:#}"));
                }
                return 0;
            }
            WM_TIMER => {
                app.poll();
                return 0;
            }
            WM_SIZE => {
                if wp == SIZE_MINIMIZED as usize {
                    app.hide_to_tray();
                } else {
                    app.layout();
                }
                return 0;
            }
            WM_DPICHANGED => {
                app.dpi = (wp & 0xffff) as u32;
                app.refresh_fonts();
                let rect = &*(lp as *const RECT);
                SetWindowPos(
                    hwnd,
                    ptr::null_mut(),
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                app.layout();
                return 0;
            }
            WM_GETMINMAXINFO => {
                let size = &mut *(lp as *mut MINMAXINFO);
                size.ptMinTrackSize.x = app.scale(900);
                size.ptMinTrackSize.y = app.scale(650);
                return 0;
            }
            WM_CTLCOLORSTATIC => {
                let dc = wp as HDC;
                SetBkColor(dc, GetSysColor(COLOR_WINDOW));
                let color = if app.by_id.get(&STATUS) == Some(&(lp as HWND)) {
                    app.status_color
                } else {
                    rgb(45, 52, 62)
                };
                SetTextColor(dc, color);
                return GetSysColorBrush(COLOR_WINDOW) as isize;
            }
            WM_CLOSE => {
                if !app.hide_to_tray() {
                    app.quit();
                }
                return 0;
            }
            WM_QUERYENDSESSION => return 1,
            WM_ENDSESSION if wp != 0 => {
                app.quit();
                return 0;
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, message, wp, lp)
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn rgb(r: u32, g: u32, b: u32) -> u32 {
    r | (g << 8) | (b << 16)
}
fn redact(message: &str, secrets: &Secrets) -> String {
    let mut result = message.to_owned();
    for secret in [&secrets.router_key, &secrets.client_key] {
        if !secret.is_empty() {
            result = result.replace(secret, "[密钥]");
        }
    }
    result
}
unsafe fn choose_file(hwnd: HWND) -> Option<String> {
    let mut path = vec![0u16; 32768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: w!("JSON 策略文件\0*.json\0所有文件\0*.*\0"),
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: path.len() as u32,
        lpstrTitle: w!("选择本机批准的发布策略"),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..mem::zeroed()
    };
    if GetOpenFileNameW(&mut dialog) == 0 {
        return None;
    }
    let end = path.iter().position(|c| *c == 0).unwrap_or(path.len());
    Some(String::from_utf16_lossy(&path[..end]))
}
unsafe fn copy_text(hwnd: HWND, text: &str) -> Result<()> {
    ensure!(OpenClipboard(hwnd) != 0, "剪贴板暂时不可用");
    let data = wide(text);
    let memory = GlobalAlloc(GMEM_MOVEABLE, data.len() * 2);
    if memory.is_null() {
        CloseClipboard();
        anyhow::bail!("无法分配剪贴板内存");
    }
    let buffer = GlobalLock(memory);
    if buffer.is_null() {
        GlobalFree(memory);
        CloseClipboard();
        anyhow::bail!("无法访问剪贴板内存");
    }
    ptr::copy_nonoverlapping(data.as_ptr(), buffer.cast(), data.len());
    GlobalUnlock(memory);
    let success = EmptyClipboard() != 0 && !SetClipboardData(13, memory).is_null();
    if !success {
        GlobalFree(memory);
    }
    CloseClipboard();
    ensure!(success, "无法写入剪贴板");
    Ok(())
}
