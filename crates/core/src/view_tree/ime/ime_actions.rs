//! IME composition state machine (`ime_enabled`, `ime_preedit`, `ime_commit`, `ime_finish_composing`, `ime_disabled`).

use crate::view_tree::grapheme::{
    sanitize_multiline_text, sanitize_single_line_text, update_input_scroll_x,
    update_textarea_scroll_y,
};
use crate::view_tree::key_routing::resolve_editable_line_caret_rect;
use crate::view_tree::node::{
    char_to_byte_idx, compute_node_shaped_key, delete_active_selection_in_node, ImeState,
};
use crate::view_tree::paint_text::compute_shaped_width_key;
use crate::view_tree::paint_text_decorations::single_line_input_align_dx;
use crate::view_tree::text_edit::{
    compose_node_display_and_ime_ranges, insert_clean_str_at_cursor,
};
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_signals::batch;
use krui_style::CompiledStyle;

impl ViewTree {
    /// Returns `true` if `ime_target` or the focused `<input>` currently has an active IME preedit composition string.
    pub fn has_active_ime_preedit(&self) -> bool {
        let inner = self.inner.borrow();
        let nid = inner.ime_target.or_else(|| {
            inner
                .a11y_engine
                .focused_id()
                .and_then(|ak_id| NodeId::try_from(ak_id).ok())
        });
        nid.filter(|&id| inner.node(id).is_some())
            .and_then(|id| inner.text_edit(id))
            .is_some_and(|e| !e.ime_state.preedit.is_empty())
    }

    /// Returns the `ImeState` of `ime_target` (if pinned) or the currently focused `<input>` node, if any.
    pub fn focused_ime_state(&self) -> Option<ImeState> {
        let inner = self.inner.borrow();
        let nid = inner.ime_target.or_else(|| {
            inner
                .a11y_engine
                .focused_id()
                .and_then(|ak_id| NodeId::try_from(ak_id).ok())
        })?;
        let node = inner.node(nid)?;
        node.role.is_text_editable().then(|| {
            inner
                .text_edit(nid)
                .map(|e| e.ime_state.clone())
                .unwrap_or_default()
        })
    }

    /// Notifies the focused `<input>` that the platform IME session has been enabled (`Ime::Enabled`).
    pub fn ime_enabled(&mut self) -> bool {
        self.with_focused_input(|ctx| {
            ctx.edit.ime_state.enabled = true;
            ctx.edit.ime_state.pending_blur_commit = None;
            Some(())
        })
        .is_some()
    }

    /// Updates the in-progress IME composition string (`Ime::Preedit(text, cursor_offset)`)
    /// on `ime_target` (or pins the currently focused `<input>` as `ime_target` when composition starts).
    pub fn ime_preedit(&mut self, preedit: &str, cursor_offset: Option<(usize, usize)>) -> bool {
        let was_pinned = self.inner.borrow().ime_target.is_some();
        self.with_ime_target_or_focused_input_opts(true, |ctx, _nid| {
            let clean = if ctx.node.role == SemanticRole::MultilineTextInput {
                sanitize_multiline_text(preedit)
            } else {
                sanitize_single_line_text(preedit)
            };
            ctx.edit.ime_state.enabled = true;
            let sel_deleted = if !clean.is_empty() {
                let d = delete_active_selection_in_node(ctx);
                ctx.edit.ime_state.pending_blur_commit = None;
                d
            } else {
                false
            };
            let keep_pinned = if !clean.is_empty() {
                true
            } else {
                was_pinned
                    || !ctx.edit.ime_state.preedit.is_empty()
                    || ctx.edit.ime_state.pending_blur_commit.is_some()
            };
            let clamped_cursor = cursor_offset.map(|(s, e)| {
                let a = s.min(clean.len());
                let b = e.min(clean.len());
                (a.min(b), a.max(b))
            });
            let text_changed = sel_deleted || ctx.edit.ime_state.preedit != clean;
            let cursor_changed = ctx.edit.ime_state.preedit_cursor != clamped_cursor;
            if text_changed || cursor_changed {
                ctx.edit.ime_state.preedit = clean;
                ctx.edit.ime_state.preedit_cursor = clamped_cursor;
                ctx.edit.preferred_x = None;
                ctx.edit.needs_caret_scroll = true;
                if text_changed {
                    ctx.node.set_text_dirty(true);
                }
            }
            Some(((), keep_pinned))
        })
        .is_some()
    }

