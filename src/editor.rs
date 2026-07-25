//! Surgical, text-level edits to the JSON5 config.
//!
//! The configurator must rewrite the `monitors` section and append module
//! definitions while leaving every other byte of the file untouched, comments
//! included. `json5` can parse but not preserve, so this module scans the raw
//! text: it finds the byte spans of top-level keys (string- and comment-aware)
//! and splices replacements into the original string.

/// A top-level key and the byte span of its value.
#[derive(Debug, Clone)]
pub struct KeySpan {
    pub key: String,
    /// First byte of the key token (its quote, if quoted).
    pub start: usize,
    /// First byte of the value.
    pub value_start: usize,
    /// One past the last byte of the value.
    pub end: usize,
}

/// Advance past whitespace, `// line` and `/* block */` comments.
pub(crate) fn skip_ws_and_comments(b: &[u8], mut i: usize) -> usize {
    loop {
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = if i + 1 < b.len() { i + 2 } else { b.len() };
        } else {
            return i;
        }
    }
}

/// Advance past a quoted string (either quote style). `i` is on the opening
/// quote; returns one past the closing quote.
fn skip_string(b: &[u8], mut i: usize) -> Result<usize, String> {
    let quote = b[i];
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            c if c == quote => return Ok(i + 1),
            _ => i += 1,
        }
    }
    Err("unterminated string".to_string())
}

/// Advance past one value (object, array, string, or primitive token).
fn skip_value(b: &[u8], mut i: usize) -> Result<usize, String> {
    if i >= b.len() {
        return Err("expected a value, found end of input".to_string());
    }
    match b[i] {
        b'"' | b'\'' => skip_string(b, i),
        b'{' | b'[' => {
            let mut depth: i32 = 0;
            while i < b.len() {
                if i + 1 < b.len() && b[i] == b'/' && (b[i + 1] == b'/' || b[i + 1] == b'*') {
                    i = skip_ws_and_comments(b, i);
                    continue;
                }
                match b[i] {
                    b'"' | b'\'' => i = skip_string(b, i)?,
                    b'{' | b'[' => {
                        depth += 1;
                        i += 1;
                    }
                    b'}' | b']' => {
                        depth -= 1;
                        i += 1;
                        if depth == 0 {
                            return Ok(i);
                        }
                        if depth < 0 {
                            return Err("unbalanced brackets".to_string());
                        }
                    }
                    _ => i += 1,
                }
            }
            Err("unterminated object or array".to_string())
        }
        _ => {
            // number / true / false / null / bare token
            let start = i;
            while i < b.len()
                && !matches!(b[i], b',' | b'}' | b']')
                && !(b[i] as char).is_whitespace()
                && !(i + 1 < b.len() && b[i] == b'/' && (b[i + 1] == b'/' || b[i + 1] == b'*'))
            {
                i += 1;
            }
            if i == start {
                Err(format!("expected a value at byte {i}"))
            } else {
                Ok(i)
            }
        }
    }
}

/// Scan the top-level object and return each key with its value span, plus the
/// byte offsets of the opening and closing braces.
pub fn top_level_spans(text: &str) -> Result<(Vec<KeySpan>, usize, usize), String> {
    let b = text.as_bytes();
    let mut i = skip_ws_and_comments(b, 0);
    if i >= b.len() || b[i] != b'{' {
        return Err("config does not start with '{'".to_string());
    }
    let open = i;
    i += 1;
    let mut spans = Vec::new();
    loop {
        i = skip_ws_and_comments(b, i);
        if i >= b.len() {
            return Err("unterminated top-level object".to_string());
        }
        if b[i] == b'}' {
            return Ok((spans, open, i));
        }
        if b[i] == b',' {
            i += 1;
            continue;
        }
        let start = i;
        let key = if b[i] == b'"' || b[i] == b'\'' {
            let end = skip_string(b, i)?;
            let k = text[i + 1..end - 1].to_string();
            i = end;
            k
        } else {
            let s = i;
            while i < b.len()
                && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'$' | b'-'))
            {
                i += 1;
            }
            if i == s {
                return Err(format!("unexpected character at byte {i}"));
            }
            text[s..i].to_string()
        };
        i = skip_ws_and_comments(b, i);
        if i >= b.len() || b[i] != b':' {
            return Err(format!("expected ':' after key '{key}'"));
        }
        i += 1;
        let value_start = skip_ws_and_comments(b, i);
        let end = skip_value(b, value_start)?;
        spans.push(KeySpan {
            key,
            start,
            value_start,
            end,
        });
        i = end;
    }
}

