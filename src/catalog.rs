//! The module catalog: ready-made modules the configurator can copy into a
//! user's config. Every entry is a manifest (a module definition plus a
//! `description`). Built-ins embed theirs from `modules/` at compile time,
//! along with any script they need, and write the script out beside
//! `config.json` on first use.

use std::path::{Path, PathBuf};

/// A script file an entry needs on disk, embedded at compile time.
#[derive(Clone)]
pub struct Payload {
    pub file: &'static str,
    pub contents: &'static str,
}

/// A ready-made module the configurator can drop into a user's config.
pub struct CatalogEntry {
    pub name: String,
    pub description: String,
    /// The module definition, `description` removed and `${dir}` unexpanded.
    pub manifest: serde_json::Map<String, serde_json::Value>,
    /// Script to write out on first use. `None` for modules that need no file
    /// on disk (inline `exec`) and for folder modules, whose files are there
    /// already.
    pub payload: Option<Payload>,
}

/// A module shipped inside the binary: its manifest and script are embedded
/// from the repo's `modules/` folder at compile time.
struct Builtin {
    name: &'static str,
    manifest: &'static str,
    payload: Option<Payload>,
}

const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "cpu",
        manifest: include_str!("../modules/cpu/module.json"),
        payload: None,
    },
    Builtin {
        name: "clock",
        manifest: include_str!("../modules/clock/module.json"),
        payload: None,
    },
    Builtin {
        name: "memory",
        manifest: include_str!("../modules/memory/module.json"),
        payload: Some(Payload {
            file: "memory.ps1",
            contents: include_str!("../modules/memory/memory.ps1"),
        }),
    },
    Builtin {
        name: "disk-space",
        manifest: include_str!("../modules/disk-space/module.json"),
        payload: Some(Payload {
            file: "disk-space.ps1",
            contents: include_str!("../modules/disk-space/disk-space.ps1"),
        }),
    },
];

/// Parse manifest text into its palette description and the module definition
/// that remains once `description` is removed. `exec` is required, because a
/// definition without it cannot deserialize into a `ModuleConfig`.
pub fn parse_manifest(
    text: &str,
) -> Result<(String, serde_json::Map<String, serde_json::Value>), String> {
    let value: serde_json::Value = json5::from_str(text).map_err(|e| e.to_string())?;
    let serde_json::Value::Object(mut map) = value else {
        return Err("manifest is not a JSON object".to_string());
    };
    let description = match map.remove("description") {
        Some(serde_json::Value::String(s)) => s,
        Some(_) => return Err("\"description\" must be a string".to_string()),
        None => String::new(),
    };
    match map.get("exec") {
        Some(serde_json::Value::String(_)) => {}
        Some(_) => return Err("\"exec\" must be a string".to_string()),
        None => return Err("no \"exec\" key".to_string()),
    }
    Ok((description, map))
}

