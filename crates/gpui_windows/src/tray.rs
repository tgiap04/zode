//! The tray icon: `Shell_NotifyIconW` on a hidden window.
//!
//! The window is a top-level one that is never shown, not a message-only
//! window: `TaskbarCreated` is broadcast, and broadcasts skip message-only
//! windows, so on one the icon would stay gone after Explorer restarts.
//!
//! Clicks arrive as the icon's callback message on that window, on the thread
//! that runs the message loop, so the app's callback runs there directly. The
//! icon is added again when Explorer restarts (`TaskbarCreated`), and removed
//! when this is dropped, so no ghost icon stays behind after quitting.

use std::{cell::RefCell, rc::Rc};

use gpui::{DisplayId, TrayEdge, TrayIcon, TrayIconAnchor, point, px, size};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{HMONITOR, MONITOR_DEFAULTTONEAREST, MonitorFromRect},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETTASKBARPOS, APPBARDATA, NIF_ICON,
                NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NIN_SELECT,
                NOTIFYICON_VERSION_4, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, SHAppBarMessage,
                Shell_NotifyIconGetRect, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                CreateIconFromResourceEx, CreateWindowExW, DefWindowProcW, DestroyIcon,
                DestroyWindow, GWLP_USERDATA, GetSystemMetrics, GetWindowLongPtrW, HICON,
                LR_DEFAULTCOLOR, RegisterClassW, RegisterWindowMessageW, SM_CXSMICON,
                SetWindowLongPtrW, WM_APP, WM_CONTEXTMENU, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

/// The message the shell sends for the icon.
const CALLBACK_MESSAGE: u32 = WM_APP + 0x7A1;
const ICON_ID: u32 = 1;
/// `NIN_KEYSELECT`: the icon chosen with the keyboard (`NIN_SELECT | NINF_KEY`),
/// not exported by the `windows` crate.
const NIN_KEYSELECT: u32 = NIN_SELECT | 0x1;
const CLASS_NAME: PCWSTR = w!("Zode::TrayWindow");

pub(crate) type ClickCallback = Rc<RefCell<Option<Box<dyn FnMut(Option<TrayIconAnchor>)>>>>;

/// How the platform names a monitor: its display id and scale factor.
pub(crate) type DisplayLookup = fn(HMONITOR) -> Option<(DisplayId, f32)>;

/// Everything the window procedure needs, reached through `GWLP_USERDATA`.
struct Shared {
    click: ClickCallback,
    display_for: DisplayLookup,
    icon: HICON,
    tooltip: Vec<u16>,
    taskbar_created: u32,
}

/// The icon while it is shown.
pub(crate) struct WindowsTray {
    hwnd: HWND,
    shared: Box<Shared>,
}

/// `text` as a null-terminated UTF-16 string, cut to fit `limit` units.
fn wide(text: &str, limit: usize) -> Vec<u16> {
    let mut units: Vec<u16> = text.encode_utf16().take(limit.saturating_sub(1)).collect();
    units.push(0);
    units
}

/// The screen edge the taskbar is on, from `ABM_GETTASKBARPOS`, and the taskbar's rectangle.
fn taskbar() -> Option<(TrayEdge, RECT)> {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut data) } == 0 {
        return None;
    }
    let edge = match data.uEdge {
        edge if edge == ABE_TOP => TrayEdge::Top,
        edge if edge == ABE_LEFT => TrayEdge::Left,
        edge if edge == ABE_RIGHT => TrayEdge::Right,
        edge if edge == ABE_BOTTOM => TrayEdge::Bottom,
        _ => TrayEdge::Bottom,
    };
    Some((edge, data.rc))
}

