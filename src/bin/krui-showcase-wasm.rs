//! Platform-neutral WebAssembly showcase entry point for KRui (`krui-showcase-wasm`).
//!
//! Mounts the shared pure-CSS Component Gallery (`examples/component_gallery`) directly on top of
//! `krui-core` + `krui-renderer`'s `TinySkiaRenderer`, without pulling in `krui-winit`, system tray,
//! clipboard, or `tokio` runtime dependencies.

#![forbid(unsafe_code)]

#[allow(dead_code)]
#[path = "../../examples/component_gallery/controller/mod.rs"]
mod controller;

#[cfg(target_arch = "wasm32")]
#[path = "wasm_showcase/browser_host.rs"]
mod wasm_browser_host;

use controller::GalleryController;
use krui::*;

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 840;
const FRAME_DT_SECONDS: f32 = 1.0 / 60.0;

/// Self-contained CPU-rendered showcase session suitable for `wasm32-unknown-unknown` and browser hosts.
pub struct WasmShowcaseHost {
    reactive_session: ReactiveSession,
    clock: FrameClock,
    tree: ViewTree,
    renderer: TinySkiaRenderer,
    controller: GalleryController,
    width: u32,
    height: u32,
    applied_scheme_mode: i32,
    applied_anim_paused: bool,
    rendered_first_frame: bool,
}

impl WasmShowcaseHost {
    /// Initializes the gallery reactive session, `ViewTree`, and `TinySkiaRenderer` at `1280x840`.
    pub fn new() -> Self {
        let reactive_session = ReactiveSession::new();
        let (clock, tree, controller, applied_scheme_mode, applied_anim_paused) = reactive_session
            .enter(|| {
                let clock = FrameClock::new();
                let mut tree = ViewTree::new();
                let controller = GalleryController::new();
                let ctrl_for_view = controller.clone();
                tree.mount_with(move || {
                    let controller = ctrl_for_view;
                    include_krui!("examples/component_gallery/views/component_gallery.krui")
                });
                tree.set_color_scheme_preference(controller.current_color_scheme_pref());
                let scheme_mode = controller.color_scheme_mode.get_untracked();
                let anim_paused = controller.animations_paused.get_untracked();
                (clock, tree, controller, scheme_mode, anim_paused)
            });
        let renderer =
            TinySkiaRenderer::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, GpuResourceLimits::default());
        Self {
            reactive_session,
            clock,
            tree,
            renderer,
            controller,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            applied_scheme_mode,
            applied_anim_paused,
            rendered_first_frame: false,
        }
    }

    /// Updates the logical viewport size in pixels.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    /// Updates the HiDPI display scale factor applied to layout and rasterization.
    pub fn set_scale_factor(&mut self, scale_factor: f32) {
        self.tree.set_scale_factor(scale_factor);
    }

    /// Returns a reference to the shared gallery MVC controller.
    pub fn controller(&self) -> &GalleryController {
        &self.controller
    }

    /// Returns `true` when reactive state, transitions, animations, or initial paint require a frame.
    pub fn needs_frame(&mut self) -> bool {
        let controller = &self.controller;
        let scheme_dirty = self.applied_scheme_mode != controller.color_scheme_mode.get_untracked();
        let anim_dirty = self.applied_anim_paused != controller.animations_paused.get_untracked();
        let active_clock = self
            .reactive_session
            .enter(|| self.clock.has_active_animations());
        !self.rendered_first_frame
            || scheme_dirty
            || anim_dirty
            || active_clock
            || self.tree.has_active_transitions()
            || self.tree.is_paint_dirty()
    }

    /// Dispatches a pointer move event in logical CSS pixels.
    pub fn dispatch_pointer_move(&mut self, x: f32, y: f32, mods: Mods) -> bool {
        let tree = &mut self.tree;
        self.reactive_session
            .enter(|| tree.dispatch_pointer_move_with_mods(x, y, mods))
    }

    /// Dispatches a mouse button transition in logical CSS pixels.
    pub fn dispatch_pointer_button(
        &mut self,
        x: f32,
        y: f32,
        button: PointerButton,
        pressed: bool,
        mods: Mods,
        timestamp_ms: u64,
    ) -> bool {
        let tree = &mut self.tree;
        self.reactive_session
            .enter(|| tree.dispatch_pointer_button(x, y, button, pressed, mods, timestamp_ms))
    }

    /// Dispatches pointer leave when the cursor exits the canvas.
    pub fn dispatch_pointer_leave(&mut self) -> bool {
        let tree = &mut self.tree;
        self.reactive_session
            .enter(|| tree.dispatch_pointer_leave())
    }

    /// Dispatches a scroll wheel event at `(x, y)` in logical CSS pixels.
    pub fn dispatch_wheel(
        &mut self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
        mods: Mods,
    ) -> bool {
        let tree = &mut self.tree;
        self.reactive_session
            .enter(|| tree.dispatch_wheel(x, y, delta_x, delta_y, mods))
    }

    /// Dispatches a DOM `KeyboardEvent.key` string with modifiers into the showcase and `ViewTree`.
    pub fn dispatch_key(&mut self, key: &str, mods: Mods) -> bool {
        let tree = &mut self.tree;
        let controller = &self.controller;
        self.reactive_session.enter(|| {
            if key == "Escape" {
                if tree.active_select_popup().is_some() {
                    return tree.dispatch_key_escape();
                }
                return controller.dismiss_top_overlay() || tree.dispatch_key_escape();
            }
            if key == "F5" || key == "F6" {
                controller.select_theme(if key == "F5" { 0 } else { 1 });
                return true;
            }
            match key {
                "Tab" => tree.dispatch_key_tab(mods.shift).is_some(),
                "Enter" => tree.dispatch_key_enter(),
                " " => tree.dispatch_key_space() || tree.dispatch_key_text(" "),
                k if !mods.ctrl && !mods.alt && !mods.super_key && k.chars().count() == 1 => {
                    tree.dispatch_key_text(k)
                }
                k => tree.dispatch_key_down(k),
            }
        })
    }

    /// Advances animations, transitions, layout, and `TinySkiaRenderer` rasterization by `dt_seconds`.
    pub fn step_frame(&mut self, dt_seconds: f32) -> FrameRenderStats {
        let controller = &self.controller;
        let tree = &mut self.tree;
        let clock = &mut self.clock;
        let renderer = &mut self.renderer;
        let (width, height) = (self.width, self.height);
        let applied_scheme = &mut self.applied_scheme_mode;
        let applied_paused = &mut self.applied_anim_paused;
        let first = &mut self.rendered_first_frame;

        self.reactive_session.enter(|| {
            let target_scheme = controller.color_scheme_mode.get_untracked();
            if target_scheme != *applied_scheme {
                *applied_scheme = target_scheme;
                tree.set_color_scheme_preference(controller.current_color_scheme_pref());
            }
            let paused = controller.animations_paused.get_untracked();
            if paused != *applied_paused {
                *applied_paused = paused;
                tree.set_animations_paused(paused);
            }
            let _ = clock.tick(dt_seconds);
            let _ = tree.sync_viewport_and_media(width as f32, height as f32);
            let _ = tree.tick_transitions(dt_seconds);
            let (scene, _layout_ran) = tree.build_paint_scene(width, height);
            *first = true;
            renderer.render_scene(&scene)
        })
    }

    /// Returns the latest rasterized premultiplied RGBA8 framebuffer slice and pixel dimensions.
    pub fn framebuffer_rgba_and_size(&mut self) -> Option<(&[u8], u32, u32)> {
        let (pw, ph) = self.renderer.framebuffer_dimensions()?;
        let rgba = self.renderer.framebuffer_rgba()?;
        Some((rgba, pw, ph))
    }

    /// Returns the latest rasterized RGBA8 framebuffer slice when available.
    pub fn framebuffer_rgba(&mut self) -> Option<&[u8]> {
        self.renderer.framebuffer_rgba()
    }
}

