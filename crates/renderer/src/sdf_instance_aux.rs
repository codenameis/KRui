//! Auxiliary `SdfQuadInstance` builders (`glyph`, `backdrop_blur_box`, `image_quad`, `composite_layer`).

use crate::dual_atlas::GlyphSlot;
use crate::sdf_instance::{
    encode_clip_geom, prepare_box_common, project_screen_bounds, BoxInstanceSpec, SdfQuadInstance,
    SurfaceFrameTransform, KIND_COLOR_GLYPH, KIND_IMAGE_TEX, KIND_MASK_GLYPH, KIND_SAMPLE_AUX_TEX,
};
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color};
use krui_text::RasterizedGlyph;

impl SdfQuadInstance {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn glyph(
        g: &RasterizedGlyph,
        slot: GlyphSlot,
        origin: (f32, f32),
        clip: Option<LayoutRect>,
        clip_radius: f32,
        clip_radii_raw: [f32; 4],
        transform: Affine2,
        opacity: f32,
        default_color: Color,
        sf: SurfaceFrameTransform,
    ) -> Option<Self> {
        let fg = g.glyph_color.unwrap_or(default_color);
        if g.width == 0 || g.height == 0 || opacity <= 0.0 || (!slot.is_color && fg.a == 0) {
            return None;
        }
        let (clip_geom, clip_radii, clip_r_phys) =
            encode_clip_geom(clip, clip_radius, clip_radii_raw, sf.surface_scale)?;
        let inv = transform.inverse()?;
        let (hw, hh, center_phys) = compute_glyph_center(g, origin, transform, sf);
        let bounds = project_screen_bounds(
            center_phys,
            transform,
            (0.0, 0.0),
            (hw + 1.0, hh + 1.0),
            sf.phys_w,
            sf.phys_h,
        )?;
        let kind = if slot.is_color {
            KIND_COLOR_GLYPH
        } else {
            KIND_MASK_GLYPH
        };
        Some(Self {
            screen_bounds: bounds,
            center_and_vp: [
                center_phys.0,
                center_phys.1,
                1.0 / sf.phys_w,
                1.0 / sf.phys_h,
            ],
            inv_linear: [inv.a, inv.b, inv.c, inv.d],
            box_geom: [hw, hh, 0.0, 0.0],
            clip_geom,
            color_a: fg.to_f32_array(),
            color_b: [0.0; 4],
            grad_start: [0.0; 4],
            grad_end: [0.0; 4],
            uv_rect: slot.uv,
            shadow_params: [0.0; 4],
            extra_params: [opacity.clamp(0.0, 1.0), clip_r_phys, 0.0, 0.0],
            flags: [kind, slot.layer, 0, 0],
            corner_radii: [0.0; 4],
            clip_radii,
        })
    }

