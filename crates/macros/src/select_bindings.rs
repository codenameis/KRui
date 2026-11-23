//! `<select><for>` dynamic option binding codegen.

use crate::expr_vars::{
    collect_free_idents_from_select_children, emit_clone_lets, emit_template_expr,
};
use krui_compiler::{
    CompiledDynamicSelectOption, CompiledNode, CompiledSelectChild, CompiledTextPart,
};
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::quote;

pub(crate) fn emit_select_children_binding(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    if node.select_children.is_empty() {
        return Ok(quote! {});
    }
    let mut idents = Vec::new();
    collect_free_idents_from_select_children(&node.select_children, span, loop_vars, &mut idents)?;
    let clones = emit_clone_lets(&idents);
    let mut stmts = Vec::with_capacity(node.select_children.len());
    for child in &node.select_children {
        match child {
            CompiledSelectChild::Option(opt) => {
                stmts.push(emit_single_select_option_push(opt, span, loop_vars)?);
            }
            CompiledSelectChild::For(sfor) => {
                let mut item_ident = syn::parse_str::<Ident>(&sfor.item_var).map_err(|e| {
                    syn::Error::new(span, format!("Invalid `<select><for>` item var: {e}"))
                })?;
                item_ident.set_span(span);
                let list_toks = emit_template_expr(&sfor.list, span, loop_vars)?;
                let mut inner_vars = loop_vars.to_vec();
                inner_vars.push(sfor.item_var.clone());
                let key_toks = emit_template_expr(&sfor.key, span, &inner_vars)?;
                let mut opt_stmts = Vec::with_capacity(sfor.options.len());
                for opt in &sfor.options {
                    opt_stmts.push(emit_single_select_option_push(opt, span, &inner_vars)?);
                }
                stmts.push(quote! {
                    for __raw_opt in ::krui::KruiIterableSource::read_krui_items(&(#list_toks)) {
                        #[allow(unused_variables)]
                        let #item_ident = ::krui::KruiLoopItem(__raw_opt);
                        let _ = ::std::format!("{}", #key_toks);
                        #(#opt_stmts)*
                    }
                });
            }
        }
    }
    Ok(quote! {
        {
            #clones
            __tree.bind_select_options(__node_id, move || {
                let mut __opts = ::std::vec::Vec::new();
                #(#stmts)*
                __opts
            });
        }
    })
}

fn emit_single_select_option_push(
    opt: &CompiledDynamicSelectOption,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let val_expr = emit_parts_string_expr(&opt.value_parts, span, loop_vars)?;
    let lbl_expr = emit_parts_string_expr(&opt.label_parts, span, loop_vars)?;
    let sel = opt.selected;
    Ok(quote! { __opts.push((#val_expr, #lbl_expr, #sel)); })
}

fn emit_parts_string_expr(
    parts: &[CompiledTextPart],
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let mut pushers = Vec::with_capacity(parts.len());
    for p in parts {
        match p {
            CompiledTextPart::Static(s) => pushers.push(quote! { __s.push_str(#s); }),
            CompiledTextPart::Dynamic { expr, .. } => {
                let toks = emit_template_expr(expr, span, loop_vars)?;
                pushers.push(quote! {
                    let __r = ::std::format!("{}", #toks);
                    __s.push_str(&::krui::sanitize_untrusted_text(&__r));
                });
            }
        }
    }
    Ok(quote! {{
        let mut __s = ::std::string::String::new();
        #(#pushers)*
        __s
    }})
}
