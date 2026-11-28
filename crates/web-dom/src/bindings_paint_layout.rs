//! Reactive paint-slot and layout-style CSS property bindings for [`WebDomTree`].

use crate::backend::DomNodeId;
use crate::tree::WebDomTree;
use krui_signals::{Effect, FastPathTransform, Signal};
use krui_style::{Dimension, Edges, LayoutStyle, PaintStyle};

impl WebDomTree {
    /// Reactively binds a `PaintStyle` slot directly to a [`Signal`].
    pub fn bind_paint<T: Copy + 'static>(
        &mut self,
        id: DomNodeId,
        sig: Signal<T>,
        apply: fn(&mut PaintStyle, T),
    ) {
        self.bind_paint_eval(id, move || sig.get(), apply);
    }

    /// Evaluates `PaintStyle` mutations produced by fast-path style bindings onto CSS properties.
    pub fn bind_paint_eval<T: 'static>(
        &mut self,
        id: DomNodeId,
        eval: impl Fn() -> T + 'static,
        apply: fn(&mut PaintStyle, T),
    ) {
        let tree = self.clone();
        let _ = Effect::new(move || {
            let val = eval();
            let mut ps = PaintStyle::default();
            apply(&mut ps, val);
            let mut guard = tree.inner.borrow_mut();
            let tr = ps.transform;
            let tf = format!(
                "translate({:.3}px, {:.3}px) rotate({:.3}deg) scale({:.4}, {:.4})",
                tr.translate_x, tr.translate_y, tr.rotate_deg, tr.scale_x, tr.scale_y
            );
            guard.backend.set_style_property(id, "transform", &tf);
            guard
                .backend
                .set_style_property(id, "opacity", &format!("{:.4}", ps.opacity));
        });
    }

    /// Reactively binds CSS `transform: translateX(...)`.
    pub fn bind_paint_translate_x(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(
            id,
            "translate",
            |v| format!("{v:.2}px 0px"),
            move || sig.get(),
        );
    }

    /// Reactively binds CSS `translate: 0 y`.
    pub fn bind_paint_translate_y(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(
            id,
            "translate",
            |v| format!("0px {v:.2}px"),
            move || sig.get(),
        );
    }

    /// Reactively binds CSS `scale: sx 1`.
    pub fn bind_paint_scale_x(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "scale", |v| format!("{v:.4} 1"), move || sig.get());
    }

    /// Reactively binds CSS `scale: 1 sy`.
    pub fn bind_paint_scale_y(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "scale", |v| format!("1 {v:.4}"), move || sig.get());
    }

    /// Reactively binds uniform CSS `scale`.
    pub fn bind_paint_scale_uniform(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "scale", |v| format!("{v:.4}"), move || sig.get());
    }

    /// Reactively binds CSS `rotate`.
    pub fn bind_paint_rotate_deg(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "rotate", |v| format!("{v:.3}deg"), move || sig.get());
    }

    /// Reactively binds CSS `opacity`.
    pub fn bind_paint_opacity(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "opacity", |v| format!("{v:.4}"), move || sig.get());
    }

    /// Reactively binds a composite [`FastPathTransform`] onto CSS `transform`.
    pub fn bind_paint_fast_transform(&mut self, id: DomNodeId, sig: Signal<FastPathTransform>) {
        let tree = self.clone();
        let _ = Effect::new(move || {
            let t = sig.get();
            let css = format!(
                "translate({:.3}px, {:.3}px) rotate({:.3}deg) scale({:.4}, {:.4})",
                t.translate_x, t.translate_y, t.rotate_deg, t.scale_x, t.scale_y
            );
            let mut guard = tree.inner.borrow_mut();
            guard.backend.set_style_property(id, "transform", &css);
        });
    }

    /// Reactively applies scalar layout style mutations onto browser inline CSS properties.
    pub fn bind_layout_eval(
        &mut self,
        id: DomNodeId,
        eval: impl Fn() -> f32 + 'static,
        mutate: fn(&mut LayoutStyle, f32),
    ) {
        let tree = self.clone();
        let _ = Effect::new(move || {
            let val = eval();
            let mut ls = LayoutStyle::default();
            mutate(&mut ls, val);
            apply_layout_diff_to_css(&tree, id, &ls);
        });
    }

    /// Reactively binds `width: {v}px`.
    pub fn bind_layout_width_px(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "width", |v| format!("{v:.2}px"), move || sig.get());
    }

    /// Reactively binds `height: {v}px`.
    pub fn bind_layout_height_px(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "height", |v| format!("{v:.2}px"), move || sig.get());
    }

    /// Reactively binds `top: {v}px`.
    pub fn bind_layout_top_px(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "top", |v| format!("{v:.2}px"), move || sig.get());
    }

    /// Reactively binds `left: {v}px`.
    pub fn bind_layout_left_px(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "left", |v| format!("{v:.2}px"), move || sig.get());
    }

    /// Reactively binds `right: {v}px`.
    pub fn bind_layout_right_px(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "right", |v| format!("{v:.2}px"), move || sig.get());
    }

    /// Reactively binds `bottom: {v}px`.
    pub fn bind_layout_bottom_px(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "bottom", |v| format!("{v:.2}px"), move || sig.get());
    }

    /// Reactively binds `margin: {v}px`.
    pub fn bind_layout_margin_px(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "margin", |v| format!("{v:.2}px"), move || sig.get());
    }

    /// Reactively binds `padding: {v}px`.
    pub fn bind_layout_padding_px(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "padding", |v| format!("{v:.2}px"), move || sig.get());
    }

    /// Reactively binds `gap: {v}px`.
    pub fn bind_layout_gap_px(&mut self, id: DomNodeId, sig: Signal<f32>) {
        self.bind_css_prop_f32(id, "gap", |v| format!("{v:.2}px"), move || sig.get());
    }

    fn bind_css_prop_f32(
        &mut self,
        id: DomNodeId,
        prop: &'static str,
        fmt: fn(f32) -> String,
        val_fn: impl Fn() -> f32 + 'static,
    ) {
        let tree = self.clone();
        let _ = Effect::new(move || {
            let v = val_fn();
            tree.inner
                .borrow_mut()
                .backend
                .set_style_property(id, prop, &fmt(v));
        });
    }
}

