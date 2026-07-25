//! The configurator backend: a loopback HTTP server started from the tray
//! that serves the drag-and-drop page plus two JSON endpoints, /api/state and
//! /api/apply. This file keeps the endpoint logic pure (text in, text out) so
//! it is unit-testable; the HTTP and Win32 wiring lives at the bottom.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// One attached monitor, as reported to the page.
pub struct MonitorSnapshot {
    pub index: usize,
    pub name: String,
    pub primary: bool,
    pub width: i32,
    pub height: i32,
    pub has_taskbar: bool,
}

/// The /api/apply request body.
#[derive(serde::Deserialize)]
pub struct ApplyRequest {
    pub hash: String,
    /// Monitor index (as a string) to ordered module names, for every monitor
    /// shown on the page.
    pub monitors: HashMap<String, Vec<String>>,
    /// Catalog modules to copy into the config before arranging.
    #[serde(default)]
    pub add: Vec<String>,
}

/// The /api/state response: the parsed arrangement, defined module names, the
/// catalog (minus already-defined names), and a content hash for conflict
/// detection. A config that fails to parse reports `error` instead.
pub fn state_json(config_text: &str, monitors: &[MonitorSnapshot]) -> serde_json::Value {
    let hash = crate::editor::content_hash(config_text);
    let cfg = match crate::config::parse(config_text) {
        Ok(cfg) => cfg,
        Err(e) => return serde_json::json!({ "hash": hash, "error": e }),
    };

    let arrangement = |m: &MonitorSnapshot| -> Vec<String> {
        if !cfg.monitors.is_empty() {
            cfg.monitors
                .get(&m.index.to_string())
                .map(|mc| mc.module_order.clone())
                .unwrap_or_default()
        } else if m.primary {
            cfg.module_order.clone()
        } else {
            Vec::new()
        }
    };

    let mut defined: Vec<&String> = cfg.modules.keys().collect();
    defined.sort();
    let catalog: Vec<serde_json::Value> = crate::catalog::ENTRIES
        .iter()
        .filter(|e| !cfg.modules.contains_key(e.name))
        .map(|e| serde_json::json!({ "name": e.name, "description": e.description }))
        .collect();

    serde_json::json!({
        "hash": hash,
        "error": null,
        "monitors": monitors.iter().map(|m| serde_json::json!({
            "index": m.index,
            "name": m.name,
            "primary": m.primary,
            "width": m.width,
            "height": m.height,
            "hasTaskbar": m.has_taskbar,
            "modules": arrangement(m),
        })).collect::<Vec<_>>(),
        "defined": defined,
        "catalog": catalog,
    })
}

/// Apply an arrangement to the config text: materialize and append requested
/// catalog modules, then rewrite `monitors`. Entries for monitor indices that
/// are not currently `attached` are preserved verbatim (an unplugged dock
/// monitor keeps its arrangement). The result is guaranteed to parse.
pub fn apply_to_text(
    text: &str,
    req: &ApplyRequest,
    attached: &[usize],
    config_dir: &Path,
) -> Result<String, String> {
    let indent = crate::editor::detect_indent(text);
    let mut out = text.to_string();

    for name in &req.add {
        let entry = crate::catalog::find(name)
            .ok_or_else(|| format!("unknown catalog module '{name}'"))?;
        let script = crate::catalog::materialize(entry, config_dir)?;
        let body = crate::catalog::definition_body(entry, &indent, script.as_deref());
        out = crate::editor::append_module(&out, name, &body)?;
    }

    let mut merged: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    if let Ok(cfg) = crate::config::parse(text) {
        for (key, mc) in &cfg.monitors {
            if let Ok(index) = key.parse::<usize>() {
                if !attached.contains(&index) {
                    merged.insert(index, mc.module_order.clone());
                }
            }
        }
    }
    for (key, mods) in &req.monitors {
        let index = key
            .parse::<usize>()
            .map_err(|_| format!("bad monitor index '{key}'"))?;
        merged.insert(index, mods.clone());
    }
    out = crate::editor::set_monitors(&out, &merged)?;

    crate::config::parse(&out)
        .map_err(|e| format!("internal error, apply produced an invalid config: {e}"))?;
    Ok(out)
}

use std::path::PathBuf;
use std::sync::OnceLock;

/// The configurator page, bundled into the binary.
const PAGE: &str = include_str!("../assets/configurator.html");

/// Start the server on first call and open the default browser at its URL.
/// `driver` is the primary bar window handle (as an integer, so it can cross
/// threads); a successful apply posts `WM_APP_RELOAD` to it.
pub fn open_configurator(config_path: &Path, driver: isize) {
    static URL: OnceLock<Option<String>> = OnceLock::new();
    let url = URL.get_or_init(|| start(config_path.to_path_buf(), driver));
    match url {
        Some(url) => open_in_browser(url),
        None => eprintln!("Taskband: configurator server failed to start (see earlier error)"),
    }
}

