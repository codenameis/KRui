//! Zero-relayout compositor CSS transitions (`transform`, `opacity`, `background-color`,
//! `color`, `border-*`, `box-shadow`, `backdrop-filter`, `outline*`, gradients) plus unified
//! W3C per-property slot reversing and delay/wake timing inside `ViewTree`.

use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_style::{
    apply_masked_lerp, paint_differs_on_mask, PaintStyle, TransitionClause, TransitionCurve,
    TransitionMask,
};

const PAINT_BITS: [u16; 11] = [
    TransitionMask::TRANSFORM,
    TransitionMask::OPACITY,
    TransitionMask::BG_COLOR,
    TransitionMask::TEXT_COLOR,
    TransitionMask::BORDER_COLOR,
    TransitionMask::BOX_SHADOW,
    TransitionMask::BACKDROP_FILTER,
    TransitionMask::BORDER_RADIUS,
    TransitionMask::BORDER_WIDTH,
    TransitionMask::OUTLINE,
    TransitionMask::RADIAL_GRADIENT,
];

/// Shared W3C per-property slot timing + reversing shortening state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SlotTiming {
    pub(crate) duration_ms: f32,
    pub(crate) delay_ms: i32,
    pub(crate) curve: TransitionCurve,
    pub(crate) shortening_factor: f32,
    pub(crate) elapsed_ms: f32,
}

impl SlotTiming {
    pub(crate) fn new(duration_ms: u16, delay_ms: i32, curve: TransitionCurve) -> Self {
        Self {
            duration_ms: duration_ms as f32,
            delay_ms,
            curve,
            shortening_factor: 1.0,
            elapsed_ms: 0.0,
        }
    }

    #[inline]
    pub(crate) fn effective_duration(self) -> f32 {
        (self.duration_ms * self.shortening_factor).max(0.0)
    }

    #[inline]
    pub(crate) fn effective_delay(self) -> f32 {
        if self.delay_ms < 0 {
            (self.delay_ms as f32) * self.shortening_factor
        } else {
            self.delay_ms as f32
        }
    }

    #[inline]
    pub(crate) fn is_in_delay(self) -> bool {
        self.delay_ms > 0 && self.elapsed_ms < self.delay_ms as f32
    }

    #[inline]
    pub(crate) fn remaining_delay_ms(self) -> f32 {
        (self.effective_delay() - self.elapsed_ms).max(0.0)
    }

    #[inline]
    pub(crate) fn is_finished(self) -> bool {
        self.elapsed_ms + 1e-3 >= (self.effective_delay() + self.effective_duration()).max(0.0)
    }

    pub(crate) fn raw_and_eased(self) -> (f32, f32) {
        let del = self.effective_delay();
        let dur = self.effective_duration();
        if self.elapsed_ms <= del {
            return (0.0, 0.0);
        }
        if dur <= 1e-3 {
            return (1.0, 1.0);
        }
        let raw = ((self.elapsed_ms - del) / dur).clamp(0.0, 1.0);
        let eased = if raw >= 1.0 {
            1.0
        } else {
            self.curve.sample(raw)
        };
        (raw, eased)
    }