fn apply_layout_diff_to_css(tree: &WebDomTree, id: DomNodeId, ls: &LayoutStyle) {
    let mut guard = tree.inner.borrow_mut();
    let b = guard.backend.as_mut();
    if let Dimension::Px(w) = ls.width {
        b.set_style_property(id, "width", &format!("{w:.2}px"));
    }
    if let Dimension::Px(h) = ls.height {
        b.set_style_property(id, "height", &format!("{h:.2}px"));
    }
    for (prop, dim) in [
        ("top", ls.inset.top),
        ("right", ls.inset.right),
        ("bottom", ls.inset.bottom),
        ("left", ls.inset.left),
    ] {
        if let Dimension::Px(v) = dim {
            b.set_style_property(id, prop, &format!("{v:.2}px"));
        }
    }
    if ls.gap != 0.0 {
        b.set_style_property(id, "gap", &format!("{:.2}px", ls.gap));
    }
    if ls.margin != Edges::ZERO {
        let m = ls.margin;
        let css = format!(
            "{:.2}px {:.2}px {:.2}px {:.2}px",
            m.top, m.right, m.bottom, m.left
        );
        b.set_style_property(id, "margin", &css);
    }
    if ls.padding != Edges::ZERO {
        let p = ls.padding;
        let css = format!(
            "{:.2}px {:.2}px {:.2}px {:.2}px",
            p.top, p.right, p.bottom, p.left
        );
        b.set_style_property(id, "padding", &css);
    }
}
