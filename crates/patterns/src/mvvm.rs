//! MVVM primitives: `Command` (sync & `new_async` with `is_running`), `InputCommand`,
//! `IntoCanExecute`, `FromAppState`, and `AppStateExt`.

#[cfg(not(target_arch = "wasm32"))]
use krui_signals::Scope;
use krui_signals::{
    batch, poll_async_tasks, register_async_poller_with_retention, Computed, ReactiveSession,
    Signal,
};
use krui_ui_types::{
    ClickTarget, InputTarget, PointerEvent, PointerTarget, WheelEvent, WheelTarget,
};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Completion callback passed to `Command::new_async(|done| { ... done(); })`.
pub type AsyncCommandDone = Box<dyn FnOnce() + Send + 'static>;

/// Compile-time sub-state extraction trait (modeled after `axum::extract::FromRef`).
///
/// Allows a ViewModel or sub-service to extract its required dependencies from a strongly-typed
/// application state struct (`AppState`) with **100% compile-time verification** and zero `dyn Any`.
pub trait FromAppState<T> {
    /// Extracts or constructs `Self` from the strongly-typed application state `app_state`.
    fn from_app_state(app_state: &T) -> Self;
}

impl<T: Clone> FromAppState<T> for T {
    fn from_app_state(app_state: &T) -> Self {
        app_state.clone()
    }
}

/// Extension helper on any strongly-typed `AppState` struct to construct ViewModels or extract
/// shared services with compile-time type safety.
pub trait AppStateExt {
    /// Constructs a ViewModel or extracts a sub-service `VM` whose dependencies are satisfied
    /// at compile time by `Self`.
    fn build<VM: FromAppState<Self>>(&self) -> VM
    where
        Self: Sized,
    {
        VM::from_app_state(self)
    }
}

impl<S> AppStateExt for S {}

/// Converts either a reactive `Computed<bool>` or a predicate closure `Fn() -> bool` into a `Computed<bool>`
/// guard for `Command::when` / `Command::with_can_execute`.
pub trait IntoCanExecute {
    /// Converts `self` into a memoized `Computed<bool>` guard.
    fn into_can_execute(self) -> Computed<bool>;
}

impl IntoCanExecute for Computed<bool> {
    fn into_can_execute(self) -> Computed<bool> {
        self
    }
}

impl<F> IntoCanExecute for F
where
    F: Fn() -> bool + 'static,
{
    fn into_can_execute(self) -> Computed<bool> {
        Computed::new_memoized(self)
    }
}

/// An MVVM command (`onclick="{vm.my_cmd}"`) that executes its handler inside a glitch-free
/// `krui_signals::batch`, tracks `is_running: Signal<bool>` for asynchronous operations, and
/// optionally tracks a reactive `can_execute: Option<Computed<bool>>` guard that disables bound
/// `<button>` widgets and applies `:disabled` styling when `false`.
#[derive(Clone)]
pub struct Command {
    action: Rc<dyn Fn()>,
    can_execute: Option<Computed<bool>>,
    /// Reactive signal that is `true` while an asynchronous `Command::new_async` task is running.
    pub is_running: Signal<bool>,
}

impl Command {
    /// Creates an always-enabled synchronous `Command`.
    pub fn new(action: impl Fn() + 'static) -> Self {
        let raw = Rc::new(action);
        let owner_session = ReactiveSession::current();
        let is_running = Signal::new(false);
        Self {
            action: Rc::new(move || {
                let f = &raw;
                owner_session.enter(|| {
                    batch(|| {
                        f();
                    });
                });
            }),
            can_execute: None,
            is_running,
        }
    }

    /// Creates an asynchronous `Command` (`Command::new_async(|done| { ... done(); })`).
    ///
    /// Guarantees `is_running` resets via a `Drop` guard on cancel or thread panic, and wakes only
    /// the originating session's `SessionWakeHandle`.
    pub fn new_async(task: impl Fn(AsyncCommandDone) + 'static) -> Self {
        let owner_session = ReactiveSession::current();
        let is_running = Signal::new(false);
        let completed_flag = Arc::new(AtomicBool::new(false));
        let in_flight = Arc::new(AtomicBool::new(false));
        let cmd_alive = Rc::new(());
        register_async_cmd_poller(&cmd_alive, &completed_flag, &in_flight, is_running);

        let task_rc = Rc::new(task);
        let flag_exec = completed_flag;
        let in_flight_exec = in_flight;
        let keep_cmd_alive = cmd_alive;
        let action: Rc<dyn Fn()> = Rc::new(move || {
            let _ = &keep_cmd_alive;
            owner_session.enter(|| {
                if is_running.get_untracked() {
                    return;
                }
                in_flight_exec.store(true, Ordering::Release);
                is_running.set_if_changed(true);
                task_rc(make_drop_guarded_done(flag_exec.clone()));
                if flag_exec.swap(false, Ordering::AcqRel) {
                    in_flight_exec.store(false, Ordering::Release);
                    is_running.set_if_changed(false);
                }
            });
        });

        Self {
            action,
            can_execute: Some(Computed::new_memoized(move || !is_running.get())),
            is_running,
        }
    }

