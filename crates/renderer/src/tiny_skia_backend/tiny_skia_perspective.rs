//! Bilinear inverse-homography warping for CSS `perspective` 3D offscreen layers in `TinySkiaRenderer`.

use krui_layout::LayoutRect;
use krui_style::{compute_projective_quad, inverse_project_point, Mat4};

#[inline]
fn sample_bilinear_pm(data: &[u8], sw: i32, sh: i32, fx: f32, fy: f32) -> [f32; 4] {
    let x0 = fx.floor() as i32;
    let y0 = fy.floor() as i32;
    let tx = (fx - x0 as f32).clamp(0.0, 1.0);
    let ty = (fy - y0 as f32).clamp(0.0, 1.0);
    let get = |px: i32, py: i32| -> [f32; 4] {
        if px < 0 || py < 0 || px >= sw || py >= sh {
            return [0.0; 4];
        }
        let off = ((py as usize) * (sw as usize) + (px as usize)) * 4;
        [
            data[off] as f32 / 255.0,
            data[off + 1] as f32 / 255.0,
            data[off + 2] as f32 / 255.0,
            data[off + 3] as f32 / 255.0,
        ]
    };
    let c00 = get(x0, y0);
    let c10 = get(x0 + 1, y0);
    let c01 = get(x0, y0 + 1);
    let c11 = get(x0 + 1, y0 + 1);
    let mut out = [0.0_f32; 4];
    for k in 0..4 {
        let top = c00[k] + (c10[k] - c00[k]) * tx;
        let bot = c01[k] + (c11[k] - c01[k]) * tx;
        out[k] = top + (bot - top) * ty;
    }
    out
}

/// Warps `src` from logical `src_rect` through 4x4 projective `mat4` and composites onto `dst`.
pub(super) fn composite_perspective_layer_skia(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    mask: Option<&tiny_skia::Mask>,
    (src_rect, mat4): (LayoutRect, Mat4),
    opacity: f32,
    scale: f32,
) {
    let s = scale.clamp(1.0, 4.0);
    let inv_s = 1.0 / s;
    let phys_mat = Mat4::scale(s, s, s) * mat4 * Mat4::scale(inv_s, inv_s, inv_s);
    let (rx, ry, rw, rh) = (
        src_rect.x * s,
        src_rect.y * s,
        src_rect.width * s,
        src_rect.height * s,
    );
    let Some(quad) = compute_projective_quad(phys_mat, rx, ry, rw, rh) else {
        return;
    };
    let (dw, dh) = (dst.width() as i32, dst.height() as i32);
    let (dw_f, dh_f) = (dw as f32, dh as f32);
    let (sw, sh) = (src.width() as i32, src.height() as i32);
    let x0 = (quad.screen_aabb[0].floor() - 1.0).clamp(0.0, dw_f) as i32;
    let y0 = (quad.screen_aabb[1].floor() - 1.0).clamp(0.0, dh_f) as i32;
    let x1 = (quad.screen_aabb[2].ceil() + 1.0).clamp(0.0, dw_f) as i32;
    let y1 = (quad.screen_aabb[3].ceil() + 1.0).clamp(0.0, dh_f) as i32;
    let src_data = src.data();
    let mask_data = mask.map(tiny_skia::Mask::data);
    let dst_data = dst.data_mut();
    let op = opacity.clamp(0.0, 1.0);
    for py in y0..y1 {
        for px in x0..x1 {
            let Some((ux, uy)) =
                inverse_project_point(&quad.inv_homography, px as f32 + 0.5, py as f32 + 0.5)
            else {
                continue;
            };
            let cx = (ux - rx).min(rx + rw - ux) + 0.5;
            let cy = (uy - ry).min(ry + rh - uy) + 0.5;
            let edge_cov = cx.clamp(0.0, 1.0) * cy.clamp(0.0, 1.0);
            if edge_cov <= 0.0 {
                continue;
            }
            let pidx = (py as usize) * (dw as usize) + (px as usize);
            let m_cov = mask_data.map_or(1.0, |m| m[pidx] as f32 / 255.0);
            let factor = edge_cov * op * m_cov;
            if factor <= 0.0 {
                continue;
            }
            let spm = sample_bilinear_pm(src_data, sw, sh, ux - 0.5, uy - 0.5);
            let sr = [
                spm[0] * factor,
                spm[1] * factor,
                spm[2] * factor,
                spm[3] * factor,
            ];
            let off = pidx * 4;
            let inv_sa = 1.0 - sr[3].clamp(0.0, 1.0);
            for k in 0..4 {
                let d_ch = dst_data[off + k] as f32 / 255.0;
                dst_data[off + k] = ((sr[k] + d_ch * inv_sa).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
}