    pub(crate) fn reversed(
        self,
        new_clause_dur_ms: u16,
        new_delay_ms: i32,
        new_curve: TransitionCurve,
    ) -> Self {
        let (_, v) = self.raw_and_eased();
        let next_f = (v * self.shortening_factor + (1.0 - self.shortening_factor)).clamp(0.0, 1.0);
        Self {
            duration_ms: new_clause_dur_ms as f32,
            delay_ms: new_delay_ms,
            curve: new_curve,
            shortening_factor: next_f,
            elapsed_ms: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunningPaintSlot {
    pub(crate) bit: u16,
    pub(crate) from: PaintStyle,
    pub(crate) to: PaintStyle,
    pub(crate) reversing_adjusted_start: PaintStyle,
    pub(crate) timing: SlotTiming,
}

/// Active per-node compositor paint transition slots.
#[derive(Clone, Debug)]
pub(crate) struct ActivePaintTransition {
    pub(crate) latest_target: PaintStyle,
    pub(crate) slots: Vec<RunningPaintSlot>,
}

impl ActivePaintTransition {
    pub(crate) fn sample_paint(&self) -> PaintStyle {
        let mut out = self.latest_target;
        for slot in &self.slots {
            let (_, eased) = slot.timing.raw_and_eased();
            apply_masked_lerp(
                &slot.from,
                &slot.to,
                TransitionMask(slot.bit),
                eased,
                &mut out,
            );
        }
        out
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.slots.iter().all(|s| s.timing.is_finished())
    }
}

fn find_paint_clause(clauses: &[TransitionClause], bit: u16) -> Option<TransitionClause> {
    clauses
        .iter()
        .rev()
        .find(|c| (c.mask.0 & bit) != 0)
        .copied()
}

fn sync_paint_slot(
    slots: &mut Vec<RunningPaintSlot>,
    bit: u16,
    prev_target: &PaintStyle,
    new_target: &PaintStyle,
    cl: Option<TransitionClause>,
) {
    let mask = TransitionMask(bit);
    let existing_idx = slots.iter().position(|s| s.bit == bit);
    let Some(cl) = cl.filter(|c| c.duration_ms > 0 || c.delay_ms > 0) else {
        if let Some(i) = existing_idx {
            slots.remove(i);
        }
        return;
    };
    if let Some(idx) = existing_idx {
        let old = slots[idx];
        if !paint_differs_on_mask(&old.to, new_target, mask) {
            return;
        }
        let (_, eased) = old.timing.raw_and_eased();
        let mut cur = old.to;
        apply_masked_lerp(&old.from, &old.to, mask, eased, &mut cur);
        if !paint_differs_on_mask(&old.reversing_adjusted_start, new_target, mask) {
            slots[idx] = RunningPaintSlot {
                bit,
                from: cur,
                to: *new_target,
                reversing_adjusted_start: old.to,
                timing: old.timing.reversed(cl.duration_ms, cl.delay_ms, cl.curve),
            };
        } else {
            slots[idx] = RunningPaintSlot {
                bit,
                from: cur,
                to: *new_target,
                reversing_adjusted_start: *prev_target,
                timing: SlotTiming::new(cl.duration_ms, cl.delay_ms, cl.curve),
            };
        }
        return;
    }
    if paint_differs_on_mask(prev_target, new_target, mask) {
        slots.push(RunningPaintSlot {
            bit,
            from: *prev_target,
            to: *new_target,
            reversing_adjusted_start: *prev_target,
            timing: SlotTiming::new(cl.duration_ms, cl.delay_ms, cl.curve),
        });
    }
}

/// Seeds initial baselines once after initial layout/mount so children inside `opacity: 0` or
/// scrolled containers still receive entry transitions when later mutated.
pub(crate) fn seed_initial_transition_baselines(inner: &mut ViewTreeInner) {
    let candidates: Vec<NodeId> = inner
        .nodes
        .iter()
        .filter(|n| n.alive() && !n.is_anchor())
        .map(|n| n.id)
        .collect();
    for nid in candidates {
        if inner.sparse.last_target_paints.contains_key(&nid) {
            continue;
        }
        let is_focused =
            inner.a11y_engine.focused_id() == Some(crate::view_tree::A11yId::from(nid).0);
        let Some((raw_paint, layout, fs)) = inner.node(nid).map(|n| {
            let rp = crate::view_tree::paint::resolve_node_paint_state(
                inner,
                nid,
                n,
                n.is_disabled(),
                is_focused && !n.is_disabled(),
            );
            (rp, n.style.layout, n.style.text.font_size)
        }) else {
            continue;
        };
        inner.sparse.last_target_paints.insert(nid, raw_paint);
        inner.sparse.last_target_layouts.insert(nid, (layout, fs));
        inner
            .sparse
            .last_cascaded_animations
            .insert(nid, raw_paint.animations);
    }
}

/// Commits state/style/animation transitions on `node_id` immediately upon style change.
pub(crate) fn commit_node_transitions_from_style(inner: &mut ViewTreeInner, node_id: NodeId) {
    let is_focused =
        inner.a11y_engine.focused_id() == Some(crate::view_tree::A11yId::from(node_id).0);
    let Some(raw_paint) = inner
        .node(node_id)
        .filter(|n| n.alive() && !n.is_anchor())
        .map(|n| {
            crate::view_tree::paint::resolve_node_paint_state(
                inner,
                node_id,
                n,
                n.is_disabled(),
                is_focused && !n.is_disabled(),
            )
        })
    else {
        return;
    };
    crate::view_tree::keyframes_driver::sync_node_css_animations(
        inner,
        node_id,
        raw_paint.animations,
    );
    let _ = inner.resolve_css_transition_paint(node_id, raw_paint);
}

impl ViewTreeInner {
    /// Returns the unified visual `PaintStyle` (`transition` + `@keyframes`) for read-only
    /// hit-testing, stacking, A11y, and IME cursor geometry queries.
    pub(crate) fn active_transition_paint(&self, id: NodeId, fallback: PaintStyle) -> PaintStyle {
        if self.animations_paused {
            return fallback;
        }
        let trans = self
            .sparse
            .active_transitions
            .get(&id)
            .map_or(fallback, ActivePaintTransition::sample_paint);
        crate::view_tree::keyframes_driver::resolve_node_paint(self, id, trans)
    }

    /// Detects target paint changes on nodes with active CSS `transition` specs and returns
    /// the current frame's interpolated `PaintStyle` (`transition` + `@keyframes`).
    pub(crate) fn resolve_transitioned_node_paint(
        &mut self,
        id: NodeId,
        raw_paint: PaintStyle,
    ) -> PaintStyle {
        let trans_paint = self.resolve_css_transition_paint(id, raw_paint);
        crate::view_tree::keyframes_driver::resolve_node_paint(self, id, trans_paint)
    }

    pub(crate) fn resolve_css_transition_paint(
        &mut self,
        id: NodeId,
        raw_paint: PaintStyle,
    ) -> PaintStyle {
        if self.animations_paused {
            self.sparse.last_target_paints.remove(&id);
            self.sparse.active_transitions.remove(&id);
            return raw_paint;
        }
        if !raw_paint.transition.is_active() {
            self.sparse.last_target_paints.insert(id, raw_paint);
            self.sparse.active_transitions.remove(&id);
            return raw_paint;
        }
        let Some(&prev_target) = self.sparse.last_target_paints.get(&id) else {
            self.sparse.last_target_paints.insert(id, raw_paint);
            return raw_paint;
        };
        if self.mount_batch_depth > 0 || self.layout_engine.relayout_passes() == 0 {
            self.sparse.last_target_paints.insert(id, raw_paint);
            return raw_paint;
        }
        self.sparse.last_target_paints.insert(id, raw_paint);
        let clauses = raw_paint.transition.collect_clauses();
        let mut active =
            self.sparse
                .active_transitions
                .remove(&id)
                .unwrap_or(ActivePaintTransition {
                    latest_target: raw_paint,
                    slots: Vec::new(),
                });
        active.latest_target = raw_paint;
        for bit in PAINT_BITS {
            let cl = find_paint_clause(&clauses, bit);
            sync_paint_slot(&mut active.slots, bit, &prev_target, &raw_paint, cl);
        }
        if active.slots.is_empty() {
            return raw_paint;
        }
        let sampled = active.sample_paint();
        self.sparse.active_transitions.insert(id, active);
        sampled
    }
}

impl ViewTree {
    /// Advances all active Tier-1 paint, Tier-2 layout transitions, and `@keyframes` by `dt_seconds`.
    /// Waiting during positive `delay_ms` avoids redundant `paint_dirty` repaints.
    pub fn tick_transitions(&mut self, dt_seconds: f32) -> usize {
        let dt_ms = dt_seconds.max(0.0) * 1000.0;
        let mut inner = self.inner.borrow_mut();
        crate::view_tree::sync::flush_pending_restyle(&mut inner);
        if inner.animations_paused {
            return 0;
        }
        let mut finished = Vec::new();
        let mut paint_progressed = false;
        for (&id, tr) in &mut inner.sparse.active_transitions {
            for slot in &mut tr.slots {
                let was_delay = slot.timing.is_in_delay();
                slot.timing.elapsed_ms += dt_ms;
                if !(was_delay && slot.timing.is_in_delay()) {
                    paint_progressed = true;
                }
            }
            tr.slots.retain(|s| !s.timing.is_finished());
            if tr.is_finished() {
                finished.push(id);
            }
        }
        for id in finished {
            paint_progressed = true;
            inner.sparse.active_transitions.remove(&id);
        }
        if paint_progressed {
            inner.paint_dirty = true;
        }
        let layout_count =
            crate::view_tree::transitions_layout::tick_layout_transitions(&mut inner, dt_ms);
        let kf_count = crate::view_tree::keyframes_driver::tick_keyframes(&mut inner, dt_seconds);
        inner.sparse.active_transitions.len() + layout_count + kf_count
    }

    /// Returns `true` when any CSS paint/layout transition or `@keyframes` track is active.
    pub fn has_active_transitions(&self) -> bool {
        let mut inner = self.inner.borrow_mut();
        crate::view_tree::sync::flush_pending_restyle(&mut inner);
        !inner.animations_paused
            && (!inner.sparse.active_transitions.is_empty()
                || !inner.sparse.active_layout_transitions.is_empty()
                || crate::view_tree::keyframes_driver::has_active_tracks(&inner))
    }

    /// Returns milliseconds until the next visual animation update (`Some(0)` if actively interpolating,
    /// `Some(ms)` while waiting inside positive `delay_ms`, or `None` when completely idle).
    pub fn next_animation_wake_ms(&self) -> Option<u64> {
        let inner = self.inner.borrow();
        if inner.animations_paused {
            return None;
        }
        let mut min_delay: Option<f32> = None;
        for tr in inner.sparse.active_transitions.values() {
            for slot in &tr.slots {
                if !slot.timing.is_in_delay() {
                    return Some(0);
                }
                let rem = slot.timing.remaining_delay_ms();
                min_delay = Some(min_delay.map_or(rem, |m| m.min(rem)));
            }
        }
        for tr in inner.sparse.active_layout_transitions.values() {
            for slot in &tr.slots {
                if !slot.timing.is_in_delay() {
                    return Some(0);
                }
                let rem = slot.timing.remaining_delay_ms();
                min_delay = Some(min_delay.map_or(rem, |m| m.min(rem)));
            }
        }
        if let Some(kf_wake) = crate::view_tree::keyframes_driver::next_wake_ms(&inner) {
            if kf_wake == 0 {
                return Some(0);
            }
            let kf_f = kf_wake as f32;
            min_delay = Some(min_delay.map_or(kf_f, |m| m.min(kf_f)));
        }
        min_delay.map(|d| d.ceil().max(1.0) as u64)
    }

    /// Evaluates the effective composited `PaintStyle` for `node_id` (states + transitions + `@keyframes`).
    pub fn node_effective_paint(&self, node_id: NodeId) -> Option<PaintStyle> {
        let mut inner = self.inner.borrow_mut();
        crate::view_tree::sync::flush_pending_restyle(&mut inner);
        let is_focused =
            inner.a11y_engine.focused_id() == Some(crate::view_tree::A11yId::from(node_id).0);
        let raw_paint = {
            let n = inner.node(node_id)?;
            if !n.alive() || n.is_anchor() {
                return None;
            }
            crate::view_tree::paint::resolve_node_paint_state(
                &inner,
                node_id,
                n,
                n.is_disabled(),
                is_focused && !n.is_disabled(),
            )
        };
        Some(inner.resolve_transitioned_node_paint(node_id, raw_paint))
    }

    /// Returns the number of currently active CSS compositor/layout transitions.
    pub fn active_transition_count(&self) -> usize {
        let mut inner = self.inner.borrow_mut();
        crate::view_tree::sync::flush_pending_restyle(&mut inner);
        if inner.animations_paused {
            0
        } else {
            inner.sparse.active_transitions.len() + inner.sparse.active_layout_transitions.len()
        }
    }

    /// Globally pauses or resumes CSS transitions and `@keyframes`.
    pub fn set_animations_paused(&mut self, paused: bool) {
        let mut inner = self.inner.borrow_mut();
        if inner.animations_paused != paused {
            inner.animations_paused = paused;
            if paused {
                inner.sparse.clear_transitions();
            }
            inner.paint_dirty = true;
        }
    }

    /// Returns whether CSS transitions and `@keyframes` are globally paused.
    pub fn animations_paused(&self) -> bool {
        self.inner.borrow().animations_paused
    }
}
