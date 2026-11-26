//! Core interaction regressions for Component Gallery fixes:
//! - Transformed child stacking contexts inside `<button>` / `<Switch>` still activate parent click
//! - `<progress value max>` renders proportional fill bar without literal value text
//! - Overlay `#scrim-backdrop` closes on outside click while modal body shields clicks
//! - `@layer preset` checkables & range inputs keep circular radios and draggable transparent sliders

#![forbid(unsafe_code)]

use krui::*;

fn find_id(tree: &ViewTree, id: &str) -> NodeId {
    tree.find_by_id(id)
        .unwrap_or_else(|| panic!("expected element `#{id}`"))
}

#[test]
fn switch_transformed_thumb_still_toggles_without_pointer_events_none() {
    reset_reactive_runtime();
    let raw_checked = Signal::new(true);
    let builtin_sw = Signal::new(true);
    let (rc, bc) = (raw_checked, builtin_sw);
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let on_raw = Command::new(move || rc.update(|v| *v = !*v));
        let on_sw = Command::new(move || bc.update(|v| *v = !*v));
        krui!(
            r#"
            <style>
                #raw-btn { display: flex; flex-direction: row; align-items: center; justify-content: space-between; width: 220px; height: 40px; padding: 6px 10px; }
                #raw-track { display: flex; flex-direction: row; align-items: center; width: 52px; height: 30px; padding: 2px; background-color: #6750a4; }
                #raw-thumb { width: 22px; height: 22px; background-color: #ffffff; transform: translateX(22px); }
            </style>
            <div style="display: flex; flex-direction: column; gap: 12px; padding: 12px; width: 260px;">
                <button id="raw-btn" type="button" onclick="{on_raw}">
                    <span>Raw</span>
                    <span id="raw-track"><span id="raw-thumb"></span></span>
                </button>
                <Switch id="builtin-sw" label="Dark" checked="{bc}" onclick="{on_sw}" />
            </div>
        "#
        )
    });
    let _ = tree.build_paint_scene(280, 160);
    let thumb_r = tree.node_layout(find_id(&tree, "raw-thumb")).unwrap();
    assert!(tree.dispatch_click(thumb_r.x + 22.0 + 11.0, thumb_r.y + 11.0));
    assert!(
        !raw_checked.get(),
        "clicking transformed non-interactive child inside `<button>` must invoke button onclick"
    );

    let sw_r = tree.node_layout(find_id(&tree, "builtin-sw")).unwrap();
    assert!(tree.dispatch_click(sw_r.x + sw_r.width - 14.0, sw_r.y + sw_r.height * 0.5));
    assert!(
        !builtin_sw.get(),
        "clicking right-side thumb of `<Switch>` must toggle checked state"
    );
}

#[test]
fn progress_element_renders_fill_bar_without_literal_value_text() {
    reset_reactive_runtime();
    let pct = Signal::new(40_i32);
    let pc = pct;
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        krui!(
            r#"
            <style>
                #p1 {
                    width: 220px;
                    height: 10px;
                    border-radius: 999px;
                    background-color: #e6e0e9;
                    border: 1px solid #6750a4;
                    accent-color: #10b981;
                }
            </style>
            <div style="padding: 16px;">
                <progress id="p1" value="{pc}" max="100"></progress>
            </div>
        "#
        )
    });
    let (scene40, _) = tree.build_paint_scene(300, 80);
    let p_rect = tree.node_layout(find_id(&tree, "p1")).unwrap();
    let has_text = scene40
        .primitives
        .iter()
        .any(|p| matches!(p, DrawPrimitive::TextGlyphs { glyphs, .. } if !glyphs.is_empty()));
    assert!(!has_text, "<progress> must not paint literal value text");

    let fill_w_40 = max_inner_fill_width(&scene40, p_rect);
    assert!(
        (fill_w_40 - 218.0 * 0.40).abs() < 1.5,
        "40% progress fill width expected ~87.2px, got {fill_w_40}"
    );

    pct.set(85);
    let (scene85, _) = tree.build_paint_scene(300, 80);
    let fill_w_85 = max_inner_fill_width(&scene85, p_rect);
    assert!(
        (fill_w_85 - 218.0 * 0.85).abs() < 1.5 && fill_w_85 > fill_w_40 + 80.0,
        "85% progress fill width expected ~185.3px, got {fill_w_85}"
    );
}

fn max_inner_fill_width(scene: &PaintScene, outer: LayoutRect) -> f32 {
    scene
        .primitives
        .iter()
        .filter_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if *background_color == Color::rgb(0x10, 0xb9, 0x81)
                && rect.x >= outer.x - 0.5
                && rect.width < outer.width =>
            {
                Some(rect.width)
            }
            _ => None,
        })
        .fold(0.0_f32, f32::max)
}

