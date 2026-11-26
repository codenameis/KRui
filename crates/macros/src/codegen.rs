//! Procedural macro code generation helpers for `CompiledDocument` (D1 static descriptors).

use crate::expr_vars::collect_free_idents_from_node;
use krui_compiler::{CompileWarning, CompiledDocument, CompiledNode};
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::quote;

/// Lowers a [`CompiledDocument`] into a `krui::ViewFragment` token stream via D1 static IR.
pub fn expand_compiled_doc(doc: &CompiledDocument, span: Span) -> Result<TokenStream2, syn::Error> {
    crate::static_codegen::expand_compiled_doc_static(doc, span)
}

pub(crate) fn emit_tracked_file_tokens(doc: &CompiledDocument) -> Vec<TokenStream2> {
    let mut tracked_file_tokens = Vec::new();
    if let Some(src_path) = &doc.source_path {
        let p = src_path.to_string_lossy().into_owned();
        tracked_file_tokens.push(quote! { const _: &[u8] = include_bytes!(#p); });
    }
    for css_path in &doc.external_css_paths {
        let p = css_path.to_string_lossy().into_owned();
        tracked_file_tokens.push(quote! { const _: &[u8] = include_bytes!(#p); });
    }
    for comp_path in &doc.external_component_paths {
        let p = comp_path.to_string_lossy().into_owned();
        tracked_file_tokens.push(quote! { const _: &[u8] = include_bytes!(#p); });
    }
    for (_, font_path) in &doc.embedded_font_files {
        let p = font_path.to_string_lossy().into_owned();
        tracked_file_tokens.push(quote! { const _: &[u8] = include_bytes!(#p); });
    }
    tracked_file_tokens
}

pub(crate) fn emit_preview_registry_stmt(
    roots: &[CompiledNode],
    span: Span,
) -> Result<TokenStream2, syn::Error> {
    if !cfg!(feature = "preview") {
        return Ok(quote! {});
    }
    let mut root_idents = Vec::new();
    for node in roots {
        collect_free_idents_from_node(node, span, &[], &mut root_idents)?;
    }
    let root_ident_strs: Vec<String> = root_idents.iter().map(ToString::to_string).collect();
    Ok(quote! {{
        #[allow(unused_imports)]
        use ::krui::RegistryFieldFallback as _;
        let mut __reg = ::krui::ScopeRegistry::new();
        #(
            __reg.insert_probed_field(
                #root_ident_strs,
                (&::krui::ScopeFieldProbe(&#root_idents)).probe_field(),
            );
        )*
        __tree.set_preview_registry(::std::rc::Rc::new(__reg));
    }})
}

pub(crate) fn emit_compile_warning_tokens(
    idx: usize,
    warning: &CompileWarning,
    span: Span,
) -> TokenStream2 {
    let fn_ident = Ident::new(&format!("__krui_performance_cliff_warning_{idx}"), span);
    let note = &warning.message;
    quote! { #[deprecated(note = #note)] #[allow(non_snake_case)] fn #fn_ident() {} #fn_ident(); }
}

pub(crate) fn format_path_expr(
    parent_path_expr: &TokenStream2,
    for_key_ident: Option<&Ident>,
    sibling_idx: usize,
    tag_str: &str,
    element_id: Option<&str>,
    suffix: &str,
) -> TokenStream2 {
    let tail = match element_id {
        Some(id) => format!("{sibling_idx}:{tag_str}#{id}{suffix}"),
        None => format!("{sibling_idx}:{tag_str}{suffix}"),
    };
    match for_key_ident {
        Some(fk) => quote! { ::std::format!("{}/for[{}]/{}", #parent_path_expr, #fk, #tail) },
        None => quote! { ::std::format!("{}/{}", #parent_path_expr, #tail) },
    }
}
