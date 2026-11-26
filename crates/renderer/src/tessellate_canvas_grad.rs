//! Gradient mesh tessellation for `CanvasCommand::FillGradient` on the `wgpu` backend.

use crate::tessellate::GpuVertex;
use crate::tessellate_aa::AA_TOLERANCE_PHYS_PX;
use crate::tessellate_primitives::{apply_opacity, build_custom_lyon_path, to_ndc};
use crate::ATLAS_SIZE;
use krui_core::{CanvasFillRule, CanvasGradient, PathSegment};
use krui_style::Affine2;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
};

const SOLID_WHITE_UV: [f32; 2] = [1.5 / ATLAS_SIZE as f32, 1.5 / ATLAS_SIZE as f32];
const MAX_SUBDIV_EDGE_PX: f32 = 12.0;
const MAX_GRADIENT_SUBDIV_VERTS: usize = 8192;

#[allow(clippy::too_many_arguments)]
pub(crate) fn tessellate_canvas_gradient_fill(
    segments: &[PathSegment],
    fill_rule: CanvasFillRule,
    gradient: &CanvasGradient,
    alpha: f32,
    phys_tx: Affine2,
    opacity: f32,
    (phys_w, phys_h): (f32, f32),
    fill_tess: &mut FillTessellator,
    mesh: &mut VertexBuffers<GpuVertex, u32>,
) {
    let eff_alpha = (opacity * alpha).clamp(0.0, 1.0);
    if eff_alpha <= 0.0 || gradient.stops().is_empty() || segments.is_empty() {
        return;
    }
    let scale = phys_tx
        .a
        .hypot(phys_tx.b)
        .max(phys_tx.c.hypot(phys_tx.d))
        .max(1.0);
    let tol = (AA_TOLERANCE_PHYS_PX / scale).clamp(1e-4, 0.25);
    let local_path = build_custom_lyon_path(segments, Affine2::IDENTITY);
    let lyon_rule = match fill_rule {
        CanvasFillRule::NonZero => FillRule::NonZero,
        CanvasFillRule::EvenOdd => FillRule::EvenOdd,
    };
    let mut raw: VertexBuffers<[f32; 2], u32> = VertexBuffers::with_capacity(128, 384);
    let opts = FillOptions::default()
        .with_fill_rule(lyon_rule)
        .with_tolerance(tol);
    let _ = fill_tess.tessellate_path(
        &local_path,
        &opts,
        &mut BuffersBuilder::new(&mut raw, |v: FillVertex| v.position().to_array()),
    );
    if needs_gradient_subdivision(gradient) {
        subdivide_gradient_mesh(
            &mut raw.vertices,
            &mut raw.indices,
            MAX_SUBDIV_EDGE_PX / scale,
        );
    }
    let base = mesh.vertices.len() as u32;
    for &[lx, ly] in &raw.vertices {
        let color = apply_opacity(gradient.sample(lx, ly), eff_alpha);
        let (px, py) = phys_tx.transform_point(lx, ly);
        mesh.vertices.push(GpuVertex {
            position: to_ndc(px, py, phys_w, phys_h),
            uv: SOLID_WHITE_UV,
            color,
        });
    }
    mesh.indices
        .extend(raw.indices.iter().map(|&idx| base + idx));
}

fn needs_gradient_subdivision(gradient: &CanvasGradient) -> bool {
    match gradient {
        CanvasGradient::Radial { .. } => true,
        CanvasGradient::Linear { stops, .. } => {
            stops.len() > 2
                || stops.first().is_some_and(|s| s.offset > 0.01)
                || stops.last().is_some_and(|s| s.offset < 0.99)
        }
    }
}

fn subdivide_gradient_mesh(verts: &mut Vec<[f32; 2]>, indices: &mut Vec<u32>, max_edge: f32) {
    let thresh2 = max_edge.max(1.0) * max_edge.max(1.0);
    for _pass in 0..3 {
        if verts.len() >= MAX_GRADIENT_SUBDIV_VERTS {
            break;
        }
        let mut next_idx = Vec::with_capacity(indices.len() * 2);
        let mut did_split = false;
        for &[i0, i1, i2] in indices.as_chunks::<3>().0 {
            if verts.len() + 3 > MAX_GRADIENT_SUBDIV_VERTS {
                next_idx.extend_from_slice(&[i0, i1, i2]);
                continue;
            }
            let (p0, p1, p2) = (verts[i0 as usize], verts[i1 as usize], verts[i2 as usize]);
            let d01 = dist2(p0, p1);
            let d12 = dist2(p1, p2);
            let d20 = dist2(p2, p0);
            if d01.max(d12).max(d20) <= thresh2 {
                next_idx.extend_from_slice(&[i0, i1, i2]);
                continue;
            }
            did_split = true;
            let m01 = push_mid(verts, p0, p1);
            let m12 = push_mid(verts, p1, p2);
            let m20 = push_mid(verts, p2, p0);
            next_idx.extend_from_slice(&[i0, m01, m20, m01, i1, m12, m20, m12, i2, m01, m12, m20]);
        }
        *indices = next_idx;
        if !did_split {
            break;
        }
    }
}

#[inline]
fn dist2(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    dx * dx + dy * dy
}

#[inline]
fn push_mid(verts: &mut Vec<[f32; 2]>, a: [f32; 2], b: [f32; 2]) -> u32 {
    let idx = verts.len() as u32;
    verts.push([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]);
    idx
}
