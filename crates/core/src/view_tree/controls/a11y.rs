//! Accessibility ID conversion (`A11yId` <-> `NodeId`) and `accesskit::TreeUpdate` generation.

use crate::view_tree::controls::{activate_control_node, format_range_value};
use crate::view_tree::form_submit::{refresh_control_validity_if_needed, select_display_label};
use crate::view_tree::grapheme::apply_bound_text_update;
use crate::view_tree::hit_test::{
    effective_node_world_matrix_and_rect, effective_node_world_matrix_rect_and_clip,
    transform_rect_aabb,
};
use crate::view_tree::ime_actions::resolve_ime_local_caret_rect;
use crate::view_tree::media_nodes::invoke_post_activation_actions;
use crate::view_tree::node::{sync_node_text_context, ElementNode};
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_a11y::{A11yNodeDescriptor, AccessibilityEngine, SemanticRole};
use krui_layout::LayoutRect;
use krui_signals::{batch, Effect};
use krui_style::{sanitize_untrusted_text, DisplayMode};
use std::rc::Rc;

const DEFAULT_CARET_WIDTH_PX: f32 = 2.0;

/// Offset added to `NodeId` indices when generating `accesskit` node IDs (`1` is reserved for root window).
pub const A11Y_ID_OFFSET: u64 = 10;

/// Strongly-typed wrapper around an `accesskit` 64-bit node identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct A11yId(pub u64);

impl From<NodeId> for A11yId {
    fn from(node_id: NodeId) -> Self {
        let high = (node_id.generation as u64) << 32;
        let low = (node_id.index as u64) + A11Y_ID_OFFSET;
        Self(high | low)
    }
}

impl TryFrom<A11yId> for NodeId {
    type Error = ();

    fn try_from(a11y_id: A11yId) -> Result<Self, Self::Error> {
        let generation = (a11y_id.0 >> 32) as u32;
        let low = a11y_id.0 & 0xFFFF_FFFF;
        if low >= A11Y_ID_OFFSET {
            Ok(NodeId {
                index: (low - A11Y_ID_OFFSET) as u32,
                generation,
            })
        } else {
            Err(())
        }
    }
}

impl TryFrom<u64> for NodeId {
    type Error = ();

    fn try_from(raw_ak_id: u64) -> Result<Self, Self::Error> {
        Self::try_from(A11yId(raw_ak_id))
    }
}

impl ViewTree {
    /// Returns the `NodeId` of the currently focused widget, if any.
    pub fn focused_node_id(&self) -> Option<NodeId> {
        let inner = self.inner.borrow();
        let ak_id = inner.a11y_engine.focused_id()?;
        let nid = NodeId::try_from(ak_id).ok()?;
        inner.node(nid).map(|n| n.id)
    }

    /// Returns the world-space axis-aligned bounding box (`LayoutRect`) of `node_id` after layout.
    pub fn node_layout(&self, node_id: NodeId) -> Option<LayoutRect> {
        let inner = self.inner.borrow();
        let _ = inner.node(node_id)?;
        let (world_affine, rect) = effective_node_world_matrix_and_rect(&inner, node_id);
        Some(transform_rect_aabb(world_affine, rect))
    }

    /// Returns the world bounding box of the currently focused `<input>` widget, if any.
    pub fn focused_input_rect(&self) -> Option<LayoutRect> {
        let inner = self.inner.borrow();
        let ak_id = inner.a11y_engine.focused_id()?;
        let nid = NodeId::try_from(ak_id).ok()?;
        let node = inner.node(nid)?;
        if node.role.is_text_editable() {
            let (world_affine, rect) = effective_node_world_matrix_and_rect(&inner, nid);
            Some(transform_rect_aabb(world_affine, rect))
        } else {
            None
        }
    }

