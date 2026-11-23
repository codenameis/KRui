//! MVC action dispatch helpers (`ClickActionResult`).

use crate::mvvm::Command;
use std::rc::Rc;

/// Trait that unifies inline click expressions in `.krui` templates:
/// - Controller method calls returning `()`: `onclick="{controller.buy_equity(5)}"`
/// - Store action emitters returning `Command`: `onclick="{store.emit(Action::ScaleOut)}"`
/// - Closure expressions returning `Rc<dyn Fn()>`
pub trait ClickActionResult {
    /// Invokes the result of an inline click expression.
    fn invoke_click_result(self);
}

impl ClickActionResult for () {
    fn invoke_click_result(self) {}
}

impl ClickActionResult for Command {
    fn invoke_click_result(self) {
        self.execute();
    }
}

impl ClickActionResult for Rc<dyn Fn()> {
    fn invoke_click_result(self) {
        self();
    }
}
