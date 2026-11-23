#![forbid(unsafe_code)]

use krui::*;

fn first_styled_box_for_size(
    scene: &PaintScene,
    w: f32,
    h: f32,
) -> (Color, Affine2, Option<BoxShadow>) {
    scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                transform,
                box_shadow,
                border_width,
                ..
            } if (rect.width - w).abs() < 1.0
                && (rect.height - h).abs() < 1.0
                && *border_width > 0.0 =>
            {
                Some((*background_color, *transform, *box_shadow))
            }
            _ => None,
        })
        .expect("expected base StyledBox for target element")
}

fn outline_alpha_for_size(scene: &PaintScene, w: f32, h: f32) -> u8 {
    scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                border_color,
                border_width,
                ..
            } if background_color.a == 0
                && *border_width > 0.0
                && (rect.width - (w + 2.0 * *border_width)).abs() < 2.0
                && (rect.height - (h + 2.0 * *border_width)).abs() < 2.0 =>
            {
                Some(border_color.a)
            }
            _ => None,
        })
        .unwrap_or(0)
}

fn build_hover_and_slide_session() -> (EngineSession, Signal<bool>, Signal<u32>) {
    reset_reactive_runtime();
    let moved = Signal::new(false);
    let clicks = Signal::new(0_u32);
    let on_move_click = move || clicks.update(|v| *v += 1);

    let mut session = EngineSession::new(480, 320, RenderBackendPreference::CpuFallback);
    session.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                #root { width: 480px; height: 320px; padding: 20px; gap: 16px; display: flex; flex-direction: column; }
                #card {
                    width: 140px; height: 60px; background-color: #102030; border: 1px solid #405060;
                    transform: translateY(0px); box-shadow: none;
                    transition: transform 200ms cubic-bezier(0.2, 0, 0, 1), background-color 200ms linear, box-shadow 200ms ease, outline-color 200ms linear;
                }
                #card:hover {
                    background-color: #90a0b0; transform: translateY(-20px);
                    outline: 2px solid rgba(103, 80, 164, 0.5);
                    box-shadow: 0px 4px 8px 2px rgba(0, 0, 0, 0.4);
                }
                #mover {
                    width: 100px; height: 44px; background-color: #6750a4; border: 1px solid #6750a4;
                    transform: translateX(0px);
                    transition: transform 200ms linear, background-color 200ms linear(0, 0.72 25%, 1.04 58%, 1);
                }
                #mover.slid-true { transform: translateX(120px); background-color: #381e72; }
            </style>
            <div id="root">
                <button id="card">Hover Card</button>
                <button id="mover" class="slid-{moved}" onclick="{on_move_click}">Move Me</button>
            </div>
        "#
        )
    });
    (session, moved, clicks)
}

fn assert_mid_flight_paint_and_hit_test(
    session: &mut EngineSession,
    mover_id: NodeId,
    initial_mover_rect: LayoutRect,
    clicks: Signal<u32>,
) {
    let (mid_scene, mid_relayout) = session.tree.build_paint_scene(480, 320);
    assert!(!mid_relayout);

    let (card_bg, card_tf, card_shadow) = first_styled_box_for_size(&mid_scene, 140.0, 60.0);
    assert!(card_bg.r > 0x10 && card_bg.r < 0x90, "r={}", card_bg.r);
    assert!(card_tf.ty < -2.0 && card_tf.ty > -20.0, "ty={}", card_tf.ty);
    assert!(card_shadow.is_some_and(|s| s.blur_radius > 0.5 && s.blur_radius < 8.0));
    let ov_a = outline_alpha_for_size(&mid_scene, 140.0, 60.0);
    assert!(ov_a > 10 && ov_a < 128, "ov_a={ov_a}");

    let (_, mover_tf, _) = first_styled_box_for_size(&mid_scene, 100.0, 44.0);
    assert!(
        mover_tf.tx > 25.0 && mover_tf.tx < 118.0,
        "tx={}",
        mover_tf.tx
    );
    let mover_rect = session.tree.node_layout(mover_id).expect("mover rect");
    assert!(
        (mover_rect.x - (initial_mover_rect.x + mover_tf.tx)).abs() < 0.5,
        "node_layout must track transition: init={}, tx={}, rect.x={}",
        initial_mover_rect.x,
        mover_tf.tx,
        mover_rect.x
    );
    let _ = session
        .tree
        .dispatch_click(initial_mover_rect.x + 5.0, initial_mover_rect.y + 22.0);
    assert_eq!(clicks.get(), 0);
    let cx = mover_rect.x + mover_rect.width * 0.5;
    let cy = mover_rect.y + mover_rect.height * 0.5;
    assert!(session.tree.dispatch_click(cx, cy));
    assert_eq!(clicks.get(), 1);
}