impl Default for WasmShowcaseHost {
    fn default() -> Self {
        Self::new()
    }
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    wasm_browser_host::launch_browser_showcase();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut host = WasmShowcaseHost::new();
        let _ = host.step_frame(FRAME_DT_SECONDS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_showcase_mounts_renders_and_switches_pure_css_themes_with_zero_relayout() {
        let mut host = WasmShowcaseHost::new();
        assert!(host.needs_frame(), "initial frame must be dirty");
        let stats = host.step_frame(FRAME_DT_SECONDS);
        assert!(
            stats.draw_primitives_submitted > 0,
            "initial showcase frame must submit primitives"
        );
        let fb_len = host
            .framebuffer_rgba()
            .expect("framebuffer must exist after step_frame")
            .len();
        assert_eq!(fb_len, (DEFAULT_WIDTH * DEFAULT_HEIGHT * 4) as usize);

        host.reactive_session.enter(|| {
            host.controller().select_doc_page(3);
            host.controller().set_color_scheme_mode(1);
        });
        let _ = host.step_frame(FRAME_DT_SECONDS);
        let _ = host.step_frame(0.5);
        let relayout_light = host.tree.relayout_passes();
        host.reactive_session.enter(|| {
            host.controller().select_theme(1);
        });
        let _ = host.step_frame(FRAME_DT_SECONDS);
        let _ = host.step_frame(0.5);
        assert_eq!(
            host.tree.relayout_passes() - relayout_light,
            0,
            "Material -> Tailwind pure-CSS theme switch must keep 0 Taffy relayouts"
        );

        host.reactive_session.enter(|| {
            host.controller().set_color_scheme_mode(2);
        });
        let _ = host.step_frame(FRAME_DT_SECONDS);
        let _ = host.step_frame(0.5);
        let relayout_dark = host.tree.relayout_passes();
        host.reactive_session.enter(|| {
            host.controller().select_theme(0);
        });
        let _ = host.step_frame(FRAME_DT_SECONDS);
        let _ = host.step_frame(0.5);
        assert_eq!(
            host.tree.relayout_passes() - relayout_dark,
            0,
            "Tailwind -> Material pure-CSS theme switch in dark mode must keep 0 Taffy relayouts"
        );
    }
}
