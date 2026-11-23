//! Phase 5 Integration Suite:
//! 1. Lean `ElementNode` + sparse secondary tables & zero-leak 100x nested `<if><for>` stress test
//! 2. Real Top-Layer `<select>` dropdown overlay (`overflow: hidden` escape, option click, outside dismiss, upward flip)
//! 3. `<textarea>` vertical scrolling, caret auto-scroll, strict inner-box clipping, resize re-wrap, & scroll chaining
//! 4. `UiWaker` background wakeup (`Command::new_async` + `Effects::task`), `ControlFlow::Wait` 0% idle CPU, & `softbuffer` pixel packing
//! 5. Zero global `"1"`~`"4"` / `F5`~`F8` theme shortcut hijacking

#![forbid(unsafe_code)]

use krui::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use winit::event_loop::ControlFlow;

#[derive(Clone, PartialEq)]
struct RowItem {
    id: u32,
    title: String,
    flagged: bool,
}

fn make_rows(prefix: &str, base_id: u32) -> Vec<RowItem> {
    vec![
        RowItem {
            id: base_id + 1,
            title: format!("{prefix}-1"),
            flagged: true,
        },
        RowItem {
            id: base_id + 2,
            title: format!("{prefix}-2"),
            flagged: false,
        },
        RowItem {
            id: base_id + 3,
            title: format!("{prefix}-3"),
            flagged: true,
        },
    ]
}

fn find_id(tree: &ViewTree, element_id: &str) -> NodeId {
    tree.find_by_id(element_id)
        .unwrap_or_else(|| panic!("expected `#{element_id}`"))
}

fn center_of(tree: &ViewTree, id: NodeId) -> (f32, f32) {
    let r = tree.node_layout(id).expect("expected layout rect");
    (r.x + r.width * 0.5, r.y + r.height * 0.5)
}

fn assert_lean_element_node_and_zero_plain_sparse() {
    let overhead =
        ViewTree::element_node_byte_size().saturating_sub(std::mem::size_of::<CompiledStyle>());
    assert!(
        overhead < 360,
        "ElementNode overhead beyond CompiledStyle should remain lean (< 360B): got {overhead}"
    );
    let mut plain_tree = ViewTree::new();
    plain_tree.mount(krui!(
        r#"
        <div style="display: flex; flex-direction: column; width: 300px; height: 200px;">
            <div><span>Alpha</span><span>Beta</span></div>
            <div><p>Gamma</p><p>Delta</p></div>
        </div>
    "#
    ));
    assert_eq!(
        plain_tree.sparse_behavior_entry_count(),
        0,
        "static plain containers/labels must allocate zero sparse behavior entries"
    );
}

#[test]
fn phase5_sparse_dom_and_nested_if_for_zero_leak_100x() {
    reset_reactive_runtime();
    assert_lean_element_node_and_zero_plain_sparse();

    reset_reactive_runtime();
    let show_outer = Signal::new(true);
    let rows = Signal::new(make_rows("Init", 0));
    let hits = Signal::new(0_u32);
    let on_hit = move || hits.update(|v| *v += 1);

    let mut session = EngineSession::new(480, 320, RenderBackendPreference::CpuFallback);
    session.tree.mount_with(|| {
        krui!(
            r#"
            <div style="display: flex; flex-direction: column; width: 460px; height: 300px;">
                <if test="{show_outer}">
                    <ul style="display: flex; flex-direction: column; gap: 4px;">
                        <for each="{item in rows}" key="{item.id}">
                            <li style="display: flex; gap: 6px;">
                                <span>{item.title}</span>
                                <if test="{item.flagged}">
                                    <button onclick="{on_hit}">Flag</button>
                                </if>
                            </li>
                        </for>
                    </ul>
                </if>
            </div>
        "#
        )
    });

    show_outer.set(false);
    let _ = session.step_frame(1.0 / 120.0);
    show_outer.set(true);
    let _ = session.step_frame(1.0 / 120.0);

    let base = capture_runtime_counts(&session.tree);
    for step in 1..=100_u32 {
        show_outer.set(false);
        let _ = session.step_frame(1.0 / 120.0);
        rows.set(make_rows("Step", step * 10));
        show_outer.set(true);
        let _ = session.step_frame(1.0 / 120.0);
    }
    rows.set(make_rows("Init", 0));
    let _ = session.step_frame(1.0 / 120.0);
    let after = capture_runtime_counts(&session.tree);
    assert_eq!(
        base, after,
        "100x nested <if><for> toggles must have +0 delta across all runtime/DOM/Taffy/sparse counts"
    );
}

