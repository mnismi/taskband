use std::sync::mpsc::Receiver;

use windows::core::{w, PCWSTR, Result};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, TRUE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC,
    InvalidateRect, ReleaseDC, SelectObject, SetBkMode, SetTextColor, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DT_CALCRECT, DT_LEFT, DT_SINGLELINE,
    DT_VCENTER, FF_DONTCARE, HDC, HFONT, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW, GetParent,
    GetWindow, GetWindowLongPtrW, GetWindowRect, IsWindowVisible, KillTimer, PostQuitMessage,
    RegisterClassW, SetLayeredWindowAttributes, SetParent, SetTimer, SetWindowLongPtrW,
    SetWindowPos, TranslateMessage, GWLP_USERDATA, GWL_STYLE, GW_CHILD, GW_HWNDNEXT, HWND_TOP,
    LWA_COLORKEY, MSG, SWP_SHOWWINDOW, WINDOW_STYLE, WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSW,
    WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_POPUP, WS_VISIBLE,
};

use crate::css::Style;
use crate::plugin::Update;

const TIMER_ID: usize = 1;
const TIMER_MS: u32 = 250;
const GAP: i32 = 8;

/// UI-thread render state, attached to the window via GWLP_USERDATA. Only the UI
/// thread touches it. The worker thread owns nothing here — it only sends Updates.
pub struct State {
    texts: Vec<String>,
    styles: Vec<Style>,
    widths: Vec<i32>,
    offsets: Vec<i32>,
    total_width: i32,
    rx: Receiver<Update>,
}

impl State {
    pub fn new(styles: Vec<Style>, rx: Receiver<Update>) -> Self {
        let n = styles.len();
        State {
            texts: vec![String::new(); n],
            widths: vec![0; n],
            offsets: vec![0; n],
            total_width: 0,
            styles,
            rx,
        }
    }
}

/// Build a GDI font from a resolved style. Caller must DeleteObject it.
unsafe fn make_font(style: &Style) -> HFONT {
    let mut face: Vec<u16> = style.font_family.encode_utf16().collect();
    face.push(0);
    CreateFontW(
        -style.font_size, // negative => character height in logical (pixel) units
        0,
        0,
        0,
        style.font_weight,
        0, // italic
        0, // underline
        0, // strikeout
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        PCWSTR(face.as_ptr()),
    )
}

/// Measure a module's full width: text extent + horizontal padding + margin.
unsafe fn measure(hdc: HDC, style: &Style, text: &str) -> i32 {
    let font = make_font(style);
    let old = SelectObject(hdc, font);
    // DrawTextW with an empty slice dereferences a dangling pointer (AV), so
    // only measure when there is text; empty text contributes zero text width.
    let text_w = if text.is_empty() {
        0
    } else {
        let mut utf16: Vec<u16> = text.encode_utf16().collect();
        let mut r = RECT::default();
        DrawTextW(hdc, &mut utf16, &mut r, DT_CALCRECT | DT_SINGLELINE | DT_LEFT);
        r.right - r.left
    };
    SelectObject(hdc, old);
    let _ = DeleteObject(font);
    text_w
        + style.padding.left
        + style.padding.right
        + style.margin.left
        + style.margin.right
}

/// Re-measure all modules against current text and recompute offsets/total width.
unsafe fn relayout(hwnd: HWND, state: &mut State) {
    let hdc = GetDC(hwnd);
    for i in 0..state.styles.len() {
        state.widths[i] = measure(hdc, &state.styles[i], &state.texts[i]);
    }
    ReleaseDC(hwnd, hdc);
    let (offsets, total) = crate::layout::place_modules(&state.widths);
    state.offsets = offsets;
    state.total_width = total;
}

/// Create the layered taskbar window and attach its render state.
pub fn create_window(state: Box<State>) -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("vEnterTaskbarWindow"),
            hbrBackground: CreateSolidBrush(COLORREF(0x0000_0000)), // black = transparent key
            ..Default::default()
        };
        RegisterClassW(&wc);

        // WS_EX_LAYERED is required: the Windows 11 taskbar only composites
        // layered windows. Color-key black => transparent (see design doc).
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED,
            w!("vEnterTaskbarWindow"),
            w!("vEnter"),
            WS_POPUP | WS_VISIBLE | WS_CLIPSIBLINGS,
            100,
            100,
            260,
            40,
            None,
            None,
            instance,
            None,
        )?;

        SetLayeredWindowAttributes(hwnd, COLORREF(0x0000_0000), 0, LWA_COLORKEY)?;

        // Hand ownership of State to the window.
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

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

/// Recompute where the bar should sit (just left of the tray / embedded apps)
/// using the current total width, and move it there only if something changed.
fn reposition(hwnd: HWND) {
    unsafe {
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
        let width = if state_ptr.is_null() {
            0
        } else {
            (*state_ptr).total_width
        };

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

        let x = crate::layout::compute_x(taskbar_left, taskbar_width, &obstacles, width, GAP);

        let mut cur = RECT::default();
        if GetWindowRect(hwnd, &mut cur).is_err() {
            return;
        }
        let cur_x = cur.left - taskbar_left;
        let cur_w = cur.right - cur.left;
        let cur_h = cur.bottom - cur.top;
        if cur_x != x || cur_w != width || cur_h != tb_height {
            let _ = SetWindowPos(hwnd, HWND_TOP, x, 0, width, tb_height, SWP_SHOWWINDOW);
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

                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
                if !state_ptr.is_null() {
                    let state = &*state_ptr;
                    let mut client = RECT::default();
                    let _ = GetClientRect(hwnd, &mut client);
                    let height = client.bottom - client.top;

                    for i in 0..state.styles.len() {
                        let style = &state.styles[i];
                        let x0 = state.offsets[i];
                        let w = state.widths[i];

                        // Module box excludes its margins.
                        let left = x0 + style.margin.left;
                        let right = x0 + w - style.margin.right;
                        let mrect = RECT { left, top: 0, right, bottom: height };

                        // Background: real color if set, else the transparency key (black).
                        let bg = match style.background {
                            Some(c) => c.colorref(),
                            None => 0x0000_0000,
                        };
                        let brush = CreateSolidBrush(COLORREF(bg));
                        FillRect(hdc, &mrect, brush);
                        let _ = DeleteObject(brush);

                        // Text within the padded area.
                        let font = make_font(style);
                        let old = SelectObject(hdc, font);
                        SetBkMode(hdc, TRANSPARENT);
                        SetTextColor(hdc, COLORREF(style.color.colorref()));
                        let mut trect = RECT {
                            left: left + style.padding.left,
                            top: 0,
                            right: right - style.padding.right,
                            bottom: height,
                        };
                        if !state.texts[i].is_empty() {
                            let mut utf16: Vec<u16> = state.texts[i].encode_utf16().collect();
                            DrawTextW(hdc, &mut utf16, &mut trect, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                        }
                        SelectObject(hdc, old);
                        let _ = DeleteObject(font);
                    }
                }

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_TIMER => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    let mut changed = false;
                    while let Ok(update) = state.rx.try_recv() {
                        if update.index < state.texts.len()
                            && state.texts[update.index] != update.text
                        {
                            state.texts[update.index] = update.text;
                            changed = true;
                        }
                    }
                    if changed {
                        relayout(hwnd, state);
                        let _ = InvalidateRect(hwnd, None, TRUE);
                    }
                }
                reposition(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                let _ = KillTimer(hwnd, TIMER_ID);
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
                if !ptr.is_null() {
                    drop(Box::from_raw(ptr)); // drops State + rx (worker stops)
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
