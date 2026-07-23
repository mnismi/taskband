use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::SystemTime;

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, TRUE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC,
    GetTextMetricsW, InvalidateRect, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DRAW_TEXT_FORMAT,
    DT_CALCRECT, DT_CENTER, DT_LEFT, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, HDC, HFONT,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, TEXTMETRICW, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW, GetWindow,
    GetWindowLongPtrW, GetWindowRect, IsWindowVisible, KillTimer, LoadCursorW, PostQuitMessage,
    RegisterClassW, SetLayeredWindowAttributes, SetParent, SetTimer, SetWindowLongPtrW,
    SetWindowPos, TranslateMessage, GWLP_USERDATA, GWL_STYLE, GW_CHILD, GW_HWNDNEXT, HWND_TOP,
    IDC_ARROW, LWA_COLORKEY, MSG, SWP_SHOWWINDOW, WINDOW_STYLE, WM_APP, WM_DESTROY, WM_PAINT,
    WM_TIMER, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_POPUP, WS_VISIBLE,
};

use crate::css::{Style, TextAlign};
use crate::plugin::Update;

const TIMER_ID: usize = 1;
const TIMER_MS: u32 = 250;
const GAP: i32 = 8;

/// Posted to the driver window by the tray's "Reload config" item. Clearing the
/// tracked mtime makes the next timer tick treat the config as changed and reload
/// it (re-running every module immediately).
pub const WM_APP_RELOAD: u32 = WM_APP + 1;

/// One monitor's bar: its layered child window on that monitor's taskbar plus the
/// layout of the module slots it shows. Slots index into `App::texts`/`styles`.
pub struct Bar {
    hwnd: HWND,
    taskbar: HWND,
    monitor_index: usize,
    primary: bool,
    modules: Vec<usize>,
    widths: Vec<i32>,
    offsets: Vec<i32>,
    total_width: i32,
    /// Pixels reserved at the taskbar's right edge for the OS clock. Zero on the
    /// primary (its tray is a detectable obstacle); the configured value on a
    /// secondary, whose clock has no obstacle window.
    clock_reserve: i32,
}

impl Bar {
    pub fn new(
        hwnd: HWND,
        taskbar: HWND,
        monitor_index: usize,
        primary: bool,
        modules: Vec<usize>,
        clock_reserve: i32,
    ) -> Self {
        let n = modules.len();
        Bar {
            hwnd,
            taskbar,
            monitor_index,
            primary,
            modules,
            widths: vec![0; n],
            offsets: vec![0; n],
            total_width: 0,
            clock_reserve: if primary { 0 } else { clock_reserve },
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

/// Shared UI-thread render state: per-slot texts/styles plus every monitor's bar.
/// Only the UI thread touches it; the worker thread only sends `Update`s.
pub struct App {
    texts: Vec<String>,
    styles: Vec<Style>,
    rx: Receiver<Update>,
    path: PathBuf,
    mtime: Option<SystemTime>,
    bars: Vec<Bar>,
}

impl App {
    pub fn new(styles: Vec<Style>, rx: Receiver<Update>, path: PathBuf, bars: Vec<Bar>) -> Self {
        let mtime = file_mtime(&path);
        App {
            texts: vec![String::new(); styles.len()],
            styles,
            rx,
            path,
            mtime,
            bars,
        }
    }
}

/// Last-modified time of a file, or None if it can't be read.
fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// If the config changed on disk, re-parse it and rebuild the registry + each
/// bar's slot list with a fresh worker. A config that fails to parse is ignored,
/// keeping the running config. Returns true if a reload was applied.
unsafe fn maybe_reload(app: &mut App) -> bool {
    let current = file_mtime(&app.path);
    if current == app.mtime {
        return false;
    }
    app.mtime = current;

    let cfg = match crate::config::load(&app.path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Winbar: reload skipped — {e}");
            return false;
        }
    };

    let build = crate::config::build_registry(&cfg);
    let n = build.styles.len();
    // Spawn the new worker first; assigning its receiver drops the old one,
    // which makes the old worker exit on its next send.
    let rx = crate::plugin::spawn_worker(build.specs);
    app.styles = build.styles;
    app.texts = vec![String::new(); n];
    app.rx = rx;
    for bar in &mut app.bars {
        let slots = crate::config::slots_for_monitor(
            &build.monitors,
            &build.legacy,
            bar.monitor_index,
            bar.primary,
        );
        let m = slots.len();
        bar.modules = slots;
        bar.widths = vec![0; m];
        bar.offsets = vec![0; m];
        bar.total_width = 0;
        bar.clock_reserve = if bar.primary { 0 } else { build.clock_reserve };
    }
    println!("Winbar: reloaded config — {n} module slot(s).");
    true
}

/// Build a GDI font from a resolved style. Caller must DeleteObject it.
unsafe fn make_font(style: &Style) -> HFONT {
    let mut face: Vec<u16> = style.font_family.encode_utf16().collect();
    face.push(0);
    CreateFontW(
        -style.font_size,
        0,
        0,
        0,
        style.font_weight,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        PCWSTR(face.as_ptr()),
    )
}

/// The DrawTextW horizontal-alignment flag for a resolved text alignment.
fn align_flag(align: TextAlign) -> DRAW_TEXT_FORMAT {
    match align {
        TextAlign::Left => DT_LEFT,
        TextAlign::Center => DT_CENTER,
        TextAlign::Right => DT_RIGHT,
    }
}

/// Measure a module's full width: widest line's extent + horizontal padding +
/// margin. For multi-line text the module is as wide as its longest line.
unsafe fn measure(hdc: HDC, style: &Style, text: &str) -> i32 {
    let font = make_font(style);
    let old = SelectObject(hdc, font);
    let mut text_w = 0;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let mut utf16: Vec<u16> = line.encode_utf16().collect();
        let mut r = RECT::default();
        DrawTextW(
            hdc,
            &mut utf16,
            &mut r,
            DT_CALCRECT | DT_SINGLELINE | DT_LEFT,
        );
        text_w = text_w.max(r.right - r.left);
    }
    SelectObject(hdc, old);
    let _ = DeleteObject(font);
    text_w + style.padding.left + style.padding.right + style.margin.left + style.margin.right
}

/// Re-measure one bar's modules against current text and recompute its layout.
unsafe fn relayout_bar(bar: &mut Bar, texts: &[String], styles: &[Style]) {
    let hdc = GetDC(bar.hwnd);
    bar.widths.clear();
    for &slot in &bar.modules {
        bar.widths.push(measure(hdc, &styles[slot], &texts[slot]));
    }
    ReleaseDC(bar.hwnd, hdc);
    let (offsets, total) = crate::layout::place_modules(&bar.widths);
    bar.offsets = offsets;
    bar.total_width = total;
}

/// Register the window class once. Returns the module instance for CreateWindow.
pub fn register_class() -> Result<HINSTANCE> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("WinbarTaskbarWindow"),
            hbrBackground: CreateSolidBrush(COLORREF(0x0000_0000)), // black = transparent key
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        Ok(instance.into())
    }
}

