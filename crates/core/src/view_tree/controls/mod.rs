//! Interactive widget controls, forms, submit/reset, `<select>` popups, HTML5 semantics, and AccessKit A11y.

pub mod a11y;
pub(crate) mod form_submit;
pub mod forms;
pub mod select_popup;
pub(crate) mod semantics;
mod widgets;

pub use widgets::*;
