//! Whole-document preflight validation for `krui-preview` before tearing down an active `ViewTree`.

use crate::eval::{resolve_signal_from_expr, try_eval_bool, validate_template_expr, EvalError};
use krui_compiler::{
    CompileError, CompiledBoolAttr, CompiledDocument, CompiledDynamicSelectOption, CompiledFor,
    CompiledIf, CompiledNode, CompiledNodeKind, CompiledSelectChild, CompiledTextPart, PaintSlot,
    TemplateExpr,
};
use krui_core::{CallError, DynSignal, Registry, Value};

/// Validates all nodes, branches (including inactive `<else-if>`/`<else>` and `<for>`/`<empty>` branches),
/// and binding slots (`text`, `placeholder`, `aria-label`, `class`, `paint`, `layout`, `disabled`,
/// `onclick`, `bind:value`, `oninput`, `onchange`, `onkeydown`, `ondraw`) in `doc` against `registry`
/// before mutating or tearing down an existing `ViewTree`.
pub fn validate_compiled_document_against_registry(
    doc: &CompiledDocument,
    registry: &dyn Registry,
) -> Result<(), CompileError> {
    for root in &doc.roots {
        validate_node(root, registry, &[], doc.source_path.as_deref())?;
    }
    Ok(())
}

fn expr_uses_untyped_var(expr: &TemplateExpr, untyped_vars: &[&str]) -> bool {
    if untyped_vars.is_empty() {
        return false;
    }
    match expr {
        TemplateExpr::Path(segs) => segs
            .first()
            .is_some_and(|h| untyped_vars.contains(&h.as_str())),
        TemplateExpr::EnumVariant(_) | TemplateExpr::Literal(_) => false,
        TemplateExpr::Unary { expr: inner, .. } => expr_uses_untyped_var(inner, untyped_vars),
        TemplateExpr::Binary { left, right, .. } => {
            expr_uses_untyped_var(left, untyped_vars) || expr_uses_untyped_var(right, untyped_vars)
        }
        TemplateExpr::Call { receiver, args, .. } => {
            receiver
                .first()
                .is_some_and(|h| untyped_vars.contains(&h.as_str()))
                || args.iter().any(|a| expr_uses_untyped_var(a, untyped_vars))
        }
    }
}

fn validate_outer_subexprs_when_untyped(
    expr: &TemplateExpr,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    if !expr_uses_untyped_var(expr, untyped_vars) {
        let _ =
            validate_template_expr(expr, registry).map_err(|e| to_compile_err(e, source_path))?;
        return Ok(());
    }
    match expr {
        TemplateExpr::Unary { expr: inner, .. } => {
            validate_outer_subexprs_when_untyped(inner, registry, untyped_vars, source_path)
        }
        TemplateExpr::Binary { left, right, .. } => {
            validate_outer_subexprs_when_untyped(left, registry, untyped_vars, source_path)?;
            validate_outer_subexprs_when_untyped(right, registry, untyped_vars, source_path)
        }
        TemplateExpr::Call { args, .. } => {
            for a in args {
                validate_outer_subexprs_when_untyped(a, registry, untyped_vars, source_path)?;
            }
            Ok(())
        }
        TemplateExpr::Path(_) | TemplateExpr::EnumVariant(_) | TemplateExpr::Literal(_) => Ok(()),
    }
}

