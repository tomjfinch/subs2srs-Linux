//! Running core work off the main thread while the UI polls progress.

use gtk::glib;
use std::sync::Arc;
use std::time::Duration;
use subs2srs_core::progress::{Progress, State};
use subs2srs_core::{Error, Result};

/// Run `work` on a worker thread. `on_tick` is called on the main thread every
/// 100 ms with the progress state; `on_done` once with the result.
pub fn run<T: Send + 'static>(
    progress: Arc<Progress>,
    work: impl FnOnce(&Progress) -> Result<T> + Send + 'static,
    on_tick: impl Fn(&State, Option<f64>) + 'static,
    on_done: impl FnOnce(Result<T>) + 'static,
) {
    let p = progress.clone();
    let mut handle = Some(std::thread::spawn(move || work(&p)));
    let mut on_done = Some(on_done);
    glib::timeout_add_local(Duration::from_millis(100), move || {
        on_tick(&progress.snapshot(), progress.overall());
        if !handle.as_ref().map(|h| h.is_finished()).unwrap_or(true) {
            return glib::ControlFlow::Continue;
        }
        let r = match handle.take().map(|h| h.join()) {
            Some(Ok(r)) => r,
            Some(Err(_)) => Err(Error::msg("The worker thread crashed. Please report this as a bug.")),
            None => Err(Error::msg("internal error")),
        };
        if let Some(f) = on_done.take() {
            f(r);
        }
        glib::ControlFlow::Break
    });
}

/// Run a quick blocking call (probe, snapshot…) and hand the result back.
pub fn background<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static) {
    let progress = Arc::new(Progress::new());
    run(progress, move |_| Ok(work()), |_, _| {}, move |r: Result<T>| {
        if let Ok(v) = r {
            done(v)
        }
    });
}
