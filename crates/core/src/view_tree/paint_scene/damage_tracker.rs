//! Optional per-frame damage rectangle tracking (`DirtyFlagOnly` vs `BoundingRects`).

use super::damage_hash::hash_primitive;
use crate::scene::{DrawPrimitive, PaintScene};
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color, DamageRect, DamageTrackingMode};

const PRIMITIVE_AA_PAD_PX: f32 = 2.0;

#[derive(Clone, Debug, PartialEq)]
struct TrackedPrimSnap {
    bounds: DamageRect,
    signature: u64,
}

/// Tracks changed viewport damage rectangles between consecutive [`PaintScene`] frames when enabled.
#[derive(Clone, Debug, Default)]
pub struct DamageTracker {
    mode: DamageTrackingMode,
    prev_snaps: Vec<TrackedPrimSnap>,
    prev_vp: (u32, u32),
    prev_scale_bits: u32,
    prev_clear: Color,
    force_full: bool,
}

impl DamageTracker {
    /// Returns the active [`DamageTrackingMode`].
    #[must_use]
    pub const fn mode(&self) -> DamageTrackingMode {
        self.mode
    }

    /// Switches tracking mode, marking the next frame full-damage if bounding tracking is enabled.
    pub fn set_mode(&mut self, mode: DamageTrackingMode) {
        if self.mode != mode {
            self.mode = mode;
            self.prev_snaps.clear();
            self.force_full = true;
        }
    }

    /// Marks the next frame as requiring full framebuffer refresh.
    pub fn invalidate_full(&mut self) {
        self.force_full = true;
    }

    /// Computes minimal clamped damage rects for `scene`, or empty `Vec` under `DirtyFlagOnly`.
    pub fn compute_damage(&mut self, scene: &PaintScene) -> Vec<DamageRect> {
        let DamageTrackingMode::BoundingRects {
            max_rects_per_commit,
            ..
        } = self.mode
        else {
            self.prev_vp = (scene.width, scene.height);
            self.prev_scale_bits = scene.scale_factor.to_bits();
            self.prev_clear = scene.clear_color;
            self.force_full = false;
            return Vec::new();
        };

        let scale = scene.scale_factor.clamp(1.0, 4.0);
        let phys_w = ((scene.width as f32 * scale).round() as u32).max(1);
        let phys_h = ((scene.height as f32 * scale).round() as u32).max(1);
        let cur_snaps = collect_primitive_snaps(scene, scale, phys_w, phys_h);

        let full_invalidate = self.force_full
            || self.prev_vp != (scene.width, scene.height)
            || self.prev_scale_bits != scene.scale_factor.to_bits()
            || self.prev_clear != scene.clear_color;

        let out = if full_invalidate {
            vec![DamageRect::new(0, 0, phys_w, phys_h)]
        } else {
            diff_snapshots(&self.prev_snaps, &cur_snaps, max_rects_per_commit)
        };

        self.prev_snaps = cur_snaps;
        self.prev_vp = (scene.width, scene.height);
        self.prev_scale_bits = scene.scale_factor.to_bits();
        self.prev_clear = scene.clear_color;
        self.force_full = false;
        out
    }
}

fn collect_primitive_snaps(
    scene: &PaintScene,
    scale: f32,
    phys_w: u32,
    phys_h: u32,
) -> Vec<TrackedPrimSnap> {
    let mut out = Vec::with_capacity(scene.primitives.len());
    for prim in &scene.primitives {
        if let Some((rect, tx, extra_pad)) = primitive_world_rect(prim) {
            let bounds = transform_and_clamp(rect, tx, extra_pad, scale, phys_w, phys_h);
            out.push(TrackedPrimSnap {
                bounds,
                signature: hash_primitive(prim),
            });
        }
    }
    out
}

