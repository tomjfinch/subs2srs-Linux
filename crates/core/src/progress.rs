//! Thread-safe progress reporting + cancellation, polled by the UI.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, Default)]
pub struct State {
    pub step: usize,
    pub steps: usize,
    pub step_name: String,
    /// 0..=1 within the current step, or `None` for "busy" (pulse).
    pub fraction: Option<f64>,
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct Progress {
    cancel: AtomicBool,
    state: Mutex<State>,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn check(&self) -> crate::Result<()> {
        if self.is_cancelled() {
            Err(crate::Error::Cancelled)
        } else {
            Ok(())
        }
    }

    pub fn set_steps(&self, steps: usize) {
        let mut s = self.state.lock().unwrap();
        s.steps = steps;
        s.step = 0;
    }

    pub fn next_step(&self, name: &str) {
        let mut s = self.state.lock().unwrap();
        s.step += 1;
        s.step_name = name.to_string();
        s.fraction = Some(0.0);
        s.detail.clear();
    }

    pub fn update(&self, fraction: Option<f64>, detail: impl Into<String>) {
        let mut s = self.state.lock().unwrap();
        s.fraction = fraction.map(|f| f.clamp(0.0, 1.0));
        s.detail = detail.into();
    }

    pub fn snapshot(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    /// Overall fraction across all steps.
    pub fn overall(&self) -> Option<f64> {
        let s = self.state.lock().unwrap();
        if s.steps == 0 {
            return s.fraction;
        }
        let f = s.fraction?;
        Some(((s.step.saturating_sub(1)) as f64 + f) / s.steps as f64)
    }
}
