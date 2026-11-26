//! Visual composition for `krui serve` device frames (`--frame iphone-15|pixel-8|rpi-800x480`),
//! non-destructive `CompileError` overlays, and debug overlays (`set_overlay(relayout|fps|hit)`).

use crate::protocol::{DeviceFramePreset, PreviewOverlayFlags};
use krui_compiler::CompileError;
use krui_core::{DrawPrimitive, InspectedNodeInfo, PaintScene, ViewTree};
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color, FontFamilyStack};
use krui_text::TextEngine;

pub(crate) fn decorate_tree_with_engine(
    tree: &ViewTree,
    scene: &mut PaintScene,
    text_engine: &mut Option<TextEngine>,
    compile_error: Option<&CompileError>,
    overlays: PreviewOverlayFlags,
    device_frame: Option<DeviceFramePreset>,
    last_frame_us: u128,
) {
    if compile_error.is_none()
        && !overlays.relayout
        && !overlays.fps
        && !overlays.hit
        && device_frame.is_none()
    {
        return;
    }
    let inspected = if overlays.hit {
        tree.inspect_nodes()
    } else {
        Vec::new()
    };
    let engine = text_engine.get_or_insert_with(TextEngine::new);
    decorate_preview_paint_scene(
        scene,
        engine,
        &inspected,
        compile_error,
        overlays,
        device_frame,
        tree.relayout_passes(),
        last_frame_us,
    );
}

/// Appends debug overlays (`relayout`, `fps`, `hit`), a non-destructive `CompileError` banner,
/// and an optional device frame bezel (`iphone-15`, `pixel-8`, `rpi-800x480`) onto `scene`.
#[allow(clippy::too_many_arguments)]
pub fn decorate_preview_paint_scene(
    scene: &mut PaintScene,
    text_engine: &mut TextEngine,
    inspected_nodes: &[InspectedNodeInfo],
    compile_error: Option<&CompileError>,
    overlays: PreviewOverlayFlags,
    device_frame: Option<DeviceFramePreset>,
    relayout_passes: u64,
    last_frame_us: u128,
) {
    if overlays.hit {
        for node in inspected_nodes.iter().filter(|n| n.is_interactive) {
            scene.primitives.push(DrawPrimitive::StyledBox {
                rect: node.world_rect,
                clip: None,
                clip_radius: 0.0,
                clip_radii: [0.0; 4],
                transform: Affine2::IDENTITY,
                opacity: 0.9,
                background_color: Color::rgba(56, 189, 248, 28),
                background_gradient: None,
                radial_gradient: None,
                conic_gradient: None,
                border_color: Color::rgb(56, 189, 248),
                border_width: 1.5,
                border_radius: 4.0,
                corner_radii: [4.0; 4],
                box_shadow: None,
                backdrop_blur_px: 0.0,
            });
        }
    }

    if let Some(frame) = device_frame {
        apply_device_frame_bezel(scene, text_engine, frame);
    }

    push_hud_and_error_banners(
        scene,
        text_engine,
        compile_error,
        overlays,
        relayout_passes,
        last_frame_us,
    );
}

fn push_hud_and_error_banners(
    scene: &mut PaintScene,
    text_engine: &mut TextEngine,
    compile_error: Option<&CompileError>,
    overlays: PreviewOverlayFlags,
    relayout_passes: u64,
    last_frame_us: u128,
) {
    if overlays.relayout || overlays.fps {
        let mut badge_parts = Vec::new();
        if overlays.fps {
            let fps = if last_frame_us > 0 {
                (1_000_000.0 / (last_frame_us as f64)).min(999.0) as u32
            } else {
                120
            };
            badge_parts.push(format!("{fps} FPS ({last_frame_us} us)"));
        }
        if overlays.relayout {
            badge_parts.push(format!("relayouts: {relayout_passes}"));
        }
        push_banner_box_and_text(
            scene,
            text_engine,
            LayoutRect {
                x: 12.0,
                y: 12.0,
                width: 280.0,
                height: 28.0,
            },
            Color::rgba(15, 23, 42, 225),
            Color::rgb(56, 189, 248),
            &badge_parts.join(" | "),
            12.0,
        );
    }

    if let Some(err) = compile_error {
        push_compile_error_banner(scene, text_engine, err);
    }
}

