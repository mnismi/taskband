mod taskbar;

fn main() {
    match taskbar::find_taskbar() {
        Ok(hwnd) => println!("Found taskbar (Shell_TrayWnd): {hwnd:?}"),
        Err(e) => eprintln!("Failed to find taskbar: {e}"),
    }
}