fn validate_any_expr(
    expr: &TemplateExpr,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<Option<Value>, CompileError> {
    if expr_uses_untyped_var(expr, untyped_vars) {
        validate_outer_subexprs_when_untyped(expr, registry, untyped_vars, source_path)?;
        Ok(None)
    } else {
        validate_template_expr(expr, registry)
            .map(Some)
            .map_err(|e| to_compile_err(e, source_path))
    }
}

fn to_compile_err(err: EvalError, source_path: Option<&std::path::Path>) -> CompileError {
    let mut ce = CompileError::new(1, 1, err.to_string());
    if let Some(p) = source_path {
        ce = ce.with_file(p.to_path_buf());
    }
    ce
}

fn validate_node(
    node: &CompiledNode,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    match &node.kind {
        CompiledNodeKind::If(cif) => validate_if_node(cif, registry, untyped_vars, source_path),
        CompiledNodeKind::For(cfor) => validate_for_node(cfor, registry, untyped_vars, source_path),
        CompiledNodeKind::Element => {
            validate_element_node(node, registry, untyped_vars, source_path)
        }
    }
}

fn validate_if_node(
    cif: &CompiledIf,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for br in &cif.branches {
        if let Some(cond_expr) = &br.condition {
            if let Some(val) = validate_any_expr(cond_expr, registry, untyped_vars, source_path)? {
                try_eval_bool(&val).map_err(|e| to_compile_err(e, source_path))?;
            }
        }
        for child in &br.children {
            validate_node(child, registry, untyped_vars, source_path)?;
        }
    }
    Ok(())
}

struct PreflightForItemReg<'a> {
    parent: &'a dyn Registry,
    item_var: &'a str,
    sample_item: Option<&'a Value>,
}

impl Registry for PreflightForItemReg<'_> {
    fn get(&self, path: &str) -> Option<Value> {
        let trimmed = path.trim();
        if trimmed == self.item_var {
            return Some(
                self.sample_item
                    .cloned()
                    .unwrap_or_else(|| Value::Str(String::new())),
            );
        }
        if let Some(tail) = trimmed
            .strip_prefix(self.item_var)
            .and_then(|r| r.strip_prefix('.'))
        {
            if let Some(item) = self.sample_item {
                if let Value::Record(rec) = item {
                    return rec.get(tail);
                }
                return None;
            }
            return Some(Value::Str(String::new()));
        }
        self.parent.get(path)
    }

    fn signal(&self, path: &str) -> Option<DynSignal> {
        let trimmed = path.trim();
        if trimmed == self.item_var
            || trimmed
                .strip_prefix(self.item_var)
                .and_then(|r| r.strip_prefix('.'))
                .is_some()
        {
            if let Some(Value::Record(rec)) = self.sample_item {
                if let Some(tail) = trimmed
                    .strip_prefix(self.item_var)
                    .and_then(|r| r.strip_prefix('.'))
                {
                    return rec.signal(tail);
                }
            }
            return None;
        }
        self.parent.signal(path)
    }

    fn call(&self, name: &str, args: &[Value]) -> Result<Value, CallError> {
        self.parent.call(name, args)
    }

    fn validate_call(&self, name: &str, args: &[Value]) -> Result<Value, CallError> {
        if let Some(tail) = name
            .trim()
            .strip_prefix(self.item_var)
            .and_then(|r| r.strip_prefix('.'))
        {
            if let Some(Value::Record(rec)) = self.sample_item {
                return rec.validate_call(tail, args);
            }
            return Err(CallError::UnknownMethod(name.to_string()));
        }
        self.parent.validate_call(name, args)
    }
}

fn validate_for_node(
    cfor: &CompiledFor,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    let maybe_list = validate_any_expr(&cfor.list, registry, untyped_vars, source_path)?;
    let sample_item = match &maybe_list {
        Some(Value::List(items)) => items.first(),
        Some(other) => {
            return Err(to_compile_err(
                EvalError::TypeError(format!("`<for each>` requires List value, got {other:?}")),
                source_path,
            ));
        }
        None => None,
    };

    for empty_child in &cfor.empty_body {
        validate_node(empty_child, registry, untyped_vars, source_path)?;
    }

    let item_reg = PreflightForItemReg {
        parent: registry,
        item_var: &cfor.item_var,
        sample_item,
    };
    let mut inner_untyped: Vec<&str> = untyped_vars.to_vec();
    if sample_item.is_none() {
        inner_untyped.push(cfor.item_var.as_str());
    }
    let _ = validate_any_expr(&cfor.key, &item_reg, &inner_untyped, source_path)?;
    for body_child in &cfor.body {
        validate_node(body_child, &item_reg, &inner_untyped, source_path)?;
    }
    Ok(())
}

