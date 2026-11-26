//! Ping-pong `PaintScenePool` and high-frequency `DynamicSlotBuffer` for zero-allocation
//! steady-state frame submission (`krui-engine-host` 120 FPS & `krui-kms` CPU partial refresh).

use crate::scene::{DrawPrimitive, PaintScene};
use krui_style::{Affine2, Color, DynamicSlotId};

/// Double-buffered `PaintScene` pool that retains heap `Vec<DrawPrimitive>` capacity across frames
/// via `.clear()`, achieving zero steady-state allocations at 120 FPS.
#[derive(Clone, Debug)]
pub struct PaintScenePool {
    scenes: [PaintScene; 2],
    back_idx: usize,
}

impl PaintScenePool {
    /// Creates a pre-sized ping-pong scene pool with initial `primitive_capacity`.
    #[must_use]
    pub fn new(width: u32, height: u32, clear_color: Color, primitive_capacity: usize) -> Self {
        let mut s0 = PaintScene::new(width, height, clear_color);
        let mut s1 = PaintScene::new(width, height, clear_color);
        s0.primitives.reserve(primitive_capacity);
        s1.primitives.reserve(primitive_capacity);
        Self {
            scenes: [s0, s1],
            back_idx: 0,
        }
    }

    /// Returns a cleared back buffer retaining its existing `Vec<DrawPrimitive>` capacity.
    pub fn acquire_back(
        &mut self,
        width: u32,
        height: u32,
        clear_color: Color,
        scale_factor: f32,
    ) -> &mut PaintScene {
        let back = &mut self.scenes[self.back_idx];
        back.width = width;
        back.height = height;
        back.clear_color = clear_color;
        back.scale_factor = scale_factor;
        back.primitives.clear();
        back
    }

    /// Publishes the back buffer as the active front snapshot and swaps buffers for the next frame.
    pub fn publish_back(&mut self) -> &PaintScene {
        let published = self.back_idx;
        self.back_idx ^= 1;
        &self.scenes[published]
    }

    /// Returns the most recently published front snapshot.
    #[must_use]
    pub fn front(&self) -> &PaintScene {
        &self.scenes[self.back_idx ^ 1]
    }

    /// Returns backing primitive `Vec` capacities `[buf0, buf1]` for zero-realloc diagnostics/tests.
    #[must_use]
    pub fn capacities(&self) -> [usize; 2] {
        [
            self.scenes[0].primitives.capacity(),
            self.scenes[1].primitives.capacity(),
        ]
    }
}

/// High-frequency override patched onto a retained primitive slot without layout or style rebuilds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DynamicSlotOverride {
    pub transform: Option<Affine2>,
    pub opacity: Option<f32>,
    pub background_color: Option<Color>,
}

/// Sparse mapping of [`DynamicSlotId`]s to primitive indices and live overrides.
#[derive(Clone, Debug, Default)]
pub struct DynamicSlotBuffer {
    slots: Vec<Option<(usize, DynamicSlotOverride)>>,
}

impl DynamicSlotBuffer {
    /// Clears primitive index bindings before rebuilding a scene while retaining buffer capacity.
    pub fn clear(&mut self) {
        self.slots.clear();
    }

    /// Associates `slot` with primitive index `prim_idx` in the active [`PaintScene`].
    pub fn bind_primitive(&mut self, slot: DynamicSlotId, prim_idx: usize) {
        let idx = slot.0 as usize;
        if self.slots.len() <= idx {
            self.slots.resize(idx + 1, None);
        }
        let existing_patch = self.slots[idx].map(|(_, ov)| ov).unwrap_or_default();
        self.slots[idx] = Some((prim_idx, existing_patch));
    }

    /// Updates override values for `slot` at high frequency without triggering relayout or restyle.
    pub fn set_override(&mut self, slot: DynamicSlotId, patch: DynamicSlotOverride) {
        let idx = slot.0 as usize;
        if self.slots.len() <= idx {
            self.slots.resize(idx + 1, None);
        }
        let prim_idx = self.slots[idx].map(|(p, _)| p).unwrap_or(usize::MAX);
        self.slots[idx] = Some((prim_idx, patch));
    }

