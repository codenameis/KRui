//! `#[cfg(feature = "preview")]` `Registry` and `StoreMsgRegistry` integration for [`Store`].

use crate::mvvm::InputCommand;
use crate::store::Store;
use krui_signals::Computed;

/// Optional parser trait implemented by `Msg` / `Intent` enums (or via `#[store_msg]`) so
/// `Store<State, Msg>` can dispatch `store.on(Msg::Variant)` and `store.on_input(Msg::Variant)`
/// dynamically in live preview mode (`#[cfg(feature = "preview")]`).
pub trait StoreMsgRegistry: Clone + Send + 'static {
    /// Parses a unit enum variant name (e.g. `"ScaleOutNodes"` or `"TelemetryAction::ScaleOutNodes"`).
    fn parse_unit_msg(_variant: &str) -> Option<Self> {
        None
    }

    /// Parses a single-`String` tuple enum variant name (e.g. `"UpdateClusterTag"` or `"TelemetryAction::UpdateClusterTag"`).
    fn parse_string_msg(_variant: &str, _input: String) -> Option<Self> {
        None
    }
}

impl<State, Msg> krui_core::Registry for Store<State, Msg>
where
    State: krui_core::Registry + Clone + 'static,
    Msg: StoreMsgRegistry,
{
    fn get(&self, path: &str) -> Option<krui_core::Value> {
        let state_sig = self.inner.state;
        state_sig.with(|s| s.get(path))
    }

    fn signal(&self, path: &str) -> Option<krui_core::DynSignal> {
        if let Some(existing) = self.inner.preview_signals.borrow().get(path).cloned() {
            return Some(existing);
        }
        let state_sig = self.inner.state;
        let owned_path = path.to_string();
        let _ = self
            .inner
            .owner_session
            .enter(|| state_sig.with_untracked(|s| s.get(&owned_path)))?;
        let dyn_sig = self.inner.owner_session.enter(|| {
            self.inner.owner_scope.run(|| {
                let path_for_compute = owned_path.clone();
                let computed = Computed::new(move || {
                    state_sig
                        .with(|s| s.get(&path_for_compute))
                        .unwrap_or(krui_core::Value::Unit)
                });
                krui_core::DynSignal::from_computed(computed)
            })
        });
        self.inner
            .preview_signals
            .borrow_mut()
            .insert(owned_path, dyn_sig.clone());
        Some(dyn_sig)
    }

    fn call(
        &self,
        name: &str,
        args: &[krui_core::Value],
    ) -> Result<krui_core::Value, krui_core::CallError> {
        match name {
            "on" | "emit" | "send" => self.call_unit_msg(name, args),
            "on_input" | "emit_input" => self.call_input_msg(name, args),
            other => Err(krui_core::CallError::UnknownMethod(other.to_string())),
        }
    }

    fn validate_call(
        &self,
        name: &str,
        args: &[krui_core::Value],
    ) -> Result<krui_core::Value, krui_core::CallError> {
        if name == "send" {
            let _ = parse_unit_msg_arg::<Msg>(name, args)?;
            return Ok(krui_core::Value::Unit);
        }
        self.call(name, args)
    }
}

fn parse_unit_msg_arg<Msg: StoreMsgRegistry>(
    name: &str,
    args: &[krui_core::Value],
) -> Result<Msg, krui_core::CallError> {
    if args.len() != 1 {
        return Err(krui_core::CallError::ArityMismatch {
            method: name.to_string(),
            expected: 1,
            actual: args.len(),
        });
    }
    let variant_str = args[0]
        .as_str()
        .ok_or_else(|| krui_core::CallError::TypeMismatch {
            method: name.to_string(),
            arg_index: 0,
            expected: "EnumVariant",
            actual: format!("{:?}", args[0]),
        })?;
    let short = variant_str
        .rsplit_once("::")
        .map(|(_, v)| v)
        .unwrap_or(variant_str);
    Msg::parse_unit_msg(short)
        .or_else(|| Msg::parse_unit_msg(variant_str))
        .ok_or_else(|| krui_core::CallError::UnknownMethod(format!("{name}({variant_str})")))
}

impl<State, Msg> Store<State, Msg>
where
    State: krui_core::Registry + Clone + 'static,
    Msg: StoreMsgRegistry,
{
    fn call_unit_msg(
        &self,
        name: &str,
        args: &[krui_core::Value],
    ) -> Result<krui_core::Value, krui_core::CallError> {
        let msg = parse_unit_msg_arg::<Msg>(name, args)?;
        if name == "send" {
            self.send(msg);
            Ok(krui_core::Value::Unit)
        } else {
            let cmd = self.on(msg);
            Ok(krui_core::IntoRegistryValue::into_registry_value(cmd))
        }
    }

    fn call_input_msg(
        &self,
        name: &str,
        args: &[krui_core::Value],
    ) -> Result<krui_core::Value, krui_core::CallError> {
        if args.len() != 1 {
            return Err(krui_core::CallError::ArityMismatch {
                method: name.to_string(),
                expected: 1,
                actual: args.len(),
            });
        }
        let variant_str = args[0]
            .as_str()
            .ok_or_else(|| krui_core::CallError::TypeMismatch {
                method: name.to_string(),
                arg_index: 0,
                expected: "EnumVariant",
                actual: format!("{:?}", args[0]),
            })?
            .to_string();
        let short = variant_str
            .rsplit_once("::")
            .map(|(_, v)| v.to_string())
            .unwrap_or_else(|| variant_str.clone());
        if Msg::parse_string_msg(&short, String::new()).is_none()
            && Msg::parse_string_msg(&variant_str, String::new()).is_none()
        {
            return Err(krui_core::CallError::UnknownMethod(format!(
                "{name}({variant_str})"
            )));
        }
        let store = self.clone();
        let input_cmd = InputCommand::new(move |val: String| {
            if let Some(msg) = Msg::parse_string_msg(&short, val.clone())
                .or_else(|| Msg::parse_string_msg(&variant_str, val))
            {
                store.send(msg);
            }
        });
        Ok(krui_core::IntoRegistryValue::into_registry_value(input_cmd))
    }
}

impl<State, Msg> krui_core::RegistryFieldExt for Store<State, Msg>
where
    State: krui_core::Registry + Clone + 'static,
    Msg: StoreMsgRegistry,
{
    fn registry_field_get(&self) -> Option<krui_core::Value> {
        Some(krui_core::Value::Record(std::rc::Rc::new(self.clone())))
    }
}
