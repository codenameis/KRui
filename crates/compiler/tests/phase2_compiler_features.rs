#![forbid(unsafe_code)]

use krui_compiler::{
    compile_krui_source, compile_krui_source_from_path, CompiledTextPart, HtmlTag,
};
use krui_style::{Color, FontFamilyStack};

const EXPR_TAG_BOUNDARY_SRC: &str = r#"
<div>
    <p id="lt-check">{a < b}</p>
    <if test="{count < 10 && ratio > 0.5}">
        <button id="next-btn" title="Next >">계속</button>
    </if>
</div>
"#;

const INLINE_FLOW_SRC: &str = r#"
<div>
    <p id="single-p">환영합니다, {user_name}님</p>
    <p id="mixed-p">안녕 <span class="hl">{user_name}</span>님 <b>굵게</b> <strong>강조</strong> <em>기울임</em> <code>코드</code></p>
    <p id="rich-p">앞부분 <b>굵게</b> <span style="color: #dc2626">빨강</span> <code>코드</code></p>
</div>
"#;

const INLINE_TEMPLATE_COMPONENT_SRC: &str = r#"
<template name="Card">
    <style>
        .card-shell { padding: 18px; border-radius: 10px; }
    </style>
    <section class="card-shell">
        <h2>{title}</h2>
        <div class="card-body"><slot/></div>
    </section>
</template>
<div>
    <Card title="{vm.heading}">
        <p id="slotted-p">내부 콘텐츠</p>
    </Card>
</div>
"#;

#[test]
fn rejects_meta_tag_with_compile_error_and_line_column() {
    let src = "<div>\n  <meta charset=\"utf-8\" />\n</div>";
    let err = compile_krui_source(src).expect_err("<meta> must be rejected");
    assert!(err.message.contains("Unsupported HTML element `<meta>`"));
    assert_eq!(err.line, 2);
    assert_eq!(err.column, 3);
}

#[test]
fn parses_angle_brackets_inside_expressions_and_quoted_attributes() {
    let doc = compile_krui_source(EXPR_TAG_BOUNDARY_SRC).expect("should parse angle brackets");
    let root = &doc.roots[0];
    assert_eq!(root.children.len(), 2);
    assert_eq!(root.children[0].id.as_deref(), Some("lt-check"));
    assert!(matches!(
        root.children[0].text_parts.as_slice(),
        [CompiledTextPart::Dynamic { .. }]
    ));
}

#[test]
fn inline_formatting_preserves_spaces_and_collapses_eligible_rich_paragraphs() {
    let doc = compile_krui_source(INLINE_FLOW_SRC).expect("should compile inline flow");
    let single_p = &doc.roots[0].children[0];
    let mixed_p = &doc.roots[0].children[1];
    let rich_p = &doc.roots[0].children[2];

    assert!(single_p.children.is_empty());
    assert_eq!(single_p.text_parts.len(), 3);

    assert_eq!(
        mixed_p.children[0].text_parts,
        vec![CompiledTextPart::Static("안녕 ".to_string())]
    );
    assert!(mixed_p.children[0].is_synthetic_text_run);
    assert_eq!(mixed_p.children[3].tag, HtmlTag::B);

    assert!(rich_p.children.is_empty());
    assert_eq!(rich_p.rich_spans.len(), 6);
    assert_eq!(rich_p.rich_spans[1].font_weight, 700);
    assert_eq!(
        rich_p.rich_spans[3].color,
        Some(Color::rgb(0xdc, 0x26, 0x26))
    );
    assert_eq!(
        rich_p.rich_spans[5].font_family,
        FontFamilyStack::CosmicMono
    );
}

#[test]
fn expands_inline_and_external_components_with_layer_scope_and_cycle_detection() {
    let doc = compile_krui_source(INLINE_TEMPLATE_COMPONENT_SRC).expect("inline component");
    let section = &doc.roots[0].children[0];
    assert_eq!(section.tag, HtmlTag::Section);
    assert_eq!(section.classes, vec!["card-shell".to_string()]);
    assert_eq!(
        section.children[1].children[0].id.as_deref(),
        Some("slotted-p")
    );

    let tmp_dir = std::env::temp_dir().join(format!("krui_comp_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(tmp_dir.join("components")).expect("mkdir");
    let badge_path = tmp_dir.join("components").join("Badge.krui");
    std::fs::write(
        &badge_path,
        "<style>.badge { border-radius: 99px; }</style><span class=\"badge\">{label}: <slot/></span>",
    )
    .expect("write Badge.krui");
    let page_path = tmp_dir.join("page.krui");
    let page_src = "<style>.badge { color: #123456; }</style><div><Badge label=\"상태\"><b>정상</b></Badge></div>";
    let ext_doc = compile_krui_source_from_path(&page_path, page_src).expect("external Badge.krui");
    assert_eq!(ext_doc.external_component_paths.len(), 1);
    let badge = &ext_doc.roots[0].children[0];
    assert!(badge
        .static_attrs
        .iter()
        .any(|(k, v)| k == "data-krui-scope" && v == "Badge"));
    assert!(ext_doc.stylesheets[0].contains("@layer component"));
    assert!(ext_doc.stylesheets[0].contains(":where([data-krui-scope~=\"Badge\"])"));

    let cycle_src = "<template name=\"Loop\"><Loop/></template><div><Loop/></div>";
    let err = compile_krui_source(cycle_src).expect_err("recursive cycle must fail");
    assert!(err.message.contains("Recursive component cycle"));
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn preserves_dynamic_classes_and_mixed_inline_styles_for_stylo() {
    let src = r#"
        <style>.c0 { padding: 1px; } .c1 { padding: 2px; }</style>
        <div class="base {dyn_cls}" style="padding: 8px; color: {fg_hex}; opacity: {alpha}">Hi</div>
    "#;
    let doc = compile_krui_source(src).expect("dynamic classes + inline styles");
    let root = &doc.roots[0];
    assert_eq!(root.classes, vec!["base".to_string()]);
    assert_eq!(root.class_parts.len(), 2);
    assert_eq!(root.paint_bindings.len(), 1);
    assert!(root.static_inline_style.is_none());
    assert!(!root.dynamic_inline_style_parts.is_empty());
}