    /// Computes and returns the viewport-space logical-pixel caret rectangle (`LayoutRect`) for
    /// the currently focused `<input>` / `<textarea>` widget (or `ime_target` if an in-progress
    /// preedit is active), incorporating ancestor `(scroll_x, scroll_y)`, CSS `transform` matrices,
    /// and ancestor scroll/overflow clip bounds.
    pub fn focused_ime_cursor_area(&self) -> Option<LayoutRect> {
        let mut inner = self.inner.borrow_mut();
        let nid = crate::view_tree::ime_host::resolve_host_input_node_id(&inner)?;
        let (world_affine, eff_rect, clip_bounds) =
            effective_node_world_matrix_rect_and_clip(&inner, nid);
        let caret_w = DEFAULT_CARET_WIDTH_PX;
        let local_caret = resolve_ime_local_caret_rect(&mut inner, nid, eff_rect, caret_w)?;
        let mut world_caret = transform_rect_aabb(world_affine, local_caret);
        if let Some(clip) = clip_bounds.filter(|c| c.width > 0.0 && c.height > 0.0) {
            let w = world_caret.width.clamp(1.0_f32.min(clip.width), clip.width);
            let h = world_caret
                .height
                .clamp(1.0_f32.min(clip.height), clip.height);
            world_caret.x = world_caret
                .x
                .clamp(clip.x, (clip.x + clip.width - w).max(clip.x));
            world_caret.y = world_caret
                .y
                .clamp(clip.y, (clip.y + clip.height - h).max(clip.y));
            world_caret.width = w;
            world_caret.height = h;
        }
        Some(world_caret)
    }

    /// Sets a static `aria-label` string on `node_id`.
    pub fn set_aria_label(&mut self, node_id: NodeId, label: &str) {
        let mut inner = self.inner.borrow_mut();
        if let Some(node) = inner.node_mut(node_id) {
            node.aria_label = Some(sanitize_untrusted_text(label));
        }
    }

    /// Returns the current `aria-label` string on `node_id`, or `None` if unset or `node_id` is stale.
    pub fn node_aria_label(&self, node_id: NodeId) -> Option<String> {
        self.inner
            .borrow()
            .node(node_id)
            .and_then(|n| n.aria_label.clone())
    }

    /// Binds a reactive `aria-label` formatting closure directly to `node_id`.
    pub fn bind_dynamic_aria_label(
        &mut self,
        node_id: NodeId,
        formatter: impl Fn() -> String + 'static,
    ) {
        self.bind_dynamic_aria_label_rc(node_id, Rc::new(formatter));
    }

    fn bind_dynamic_aria_label_rc(&mut self, node_id: NodeId, formatter: Rc<dyn Fn() -> String>) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let new_label = sanitize_untrusted_text(&formatter());
            if let Some(rc) = tree_weak.upgrade() {
                let mut inner = rc.borrow_mut();
                if let Some(node) = inner.node_mut(node_id) {
                    node.aria_label = Some(new_label);
                }
            }
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Reactively binds a whitespace-separated dynamic `class="..."` formatter to `node_id`.
    pub fn bind_dynamic_class(
        &mut self,
        node_id: NodeId,
        formatter: impl Fn() -> String + 'static,
    ) {
        self.bind_dynamic_class_rc(node_id, Rc::new(formatter));
    }

    fn bind_dynamic_class_rc(&mut self, node_id: NodeId, formatter: Rc<dyn Fn() -> String>) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let class_str = formatter();
            let classes: Vec<&str> = class_str.split_whitespace().collect();
            if let Some(rc) = tree_weak.upgrade() {
                let mut inner = rc.borrow_mut();
                crate::view_tree::sync::set_element_classes_inner(&mut inner, node_id, &classes);
            }
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Builds and returns the current `accesskit::TreeUpdate` using the latest viewport dimensions.
    pub fn build_a11y_tree_update(&mut self) -> accesskit::TreeUpdate {
        let (vw, vh) = self.inner.borrow().last_viewport;
        self.build_accessibility_tree(vw, vh)
    }