    /// Creates a `Command` guarded by a reactive `can_execute: Computed<bool>` condition.
    pub fn new_with_can_execute(action: impl Fn() + 'static, can_execute: Computed<bool>) -> Self {
        Self::new(action).with_can_execute(can_execute)
    }

    /// Creates an async command driven directly by KRui's built-in `tokio` runtime.
    ///
    /// Automatically exposes a reactive `is_running` `Signal<bool>`, prevents concurrent
    /// re-invocation, runs `on_done` on the UI thread, and cleans up via `Drop` guards on cancel/panic.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_future<Fut, T, MakeFut, OnDone>(future_factory: MakeFut, on_done: OnDone) -> Self
    where
        T: Send + 'static,
        Fut: std::future::Future<Output = T> + Send + 'static,
        MakeFut: Fn() -> Fut + 'static,
        OnDone: Fn(T) + 'static,
    {
        struct ResetRunningOnDrop {
            flag: Signal<bool>,
            sess: ReactiveSession,
        }
        impl Drop for ResetRunningOnDrop {
            fn drop(&mut self) {
                let flag = self.flag;
                self.sess.enter(|| {
                    flag.set_if_changed(false);
                });
            }
        }

        let owner_session = ReactiveSession::current();
        let owner_scope = Scope::current();
        let is_running = Signal::new(false);
        let on_done_rc = Rc::new(on_done);
        let sess_for_action = owner_session.clone();
        let action: Rc<dyn Fn()> = Rc::new(move || {
            let sess_inner = sess_for_action.clone();
            sess_for_action.enter(|| {
                if is_running.get_untracked() || !krui_signals::is_scope_alive(owner_scope.id()) {
                    return;
                }
                is_running.set(true);
                let fut = future_factory();
                let done_cb = on_done_rc.clone();
                let reset_guard = ResetRunningOnDrop {
                    flag: is_running,
                    sess: sess_inner,
                };
                owner_scope.run(|| {
                    let _handle =
                        krui_signals::spawn_tokio_task_with_status(fut, move |maybe_val| {
                            krui_signals::batch(move || {
                                drop(reset_guard);
                                if let Some(val) = maybe_val {
                                    done_cb(val);
                                }
                            });
                        });
                });
            });
        });
        let can_execute = Computed::new_memoized(move || !is_running.get());
        Self {
            action,
            can_execute: Some(can_execute),
            is_running,
        }
    }

    /// Attaches a reactive `can_execute` guard (`Computed<bool>` or closure `Fn() -> bool`) to this `Command`.
    /// Automatically combines with `!self.is_running.get()`.
    pub fn with_can_execute(mut self, can_execute: impl IntoCanExecute) -> Self {
        let user_guard = can_execute.into_can_execute();
        let running = self.is_running;
        self.can_execute = Some(Computed::new_memoized(move || {
            !running.get() && user_guard.get()
        }));
        self
    }

    /// Ergonomic alias for `with_can_execute` accepting either a `Computed<bool>` or a closure `move || ...`.
    pub fn when(self, can_execute: impl IntoCanExecute) -> Self {
        self.with_can_execute(can_execute)
    }

    /// Returns the reactive `Signal<bool>` indicating whether an async command is currently running.
    pub fn is_running(&self) -> Signal<bool> {
        self.is_running
    }

    /// Returns the optional `Computed<bool>` guard signal attached to this command.
    pub fn can_execute_signal(&self) -> Option<Computed<bool>> {
        self.can_execute
    }

    /// Returns `true` if this command is currently enabled (subscribing the active effect/computed).
    pub fn is_enabled(&self) -> bool {
        let _ = poll_async_tasks();
        if self.is_running.get() {
            return false;
        }
        self.can_execute.map(|c| c.get()).unwrap_or(true)
    }

    /// Returns `true` if this command is currently enabled without subscribing.
    pub fn is_enabled_untracked(&self) -> bool {
        let _ = poll_async_tasks();
        if self.is_running.get_untracked() {
            return false;
        }
        self.can_execute.map(|c| c.get_untracked()).unwrap_or(true)
    }

    /// Executes the command if `is_enabled_untracked()` is `true`; otherwise does nothing.
    pub fn execute(&self) {
        if self.is_enabled_untracked() {
            (self.action)();
        }
    }

    /// Returns an `Rc<dyn Fn()>` that respects `can_execute` and `is_running`.
    pub fn as_rc_fn(&self) -> Rc<dyn Fn()> {
        let running = self.is_running;
        if let Some(can_exec) = self.can_execute {
            let act = self.action.clone();
            Rc::new(move || {
                let _ = poll_async_tasks();
                if !running.get_untracked() && can_exec.get_untracked() {
                    act();
                }
            })
        } else {
            let act = self.action.clone();
            Rc::new(move || {
                let _ = poll_async_tasks();
                if !running.get_untracked() {
                    act();
                }
            })
        }
    }
}

impl From<Command> for Rc<dyn Fn()> {
    fn from(cmd: Command) -> Self {
        cmd.as_rc_fn()
    }
}

