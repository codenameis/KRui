//! Two-tier `@keyframes` animation driver for `ViewTree`.
//!
//! Tier-1 compositor keyframes (`!touches_layout`) sample only into paint state with
//! zero Taffy relayouts. Tier-2 layout keyframes (`touches_layout`) update Taffy only
//! on animated nodes and restore/resync cleanly on completion or CSS hot-patch.

use crate::view_tree::content_box::sync_node_text_context;
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_style::{
    sample_keyframes_with_timing, AnimationListRef, AnimationSpec, DisplayMode, KeyframesDef,
    LayoutStyle, PaintStyle,
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct AnimationTrack {
    name: String,
    elapsed_ms: f64,
}

#[derive(Clone, Debug, Default)]
struct NodeKeyframeState {
    specs: Vec<AnimationSpec>,
    tracks: Vec<AnimationTrack>,
    base_layout: Option<LayoutStyle>,
}

/// Retained document `@keyframes` definitions and active per-node playback tracks.
#[derive(Clone, Debug, Default)]
pub(crate) struct KeyframeDriver {
    pub(crate) definitions: HashMap<String, KeyframesDef>,
    nodes: HashMap<NodeId, NodeKeyframeState>,
}

impl KeyframeDriver {
    pub(crate) fn purge_node(&mut self, id: NodeId) {
        self.nodes.remove(&id);
    }

    pub(crate) fn clear_all(&mut self) {
        self.definitions.clear();
        self.nodes.clear();
    }

    pub(crate) fn has_node(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    pub(crate) fn update_base_layout_if_tracked(&mut self, id: NodeId, base: LayoutStyle) {
        if let Some(st) = self.nodes.get_mut(&id) {
            if st.base_layout.is_some() {
                st.base_layout = Some(base);
            }
        }
    }
}

pub(crate) fn is_node_animation_visible(inner: &ViewTreeInner, node_id: NodeId) -> bool {
    let mut cur = Some(node_id);
    while let Some(nid) = cur {
        let Some(node) = inner.node(nid) else {
            return false;
        };
        if !node.alive() || node.is_anchor() || node.style.layout.display == DisplayMode::None {
            return false;
        }
        cur = node.parent;
    }
    true
}

fn track_is_active(
    spec: &AnimationSpec,
    elapsed_ms: f64,
    def: Option<&KeyframesDef>,
    visible: bool,
    reduced_motion: bool,
) -> bool {
    if !visible || reduced_motion || spec.paused || !spec.is_configured() {
        return false;
    }
    if def.is_none_or(|d| d.stops.is_empty()) {
        return false;
    }
    match spec.iterations {
        None => true,
        Some(iters) => {
            let end_ms =
                (spec.delay_ms as f64) + (spec.duration_ms as f64) * (iters.max(0.0) as f64);
            elapsed_ms < end_ms
        }
    }
}

pub(crate) fn has_active_tracks(inner: &ViewTreeInner) -> bool {
    if inner.animations_paused || inner.keyframes.nodes.is_empty() {
        return false;
    }
    let reduced = inner.effective_reduced_motion();
    if reduced {
        return false;
    }
    inner.keyframes.nodes.iter().any(|(&nid, st)| {
        let visible = is_node_animation_visible(inner, nid);
        st.specs.iter().zip(st.tracks.iter()).any(|(spec, tr)| {
            track_is_active(
                spec,
                tr.elapsed_ms,
                inner.keyframes.definitions.get(&spec.name),
                visible,
                reduced,
            )
        })
    })
}

pub(crate) fn active_track_count(inner: &ViewTreeInner) -> usize {
    if inner.animations_paused || inner.keyframes.nodes.is_empty() {
        return 0;
    }
    let reduced = inner.effective_reduced_motion();
    if reduced {
        return 0;
    }
    let mut total = 0usize;
    for (&nid, st) in &inner.keyframes.nodes {
        let visible = is_node_animation_visible(inner, nid);
        for (spec, tr) in st.specs.iter().zip(st.tracks.iter()) {
            if track_is_active(
                spec,
                tr.elapsed_ms,
                inner.keyframes.definitions.get(&spec.name),
                visible,
                reduced,
            ) {
                total += 1;
            }
        }
    }
    total
}

pub(crate) fn resolve_node_paint(
    inner: &ViewTreeInner,
    node_id: NodeId,
    paint: PaintStyle,
) -> PaintStyle {
    let Some(st) = inner.keyframes.nodes.get(&node_id) else {
        return paint;
    };
    if !is_node_animation_visible(inner, node_id) {
        return paint;
    }
    let reduced = inner.effective_reduced_motion();
    let base_layout = LayoutStyle::default();
    let mut out = paint;
    for (spec, tr) in st.specs.iter().zip(st.tracks.iter()) {
        let Some(def) = inner.keyframes.definitions.get(&spec.name) else {
            continue;
        };
        if let Some(t) = spec.sample_progress(tr.elapsed_ms, reduced) {
            let (next_paint, _) =
                sample_keyframes_with_timing(def, &out, &base_layout, t, spec.timing);
            out = next_paint;
        }
    }
    out
}

pub(crate) fn sync_node_tier2_layout(inner: &mut ViewTreeInner, node_id: NodeId) -> bool {
    let needs_tier2 = inner.keyframes.nodes.get(&node_id).is_some_and(|st| {
        st.specs.iter().any(|s| {
            inner
                .keyframes
                .definitions
                .get(&s.name)
                .is_some_and(|d| d.touches_layout)
        })
    });
    if !needs_tier2 {
        let restored = inner
            .keyframes
            .nodes
            .get_mut(&node_id)
            .and_then(|st| st.base_layout.take());
        return restored.is_some_and(|base| apply_layout_if_changed(inner, node_id, base));
    }
    let current_layout = match inner.node(node_id) {
        Some(n) if n.alive() && !n.is_anchor() => n.style.layout,
        _ => return false,
    };
    let visible = is_node_animation_visible(inner, node_id);
    let reduced = inner.effective_reduced_motion();
    let defs = &inner.keyframes.definitions;
    let Some(st) = inner.keyframes.nodes.get_mut(&node_id) else {
        return false;
    };
    let base = *st.base_layout.get_or_insert(current_layout);
    let mut target = base;
    if visible {
        let base_paint = PaintStyle::default();
        for (spec, tr) in st.specs.iter().zip(st.tracks.iter()) {
            let Some(def) = defs.get(&spec.name) else {
                continue;
            };
            if !def.touches_layout {
                continue;
            }
            if let Some(t) = spec.sample_progress(tr.elapsed_ms, reduced) {
                let (_, next_layout) =
                    sample_keyframes_with_timing(def, &base_paint, &target, t, spec.timing);
                target = next_layout;
            }
        }
    }
    apply_layout_if_changed(inner, node_id, target)
}

fn apply_layout_if_changed(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    target: LayoutStyle,
) -> bool {
    let mut taffy_upd = None;
    if let Some(node) = inner.node_mut(node_id) {
        if node.style.layout == target {
            return false;
        }
        node.style.layout = target;
        node.set_text_dirty(true);
        taffy_upd = node.taffy_node;
    }
    if let Some(tn) = taffy_upd {
        crate::view_tree::semantics::set_effective_taffy_layout(inner, node_id, tn, target);
        sync_node_text_context(inner, node_id);
    }
    inner.paint_dirty = true;
    true
}

fn track_waiting_in_delay(spec: &AnimationSpec, elapsed_ms: f64) -> bool {
    spec.delay_ms > 0 && elapsed_ms < spec.delay_ms as f64
}

pub(crate) fn next_wake_ms(inner: &ViewTreeInner) -> Option<u64> {
    if inner.animations_paused || inner.keyframes.nodes.is_empty() {
        return None;
    }
    let reduced = inner.effective_reduced_motion();
    if reduced {
        return None;
    }
    let mut min_delay: Option<f64> = None;
    for (&nid, st) in &inner.keyframes.nodes {
        let visible = is_node_animation_visible(inner, nid);
        for (spec, tr) in st.specs.iter().zip(st.tracks.iter()) {
            let def = inner.keyframes.definitions.get(&spec.name);
            if !track_is_active(spec, tr.elapsed_ms, def, visible, reduced) {
                continue;
            }
            if !track_waiting_in_delay(spec, tr.elapsed_ms) {
                return Some(0);
            }
            let rem = (spec.delay_ms as f64 - tr.elapsed_ms).max(0.0);
            min_delay = Some(min_delay.map_or(rem, |m| m.min(rem)));
        }
    }
    min_delay.map(|d| d.ceil().max(1.0) as u64)
}

pub(crate) fn sync_node_css_animations(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    next_ref: AnimationListRef,
) {
    let prev = inner
        .sparse
        .last_cascaded_animations
        .get(&node_id)
        .copied()
        .unwrap_or(AnimationListRef::Empty);
    if prev == next_ref {
        return;
    }
    inner
        .sparse
        .last_cascaded_animations
        .insert(node_id, next_ref);
    set_node_animations_inner(inner, node_id, next_ref.to_vec());
}

pub(crate) fn tick_keyframes(inner: &mut ViewTreeInner, dt_seconds: f32) -> usize {
    if inner.animations_paused || inner.keyframes.nodes.is_empty() {
        return 0;
    }
    let dt_ms = if dt_seconds.is_finite() && dt_seconds > 0.0 {
        (dt_seconds as f64) * 1000.0
    } else {
        0.0
    };
    let reduced = inner.effective_reduced_motion();
    let node_ids: Vec<NodeId> = inner.keyframes.nodes.keys().copied().collect();
    let mut active_count = 0usize;
    let mut any_paint = false;
    let mut tier2_nodes = Vec::new();
    for nid in node_ids {
        let visible = is_node_animation_visible(inner, nid);
        let defs = &inner.keyframes.definitions;
        let Some(st) = inner.keyframes.nodes.get_mut(&nid) else {
            continue;
        };
        let mut has_tier2 = st.base_layout.is_some();
        for (spec, tr) in st.specs.iter().zip(st.tracks.iter_mut()) {
            let def = defs.get(&spec.name);
            if track_is_active(spec, tr.elapsed_ms, def, visible, reduced) {
                let was_wait = track_waiting_in_delay(spec, tr.elapsed_ms);
                tr.elapsed_ms += dt_ms;
                let now_wait = track_waiting_in_delay(spec, tr.elapsed_ms);
                active_count += 1;
                if !(was_wait && now_wait) {
                    if def.is_some_and(|d| d.touches_layout) {
                        has_tier2 = true;
                    } else {
                        any_paint = true;
                    }
                }
            }
        }
        if has_tier2 {
            tier2_nodes.push(nid);
        }
    }
    if any_paint {
        inner.paint_dirty = true;
    }
    for nid in tier2_nodes {
        let _ = sync_node_tier2_layout(inner, nid);
    }
    active_count
}

pub(crate) fn set_node_animations_inner(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    specs: Vec<AnimationSpec>,
) {
    if !inner
        .node(node_id)
        .is_some_and(|n| n.alive() && !n.is_anchor())
    {
        return;
    }
    if specs.is_empty() {
        if let Some(old) = inner.keyframes.nodes.remove(&node_id) {
            if let Some(base) = old.base_layout {
                let _ = apply_layout_if_changed(inner, node_id, base);
            }
        }
        return;
    }
    let old = inner.keyframes.nodes.remove(&node_id);
    let mut old_elapsed: HashMap<String, f64> = HashMap::new();
    let base_layout = old.as_ref().and_then(|o| o.base_layout);
    if let Some(prev) = old {
        for tr in prev.tracks {
            old_elapsed.entry(tr.name).or_insert(tr.elapsed_ms);
        }
    }
    let tracks = specs
        .iter()
        .map(|s| AnimationTrack {
            name: s.name.clone(),
            elapsed_ms: old_elapsed.get(&s.name).copied().unwrap_or(0.0),
        })
        .collect();
    inner.keyframes.nodes.insert(
        node_id,
        NodeKeyframeState {
            specs,
            tracks,
            base_layout,
        },
    );
    let _ = sync_node_tier2_layout(inner, node_id);
    inner.paint_dirty = true;
}

pub(crate) fn resync_all_keyframes(inner: &mut ViewTreeInner) {
    let ids: Vec<NodeId> = inner.keyframes.nodes.keys().copied().collect();
    for nid in ids {
        let _ = sync_node_tier2_layout(inner, nid);
    }
    if !inner.keyframes.nodes.is_empty() {
        inner.paint_dirty = true;
    }
}

impl ViewTree {
    /// Registers or updates document `@keyframes` definitions without discarding other rules.
    pub fn register_keyframes(&mut self, defs: Vec<KeyframesDef>) {
        if defs.is_empty() {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        for def in defs {
            inner.keyframes.definitions.insert(def.name.clone(), def);
        }
        resync_all_keyframes(&mut inner);
    }

    /// Replaces the active document `@keyframes` table (used during debug CSS hot-patch).
    pub fn replace_keyframes(&mut self, defs: Vec<KeyframesDef>) {
        let mut inner = self.inner.borrow_mut();
        inner.keyframes.definitions.clear();
        for def in defs {
            inner.keyframes.definitions.insert(def.name.clone(), def);
        }
        resync_all_keyframes(&mut inner);
    }

    /// Attaches compiled `animation` declarations (`AnimationSpec` list) to `node_id`.
    pub fn set_node_animations(&mut self, node_id: NodeId, specs: Vec<AnimationSpec>) {
        let mut inner = self.inner.borrow_mut();
        set_node_animations_inner(&mut inner, node_id, specs);
    }

    /// Returns the number of actively advancing `@keyframes` tracks (excludes paused, hidden,
    /// `prefers-reduced-motion: reduce`, unmounted, and finished animations).
    pub fn active_keyframe_count(&self) -> usize {
        active_track_count(&self.inner.borrow())
    }
}
