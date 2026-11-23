//! Wave 1 adversarial tests: modal/opaque Z-order pointer shielding, `selection_owner` vs
//! `focused_id` (`Tab` anchor continuation), `:focus` vs `:focus-visible`, pointer capture
//! (`:active` suspension, edge autoscroll, `CursorLeft`, device bezel coords), and OS IME reset.

#![forbid(unsafe_code)]

use krui::{krui, Color, Command, DrawPrimitive, ImeEvent, Signal, ViewTree};
use krui_winit::{sync_ime_host_with_sink, RecordedImeOp, RecordedImeSink, WinitImeStateCache};
use std::cell::Cell;
use std::rc::Rc;

fn box_with_width(scene: &krui::PaintScene, w: f32) -> Option<(Color, Color, f32)> {
    scene.primitives.iter().find_map(|p| match p {
        DrawPrimitive::StyledBox {
            rect,
            background_color,
            border_color,
            border_width,
            ..
        } if (rect.width - w).abs() < 1.5 => {
            Some((*background_color, *border_color, *border_width))
        }
        _ => None,
    })
}

#[test]
fn modal_backdrop_shields_clicks_while_button_child_span_still_activates() {
    let behind_clicks = Rc::new(Cell::new(0_u32));
    let fg_clicks = Rc::new(Cell::new(0_u32));
    let show_modal = Signal::new(true);
    let on_behind = {
        let c = behind_clicks.clone();
        Command::new(move || c.set(c.get() + 1))
    };
    let on_fg = {
        let c = fg_clicks.clone();
        Command::new(move || c.set(c.get() + 1))
    };
    let view = krui!(
        r#"
        <div style="width: 400px; height: 240px;">
            <button id="behind-btn" onclick="{on_behind}" style="position: absolute; left: 20px; top: 20px; width: 120px; height: 40px;">Behind</button>
            <input id="behind-inp" value="bg" style="position: absolute; left: 20px; top: 80px; width: 160px; height: 36px;" />
            <if test="{show_modal}">
                <div id="backdrop" style="position: absolute; left: 0px; top: 0px; width: 400px; height: 240px; background: rgba(0, 0, 0, 0.7);">
                    <button id="fg-btn" onclick="{on_fg}" style="position: absolute; left: 220px; top: 40px; width: 140px; height: 44px;">
                        <span style="background: #ff00ff; width: 60px; height: 20px;">Badge</span>
                    </button>
                </div>
            </if>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let _ = tree.build_paint_scene(400, 240);

    let _ = tree.dispatch_pointer_down(60.0, 35.0);
    let _ = tree.dispatch_pointer_up_at(60.0, 35.0);
    assert_eq!(
        behind_clicks.get(),
        0,
        "modal backdrop must block behind button"
    );

    let _ = tree.dispatch_pointer_down(80.0, 95.0);
    let _ = tree.dispatch_pointer_up_at(80.0, 95.0);
    assert_eq!(
        tree.focused_node_id(),
        None,
        "modal backdrop must block behind input"
    );

    let _ = tree.dispatch_pointer_down(250.0, 60.0);
    let _ = tree.dispatch_pointer_up_at(250.0, 60.0);
    assert_eq!(
        fg_clicks.get(),
        1,
        "opaque child span inside button must still click button"
    );
}

#[test]
fn selectable_paragraph_uses_selection_owner_and_preserves_tab_anchor() {
    let view = krui!(
        r#"
        <div style="width: 360px; height: 200px; flex-direction: column; gap: 8px;">
            <input id="first" value="top" style="width: 160px; height: 32px;" />
            <p id="mid" style="width: 240px; height: 24px; user-select: text;">Selectable middle copy</p>
            <button id="after" style="width: 140px; height: 32px;">After</button>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let _ = tree.build_paint_scene(360, 200);
    let first = tree.find_by_id("first").unwrap();
    let mid = tree.find_by_id("mid").unwrap();
    let after = tree.find_by_id("after").unwrap();
    let mid_r = tree.node_layout(mid).unwrap();

    assert!(tree.dispatch_pointer_down(mid_r.x + 12.0, mid_r.y + 8.0));
    let _ = tree.dispatch_pointer_up_at(mid_r.x + 12.0, mid_r.y + 8.0);
    assert_eq!(tree.focused_node_id(), None);
    assert_eq!(tree.selection_owner(), Some(mid));
    assert_eq!(tree.dispatch_key_tab(false), Some(after));

    let _ = tree.dispatch_pointer_down(mid_r.x + 12.0, mid_r.y + 8.0);
    let _ = tree.dispatch_pointer_up_at(mid_r.x + 12.0, mid_r.y + 8.0);
    assert_eq!(tree.dispatch_key_tab(true), Some(first));
}

#[test]
fn focus_visible_separates_pointer_clicks_from_keyboard_tab_and_inputs() {
    let view = krui!(
        r#"
        <style>
            button { height: 36px; border: 2px solid #223344; }
            #btn-a { width: 128px; }
            #btn-b { width: 134px; }
            button:focus-visible { border-color: #00ff00; }
            input { width: 150px; height: 36px; border: 2px solid #223344; }
            input:focus-visible { border-color: #ffaa00; }
        </style>
        <div style="display: flex; flex-direction: row; width: 480px; height: 120px; gap: 10px;">
            <button id="btn-a">A</button>
            <button id="btn-b">B</button>
            <input id="inp" value="hello" />
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let _ = tree.build_paint_scene(480, 120);
    let btn_a = tree.find_by_id("btn-a").unwrap();
    let btn_b = tree.find_by_id("btn-b").unwrap();
    let inp = tree.find_by_id("inp").unwrap();

    let ra = tree.node_layout(btn_a).unwrap();
    let _ = tree.dispatch_pointer_down(ra.x + 10.0, ra.y + 10.0);
    let _ = tree.dispatch_pointer_up_at(ra.x + 10.0, ra.y + 10.0);
    let _ = tree.dispatch_pointer_leave();
    assert_eq!(tree.focused_node_id(), Some(btn_a));
    assert!(!tree.is_focus_visible(btn_a));
    let (scene_ptr, _) = tree.build_paint_scene(480, 120);
    assert_ne!(
        box_with_width(&scene_ptr, 128.0).unwrap().1,
        Color::rgb(0, 255, 0)
    );

    assert_eq!(tree.dispatch_key_tab(false), Some(btn_b));
    assert!(tree.is_focus_visible(btn_b));
    let (scene_tab, _) = tree.build_paint_scene(480, 120);
    assert_eq!(
        box_with_width(&scene_tab, 134.0).unwrap().1,
        Color::rgb(0, 255, 0)
    );

    let ri = tree.node_layout(inp).unwrap();
    let _ = tree.dispatch_pointer_down(ri.x + 10.0, ri.y + 10.0);
    let _ = tree.dispatch_pointer_up_at(ri.x + 10.0, ri.y + 10.0);
    assert!(tree.is_focus_visible(inp));
    let (scene_inp, _) = tree.build_paint_scene(480, 120);
    assert_eq!(
        box_with_width(&scene_inp, 150.0).unwrap().1,
        Color::rgb(0xff, 0xaa, 0x00)
    );
}

#[test]
fn pointer_capture_drags_outside_input_and_autoscrolls_without_hovering_neighbors() {
    let adj_clicks = Rc::new(Cell::new(0_u32));
    let on_adj = {
        let c = adj_clicks.clone();
        Command::new(move || c.set(c.get() + 1))
    };
    let view = krui!(
        r#"
        <style>
            #adj { width: 120px; height: 36px; background: #112233; }
            #adj:hover { background: #ff00ff; }
        </style>
        <div style="display: flex; flex-direction: row; width: 480px; height: 120px; gap: 16px;">
            <input id="inp" style="width: 140px; height: 36px; padding: 6px 10px;" value="0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ" />
            <button id="adj" onclick="{on_adj}">Neighbor</button>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let _ = tree.build_paint_scene(480, 120);
    let inp = tree.find_by_id("inp").unwrap();
    let adj_r = tree.node_layout(tree.find_by_id("adj").unwrap()).unwrap();

    // Reset caret & horizontal scroll to start so right-edge autoscroll has room to advance.
    assert!(tree.dispatch_pointer_down(40.0, 18.0));
    let _ = tree.dispatch_pointer_up_at(40.0, 18.0);
    assert!(tree.dispatch_cursor_home());
    let _ = tree.build_paint_scene(480, 120);

    assert!(tree.dispatch_pointer_down(40.0, 18.0));
    assert_eq!(tree.captured_pointer_node(), Some(inp));
    assert!(tree.is_text_drag_selecting());
    let _ = tree.dispatch_pointer_drag(adj_r.x + 20.0, adj_r.y + 15.0);
    let _ = tree.dispatch_pointer_move(adj_r.x + 20.0, adj_r.y + 15.0);
    assert!(tree.tick_pointer_drag_autoscroll(1.0 / 30.0));
    let (scene_during, _) = tree.build_paint_scene(480, 120);
    assert_eq!(
        box_with_width(&scene_during, 120.0).unwrap().0,
        Color::rgb(0x11, 0x22, 0x33)
    );

    let _ = tree.dispatch_pointer_drag(-50.0, 18.0);
    assert!(tree.tick_pointer_drag_autoscroll(1.0 / 30.0));
    assert!(tree.selected_text().is_some());
    assert!(tree.dispatch_pointer_up_at(adj_r.x + 20.0, adj_r.y + 15.0));
    assert_eq!(adj_clicks.get(), 0);
    assert_eq!(tree.captured_pointer_node(), None);
}

#[test]
fn button_active_suspends_outside_capture_and_restores_on_reentry() {
    let clicks = Rc::new(Cell::new(0_u32));
    let on_click = {
        let c = clicks.clone();
        Command::new(move || c.set(c.get() + 1))
    };
    let view = krui!(
        r#"
        <style>
            #btn { width: 120px; height: 40px; background-color: #112233; }
            #btn:hover { background-color: #223344; }
            #btn:active { background-color: #ef4444; }
        </style>
        <div style="width: 300px; height: 120px;">
            <button id="btn" onclick="{on_click}">Hold</button>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let _ = tree.build_paint_scene(300, 120);
    let btn = tree.find_by_id("btn").unwrap();
    let r = tree.node_layout(btn).unwrap();

    assert!(tree.dispatch_pointer_down(r.x + 20.0, r.y + 20.0));
    let (s_in, _) = tree.build_paint_scene(300, 120);
    assert_eq!(
        box_with_width(&s_in, 120.0).unwrap().0,
        Color::rgb(0xef, 0x44, 0x44)
    );

    let _ = tree.dispatch_pointer_move(r.x + r.width + 2.0, r.y + 20.0);
    assert_eq!(tree.captured_pointer_node(), Some(btn));
    let (s_out, _) = tree.build_paint_scene(300, 120);
    assert_eq!(
        box_with_width(&s_out, 120.0).unwrap().0,
        Color::rgb(0x11, 0x22, 0x33)
    );

    let _ = tree.dispatch_pointer_move(r.x + 20.0, r.y + 20.0);
    let (s_back, _) = tree.build_paint_scene(300, 120);
    assert_eq!(
        box_with_width(&s_back, 120.0).unwrap().0,
        Color::rgb(0xef, 0x44, 0x44)
    );

    let _ = tree.dispatch_pointer_move(r.x + r.width + 10.0, r.y + 20.0);
    let _ = tree.dispatch_pointer_up_at(r.x + r.width + 10.0, r.y + 20.0);
    assert_eq!(clicks.get(), 0);
    assert_eq!(tree.captured_pointer_node(), None);
}

#[test]
fn cursor_left_clears_hover_and_device_bezel_unclamped_maps_coordinates() {
    let view = krui!(
        r#"
        <style>
            #btn { width: 120px; height: 40px; background: #112233; }
            #btn:hover { background: #33aa55; }
        </style>
        <div style="width: 240px; height: 100px;">
            <button id="btn">Hover</button>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let _ = tree.build_paint_scene(240, 100);
    let _ = tree.dispatch_pointer_move(20.0, 20.0);
    assert!(tree.dispatch_pointer_leave());
    let (s, _) = tree.build_paint_scene(240, 100);
    assert_eq!(
        box_with_width(&s, 120.0).unwrap().0,
        Color::rgb(0x11, 0x22, 0x33)
    );

    #[cfg(feature = "preview")]
    {
        let frame = krui_preview::DeviceFramePreset::Iphone15;
        assert_eq!(frame.map_window_to_screen_point(5.0, 10.0), None);
        let (ux, uy) = frame.map_window_to_screen_point_unclamped(5.0, 10.0);
        assert!(ux < 0.0 && uy < 0.0);
    }
}

#[test]
fn input_a_to_b_click_resets_os_ime_while_composing_preserves_target() {
    let view = krui!(
        r#"
        <div style="width: 360px; height: 140px; gap: 12px;">
            <input id="a" value="alpha" style="width: 140px; height: 36px;" />
            <input id="b" value="beta" style="width: 140px; height: 36px;" />
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let _ = tree.build_paint_scene(360, 140);
    let ra = tree.node_layout(tree.find_by_id("a").unwrap()).unwrap();
    let rb = tree.node_layout(tree.find_by_id("b").unwrap()).unwrap();

    let mut cache = WinitImeStateCache::default();
    let mut sink = RecordedImeSink::default();
    assert!(tree.dispatch_pointer_down(ra.x + 10.0, ra.y + 10.0));
    let _ = tree.dispatch_pointer_up_at(ra.x + 10.0, ra.y + 10.0);
    sync_ime_host_with_sink(&mut sink, &tree, &mut cache);
    assert_eq!(cache.reset_count, 0);

    sink.ops.clear();
    assert!(tree.dispatch_pointer_down(rb.x + 10.0, rb.y + 10.0));
    let _ = tree.dispatch_pointer_up_at(rb.x + 10.0, rb.y + 10.0);
    sync_ime_host_with_sink(&mut sink, &tree, &mut cache);
    assert_eq!(cache.reset_count, 1);
    assert_eq!(sink.ops[0], RecordedImeOp::SetAllowed(false));
    assert_eq!(sink.ops[1], RecordedImeOp::SetAllowed(true));

    assert!(tree.dispatch_ime_event(ImeEvent::Preedit {
        text: "ㅎ".to_string(),
        cursor_offset: Some((3, 3)),
    }));
    sync_ime_host_with_sink(&mut sink, &tree, &mut cache);
    assert_eq!(cache.reset_count, 1);
}
