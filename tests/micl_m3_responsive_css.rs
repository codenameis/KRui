#![forbid(unsafe_code)]

use krui::*;
#[cfg(feature = "preview")]
use std::rc::Rc;

#[cfg(feature = "preview")]
use krui::dev_hot_reload::{
    compile_krui_source, mount_compiled_document, mount_compiled_document_with_registry,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LeakSnapshot {
    nodes: usize,
    stylo: usize,
    retained: usize,
    taffy: usize,
    keyframes: usize,
    comp_entries: usize,
    beh_entries: usize,
    clock_cbs: usize,
    signals: usize,
    subscribers: usize,
    scopes: usize,
    pollers: usize,
}

fn capture_leak_snapshot(session: &EngineSession) -> LeakSnapshot {
    let st = debug_stats();
    LeakSnapshot {
        nodes: session.tree.node_count(),
        stylo: session.tree.stylo_node_count(),
        retained: session.tree.retained_node_count(),
        taffy: session.tree.taffy_node_count(),
        keyframes: session.tree.active_keyframe_count(),
        comp_entries: session.tree.sparse_component_entry_count(),
        beh_entries: session.tree.sparse_behavior_entry_count(),
        clock_cbs: session.clock.active_callback_count(),
        signals: st.live_signals,
        subscribers: st.live_subscribers,
        scopes: st.live_scopes,
        pollers: st.live_async_pollers,
    }
}

fn new_cpu_session(w: u32, h: u32) -> EngineSession {
    reset_reactive_runtime();
    EngineSession::new(w, h, RenderBackendPreference::CpuFallback)
}

fn paint_of(tree: &ViewTree, id: &str) -> PaintStyle {
    tree.node_effective_paint(tree.find_by_id(id).unwrap())
        .unwrap()
}

fn bg_of(tree: &ViewTree, id: &str) -> Color {
    paint_of(tree, id).background_color
}

fn border_of(tree: &ViewTree, id: &str) -> Color {
    paint_of(tree, id).border_color
}

#[test]
fn w3c_forward_compat_drop_warnings_supports_and_strict_mode() {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                .card {
                    width: 100px; height: 40px; background-color: #123456;
                    background-color: invalid-fn(1, 2); will-change: transform;
                    touch-action: manipulation; unknown-css-prop: foo;
                    -webkit-backdrop-filter: blur(8px); -webkit-appearance: none;
                    backdrop-filter: blur(8px);
                }
                @supports (backdrop-filter: blur(8px)) { .card { border: 2px solid #224466; } }
                @supports (grid-template-rows: subgrid) { .card { background-color: #ff0000; } }
                @supports not (grid-template-rows: subgrid) { .card { opacity: 0.75; } }
            </style>
            <div id="w3c" class="card"></div>
        "#
        )
    });
    let (_, _) = tree.build_paint_scene(400, 300);
    let p = paint_of(&tree, "w3c");
    assert_eq!(p.background_color, Color::rgb(0x12, 0x34, 0x56));
    assert!(p.backdrop_blur_px >= 7.5 && p.border_color == Color::rgb(0x22, 0x44, 0x66));
    assert!((p.opacity - 0.75).abs() < 0.02);

    let _ = tree.take_style_diagnostics();
    let _ = tree.add_author_stylesheet(
        ".a { unknown-prop: 1; background-color: invalid-fn(1, 2); color: #112233; }",
    );
    assert!(!tree.take_style_diagnostics().is_empty());
}

fn assert_selector_baseline(tree: &ViewTree) {
    let (wp, ap, sp) = (
        paint_of(tree, "where_el"),
        paint_of(tree, "adj_el"),
        paint_of(tree, "sib_el"),
    );
    assert_eq!(wp.background_color, Color::rgb(0x44, 0x55, 0x66));
    assert_eq!(wp.border_color, Color::rgb(0x01, 0x02, 0x03));
    assert_eq!(ap.background_color, Color::rgb(0x55, 0x33, 0x88));
    assert_eq!(ap.border_color, Color::rgb(0x33, 0x66, 0x99));
    assert_eq!(sp.border_color, Color::rgb(0xaa, 0x22, 0x11));
}

fn assert_focus_within_and_placeholder(sess: &mut EngineSession, chk_id: NodeId) {
    let txt_id = sess.tree.find_by_id("txt_el").unwrap();
    assert_eq!(bg_of(&sess.tree, "txt_el"), Color::rgb(0x99, 0x88, 0x77));
    assert!(sess.tree.set_focus_to_node(txt_id));
    let _ = sess.step_frame(1.0 / 120.0);
    assert_eq!(
        border_of(&sess.tree, "field_el"),
        Color::rgb(0x77, 0x22, 0xcc)
    );
    assert!(sess.tree.dispatch_text_input("K"));
    assert!(sess.tree.set_focus_to_node(chk_id));
    let _ = sess.step_frame(1.0 / 120.0);
    assert_ne!(
        border_of(&sess.tree, "field_el"),
        Color::rgb(0x77, 0x22, 0xcc)
    );
    assert_eq!(bg_of(&sess.tree, "txt_el"), Color::rgb(0xcc, 0xcc, 0xcc));
}

#[test]
fn micl_complex_selectors_has_focus_within_placeholder_and_where_specificity() {
    let mut sess = new_cpu_session(640, 480);
    let (checked, text_sig) = (Signal::new(false), Signal::new(String::new()));
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                :where(.low) { background-color: #101010; color: #ffffff; }
                .high { background-color: #204060; }
                .row > .direct { border: 2px solid #010203; }
                .anchor + .adj { background-color: #11aa22; }
                .anchor ~ .sib { border: 3px solid #aa2211; }
                .chip[data-role*="chip"] { background-color: #553388; }
                .grp > :is(.alpha, .beta):not(.muted) { border: 2px solid #336699; }
                .zebra > div:nth-child(2n+1) { background-color: #445566; }
                .card { width: 220px; height: 48px; background-color: #eeeeee; display: flex; flex-direction: row; align-items: center; }
                .card:has(input:checked) { background-color: #332299; }
                .field { width: 220px; height: 48px; background-color: #dddddd; }
                .field:focus-within { border: 2px solid #7722cc; }
                input.txt { width: 160px; height: 32px; background-color: #cccccc; }
                input.txt:placeholder-shown { background-color: #998877; }
                input.chk { width: 20px; height: 20px; }
            </style>
            <div class="row grp zebra">
                <div id="where_el" class="high low direct anchor"></div>
                <div id="adj_el" class="adj chip alpha" data-role="nav-chip"></div>
                <div id="sib_el" class="sib chip beta muted" data-role="nav-chip"></div>
                <div id="card_el" class="card"><input id="chk_el" class="chk" type="checkbox" bind:checked="{checked}" /></div>
                <div id="field_el" class="field"><input id="txt_el" class="txt" placeholder="Name" bind:value="{text_sig}" /></div>
            </div>
        "#
        )
    });
    let _ = sess.step_frame(1.0 / 120.0);
    assert_selector_baseline(&sess.tree);
    let chk_id = sess.tree.find_by_id("chk_el").unwrap();
    let chk_r = sess.tree.node_layout(chk_id).unwrap();
    assert!(sess
        .tree
        .dispatch_click(chk_r.x + chk_r.width * 0.5, chk_r.y + chk_r.height * 0.5));
    let _ = sess.step_frame(1.0 / 120.0);
    assert_eq!(bg_of(&sess.tree, "card_el"), Color::rgb(0x33, 0x22, 0x99));
    assert_focus_within_and_placeholder(&mut sess, chk_id);
}

#[test]
fn css_color4_light_dark_and_preset_token_survival() {
    let mut sess = new_cpu_session(480, 320);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                :root { --card-surf: light-dark(#F5EFF7, #1D1B20); }
                .c4-box {
                    width: 80px; height: 40px; background-color: oklch(0.65 0.15 260);
                    color: oklab(0.5 0.1 -0.1); border-color: hwb(120 10% 20%);
                }
                .mixed { width: 80px; height: 40px; background-color: color-mix(in oklch, #ff0000 50%, #0000ff); }
                .ld { width: 80px; height: 40px; background-color: var(--card-surf); border: 2px solid light-dark(#6750A4, #D0BCFF); }
            </style>
            <div><div id="c4" class="c4-box"></div><div id="mix" class="mixed"></div><div id="ld" class="ld"></div></div>
        "#
        )
    });
    let _ = sess.step_frame(1.0 / 120.0);
    let c4 = paint_of(&sess.tree, "c4");
    assert!(c4.background_color != Color::TRANSPARENT && c4.border_color != Color::TRANSPARENT);
    assert_ne!(bg_of(&sess.tree, "mix"), Color::TRANSPARENT);

    sess.tree
        .set_color_scheme_preference(ColorSchemePreference::System);
    sess.tree.set_system_color_scheme(ColorScheme::Light);
    let _ = sess.step_frame(1.0 / 120.0);
    assert_eq!(sess.tree.effective_color_scheme(), ColorScheme::Light);
    let p_sl = paint_of(&sess.tree, "ld");
    assert_eq!(
        (p_sl.background_color, p_sl.border_color),
        (Color::rgb(0xF5, 0xEF, 0xF7), Color::rgb(0x67, 0x50, 0xA4))
    );

    sess.tree.set_system_color_scheme(ColorScheme::Dark);
    let _ = sess.step_frame(1.0 / 120.0);
    let p_sd = paint_of(&sess.tree, "ld");
    assert_eq!(
        (p_sd.background_color, p_sd.border_color),
        (Color::rgb(0x1D, 0x1B, 0x20), Color::rgb(0xD0, 0xBC, 0xFF))
    );

    sess.tree
        .set_color_scheme_preference(ColorSchemePreference::Light);
    let _ = sess.step_frame(1.0 / 120.0);
    assert_eq!(bg_of(&sess.tree, "ld"), Color::rgb(0xF5, 0xEF, 0xF7));
    sess.tree
        .set_color_scheme_preference(ColorSchemePreference::Dark);
    let _ = sess.step_frame(1.0 / 120.0);
    assert_eq!(bg_of(&sess.tree, "ld"), Color::rgb(0x1D, 0x1B, 0x20));
}

fn assert_boxy_geometry_and_scene(sess: &mut EngineSession) {
    let p = paint_of(&sess.tree, "boxy");
    let cr = p.effective_corner_radii();
    assert_eq!(
        (cr.top_left, cr.top_right, cr.bottom_right, cr.bottom_left),
        (28.0, 12.0, 20.0, 4.0)
    );
    assert_eq!(
        p.border_side_colors,
        Some([
            Color::rgb(0x11, 0x22, 0x33),
            Color::rgb(0x44, 0x55, 0x66),
            Color::rgb(0x77, 0x88, 0x99),
            Color::rgb(0xaa, 0xbb, 0xcc)
        ])
    );
    assert!(p.box_shadow.is_some_and(|s| !s.inset) && p.secondary_shadow.is_some_and(|s| s.inset));
    let boxy = sess.tree.find_by_id("boxy").unwrap();
    let child = sess.tree.find_by_id("child").unwrap();
    let (br, ch_r) = (
        sess.tree.node_layout(boxy).unwrap(),
        sess.tree.node_layout(child).unwrap(),
    );
    assert!((ch_r.x - br.x - 8.0).abs() < 0.5 && (ch_r.y - br.y - 2.0).abs() < 0.5);
    let (scene, _) = sess.tree.build_paint_scene(1300, 1000);
    let left_col = Color::rgb(0xaa, 0xbb, 0xcc);
    assert!(scene.primitives.iter().any(|pr| match pr {
        DrawPrimitive::StyledBox {
            background_color, ..
        } => *background_color == left_col,
        DrawPrimitive::CanvasBatch { commands, .. } => commands.iter().any(|cmd| matches!(
            cmd,
            CanvasCommand::StrokePath { color, .. } if *color == left_col
        )),
        _ => false,
    }));
}

#[test]
fn m3_window_size_classes_units_corners_borders_and_dual_shadows() {
    let mut sess = new_cpu_session(400, 800);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                :root { font-size: 20px; }
                .pane { width: 100px; height: 25dvh; }
                .dyn { width: clamp(120px, calc(10vw + 2rem), 260px); height: 2rem; }
                .boxy {
                    width: 120px; height: 80px; padding: 0px; border-radius: 28px 12px 20px 4px;
                    border-top: 2px solid #112233; border-right: 4px solid #445566;
                    border-bottom: 6px solid #778899; border-left: 8px solid #aabbcc;
                    box-shadow: 0px 4px 10px 0px rgba(0,0,0,0.25), inset 0px 2px 4px 0px rgba(255,255,255,0.4);
                }
                .inner { width: 20px; height: 20px; }
                @media (max-width: 599.98px) { .pane { width: 40px; } }
                @media (min-width: 600px) and (max-width: 839.98px) { .pane { width: 80px; } }
                @media (min-width: 840px) and (max-width: 1199.98px) { .pane { width: 140px; } }
                @media (min-width: 1200px) { .pane { width: 220px; } }
            </style>
            <div><div id="pane" class="pane"></div><div id="dyn" class="dyn"></div><div id="boxy" class="boxy"><div id="child" class="inner"></div></div></div>
        "#
        )
    });
    let pane = sess.tree.find_by_id("pane").unwrap();
    for (w, h, exp) in [
        (400, 800, 40.0),
        (700, 800, 80.0),
        (1000, 800, 140.0),
        (1300, 1000, 220.0),
    ] {
        sess.resize(w, h);
        let _ = sess.step_frame(1.0 / 120.0);
        assert!((sess.tree.node_layout(pane).unwrap().width - exp).abs() < 0.5);
    }
    let dl = sess
        .tree
        .node_layout(sess.tree.find_by_id("dyn").unwrap())
        .unwrap();
    assert!((dl.width - 170.0).abs() < 1.0 && (dl.height - 40.0).abs() < 0.5);
    assert!((sess.tree.node_layout(pane).unwrap().height - 250.0).abs() < 1.0);
    assert_boxy_geometry_and_scene(&mut sess);
}

fn assert_keyframe_halt_and_resume(
    sess: &mut EngineSession,
    show_wrap: Signal<bool>,
    show_if: Signal<bool>,
) {
    sess.tree.set_animations_paused(true);
    assert_eq!(sess.tree.active_keyframe_count(), 0);
    sess.tree.set_animations_paused(false);
    assert!(sess.tree.active_keyframe_count() >= 1);
    sess.tree
        .set_reduced_motion_preference(ReducedMotionPreference::Reduce);
    assert_eq!(sess.tree.active_keyframe_count(), 0);
    sess.tree
        .set_reduced_motion_preference(ReducedMotionPreference::NoPreference);
    assert!(sess.tree.active_keyframe_count() >= 1);
    show_wrap.set(false);
    let _ = sess.step_frame(1.0 / 120.0);
    assert_eq!(sess.tree.active_keyframe_count(), 0);
    show_wrap.set(true);
    let _ = sess.step_frame(1.0 / 120.0);
    assert!(sess.tree.active_keyframe_count() >= 1);
    show_if.set(false);
    let _ = sess.step_frame(1.0 / 120.0);
    assert_eq!(sess.tree.active_keyframe_count(), 0);
}

#[test]
fn two_tier_keyframes_budget_and_halt_conditions() {
    let mut sess = new_cpu_session(400, 300);
    let (show_wrap, show_if) = (Signal::new(true), Signal::new(true));
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                @keyframes spin { from { transform: rotate(0deg); opacity: 0.2; } to { transform: rotate(360deg); opacity: 1.0; } }
                @keyframes once { from { opacity: 0.1; } to { opacity: 0.9; } }
                .spin { width: 40px; height: 40px; animation: spin 600ms linear infinite; }
                .once { width: 30px; height: 30px; animation: once 100ms linear 1 forwards; }
                .hide-true { display: flex; }
                .hide-false { display: none; }
            </style>
            <div class="hide-{show_wrap}">
                <if test="{show_if}"><div id="spinner" class="spin"></div></if>
                <div id="finite" class="once"></div>
            </div>
        "#
        )
    });
    let _ = sess.step_frame(1.0 / 120.0);
    let sp = sess.tree.find_by_id("spinner").unwrap();
    let r = sess.tree.node_layout(sp).unwrap();
    let relayouts = sess.tree.relayout_passes();
    let aff0 = sess
        .tree
        .node_effective_paint(sp)
        .unwrap()
        .effective_affine_for_rect(r.x, r.y, r.width, r.height);
    let deg0 = aff0.b.atan2(aff0.a).to_degrees();
    for _ in 0..60 {
        assert!(!sess.step_frame(1.0 / 120.0).layout_ran);
    }
    assert_eq!(sess.tree.relayout_passes(), relayouts);
    let aff1 = sess
        .tree
        .node_effective_paint(sp)
        .unwrap()
        .effective_affine_for_rect(r.x, r.y, r.width, r.height);
    let deg1 = aff1.b.atan2(aff1.a).to_degrees();
    assert!((deg1 - deg0).abs() > 10.0, "deg0={deg0}, deg1={deg1}");
    assert!((paint_of(&sess.tree, "finite").opacity - 0.9).abs() < 0.05);
    assert_keyframe_halt_and_resume(&mut sess, show_wrap, show_if);
}

#[cfg(feature = "preview")]
#[test]
fn preview_tier2_hot_patch_and_macro_parity() {
    let src1 = r#"
        <style>
            @keyframes grow { from { width: 40px; } to { width: 120px; } }
            .box { width: 40px; height: 24px; background-color: #225588; animation: grow 200ms linear infinite alternate; }
        </style>
        <div id="t2" class="box"></div>
    "#;
    let src2 = src1
        .replace("to { width: 120px; }", "to { width: 180px; }")
        .replace("#225588", "#885522");
    let (doc1, doc2) = (
        compile_krui_source(src1).unwrap(),
        compile_krui_source(&src2).unwrap(),
    );
    let reg: RegistryRef = Rc::new(ScopeRegistry::new());
    let mut sess = new_cpu_session(400, 200);
    mount_compiled_document_with_registry(&mut sess.tree, &doc1, reg.clone());
    let _ = sess.step_frame(1.0 / 120.0);
    let rel0 = sess.tree.relayout_passes();
    let _ = sess.step_frame(0.10);
    let id = sess.tree.find_by_id("t2").unwrap();
    assert!(sess.tree.node_layout(id).unwrap().width > 55.0 && sess.tree.relayout_passes() > rel0);

    assert!(krui_preview::try_hot_patch_compiled_styles_in_place(
        &mut sess.tree,
        &doc1,
        &doc2,
        &reg
    )
    .unwrap()
    .is_some());
    let _ = sess.step_frame(0.15);
    assert!(sess.tree.node_layout(id).unwrap().width > 125.0);
    assert_eq!(
        sess.tree.node_effective_paint(id).unwrap().background_color,
        Color::rgb(0x88, 0x55, 0x22)
    );

    let parity_src = "<style>.p { width: 96px; height: 32px; border-radius: 12px 4px 8px 2px; background-color: oklch(0.6 0.1 140); }</style><div id=\"par\" class=\"p\"></div>";
    let (mut s_macro, mut s_prev) = (new_cpu_session(300, 200), new_cpu_session(300, 200));
    s_macro.tree.mount_with(|| krui!(r#"<style>.p { width: 96px; height: 32px; border-radius: 12px 4px 8px 2px; background-color: oklch(0.6 0.1 140); }</style><div id="par" class="p"></div>"#));
    mount_compiled_document(&mut s_prev.tree, &compile_krui_source(parity_src).unwrap());
    let (_, _) = (
        s_macro.step_frame(1.0 / 120.0),
        s_prev.step_frame(1.0 / 120.0),
    );
    let (a, b) = (
        s_macro.tree.find_by_id("par").unwrap(),
        s_prev.tree.find_by_id("par").unwrap(),
    );
    assert_eq!(s_macro.tree.node_layout(a), s_prev.tree.node_layout(b));
    assert_eq!(
        s_macro.tree.node_effective_paint(a),
        s_prev.tree.node_effective_paint(b)
    );
}

fn mount_leak_screen(sess: &mut EngineSession, flag: Signal<bool>, alt: bool) {
    sess.transition_to_view_with(|_| {
        if !alt {
            krui!(
                r#"
                <style>
                    @keyframes pulse { from { opacity: 0.4; } to { opacity: 1.0; } }
                    @media (min-width: 300px) { .scr { background-color: #203040; } }
                    .scr { width: 160px; height: 80px; }
                    .scr:has(input:checked) { background-color: #406080; }
                    .anim { width: 20px; height: 20px; animation: pulse 400ms linear infinite; }
                </style>
                <div class="scr"><input type="checkbox" bind:checked="{flag}" /><div class="anim"></div></div>
            "#
            )
        } else {
            krui!(
                r#"
                <style>
                    @keyframes drift { from { transform: rotate(0deg); } to { transform: rotate(90deg); } }
                    @media (min-width: 300px) { .alt { border: 2px solid #506070; } }
                    .alt { width: 140px; height: 60px; }
                    .alt:has(input:checked) { border: 2px solid #90a0b0; }
                    .rot { width: 16px; height: 16px; animation: drift 300ms linear infinite; }
                </style>
                <div class="alt"><input type="checkbox" bind:checked="{flag}" /><div class="rot"></div></div>
            "#
            )
        }
    });
}

#[test]
fn responsive_keyframe_has_screen_transitions_100x_leak_zero() {
    let mut sess = new_cpu_session(480, 320);
    let flag = Signal::new(false);
    for cycle in 0..2 {
        mount_leak_screen(&mut sess, flag, cycle % 2 == 1);
        for _ in 0..30 {
            let _ = sess.step_frame(0.05);
        }
    }
    let baseline = capture_leak_snapshot(&sess);
    for i in 0..100 {
        flag.set(i % 3 == 0);
        mount_leak_screen(&mut sess, flag, i % 2 == 1);
        for _ in 0..30 {
            let _ = sess.step_frame(0.05);
        }
    }
    assert_eq!(capture_leak_snapshot(&sess), baseline);
}