use std::collections::BTreeMap;

/// The indentation of the file's first indented line, or four spaces.
pub fn detect_indent(text: &str) -> String {
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.len() == line.len() {
            continue;
        }
        return line[..line.len() - trimmed.len()].to_string();
    }
    "    ".to_string()
}

/// Escape a string for inclusion inside double quotes in JSON text.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// FNV-1a 64 over the raw text, as 16 hex chars. Used to detect concurrent
/// hand-edits between /api/state and /api/apply; not cryptographic.
pub fn content_hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Pretty-print a monitors map in the config's `{ "0": { "modules": [...] } }`
/// shape, using `indent` per level (the value sits one level deep already).
fn format_monitors(map: &BTreeMap<usize, Vec<String>>, indent: &str) -> String {
    if map.is_empty() {
        return "{}".to_string();
    }
    let entries: Vec<String> = map
        .iter()
        .map(|(index, mods)| {
            let list = mods
                .iter()
                .map(|m| format!("\"{}\"", json_escape(m)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{indent}{indent}\"{index}\": {{ \"modules\": [{list}] }}")
        })
        .collect();
    format!("{{\n{}\n{indent}}}", entries.join(",\n"))
}

/// Remove a top-level key and its value, swallowing the surrounding line
/// leftovers: leading indentation, a trailing comma, and a now-blank line end.
fn remove_span(text: &str, span: &KeySpan) -> String {
    let b = text.as_bytes();
    let line_start = text[..span.start].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let start = if text[line_start..span.start]
        .chars()
        .all(|c| c == ' ' || c == '\t')
    {
        line_start
    } else {
        span.start
    };
    let mut end = span.end;
    let mut j = end;
    while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
        j += 1;
    }
    if j < b.len() && b[j] == b',' {
        end = j + 1;
        j += 1;
    }
    while j < b.len() && (b[j] == b' ' || b[j] == b'\t' || b[j] == b'\r') {
        j += 1;
    }
    if j < b.len() && b[j] == b'\n' {
        end = j + 1;
    }
    format!("{}{}", &text[..start], &text[end..])
}

/// Replace (or insert) the top-level `"monitors"` value and drop a legacy
/// top-level `"modules"` key. Everything else is preserved byte-for-byte.
pub fn set_monitors(text: &str, monitors: &BTreeMap<usize, Vec<String>>) -> Result<String, String> {
    let indent = detect_indent(text);
    let value = format_monitors(monitors, &indent);
    let (spans, open, _close) = top_level_spans(text)?;

    let mut out = if let Some(s) = spans.iter().find(|s| s.key == "monitors") {
        format!("{}{}{}", &text[..s.value_start], value, &text[s.end..])
    } else {
        format!(
            "{}\n{indent}\"monitors\": {value},{}",
            &text[..open + 1],
            &text[open + 1..]
        )
    };

    let (spans, _, _) = top_level_spans(&out)?;
    if let Some(s) = spans.iter().find(|s| s.key == "modules").cloned() {
        out = remove_span(&out, &s);
    }
    Ok(out)
}