fn capture_runtime_counts(tree: &ViewTree) -> (usize, usize, usize, usize, usize, usize) {
    let stats = debug_stats();
    (
        tree.node_count(),
        tree.taffy_node_count(),
        tree.sparse_component_entry_count(),
        stats.live_signals,
        stats.live_subscribers,
        stats.live_scopes,
    )
}

#[test]
fn phase5_select_top_layer_escapes_overflow_hidden_and_flips_upward() {
    reset_reactive_runtime();
    let chosen = Signal::new(String::from("opt1"));
    let changed_log = Signal::new(Vec::<String>::new());
    let on_sel_change = move |val: String| changed_log.update(|v| v.push(val));

    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                #clip-card { display: flex; width: 320px; height: 44px; padding: 4px; overflow: hidden; }
                select { width: 220px; height: 34px; }
            </style>
            <div style="display: flex; flex-direction: column; width: 400px; height: 260px;">
                <div id="clip-card">
                    <select id="top-sel" bind:value="{chosen}" onchange="{on_sel_change}">
                        <option value="opt1" selected>Option One</option>
                        <option value="opt2">Option Two</option>
                        <option value="opt3">Option Three</option>
                        <option value="opt4">Option Four</option>
                    </select>
                </div>
            </div>
        "#
        )
    });

    let _ = tree.build_paint_scene(400, 260);
    let sel_id = find_id(&tree, "top-sel");
    let (sx, sy) = center_of(&tree, sel_id);
    assert!(tree.dispatch_click(sx, sy) && tree.is_select_popup_open(sel_id));

    let (scene, _) = tree.build_paint_scene(400, 260);
    let popup_rect = assert_popup_escapes_overflow_hidden(&tree, &scene);

    let row3_y = popup_rect.y + 4.0 + 2.5 * 32.0;
    let _ = tree.dispatch_pointer_move(popup_rect.x + 30.0, row3_y);
    assert!(tree.dispatch_click(popup_rect.x + 30.0, row3_y));
    assert_eq!(chosen.get(), "opt3");
    assert_eq!(changed_log.get(), vec!["opt3".to_string()]);
    assert!(!tree.is_select_popup_open(sel_id));

    assert!(tree.dispatch_click(sx, sy) && tree.is_select_popup_open(sel_id));
    assert!(tree.clear_focus() && !tree.is_select_popup_open(sel_id));
    assert_eq!(tree.focused_node_id(), Some(sel_id));

    assert!(tree.dispatch_click(sx, sy) && tree.is_select_popup_open(sel_id));
    assert!(tree.dispatch_click(380.0, 240.0) && !tree.is_select_popup_open(sel_id));

    verify_select_popup_flips_upward_near_bottom();
}

fn assert_popup_escapes_overflow_hidden(tree: &ViewTree, scene: &PaintScene) -> LayoutRect {
    let popup_rect = tree.active_select_popup_rect().expect("popup rect");
    assert!(
        popup_rect.y + popup_rect.height > 44.0,
        "popup must extend below 44px card: {popup_rect:?}"
    );
    let has_unclipped_popup_box = scene.primitives.iter().any(|p| match p {
        DrawPrimitive::StyledBox { rect, clip, .. } => {
            clip.is_none() && (rect.y - popup_rect.y).abs() < 1.0 && rect.y + rect.height > 44.0
        }
        _ => false,
    });
    let has_below_card_glyphs = scene.primitives.iter().any(|p| match p {
        DrawPrimitive::TextGlyphs { origin_y, .. } => *origin_y > 44.0,
        _ => false,
    });
    assert!(
        has_unclipped_popup_box && has_below_card_glyphs,
        "Top-Layer popup box and glyphs must escape ancestor overflow: hidden"
    );
    popup_rect
}

fn verify_select_popup_flips_upward_near_bottom() {
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <div style="display: flex; flex-direction: column; width: 320px; height: 220px; padding-top: 176px;">
            <select id="bottom-sel" style="width: 200px; height: 34px;">
                <option value="a" selected>Alpha</option>
                <option value="b">Bravo</option>
                <option value="c">Charlie</option>
                <option value="d">Delta</option>
            </select>
        </div>
    "#
    ));
    let _ = tree.build_paint_scene(320, 220);
    let sel_id = find_id(&tree, "bottom-sel");
    let sel_rect = tree.node_layout(sel_id).expect("bottom select rect");
    assert!(tree.dispatch_click(sel_rect.x + 20.0, sel_rect.y + 16.0));
    let _ = tree.build_paint_scene(320, 220);
    let popup_rect = tree
        .active_select_popup_rect()
        .expect("upward flipped popup rect");
    assert!(
        popup_rect.y + popup_rect.height <= sel_rect.y + 0.5,
        "popup near bottom of viewport must flip upward above <select>: sel={sel_rect:?}, popup={popup_rect:?}"
    );
}

