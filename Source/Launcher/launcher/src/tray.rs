use std::{mem::{size_of, zeroed}, num::NonZeroIsize, ptr::null};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Shell::{
            DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW,
            NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
            NIN_SELECT, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
        },
        WindowsAndMessaging::{
            LoadIconW, PostMessageW, RegisterWindowMessageW, SetForegroundWindow, ShowWindow,
            SW_HIDE, SW_RESTORE, WM_APP, WM_LBUTTONUP, WM_NCDESTROY,
        },
    },
};

const ICON_ID: u32 = 1;
const TRAY_EVENT: u32 = WM_APP + 20;
const GAME_ENDED: u32 = WM_APP + 21;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;

pub fn window_handle(window: &impl HasWindowHandle) -> Option<NonZeroIsize> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd),
        _ => None,
    }
}

pub fn wake_for_game_exit(window: NonZeroIsize) {
    unsafe { PostMessageW(window.get() as HWND, GAME_ENDED, 0, 0); }
}

pub struct TrayIcon(HWND);

impl TrayIcon {
    pub fn hide_window(window: NonZeroIsize) -> Option<Self> {
        let hwnd = window.get() as HWND;
        let taskbar_created: Vec<u16> = "TaskbarCreated\0".encode_utf16().collect();
        unsafe {
            let taskbar_created = RegisterWindowMessageW(taskbar_created.as_ptr());
            if SetWindowSubclass(hwnd, Some(window_proc), ICON_ID as usize, taskbar_created as usize) == 0 {
                return None;
            }
            let icon = Self(hwnd);
            if !add_icon(hwnd) {
                return None;
            }
            ShowWindow(hwnd, SW_HIDE);
            Some(icon)
        }
    }

    pub fn restore(self) {
        show(self.0);
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        remove_icon(self.0);
    }
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = ICON_ID;
    data
}

fn add_icon(hwnd: HWND) -> bool {
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = TRAY_EVENT;
    data.hIcon = unsafe { LoadIconW(GetModuleHandleW(null()), 1usize as *const u16) };
    for (unit, slot) in "Project Reboot Launcher".encode_utf16().zip(&mut data.szTip) {
        *slot = unit;
    }
    unsafe {
        if data.hIcon.is_null() || Shell_NotifyIconW(NIM_ADD, &data) == 0 {
            return false;
        }
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &data);
    }
    true
}

fn remove_icon(hwnd: HWND) {
    unsafe {
        if RemoveWindowSubclass(hwnd, Some(window_proc), ICON_ID as usize) != 0 {
            Shell_NotifyIconW(NIM_DELETE, &icon_data(hwnd));
        }
    }
}

fn show(hwnd: HWND) {
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd);
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, _: usize, taskbar_created: usize,
) -> LRESULT {
    match message {
        TRAY_EVENT => {
            if matches!((lparam as u32) & 0xffff, WM_LBUTTONUP | NIN_SELECT | NIN_KEYSELECT) {
                show(hwnd);
            }
            return 0;
        }
        GAME_ENDED => {
            show(hwnd);
            return 0;
        }
        WM_NCDESTROY => remove_icon(hwnd),
        _ if taskbar_created != 0 && message as usize == taskbar_created => {
            if !add_icon(hwnd) {
                show(hwnd);
            }
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
