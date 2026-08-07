//! The built-in module catalog: ready-made modules the configurator can copy
//! into a user's config. Entries with a script payload embed it at compile
//! time (from `modules/`) and write it beside `config.json` on first use.

use std::path::{Path, PathBuf};

/// A script file an entry needs on disk, embedded at compile time.
pub struct Payload {
    pub file: &'static str,
    pub contents: &'static str,
}

pub struct CatalogEntry {
    pub name: &'static str,
    pub description: &'static str,
    pub payload: Option<Payload>,
}

pub const ENTRIES: &[CatalogEntry] = &[
    CatalogEntry {
        name: "cpu",
        description: "Processor load percentage",
        payload: None,
    },
    CatalogEntry {
        name: "clock",
        description: "Date over a ticking time",
        payload: None,
    },
    CatalogEntry {
        name: "memory",
        description: "Physical memory in use, bar colored by level",
        payload: Some(Payload {
            file: "memory.ps1",
            contents: include_str!("../modules/memory/memory.ps1"),
        }),
    },
    CatalogEntry {
        name: "disk-space",
        description: "A usage bar per fixed drive",
        payload: Some(Payload {
            file: "disk-space.ps1",
            contents: include_str!("../modules/disk-space/disk-space.ps1"),
        }),
    },
];

pub fn find(name: &str) -> Option<&'static CatalogEntry> {
    ENTRIES.iter().find(|e| e.name == name)
}

