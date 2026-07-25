//! The built-in module catalog: ready-made modules the configurator can copy
//! into a user's config. Entries with a script payload embed it at compile
//! time (from `examples/`) and write it beside `config.json` on first use.

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
            contents: include_str!("../examples/memory/memory.ps1"),
        }),
    },
    CatalogEntry {
        name: "disk-space",
        description: "A usage bar per fixed drive",
        payload: Some(Payload {
            file: "disk-space.ps1",
            contents: include_str!("../examples/disk-space/disk-space.ps1"),
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let dir = std::env::temp_dir().join(format!(
            "taskband-catalog-test-{}",
            std::process::id()
        ));
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
