use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

/// Prevents a console window flashing when a plugin command spawns cmd.exe.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Worker tick; how often the worker wakes to check which modules are due.
const TICK: Duration = Duration::from_millis(100);

/// How a module's stdout is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Raw text, shown verbatim (the default).
    Text,
    /// Text with inline `<span class='...'>` markup (see `src/markup.rs`).
    Html,
}

pub struct PluginSpec {
    pub name: String,
    pub exec: String,
    pub interval: Duration,
    pub output: OutputMode,
}

pub struct Update {
    pub index: usize,
    pub lines: Vec<crate::markup::Line>,
}

/// A module is due when it has never run, or `interval` has elapsed since it did.
pub fn is_due(elapsed_since_last: Option<Duration>, interval: Duration) -> bool {
    match elapsed_since_last {
        None => true,
        Some(elapsed) => elapsed >= interval,
    }
}

/// Run one command line through `cmd /C` verbatim and return trimmed stdout.
fn run_exec(name: &str, exec: &str) -> String {
    match Command::new("cmd")
        .raw_arg(format!("/C {exec}"))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(e) => {
            eprintln!("Taskband: module '{name}' exec failed: {e}");
            String::new()
        }
    }
}

/// Turn raw stdout into display lines per the module's output mode. Bad
/// markup warns on stderr and falls back to plain text, so a broken module
/// stays visible.
pub fn interpret(name: &str, mode: OutputMode, raw: &str) -> Vec<crate::markup::Line> {
    if mode == OutputMode::Text {
        return crate::markup::plain(raw);
    }
    match crate::markup::parse(raw) {
        Ok((lines, warnings)) => {
            for w in warnings {
                eprintln!("Taskband: module '{name}': {w}");
            }
            lines
        }
        Err(e) => {
            eprintln!("Taskband: module '{name}': bad markup ({e}); showing raw text");
            crate::markup::plain(raw)
        }
    }
}

/// Spawn a background thread that runs due plugins and streams `(index, text)`
/// updates. The thread exits when the receiver is dropped.
pub fn spawn_worker(specs: Vec<PluginSpec>) -> Receiver<Update> {
    let (tx, rx) = mpsc::channel::<Update>();
    thread::spawn(move || {
        let mut last_run: Vec<Option<Instant>> = vec![None; specs.len()];
        loop {
            let now = Instant::now();
            for (i, spec) in specs.iter().enumerate() {
                let elapsed = last_run[i].map(|t| now.duration_since(t));
                if is_due(elapsed, spec.interval) {
                    last_run[i] = Some(now);
                    let raw = run_exec(&spec.name, &spec.exec);
                    let lines = interpret(&spec.name, spec.output, &raw);
                    if tx.send(Update { index: i, lines }).is_err() {
                        return; // receiver gone; UI closed
                    }
                }
            }
            thread::sleep(TICK);
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn never_run_is_due() {
        assert!(is_due(None, Duration::from_secs(2)));
    }

    #[test]
    fn due_when_interval_elapsed() {
        assert!(is_due(Some(Duration::from_secs(2)), Duration::from_secs(2)));
        assert!(is_due(Some(Duration::from_secs(3)), Duration::from_secs(2)));
    }

    #[test]
    fn not_due_before_interval() {
        assert!(!is_due(
            Some(Duration::from_millis(500)),
            Duration::from_secs(2)
        ));
    }

    use crate::markup::Segment;

    fn seg(text: &str, classes: &[&str]) -> Segment {
        Segment {
            text: text.to_string(),
            classes: classes.iter().map(|c| c.to_string()).collect(),
        }
    }

    #[test]
    fn text_mode_passes_output_through_verbatim() {
        let lines = interpret("m", OutputMode::Text, "<span class='x'>a</span>\n<b>&</b>");
        assert_eq!(lines[0], vec![seg("<span class='x'>a</span>", &[])]);
        assert_eq!(lines[1], vec![seg("<b>&</b>", &[])]);
    }

    #[test]
    fn html_mode_parses_spans_and_newlines() {
        let lines = interpret(
            "m",
            OutputMode::Html,
            "<span class='title'>MEM</span>\n<span class='warning'>76%</span> used",
        );
        assert_eq!(lines[0], vec![seg("MEM", &["title"])]);
        assert_eq!(lines[1], vec![seg("76%", &["warning"]), seg(" used", &[])]);
    }

    #[test]
    fn html_mode_plain_text_passes_through() {
        let lines = interpret("m", OutputMode::Html, "just text\nsecond line");
        assert_eq!(lines, crate::markup::plain("just text\nsecond line"));
    }

    #[test]
    fn html_mode_bad_markup_falls_back_to_raw_text() {
        let raw = "<b>92%</b> & done";
        let lines = interpret("m", OutputMode::Html, raw);
        assert_eq!(lines, vec![vec![seg(raw, &[])]]);
    }

    #[test]
    fn html_mode_tolerates_unclosed_span() {
        let lines = interpret("m", OutputMode::Html, "<span class='c'>oops");
        assert_eq!(lines, vec![vec![seg("oops", &["c"])]]);
    }

    #[test]
    fn empty_output_yields_no_lines() {
        assert!(interpret("m", OutputMode::Text, "").is_empty());
        assert!(interpret("m", OutputMode::Html, "").is_empty());
    }
}
