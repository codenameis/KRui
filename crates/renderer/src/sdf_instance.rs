//! GPU-instanced SDF quad data layout (`SdfQuadInstance`) and builders for `StyledBox`,
//! outer/inset `BoxShadow`, monochrome/color `TextGlyphs`, backdrop blur, and `PopLayer`.

use bytemuck::{Pod, Zeroable};
use krui_core::{ResolvedConicGradient, ResolvedRadialGradient};
use krui_layout::LayoutRect;
use krui_style::{Affine2, BoxShadow, Color, GradientStopsRef, LinearGradient};

pub(crate) const KIND_STYLED_BOX: u32 = 0;
pub(crate) const KIND_OUTER_SHADOW: u32 = 1;
pub(crate) const KIND_INSET_SHADOW: u32 = 2;
pub(crate) const KIND_MASK_GLYPH: u32 = 3;
pub(crate) const KIND_COLOR_GLYPH: u32 = 4;
pub(crate) const KIND_SAMPLE_AUX_TEX: u32 = 5;
pub(crate) const KIND_IMAGE_TEX: u32 = 6;

/// Per-instance GPU payload (`240` bytes) consumed by `vs_main` / `fs_main` in `shader.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SdfQuadInstance {
    /// Physical screen-space bounds `[min_x, min_y, max_x, max_y]`.
    pub screen_bounds: [f32; 4],
    /// Transformed physical box center and inverse viewport `[cx, cy, 1.0 / phys_w, 1.0 / phys_h]`.
    pub center_and_vp: [f32; 4],
    /// Inverse 2x2 linear transform matrix `[a, b, c, d]`.
    pub inv_linear: [f32; 4],
    /// Local half-extents, uniform corner radius, and border width `[hw, hh, r, bw]`.
    pub box_geom: [f32; 4],
    /// Clip rectangle `[cx, cy, hw, hh]` in physical pixels; negative extents disable clipping.
    pub clip_geom: [f32; 4],
    /// Primary RGBA color (fill, shadow, glyph foreground, or quad tint).
    pub color_a: [f32; 4],
    /// Secondary RGBA color (border color for boxes).
    pub color_b: [f32; 4],
    /// Gradient start RGBA color.
    pub grad_start: [f32; 4],
    /// Gradient end RGBA color.
    pub grad_end: [f32; 4],
    /// Texture UV bounds `[u0, v0, u1, v1]` or gradient positions/center+radii.
    pub uv_rect: [f32; 4],
    /// Shadow parameters `[ox, oy, blur, spread]` or middle linear gradient stop RGBA color.
    pub shadow_params: [f32; 4],
    /// Extra per-instance scalars `[opacity, clip_radius, grad_angle_or_inner_stop, mode_or_outer_stop]`.
    pub extra_params: [f32; 4],
    /// Packed instance metadata `[kind, layer_or_premultiplied, gradient_mode, reserved]`.
    pub flags: [u32; 4],
    /// Normalized corner radii `[top_left, top_right, bottom_right, bottom_left]` in physical pixels.
    pub corner_radii: [f32; 4],
    /// Normalized 4-corner ancestor clip radii `[tl, tr, br, bl]` in physical pixels.
    pub clip_radii: [f32; 4],
}

const fn sdf_attr(shader_location: u32, format: wgpu::VertexFormat) -> wgpu::VertexAttribute {
    wgpu::VertexAttribute {
        offset: (shader_location as u64) * 16,
        shader_location,
        format,
    }
}

pub(crate) const SDF_INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 15] = [
    sdf_attr(0, wgpu::VertexFormat::Float32x4),
    sdf_attr(1, wgpu::VertexFormat::Float32x4),
    sdf_attr(2, wgpu::VertexFormat::Float32x4),
    sdf_attr(3, wgpu::VertexFormat::Float32x4),
    sdf_attr(4, wgpu::VertexFormat::Float32x4),
    sdf_attr(5, wgpu::VertexFormat::Float32x4),
    sdf_attr(6, wgpu::VertexFormat::Float32x4),
    sdf_attr(7, wgpu::VertexFormat::Float32x4),
    sdf_attr(8, wgpu::VertexFormat::Float32x4),
    sdf_attr(9, wgpu::VertexFormat::Float32x4),
    sdf_attr(10, wgpu::VertexFormat::Float32x4),
    sdf_attr(11, wgpu::VertexFormat::Float32x4),
    sdf_attr(12, wgpu::VertexFormat::Uint32x4),
    sdf_attr(13, wgpu::VertexFormat::Float32x4),
    sdf_attr(14, wgpu::VertexFormat::Float32x4),
];

/// Shared surface coordinate parameters for a single frame/pass.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SurfaceFrameTransform {
    pub surface_scale: f32,
    pub glyph_raster_scale: f32,
    pub phys_w: f32,
    pub phys_h: f32,
}

