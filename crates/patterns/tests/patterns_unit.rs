//! Core MVVM, MVC, Store, and tokio `Command::from_future` / `Drop`-guard unit tests for `krui-patterns`.

#![forbid(unsafe_code)]

use krui_patterns::*;
use krui_signals::{
    poll_async_tasks, reset_reactive_runtime, Computed, Effect, FrameClock, Signal,
};
use std::cell::Cell;
use std::rc::Rc;

#[allow(unused_imports)]
mod krui {
    #[cfg(feature = "preview")]
    pub use krui_core::{
        CallError, DynSignal, IntoRegistryValue, Registry, RegistryFieldExt,
        RegistryMethodDispatchFallback, Value,
    };
    pub use krui_signals::Signal;
}

#[derive(Clone, Copy)]
struct SharedCounterService {
    total_clicks: Signal<i32>,
}

#[derive(Clone, Copy)]
struct CounterAppState {
    counter_service: SharedCounterService,
}

#[derive(Clone)]
struct PanelAViewModel {
    #[allow(dead_code)]
    service: SharedCounterService,
    increment_cmd: Command,
}

impl PanelAViewModel {
    fn new(service: SharedCounterService) -> Self {
        let total = service.total_clicks;
        let can_inc = Computed::new(move || total.get() < 12);
        Self {
            service,
            increment_cmd: Command::new(move || total.update(|v| *v += 1))
                .with_can_execute(can_inc),
        }
    }
}

impl FromAppState<CounterAppState> for PanelAViewModel {
    fn from_app_state(state: &CounterAppState) -> Self {
        Self::new(state.counter_service)
    }
}

#[derive(Clone, Copy)]
struct PanelBViewModel {
    service: SharedCounterService,
}

impl PanelBViewModel {
    fn new(service: SharedCounterService) -> Self {
        Self { service }
    }
}

impl FromAppState<CounterAppState> for PanelBViewModel {
    fn from_app_state(state: &CounterAppState) -> Self {
        Self::new(state.counter_service)
    }
}

#[view_model]
#[derive(Clone, Debug, PartialEq)]
pub struct CounterState {
    pub count: i32,
    pub label: String,
}

#[view_model]
#[derive(Debug)]
pub struct DebugOnlyViewModel {
    #[state]
    pub value: i32,
}

#[test]
fn command_can_execute_blocks_execution_when_disabled() {
    reset_reactive_runtime();
    let app_state = CounterAppState {
        counter_service: SharedCounterService {
            total_clicks: Signal::new(10),
        },
    };
    let vm_a: PanelAViewModel = app_state.build();
    let vm_b: PanelBViewModel = app_state.build();

    assert!(vm_a.increment_cmd.is_enabled());
    vm_a.increment_cmd.execute();
    assert_eq!(vm_b.service.total_clicks.get(), 11);

    vm_a.increment_cmd.execute();
    assert_eq!(vm_b.service.total_clicks.get(), 12);
    assert!(!vm_a.increment_cmd.is_enabled());

    vm_a.increment_cmd.execute();
    assert_eq!(vm_b.service.total_clicks.get(), 12);
}

#[test]
fn generic_store_memoized_select_skips_unchanged_downstream_propagation() {
    reset_reactive_runtime();
    #[derive(Clone, PartialEq, Debug)]
    struct AppState {
        nodes: i32,
        load_pct: i32,
    }
    #[derive(Clone)]
    enum AppAction {
        AddLoad(i32),
        ScaleNodes(i32),
    }

    let store = Store::from_reducer(
        AppState {
            nodes: 16,
            load_pct: 40,
        },
        |state, action| match action {
            AppAction::AddLoad(delta) => AppState {
                nodes: state.nodes,
                load_pct: state.load_pct + delta,
            },
            AppAction::ScaleNodes(delta) => AppState {
                nodes: state.nodes + delta,
                load_pct: state.load_pct,
            },
        },
    );
    let nodes_sel = store.select(|s| s.nodes);
    let load_sel = store.select(|s| s.load_pct);
    let nodes_runs = Rc::new(Cell::new(0));
    let runs_c = nodes_runs.clone();
    let _eff = Effect::new(move || {
        let _ = nodes_sel.get();
        runs_c.set(runs_c.get() + 1);
    });
    assert_eq!(nodes_runs.get(), 1);

    store.on(AppAction::AddLoad(20)).execute();
    assert_eq!(load_sel.get(), 60);
    assert_eq!(nodes_runs.get(), 1);

    store.on(AppAction::ScaleNodes(4)).execute();
    assert_eq!(nodes_sel.get(), 20);
    assert_eq!(nodes_runs.get(), 2);
}

#[derive(Clone, Debug, PartialEq)]
struct MviState {
    value: i32,
    loading: bool,
    status: String,
}