    /// Commits finalized IME text (`Ime::Commit(text)`) into `ime_target` (or the focused `<input>`),
    /// always unpinning `ime_target` when composition finishes.
    pub fn ime_commit(&mut self, raw_committed: &str) -> bool {
        let Some((cb, updated_text, handled)) =
            self.with_ime_target_or_focused_input(|ctx, _nid| {
                let clean = if ctx.node.role == SemanticRole::MultilineTextInput {
                    sanitize_multiline_text(raw_committed)
                } else {
                    sanitize_single_line_text(raw_committed)
                };
                ctx.edit.ime_state.enabled = true;
                ctx.edit.ime_state.preedit.clear();
                ctx.edit.ime_state.preedit_cursor = None;
                ctx.edit.preferred_x = None;
                ctx.edit.needs_caret_scroll = true;
                ctx.node.set_text_dirty(true);

                if let Some((start_char, end_char, blur_committed)) =
                    ctx.edit.ime_state.pending_blur_commit.take()
                {
                    if blur_committed == clean {
                        return Some(((None, String::new(), true), false));
                    }
                    let char_count = ctx.node.text.chars().count();
                    let s = start_char.min(char_count);
                    let e = end_char.min(char_count).max(s);
                    let b0 = char_to_byte_idx(&ctx.node.text, s);
                    let b1 = char_to_byte_idx(&ctx.node.text, e);
                    ctx.node.text.replace_range(b0..b1, &clean);
                    ctx.edit.cursor_char_idx = s + clean.chars().count();
                    ctx.edit.selection_anchor_char_idx = None;
                    return Some(((ctx.on_input.clone(), ctx.node.text.clone(), true), false));
                }
                if clean.is_empty() {
                    return Some(((None, String::new(), false), false));
                }
                let (cb, txt) = insert_clean_str_at_cursor(ctx, &clean);
                Some(((cb, txt, true), false))
            })
        else {
            return false;
        };

        if let Some(on_input) = cb {
            batch(|| on_input(updated_text));
        }
        handled
    }

    /// Finishes an active IME composition in-place (`ImeEvent::FinishComposing`), committing any
    /// remaining `preedit` into `ime_target` and unpinning `ime_target`.
    pub fn ime_finish_composing(&mut self) -> bool {
        let Some((cb, updated_text)) = self.with_ime_target_or_focused_input(|ctx, _nid| {
            ctx.edit.ime_state.pending_blur_commit = None;
            ctx.edit.ime_state.preedit_cursor = None;
            if ctx.edit.ime_state.preedit.is_empty() {
                return Some(((None, String::new()), false));
            }
            let preedit_str = std::mem::take(&mut ctx.edit.ime_state.preedit);
            Some((insert_clean_str_at_cursor(ctx, &preedit_str), false))
        }) else {
            return false;
        };
        if let Some(on_input) = cb {
            batch(|| on_input(updated_text));
        }
        true
    }

    /// Notifies `ime_target` (or the focused `<input>`) that the platform IME session has been disabled (`Ime::Disabled`).
    pub fn ime_disabled(&mut self) -> bool {
        self.with_ime_target_or_focused_input(|ctx, _nid| {
            ctx.edit.ime_state.enabled = false;
            ctx.edit.ime_state.preedit.clear();
            ctx.edit.ime_state.preedit_cursor = None;
            ctx.edit.ime_state.pending_blur_commit = None;
            ctx.node.set_text_dirty(true);
            Some(((), false))
        })
        .is_some()
    }
}

type ImeNodeShapeState = (
    SemanticRole,
    String,
    usize,
    CompiledStyle,
    f32,
    Option<f32>,
    u64,
    bool,
);

fn extract_ime_node_shape_state(
    inner: &ViewTreeInner,
    nid: NodeId,
    eff_rect: LayoutRect,
    scale_pct: u32,
) -> Option<ImeNodeShapeState> {
    let node = inner.node(nid)?;
    let eff_bw = inner
        .active_transition_paint(nid, node.style.paint)
        .border_width;
    let (max_w, w_key) = compute_shaped_width_key(node.role, &node.style, eff_rect.width);
    let s_key = compute_node_shaped_key(&node.style.text, scale_pct, w_key);
    let c = compose_node_display_and_ime_ranges(inner, nid);
    let need =
        node.text_dirty() || node.cached_text_layout.is_none() || node.last_shaped_key != s_key;
    Some((
        node.role,
        c.display_string,
        c.active_caret_char,
        node.style,
        eff_bw,
        max_w,
        s_key,
        need,
    ))
}