fn push_compile_error_banner(
    scene: &mut PaintScene,
    text_engine: &mut TextEngine,
    err: &CompileError,
) {
    let file_label = err
        .file
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("<krui>");
    let banner_msg = format!(
        "CompileError ({file_label}:{}:{}): {}",
        err.line, err.column, err.message
    );
    let vw = scene.width as f32;
    push_banner_box_and_text(
        scene,
        text_engine,
        LayoutRect {
            x: 12.0,
            y: 12.0,
            width: (vw - 24.0).max(200.0),
            height: 52.0,
        },
        Color::rgba(127, 29, 29, 242),
        Color::rgb(254, 226, 226),
        &banner_msg,
        13.0,
    );
}

fn push_banner_box_and_text(
    scene: &mut PaintScene,
    text_engine: &mut TextEngine,
    rect: LayoutRect,
    bg: Color,
    fg: Color,
    text: &str,
    font_size: f32,
) {
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: bg,
        background_gradient: None,
        radial_gradient: None,
        conic_gradient: None,
        border_color: fg,
        border_width: 1.5,
        border_radius: 6.0,
        corner_radii: [6.0; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });
    let shaped = text_engine.shape_and_rasterize_with_scale(
        text,
        font_size,
        1.35,
        600,
        FontFamilyStack::default(),
        fg,
        Some((rect.width - 20.0).max(60.0)),
        scene.scale_factor,
    );
    scene.primitives.push(DrawPrimitive::TextGlyphs {
        origin_x: rect.x + 10.0,
        origin_y: rect.y + 8.0,
        clip: Some(rect),
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        color: fg,
        glyphs: shaped.glyphs,
    });
}

fn intersect_rect(a: LayoutRect, b: LayoutRect) -> LayoutRect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    LayoutRect {
        x: x0,
        y: y0,
        width: (x1 - x0).max(0.0),
        height: (y1 - y0).max(0.0),
    }
}

fn shift_and_clip_primitives_to_screen(
    scene: &mut PaintScene,
    origin_x: f32,
    origin_y: f32,
    screen_rect: LayoutRect,
) {
    let shift = Affine2::translation(origin_x, origin_y);
    for prim in &mut scene.primitives {
        match prim {
            DrawPrimitive::StyledBox {
                clip, transform, ..
            }
            | DrawPrimitive::TextGlyphs {
                clip, transform, ..
            }
            | DrawPrimitive::CanvasBatch {
                clip, transform, ..
            }
            | DrawPrimitive::Image {
                clip, transform, ..
            } => {
                *transform = shift * *transform;
                let shifted_clip = match clip {
                    Some(c) => {
                        let moved = LayoutRect {
                            x: c.x + origin_x,
                            y: c.y + origin_y,
                            width: c.width,
                            height: c.height,
                        };
                        intersect_rect(moved, screen_rect)
                    }
                    None => screen_rect,
                };
                *clip = Some(shifted_clip);
            }
            DrawPrimitive::PushLayer { bounds, clip, .. } => {
                bounds.x += origin_x;
                bounds.y += origin_y;
                let shifted_clip = match clip {
                    Some(c) => intersect_rect(
                        LayoutRect {
                            x: c.x + origin_x,
                            y: c.y + origin_y,
                            width: c.width,
                            height: c.height,
                        },
                        screen_rect,
                    ),
                    None => screen_rect,
                };
                *clip = Some(shifted_clip);
            }
            DrawPrimitive::PopLayer => {}
        }
    }
}

