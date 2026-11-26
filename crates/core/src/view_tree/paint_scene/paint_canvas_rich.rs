//! Mixed vector, text, and raster-image `<canvas>` primitive emission split out of [`super::paint_canvas`].

use crate::scene::{
    CanvasCommand, CanvasFillRule, CanvasTextAlign, DrawPrimitive, PaintScene, PathSegment,
};
use crate::view_tree::clip_shape::{intersect_clip_states, ClipState};
use crate::view_tree::hit_test::transform_rect_aabb;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_layout::LayoutRect;
use krui_style::{Affine2, FontFamilyStack, TextAlign, TextDirection};
use std::sync::Arc;

pub(super) fn emit_canvas_commands_to_scene(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    world_rect: LayoutRect,
    clip: ClipState,
    effective_transform: Affine2,
    effective_opacity: f32,
    scene: &mut PaintScene,
) {
    let Some(commands) = inner
        .sparse
        .canvases
        .get(&node_id)
        .map(|cv| cv.canvas_commands.clone())
    else {
        return;
    };
    if commands.is_empty() {
        return;
    }
    let has_rich = commands.iter().any(|cmd| {
        matches!(
            cmd,
            CanvasCommand::DrawText { .. } | CanvasCommand::DrawImage { .. }
        )
    });
    if !has_rich {
        scene.primitives.push(DrawPrimitive::CanvasBatch {
            viewport: world_rect,
            clip: clip.rect,
            clip_radius: clip.max_radius(),
            clip_radii: clip.radii,
            transform: effective_transform,
            opacity: effective_opacity,
            commands,
        });
        return;
    }
    emit_interleaved_canvas_batches(
        inner,
        world_rect,
        clip,
        effective_transform,
        effective_opacity,
        &commands,
        scene,
    );
}

#[derive(Default)]
struct CanvasEmitState {
    local_tx: Affine2,
    clips: Vec<(Affine2, Vec<PathSegment>, CanvasFillRule)>,
    pending: Vec<CanvasCommand>,
    pending_seeded: bool,
}

impl CanvasEmitState {
    fn seed_if_needed(&mut self) {
        if self.pending_seeded {
            return;
        }
        self.pending_seeded = true;
        for (clip_tx, segs, rule) in &self.clips {
            self.pending.push(CanvasCommand::SetTransform(*clip_tx));
            self.pending.push(CanvasCommand::PushClip {
                segments: segs.clone(),
                fill_rule: *rule,
            });
        }
        if self.local_tx != Affine2::IDENTITY || !self.clips.is_empty() {
            self.pending
                .push(CanvasCommand::SetTransform(self.local_tx));
        }
    }

    fn flush_batch(
        &mut self,
        world_rect: LayoutRect,
        clip: ClipState,
        effective_transform: Affine2,
        effective_opacity: f32,
        scene: &mut PaintScene,
    ) {
        if self.pending.is_empty() {
            self.pending_seeded = false;
            return;
        }
        for _ in 0..self.clips.len() {
            self.pending.push(CanvasCommand::PopClip);
        }
        let cmds: Arc<[CanvasCommand]> = Arc::from(std::mem::take(&mut self.pending));
        self.pending_seeded = false;
        scene.primitives.push(DrawPrimitive::CanvasBatch {
            viewport: world_rect,
            clip: clip.rect,
            clip_radius: clip.max_radius(),
            clip_radii: clip.radii,
            transform: effective_transform,
            opacity: effective_opacity,
            commands: cmds,
        });
    }
}

#[derive(Clone, Copy)]
struct CanvasEmitCtx {
    world_rect: LayoutRect,
    clip: ClipState,
    effective_transform: Affine2,
    effective_opacity: f32,
    canvas_origin: Affine2,
}

fn emit_interleaved_canvas_batches(
    inner: &mut ViewTreeInner,
    world_rect: LayoutRect,
    clip: ClipState,
    effective_transform: Affine2,
    effective_opacity: f32,
    commands: &[CanvasCommand],
    scene: &mut PaintScene,
) {
    let mut state = CanvasEmitState::default();
    let ctx = CanvasEmitCtx {
        world_rect,
        clip,
        effective_transform,
        effective_opacity,
        canvas_origin: effective_transform * Affine2::translation(world_rect.x, world_rect.y),
    };
    for cmd in commands {
        emit_interleaved_canvas_cmd(&mut state, inner, cmd, ctx, scene);
    }
    state.flush_batch(
        world_rect,
        clip,
        effective_transform,
        effective_opacity,
        scene,
    );
}

fn emit_interleaved_canvas_cmd(
    state: &mut CanvasEmitState,
    inner: &mut ViewTreeInner,
    cmd: &CanvasCommand,
    ctx: CanvasEmitCtx,
    scene: &mut PaintScene,
) {
    match cmd {
        CanvasCommand::SetTransform(m) => {
            state.local_tx = *m;
            if state.pending_seeded {
                state.pending.push(CanvasCommand::SetTransform(*m));
            }
        }
        CanvasCommand::PushClip {
            segments,
            fill_rule,
        } => {
            state.seed_if_needed();
            state
                .clips
                .push((state.local_tx, segments.clone(), *fill_rule));
            state.pending.push(cmd.clone());
        }
        CanvasCommand::PopClip => {
            let _ = state.clips.pop();
            if state.pending_seeded {
                state.pending.push(CanvasCommand::PopClip);
            }
        }
        CanvasCommand::DrawText { .. } | CanvasCommand::DrawImage { .. } => {
            emit_interleaved_primitive_cmd(state, inner, cmd, ctx, scene);
        }
        vector_cmd => {
            state.seed_if_needed();
            state.pending.push(vector_cmd.clone());
        }
    }
}