pub(crate) fn resolve_ime_local_caret_rect(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    eff_rect: LayoutRect,
    caret_width: f32,
) -> Option<LayoutRect> {
    let scale_pct = (inner.scale_factor.clamp(1.0, 4.0) * 100.0).round() as u32;
    let (role, disp, caret_ch, style, eff_bw, max_w, shaped_key, need) =
        extract_ime_node_shape_state(inner, nid, eff_rect, scale_pct)?;
    let empty_lh = inner.text_engine.measure_empty_line_height(&style.text);
    if disp.is_empty() {
        if let Some(ed) = inner.sparse.text_edits.get_mut(&nid) {
            ed.scroll_x = 0.0;
            ed.scroll_y = 0.0;
        }
        let dx = single_line_input_align_dx(&style, role, eff_rect, 0.0);
        return Some(resolve_editable_line_caret_rect(
            eff_rect,
            &style,
            eff_bw,
            role,
            None,
            0,
            dx,
            0.0,
            empty_lh,
            caret_width,
        ));
    }
    if need {
        shape_ime_node(inner, nid, max_w, shaped_key, &disp, &style);
    }
    compute_ime_scrolled_caret_rect(
        inner,
        nid,
        (role, style, eff_bw, empty_lh, caret_width),
        caret_ch,
        eff_rect,
    )
}

fn compute_ime_scrolled_caret_rect(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    (role, style, eff_bw, empty_lh, caret_width): (SemanticRole, CompiledStyle, f32, f32, f32),
    caret_ch: usize,
    eff_rect: LayoutRect,
) -> Option<LayoutRect> {
    let (raw_x, line_top, line_h, measured_w, measured_h) = inner
        .node(nid)?
        .cached_text_layout
        .as_ref()
        .map(|s| {
            let ln = s.line_for_char(caret_ch);
            (
                s.cursor_x_for_char(caret_ch),
                ln.line_top,
                ln.line_height.max(1.0),
                s.measured_width,
                s.measured_height,
            )
        })
        .unwrap_or((0.0, 0.0, empty_lh.max(1.0), 0.0, 0.0));
    let (cursor_x, scroll_y) = {
        let ed = inner.ensure_text_edit(nid);
        if role == SemanticRole::MultilineTextInput {
            ed.scroll_x = 0.0;
            ed.scroll_y = update_textarea_scroll_y(
                ed.scroll_y,
                ed.needs_caret_scroll,
                line_top,
                line_h,
                measured_h,
                eff_rect,
                &style,
            );
            ed.needs_caret_scroll = false;
            (raw_x.max(0.0), ed.scroll_y)
        } else {
            ed.scroll_x = update_input_scroll_x(ed.scroll_x, raw_x, measured_w, eff_rect, &style);
            ed.scroll_y = 0.0;
            ed.needs_caret_scroll = false;
            let dx = single_line_input_align_dx(&style, role, eff_rect, measured_w);
            ((raw_x - ed.scroll_x + dx).max(0.0), 0.0)
        }
    };
    let shaped = inner.node(nid).and_then(|n| n.cached_text_layout.as_ref());
    Some(resolve_editable_line_caret_rect(
        eff_rect,
        &style,
        eff_bw,
        role,
        shaped,
        caret_ch,
        cursor_x,
        scroll_y,
        empty_lh,
        caret_width,
    ))
}

fn shape_ime_node(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    max_w: Option<f32>,
    shaped_key: u64,
    display_string: &str,
    style: &CompiledStyle,
) {
    let scale = inner.scale_factor;
    let shaped = inner
        .text_engine
        .shape_styled_text(display_string, &style.text, max_w, scale);
    if let Some(node) = inner.node_mut(nid) {
        node.cached_text_layout = Some(shaped);
        node.last_shaped_key = shaped_key;
        node.set_text_dirty(false);
    }
}