fn push_top_cutout(scene: &mut PaintScene, frame: DeviceFramePreset, bezel: f32, sw: u32) {
    let (w, h, y, radius, alpha) = match frame {
        DeviceFramePreset::Iphone15 => (116.0, 24.0, 10.0, 12.0, 0.92),
        DeviceFramePreset::Pixel8 => (10.0, 10.0, 12.0, 5.0, 0.95),
        DeviceFramePreset::Rpi800x480 => return,
    };
    push_chrome_pill(
        scene,
        LayoutRect {
            x: bezel + (sw as f32 - w) * 0.5,
            y: bezel + y,
            width: w,
            height: h,
        },
        Color::rgb(15, 23, 42),
        Color::rgb(51, 65, 85),
        1.0,
        radius,
        alpha,
    );
}

fn push_safe_area_chrome(
    scene: &mut PaintScene,
    frame: DeviceFramePreset,
    bezel: f32,
    sw: u32,
    sh: u32,
    bottom_safe: f32,
) {
    if frame == DeviceFramePreset::Rpi800x480 {
        return;
    }
    push_top_cutout(scene, frame, bezel, sw);
    if bottom_safe > 0.0 {
        let bar_w = if frame == DeviceFramePreset::Iphone15 {
            132.0
        } else {
            108.0
        };
        push_chrome_pill(
            scene,
            LayoutRect {
                x: bezel + (sw as f32 - bar_w) * 0.5,
                y: bezel + sh as f32 - 14.0,
                width: bar_w,
                height: 5.0,
            },
            Color::rgb(148, 163, 184),
            Color::TRANSPARENT,
            0.0,
            2.5,
            0.75,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn push_chrome_pill(
    scene: &mut PaintScene,
    rect: LayoutRect,
    bg: Color,
    border: Color,
    border_width: f32,
    radius: f32,
    opacity: f32,
) {
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity,
        background_color: bg,
        background_gradient: None,
        radial_gradient: None,
        conic_gradient: None,
        border_color: border,
        border_width,
        border_radius: radius,
        corner_radii: [radius; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });
}

fn bezel_border_spec(frame: DeviceFramePreset) -> (f32, f32) {
    match frame {
        DeviceFramePreset::Iphone15 => (3.0, 36.0),
        DeviceFramePreset::Pixel8 => (3.0, 28.0),
        DeviceFramePreset::Rpi800x480 => (4.0, 4.0),
    }
}

fn apply_device_frame_bezel(
    scene: &mut PaintScene,
    text_engine: &mut TextEngine,
    frame: DeviceFramePreset,
) {
    let bezel = frame.bezel_padding();
    let (sw, sh) = frame.screen_size();
    let (_, _, bottom_safe, _) = frame.safe_area_insets();
    let (origin_x, origin_y) = frame.content_origin();
    let screen_rect = LayoutRect {
        x: bezel,
        y: bezel,
        width: sw as f32,
        height: sh as f32,
    };

    shift_and_clip_primitives_to_screen(scene, origin_x, origin_y, screen_rect);
    let (ow, oh) = frame.outer_window_size();
    scene.width = ow;
    scene.height = oh;

    let (border_width, border_radius) = bezel_border_spec(frame);
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: screen_rect,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: Color::TRANSPARENT,
        background_gradient: None,
        radial_gradient: None,
        conic_gradient: None,
        border_color: Color::rgb(71, 85, 105),
        border_width,
        border_radius,
        corner_radii: [border_radius; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });

    push_safe_area_chrome(scene, frame, bezel, sw, sh, bottom_safe);
    let label_rect = LayoutRect {
        x: bezel,
        y: 2.0,
        width: 240.0,
        height: 20.0,
    };
    push_banner_box_and_text(
        scene,
        text_engine,
        label_rect,
        Color::rgba(15, 23, 42, 220),
        Color::rgb(148, 163, 184),
        frame.label(),
        11.0,
    );
}
