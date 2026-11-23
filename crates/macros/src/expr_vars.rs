//! Free-variable analysis and token generation on `TemplateExpr` and `CompiledNode`
//! so closures clone captured variables (`vm`, `store`, `ctrl`) and read `<for>` loop scopes.

use krui_compiler::{
    BinaryOp, CompiledBoolAttr, CompiledNode, CompiledNodeKind, CompiledSelectChild,
    CompiledTextPart, TemplateExpr, TemplateLit, UnaryOp,
};
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::quote;

/// Lowers a `TemplateExpr` AST into a `proc_macro2::TokenStream`, parsing only leaf identifiers via `syn::parse_str::<Ident>`.
pub fn emit_template_expr(
    expr: &TemplateExpr,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    match expr {
        TemplateExpr::Path(segments) => emit_path_expr(segments, span, loop_vars),
        TemplateExpr::EnumVariant(segments) => emit_enum_variant_expr(segments, span),
        TemplateExpr::Literal(lit) => emit_literal_expr(lit),
        TemplateExpr::Unary { op, expr } => {
            let inner = emit_template_expr(expr, span, loop_vars)?;
            match op {
                UnaryOp::Not => {
                    Ok(quote! { (!::krui::KruiBoolCondition::eval_krui_bool(&(#inner))) })
                }
                UnaryOp::Neg => Ok(quote! {
                    ::krui::IntoKruiTemplateScalar::into_krui_scalar(&(#inner)).checked_neg()
                }),
            }
        }
        TemplateExpr::Binary { op, left, right } => {
            emit_binary_expr(*op, left, right, span, loop_vars)
        }
        TemplateExpr::Call {
            receiver,
            method,
            args,
        } => emit_call_expr(receiver, method, args, span, loop_vars),
    }
}

pub(crate) fn is_loop_var(name: &str, loop_vars: &[String]) -> bool {
    loop_vars.iter().any(|lv| lv == name)
}

fn emit_path_expr(
    segments: &[String],
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let mut idents = Vec::with_capacity(segments.len());
    for seg in segments {
        let mut id = syn::parse_str::<Ident>(seg).map_err(|err| {
            syn::Error::new(
                span,
                format!("Invalid path segment `{seg}` in template expression: {err}"),
            )
        })?;
        id.set_span(span);
        idents.push(id);
    }
    let first = &idents[0];
    let rest = &idents[1..];
    if is_loop_var(&segments[0], loop_vars) {
        Ok(quote! { (#first.get()) #( . #rest )* })
    } else {
        Ok(quote! { #first #( . #rest )* })
    }
}

fn emit_enum_variant_expr(segments: &[String], span: Span) -> Result<TokenStream2, syn::Error> {
    let mut idents = Vec::with_capacity(segments.len());
    for seg in segments {
        let mut id = syn::parse_str::<Ident>(seg).map_err(|err| {
            syn::Error::new(
                span,
                format!("Invalid enum path segment `{seg}` in template expression: {err}"),
            )
        })?;
        id.set_span(span);
        idents.push(id);
    }
    let first = &idents[0];
    let rest = &idents[1..];
    Ok(quote! { #first #( :: #rest )* })
}

fn emit_literal_expr(lit: &TemplateLit) -> Result<TokenStream2, syn::Error> {
    match lit {
        TemplateLit::Bool(b) => Ok(quote! { #b }),
        TemplateLit::Int(i) => {
            let lit_tok = proc_macro2::Literal::i64_unsuffixed(*i);
            Ok(quote! { #lit_tok })
        }
        TemplateLit::Float(f) => {
            let lit_tok = proc_macro2::Literal::f64_unsuffixed(*f);
            Ok(quote! { #lit_tok })
        }
        TemplateLit::Str(s) => Ok(quote! { #s }),
    }
}

fn emit_binary_expr(
    op: BinaryOp,
    left: &TemplateExpr,
    right: &TemplateExpr,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let lhs = emit_template_expr(left, span, loop_vars)?;
    let rhs = emit_template_expr(right, span, loop_vars)?;
    match op {
        BinaryOp::Or => Ok(quote! {
            (::krui::KruiBoolCondition::eval_krui_bool(&(#lhs))
                || ::krui::KruiBoolCondition::eval_krui_bool(&(#rhs)))
        }),
        BinaryOp::And => Ok(quote! {
            (::krui::KruiBoolCondition::eval_krui_bool(&(#lhs))
                && ::krui::KruiBoolCondition::eval_krui_bool(&(#rhs)))
        }),
        BinaryOp::Eq => Ok(quote! {
            (::krui::IntoKruiTemplateScalar::into_krui_scalar(&(#lhs))
                == ::krui::IntoKruiTemplateScalar::into_krui_scalar(&(#rhs)))
        }),
        BinaryOp::Ne => Ok(quote! {
            (::krui::IntoKruiTemplateScalar::into_krui_scalar(&(#lhs))
                != ::krui::IntoKruiTemplateScalar::into_krui_scalar(&(#rhs)))
        }),
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            emit_relational_binary_expr(op, &lhs, &rhs)
        }
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            emit_arithmetic_binary_expr(op, &lhs, &rhs)
        }
    }
}

fn emit_relational_binary_expr(
    op: BinaryOp,
    lhs: &TokenStream2,
    rhs: &TokenStream2,
) -> Result<TokenStream2, syn::Error> {
    let cmp_method = match op {
        BinaryOp::Lt => quote!(is_lt),
        BinaryOp::Le => quote!(is_le),
        BinaryOp::Gt => quote!(is_gt),
        _ => quote!(is_ge),
    };
    Ok(quote! {
        ::krui::IntoKruiTemplateScalar::into_krui_scalar(&(#lhs))
            .compare_ord(&::krui::IntoKruiTemplateScalar::into_krui_scalar(&(#rhs)))
            .#cmp_method()
    })
}

fn emit_arithmetic_binary_expr(
    op: BinaryOp,
    lhs: &TokenStream2,
    rhs: &TokenStream2,
) -> Result<TokenStream2, syn::Error> {
    let method = match op {
        BinaryOp::Add => quote!(checked_add),
        BinaryOp::Sub => quote!(checked_sub),
        BinaryOp::Mul => quote!(checked_mul),
        BinaryOp::Div => quote!(safe_div),
        _ => quote!(safe_rem),
    };
    Ok(quote! {
        ::krui::IntoKruiTemplateScalar::into_krui_scalar(&(#lhs))
            .#method(::krui::IntoKruiTemplateScalar::into_krui_scalar(&(#rhs)))
    })
}

fn emit_call_expr(
    receiver: &[String],
    method: &str,
    args: &[TemplateExpr],
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let recv_toks = emit_path_expr(receiver, span, loop_vars)?;
    let mut method_id = syn::parse_str::<Ident>(method)
        .map_err(|err| syn::Error::new(span, format!("Invalid method name `{method}`: {err}")))?;
    method_id.set_span(span);
    let mut arg_toks = Vec::with_capacity(args.len());
    for arg in args {
        arg_toks.push(emit_template_expr(arg, span, loop_vars)?);
    }
    Ok(quote! {
        (#recv_toks).__krui_template_callable(). #method_id ( #( #arg_toks ),* )
    })
}

/// Wraps the emitted expression tokens in parentheses for binding methods.
pub fn emit_binding_expr(
    expr: &TemplateExpr,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let inner = emit_template_expr(expr, span, loop_vars)?;
    Ok(quote! { (#inner) })
}

/// Collects free root identifiers from `expr` (excluding `loop_vars`) as `proc_macro2::Ident`s.
pub fn collect_free_idents_from_template_expr(
    expr: &TemplateExpr,
    span: Span,
    loop_vars: &[String],
    out: &mut Vec<Ident>,
) -> Result<(), syn::Error> {
    let mut names: Vec<String> = Vec::new();
    expr.collect_free_root_idents(loop_vars, &mut names);
    for name in names {
        let mut ident = syn::parse_str::<Ident>(&name).map_err(|err| {
            syn::Error::new(span, format!("Invalid root identifier `{name}`: {err}"))
        })?;
        ident.set_span(span);
        if !out.iter().any(|existing| existing == &ident) {
            out.push(ident);
        }
    }
    Ok(())
}

/// Collects all free root identifiers referenced by `node` and its descendants.
pub fn collect_free_idents_from_node(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
    out: &mut Vec<Ident>,
) -> Result<(), syn::Error> {
    match &node.kind {
        CompiledNodeKind::If(cif) => {
            for br in &cif.branches {
                if let Some(cond) = &br.condition {
                    collect_free_idents_from_template_expr(cond, span, loop_vars, out)?;
                }
                for ch in &br.children {
                    collect_free_idents_from_node(ch, span, loop_vars, out)?;
                }
            }
        }
        CompiledNodeKind::For(cfor) => {
            collect_free_idents_from_template_expr(&cfor.list, span, loop_vars, out)?;
            let mut inner_vars = loop_vars.to_vec();
            inner_vars.push(cfor.item_var.clone());
            collect_free_idents_from_template_expr(&cfor.key, span, &inner_vars, out)?;
            for ch in &cfor.body {
                collect_free_idents_from_node(ch, span, &inner_vars, out)?;
            }
            for ch in &cfor.empty_body {
                collect_free_idents_from_node(ch, span, loop_vars, out)?;
            }
        }
        CompiledNodeKind::Element => {
            collect_free_idents_from_element(node, span, loop_vars, out)?;
        }
    }
    Ok(())
}

pub(crate) fn collect_free_idents_from_select_children(
    children: &[CompiledSelectChild],
    span: Span,
    loop_vars: &[String],
    out: &mut Vec<Ident>,
) -> Result<(), syn::Error> {
    for child in children {
        match child {
            CompiledSelectChild::Option(opt) => {
                for p in opt.value_parts.iter().chain(opt.label_parts.iter()) {
                    if let CompiledTextPart::Dynamic { expr, .. } = p {
                        collect_free_idents_from_template_expr(expr, span, loop_vars, out)?;
                    }
                }
            }
            CompiledSelectChild::For(sfor) => {
                collect_free_idents_from_template_expr(&sfor.list, span, loop_vars, out)?;
                let mut inner_vars = loop_vars.to_vec();
                inner_vars.push(sfor.item_var.clone());
                collect_free_idents_from_template_expr(&sfor.key, span, &inner_vars, out)?;
                for opt in &sfor.options {
                    for p in opt.value_parts.iter().chain(opt.label_parts.iter()) {
                        if let CompiledTextPart::Dynamic { expr, .. } = p {
                            collect_free_idents_from_template_expr(expr, span, &inner_vars, out)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn collect_free_idents_from_element(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
    out: &mut Vec<Ident>,
) -> Result<(), syn::Error> {
    for b in &node.paint_bindings {
        collect_free_idents_from_template_expr(&b.expr, span, loop_vars, out)?;
    }
    for b in &node.layout_bindings {
        collect_free_idents_from_template_expr(&b.expr, span, loop_vars, out)?;
    }
    for p in node
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
    {
        if let CompiledTextPart::Dynamic { expr, .. } = p {
            collect_free_idents_from_template_expr(expr, span, loop_vars, out)?;
        }
    }
    collect_free_idents_from_element_events(node, span, loop_vars, out)?;
    collect_free_idents_from_select_children(&node.select_children, span, loop_vars, out)?;
    for ch in &node.children {
        collect_free_idents_from_node(ch, span, loop_vars, out)?;
    }
    for sp in &node.rich_spans {
        for p in &sp.parts {
            if let CompiledTextPart::Dynamic { expr, .. } = p {
                collect_free_idents_from_template_expr(expr, span, loop_vars, out)?;
            }
        }
    }
    Ok(())
}

fn collect_free_idents_from_element_events(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
    out: &mut Vec<Ident>,
) -> Result<(), syn::Error> {
    for opt in [
        &node.on_click,
        &node.on_focus,
        &node.on_blur,
        &node.on_pointer_down,
        &node.on_pointer_up,
        &node.on_pointer_move,
        &node.on_scroll,
        &node.on_input,
        &node.on_change,
        &node.on_keydown,
        &node.on_submit,
        &node.on_reset,
        &node.bind_value,
        &node.bind_checked,
        &node.canvas_draw,
    ]
    .into_iter()
    .flatten()
    {
        collect_free_idents_from_template_expr(opt, span, loop_vars, out)?;
    }
    for bool_attr in [&node.disabled, &node.required, &node.checked]
        .into_iter()
        .flatten()
    {
        if let CompiledBoolAttr::Dynamic { expr, .. } = bool_attr {
            collect_free_idents_from_template_expr(expr, span, loop_vars, out)?;
        }
    }
    Ok(())
}

/// Emits `let ident = ident.clone();` bindings for closure captures.
pub fn emit_clone_lets(idents: &[Ident]) -> TokenStream2 {
    quote! {
        #( let #idents = #idents.clone(); )*
    }
}