fn emit_interleaved_primitive_cmd(
    state: &mut CanvasEmitState,
    inner: &mut ViewTreeInner,
    cmd: &CanvasCommand,
    ctx: CanvasEmitCtx,
    scene: &mut PaintScene,
) {
    state.flush_batch(
        ctx.world_rect,
        ctx.clip,
        ctx.effective_transform,
        ctx.effective_opacity,
        scene,
    );
    let eff_clip = intersect_active_rect_clips(ctx.clip, ctx.canvas_origin, &state.clips);
    let prim_tx = ctx.canvas_origin * state.local_tx;
    match cmd {
        CanvasCommand::DrawText {
            text,
            x,
            y,
            font_size,
            font_weight,
            color,
            align,
        } => push_canvas_text_primitive(
            inner,
            (text, *x, *y, *font_size, *font_weight, *color, *align),
            (prim_tx, eff_clip, ctx.effective_opacity),
            scene,
        ),
        CanvasCommand::DrawImage {
            source,
            src_rect,
            dst_rect,
            alpha,
        } => push_canvas_image_primitive(
            source,
            *src_rect,
            *dst_rect,
            (prim_tx, eff_clip, ctx.effective_opacity * *alpha),
            scene,
        ),
        _ => {}
    }
}

#[allow(clippy::type_complexity)]
fn push_canvas_text_primitive(
    inner: &mut ViewTreeInner,
    (text, x, y, font_size, font_weight, color, align): (
        &str,
        f32,
        f32,
        f32,
        u16,
        krui_style::Color,
        CanvasTextAlign,
    ),
    (world_tx, clip, opacity): (Affine2, ClipState, f32),
    scene: &mut PaintScene,
) {
    if text.is_empty() || opacity <= 0.0 {
        return;
    }
    let scale = inner.scale_factor.clamp(1.0, 4.0);
    let shaped = inner.text_engine.shape_text(
        text,
        font_size,
        1.2,
        font_weight,
        FontFamilyStack::FluentUI,
        None,
        TextAlign::Left,
        TextDirection::Auto,
        scale,
    );
    if shaped.glyphs.is_empty() {
        return;
    }
    let align_dx = match align {
        CanvasTextAlign::Left => 0.0,
        CanvasTextAlign::Center => -shaped.measured_width * 0.5,
        CanvasTextAlign::Right => -shaped.measured_width,
    };
    scene.primitives.push(DrawPrimitive::TextGlyphs {
        origin_x: 0.0,
        origin_y: 0.0,
        clip: clip.rect,
        clip_radius: clip.max_radius(),
        clip_radii: clip.radii,
        transform: world_tx * Affine2::translation(x + align_dx, y),
        opacity,
        color,
        glyphs: shaped.glyphs,
    });
}

fn push_canvas_image_primitive(
    source: &crate::media::ImageSource,
    src_rect: Option<[f32; 4]>,
    dst_rect: LayoutRect,
    (world_tx, clip, opacity): (Affine2, ClipState, f32),
    scene: &mut PaintScene,
) {
    if opacity <= 0.0 || dst_rect.width <= 0.0 || dst_rect.height <= 0.0 {
        return;
    }
    let uv_rect = match src_rect {
        Some([sx, sy, sw, sh]) => {
            let (iw, ih) = source.intrinsic_dimensions();
            let (w, h) = (iw.max(1) as f32, ih.max(1) as f32);
            [
                (sx / w).clamp(0.0, 1.0),
                (sy / h).clamp(0.0, 1.0),
                ((sx + sw) / w).clamp(0.0, 1.0),
                ((sy + sh) / h).clamp(0.0, 1.0),
            ]
        }
        None => [0.0, 0.0, 1.0, 1.0],
    };
    scene.primitives.push(DrawPrimitive::Image {
        rect: dst_rect,
        uv_rect,
        clip: clip.rect,
        clip_radius: clip.max_radius(),
        clip_radii: clip.radii,
        transform: world_tx,
        opacity,
        border_radius: 0.0,
        source: source.clone(),
    });
}

fn intersect_active_rect_clips(
    base: ClipState,
    canvas_origin: Affine2,
    clips: &[(Affine2, Vec<PathSegment>, CanvasFillRule)],
) -> ClipState {
    let mut cur = base;
    for (tx, segs, _) in clips {
        if let Some(rect) = extract_axis_rect_from_segments(segs) {
            let aabb = transform_rect_aabb(canvas_origin * *tx, rect);
            cur = intersect_clip_states(cur, aabb, [0.0; 4]);
        }
    }
    cur
}

fn extract_axis_rect_from_segments(segs: &[PathSegment]) -> Option<LayoutRect> {
    if segs.len() != 5 {
        return None;
    }
    let (
        PathSegment::MoveTo(x0, y0),
        PathSegment::LineTo(x1, y1),
        PathSegment::LineTo(x2, y2),
        PathSegment::LineTo(x3, y3),
        PathSegment::Close,
    ) = (segs[0], segs[1], segs[2], segs[3], segs[4])
    else {
        return None;
    };
    if (y0 - y1).abs() > 1e-3
        || (x1 - x2).abs() > 1e-3
        || (y2 - y3).abs() > 1e-3
        || (x3 - x0).abs() > 1e-3
    {
        return None;
    }
    Some(LayoutRect {
        x: x0.min(x2),
        y: y0.min(y2),
        width: (x2 - x0).abs(),
        height: (y2 - y0).abs(),
    })
}
