#![forbid(unsafe_code)]

use krui_compiler::{
    compile_krui_source, CompiledFillRule, CompiledLineCap, CompiledLineJoin, CompiledPathSegment,
    CompiledSvgColor, CompiledSvgOp, CompiledTextPart, HtmlTag,
};

#[test]
fn img_supports_void_and_self_closing_alt_and_hero_class() {
    let src = r#"
        <style>
            img.hero { width: 120px; height: 80px; object-fit: cover; border-radius: 12px; }
        </style>
        <div>
            <img class="hero" src="logo.png" alt="App Logo">
            <img src="{vm.avatar}" />
        </div>
    "#;
    let doc = compile_krui_source(src).expect("<img> should compile");
    let div = &doc.roots[0];
    assert_eq!(div.children.len(), 2);
    assert!(doc
        .stylesheets
        .iter()
        .any(|s| s.contains("object-fit: cover")));

    let first = &div.children[0];
    assert_eq!(first.tag, HtmlTag::Img);
    assert_eq!(first.classes, vec!["hero".to_string()]);
    assert_eq!(
        first.img_src,
        vec![CompiledTextPart::Static("logo.png".into())]
    );
    assert_eq!(first.aria_label.as_deref(), Some("App Logo"));

    let second = &div.children[1];
    assert_eq!(second.tag, HtmlTag::Img);
    assert!(second
        .img_src
        .iter()
        .any(|p| matches!(p, CompiledTextPart::Dynamic { .. })));
}

#[test]
fn img_rejects_missing_src_and_stray_img_attrs_on_div() {
    let err = compile_krui_source(r#"<img alt="missing">"#).expect_err("missing src");
    assert!(err.message.contains("src"), "{err}");

    let err = compile_krui_source(r#"<div src="bad.png"></div>"#).expect_err("src on div");
    assert!(err.message.contains("img"), "{err}");
}

#[test]
fn svg_compiles_paths_arcs_shapes_and_current_color() {
    let src = r##"
        <svg width="32" height="32" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" fill="none">
            <rect x="2" y="3" width="20" height="18" rx="4" fill="#102030" />
            <circle cx="12" cy="12" r="5" />
            <line x1="0" y1="24" x2="24" y2="0" />
            <polyline points="1,2 3,4 5,6" />
            <polygon points="0,0 10,0 5,10" fill="#ff0000" />
            <path d="M2 12 L10 18 A 6 6 0 0 1 20 8 C 21 5 22 4 22 4 Z" />
        </svg>
    "##;
    let doc = compile_krui_source(src).expect("<svg> should compile");
    let svg_node = &doc.roots[0];
    assert_eq!(svg_node.tag, HtmlTag::Svg);
    let svg = svg_node.svg_data.as_ref().expect("svg_data populated");
    assert_eq!(svg.view_box, [0.0, 0.0, 24.0, 24.0]);
    assert_eq!(svg.intrinsic_size, (32.0, 32.0));
    assert!(svg.ops.len() >= 6);

    assert!(svg.ops.iter().any(|op| matches!(
        op,
        CompiledSvgOp::StrokeCircle {
            color: CompiledSvgColor::CurrentColor,
            ..
        }
    )));
    let has_arc_and_cubic = svg.ops.iter().any(|op| match op {
        CompiledSvgOp::StrokePath { segments, .. } => {
            segments
                .iter()
                .any(|s| matches!(s, CompiledPathSegment::Arc { .. }))
                && segments
                    .iter()
                    .any(|s| matches!(s, CompiledPathSegment::CubicTo { .. }))
        }
        _ => false,
    });
    assert!(has_arc_and_cubic, "expected Arc and CubicTo path segments");
}

#[test]
fn svg_rejects_unknown_child_element_and_unknown_attribute() {
    let err = compile_krui_source(r#"<svg viewBox="0 0 10 10"><filter id="x"/></svg>"#)
        .expect_err("unknown <filter> must fail");
    assert!(err.message.contains("filter"), "{err}");

    let err = compile_krui_source(
        r#"<svg viewBox="0 0 10 10"><rect x="0" y="0" width="5" height="5" bogus="1"/></svg>"#,
    )
    .expect_err("unknown attribute must fail");
    assert!(err.message.contains("bogus"), "{err}");
}

#[test]
fn svg_compiles_and_validates_line_cap_join_and_fill_rule() {
    let src = r##"
        <svg viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"
             stroke-linecap="round" stroke-linejoin="round" fill-rule="evenodd">
            <path d="M2 2 L10 10 L18 2 Z" />
            <line x1="2" y1="20" x2="20" y2="20" stroke-linecap="square" />
        </svg>
    "##;
    let doc = compile_krui_source(src).expect("<svg> cap/join/fill-rule should compile");
    let svg = doc.roots[0].svg_data.as_ref().expect("svg_data");
    assert!(svg.ops.iter().any(|op| matches!(
        op,
        CompiledSvgOp::FillPath {
            fill_rule: CompiledFillRule::EvenOdd,
            ..
        }
    )));
    assert!(svg.ops.iter().any(|op| matches!(
        op,
        CompiledSvgOp::StrokePath {
            line_cap: CompiledLineCap::Round,
            line_join: CompiledLineJoin::Round,
            ..
        }
    )));
    assert!(svg.ops.iter().any(|op| matches!(
        op,
        CompiledSvgOp::StrokePath {
            line_cap: CompiledLineCap::Square,
            ..
        }
    )));
    let bad = compile_krui_source(
        r#"<svg viewBox="0 0 10 10"><path d="M0 0 L5 5" stroke-linecap="invalid"/></svg>"#,
    )
    .expect_err("invalid stroke-linecap must fail");
    assert!(bad.message.contains("stroke-linecap"), "{bad}");
}

#[test]
fn anchor_href_binds_interpolated_link() {
    let src = r#"<a href="/users/{vm.id}">Profile</a>"#;
    let doc = compile_krui_source(src).expect("<a href> should compile");
    let a = &doc.roots[0];
    assert_eq!(a.tag, HtmlTag::A);
    assert_eq!(a.link_href.len(), 2);
}