#[test]
fn phase5_textarea_vertical_scroll_strict_clip_resize_rewrap_and_scroll_chaining() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <style>
            #outer-scroll { display: flex; flex-direction: column; width: 100%; height: 180px; overflow-y: scroll; padding: 8px; gap: 8px; }
            #ed { width: 220px; height: 68px; border-width: 2px; padding: 6px; flex-shrink: 0; }
            #wrap-p { width: 100%; flex-shrink: 0; }
            .tall-tail { width: 180px; height: 280px; flex-shrink: 0; }
        </style>
        <div id="outer-scroll">
            <textarea id="ed"></textarea>
            <p id="wrap-p">Resizing the viewport container width re-wraps multiline paragraph text cleanly across lines.</p>
            <div class="tall-tail">Tail</div>
        </div>
    "#
    ));

    let _ = tree.build_paint_scene(420, 180);
    let ed_id = find_id(&tree, "ed");
    let outer_id = find_id(&tree, "outer-scroll");
    let wrap_id = find_id(&tree, "wrap-p");

    assert!(tree.set_focus_to_node(ed_id));
    let ten_lines = (1..=10)
        .map(|i| format!("Line number {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(tree.dispatch_text_input(&ten_lines));
    let (scene, _) = tree.build_paint_scene(420, 180);
    assert!(
        tree.node_scroll_y(ed_id).unwrap_or(0.0) > 40.0,
        "typing 10 lines into 68px <textarea> must auto-scroll caret into view"
    );
    verify_textarea_caret_and_glyphs_strictly_clipped(&tree, ed_id, &scene);

    for _ in 0..9 {
        assert!(tree.dispatch_cursor_up());
    }
    let _ = tree.build_paint_scene(420, 180);
    assert!(
        tree.node_scroll_y(ed_id).unwrap_or(99.0) < 1.0,
        "moving caret back to line 1 must auto-scroll <textarea> back to top"
    );

    assert_scroll_chaining_and_resize_rewrap(&mut tree, ed_id, outer_id, wrap_id);
}

fn assert_scroll_chaining_and_resize_rewrap(
    tree: &mut ViewTree,
    ed_id: NodeId,
    outer_id: NodeId,
    wrap_id: NodeId,
) {
    let (ex, ey) = center_of(tree, ed_id);
    assert!(tree.dispatch_scroll(ex, ey, -30.0));
    assert!((tree.node_scroll_y(ed_id).unwrap_or(0.0) - 30.0).abs() < 0.5);
    assert_eq!(tree.scroll_offset_y(outer_id), Some(0.0));
    assert!(tree.dispatch_scroll(ex, ey, -500.0));
    assert!(
        tree.scroll_offset_y(outer_id).unwrap_or(0.0) > 20.0,
        "wheel event exceeding <textarea> bottom boundary must chain remaining delta up to outer scroll container"
    );

    assert!(tree.dispatch_scroll(ex, ey, 800.0));
    let _ = tree.build_paint_scene(420, 180);
    let wide_h = tree.node_layout(wrap_id).expect("wide p rect").height;
    let (narrow_scene, _) = tree.build_paint_scene(160, 180);
    let narrow_h = tree.node_layout(wrap_id).expect("narrow p rect").height;
    let max_glyph_y = narrow_scene
        .primitives
        .iter()
        .filter_map(|p| match p {
            DrawPrimitive::TextGlyphs { glyphs, .. } if glyphs.len() > 20 => {
                glyphs.iter().map(|g| g.y as i32).max()
            }
            _ => None,
        })
        .max()
        .unwrap_or(0);
    assert!(
        narrow_h > wide_h && max_glyph_y > 18,
        "narrowing container must re-wrap paragraph text: wide_h={wide_h}, narrow_h={narrow_h}, max_glyph_y={max_glyph_y}"
    );
}

fn verify_textarea_caret_and_glyphs_strictly_clipped(
    tree: &ViewTree,
    ed_id: NodeId,
    scene: &PaintScene,
) {
    let caret_color = tree
        .node_effective_paint(ed_id)
        .map(|p| p.resolve_caret_color(p.text_color))
        .expect("textarea paint");
    let ed_rect = tree.node_layout(ed_id).expect("textarea rect");
    let expected_inner = LayoutRect {
        x: ed_rect.x + 2.0,
        y: ed_rect.y + 2.0,
        width: ed_rect.width - 4.0,
        height: ed_rect.height - 4.0,
    };
    let caret_clip = scene.primitives.iter().find_map(|p| match p {
        DrawPrimitive::StyledBox {
            rect,
            clip,
            background_color,
            ..
        } if *background_color == caret_color && rect.width <= 4.0 => Some(*clip),
        _ => None,
    });
    let inner_clip = caret_clip
        .flatten()
        .expect("focused <textarea> caret must carry Some(inner_clip_rect)");
    assert!(
        inner_clip.x >= expected_inner.x - 0.5
            && inner_clip.y >= expected_inner.y - 0.5
            && inner_clip.width <= expected_inner.width + 0.5
            && inner_clip.height <= expected_inner.height + 0.5,
        "caret clip must stay strictly inside 2px border: got {inner_clip:?}, expected {expected_inner:?}"
    );
}

struct CountingWaker(Arc<AtomicUsize>, mpsc::Sender<()>);
impl UiWaker for CountingWaker {
    fn wake(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        let _ = self.1.send(());
    }
}

#[test]
fn phase5_ui_waker_async_wakeup_and_zero_poll_idle_control_flow() {
    reset_reactive_runtime();
    let wake_count = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel();
    set_ui_waker(Some(Arc::new(CountingWaker(wake_count.clone(), tx))));

    let cmd = Command::new_async(|done| {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(15));
            done();
        });
    });
    let store = Store::new(0_i32, |state, msg: i32| {
        if msg == 0 {
            Effects::task(|done| {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(15));
                    done(42);
                });
            })
        } else {
            *state = msg;
            Effects::none()
        }
    });

    let mut session = EngineSession::new(320, 200, RenderBackendPreference::CpuFallback);
    cmd.execute();
    store.send(0);
    assert!(cmd.is_running.get_untracked() && !session.clock.has_active_animations());
    let idle_flow = compute_idle_control_flow(
        session.clock.has_active_animations(),
        false,
        false,
        false,
        Instant::now(),
    );
    assert_eq!(idle_flow, ControlFlow::Wait);

    rx.recv_timeout(Duration::from_secs(2))
        .expect("first background wakeup");
    rx.recv_timeout(Duration::from_secs(2))
        .expect("second background wakeup");
    assert!(wake_count.load(Ordering::SeqCst) >= 2 && poll_async_tasks());
    assert!(!cmd.is_running.get_untracked() && store.get() == 42);
    set_ui_waker(None);

    assert_cpu_fallback_and_softbuffer_packing(&mut session);
}