fn start(path: PathBuf, driver: isize) -> Option<String> {
    let server = match tiny_http::Server::http("127.0.0.1:0") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Taskband: cannot bind configurator server: {e}");
            return None;
        }
    };
    let port = server.server_addr().to_ip()?.port();
    let mut bytes = [0u8; 16];
    if let Err(e) = getrandom::getrandom(&mut bytes) {
        eprintln!("Taskband: cannot generate configurator token: {e}");
        return None;
    }
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let url = format!("http://127.0.0.1:{port}/?token={token}");
    std::thread::spawn(move || serve(server, path, driver, token));
    Some(url)
}

fn open_in_browser(url: &str) {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// The config text the page should see: the file if present, else the same
/// built-in default the app itself falls back to.
fn config_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|_| crate::config::DEFAULT_CONFIG.to_string())
}

fn snapshots() -> Vec<MonitorSnapshot> {
    crate::taskbar::detect()
        .iter()
        .map(|m| MonitorSnapshot {
            index: m.index,
            name: crate::taskbar::display_name(m.hmonitor),
            primary: m.primary,
            width: m.rect.right - m.rect.left,
            height: m.rect.bottom - m.rect.top,
            has_taskbar: m.taskbar.is_some(),
        })
        .collect()
}

type HttpResponse = tiny_http::Response<std::io::Cursor<Vec<u8>>>;

fn response(status: u16, content_type: &str, body: String) -> HttpResponse {
    tiny_http::Response::from_string(body)
        .with_status_code(status)
        .with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
                .expect("static header"),
        )
}

fn json_response(status: u16, body: serde_json::Value) -> HttpResponse {
    response(status, "application/json", body.to_string())
}

