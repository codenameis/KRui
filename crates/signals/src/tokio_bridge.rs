//! Built-in multi-threaded `tokio` async runtime and `Scope`-aware task/stream/timer bridges.

use crate::frame_clock::register_async_poller_with_retention;
use crate::session_hub::{current_wake_handle, SessionWakeHandle};
use crate::{batch, current_scope_id, is_scope_alive, ScopeId};
use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::mpsc;

const DEFAULT_ASYNC_WORKER_THREADS: usize = 2;

static TOKIO_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Returns the process-wide shared `tokio` multi-threaded async runtime built into KRui.
pub fn tokio_runtime() -> &'static tokio::runtime::Runtime {
    TOKIO_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(DEFAULT_ASYNC_WORKER_THREADS)
            .thread_name("krui-tokio")
            .enable_all()
            .build()
            .expect("Failed to initialize KRui built-in tokio runtime")
    })
}

/// Returns a clonable `tokio::runtime::Handle` to KRui's built-in async runtime.
pub fn tokio_handle() -> tokio::runtime::Handle {
    tokio_runtime().handle().clone()
}

/// Handle to a spawned `tokio` task/stream/interval auto-aborted if its owning `Scope` disposes.
///
/// Strictly `!Send` and `!Sync` so `is_active()` evaluates against the owning UI thread's arena.
///
/// ```compile_fail
/// use krui_signals::AsyncTaskHandle;
/// fn assert_send<T: Send>() {}
/// assert_send::<AsyncTaskHandle>();
/// ```
#[derive(Clone)]
pub struct AsyncTaskHandle {
    abort_handle: tokio::task::AbortHandle,
    is_active: Arc<AtomicBool>,
    scope_id: ScopeId,
    _marker: PhantomData<*const ()>,
}

impl AsyncTaskHandle {
    /// Immediately cancels the underlying `tokio` task or stream.
    pub fn abort(&self) {
        self.abort_handle.abort();
        self.is_active.store(false, Ordering::Release);
    }

    /// Returns `true` if the task has completed, panicked, or been aborted.
    pub fn is_finished(&self) -> bool {
        !self.is_active.load(Ordering::Acquire)
    }

    /// Returns `true` if the task is still running and its owning `Scope` is alive.
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Acquire) && is_scope_alive(self.scope_id)
    }
}

/// `Drop` guard guaranteeing UI state is cleaned up and woken even on task panic or abort.
struct CompletionDropGuard {
    active: Arc<AtomicBool>,
    wake: SessionWakeHandle,
}

impl Drop for CompletionDropGuard {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        self.wake.wake();
    }
}

/// `Send + Sync` stream sender that pushes items to the UI thread and wakes only the owning session.
#[derive(Clone)]
pub struct StreamSink<T> {
    tx: mpsc::UnboundedSender<T>,
    wake: SessionWakeHandle,
}

impl<T> StreamSink<T> {
    /// Sends `value` to the UI thread consumer and wakes the owning session; returns `false` if closed.
    pub fn send(&self, value: T) -> bool {
        let ok = self.tx.send(value).is_ok();
        if ok {
            self.wake.wake();
        }
        ok
    }

