use std::io::{IsTerminal, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const FRAMES: &[char] = &['|', '/', '-', '\\'];

pub struct Progress {
    interactive: bool,
    spinner: Option<Spinner>,
}

impl Progress {
    pub fn new() -> Self {
        Self::new_for(crate::terminal::is_tty() && std::io::stderr().is_terminal())
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

impl Drop for Progress {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct Spinner {
    stopped: Arc<AtomicBool>,
    message: Arc<Mutex<String>>,
    worker: Option<JoinHandle<()>>,
}

impl Spinner {
    pub fn start(message: &str) -> Self {
        Self::start_for(
            message,
            crate::terminal::is_tty() && std::io::stderr().is_terminal(),
        )
    }

    fn start_for(message: &str, interactive: bool) -> Self {
        Self::start_for_interval(message, interactive, Duration::from_millis(50))
    }

    fn start_for_interval(message: &str, interactive: bool, frame_interval: Duration) -> Self {
        let message = Arc::new(Mutex::new(message.to_string()));
        if !interactive {
            return Self {
                stopped: Arc::new(AtomicBool::new(true)),
                message,
                worker: None,
            };
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = Arc::clone(&stopped);
        let worker_message = Arc::clone(&message);
        eprint!("\r\u{1b}[2K  {} {}", FRAMES[0], message.lock().unwrap());
        let _ = std::io::stderr().flush();
        let worker = thread::spawn(move || {
            let mut frame = 1_usize;
            while !worker_stopped.load(Ordering::Relaxed) {
                thread::park_timeout(frame_interval);
                if worker_stopped.load(Ordering::Relaxed) {
                    break;
                }
                eprint!(
                    "\r\u{1b}[2K  {} {}",
                    FRAMES[frame % FRAMES.len()],
                    worker_message.lock().unwrap()
                );
                let _ = std::io::stderr().flush();
                frame += 1;
            }
        });
        Self {
            stopped,
            message,
            worker: Some(worker),
        }
    }

    fn update_message(&mut self, message: &str) {
        let mut current = self.message.lock().unwrap();
        *current = message.to_string();
        if self.worker.is_some() {
            eprint!("\r\u{1b}[2K  {} {current}", FRAMES[0]);
            let _ = std::io::stderr().flush();
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

    use super::{Progress, Spinner};

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
