use std::io::{IsTerminal, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
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
        Self {
            interactive: crate::terminal::is_tty() && std::io::stderr().is_terminal(),
            spinner: None,
        }
    }

    pub fn stage(&mut self, message: &str) {
        self.stop();
        if self.interactive {
            self.spinner = Some(Spinner::start(message));
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
    worker: Option<JoinHandle<()>>,
}

impl Spinner {
    pub fn start(message: &str) -> Self {
        Self::start_for(message, std::io::stderr().is_terminal())
    }

    fn start_for(message: &str, interactive: bool) -> Self {
        if !interactive {
            return Self {
                stopped: Arc::new(AtomicBool::new(true)),
                worker: None,
            };
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = Arc::clone(&stopped);
        let message = message.to_string();
        eprint!("\r\u{1b}[2K  {} {message}", FRAMES[0]);
        let _ = std::io::stderr().flush();
        let worker = thread::spawn(move || {
            let mut frame = 1_usize;
            while !worker_stopped.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(50));
                if worker_stopped.load(Ordering::Relaxed) {
                    break;
                }
                eprint!("\r\u{1b}[2K  {} {message}", FRAMES[frame % FRAMES.len()]);
                let _ = std::io::stderr().flush();
                frame += 1;
            }
        });
        Self {
            stopped,
            worker: Some(worker),
        }
    }

    pub fn stop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
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

    use super::Spinner;

    #[test]
    fn rapid_stage_switches_do_not_wait_for_each_frame_interval() {
        let started = Instant::now();
        for _ in 0..5 {
            let mut spinner = Spinner::start_for("Switching stage", true);
            std::thread::sleep(Duration::from_millis(5));
            spinner.stop();
        }
        assert!(
            started.elapsed() < Duration::from_millis(150),
            "stopping five stages took {:?}",
            started.elapsed()
        );
    }
}
