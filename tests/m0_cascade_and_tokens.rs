//! Extended CSS cascade, theme preset token resolution, dynamic class set, and specificity tests:
//! - Theme preset `TokenTable` resolution across all 4 presets (`Fluent`, `Material`, `LiquidGlass`, `Cosmic`)
//! - `<for>` unused item variable, 3-branch `<if>`, and dynamic `placeholder` interpolation
//! - Pseudo-class specificity (`button:hover` vs `#save` vs `button`), multi-class `class="card {a} {b}"` compound rules, and dynamic `aria-label`

#![forbid(unsafe_code)]

use krui::{krui, Color, DrawPrimitive, Signal, ViewTree};

#[test]
fn theme_presets_resolve_via_data_theme_and_preserve_explicit_user_colors() {
    let fragment = krui!(
        r#"
        <style>
            :root {
                --app-badge-bg: #10b981;
            }
            [data-theme="fluent"] {
                --demo-accent: #0078d4;
                --demo-btn-radius: 4px;
            }
            [data-theme="material"] {
                --demo-accent: #6750a4;
                --demo-btn-radius: 999px;
            }
            [data-theme="glass"] {
                --demo-accent: #0ea5e9;
                --demo-btn-radius: 14px;
            }
            [data-theme="cosmic"] {
                --demo-accent: #8b5cf6;
                --demo-btn-radius: 8px;
            }
            #preset-btn {
                background-color: var(--demo-accent);
                border-radius: var(--demo-btn-radius);
            }
            #save {
                background-color: #ff0000;
            }
            #badge {
                background-color: var(--app-badge-bg);
            }
        </style>
        <div id="theme-host" data-theme="fluent" style="width: 480px; height: 200px; gap: 12px">
            <button id="preset-btn" style="width: 140px; height: 40px">기본 버튼</button>
            <button id="save" style="width: 120px; height: 40px">저장</button>
            <div id="badge" style="width: 100px; height: 32px">배지</div>
        </div>
    "#
    );

    let mut tree = ViewTree::new();
    tree.mount(fragment);
    let host = tree.find_by_id("theme-host").expect("theme-host");

    for (name, accent, radius) in [
        ("fluent", Color::rgb(0x00, 0x78, 0xd4), 4.0_f32),
        ("material", Color::rgb(0x67, 0x50, 0xa4), 999.0_f32),
        ("glass", Color::rgb(0x0e, 0xa5, 0xe9), 14.0_f32),
        ("cosmic", Color::rgb(0x8b, 0x5c, 0xf6), 8.0_f32),
    ] {
        tree.set_node_attr(host, "data-theme", name);
        assert_preset_tokens_and_user_overrides(&mut tree, accent, radius);
    }
}

fn assert_preset_tokens_and_user_overrides(
    tree: &mut ViewTree,
    expected_accent: Color,
    expected_radius: f32,
) {
    let (scene, _) = tree.build_paint_scene(480, 200);
    let (btn_bg, btn_radius) = scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                border_radius,
                ..
            } if (rect.width - 140.0).abs() < 1.0 => Some((*background_color, *border_radius)),
            _ => None,
        })
        .expect("preset-btn StyledBox");
    assert_eq!(btn_bg, expected_accent);
    assert!((btn_radius - expected_radius).abs() < 1e-4);

    let save_bg = scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if (rect.width - 120.0).abs() < 1.0 => Some(*background_color),
            _ => None,
        })
        .expect("save button StyledBox");
    assert_eq!(save_bg, Color::rgb(255, 0, 0));

    let badge_bg = scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if (rect.width - 100.0).abs() < 1.0 => Some(*background_color),
            _ => None,
        })
        .expect("badge StyledBox");
    assert_eq!(badge_bg, Color::rgb(0x10, 0xb9, 0x81));
}

#[test]
fn for_unused_item_var_three_branch_if_and_dynamic_placeholder_compile_cleanly() {
    let items = Signal::new(vec![100_u32]);
    let fixed_key = 42_u32;
    let is_first = Signal::new(false);
    let is_second = Signal::new(false);
    let hint_target = Signal::new("사번".to_string());

    let fragment = krui!(
        r#"
        <div style="width: 480px; height: 240px; gap: 8px">
            <for each="{unused_item in items}" key="{fixed_key}">
                <span id="static-row">고정 행</span>
            </for>
            <if test="{is_first}">
                <span id="branch-1">분기 1</span>
            <else-if test="{is_second}">
                <span id="branch-2">분기 2</span>
            <else>
                <span id="branch-3">분기 3</span>
            </if>
            <input id="dyn-ph-input" style="width: 240px; height: 36px" placeholder="{hint_target}을 입력하세요" />
        </div>
    "#
    );

    let mut tree = ViewTree::new();
    tree.mount(fragment);
    let _ = tree.build_paint_scene(480, 240);

    assert!(tree.find_by_id("static-row").is_some());
    assert!(tree.find_by_id("branch-3").is_some());
    let inp_id = tree.find_by_id("dyn-ph-input").expect("dyn-ph-input");
    assert_eq!(
        tree.node_placeholder(inp_id).as_deref(),
        Some("사번을 입력하세요")
    );

    hint_target.set("이메일".to_string());
    is_second.set(true);
    let _ = tree.build_paint_scene(480, 240);
    assert_eq!(
        tree.node_placeholder(inp_id).as_deref(),
        Some("이메일을 입력하세요")
    );
    assert!(tree.find_by_id("branch-2").is_some());
    assert!(tree.find_by_id("branch-3").is_none());
}