impl ClickTarget for Command {
    fn into_click_fn(self) -> Rc<dyn Fn()> {
        self.as_rc_fn()
    }

    fn can_execute_signal(&self) -> Option<Computed<bool>> {
        self.can_execute
    }
}

impl PointerTarget for Command {
    fn into_pointer_fn(self) -> Rc<dyn Fn(PointerEvent)> {
        let cb = self.as_rc_fn();
        Rc::new(move |_ev| cb())
    }
}

impl WheelTarget for Command {
    fn into_wheel_fn(self) -> Rc<dyn Fn(WheelEvent) -> bool> {
        let cb = self.as_rc_fn();
        Rc::new(move |_ev| {
            cb();
            false
        })
    }
}

/// An MVVM text-input command (`oninput="{vm.my_input_cmd}"`) that receives the updated `String`
/// from an `<input>` element inside a glitch-free `krui_signals::batch`.
#[derive(Clone)]
pub struct InputCommand {
    action: Rc<dyn Fn(String)>,
}

impl InputCommand {
    /// Creates a new `InputCommand` wrapping `action` inside a `batch`.
    pub fn new(action: impl Fn(String) + 'static) -> Self {
        let raw = Rc::new(action);
        let owner_session = ReactiveSession::current();
        Self {
            action: Rc::new(move |val: String| {
                let f = &raw;
                owner_session.enter(|| {
                    batch(|| {
                        f(val);
                    });
                });
            }),
        }
    }

    /// Creates an `InputCommand` for `onkeydown` whose closure returns `true` to prevent default key handling.
    pub fn with_prevent_default(action: impl Fn(String) -> bool + 'static) -> Self {
        let raw = Rc::new(action);
        let owner_session = ReactiveSession::current();
        Self {
            action: Rc::new(move |val: String| {
                let f = &raw;
                let prevented = owner_session.enter(|| batch(|| f(val)));
                if prevented {
                    krui_ui_types::prevent_default_key();
                }
            }),
        }
    }

    /// Executes the input command with `value`.
    pub fn execute(&self, value: impl Into<String>) {
        (self.action)(value.into());
    }

    /// Returns the underlying `Rc<dyn Fn(String)>`.
    pub fn as_rc_fn(&self) -> Rc<dyn Fn(String)> {
        self.action.clone()
    }
}

impl From<InputCommand> for Rc<dyn Fn(String)> {
    fn from(cmd: InputCommand) -> Self {
        cmd.action
    }
}

impl InputTarget for InputCommand {
    fn into_input_fn(self) -> Rc<dyn Fn(String)> {
        self.action
    }
}

fn register_async_cmd_poller(
    cmd_alive: &Rc<()>,
    completed_flag: &Arc<AtomicBool>,
    in_flight: &Arc<AtomicBool>,
    is_running: Signal<bool>,
) {
    let flag_poll = completed_flag.clone();
    let in_flight_poll = in_flight.clone();
    let weak_alive = Rc::downgrade(cmd_alive);
    register_async_poller_with_retention(move || {
        let mut just_settled = false;
        if flag_poll.swap(false, Ordering::AcqRel) {
            in_flight_poll.store(false, Ordering::Release);
            is_running.set_if_changed(false);
            just_settled = true;
        }
        let active = just_settled || in_flight_poll.load(Ordering::Acquire);
        let keep = weak_alive.strong_count() > 0 || active;
        (keep, active)
    });
}

fn make_drop_guarded_done(flag: Arc<AtomicBool>) -> AsyncCommandDone {
    struct DoneDropGuard {
        flag: Arc<AtomicBool>,
        wake: krui_signals::SessionWakeHandle,
    }
    impl Drop for DoneDropGuard {
        fn drop(&mut self) {
            self.flag.store(true, Ordering::Release);
            self.wake.wake();
        }
    }
    let guard = DoneDropGuard {
        flag,
        wake: krui_signals::current_wake_handle(),
    };
    Box::new(move || {
        drop(guard);
    })
}

#[cfg(feature = "preview")]
impl krui_core::IntoRegistryValue for Command {
    fn into_registry_value(self) -> krui_core::Value {
        krui_core::Value::Command(krui_core::DynCommand::new(
            self.as_rc_fn(),
            self.can_execute,
        ))
    }
}

#[cfg(feature = "preview")]
impl krui_core::RegistryFieldExt for Command {
    fn registry_field_get(&self) -> Option<krui_core::Value> {
        Some(krui_core::Value::Command(krui_core::DynCommand::new(
            self.as_rc_fn(),
            self.can_execute,
        )))
    }
}

#[cfg(feature = "preview")]
impl krui_core::IntoRegistryValue for InputCommand {
    fn into_registry_value(self) -> krui_core::Value {
        krui_core::Value::InputHandler(krui_core::DynInputHandler(self.action))
    }
}

#[cfg(feature = "preview")]
impl krui_core::RegistryFieldExt for InputCommand {
    fn registry_field_get(&self) -> Option<krui_core::Value> {
        Some(krui_core::Value::InputHandler(krui_core::DynInputHandler(
            self.action.clone(),
        )))
    }
}
