//! Debug-only hot-patching & M3 dynamic-class in-place style hot-patch verification tests.

#![forbid(unsafe_code)]

#[cfg(feature = "preview")]
use krui::*;

#[cfg(feature = "preview")]
fn rect_matches(a: LayoutRect, b: LayoutRect) -> bool {
    (a.x - b.x).abs() < 1.5
        && (a.y - b.y).abs() < 1.5
        && (a.width - b.width).abs() < 1.5
        && (a.height - b.height).abs() < 1.5
}

#[cfg(feature = "preview")]
fn find_box_bg(tree: &mut ViewTree, node: NodeId, w: u32, h: u32) -> Color {
    let (scene, _) = tree.build_paint_scene(w, h);
    let target = tree.node_layout(node).expect("node layout");
    scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                border_width,
                ..
            } if rect_matches(*rect, target) && *background_color != Color::TRANSPARENT => {
                let _ = border_width;
                Some(*background_color)
            }
            _ => None,
        })
        .expect("styled box background primitive")
}

#[test]
fn hot_reload_flag_is_strictly_debug_only() {
    if cfg!(all(feature = "preview", debug_assertions)) {
        assert!(krui_winit::is_dev_hot_reload_enabled());
    } else {
        assert!(!krui_winit::is_dev_hot_reload_enabled());
    }
}

#[cfg(feature = "preview")]
#[test]
fn hot_patch_literal_and_watcher_follow_debug_only_invariant() {
    let view = krui!(
        r#"
        <div style="width: 200px; height: 80px;">
            <span id="leaf" style="width: 80px; height: 30px; background-color: #0c2238;">before</span>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let leaf = tree.find_by_id("leaf").expect("mounted #leaf");

    krui_preview::hot_patch_text_literal(&mut tree, leaf, "after");
    krui_preview::hot_patch_background_color(&mut tree, leaf, Color::rgb(90, 91, 92));

    if cfg!(debug_assertions) {
        assert_eq!(tree.node_text(leaf).as_deref(), Some("after"));
        assert_eq!(
            find_box_bg(&mut tree, leaf, 200, 80),
            Color::rgb(90, 91, 92)
        );
        assert!(krui_preview::DevFileWatcher::new(
            "examples/component_gallery/views/component_gallery.krui"
        )
        .is_some());
    } else {
        assert_eq!(tree.node_text(leaf).as_deref(), Some("before"));
        assert_eq!(
            find_box_bg(&mut tree, leaf, 200, 80),
            Color::rgb(0x0c, 0x22, 0x38)
        );
        assert!(krui_preview::DevFileWatcher::new(
            "examples/component_gallery/views/component_gallery.krui"
        )
        .is_none());
    }
}

#[cfg(feature = "preview")]
#[test]
fn dynamic_class_hot_patch_works_in_debug_and_noops_in_release() {
    use std::rc::Rc;

    let sw_cls = Signal::new("is-on".to_string());
    let mut vm = ScopeRegistry::new();
    vm.insert_signal("sw_cls", DynSignal::from_signal(sw_cls));
    let mut reg = ScopeRegistry::new();
    reg.insert_registry("vm", vm);
    let reg_ref: RegistryRef = Rc::new(reg);

    let old_doc = krui_preview::compile_krui_source(
        "<style>.sw { background: #112233; width: 48px; height: 28px; } .is-on { background: #445566; }</style>\
         <div style=\"width: 200px; height: 80px;\"><button id=\"sw\" class=\"sw {vm.sw_cls}\">toggle</button></div>",
    )
    .expect("compile old");
    let new_doc = krui_preview::compile_krui_source(
        "<style>.sw { background: #112233; width: 48px; height: 28px; } .is-on { background: #778899; }</style>\
         <div style=\"width: 200px; height: 80px;\"><button id=\"sw\" class=\"sw {vm.sw_cls}\">toggle</button></div>",
    )
    .expect("compile new");

    let mut preview_tree = ViewTree::new();
    let mounted = krui_preview::mount_compiled_document_with_registry(
        &mut preview_tree,
        &old_doc,
        reg_ref.clone(),
    );
    assert!(!mounted.is_empty());
    let sw_id = preview_tree.find_by_id("sw").expect("mounted #sw");
    assert_eq!(
        find_box_bg(&mut preview_tree, sw_id, 200, 80),
        Color::rgb(0x44, 0x55, 0x66)
    );

    let in_place = krui_preview::try_hot_patch_compiled_styles_in_place(
        &mut preview_tree,
        &old_doc,
        &new_doc,
        &reg_ref,
    )
    .expect("in-place style patch result");

    assert_dynamic_class_patch_outcome(
        &mut preview_tree,
        sw_id,
        sw_cls,
        &new_doc,
        reg_ref,
        in_place,
    );
}

#[cfg(feature = "preview")]
fn assert_dynamic_class_patch_outcome(
    preview_tree: &mut ViewTree,
    sw_id: NodeId,
    sw_cls: Signal<String>,
    new_doc: &krui_preview::CompiledDocument,
    reg_ref: RegistryRef,
    in_place: Option<usize>,
) {
    if cfg!(debug_assertions) {
        assert!(
            in_place.is_some(),
            "dynamic class nodes must hot-patch in-place in Debug mode"
        );
        assert_eq!(
            find_box_bg(preview_tree, sw_id, 200, 80),
            Color::rgb(0x77, 0x88, 0x99)
        );
        sw_cls.set(String::new());
        assert_eq!(
            find_box_bg(preview_tree, sw_id, 200, 80),
            Color::rgb(0x11, 0x22, 0x33)
        );
    } else {
        assert!(in_place.is_none());
        assert_eq!(
            find_box_bg(preview_tree, sw_id, 200, 80),
            Color::rgb(0x44, 0x55, 0x66)
        );
        let reloaded = krui_preview::try_reload_compiled_document_with_registry(
            preview_tree,
            new_doc,
            reg_ref,
        )
        .expect("release no-op reload");
        assert_eq!(reloaded, preview_tree.node_count());
        assert_eq!(
            find_box_bg(preview_tree, sw_id, 200, 80),
            Color::rgb(0x44, 0x55, 0x66)
        );
    }
}