#[test]
fn state_cascade_specificity_dynamic_class_and_dynamic_aria_label_work() {
    let active_cls = Signal::new("".to_string());
    let extra_cls = Signal::new("".to_string());
    let item_name = Signal::new("보고서".to_string());

    let fragment = krui!(
        r#"
        <style>
            button:hover { background-color: #00ff00; }
            button { background-color: #111111; }
            #save { background-color: #ff0000; }
            .card { background-color: #222222; border: 1px solid #333333; }
            .highlight { background-color: #ffaa00; }
            .outlined { border: 4px solid #00ffff; }
            .highlight.outlined { border-radius: 14px; }
        </style>
        <div style="width: 480px; height: 320px; gap: 12px">
            <button id="save" style="width: 120px; height: 40px" aria-label="삭제 {item_name}">저장</button>
            <button id="plain-btn" style="width: 130px; height: 40px">일반</button>
            <div id="dyn-card" class="card {active_cls} {extra_cls}" style="width: 160px; height: 60px">카드</div>
        </div>
    "#
    );

    let mut tree = ViewTree::new();
    tree.mount(fragment);

    let (scene_base, _) = tree.build_paint_scene(480, 320);
    assert_eq!(find_box_bg(&scene_base, 120.0), Color::rgb(255, 0, 0));
    let save_id = tree.find_by_id("save").expect("#save");
    let _ = tree.dispatch_pointer_move(20.0, 20.0);
    let (scene, _) = tree.build_paint_scene(480, 320);
    assert_eq!(find_box_bg(&scene, 120.0), Color::rgb(255, 0, 0));

    let plain_rect = tree
        .find_by_id("plain-btn")
        .and_then(|id| tree.node_layout(id))
        .expect("#plain-btn rect");
    let _ = tree.dispatch_pointer_move(plain_rect.x + 10.0, plain_rect.y + 10.0);
    let (scene_plain_hover, _) = tree.build_paint_scene(480, 320);
    assert_eq!(
        find_box_bg(&scene_plain_hover, 130.0),
        Color::rgb(0, 255, 0)
    );

    assert_eq!(
        tree.node_aria_label(save_id).as_deref(),
        Some("삭제 보고서")
    );
    item_name.set("초안".to_string());
    let _ = tree.build_paint_scene(480, 320);
    assert_eq!(tree.node_aria_label(save_id).as_deref(), Some("삭제 초안"));

    assert_eq!(find_box_bg(&scene, 160.0), Color::rgb(0x22, 0x22, 0x22));
    verify_multi_dynamic_class_compound_rule(&mut tree, active_cls, extra_cls);
}

fn find_box_bg(scene: &krui::PaintScene, expected_width: f32) -> Color {
    scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if (rect.width - expected_width).abs() < 1.0 => Some(*background_color),
            _ => None,
        })
        .expect("matching StyledBox")
}

fn verify_multi_dynamic_class_compound_rule(
    tree: &mut ViewTree,
    active_cls: Signal<String>,
    extra_cls: Signal<String>,
) {
    active_cls.set("outlined".to_string());
    extra_cls.set("highlight".to_string());
    let (scene2, _) = tree.build_paint_scene(480, 320);
    let (card_bg_after, card_bw, card_bc, card_rad) = scene2
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                border_width,
                border_color,
                border_radius,
                ..
            } if (rect.width - 160.0).abs() < 1.0 => Some((
                *background_color,
                *border_width,
                *border_color,
                *border_radius,
            )),
            _ => None,
        })
        .expect("#dyn-card StyledBox after multi-class change");
    assert_eq!(card_bg_after, Color::rgb(0xff, 0xaa, 0x00));
    assert!((card_bw - 4.0).abs() < 1e-4);
    assert_eq!(card_bc, Color::rgb(0x00, 0xff, 0xff));
    assert!((card_rad - 14.0).abs() < 1e-4);
}