#[test]
fn scrim_backdrop_closes_on_outside_click_while_modal_padding_shields() {
    reset_reactive_runtime();
    let closed_count = Signal::new(0_i32);
    let cc = closed_count;
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let on_backdrop = Command::new(move || cc.update(|v| *v += 1));
        krui!(
            r#"
            <style>
                #scrim { position: absolute; left: 0px; top: 0px; width: 400px; height: 240px; display: flex; justify-content: center; align-items: center; }
                #backdrop { position: absolute; inset: 0px; z-index: 0; }
                #card { position: relative; z-index: 1; width: 200px; height: 100px; background-color: #ffffff; }
            </style>
            <div id="scrim">
                <div id="backdrop" onclick="{on_backdrop}"></div>
                <div id="card"></div>
            </div>
        "#
        )
    });
    let _ = tree.build_paint_scene(400, 240);
    let card_r = tree.node_layout(find_id(&tree, "card")).unwrap();
    let _ = tree.dispatch_click(
        card_r.x + card_r.width * 0.5,
        card_r.y + card_r.height * 0.5,
    );
    assert_eq!(
        closed_count.get(),
        0,
        "clicking modal blank area must not fall through to scrim backdrop"
    );
    assert!(tree.dispatch_click(12.0, 12.0));
    assert_eq!(
        closed_count.get(),
        1,
        "clicking outside modal card must hit scrim backdrop"
    );
}

#[test]
fn gallery_preset_rules_keep_circular_radios_and_draggable_transparent_range() {
    reset_reactive_runtime();
    let r1 = Signal::new(false);
    let r2 = Signal::new(true);
    let pct = Signal::new(20_i32);
    let (r1c, r2c, pctc) = (r1, r2, pct);
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                @layer preset {
                    input { padding: 8px 14px; background-color: #ffffff; border: 1px solid #79747e; border-radius: 6px; }
                    input[type="radio"] { padding: 4px; border-radius: 999px; accent-color: #6750a4; }
                    input[type="range"] { width: 100%; min-width: 160px; min-height: 28px; padding: 0px; background-color: transparent; border: 0px solid transparent; accent-color: #6750a4; }
                }
            </style>
            <div style="display: flex; flex-direction: column; gap: 12px; width: 240px; padding: 12px;">
                <RadioField id="rf-1" label="One" group="g" value="1" bind_checked="{r1c}" />
                <RadioField id="rf-2" label="Two" group="g" value="2" bind_checked="{r2c}" />
                <SliderField id="sf" label="Vol" min="0" max="100" step="5" bind_value="{pctc}" />
            </div>
        "#
        )
    });
    let _ = tree.build_paint_scene(280, 200);
    let rf1_r = tree.node_layout(find_id(&tree, "rf-1")).unwrap();
    assert!(tree.dispatch_click(rf1_r.x + 10.0, rf1_r.y + 10.0));
    assert!(r1.get() && !r2.get(), "radio group must sync both signals");

    let sf_id = find_id(&tree, "sf");
    let sf_r = tree.node_layout(sf_id).unwrap();
    assert!(tree.dispatch_pointer_down(sf_r.x + 12.0, sf_r.y + sf_r.height - 10.0));
    let _ = tree.dispatch_pointer_drag(sf_r.x + sf_r.width - 12.0, sf_r.y + sf_r.height - 10.0);
    let _ = tree.dispatch_pointer_up();
    assert!(
        pct.get() >= 85,
        "dragging range slider to right edge must update Signal (got {})",
        pct.get()
    );
}

#[test]
fn preset_layer_never_overrides_component_layer_navigation_drawer_selection() {
    reset_reactive_runtime();
    let idx = Signal::new(0_i32);
    let idxc = idx;
    let s0 = Computed::new(move || idxc.get() == 0);
    let s1 = Computed::new(move || idxc.get() == 1);
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let pick_0 = Command::new(move || idxc.set(0));
        let pick_1 = Command::new(move || idxc.set(1));
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                :root {
                    --krui-card: #f7f2fa;
                    --krui-secondary-bg: #e8def8;
                    --krui-secondary-fg: #1d192b;
                    --krui-text-secondary: #49454f;
                    --krui-nav-indicator-radius: 999px;
                    --krui-sheet-radius: 28px;
                    --krui-pill-radius: 999px;
                    --krui-badge-bg: #b3261e;
                    --krui-badge-fg: #ffffff;
                    --krui-accent: #6750a4;
                    --krui-accent-fg: #ffffff;
                    --krui-accent-border: #6750a4;
                    --krui-btn-radius: 999px;
                    --krui-border-width: 1px;
                }
                @layer preset {
                    button {
                        background-color: var(--krui-accent);
                        color: var(--krui-accent-fg);
                        border: 1px solid var(--krui-accent-border);
                        padding: 8px 18px;
                    }
                }
            </style>
            <NavigationDrawer id="nd" title="Explore">
                <NavigationDrawerItem id="nd-0" label="Root" selected="{s0}" onclick="{pick_0}" />
                <NavigationDrawerItem id="nd-1" label="Packs" selected="{s1}" onclick="{pick_1}" />
            </NavigationDrawer>
        "#
        )
    });
    let _ = tree.build_paint_scene(360, 260);
    let bg0 = tree
        .node_effective_paint(find_id(&tree, "nd-0"))
        .unwrap()
        .background_color;
    let bg1 = tree
        .node_effective_paint(find_id(&tree, "nd-1"))
        .unwrap()
        .background_color;
    assert_eq!(bg0, Color::parse_css("#e8def8").unwrap());
    assert_eq!(
        bg1.a, 0,
        "unselected drawer item must stay transparent, got {bg1:?}"
    );

    let r1 = tree.node_layout(find_id(&tree, "nd-1")).unwrap();
    assert!(tree.dispatch_click(r1.x + r1.width * 0.5, r1.y + r1.height * 0.5));
    let _ = tree.build_paint_scene(360, 260);
    let _ = tree.tick_transitions(0.25);
    let _ = tree.build_paint_scene(360, 260);
    assert_eq!(idx.get(), 1);
    let bg0_after = tree
        .node_effective_paint(find_id(&tree, "nd-0"))
        .unwrap()
        .background_color;
    let bg1_after = tree
        .node_effective_paint(find_id(&tree, "nd-1"))
        .unwrap()
        .background_color;
    assert_eq!(bg0_after.a, 0);
    assert_eq!(bg1_after, Color::parse_css("#e8def8").unwrap());
}