    /// Returns `true` if the UI-side receiver has been dropped.
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

/// Spawns a `tokio` future and delivers its result `T` to `on_done` on the UI thread inside `batch`.
///
/// Automatically aborts the future if the owning `Scope` is disposed, and safely clears active
/// state if the future panics or is cancelled.
pub fn spawn_tokio_task<T, F, OnDone>(future: F, on_done: OnDone) -> AsyncTaskHandle
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
    OnDone: FnOnce(T) + 'static,
{
    spawn_tokio_task_with_status(future, move |maybe_val| {
        if let Some(val) = maybe_val {
            on_done(val);
        }
    })
}

/// Spawns a `tokio` future and calls `on_finish(Some(T))` on success or `on_finish(None)` if the
/// future panicked or was aborted while its owning `Scope` remained alive.
pub fn spawn_tokio_task_with_status<T, F, OnFinish>(
    future: F,
    on_finish: OnFinish,
) -> AsyncTaskHandle
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
    OnFinish: FnOnce(Option<T>) + 'static,
{
    let scope_id = current_scope_id();
    let wake = current_wake_handle();
    let is_active = Arc::new(AtomicBool::new(true));
    let result_slot: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));

    let active_bg = is_active.clone();
    let wake_bg = wake.clone();
    let result_bg = result_slot.clone();

    let join = tokio_runtime().spawn(async move {
        let _guard = CompletionDropGuard {
            active: active_bg,
            wake: wake_bg,
        };
        let value = future.await;
        if let Ok(mut slot) = result_bg.lock() {
            *slot = Some(value);
        }
    });

    let abort_handle = join.abort_handle();
    let abort_on_dispose = abort_handle.clone();
    let active_on_dispose = is_active.clone();
    crate::runtime::register_scope_cleanup(scope_id, move || {
        abort_on_dispose.abort();
        active_on_dispose.store(false, Ordering::Release);
    });
    register_one_shot_poller(
        scope_id,
        abort_handle.clone(),
        is_active.clone(),
        result_slot,
        on_finish,
    );
    AsyncTaskHandle {
        abort_handle,
        is_active,
        scope_id,
        _marker: PhantomData,
    }
}

type OneShotCallbackCell<T> = Rc<RefCell<Option<Box<dyn FnOnce(Option<T>)>>>>;
type StreamCallbackCell<T> = Rc<RefCell<Box<dyn FnMut(T)>>>;

fn register_one_shot_poller<T: Send + 'static>(
    scope_id: ScopeId,
    abort_handle: tokio::task::AbortHandle,
    is_active: Arc<AtomicBool>,
    result_slot: Arc<Mutex<Option<T>>>,
    on_finish: impl FnOnce(Option<T>) + 'static,
) {
    let cb_cell: OneShotCallbackCell<T> = Rc::new(RefCell::new(Some(Box::new(on_finish))));
    register_async_poller_with_retention(move || {
        if !is_scope_alive(scope_id) {
            abort_handle.abort();
            is_active.store(false, Ordering::Release);
            let _ = cb_cell.borrow_mut().take();
            return (false, false);
        }
        // Acquire `still_running` BEFORE locking `result_slot` so a worker write followed by
        // `CompletionDropGuard`'s Release store is guaranteed visible when `!still_running`.
        let still_running = is_active.load(Ordering::Acquire);
        let maybe_val = result_slot.lock().ok().and_then(|mut s| s.take());
        if let Some(val) = maybe_val {
            is_active.store(false, Ordering::Release);
            if let Some(cb) = cb_cell.borrow_mut().take() {
                batch(|| cb(Some(val)));
            }
            return (false, true);
        }
        if !still_running {
            if let Some(cb) = cb_cell.borrow_mut().take() {
                batch(|| cb(None));
            }
            return (false, true);
        }
        (true, true)
    });
}

/// Spawns an async producer driving a [`StreamSink<T>`], calling `on_item` on the UI thread per item.
pub fn spawn_tokio_stream<T, MakeFut, Fut>(
    make_future: MakeFut,
    on_item: impl FnMut(T) + 'static,
) -> AsyncTaskHandle
where
    T: Send + 'static,
    MakeFut: FnOnce(StreamSink<T>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let scope_id = current_scope_id();
    let wake = current_wake_handle();
    let is_active = Arc::new(AtomicBool::new(true));
    let (tx, rx) = mpsc::unbounded_channel();
    let sink = StreamSink {
        tx,
        wake: wake.clone(),
    };

    let active_bg = is_active.clone();
    let join = tokio_runtime().spawn(async move {
        let _guard = CompletionDropGuard {
            active: active_bg,
            wake,
        };
        make_future(sink).await;
    });

    let abort_handle = join.abort_handle();
    let abort_on_dispose = abort_handle.clone();
    let active_on_dispose = is_active.clone();
    crate::runtime::register_scope_cleanup(scope_id, move || {
        abort_on_dispose.abort();
        active_on_dispose.store(false, Ordering::Release);
    });
    register_stream_poller(
        scope_id,
        abort_handle.clone(),
        is_active.clone(),
        rx,
        on_item,
    );
    AsyncTaskHandle {
        abort_handle,
        is_active,
        scope_id,
        _marker: PhantomData,
    }
}

