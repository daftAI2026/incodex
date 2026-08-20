use std::io::{IsTerminal, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const FRAMES: &[char] = &['|', '/', '-', '\\'];

pub struct Spinner {
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Spinner {
    pub fn start(message: &str) -> Self {
        if !std::io::stderr().is_terminal() {
            return Self {
                stopped: Arc::new(AtomicBool::new(true)),
                worker: None,
            };
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = Arc::clone(&stopped);
        let message = message.to_string();
        let worker = thread::spawn(move || {
            let mut frame = 0_usize;
            while !worker_stopped.load(Ordering::Relaxed) {
                eprint!("\r\u{1b}[2K  {} {message}", FRAMES[frame % FRAMES.len()]);
                let _ = std::io::stderr().flush();
                frame += 1;
                thread::sleep(Duration::from_millis(50));
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