fn primitive_world_rect(prim: &DrawPrimitive) -> Option<(LayoutRect, Affine2, f32)> {
    match prim {
        DrawPrimitive::StyledBox {
            rect,
            transform,
            box_shadow,
            ..
        } => {
            let pad = box_shadow
                .map(|s| {
                    s.blur_radius.max(0.0) * 2.5
                        + s.spread_radius.abs()
                        + s.offset_x.abs().max(s.offset_y.abs())
                })
                .unwrap_or(0.0);
            Some((*rect, *transform, PRIMITIVE_AA_PAD_PX + pad))
        }
        DrawPrimitive::Image {
            rect, transform, ..
        } => Some((*rect, *transform, PRIMITIVE_AA_PAD_PX)),
        DrawPrimitive::TextGlyphs {
            origin_x,
            origin_y,
            transform,
            glyphs,
            ..
        } => glyph_run_world_rect(*origin_x, *origin_y, *transform, glyphs),
        DrawPrimitive::CanvasBatch {
            viewport,
            transform,
            ..
        } => Some((*viewport, *transform, PRIMITIVE_AA_PAD_PX)),
        DrawPrimitive::PushLayer {
            bounds,
            filter,
            perspective,
            ..
        } => {
            if perspective.is_some() {
                Some((
                    LayoutRect {
                        x: 0.0,
                        y: 0.0,
                        width: 16_384.0,
                        height: 16_384.0,
                    },
                    Affine2::IDENTITY,
                    0.0,
                ))
            } else {
                Some((
                    *bounds,
                    Affine2::IDENTITY,
                    PRIMITIVE_AA_PAD_PX + filter.blur_px.max(0.0) * 2.5,
                ))
            }
        }
        DrawPrimitive::PopLayer => None,
    }
}

fn glyph_run_world_rect(
    origin_x: f32,
    origin_y: f32,
    transform: Affine2,
    glyphs: &[krui_text::RasterizedGlyph],
) -> Option<(LayoutRect, Affine2, f32)> {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for g in glyphs {
        let gx = origin_x + g.x;
        let gy = origin_y + g.y;
        min_x = min_x.min(gx);
        min_y = min_y.min(gy);
        max_x = max_x.max(gx + g.width as f32);
        max_y = max_y.max(gy + g.height as f32);
    }
    min_x.is_finite().then_some((
        LayoutRect {
            x: min_x,
            y: min_y,
            width: (max_x - min_x).max(0.0),
            height: (max_y - min_y).max(0.0),
        },
        transform,
        PRIMITIVE_AA_PAD_PX,
    ))
}

