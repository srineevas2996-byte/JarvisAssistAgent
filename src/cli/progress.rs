use colored::*;
use std::io::{stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::task::JoinHandle;

pub struct ProgressSpinner {
    stop_signal: Arc<AtomicBool>,
    message: Arc<RwLock<String>>,
    join_handle: Option<JoinHandle<()>>,
    start_time: Instant,
}

impl ProgressSpinner {
    /// Starts an asynchronous terminal spinner with an elapsed time ticker
    pub fn start(initial_msg: &str) -> Self {
        let stop_signal = Arc::new(AtomicBool::new(false));
        let message = Arc::new(RwLock::new(initial_msg.to_string()));
        let start_time = Instant::now();

        let stop_clone = Arc::clone(&stop_signal);
        let msg_clone = Arc::clone(&message);

        let join_handle = tokio::spawn(async move {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut i = 0;
            let start = Instant::now();

            while !stop_clone.load(Ordering::Relaxed) {
                let text = {
                    msg_clone.read().map(|g| g.clone()).unwrap_or_default()
                };
                let elapsed = start.elapsed().as_secs_f32();
                let frame = frames[i % frames.len()];

                print!(
                    "\r\x1B[2K{} {} {}",
                    frame.cyan().bold(),
                    text.white().bold(),
                    format!("[{:.1}s]", elapsed).dimmed()
                );
                let _ = stdout().flush();

                i += 1;
                tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;
            }
        });

        Self {
            stop_signal,
            message,
            join_handle: Some(join_handle),
            start_time,
        }
    }

    /// Dynamically updates the displayed spinner message
    pub fn update(&self, new_msg: &str) {
        if let Ok(mut guard) = self.message.write() {
            *guard = new_msg.to_string();
        }
    }

    /// Stops the spinner and completely erases its line from stdout
    pub fn stop_and_clear(mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(h) = self.join_handle.take() {
            h.abort();
        }
        print!("\r\x1B[2K");
        let _ = stdout().flush();
    }

    /// Stops the spinner and prints a green success checkmark with total duration
    pub fn stop_with_success(mut self, final_msg: &str) {
        let elapsed = self.start_time.elapsed().as_secs_f32();
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(h) = self.join_handle.take() {
            h.abort();
        }
        println!(
            "\r\x1B[2K{} {} {}",
            "✓".green().bold(),
            final_msg.green().bold(),
            format!("[{:.1}s]", elapsed).dimmed()
        );
        let _ = stdout().flush();
    }

    /// Stops the spinner and prints an informational message with total duration
    pub fn stop_with_info(mut self, info_msg: &str) {
        let elapsed = self.start_time.elapsed().as_secs_f32();
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(h) = self.join_handle.take() {
            h.abort();
        }
        println!(
            "\r\x1B[2K{} {} {}",
            "ℹ".cyan().bold(),
            info_msg.cyan(),
            format!("[{:.1}s]", elapsed).dimmed()
        );
        let _ = stdout().flush();
    }

    /// Stops the spinner and prints a red failure indicator
    pub fn stop_with_error(mut self, err_msg: &str) {
        let elapsed = self.start_time.elapsed().as_secs_f32();
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(h) = self.join_handle.take() {
            h.abort();
        }
        println!(
            "\r\x1B[2K{} {} {}",
            "✗".red().bold(),
            err_msg.red().bold(),
            format!("[{:.1}s]", elapsed).dimmed()
        );
        let _ = stdout().flush();
    }
}

impl Drop for ProgressSpinner {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(ref h) = self.join_handle {
            h.abort();
        }
    }
}
