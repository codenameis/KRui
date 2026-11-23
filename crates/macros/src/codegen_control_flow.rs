//! Procedural macro code generation for `<if>`/`<else-if>`/`<else>`, `<for>`/`<empty>`,
//! static/dynamic `class="..."` bindings, and interpolated text slots.

use crate::codegen::{emit_node_construction, format_path_expr};
use crate::expr_vars::{
    collect_free_idents_from_node, collect_free_idents_from_template_expr, emit_clone_lets,
    emit_template_expr,
};
use krui_compiler::{CompiledFor, CompiledIf, CompiledNode, CompiledTextPart};
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use std::path::Path;

pub(crate) fn emit_if_construction(
    cif: &CompiledIf,
    source_base_dir: Option<&Path>,
    span: Span,
    parent_path_expr: &TokenStream2,
    for_key_ident: Option<&Ident>,
    sibling_idx: usize,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let mut cond_idents = Vec::new();
    let mut cond_arms = Vec::with_capacity(cif.branches.len());
    let mut branch_closures = Vec::with_capacity(cif.branches.len());

    for (idx, br) in cif.branches.iter().enumerate() {
        if let Some(cond_expr) = &br.condition {
            collect_free_idents_from_template_expr(cond_expr, span, loop_vars, &mut cond_idents)?;
            let cond_tokens = emit_template_expr(cond_expr, span, loop_vars)?;
            cond_arms.push(quote! {
                if ::krui::KruiBoolCondition::eval_krui_bool(&(#cond_tokens)) {
                    return Some(#idx);
                }
            });
        } else {
            cond_arms.push(quote! { return Some(#idx); });
        }
        branch_closures.push(emit_if_branch_closure(
            &br.children,
            source_base_dir,
            span,
            (parent_path_expr, for_key_ident, sibling_idx, idx),
            loop_vars,
        )?);
    }

    let cond_clones = emit_clone_lets(&cond_idents);
    let branch_count = branch_closures.len();
    Ok(quote! {{
        let __branch_fn = {
            #cond_clones
            move || -> Option<usize> {
                #[allow(unreachable_code)]
                {
                    #(#cond_arms)*
                    None
                }
            }
        };
        let mut __branches: ::std::vec::Vec<::krui::BranchMountFn> =
            ::std::vec::Vec::with_capacity(#branch_count);
        #( __branches.push(#branch_closures); )*
        __tree.mount_if(__parent, __branch_fn, __branches)
    }})
}

fn emit_if_branch_closure(
    children: &[CompiledNode],
    source_base_dir: Option<&Path>,
    span: Span,
    (parent_path_expr, for_key_ident, sibling_idx, idx): (
        &TokenStream2,
        Option<&Ident>,
        usize,
        usize,
    ),
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let br_path_ident = format_ident!("__if_br_path_{sibling_idx}_{idx}");
    let br_path_let = if cfg!(feature = "preview") {
        let br_suffix = format!("/branch[{idx}]");
        let br_path_expr = format_path_expr(
            parent_path_expr,
            for_key_ident,
            sibling_idx,
            "#if",
            None,
            &br_suffix,
        );
        quote! { #[allow(unused_variables)] let #br_path_ident = #br_path_expr; }
    } else {
        quote! { #[allow(unused_variables)] let #br_path_ident = ""; }
    };
    let mut br_idents = Vec::new();
    for ch in children {
        collect_free_idents_from_node(ch, span, loop_vars, &mut br_idents)?;
    }
    let br_clones = emit_clone_lets(&br_idents);
    let mut child_exprs = Vec::with_capacity(children.len());
    for (ch_idx, ch) in children.iter().enumerate() {
        child_exprs.push(emit_node_construction(
            ch,
            source_base_dir,
            span,
            &quote! { #br_path_ident },
            None,
            ch_idx,
            loop_vars,
        )?);
    }
    Ok(quote! {{
        #br_clones
        #br_path_let
        ::std::rc::Rc::new(move |__tree: &mut ::krui::ViewTree, __parent: Option<::krui::NodeId>| {
            let mut __ids = ::std::vec::Vec::new();
            #( __ids.push(#child_exprs); )*
            __ids
        }) as ::krui::BranchMountFn
    }})
}

fn emit_for_path_let(
    parent_path_expr: &TokenStream2,
    for_key_ident: Option<&Ident>,
    sibling_idx: usize,
    for_path_ident: &Ident,
) -> TokenStream2 {
    if cfg!(feature = "preview") {
        let expr = format_path_expr(
            parent_path_expr,
            for_key_ident,
            sibling_idx,
            "#for",
            None,
            "",
        );
        quote! { #[allow(unused_variables)] let #for_path_ident = #expr; }
    } else {
        quote! { #[allow(unused_variables)] let #for_path_ident = ""; }
    }
}

pub(crate) fn emit_for_construction(
    cfor: &CompiledFor,
    source_base_dir: Option<&Path>,
    span: Span,
    parent_path_expr: &TokenStream2,
    for_key_ident: Option<&Ident>,
    sibling_idx: usize,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let for_path_ident = format_ident!("__for_path_{sibling_idx}");
    let for_path_let = emit_for_path_let(
        parent_path_expr,
        for_key_ident,
        sibling_idx,
        &for_path_ident,
    );
    let item_for_key_ident = format_ident!("__for_item_key_{sibling_idx}");
    let item_ident = parse_for_item_ident(&cfor.item_var, span)?;
    let mut inner_vars = loop_vars.to_vec();
    inner_vars.push(cfor.item_var.clone());

    let list_tokens = emit_template_expr(&cfor.list, span, loop_vars)?;
    let key_tokens = emit_template_expr(&cfor.key, span, &inner_vars)?;
    let (list_clones, key_clones, key_idents) =
        collect_for_clone_lets(cfor, span, loop_vars, &inner_vars)?;
    let body_closure = emit_for_body_closure(
        cfor,
        source_base_dir,
        span,
        (&for_path_ident, &item_ident, &item_for_key_ident),
        (&key_tokens, &key_idents),
        &inner_vars,
    )?;
    let empty_opt = emit_for_empty_branch(
        &cfor.empty_body,
        source_base_dir,
        span,
        &for_path_ident,
        sibling_idx,
        loop_vars,
    )?;
    let key_closure = emit_for_key_closure(&key_clones, &item_ident, &key_tokens);
    Ok(quote! {{
        #for_path_let
        __tree.mount_for(
            __parent,
            { #list_clones move || ::krui::KruiIterableSource::read_krui_items(&(#list_tokens)) },
            #key_closure,
            #body_closure,
            #empty_opt,
        )
    }})
}

fn emit_for_key_closure(
    key_clones: &TokenStream2,
    item_ident: &Ident,
    key_tokens: &TokenStream2,
) -> TokenStream2 {
    quote! {{
        #key_clones
        move |#item_ident| -> ::std::string::String {
            #[allow(unused_variables)]
            let #item_ident = ::krui::KruiLoopItem((#item_ident).clone());
            ::std::format!("{}", #key_tokens)
        }
    }}
}

fn collect_for_clone_lets(
    cfor: &CompiledFor,
    span: Span,
    loop_vars: &[String],
    inner_vars: &[String],
) -> Result<(TokenStream2, TokenStream2, Vec<Ident>), syn::Error> {
    let mut list_idents = Vec::new();
    collect_free_idents_from_template_expr(&cfor.list, span, loop_vars, &mut list_idents)?;
    let list_clones = emit_clone_lets(&list_idents);
    let mut key_idents = Vec::new();
    collect_free_idents_from_template_expr(&cfor.key, span, inner_vars, &mut key_idents)?;
    let key_clones = emit_clone_lets(&key_idents);
    Ok((list_clones, key_clones, key_idents))
}

fn parse_for_item_ident(item_var: &str, span: Span) -> Result<Ident, syn::Error> {
    let mut item_ident = syn::parse_str::<Ident>(item_var).map_err(|err| {
        syn::Error::new(
            span,
            format!("Invalid `<for each>` item variable `{item_var}`: {err}"),
        )
    })?;
    item_ident.set_span(span);
    Ok(item_ident)
}

fn emit_for_body_closure(
    cfor: &CompiledFor,
    source_base_dir: Option<&Path>,
    span: Span,
    (for_path_ident, item_ident, item_for_key_ident): (&Ident, &Ident, &Ident),
    (key_tokens, key_idents): (&TokenStream2, &[Ident]),
    inner_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let mut body_idents = Vec::new();
    for ch in &cfor.body {
        collect_free_idents_from_node(ch, span, inner_vars, &mut body_idents)?;
    }
    if cfg!(feature = "preview") {
        for kid in key_idents {
            if !body_idents.iter().any(|existing| existing == kid) {
                body_idents.push(kid.clone());
            }
        }
    }
    let body_clones = emit_clone_lets(&body_idents);
    let mut body_exprs = Vec::with_capacity(cfor.body.len());
    for (ch_idx, ch) in cfor.body.iter().enumerate() {
        body_exprs.push(emit_node_construction(
            ch,
            source_base_dir,
            span,
            &quote! { #for_path_ident },
            Some(item_for_key_ident),
            ch_idx,
            inner_vars,
        )?);
    }
    let item_key_let = if cfg!(feature = "preview") {
        quote! { #[allow(unused_variables)] let #item_for_key_ident = ::std::format!("{}", #key_tokens); }
    } else {
        quote! { #[allow(unused_variables)] let #item_for_key_ident = ""; }
    };
    Ok(quote! {{
        #body_clones
        #[allow(unused_variables)]
        let #for_path_ident = #for_path_ident.clone();
        move |__tree: &mut ::krui::ViewTree, __parent: Option<::krui::NodeId>, #item_ident| -> ::std::vec::Vec<::krui::NodeId> {
            let _ = &#item_ident;
            #item_key_let
            let mut __ids = ::std::vec::Vec::new();
            #( __ids.push(#body_exprs); )*
            __ids
        }
    }})
}

fn emit_for_empty_branch(
    empty_body: &[CompiledNode],
    source_base_dir: Option<&Path>,
    span: Span,
    for_path_ident: &Ident,
    sibling_idx: usize,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    if empty_body.is_empty() {
        return Ok(quote! { None });
    }
    let empty_path_ident = format_ident!("__for_empty_path_{sibling_idx}");
    let empty_path_let = if cfg!(feature = "preview") {
        quote! { #[allow(unused_variables)] let #empty_path_ident = ::std::format!("{}/empty", #for_path_ident); }
    } else {
        quote! { #[allow(unused_variables)] let #empty_path_ident = ""; }
    };
    let mut empty_idents = Vec::new();
    for ch in empty_body {
        collect_free_idents_from_node(ch, span, loop_vars, &mut empty_idents)?;
    }
    let empty_clones = emit_clone_lets(&empty_idents);
    let mut empty_exprs = Vec::with_capacity(empty_body.len());
    for (ch_idx, ch) in empty_body.iter().enumerate() {
        empty_exprs.push(emit_node_construction(
            ch,
            source_base_dir,
            span,
            &quote! { #empty_path_ident },
            None,
            ch_idx,
            loop_vars,
        )?);
    }
    Ok(quote! {
        Some({
            #empty_clones
            #empty_path_let
            ::std::rc::Rc::new(move |__tree: &mut ::krui::ViewTree, __parent: Option<::krui::NodeId>| {
                let mut __ids = ::std::vec::Vec::new();
                #( __ids.push(#empty_exprs); )*
                __ids
            }) as ::krui::BranchMountFn
        })
    })
}

pub(crate) fn emit_class_binding(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let has_dyn = node
        .class_parts
        .iter()
        .any(|p| matches!(p, CompiledTextPart::Dynamic { .. }));
    if !has_dyn {
        if node.classes.is_empty() {
            return Ok(quote! {});
        }
        let cls = &node.classes;
        return Ok(quote! { __tree.set_element_classes(__node_id, &[#(#cls),*]); });
    }
    let mut idents = Vec::new();
    let mut segment_pushers = Vec::with_capacity(node.class_parts.len());
    for part in &node.class_parts {
        match part {
            CompiledTextPart::Static(s) => segment_pushers.push(quote! { __buf.push_str(#s); }),
            CompiledTextPart::Dynamic { expr, .. } => {
                let expr_toks = emit_template_expr(expr, span, loop_vars)?;
                collect_free_idents_from_template_expr(expr, span, loop_vars, &mut idents)?;
                segment_pushers.push(quote! {
                    let __raw = ::std::format!("{}", #expr_toks);
                    __buf.push_str(&__raw);
                });
            }
        }
    }
    let clones = emit_clone_lets(&idents);
    Ok(quote! {{
        #clones
        __tree.bind_dynamic_class(__node_id, move || {
            let mut __buf = ::std::string::String::new();
            #(#segment_pushers)*
            __buf
        });
    }})
}

pub(crate) fn emit_interpolated_slot(
    parts: &[CompiledTextPart],
    span: Span,
    static_method: Ident,
    dynamic_method: Ident,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    if parts.is_empty() {
        return Ok(quote! {});
    }
    if !parts
        .iter()
        .any(|p| matches!(p, CompiledTextPart::Dynamic { .. }))
    {
        let mut combined = String::new();
        for part in parts {
            if let CompiledTextPart::Static(s) = part {
                combined.push_str(s);
            }
        }
        return Ok(quote! { __tree.#static_method(__node_id, #combined); });
    }
    let mut idents = Vec::new();
    let mut segment_pushers = Vec::with_capacity(parts.len());
    for part in parts {
        match part {
            CompiledTextPart::Static(s) => segment_pushers.push(quote! { __buf.push_str(#s); }),
            CompiledTextPart::Dynamic { expr, .. } => {
                let expr_toks = emit_template_expr(expr, span, loop_vars)?;
                collect_free_idents_from_template_expr(expr, span, loop_vars, &mut idents)?;
                segment_pushers.push(quote! {
                    let __raw = ::std::format!("{}", #expr_toks);
                    __buf.push_str(&::krui::sanitize_untrusted_text(&__raw));
                });
            }
        }
    }
    let clones = emit_clone_lets(&idents);
    Ok(quote! {{
        #clones
        __tree.#dynamic_method(__node_id, move || {
            let mut __buf = ::std::string::String::new();
            #(#segment_pushers)*
            __buf
        });
    }})
}
