//! Cross-platform WebAssembly showcase entry point (`krui-showcase-wasm`).
//!
//! - Under `--features web-dom` (recommended browser runtime): mounts `component_gallery.krui`
//!   directly onto live Browser DOM + Native CSS + lazy `<canvas>` 2D/3D islands.
//! - Under default `native-engine` (workspace tests/fallback): preserves headless `TinySkiaRenderer`
//!   + `ViewTree` integration.

#![forbid(unsafe_code)]

#[allow(dead_code)]
#[path = "../../examples/component_gallery/controller/mod.rs"]
mod controller;

#[cfg(all(target_arch = "wasm32", feature = "native-engine"))]
#[path = "wasm_showcase/browser_host.rs"]
mod wasm_browser_host;

#[cfg(all(target_arch = "wasm32", feature = "web-dom"))]
#[path = "wasm_showcase/web_dom_host.rs"]
mod web_dom_host;

#[cfg(all(target_arch = "wasm32", feature = "web-gpu-3d"))]
#[path = "wasm_showcase/web_viewport3d.rs"]
mod web_viewport3d;

#[cfg(feature = "native-engine")]
#[path = "wasm_showcase/native_host.rs"]
mod native_impl;

#[cfg(feature = "web-dom")]
mod web_dom_impl {
    use super::controller::GalleryController;
    use krui::*;

    /// Native Browser DOM showcase host mounting `component_gallery.krui` into `#krui-root`.
    pub struct WasmShowcaseHost {
        reactive_session: ReactiveSession,
        clock: FrameClock,
        tree: ViewTree,
        controller: GalleryController,
    }

    impl Default for WasmShowcaseHost {
        fn default() -> Self {
            Self::new()
        }
    }

    impl WasmShowcaseHost {
        /// Mounts `component_gallery.krui` and connects reactive color-scheme, motion, and 3D hooks.
        pub fn new() -> Self {
            let reactive_session = ReactiveSession::new();
            let (clock, tree, controller) = reactive_session.enter(|| {
                let clock = FrameClock::new();
                let mut tree = ViewTree::new();
                let controller = GalleryController::new();
                #[cfg(all(target_arch = "wasm32", feature = "web-gpu-3d"))]
                super::web_viewport3d::register(&mut tree, &controller);
                let ctrl_for_view = controller.clone();
                tree.mount_with(move || {
                    let controller = ctrl_for_view;
                    include_krui!("examples/component_gallery/views/component_gallery.krui")
                });
                let tree_scheme = tree.clone();
                let ctrl_scheme = controller.clone();
                let _ = Effect::new(move || {
                    let _ = ctrl_scheme.color_scheme_mode.get();
                    tree_scheme
                        .clone()
                        .set_color_scheme_preference(ctrl_scheme.current_color_scheme_pref());
                });
                let tree_pause = tree.clone();
                let ctrl_pause = controller.clone();
                let _ = Effect::new(move || {
                    tree_pause
                        .clone()
                        .set_animations_paused(ctrl_pause.animations_paused.get());
                });
                (clock, tree, controller)
            });
            Self {
                reactive_session,
                clock,
                tree,
                controller,
            }
        }

        /// Returns a reference to [`ViewTree`] (`WebDomTree`).
        pub fn tree(&self) -> &ViewTree {
            &self.tree
        }

        /// Returns shared [`GalleryController`].
        pub fn controller(&self) -> &GalleryController {
            &self.controller
        }

        /// Executes `f` within this host's isolated [`ReactiveSession`].
        pub fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
            self.reactive_session.enter(f)
        }

        /// Returns `true` when reactive springs or 3D turntable auto-spin require animation frames.
        pub fn needs_spin_tick(&self) -> bool {
            self.reactive_session.enter(|| {
                self.clock.has_active_animations()
                    || (self.controller.active_section.get_untracked() == 4
                        && self.controller.w4.studio.develop_subroute.get_untracked() == 2
                        && self.controller.w4.studio.auto_spin.get_untracked())
            })
        }

        /// Advances reactive frame clocks and optional 3D auto-spin by `dt_secs`.
        pub fn tick(&mut self, dt_secs: f32) -> bool {
            self.reactive_session.enter(|| {
                self.clock.tick(dt_secs);
                if self.controller.active_section.get_untracked() == 4
                    && self.controller.w4.studio.develop_subroute.get_untracked() == 2
                {
                    self.controller.w4.studio.tick_auto_spin(dt_secs);
                }
            });
            self.needs_spin_tick()
        }
    }
}

