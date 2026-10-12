//! Windows notification-area icon. A failed icon registration must not hide the window.
#![allow(unsafe_op_in_unsafe_fn)]
use super::{rgb, wide};
use std::{mem, ptr};
use windows_sys::{
    Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::w,
};

pub(super) const MESSAGE: u32 = WM_APP + 17;
pub(super) const OPEN: u16 = 401;
pub(super) const TOGGLE: u16 = 402;
pub(super) const EXIT: u16 = 403;
// Shell defines this as (NIN_SELECT | NINF_KEY); windows-sys omits the macro.
pub(super) const KEY_SELECT: u32 = NIN_SELECT | NINF_KEY;
pub(super) struct Tray {
    hwnd: HWND,
    pub installed: bool,
    pub restart: u32,
}
impl Tray {
    pub unsafe fn new(hwnd: HWND) -> Self {
        let mut tray = Self {
            hwnd,
            installed: false,
            restart: RegisterWindowMessageW(w!("TaskbarCreated")),
        };
        tray.recreate();
        tray
    }
    fn data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            ..Default::default()
        }
    }
    pub unsafe fn recreate(&mut self) {
        let mut data = self.data();
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = MESSAGE;
        data.hIcon = LoadIconW(GetModuleHandleW(ptr::null()), 101usize as *const u16);
        let tip = wide("teeproxy · 双击打开，右键退出");
        data.szTip[..tip.len()].copy_from_slice(&tip);
        self.installed = Shell_NotifyIconW(NIM_ADD, &data) != 0;
        if self.installed {
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
    }
    pub unsafe fn status(&self, text: &str) {
        if !self.installed {
            return;
        }
        let mut data = self.data();
        data.uFlags = NIF_TIP | NIF_SHOWTIP;
        for (out, value) in data
            .szTip
            .iter_mut()
            .take(127)
            .zip(format!("teeproxy · {text}").encode_utf16())
        {
            *out = value;
        }
        Shell_NotifyIconW(NIM_MODIFY, &data);
    }
    pub unsafe fn remove(&mut self) {
        if self.installed {
            Shell_NotifyIconW(NIM_DELETE, &self.data());
            self.installed = false;
        }
    }
    pub unsafe fn menu(&self, active: bool) -> u16 {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return 0;
        }
        AppendMenuW(menu, MF_STRING, OPEN as usize, w!("打开窗口"));
        AppendMenuW(
            menu,
            MF_STRING,
            TOGGLE as usize,
            wide(if active {
                "停止网关 / 取消操作"
            } else {
                "启动网关"
            })
            .as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
        AppendMenuW(menu, MF_STRING, EXIT as usize, w!("退出 teeproxy"));
        let mut point = POINT::default();
        GetCursorPos(&mut point);
        SetForegroundWindow(self.hwnd);
        let action = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            self.hwnd,
            ptr::null(),
        ) as u16;
        DestroyMenu(menu);
        PostMessageW(self.hwnd, WM_NULL, 0, 0);
        action
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            self.remove();
        }
    }
}
pub(super) unsafe fn restore(hwnd: HWND) {
    ShowWindow(hwnd, SW_RESTORE);
    SetForegroundWindow(hwnd);
}
// Keep all window transitions on the UI thread, including Explorer restart recovery.
pub(super) unsafe fn unavailable(app: &mut super::App) {
    restore(app.hwnd);
    app.status("系统托盘不可用 · 窗口将保持可见", rgb(170, 40, 40));
}
