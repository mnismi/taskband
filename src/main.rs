mod taskbar;
mod window;

fn main() -> windows::core::Result<()> {
    let _hwnd = window::create_window()?;
    println!("Standalone vEnter window created near the top-left of the screen.");
    window::run_message_loop();
    Ok(())
}
