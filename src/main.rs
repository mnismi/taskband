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

    let monitors = taskbar::detect();
    println!("vEnter monitors:");
    for m in &monitors {
        println!("{}", taskbar::monitor_log_line(m));
    }

    let build = config::build_registry(&cfg);
    let slot_count = build.styles.len();

    // Warn about monitors named in the config that can't host a bar.
    for &idx in build.monitors.keys() {
        match monitors.iter().find(|m| m.index == idx) {
            Some(m) if m.taskbar.is_none() => eprintln!(
                "vEnter: monitor {idx} has no taskbar — enable 'Show my taskbar on all displays'."
            ),
            None => eprintln!("vEnter: monitor {idx} does not exist (skipped)."),
            _ => {}
        }
    }

    let rx = plugin::spawn_worker(build.specs);
    let instance = window::register_class()?;

    let mut bars = Vec::new();
    let mut driver = None;
    for m in &monitors {
        let Some(taskbar) = m.taskbar else {
            continue;
        };
        let slots = config::slots_for_monitor(&build.monitors, &build.legacy, m.index, m.primary);
        let hwnd = window::create_bar_window(instance)?;
        window::embed_in_taskbar(hwnd, taskbar)?;
        if m.primary {
            driver = Some(hwnd);
        }
        bars.push(window::Bar::new(hwnd, taskbar, m.index, m.primary, slots, build.clock_reserve));
    }

    if bars.is_empty() {
        eprintln!("vEnter: no taskbars found; nothing to display.");
        std::process::exit(1);
    }
    let driver = driver.unwrap_or_else(|| bars[0].hwnd());
    let bar_count = bars.len();

    let app = window::App::new(build.styles, rx, path, bars);
    window::install(app, driver);
    println!(
        "vEnter embedded on {bar_count} monitor(s), {slot_count} module slot(s). Edit venter.json to reload live."
    );
    window::run_message_loop();
    Ok(())
}
