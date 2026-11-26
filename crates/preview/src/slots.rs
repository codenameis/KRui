//! Element slot binding helpers (text, placeholder, aria-label, dynamic class, paint, layout, events)
//! for `krui-preview`.

use crate::eval::{eval_interpolated_parts, eval_template_expr, resolve_signal_from_expr};
use crate::select_slots::bind_select_children_slot;
pub(crate) use crate::slots_events::bind_event_slots;
use krui_compiler::{
    CompiledButtonType, CompiledInputType, CompiledNode, CompiledTextPart, HtmlTag, LayoutSlot,
    PaintSlot,
};
use krui_core::{ButtonKind, InputKind, NodeId, RegistryRef, Value, ViewTree};
use krui_style::{Dimension, Edges, FontVariationList, StyledTextSpan, Transform2D};

pub(crate) fn bind_text_and_class_slots(
    tree: &mut ViewTree,
    node_id: NodeId,
    node: &CompiledNode,
    registry: &RegistryRef,
) {
    configure_control_role_and_meta(tree, node_id, node);
    crate::media_slots::bind_media_slots(tree, node_id, node, registry);
    bind_select_children_slot(tree, node_id, &node.select_children, registry);
    bind_rich_text_slot(tree, node_id, node, registry);
    bind_interpolated(
        tree,
        node_id,
        &node.text_parts,
        registry,
        ViewTree::set_static_text,
        ViewTree::bind_dynamic_text,
    );
    bind_interpolated(
        tree,
        node_id,
        &node.placeholder_parts,
        registry,
        ViewTree::set_placeholder,
        ViewTree::bind_dynamic_placeholder,
    );
    bind_interpolated(
        tree,
        node_id,
        &node.aria_label_parts,
        registry,
        ViewTree::set_aria_label,
        ViewTree::bind_dynamic_aria_label,
    );
    bind_interpolated(
        tree,
        node_id,
        &node.title_parts,
        registry,
        ViewTree::set_title,
        |t, id, f| t.bind_dynamic_title(id, f),
    );
    if node
        .class_parts
        .iter()
        .any(|p| matches!(p, CompiledTextPart::Dynamic { .. }))
    {
        let parts = node.class_parts.clone();
        let reg = registry.clone();
        tree.bind_dynamic_class(node_id, move || {
            eval_interpolated_parts(&parts, reg.as_ref(), false)
        });
    } else if !node.classes.is_empty() {
        let refs: Vec<&str> = node.classes.iter().map(String::as_str).collect();
        tree.set_element_classes(node_id, &refs);
    }
    if !node.dynamic_inline_style_parts.is_empty() {
        let parts = node.dynamic_inline_style_parts.clone();
        let reg = registry.clone();
        tree.bind_dynamic_inline_style(node_id, move || {
            eval_interpolated_parts(&parts, reg.as_ref(), false)
        });
    } else if let Some(css) = &node.static_inline_style {
        tree.set_inline_style(node_id, css);
    }
}

fn eval_rich_spans(
    raw: &[krui_compiler::CompiledRichSpan],
    registry: &dyn krui_core::Registry,
) -> Vec<StyledTextSpan> {
    raw.iter()
        .map(|sp| StyledTextSpan {
            text: eval_interpolated_parts(&sp.parts, registry, true),
            font_weight: sp.font_weight,
            italic: sp.italic,
            font_family: sp.font_family,
            color: sp.color,
            variations: FontVariationList::EMPTY,
        })
        .collect()
}

fn bind_rich_text_slot(
    tree: &mut ViewTree,
    node_id: NodeId,
    node: &CompiledNode,
    registry: &RegistryRef,
) {
    if node.rich_spans.is_empty() {
        return;
    }
    let has_dyn = node.rich_spans.iter().any(|sp| {
        sp.parts
            .iter()
            .any(|p| matches!(p, CompiledTextPart::Dynamic { .. }))
    });
    if !has_dyn {
        let resolved = eval_rich_spans(&node.rich_spans, registry.as_ref());
        tree.set_rich_spans(node_id, resolved);
    } else {
        let raw_spans = node.rich_spans.clone();
        let reg = registry.clone();
        tree.bind_dynamic_rich_spans(node_id, move || eval_rich_spans(&raw_spans, reg.as_ref()));
    }
}

fn configure_control_role_and_meta(tree: &mut ViewTree, node_id: NodeId, node: &CompiledNode) {
    if let Some(target_id) = &node.label_for {
        tree.set_label_for(node_id, target_id);
    }
    if let Some(name) = &node.control_name {
        tree.set_control_name(node_id, name);
    }
    match node.button_type {
        CompiledButtonType::Auto => {}
        CompiledButtonType::Button => tree.set_button_kind(node_id, ButtonKind::Button),
        CompiledButtonType::Submit => tree.set_button_kind(node_id, ButtonKind::Submit),
        CompiledButtonType::Reset => tree.set_button_kind(node_id, ButtonKind::Reset),
    }
    if node.select_children.is_empty() {
        for opt in &node.select_options {
            tree.add_select_option(node_id, &opt.value, &opt.label, opt.selected);
        }
    }
    if node.tag == HtmlTag::Input {
        match node.input_type {
            CompiledInputType::Checkbox => tree.set_checkbox_role(node_id),
            CompiledInputType::Radio => tree.set_radio_role(node_id),
            CompiledInputType::Range => {
                let (min, max, step) = node.range_bounds.unwrap_or((0.0, 100.0, 1.0));
                tree.set_range_bounds(node_id, min, max, step);
            }
            _ => {
                let kind = match node.input_type {
                    CompiledInputType::Password => InputKind::Password,
                    CompiledInputType::Number => InputKind::Number,
                    CompiledInputType::Email => InputKind::Email,
                    CompiledInputType::Search => InputKind::Search,
                    _ => InputKind::Text,
                };
                tree.set_input_kind(node_id, kind);
            }
        }
    }
}