fn assert_cpu_fallback_and_softbuffer_packing(session: &mut EngineSession) {
    session.tree.mount(krui!(
        r#"<div style="width: 80px; height: 60px;"><button>CPU</button></div>"#
    ));
    let telem = session.step_frame(1.0 / 120.0);
    assert_eq!(
        telem.render_stats.backend_kind,
        ActiveBackendKind::TinySkiaCpuFallback
    );
    let src_rgba = [
        255_u8, 64, 32, 255, // opaque
        0, 0, 0, 0, // transparent
        128, 64, 0, 128, // 50% premultiplied -> unpremultiplies back to ~(255, 128, 0)
    ];
    let mut dst_u32 = [0_u32; 3];
    pack_premultiplied_rgba_to_softbuffer_u32(&src_rgba, &mut dst_u32);
    assert_eq!(dst_u32[0], 0x00FF_4020);
    assert_eq!(dst_u32[1], 0x0000_0000);
    assert_eq!(dst_u32[2] >> 16, 255);
}

#[test]
fn phase5_no_global_theme_key_hijack() {
    reset_reactive_runtime();
    let mut session = EngineSession::new(400, 240, RenderBackendPreference::CpuFallback);
    session.tree.mount(krui!(
        r#"
        <style>[data-theme="fluent"] { background-color: #112233; }</style>
        <div id="host" data-theme="fluent" style="width: 300px; height: 120px;"><p>App</p></div>
        "#
    ));
    let _ = session.step_frame(1.0 / 120.0);
    session.tree.clear_focus();
    let host_id = session.tree.find_by_id("host").expect("host");
    let expected_bg = Color::rgb(0x11, 0x22, 0x33);
    assert_eq!(
        session
            .tree
            .node_computed_style(host_id)
            .unwrap()
            .paint
            .background_color,
        expected_bg
    );

    for digit in ["1", "2", "3", "4"] {
        let ki = KeyInfo::Character {
            logical: digit.to_string(),
            text: Some(digit.to_string()),
        };
        let _ = session.handle_key_down(&ki, Mods::default(), false);
        assert_eq!(
            session
                .tree
                .node_computed_style(host_id)
                .unwrap()
                .paint
                .background_color,
            expected_bg
        );
    }
    for fkey in ["F5", "F6", "F7", "F8"] {
        let ki = KeyInfo::Other(fkey.to_string());
        let _ = session.handle_key_down(&ki, Mods::default(), false);
        assert_eq!(
            session
                .tree
                .node_computed_style(host_id)
                .unwrap()
                .paint
                .background_color,
            expected_bg
        );
    }
}
