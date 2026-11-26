//! Element event, form state, and canvas slot binding helpers for `krui-preview`.

use crate::eval::{eval_template_expr, eval_truthy, resolve_signal_from_expr};
use krui_compiler::{CompiledBoolAttr, CompiledNode, TemplateExpr};
use krui_core::{NodeId, RegistryRef, Value, ViewTree};
use std::rc::Rc;

fn bind_click_slot(
    tree: &mut ViewTree,
    node_id: NodeId,
    click_expr: &TemplateExpr,
    registry: &RegistryRef,
) {
    if matches!(click_expr, TemplateExpr::Path(_)) {
        if let Value::Command(cmd) = eval_template_expr(click_expr, registry.as_ref()) {
            tree.bind_click(node_id, cmd);
            return;
        }
    }
    let expr = click_expr.clone();
    let reg = registry.clone();
    tree.set_on_click(node_id, move || {
        if let Value::Command(cmd) = eval_template_expr(&expr, reg.as_ref()) {
            cmd.execute();
        }
    });
}

fn bind_action_slot(
    tree: &mut ViewTree,
    node_id: NodeId,
    action_expr: &TemplateExpr,
    registry: &RegistryRef,
    attach: fn(&mut ViewTree, NodeId, krui_core::DynCommand),
) {
    if matches!(action_expr, TemplateExpr::Path(_)) {
        if let Value::Command(cmd) = eval_template_expr(action_expr, registry.as_ref()) {
            attach(tree, node_id, cmd);
            return;
        }
    }
    let expr = action_expr.clone();
    let reg = registry.clone();
    let cb: Rc<dyn Fn()> = Rc::new(move || {
        if let Value::Command(cmd) = eval_template_expr(&expr, reg.as_ref()) {
            cmd.execute();
        }
    });
    attach(tree, node_id, krui_core::DynCommand::new(cb, None));
}

fn bind_pointer_slot(
    tree: &mut ViewTree,
    node_id: NodeId,
    expr: &TemplateExpr,
    registry: &RegistryRef,
    attach: fn(&mut ViewTree, NodeId, krui_core::PointerHandler),
) {
    if matches!(expr, TemplateExpr::Path(_)) {
        match eval_template_expr(expr, registry.as_ref()) {
            Value::PointerHandler(h) => {
                attach(tree, node_id, h);
                return;
            }
            Value::Command(cmd) => {
                attach(
                    tree,
                    node_id,
                    krui_core::PointerHandler::new(move |_| cmd.execute()),
                );
                return;
            }
            _ => {}
        }
    }
    let expr = expr.clone();
    let reg = registry.clone();
    attach(
        tree,
        node_id,
        krui_core::PointerHandler::new(move |_| {
            if let Value::Command(cmd) = eval_template_expr(&expr, reg.as_ref()) {
                cmd.execute();
            }
        }),
    );
}

fn bind_wheel_slot(
    tree: &mut ViewTree,
    node_id: NodeId,
    expr: &TemplateExpr,
    registry: &RegistryRef,
) {
    if matches!(expr, TemplateExpr::Path(_)) {
        match eval_template_expr(expr, registry.as_ref()) {
            Value::WheelHandler(w) => {
                tree.set_on_wheel(node_id, w);
                return;
            }
            Value::Command(cmd) => {
                tree.set_on_wheel(node_id, cmd);
                return;
            }
            _ => {}
        }
    }
    let expr = expr.clone();
    let reg = registry.clone();
    tree.set_on_wheel(
        node_id,
        krui_core::WheelHandler::new(move |_| {
            if let Value::Command(cmd) = eval_template_expr(&expr, reg.as_ref()) {
                cmd.execute();
            }
        }),
    );
}

pub(crate) fn bind_event_slots(
    tree: &mut ViewTree,
    node_id: NodeId,
    node: &CompiledNode,
    registry: &RegistryRef,
) {
    bind_bool_state_slots(tree, node_id, node, registry);
    bind_focus_and_pointer_slots(tree, node_id, node, registry);

    if let Some(bind_val_expr) = &node.bind_value {
        bind_two_way_value_slot(
            tree,
            node_id,
            bind_val_expr,
            node.text_parts.is_empty(),
            registry,
        );
    } else if let Some(input_expr) = &node.on_input {
        bind_string_handler(tree, node_id, input_expr, registry, ViewTree::set_on_input);
    }

    if let Some(change_expr) = &node.on_change {
        bind_string_handler(
            tree,
            node_id,
            change_expr,
            registry,
            ViewTree::set_on_change,
        );
    }

    if let Some(keydown_expr) = &node.on_keydown {
        bind_string_handler(
            tree,
            node_id,
            keydown_expr,
            registry,
            ViewTree::set_on_keydown,
        );
    }

    if let Some(canvas_expr) = &node.canvas_draw {
        if let Value::CanvasDraw(draw) = eval_template_expr(canvas_expr, registry.as_ref()) {
            tree.set_canvas_draw(node_id, draw);
        }
    }
}