#[test]
fn hover_and_class_transitions_interpolate_smoothly_with_zero_relayouts_and_match_hit_test() {
    let (mut session, moved, clicks) = build_hover_and_slide_session();
    let _ = session.step_frame(1.0 / 120.0);
    assert_eq!(session.tree.active_transition_count(), 0);
    let baseline_relayouts = session.tree.relayout_passes();

    let card_id = session.tree.find_by_id("card").expect("card");
    let card_rect = session.tree.node_layout(card_id).expect("card rect");
    let mover_id = session.tree.find_by_id("mover").expect("mover");
    let initial_mover_rect = session.tree.node_layout(mover_id).expect("mover rect");
    let _ = session
        .tree
        .dispatch_pointer_move(card_rect.x + 20.0, card_rect.y + 20.0);
    moved.set(true);

    let f0 = session.step_frame(1.0 / 120.0);
    assert!(!f0.layout_ran);
    assert!(session.tree.has_active_transitions());
    assert_eq!(session.tree.active_transition_count(), 2);

    for _ in 0..6 {
        let f = session.step_frame(0.010);
        assert!(!f.layout_ran);
    }
    assert_mid_flight_paint_and_hit_test(&mut session, mover_id, initial_mover_rect, clicks);

    for _ in 0..114 {
        let f = session.step_frame(1.0 / 120.0);
        assert!(!f.layout_ran);
    }
    assert_eq!(session.tree.relayout_passes(), baseline_relayouts);
    assert!(!session.tree.has_active_transitions());
    assert_eq!(session.tree.active_transition_count(), 0);
}

#[test]
fn pause_snaps_immediately_and_cosmic_clears_shadow_transitions() {
    reset_reactive_runtime();
    let elevated = Signal::new(false);

    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                [data-theme="material"] { --demo-shadow: 0px 4px 12px 0px rgba(0, 0, 0, 0.25); }
                [data-theme="cosmic"] { --demo-shadow: none; }
                #box {
                    width: 120px;
                    height: 60px;
                    background-color: #202020;
                    border: 1px solid #303030;
                    box-shadow: var(--demo-shadow);
                    transition: background-color 200ms linear, box-shadow 200ms linear;
                }
                #box.active-true {
                    background-color: #e0e0e0;
                }
            </style>
            <div id="theme-root" data-theme="material">
                <div id="box" class="active-{elevated}"></div>
            </div>
        "#
        )
    });

    let theme_root = tree.find_by_id("theme-root").expect("theme-root");
    let _ = tree.build_paint_scene(320, 200);

    // Start transition -> verify active at 40ms -> pause must snap immediately.
    elevated.set(true);
    let _ = tree.build_paint_scene(320, 200);
    assert_eq!(tree.tick_transitions(0.040), 1);
    assert!(tree.has_active_transitions());

    tree.set_animations_paused(true);
    assert!(!tree.has_active_transitions());
    assert_eq!(tree.active_transition_count(), 0);
    let (snapped_scene, _) = tree.build_paint_scene(320, 200);
    let (snapped_bg, _, _) = first_styled_box_for_size(&snapped_scene, 120.0, 60.0);
    assert_eq!(snapped_bg, Color::rgb(0xe0, 0xe0, 0xe0));

    // Resume and switch to Cosmic -> shadow must vanish cleanly with zero stale blend.
    tree.set_animations_paused(false);
    elevated.set(false);
    let _ = tree.build_paint_scene(320, 200);
    let _ = tree.tick_transitions(0.040);
    tree.set_node_attr(theme_root, "data-theme", "cosmic");
    let _ = tree.build_paint_scene(320, 200);
    let _ = tree.tick_transitions(0.250);
    let (cos_scene, _) = tree.build_paint_scene(320, 200);
    let (_, _, cos_shadow) = first_styled_box_for_size(&cos_scene, 120.0, 60.0);
    assert!(
        cos_shadow.is_none(),
        "Cosmic target shadow must clear once data-theme switches: {cos_shadow:?}"
    );
}