/// Create one layered bar window (class must already be registered).
pub fn create_bar_window(instance: HINSTANCE) -> Result<HWND> {
    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED,
            w!("WinbarTaskbarWindow"),
            w!("Winbar"),
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
        Ok(hwnd)
    }
}

/// Reparent `child` into `taskbar` and switch it to a child window.
pub fn embed_in_taskbar(child: HWND, taskbar: HWND) -> Result<()> {
    unsafe {
        SetParent(child, taskbar)?;
        let current = WINDOW_STYLE(GetWindowLongPtrW(child, GWL_STYLE) as u32);
        let child_style = (current & !WS_POPUP) | WS_CHILD | WS_VISIBLE;
        SetWindowLongPtrW(child, GWL_STYLE, child_style.0 as isize);
        Ok(())
    }
}

/// Take ownership of `app`, point every bar window at it, and start the single
/// driver timer on the `driver` window (the primary monitor's bar).
pub fn install(app: App, driver: HWND) {
    unsafe {
        let ptr = Box::into_raw(Box::new(app));
        for bar in &(*ptr).bars {
            SetWindowLongPtrW(bar.hwnd, GWLP_USERDATA, ptr as isize);
        }
        SetTimer(driver, TIMER_ID, TIMER_MS, None);
    }
}