fn intersects(a: &RECT, b: &RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

impl Shared {
    fn data(&self, hwnd: HWND) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: ICON_ID,
            uFlags: NIF_ICON | NIF_TIP | NIF_MESSAGE | NIF_SHOWTIP,
            uCallbackMessage: CALLBACK_MESSAGE,
            hIcon: self.icon,
            ..Default::default()
        };
        let length = self.tooltip.len().min(data.szTip.len());
        data.szTip[..length].copy_from_slice(&self.tooltip[..length]);
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        data
    }

    fn add(&self, hwnd: HWND) {
        let data = self.data(hwnd);
        unsafe {
            if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
                log::info!("the taskbar did not take the tray icon");
                return;
            }
            if !Shell_NotifyIconW(NIM_SETVERSION, &data).as_bool() {
                log::debug!("the taskbar refused tray icon version 4");
            }
        }
    }

    fn icon_rect(&self, hwnd: HWND) -> Option<RECT> {
        let identifier = NOTIFYICONIDENTIFIER {
            cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: hwnd,
            uID: ICON_ID,
            ..Default::default()
        };
        unsafe { Shell_NotifyIconGetRect(&identifier) }.ok()
    }

    fn anchor(&self, hwnd: HWND) -> Option<TrayIconAnchor> {
        let rect = self.icon_rect(hwnd)?;
        let monitor = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) };
        let (display_id, scale) = (self.display_for)(monitor)?;
        let edge = taskbar().map_or(TrayEdge::Bottom, |(edge, _)| edge);
        // Window bounds on Windows are global, in logical pixels.
        Some(TrayIconAnchor {
            bounds: gpui::Bounds::new(
                point(px(rect.left as f32 / scale), px(rect.top as f32 / scale)),
                size(
                    px((rect.right - rect.left) as f32 / scale),
                    px((rect.bottom - rect.top) as f32 / scale),
                ),
            ),
            display_id: Some(display_id),
            edge,
        })
    }

    fn clicked(&self, hwnd: HWND) {
        let anchor = self.anchor(hwnd);
        let callback = self.click.borrow_mut().take();
        if let Some(mut callback) = callback {
            callback(anchor);
            self.click.borrow_mut().get_or_insert(callback);
        }
    }
}

unsafe extern "system" fn window_procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let shared = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Shared;
    if !shared.is_null() {
        let shared = unsafe { &*shared };
        if message == CALLBACK_MESSAGE {
            // Version 4: the event is in the low word of `lparam`.
            let event = (lparam.0 as u32) & 0xFFFF;
            if event == NIN_SELECT || event == NIN_KEYSELECT || event == WM_CONTEXTMENU {
                shared.clicked(hwnd);
            }
            return LRESULT(0);
        }
        if message == shared.taskbar_created {
            shared.add(hwnd);
            return LRESULT(0);
        }
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

impl WindowsTray {
    pub(crate) fn new(
        icon: &TrayIcon,
        click: ClickCallback,
        display_for: DisplayLookup,
    ) -> anyhow::Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_procedure),
                hInstance: instance.into(),
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            // Registering twice fails harmlessly; the class is the same each time.
            RegisterClassW(&class);
            // A tool window has no taskbar button, and this one is never shown.
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                CLASS_NAME,
                None,
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let side = GetSystemMetrics(SM_CXSMICON).max(16) * 2;
            let icon_handle = match CreateIconFromResourceEx(
                icon.image,
                true,
                0x0003_0000,
                side,
                side,
                LR_DEFAULTCOLOR,
            ) {
                Ok(handle) => handle,
                Err(error) => {
                    if let Err(destroy_error) = DestroyWindow(hwnd) {
                        log::debug!("could not destroy the tray window: {destroy_error}");
                    }
                    return Err(error.into());
                }
            };
            let shared = Box::new(Shared {
                click,
                display_for,
                icon: icon_handle,
                tooltip: wide(&icon.tooltip, 128),
                taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*shared as *const Shared as isize);
            shared.add(hwnd);
            Ok(Self { hwnd, shared })
        }
    }

    pub(crate) fn anchor(&self) -> Option<TrayIconAnchor> {
        self.shared.anchor(self.hwnd)
    }

    /// Whether the icon sits on the taskbar itself rather than in its overflow.
    pub(crate) fn visible(&self) -> bool {
        let Some(icon) = self.shared.icon_rect(self.hwnd) else {
            return false;
        };
        taskbar().is_some_and(|(_, bar)| intersects(&icon, &bar))
    }
}

impl Drop for WindowsTray {
    fn drop(&mut self) {
        let data = self.shared.data(self.hwnd);
        unsafe {
            if !Shell_NotifyIconW(NIM_DELETE, &data).as_bool() {
                log::debug!("the tray icon was already gone");
            }
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            if let Err(error) = DestroyWindow(self.hwnd) {
                log::debug!("could not destroy the tray window: {error}");
            }
            if let Err(error) = DestroyIcon(self.shared.icon) {
                log::debug!("could not destroy the tray icon: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltips_are_null_terminated_and_cut_to_fit() {
        assert_eq!(wide("Zode", 128), [90, 111, 100, 101, 0]);
        let long = wide(&"x".repeat(200), 128);
        assert_eq!(long.len(), 128);
        assert_eq!(long.last(), Some(&0));
    }

    #[test]
    fn rectangles_that_touch_do_not_intersect() {
        let a = RECT {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10,
        };
        let b = RECT {
            left: 10,
            top: 0,
            right: 20,
            bottom: 10,
        };
        let c = RECT {
            left: 5,
            top: 5,
            right: 15,
            bottom: 15,
        };
        assert!(!intersects(&a, &b));
        assert!(intersects(&a, &c));
    }
}
