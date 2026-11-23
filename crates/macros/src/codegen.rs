//! Procedural macro code generation for `CompiledDocument` and Stylo-backed `ViewTree` nodes.

use crate::codegen_control_flow::{
    emit_class_binding, emit_for_construction, emit_if_construction, emit_interpolated_slot,
};
use crate::codegen_events::emit_element_event_bindings;
use crate::codegen_slots::{
    emit_inline_style_binding, emit_layout_slot_bindings, emit_paint_slot_bindings,
    emit_static_and_dynamic_attr_bindings,
};
use crate::expr_vars::collect_free_idents_from_node;
use crate::select_bindings::emit_select_children_binding;
use krui_compiler::{
    CompileWarning, CompileWarningKind, CompiledButtonType, CompiledDocument, CompiledInputType,
    CompiledNode, CompiledNodeKind, HtmlTag,
};
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use std::path::Path;

/// Lowers a [`CompiledDocument`] into a `krui::ViewFragment` token stream.
pub fn expand_compiled_doc(doc: &CompiledDocument, span: Span) -> Result<TokenStream2, syn::Error> {
    let tracked_file_tokens = emit_tracked_file_tokens(doc);
    let spawn_helper = emit_spawn_node_helper();
    let warning_tokens: Vec<TokenStream2> = doc
        .warnings
        .iter()
        .filter(|w| w.kind == CompileWarningKind::LayoutPerformanceCliff && !w.suppressed)
        .enumerate()
        .map(|(idx, w)| emit_compile_warning_tokens(idx, w, span))
        .collect();
    let doc_setup_stmt = emit_document_setup_stmts(doc);
    let preview_registry_stmt = emit_preview_registry_stmt(&doc.roots, span)?;
    let source_base_dir = doc.source_path.as_deref().and_then(Path::parent);
    let node_builders: Vec<TokenStream2> = doc
        .roots
        .iter()
        .enumerate()
        .map(|(idx, n)| {
            emit_node_construction(n, source_base_dir, span, &quote!("root"), None, idx, &[])
        })
        .collect::<Result<_, _>>()?;
    Ok(quote! {{
        #(#tracked_file_tokens)* #spawn_helper #(#warning_tokens)*
        ::krui::ViewFragment::new(move |__tree: &mut ::krui::ViewTree, __parent: Option<::krui::NodeId>| {
            let __is_top_mount = __parent.is_none();
            if __is_top_mount {
                __tree.begin_mount_batch();
                #doc_setup_stmt
            }
            #preview_registry_stmt
            let mut __root_ids = ::std::vec::Vec::new();
            #( let __id = #node_builders; __root_ids.push(__id); )*
            if __is_top_mount {
                __tree.end_mount_batch();
            }
            __root_ids
        })
    }})
}

fn emit_spawn_node_helper() -> TokenStream2 {
    quote! {
        #[inline(never)]
        #[allow(clippy::too_many_arguments)]
        fn __krui_spawn_node(
            __tree: &mut ::krui::ViewTree,
            __parent: ::std::option::Option<::krui::NodeId>,
            __tag: &'static str,
            __id: ::std::option::Option<&'static str>,
            __aria: ::std::option::Option<&'static str>,
            __flags: (bool, bool, bool, bool),
        ) -> ::krui::NodeId {
            __tree.create_element_node(
                __parent,
                __tag,
                __id.map(str::to_string),
                __aria.map(str::to_string),
                __flags.0,
                __flags.1,
                __flags.2,
                __flags.3,
            )
        }
    }
}

