use windows::core::{w, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

/// Locate the native Windows taskbar window (`Shell_TrayWnd`).
pub fn find_taskbar() -> Result<HWND> {
    // Safety: FindWindowW only queries the window manager; there are no
    // invariants for the caller to uphold.
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::HWND;

    #[test]
    fn find_taskbar_returns_a_handle() {
        let hwnd = find_taskbar().expect("Shell_TrayWnd should exist while explorer is running");
        assert_ne!(hwnd, HWND::default(), "taskbar handle should not be the null handle");
    }
}