/// Write the entry's payload to `<config_dir>\modules\<name>\<file>` unless it
/// already exists (a user's edits are never clobbered). Returns the script
/// path, or `None` for payload-free entries.
pub fn materialize(entry: &CatalogEntry, config_dir: &Path) -> Result<Option<PathBuf>, String> {
    let Some(payload) = &entry.payload else {
        return Ok(None);
    };
    let dir = config_dir.join("modules").join(entry.name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let path = dir.join(payload.file);
    if !path.exists() {
        std::fs::write(&path, payload.contents)
            .map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    Ok(Some(path))
}

/// The four-color classes block shared by the bar-drawing modules, as JSON5
/// text at two levels of indentation.
fn bar_classes(indent: &str) -> String {
    let i2 = format!("{indent}{indent}");
    let i3 = format!("{indent}{indent}{indent}");
    format!(
        "{{\n{i3}\"green\":  {{ \"color\": \"#7fdbb0\" }},\n{i3}\"yellow\": {{ \"color\": \"#f5c542\" }},\n{i3}\"orange\": {{ \"color\": \"#ff9f43\" }},\n{i3}\"red\":    {{ \"color\": \"#ff5555\" }}\n{i2}}}"
    )
}

/// A styled bar module (memory, disk-space) definition body.
fn bar_module_body(indent: &str, script: &Path, interval: u32) -> String {
    let i2 = format!("{indent}{indent}");
    let exec = crate::editor::json_escape(&format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" -Styled",
        script.display()
    ));
    let classes = bar_classes(indent);
    format!(
        "{{\n{i2}\"exec\": \"{exec}\",\n{i2}\"interval\": {interval},\n{i2}\"output\": \"html\",\n{i2}\"css\": {{ \"font-family\": \"Consolas\", \"text-align\": \"left\" }},\n{i2}\"classes\": {classes}\n{indent}}}"
    )
}

/// The JSON5 object text for this entry's module definition, ready for
/// `editor::append_module`. `script` is required for entries with a payload
/// (the path `materialize` returned).
pub fn definition_body(entry: &CatalogEntry, indent: &str, script: Option<&Path>) -> String {
    let i2 = format!("{indent}{indent}");
    match entry.name {
        "cpu" => {
            let exec = crate::editor::json_escape(
                r#"powershell -NoProfile -Command "'CPU ' + (Get-CimInstance Win32_Processor).LoadPercentage + '%'""#,
            );
            format!(
                "{{\n{i2}\"exec\": \"{exec}\",\n{i2}\"interval\": 2,\n{i2}\"css\": {{ \"color\": \"#7fdbb0\", \"font-weight\": \"bold\", \"font-size\": \"14px\" }}\n{indent}}}"
            )
        }
        "clock" => {
            let exec = crate::editor::json_escape(
                r#"powershell -NoProfile -Command "(Get-Date).ToString('ddd dd MMM'); (Get-Date).ToString('HH:mm:ss')""#,
            );
            format!(
                "{{\n{i2}\"exec\": \"{exec}\",\n{i2}\"interval\": 1,\n{i2}\"css\": {{ \"color\": \"#ffffff\", \"font-size\": \"14px\", \"text-align\": \"left\" }}\n{indent}}}"
            )
        }
        "memory" => bar_module_body(indent, script.expect("memory has a payload"), 5),
        "disk-space" => bar_module_body(indent, script.expect("disk-space has a payload"), 30),
        other => unreachable!("no definition body for catalog entry '{other}'"),
    }
}

/// Keys emitted first, in this order; any other key follows alphabetically.
/// `serde_json::Map` is a `BTreeMap` here (no `preserve_order` feature), so
/// without this the output would read `classes, css, exec, ...`.
const KEY_ORDER: &[&str] = &["exec", "interval", "output", "css", "classes"];

/// Longest line the emitter will produce before breaking an object across
/// lines. Chosen so `css` stays inline and a four-color `classes` block does not.
const MAX_WIDTH: usize = 80;

fn ordered_keys(map: &serde_json::Map<String, serde_json::Value>) -> Vec<&String> {
    let mut out: Vec<&String> = Vec::new();
    for want in KEY_ORDER {
        if let Some((key, _)) = map.get_key_value(*want) {
            out.push(key);
        }
    }
    let mut rest: Vec<&String> = map
        .keys()
        .filter(|k| !KEY_ORDER.contains(&k.as_str()))
        .collect();
    rest.sort();
    out.extend(rest);
    out
}

/// One-line form of a value. Scalars and arrays use serde's compact output
/// (which escapes strings correctly); objects get `{ "k": v, "k": v }`.
fn inline(value: &serde_json::Value) -> String {
    let serde_json::Value::Object(map) = value else {
        return value.to_string();
    };
    if map.is_empty() {
        return "{}".to_string();
    }
    let parts: Vec<String> = ordered_keys(map)
        .into_iter()
        .map(|k| format!("\"{}\": {}", crate::editor::json_escape(k), inline(&map[k])))
        .collect();
    format!("{{ {} }}", parts.join(", "))
}

/// Render `value` as JSON5 text. `depth` is how many indent levels the value's
/// own opening brace sits at. An object is emitted inline when that form fits
/// in [`MAX_WIDTH`] columns, otherwise one key per line.
fn render(value: &serde_json::Value, indent: &str, depth: usize) -> String {
    let serde_json::Value::Object(map) = value else {
        return value.to_string();
    };
    let one_line = inline(value);
    if map.is_empty() || indent.len() * depth + one_line.len() <= MAX_WIDTH {
        return one_line;
    }
    let pad = indent.repeat(depth + 1);
    let close = indent.repeat(depth);
    let parts: Vec<String> = ordered_keys(map)
        .into_iter()
        .map(|k| {
            format!(
                "{pad}\"{}\": {}",
                crate::editor::json_escape(k),
                render(&map[k], indent, depth + 1)
            )
        })
        .collect();
    format!("{{\n{}\n{close}}}", parts.join(",\n"))
}

/// A module definition object as JSON5 text, ready for
/// `editor::append_module`: keys at two indent levels, closing brace at one.
pub fn render_body(map: &serde_json::Map<String, serde_json::Value>, indent: &str) -> String {
    render(&serde_json::Value::Object(map.clone()), indent, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(text: &str) -> serde_json::Map<String, serde_json::Value> {
        match json5::from_str::<serde_json::Value>(text).unwrap() {
            serde_json::Value::Object(m) => m,
            other => panic!("not an object: {other}"),
        }
    }

    #[test]
    fn render_body_orders_keys_and_inlines_short_objects() {
        // authored alphabetically-last-first to prove the order is imposed
        let m = map(r##"{
                "css": { "font-family": "Consolas", "text-align": "left" },
                "interval": 5,
                "exec": "run.exe",
                "output": "html"
            }"##);
        let out = render_body(&m, "    ");

        let exec = out.find("\"exec\"").expect("exec present");
        let interval = out.find("\"interval\"").expect("interval present");
        let output = out.find("\"output\"").expect("output present");
        let css = out.find("\"css\"").expect("css present");
        assert!(
            exec < interval && interval < output && output < css,
            "got:\n{out}"
        );

        // a short nested object stays on one line
        assert!(
            out.contains(r#""css": { "font-family": "Consolas", "text-align": "left" }"#),
            "css should be inline, got:\n{out}"
        );
        // top-level keys sit at two indents, the closing brace at one
        assert!(out.starts_with("{\n        \"exec\""), "got:\n{out}");
        assert!(out.ends_with("\n    }"), "got:\n{out}");
    }

    #[test]
    fn render_body_expands_objects_that_do_not_fit() {
        let m = map(r##"{
                "exec": "run.exe",
                "classes": {
                    "green":  { "color": "#7fdbb0" },
                    "yellow": { "color": "#f5c542" },
                    "orange": { "color": "#ff9f43" },
                    "red":    { "color": "#ff5555" }
                }
            }"##);
        let out = render_body(&m, "    ");
        // too wide for one line, so one key per line, but each leaf stays inline
        assert!(out.contains("\"classes\": {\n"), "got:\n{out}");
        assert!(
            out.contains(r##""green": { "color": "#7fdbb0" }"##),
            "got:\n{out}"
        );
    }

    #[test]
    fn render_body_output_parses_as_a_module() {
        let m = map(r##"{
                "exec": "powershell -File \"C:\\a b\\x.ps1\"",
                "interval": 30,
                "output": "html",
                "classes": { "red": { "color": "#ff5555" } }
            }"##);
        let body = render_body(&m, "    ");
        let text = crate::editor::append_module("{}", "thing", &body).unwrap();
        let cfg = crate::config::parse(&text).expect("emitted body parses");
        let thing = cfg.modules.get("thing").expect("thing defined");
        assert_eq!(thing.exec, r#"powershell -File "C:\a b\x.ps1""#);
        assert_eq!(thing.interval, 30);
        assert_eq!(thing.output, "html");
        assert!(thing.classes.contains_key("red"));
    }

    #[test]
    fn render_body_handles_an_empty_map() {
        assert_eq!(render_body(&serde_json::Map::new(), "    "), "{}");
    }

    /// Append every catalog entry to an empty config and check it parses into
    /// the expected definition. Pins the /api/apply contract for `add`.
    #[test]
    fn every_entry_produces_a_valid_definition() {
        let indent = "    ";
        let fake_script = std::path::Path::new(r"C:\tools\taskband\modules\x\x.ps1");
        let mut text = "{}".to_string();
        for entry in ENTRIES {
            let script = entry.payload.as_ref().map(|_| fake_script);
            let body = definition_body(entry, indent, script);
            text = crate::editor::append_module(&text, entry.name, &body).unwrap();
        }
        let cfg = crate::config::parse(&text).expect("all catalog definitions parse");
        assert_eq!(cfg.modules.len(), ENTRIES.len());

        let memory = cfg.modules.get("memory").expect("memory defined");
        assert!(memory.exec.contains(r"C:\tools\taskband\modules\x\x.ps1"));
        assert!(memory.exec.contains("-Styled"));
        assert_eq!(memory.output, "html");
        assert!(memory.classes.contains_key("red"));

        let disk = cfg.modules.get("disk-space").expect("disk-space defined");
        assert_eq!(disk.interval, 30);

        let cpu = cfg.modules.get("cpu").expect("cpu defined");
        assert!(cpu.exec.contains("LoadPercentage"));
        assert_eq!(cpu.interval, 2);
    }

    #[test]
    fn find_is_exact() {
        assert!(find("memory").is_some());
        assert!(find("Memory").is_none());
        assert!(find("ghost").is_none());
    }

    #[test]
    fn materialize_writes_once_and_never_overwrites() {
        let dir =
            std::env::temp_dir().join(format!("taskband-catalog-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let entry = find("memory").unwrap();
        let path = materialize(entry, &dir).unwrap().expect("script path");
        assert!(path.ends_with(r"modules\memory\memory.ps1"));
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("Taskband module"));

        // a user-edited script is left alone
        std::fs::write(&path, "user edited").unwrap();
        materialize(entry, &dir).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "user edited");

        // entries without payloads materialize to nothing
        assert!(materialize(find("cpu").unwrap(), &dir).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
