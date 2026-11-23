//! Cross-thread UI loop waker trait and active-session dispatchers.

use crate::session_hub::{
    active_session, has_entered_session, set_global_fallback_waker, wake_global_fallback_waker,
};
use std::sync::Arc;

/// Thread-safe UI loop wakeup trait supplied by windowing/embed hosts (e.g. `winit`, Android JNI).
pub trait UiWaker: Send + Sync + 'static {
    /// Signals the host UI event loop to wake up from idle sleep and pump queued async work.
    fn wake(&self);
}

/// Registers (or clears) the `UiWaker` for the currently active `ReactiveSession`.
pub fn set_ui_waker(waker: Option<Arc<dyn UiWaker>>) {
    if has_entered_session() {
        active_session().set_ui_waker(waker);
    } else {
        active_session().set_ui_waker(waker.clone());
        set_global_fallback_waker(waker);
    }
}

/// Triggers the currently active session's `UiWaker` from any thread when a background task finishes.
pub fn wake_ui_loop() {
    if has_entered_session() {
        active_session().wake();
    } else if !active_session().try_wake() {
        wake_global_fallback_waker();
    }
}