fn build_overlay_motion_tree() -> (ViewTree, Signal<bool>, Signal<i32>) {
    reset_reactive_runtime();
    let open = Signal::new(false);
    let clicks = Signal::new(0_i32);
    let (open_c, clicks_c) = (open, clicks);
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let on_under = Command::new(move || clicks_c.update(|v| *v += 1));
        let on_close = Command::new(move || open_c.set(false));
        krui!(
            r#"
            <style>
                #root { position: relative; width: 400px; height: 260px; }
                #vp { width: 400px; height: 260px; overflow-y: scroll; padding: 16px; display: flex; flex-direction: column; gap: 16px; }
                #under-btn { width: 140px; height: 40px; background-color: #6750a4; }
                #spacer { width: 100%; height: 480px; flex-shrink: 0; }
                #portal { position: absolute; inset: 0px; pointer-events: none; overflow: hidden; }
                #scrim {
                    position: absolute; inset: 0px; display: flex; justify-content: center; align-items: center;
                    background-color: rgba(0, 0, 0, 0.4); opacity: 0; pointer-events: none;
                    transition: opacity 200ms cubic-bezier(0.3, 0, 0.8, 0.15);
                }
                #scrim[data-open="true"] {
                    opacity: 1; pointer-events: auto;
                    transition: opacity 250ms cubic-bezier(0.05, 0.7, 0.1, 1);
                }
            </style>
            <div id="root">
                <div id="vp"><button id="under-btn" onclick="{on_under}">Under</button><div id="spacer"></div></div>
                <div id="portal">
                    <div id="scrim" data-open="{open_c}">
                        <Dialog id="dlg" open="{open_c}" title="Motion" confirm_label="Close" dismiss_label="" on_confirm="{on_close}"><span>Body</span></Dialog>
                    </div>
                </div>
            </div>
        "#
        )
    });
    (tree, open, clicks)
}

#[test]
fn overlay_open_and_close_transitions_run_zero_relayout_and_allow_passthrough() {
    let (mut tree, open, under_clicks) = build_overlay_motion_tree();
    let _ = tree.build_paint_scene(400, 260);
    let under_r = tree.node_layout(find_id(&tree, "under-btn")).unwrap();
    let vp_id = find_id(&tree, "vp");
    assert!(tree.dispatch_scroll(200.0, 180.0, -60.0));
    assert!(tree.scroll_offset_y(vp_id).unwrap_or(0.0) > 10.0);
    let _ = tree.set_scroll_offsets(vp_id, 0.0, 0.0);
    assert!(tree.dispatch_click(under_r.x + 12.0, under_r.y + 12.0));
    assert_eq!(under_clicks.get(), 1);
    let t1 = tree.dispatch_key_tab(false);
    let t2 = tree.dispatch_key_tab(false);
    assert_eq!(t1, Some(find_id(&tree, "under-btn")));
    assert_eq!(t2, Some(find_id(&tree, "under-btn")));

    let rel_open = tree.relayout_passes();
    open.set(true);
    let _ = tree.tick_transitions(0.08);
    let _ = tree.build_paint_scene(400, 260);
    assert_eq!(tree.relayout_passes() - rel_open, 0);
    assert!(tree.active_transition_count() >= 2);
    let mid_dlg = tree.node_effective_paint(find_id(&tree, "dlg")).unwrap();
    assert!(mid_dlg.opacity > 0.05 && mid_dlg.opacity < 0.98);
    let _ = tree.tick_transitions(0.45);
    assert_eq!(tree.active_transition_count(), 0);

    let rel_close = tree.relayout_passes();
    open.set(false);
    let _ = tree.tick_transitions(0.04);
    let _ = tree.build_paint_scene(400, 260);
    assert_eq!(tree.relayout_passes() - rel_close, 0);
    assert!(tree.active_transition_count() >= 1);
    assert!(tree.dispatch_click(under_r.x + 12.0, under_r.y + 12.0));
    assert_eq!(under_clicks.get(), 2);
    let _ = tree.tick_transitions(0.35);
    assert_eq!(tree.active_transition_count(), 0);
}
