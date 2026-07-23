mod config;
mod css;
mod layout;
mod plugin;
mod taskbar;
mod window;

fn main() -> windows::core::Result<()> {
    let path = config::config_path();
    let cfg = match config::load(&path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("vEnter: {e}");
            std::process::exit(1);
        }
    };

    let (styles, specs) = config::build(&cfg);
    if styles.is_empty() {
        eprintln!("vEnter: no modules to render (check \"modules-right\" in {})", path.display());
    }
    let count = styles.len();

    let rx = plugin::spawn_worker(specs);
    let state = Box::new(window::State::new(styles, rx, path));

    let taskbar = taskbar::find_taskbar()?;
    let child = window::create_window(state)?;
    window::embed_in_taskbar(child, taskbar)?;
    println!("vEnter embedded — {count} module(s). Edit venter.json to reload live.");
    window::run_message_loop();
    Ok(())
}
