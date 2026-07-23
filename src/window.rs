use windows::core::{w, Result};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DrawTextW, EndPaint, SetBkMode, SetTextColor,
    DT_LEFT, DT_SINGLELINE, DT_VCENTER, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, GetWindowRect, PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes,
    SetParent, SetWindowLongPtrW, SetWindowPos, TranslateMessage, GWL_STYLE, LWA_ALPHA, MSG,
    SWP_FRAMECHANGED, SWP_NOZORDER, SWP_SHOWWINDOW, WINDOW_STYLE, WM_DESTROY, WM_PAINT, WNDCLASSW,
    WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_POPUP, WS_VISIBLE,
};

/// Create a standalone, visible window that paints the spike text.
pub fn create_window() -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("vEnterTaskbarWindow"),
            hbrBackground: CreateSolidBrush(COLORREF(0x0000_0000)), // black
            ..Default::default()
        };
        RegisterClassW(&wc);

        // WS_EX_LAYERED is required: on Windows 11 the taskbar is DWM-composited
        // and only composites layered windows. A plain GDI child reparented into
        // Shell_TrayWnd stays invisible regardless of position or z-order.
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED,
            w!("vEnterTaskbarWindow"),
            w!("vEnter ▲ hello"),
            WS_POPUP | WS_VISIBLE | WS_CLIPSIBLINGS,
            100, 100,   // x, y on screen
            260, 40,    // width, height
            None,       // no parent (top-level for now)
            None,       // no menu
            instance,   // module handle
            None,       // no create param
        )?;

        // A layered window is invisible until its attributes are set. Make it
        // fully opaque; it then paints normally through WM_PAINT.
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)?;

        Ok(hwnd)
    }
}

/// Reparent `child` into the taskbar (`taskbar`), turn it into a child
/// window, and position it inside the taskbar just left of the tray area.
pub fn embed_in_taskbar(child: HWND, taskbar: HWND) -> Result<()> {
    unsafe {
        // 1. Reparent our window into the taskbar.
        SetParent(child, taskbar)?;

        // 2. Convert it to a child window so it clips to and moves with the taskbar.
        let current = WINDOW_STYLE(GetWindowLongPtrW(child, GWL_STYLE) as u32);
        let child_style = (current & !WS_POPUP) | WS_CHILD | WS_VISIBLE;
        SetWindowLongPtrW(child, GWL_STYLE, child_style.0 as isize);

        // 3. Position inside the taskbar, leaving room on the right for the tray/clock.
        let mut tb = RECT::default();
        GetWindowRect(taskbar, &mut tb)?;
        let tb_width = tb.right - tb.left;
        let tb_height = tb.bottom - tb.top;

        let width = 260;
        // Clearance leaves room on the right for the tray/clock and any existing
        // embedded app (e.g. TrafficMonitor). Tuned for a 1920-wide taskbar; this
        // is the spot the diagnostics confirmed renders in a clear area.
        let x = (tb_width - width - 610).max(0);
        SetWindowPos(
            child,
            None,
            x, 0, width, tb_height,
            SWP_NOZORDER | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
        )?;

        Ok(())
    }
}

/// Blocking Win32 message loop. Returns when the window is destroyed.
pub fn run_message_loop() {
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);

                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);

                SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, COLORREF(0x00FF_FFFF)); // white

                let mut text: Vec<u16> = "vEnter ▲ hello".encode_utf16().collect();
                DrawTextW(hdc, &mut text, &mut rect, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
