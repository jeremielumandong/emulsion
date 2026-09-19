//! Progress and cancellation for long-running work.
//!
//! A [`Job`] is shared between the thread doing the work and whoever shows
//! a progress bar: the worker calls [`Job::progress`] and checks
//! [`Job::cancelled`]; the UI reads [`Job::fraction`] and [`Job::stage`] and
//! may [`Job::cancel`]. It carries no threads of its own so it works with any
//! executor.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[derive(Debug, Default)]
pub struct Job {
    /// Progress in 1/10000ths.
    done: AtomicU32,
    cancel: AtomicBool,
    finished: AtomicBool,
    stage: Mutex<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

impl Job {
    pub fn new() -> Arc<Job> {
        Arc::new(Job::default())
    }

    /// Report progress as a fraction (0–1) of the whole job.
    pub fn progress(&self, fraction: f32) {
        let v = (fraction.clamp(0.0, 1.0) * 10_000.0) as u32;
        self.done.fetch_max(v, Ordering::Relaxed);
    }

    /// Report progress of one stage that spans `from..to` of the whole.
    pub fn progress_in(&self, from: f32, to: f32, fraction: f32) {
        self.progress(from + (to - from) * fraction.clamp(0.0, 1.0));
    }

    pub fn fraction(&self) -> f32 {
        self.done.load(Ordering::Relaxed) as f32 / 10_000.0
    }

    pub fn set_stage(&self, s: impl Into<String>) {
        if let Ok(mut g) = self.stage.lock() {
            *g = s.into();
        }
    }

    pub fn stage(&self) -> String {
        self.stage.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }

    /// Mark the work over, successful or not; pollers stop here.
    pub fn finish(&self) {
        self.finished.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    /// `stage · 42 %`, for a status line.
    pub fn summary(&self) -> String {
        let stage = self.stage();
        let pct = (self.fraction() * 100.0).round() as u32;
        if stage.is_empty() {
            format!("{pct} %")
        } else {
            format!("{stage} · {pct} %")
        }
    }

    /// Return early from a worker when cancelled.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_monotonic_and_stages_are_shared() {
        let j = Job::new();
        j.progress(0.5);
        j.progress(0.2);
        assert!((j.fraction() - 0.5).abs() < 1e-3, "never goes backwards");
        j.progress_in(0.5, 1.0, 0.5);
        assert!((j.fraction() - 0.75).abs() < 1e-3);
        j.set_stage("encoding");
        let j2 = j.clone();
        assert_eq!(j2.stage(), "encoding");
        assert!(j.check().is_ok());
        j2.cancel();
        assert!(j.check().is_err());
    }
}