/// Geometry and paint inputs for constructing `StyledBox` and `BoxShadow` SDF instances.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BoxInstanceSpec {
    pub rect: LayoutRect,
    pub clip: Option<LayoutRect>,
    pub clip_radius: f32,
    pub clip_radii: [f32; 4],
    pub transform: Affine2,
    pub opacity: f32,
    pub background_color: Color,
    pub background_gradient: Option<LinearGradient>,
    pub radial_gradient: Option<ResolvedRadialGradient>,
    pub conic_gradient: Option<ResolvedConicGradient>,
    pub border_color: Color,
    pub border_width: f32,
    pub border_radius: f32,
    pub corner_radii: [f32; 4],
}

/// Resolves and normalizes 4-corner radii `[tl, tr, br, bl]` via shared [`crate::radii`].
pub(crate) use crate::radii::{effective_radii_4, normalize_radii_4};

pub(crate) fn encode_clip_geom(
    clip: Option<LayoutRect>,
    clip_radius: f32,
    clip_radii: [f32; 4],
    scale: f32,
) -> Option<([f32; 4], [f32; 4], f32)> {
    let Some(c) = clip else {
        return Some(([0.0, 0.0, -1.0, -1.0], [0.0; 4], 0.0));
    };
    if c.width <= 0.0 || c.height <= 0.0 {
        return None;
    }
    let hw = 0.5 * c.width * scale;
    let hh = 0.5 * c.height * scale;
    let cx = c.x * scale + hw;
    let cy = c.y * scale + hh;
    let r4 = normalize_radii_4(
        2.0 * hw,
        2.0 * hh,
        effective_radii_4(clip_radius, clip_radii).map(|v| v * scale),
    );
    let max_r = r4.into_iter().fold(0.0_f32, f32::max);
    Some(([cx, cy, hw, hh], r4, max_r))
}

pub(crate) fn project_screen_bounds(
    center_screen: (f32, f32),
    transform: Affine2,
    local_offset: (f32, f32),
    local_half_ext: (f32, f32),
    phys_w: f32,
    phys_h: f32,
) -> Option<[f32; 4]> {
    let dx = transform.a * local_offset.0 + transform.c * local_offset.1;
    let dy = transform.b * local_offset.0 + transform.d * local_offset.1;
    let rx = transform.a.abs() * local_half_ext.0 + transform.c.abs() * local_half_ext.1;
    let ry = transform.b.abs() * local_half_ext.0 + transform.d.abs() * local_half_ext.1;
    let min_x = (center_screen.0 + dx - rx - 1.5).floor().clamp(0.0, phys_w);
    let min_y = (center_screen.1 + dy - ry - 1.5).floor().clamp(0.0, phys_h);
    let max_x = (center_screen.0 + dx + rx + 1.5).ceil().clamp(0.0, phys_w);
    let max_y = (center_screen.1 + dy + ry + 1.5).ceil().clamp(0.0, phys_h);
    (max_x > min_x && max_y > min_y).then_some([min_x, min_y, max_x, max_y])
}

pub(crate) struct ProjectedBoxCommon {
    pub bounds: [f32; 4],
    pub center_and_vp: [f32; 4],
    pub inv_linear: [f32; 4],
    pub box_geom: [f32; 4],
    pub clip_geom: [f32; 4],
    pub clip_radii: [f32; 4],
    pub clip_r_phys: f32,
    pub corner_radii: [f32; 4],
}

pub(crate) fn prepare_box_common(
    spec: &BoxInstanceSpec,
    sf: SurfaceFrameTransform,
    offset: (f32, f32),
    pad: (f32, f32),
) -> Option<ProjectedBoxCommon> {
    if spec.rect.width <= 0.0 || spec.rect.height <= 0.0 || spec.opacity <= 0.0 {
        return None;
    }
    let (clip_geom, clip_radii, clip_r_phys) = encode_clip_geom(
        spec.clip,
        spec.clip_radius,
        spec.clip_radii,
        sf.surface_scale,
    )?;
    let inv = spec.transform.inverse()?;
    let s = sf.surface_scale;
    let (hw, hh) = (0.5 * spec.rect.width * s, 0.5 * spec.rect.height * s);
    let (scx, scy) = spec.transform.transform_point(
        spec.rect.x + 0.5 * spec.rect.width,
        spec.rect.y + 0.5 * spec.rect.height,
    );
    let center_phys = (scx * s, scy * s);
    let bounds = project_screen_bounds(
        center_phys,
        spec.transform,
        offset,
        (hw + pad.0, hh + pad.1),
        sf.phys_w,
        sf.phys_h,
    )?;
    let corner_radii = normalize_radii_4(
        2.0 * hw,
        2.0 * hh,
        effective_radii_4(spec.border_radius, spec.corner_radii).map(|r| (r * s).max(0.0)),
    );
    Some(ProjectedBoxCommon {
        bounds,
        center_and_vp: [
            center_phys.0,
            center_phys.1,
            1.0 / sf.phys_w,
            1.0 / sf.phys_h,
        ],
        inv_linear: [inv.a, inv.b, inv.c, inv.d],
        box_geom: [
            hw,
            hh,
            (spec.border_radius * s).max(0.0),
            (spec.border_width * s).max(0.0),
        ],
        clip_geom,
        clip_radii,
        clip_r_phys,
        corner_radii,
    })
}

