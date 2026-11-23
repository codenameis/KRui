use super::*;

#[test]
fn stylesheet_order_and_import_font_face_collection_work() {
    let tmp_dir = std::env::temp_dir().join(format!("krui_stylo_compiler_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(tmp_dir.join("fonts")).expect("create font dir");
    let imported_css = tmp_dir.join("tokens.css");
    let font_file = tmp_dir.join("fonts").join("ui.ttf");
    let krui_path = tmp_dir.join("page.krui");

    std::fs::write(&font_file, b"DUMMY_TTF_BYTES").expect("write dummy font");
    std::fs::write(
        &imported_css,
        "@font-face { font-family: \"UI\"; src: url(\"./fonts/ui.ttf\") format(\"truetype\"); }\n:root { --accent: #2563eb; }\n",
    )
    .expect("write tokens.css");

    let src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <link rel="stylesheet" href="tokens.css" />
        <style>.box { font-family: "UI"; color: var(--accent); }</style>
        <div class="box"><Button>확인</Button></div>
    "#;
    let doc = compile_krui_source_from_path(&krui_path, src).expect("compile path doc");
    assert!(doc.unstyled);
    assert_eq!(doc.embedded_font_files.len(), 1);
    assert!(doc
        .external_css_paths
        .iter()
        .any(|p| p.ends_with("tokens.css")));
    assert!(doc.stylesheets[0].starts_with("@layer component"));
    assert!(doc
        .stylesheets
        .iter()
        .any(|s| s.contains("url(\"krui://inline/")));

    let err_preset =
        compile_krui_source(r#"<link rel="krui-preset" href="krui://preset/fluent" /><div/>"#)
            .expect_err("removed preset URI must return helpful CompileError");
    assert!(err_preset.message.contains("krui://preset/unstyled"));

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn recursive_import_cycle_fails_with_file_and_line() {
    let tmp_dir = std::env::temp_dir().join(format!("krui_import_cycle_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).expect("mkdir");
    let a = tmp_dir.join("a.css");
    let b = tmp_dir.join("b.css");
    let krui_path = tmp_dir.join("page.krui");
    std::fs::write(&a, "@import \"b.css\";\n.a { color: red; }\n").expect("write a");
    std::fs::write(&b, "@import \"a.css\";\n.b { color: blue; }\n").expect("write b");
    let err = compile_krui_source_from_path(&krui_path, "<link href=\"a.css\" /><div/>")
        .expect_err("cyclic @import must fail");
    assert!(err.message.contains("Cyclic CSS `@import`"), "{err}");
    let _ = std::fs::remove_dir_all(&tmp_dir);
}