    /// Applies all bound overrides in-place onto `scene.primitives`.
    pub fn apply_to_scene(&self, scene: &mut PaintScene) {
        for &(prim_idx, ov) in self.slots.iter().flatten() {
            if let Some(prim) = scene.primitives.get_mut(prim_idx) {
                apply_override_to_primitive(prim, ov);
            }
        }
    }
}

fn apply_override_to_primitive(prim: &mut DrawPrimitive, ov: DynamicSlotOverride) {
    match prim {
        DrawPrimitive::StyledBox {
            transform,
            opacity,
            background_color,
            ..
        } => {
            if let Some(tx) = ov.transform {
                *transform = tx;
            }
            if let Some(op) = ov.opacity {
                *opacity = op;
            }
            if let Some(bg) = ov.background_color {
                *background_color = bg;
            }
        }
        DrawPrimitive::TextGlyphs {
            transform, opacity, ..
        }
        | DrawPrimitive::Image {
            transform, opacity, ..
        }
        | DrawPrimitive::CanvasBatch {
            transform, opacity, ..
        } => {
            if let Some(tx) = ov.transform {
                *transform = tx;
            }
            if let Some(op) = ov.opacity {
                *opacity = op;
            }
        }
        DrawPrimitive::PushLayer { opacity, .. } => {
            if let Some(op) = ov.opacity {
                *opacity = op;
            }
        }
        DrawPrimitive::PopLayer => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use krui_layout::LayoutRect;

    fn sample_box(x: f32) -> DrawPrimitive {
        DrawPrimitive::StyledBox {
            rect: LayoutRect {
                x,
                y: 4.0,
                width: 40.0,
                height: 20.0,
            },
            clip: None,
            clip_radius: 0.0,
            clip_radii: [0.0; 4],
            transform: Affine2::IDENTITY,
            opacity: 1.0,
            background_color: Color {
                r: 10,
                g: 20,
                b: 30,
                a: 255,
            },
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
    fn paint_scene_pool_120_frames_zero_steady_state_reallocations() {
        let mut pool = PaintScenePool::new(320, 240, Color::BLACK, 64);
        for _ in 0..2 {
            let s = pool.acquire_back(320, 240, Color::BLACK, 1.0);
            for idx in 0..24 {
                s.primitives.push(sample_box(idx as f32));
            }
            let _ = pool.publish_back();
        }
        let baseline = pool.capacities();
        for frame in 0..120 {
            let s = pool.acquire_back(320, 240, Color::BLACK, 1.0);
            for idx in 0..24 {
                s.primitives.push(sample_box((idx + frame % 5) as f32));
            }
            let front = pool.publish_back();
            assert_eq!(front.primitives.len(), 24);
        }
        assert_eq!(pool.capacities(), baseline);
    }

    #[test]
    fn dynamic_slot_buffer_patches_scene_without_rebuild() {
        let mut scene = PaintScene::new(200, 100, Color::BLACK);
        scene.primitives.push(sample_box(0.0));
        let mut slots = DynamicSlotBuffer::default();
        slots.bind_primitive(DynamicSlotId(0), 0);
        slots.set_override(
            DynamicSlotId(0),
            DynamicSlotOverride {
                transform: Some(Affine2::translation(12.0, 4.0)),
                opacity: Some(0.5),
                background_color: Some(Color {
                    r: 200,
                    g: 50,
                    b: 10,
                    a: 255,
                }),
            },
        );
        slots.apply_to_scene(&mut scene);
        match &scene.primitives[0] {
            DrawPrimitive::StyledBox {
                transform,
                opacity,
                background_color,
                ..
            } => {
                assert_eq!(*transform, Affine2::translation(12.0, 4.0));
                assert!((*opacity - 0.5).abs() < 1e-6);
                assert_eq!(
                    *background_color,
                    Color {
                        r: 200,
                        g: 50,
                        b: 10,
                        a: 255
                    }
                );
            }
            _ => panic!("expected StyledBox"),
        }
    }
}
