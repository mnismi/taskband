// Release builds run without a console window (background taskbar widget);
// debug builds keep the console so startup diagnostics are visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod catalog;
mod config;
mod css;
mod editor;
mod layout;
mod markup;
mod plugin;
mod server;
mod taskbar;
mod tray;
mod window;

fn main() -> windows::core::Result<()> {
    let path = config::config_path();
    // First run with no config anywhere: create the per-user one so there is a
    // real file to edit, watch, and drop modules next to.
    config::ensure_user_config(&path);
    // A missing config falls back to the built-in default inside `load`; a config
    // that exists but fails to parse also falls back, so the app always starts.
    let cfg = config::load(&path).unwrap_or_else(|e| {
        eprintln!("Taskband: {e}; using built-in default config");
        config::parse(config::DEFAULT_CONFIG).expect("built-in config must parse")
    });

    let monitors = taskbar::detect();
    println!("Taskband monitors:");
    for m in &monitors {
        println!("{}", taskbar::monitor_log_line(m));
    }

    let build = config::build_registry(&cfg);
    let slot_count = build.styles.len();

    // Warn about monitors named in the config that can't host a bar.
    for &idx in build.monitors.keys() {
        match monitors.iter().find(|m| m.index == idx) {
            Some(m) if m.taskbar.is_none() => eprintln!(
                "Taskband: monitor {idx} has no taskbar; enable 'Show my taskbar on all displays'."
            ),
            None => eprintln!("Taskband: monitor {idx} does not exist (skipped)."),
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
        bars.push(window::Bar::new(
            hwnd,
            taskbar,
            m.index,
            m.primary,
            slots,
            build.clock_reserve,
        ));
    }

    if bars.is_empty() {
        eprintln!("Taskband: no taskbars found; nothing to display.");
        std::process::exit(1);
    }
    let driver = driver.unwrap_or_else(|| bars[0].hwnd());
    let bar_count = bars.len();

    let app = window::App::new(build.styles, build.class_maps, rx, path.clone(), bars);
    window::install(app, driver);
    tray::install(instance, driver, path)?;
    tray::ensure_default_startup();
    println!(
        "Taskband embedded on {bar_count} monitor(s), {slot_count} module slot(s). Edit config.json to reload live."
    );
    window::run_message_loop();
    Ok(())
}
