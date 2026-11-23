#![forbid(unsafe_code)]

use krui_compiler::{
    compile_krui_source, parse_template_expr, BinaryOp, CompiledIfBranch, CompiledNodeKind,
    CompiledTextPart, LayoutSlot, PaintSlot, PaintSlotBinding, TemplateExpr, TemplateLit, UnaryOp,
};

#[test]
fn collects_stylesheets_and_preserves_inline_style_and_classes() {
    let src = r#"
        <style>
            div { padding: 4px; background-color: #111111; }
            .card { padding: 16px; background-color: #222222; }
        </style>
        <div id="hero" class="card accent" style="border-radius: 12px" width="200">
            <h1>KRui 엔진</h1>
        </div>
    "#;

    let doc = compile_krui_source(src).expect("should compile");
    assert_eq!(doc.roots.len(), 1);
    assert_eq!(doc.stylesheets.len(), 1);
    let root = &doc.roots[0];
    assert_eq!(root.id.as_deref(), Some("hero"));
    assert_eq!(root.classes, vec!["card".to_string(), "accent".to_string()]);
    assert_eq!(
        root.static_inline_style.as_deref(),
        Some("width: 200px; border-radius: 12px")
    );
}

#[test]
fn partial_value_interpolation_routes_transform_and_opacity_to_paint_slots() {
    let src = r#"
        <div style="transform: translate({x_pos}px, {y_pos}px) scale({zoom}); opacity: {fade_alpha}">
            <span>점수: {score}점</span>
        </div>
    "#;

    let doc = compile_krui_source(src).expect("should compile");
    let root = &doc.roots[0];
    assert!(root.layout_bindings.is_empty());
    assert_eq!(
        root.paint_bindings,
        vec![
            PaintSlotBinding {
                slot: PaintSlot::TranslateX,
                signal_ident: "x_pos".to_string(),
                expr: TemplateExpr::Path(vec!["x_pos".to_string()]),
            },
            PaintSlotBinding {
                slot: PaintSlot::TranslateY,
                signal_ident: "y_pos".to_string(),
                expr: TemplateExpr::Path(vec!["y_pos".to_string()]),
            },
            PaintSlotBinding {
                slot: PaintSlot::ScaleUniform,
                signal_ident: "zoom".to_string(),
                expr: TemplateExpr::Path(vec!["zoom".to_string()]),
            },
            PaintSlotBinding {
                slot: PaintSlot::Opacity,
                signal_ident: "fade_alpha".to_string(),
                expr: TemplateExpr::Path(vec!["fade_alpha".to_string()]),
            },
        ]
    );
    assert!(doc.warnings.is_empty());
}

#[test]
fn emits_compile_time_warning_for_layout_performance_cliff() {
    let src = r#"
        <div style="left: {anim_x}px; top: {frame:y_val}px">
            <span>경고 테스트</span>
        </div>
    "#;

    let doc = compile_krui_source(src).expect("should compile with warnings");
    assert_eq!(doc.warnings.len(), 2);
    assert!(doc.warnings[0].message.contains("Performance Cliff"));
    assert_eq!(
        doc.warnings[0].suggested_fix,
        "transform: translateX({anim_x}px)"
    );
    assert_eq!(
        doc.warnings[1].suggested_fix,
        "transform: translateY({y_val}px)"
    );
    assert_eq!(doc.roots[0].layout_bindings[1].signal_ident, "y_val");
    assert_eq!(doc.roots[0].layout_bindings[0].slot, LayoutSlot::LeftPx);
}

#[test]
fn p0_1_guardrails_reject_invalid_attributes_and_bindings_with_line_and_column() {
    let err1 = compile_krui_source("<div\n  clas=\"x\"></div>").expect_err("must reject clas");
    assert!(err1.message.contains("Unknown HTML attribute `clas`"));
    assert_eq!(err1.line, 2);
    assert_eq!(err1.column, 3);

    let err2 = compile_krui_source("<div>\n    <div class=foo></div>\n</div>")
        .expect_err("must reject unquoted");
    assert!(err2.message.contains("Unquoted attribute value"));
    assert_eq!(err2.line, 2);
    assert!(err2.column >= 16);

    let err4 =
        compile_krui_source("<div 이름=\"x\"></div>").expect_err("must reject Korean attr name");
    assert!(err4.message.contains("Unknown HTML attribute `이름`"));
    assert_eq!(err4.line, 1);
    assert_eq!(err4.column, 6);

    let err5 = compile_krui_source("<div>\n  <span>{frame:x}</span>\n</div>")
        .expect_err("must reject {frame:x} in text");
    assert!(err5
        .message
        .contains("Invalid binding expression `{frame:x}`"));
    assert_eq!(err5.line, 2);
    assert!(err5.column >= 9);
}

#[test]
fn p0_9_dsl_parses_if_else_for_empty_escaped_braces_and_single_pass_entities() {
    let src = r#"
        <div>
            <span>중괄호 이스케이프: {{hello}} &amp;lt; &nbsp;</span>
            <if test="{vm.is_logged_in}">
                <p>환영합니다</p>
            <else-if test="{vm.is_loading}">
                <p>로딩 중...</p>
            <else>
                <button disabled>로그인</button>
            </if>
            <ul>
                <for each="{item in vm.items}" key="{item.id}">
                    <li>{item.title}</li>
                </for>
                <empty>
                    <li>항목 없음</li>
                </empty>
            </ul>
        </div>
    "#;
    let doc = compile_krui_source(src).expect("should compile");
    let root = &doc.roots[0];
    assert_eq!(
        root.children[0].text_parts,
        vec![CompiledTextPart::Static(
            "중괄호 이스케이프: {hello} &lt; \u{00A0}".to_string()
        )]
    );
    match &root.children[1].kind {
        CompiledNodeKind::If(compiled_if) => {
            assert_eq!(compiled_if.branches.len(), 3);
            let CompiledIfBranch { condition_expr, .. } = &compiled_if.branches[0];
            assert_eq!(condition_expr.as_deref(), Some("vm.is_logged_in"));
            assert_eq!(
                compiled_if.branches[1].condition_expr.as_deref(),
                Some("vm.is_loading")
            );
            assert!(compiled_if.branches[2].condition_expr.is_none());
        }
        other => panic!("expected If, got {other:?}"),
    }
    let ul = &root.children[2];
    match &ul.children[0].kind {
        CompiledNodeKind::For(compiled_for) => {
            assert_eq!(compiled_for.item_var, "item");
            assert_eq!(compiled_for.list_expr, "vm.items");
            assert_eq!(compiled_for.key_expr, "item.id");
            assert_eq!(compiled_for.body.len(), 1);
            assert_eq!(compiled_for.empty_body.len(), 1);
        }
        other => panic!("expected For, got {other:?}"),
    }
}

#[test]
fn a7_rejects_missing_for_key_invalid_var_unclosed_comments_and_attr_conflicts() {
    let err_for =
        compile_krui_source(r#"<ul><for each="{item in items}"><li>{item}</li></for></ul>"#)
            .expect_err("missing key must fail");
    assert!(err_for.message.contains("key"));

    assert!(compile_krui_source(r#"<span>{unclosed</span>"#).is_err());
    assert!(compile_krui_source(r#"<span>stray } brace</span>"#).is_err());
    assert!(compile_krui_source(r#"<!-- unclosed comment <div></div>"#).is_err());

    let err_for_var = compile_krui_source(
        r#"<ul><for each="{123 in items}" key="{item.id}"><li>x</li></for></ul>"#,
    )
    .expect_err("invalid item_var must fail");
    assert!(err_for_var
        .message
        .contains("Invalid `<for each>` item variable"));

    let doc_ent =
        compile_krui_source("<span>&#123;x&#125;</span>").expect("entity braces must compile");
    assert_eq!(
        doc_ent.roots[0].text_parts,
        vec![CompiledTextPart::Static("{x}".to_string())]
    );

    let err_unbraced = compile_krui_source(r#"<button onclick="vm.save">Save</button>"#)
        .expect_err("unbraced onclick must fail");
    assert!(err_unbraced.message.contains("requires `{...}` braces"));

    for bad_src in [
        r#"<div selectable="banana"></div>"#,
        r#"<input type="color" />"#,
        r#"<div type="text"></div>"#,
        r#"<div class="a" class="b"></div>"#,
        r#"<input bind:value="{vm.x}" value="hi" />"#,
    ] {
        assert!(
            compile_krui_source(bad_src).is_err(),
            "expected `{bad_src}` to fail"
        );
    }
}

fn path_expr(segs: &[&str]) -> TemplateExpr {
    TemplateExpr::Path(segs.iter().map(|s| (*s).to_string()).collect())
}

fn expected_complex_bool_expr() -> TemplateExpr {
    TemplateExpr::Binary {
        op: BinaryOp::Or,
        left: Box::new(TemplateExpr::Binary {
            op: BinaryOp::And,
            left: Box::new(TemplateExpr::Binary {
                op: BinaryOp::Eq,
                left: Box::new(TemplateExpr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(path_expr(&["a"])),
                    right: Box::new(TemplateExpr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(path_expr(&["b"])),
                        right: Box::new(TemplateExpr::Literal(TemplateLit::Int(2))),
                    }),
                }),
                right: Box::new(TemplateExpr::Literal(TemplateLit::Int(10))),
            }),
            right: Box::new(TemplateExpr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(path_expr(&["flag"])),
            }),
        }),
        right: Box::new(path_expr(&["ready"])),
    }
}

fn assert_allowed_template_exprs() {
    let allowed_cases: &[(&str, TemplateExpr)] = &[
        ("vm.cart.total", path_expr(&["vm", "cart", "total"])),
        (
            "Msg::Add",
            TemplateExpr::EnumVariant(vec!["Msg".to_string(), "Add".to_string()]),
        ),
        ("true", TemplateExpr::Literal(TemplateLit::Bool(true))),
        ("42", TemplateExpr::Literal(TemplateLit::Int(42))),
        ("3.5", TemplateExpr::Literal(TemplateLit::Float(3.5))),
        (
            "\"hello\"",
            TemplateExpr::Literal(TemplateLit::Str("hello".to_string())),
        ),
        (
            "!vm.can_save",
            TemplateExpr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(path_expr(&["vm", "can_save"])),
            },
        ),
        (
            "a + b * 2 == 10 && !flag || ready",
            expected_complex_bool_expr(),
        ),
        (
            "ctrl.buy(5, Msg::Add)",
            TemplateExpr::Call {
                receiver: vec!["ctrl".to_string()],
                method: "buy".to_string(),
                args: vec![
                    TemplateExpr::Literal(TemplateLit::Int(5)),
                    TemplateExpr::EnumVariant(vec!["Msg".to_string(), "Add".to_string()]),
                ],
            },
        ),
    ];
    for (raw, expected) in allowed_cases {
        let parsed = parse_template_expr(raw, 0, raw).expect("allowed expr");
        assert_eq!(&parsed, expected, "AST mismatch for `{raw}`");
    }
}

#[test]
fn b1_template_expr_allows_and_forbids_grammar_table() {
    assert_allowed_template_exprs();

    let forbidden_cases: &[(&str, &str)] = &[
        (
            "<div>\n  <span>{|x| x + 1}</span>\n</div>",
            "Closures are forbidden",
        ),
        (
            "<div>\n  <button onclick=\"{match x}\">x</button>\n</div>",
            "Rust keyword `match` is forbidden",
        ),
        (
            "<div>\n  <button onclick=\"{if a}\">x</button>\n</div>",
            "Rust keyword `if` is forbidden",
        ),
        (
            "<div>\n  <button onclick=\"{{ let a = 1; a }}\">x</button>\n</div>",
            "Block expressions are forbidden",
        ),
        (
            "<div>\n  <button onclick=\"{foo(1)}\">x</button>\n</div>",
            "Standalone function call `foo(...)` is forbidden",
        ),
        (
            "<div>\n  <span>{std::process::exit(0)}</span>\n</div>",
            "Arbitrary function call `std::process::exit()` is forbidden",
        ),
    ];
    for (krui_src, expected_msg_substr) in forbidden_cases {
        let err = compile_krui_source(krui_src).expect_err("forbidden expr must fail");
        assert!(
            err.message.contains(expected_msg_substr),
            "expected `{expected_msg_substr}`, got: {}",
            err.message
        );
        assert_eq!(err.line, 2);
        assert!(err.column >= 1);
    }
}

#[cfg(feature = "preview")]
#[test]
fn b3_compiled_document_round_trips_gallery_and_rejects_version_mismatch() {
    use krui_compiler::{
        compile_krui_source_from_path, decode_compiled_document, encode_compiled_document,
        encode_compiled_document_with_version, IrCodecError, IR_FORMAT_VERSION,
    };
    use std::path::Path;

    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");

    let gallery_path =
        workspace_root.join("examples/component_gallery/views/component_gallery.krui");
    let source = std::fs::read_to_string(&gallery_path).expect("read component_gallery.krui");
    let compiled = compile_krui_source_from_path(&gallery_path, &source).expect("compile gallery");
    let bytes = encode_compiled_document(&compiled).expect("encode IR");
    let decoded = decode_compiled_document(&bytes).expect("decode IR");
    assert_eq!(compiled, decoded, "round-trip IR mismatch");

    let cf_src = r#"<style>:root { --app-gap: 14px; } .row { gap: var(--app-gap); }</style>
        <div class="row"><if test="{vm.ok}"><p>{vm.name}</p><else><button onclick="{vm.login}">Go</button></if>
        <ul><for each="{item in vm.items}" key="{item.id}"><li>{item.title}</li></for><empty><li>Empty</li></empty></ul></div>"#;
    let cf_doc = compile_krui_source(cf_src).expect("compile control flow");
    let cf_bytes = encode_compiled_document(&cf_doc).expect("encode control flow");
    assert_eq!(decode_compiled_document(&cf_bytes).expect("decode"), cf_doc);

    let wrong_ver =
        encode_compiled_document_with_version(&cf_doc, IR_FORMAT_VERSION + 99).expect("encode");
    assert_eq!(
        decode_compiled_document(&wrong_ver),
        Err(IrCodecError::VersionMismatch {
            expected: IR_FORMAT_VERSION,
            found: IR_FORMAT_VERSION + 99,
        })
    );

    let mut bad_magic = cf_bytes;
    bad_magic[0] = b'X';
    assert_eq!(
        decode_compiled_document(&bad_magic),
        Err(IrCodecError::InvalidMagic)
    );
}
