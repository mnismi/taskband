mod config;
mod css;
mod layout;
mod plugin;
mod taskbar;
mod window;

fn main() -> windows::core::Result<()> {
    let taskbar = taskbar::find_taskbar()?;
    let child = window::create_window()?;
    window::embed_in_taskbar(child, taskbar)?;
    println!("vEnter embedded into the taskbar — look to the left of the tray/clock.");
    window::run_message_loop();
    Ok(())
}
