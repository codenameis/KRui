//! Procedural macro code generation for element event, boolean state, and two-way bindings.

use crate::expr_vars::{
    collect_free_idents_from_template_expr, emit_binding_expr, emit_clone_lets, emit_template_expr,
    is_loop_var,
};
use krui_compiler::{CompiledBoolAttr, CompiledNode, TemplateExpr};
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};

/// Emits dynamic-only boolean bindings and event/two-way handlers (`Static` bool attrs live in `StaticNodeDesc`).
pub(crate) fn emit_dynamic_element_event_bindings(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let dyn_only = |opt: &Option<CompiledBoolAttr>| -> Option<CompiledBoolAttr> {
        match opt {
            Some(d @ CompiledBoolAttr::Dynamic { .. }) => Some(d.clone()),
            _ => None,
        }
    };
    let disabled_stmt = emit_bool_attr_binding(
        &dyn_only(&node.disabled),
        span,
        format_ident!("set_disabled"),
        format_ident!("bind_disabled"),
        loop_vars,
    )?;
    let required_stmt = emit_bool_attr_binding(
        &dyn_only(&node.required),
        span,
        format_ident!("set_required"),
        format_ident!("bind_required"),
        loop_vars,
    )?;
    let checked_stmt = emit_bool_attr_binding(
        &dyn_only(&node.checked),
        span,
        format_ident!("set_checked"),
        format_ident!("bind_checked"),
        loop_vars,
    )?;
    let bind_checked_stmt = emit_bind_checked_stmt(node, span, loop_vars)?;
    let input_stmt = emit_input_value_binding(node, span, loop_vars)?;
    let change_stmt = emit_opt_binding(
        &node.on_change,
        span,
        format_ident!("set_on_change"),
        loop_vars,
    )?;
    let keydown_stmt = emit_opt_binding(
        &node.on_keydown,
        span,
        format_ident!("set_on_keydown"),
        loop_vars,
    )?;
    let canvas_stmt = emit_opt_binding(
        &node.canvas_draw,
        span,
        format_ident!("set_canvas_draw"),
        loop_vars,
    )?;
    let lifecycle_stmt = emit_focus_and_pointer_event_bindings(node, span, loop_vars)?;
    Ok(quote! {
        #disabled_stmt #required_stmt #checked_stmt #bind_checked_stmt #lifecycle_stmt
        #input_stmt #change_stmt #keydown_stmt #canvas_stmt
    })
}

#[allow(dead_code)]
pub(crate) fn emit_element_event_bindings(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let disabled_stmt = emit_bool_attr_binding(
        &node.disabled,
        span,
        format_ident!("set_disabled"),
        format_ident!("bind_disabled"),
        loop_vars,
    )?;
    let required_stmt = emit_bool_attr_binding(
        &node.required,
        span,
        format_ident!("set_required"),
        format_ident!("bind_required"),
        loop_vars,
    )?;
    let checked_stmt = emit_bool_attr_binding(
        &node.checked,
        span,
        format_ident!("set_checked"),
        format_ident!("bind_checked"),
        loop_vars,
    )?;
    let bind_checked_stmt = emit_bind_checked_stmt(node, span, loop_vars)?;
    let input_stmt = emit_input_value_binding(node, span, loop_vars)?;
    let change_stmt = emit_opt_binding(
        &node.on_change,
        span,
        format_ident!("set_on_change"),
        loop_vars,
    )?;
    let keydown_stmt = emit_opt_binding(
        &node.on_keydown,
        span,
        format_ident!("set_on_keydown"),
        loop_vars,
    )?;
    let canvas_stmt = emit_opt_binding(
        &node.canvas_draw,
        span,
        format_ident!("set_canvas_draw"),
        loop_vars,
    )?;
    let lifecycle_stmt = emit_focus_and_pointer_event_bindings(node, span, loop_vars)?;
    Ok(quote! {
        #disabled_stmt #required_stmt #checked_stmt #bind_checked_stmt #lifecycle_stmt
        #input_stmt #change_stmt #keydown_stmt #canvas_stmt
    })
}