    pub(crate) fn backdrop_blur_box(
        spec: &BoxInstanceSpec,
        sf: SurfaceFrameTransform,
    ) -> Option<Self> {
        let c = prepare_box_common(spec, sf, (0.0, 0.0), (2.0, 2.0))?;
        Some(Self {
            screen_bounds: c.bounds,
            center_and_vp: c.center_and_vp,
            inv_linear: c.inv_linear,
            box_geom: [c.box_geom[0], c.box_geom[1], c.box_geom[2], 0.0],
            clip_geom: c.clip_geom,
            color_a: [1.0; 4],
            color_b: [0.0; 4],
            grad_start: [0.0; 4],
            grad_end: [0.0; 4],
            uv_rect: [0.0; 4],
            shadow_params: [0.0; 4],
            extra_params: [spec.opacity.clamp(0.0, 1.0), c.clip_r_phys, 0.0, 1.0],
            flags: [KIND_SAMPLE_AUX_TEX, 0, 0, 0],
            corner_radii: c.corner_radii,
            clip_radii: c.clip_radii,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn image_quad(
        rect: LayoutRect,
        uv_rect: [f32; 4],
        clip: Option<LayoutRect>,
        clip_radius: f32,
        clip_radii: [f32; 4],
        transform: Affine2,
        opacity: f32,
        border_radius: f32,
        is_premultiplied: bool,
        sf: SurfaceFrameTransform,
    ) -> Option<Self> {
        let snapped_rect = snap_identity_image_rect(rect, transform, sf.surface_scale);
        let spec = BoxInstanceSpec {
            rect: snapped_rect,
            clip,
            clip_radius,
            clip_radii,
            transform,
            opacity,
            background_color: Color::TRANSPARENT,
            background_gradient: None,
            radial_gradient: None,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            border_radius,
            corner_radii: [border_radius; 4],
        };
        let c = prepare_box_common(&spec, sf, (0.0, 0.0), (1.0, 1.0))?;
        Some(Self {
            screen_bounds: c.bounds,
            center_and_vp: c.center_and_vp,
            inv_linear: c.inv_linear,
            box_geom: [c.box_geom[0], c.box_geom[1], c.box_geom[2], 0.0],
            clip_geom: c.clip_geom,
            color_a: [1.0; 4],
            color_b: [0.0; 4],
            grad_start: [0.0; 4],
            grad_end: [0.0; 4],
            uv_rect,
            shadow_params: [0.0; 4],
            extra_params: [opacity.clamp(0.0, 1.0), c.clip_r_phys, 0.0, 0.0],
            flags: [KIND_IMAGE_TEX, u32::from(is_premultiplied), 0, 0],
            corner_radii: c.corner_radii,
            clip_radii: c.clip_radii,
        })
    }

    pub(crate) fn composite_layer(
        clip: Option<LayoutRect>,
        clip_radius: f32,
        clip_radii_raw: [f32; 4],
        local_clip: Option<(LayoutRect, [f32; 4], Affine2)>,
        drawn_bounds: Option<[f32; 4]>,
        opacity: f32,
        sf: SurfaceFrameTransform,
    ) -> Option<Self> {
        let drawn = drawn_bounds?;
        if opacity <= 0.0 {
            return None;
        }
        let (clip_geom, clip_radii, clip_r_phys) =
            encode_clip_geom(clip, clip_radius, clip_radii_raw, sf.surface_scale)?;
        let cx = |v: f32| v.clamp(0.0, sf.phys_w);
        let cy = |v: f32| v.clamp(0.0, sf.phys_h);
        let (mut x0, mut y0, mut x1, mut y1) = (
            cx(drawn[0].floor()),
            cy(drawn[1].floor()),
            cx(drawn[2].ceil()),
            cy(drawn[3].ceil()),
        );
        if let Some(c) = clip {
            let s = sf.surface_scale;
            x0 = x0.max(cx((c.x * s).floor()));
            y0 = y0.max(cy((c.y * s).floor()));
            x1 = x1.min(cx(((c.x + c.width) * s).ceil()));
            y1 = y1.min(cy(((c.y + c.height) * s).ceil()));
        }
        let (center_and_vp, inv_linear, box_geom, corner_radii, local_mask_flag) =
            resolve_layer_local_clip_geom(
                (clip, clip_radius, clip_radii_raw),
                local_clip,
                opacity,
                sf,
                (&mut x0, &mut y0, &mut x1, &mut y1),
            )?;
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(Self {
            screen_bounds: [x0, y0, x1, y1],
            center_and_vp,
            inv_linear,
            box_geom,
            clip_geom,
            color_a: [1.0; 4],
            color_b: [0.0; 4],
            grad_start: [0.0; 4],
            grad_end: [0.0; 4],
            uv_rect: [0.0; 4],
            shadow_params: [0.0; 4],
            extra_params: [opacity.clamp(0.0, 1.0), clip_r_phys, 0.0, local_mask_flag],
            flags: [KIND_SAMPLE_AUX_TEX, 0, 0, 0],
            corner_radii,
            clip_radii,
        })
    }
}

type LayerLocalClipResult = ([f32; 4], [f32; 4], [f32; 4], [f32; 4], f32);

fn resolve_layer_local_clip_geom(
    outer: (Option<LayoutRect>, f32, [f32; 4]),
    local_clip: Option<(LayoutRect, [f32; 4], Affine2)>,
    opacity: f32,
    sf: SurfaceFrameTransform,
    (x0, y0, x1, y1): (&mut f32, &mut f32, &mut f32, &mut f32),
) -> Option<LayerLocalClipResult> {
    let Some((lrect, lradii, ltx)) = local_clip else {
        return Some((
            [
                0.5 * sf.phys_w,
                0.5 * sf.phys_h,
                1.0 / sf.phys_w,
                1.0 / sf.phys_h,
            ],
            [1.0, 0.0, 0.0, 1.0],
            [0.5 * sf.phys_w, 0.5 * sf.phys_h, 0.0, 0.0],
            [0.0; 4],
            0.0,
        ));
    };
    let c = prepare_local_layer_clip(outer, (lrect, lradii, ltx), opacity, sf)?;
    *x0 = x0.max(c.bounds[0]);
    *y0 = y0.max(c.bounds[1]);
    *x1 = x1.min(c.bounds[2]);
    *y1 = y1.min(c.bounds[3]);
    Some((
        c.center_and_vp,
        c.inv_linear,
        [c.box_geom[0], c.box_geom[1], c.box_geom[2], 0.0],
        c.corner_radii,
        1.0,
    ))
}

fn prepare_local_layer_clip(
    (clip, clip_radius, clip_radii): (Option<LayoutRect>, f32, [f32; 4]),
    (rect, corner_radii, transform): (LayoutRect, [f32; 4], Affine2),
    opacity: f32,
    sf: SurfaceFrameTransform,
) -> Option<crate::sdf_instance::ProjectedBoxCommon> {
    let spec = BoxInstanceSpec {
        rect,
        clip,
        clip_radius,
        clip_radii,
        transform,
        opacity,
        background_color: Color::TRANSPARENT,
        background_gradient: None,
        radial_gradient: None,
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        border_radius: 0.0,
        corner_radii,
    };
    prepare_box_common(&spec, sf, (0.0, 0.0), (1.0, 1.0))
}

fn compute_glyph_center(
    g: &RasterizedGlyph,
    origin: (f32, f32),
    transform: Affine2,
    sf: SurfaceFrameTransform,
) -> (f32, f32, (f32, f32)) {
    let s = sf.surface_scale;
    let gw_logical = (g.width as f32) / sf.glyph_raster_scale;
    let gh_logical = (g.height as f32) / sf.glyph_raster_scale;
    let (hw, hh) = (0.5 * gw_logical * s, 0.5 * gh_logical * s);
    let center_phys = if transform.is_pure_translation() {
        let gx_phys = ((origin.0 + transform.tx + g.x) * s).round();
        let gy_phys = ((origin.1 + transform.ty + g.y) * s).round();
        (gx_phys + hw, gy_phys + hh)
    } else {
        let (scx, scy) = transform.transform_point(
            origin.0 + g.x + 0.5 * gw_logical,
            origin.1 + g.y + 0.5 * gh_logical,
        );
        (scx * s, scy * s)
    };
    (hw, hh, center_phys)
}

fn snap_identity_image_rect(rect: LayoutRect, transform: Affine2, s: f32) -> LayoutRect {
    if transform != Affine2::IDENTITY || rect.width <= 0.0 || rect.height <= 0.0 {
        return rect;
    }
    let x0 = (rect.x * s).round();
    let y0 = (rect.y * s).round();
    let x1 = ((rect.x + rect.width) * s).round().max(x0 + 1.0);
    let y1 = ((rect.y + rect.height) * s).round().max(y0 + 1.0);
    LayoutRect {
        x: x0 / s,
        y: y0 / s,
        width: (x1 - x0) / s,
        height: (y1 - y0) / s,
    }
}