fn emit_document_setup_stmts(doc: &CompiledDocument) -> TokenStream2 {
    let unstyled = doc.unstyled;
    let font_stmts: Vec<TokenStream2> = doc
        .embedded_font_files
        .iter()
        .map(|(key, path)| {
            let p = path.to_string_lossy().into_owned();
            quote! {
                __tree.register_embedded_font(
                    #key,
                    ::std::sync::Arc::from(include_bytes!(#p).as_slice()),
                );
            }
        })
        .collect();
    let sheets = &doc.stylesheets;
    let sheets_stmt = if sheets.is_empty() {
        quote! { __tree.set_document_stylesheets(&[]); }
    } else {
        quote! { __tree.set_document_stylesheets(&[#(#sheets),*]); }
    };
    quote! {
        __tree.set_unstyled(#unstyled);
        #(#font_stmts)*
        #sheets_stmt
    }
}

fn emit_tracked_file_tokens(doc: &CompiledDocument) -> Vec<TokenStream2> {
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

fn emit_preview_registry_stmt(
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

fn emit_compile_warning_tokens(idx: usize, warning: &CompileWarning, span: Span) -> TokenStream2 {
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

pub(crate) fn emit_node_construction(
    node: &CompiledNode,
    source_base_dir: Option<&Path>,
    span: Span,
    parent_path_expr: &TokenStream2,
    for_key_ident: Option<&Ident>,
    sibling_idx: usize,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    match &node.kind {
        CompiledNodeKind::If(cif) => emit_if_construction(
            cif,
            source_base_dir,
            span,
            parent_path_expr,
            for_key_ident,
            sibling_idx,
            loop_vars,
        ),
        CompiledNodeKind::For(cfor) => emit_for_construction(
            cfor,
            source_base_dir,
            span,
            parent_path_expr,
            for_key_ident,
            sibling_idx,
            loop_vars,
        ),
        CompiledNodeKind::Element => emit_element_construction(
            node,
            source_base_dir,
            span,
            parent_path_expr,
            for_key_ident,
            sibling_idx,
            loop_vars,
        ),
    }
}

fn emit_element_path_lets(
    node: &CompiledNode,
    parent_path_expr: &TokenStream2,
    for_key_ident: Option<&Ident>,
    sibling_idx: usize,
    path_ident: &Ident,
) -> (TokenStream2, TokenStream2) {
    if cfg!(feature = "preview") {
        let path_expr = format_path_expr(
            parent_path_expr,
            for_key_ident,
            sibling_idx,
            node.tag.as_str(),
            node.id.as_deref(),
            "",
        );
        let (src_line, src_col) = (node.source_line, node.source_column);
        (
            quote! { #[allow(unused_variables)] let #path_ident = #path_expr; },
            quote! {
                __tree.set_node_preview_path_key(__node_id, #path_ident.clone());
                __tree.set_node_source_span(__node_id, #src_line, #src_col);
            },
        )
    } else {
        (
            quote! { #[allow(unused_variables)] let #path_ident = ""; },
            quote! {},
        )
    }
}

fn emit_element_construction(
    node: &CompiledNode,
    source_base_dir: Option<&Path>,
    span: Span,
    parent_path_expr: &TokenStream2,
    for_key_ident: Option<&Ident>,
    sibling_idx: usize,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let path_ident = format_ident!("__elem_path_{sibling_idx}");
    let (path_let_stmt, set_preview_stmt) = emit_element_path_lets(
        node,
        parent_path_expr,
        for_key_ident,
        sibling_idx,
        &path_ident,
    );
    let create_call = emit_create_element_call(node);
    let attr_stmts = emit_static_and_dynamic_attr_bindings(node, span, loop_vars)?;
    let inline_style_stmt = emit_inline_style_binding(node, span, loop_vars)?;
    let paint_binding_stmts = emit_paint_slot_bindings(node, span, loop_vars)?;
    let layout_binding_stmts = emit_layout_slot_bindings(node, span, loop_vars)?;
    let slot_stmts = emit_element_text_and_class_slots(node, source_base_dir, span, loop_vars)?;
    let event_stmts = emit_element_event_bindings(node, span, loop_vars)?;
    let synthetic_stmt = if node.is_synthetic_text_run {
        quote! { __tree.mark_synthetic_text_run(__node_id); }
    } else {
        quote! {}
    };
    let mut child_stmts = Vec::with_capacity(node.children.len());
    for (ch_idx, child) in node.children.iter().enumerate() {
        let child_expr = emit_node_construction(
            child,
            source_base_dir,
            span,
            &quote! { #path_ident },
            None,
            ch_idx,
            loop_vars,
        )?;
        child_stmts.push(quote! { let __parent = Some(__node_id); let _ = #child_expr; });
    }
    Ok(quote! {{
        #path_let_stmt
        let __node_id = #create_call;
        #set_preview_stmt #synthetic_stmt
        #attr_stmts #inline_style_stmt #slot_stmts
        #(#paint_binding_stmts)* #(#layout_binding_stmts)* #event_stmts #(#child_stmts)*
        __node_id
    }})
}

fn emit_create_element_call(node: &CompiledNode) -> TokenStream2 {
    let tag_str = node.tag.as_str();
    let (is_button, is_input, is_canvas, is_secret) = (
        node.tag == HtmlTag::Button,
        node.tag == HtmlTag::Input,
        node.tag == HtmlTag::Canvas,
        node.is_secret_input,
    );
    let id_expr = match &node.id {
        Some(s) => quote! { ::std::option::Option::Some(#s) },
        None => quote! { ::std::option::Option::None },
    };
    let aria_expr = match &node.aria_label {
        Some(s) => quote! { ::std::option::Option::Some(#s) },
        None => quote! { ::std::option::Option::None },
    };
    quote! {
        __krui_spawn_node(
            __tree, __parent, #tag_str, #id_expr, #aria_expr,
            (#is_button, #is_input, #is_canvas, #is_secret),
        )
    }
}

fn emit_control_meta_slots(node: &CompiledNode) -> TokenStream2 {
    let label_for_stmt = match &node.label_for {
        Some(target_id) => quote! { __tree.set_label_for(__node_id, #target_id); },
        None => quote! {},
    };
    let control_name_stmt = match &node.control_name {
        Some(name) => quote! { __tree.set_control_name(__node_id, #name); },
        None => quote! {},
    };
    let button_kind_stmt = match node.button_type {
        CompiledButtonType::Auto => quote! {},
        CompiledButtonType::Button => {
            quote! { __tree.set_button_kind(__node_id, ::krui::ButtonKind::Button); }
        }
        CompiledButtonType::Submit => {
            quote! { __tree.set_button_kind(__node_id, ::krui::ButtonKind::Submit); }
        }
        CompiledButtonType::Reset => {
            quote! { __tree.set_button_kind(__node_id, ::krui::ButtonKind::Reset); }
        }
    };
    let select_opt_stmts: Vec<TokenStream2> = if node.select_children.is_empty() {
        node.select_options
            .iter()
            .map(|opt| {
                let (v, l, sel) = (&opt.value, &opt.label, opt.selected);
                quote! { __tree.add_select_option(__node_id, #v, #l, #sel); }
            })
            .collect()
    } else {
        Vec::new()
    };
    let input_kind_stmt = emit_input_kind_stmt(node);
    quote! { #label_for_stmt #control_name_stmt #button_kind_stmt #(#select_opt_stmts)* #input_kind_stmt }
}

fn emit_input_kind_stmt(node: &CompiledNode) -> TokenStream2 {
    if node.tag != HtmlTag::Input {
        return quote! {};
    }
    match node.input_type {
        CompiledInputType::Checkbox => quote! { __tree.set_checkbox_role(__node_id); },
        CompiledInputType::Radio => quote! { __tree.set_radio_role(__node_id); },
        CompiledInputType::Range => {
            let (min, max, step) = node.range_bounds.unwrap_or((0.0, 100.0, 1.0));
            quote! { __tree.set_range_bounds(__node_id, #min, #max, #step); }
        }
        _ => {
            let kind_tok = match node.input_type {
                CompiledInputType::Password => quote! { ::krui::InputKind::Password },
                CompiledInputType::Number => quote! { ::krui::InputKind::Number },
                CompiledInputType::Email => quote! { ::krui::InputKind::Email },
                CompiledInputType::Search => quote! { ::krui::InputKind::Search },
                _ => quote! { ::krui::InputKind::Text },
            };
            quote! { __tree.set_input_kind(__node_id, #kind_tok); }
        }
    }
}

fn emit_element_text_and_class_slots(
    node: &CompiledNode,
    source_base_dir: Option<&Path>,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let meta_stmt = emit_control_meta_slots(node);
    let dyn_select_stmt = emit_select_children_binding(node, span, loop_vars)?;
    let rich_text_stmt = crate::codegen_rich_text::emit_rich_text_slot(node, span, loop_vars)?;
    let text_stmt = emit_interpolated_slot(
        &node.text_parts,
        span,
        format_ident!("set_static_text"),
        format_ident!("bind_dynamic_text"),
        loop_vars,
    )?;
    let placeholder_stmt = emit_interpolated_slot(
        &node.placeholder_parts,
        span,
        format_ident!("set_placeholder"),
        format_ident!("bind_dynamic_placeholder"),
        loop_vars,
    )?;
    let aria_stmt = emit_interpolated_slot(
        &node.aria_label_parts,
        span,
        format_ident!("set_aria_label"),
        format_ident!("bind_dynamic_aria_label"),
        loop_vars,
    )?;
    let title_stmt = emit_interpolated_slot(
        &node.title_parts,
        span,
        format_ident!("set_title"),
        format_ident!("bind_dynamic_title"),
        loop_vars,
    )?;
    let class_stmt = emit_class_binding(node, span, loop_vars)?;
    let media_stmt =
        crate::media_codegen::emit_media_slots(node, source_base_dir, span, loop_vars)?;
    Ok(quote! {
        #meta_stmt #dyn_select_stmt #rich_text_stmt #text_stmt #placeholder_stmt #aria_stmt #title_stmt #class_stmt #media_stmt
    })
}