fn stops_max_span(stops: GradientStopsRef, repeating: bool) -> f32 {
    if !repeating {
        return 1.0;
    }
    stops.with_stops(|s| s.last().map_or(1.0, |st| st.position).clamp(1e-4, 1.0))
}

pub(crate) fn box_gradient_lut(spec: &BoxInstanceSpec) -> Option<(GradientStopsRef, f32)> {
    if spec.background_gradient.is_none() && spec.radial_gradient.is_none() {
        if let Some(cg) = spec.conic_gradient {
            if !cg.stops.is_empty() {
                return Some((cg.stops, stops_max_span(cg.stops, cg.repeating)));
            }
        }
    }
    if spec.background_gradient.is_none() {
        if let Some(rg) = spec.radial_gradient {
            if !rg.extra_stops.is_empty() {
                return Some((rg.extra_stops, stops_max_span(rg.extra_stops, rg.repeating)));
            }
        }
    }
    if let Some(lg) = spec.background_gradient {
        if !lg.extra_stops.is_empty() {
            return Some((lg.extra_stops, stops_max_span(lg.extra_stops, lg.repeating)));
        }
    }
    None
}

type LinearGradientUniform = (u32, u32, [f32; 4], [f32; 4], f32, [f32; 4], [f32; 4]);

fn encode_linear_gradient(grad: Option<LinearGradient>) -> LinearGradientUniform {
    let Some(g) = grad else {
        return (
            0,
            0,
            [0.0; 4],
            [0.0; 4],
            0.0,
            [0.0; 4],
            [0.0, 0.5, 1.0, 0.0],
        );
    };
    if !g.extra_stops.is_empty() {
        let span = stops_max_span(g.extra_stops, g.repeating);
        return (
            4,
            u32::from(g.repeating),
            g.start_color.to_f32_array(),
            g.end_color.to_f32_array(),
            g.angle_deg.to_radians(),
            [0.0; 4],
            [span, 0.0, 0.0, 0.0],
        );
    }
    let (cnt, mc) = match g.mid_color {
        Some(col) => (2_u32, col.to_f32_array()),
        None => (1_u32, [0.0; 4]),
    };
    let [p0, p1, p2] = g.stop_positions;
    (
        cnt,
        0,
        g.start_color.to_f32_array(),
        g.end_color.to_f32_array(),
        g.angle_deg.to_radians(),
        mc,
        [
            p0.clamp(0.0, 1.0),
            p1.clamp(0.0, 1.0),
            p2.clamp(0.0, 1.0),
            0.0,
        ],
    )
}

fn build_conic_box(
    spec: &BoxInstanceSpec,
    cg: ResolvedConicGradient,
    c: &ProjectedBoxCommon,
    scale: f32,
) -> SdfQuadInstance {
    let span = stops_max_span(cg.stops, cg.repeating);
    SdfQuadInstance {
        screen_bounds: c.bounds,
        center_and_vp: c.center_and_vp,
        inv_linear: c.inv_linear,
        box_geom: c.box_geom,
        clip_geom: c.clip_geom,
        color_a: spec.background_color.to_f32_array(),
        color_b: spec.border_color.to_f32_array(),
        grad_start: [0.0; 4],
        grad_end: [0.0; 4],
        uv_rect: [cg.center_dx * scale, cg.center_dy * scale, span, 0.0],
        shadow_params: [0.0; 4],
        extra_params: [
            spec.opacity.clamp(0.0, 1.0),
            c.clip_r_phys,
            cg.from_angle_deg.to_radians(),
            0.0,
        ],
        flags: [KIND_STYLED_BOX, 0, 6, u32::from(cg.repeating)],
        corner_radii: c.corner_radii,
        clip_radii: c.clip_radii,
    }
}