fn bind_bool_state_slots(
    tree: &mut ViewTree,
    node_id: NodeId,
    node: &CompiledNode,
    registry: &RegistryRef,
) {
    match &node.disabled {
        None => {}
        Some(CompiledBoolAttr::Static(val)) => tree.set_disabled(node_id, *val),
        Some(CompiledBoolAttr::Dynamic { expr, .. }) => {
            let expr = expr.clone();
            let reg = registry.clone();
            tree.bind_disabled(node_id, move || {
                eval_truthy(&eval_template_expr(&expr, reg.as_ref()))
            });
        }
    }
    match &node.required {
        None => {}
        Some(CompiledBoolAttr::Static(val)) => tree.set_required(node_id, *val),
        Some(CompiledBoolAttr::Dynamic { expr, .. }) => {
            let expr = expr.clone();
            let reg = registry.clone();
            tree.bind_required(node_id, move || {
                eval_truthy(&eval_template_expr(&expr, reg.as_ref()))
            });
        }
    }
    match &node.checked {
        None => {}
        Some(CompiledBoolAttr::Static(val)) => tree.set_checked(node_id, *val),
        Some(CompiledBoolAttr::Dynamic { expr, .. }) => {
            let expr = expr.clone();
            let reg = registry.clone();
            tree.bind_checked(node_id, move || {
                eval_truthy(&eval_template_expr(&expr, reg.as_ref()))
            });
        }
    }
    if let Some(bind_chk_expr) = &node.bind_checked {
        if let Some(dyn_sig) = resolve_signal_from_expr(bind_chk_expr, registry.as_ref()) {
            if node.checked.is_none() {
                let sig_read = dyn_sig.clone();
                tree.bind_checked(node_id, move || eval_truthy(&sig_read.get()));
            }
            tree.set_on_checked_change(node_id, move |checked: bool| {
                let _ = dyn_sig.set(Value::Bool(checked));
            });
        }
    }
}

fn bind_focus_and_pointer_slots(
    tree: &mut ViewTree,
    node_id: NodeId,
    node: &CompiledNode,
    registry: &RegistryRef,
) {
    if let Some(click_expr) = &node.on_click {
        bind_click_slot(tree, node_id, click_expr, registry);
    }
    if let Some(expr) = &node.on_focus {
        bind_action_slot(tree, node_id, expr, registry, ViewTree::set_on_focus);
    }
    if let Some(expr) = &node.on_blur {
        bind_action_slot(tree, node_id, expr, registry, ViewTree::set_on_blur);
    }
    if let Some(expr) = &node.on_pointer_down {
        bind_pointer_slot(tree, node_id, expr, registry, ViewTree::set_on_pointer_down);
    }
    if let Some(expr) = &node.on_pointer_up {
        bind_pointer_slot(tree, node_id, expr, registry, ViewTree::set_on_pointer_up);
    }
    if let Some(expr) = &node.on_pointer_move {
        bind_pointer_slot(tree, node_id, expr, registry, ViewTree::set_on_pointer_move);
    }
    if let Some(expr) = &node.on_context_menu {
        bind_pointer_slot(tree, node_id, expr, registry, ViewTree::set_on_context_menu);
    }
    if let Some(expr) = &node.on_wheel {
        bind_wheel_slot(tree, node_id, expr, registry);
    }
    if let Some(expr) = &node.on_scroll {
        bind_action_slot(tree, node_id, expr, registry, ViewTree::set_on_scroll);
    }
    if let Some(expr) = &node.on_submit {
        bind_action_slot(tree, node_id, expr, registry, ViewTree::set_on_submit);
    }
    if let Some(expr) = &node.on_reset {
        bind_action_slot(tree, node_id, expr, registry, ViewTree::set_on_reset);
    }
}

fn bind_two_way_value_slot(
    tree: &mut ViewTree,
    node_id: NodeId,
    bind_val_expr: &TemplateExpr,
    bind_read_text: bool,
    registry: &RegistryRef,
) {
    let Some(dyn_sig) = resolve_signal_from_expr(bind_val_expr, registry.as_ref()) else {
        return;
    };
    if bind_read_text {
        let sig_read = dyn_sig.clone();
        tree.bind_dynamic_text(
            node_id,
            Box::new(move || {
                let raw = sig_read.get().to_string();
                krui_style::sanitize_untrusted_text(&raw)
            }),
        );
    }
    tree.set_on_input(node_id, move |val: String| {
        let _ = dyn_sig.set(Value::Str(val));
    });
}

fn bind_string_handler(
    tree: &mut ViewTree,
    node_id: NodeId,
    expr: &TemplateExpr,
    registry: &RegistryRef,
    attach: fn(&mut ViewTree, NodeId, krui_core::DynInputHandler),
) {
    if let Value::InputHandler(handler) = eval_template_expr(expr, registry.as_ref()) {
        attach(tree, node_id, handler);
    } else if let Some(dyn_sig) = resolve_signal_from_expr(expr, registry.as_ref()) {
        attach(
            tree,
            node_id,
            krui_core::DynInputHandler(Rc::new(move |val: String| {
                let _ = dyn_sig.set(Value::Str(val));
            })),
        );
    }
}