#[test]
fn hovering_child_button_preserves_ancestor_card_hover_without_jitter() {
    reset_reactive_runtime();
    let noop = || {};
    let mut session = EngineSession::new(360, 240, RenderBackendPreference::CpuFallback);
    session.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                #outer-card {
                    width: 240px; height: 140px; padding: 12px;
                    background-color: #f3edf7; border: 1px solid #cac4d0;
                    border-radius: 24px;
                    transition: background-color 180ms linear, border-radius 180ms linear;
                }
                #outer-card:hover {
                    background-color: #e8def8; border-radius: 36px;
                }
                #inner-more {
                    width: 48px; height: 32px; background-color: transparent;
                    border: none;
                    transition: background-color 180ms linear;
                }
                #inner-more:hover {
                    background-color: rgba(29, 27, 32, 0.08);
                }
            </style>
            <div id="outer-card" onclick="{noop}">
                <button id="inner-more" onclick="{noop}">...</button>
            </div>
        "#
        )
    });
    let _ = session.step_frame(1.0 / 120.0);
    let card_id = session.tree.find_by_id("outer-card").expect("outer-card");
    let btn_id = session.tree.find_by_id("inner-more").expect("inner-more");
    let card_rect = session.tree.node_layout(card_id).expect("card rect");
    let btn_rect = session.tree.node_layout(btn_id).expect("btn rect");

    // 1. Hover card body and settle transition
    let _ = session
        .tree
        .dispatch_pointer_move(card_rect.x + 120.0, card_rect.y + 80.0);
    for _ in 0..30 {
        let _ = session.step_frame(0.010);
    }
    assert_eq!(session.tree.active_transition_count(), 0);

    // 2. Move pointer directly onto inner child button: outer card must stay hovered (0 new transitions on card)
    let _ = session
        .tree
        .dispatch_pointer_move(btn_rect.x + 16.0, btn_rect.y + 16.0);
    let (scene, _) = session.tree.build_paint_scene(360, 240);
    let card_bg = scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if (rect.width - 240.0).abs() < 1.0 && (rect.height - 140.0).abs() < 1.0 => {
                Some(*background_color)
            }
            _ => None,
        })
        .expect("outer card styled box");
    assert_eq!(card_bg, Color::rgb(0xe8, 0xde, 0xf8));
}

