//! D1 Static UI descriptor (`.rdata` slice) + flat dynamic binding codegen for `krui!`/`include_krui!`.

use crate::codegen::{
    emit_compile_warning_tokens, emit_preview_registry_stmt, emit_tracked_file_tokens,
    format_path_expr,
};
use crate::codegen_control_flow::{
    emit_class_binding, emit_for_at_anchor, emit_if_at_anchor, emit_interpolated_slot,
};
use crate::codegen_events::emit_dynamic_element_event_bindings;
use crate::codegen_rich_text::emit_rich_text_slot;
use crate::codegen_slots::{
    emit_dynamic_attr_bindings, emit_dynamic_inline_style_binding, emit_layout_slot_bindings,
    emit_paint_slot_bindings,
};
use crate::media_codegen::emit_dynamic_media_slots;
use crate::select_bindings::emit_select_children_binding;
use krui_compiler::{
    CompileWarningKind, CompiledBoolAttr, CompiledButtonType, CompiledDocument, CompiledInputType,
    CompiledNode, CompiledNodeKind, CompiledTextPart, HtmlTag,
};
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use std::path::Path;

struct FlatSlot<'a> {
    slot: usize,
    parent_slot: Option<usize>,
    sibling_idx: usize,
    node: &'a CompiledNode,
}

