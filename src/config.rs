use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct RawConfig {
    #[serde(rename = "modules", default)]
    pub module_order: Vec<String>,
    #[serde(default)]
    pub css: HashMap<String, String>,
    /// Shared named style fragments, available to every module.
    #[serde(default)]
    pub classes: crate::css::ClassMap,
    /// Pixels reserved at the right edge of each secondary taskbar for the
    /// Windows 11 clock (which is painted in the XAML host and has no obstacle
    /// window to detect). Ignored on the primary taskbar.
    #[serde(rename = "secondary-clock-reserve", default = "default_clock_reserve")]
    pub secondary_clock_reserve: i32,
    /// Per-monitor module routing, keyed by monitor index (as a string).
    #[serde(default)]
    pub monitors: HashMap<String, MonitorConfig>,
    /// Every remaining top-level key is a module definition, keyed by name.
    #[serde(flatten)]
    pub modules: HashMap<String, ModuleConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitorConfig {
    #[serde(rename = "modules", default)]
    pub module_order: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ModuleConfig {
    pub exec: String,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default)]
    pub css: HashMap<String, String>,
    /// Module-only named style fragments, overlaid on the shared `classes`.
    #[serde(default)]
    pub classes: crate::css::ClassMap,
    /// "text" (default) or "html". Anything else warns and falls back to text.
    #[serde(default = "default_output")]
    pub output: String,
}

fn default_interval() -> u64 {
    5
}

fn default_output() -> String {
    "text".to_string()
}

/// The default config, baked into the binary so a lone `.exe` runs with no
/// `config.json` beside it. The tray's "Edit config" writes this out on demand.
pub const DEFAULT_CONFIG: &str = include_str!("../config.json");

/// Default pixels reserved for the secondary taskbar clock. Measured at ~81px
/// for the two-line time/date at 100% scaling; 100 leaves headroom.
fn default_clock_reserve() -> i32 {
    100
}

/// Parse a JSONC (JSON5) config string. Comments and trailing commas allowed.
pub fn parse(text: &str) -> Result<RawConfig, String> {
    json5::from_str(text).map_err(|e| e.to_string())
}

/// Read and parse the config file at `path`. If the file is missing, the
/// built-in [`DEFAULT_CONFIG`] is used instead. A file that exists but fails to
/// parse returns an error (the caller decides whether to fall back).
pub fn load(path: &Path) -> Result<RawConfig, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).map_err(|e| format!("parsing {}: {e}", path.display())),
        Err(_) => parse(DEFAULT_CONFIG).map_err(|e| format!("parsing built-in config: {e}")),
    }
}

/// A resolved registry: per-slot styles/specs (each referenced module once),
/// plus each monitor's ordered slot list and the legacy (primary-only) list.
pub struct BuildResult {
    pub styles: Vec<crate::css::Style>,
    pub class_maps: Vec<crate::css::ClassMap>,
    pub specs: Vec<crate::plugin::PluginSpec>,
    pub monitors: HashMap<usize, Vec<usize>>,
    pub legacy: Vec<usize>,
    /// Right-edge reserve for secondary taskbars (clock clearance), in pixels.
    pub clock_reserve: i32,
}

/// Resolve a list of module names into slot indices, registering each newly-seen
/// module (its style + spec) into the shared registry. Undefined names warn and
/// are skipped; a repeated name reuses its existing slot.
fn resolve_list(
    names: &[String],
    cfg: &RawConfig,
    styles: &mut Vec<crate::css::Style>,
    class_maps: &mut Vec<crate::css::ClassMap>,
    specs: &mut Vec<crate::plugin::PluginSpec>,
    slot_of: &mut HashMap<String, usize>,
) -> Vec<usize> {
    let mut slots = Vec::new();
    for name in names {
        if let Some(&slot) = slot_of.get(name) {
            slots.push(slot);
            continue;
        }
        match cfg.modules.get(name) {
            Some(m) => {
                let slot = styles.len();
                styles.push(crate::css::resolve(&cfg.css, &m.css));
                class_maps.push(crate::css::merge_class_maps(&cfg.classes, &m.classes));
                let output = match m.output.as_str() {
                    "text" => crate::plugin::OutputMode::Text,
                    "html" => crate::plugin::OutputMode::Html,
                    other => {
                        eprintln!(
                            "Taskband: module '{name}': unknown output '{other}' (using text)"
                        );
                        crate::plugin::OutputMode::Text
                    }
                };
                specs.push(crate::plugin::PluginSpec {
                    name: name.clone(),
                    exec: m.exec.clone(),
                    interval: std::time::Duration::from_secs(m.interval.max(1)),
                    output,
                });
                slot_of.insert(name.clone(), slot);
                slots.push(slot);
            }
            None => eprintln!("Taskband: module '{name}' is not defined (skipped)"),
        }
    }
    slots
}