fn transform_and_clamp(
    r: LayoutRect,
    tx: Affine2,
    pad_logical: f32,
    scale: f32,
    max_w: u32,
    max_h: u32,
) -> DamageRect {
    let corners = [
        tx.transform_point(r.x - pad_logical, r.y - pad_logical),
        tx.transform_point(r.x + r.width + pad_logical, r.y - pad_logical),
        tx.transform_point(r.x + r.width + pad_logical, r.y + r.height + pad_logical),
        tx.transform_point(r.x - pad_logical, r.y + r.height + pad_logical),
    ];
    let min_x = corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min) * scale;
    let min_y = corners.iter().map(|c| c.1).fold(f32::INFINITY, f32::min) * scale;
    let max_x = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::NEG_INFINITY, f32::max)
        * scale;
    let max_y = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::NEG_INFINITY, f32::max)
        * scale;
    let x0 = (min_x.floor().max(0.0) as u32).min(max_w);
    let y0 = (min_y.floor().max(0.0) as u32).min(max_h);
    let x1 = (max_x.ceil().max(0.0) as u32).min(max_w);
    let y1 = (max_y.ceil().max(0.0) as u32).min(max_h);
    DamageRect::new(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

fn diff_snapshots(
    prev: &[TrackedPrimSnap],
    cur: &[TrackedPrimSnap],
    max_rects: u8,
) -> Vec<DamageRect> {
    let mut dirty = Vec::new();
    let shared = prev.len().min(cur.len());
    for i in 0..shared {
        if prev[i] != cur[i] {
            let u = prev[i].bounds.union(cur[i].bounds);
            if !u.is_empty() {
                dirty.push(u);
            }
        }
    }
    for s in prev.iter().skip(shared) {
        if !s.bounds.is_empty() {
            dirty.push(s.bounds);
        }
    }
    for s in cur.iter().skip(shared) {
        if !s.bounds.is_empty() {
            dirty.push(s.bounds);
        }
    }
    merge_damage_rects(&mut dirty, max_rects.max(1) as usize);
    dirty
}

fn merge_damage_rects(rects: &mut Vec<DamageRect>, cap: usize) {
    let mut changed = true;
    while changed && rects.len() > 1 {
        changed = false;
        'outer: for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                if rects[i].intersects(rects[j]) {
                    let merged = rects[i].union(rects[j]);
                    rects[i] = merged;
                    rects.remove(j);
                    changed = true;
                    break 'outer;
                }
            }
        }
    }
    if rects.len() > cap {
        let mut total = DamageRect::default();
        for r in rects.iter() {
            total = total.union(*r);
        }
        rects.clear();
        if !total.is_empty() {
            rects.push(total);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_box(x: f32, y: f32, w: f32, h: f32, col: Color) -> DrawPrimitive {
        DrawPrimitive::StyledBox {
            rect: LayoutRect {
                x,
                y,
                width: w,
                height: h,
            },
            clip: None,
            clip_radius: 0.0,
            clip_radii: [0.0; 4],
            transform: Affine2::IDENTITY,
            opacity: 1.0,
            background_color: col,
            background_gradient: None,
            radial_gradient: None,
            conic_gradient: None,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            border_radius: 0.0,
            corner_radii: [0.0; 4],
            box_shadow: None,
            backdrop_blur_px: 0.0,
        }
    }

    #[test]
    fn damage_tracker_respects_mode_and_isolates_changed_widget() {
        let mut dt = DamageTracker::default();
        let mut s = PaintScene::new(480, 320, Color::BLACK);
        s.primitives.push(mk_box(
            10.0,
            10.0,
            40.0,
            20.0,
            Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
        ));
        assert!(dt.compute_damage(&s).is_empty());

        dt.set_mode(DamageTrackingMode::BoundingRects {
            split_area_ratio_x10: 15,
            max_rects_per_commit: 8,
        });
        assert_eq!(dt.compute_damage(&s), vec![DamageRect::new(0, 0, 480, 320)]);
        assert!(dt.compute_damage(&s).is_empty());

        s.primitives[0] = mk_box(
            10.0,
            10.0,
            40.0,
            20.0,
            Color {
                r: 0,
                g: 255,
                b: 0,
                a: 255,
            },
        );
        let partial = dt.compute_damage(&s);
        assert_eq!(partial, vec![DamageRect::new(8, 8, 44, 24)]);
    }

    #[test]
    fn damage_tracker_detects_push_layer_group_opacity_and_filter_changes() {
        let mut dt = DamageTracker::default();
        dt.set_mode(DamageTrackingMode::BoundingRects {
            split_area_ratio_x10: 15,
            max_rects_per_commit: 8,
        });
        let make_layered_scene = |opacity: f32, brightness: f32| {
            let mut s = PaintScene::new(480, 320, Color::BLACK);
            s.primitives.push(DrawPrimitive::PushLayer {
                bounds: LayoutRect {
                    x: 20.0,
                    y: 30.0,
                    width: 60.0,
                    height: 40.0,
                },
                clip: None,
                clip_radius: 0.0,
                clip_radii: [0.0; 4],
                local_clip: None,
                clip_path: None,
                perspective: None,
                opacity,
                filter: krui_style::CssFilterEffect {
                    brightness,
                    ..krui_style::CssFilterEffect::IDENTITY
                },
            });
            s.primitives
                .push(mk_box(20.0, 30.0, 60.0, 40.0, Color::WHITE));
            s.primitives.push(DrawPrimitive::PopLayer);
            s
        };
        let s1 = make_layered_scene(0.8, 1.0);
        assert_eq!(
            dt.compute_damage(&s1),
            vec![DamageRect::new(0, 0, 480, 320)]
        );
        assert!(dt.compute_damage(&s1).is_empty());

        let s2 = make_layered_scene(0.4, 1.0);
        assert_eq!(
            dt.compute_damage(&s2),
            vec![DamageRect::new(18, 28, 64, 44)]
        );

        let s3 = make_layered_scene(0.4, 1.3);
        assert_eq!(
            dt.compute_damage(&s3),
            vec![DamageRect::new(18, 28, 64, 44)]
        );
    }
}