fn emit_bool_attr_binding(
    opt_attr: &Option<CompiledBoolAttr>,
    span: Span,
    static_m: Ident,
    dyn_m: Ident,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    match opt_attr {
        None => Ok(quote! {}),
        Some(CompiledBoolAttr::Static(val)) => Ok(quote! { __tree.#static_m(__node_id, #val); }),
        Some(CompiledBoolAttr::Dynamic { expr, .. }) => {
            let expr_toks = emit_template_expr(expr, span, loop_vars)?;
            let mut idents = Vec::new();
            collect_free_idents_from_template_expr(expr, span, loop_vars, &mut idents)?;
            let clones = emit_clone_lets(&idents);
            Ok(quote! {
                {
                    #clones
                    __tree.#dyn_m(__node_id, move || ::krui::KruiBoolCondition::eval_krui_bool(&(#expr_toks)));
                }
            })
        }
    }
}

fn emit_bind_checked_stmt(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let Some(bind_chk_expr) = &node.bind_checked else {
        return Ok(quote! {});
    };
    let bind_expr = emit_binding_expr(bind_chk_expr, span, loop_vars)?;
    let read_stmt = if node.checked.is_none() {
        quote! {
            let __bind_chk_read = (#bind_expr).clone();
            __tree.bind_checked(__node_id, move || {
                ::krui::KruiBoolCondition::eval_krui_bool(&__bind_chk_read)
            });
        }
    } else {
        quote! {}
    };
    Ok(quote! {{
        #read_stmt
        let __bind_chk = (#bind_expr).clone();
        __tree.set_on_checked_change(__node_id, move |__checked: bool| {
            __bind_chk.set(__checked);
        });
    }})
}

fn emit_input_value_binding(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    if let Some(bind_val_expr) = &node.bind_value {
        let bind_expr = emit_binding_expr(bind_val_expr, span, loop_vars)?;
        let read_stmt = if node.text_parts.is_empty() {
            quote! {
                let __bind_sig_read = (#bind_expr).clone();
                __tree.bind_dynamic_text(__node_id, move || {
                    let __raw = ::std::format!("{}", __bind_sig_read.get());
                    ::krui::sanitize_untrusted_text(&__raw)
                });
            }
        } else {
            quote! {}
        };
        Ok(quote! {{
            #read_stmt
            let __bind_sig = (#bind_expr).clone();
            __tree.set_on_input(__node_id, move |__val: ::std::string::String| {
                ::krui::KruiBindableSignal::set_from_krui_input(&__bind_sig, __val);
            });
        }})
    } else if let Some(input_expr_ast) = &node.on_input {
        let input_expr = emit_binding_expr(input_expr_ast, span, loop_vars)?;
        Ok(quote! { __tree.set_on_input(__node_id, (#input_expr).clone()); })
    } else {
        Ok(quote! {})
    }
}

fn emit_focus_and_pointer_event_bindings(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let click_stmt = emit_action_binding(
        &node.on_click,
        span,
        format_ident!("bind_click"),
        format_ident!("set_on_click"),
        loop_vars,
    )?;
    let mut stmts = vec![click_stmt];
    for (expr, method_name) in [
        (&node.on_focus, "set_on_focus"),
        (&node.on_blur, "set_on_blur"),
        (&node.on_pointer_down, "set_on_pointer_down"),
        (&node.on_pointer_up, "set_on_pointer_up"),
        (&node.on_pointer_move, "set_on_pointer_move"),
        (&node.on_context_menu, "set_on_context_menu"),
        (&node.on_wheel, "set_on_wheel"),
        (&node.on_scroll, "set_on_scroll"),
        (&node.on_submit, "set_on_submit"),
        (&node.on_reset, "set_on_reset"),
    ] {
        let m = format_ident!("{method_name}");
        stmts.push(emit_action_binding(expr, span, m.clone(), m, loop_vars)?);
    }
    Ok(quote! { #(#stmts)* })
}

fn emit_action_binding(
    opt_expr: &Option<TemplateExpr>,
    span: Span,
    path_method: Ident,
    closure_method: Ident,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let Some(expr) = opt_expr else {
        return Ok(quote! {});
    };
    let expr_tokens = emit_template_expr(expr, span, loop_vars)?;
    match expr {
        TemplateExpr::Path(segs) if !is_loop_var(&segs[0], loop_vars) => Ok(quote! {
            __tree.#path_method(__node_id, (#expr_tokens).clone());
        }),
        _ => {
            let mut idents = Vec::new();
            collect_free_idents_from_template_expr(expr, span, loop_vars, &mut idents)?;
            let clone_stmts = emit_clone_lets(&idents);
            Ok(quote! {
                {
                    #clone_stmts
                    __tree.#closure_method(__node_id, move || {
                        ::krui::ClickActionResult::invoke_click_result(#expr_tokens);
                    });
                }
            })
        }
    }
}

fn emit_opt_binding(
    opt_expr: &Option<TemplateExpr>,
    span: Span,
    method: Ident,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    match opt_expr {
        Some(s) => {
            let e = emit_binding_expr(s, span, loop_vars)?;
            Ok(quote! { __tree.#method(__node_id, (#e).clone()); })
        }
        None => Ok(quote! {}),
    }
}
