//! Generic Unidirectional Flux / Elm / MVI Store (`Store<State, Msg>`) and declarative `Effects<Msg>`.

use crate::mvvm::{Command, InputCommand};
use krui_signals::{
    batch, current_wake_handle, poll_async_tasks, register_async_poller_with_retention,
    spawn_tokio_stream, spawn_tokio_task_with_status, Computed, ReactiveSession, Scope,
    SessionWakeHandle, Signal, StreamSink,
};
use std::cell::Cell;
#[cfg(feature = "preview")]
use std::cell::RefCell;
#[cfg(feature = "preview")]
use std::collections::HashMap;
use std::collections::VecDeque;
use std::future::Future;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Completion callback passed to `Effects::task(|done| { ... done(msg); })`.
pub type EffectDoneCallback<Msg> = Box<dyn FnOnce(Msg) + Send + 'static>;

pub(crate) type FutureEffectSpawner<Msg> =
    Box<dyn FnOnce(Arc<Mutex<VecDeque<Msg>>>, Arc<AtomicUsize>, Rc<dyn Fn()>) + 'static>;

pub(crate) enum EffectItem<Msg> {
    Immediate(Msg),
    Task(Box<dyn FnOnce(EffectDoneCallback<Msg>) + 'static>),
    Future(FutureEffectSpawner<Msg>),
}

/// Declarative side-effect description returned by an Elm / MVI `update(&mut State, Msg) -> Effects<Msg>` function.
pub struct Effects<Msg> {
    pub(crate) items: Vec<EffectItem<Msg>>,
}

impl<Msg> Default for Effects<Msg> {
    fn default() -> Self {
        Self::none()
    }
}

impl<Msg> Effects<Msg> {
    /// Returns an empty `Effects` (no follow-up message or background task).
    pub const fn none() -> Self {
        Self { items: Vec::new() }
    }

    /// Schedules an immediate follow-up `Msg` to be processed in the current store drain cycle.
    pub fn msg(msg: Msg) -> Self {
        Self {
            items: vec![EffectItem::Immediate(msg)],
        }
    }

    /// Combines multiple `Effects<Msg>` into a single batch in order.
    pub fn batch(effects_iter: impl IntoIterator<Item = Effects<Msg>>) -> Self {
        let mut items = Vec::new();
        for eff in effects_iter {
            items.extend(eff.items);
        }
        Self { items }
    }

    /// Returns `true` if no effects are scheduled.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl<Msg: 'static> Effects<Msg> {
    /// Schedules an asynchronous or deferred task (`|done| { ... done(Msg) }`).
    ///
    /// Automatically decrements in-flight state and wakes only the owning session even if `done`
    /// is dropped on cancellation or thread panic.
    pub fn task(task_fn: impl FnOnce(EffectDoneCallback<Msg>) + 'static) -> Self {
        Self {
            items: vec![EffectItem::Task(Box::new(task_fn))],
        }
    }
}

struct DecInFlightOnDrop(Arc<AtomicUsize>);

impl Drop for DecInFlightOnDrop {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl<Msg: Send + 'static> Effects<Msg> {
    /// Schedules a `tokio` future yielding `Msg` directly back to this store on completion.
    pub fn future<F>(future: F) -> Self
    where
        F: Future<Output = Msg> + Send + 'static,
    {
        Self {
            items: vec![EffectItem::Future(Box::new(
                move |queue, in_flight, drain_now| {
                    in_flight.fetch_add(1, Ordering::AcqRel);
                    let dec_guard = DecInFlightOnDrop(in_flight);
                    let _handle = spawn_tokio_task_with_status(future, move |maybe_msg| {
                        drop(dec_guard);
                        if let Some(result_msg) = maybe_msg {
                            if let Ok(mut q) = queue.lock() {
                                q.push_back(result_msg);
                            }
                            drain_now();
                        }
                    });
                },
            ))],
        }
    }

    /// Schedules a multi-item `tokio` stream producing `Msg` values via [`StreamSink<Msg>`].
    pub fn stream<MakeFut, Fut>(make_future: MakeFut) -> Self
    where
        MakeFut: FnOnce(StreamSink<Msg>) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self {
            items: vec![EffectItem::Future(Box::new(
                move |queue, in_flight, drain_now| {
                    in_flight.fetch_add(1, Ordering::AcqRel);
                    let dec_guard = DecInFlightOnDrop(in_flight);
                    let _handle = spawn_tokio_stream(make_future, move |msg| {
                        let _keep_counter = &dec_guard;
                        if let Ok(mut q) = queue.lock() {
                            q.push_back(msg);
                        }
                        drain_now();
                    });
                },
            ))],
        }
    }
}

struct TaskInFlightDropGuard {
    in_flight: Arc<AtomicUsize>,
    wake: SessionWakeHandle,
}

