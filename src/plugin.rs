use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

/// Prevents a console window flashing when a plugin command spawns cmd.exe.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Worker tick; how often the worker wakes to check which modules are due.
const TICK: Duration = Duration::from_millis(100);

pub struct PluginSpec {
    pub name: String,
    pub exec: String,
    pub interval: Duration,
}

pub struct Update {
    pub index: usize,
    pub text: String,
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
            eprintln!("vEnter: module '{name}' exec failed: {e}");
            String::new()
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
                    let text = run_exec(&spec.name, &spec.exec);
                    if tx.send(Update { index: i, text }).is_err() {
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
        assert!(!is_due(Some(Duration::from_millis(500)), Duration::from_secs(2)));
    }
}
