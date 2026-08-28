use std::io::{IsTerminal, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const FRAMES: &[char] = &['|', '/', '-', '\\'];
const SPINNER_PREFIX_COLUMNS: usize = 4;
const SPINNER_RESERVED_COLUMNS: usize = 8;
const MIN_MESSAGE_COLUMNS: usize = 20;

fn terminal_is_interactive() -> bool {
    #[cfg(target_os = "windows")]
    {
        crate::windows_console::is_tty() && std::io::stderr().is_terminal()
    }
    #[cfg(not(target_os = "windows"))]
    {
        crate::terminal::is_tty() && std::io::stderr().is_terminal()
    }
}

fn terminal_columns() -> usize {
    #[cfg(target_os = "windows")]
    {
        crate::windows_console::stderr_columns()
    }
    #[cfg(not(target_os = "windows"))]
    {
        crate::terminal::stderr_columns()
    }
}

fn format_spinner_message(message: &str, columns: usize) -> String {
    let sanitized = message.replace(['\r', '\n'], " ");
    let maximum = columns.saturating_sub(SPINNER_PREFIX_COLUMNS);
    let preferred = columns
        .saturating_sub(SPINNER_RESERVED_COLUMNS)
        .max(MIN_MESSAGE_COLUMNS);
    let available = preferred.min(maximum);
    let length = sanitized.chars().count();
    if length <= available {
        return sanitized;
    }
    if available > 3 {
        format!(
            "{}...",
            sanitized.chars().take(available - 3).collect::<String>()
        )
    } else {
        sanitized.chars().take(available).collect()
    }
}

fn format_spinner_frame(frame: char, message: &str, columns: usize, clear: bool) -> String {
    let lead = if clear { "\r\u{1b}[2K" } else { "\r" };
    let message = format_spinner_message(message, columns);
    format!("{lead}  \u{1b}[1;34m{frame}\u{1b}[0m {message}")
}

pub struct Progress {
    interactive: bool,
    spinner: Option<Spinner>,
}

impl Progress {
    pub fn new() -> Self {
        Self::new_for(terminal_is_interactive())
    }

    fn new_for(interactive: bool) -> Self {
        Self {
            interactive,
            spinner: None,
        }
    }

    pub fn stage(&mut self, message: &str) {
        if self.interactive {
            if let Some(spinner) = &mut self.spinner {
                spinner.update_message(message);
            } else {
                self.spinner = Some(Spinner::start_for(message, true));
            }
        } else {
            println!("{}", incodex_core::format_step(message, None));
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut spinner) = self.spinner.take() {
            spinner.stop();
        }
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct Spinner {
    stopped: Arc<AtomicBool>,
    state: Arc<Mutex<SpinnerState>>,
    worker: Option<JoinHandle<()>>,
}

struct SpinnerState {
    message: String,
    next_frame: usize,
    last_columns: usize,
}

impl SpinnerState {
    fn line_needs_clear(&mut self, columns: usize, message_changed: bool) -> bool {
        let columns_changed = self.last_columns != columns;
        self.last_columns = columns;
        message_changed || columns_changed
    }

    fn render(&mut self, columns: usize, clear: bool) {
        eprint!(
            "{}",
            format_spinner_frame(
                FRAMES[self.next_frame % FRAMES.len()],
                &self.message,
                columns,
                clear,
            )
        );
        self.next_frame += 1;
        let _ = std::io::stderr().flush();
    }
}

impl Spinner {
    pub fn start(message: &str) -> Self {
        Self::start_for(message, terminal_is_interactive())
    }

    fn start_for(message: &str, interactive: bool) -> Self {
        Self::start_for_interval(message, interactive, Duration::from_millis(50))
    }

    fn start_for_interval(message: &str, interactive: bool, frame_interval: Duration) -> Self {
        let initial_columns = terminal_columns();
        let state = Arc::new(Mutex::new(SpinnerState {
            message: message.to_string(),
            next_frame: 0,
            last_columns: initial_columns,
        }));
        if !interactive {
            return Self {
                stopped: Arc::new(AtomicBool::new(true)),
                state,
                worker: None,
            };
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = Arc::clone(&stopped);
        let worker_state = Arc::clone(&state);
        {
            let mut state = state.lock().unwrap();
            state.render(initial_columns, true);
        }
        let worker = thread::spawn(move || {
            while !worker_stopped.load(Ordering::Relaxed) {
                thread::park_timeout(frame_interval);
                if worker_stopped.load(Ordering::Relaxed) {
                    break;
                }
                let mut state = worker_state.lock().unwrap();
                let columns = terminal_columns();
                let clear = state.line_needs_clear(columns, false);
                state.render(columns, clear);
            }
        });
        Self {
            stopped,
            state,
            worker: Some(worker),
        }
    }

    fn update_message(&mut self, message: &str) {
        let mut state = self.state.lock().unwrap();
        if state.message == message {
            return;
        }
        state.message = message.to_string();
        if self.worker.is_some() {
            let columns = terminal_columns();
            let clear = state.line_needs_clear(columns, true);
            state.render(columns, clear);
        }
    }

    pub fn stop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
            eprint!("\r\u{1b}[2K");
            let _ = std::io::stderr().flush();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{format_spinner_message, Progress, Spinner, SpinnerState};

    #[test]
    fn spinner_clears_the_line_when_terminal_width_changes() {
        let mut state = SpinnerState {
            message: "Working".into(),
            next_frame: 1,
            last_columns: 80,
        };
        assert!(state.line_needs_clear(24, false));
        assert!(state.line_needs_clear(100, false));
    }

    #[test]
    fn spinner_message_replaces_line_breaks_with_spaces() {
        assert_eq!(
            format_spinner_message("First\rSecond\nThird", 80),
            "First Second Third"
        );
    }

    #[test]
    fn stopping_wakes_worker_before_long_frame_timeout() {
        let mut spinner =
            Spinner::start_for_interval("Switching stage", true, Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(50));
        let started = Instant::now();
        spinner.stop();
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "stopping the worker took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn progress_stage_updates_reuse_the_running_worker() {
        let mut progress = Progress::new_for(true);
        progress.stage("First stage");
        let worker = progress
            .spinner
            .as_ref()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .thread()
            .id();
        progress.stage("Second stage");
        assert_eq!(
            progress
                .spinner
                .as_ref()
                .unwrap()
                .worker
                .as_ref()
                .unwrap()
                .thread()
                .id(),
            worker
        );
        progress.stop();
    }
}