impl Drop for TaskInFlightDropGuard {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
        self.wake.wake();
    }
}

struct DrainingResetGuard<'a>(&'a Cell<bool>);

impl Drop for DrainingResetGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

type StoreUpdateFn<State, Msg> = Box<dyn Fn(&mut State, Msg) -> Effects<Msg>>;

pub(crate) struct StoreInner<State: Clone + 'static, Msg: Clone + Send + 'static> {
    pub(crate) owner_session: ReactiveSession,
    pub(crate) owner_scope: Scope,
    pub(crate) state: Signal<State>,
    update: StoreUpdateFn<State, Msg>,
    pending_msgs: Arc<Mutex<VecDeque<Msg>>>,
    in_flight_tasks: Arc<AtomicUsize>,
    is_draining: Cell<bool>,
    #[cfg(feature = "preview")]
    pub(crate) preview_signals: RefCell<HashMap<String, krui_core::DynSignal>>,
}

impl<State: Clone + 'static, Msg: Clone + Send + 'static> StoreInner<State, Msg> {
    fn drain_pending_on_ui_thread(self: &Rc<Self>) -> usize {
        if self.is_draining.get() {
            return 0;
        }
        self.is_draining.set(true);
        let _reset_draining = DrainingResetGuard(&self.is_draining);

        self.owner_session.enter(|| {
            let mut processed = 0;
            loop {
                let next_msg = self
                    .pending_msgs
                    .lock()
                    .ok()
                    .and_then(|mut q| q.pop_front());
                let Some(msg) = next_msg else {
                    break;
                };

                let mut next_state = self.state.with_untracked(|s| s.clone());
                let effects = (self.update)(&mut next_state, msg);
                let state_sig = self.state;
                batch(move || {
                    state_sig.set(next_state);
                });
                processed += 1;
                self.spawn_scheduled_effects(effects);
            }
            processed
        })
    }

    fn spawn_scheduled_effects(self: &Rc<Self>, effects: Effects<Msg>) {
        for item in effects.items {
            match item {
                EffectItem::Immediate(follow_up) => {
                    if let Ok(mut q) = self.pending_msgs.lock() {
                        q.push_back(follow_up);
                    }
                }
                EffectItem::Task(task_fn) => {
                    self.in_flight_tasks.fetch_add(1, Ordering::AcqRel);
                    let queue_clone = self.pending_msgs.clone();
                    let guard = TaskInFlightDropGuard {
                        in_flight: self.in_flight_tasks.clone(),
                        wake: current_wake_handle(),
                    };
                    task_fn(Box::new(move |result_msg| {
                        let _guard = guard;
                        if let Ok(mut q) = queue_clone.lock() {
                            q.push_back(result_msg);
                        }
                    }));
                }
                EffectItem::Future(runner) => {
                    if !krui_signals::is_scope_alive(self.owner_scope.id()) {
                        continue;
                    }
                    let weak_self = Rc::downgrade(self);
                    let drain_now: Rc<dyn Fn()> = Rc::new(move || {
                        if let Some(strong) = weak_self.upgrade() {
                            let _ = strong.drain_pending_on_ui_thread();
                        }
                    });
                    let queue = self.pending_msgs.clone();
                    let in_flight = self.in_flight_tasks.clone();
                    self.owner_scope.run(|| {
                        runner(queue, in_flight, drain_now);
                    });
                }
            }
        }
    }
}

/// Generic unidirectional `Store<State, Msg>` for Flux / Elm / MVI architectures.
///
/// Supports both:
/// - `Store::new(initial_state, update: fn(&mut State, Msg) -> Effects<Msg>)` (Elm / MVI with side-effects)
/// - `Store::from_reducer(initial_state, reducer: fn(&State, Msg) -> State)` (pure reducer without side-effects)
pub struct Store<State: Clone + 'static, Msg: Clone + Send + 'static> {
    pub(crate) inner: Rc<StoreInner<State, Msg>>,
}

impl<State: Clone + 'static, Msg: Clone + Send + 'static> Clone for Store<State, Msg> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<State: Clone + 'static, Msg: Clone + Send + 'static> Store<State, Msg> {
    /// Creates a new `Store<State, Msg>` from `initial_state` and an Elm / MVI `update` function
    /// `fn(&mut State, Msg) -> Effects<Msg>`.
    pub fn new(
        initial_state: State,
        update: impl Fn(&mut State, Msg) -> Effects<Msg> + 'static,
    ) -> Self {
        let inner = Rc::new(StoreInner {
            owner_session: ReactiveSession::current(),
            owner_scope: Scope::current(),
            state: Signal::new(initial_state),
            update: Box::new(update),
            pending_msgs: Arc::new(Mutex::new(VecDeque::new())),
            in_flight_tasks: Arc::new(AtomicUsize::new(0)),
            is_draining: Cell::new(false),
            #[cfg(feature = "preview")]
            preview_signals: RefCell::new(HashMap::new()),
        });

        let weak = Rc::downgrade(&inner);
        register_async_poller_with_retention(move || {
            let Some(strong) = weak.upgrade() else {
                return (false, false);
            };
            let drained = strong.drain_pending_on_ui_thread();
            let has_in_flight = strong.in_flight_tasks.load(Ordering::Acquire) > 0;
            let has_queued = !strong
                .pending_msgs
                .lock()
                .map(|q| q.is_empty())
                .unwrap_or(true);
            (true, drained > 0 || has_in_flight || has_queued)
        });

        Self { inner }
    }

