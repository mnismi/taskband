use std::mem::size_of;

use windows::core::{w, Result};
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{BOOL, ERROR_SUCCESS, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW, MONITORINFOF_PRIMARY};

/// One display and the taskbar (if any) sitting on it. `index` is the monitor's
/// position in `EnumDisplayMonitors` order.
pub struct MonitorInfo {
    pub index: usize,
    pub rect: RECT,
    pub primary: bool,
    pub hmonitor: HMONITOR,
    pub taskbar: Option<HWND>,
}

/// Locate the native primary taskbar window (`Shell_TrayWnd`).
pub fn find_taskbar() -> Result<HWND> {
    // Safety: FindWindowW only queries the window manager; there are no
    // invariants for the caller to uphold.
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }
}

/// `EnumDisplayMonitors` callback: push one `MonitorInfo` per monitor.
unsafe extern "system" fn enum_proc(
    hmon: HMONITOR,
    _hdc: HDC,
    _rc: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let monitors = &mut *(data.0 as *mut Vec<MonitorInfo>);
    let mut mi = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(hmon, &mut mi).as_bool() {
        let index = monitors.len();
        monitors.push(MonitorInfo {
            index,
            rect: mi.rcMonitor,
            primary: mi.dwFlags & MONITORINFOF_PRIMARY != 0,
            hmonitor: hmon,
            taskbar: None,
        });
    }
    TRUE
}

/// All monitors in enumeration order (taskbars not yet attached).
fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mut monitors: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM(&mut monitors as *mut Vec<MonitorInfo> as isize),
        );
    }
    monitors
}

/// Every secondary taskbar window (`Shell_SecondaryTrayWnd`), one per secondary
/// monitor when "show taskbar on all displays" is enabled.
fn find_secondary_taskbars() -> Vec<HWND> {
    let mut taskbars = Vec::new();
    unsafe {
        // Null `hwndchildafter` starts the search from the beginning; each match
        // becomes the next search's cursor.
        let mut prev = HWND::default();
        while let Ok(h) = FindWindowExW(None, prev, w!("Shell_SecondaryTrayWnd"), None) {
            if h == HWND::default() {
                break;
            }
            taskbars.push(h);
            prev = h;
        }
    }
    taskbars
}

/// Detect all monitors and attach each taskbar (primary + secondaries) to the
/// monitor it sits on, via `MonitorFromWindow`.
pub fn detect() -> Vec<MonitorInfo> {
    let mut monitors = enumerate_monitors();

    let mut taskbars = Vec::new();
    if let Ok(primary) = find_taskbar() {
        if primary != HWND::default() {
            taskbars.push(primary);
        }
    }
    taskbars.extend(find_secondary_taskbars());

    for tb in taskbars {
        let hmon = unsafe { MonitorFromWindow(tb, MONITOR_DEFAULTTONEAREST) };
        if let Some(m) = monitors.iter_mut().find(|m| m.hmonitor == hmon) {
            m.taskbar = Some(tb);
        }
    }
    monitors
}

/// The display device name (e.g. `\\.\DISPLAY1`) for a monitor handle, or an
/// empty string if the query fails.
pub fn device_name(hmonitor: HMONITOR) -> String {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let ok = unsafe { GetMonitorInfoW(hmonitor, &mut info as *mut _ as *mut MONITORINFO) };
    if ok.as_bool() {
        utf16_until_nul(&info.szDevice)
    } else {
        String::new()
    }
}

/// The monitor's human-readable model name from its EDID (e.g. `DELL U2515H`),
/// falling back to the GDI device name (e.g. `\\.\DISPLAY1`) when the
/// DisplayConfig query fails or the monitor reports no name.
pub fn display_name(hmonitor: HMONITOR) -> String {
    let gdi = device_name(hmonitor);
    friendly_name_for_gdi(&gdi).unwrap_or(gdi)
}

/// Resolve a GDI device name to the attached monitor's friendly name via the
/// DisplayConfig API: enumerate active display paths, match each path's source
/// (the GDI device) against `gdi`, then read the path target's EDID name.
fn friendly_name_for_gdi(gdi: &str) -> Option<String> {
    if gdi.is_empty() {
        return None;
    }
    unsafe {
        let (mut n_paths, mut n_modes) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut n_paths, &mut n_modes)
            != ERROR_SUCCESS
        {
            return None;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); n_paths as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); n_modes as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut n_paths,
            paths.as_mut_ptr(),
            &mut n_modes,
            modes.as_mut_ptr(),
            None,
        ) != ERROR_SUCCESS
        {
            return None;
        }
        paths.truncate(n_paths as usize);

        for path in &paths {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                    adapterId: path.sourceInfo.adapterId,
                    id: path.sourceInfo.id,
                },
                ..Default::default()
            };
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0
                || utf16_until_nul(&source.viewGdiDeviceName) != gdi
            {
                continue;
            }

            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                    size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                    adapterId: path.targetInfo.adapterId,
                    id: path.targetInfo.id,
                },
                ..Default::default()
            };
            if DisplayConfigGetDeviceInfo(&mut target.header) != 0 {
                continue;
            }
            let name = utf16_until_nul(&target.monitorFriendlyDeviceName);
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Decode a NUL-terminated UTF-16 buffer (whole buffer if no NUL).
fn utf16_until_nul(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// A one-line human-readable description of a monitor for the startup log.
pub fn monitor_log_line(m: &MonitorInfo) -> String {
    let w = m.rect.right - m.rect.left;
    let h = m.rect.bottom - m.rect.top;
    format!(
        "  [{}] {}x{} @ ({},{}){}   taskbar: {}",
        m.index,
        w,
        h,
        m.rect.left,
        m.rect.top,
        if m.primary {
            "   primary"
        } else {
            "          "
        },
        if m.taskbar.is_some() { "yes" } else { "no" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::HMONITOR;

    #[test]
    fn find_taskbar_returns_a_handle() {
        let hwnd = find_taskbar().expect("Shell_TrayWnd should exist while explorer is running");
        assert_ne!(
            hwnd,
            HWND::default(),
            "taskbar handle should not be the null handle"
        );
    }

    #[test]
    fn detects_exactly_one_primary_with_a_taskbar() {
        let monitors = detect();
        assert!(
            !monitors.is_empty(),
            "at least one monitor should be detected"
        );
        assert_eq!(
            monitors.iter().filter(|m| m.primary).count(),
            1,
            "exactly one primary monitor"
        );
        let primary = monitors.iter().find(|m| m.primary).unwrap();
        assert!(
            primary.taskbar.is_some(),
            "the primary monitor has the Shell_TrayWnd taskbar"
        );
    }

    #[test]
    fn display_name_of_primary_is_not_empty() {
        let monitors = detect();
        let primary = monitors.iter().find(|m| m.primary).unwrap();
        let name = display_name(primary.hmonitor);
        assert!(!name.is_empty(), "expected a friendly or GDI name");
    }

    #[test]
    fn monitor_log_line_formats_index_size_and_taskbar() {
        let m = MonitorInfo {
            index: 1,
            rect: RECT {
                left: 1920,
                top: 0,
                right: 3840,
                bottom: 1080,
            },
            primary: false,
            hmonitor: HMONITOR::default(),
            taskbar: None,
        };
        let line = monitor_log_line(&m);
        assert!(line.contains("[1]"), "line was: {line}");
        assert!(line.contains("1920x1080"), "line was: {line}");
        assert!(line.contains("@ (1920,0)"), "line was: {line}");
        assert!(line.contains("taskbar: no"), "line was: {line}");
    }
}
