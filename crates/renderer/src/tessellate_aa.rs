//! Physical-pixel geometric Fringe AA and subpixel hairline energy conservation for `<canvas>`/`<svg>`.

use crate::tessellate::GpuVertex;
use crate::tessellate_primitives::to_ndc;
use crate::ATLAS_SIZE;
use krui_core::{CanvasCommand, CanvasFillRule, CanvasLineCap, CanvasLineJoin};
use krui_style::Color;
use lyon_path::Path as LyonPath;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, LineCap, LineJoin,
    StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
};
use std::collections::HashMap;

pub(crate) const AA_TOLERANCE_PHYS_PX: f32 = 0.12;
const SOLID_WHITE_UV: [f32; 2] = [1.5 / ATLAS_SIZE as f32, 1.5 / ATLAS_SIZE as f32];
const WELD_QUANT_SCALE: f32 = 256.0;
const MIN_HAIRLINE_CORE_PX: f32 = 0.08;
const MAX_MITER_SCALE: f32 = 2.0;

#[derive(Clone, Copy)]
pub(crate) enum CanvasCmdStyle {
    Fill(FillRule, Color),
    Stroke {
        width: f32,
        cap: LineCap,
        join: LineJoin,
        color: Color,
    },
}

fn to_lyon_fill_rule(rule: CanvasFillRule) -> FillRule {
    match rule {
        CanvasFillRule::NonZero => FillRule::NonZero,
        CanvasFillRule::EvenOdd => FillRule::EvenOdd,
    }
}

fn to_lyon_line_cap(cap: CanvasLineCap) -> LineCap {
    match cap {
        CanvasLineCap::Butt => LineCap::Butt,
        CanvasLineCap::Round => LineCap::Round,
        CanvasLineCap::Square => LineCap::Square,
    }
}

fn to_lyon_line_join(join: CanvasLineJoin) -> LineJoin {
    match join {
        CanvasLineJoin::Miter => LineJoin::Miter,
        CanvasLineJoin::Round => LineJoin::Round,
        CanvasLineJoin::Bevel => LineJoin::Bevel,
    }
}

pub(crate) fn resolve_canvas_cmd_style(cmd: &CanvasCommand) -> CanvasCmdStyle {
    match *cmd {
        CanvasCommand::FillRect { color, .. } | CanvasCommand::FillCircle { color, .. } => {
            CanvasCmdStyle::Fill(FillRule::NonZero, color)
        }
        CanvasCommand::FillPath {
            fill_rule, color, ..
        } => CanvasCmdStyle::Fill(to_lyon_fill_rule(fill_rule), color),
        CanvasCommand::StrokeRect {
            line_width, color, ..
        } => CanvasCmdStyle::Stroke {
            width: line_width,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            color,
        },
        CanvasCommand::StrokeLine {
            line_width,
            line_cap,
            color,
            ..
        } => CanvasCmdStyle::Stroke {
            width: line_width,
            cap: to_lyon_line_cap(line_cap),
            join: LineJoin::Miter,
            color,
        },
        CanvasCommand::StrokePath {
            line_width,
            line_cap,
            line_join,
            color,
            ..
        } => CanvasCmdStyle::Stroke {
            width: line_width,
            cap: to_lyon_line_cap(line_cap),
            join: to_lyon_line_join(line_join),
            color,
        },
        CanvasCommand::SetTransform(_)
        | CanvasCommand::PushClip { .. }
        | CanvasCommand::PopClip
        | CanvasCommand::FillGradient { .. }
        | CanvasCommand::DrawText { .. }
        | CanvasCommand::DrawImage { .. } => {
            CanvasCmdStyle::Fill(FillRule::NonZero, Color::TRANSPARENT)
        }
    }
}

#[derive(Clone, Copy, Default)]
struct BoundaryAcc {
    n0: [f32; 2],
    n1: [f32; 2],
    count: u8,
    min_edge_len: f32,
}

impl BoundaryAcc {
    fn push(&mut self, n: [f32; 2], edge_len: f32) {
        if self.count == 0 {
            self.n0 = n;
            self.min_edge_len = edge_len;
        } else if self.count == 1 {
            self.n1 = n;
            self.min_edge_len = self.min_edge_len.min(edge_len);
        } else {
            self.min_edge_len = self.min_edge_len.min(edge_len);
        }
        self.count = self.count.saturating_add(1);
    }

