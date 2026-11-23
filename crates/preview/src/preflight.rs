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
        validate_node(root, registry, doc.source_path.as_deref())?;
    }
    Ok(())
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
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    match &node.kind {
        CompiledNodeKind::If(cif) => validate_if_node(cif, registry, source_path),
        CompiledNodeKind::For(cfor) => validate_for_node(cfor, registry, source_path),
        CompiledNodeKind::Element => validate_element_node(node, registry, source_path),
    }
}

fn validate_if_node(
    cif: &CompiledIf,
    registry: &dyn Registry,
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for br in &cif.branches {
        if let Some(cond_expr) = &br.condition {
            let val = validate_template_expr(cond_expr, registry)
                .map_err(|e| to_compile_err(e, source_path))?;
            try_eval_bool(&val).map_err(|e| to_compile_err(e, source_path))?;
        }
        for child in &br.children {
            validate_node(child, registry, source_path)?;
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
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    let list_val =
        validate_template_expr(&cfor.list, registry).map_err(|e| to_compile_err(e, source_path))?;
    let Value::List(items) = list_val else {
        return Err(to_compile_err(
            EvalError::TypeError(format!(
                "`<for each>` requires List value, got {list_val:?}"
            )),
            source_path,
        ));
    };

    for empty_child in &cfor.empty_body {
        validate_node(empty_child, registry, source_path)?;
    }

    let item_reg = PreflightForItemReg {
        parent: registry,
        item_var: &cfor.item_var,
        sample_item: items.first(),
    };
    let _ =
        validate_template_expr(&cfor.key, &item_reg).map_err(|e| to_compile_err(e, source_path))?;
    for body_child in &cfor.body {
        validate_node(body_child, &item_reg, source_path)?;
    }
    Ok(())
}

fn validate_element_node(
    node: &CompiledNode,
    registry: &dyn Registry,
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for parts in [
        &node.text_parts,
        &node.placeholder_parts,
        &node.aria_label_parts,
        &node.title_parts,
        &node.img_src,
        &node.img_alt,
        &node.link_href,
        &node.class_parts,
        &node.dynamic_inline_style_parts,
    ] {
        for part in parts {
            if let CompiledTextPart::Dynamic { expr, .. } = part {
                let _ = validate_template_expr(expr, registry)
                    .map_err(|e| to_compile_err(e, source_path))?;
            }
        }
    }

    validate_select_children(&node.select_children, registry, source_path)?;
    validate_paint_and_layout_slots(node, registry, source_path)?;
    validate_event_and_binding_slots(node, registry, source_path)?;

    for child in &node.children {
        validate_node(child, registry, source_path)?;
    }
    Ok(())
}

fn validate_select_children(
    children: &[CompiledSelectChild],
    registry: &dyn Registry,
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for child in children {
        match child {
            CompiledSelectChild::Option(opt) => {
                validate_select_option_parts(opt, registry, source_path)?;
            }
            CompiledSelectChild::For(sfor) => {
                let list_val = validate_template_expr(&sfor.list, registry)
                    .map_err(|e| to_compile_err(e, source_path))?;
                let Value::List(items) = list_val else {
                    return Err(to_compile_err(
                        EvalError::TypeError(format!(
                            "`<for each>` requires List value, got {list_val:?}"
                        )),
                        source_path,
                    ));
                };
                let item_reg = PreflightForItemReg {
                    parent: registry,
                    item_var: &sfor.item_var,
                    sample_item: items.first(),
                };
                let _ = validate_template_expr(&sfor.key, &item_reg)
                    .map_err(|e| to_compile_err(e, source_path))?;
                for opt in &sfor.options {
                    validate_select_option_parts(opt, &item_reg, source_path)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_select_option_parts(
    opt: &CompiledDynamicSelectOption,
    registry: &dyn Registry,
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for parts in [&opt.value_parts, &opt.label_parts] {
        for part in parts {
            if let CompiledTextPart::Dynamic { expr, .. } = part {
                let _ = validate_template_expr(expr, registry)
                    .map_err(|e| to_compile_err(e, source_path))?;
            }
        }
    }
    Ok(())
}

fn validate_paint_and_layout_slots(
    node: &CompiledNode,
    registry: &dyn Registry,
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for pb in &node.paint_bindings {
        let val = validate_template_expr(&pb.expr, registry)
            .map_err(|e| to_compile_err(e, source_path))?;
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
        let val = validate_template_expr(&lb.expr, registry)
            .map_err(|e| to_compile_err(e, source_path))?;
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

fn validate_click_slot(
    click_expr: &TemplateExpr,
    registry: &dyn Registry,
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    let val =
        validate_template_expr(click_expr, registry).map_err(|e| to_compile_err(e, source_path))?;
    match click_expr {
        TemplateExpr::Path(_) => {
            if !matches!(val, Value::Command(_)) {
                return Err(to_compile_err(
                    EvalError::TypeError(format!("`onclick` path requires Command, got {val:?}")),
                    source_path,
                ));
            }
        }
        TemplateExpr::Call { .. } => {
            if !matches!(val, Value::Command(_) | Value::Unit) {
                return Err(to_compile_err(
                    EvalError::TypeError(format!(
                        "`onclick` call requires Command or `()` return, got {val:?}"
                    )),
                    source_path,
                ));
            }
        }
        _ => {
            return Err(to_compile_err(
                EvalError::TypeError(
                    "`onclick` requires a Command path or method call".to_string(),
                ),
                source_path,
            ));
        }
    }
    Ok(())
}

fn validate_event_and_binding_slots(
    node: &CompiledNode,
    registry: &dyn Registry,
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for bool_attr in [&node.disabled, &node.required, &node.checked]
        .into_iter()
        .flatten()
    {
        if let CompiledBoolAttr::Dynamic { expr, .. } = bool_attr {
            let val = validate_template_expr(expr, registry)
                .map_err(|e| to_compile_err(e, source_path))?;
            try_eval_bool(&val).map_err(|e| to_compile_err(e, source_path))?;
        }
    }
    for action_expr in [
        &node.on_click,
        &node.on_focus,
        &node.on_blur,
        &node.on_pointer_down,
        &node.on_pointer_up,
        &node.on_pointer_move,
        &node.on_scroll,
        &node.on_submit,
        &node.on_reset,
    ]
    .into_iter()
    .flatten()
    {
        validate_click_slot(action_expr, registry, source_path)?;
    }
    validate_two_way_and_handler_slots(node, registry, source_path)
}

fn validate_two_way_and_handler_slots(
    node: &CompiledNode,
    registry: &dyn Registry,
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    for bind_expr in [&node.bind_value, &node.bind_checked].into_iter().flatten() {
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
        validate_string_handler_expr(handler_expr, registry, source_path)?;
    }
    if let Some(canvas_expr) = &node.canvas_draw {
        let val = validate_template_expr(canvas_expr, registry)
            .map_err(|e| to_compile_err(e, source_path))?;
        if !matches!(val, Value::CanvasDraw(_)) {
            return Err(to_compile_err(
                EvalError::TypeError(format!("`ondraw` requires CanvasDraw, got {val:?}")),
                source_path,
            ));
        }
    }
    Ok(())
}

fn validate_string_handler_expr(
    expr: &TemplateExpr,
    registry: &dyn Registry,
    source_path: Option<&std::path::Path>,
) -> Result<(), CompileError> {
    let val = validate_template_expr(expr, registry).map_err(|e| to_compile_err(e, source_path))?;
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
