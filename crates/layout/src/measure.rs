//! Intrinsic text and media leaf measurement helpers for Taffy 0.15 callbacks.

use crate::TextContext;
use taffy::prelude::{AvailableSpace, Size};

/// Minimum font size (logical px) used when calculating intrinsic line height for empty inputs/buttons.
pub const MIN_MEASURED_FONT_SIZE_PX: f32 = 8.0;
/// Wrap width used to measure min-content text (widest unbreakable run).
const MIN_CONTENT_PROBE_WIDTH_PX: f32 = 1.0;

pub(crate) fn measure_taffy_leaf(
    known_dimensions: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    node_context: Option<&TextContext>,
    measure: &mut impl FnMut(&TextContext, Option<f32>) -> (f32, f32),
) -> Size<f32> {
    if let Size {
        width: Some(width),
        height: Some(height),
    } = known_dimensions
    {
        return Size { width, height };
    }
    let Some(ctx) = node_context else {
        return Size::ZERO;
    };
    if let Some((iw, ih)) = ctx.intrinsic_size {
        return measure_intrinsic_size(known_dimensions, iw, ih);
    }
    let has_rich = ctx.rich_spans.as_ref().is_some_and(|s| !s.is_empty());
    if ctx.text.is_empty() && !has_rich {
        let min_h = if ctx.min_line_height {
            let (_, strut_h) = measure(ctx, None);
            if strut_h > 0.0 {
                strut_h
            } else {
                let fs = ctx.text_style.font_size.max(MIN_MEASURED_FONT_SIZE_PX);
                ctx.text_style.line_height.resolve_px(fs, fs * 1.2).ceil()
            }
        } else {
            0.0
        };
        return Size {
            width: known_dimensions.width.unwrap_or(0.0),
            height: known_dimensions.height.unwrap_or(min_h),
        };
    }
    let max_width = if ctx.single_line {
        None
    } else {
        known_dimensions.width.or(match available_space.width {
            AvailableSpace::Definite(w) => Some(w.max(1.0)),
            AvailableSpace::MinContent => Some(MIN_CONTENT_PROBE_WIDTH_PX),
            AvailableSpace::MaxContent => None,
        })
    };
    let (measured_w, measured_h) = measure(ctx, max_width);
    let intrinsic_w = match (ctx.single_line, available_space.width) {
        (true, AvailableSpace::Definite(w)) => measured_w.min(w.max(1.0)),
        _ => measured_w,
    };
    Size {
        width: known_dimensions.width.unwrap_or(intrinsic_w),
        height: known_dimensions.height.unwrap_or(measured_h),
    }
}

fn measure_intrinsic_size(known_dimensions: Size<Option<f32>>, iw: f32, ih: f32) -> Size<f32> {
    let (iw, ih) = (iw.max(0.0), ih.max(0.0));
    match (known_dimensions.width, known_dimensions.height) {
        (Some(w), Some(h)) => Size {
            width: w,
            height: h,
        },
        (Some(w), None) if iw > 0.0 => Size {
            width: w,
            height: w * (ih / iw),
        },
        (None, Some(h)) if ih > 0.0 => Size {
            width: h * (iw / ih),
            height: h,
        },
        _ => Size {
            width: known_dimensions.width.unwrap_or(iw),
            height: known_dimensions.height.unwrap_or(ih),
        },
    }
}
