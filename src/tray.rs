//! System-tray icon with a right-click menu: reload/edit the config, toggle
//! "Start at login" (an `HKCU\...\Run` registry entry), and quit.
//!
//! The icon lives on its own hidden top-level window so the popup menu gets
//! proper foreground focus (a taskbar-child window can't). Menu actions that
//! touch the running bars are posted to the driver window; the rest are handled
//! here.

use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{
    HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM, ERROR_SUCCESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, LoadImageW, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, SM_CXSMICON, SM_CYSMICON,
};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, GetCursorPos,
    GetWindowLongPtrW, LoadIconW, PostMessageW, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, SetWindowLongPtrW, TrackPopupMenu, GWLP_USERDATA, IDI_APPLICATION,
    MF_CHECKED, MF_SEPARATOR, MF_STRING, SW_SHOWNORMAL, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP,
    WM_CONTEXTMENU, WM_DESTROY, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

/// Tray-icon callback message (icon -> our window).
const WM_TRAY: u32 = WM_APP + 100;
/// Tray-icon id within our window (only one icon).
const TRAY_UID: u32 = 1;

// Popup-menu command ids.
const ID_RELOAD: usize = 1;
const ID_EDIT: usize = 2;
const ID_STARTUP: usize = 3;
const ID_QUIT: usize = 4;

const RUN_SUBKEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Winbar");

/// Per-window tray state, owned via `GWLP_USERDATA`.
struct TrayState {
    nid: NOTIFYICONDATAW,
    /// The driver (primary bar) window, for posting reload requests.
    driver: HWND,
    /// The config file the app watches, for "Edit config".
    path: PathBuf,
}

/// Create the hidden tray window and add its notification icon. `driver` is the
/// primary bar window (reload target); `path` is the watched config file.
pub fn install(instance: HINSTANCE, driver: HWND, path: PathBuf) -> Result<()> {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(tray_wndproc),
            hInstance: instance,
            lpszClassName: w!("WinbarTrayWindow"),
            ..Default::default()
        };
        RegisterClassW(&wc);

        // A normal top-level window, never shown; WS_EX_TOOLWINDOW keeps it off
        // the taskbar. It exists only to own the icon and host the popup menu.
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("WinbarTrayWindow"),
            w!("Winbar"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            instance,
            None,
        )?;

        // Load the embedded app icon (resource id 1, set in build.rs) at the
        // system small-icon size so the tray gets the crisp 16px frame rather
        // than a downscaled large one. Fall back to the stock icon if missing.
        let hicon = LoadImageW(
            instance,
            PCWSTR(1 as *const u16),
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_DEFAULTCOLOR,
        )
        .map(|h| HICON(h.0))
        .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default());

        let mut nid = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_UID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: hicon,
            ..Default::default()
        };
        let tip: Vec<u16> = "Winbar".encode_utf16().collect();
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);

        let state = Box::new(TrayState { nid, driver, path });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        Ok(())
    }
}

extern "system" fn tray_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TRAY => {
                // The low word of lParam is the mouse event over the icon.
                let event = (lparam.0 as u32) & 0xFFFF;
                if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                    show_menu(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayState;
                if !ptr.is_null() {
                    let state = Box::from_raw(ptr);
                    let _ = Shell_NotifyIconW(NIM_DELETE, &state.nid);
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Build and show the right-click popup menu, then act on the chosen item.
unsafe fn show_menu(hwnd: HWND) {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayState;
    if ptr.is_null() {
        return;
    }
    let state = &*ptr;

    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let _ = AppendMenuW(menu, MF_STRING, ID_RELOAD, w!("Reload config"));
    let _ = AppendMenuW(menu, MF_STRING, ID_EDIT, w!("Edit config"));
    let startup = MF_STRING | if startup_enabled() { MF_CHECKED } else { Default::default() };
    let _ = AppendMenuW(menu, startup, ID_STARTUP, w!("Start at login"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("Quit"));

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    // Required so the menu dismisses correctly when clicking elsewhere.
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_RETURNCMD,
        pt.x,
        pt.y,
        0,
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));

    match cmd.0 as usize {
        ID_RELOAD => {
            let _ = PostMessageW(state.driver, crate::window::WM_APP_RELOAD, WPARAM(0), LPARAM(0));
        }
        ID_EDIT => edit_config(hwnd, &state.path),
        ID_STARTUP => {
            let _ = set_startup(!startup_enabled());
        }
        ID_QUIT => {
            let _ = Shell_NotifyIconW(NIM_DELETE, &state.nid);
            PostQuitMessage(0);
        }
        _ => {}
    }
}

/// Open the config in the default editor, creating it from the built-in default
/// first if it doesn't exist yet (single-exe first run).
unsafe fn edit_config(hwnd: HWND, path: &Path) {
    if !path.exists() {
        let _ = std::fs::write(path, crate::config::DEFAULT_CONFIG);
    }
    let file: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    ShellExecuteW(
        hwnd,
        w!("open"),
        PCWSTR(file.as_ptr()),
        PCWSTR::null(),
        PCWSTR::null(),
        SW_SHOWNORMAL,
    );
}

/// Whether the `Winbar` value exists under the per-user `Run` key.
pub fn startup_enabled() -> bool {
    unsafe {
        let mut size = 0u32;
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_SUBKEY,
            RUN_VALUE,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        ) == ERROR_SUCCESS
    }
}

/// Add or remove the per-user `Run` entry pointing at this executable.
pub fn set_startup(enable: bool) -> bool {
    unsafe {
        if enable {
            let exe = std::env::current_exe().unwrap_or_default();
            let quoted = format!("\"{}\"", exe.display());
            let data: Vec<u16> =
                quoted.encode_utf16().chain(std::iter::once(0)).collect();
            let cb = (data.len() * 2) as u32;
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_SUBKEY,
                RUN_VALUE,
                REG_SZ.0,
                Some(data.as_ptr() as *const c_void),
                cb,
            ) == ERROR_SUCCESS
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_SUBKEY, RUN_VALUE) == ERROR_SUCCESS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Exercises the real HKCU `Run` key, so it's ignored by default. Run with:
    //   cargo test -- --ignored startup_roundtrip
    // Restores the prior state on exit.
    #[test]
    #[ignore]
    fn startup_roundtrip() {
        let was = startup_enabled();

        assert!(set_startup(true), "enabling should succeed");
        assert!(startup_enabled(), "value should read back as present");

        assert!(set_startup(false), "disabling should succeed");
        assert!(!startup_enabled(), "value should be gone after disable");

        if was {
            let _ = set_startup(true);
        }
    }
}