    fn resolve_miter(&self) -> (bool, [f32; 2], bool) {
        if self.count == 0 {
            return (false, [0.0, 0.0], false);
        }
        if self.count == 1 {
            return (true, self.n0, false);
        }
        if self.count > 2 {
            let sum = [self.n0[0] + self.n1[0], self.n0[1] + self.n1[1]];
            let len = sum[0].hypot(sum[1]);
            let dir = if len > 1e-4 {
                [sum[0] / len, sum[1] / len]
            } else {
                self.n0
            };
            return (true, dir, false);
        }
        let dot = (self.n0[0] * self.n1[0] + self.n0[1] * self.n1[1]).clamp(-1.0, 1.0);
        let sum = [self.n0[0] + self.n1[0], self.n0[1] + self.n1[1]];
        let len = sum[0].hypot(sum[1]);
        if len <= 1e-4 {
            return (true, self.n0, false);
        }
        let half_cos = ((1.0 + dot) * 0.5)
            .max(0.0)
            .sqrt()
            .max(1.0 / MAX_MITER_SCALE);
        let scale = (1.0 / half_cos).min(MAX_MITER_SCALE);
        (true, [sum[0] / len * scale, sum[1] / len * scale], true)
    }
}

pub(crate) fn tessellate_aa_fill(
    path: &LyonPath,
    fill_rule: FillRule,
    rgba: [f32; 4],
    (phys_w, phys_h): (f32, f32),
    tess: &mut FillTessellator,
    mesh: &mut VertexBuffers<GpuVertex, u32>,
) {
    if rgba[3] <= 0.0 {
        return;
    }
    let mut raw: VertexBuffers<[f32; 2], u32> = VertexBuffers::with_capacity(128, 384);
    let opts = FillOptions::default()
        .with_fill_rule(fill_rule)
        .with_tolerance(AA_TOLERANCE_PHYS_PX);
    let _ = tess.tessellate_path(
        path,
        &opts,
        &mut BuffersBuilder::new(&mut raw, |v: FillVertex| v.position().to_array()),
    );
    emit_aa_fringe_from_physical_mesh(
        &raw.vertices,
        &raw.indices,
        (true, 1.0),
        rgba,
        phys_w,
        phys_h,
        mesh,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tessellate_aa_scaled_stroke(
    local_path: &LyonPath,
    phys_tx: krui_style::Affine2,
    (line_width, cap, join, s_max): (f32, LineCap, LineJoin, f32),
    rgba: [f32; 4],
    (phys_w, phys_h): (f32, f32),
    tess: &mut StrokeTessellator,
    mesh: &mut VertexBuffers<GpuVertex, u32>,
) {
    if line_width <= 0.0 || rgba[3] <= 0.0 {
        return;
    }
    let tol = (AA_TOLERANCE_PHYS_PX / s_max).clamp(1e-4, 0.25);
    let opts = StrokeOptions::default()
        .with_tolerance(tol)
        .with_line_width(line_width)
        .with_line_cap(cap)
        .with_line_join(join);
    let mut raw: VertexBuffers<[f32; 2], u32> = VertexBuffers::with_capacity(128, 384);
    let _ = tess.tessellate_path(
        local_path,
        &opts,
        &mut BuffersBuilder::new(&mut raw, |v: StrokeVertex| {
            let p = v.position();
            let (px, py) = phys_tx.transform_point(p.x, p.y);
            [px, py]
        }),
    );
    emit_aa_fringe_from_physical_mesh(
        &raw.vertices,
        &raw.indices,
        (true, 1.0),
        rgba,
        phys_w,
        phys_h,
        mesh,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tessellate_aa_hairline_stroke(
    path: &LyonPath,
    w_phys: f32,
    cap: LineCap,
    join: LineJoin,
    rgba: [f32; 4],
    (phys_w, phys_h): (f32, f32),
    tess: &mut StrokeTessellator,
    mesh: &mut VertexBuffers<GpuVertex, u32>,
) {
    if w_phys <= 1e-4 || rgba[3] <= 0.0 {
        return;
    }
    let core_w = (w_phys - 1.0).max(MIN_HAIRLINE_CORE_PX);
    let skirt_w = 1.0_f32;
    let eff_alpha = (rgba[3] * (w_phys / (core_w + skirt_w)).clamp(0.0, 1.0)).clamp(0.0, 1.0);
    let mut raw: VertexBuffers<[f32; 2], u32> = VertexBuffers::with_capacity(128, 384);
    let opts = StrokeOptions::default()
        .with_tolerance(AA_TOLERANCE_PHYS_PX)
        .with_line_width(core_w)
        .with_line_cap(cap)
        .with_line_join(join);
    let _ = tess.tessellate_path(
        path,
        &opts,
        &mut BuffersBuilder::new(&mut raw, |v: StrokeVertex| v.position().to_array()),
    );
    let color = [rgba[0], rgba[1], rgba[2], eff_alpha];
    emit_aa_fringe_from_physical_mesh(
        &raw.vertices,
        &raw.indices,
        (false, skirt_w),
        color,
        phys_w,
        phys_h,
        mesh,
    );
}

pub(crate) fn emit_aa_fringe_from_physical_mesh(
    raw_pts: &[[f32; 2]],
    raw_indices: &[u32],
    (allow_inset, skirt_span): (bool, f32),
    rgba: [f32; 4],
    phys_w: f32,
    phys_h: f32,
    out_mesh: &mut VertexBuffers<GpuVertex, u32>,
) {
    if raw_pts.is_empty() || raw_indices.len() < 3 || rgba[3] <= 0.0 {
        return;
    }
    let (pts, tris) = weld_and_orient_triangles(raw_pts, raw_indices);
    if tris.is_empty() {
        return;
    }
    let boundary_edges = extract_boundary_edges(&tris);
    let (miters, is_boundary, mut d_in) =
        compute_boundary_miters(&pts, &boundary_edges, allow_inset);
    if allow_inset {
        guard_inset_triangles(&pts, &tris, &miters, &mut d_in);
    }
    append_core_and_skirt(
        &pts,
        &tris,
        &boundary_edges,
        &miters,
        &is_boundary,
        &d_in,
        skirt_span,
        rgba,
        (phys_w, phys_h),
        out_mesh,
    );
}

fn cross2(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn weld_and_orient_triangles(
    raw_pts: &[[f32; 2]],
    raw_indices: &[u32],
) -> (Vec<[f32; 2]>, Vec<[u32; 3]>) {
    let mut map: HashMap<(i32, i32), u32> = HashMap::with_capacity(raw_pts.len());
    let mut remap = Vec::with_capacity(raw_pts.len());
    let mut pts = Vec::with_capacity(raw_pts.len());
    for &[x, y] in raw_pts {
        let key = (
            (x * WELD_QUANT_SCALE).round() as i32,
            (y * WELD_QUANT_SCALE).round() as i32,
        );
        let id = *map.entry(key).or_insert_with(|| {
            let next = pts.len() as u32;
            pts.push([x, y]);
            next
        });
        remap.push(id);
    }
    let mut tris = Vec::with_capacity(raw_indices.len() / 3);
    for tri in raw_indices.as_chunks::<3>().0 {
        let a = remap[tri[0] as usize];
        let mut b = remap[tri[1] as usize];
        let mut c = remap[tri[2] as usize];
        if a == b || b == c || a == c {
            continue;
        }
        let area = cross2(pts[a as usize], pts[b as usize], pts[c as usize]);
        if area.abs() <= 1e-4 {
            continue;
        }
        if area < 0.0 {
            std::mem::swap(&mut b, &mut c);
        }
        tris.push([a, b, c]);
    }
    (pts, tris)
}

fn extract_boundary_edges(tris: &[[u32; 3]]) -> Vec<(u32, u32)> {
    let mut counts: HashMap<(u32, u32), i32> = HashMap::with_capacity(tris.len() * 3);
    for &[a, b, c] in tris {
        for (u, v) in [(a, b), (b, c), (c, a)] {
            *counts.entry((u, v)).or_insert(0) += 1;
        }
    }
    let mut edges = Vec::new();
    for &[a, b, c] in tris {
        for (u, v) in [(a, b), (b, c), (c, a)] {
            let fwd = counts.get(&(u, v)).copied().unwrap_or(0);
            let rev = counts.get(&(v, u)).copied().unwrap_or(0);
            if fwd > rev {
                edges.push((u, v));
                counts.insert((u, v), fwd - 1);
            }
        }
    }
    edges
}

fn compute_boundary_miters(
    pts: &[[f32; 2]],
    boundary_edges: &[(u32, u32)],
    allow_inset: bool,
) -> (Vec<[f32; 2]>, Vec<bool>, Vec<f32>) {
    let mut acc = vec![BoundaryAcc::default(); pts.len()];
    for &(u, v) in boundary_edges {
        let pu = pts[u as usize];
        let pv = pts[v as usize];
        let dx = pv[0] - pu[0];
        let dy = pv[1] - pu[1];
        let len = dx.hypot(dy);
        if len > 1e-4 {
            let n = [dy / len, -dx / len];
            acc[u as usize].push(n, len);
            acc[v as usize].push(n, len);
        }
    }
    let mut miters = vec![[0.0_f32; 2]; pts.len()];
    let mut is_boundary = vec![false; pts.len()];
    let mut d_in = vec![0.0_f32; pts.len()];
    for (idx, a) in acc.iter().enumerate() {
        let (is_b, miter, clean_2edge) = a.resolve_miter();
        is_boundary[idx] = is_b;
        miters[idx] = miter;
        if allow_inset && is_b && clean_2edge {
            d_in[idx] = (0.35 * a.min_edge_len).min(0.5);
        }
    }
    (miters, is_boundary, d_in)
}

fn guard_inset_triangles(
    pts: &[[f32; 2]],
    tris: &[[u32; 3]],
    miters: &[[f32; 2]],
    d_in: &mut [f32],
) {
    for _pass in 0..2 {
        for &[i0, i1, i2] in tris {
            let (a, b, c) = (i0 as usize, i1 as usize, i2 as usize);
            if d_in[a] == 0.0 && d_in[b] == 0.0 && d_in[c] == 0.0 {
                continue;
            }
            let area0 = cross2(pts[a], pts[b], pts[c]);
            let pa = inset_pt(pts[a], miters[a], d_in[a]);
            let pb = inset_pt(pts[b], miters[b], d_in[b]);
            let pc = inset_pt(pts[c], miters[c], d_in[c]);
            let area1 = cross2(pa, pb, pc);
            if area1 <= 0.01 * area0 || area1 <= 1e-4 {
                d_in[a] = 0.0;
                d_in[b] = 0.0;
                d_in[c] = 0.0;
            }
        }
    }
}

#[inline]
fn inset_pt(p: [f32; 2], m: [f32; 2], d: f32) -> [f32; 2] {
    [p[0] - d * m[0], p[1] - d * m[1]]
}

#[allow(clippy::too_many_arguments)]
fn append_core_and_skirt(
    pts: &[[f32; 2]],
    tris: &[[u32; 3]],
    boundary_edges: &[(u32, u32)],
    miters: &[[f32; 2]],
    is_boundary: &[bool],
    d_in: &[f32],
    skirt_span: f32,
    rgba: [f32; 4],
    (phys_w, phys_h): (f32, f32),
    out_mesh: &mut VertexBuffers<GpuVertex, u32>,
) {
    let base = out_mesh.vertices.len() as u32;
    let outer_rgba = [rgba[0], rgba[1], rgba[2], 0.0];
    for (idx, &p) in pts.iter().enumerate() {
        let cp = inset_pt(p, miters[idx], d_in[idx]);
        out_mesh.vertices.push(GpuVertex {
            position: to_ndc(cp[0], cp[1], phys_w, phys_h),
            uv: SOLID_WHITE_UV,
            color: rgba,
        });
    }
    for &[a, b, c] in tris {
        out_mesh
            .indices
            .extend_from_slice(&[base + a, base + b, base + c]);
    }
    let mut outer_idx = vec![u32::MAX; pts.len()];
    for (idx, &is_b) in is_boundary.iter().enumerate() {
        if is_b {
            let d_out = skirt_span - d_in[idx];
            let op = [
                pts[idx][0] + d_out * miters[idx][0],
                pts[idx][1] + d_out * miters[idx][1],
            ];
            outer_idx[idx] = out_mesh.vertices.len() as u32;
            out_mesh.vertices.push(GpuVertex {
                position: to_ndc(op[0], op[1], phys_w, phys_h),
                uv: SOLID_WHITE_UV,
                color: outer_rgba,
            });
        }
    }
    for &(u, v) in boundary_edges {
        let (cu, cv) = (base + u, base + v);
        let (ou, ov) = (outer_idx[u as usize], outer_idx[v as usize]);
        if ou != u32::MAX && ov != u32::MAX {
            out_mesh
                .indices
                .extend_from_slice(&[cu, cv, ov, cu, ov, ou]);
        }
    }
}
