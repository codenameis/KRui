//! Regression tests for Stage E5b:
//! 1. External component discovery in `icons/<Name>.krui` (including `<template name="...">` default props).
//! 2. Named `<slot name="icon">fallback</slot>` rendering fallback children when omitted and overriding when supplied.

#![forbid(unsafe_code)]

use krui::*;
use krui_compiler::{compile_krui_source, compile_krui_source_from_path, CompiledTextPart};
use krui_preview::mount_compiled_document;
use std::fs;

#[test]
fn external_icon_component_resolves_from_icons_subdir_with_template_props() {
    let dir = std::env::temp_dir().join(format!("krui_e5b_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let icons_dir = dir.join("icons");
    fs::create_dir_all(&icons_dir).expect("create icons dir");

    fs::write(
        icons_dir.join("IconAdd.krui"),
        r#"<template name="IconAdd" class="">
    <svg class="m3-icon {class}" viewBox="0 -960 960 960" fill="currentColor">
        <path d="M440-440H200v-80h240v-240h80v240h240v80H520v240h-80v-240Z"/>
    </svg>
</template>"#,
    )
    .expect("write IconAdd.krui");

    let app_path = dir.join("app.krui");
    let app_src = r#"<link rel="krui-preset" href="krui://preset/unstyled" />
<style>.m3-icon { width: 24px; height: 24px; } .extra { width: 20px; height: 20px; }</style>
<div>
    <IconAdd />
    <IconAdd class="extra" />
</div>"#;
    fs::write(&app_path, app_src).expect("write app.krui");

    let doc_res = compile_krui_source_from_path(&app_path, app_src);
    let _ = fs::remove_dir_all(&dir);
    let doc = doc_res.expect("app.krui with <IconAdd /> in icons/ must compile");
    let root = &doc.roots[0];
    assert_eq!(
        root.children.len(),
        2,
        "both <IconAdd /> instances must expand to <svg> children"
    );
    assert!(root.children[0].svg_data.is_some());
    assert!(root.children[1].svg_data.is_some());

    let mut tree = ViewTree::new();
    let _mounted = mount_compiled_document(&mut tree, &doc);
    let (scene, _) = tree.build_paint_scene(200, 120);
    let svg_widths: Vec<f32> = scene
        .primitives
        .iter()
        .filter_map(|p| match p {
            DrawPrimitive::CanvasBatch { viewport, .. } => Some(viewport.width),
            _ => None,
        })
        .collect();
    assert_eq!(svg_widths, vec![24.0, 20.0]);
}

#[test]
fn slot_fallback_renders_when_omitted_and_is_replaced_when_provided() {
    let src = r#"<link rel="krui-preset" href="krui://preset/unstyled" />
<template name="DemoBox" id="">
    <div id="{id}">
        <slot name="icon"><span class="fallback-icon">FB</span></slot>
        <slot/>
    </div>
</template>
<div>
    <DemoBox id="with-fallback"><span>Body A</span></DemoBox>
    <DemoBox id="with-override"><span slot="icon" class="custom-icon">CU</span><span>Body B</span></DemoBox>
</div>"#;

    let doc = compile_krui_source(src).expect("<slot name=\"icon\">fallback</slot> must compile");
    let root = &doc.roots[0];
    assert_eq!(root.children.len(), 2);
    let fb_box = &root.children[0];
    let ov_box = &root.children[1];
    assert_eq!(fb_box.children.len(), 2);
    assert_eq!(
        fb_box.children[0].text_parts,
        vec![CompiledTextPart::Static("FB".to_string())]
    );
    assert_eq!(ov_box.children.len(), 2);
    assert_eq!(
        ov_box.children[0].text_parts,
        vec![CompiledTextPart::Static("CU".to_string())]
    );
}
