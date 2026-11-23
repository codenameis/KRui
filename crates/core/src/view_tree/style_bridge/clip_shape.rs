//! Four-corner CSS padding-box clip geometry (`overflow: hidden | scroll`) and clip intersection.

use crate::view_tree::hit_test::transform_rect_aabb;
use crate::view_tree::paint::intersect_rects;
use crate::view_tree::paint_canvas::pad_aabb_for_paint_shadows;
use krui_layout::LayoutRect;
use krui_style::{Affine2, LayoutStyle, PaintStyle};

/// Active ancestor clip bounds with independent `[top_left, top_right, bottom_right, bottom_left]`
/// corner radii in logical CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClipState {
    pub(crate) rect: Option<LayoutRect>,
    pub(crate) radii: [f32; 4],
}

impl ClipState {
    pub(crate) const NONE: Self = Self {
        rect: None,
        radii: [0.0; 4],
    };

    #[inline]
    pub(crate) fn from_opt(rect: Option<LayoutRect>, radius: f32, radii: [f32; 4]) -> Self {
        if rect.is_none() {
            Self::NONE
        } else {
            let eff = if radii == [0.0; 4] && radius > 0.0 {
                [radius.max(0.0); 4]
            } else {
                radii.map(|v| if v.is_nan() { 0.0 } else { v.max(0.0) })
            };
            Self { rect, radii: eff }
        }
    }

    #[inline]
    pub(crate) fn max_radius(self) -> f32 {
        self.radii.into_iter().fold(0.0_f32, f32::max)
    }
}

/// Applies W3C CSS Backgrounds & Borders §5.5 corner-overlap scaling with safe finite clamping.
#[inline]
pub(crate) fn normalize_radii_4_core(w: f32, h: f32, radii: [f32; 4]) -> [f32; 4] {
    if w <= 0.0 || h <= 0.0 {
        return [0.0; 4];
    }
    let r = radii.map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0e6) });
    let f = [
        w / (r[0] + r[1]).max(1e-4),
        w / (r[3] + r[2]).max(1e-4),
        h / (r[0] + r[3]).max(1e-4),
        h / (r[1] + r[2]).max(1e-4),
    ]
    .into_iter()
    .fold(1.0_f32, f32::min);
    r.map(|v| v * f)
}

/// Computes a node's padding-box clip rect and 4 inner corner radii inset by adjacent border averages.
pub(crate) fn compute_padding_box_clip(
    world_rect: LayoutRect,
    paint: &PaintStyle,
    layout: &LayoutStyle,
) -> (LayoutRect, [f32; 4]) {
    let edges = layout.effective_border_edges();
    let x = world_rect.x + edges.left.max(0.0);
    let y = world_rect.y + edges.top.max(0.0);
    let w = (world_rect.width - edges.left.max(0.0) - edges.right.max(0.0)).max(0.0);
    let h = (world_rect.height - edges.top.max(0.0) - edges.bottom.max(0.0)).max(0.0);
    let rect = LayoutRect {
        x,
        y,
        width: w,
        height: h,
    };
    let outer = normalize_radii_4_core(
        world_rect.width,
        world_rect.height,
        paint.effective_corner_radii().to_array(),
    );
    let inner = [
        (outer[0] - 0.5 * (edges.top.max(0.0) + edges.left.max(0.0))).max(0.0),
        (outer[1] - 0.5 * (edges.top.max(0.0) + edges.right.max(0.0))).max(0.0),
        (outer[2] - 0.5 * (edges.bottom.max(0.0) + edges.right.max(0.0))).max(0.0),
        (outer[3] - 0.5 * (edges.bottom.max(0.0) + edges.left.max(0.0))).max(0.0),
    ];
    (rect, normalize_radii_4_core(w, h, inner))
}

