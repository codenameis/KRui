//! Reactive paint/layout slot, dynamic inline `style="..."`, and HTML attribute codegen.

use crate::expr_vars::{
    collect_free_idents_from_template_expr, emit_binding_expr, emit_clone_lets, emit_template_expr,
    is_loop_var,
};
use krui_compiler::{CompiledNode, CompiledTextPart, LayoutSlot, PaintSlot, TemplateExpr};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;

pub(crate) fn emit_paint_slot_bindings(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<Vec<TokenStream2>, syn::Error> {
    let mut out = Vec::with_capacity(node.paint_bindings.len());
    for b in &node.paint_bindings {
        let is_direct_sig = match &b.expr {
            TemplateExpr::Path(segs) => !is_loop_var(&segs[0], loop_vars),
            _ => false,
        };
        if is_direct_sig || b.slot == PaintSlot::FastTransform {
            let sig_expr = emit_binding_expr(&b.expr, span, loop_vars)?;
            out.push(emit_direct_paint_slot_stmt(&b.slot, &sig_expr));
        } else {
            let expr_toks = emit_template_expr(&b.expr, span, loop_vars)?;
            let mut idents = Vec::new();
            collect_free_idents_from_template_expr(&b.expr, span, loop_vars, &mut idents)?;
            let clones = emit_clone_lets(&idents);
            let eval_fn = quote! {
                move || ::krui::IntoKruiTemplateScalar::into_krui_scalar(&(#expr_toks)).as_f32()
            };
            let apply_tok = emit_paint_eval_mutator(&b.slot);
            out.push(quote! {{
                #clones
                __tree.bind_paint_eval(__node_id, #eval_fn, #apply_tok);
            }});
        }
    }
    Ok(out)
}

fn emit_direct_paint_slot_stmt(slot: &PaintSlot, sig_expr: &TokenStream2) -> TokenStream2 {
    match slot {
        PaintSlot::TranslateX => quote!(__tree.bind_paint_translate_x(__node_id, #sig_expr);),
        PaintSlot::TranslateY => quote!(__tree.bind_paint_translate_y(__node_id, #sig_expr);),
        PaintSlot::ScaleX => quote!(__tree.bind_paint_scale_x(__node_id, #sig_expr);),
        PaintSlot::ScaleY => quote!(__tree.bind_paint_scale_y(__node_id, #sig_expr);),
        PaintSlot::ScaleUniform => quote!(__tree.bind_paint_scale_uniform(__node_id, #sig_expr);),
        PaintSlot::RotateDeg => quote!(__tree.bind_paint_rotate_deg(__node_id, #sig_expr);),
        PaintSlot::Opacity => quote!(__tree.bind_paint_opacity(__node_id, #sig_expr);),
        PaintSlot::FastTransform => quote!(__tree.bind_paint_fast_transform(__node_id, #sig_expr);),
    }
}

fn emit_paint_eval_mutator(slot: &PaintSlot) -> TokenStream2 {
    match slot {
        PaintSlot::TranslateX => quote!(|p, v| p.transform.translate_x = v),
        PaintSlot::TranslateY => quote!(|p, v| p.transform.translate_y = v),
        PaintSlot::ScaleX => quote!(|p, v| p.transform.scale_x = v),
        PaintSlot::ScaleY => quote!(|p, v| p.transform.scale_y = v),
        PaintSlot::ScaleUniform => quote!(|p, v| {
            p.transform.scale_x = v;
            p.transform.scale_y = v;
        }),
        PaintSlot::RotateDeg => quote!(|p, v| p.transform.rotate_deg = v),
        PaintSlot::Opacity | PaintSlot::FastTransform => {
            quote!(|p, v| p.opacity = v.clamp(0.0, 1.0))
        }
    }
}

pub(crate) fn emit_layout_slot_bindings(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<Vec<TokenStream2>, syn::Error> {
    let mut out = Vec::with_capacity(node.layout_bindings.len());
    for b in &node.layout_bindings {
        let is_direct_sig = match &b.expr {
            TemplateExpr::Path(segs) => !is_loop_var(&segs[0], loop_vars),
            _ => false,
        };
        if is_direct_sig {
            let sig_expr = emit_binding_expr(&b.expr, span, loop_vars)?;
            out.push(emit_direct_layout_slot_stmt(&b.slot, &sig_expr));
        } else {
            let expr_toks = emit_template_expr(&b.expr, span, loop_vars)?;
            let mut idents = Vec::new();
            collect_free_idents_from_template_expr(&b.expr, span, loop_vars, &mut idents)?;
            let clones = emit_clone_lets(&idents);
            let eval_fn = quote! {
                move || ::krui::IntoKruiTemplateScalar::into_krui_scalar(&(#expr_toks)).as_f32()
            };
            let mutate_tok = emit_layout_eval_mutator(&b.slot);
            out.push(quote! {{
                #clones
                __tree.bind_layout_eval(__node_id, #eval_fn, #mutate_tok);
            }});
        }
    }
    Ok(out)
}

fn emit_direct_layout_slot_stmt(slot: &LayoutSlot, sig_expr: &TokenStream2) -> TokenStream2 {
    match slot {
        LayoutSlot::WidthPx => quote!(__tree.bind_layout_width_px(__node_id, #sig_expr);),
        LayoutSlot::HeightPx => quote!(__tree.bind_layout_height_px(__node_id, #sig_expr);),
        LayoutSlot::TopPx => quote!(__tree.bind_layout_top_px(__node_id, #sig_expr);),
        LayoutSlot::LeftPx => quote!(__tree.bind_layout_left_px(__node_id, #sig_expr);),
        LayoutSlot::RightPx => quote!(__tree.bind_layout_right_px(__node_id, #sig_expr);),
        LayoutSlot::BottomPx => quote!(__tree.bind_layout_bottom_px(__node_id, #sig_expr);),
        LayoutSlot::MarginPx => quote!(__tree.bind_layout_margin_px(__node_id, #sig_expr);),
        LayoutSlot::PaddingPx => quote!(__tree.bind_layout_padding_px(__node_id, #sig_expr);),
        LayoutSlot::GapPx => quote!(__tree.bind_layout_gap_px(__node_id, #sig_expr);),
    }
}

fn emit_layout_eval_mutator(slot: &LayoutSlot) -> TokenStream2 {
    match slot {
        LayoutSlot::WidthPx => quote!(|l, v| l.width = ::krui::Dimension::Px(v)),
        LayoutSlot::HeightPx => quote!(|l, v| l.height = ::krui::Dimension::Px(v)),
        LayoutSlot::TopPx => quote!(|l, v| l.inset.top = ::krui::Dimension::Px(v)),
        LayoutSlot::LeftPx => quote!(|l, v| l.inset.left = ::krui::Dimension::Px(v)),
        LayoutSlot::RightPx => quote!(|l, v| l.inset.right = ::krui::Dimension::Px(v)),
        LayoutSlot::BottomPx => quote!(|l, v| l.inset.bottom = ::krui::Dimension::Px(v)),
        LayoutSlot::MarginPx => quote!(|l, v| l.margin = ::krui::Edges::all(v)),
        LayoutSlot::PaddingPx => quote!(|l, v| l.padding = ::krui::Edges::all(v)),
        LayoutSlot::GapPx => quote!(|l, v| l.gap = v),
    }
}

pub(crate) fn emit_inline_style_binding(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    if !node.dynamic_inline_style_parts.is_empty() {
        let mut idents = Vec::new();
        let mut pushers = Vec::with_capacity(node.dynamic_inline_style_parts.len());
        for part in &node.dynamic_inline_style_parts {
            match part {
                CompiledTextPart::Static(s) => pushers.push(quote! { __buf.push_str(#s); }),
                CompiledTextPart::Dynamic { expr, .. } => {
                    let expr_toks = emit_template_expr(expr, span, loop_vars)?;
                    collect_free_idents_from_template_expr(expr, span, loop_vars, &mut idents)?;
                    pushers.push(quote! {
                        __buf.push_str(&::std::format!("{}", #expr_toks));
                    });
                }
            }
        }
        let clones = emit_clone_lets(&idents);
        return Ok(quote! {{
            #clones
            __tree.bind_dynamic_inline_style(__node_id, move || {
                let mut __buf = ::std::string::String::new();
                #(#pushers)*
                __buf
            });
        }});
    }
    if let Some(css) = &node.static_inline_style {
        return Ok(quote! { __tree.set_inline_style(__node_id, #css); });
    }
    Ok(quote! {})
}

fn is_boolean_html_attr(name: &str) -> bool {
    matches!(
        name,
        "open" | "readonly" | "hidden" | "autofocus" | "multiple" | "novalidate" | "inert"
    )
}

pub(crate) fn emit_static_and_dynamic_attr_bindings(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let mut stmts = Vec::new();
    if let Some(ref ct) = node.custom_tag {
        stmts.push(quote! { __tree.set_custom_tag(__node_id, #ct); });
    }
    for (k, v) in &node.static_attrs {
        stmts.push(quote! { __tree.set_node_attr(__node_id, #k, #v); });
    }
    for binding in &node.dynamic_attrs {
        let name = &binding.name;
        let is_bool = is_boolean_html_attr(name);
        let mut idents = Vec::new();
        let mut pushers = Vec::with_capacity(binding.parts.len());
        for part in &binding.parts {
            match part {
                CompiledTextPart::Static(s) => pushers.push(quote! { __buf.push_str(#s); }),
                CompiledTextPart::Dynamic { expr, .. } => {
                    let expr_toks = emit_template_expr(expr, span, loop_vars)?;
                    collect_free_idents_from_template_expr(expr, span, loop_vars, &mut idents)?;
                    pushers.push(quote! {
                        let __raw = ::std::format!("{}", #expr_toks);
                        __buf.push_str(&__raw);
                    });
                }
            }
        }
        let clones = emit_clone_lets(&idents);
        stmts.push(quote! {{
            #clones
            __tree.bind_dynamic_attr(__node_id, #name, move || {
                let mut __buf = ::std::string::String::new();
                #(#pushers)*
                if #is_bool {
                    if __buf.trim().eq_ignore_ascii_case("false") {
                        ::std::option::Option::None
                    } else {
                        ::std::option::Option::Some(__buf)
                    }
                } else {
                    ::std::option::Option::Some(__buf)
                }
            });
        }});
    }
    Ok(quote! { #(#stmts)* })
}