/// Recompute where one bar should sit (just left of its taskbar's tray / embedded
/// apps) and move it there only if something changed.
fn reposition(bar: &Bar) {
    unsafe {
        let width = bar.total_width;
        let mut tb = RECT::default();
        if GetWindowRect(bar.taskbar, &mut tb).is_err() {
            return;
        }
        let taskbar_left = tb.left;
        let taskbar_width = tb.right - tb.left;
        let tb_height = tb.bottom - tb.top;

        // Obstacle = a visible sibling in the right half that is not full-width
        // (excludes the full-width XAML content bridge) and not our own window.
        let mut obstacles: Vec<i32> = Vec::new();
        let mut sib = GetWindow(bar.taskbar, GW_CHILD).ok();
        while let Some(h) = sib {
            if h != bar.hwnd && IsWindowVisible(h).as_bool() {
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

        let x = crate::layout::compute_x(
            taskbar_left,
            taskbar_width,
            &obstacles,
            width,
            GAP,
            bar.clock_reserve,
        );

        let mut cur = RECT::default();
        if GetWindowRect(bar.hwnd, &mut cur).is_err() {
            return;
        }
        let cur_x = cur.left - taskbar_left;
        let cur_w = cur.right - cur.left;
        let cur_h = cur.bottom - cur.top;
        if cur_x != x || cur_w != width || cur_h != tb_height {
            let _ = SetWindowPos(bar.hwnd, HWND_TOP, x, 0, width, tb_height, SWP_SHOWWINDOW);
        }
    }
}

/// Blocking Win32 message loop. Returns when the driver window is destroyed.
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

                let app_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const App;
                if !app_ptr.is_null() {
                    let app = &*app_ptr;
                    if let Some(bar) = app.bars.iter().find(|b| b.hwnd == hwnd) {
                        let mut client = RECT::default();
                        let _ = GetClientRect(hwnd, &mut client);
                        let height = client.bottom - client.top;

                        for i in 0..bar.modules.len() {
                            let slot = bar.modules[i];
                            let style = &app.styles[slot];
                            let x0 = bar.offsets[i];
                            let w = bar.widths[i];

                            let left = x0 + style.margin.left;
                            let right = x0 + w - style.margin.right;
                            let mrect = RECT {
                                left,
                                top: 0,
                                right,
                                bottom: height,
                            };

                            let bg = match style.background {
                                Some(c) => c.colorref(),
                                None => 0x0000_0000,
                            };
                            let brush = CreateSolidBrush(COLORREF(bg));
                            FillRect(hdc, &mrect, brush);
                            let _ = DeleteObject(brush);

                            let font = make_font(style);
                            let old = SelectObject(hdc, font);
                            SetBkMode(hdc, TRANSPARENT);
                            SetTextColor(hdc, COLORREF(style.color.colorref()));

                            let mut tm = TEXTMETRICW::default();
                            let line_h = if GetTextMetricsW(hdc, &mut tm).as_bool() {
                                tm.tmHeight + tm.tmExternalLeading
                            } else {
                                style.font_size
                            };

                            let text_left = left + style.padding.left;
                            let text_right = right - style.padding.right;
                            let lines: Vec<&str> = app.texts[slot].lines().collect();
                            let block_h = line_h * lines.len() as i32;
                            let mut y = ((height - block_h) / 2).max(0);
                            let flags = align_flag(style.text_align) | DT_VCENTER | DT_SINGLELINE;
                            for line in lines {
                                if !line.is_empty() {
                                    let mut lrect = RECT {
                                        left: text_left,
                                        top: y,
                                        right: text_right,
                                        bottom: y + line_h,
                                    };
                                    let mut utf16: Vec<u16> = line.encode_utf16().collect();
                                    DrawTextW(hdc, &mut utf16, &mut lrect, flags);
                                }
                                y += line_h;
                            }
                            SelectObject(hdc, old);
                            let _ = DeleteObject(font);
                        }
                    }
                }

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_TIMER => {
                let app_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                if !app_ptr.is_null() {
                    let app = &mut *app_ptr;
                    // Reload first, so `changed` is sized to the current texts.
                    let reloaded = maybe_reload(app);
                    let mut changed = vec![false; app.texts.len()];
                    while let Ok(update) = app.rx.try_recv() {
                        if update.index < app.texts.len() && app.texts[update.index] != update.text
                        {
                            app.texts[update.index] = update.text;
                            changed[update.index] = true;
                        }
                    }
                    let App {
                        texts,
                        styles,
                        bars,
                        ..
                    } = app;
                    for bar in bars.iter_mut() {
                        let affected = reloaded
                            || bar
                                .modules
                                .iter()
                                .any(|&s| changed.get(s).copied().unwrap_or(false));
                        if affected {
                            relayout_bar(bar, texts.as_slice(), styles.as_slice());
                        }
                        reposition(bar);
                        if affected {
                            let _ = InvalidateRect(bar.hwnd, None, TRUE);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_APP_RELOAD => {
                let app_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                if !app_ptr.is_null() {
                    // Force the next timer tick to see a change and reload.
                    (*app_ptr).mtime = None;
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                let app_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                if !app_ptr.is_null() {
                    let app = Box::from_raw(app_ptr);
                    // Null every bar's back-pointer (incl. self) so a sibling's later
                    // WM_DESTROY is a no-op, and stop the driver timer.
                    for bar in &app.bars {
                        let _ = KillTimer(bar.hwnd, TIMER_ID);
                        SetWindowLongPtrW(bar.hwnd, GWLP_USERDATA, 0);
                    }
                    drop(app); // drops rx (worker stops)
                }
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
