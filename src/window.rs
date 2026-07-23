use windows::core::{w, Result};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DrawTextW, EndPaint, SetBkMode, SetTextColor,
    DT_LEFT, DT_SINGLELINE, DT_VCENTER, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW, GetParent,
    GetWindow, GetWindowLongPtrW, GetWindowRect, IsWindowVisible, KillTimer, PostQuitMessage,
    RegisterClassW, SetLayeredWindowAttributes, SetParent, SetTimer, SetWindowLongPtrW,
    SetWindowPos, TranslateMessage, GWL_STYLE, GW_CHILD, GW_HWNDNEXT, HWND_TOP, LWA_COLORKEY, MSG,
    SWP_SHOWWINDOW, WINDOW_STYLE, WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_POPUP, WS_VISIBLE,
};

const TIMER_ID: usize = 1;
const TIMER_MS: u32 = 250;
const WIDTH: i32 = 260;
const GAP: i32 = 8;

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

        // A layered window is invisible until its attributes are set. Use a
        // color key of black (the class background fill): every black pixel
        // becomes fully transparent so the taskbar shows through, while the
        // painted text (white) stays opaque. This gives a transparent
        // background instead of a solid black box.
        SetLayeredWindowAttributes(hwnd, COLORREF(0x0000_0000), 0, LWA_COLORKEY)?;

        Ok(hwnd)
    }
}

/// Reparent `child` into the taskbar, make it a child window, place it, and
/// start a timer that keeps it parked just left of the tray / embedded apps.
pub fn embed_in_taskbar(child: HWND, taskbar: HWND) -> Result<()> {
    unsafe {
        SetParent(child, taskbar)?;

        let current = WINDOW_STYLE(GetWindowLongPtrW(child, GWL_STYLE) as u32);
        let child_style = (current & !WS_POPUP) | WS_CHILD | WS_VISIBLE;
        SetWindowLongPtrW(child, GWL_STYLE, child_style.0 as isize);

        reposition(child); // initial placement
        SetTimer(child, TIMER_ID, TIMER_MS, None); // keep tracking taskbar changes
        Ok(())
    }
}

/// Recompute where the window should sit (just left of the tray / embedded
/// apps) and move it there, but only if the target changed (avoids flicker).
fn reposition(hwnd: HWND) {
    unsafe {
        let Ok(taskbar) = GetParent(hwnd) else {
            return;
        };
        let mut tb = RECT::default();
        if GetWindowRect(taskbar, &mut tb).is_err() {
            return;
        }
        let taskbar_left = tb.left;
        let taskbar_width = tb.right - tb.left;
        let tb_height = tb.bottom - tb.top;

        // Obstacle = a visible sibling in the right half that is not full-width
        // (excludes the full-width XAML content bridge) and not our own window.
        let mut obstacles: Vec<i32> = Vec::new();
        let mut sib = GetWindow(taskbar, GW_CHILD).ok();
        while let Some(h) = sib {
            if h != hwnd && IsWindowVisible(h).as_bool() {
                let mut r = RECT::default();
                if GetWindowRect(h, &mut r).is_ok() {
                    let w = r.right - r.left;
                    if r.left > taskbar_left + taskbar_width / 2 && w < taskbar_width {
                        obstacles.push(r.left);
                    }
                }
            }
            sib = GetWindow(h, GW_HWNDNEXT).ok();
        }

        let x = crate::layout::compute_x(taskbar_left, taskbar_width, &obstacles, WIDTH, GAP);

        let mut cur = RECT::default();
        if GetWindowRect(hwnd, &mut cur).is_err() {
            return;
        }
        let cur_x = cur.left - taskbar_left;
        let cur_h = cur.bottom - cur.top;
        if cur_x != x || cur_h != tb_height {
            let _ = SetWindowPos(hwnd, HWND_TOP, x, 0, WIDTH, tb_height, SWP_SHOWWINDOW);
        }
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
            WM_TIMER => {
                reposition(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                let _ = KillTimer(hwnd, TIMER_ID);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