/// Build the shared registry plus per-monitor slot lists. When `monitors` is
/// present it wins (and the top-level `modules` list is ignored with a warning);
/// otherwise `legacy` holds the top-level `modules` slots for the primary.
pub fn build_registry(cfg: &RawConfig) -> BuildResult {
    let mut styles = Vec::new();
    let mut class_maps = Vec::new();
    let mut specs = Vec::new();
    let mut slot_of: HashMap<String, usize> = HashMap::new();

    // Resolve monitors in ascending index order so slot assignment is
    // deterministic (a HashMap's iteration order is randomized per process).
    let mut entries: Vec<(usize, &MonitorConfig)> = Vec::new();
    for (key, mc) in &cfg.monitors {
        match key.parse::<usize>() {
            Ok(index) => entries.push((index, mc)),
            Err(_) => eprintln!("Taskband: monitor key '{key}' is not a valid index (skipped)"),
        }
    }
    entries.sort_by_key(|(index, _)| *index);

    let mut monitors = HashMap::new();
    for (index, mc) in entries {
        let slots = resolve_list(
            &mc.module_order,
            cfg,
            &mut styles,
            &mut class_maps,
            &mut specs,
            &mut slot_of,
        );
        monitors.insert(index, slots);
    }

    let legacy = if cfg.monitors.is_empty() {
        resolve_list(
            &cfg.module_order,
            cfg,
            &mut styles,
            &mut class_maps,
            &mut specs,
            &mut slot_of,
        )
    } else {
        if !cfg.module_order.is_empty() {
            eprintln!("Taskband: 'monitors' is set; top-level 'modules' is ignored");
        }
        Vec::new()
    };

    BuildResult {
        styles,
        class_maps,
        specs,
        monitors,
        legacy,
        clock_reserve: cfg.secondary_clock_reserve,
    }
}

/// The slot list a monitor should display: its `monitors` entry when the map is
/// non-empty (empty for unlisted monitors), else the legacy list on the primary.
pub fn slots_for_monitor(
    monitors: &HashMap<usize, Vec<usize>>,
    legacy: &[usize],
    index: usize,
    primary: bool,
) -> Vec<usize> {
    if !monitors.is_empty() {
        monitors.get(&index).cloned().unwrap_or_default()
    } else if primary {
        legacy.to_vec()
    } else {
        Vec::new()
    }
}