/// Folder modules: every directory under `<config_dir>\modules\` that carries a
/// `module.json`. A directory without one is skipped silently, which is what a
/// materialized built-in's script folder looks like. A manifest that will not
/// parse warns and is skipped, so one bad module cannot empty the palette.
fn scan(config_dir: &Path) -> Vec<CatalogEntry> {
    let root = config_dir.join("modules");
    let Ok(read) = std::fs::read_dir(&root) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for dir_entry in read.flatten() {
        if !dir_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let manifest_path = dir_entry.path().join("module.json");
        let Ok(text) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let name = dir_entry.file_name().to_string_lossy().into_owned();
        match parse_manifest(&text) {
            Ok((description, manifest)) => out.push(CatalogEntry {
                name,
                description,
                manifest,
                payload: None,
            }),
            Err(e) => eprintln!("Taskband: {}: {e} (skipped)", manifest_path.display()),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The palette: built-ins first in their shipped order, then folder modules
/// sorted by name. A folder module whose name matches a built-in replaces it in
/// place, so overriding a shipped module means dropping in a folder.
pub fn entries(config_dir: &Path) -> Vec<CatalogEntry> {
    let mut out: Vec<CatalogEntry> = BUILTINS
        .iter()
        .filter_map(|b| match parse_manifest(b.manifest) {
            Ok((description, manifest)) => Some(CatalogEntry {
                name: b.name.to_string(),
                description,
                manifest,
                payload: b.payload.clone(),
            }),
            Err(e) => {
                // Unreachable in a correct build; a unit test pins every
                // built-in manifest. Warn rather than panic so one bad
                // manifest cannot take the app down.
                eprintln!("Taskband: built-in module '{}': {e} (skipped)", b.name);
                None
            }
        })
        .collect();

    for found in scan(config_dir) {
        match out.iter().position(|e| e.name == found.name) {
            Some(i) => out[i] = found,
            None => out.push(found),
        }
    }
    out
}

pub fn find(name: &str, config_dir: &Path) -> Option<CatalogEntry> {
    entries(config_dir).into_iter().find(|e| e.name == name)
}

/// The module's folder, `<config_dir>\modules\<name>`. An entry with a payload
/// gets its folder created and the script written, unless a file is already
/// there (a user's edits are never clobbered). An entry without one is not
/// created: nothing needs to live there, and its `exec` does not use `${dir}`.
pub fn materialize(entry: &CatalogEntry, config_dir: &Path) -> Result<PathBuf, String> {
    let dir = config_dir.join("modules").join(&entry.name);
    let Some(payload) = &entry.payload else {
        return Ok(dir);
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let path = dir.join(payload.file);
    if !path.exists() {
        std::fs::write(&path, payload.contents)
            .map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    Ok(dir)
}

/// The JSON5 object text for this entry's module definition, ready for
/// `editor::append_module`. `dir` is the module folder `materialize` returned;
/// `${dir}` in `exec` expands to it, so the config ends up holding a plain
/// absolute path with no runtime indirection.
pub fn definition_body(entry: &CatalogEntry, indent: &str, dir: &Path) -> String {
    let mut map = entry.manifest.clone();
    if let Some(serde_json::Value::String(exec)) = map.get("exec") {
        let expanded = exec.replace("${dir}", &dir.display().to_string());
        map.insert("exec".to_string(), serde_json::Value::String(expanded));
    }
    render_body(&map, indent)
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

    /// A fresh empty temp directory, removed by the caller.
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "taskband-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

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
    fn every_builtin_manifest_parses_and_produces_a_valid_definition() {
        let root = temp_dir("builtins");
        let indent = "    ";
        let mut text = "{}".to_string();

        let all = entries(&root);
        assert_eq!(all.len(), 4, "cpu, clock, memory, disk-space");
        for entry in &all {
            let dir = materialize(entry, &root).unwrap();
            let body = definition_body(entry, indent, &dir);
            text = crate::editor::append_module(&text, &entry.name, &body).unwrap();
        }

        let cfg = crate::config::parse(&text).expect("all built-in definitions parse");
        assert_eq!(cfg.modules.len(), 4);

        let memory = cfg.modules.get("memory").expect("memory defined");
        assert!(memory.exec.contains("-Styled"));
        assert!(!memory.exec.contains("${dir}"), "${{dir}} must be expanded");
        assert!(memory
            .exec
            .contains(&root.join("modules").join("memory").display().to_string()));
        assert_eq!(memory.output, "html");
        assert!(memory.classes.contains_key("red"));

        assert_eq!(cfg.modules.get("disk-space").unwrap().interval, 30);

        let cpu = cfg.modules.get("cpu").expect("cpu defined");
        assert!(cpu.exec.contains("LoadPercentage"));
        assert_eq!(cpu.interval, 2);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn find_is_exact() {
        let root = temp_dir("find");
        assert!(find("memory", &root).is_some());
        assert!(find("Memory", &root).is_none());
        assert!(find("ghost", &root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn materialize_writes_once_and_never_overwrites() {
        let root = temp_dir("materialize");

        let entry = find("memory", &root).unwrap();
        let dir = materialize(&entry, &root).unwrap();
        assert!(dir.ends_with(r"modules\memory"));
        let script = dir.join("memory.ps1");
        assert!(std::fs::read_to_string(&script)
            .unwrap()
            .contains("Taskband module"));

        // a user-edited script is left alone
        std::fs::write(&script, "user edited").unwrap();
        materialize(&entry, &root).unwrap();
        assert_eq!(std::fs::read_to_string(&script).unwrap(), "user edited");

        // a payload-free entry still reports its folder, and writes nothing
        let cpu_dir = materialize(&find("cpu", &root).unwrap(), &root).unwrap();
        assert!(cpu_dir.ends_with(r"modules\cpu"));
        assert!(!cpu_dir.exists(), "no payload means nothing to create");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Write `<root>\modules\<name>\module.json` with the given text.
    fn write_manifest(root: &std::path::Path, name: &str, text: &str) {
        let dir = root.join("modules").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("module.json"), text).unwrap();
    }

    #[test]
    fn scan_picks_up_valid_manifests_and_skips_everything_else() {
        let root = temp_dir("scan");
        let modules = root.join("modules");

        write_manifest(&root, "zebra", r#"{ "description": "Z", "exec": "z.exe" }"#);
        write_manifest(&root, "alpha", r#"{ "description": "A", "exec": "a.exe" }"#);
        write_manifest(&root, "broken", "{ not json5");
        write_manifest(&root, "no-exec", r#"{ "description": "nope" }"#);
        // a folder with no manifest (this is what a materialized built-in leaves)
        std::fs::create_dir_all(modules.join("script-only")).unwrap();
        std::fs::write(modules.join("script-only").join("x.ps1"), "# hi").unwrap();
        // a loose file directly under modules\
        std::fs::write(modules.join("stray.txt"), "ignore me").unwrap();

        let names: Vec<String> = entries(&root).into_iter().map(|e| e.name).collect();
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"zebra".to_string()));
        assert!(!names.contains(&"broken".to_string()));
        assert!(!names.contains(&"no-exec".to_string()));
        assert!(!names.contains(&"script-only".to_string()));
        assert!(!names.contains(&"stray".to_string()));

        // built-ins come first, folder modules after, sorted by name
        assert_eq!(&names[..4], &["cpu", "clock", "memory", "disk-space"]);
        assert_eq!(&names[4..], &["alpha", "zebra"]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_module_overrides_a_builtin_in_place() {
        let root = temp_dir("override");
        write_manifest(
            &root,
            "memory",
            r#"{ "description": "mine", "exec": "custom.exe", "interval": 11 }"#,
        );

        let all = entries(&root);
        let names: Vec<&str> = all.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["cpu", "clock", "memory", "disk-space"]);
        assert_eq!(all.len(), 4, "override replaces, never appends");

        let memory = find("memory", &root).unwrap();
        assert_eq!(memory.description, "mine");
        assert_eq!(memory.manifest["interval"], serde_json::json!(11));
        assert!(
            memory.payload.is_none(),
            "the folder's files are already there"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_module_round_trips_into_the_config() {
        let root = temp_dir("roundtrip");
        write_manifest(
            &root,
            "mouse-battery",
            r##"{
                "description": "Mouse battery",
                "exec": "python \"${dir}\\mouse-battery.py\" --styled",
                "interval": 60,
                "output": "html",
                "classes": { "green": { "color": "#7fdbb0" } }
            }"##,
        );

        let entry = find("mouse-battery", &root).expect("scanned");
        let dir = materialize(&entry, &root).unwrap();
        let body = definition_body(&entry, "    ", &dir);
        let text = crate::editor::append_module("{}", &entry.name, &body).unwrap();
        let cfg = crate::config::parse(&text).expect("folder module definition parses");

        let m = cfg.modules.get("mouse-battery").expect("defined");
        let expected = root.join("modules").join("mouse-battery");
        assert_eq!(
            m.exec,
            format!(
                r#"python "{}\mouse-battery.py" --styled"#,
                expected.display()
            )
        );
        assert_eq!(m.interval, 60);
        assert_eq!(m.output, "html");
        assert!(m.classes.contains_key("green"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_modules_folder_is_not_an_error() {
        let root = temp_dir("empty");
        assert_eq!(entries(&root).len(), 4, "just the built-ins");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// claude-usage ships a manifest without being built in. It is not
    /// embedded in BUILTINS, so nothing else would catch a typo in it.
    #[test]
    fn shipped_non_builtin_manifest_parses() {
        let text = include_str!("../modules/claude-usage/module.json");
        let (description, manifest) = parse_manifest(text).expect("manifest parses");
        assert!(!description.is_empty(), "needs a palette description");
        assert!(manifest.contains_key("exec"));
    }

    #[test]
    fn parse_manifest_rejects_bad_shapes() {
        assert!(parse_manifest("{ not json5").is_err());
        assert!(parse_manifest("[1, 2]").is_err());
        assert!(parse_manifest(r#"{ "interval": 5 }"#).is_err(), "no exec");
        assert!(
            parse_manifest(r#"{ "exec": 5 }"#).is_err(),
            "exec not a string"
        );

        let (desc, m) =
            parse_manifest(r#"{ "description": "d", "exec": "x", "interval": 9 }"#).unwrap();
        assert_eq!(desc, "d");
        assert!(!m.contains_key("description"), "description is stripped");
        assert_eq!(m["interval"], serde_json::json!(9));

        let (desc, _) = parse_manifest(r#"{ "exec": "x" }"#).unwrap();
        assert_eq!(desc, "", "description is optional");
    }
}
