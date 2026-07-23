mod config;
mod css;
mod layout;
mod plugin;
mod taskbar;
mod window;

use std::time::Duration;

fn main() -> windows::core::Result<()> {
    let path = config::config_path();
    let cfg = match config::load(&path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("vEnter: {e}");
            std::process::exit(1);
        }
    };

    // Resolve display order: each name in modules-right that has a definition.
    let ordered: Vec<(String, config::ModuleConfig)> = cfg
        .modules_right
        .iter()
        .filter_map(|name| cfg.modules.get(name).map(|m| (name.clone(), m.clone())))
        .collect();

    if ordered.is_empty() {
        eprintln!("vEnter: no modules to render (check \"modules-right\" in {})", path.display());
    }

    // Increment 1: every module uses the default style. Increment 2 swaps this
    // for css::resolve(&cfg.css, &m.css).
    let styles: Vec<css::Style> = ordered.iter().map(|_| css::Style::default()).collect();

    let specs: Vec<plugin::PluginSpec> = ordered
        .iter()
        .map(|(name, m)| plugin::PluginSpec {
            name: name.clone(),
            exec: m.exec.clone(),
            interval: Duration::from_secs(m.interval.max(1)),
        })
        .collect();

    let rx = plugin::spawn_worker(specs);
    let state = Box::new(window::State::new(styles, rx));

    let taskbar = taskbar::find_taskbar()?;
    let child = window::create_window(state)?;
    window::embed_in_taskbar(child, taskbar)?;
    println!("vEnter embedded — {} module(s).", ordered.len());
    window::run_message_loop();
    Ok(())
}
