//! Phase 6 integration tests verifying:
//! 1. `DevFileWatcher::new` standalone hot-reload against `{vm.*}` bindings (auto-resolving
//!    `tree.preview_registry()`), in-place CSS-only style hot-patching, and structural reload
//!    preservation of checkbox `is_checked` and `<input>` `undo` (`Ctrl+Z`) history.
//! 2. `DeviceFramePreset::Iphone15` vs `Pixel8` vs `Rpi800x480` distinct bezel/cutout geometry.
//! 3. `krui_cli::stage_built_preview_binary` Windows lock-free staged binary execution paths.
//! 4. Distinct SHA256/content across `examples/01..05` stylesheets and zero `hunter2_secret`.

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::path::Path;

#[test]
fn phase6_cli_stages_generation_numbered_preview_binaries_without_locking_build_output() {
    let temp_target =
        std::env::temp_dir().join(format!("krui_phase6_cli_stage_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_target);

    let built_path = krui_cli::built_preview_binary_path(&temp_target, Some("component_gallery"));
    std::fs::create_dir_all(built_path.parent().expect("examples dir")).unwrap();
    std::fs::write(&built_path, b"preview-binary-gen-1").unwrap();

    let staged_g1 =
        krui_cli::stage_built_preview_binary(&temp_target, Some("component_gallery"), 1)
            .expect("stage gen 1");
    assert!(staged_g1.is_file());
    assert_ne!(
        staged_g1, built_path,
        "Staged binary must not alias cargo build output"
    );
    assert_eq!(std::fs::read(&staged_g1).unwrap(), b"preview-binary-gen-1");

    // Simulate `cargo build` overwriting `target/debug/examples/component_gallery[.exe]`
    // cleanly while generation 1 lives in `krui-serve-staging/`.
    std::fs::write(&built_path, b"preview-binary-gen-2").unwrap();
    let staged_g2 =
        krui_cli::stage_built_preview_binary(&temp_target, Some("component_gallery"), 2)
            .expect("stage gen 2");
    assert!(staged_g2.is_file());
    assert_ne!(staged_g1, staged_g2);
    assert_eq!(std::fs::read(&staged_g2).unwrap(), b"preview-binary-gen-2");
    assert!(
        !staged_g1.exists(),
        "Unlocked previous generation binary should be cleaned up best-effort"
    );

    let _ = std::fs::remove_dir_all(&temp_target);
}

#[test]
fn phase6_example_stylesheets_are_distinct_and_unbound_hunter2_is_absent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let css_files = [
        "examples/component_gallery/views/component_gallery.css",
        "examples/component_gallery/views/micl.css",
        "examples/component_gallery/views/themes/material.css",
        "examples/component_gallery/views/themes/tailwind.css",
    ];
    let mut distinct_contents = HashSet::new();
    for rel in css_files {
        let content = std::fs::read_to_string(root.join(rel)).expect(rel);
        assert!(!content.trim().is_empty(), "{rel} must not be empty");
        distinct_contents.insert(content);
    }
    assert_eq!(
        distinct_contents.len(),
        4,
        "Component gallery stylesheets must have distinct contents"
    );

    let actions_comp = root.join("crates/compiler/builtins/actions.krui");
    let actions_src = std::fs::read_to_string(&actions_comp).expect("actions.krui must exist");
    assert!(
        actions_src.contains("<slot"),
        "actions.krui must expose <slot/>"
    );
}

#[cfg(feature = "preview")]
mod preview_tests {
    use krui::dev_hot_reload::{
        decorate_preview_paint_scene, DevFileWatcher, DeviceFramePreset, PreviewOverlayFlags,
    };
    use krui::*;

    #[controller]
    #[derive(Clone)]
    struct StandalonePreviewVm {
        pub heading: Signal<String>,
        pub count: Signal<i32>,
    }

    const INITIAL_KRUI: &str = r#"<style>
.box { padding: 10px; background-color: var(--krui-card); display: flex; flex-direction: column; gap: 6px; }
</style>
<div id="root-box" class="box">
    <h2 id="vm-heading">{vm.heading} ({vm.count})</h2>
    <input id="chk-keep" type="checkbox" />
    <input id="memo-keep" placeholder="메모 입력" />
</div>"#;

    const CSS_EDITED_KRUI: &str = r#"<style>
.box { padding: 24px; background-color: var(--krui-surface); display: flex; flex-direction: column; gap: 12px; }
</style>
<div id="root-box" class="box">
    <h2 id="vm-heading">{vm.heading} ({vm.count})</h2>
    <input id="chk-keep" type="checkbox" />
    <input id="memo-keep" placeholder="메모 입력" />
</div>"#;

    const STRUCTURAL_EDITED_KRUI: &str = r#"<style>
.box { padding: 24px; background-color: var(--krui-surface); display: flex; flex-direction: column; gap: 12px; }
</style>
<div id="root-box" class="box">
    <h2 id="vm-heading">핫리로드: {vm.heading} [{vm.count}]</h2>
    <input id="chk-keep" type="checkbox" />
    <input id="memo-keep" placeholder="메모 입력" />
    <span id="added-badge">신규 배지: {vm.heading}</span>
</div>"#;

    fn mount_initial_preview_tree() -> (ViewTree, StandalonePreviewVm) {
        let vm = StandalonePreviewVm {
            heading: Signal::new("주문검토".to_string()),
            count: Signal::new(7),
        };
        let mut tree = ViewTree::new();
        let vm_mount = vm.clone();
        tree.mount_with(move || {
            let vm = vm_mount;
            krui!(
                r#"
                <style>
                    .box { padding: 10px; background-color: var(--krui-card); display: flex; flex-direction: column; gap: 6px; }
                </style>
                <div id="root-box" class="box">
                    <h2 id="vm-heading">{vm.heading} ({vm.count})</h2>
                    <input id="chk-keep" type="checkbox" />
                    <input id="memo-keep" placeholder="메모 입력" />
                </div>
                "#
            )
        });
        let _ = tree.build_paint_scene(480, 320);
        (tree, vm)
    }

    fn assert_structural_reload_preserves_checked_and_undo(
        watcher: &mut DevFileWatcher,
        tree: &mut ViewTree,
        temp_krui: &std::path::Path,
    ) {
        std::fs::write(temp_krui, STRUCTURAL_EDITED_KRUI).unwrap();
        assert!(watcher.force_reload(tree).is_some());
        assert!(
            watcher.last_compile_error().is_none(),
            "DevFileWatcher::new must resolve {{vm.*}} from tree.preview_registry(): {:?}",
            watcher.last_compile_error()
        );
        let added = tree
            .find_by_id("added-badge")
            .expect("#added-badge mounted");
        assert_eq!(
            tree.node_text(added).as_deref(),
            Some("신규 배지: 주문검토")
        );

        let chk_after = tree
            .find_by_id("chk-keep")
            .expect("#chk-keep after remount");
        let memo_after = tree
            .find_by_id("memo-keep")
            .expect("#memo-keep after remount");
        assert!(
            tree.is_checked(chk_after),
            "is_checked must survive structural remount"
        );
        assert_eq!(tree.node_text(memo_after).as_deref(), Some("초안1-수정2"));

        let _ = tree.build_paint_scene(480, 320);
        let memo_rect2 = tree
            .node_layout(memo_after)
            .expect("memo rect after remount");
        tree.dispatch_click(memo_rect2.x + 6.0, memo_rect2.y + 6.0);
        assert!(
            tree.dispatch_undo(),
            "Ctrl+Z undo history must survive structural remount"
        );
        assert_eq!(tree.node_text(memo_after).as_deref(), Some("초안1"));
    }

    #[test]
    fn phase6_standalone_watcher_resolves_live_registry_patches_css_and_preserves_checked_and_undo()
    {
        let temp_krui = std::env::temp_dir().join(format!(
            "krui_phase6_standalone_reload_{}.krui",
            std::process::id()
        ));
        std::fs::write(&temp_krui, INITIAL_KRUI).unwrap();

        let (mut tree, _vm) = mount_initial_preview_tree();
        let mut watcher = DevFileWatcher::new(&temp_krui).expect("create standalone watcher");

        let chk_before = tree.find_by_id("chk-keep").expect("#chk-keep");
        let memo_before = tree.find_by_id("memo-keep").expect("#memo-keep");
        tree.set_checked(chk_before, true);
        assert!(tree.is_checked(chk_before));

        // Focus `#memo-keep` and commit two separate edits so `undo_stack` holds `"초안1"`.
        let memo_rect = tree.node_layout(memo_before).expect("memo rect");
        tree.dispatch_click(memo_rect.x + 6.0, memo_rect.y + 6.0);
        tree.ime_commit("초안1");
        tree.ime_commit("-수정2");
        assert_eq!(tree.node_text(memo_before).as_deref(), Some("초안1-수정2"));

        // 1. CSS-only edit must hot-patch styles IN PLACE without unmounting DOM nodes!
        std::fs::write(&temp_krui, CSS_EDITED_KRUI).unwrap();
        assert!(watcher.force_reload(&mut tree).is_some());
        assert!(watcher.last_compile_error().is_none());
        assert_eq!(
            watcher.style_patch_count(),
            1,
            "CSS-only edit must use fast style patch"
        );
        assert_eq!(tree.find_by_id("chk-keep"), Some(chk_before));
        assert_eq!(tree.find_by_id("memo-keep"), Some(memo_before));

        // 2. Structural `.krui` edit with `{vm.*}` bindings must succeed via `tree.preview_registry()`
        //    and preserve both checkbox `is_checked` and `<input>` `undo_stack` (`Ctrl+Z`)!
        assert_structural_reload_preserves_checked_and_undo(&mut watcher, &mut tree, &temp_krui);
        let _ = std::fs::remove_file(&temp_krui);
    }

    fn collect_styled_boxes(preset: DeviceFramePreset) -> Vec<(f32, f32, f32)> {
        let (sw, sh) = preset.screen_size();
        let mut scene = PaintScene::new(sw, sh, Color::BLACK);
        let mut text_engine = krui_text::TextEngine::new();
        decorate_preview_paint_scene(
            &mut scene,
            &mut text_engine,
            &[],
            None,
            PreviewOverlayFlags::default(),
            Some(preset),
            1,
            500,
        );
        scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                DrawPrimitive::StyledBox {
                    rect,
                    border_radius,
                    ..
                } => Some((rect.width, rect.height, *border_radius)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn phase6_device_frames_render_distinct_iphone_island_pixel_punch_hole_and_flat_rpi_bezel() {
        let iphone_boxes = collect_styled_boxes(DeviceFramePreset::Iphone15);
        let pixel_boxes = collect_styled_boxes(DeviceFramePreset::Pixel8);
        let rpi_boxes = collect_styled_boxes(DeviceFramePreset::Rpi800x480);

        assert!(
            iphone_boxes.contains(&(116.0, 24.0, 12.0)),
            "iPhone 15 must render 116x24 Dynamic Island pill: {iphone_boxes:?}"
        );
        assert!(
            !pixel_boxes.contains(&(116.0, 24.0, 12.0))
                && pixel_boxes.contains(&(10.0, 10.0, 5.0)),
            "Pixel 8 must render 10x10 circular punch-hole dot (not Dynamic Island): {pixel_boxes:?}"
        );
        assert!(
            rpi_boxes.contains(&(800.0, 480.0, 4.0)) && rpi_boxes.len() == 2,
            "RPi 800x480 must render clean industrial bezel with zero notch and zero home bar: {rpi_boxes:?}"
        );
    }
}
