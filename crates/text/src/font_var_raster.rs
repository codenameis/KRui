use crate::font_metrics::{CachedFaceMeta, FvarAxisInfo};
use cosmic_text::rustybuzz::Variation as RbVariation;
use cosmic_text::ttf_parser::{GlyphId as TtfGlyphId, Tag as TtfTag};
use cosmic_text::SwashImage;
use krui_style::FontVariationList;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Vector;
use swash::{tag_from_bytes, Setting};

/// Decoded bitmap payload from a `cosmic-text` / `swash` rasterized glyph image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedGlyphBitmap {
    /// Monochrome 8-bit coverage mask (`width * height` bytes), always populated.
    pub alpha_mask: Arc<[u8]>,
    /// Straight-alpha `RGBA8` bitmap (`width * height * 4` bytes) when `is_color` is `true`.
    pub color_rgba: Option<Arc<[u8]>>,
    /// `true` when the source glyph used `SwashContent::Color` (e.g. COLRv0/COLRv1/CBDT color emoji).
    pub is_color: bool,
}

/// Extracts the `R8` alpha mask and optional straight `RGBA8` color bitmap from a `SwashImage`.
pub fn decode_swash_image(image: &cosmic_text::SwashImage) -> DecodedGlyphBitmap {
    match image.content {
        cosmic_text::SwashContent::Mask => DecodedGlyphBitmap {
            alpha_mask: Arc::from(image.data.as_slice()),
            color_rgba: None,
            is_color: false,
        },
        cosmic_text::SwashContent::Color => {
            let alpha: Vec<u8> = image
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|px| px[3])
                .collect();
            DecodedGlyphBitmap {
                alpha_mask: Arc::from(alpha),
                color_rgba: Some(Arc::from(image.data.as_slice())),
                is_color: true,
            }
        }
        cosmic_text::SwashContent::SubpixelMask => {
            let pixel_count = (image.placement.width as usize) * (image.placement.height as usize);
            let stride = if (pixel_count > 0 && image.data.len() == pixel_count * 3)
                || (pixel_count == 0
                    && image.data.len().is_multiple_of(3)
                    && !image.data.len().is_multiple_of(4))
            {
                3
            } else {
                4
            };
            let alpha: Vec<u8> = image
                .data
                .chunks_exact(stride)
                .map(|px| ((px[0] as u16 + px[1] as u16 + px[2] as u16 + 1) / 3) as u8)
                .collect();
            DecodedGlyphBitmap {
                alpha_mask: Arc::from(alpha),
                color_rgba: None,
                is_color: false,
            }
        }
    }
}

const F2DOT14_ONE: i32 = 1 << 14;
const NORM_COORD_BINS_PER_SIDE: i32 = 32;
const NORM_COORD_QUANT_STEP: i32 = F2DOT14_ONE / NORM_COORD_BINS_PER_SIDE;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedAxisSetting {
    pub(crate) tag: [u8; 4],
    pub(crate) quantized_user_value: f32,
    pub(crate) quantized_norm_bin: i16,
}

pub(crate) fn resolve_active_fvar_settings(
    meta: &CachedFaceMeta,
    font_size: f32,
    font_weight: u16,
    variations: FontVariationList,
) -> Vec<ResolvedAxisSetting> {
    if meta.fvar_axes.is_empty() {
        return Vec::new();
    }
    let mut values: Vec<([u8; 4], f32, FvarAxisInfo)> = meta
        .fvar_axes
        .iter()
        .map(|&ax| {
            let initial = if ax.tag == *b"opsz" {
                font_size.clamp(ax.min, ax.max)
            } else if ax.tag == *b"wght" {
                (font_weight as f32).clamp(ax.min, ax.max)
            } else {
                ax.def
            };
            (ax.tag, initial, ax)
        })
        .collect();

    for custom in variations.as_slice() {
        for (tag, slot_val, ax) in &mut values {
            if *tag == custom.tag {
                *slot_val = custom.value.clamp(ax.min, ax.max);
            }
        }
    }

    let mut active = Vec::new();
    for (tag, val, ax) in values {
        let (q_bin, q_val) = quantize_axis_coord(val, ax);
        if q_bin != 0 || (val - ax.def).abs() > 1e-3 {
            active.push(ResolvedAxisSetting {
                tag,
                quantized_user_value: q_val,
                quantized_norm_bin: q_bin,
            });
        }
    }
    active
}