fn build_radial_box(
    spec: &BoxInstanceSpec,
    rg: ResolvedRadialGradient,
    c: &ProjectedBoxCommon,
    s: f32,
) -> SdfQuadInstance {
    let (mode, rep_flag, inner_or_span, outer_or_zero) = if !rg.extra_stops.is_empty() {
        (
            5,
            u32::from(rg.repeating),
            stops_max_span(rg.extra_stops, rg.repeating),
            0.0,
        )
    } else {
        (3, 0, rg.inner_stop, rg.outer_stop)
    };
    SdfQuadInstance {
        screen_bounds: c.bounds,
        center_and_vp: c.center_and_vp,
        inv_linear: c.inv_linear,
        box_geom: c.box_geom,
        clip_geom: c.clip_geom,
        color_a: spec.background_color.to_f32_array(),
        color_b: spec.border_color.to_f32_array(),
        grad_start: rg.inner_color.to_f32_array(),
        grad_end: rg.outer_color.to_f32_array(),
        uv_rect: [
            rg.center_dx * s,
            rg.center_dy * s,
            (rg.radius_x * s).max(1e-3),
            (rg.radius_y * s).max(1e-3),
        ],
        shadow_params: [0.0; 4],
        extra_params: [
            spec.opacity.clamp(0.0, 1.0),
            c.clip_r_phys,
            inner_or_span,
            outer_or_zero,
        ],
        flags: [KIND_STYLED_BOX, 0, mode, rep_flag],
        corner_radii: c.corner_radii,
        clip_radii: c.clip_radii,
    }
}

impl SdfQuadInstance {
    pub(crate) fn styled_box(spec: &BoxInstanceSpec, sf: SurfaceFrameTransform) -> Option<Self> {
        let has_fill = spec.background_color.a > 0
            || spec.background_gradient.is_some()
            || spec.radial_gradient.is_some()
            || spec.conic_gradient.is_some();
        let has_border = spec.border_width > 0.0 && spec.border_color.a > 0;
        if !has_fill && !has_border {
            return None;
        }
        let c = prepare_box_common(spec, sf, (0.0, 0.0), (2.0, 2.0))?;
        if spec.background_gradient.is_none() && spec.radial_gradient.is_none() {
            if let Some(cg) = spec.conic_gradient {
                return Some(build_conic_box(spec, cg, &c, sf.surface_scale));
            }
        }
        if spec.background_gradient.is_none() {
            if let Some(rg) = spec.radial_gradient {
                return Some(build_radial_box(spec, rg, &c, sf.surface_scale));
            }
        }
        let (has_grad, rep_flag, grad_s, grad_e, angle_rad, mid_col, stop_uv) =
            encode_linear_gradient(spec.background_gradient);
        Some(Self {
            screen_bounds: c.bounds,
            center_and_vp: c.center_and_vp,
            inv_linear: c.inv_linear,
            box_geom: c.box_geom,
            clip_geom: c.clip_geom,
            color_a: spec.background_color.to_f32_array(),
            color_b: spec.border_color.to_f32_array(),
            grad_start: grad_s,
            grad_end: grad_e,
            uv_rect: stop_uv,
            shadow_params: mid_col,
            extra_params: [spec.opacity.clamp(0.0, 1.0), c.clip_r_phys, angle_rad, 0.0],
            flags: [KIND_STYLED_BOX, 0, has_grad, rep_flag],
            corner_radii: c.corner_radii,
            clip_radii: c.clip_radii,
        })
    }

    pub(crate) fn box_shadow(
        spec: &BoxInstanceSpec,
        shadow: BoxShadow,
        sf: SurfaceFrameTransform,
    ) -> Option<Self> {
        if shadow.color.a == 0 {
            return None;
        }
        let s = sf.surface_scale;
        let (ox, oy) = (shadow.offset_x * s, shadow.offset_y * s);
        let blur = (shadow.blur_radius * s).max(0.0);
        let spread = shadow.spread_radius * s;
        let (kind, offset, pad) = if shadow.inset {
            (KIND_INSET_SHADOW, (0.0, 0.0), (1.5, 1.5))
        } else {
            let p = spread.max(0.0) + blur * 2.2 + 2.5;
            (KIND_OUTER_SHADOW, (ox, oy), (p, p))
        };
        let c = prepare_box_common(spec, sf, offset, pad)?;
        Some(Self {
            screen_bounds: c.bounds,
            center_and_vp: c.center_and_vp,
            inv_linear: c.inv_linear,
            box_geom: c.box_geom,
            clip_geom: c.clip_geom,
            color_a: shadow.color.to_f32_array(),
            color_b: [0.0; 4],
            grad_start: [0.0; 4],
            grad_end: [0.0; 4],
            uv_rect: [0.0; 4],
            shadow_params: [ox, oy, blur, spread],
            extra_params: [spec.opacity.clamp(0.0, 1.0), c.clip_r_phys, 0.0, 0.0],
            flags: [kind, 0, 0, 0],
            corner_radii: c.corner_radii,
            clip_radii: c.clip_radii,
        })
    }
}