#[cfg(feature = "preview")]
#[test]
fn css_root_motion_tokens_compile_and_invalid_root_tokens_drop_safely() {
    use krui::dev_hot_reload::{compile_krui_source, mount_compiled_document};
    reset_reactive_runtime();
    let doc = compile_krui_source(
        r#"<style>
            :root {
                --motion-dur: 200ms;
                --motion-curve: cubic-bezier(0.2, 0, 0, 1);
                --card-transition: transform var(--motion-dur) var(--motion-curve), opacity 200ms linear;
                --hover-lift: translateY(-12px) scale(1.05);
            }
            #mcard { width: 160px; height: 80px; background-color: #224466; transition: var(--card-transition); }
            #mcard:hover { transform: var(--hover-lift); opacity: 0.75; }
        </style>
        <div style="width: 320px; height: 200px;"><div id="mcard">card</div></div>"#,
    )
    .expect(":root motion/transform custom properties must compile");
    let mut session = EngineSession::new(320, 200, RenderBackendPreference::CpuFallback);
    mount_compiled_document(&mut session.tree, &doc);
    let _ = session.step_frame(1.0 / 120.0);
    let card_id = session.tree.find_by_id("mcard").expect("mcard");
    let initial_rect = session.tree.node_layout(card_id).expect("initial rect");
    let relayouts_before = session.tree.relayout_passes();
    let _ = session
        .tree
        .dispatch_pointer_move(initial_rect.x + 20.0, initial_rect.y + 20.0);
    for _ in 0..5 {
        let _ = session.step_frame(0.016);
    }
    assert!(session.tree.active_transition_count() >= 1);
    assert_eq!(session.tree.relayout_passes(), relayouts_before);
    let mid_rect = session.tree.node_layout(card_id).expect("mid rect");
    assert!(
        mid_rect.y < initial_rect.y - 1.0,
        "hover transition must lift #mcard upward (initial_y={}, mid_y={})",
        initial_rect.y,
        mid_rect.y
    );
    for bad_token_src in [
        "<style>:root { --bad-token: ???; }</style><div></div>",
        "<style>:root { --krui-bg: 200ms; }</style><div></div>",
    ] {
        assert!(compile_krui_source(bad_token_src).is_ok());
    }
}

#[cfg(all(feature = "preview", debug_assertions))]
fn write_temp_swap_theme(dir: &std::path::Path, css: &str) {
    std::fs::write(dir.join("theme.css"), css).expect("write temp theme.css");
}

#[cfg(all(feature = "preview", debug_assertions))]
fn setup_temp_swap_session(
    temp_dir: &std::path::Path,
) -> (
    EngineSession,
    krui::dev_hot_reload::DevFileWatcher,
    Signal<i32>,
) {
    use krui::dev_hot_reload::{
        compile_krui_source_from_path, mount_compiled_document_with_registry, DevFileWatcher,
    };
    use std::rc::Rc;

    let _ = std::fs::remove_dir_all(temp_dir);
    std::fs::create_dir_all(temp_dir).expect("create temp swap dir");
    let krui_path = temp_dir.join("swap_app.krui");
    std::fs::write(
        &krui_path,
        r#"<link rel="stylesheet" href="theme.css" />
        <div id="root-shell" class="shell">
            <if test="{vm.tab == 0}">
                <button id="tab0-btn" class="card" onclick="{vm.noop}">T0</button>
            <else>
                <button id="tab1-btn" class="card" onclick="{vm.noop}">T1</button>
            </if>
        </div>"#,
    )
    .expect("write swap_app.krui");
    write_temp_swap_theme(
        temp_dir,
        ":root { --krui-bg: #112233; --card-tr: transform 180ms linear; }\n\
         .shell { width: 300px; height: 200px; background-color: var(--krui-bg); }\n\
         .card { width: 120px; height: 48px; background-color: #334455; border: 1px solid #000000; transition: var(--card-tr); }\n\
         .card:hover { transform: translateY(-4px); }\n",
    );
    let tab = Signal::new(0_i32);
    let mut vm = ScopeRegistry::new();
    vm.insert_signal("tab", DynSignal::from_signal(tab));
    vm.insert_value(
        "noop",
        Value::Command(DynCommand::new(Rc::new(|| {}), None)),
    );
    let mut root_reg = ScopeRegistry::new();
    root_reg.insert_registry("vm", vm);
    let reg_ref: RegistryRef = Rc::new(root_reg);
    let doc =
        compile_krui_source_from_path(&krui_path, &std::fs::read_to_string(&krui_path).unwrap())
            .expect("compile initial swap_app.krui");
    let mut session = EngineSession::new(300, 200, RenderBackendPreference::CpuFallback);
    mount_compiled_document_with_registry(&mut session.tree, &doc, reg_ref.clone());
    let watcher = DevFileWatcher::with_registry(&krui_path, reg_ref).expect("watcher");
    let _ = session.step_frame(1.0 / 120.0);
    (session, watcher, tab)
}

#[cfg(all(feature = "preview", debug_assertions))]
fn step_and_box_color(session: &mut EngineSession, w: f32, h: f32) -> Color {
    let _ = session.step_frame(1.0 / 120.0);
    let (scene, _) = session.tree.build_paint_scene(300, 200);
    first_styled_box_for_size(&scene, w, h).0
}

