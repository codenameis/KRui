//! # `krui-patterns`
//!
//! Compile-Time Type-Safe Architectural Patterns for KRui:
//! - **MVVM (`mvvm`)**: `#[view_model]`, `Command` (`new_async` & `from_future` + `can_execute`),
//!   `InputCommand`, `FromAppState`, and `AppStateExt`.
//! - **MVC (`mvc`)**: Direct controller method invocation via `ClickActionResult`.
//! - **Flux / Elm / MVI (`store`)**: Generic `Store<State, Msg>`, `Effects::future`/`stream`/`task`,
//!   and memoized `store.select(|s| ...)`.

#![forbid(unsafe_code)]

pub mod mvc;
pub mod mvvm;
pub mod store;
#[cfg(feature = "preview")]
mod store_preview;

pub use krui_macros::{controller, store_msg, view_model};
pub use mvc::ClickActionResult;
pub use mvvm::{
    AppStateExt, AsyncCommandDone, Command, FromAppState, InputCommand, IntoCanExecute,
};
pub use store::{EffectDoneCallback, Effects, Store};
#[cfg(feature = "preview")]
pub use store_preview::StoreMsgRegistry;
