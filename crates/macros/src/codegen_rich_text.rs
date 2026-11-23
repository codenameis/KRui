//! Procedural macro code generation for compiled rich inline text paragraphs (`CompiledRichSpan`).

use crate::expr_vars::{
    collect_free_idents_from_template_expr, emit_clone_lets, emit_template_expr,
};
use krui_compiler::{CompiledNode, CompiledRichSpan, CompiledTextPart};
use krui_style::{Color, FontFamilyStack};
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::quote;

fn emit_color(c: Color) -> TokenStream2 {
    let (r, g, b, a) = (c.r, c.g, c.b, c.a);
    quote!(::krui::__private::style::Color { r: #r, g: #g, b: #b, a: #a })
}

fn emit_font_family(f: FontFamilyStack) -> TokenStream2 {
    match f {
        FontFamilyStack::FluentUI => quote!(::krui::FontFamilyStack::FluentUI),
        FontFamilyStack::MaterialSans => quote!(::krui::FontFamilyStack::MaterialSans),
        FontFamilyStack::LiquidGlassSans => quote!(::krui::FontFamilyStack::LiquidGlassSans),
        FontFamilyStack::CosmicMono => quote!(::krui::FontFamilyStack::CosmicMono),
        FontFamilyStack::Serif => quote!(::krui::FontFamilyStack::Serif),
        FontFamilyStack::Custom(_) => {
            let fams = f.custom_families().unwrap_or(&[]);
            quote!(::krui::FontFamilyStack::intern_custom(&[#(#fams),*]))
        }
    }
}

fn emit_parts_string_expr(
    parts: &[CompiledTextPart],
    span: Span,
    loop_vars: &[String],
    idents: &mut Vec<Ident>,
    owned_string: bool,
) -> Result<TokenStream2, syn::Error> {
    let has_dyn = parts
        .iter()
        .any(|p| matches!(p, CompiledTextPart::Dynamic { .. }));
    if !has_dyn {
        let mut combined = String::new();
        for p in parts {
            if let CompiledTextPart::Static(s) = p {
                combined.push_str(s);
            }
        }
        return Ok(if owned_string {
            quote! { ::std::string::String::from(#combined) }
        } else {
            quote! { #combined.into() }
        });
    }
    let mut pushers = Vec::with_capacity(parts.len());
    for part in parts {
        match part {
            CompiledTextPart::Static(s) => pushers.push(quote! { __buf.push_str(#s); }),
            CompiledTextPart::Dynamic { expr, .. } => {
                let expr_toks = emit_template_expr(expr, span, loop_vars)?;
                collect_free_idents_from_template_expr(expr, span, loop_vars, idents)?;
                pushers.push(quote! {
                    let __raw = ::std::format!("{}", #expr_toks);
                    __buf.push_str(&::krui::sanitize_untrusted_text(&__raw));
                });
            }
        }
    }
    Ok(quote! {{
        let mut __buf = ::std::string::String::new();
        #(#pushers)*
        __buf
    }})
}

fn emit_single_rich_span(
    sp: &CompiledRichSpan,
    span: Span,
    loop_vars: &[String],
    idents: &mut Vec<Ident>,
) -> Result<TokenStream2, syn::Error> {
    let text_expr = emit_parts_string_expr(&sp.parts, span, loop_vars, idents, true)?;
    let fw = sp.font_weight;
    let italic = sp.italic;
    let ff = emit_font_family(sp.font_family);
    let col = match sp.color {
        Some(c) => {
            let ct = emit_color(c);
            quote! { ::std::option::Option::Some(#ct) }
        }
        None => quote! { ::std::option::Option::None },
    };
    Ok(quote! {
        ::krui::__private::style::StyledTextSpan {
            text: #text_expr,
            font_weight: #fw,
            italic: #italic,
            font_family: #ff,
            color: #col,
            variations: ::krui::__private::style::FontVariationList::EMPTY,
        }
    })
}

/// Emits static or reactive `rich_spans` binding calls onto `__tree` for `__node_id`.
pub(crate) fn emit_rich_text_slot(
    node: &CompiledNode,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    if node.rich_spans.is_empty() {
        return Ok(quote! {});
    }
    let any_dynamic = node.rich_spans.iter().any(|sp| {
        sp.parts
            .iter()
            .any(|p| matches!(p, CompiledTextPart::Dynamic { .. }))
    });
    let mut idents = Vec::new();
    let mut span_exprs = Vec::with_capacity(node.rich_spans.len());
    for sp in &node.rich_spans {
        span_exprs.push(emit_single_rich_span(sp, span, loop_vars, &mut idents)?);
    }
    if !any_dynamic {
        Ok(quote! {
            __tree.set_rich_spans(__node_id, ::std::vec![#(#span_exprs),*]);
        })
    } else {
        let clones = emit_clone_lets(&idents);
        Ok(quote! {{
            #clones
            __tree.bind_dynamic_rich_spans(__node_id, move || ::std::vec![#(#span_exprs),*]);
        }})
    }
}