fn serve(server: tiny_http::Server, path: PathBuf, driver: isize, token: String) {
    for mut request in server.incoming_requests() {
        let url = request.url().to_string();
        let (route, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
        let expected = format!("token={token}");
        let authorized = query.split('&').any(|kv| kv == expected);

        let resp = match (request.method().clone(), route) {
            (tiny_http::Method::Get, "/") => {
                response(200, "text/html; charset=utf-8", PAGE.to_string())
            }
            (tiny_http::Method::Get, "/api/state") if authorized => {
                json_response(200, state_json(&config_text(&path), &snapshots()))
            }
            (tiny_http::Method::Post, "/api/apply") if authorized => {
                handle_apply(&mut request, &path, driver)
            }
            _ if route.starts_with("/api/") => {
                json_response(403, serde_json::json!({ "error": "missing or bad token" }))
            }
            _ => json_response(404, serde_json::json!({ "error": "not found" })),
        };
        let _ = request.respond(resp);
    }
}

fn handle_apply(request: &mut tiny_http::Request, path: &Path, driver: isize) -> HttpResponse {
    let req: ApplyRequest = match serde_json::from_reader(request.as_reader()) {
        Ok(r) => r,
        Err(e) => {
            return json_response(400, serde_json::json!({ "error": format!("bad request: {e}") }))
        }
    };

    let text = config_text(path);
    if crate::editor::content_hash(&text) != req.hash {
        return json_response(
            409,
            serde_json::json!({ "error": "config.json changed on disk" }),
        );
    }

    let attached: Vec<usize> = crate::taskbar::detect().iter().map(|m| m.index).collect();
    let config_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let out = match apply_to_text(&text, &req, &attached, config_dir) {
        Ok(out) => out,
        Err(e) => return json_response(422, serde_json::json!({ "error": e })),
    };

    // Atomic write: the watcher (and any concurrent reader) never sees a
    // partial file. `rename` replaces the destination on Windows.
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, &out).and_then(|()| std::fs::rename(&tmp, path)) {
        return json_response(
            500,
            serde_json::json!({ "error": format!("writing config: {e}") }),
        );
    }

    // The watcher reloads on its own; this post covers the file-was-missing
    // case (nothing was being watched at the old path yet) and costs nothing.
    unsafe {
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
        let _ = PostMessageW(
            HWND(driver as *mut core::ffi::c_void),
            crate::window::WM_APP_RELOAD,
            WPARAM(0),
            LPARAM(0),
        );
    }

    json_response(
        200,
        serde_json::json!({ "ok": true, "hash": crate::editor::content_hash(&out) }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(index: usize, primary: bool) -> MonitorSnapshot {
        MonitorSnapshot {
            index,
            name: format!(r"\\.\DISPLAY{}", index + 1),
            primary,
            width: 2560,
            height: 1440,
            has_taskbar: true,
        }
    }

    #[test]
    fn state_maps_legacy_modules_to_primary() {
        let text = r##"{
            "modules": ["cpu", "clock"],
            "cpu": { "exec": "echo c" },
            "clock": { "exec": "echo t" }
        }"##;
        let v = state_json(text, &[snapshot(0, true), snapshot(1, false)]);
        assert!(v["error"].is_null());
        assert_eq!(v["hash"], crate::editor::content_hash(text));
        assert_eq!(v["monitors"][0]["modules"], serde_json::json!(["cpu", "clock"]));
        assert_eq!(v["monitors"][1]["modules"], serde_json::json!([]));
        // defined is sorted; catalog excludes already-defined names
        assert_eq!(v["defined"], serde_json::json!(["clock", "cpu"]));
        let catalog_names: Vec<&str> = v["catalog"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert_eq!(catalog_names, vec!["memory", "disk-space"]);
    }

    #[test]
    fn state_uses_monitors_map_when_present() {
        let text = r##"{
            "monitors": { "1": { "modules": ["cpu"] } },
            "cpu": { "exec": "echo c" }
        }"##;
        let v = state_json(text, &[snapshot(0, true), snapshot(1, false)]);
        assert_eq!(v["monitors"][0]["modules"], serde_json::json!([]));
        assert_eq!(v["monitors"][1]["modules"], serde_json::json!(["cpu"]));
    }

    #[test]
    fn state_reports_parse_errors() {
        let v = state_json("{ not json5", &[snapshot(0, true)]);
        assert!(v["error"].is_string());
        assert!(v["hash"].is_string());
    }

    #[test]
    fn apply_rearranges_and_preserves_detached_monitors() {
        let text = r##"{
    // keep me
    "monitors": {
        "0": { "modules": ["cpu"] },
        "7": { "modules": ["clock"] }
    },
    "cpu": { "exec": "echo c" },
    "clock": { "exec": "echo t" }
}"##;
        let req = ApplyRequest {
            hash: crate::editor::content_hash(text),
            monitors: HashMap::from([(
                "0".to_string(),
                vec!["clock".to_string(), "cpu".to_string()],
            )]),
            add: vec![],
        };
        // monitor 7 is not attached: its arrangement must survive
        let out = apply_to_text(text, &req, &[0], Path::new(".")).unwrap();
        let cfg = crate::config::parse(&out).unwrap();
        assert_eq!(
            cfg.monitors.get("0").unwrap().module_order,
            vec!["clock", "cpu"]
        );
        assert_eq!(cfg.monitors.get("7").unwrap().module_order, vec!["clock"]);
        assert!(out.contains("// keep me"));
    }

    #[test]
    fn apply_drops_entries_the_user_cleared_on_attached_monitors() {
        let text = r##"{
            "monitors": { "0": { "modules": ["cpu"] }, "1": { "modules": ["cpu"] } },
            "cpu": { "exec": "echo c" }
        }"##;
        let req = ApplyRequest {
            hash: String::new(),
            monitors: HashMap::from([
                ("0".to_string(), vec![]),
                ("1".to_string(), vec!["cpu".to_string()]),
            ]),
            add: vec![],
        };
        let out = apply_to_text(text, &req, &[0, 1], Path::new(".")).unwrap();
        let cfg = crate::config::parse(&out).unwrap();
        assert!(cfg.monitors.get("0").unwrap().module_order.is_empty());
        assert_eq!(cfg.monitors.get("1").unwrap().module_order, vec!["cpu"]);
    }

    #[test]
    fn apply_adds_catalog_modules() {
        let dir = std::env::temp_dir().join(format!("taskband-apply-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let text = r##"{ "cpu": { "exec": "echo c" } }"##;
        let req = ApplyRequest {
            hash: String::new(),
            monitors: HashMap::from([("0".to_string(), vec!["memory".to_string()])]),
            add: vec!["memory".to_string()],
        };
        let out = apply_to_text(text, &req, &[0], &dir).unwrap();
        let cfg = crate::config::parse(&out).unwrap();
        let memory = cfg.modules.get("memory").expect("memory added");
        assert!(memory.exec.contains("memory.ps1"));
        assert!(dir.join(r"modules\memory\memory.ps1").exists());
        assert!(out.contains("// added from the Taskband catalog"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_rejects_unknown_catalog_names_and_bad_indices() {
        let text = "{}";
        let bad_add = ApplyRequest {
            hash: String::new(),
            monitors: HashMap::new(),
            add: vec!["ghost".to_string()],
        };
        assert!(apply_to_text(text, &bad_add, &[], Path::new(".")).is_err());

        let bad_index = ApplyRequest {
            hash: String::new(),
            monitors: HashMap::from([("first".to_string(), vec![])]),
            add: vec![],
        };
        assert!(apply_to_text(text, &bad_index, &[], Path::new(".")).is_err());
    }
}