#[cfg(feature = "native-engine")]
pub use native_impl::{WasmShowcaseHost, DEFAULT_HEIGHT, DEFAULT_WIDTH, FRAME_DT_SECONDS};
#[cfg(feature = "web-dom")]
pub use web_dom_impl::WasmShowcaseHost;

fn main() {
    #[cfg(all(target_arch = "wasm32", feature = "web-dom"))]
    web_dom_host::run_browser_showcase();
    #[cfg(all(target_arch = "wasm32", feature = "native-engine"))]
    wasm_browser_host::launch_browser_showcase();
}

#[cfg(test)]
mod tests {
    use super::WasmShowcaseHost;
    #[cfg(feature = "native-engine")]
    use super::{DEFAULT_HEIGHT, DEFAULT_WIDTH, FRAME_DT_SECONDS};
    #[cfg(feature = "web-dom")]
    use krui::{PointerButton, PointerEvent};

    #[cfg(feature = "native-engine")]
    #[test]
    fn wasm_showcase_mounts_renders_and_switches_pure_css_themes_with_zero_relayout() {
        let mut host = WasmShowcaseHost::new();
        assert!(host.needs_frame(), "initial frame must be dirty");
        let stats = host.step_frame(FRAME_DT_SECONDS);
        assert!(stats.draw_primitives_submitted > 0);
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
        host.reactive_session
            .enter(|| host.controller().select_theme(1));
        let _ = host.step_frame(FRAME_DT_SECONDS);
        let _ = host.step_frame(0.5);
        assert_eq!(host.tree.relayout_passes() - relayout_light, 0);

        host.reactive_session
            .enter(|| host.controller().set_color_scheme_mode(2));
        let _ = host.step_frame(FRAME_DT_SECONDS);
        let _ = host.step_frame(0.5);
        let relayout_dark = host.tree.relayout_passes();
        host.reactive_session
            .enter(|| host.controller().select_theme(0));
        let _ = host.step_frame(FRAME_DT_SECONDS);
        let _ = host.step_frame(0.5);
        assert_eq!(host.tree.relayout_passes() - relayout_dark, 0);
    }

    #[cfg(feature = "web-dom")]
    #[test]
    fn wasm_showcase_web_dom_canvas_and_3d_spin() {
        let mut host = WasmShowcaseHost::new();
        let mock_roots = host.tree().with_backend_ref(|b| {
            b.as_any()
                .unwrap()
                .downcast_ref::<krui::MockDomBackend>()
                .unwrap()
                .roots()
                .len()
        });
        assert!(mock_roots > 0);
        dispatch_mock_click(
            &host,
            find_mock_id(&host, "nav-sec-develop").expect("#nav-sec-develop"),
        );
        host.enter(|| assert_eq!(host.controller().active_section.get_untracked(), 4));

        dispatch_mock_click(&host, find_mock_id(&host, "dev-sub-1").expect("#dev-sub-1"));
        let add_btn = find_mock_id(&host, "canvas-add-rect").expect("#canvas-add-rect");
        let canvas_id = find_mock_id(&host, "studio-2d-canvas").expect("#studio-2d-canvas");
        dispatch_mock_click(&host, add_btn);
        let cmds_after_add = host.tree().with_backend_ref(|b| {
            b.as_any()
                .unwrap()
                .downcast_ref::<krui::MockDomBackend>()
                .unwrap()
                .node(canvas_id)
                .unwrap()
                .canvas_commands
                .len()
        });
        dispatch_canvas_drag(&host, canvas_id);
        assert!(cmds_after_add > 0);

        dispatch_mock_click(&host, find_mock_id(&host, "dev-sub-2").expect("#dev-sub-2"));
        host.enter(|| {
            assert_eq!(
                host.controller().w4.studio.develop_subroute.get_untracked(),
                2
            );
            assert!(host.needs_spin_tick());
        });
        let yaw_before = host.enter(|| host.controller().w4.studio.orbit_yaw.get_untracked());
        assert!(host.tick(0.05));
        let yaw_after = host.enter(|| host.controller().w4.studio.orbit_yaw.get_untracked());
        assert!(
            yaw_after > yaw_before,
            "auto-spin tick must advance orbit_yaw"
        );
    }

