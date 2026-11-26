//! Lyon 2D path construction and fill/stroke tessellation helpers for `krui-renderer`.

use crate::tessellate::GpuVertex;
use crate::tessellate_aa::{
    resolve_canvas_cmd_style, tessellate_aa_fill, tessellate_aa_hairline_stroke,
    tessellate_aa_scaled_stroke, CanvasCmdStyle,
};
use crate::ATLAS_SIZE;
use krui_core::{CanvasCommand, PathSegment};
use krui_style::{Affine2, Color};
use lyon_path::math::point;
use lyon_path::Path as LyonPath;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
};

const SOLID_WHITE_UV: [f32; 2] = [1.5 / ATLAS_SIZE as f32, 1.5 / ATLAS_SIZE as f32];
const CIRCLE_KAPPA: f32 = 0.552_284_8;

pub(crate) fn apply_opacity(c: Color, opacity: f32) -> [f32; 4] {
    let mut arr = c.to_f32_array();
    arr[3] = (arr[3] * opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    arr
}

pub(crate) fn to_ndc(px: f32, py: f32, vw: f32, vh: f32) -> [f32; 2] {
    [(px / vw) * 2.0 - 1.0, 1.0 - (py / vh) * 2.0]
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tessellate_canvas_cmd(
    cmd: &CanvasCommand,
    phys_tx: Affine2,
    opacity: f32,
    phys_w: f32,
    phys_h: f32,
    fill_tess: &mut FillTessellator,
    stroke_tess: &mut StrokeTessellator,
    mesh: &mut VertexBuffers<GpuVertex, u32>,
) {
    match *cmd {
        CanvasCommand::FillGradient {
            ref segments,
            fill_rule,
            ref gradient,
            alpha,
        } => {
            crate::tessellate_canvas_grad::tessellate_canvas_gradient_fill(
                segments,
                fill_rule,
                gradient,
                alpha,
                phys_tx,
                opacity,
                (phys_w, phys_h),
                fill_tess,
                mesh,
            );
            return;
        }
        CanvasCommand::SetTransform(_)
        | CanvasCommand::PushClip { .. }
        | CanvasCommand::PopClip
        | CanvasCommand::DrawText { .. }
        | CanvasCommand::DrawImage { .. } => return,
        _ => {}
    }
    match resolve_canvas_cmd_style(cmd) {
        CanvasCmdStyle::Fill(rule, color) => {
            let path = build_canvas_cmd_path(cmd, phys_tx);
            let rgba = apply_opacity(color, opacity);
            tessellate_aa_fill(&path, rule, rgba, (phys_w, phys_h), fill_tess, mesh);
        }
        CanvasCmdStyle::Stroke {
            width,
            cap,
            join,
            color,
        } => {
            let rgba = apply_opacity(color, opacity);
            tessellate_aa_canvas_stroke(
                cmd,
                phys_tx,
                (width, cap, join),
                rgba,
                (phys_w, phys_h),
                stroke_tess,
                mesh,
            );
        }
    }
}

fn tessellate_aa_canvas_stroke(
    cmd: &CanvasCommand,
    phys_tx: Affine2,
    (line_width, cap, join): (f32, LineCap, LineJoin),
    rgba: [f32; 4],
    (phys_w, phys_h): (f32, f32),
    stroke_tess: &mut StrokeTessellator,
    mesh: &mut VertexBuffers<GpuVertex, u32>,
) {
    let sx = phys_tx.a.hypot(phys_tx.b);
    let sy = phys_tx.c.hypot(phys_tx.d);
    let s_min = sx.min(sy);
    let s_max = sx.max(sy).max(1e-4);
    if line_width * s_min >= 1.25 {
        let local_path = build_canvas_cmd_path(cmd, Affine2::IDENTITY);
        tessellate_aa_scaled_stroke(
            &local_path,
            phys_tx,
            (line_width, cap, join, s_max),
            rgba,
            (phys_w, phys_h),
            stroke_tess,
            mesh,
        );
    } else {
        let w_phys = line_width * 0.5 * (sx + sy);
        let phys_path = build_canvas_cmd_path(cmd, phys_tx);
        tessellate_aa_hairline_stroke(
            &phys_path,
            w_phys,
            cap,
            join,
            rgba,
            (phys_w, phys_h),
            stroke_tess,
            mesh,
        );
    }
}

fn build_canvas_cmd_path(cmd: &CanvasCommand, tx: Affine2) -> LyonPath {
    match *cmd {
        CanvasCommand::FillRect {
            x, y, w, h, radius, ..
        } => build_rect_lyon_path(x, y, w, h, radius, tx),
        CanvasCommand::StrokeRect { x, y, w, h, .. } => build_rect_lyon_path(x, y, w, h, 0.0, tx),
        CanvasCommand::FillCircle { cx, cy, radius, .. } => {
            build_circle_lyon_path(cx, cy, radius, tx)
        }
        CanvasCommand::StrokeLine { x1, y1, x2, y2, .. } => {
            build_line_lyon_path(x1, y1, x2, y2, tx)
        }
        CanvasCommand::FillPath { ref segments, .. }
        | CanvasCommand::StrokePath { ref segments, .. } => build_custom_lyon_path(segments, tx),
        CanvasCommand::SetTransform(_)
        | CanvasCommand::PushClip { .. }
        | CanvasCommand::PopClip
        | CanvasCommand::FillGradient { .. }
        | CanvasCommand::DrawText { .. }
        | CanvasCommand::DrawImage { .. } => LyonPath::builder().build(),
    }
}

fn build_line_lyon_path(x1: f32, y1: f32, x2: f32, y2: f32, tx: Affine2) -> LyonPath {
    let mut b = LyonPath::builder();
    let ((p1x, p1y), (p2x, p2y)) = (tx.transform_point(x1, y1), tx.transform_point(x2, y2));
    b.begin(point(p1x, p1y));
    b.line_to(point(p2x, p2y));
    b.end(false);
    b.build()
}

pub(crate) fn build_rect_lyon_path(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    tx: Affine2,
) -> LyonPath {
    let mut b = LyonPath::builder();
    if w <= 0.0 || h <= 0.0 {
        return b.build();
    }
    let max_scale = tx.a.hypot(tx.b).max(tx.c.hypot(tx.d)).max(1.0);
    let r = radius.clamp(0.0, (w.min(h) * 0.5).max(0.0));
    let p = |lx: f32, ly: f32| {
        let (px, py) = tx.transform_point(lx, ly);
        point(px, py)
    };
    if r * max_scale <= 0.05 {
        b.begin(p(x, y));
        b.line_to(p(x + w, y));
        b.line_to(p(x + w, y + h));
        b.line_to(p(x, y + h));
        b.close();
        return b.build();
    }
    let k = r * CIRCLE_KAPPA;
    b.begin(p(x + r, y));
    b.line_to(p(x + w - r, y));
    b.cubic_bezier_to(p(x + w - r + k, y), p(x + w, y + r - k), p(x + w, y + r));
    b.line_to(p(x + w, y + h - r));
    b.cubic_bezier_to(
        p(x + w, y + h - r + k),
        p(x + w - r + k, y + h),
        p(x + w - r, y + h),
    );
    b.line_to(p(x + r, y + h));
    b.cubic_bezier_to(p(x + r - k, y + h), p(x, y + h - r + k), p(x, y + h - r));
    b.line_to(p(x, y + r));
    b.cubic_bezier_to(p(x, y + r - k), p(x + r - k, y), p(x + r, y));
    b.close();
    b.build()
}

pub(crate) fn build_circle_lyon_path(cx: f32, cy: f32, radius: f32, tx: Affine2) -> LyonPath {
    let mut b = LyonPath::builder();
    if radius <= 0.0 {
        return b.build();
    }
    let k = radius * CIRCLE_KAPPA;
    let p = |lx: f32, ly: f32| {
        let (px, py) = tx.transform_point(lx, ly);
        point(px, py)
    };
    b.begin(p(cx + radius, cy));
    b.cubic_bezier_to(
        p(cx + radius, cy + k),
        p(cx + k, cy + radius),
        p(cx, cy + radius),
    );
    b.cubic_bezier_to(
        p(cx - k, cy + radius),
        p(cx - radius, cy + k),
        p(cx - radius, cy),
    );
    b.cubic_bezier_to(
        p(cx - radius, cy - k),
        p(cx - k, cy - radius),
        p(cx, cy - radius),
    );
    b.cubic_bezier_to(
        p(cx + k, cy - radius),
        p(cx + radius, cy - k),
        p(cx + radius, cy),
    );
    b.close();
    b.build()
}

pub(crate) fn build_custom_lyon_path(segments: &[PathSegment], tx: Affine2) -> LyonPath {
    let mut b = LyonPath::builder();
    let mut open = false;
    for &seg in segments {
        match seg {
            PathSegment::MoveTo(x, y) => push_move_lyon(&mut b, &mut open, x, y, tx),
            PathSegment::LineTo(x, y) => push_line_lyon(&mut b, &mut open, x, y, tx),
            PathSegment::QuadTo { cx, cy, x, y } => {
                push_quad_lyon(&mut b, &mut open, (cx, cy, x, y), tx);
            }
            PathSegment::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => push_cubic_lyon(&mut b, &mut open, (c1x, c1y, c2x, c2y, x, y), tx),
            PathSegment::Arc {
                cx,
                cy,
                rx,
                ry,
                start_rad,
                sweep_rad,
                x_rotation_rad,
            } => push_arc_lyon(
                &mut b,
                &mut open,
                (cx, cy, rx, ry, start_rad, sweep_rad, x_rotation_rad),
                tx,
            ),
            PathSegment::Close if open => {
                b.close();
                open = false;
            }
            PathSegment::Close => {}
        }
    }
    if open {
        b.end(false);
    }
    b.build()
}

fn push_move_lyon(b: &mut lyon_path::path::Builder, open: &mut bool, x: f32, y: f32, tx: Affine2) {
    if *open {
        b.end(false);
    }
    let (px, py) = tx.transform_point(x, y);
    b.begin(point(px, py));
    *open = true;
}

fn push_line_lyon(b: &mut lyon_path::path::Builder, open: &mut bool, x: f32, y: f32, tx: Affine2) {
    let (px, py) = tx.transform_point(x, y);
    if !*open {
        b.begin(point(px, py));
        *open = true;
    } else {
        b.line_to(point(px, py));
    }
}

fn push_quad_lyon(
    b: &mut lyon_path::path::Builder,
    open: &mut bool,
    (cx, cy, x, y): (f32, f32, f32, f32),
    tx: Affine2,
) {
    let (pcx, pcy) = tx.transform_point(cx, cy);
    let (px, py) = tx.transform_point(x, y);
    if !*open {
        b.begin(point(px, py));
        *open = true;
    } else {
        b.quadratic_bezier_to(point(pcx, pcy), point(px, py));
    }
}

pub(crate) fn tessellate_fill(
    path: &LyonPath,
    color: [f32; 4],
    vw: f32,
    vh: f32,
    tess: &mut FillTessellator,
    mesh: &mut VertexBuffers<GpuVertex, u32>,
) {
    let _ = tess.tessellate_path(
        path,
        &FillOptions::non_zero(),
        &mut BuffersBuilder::new(mesh, |vertex: FillVertex| GpuVertex {
            position: to_ndc(vertex.position().x, vertex.position().y, vw, vh),
            uv: SOLID_WHITE_UV,
            color,
        }),
    );
}

pub(crate) fn tessellate_stroke(
    path: &LyonPath,
    line_width: f32,
    color: [f32; 4],
    vw: f32,
    vh: f32,
    tess: &mut StrokeTessellator,
    mesh: &mut VertexBuffers<GpuVertex, u32>,
) {
    let _ = tess.tessellate_path(
        path,
        &StrokeOptions::default().with_line_width(line_width),
        &mut BuffersBuilder::new(mesh, |vertex: StrokeVertex| GpuVertex {
            position: to_ndc(vertex.position().x, vertex.position().y, vw, vh),
            uv: SOLID_WHITE_UV,
            color,
        }),
    );
}

pub(crate) fn push_transformed_textured_quad(
    mesh: &mut VertexBuffers<GpuVertex, u32>,
    corners: [(f32, f32); 4],
    [u0, v0, u1, v1]: [f32; 4],
    color: [f32; 4],
    vw: f32,
    vh: f32,
) {
    let base = mesh.vertices.len() as u32;
    for (pt, uv) in corners
        .into_iter()
        .zip([[u0, v0], [u1, v0], [u1, v1], [u0, v1]])
    {
        mesh.vertices.push(GpuVertex {
            position: to_ndc(pt.0, pt.1, vw, vh),
            uv,
            color,
        });
    }
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

use crate::radii::flatten_arc_to_cubics;

fn push_cubic_lyon(
    b: &mut lyon_path::path::Builder,
    open: &mut bool,
    (c1x, c1y, c2x, c2y, x, y): (f32, f32, f32, f32, f32, f32),
    tx: Affine2,
) {
    let (p1x, p1y) = tx.transform_point(c1x, c1y);
    let (p2x, p2y) = tx.transform_point(c2x, c2y);
    let (px, py) = tx.transform_point(x, y);
    if !*open {
        b.begin(point(px, py));
        *open = true;
    } else {
        b.cubic_bezier_to(point(p1x, p1y), point(p2x, p2y), point(px, py));
    }
}

fn push_arc_lyon(
    b: &mut lyon_path::path::Builder,
    open: &mut bool,
    (cx, cy, rx, ry, start_rad, sweep_rad, x_rot): (f32, f32, f32, f32, f32, f32, f32),
    tx: Affine2,
) {
    let (start_pt, cubics) = flatten_arc_to_cubics(cx, cy, rx, ry, start_rad, sweep_rad, x_rot);
    let (sx, sy) = tx.transform_point(start_pt.0, start_pt.1);
    if !*open {
        b.begin(point(sx, sy));
        *open = true;
    } else {
        b.line_to(point(sx, sy));
    }
    for [c1, c2, end] in cubics {
        let (p1x, p1y) = tx.transform_point(c1.0, c1.1);
        let (p2x, p2y) = tx.transform_point(c2.0, c2.1);
        let (ex, ey) = tx.transform_point(end.0, end.1);
        b.cubic_bezier_to(point(p1x, p1y), point(p2x, p2y), point(ex, ey));
    }
}