#[cfg(all(feature = "preview", debug_assertions))]
#[test]
fn swapping_only_external_css_file_updates_styles_transitions_across_if_tabs_and_resets_removed_tokens(
) {
    reset_reactive_runtime();
    let temp_dir = std::env::temp_dir().join(format!("krui_css_swap_{}", std::process::id()));
    let (mut session, mut watcher, tab) = setup_temp_swap_session(&temp_dir);
    let shell_id = session.tree.find_by_id("root-shell").expect("root-shell");

    write_temp_swap_theme(
        &temp_dir,
        ":root { --krui-bg: #551122; }\n\
         .shell { width: 300px; height: 200px; background-color: var(--krui-bg, #ffffff); }\n\
         .card { width: 120px; height: 48px; background-color: #22aa66; border: 1px solid #000000; transition: transform 320ms linear(0, 0.75 30%, 1.03 65%, 1); }\n\
         .card:hover { transform: translateY(-20px); }\n",
    );
    assert!(
        watcher.force_reload(&mut session.tree).is_some(),
        "reload theme.css v2"
    );
    assert!(
        watcher.last_compile_error().is_none(),
        "{:?}",
        watcher.last_compile_error()
    );
    assert_eq!(
        session
            .tree
            .node_computed_style(shell_id)
            .unwrap()
            .paint
            .background_color,
        Color::rgb(0x55, 0x11, 0x22)
    );

    tab.set(1);
    assert_eq!(
        step_and_box_color(&mut session, 120.0, 48.0),
        Color::rgb(0x22, 0xaa, 0x66)
    );
    tab.set(0);
    assert_eq!(
        step_and_box_color(&mut session, 120.0, 48.0),
        Color::rgb(0x22, 0xaa, 0x66)
    );

    let btn0 = session.tree.find_by_id("tab0-btn").expect("tab0-btn");
    let init0 = session.tree.node_layout(btn0).expect("btn0 rect");
    let relayout_before = session.tree.relayout_passes();
    let _ = session
        .tree
        .dispatch_pointer_move(init0.x + 20.0, init0.y + 20.0);
    for _ in 0..8 {
        let _ = session.step_frame(0.016);
    }
    assert_eq!(session.tree.relayout_passes(), relayout_before);
    let mid0 = session.tree.node_layout(btn0).expect("btn0 mid hover rect");
    assert!(mid0.y < init0.y - 6.0, "init={}, mid={}", init0.y, mid0.y);

    write_temp_swap_theme(
        &temp_dir,
        ".shell { width: 300px; height: 200px; background-color: var(--krui-bg, #ffffff); }\n\
         .card { width: 120px; height: 48px; background-color: #446688; border: 1px solid #000000; }\n",
    );
    assert!(
        watcher.force_reload(&mut session.tree).is_some(),
        "reload removed :root"
    );
    assert_eq!(
        session
            .tree
            .node_computed_style(shell_id)
            .unwrap()
            .paint
            .background_color,
        Color::WHITE,
        "removing :root override from CSS must reset --krui-bg"
    );
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn state_overlay_is_dropped_and_unsupported_in_at_supports() {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"<style>
                .box { width: 40px; height: 20px; background-color: #112233; state-overlay-color: #ff0000; }
                @supports (state-overlay-color: red) {
                    .box { opacity: 0.2; }
                }
                @supports not (state-overlay-color: red) {
                    .box { border-radius: 9px; }
                }
            </style>
            <div id="so-box" class="box"></div>"#
        )
    });
    let _ = tree.build_paint_scene(200, 120);
    let st = tree
        .node_computed_style(tree.find_by_id("so-box").unwrap())
        .unwrap();
    assert_eq!(st.paint.background_color, Color::rgb(0x11, 0x22, 0x33));
    assert!((st.paint.opacity - 1.0).abs() < 1e-4);
    assert!((st.paint.border_radius - 9.0).abs() < 1e-4);
}