#[derive(Clone, Debug)]
enum MviMsg {
    ComputeWithSignal(i32),
    StartRemoteFetch,
    RemoteFetchDone(String),
}

#[test]
fn store_reducer_reads_signals_without_borrow_panic_and_effects_task_settles_next_frame() {
    reset_reactive_runtime();
    let external_multiplier = Signal::new(3_i32);
    let store = Store::new(
        MviState {
            value: 10,
            loading: false,
            status: "idle".to_string(),
        },
        move |state, msg| match msg {
            MviMsg::ComputeWithSignal(delta) => {
                state.value += delta * external_multiplier.get();
                Effects::none()
            }
            MviMsg::StartRemoteFetch => {
                state.loading = true;
                state.status = "fetching".to_string();
                Effects::task(|done| {
                    std::thread::spawn(move || {
                        done(MviMsg::RemoteFetchDone("ok:200".to_string()));
                    });
                })
            }
            MviMsg::RemoteFetchDone(payload) => {
                state.loading = false;
                state.status = payload;
                Effects::none()
            }
        },
    );
    store.send(MviMsg::ComputeWithSignal(5));
    assert_eq!(store.get().value, 25);

    let mut clock = FrameClock::new();
    store.send(MviMsg::StartRemoteFetch);
    assert!(store.get().loading);
    for _ in 0..50 {
        clock.tick(1.0 / 120.0);
        if !store.get().loading {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(!store.get().loading);
    assert_eq!(store.get().status, "ok:200");
}

#[test]
fn command_new_async_disables_while_running_and_reenables_on_done() {
    reset_reactive_runtime();
    let counter = Signal::new(0_i32);
    let pending_done: Rc<std::cell::RefCell<Option<AsyncCommandDone>>> =
        Rc::new(std::cell::RefCell::new(None));
    let slot = pending_done.clone();

    let cmd = Command::new_async(move |done| {
        counter.update(|c| *c += 1);
        *slot.borrow_mut() = Some(done);
    });
    assert!(cmd.is_enabled());
    cmd.execute();
    assert_eq!(counter.get(), 1);
    assert!(cmd.is_running.get());
    assert!(!cmd.is_enabled());

    cmd.execute();
    assert_eq!(counter.get(), 1);

    let done_fn = pending_done.borrow_mut().take().expect("done callback");
    done_fn();
    let _ = poll_async_tasks();
    assert!(!cmd.is_running.get());
    assert!(cmd.is_enabled());
}

#[test]
fn view_model_macro_generates_reactive_companion_and_auto_derives_clone() {
    reset_reactive_runtime();
    let vm = CounterState {
        count: 10,
        label: "KRui MVVM".to_string(),
    }
    .into_view_model();
    let increment = Command::new(move || vm.count.update(|c| *c += 5));
    increment.execute();
    assert_eq!(vm.count.get(), 15);
    assert_eq!(
        vm.snapshot(),
        CounterState {
            count: 15,
            label: "KRui MVVM".to_string()
        }
    );
    let dbg_vm = DebugOnlyViewModel {
        value: Signal::new(42),
    };
    assert_eq!(dbg_vm.clone().get_value(), 42);
}

#[test]
fn async_command_and_store_drop_guards_recover_on_dropped_done_or_future() {
    reset_reactive_runtime();
    let cmd = Command::new_async(|done| {
        std::thread::spawn(move || {
            let _dropped_without_call = done;
        });
    });
    cmd.execute();
    assert!(cmd.is_running.get_untracked());
    for _ in 0..80 {
        let _ = poll_async_tasks();
        if !cmd.is_running.get_untracked() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!cmd.is_running.get_untracked());

    let fut_res = Signal::new(0_i32);
    let fut_cmd = Command::from_future(|| async { 99_i32 }, move |val| fut_res.set(val));
    fut_cmd.execute();
    for _ in 0..80 {
        let _ = poll_async_tasks();
        if fut_res.get_untracked() == 99 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(fut_res.get_untracked(), 99);
    assert!(!fut_cmd.is_running.get_untracked());
}

#[test]
fn store_effects_future_delivers_msg_to_store() {
    reset_reactive_runtime();
    #[derive(Clone, Default)]
    struct FutState {
        text: String,
    }
    #[derive(Clone)]
    enum FutMsg {
        Start,
        Done(String),
    }
    let store = Store::new(FutState::default(), |state, msg| match msg {
        FutMsg::Start => Effects::future(async { FutMsg::Done("tokio-ok".to_string()) }),
        FutMsg::Done(t) => {
            state.text = t;
            Effects::none()
        }
    });
    store.send(FutMsg::Start);
    for _ in 0..80 {
        let _ = store.poll();
        if store.get().text == "tokio-ok" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(store.get().text, "tokio-ok");
}
