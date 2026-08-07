use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

/// Prevents a console window flashing when a plugin command spawns cmd.exe.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Worker tick; how often the worker wakes to check which modules are due.
const TICK: Duration = Duration::from_millis(100);

/// Longest failure reason shown on the bar; the rest is elided. The full text
/// always goes to stderr.
const ERROR_MAX_CHARS: usize = 60;

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

/// Run one command line through `cmd /C` verbatim. Returns trimmed stdout on
/// success, or a one-line reason on failure.
///
/// Stdout is only trustworthy when the command succeeded. A failing command
/// often writes to it anyway: PowerShell prints its banner before complaining
/// that a `-File` path does not exist, so a module with a stale script path
/// would otherwise render "Windows PowerShell / Copyright (C) ..." on the bar
/// while the real explanation sat unread in stderr.
fn run_exec(name: &str, exec: &str) -> Result<String, String> {
    let out = match Command::new("cmd")
        .raw_arg(format!("/C {exec}"))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            eprintln!("Taskband: module '{name}' exec failed: {e}");
            return Err(e.to_string());
        }
    };
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    eprintln!("Taskband: module '{name}' failed: {}", stderr.trim());
    Err(
        first_line(&stderr).unwrap_or_else(|| match out.status.code() {
            Some(code) => format!("exited with code {code}"),
            None => "terminated".to_string(),
        }),
    )
}

/// The first non-blank line of `text`, trimmed.
fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

/// `text` shortened to at most `max` characters, with a trailing ellipsis.
/// Counts characters rather than bytes, so a non-ASCII reason cannot panic.
fn elide(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// Display lines for a module that failed: its name over the reason. Always
/// plain text, never markup, so a reason containing `<` or `&` stays readable
/// instead of being parsed (or failing to parse) as a span.
pub fn error_lines(name: &str, reason: &str) -> Vec<crate::markup::Line> {
    crate::markup::plain(&format!("{name}\n{}", elide(reason, ERROR_MAX_CHARS)))
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
                    let lines = match run_exec(&spec.name, &spec.exec) {
                        Ok(raw) => interpret(&spec.name, spec.output, &raw),
                        Err(reason) => error_lines(&spec.name, &reason),
                    };
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

    #[test]
    fn successful_command_returns_trimmed_stdout() {
        assert_eq!(run_exec("m", "echo hello").unwrap(), "hello");
    }

    /// The case from the field: a module whose script path no longer exists.
    /// PowerShell prints its banner to stdout *and* the real complaint to
    /// stderr, so showing stdout would put the banner on the bar.
    #[test]
    fn failing_command_reports_stderr_not_stdout() {
        let reason = run_exec("m", "echo banner && echo the real problem 1>&2 && exit 1")
            .expect_err("non-zero exit is an error");
        assert_eq!(reason, "the real problem");
        assert!(!reason.contains("banner"));
    }

    #[test]
    fn failing_command_without_stderr_reports_its_exit_code() {
        assert_eq!(run_exec("m", "exit 3").unwrap_err(), "exited with code 3");
    }

    #[test]
    fn error_lines_show_the_name_over_the_reason() {
        let lines = error_lines("memory", "file not found");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], vec![seg("memory", &[])]);
        assert_eq!(lines[1], vec![seg("file not found", &[])]);
    }

    /// A reason is never parsed as markup, so a message containing angle
    /// brackets stays readable instead of vanishing or erroring again.
    #[test]
    fn error_lines_never_parse_markup() {
        let lines = error_lines("m", "unexpected <span> & more");
        assert_eq!(lines[1], vec![seg("unexpected <span> & more", &[])]);
    }

    #[test]
    fn long_reasons_are_elided() {
        assert_eq!(elide("short", 10), "short");
        assert_eq!(elide("0123456789", 10), "0123456789");
        assert_eq!(elide("0123456789x", 10), "012345678…");
        // counts characters, not bytes, so a multi-byte reason cannot panic
        assert_eq!(elide("ünïcödé is fine here", 5), "ünïc…");
    }

    #[test]
    fn first_line_skips_blanks_and_trims() {
        assert_eq!(
            first_line("\n\n  hello  \nworld"),
            Some("hello".to_string())
        );
        assert_eq!(first_line("   \n  "), None);
    }
}