fn validate_element_node(
    node: &CompiledNode,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for part in node
        .text_parts
        .iter()
        .chain(node.placeholder_parts.iter())
        .chain(node.aria_label_parts.iter())
        .chain(node.title_parts.iter())
        .chain(node.img_src.iter())
        .chain(node.img_alt.iter())
        .chain(node.link_href.iter())
        .chain(node.class_parts.iter())
        .chain(node.dynamic_inline_style_parts.iter())
        .chain(node.dynamic_attrs.iter().flat_map(|a| a.parts.iter()))
        .chain(node.rich_spans.iter().flat_map(|s| s.parts.iter()))
    {
        if let CompiledTextPart::Dynamic { expr, .. } = part {
            let _ = validate_any_expr(expr, registry, untyped_vars, source_path)?;
        }
    }

    validate_select_children(&node.select_children, registry, untyped_vars, source_path)?;
    validate_paint_and_layout_slots(node, registry, untyped_vars, source_path)?;
    validate_event_and_binding_slots(node, registry, untyped_vars, source_path)?;

    for child in &node.children {
        validate_node(child, registry, untyped_vars, source_path)?;
    }
    Ok(())
}

fn validate_select_children(
    children: &[CompiledSelectChild],
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for child in children {
        match child {
            CompiledSelectChild::Option(opt) => {
                validate_select_option_parts(opt, registry, untyped_vars, source_path)?;
            }
            CompiledSelectChild::For(sfor) => {
                let maybe_list =
                    validate_any_expr(&sfor.list, registry, untyped_vars, source_path)?;
                let sample_item = match &maybe_list {
                    Some(Value::List(items)) => items.first(),
                    Some(other) => {
                        return Err(to_compile_err(
                            EvalError::TypeError(format!(
                                "`<for each>` requires List value, got {other:?}"
                            )),
                            source_path,
                        ));
                    }
                    None => None,
                };
                let item_reg = PreflightForItemReg {
                    parent: registry,
                    item_var: &sfor.item_var,
                    sample_item,
                };
                let mut inner_untyped: Vec<&str> = untyped_vars.to_vec();
                if sample_item.is_none() {
                    inner_untyped.push(sfor.item_var.as_str());
                }
                let _ = validate_any_expr(&sfor.key, &item_reg, &inner_untyped, source_path)?;
                for opt in &sfor.options {
                    validate_select_option_parts(opt, &item_reg, &inner_untyped, source_path)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_select_option_parts(
    opt: &CompiledDynamicSelectOption,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for parts in [&opt.value_parts, &opt.label_parts] {
        for part in parts {
            if let CompiledTextPart::Dynamic { expr, .. } = part {
                let _ = validate_any_expr(expr, registry, untyped_vars, source_path)?;
            }
        }
    }
    Ok(())
}

fn validate_paint_and_layout_slots(
    node: &CompiledNode,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for pb in &node.paint_bindings {
        let Some(val) = validate_any_expr(&pb.expr, registry, untyped_vars, source_path)? else {
            continue;
        };
        match &pb.slot {
            PaintSlot::FastTransform => {
                if !matches!(val, Value::FastTransform(_)) {
                    return Err(to_compile_err(
                        EvalError::TypeError(format!(
                            "expected FastTransform in paint slot, got {val:?}"
                        )),
                        source_path,
                    ));
                }
            }
            _ => {
                if val.as_f64().is_none() {
                    return Err(to_compile_err(
                        EvalError::TypeError(format!(
                            "expected numeric value in paint slot {:?}, got {val:?}",
                            pb.slot
                        )),
                        source_path,
                    ));
                }
            }
        }
    }

    for lb in &node.layout_bindings {
        let Some(val) = validate_any_expr(&lb.expr, registry, untyped_vars, source_path)? else {
            continue;
        };
        if val.as_f64().is_none() {
            return Err(to_compile_err(
                EvalError::TypeError(format!(
                    "expected numeric value in layout slot {:?}, got {val:?}",
                    lb.slot
                )),
                source_path,
            ));
        }
    }
    Ok(())
}

fn validate_event_slot(
    expr: &TemplateExpr,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
    slot_name: &str,
    allow_pointer: bool,
    allow_wheel: bool,
) -> Result<(), CompileError> {
    let Some(val) = validate_any_expr(expr, registry, untyped_vars, source_path)? else {
        return Ok(());
    };
    let msg = match expr {
        TemplateExpr::Path(_) => match val {
            Value::Command(_) => return Ok(()),
            Value::PointerHandler(_) if allow_pointer => return Ok(()),
            Value::WheelHandler(_) if allow_wheel => return Ok(()),
            _ => format!("`{slot_name}` path requires handler or Command, got {val:?}"),
        },
        TemplateExpr::Call { .. } => match val {
            Value::Command(_) | Value::Unit => return Ok(()),
            _ => format!("`{slot_name}` call requires Command or `()` return, got {val:?}"),
        },
        _ => format!("`{slot_name}` requires a handler, Command path, or method call"),
    };
    Err(to_compile_err(EvalError::TypeError(msg), source_path))
}

fn validate_event_and_binding_slots(
    node: &CompiledNode,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for bool_attr in [&node.disabled, &node.required, &node.checked]
        .into_iter()
        .flatten()
    {
        if let CompiledBoolAttr::Dynamic { expr, .. } = bool_attr {
            if let Some(val) = validate_any_expr(expr, registry, untyped_vars, source_path)? {
                try_eval_bool(&val).map_err(|e| to_compile_err(e, source_path))?;
            }
        }
    }
    for action_expr in [
        &node.on_click,
        &node.on_focus,
        &node.on_blur,
        &node.on_scroll,
        &node.on_submit,
        &node.on_reset,
    ]
    .into_iter()
    .flatten()
    {
        validate_event_slot(
            action_expr,
            registry,
            untyped_vars,
            source_path,
            "event",
            false,
            false,
        )?;
    }
    for ptr_expr in [
        &node.on_pointer_down,
        &node.on_pointer_up,
        &node.on_pointer_move,
        &node.on_context_menu,
    ]
    .into_iter()
    .flatten()
    {
        validate_event_slot(
            ptr_expr,
            registry,
            untyped_vars,
            source_path,
            "onpointer*",
            true,
            false,
        )?;
    }
    if let Some(wheel_expr) = &node.on_wheel {
        validate_event_slot(
            wheel_expr,
            registry,
            untyped_vars,
            source_path,
            "onwheel",
            false,
            true,
        )?;
    }
    validate_two_way_and_handler_slots(node, registry, untyped_vars, source_path)
}

fn validate_two_way_and_handler_slots(
    node: &CompiledNode,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for bind_expr in [&node.bind_value, &node.bind_checked].into_iter().flatten() {
        if expr_uses_untyped_var(bind_expr, untyped_vars) {
            continue;
        }
        let Some(dyn_sig) = resolve_signal_from_expr(bind_expr, registry) else {
            return Err(to_compile_err(
                EvalError::UnknownSignal(format!("{bind_expr:?}")),
                source_path,
            ));
        };
        if !dyn_sig.is_writable() {
            return Err(to_compile_err(
                EvalError::UnknownSignal(format!("{bind_expr:?} is read-only")),
                source_path,
            ));
        }
    }
    for handler_expr in [&node.on_input, &node.on_change, &node.on_keydown]
        .into_iter()
        .flatten()
    {
        validate_string_handler_expr(handler_expr, registry, untyped_vars, source_path)?;
    }
    if let Some(canvas_expr) = &node.canvas_draw {
        if let Some(val) = validate_any_expr(canvas_expr, registry, untyped_vars, source_path)? {
            if !matches!(val, Value::CanvasDraw(_)) {
                return Err(to_compile_err(
                    EvalError::TypeError(format!("`ondraw` requires CanvasDraw, got {val:?}")),
                    source_path,
                ));
            }
        }
    }
    Ok(())
}

fn validate_string_handler_expr(
    expr: &TemplateExpr,
    registry: &dyn Registry,
    untyped_vars: &[&str],
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    let Some(val) = validate_any_expr(expr, registry, untyped_vars, source_path)? else {
        return Ok(());
    };
    if matches!(val, Value::InputHandler(_)) {
        return Ok(());
    }
    if let Some(dyn_sig) = resolve_signal_from_expr(expr, registry) {
        if dyn_sig.is_writable() {
            return Ok(());
        }
    }
    Err(to_compile_err(
        EvalError::TypeError(format!(
            "string input event requires InputCommand or writable Signal<String>, got {val:?}"
        )),
        source_path,
    ))
}