pub(crate) fn bind_interpolated(
    tree: &mut ViewTree,
    node_id: NodeId,
    parts: &[CompiledTextPart],
    registry: &RegistryRef,
    set_static: fn(&mut ViewTree, NodeId, &str),
    bind_dyn: fn(&mut ViewTree, NodeId, Box<dyn Fn() -> String>),
) {
    if parts.is_empty() {
        return;
    }
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
        set_static(tree, node_id, &combined);
    } else {
        let owned_parts = parts.to_vec();
        let reg = registry.clone();
        bind_dyn(
            tree,
            node_id,
            Box::new(move || eval_interpolated_parts(&owned_parts, reg.as_ref(), true)),
        );
    }
}

pub(crate) fn bind_paint_and_layout_slots(
    tree: &mut ViewTree,
    node_id: NodeId,
    node: &CompiledNode,
    registry: &RegistryRef,
) {
    for b in &node.paint_bindings {
        bind_single_paint_slot(tree, node_id, b, registry);
    }

    for b in &node.layout_bindings {
        let expr = b.expr.clone();
        let reg = registry.clone();
        let mutate: fn(&mut krui_style::LayoutStyle, f32) = match &b.slot {
            LayoutSlot::WidthPx => |l, v| l.width = Dimension::Px(v),
            LayoutSlot::HeightPx => |l, v| l.height = Dimension::Px(v),
            LayoutSlot::TopPx => |l, v| l.inset.top = Dimension::Px(v),
            LayoutSlot::LeftPx => |l, v| l.inset.left = Dimension::Px(v),
            LayoutSlot::RightPx => |l, v| l.inset.right = Dimension::Px(v),
            LayoutSlot::BottomPx => |l, v| l.inset.bottom = Dimension::Px(v),
            LayoutSlot::MarginPx => |l, v| l.margin = Edges::all(v),
            LayoutSlot::PaddingPx => |l, v| l.padding = Edges::all(v),
            LayoutSlot::GapPx => |l, v| l.gap = v,
        };
        tree.bind_layout_eval(
            node_id,
            move || {
                eval_template_expr(&expr, reg.as_ref())
                    .as_f64()
                    .unwrap_or(0.0) as f32
            },
            mutate,
        );
    }
}

fn bind_single_paint_slot(
    tree: &mut ViewTree,
    node_id: NodeId,
    b: &krui_compiler::PaintSlotBinding,
    registry: &RegistryRef,
) {
    if let Some(sig) =
        resolve_signal_from_expr(&b.expr, registry.as_ref()).and_then(|ds| ds.as_f32_signal())
    {
        match &b.slot {
            PaintSlot::TranslateX => tree.bind_paint_translate_x(node_id, sig),
            PaintSlot::TranslateY => tree.bind_paint_translate_y(node_id, sig),
            PaintSlot::ScaleX => tree.bind_paint_scale_x(node_id, sig),
            PaintSlot::ScaleY => tree.bind_paint_scale_y(node_id, sig),
            PaintSlot::ScaleUniform => tree.bind_paint_scale_uniform(node_id, sig),
            PaintSlot::RotateDeg => tree.bind_paint_rotate_deg(node_id, sig),
            PaintSlot::Opacity => tree.bind_paint_opacity(node_id, sig),
            PaintSlot::FastTransform => {}
        }
        return;
    }
    let expr = b.expr.clone();
    let reg = registry.clone();
    match &b.slot {
        PaintSlot::FastTransform => {
            tree.bind_paint_eval(
                node_id,
                move || match eval_template_expr(&expr, reg.as_ref()) {
                    Value::FastTransform(ft) => Transform2D::from(ft),
                    _ => Transform2D::default(),
                },
                |p, tf| p.transform = tf,
            );
        }
        slot => {
            let apply: fn(&mut krui_style::PaintStyle, f32) = match slot {
                PaintSlot::TranslateX => |p, v| p.transform.translate_x = v,
                PaintSlot::TranslateY => |p, v| p.transform.translate_y = v,
                PaintSlot::ScaleX => |p, v| p.transform.scale_x = v,
                PaintSlot::ScaleY => |p, v| p.transform.scale_y = v,
                PaintSlot::ScaleUniform => |p, v| {
                    p.transform.scale_x = v;
                    p.transform.scale_y = v;
                },
                PaintSlot::RotateDeg => |p, v| p.transform.rotate_deg = v,
                PaintSlot::Opacity => |p, v| p.opacity = v.clamp(0.0, 1.0),
                PaintSlot::FastTransform => unreachable!(),
            };
            tree.bind_paint_eval(
                node_id,
                move || {
                    eval_template_expr(&expr, reg.as_ref())
                        .as_f64()
                        .unwrap_or(0.0) as f32
                },
                apply,
            );
        }
    }
}
