use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct RawConfig {
    #[serde(rename = "modules-right", default)]
    pub modules_right: Vec<String>,
    #[serde(default)]
    pub css: HashMap<String, String>,
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
    #[serde(rename = "modules-right", default)]
    pub modules_right: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ModuleConfig {
    pub exec: String,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default)]
    pub css: HashMap<String, String>,
}

fn default_interval() -> u64 {
    5
}

/// Default pixels reserved for the secondary taskbar clock. Measured at ~81px
/// for the two-line time/date at 100% scaling; 100 leaves headroom.
fn default_clock_reserve() -> i32 {
    100
}

/// Parse a JSONC (JSON5) config string. Comments and trailing commas allowed.
pub fn parse(text: &str) -> Result<RawConfig, String> {
    json5::from_str(text).map_err(|e| e.to_string())
}

/// Read and parse the config file at `path`.
pub fn load(path: &Path) -> Result<RawConfig, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("reading {}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("parsing {}: {e}", path.display()))
}

/// A resolved registry: per-slot styles/specs (each referenced module once),
/// plus each monitor's ordered slot list and the legacy (primary-only) list.
pub struct BuildResult {
    pub styles: Vec<crate::css::Style>,
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
                specs.push(crate::plugin::PluginSpec {
                    name: name.clone(),
                    exec: m.exec.clone(),
                    interval: std::time::Duration::from_secs(m.interval.max(1)),
                });
                slot_of.insert(name.clone(), slot);
                slots.push(slot);
            }
            None => eprintln!("vEnter: module '{name}' is not defined (skipped)"),
        }
    }
    slots
}

/// Build the shared registry plus per-monitor slot lists. When `monitors` is
/// present it wins (and top-level `modules-right` is ignored with a warning);
/// otherwise `legacy` holds the top-level `modules-right` slots for the primary.
pub fn build_registry(cfg: &RawConfig) -> BuildResult {
    let mut styles = Vec::new();
    let mut specs = Vec::new();
    let mut slot_of: HashMap<String, usize> = HashMap::new();

    // Resolve monitors in ascending index order so slot assignment is
    // deterministic (a HashMap's iteration order is randomized per process).
    let mut entries: Vec<(usize, &MonitorConfig)> = Vec::new();
    for (key, mc) in &cfg.monitors {
        match key.parse::<usize>() {
            Ok(index) => entries.push((index, mc)),
            Err(_) => eprintln!("vEnter: monitor key '{key}' is not a valid index (skipped)"),
        }
    }
    entries.sort_by_key(|(index, _)| *index);

    let mut monitors = HashMap::new();
    for (index, mc) in entries {
        let slots = resolve_list(&mc.modules_right, cfg, &mut styles, &mut specs, &mut slot_of);
        monitors.insert(index, slots);
    }

    let legacy = if cfg.monitors.is_empty() {
        resolve_list(&cfg.modules_right, cfg, &mut styles, &mut specs, &mut slot_of)
    } else {
        if !cfg.modules_right.is_empty() {
            eprintln!("vEnter: 'monitors' is set; top-level 'modules-right' is ignored");
        }
        Vec::new()
    };

    BuildResult { styles, specs, monitors, legacy, clock_reserve: cfg.secondary_clock_reserve }
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

/// Resolve the config path: `venter.json` next to the executable, else `venter.json`
/// in the current working directory.
pub fn config_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("venter.json");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("venter.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modules_order_and_css() {
        let cfg = parse(
            r##"{
                // order, left-to-right
                "modules-right": ["cpu", "clock"],
                "css": { "color": "#ffffff" },
                "cpu": { "exec": "echo hi", "interval": 2, "css": { "font-weight": "bold" } },
                "clock": { "exec": "echo now" }
            }"##,
        )
        .expect("valid config");

        assert_eq!(cfg.modules_right, vec!["cpu".to_string(), "clock".to_string()]);
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
    fn parses_monitors_map() {
        let cfg = parse(
            r##"{
                "monitors": {
                    "0": { "modules-right": ["cpu"] },
                    "1": { "modules-right": ["clock", "net"] }
                },
                "cpu":   { "exec": "echo c" },
                "clock": { "exec": "echo t" },
                "net":   { "exec": "echo n" }
            }"##,
        )
        .expect("valid config");

        assert_eq!(cfg.monitors.get("0").unwrap().modules_right, vec!["cpu".to_string()]);
        assert_eq!(
            cfg.monitors.get("1").unwrap().modules_right,
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
                    "0": { "modules-right": ["cpu", "clock"] },
                    "1": { "modules-right": ["clock", "cpu"] }
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
                "modules-right": ["cpu", "clock", "cpu"],
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
                "monitors": { "0": { "modules-right": ["cpu", "ghost"] } },
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

        let over = parse(
            r##"{ "secondary-clock-reserve": 140, "cpu": { "exec": "echo c" } }"##,
        )
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
        assert_eq!(slots_for_monitor(&monitors, &legacy, 5, false), Vec::<usize>::new());

        // map empty: primary uses legacy, non-primary -> empty
        let empty: HashMap<usize, Vec<usize>> = HashMap::new();
        assert_eq!(slots_for_monitor(&empty, &legacy, 3, true), vec![2]);
        assert_eq!(slots_for_monitor(&empty, &legacy, 3, false), Vec::<usize>::new());
    }
}