/// Lowers a [`CompiledDocument`] into static `.rdata` descriptors + flat slot attachments.
pub(crate) fn expand_compiled_doc_static(
    doc: &CompiledDocument,
    span: Span,
) -> Result<TokenStream2, syn::Error> {
    let tracked_file_tokens = emit_tracked_file_tokens(doc);
    let warning_tokens: Vec<TokenStream2> = doc
        .warnings
        .iter()
        .filter(|w| w.kind == CompileWarningKind::LayoutPerformanceCliff && !w.suppressed)
        .enumerate()
        .map(|(idx, w)| emit_compile_warning_tokens(idx, w, span))
        .collect();
    let preview_registry_stmt = emit_preview_registry_stmt(&doc.roots, span)?;
    let font_stmts = emit_embedded_font_stmts(doc);
    let source_base_dir = doc.source_path.as_deref().and_then(Path::parent);
    let (node_descs, runtime_stmts, root_slots) = build_flat_block(
        &doc.roots,
        source_base_dir,
        span,
        &quote!("root"),
        None,
        &[],
    )?;
    let unstyled = doc.unstyled;
    let sheets = &doc.stylesheets;
    Ok(quote! {{
        #(#tracked_file_tokens)* #(#warning_tokens)*
        static __KRUI_SHEETS: &[&str] = &[#(#sheets),*];
        static __KRUI_NODES: &[::krui::__private::core::static_ir::StaticNodeDesc] = &[
            #(#node_descs),*
        ];
        static __KRUI_DOC: ::krui::__private::core::static_ir::StaticDocumentDesc =
            ::krui::__private::core::static_ir::StaticDocumentDesc {
                unstyled: #unstyled,
                stylesheets: __KRUI_SHEETS,
                nodes: __KRUI_NODES,
            };
        ::krui::ViewFragment::new(move |__tree: &mut ::krui::ViewTree, __parent: Option<::krui::NodeId>| {
            let __is_top_mount = __parent.is_none();
            if __is_top_mount {
                __tree.begin_mount_batch();
                #(#font_stmts)*
            }
            #preview_registry_stmt
            let __slots = ::krui::__private::core::static_ir::mount_static_document(
                __tree,
                __parent,
                &__KRUI_DOC,
                __is_top_mount,
            );
            #(#runtime_stmts)*
            if __is_top_mount {
                __tree.end_mount_batch();
            }
            ::std::vec![#(__slots[#root_slots]),*]
        })
    }})
}

/// Emits D1 static descriptor mounting + flat dynamic attachments for a sub-block (`<if>`/`<for>`).
pub(crate) fn emit_static_subblock_body(
    nodes: &[CompiledNode],
    source_base_dir: Option<&Path>,
    span: Span,
    parent_path_expr: &TokenStream2,
    for_key_ident: Option<&Ident>,
    loop_vars: &[String],
) -> Result<(TokenStream2, Vec<usize>), syn::Error> {
    let (node_descs, runtime_stmts, root_slots) = build_flat_block(
        nodes,
        source_base_dir,
        span,
        parent_path_expr,
        for_key_ident,
        loop_vars,
    )?;
    let body = quote! {
        static __KRUI_SUB_NODES: &[::krui::__private::core::static_ir::StaticNodeDesc] = &[
            #(#node_descs),*
        ];
        let __slots = ::krui::__private::core::static_ir::mount_static_nodes(
            __tree,
            __parent,
            __KRUI_SUB_NODES,
        );
        #(#runtime_stmts)*
    };
    Ok((body, root_slots))
}

fn emit_embedded_font_stmts(doc: &CompiledDocument) -> Vec<TokenStream2> {
    doc.embedded_font_files
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
        .collect()
}

type FlatBlockOutput = (Vec<TokenStream2>, Vec<TokenStream2>, Vec<usize>);

fn build_flat_block(
    roots: &[CompiledNode],
    source_base_dir: Option<&Path>,
    span: Span,
    root_path_expr: &TokenStream2,
    for_key_ident: Option<&Ident>,
    loop_vars: &[String],
) -> Result<FlatBlockOutput, syn::Error> {
    let mut flat = Vec::new();
    for (idx, root) in roots.iter().enumerate() {
        flatten_preorder(root, None, idx, &mut flat);
    }
    let root_slots: Vec<usize> = flat
        .iter()
        .filter(|e| e.parent_slot.is_none())
        .map(|e| e.slot)
        .collect();
    let descs = flat.iter().map(emit_static_node_desc).collect();
    let mut runtime = Vec::new();
    for entry in &flat {
        emit_slot_runtime_stmts(
            entry,
            source_base_dir,
            span,
            root_path_expr,
            for_key_ident,
            loop_vars,
            &mut runtime,
        )?;
    }
    Ok((descs, runtime, root_slots))
}

fn flatten_preorder<'a>(
    node: &'a CompiledNode,
    parent_slot: Option<usize>,
    sibling_idx: usize,
    out: &mut Vec<FlatSlot<'a>>,
) {
    let slot = out.len();
    out.push(FlatSlot {
        slot,
        parent_slot,
        sibling_idx,
        node,
    });
    if matches!(node.kind, CompiledNodeKind::Element) {
        for (idx, child) in node.children.iter().enumerate() {
            flatten_preorder(child, Some(slot), idx, out);
        }
    }
}

fn emit_static_node_desc(entry: &FlatSlot<'_>) -> TokenStream2 {
    let n = entry.node;
    let p_tok = match entry.parent_slot {
        Some(p) => quote!(::std::option::Option::Some(#p)),
        None => quote!(::std::option::Option::None),
    };
    if !matches!(n.kind, CompiledNodeKind::Element) {
        return quote! {
            ::krui::__private::core::static_ir::StaticNodeDesc {
                parent_slot: #p_tok,
                ..::krui::__private::core::static_ir::StaticNodeDesc::ANCHOR
            }
        };
    }
    let fields = collect_element_desc_fields(n, p_tok);
    quote! {
        ::krui::__private::core::static_ir::StaticNodeDesc {
            #(#fields,)*
            ..::krui::__private::core::static_ir::StaticNodeDesc::BLANK
        }
    }
}

fn collect_element_desc_fields(n: &CompiledNode, parent_tok: TokenStream2) -> Vec<TokenStream2> {
    let tag_str = n.tag.as_str();
    let mut f = vec![quote!(parent_slot: #parent_tok), quote!(tag: #tag_str)];
    push_opt_str(&mut f, "custom_tag", n.custom_tag.as_deref());
    push_opt_str(&mut f, "id", n.id.as_deref());
    push_opt_str(&mut f, "aria_label", n.aria_label.as_deref());
    let static_aria = static_only_parts(&n.aria_label_parts);
    push_opt_str(&mut f, "static_aria_label_slot", static_aria.as_deref());
    if n.tag == HtmlTag::Button {
        f.push(quote!(is_button: true));
    }
    if n.tag == HtmlTag::Input {
        f.push(quote!(is_input: true));
    }
    if n.tag == HtmlTag::Canvas {
        f.push(quote!(is_canvas: true));
    }
    if n.is_secret_input {
        f.push(quote!(is_secret_input: true));
    }
    if n.is_synthetic_text_run {
        f.push(quote!(is_synthetic_text_run: true));
    }
    push_static_bool(&mut f, "disabled", &n.disabled);
    push_static_bool(&mut f, "required", &n.required);
    push_static_bool(&mut f, "checked", &n.checked);
    if !has_dyn_parts(&n.class_parts) && !n.classes.is_empty() {
        let cls = &n.classes;
        f.push(quote!(classes: &[#(#cls),*]));
    }
    if !n.static_attrs.is_empty() {
        let pairs = n.static_attrs.iter().map(|(k, v)| {
            quote!(::krui::__private::core::static_ir::StaticAttrDesc { name: #k, value: #v })
        });
        f.push(quote!(attrs: &[#(#pairs),*]));
    }
    if n.dynamic_inline_style_parts.is_empty() {
        push_opt_str(&mut f, "inline_style", n.static_inline_style.as_deref());
    }
    push_control_desc_fields(&mut f, n);
    let st = static_only_parts(&n.text_parts);
    push_opt_str(&mut f, "static_text", st.as_deref());
    let sph = static_only_parts(&n.placeholder_parts);
    push_opt_str(&mut f, "placeholder", sph.as_deref());
    let stl = static_only_parts(&n.title_parts);
    push_opt_str(&mut f, "title", stl.as_deref());
    let slh = static_only_parts(&n.link_href);
    push_opt_str(&mut f, "link_href", slh.as_deref());
    if cfg!(feature = "preview") {
        let (line, col) = (n.source_line, n.source_column);
        f.push(quote!(source_line: #line));
        f.push(quote!(source_column: #col));
    }
    f
}

fn push_control_desc_fields(f: &mut Vec<TokenStream2>, n: &CompiledNode) {
    push_opt_str(f, "label_for", n.label_for.as_deref());
    push_opt_str(f, "control_name", n.control_name.as_deref());
    match n.button_type {
        CompiledButtonType::Auto => {}
        CompiledButtonType::Button => {
            f.push(
                quote!(button_kind: ::krui::__private::core::static_ir::StaticButtonKind::Button),
            );
        }
        CompiledButtonType::Submit => {
            f.push(
                quote!(button_kind: ::krui::__private::core::static_ir::StaticButtonKind::Submit),
            );
        }
        CompiledButtonType::Reset => {
            f.push(
                quote!(button_kind: ::krui::__private::core::static_ir::StaticButtonKind::Reset),
            );
        }
    }
    if n.select_children.is_empty() && !n.select_options.is_empty() {
        let opts = n.select_options.iter().map(|opt| {
            let (v, l, sel) = (&opt.value, &opt.label, opt.selected);
            quote!(::krui::__private::core::static_ir::StaticSelectOptDesc { value: #v, label: #l, selected: #sel })
        });
        f.push(quote!(select_options: &[#(#opts),*]));
    }
    if n.tag == HtmlTag::Input {
        let ik = match n.input_type {
            CompiledInputType::Checkbox => {
                quote!(::krui::__private::core::static_ir::StaticInputKind::Checkbox)
            }
            CompiledInputType::Radio => {
                quote!(::krui::__private::core::static_ir::StaticInputKind::Radio)
            }
            CompiledInputType::Range => {
                let (min, max, step) = n.range_bounds.unwrap_or((0.0, 100.0, 1.0));
                quote!(::krui::__private::core::static_ir::StaticInputKind::Range(#min, #max, #step))
            }
            CompiledInputType::Password => {
                quote!(::krui::__private::core::static_ir::StaticInputKind::Password)
            }
            CompiledInputType::Number => {
                quote!(::krui::__private::core::static_ir::StaticInputKind::Number)
            }
            CompiledInputType::Email => {
                quote!(::krui::__private::core::static_ir::StaticInputKind::Email)
            }
            CompiledInputType::Search => {
                quote!(::krui::__private::core::static_ir::StaticInputKind::Search)
            }
            _ => quote!(::krui::__private::core::static_ir::StaticInputKind::Text),
        };
        f.push(quote!(input_kind: #ik));
    }
}

fn emit_slot_runtime_stmts(
    entry: &FlatSlot<'_>,
    source_base_dir: Option<&Path>,
    span: Span,
    root_path_expr: &TokenStream2,
    for_key_ident: Option<&Ident>,
    loop_vars: &[String],
    out: &mut Vec<TokenStream2>,
) -> Result<(), syn::Error> {
    let parent_path_expr = match entry.parent_slot {
        Some(p) => {
            let pid = format_ident!("__krui_slot_path_{p}");
            quote!(#pid)
        }
        None => root_path_expr.clone(),
    };
    let fk = if entry.parent_slot.is_none() {
        for_key_ident
    } else {
        None
    };
    let parent_node_expr = match entry.parent_slot {
        Some(p) => quote!(::std::option::Option::Some(__slots[#p])),
        None => quote!(__parent),
    };
    let slot_idx = entry.slot;
    match &entry.node.kind {
        CompiledNodeKind::If(cif) => {
            let anchor_tok = quote!(__slots[#slot_idx]);
            out.push(emit_if_at_anchor(
                cif,
                source_base_dir,
                span,
                (&parent_path_expr, fk, entry.sibling_idx),
                loop_vars,
                Some((anchor_tok, parent_node_expr)),
            )?);
            return Ok(());
        }
        CompiledNodeKind::For(cfor) => {
            let anchor_tok = quote!(__slots[#slot_idx]);
            out.push(emit_for_at_anchor(
                cfor,
                source_base_dir,
                span,
                (&parent_path_expr, fk, entry.sibling_idx),
                loop_vars,
                Some((anchor_tok, parent_node_expr)),
            )?);
            return Ok(());
        }
        CompiledNodeKind::Element => {}
    }
    emit_element_dynamic_stmts(
        entry,
        source_base_dir,
        span,
        &parent_path_expr,
        fk,
        loop_vars,
        out,
    )
}

fn emit_element_dynamic_stmts(
    entry: &FlatSlot<'_>,
    source_base_dir: Option<&Path>,
    span: Span,
    parent_path_expr: &TokenStream2,
    fk: Option<&Ident>,
    loop_vars: &[String],
    out: &mut Vec<TokenStream2>,
) -> Result<(), syn::Error> {
    let n = entry.node;
    let slot_idx = entry.slot;
    if cfg!(feature = "preview") {
        let p_ident = format_ident!("__krui_slot_path_{slot_idx}");
        let p_expr = format_path_expr(
            parent_path_expr,
            fk,
            entry.sibling_idx,
            n.tag.as_str(),
            n.id.as_deref(),
            "",
        );
        out.push(quote! {
            let #p_ident = #p_expr;
            __tree.set_node_preview_path_key(__slots[#slot_idx], #p_ident.clone());
        });
    }
    let dyn_attrs = emit_dynamic_attr_bindings(n, span, loop_vars)?;
    let dyn_style = emit_dynamic_inline_style_binding(n, span, loop_vars)?;
    let paint = emit_paint_slot_bindings(n, span, loop_vars)?;
    let layout = emit_layout_slot_bindings(n, span, loop_vars)?;
    let dyn_select = emit_select_children_binding(n, span, loop_vars)?;
    let rich = emit_rich_text_slot(n, span, loop_vars)?;
    let dyn_text = emit_dyn_interp(
        n.text_parts.as_slice(),
        span,
        "set_static_text",
        "bind_dynamic_text",
        loop_vars,
    )?;
    let dyn_ph = emit_dyn_interp(
        n.placeholder_parts.as_slice(),
        span,
        "set_placeholder",
        "bind_dynamic_placeholder",
        loop_vars,
    )?;
    let dyn_aria = emit_dyn_interp(
        n.aria_label_parts.as_slice(),
        span,
        "set_aria_label",
        "bind_dynamic_aria_label",
        loop_vars,
    )?;
    let dyn_title = emit_dyn_interp(
        n.title_parts.as_slice(),
        span,
        "set_title",
        "bind_dynamic_title",
        loop_vars,
    )?;
    let dyn_class = if has_dyn_parts(&n.class_parts) {
        emit_class_binding(n, span, loop_vars)?
    } else {
        quote! {}
    };
    let media = emit_dynamic_media_slots(n, source_base_dir, span, loop_vars)?;
    let events = emit_dynamic_element_event_bindings(n, span, loop_vars)?;
    let all_tokens = quote! {
        #(#dyn_attrs)* #dyn_style #dyn_select #rich
        #dyn_text #dyn_ph #dyn_aria #dyn_title #dyn_class #media
        #(#paint)* #(#layout)* #events
    };
    if !all_tokens.is_empty() {
        out.push(quote! {{
            let __node_id = __slots[#slot_idx];
            #all_tokens
        }});
    }
    Ok(())
}

fn emit_dyn_interp(
    parts: &[CompiledTextPart],
    span: Span,
    static_m: &str,
    dyn_m: &str,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    if !has_dyn_parts(parts) {
        return Ok(quote! {});
    }
    emit_interpolated_slot(
        parts,
        span,
        format_ident!("{static_m}"),
        format_ident!("{dyn_m}"),
        loop_vars,
    )
}

fn has_dyn_parts(parts: &[CompiledTextPart]) -> bool {
    parts
        .iter()
        .any(|p| matches!(p, CompiledTextPart::Dynamic { .. }))
}

fn static_only_parts(parts: &[CompiledTextPart]) -> Option<String> {
    if parts.is_empty() || has_dyn_parts(parts) {
        return None;
    }
    let mut out = String::new();
    for p in parts {
        if let CompiledTextPart::Static(s) = p {
            out.push_str(s);
        }
    }
    Some(out)
}

fn push_opt_str(f: &mut Vec<TokenStream2>, field: &str, val: Option<&str>) {
    if let Some(s) = val {
        let ident = format_ident!("{field}");
        f.push(quote!(#ident: ::std::option::Option::Some(#s)));
    }
}

fn push_static_bool(f: &mut Vec<TokenStream2>, field: &str, opt: &Option<CompiledBoolAttr>) {
    if let Some(CompiledBoolAttr::Static(b)) = opt {
        let ident = format_ident!("{field}");
        f.push(quote!(#ident: ::std::option::Option::Some(#b)));
    }
}