/// Resolve the config path: an existing `config.json` next to the executable
/// wins, then one in the current working directory. If neither exists the
/// canonical location (next to the executable) is returned anyway, so the
/// tray's "Edit config" creates it there and the watcher reloads it live.
pub fn config_path() -> PathBuf {
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("config.json")));

    if let Some(candidate) = &beside_exe {
        if candidate.exists() {
            return candidate.clone();
        }
    }
    let cwd = PathBuf::from("config.json");
    if cwd.exists() {
        return cwd;
    }
    beside_exe.unwrap_or(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modules_order_and_css() {
        let cfg = parse(
            r##"{
                // order, left-to-right
                "modules": ["cpu", "clock"],
                "css": { "color": "#ffffff" },
                "cpu": { "exec": "echo hi", "interval": 2, "css": { "font-weight": "bold" } },
                "clock": { "exec": "echo now" }
            }"##,
        )
        .expect("valid config");

        assert_eq!(
            cfg.module_order,
            vec!["cpu".to_string(), "clock".to_string()]
        );
        assert_eq!(cfg.css.get("color").map(String::as_str), Some("#ffffff"));

        let cpu = cfg.modules.get("cpu").expect("cpu module");
        assert_eq!(cpu.exec, "echo hi");
        assert_eq!(cpu.interval, 2);
        assert_eq!(cpu.css.get("font-weight").map(String::as_str), Some("bold"));

        let clock = cfg.modules.get("clock").expect("clock module");
        assert_eq!(clock.interval, 5); // default when omitted
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(parse("{ not valid").is_err());
    }

    #[test]
    fn parses_classes_at_both_levels_and_output_flag() {
        let cfg = parse(
            r##"{
                "modules": ["mem"],
                "classes": { "critical": { "color": "#ff5555" } },
                "mem": {
                    "exec": "echo m",
                    "output": "html",
                    "classes": { "critical": { "font-weight": "bold" } }
                }
            }"##,
        )
        .expect("valid config");

        assert_eq!(
            cfg.classes
                .get("critical")
                .unwrap()
                .get("color")
                .map(String::as_str),
            Some("#ff5555")
        );
        let mem = cfg.modules.get("mem").unwrap();
        assert_eq!(mem.output, "html");
        assert!(mem.classes.contains_key("critical"));
    }

    #[test]
    fn output_defaults_to_text() {
        let cfg = parse(r##"{ "cpu": { "exec": "echo c" } }"##).expect("valid");
        assert_eq!(cfg.modules.get("cpu").unwrap().output, "text");
    }

    #[test]
    fn build_registry_merges_class_maps_and_resolves_output() {
        let cfg = parse(
            r##"{
                "modules": ["mem", "cpu"],
                "classes": { "critical": { "color": "#ff5555" } },
                "mem": {
                    "exec": "echo m",
                    "output": "html",
                    "classes": { "critical": { "font-weight": "bold" } }
                },
                "cpu": { "exec": "echo c" }
            }"##,
        )
        .expect("valid config");

        let b = build_registry(&cfg);
        assert_eq!(b.class_maps.len(), 2);
        // mem: merged fragment has both properties
        let crit = b.class_maps[0].get("critical").unwrap();
        assert_eq!(crit.get("color").map(String::as_str), Some("#ff5555"));
        assert_eq!(crit.get("font-weight").map(String::as_str), Some("bold"));
        // cpu: inherits the shared class untouched
        assert!(b.class_maps[1].contains_key("critical"));
        assert_eq!(b.specs[0].output, crate::plugin::OutputMode::Html);
        assert_eq!(b.specs[1].output, crate::plugin::OutputMode::Text);
    }

    #[test]
    fn unknown_output_warns_and_falls_back_to_text() {
        let cfg = parse(r##"{ "modules": ["x"], "x": { "exec": "echo x", "output": "yaml" } }"##)
            .expect("valid config");
        let b = build_registry(&cfg);
        assert_eq!(b.specs[0].output, crate::plugin::OutputMode::Text);
    }

    #[test]
    fn parses_monitors_map() {
        let cfg = parse(
            r##"{
                "monitors": {
                    "0": { "modules": ["cpu"] },
                    "1": { "modules": ["clock", "net"] }
                },
                "cpu":   { "exec": "echo c" },
                "clock": { "exec": "echo t" },
                "net":   { "exec": "echo n" }
            }"##,
        )
        .expect("valid config");

        assert_eq!(
            cfg.monitors.get("0").unwrap().module_order,
            vec!["cpu".to_string()]
        );
        assert_eq!(
            cfg.monitors.get("1").unwrap().module_order,
            vec!["clock".to_string(), "net".to_string()]
        );
        // module definitions still flatten correctly alongside the `monitors` field
        assert!(cfg.modules.contains_key("net"));
    }

    #[test]
    fn build_registry_dedups_modules_shared_across_monitors() {
        let cfg = parse(
            r##"{
                "monitors": {
                    "0": { "modules": ["cpu", "clock"] },
                    "1": { "modules": ["clock", "cpu"] }
                },
                "cpu":   { "exec": "echo c" },
                "clock": { "exec": "echo t" }
            }"##,
        )
        .expect("valid config");

        let b = build_registry(&cfg);
        // two unique modules -> two slots (each runs once)
        assert_eq!(b.specs.len(), 2);
        assert_eq!(b.styles.len(), 2);
        // first-seen order assigns slots: cpu=0, clock=1
        assert_eq!(b.monitors.get(&0).unwrap(), &vec![0, 1]);
        assert_eq!(b.monitors.get(&1).unwrap(), &vec![1, 0]);
        assert!(b.legacy.is_empty());
    }

    #[test]
    fn build_registry_legacy_fallback_when_no_monitors_key() {
        let cfg = parse(
            r##"{
                "modules": ["cpu", "clock", "cpu"],
                "cpu":   { "exec": "echo c" },
                "clock": { "exec": "echo t" }
            }"##,
        )
        .expect("valid config");

        let b = build_registry(&cfg);
        assert!(b.monitors.is_empty());
        // duplicate "cpu" dedups to one slot but appears twice in the ordered list
        assert_eq!(b.specs.len(), 2);
        assert_eq!(b.legacy, vec![0, 1, 0]);
    }

    #[test]
    fn build_registry_skips_undefined_modules() {
        let cfg = parse(
            r##"{
                "monitors": { "0": { "modules": ["cpu", "ghost"] } },
                "cpu": { "exec": "echo c" }
            }"##,
        )
        .expect("valid config");

        let b = build_registry(&cfg);
        assert_eq!(b.specs.len(), 1);
        assert_eq!(b.monitors.get(&0).unwrap(), &vec![0]); // "ghost" skipped
    }

    #[test]
    fn secondary_clock_reserve_defaults_and_overrides() {
        let def = parse(r##"{ "cpu": { "exec": "echo c" } }"##).expect("valid");
        assert_eq!(build_registry(&def).clock_reserve, 100);

        let over = parse(r##"{ "secondary-clock-reserve": 140, "cpu": { "exec": "echo c" } }"##)
            .expect("valid");
        assert_eq!(build_registry(&over).clock_reserve, 140);
    }

    #[test]
    fn slots_for_monitor_prefers_map_then_legacy() {
        let mut monitors = HashMap::new();
        monitors.insert(0usize, vec![0, 1]);
        let legacy = vec![2];

        // map present: listed monitor uses its entry; unlisted monitor -> empty
        assert_eq!(slots_for_monitor(&monitors, &legacy, 0, true), vec![0, 1]);
        assert_eq!(
            slots_for_monitor(&monitors, &legacy, 5, false),
            Vec::<usize>::new()
        );

        // map empty: primary uses legacy, non-primary -> empty
        let empty: HashMap<usize, Vec<usize>> = HashMap::new();
        assert_eq!(slots_for_monitor(&empty, &legacy, 3, true), vec![2]);
        assert_eq!(
            slots_for_monitor(&empty, &legacy, 3, false),
            Vec::<usize>::new()
        );
    }
}