/// Append a module definition (`body` is pre-indented object text) before the
/// closing brace, preceded by a catalog marker comment.
pub fn append_module(text: &str, name: &str, body: &str) -> Result<String, String> {
    let (spans, open, _close) = top_level_spans(text)?;
    if spans.iter().any(|s| s.key == name) {
        return Err(format!("module '{name}' is already defined"));
    }
    let indent = detect_indent(text);
    let key = json_escape(name);
    let b = text.as_bytes();
    let marker = "// added from the Taskband catalog";

    let (pos, block) = match spans.last() {
        None => (
            open + 1,
            format!("\n{indent}{marker}\n{indent}\"{key}\": {body},\n"),
        ),
        Some(last) => {
            let j = skip_ws_and_comments(b, last.end);
            if j < b.len() && b[j] == b',' {
                (
                    j + 1,
                    format!("\n\n{indent}{marker}\n{indent}\"{key}\": {body},"),
                )
            } else {
                (
                    last.end,
                    format!(",\n\n{indent}{marker}\n{indent}\"{key}\": {body},"),
                )
            }
        }
    };
    Ok(format!("{}{}{}", &text[..pos], block, &text[pos..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_simple_object() {
        let text = r##"{ "a": 1, "b": [1, 2], "c": { "x": "y" } }"##;
        let (spans, open, close) = top_level_spans(text).expect("scans");
        assert_eq!(open, 0);
        assert_eq!(close, text.len() - 1);
        let keys: Vec<&str> = spans.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
        assert_eq!(&text[spans[1].value_start..spans[1].end], "[1, 2]");
        assert_eq!(
            &text[spans[2].value_start..spans[2].end],
            r##"{ "x": "y" }"##
        );
    }

    #[test]
    fn skips_comments_and_tricky_strings() {
        let text = r##"{
            // a comment with braces {} and a "monitors" mention
            "modules": ["cpu"], /* block { comment */
            "cpu": { "exec": "echo \"hi\" // not a comment, has } brace" },
            'single': 'quoted',
        }"##;
        let (spans, _, _) = top_level_spans(text).expect("scans");
        let keys: Vec<&str> = spans.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, vec!["modules", "cpu", "single"]);
        // the cpu value span covers the whole object despite the brace in the string
        assert!(text[spans[1].value_start..spans[1].end].ends_with('}'));
    }

    #[test]
    fn handles_bare_identifier_keys_and_trailing_comma() {
        let text = "{ unquoted_key: 1, other: true, }";
        let (spans, _, _) = top_level_spans(text).expect("scans");
        let keys: Vec<&str> = spans.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, vec!["unquoted_key", "other"]);
        let (_, _, close) = top_level_spans(text).expect("scans");
        assert_eq!(&text[close..close + 1], "}");
    }

    #[test]
    fn errors_on_malformed_input() {
        assert!(top_level_spans("not an object").is_err());
        assert!(top_level_spans("{ \"a\": ").is_err());
        assert!(top_level_spans("{ \"a\" 1 }").is_err());
        assert!(top_level_spans("{ \"a\": \"unterminated }").is_err());
    }

    use std::collections::BTreeMap;

    fn mons(entries: &[(usize, &[&str])]) -> BTreeMap<usize, Vec<String>> {
        entries
            .iter()
            .map(|(i, m)| (*i, m.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    #[test]
    fn set_monitors_inserts_when_absent_and_preserves_bytes() {
        let text = r##"{
    // my hand-written comment
    "modules": ["cpu", "clock"],

    "cpu": { "exec": "echo c" }, // trailing comment stays
    "clock": { "exec": "echo t" }
}"##;
        let out = set_monitors(text, &mons(&[(0, &["cpu"]), (1, &["clock", "cpu"])])).unwrap();
        // still parses, and carries the new arrangement
        let cfg = crate::config::parse(&out).expect("edited config parses");
        assert_eq!(cfg.monitors.get("0").unwrap().module_order, vec!["cpu"]);
        assert_eq!(
            cfg.monitors.get("1").unwrap().module_order,
            vec!["clock", "cpu"]
        );
        // legacy top-level modules key is gone
        assert!(cfg.module_order.is_empty());
        // hand-written content survives byte-for-byte
        assert!(out.contains("// my hand-written comment"));
        assert!(out.contains(r##""cpu": { "exec": "echo c" }, // trailing comment stays"##));
    }

    #[test]
    fn set_monitors_replaces_existing_value_only() {
        let text = r##"{
    "monitors": {
        "0": { "modules": ["cpu"] } // old arrangement
    },
    "cpu": { "exec": "echo c" } // untouched
}"##;
        let out = set_monitors(text, &mons(&[(2, &["cpu"])])).unwrap();
        let cfg = crate::config::parse(&out).expect("parses");
        assert!(!cfg.monitors.contains_key("0"));
        assert_eq!(cfg.monitors.get("2").unwrap().module_order, vec!["cpu"]);
        assert!(out.contains(r##""cpu": { "exec": "echo c" } // untouched"##));
        // the old arrangement's comment lived inside the replaced span: gone
        assert!(!out.contains("old arrangement"));
    }

    #[test]
    fn set_monitors_handles_empty_map_and_string_decoys() {
        let text = r##"{
    "note": { "exec": "echo \"monitors\": fake" },
    "monitors": { "0": { "modules": ["note"] } }
}"##;
        let out = set_monitors(text, &BTreeMap::new()).unwrap();
        let cfg = crate::config::parse(&out).expect("parses");
        assert!(cfg.monitors.is_empty());
        // the decoy string value was not mistaken for the monitors key
        assert!(out.contains(r##"echo \"monitors\": fake"##));
    }

    #[test]
    fn append_module_adds_definition_with_comment_marker() {
        let text = r##"{
    "modules": ["cpu"],
    "cpu": { "exec": "echo c" }
}"##;
        let body = "{\n        \"exec\": \"echo m\",\n        \"interval\": 5\n    }";
        let out = append_module(text, "memory", body).unwrap();
        let cfg = crate::config::parse(&out).expect("parses");
        assert_eq!(cfg.modules.get("memory").unwrap().exec, "echo m");
        assert!(out.contains("// added from the Taskband catalog"));
        // existing content is untouched
        assert!(out.contains(r##""cpu": { "exec": "echo c" }"##));
    }

    #[test]
    fn append_module_rejects_duplicates_and_handles_empty_object() {
        let text = r##"{ "cpu": { "exec": "echo c" } }"##;
        assert!(append_module(text, "cpu", "{}").is_err());

        let out = append_module("{}", "cpu", "{ \"exec\": \"echo c\" }").unwrap();
        let cfg = crate::config::parse(&out).expect("parses");
        assert_eq!(cfg.modules.get("cpu").unwrap().exec, "echo c");
    }

    #[test]
    fn detect_indent_reads_first_indented_line() {
        assert_eq!(detect_indent("{\n  \"a\": 1\n}"), "  ");
        assert_eq!(detect_indent("{\n\t\"a\": 1\n}"), "\t");
        assert_eq!(detect_indent("{}"), "    "); // default
    }

    #[test]
    fn json_escape_escapes_paths_and_quotes() {
        assert_eq!(
            json_escape(r##"powershell -File "C:\tools\m.ps1""##),
            r##"powershell -File \"C:\\tools\\m.ps1\""##
        );
    }

    #[test]
    fn content_hash_is_stable_and_sensitive() {
        assert_eq!(content_hash("abc"), content_hash("abc"));
        assert_ne!(content_hash("abc"), content_hash("abd"));
        assert_eq!(content_hash("abc").len(), 16);
    }

    #[test]
    fn edits_roundtrip_on_the_default_config() {
        let out = set_monitors(
            crate::config::DEFAULT_CONFIG,
            &mons(&[(0, &["cpu", "clock"])]),
        )
        .unwrap();
        let cfg = crate::config::parse(&out).expect("default config edit parses");
        assert_eq!(
            cfg.monitors.get("0").unwrap().module_order,
            vec!["cpu", "clock"]
        );
        // the default config's css block is untouched
        assert!(out.contains(r##""font-family": "Segoe UI""##));
    }

    #[test]
    fn scans_the_default_config() {
        let text = crate::config::DEFAULT_CONFIG;
        let (spans, _, _) = top_level_spans(text).expect("default config scans");
        let keys: Vec<&str> = spans.iter().map(|s| s.key.as_str()).collect();
        assert!(keys.contains(&"modules"));
        assert!(keys.contains(&"cpu"));
        assert!(keys.contains(&"clock"));
    }
}