    #[cfg(feature = "web-dom")]
    #[test]
    fn wasm_showcase_web_dom_nested_if_unmount_and_text_inputs() {
        let host = WasmShowcaseHost::new();
        dispatch_mock_click(
            &host,
            find_mock_id(&host, "nav-sec-components").expect("#nav-sec-components"),
        );
        assert!(find_mock_id(&host, "ov-card-dialogs").is_some());
        host.enter(|| host.controller().select_doc_page(22));
        assert!(
            find_mock_id(&host, "ov-card-dialogs").is_none(),
            "Nested <if> overview card must unmount when switching to Text fields"
        );
        let tf_filled = find_mock_id(&host, "tf-name-filled").expect("#tf-name-filled");
        let tf_outlined = find_mock_id(&host, "tf-name-outlined").expect("#tf-name-outlined");
        let (outlined_input, input_cbs) = host.tree().with_backend_ref(|b| {
            let mock = b
                .as_any()
                .unwrap()
                .downcast_ref::<krui::MockDomBackend>()
                .unwrap();
            let fi =
                find_descendant_by_tag(mock, tf_filled, "input").expect("#tf-name-filled input");
            let oi = find_descendant_by_tag(mock, tf_outlined, "input")
                .expect("#tf-name-outlined input");
            (oi, mock.input_handlers(fi))
        });
        assert!(
            !input_cbs.is_empty(),
            "#tf-name-filled input must have oninput handler"
        );
        for cb in &input_cbs {
            cb("한글".to_string());
            cb("한글 입력 테스트".to_string());
        }
        host.enter(|| {
            assert_eq!(
                host.controller().user_name_text.get_untracked(),
                "한글 입력 테스트"
            );
            assert_eq!(host.controller().name_counter.get_untracked(), "9 / 32자");
        });
        host.tree().with_backend_ref(|b| {
            let mock = b
                .as_any()
                .unwrap()
                .downcast_ref::<krui::MockDomBackend>()
                .unwrap();
            assert_eq!(mock.node(outlined_input).unwrap().value, "한글 입력 테스트");
        });
        dispatch_mock_click(
            &host,
            find_mock_id(&host, "nav-sec-develop").expect("#nav-sec-develop"),
        );
        assert!(find_mock_id(&host, "tf-name-filled").is_none());
    }

    #[cfg(feature = "web-dom")]
    fn dispatch_canvas_drag(host: &WasmShowcaseHost, canvas_id: krui::NodeId) {
        let fire = |kind: krui_web::DomEventKind, ev: PointerEvent| {
            host.tree()
                .with_backend_ref(|b| {
                    b.as_any()
                        .unwrap()
                        .downcast_ref::<krui::MockDomBackend>()
                        .unwrap()
                        .pointer_handlers(canvas_id, kind)
                })
                .into_iter()
                .for_each(|cb| cb(ev));
        };
        let drag_pt = |x, y| PointerEvent {
            local_x: x,
            local_y: y,
            button: PointerButton::Primary,
            buttons: 1,
            ..PointerEvent::default()
        };
        fire(krui_web::DomEventKind::PointerDown, drag_pt(220.0, 180.0));
        fire(krui_web::DomEventKind::PointerMove, drag_pt(260.0, 210.0));
        fire(krui_web::DomEventKind::PointerUp, PointerEvent::default());
    }

    #[cfg(feature = "web-dom")]
    fn find_mock_id(host: &WasmShowcaseHost, html_id: &str) -> Option<krui::NodeId> {
        host.tree().with_backend_ref(|b| {
            b.as_any()
                .and_then(|a| a.downcast_ref::<krui::MockDomBackend>())
                .and_then(|m| m.find_by_html_id(html_id))
        })
    }

    #[cfg(feature = "web-dom")]
    fn dispatch_mock_click(host: &WasmShowcaseHost, id: krui::NodeId) {
        let handlers = host.tree().with_backend_ref(|b| {
            b.as_any()
                .and_then(|a| a.downcast_ref::<krui::MockDomBackend>())
                .unwrap()
                .click_handlers(id)
        });
        for cb in handlers {
            cb();
        }
    }

    #[cfg(feature = "web-dom")]
    fn find_descendant_by_tag(
        mock: &krui::MockDomBackend,
        root: krui::NodeId,
        tag: &str,
    ) -> Option<krui::NodeId> {
        let node = mock.node(root)?;
        if node.tag == tag {
            return Some(root);
        }
        for &c in &node.children {
            if let Some(found) = find_descendant_by_tag(mock, c, tag) {
                return Some(found);
            }
        }
        None
    }
}