    /// Creates a `Store<State, Msg>` from a pure reducer `fn(&State, Msg) -> State` that produces no side-effects.
    pub fn from_reducer(
        initial_state: State,
        reducer: impl Fn(&State, Msg) -> State + 'static,
    ) -> Self {
        Self::new(initial_state, move |state: &mut State, msg: Msg| {
            *state = reducer(state, msg);
            Effects::none()
        })
    }

    /// Returns the underlying `Signal<State>` snapshot handle.
    pub fn state(&self) -> Signal<State> {
        self.inner.state
    }

    /// Clones and returns the current `State` snapshot, subscribing the caller.
    pub fn get(&self) -> State {
        self.inner.state.get()
    }

    /// Borrows the current `State` snapshot without cloning, subscribing the caller.
    pub fn with<R>(&self, f: impl FnOnce(&State) -> R) -> R {
        self.inner.state.with(f)
    }

    /// Sends a message `Msg` to the store, running `update` with the state borrow released
    /// (`with_untracked`) and executing any returned `Effects<Msg>`.
    pub fn send(&self, msg: Msg) {
        if let Ok(mut q) = self.inner.pending_msgs.lock() {
            q.push_back(msg);
        }
        let _ = self.inner.drain_pending_on_ui_thread();
    }

    /// Polls and drains any completed `Effects::task` messages on the UI thread.
    /// Automatically called by `FrameClock::tick` on every frame.
    pub fn poll(&self) -> usize {
        let _ = self.inner.owner_session.enter(poll_async_tasks);
        self.inner.drain_pending_on_ui_thread()
    }

    /// Creates a **Memoized Selector** (`Computed<T>`) projecting `T` from `&State`.
    ///
    /// Whenever `State` transitions, if the newly projected `T` is equal (`PartialEq::eq`)
    /// to the previous `T`, downstream `Computed` / `Effect` / UI text subscribers are **skipped**.
    pub fn select<T, F>(&self, selector: F) -> Computed<T>
    where
        T: Clone + PartialEq + 'static,
        F: Fn(&State) -> T + 'static,
    {
        let state_sig = self.inner.state;
        Computed::new_memoized(move || state_sig.with(&selector))
    }

    /// Creates a `Command` (`onclick="{store.on(Msg::...)}"`) that sends `msg.clone()` when executed.
    pub fn on(&self, msg: Msg) -> Command {
        let store = self.clone();
        Command::new(move || {
            store.send(msg.clone());
        })
    }

    #[doc(hidden)]
    #[inline]
    pub fn __krui_template_callable(&self) -> &Self {
        self
    }

    /// Creates a guarded `Command` that sends `msg.clone()` only when `can_execute` is `true`.
    pub fn on_when(&self, msg: Msg, can_execute: Computed<bool>) -> Command {
        self.on(msg).with_can_execute(can_execute)
    }

    /// Creates an `InputCommand` (`oninput="{store.on_input(Msg::...)}"`) that maps the `<input>`
    /// string into a `Msg` and sends it to this store.
    pub fn on_input(&self, mapper: impl Fn(String) -> Msg + 'static) -> InputCommand {
        let store = self.clone();
        InputCommand::new(move |val: String| {
            store.send(mapper(val));
        })
    }

    /// Deprecated alias for [`Store::send`].
    #[deprecated(note = "Use `Store::send` instead")]
    pub fn dispatch(&self, msg: Msg) {
        self.send(msg);
    }

    /// Deprecated alias for [`Store::on`].
    #[deprecated(note = "Use `Store::on` instead")]
    pub fn emit(&self, msg: Msg) -> Command {
        self.on(msg)
    }

    /// Deprecated alias for [`Store::on_when`].
    #[deprecated(note = "Use `Store::on_when` instead")]
    pub fn emit_when(&self, msg: Msg, can_execute: Computed<bool>) -> Command {
        self.on_when(msg, can_execute)
    }

    /// Deprecated alias for [`Store::on_input`].
    #[deprecated(note = "Use `Store::on_input` instead")]
    pub fn emit_input(&self, mapper: impl Fn(String) -> Msg + 'static) -> InputCommand {
        self.on_input(mapper)
    }
}