fn register_stream_poller<T: Send + 'static>(
    scope_id: ScopeId,
    abort_handle: tokio::task::AbortHandle,
    is_active: Arc<AtomicBool>,
    rx: mpsc::UnboundedReceiver<T>,
    on_item: impl FnMut(T) + 'static,
) {
    let rx_cell = Rc::new(RefCell::new(rx));
    let cb_cell: StreamCallbackCell<T> = Rc::new(RefCell::new(Box::new(on_item)));
    register_async_poller_with_retention(move || {
        if !is_scope_alive(scope_id) {
            abort_handle.abort();
            is_active.store(false, Ordering::Release);
            rx_cell.borrow_mut().close();
            return (false, false);
        }
        // Acquire `still_running` BEFORE draining `rx` so items sent prior to worker exit are never missed.
        let still_running = is_active.load(Ordering::Acquire);
        let mut items = Vec::new();
        while let Ok(item) = rx_cell.borrow_mut().try_recv() {
            items.push(item);
        }
        let had_items = !items.is_empty();
        if had_items {
            batch(|| {
                let mut cb = cb_cell.borrow_mut();
                for item in items {
                    if !is_scope_alive(scope_id) {
                        break;
                    }
                    cb(item);
                }
            });
            if !is_scope_alive(scope_id) {
                abort_handle.abort();
                is_active.store(false, Ordering::Release);
                rx_cell.borrow_mut().close();
                return (false, true);
            }
        }
        (still_running, still_running || had_items)
    });
}

/// Spawns a periodic `tokio::time::interval` timer that runs `on_tick` inside `batch` on the UI thread.
///
/// Automatically stops and aborts when the owning `Scope` is disposed.
pub fn spawn_tokio_interval(period: Duration, on_tick: impl FnMut() + 'static) -> AsyncTaskHandle {
    let scope_id = current_scope_id();
    let wake = current_wake_handle();
    let is_active = Arc::new(AtomicBool::new(true));
    let pending_ticks = Arc::new(AtomicU64::new(0));

    let active_bg = is_active.clone();
    let ticks_bg = pending_ticks.clone();
    let wake_bg = wake.clone();

    let join = tokio_runtime().spawn(async move {
        let _guard = CompletionDropGuard {
            active: active_bg,
            wake: wake_bg.clone(),
        };
        let safe_period = period.max(Duration::from_millis(1));
        let mut ticker = tokio::time::interval(safe_period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let _ = ticker.tick().await;
        loop {
            let _ = ticker.tick().await;
            ticks_bg.fetch_add(1, Ordering::Release);
            wake_bg.wake();
        }
    });

    let abort_handle = join.abort_handle();
    register_interval_poller(
        scope_id,
        abort_handle.clone(),
        is_active.clone(),
        pending_ticks,
        on_tick,
    );
    AsyncTaskHandle {
        abort_handle,
        is_active,
        scope_id,
        _marker: PhantomData,
    }
}

fn register_interval_poller(
    scope_id: ScopeId,
    abort_handle: tokio::task::AbortHandle,
    is_active: Arc<AtomicBool>,
    pending_ticks: Arc<AtomicU64>,
    on_tick: impl FnMut() + 'static,
) {
    let tick_cb = Rc::new(RefCell::new(on_tick));
    let abort_on_dispose = abort_handle.clone();
    let active_on_dispose = is_active.clone();
    crate::runtime::register_scope_cleanup(scope_id, move || {
        abort_on_dispose.abort();
        active_on_dispose.store(false, Ordering::Release);
    });
    register_async_poller_with_retention(move || {
        if !is_scope_alive(scope_id) {
            abort_handle.abort();
            is_active.store(false, Ordering::Release);
            return (false, false);
        }
        let still_running = is_active.load(Ordering::Acquire);
        let count = pending_ticks.swap(0, Ordering::AcqRel);
        if count > 0 {
            batch(|| {
                let mut cb = tick_cb.borrow_mut();
                for _ in 0..count {
                    if !is_scope_alive(scope_id) {
                        break;
                    }
                    cb();
                }
            });
            if !is_scope_alive(scope_id) {
                abort_handle.abort();
                is_active.store(false, Ordering::Release);
                return (false, true);
            }
        }
        (still_running, count > 0)
    });
}
