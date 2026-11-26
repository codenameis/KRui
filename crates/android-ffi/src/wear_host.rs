//! Wear OS host profile controller (`DisplayShape::Round`, rotary bezel spatial focus navigation,
//! Vulkan-first `wgpu` vs low-power `ANativeWindow` CPU partial mode, and ambient pause state).

use krui_core::{next_spatial_focus, FocusableCandidate, NodeId, SpatialNavDirection, ViewTree};
use krui_style::{ColorDepthBits, DamageTrackingMode, DisplayShape, RootProfileAttrs, TierProfile};

/// Wear OS rendering strategy selected according to screen complexity (scrolling vs static watchface).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WearRenderBackend {
    /// Vulkan `wgpu::Backends::PRIMARY` hardware pipeline for 60 FPS continuous list scrolling.
    VulkanWgpu,
    /// Low-power CPU `TinySkiaRenderer` partial-damage rendering into `ANativeWindow` for dials/AOD.
    CpuNativeWindowPartial,
}

/// Wear OS power / visibility state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WearDisplayMode {
    /// Interactive wake mode running animations and high-cadence input.
    Active,
    /// Ambient / always-on low-power mode pausing transitions and cutting frame updates.
    Ambient,
}

/// Configuration for [`WearHostState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WearHostConfig {
    /// Circular display diameter in physical pixels (e.g. `454` on Galaxy Watch / Pixel Watch).
    pub diameter_px: u32,
    /// Selected GPU (`VulkanWgpu`) or low-power CPU partial (`CpuNativeWindowPartial`) backend.
    pub render_backend: WearRenderBackend,
    /// Pause CSS transitions and `@keyframes` automatically when entering ambient mode.
    pub pause_animations_in_ambient: bool,
}

impl Default for WearHostConfig {
    fn default() -> Self {
        Self {
            diameter_px: 454,
            render_backend: WearRenderBackend::VulkanWgpu,
            pause_animations_in_ambient: true,
        }
    }
}

/// Stateful Wear OS smartwatch coordinator attached alongside an [`crate::AndroidSession`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WearHostState {
    config: WearHostConfig,
    display_mode: WearDisplayMode,
    focused_node: Option<NodeId>,
}

impl WearHostState {
    /// Initializes Wear OS host state and configures `tree` for round bezel layout & damage mode.
    pub fn new(tree: &mut ViewTree, config: WearHostConfig) -> Self {
        let state = Self {
            config,
            display_mode: WearDisplayMode::Active,
            focused_node: None,
        };
        state.sync_tree_profile(tree);
        state
    }

    /// Returns the currently active [`WearDisplayMode`].
    pub const fn display_mode(&self) -> WearDisplayMode {
        self.display_mode
    }

    /// Returns the active [`WearRenderBackend`] preference.
    pub const fn render_backend(&self) -> WearRenderBackend {
        self.config.render_backend
    }

    /// Switches between [`WearDisplayMode::Active`] and [`WearDisplayMode::Ambient`], pausing
    /// animations and reducing root motion preference during ambient mode.
    pub fn set_display_mode(&mut self, tree: &mut ViewTree, mode: WearDisplayMode) {
        self.display_mode = mode;
        if self.config.pause_animations_in_ambient {
            tree.set_animations_paused(mode == WearDisplayMode::Ambient);
        }
        self.sync_tree_profile(tree);
    }

    /// Routes a physical rotary bezel / crown detent tick (`+n` clockwise, `-n` counter-clockwise)
    /// into spatial focus selection over `candidates` and focuses the chosen node in `tree`.
    pub fn on_rotary_detent(
        &mut self,
        tree: &mut ViewTree,
        detent_delta: i32,
        candidates: &[FocusableCandidate],
    ) -> Option<NodeId> {
        if detent_delta == 0 || candidates.is_empty() {
            return self.focused_node;
        }
        let dir = if detent_delta > 0 {
            SpatialNavDirection::Next
        } else {
            SpatialNavDirection::Prev
        };
        let steps = detent_delta.unsigned_abs().min(32);
        let mut cur = self.focused_node;
        for _ in 0..steps {
            if let Some(next_nid) = next_spatial_focus(candidates, cur, dir) {
                cur = Some(next_nid);
                if tree.set_focus_to_node(next_nid) {
                    self.focused_node = Some(next_nid);
                }
            }
        }
        cur
    }

    fn sync_tree_profile(&self, tree: &mut ViewTree) {
        let tier = match self.config.render_backend {
            WearRenderBackend::VulkanWgpu => TierProfile::FullGpu,
            WearRenderBackend::CpuNativeWindowPartial => TierProfile::CpuKiosk,
        };
        tree.apply_root_profile_attrs(RootProfileAttrs {
            tier,
            shape: DisplayShape::Round,
            color_depth: ColorDepthBits::Bpp24OrMore,
            reduced_motion: self.display_mode == WearDisplayMode::Ambient,
        });
        let tracking = match self.config.render_backend {
            WearRenderBackend::VulkanWgpu => DamageTrackingMode::DirtyFlagOnly,
            WearRenderBackend::CpuNativeWindowPartial => DamageTrackingMode::BoundingRects {
                split_area_ratio_x10: 15,
                max_rects_per_commit: 6,
            },
        };
        tree.set_damage_tracking_mode(tracking);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use krui_core::LayoutRect;
    use krui_style::CompiledStyle;

    fn focusable_button(tree: &mut ViewTree) -> NodeId {
        tree.create_compiled_node(
            None,
            "button",
            None,
            None,
            CompiledStyle::default(),
            true,
            false,
            false,
            false,
        )
    }

    #[test]
    fn rotary_bezel_and_ambient_mode_update_focus_and_animation_state() {
        let mut tree = ViewTree::new();
        let b0 = focusable_button(&mut tree);
        let b1 = focusable_button(&mut tree);
        let candidates = [
            FocusableCandidate {
                node: b0,
                rect: LayoutRect {
                    x: 40.0,
                    y: 40.0,
                    width: 120.0,
                    height: 44.0,
                },
                order_hint: 0,
            },
            FocusableCandidate {
                node: b1,
                rect: LayoutRect {
                    x: 40.0,
                    y: 120.0,
                    width: 120.0,
                    height: 44.0,
                },
                order_hint: 1,
            },
        ];

        let mut wear = WearHostState::new(
            &mut tree,
            WearHostConfig {
                diameter_px: 454,
                render_backend: WearRenderBackend::CpuNativeWindowPartial,
                pause_animations_in_ambient: true,
            },
        );
        assert!(matches!(
            tree.damage_tracking_mode(),
            DamageTrackingMode::BoundingRects { .. }
        ));
        assert!(!tree.animations_paused());

        assert_eq!(wear.on_rotary_detent(&mut tree, 1, &candidates), Some(b0));
        assert_eq!(wear.on_rotary_detent(&mut tree, 1, &candidates), Some(b1));
        assert_eq!(wear.on_rotary_detent(&mut tree, -1, &candidates), Some(b0));

        wear.set_display_mode(&mut tree, WearDisplayMode::Ambient);
        assert!(tree.animations_paused());
        wear.set_display_mode(&mut tree, WearDisplayMode::Active);
        assert!(!tree.animations_paused());
    }
}