    /// Builds and returns the current `accesskit::TreeUpdate` (with secret masking enforced).
    pub fn build_accessibility_tree(
        &mut self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> accesskit::TreeUpdate {
        let mut inner = self.inner.borrow_mut();
        inner.last_viewport = (viewport_w, viewport_h);
        let mut descriptors = Vec::with_capacity(inner.nodes.len() + 1);
        let root_children: Vec<u64> = inner
            .roots
            .iter()
            .copied()
            .filter(|&r| {
                inner
                    .node(r)
                    .is_some_and(|n| !n.is_anchor() && n.style.layout.display != DisplayMode::None)
            })
            .map(|r| A11yId::from(r).0)
            .collect();
        descriptors.push(A11yNodeDescriptor {
            id: AccessibilityEngine::ROOT_A11Y_ID,
            role: SemanticRole::Window,
            label: Some("KRui Application".to_string()),
            value: None,
            url: None,
            is_checked: None,
            is_secret: false,
            is_disabled: false,
            bounds: LayoutRect {
                x: 0.0,
                y: 0.0,
                width: viewport_w,
                height: viewport_h,
            },
            children: root_children,
        });
        let roots = inner.roots.clone();
        for root_id in roots {
            collect_visible_a11y_descriptors(&inner, root_id, &mut descriptors);
        }
        let update = inner
            .a11y_engine
            .build_tree_update(&descriptors, AccessibilityEngine::ROOT_A11Y_ID);
        let order = crate::view_tree::focus_pointer::collect_enabled_focusable_order(&inner);
        inner.a11y_engine.sync_focusable_order(order);
        update
    }

    /// Dispatches an owned `accesskit_winit` [`accesskit::ActionRequest`] into the `ViewTree`.
    pub fn dispatch_a11y_action(&mut self, req: accesskit::ActionRequest) -> bool {
        self.dispatch_accesskit_action(&req)
    }

    /// Dispatches an OS assistive-technology [`accesskit::ActionRequest`] (`Focus`, `Click`, `SetValue`)
    /// into the `ViewTree`.
    pub fn dispatch_accesskit_action(&mut self, req: &accesskit::ActionRequest) -> bool {
        let Ok(nid) = NodeId::try_from(req.target.0) else {
            return false;
        };
        {
            let inner = self.inner.borrow();
            if !is_node_a11y_visible(&inner, nid) || inner.node(nid).is_none_or(|n| n.is_disabled())
            {
                return false;
            }
        }
        match req.action {
            accesskit::Action::Focus => self.set_focus_to_node(nid),
            accesskit::Action::Click => self.apply_accesskit_click(nid),
            accesskit::Action::SetValue => {
                let raw_val = match &req.data {
                    Some(accesskit::ActionData::Value(v)) => v.to_string(),
                    Some(accesskit::ActionData::NumericValue(n)) => format_range_value(*n as f32),
                    _ => return false,
                };
                self.apply_accesskit_set_value(nid, &raw_val)
            }
            _ => false,
        }
    }

    fn apply_accesskit_click(&mut self, nid: NodeId) -> bool {
        let _ = self.set_focus_to_node(nid);
        let (ctrl_act, click_cb) = {
            let mut inner = self.inner.borrow_mut();
            let act = activate_control_node(&mut inner, nid);
            let cb = inner
                .node(nid)
                .filter(|n| !n.is_disabled())
                .and_then(|_| inner.listeners(nid))
                .and_then(|l| l.on_click.clone());
            (act, cb)
        };
        let mut handled = false;
        if let Some(act) = ctrl_act {
            act.invoke();
            handled = true;
        }
        if let Some(cb) = click_cb {
            batch(|| cb());
            handled = true;
        }
        handled |= invoke_post_activation_actions(self, nid);
        handled
    }

    fn apply_accesskit_set_value(&mut self, nid: NodeId, raw_val: &str) -> bool {
        let (input_cb, change_cb) = {
            let mut inner = self.inner.borrow_mut();
            let is_focused = inner.a11y_engine.focused_id() == Some(A11yId::from(nid).0);
            let Some((role, is_disabled)) = inner.node(nid).map(|n| (n.role, n.is_disabled()))
            else {
                return false;
            };
            if is_disabled {
                return false;
            }
            let _ = apply_bound_text_update(&mut inner, nid, raw_val, is_focused);
            let val = inner.node(nid).map(|n| n.text.clone()).unwrap_or_default();
            if role.is_text_editable() || inner.sparse.text_edits.contains_key(&nid) {
                let ed = inner.ensure_text_edit(nid);
                ed.committed_value_on_focus = val.clone();
                ed.cursor_char_idx = val.chars().count();
                ed.selection_anchor_char_idx = None;
                ed.needs_caret_scroll = true;
            }
            if let Some(node) = inner.node_mut(nid) {
                node.set_text_dirty(true);
            }
            let icb = inner
                .listeners(nid)
                .and_then(|l| l.on_input.clone())
                .map(|cb| (cb, val.clone()));
            let ccb = inner
                .listeners(nid)
                .and_then(|l| l.on_change.clone())
                .map(|cb| (cb, val));
            refresh_control_validity_if_needed(&mut inner, nid);
            sync_node_text_context(&mut inner, nid);
            inner.paint_dirty = true;
            (icb, ccb)
        };
        if let Some((cb, v)) = input_cb {
            batch(|| cb(v));
        }
        if let Some((cb, v)) = change_cb {
            batch(|| cb(v));
        }
        true
    }
}

/// Returns `true` if `node_id` and every ancestor in its chain is alive, non-anchor, and visible.
pub(crate) fn is_node_a11y_visible(inner: &ViewTreeInner, node_id: NodeId) -> bool {
    let mut cur = Some(node_id);
    while let Some(cid) = cur {
        let Some(n) = inner.node(cid) else {
            return false;
        };
        if n.is_anchor() || n.style.layout.display == DisplayMode::None {
            return false;
        }
        cur = n.parent;
    }
    true
}

fn collect_visible_a11y_descriptors(
    inner: &ViewTreeInner,
    node_id: NodeId,
    out: &mut Vec<A11yNodeDescriptor>,
) {
    let Some(node) = inner.node(node_id) else {
        return;
    };
    if node.is_anchor() || node.style.layout.display == DisplayMode::None {
        return;
    }
    if crate::view_tree::pointer_vars::is_pseudo_node(inner, node_id) && node.text.is_empty() {
        return;
    }
    out.push(build_node_a11y_descriptor(inner, node));
    for &child_id in &node.children {
        collect_visible_a11y_descriptors(inner, child_id, out);
    }
}

fn build_node_a11y_descriptor(inner: &ViewTreeInner, node: &ElementNode) -> A11yNodeDescriptor {
    let url = inner
        .media(node.id)
        .and_then(|m| m.link_href.clone())
        .filter(|u| !u.is_empty());
    let label = node.aria_label.clone().or_else(|| {
        if node.role == SemanticRole::ComboBox {
            let lbl = select_display_label(inner, node.id);
            (!lbl.is_empty()).then_some(lbl)
        } else if !node.text.is_empty() && !node.role.is_text_editable() {
            Some(node.text.clone())
        } else if matches!(node.role, SemanticRole::Link | SemanticRole::Button) {
            crate::view_tree::layout_modes::collect_visible_subtree_text(inner, node.id)
                .or_else(|| url.clone())
        } else {
            None
        }
    });
    let value = (node.role.is_text_editable() || !node.text.is_empty()).then(|| node.text.clone());
    let is_checked = matches!(
        node.role,
        SemanticRole::Checkbox | SemanticRole::RadioButton
    )
    .then_some(node.is_checked());
    let (world_affine, eff_rect) = effective_node_world_matrix_and_rect(inner, node.id);
    let bounds = transform_rect_aabb(world_affine, eff_rect);
    let children: Vec<u64> = node
        .children
        .iter()
        .copied()
        .filter(|&c| {
            inner.node(c).is_some_and(|n| {
                !n.is_anchor()
                    && n.style.layout.display != DisplayMode::None
                    && !(crate::view_tree::pointer_vars::is_pseudo_node(inner, c)
                        && n.text.is_empty())
            })
        })
        .map(|c| A11yId::from(c).0)
        .collect();
    A11yNodeDescriptor {
        id: A11yId::from(node.id).0,
        role: node.role,
        label,
        value,
        url,
        is_checked,
        is_secret: node.is_secret(),
        is_disabled: node.is_disabled(),
        bounds,
        children,
    }
}