fn quantize_axis_coord(val: f32, ax: FvarAxisInfo) -> (i16, f32) {
    let clamped = val.clamp(ax.min, ax.max);
    let norm_f = if clamped < ax.def && ax.def > ax.min {
        (clamped - ax.def) / (ax.def - ax.min)
    } else if clamped > ax.def && ax.max > ax.def {
        (clamped - ax.def) / (ax.max - ax.def)
    } else {
        0.0
    };
    let norm_2_14 = (norm_f.clamp(-1.0, 1.0) * F2DOT14_ONE as f32).round() as i32;
    let bin = (norm_2_14 + NORM_COORD_QUANT_STEP / 2)
        .div_euclid(NORM_COORD_QUANT_STEP)
        .clamp(-NORM_COORD_BINS_PER_SIDE, NORM_COORD_BINS_PER_SIDE) as i16;
    let snapped_norm =
        ((bin as i32 * NORM_COORD_QUANT_STEP) as f32 / F2DOT14_ONE as f32).clamp(-1.0, 1.0);
    let snapped_user = if snapped_norm < 0.0 {
        ax.def + snapped_norm * (ax.def - ax.min)
    } else if snapped_norm > 0.0 {
        ax.def + snapped_norm * (ax.max - ax.def)
    } else {
        ax.def
    };
    (bin, snapped_user)
}

pub(crate) fn compute_glyph_atlas_cache_key(
    ck: cosmic_text::CacheKey,
    active_vars: &[ResolvedAxisSetting],
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ck.font_id.hash(&mut hasher);
    ck.glyph_id.hash(&mut hasher);
    let ppem = f32::from_bits(ck.font_size_bits).max(1.0);
    let ppem_q = (ppem * 4.0).round() as u32;
    ppem_q.hash(&mut hasher);
    (ck.x_bin as u8).hash(&mut hasher);
    (ck.y_bin as u8).hash(&mut hasher);
    ck.flags.bits().hash(&mut hasher);
    for v in active_vars {
        v.tag.hash(&mut hasher);
        v.quantized_norm_bin.hash(&mut hasher);
    }
    hasher.finish().max(1)
}

pub(crate) fn rasterize_variable_glyph(
    scale_ctx: &mut ScaleContext,
    font: &cosmic_text::Font,
    ck: cosmic_text::CacheKey,
    active_vars: &[ResolvedAxisSetting],
) -> Option<SwashImage> {
    let ppem = ((f32::from_bits(ck.font_size_bits).max(1.0) * 4.0).round()) * 0.25;
    let settings: Vec<Setting<f32>> = active_vars
        .iter()
        .map(|v| Setting {
            tag: tag_from_bytes(&v.tag),
            value: v.quantized_user_value,
        })
        .collect();
    let mut scaler = scale_ctx
        .builder(font.as_swash())
        .size(ppem)
        .hint(true)
        .variations(settings)
        .build();
    let offset = Vector::new(ck.x_bin.as_float(), ck.y_bin.as_float());
    Render::new(&[
        Source::ColorOutline(0),
        Source::ColorBitmap(StrikeWith::BestFit),
        Source::Outline,
    ])
    .offset(offset)
    .render(&mut scaler, ck.glyph_id)
}

pub(crate) fn varied_glyph_advance_px(
    font: &cosmic_text::Font,
    glyph_id: u16,
    font_size: f32,
    active_vars: &[ResolvedAxisSetting],
) -> Option<f32> {
    if active_vars.is_empty() {
        return None;
    }
    let base_face = font.rustybuzz();
    let upem = (base_face.units_per_em().max(1)) as f32;
    let mut varied = base_face.clone();
    let rb_vars: Vec<RbVariation> = active_vars
        .iter()
        .map(|v| RbVariation {
            tag: TtfTag::from_bytes(&v.tag),
            value: v.quantized_user_value,
        })
        .collect();
    varied.set_variations(&rb_vars);
    let adv = varied.glyph_hor_advance(TtfGlyphId(glyph_id))?;
    Some((adv as f32 / upem) * font_size)
}
