//! Localized Tier-2 layout & typography CSS `transition` slots (`width`, `height`,
//! `padding`, `margin`, `gap`, `inset`, `border-width`, `flex*`, `font-size`).

use crate::view_tree::content_box::sync_node_text_context;
use crate::view_tree::transitions::SlotTiming;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_style::{
    apply_layout_masked_lerp, layout_differs_on_mask, LayoutStyle, LayoutTransitionMask,
    TransitionClause, TransitionSpec,
};

const LAYOUT_BITS: [u16; 13] = [
    LayoutTransitionMask::WIDTH,
    LayoutTransitionMask::HEIGHT,
    LayoutTransitionMask::MIN_WIDTH,
    LayoutTransitionMask::MIN_HEIGHT,
    LayoutTransitionMask::MAX_WIDTH,
    LayoutTransitionMask::MAX_HEIGHT,
    LayoutTransitionMask::PADDING,
    LayoutTransitionMask::MARGIN,
    LayoutTransitionMask::GAP,
    LayoutTransitionMask::INSET,
    LayoutTransitionMask::BORDER_WIDTH,
    LayoutTransitionMask::FLEX,
    LayoutTransitionMask::FONT_SIZE,
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunningLayoutSlot {
    pub(crate) bit: u16,
    pub(crate) from: (LayoutStyle, f32),
    pub(crate) to: (LayoutStyle, f32),
    pub(crate) reversing_adjusted_start: (LayoutStyle, f32),
    pub(crate) timing: SlotTiming,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveLayoutTransition {
    pub(crate) latest_target: (LayoutStyle, f32),
    pub(crate) slots: Vec<RunningLayoutSlot>,
}

impl ActiveLayoutTransition {
    pub(crate) fn sample_layout(&self) -> (LayoutStyle, f32) {
        let mut out = self.latest_target;
        for slot in &self.slots {
            let (_, eased) = slot.timing.raw_and_eased();
            let m = LayoutTransitionMask(slot.bit);
            apply_layout_masked_lerp(
                (&slot.from.0, slot.from.1),
                (&slot.to.0, slot.to.1),
                m,
                eased,
                &mut out.0,
                &mut out.1,
            );
        }
        out
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.slots.iter().all(|s| s.timing.is_finished())
    }
}

fn find_layout_clause(clauses: &[TransitionClause], bit: u16) -> Option<TransitionClause> {
    clauses
        .iter()
        .rev()
        .find(|c| (c.layout_mask.0 & bit) != 0)
        .copied()
}

fn sync_layout_slot(
    slots: &mut Vec<RunningLayoutSlot>,
    bit: u16,
    prev_target: (LayoutStyle, f32),
    new_target: (LayoutStyle, f32),
    cl: Option<TransitionClause>,
) {
    let mask = LayoutTransitionMask(bit);
    let existing_idx = slots.iter().position(|s| s.bit == bit);
    let Some(cl) = cl.filter(|c| c.duration_ms > 0 || c.delay_ms > 0) else {
        if let Some(i) = existing_idx {
            slots.remove(i);
        }
        return;
    };
    if let Some(idx) = existing_idx {
        let old = slots[idx];
        if !layout_differs_on_mask((&old.to.0, old.to.1), (&new_target.0, new_target.1), mask) {
            return;
        }
        let (_, eased) = old.timing.raw_and_eased();
        let mut cur = old.to;
        apply_layout_masked_lerp(
            (&old.from.0, old.from.1),
            (&old.to.0, old.to.1),
            mask,
            eased,
            &mut cur.0,
            &mut cur.1,
        );
        let reversing = !layout_differs_on_mask(
            (
                &old.reversing_adjusted_start.0,
                old.reversing_adjusted_start.1,
            ),
            (&new_target.0, new_target.1),
            mask,
        );
        if reversing {
            slots[idx] = RunningLayoutSlot {
                bit,
                from: cur,
                to: new_target,
                reversing_adjusted_start: old.to,
                timing: old.timing.reversed(cl.duration_ms, cl.delay_ms, cl.curve),
            };
        } else {
            slots[idx] = RunningLayoutSlot {
                bit,
                from: cur,
                to: new_target,
                reversing_adjusted_start: prev_target,
                timing: SlotTiming::new(cl.duration_ms, cl.delay_ms, cl.curve),
            };
        }
        return;
    }
    if layout_differs_on_mask(
        (&prev_target.0, prev_target.1),
        (&new_target.0, new_target.1),
        mask,
    ) {
        slots.push(RunningLayoutSlot {
            bit,
            from: prev_target,
            to: new_target,
            reversing_adjusted_start: prev_target,
            timing: SlotTiming::new(cl.duration_ms, cl.delay_ms, cl.curve),
        });
    }
}

/// Evaluates whether a style change on `node_id` initiates or updates a localized Tier-2 layout
/// transition, returning the current effective `(LayoutStyle, font_size)` to apply to Taffy.
pub(crate) fn commit_node_layout_transition(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    spec: TransitionSpec,
    next_layout: LayoutStyle,
    next_fs: f32,
) -> Option<(LayoutStyle, f32)> {
    let next_target = (next_layout, next_fs);
    if inner.animations_paused || !spec.has_layout_transition() {
        inner
            .sparse
            .last_target_layouts
            .insert(node_id, next_target);
        inner.sparse.active_layout_transitions.remove(&node_id);
        return None;
    }
    if inner.mount_batch_depth > 0 || inner.layout_engine.relayout_passes() == 0 {
        inner
            .sparse
            .last_target_layouts
            .insert(node_id, next_target);
        return None;
    }
    let Some(&prev_target) = inner.sparse.last_target_layouts.get(&node_id) else {
        inner
            .sparse
            .last_target_layouts
            .insert(node_id, next_target);
        return None;
    };
    inner
        .sparse
        .last_target_layouts
        .insert(node_id, next_target);
    let clauses = spec.collect_clauses();
    let mut active = inner
        .sparse
        .active_layout_transitions
        .remove(&node_id)
        .unwrap_or(ActiveLayoutTransition {
            latest_target: next_target,
            slots: Vec::new(),
        });
    active.latest_target = next_target;
    for bit in LAYOUT_BITS {
        let cl = find_layout_clause(&clauses, bit);
        sync_layout_slot(&mut active.slots, bit, prev_target, next_target, cl);
    }
    if active.slots.is_empty() {
        return None;
    }
    let eff = active.sample_layout();
    inner
        .sparse
        .active_layout_transitions
        .insert(node_id, active);
    Some(eff)
}

/// Applies `(eff_layout, eff_fs)` directly onto `node_id`'s single Taffy node without dirtying siblings.
pub(crate) fn apply_effective_node_layout_to_taffy(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    eff_layout: LayoutStyle,
    eff_fs: f32,
) {
    let mut taffy_upd = None;
    if let Some(node) = inner.node_mut(node_id) {
        if (node.style.text.font_size - eff_fs).abs() > 1e-3 {
            node.style.text.font_size = eff_fs;
            node.set_text_dirty(true);
        }
        taffy_upd = node.taffy_node;
    }
    if let Some(tn) = taffy_upd {
        crate::view_tree::semantics::set_effective_taffy_layout(inner, node_id, tn, eff_layout);
        sync_node_text_context(inner, node_id);
    }
}

/// Advances all active Tier-2 layout transition slots by `dt_ms`, updating only affected nodes.
pub(crate) fn tick_layout_transitions(inner: &mut ViewTreeInner, dt_ms: f32) -> usize {
    let ids: Vec<NodeId> = inner
        .sparse
        .active_layout_transitions
        .keys()
        .copied()
        .collect();
    if ids.is_empty() {
        return 0;
    }
    let mut apply_list: Vec<(NodeId, LayoutStyle, f32)> = Vec::new();
    let mut finished = Vec::new();
    for id in ids {
        let Some(tr) = inner.sparse.active_layout_transitions.get_mut(&id) else {
            continue;
        };
        let mut any_progressed = false;
        for slot in &mut tr.slots {
            let was_delay = slot.timing.is_in_delay();
            slot.timing.elapsed_ms += dt_ms;
            if !(was_delay && slot.timing.is_in_delay()) {
                any_progressed = true;
            }
        }
        tr.slots.retain(|s| !s.timing.is_finished());
        if any_progressed || tr.is_finished() {
            let (lay, fs) = tr.sample_layout();
            apply_list.push((id, lay, fs));
        }
        if tr.is_finished() {
            finished.push(id);
        }
    }
    for id in finished {
        inner.sparse.active_layout_transitions.remove(&id);
    }
    let has_updates = !apply_list.is_empty();
    for (id, lay, fs) in apply_list {
        apply_effective_node_layout_to_taffy(inner, id, lay, fs);
    }
    if has_updates {
        inner.paint_dirty = true;
    }
    inner.sparse.active_layout_transitions.len()
}