/// Intersects `outer` clip state with an inner clipping box and preserves tighter corner radii.
pub(crate) fn intersect_clip_states(
    outer: ClipState,
    inner_rect: LayoutRect,
    inner_radii: [f32; 4],
) -> ClipState {
    let Some(outer_rect) = outer.rect else {
        return ClipState {
            rect: Some(inner_rect),
            radii: inner_radii,
        };
    };
    let intersected = intersect_rects(outer_rect, inner_rect);
    let inner_has_radius = inner_radii.iter().any(|&v| v > 0.0);
    let outer_has_radius = outer.radii.iter().any(|&v| v > 0.0);
    let radii = if !inner_has_radius {
        if outer_rect == intersected {
            outer.radii
        } else {
            [0.0; 4]
        }
    } else if !outer_has_radius || inner_rect == intersected {
        inner_radii
    } else {
        let eps = 0.5;
        [
            if (intersected.x - inner_rect.x).abs() < eps
                && (intersected.y - inner_rect.y).abs() < eps
            {
                inner_radii[0]
            } else {
                outer.radii[0]
            },
            if ((intersected.x + intersected.width) - (inner_rect.x + inner_rect.width)).abs() < eps
                && (intersected.y - inner_rect.y).abs() < eps
            {
                inner_radii[1]
            } else {
                outer.radii[1]
            },
            if ((intersected.x + intersected.width) - (inner_rect.x + inner_rect.width)).abs() < eps
                && ((intersected.y + intersected.height) - (inner_rect.y + inner_rect.height)).abs()
                    < eps
            {
                inner_radii[2]
            } else {
                outer.radii[2]
            },
            if (intersected.x - inner_rect.x).abs() < eps
                && ((intersected.y + intersected.height) - (inner_rect.y + inner_rect.height)).abs()
                    < eps
            {
                inner_radii[3]
            } else {
                outer.radii[3]
            },
        ]
    };
    ClipState {
        rect: Some(intersected),
        radii: normalize_radii_4_core(intersected.width, intersected.height, radii),
    }
}

/// Computes clipped bounding rect and whether `world_rect`'s self box/shadow overlaps `clip_bounds`.
pub(crate) fn resolve_node_visibility(
    clips_self: bool,
    children_empty: bool,
    world_rect: LayoutRect,
    clip_bounds: Option<LayoutRect>,
    effective_transform: Affine2,
    paint: &PaintStyle,
) -> Option<(LayoutRect, bool)> {
    let aabb = transform_rect_aabb(effective_transform, world_rect);
    let self_aabb = pad_aabb_for_paint_shadows(aabb, paint);
    let (draw_rect, self_visible) = match clip_bounds {
        None => (aabb, true),
        Some(clip) => {
            let clipped_box = intersect_rects(aabb, clip);
            let clipped_self = intersect_rects(self_aabb, clip);
            (
                clipped_box,
                clipped_self.width > 0.5 && clipped_self.height > 0.5,
            )
        }
    };
    let box_in_clip = draw_rect.width > 0.5 && draw_rect.height > 0.5;
    if (clips_self && !box_in_clip && !self_visible)
        || (!clips_self && !self_visible && children_empty)
    {
        return None;
    }
    Some((draw_rect, self_visible))
}

/// `(padding_box_rect, corner_radii, transform)` promoted to `PushLayer.local_clip`.
pub(crate) type LocalClipLayer = (LayoutRect, [f32; 4], Affine2);

/// `(child_clip, optional_promoted_local_clip, optional_layer_bounds)` returned by [`compute_child_clip_layer`].
pub(crate) type ChildClipLayer = (ClipState, Option<LocalClipLayer>, Option<LayoutRect>);

/// Computes child clip state and promotes nested rounded or rotated rounded clips to `PushLayer.local_clip`.
pub(crate) fn compute_child_clip_layer(
    clips_self: bool,
    world_rect: LayoutRect,
    eff_paint: &PaintStyle,
    layout: &LayoutStyle,
    eff_transform: Affine2,
    outer_clip: ClipState,
) -> ChildClipLayer {
    if !clips_self {
        return (outer_clip, None, None);
    }
    let (pad_rect, pad_radii) = compute_padding_box_clip(world_rect, eff_paint, layout);
    let pad_aabb = transform_rect_aabb(eff_transform, pad_rect);
    let has_rot_or_skew = eff_transform.b.abs() > 1e-4 || eff_transform.c.abs() > 1e-4;
    let inner_rounded = pad_radii.iter().any(|&r| r > 0.5);
    let outer_rounded = outer_clip.rect.is_some() && outer_clip.radii.iter().any(|&r| r > 0.5);
    if (inner_rounded && has_rot_or_skew) || (inner_rounded && outer_rounded) {
        let bounds = outer_clip
            .rect
            .map_or(pad_aabb, |oc| intersect_rects(pad_aabb, oc));
        let inner_clip = ClipState {
            rect: Some(bounds),
            radii: [0.0; 4],
        };
        let bounds_opt = (bounds.width > 0.5 && bounds.height > 0.5).then_some(bounds);
        return (
            inner_clip,
            Some((pad_rect, pad_radii, eff_transform)),
            bounds_opt,
        );
    }
    (
        intersect_clip_states(outer_clip, pad_aabb, pad_radii),
        None,
        None,
    )
}
