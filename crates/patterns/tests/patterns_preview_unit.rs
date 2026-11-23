//! Preview-feature registry tests for `#[view_model]`, `#[controller]`, and `#[store_msg]`.
#![cfg(feature = "preview")]
#![forbid(unsafe_code)]

use krui_core::{CallError, Registry, ScopeRegistry, Value};
use krui_patterns::{controller, store_msg, view_model, Command, Store};
use krui_signals::{reset_reactive_runtime, Computed, Signal};

#[allow(unused_imports)]
mod krui {
    pub use krui_core::{
        CallError, DynCommand, DynInputHandler, DynSignal, IntoRegistryValue, Registry,
        RegistryFieldExt, RegistryMethodDispatchFallback, RegistryRef, ScopeRegistry,
        TryFromRegistryValue, Value,
    };
    pub use krui_patterns::StoreMsgRegistry;
    pub use krui_signals::{Computed, Signal};
}

#[view_model]
pub struct CartVm {
    #[state]
    pub qty: i32,
    #[state]
    pub item_name: String,
    #[computed]
    pub total_price: i64,
    #[command]
    pub inc_cmd: Command,
}

#[controller]
#[derive(Clone, Copy)]
pub struct DeskModel {
    pub shares: Signal<i32>,
}

#[controller]
#[derive(Clone, Copy)]
pub struct DeskController {
    pub model: DeskModel,
}

#[controller]
impl DeskController {
    #[allow(dead_code)]
    pub fn buy(&self, amount: i32) {
        self.model.shares.update(|s| *s += amount);
    }
}

#[controller]
#[derive(Clone, PartialEq, Debug)]
pub struct HudState {
    pub nodes: i32,
    pub tag: String,
}

#[store_msg]
#[derive(Clone, Debug)]
pub enum HudMsg {
    ScaleOut,
    Rename(String),
}

fn verify_mvvm_and_mvc_registries() -> (CartVm, DeskController) {
    let qty = Signal::new(2_i32);
    let item_name = Signal::new("KRui Pro".to_string());
    let total_price = Computed::new(move || qty.get() as i64 * 10_000);
    let inc_cmd = Command::new(move || qty.update(|q| *q += 1));
    let vm = CartVm {
        qty,
        item_name,
        total_price,
        inc_cmd,
    };
    assert_eq!(vm.get("qty"), Some(Value::Int(2)));
    assert_eq!(vm.get("total_price"), Some(Value::Int(20_000)));
    assert_eq!(vm.call("inc_cmd", &[]), Ok(Value::Unit));
    assert_eq!(vm.get("qty"), Some(Value::Int(3)));

    let name_sig = vm.signal("item_name").expect("writable item_name signal");
    assert!(name_sig.is_writable());
    name_sig
        .set(Value::Str("KRui Enterprise".to_string()))
        .expect("set item_name");
    assert_eq!(vm.get_item_name(), "KRui Enterprise");

    let ctrl = DeskController {
        model: DeskModel {
            shares: Signal::new(10),
        },
    };
    assert_eq!(ctrl.get("model.shares"), Some(Value::Int(10)));
    assert_eq!(ctrl.call("buy", &[Value::Int(5)]), Ok(Value::Unit));
    assert_eq!(ctrl.get("model.shares"), Some(Value::Int(15)));
    assert_eq!(
        ctrl.call("buy", &[]),
        Err(CallError::ArityMismatch {
            method: "buy".to_string(),
            expected: 1,
            actual: 0,
        })
    );
    (vm, ctrl)
}

#[test]
fn b2_registry_reads_signals_and_invokes_commands_controller_methods_and_store_actions() {
    reset_reactive_runtime();
    let (vm, ctrl) = verify_mvvm_and_mvc_registries();
    let store = Store::from_reducer(
        HudState {
            nodes: 4,
            tag: "seoul".to_string(),
        },
        |s, m| match m {
            HudMsg::ScaleOut => HudState {
                nodes: s.nodes + 4,
                tag: s.tag.clone(),
            },
            HudMsg::Rename(new_tag) => HudState {
                nodes: s.nodes,
                tag: new_tag,
            },
        },
    );

    let mut scope = ScopeRegistry::new();
    scope.insert_registry("vm", vm);
    scope.insert_registry("ctrl", ctrl);
    scope.insert_registry("store", store);

    assert_eq!(scope.get("store.nodes"), Some(Value::Int(4)));
    let cmd_val = scope
        .call("store.on", &[Value::Str("HudMsg::ScaleOut".to_string())])
        .expect("store.on");
    if let Value::Command(dyn_cmd) = cmd_val {
        dyn_cmd.execute();
    } else {
        panic!("expected Value::Command");
    }
    assert_eq!(scope.get("store.nodes"), Some(Value::Int(8)));

    let input_val = scope
        .call(
            "store.on_input",
            &[Value::Str("HudMsg::Rename".to_string())],
        )
        .expect("store.on_input");
    if let Value::InputHandler(handler) = input_val {
        (handler.0)("busan-cluster".to_string());
    } else {
        panic!("expected Value::InputHandler");
    }
    assert_eq!(
        scope.get("store.tag"),
        Some(Value::Str("busan-cluster".to_string()))
    );
}
