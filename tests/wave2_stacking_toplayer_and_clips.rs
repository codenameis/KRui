#![forbid(unsafe_code)]

use krui::*;

fn px_at(fb: &[u8], stride_px: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * stride_px + x) * 4) as usize;
    [fb[idx], fb[idx + 1], fb[idx + 2], fb[idx + 3]]
}

fn render_cpu_and_optional_gpu(scene: &PaintScene, check: impl Fn(&str, &[u8], u32)) {
    let mut cpu = TinySkiaRenderer::new(scene.width, scene.height, GpuResourceLimits::default());
    cpu.render_scene(scene);
    check("cpu", cpu.framebuffer_rgba().expect("cpu fb"), scene.width);
    if let Some(mut gpu) =
        WgpuRenderer::try_new(scene.width, scene.height, GpuResourceLimits::default())
    {
        gpu.render_scene(scene);
        check("gpu", gpu.framebuffer_rgba().expect("gpu fb"), scene.width);
    }
}

#[test]
fn non_sc_wrapper_escape_beats_later_sibling_in_paint_and_hit_testing() {
    let pop_hits = Signal::new(0u32);
    let sib_hits = Signal::new(0u32);
    let (ph, sh) = (pop_hits, sib_hits);

    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let on_pop = Command::new(move || ph.update(|n| *n += 1));
        let on_sib = Command::new(move || sh.update(|n| *n += 1));
        krui!(
            r#"
            <style>
                #root { display: flex; flex-direction: column; width: 240px; height: 200px; background: #ffffff; }
                .wrapper { position: relative; width: 240px; height: 40px; }
                #pop { position: absolute; top: 0px; left: 10px; width: 140px; height: 110px; background: #10d040; z-index: 100; }
                #sib { width: 200px; height: 100px; background: #e02020; }
            </style>
            <div id="root">
                <div class="wrapper">
                    <button id="pop" onclick="{on_pop}">Pop</button>
                </div>
                <button id="sib" onclick="{on_sib}">Sib</button>
            </div>
            "#
        )
    });

    let (scene, _) = tree.build_paint_scene(240, 200);
    render_cpu_and_optional_gpu(&scene, |tag, fb, w| {
        let p = px_at(fb, w, 60, 75);
        assert!(
            p[1] > p[0].saturating_add(60),
            "{tag}: hoisted z-index:100 child must paint green above later sibling, got {p:?}"
        );
    });

    assert!(tree.dispatch_click(60.0, 75.0));
    assert_eq!(pop_hits.get(), 1);
    assert_eq!(sib_hits.get(), 0);
}

#[test]
fn negative_z_index_paints_behind_parent_text_and_keeps_button_clickable() {
    let clicks = Signal::new(0u32);
    let c = clicks;

    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let on_btn = Command::new(move || c.update(|n| *n += 1));
        krui!(
            r#"
            <style>
                #stage { width: 200px; height: 100px; padding: 12px; background: #ffffff; }
                #btn { position: relative; z-index: 0; width: 160px; height: 56px; background: #102040; color: #ff2200; font-size: 20px; }
                #under { position: absolute; inset: 0px; z-index: -1; background: #18c858; }
            </style>
            <div id="stage">
                <button id="btn" onclick="{on_btn}">AAAA<span id="under"></span></button>
            </div>
            "#
        )
    });

    let (scene, _) = tree.build_paint_scene(200, 100);
    let bg_box_idx = scene
        .primitives
        .iter()
        .position(|p| matches!(p, DrawPrimitive::StyledBox { background_color, .. } if *background_color == Color::rgb(0x10, 0x20, 0x40)))
        .expect("btn background");
    let under_box_idx = scene
        .primitives
        .iter()
        .position(|p| matches!(p, DrawPrimitive::StyledBox { background_color, .. } if *background_color == Color::rgb(0x18, 0xc8, 0x58)))
        .expect("z-index -1 span");
    let text_idx = scene
        .primitives
        .iter()
        .position(|p| matches!(p, DrawPrimitive::TextGlyphs { glyphs, .. } if !glyphs.is_empty()))
        .expect("button text");
    assert!(bg_box_idx < under_box_idx && under_box_idx < text_idx);

    render_cpu_and_optional_gpu(&scene, |tag, fb, w| {
        let edge_px = px_at(fb, w, 20, 48);
        assert!(
            edge_px[1] > edge_px[0].saturating_add(40),
            "{tag}: under fill visible: {edge_px:?}"
        );
    });
    assert!(tree.dispatch_click(90.0, 40.0));
    assert_eq!(clicks.get(), 1);
}

#[test]
fn dialog_open_top_layer_escapes_clip_traps_backdrop_and_closes_on_escape() {
    let high_hits = Signal::new(0u32);
    let dlg_btn_hits = Signal::new(0u32);
    let (hh, dh) = (high_hits, dlg_btn_hits);

    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let on_high = Command::new(move || hh.update(|n| *n += 1));
        let on_dlg_btn = Command::new(move || dh.update(|n| *n += 1));
        krui!(
            r#"
            <style>
                #stage { position: relative; width: 320px; height: 220px; background: #ffffff; }
                #clipbox { overflow: hidden; width: 110px; height: 90px; }
                #dlg { width: 220px; height: 110px; background: #14c050; border: 0px solid transparent; }
                #dlg::backdrop { background-color: rgba(16, 32, 64, 0.5); }
                #dlg-ok { width: 90px; height: 36px; background: #ffffff; }
                #high { position: absolute; top: 130px; left: 20px; width: 120px; height: 50px; background: #d02020; z-index: 99999; }
            </style>
            <div id="stage">
                <div id="clipbox">
                    <dialog id="dlg" open="true" modal="true">
                        <button id="dlg-ok" onclick="{on_dlg_btn}">OK</button>
                    </dialog>
                </div>
                <button id="high" onclick="{on_high}">High</button>
            </div>
            "#
        )
    });

    let (scene, _) = tree.build_paint_scene(320, 220);
    render_cpu_and_optional_gpu(&scene, |tag, fb, w| {
        let escaped = px_at(fb, w, 175, 50);
        assert!(
            escaped[1] > escaped[0].saturating_add(40),
            "{tag}: dialog must escape clipbox: {escaped:?}"
        );
        let scrim = px_at(fb, w, 260, 190);
        assert!(
            scrim[2] < 245 && scrim[0] < 200,
            "{tag}: backdrop scrim must dim white background: {scrim:?}"
        );
    });

    assert!(!tree.dispatch_click(60.0, 155.0));
    assert_eq!(high_hits.get(), 0);
    assert!(tree.dispatch_click(30.0, 18.0));
    assert_eq!(dlg_btn_hits.get(), 1);

    assert!(tree.dispatch_key_escape());
    let (after_esc, _) = tree.build_paint_scene(320, 220);
    let dlg_box = after_esc.primitives.iter().any(|p| {
        matches!(p, DrawPrimitive::StyledBox { background_color, .. } if *background_color == Color::rgb(0x14, 0xc0, 0x50))
    });
    assert!(!dlg_box, "Escape must close top-layer <dialog open>");
    assert!(tree.dispatch_click(60.0, 155.0));
    assert_eq!(high_hits.get(), 1);
}

#[test]
fn popovertarget_toggles_top_layer_and_light_dismisses_outside() {
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <style>
            #stage { width: 300px; height: 200px; padding: 12px; background: #ffffff; }
            #trig, #outside { width: 100px; height: 36px; background: #334455; }
            #pop { width: 160px; height: 64px; background: #20c890; }
        </style>
        <div id="stage">
            <button id="trig" popovertarget="pop">Toggle</button>
            <button id="outside">Out</button>
            <div id="pop" popover="auto"><span>Menu</span></div>
        </div>
        "#
    ));

    let _ = tree.build_paint_scene(300, 200);
    let pop_id = tree.find_by_id("pop").expect("pop");
    assert_eq!(tree.node_layout(pop_id).map(|r| r.width), Some(0.0));

    let trig_r = tree
        .node_layout(tree.find_by_id("trig").expect("trig"))
        .expect("r");
    assert!(tree.dispatch_click(trig_r.x + 15.0, trig_r.y + 15.0));
    let (open_scene, _) = tree.build_paint_scene(300, 200);
    assert!(tree.node_layout(pop_id).is_some_and(|r| r.width > 100.0));
    render_cpu_and_optional_gpu(&open_scene, |tag, fb, w| {
        let r = tree.node_layout(pop_id).expect("pop open");
        let px = px_at(fb, w, (r.x + 12.0) as u32, (r.y + 12.0) as u32);
        assert!(
            px[1] > px[0].saturating_add(40),
            "{tag}: popover fill must paint: {px:?}"
        );
    });

    let out_r = tree
        .node_layout(tree.find_by_id("outside").expect("outside"))
        .expect("r");
    tree.dispatch_pointer_down(out_r.x + 15.0, out_r.y + 15.0);
    let _ = tree.dispatch_pointer_up();
    let _ = tree.build_paint_scene(300, 200);
    assert_eq!(tree.node_layout(pop_id).map(|r| r.width), Some(0.0));

    assert!(tree.dispatch_click(trig_r.x + 15.0, trig_r.y + 15.0));
    let _ = tree.build_paint_scene(300, 200);
    assert!(tree.node_layout(pop_id).is_some_and(|r| r.width > 100.0));
    assert!(tree.dispatch_key_escape());
    let _ = tree.build_paint_scene(300, 200);
    assert_eq!(tree.node_layout(pop_id).map(|r| r.width), Some(0.0));
}

#[test]
fn details_summary_toggles_body_and_label_forwards_active_and_click() {
    let chk_clicks = Signal::new(0u32);
    let cc = chk_clicks;
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let on_chk = Command::new(move || cc.update(|n| *n += 1));
        krui!(
            r#"
            <style>
                #stage { width: 280px; height: 220px; padding: 10px; background: #ffffff; gap: 8px; }
                #sum { height: 32px; background: #ddeeff; }
                #body { height: 48px; background: #22cc66; }
                #chk { width: 24px; height: 24px; background: #888888; }
                #chk:active { background: #ee3322; }
                #lbl { height: 32px; background: #eeeeee; }
            </style>
            <div id="stage">
                <details id="det">
                    <summary id="sum">Toggle</summary>
                    <div id="body">Content</div>
                </details>
                <input id="chk" type="checkbox" onclick="{on_chk}" />
                <label id="lbl" for="chk">Click me</label>
            </div>
            "#
        )
    });

    let _ = tree.build_paint_scene(280, 220);
    let body_id = tree.find_by_id("body").expect("body");
    assert_eq!(tree.node_layout(body_id).map(|r| r.height), Some(0.0));

    let sum_r = tree
        .node_layout(tree.find_by_id("sum").expect("sum"))
        .expect("sum");
    assert!(tree.dispatch_click(sum_r.x + 12.0, sum_r.y + 12.0));
    let _ = tree.build_paint_scene(280, 220);
    assert!(tree.node_layout(body_id).is_some_and(|r| r.height > 30.0));

    let lbl_r = tree
        .node_layout(tree.find_by_id("lbl").expect("lbl"))
        .expect("lbl");
    let chk_id = tree.find_by_id("chk").expect("chk");
    tree.dispatch_pointer_down(lbl_r.x + 10.0, lbl_r.y + 10.0);
    let (active_scene, _) = tree.build_paint_scene(280, 220);
    let has_active_red = active_scene.primitives.iter().any(|p| {
        matches!(p, DrawPrimitive::StyledBox { background_color, .. } if *background_color == Color::rgb(0xee, 0x33, 0x22))
    });
    assert!(
        has_active_red,
        "<label> press must forward :active style to #chk"
    );
    assert!(tree.dispatch_pointer_up());
    assert_eq!(chk_clicks.get(), 1);
    assert!(tree.node_layout(chk_id).is_some());
}

#[test]
fn nested_rounded_and_rotated_overflow_hidden_clip_cpu_and_gpu_parity() {
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <style>
            #stage { width: 260px; height: 240px; padding: 30px; background: #ffffff; }
            #outer {
                overflow: hidden; width: 180px; height: 180px; border-radius: 36px;
                background: #dddddd;
            }
            #inner {
                overflow: hidden; width: 150px; height: 150px; border-radius: 32px;
                transform: rotate(25deg); background: #222222;
            }
            #fill { width: 200px; height: 200px; background: #10ee40; }
        </style>
        <div id="stage">
            <div id="outer">
                <div id="inner"><button id="fill">Hit</button></div>
            </div>
        </div>
        "#
    ));

    let (scene, _) = tree.build_paint_scene(260, 240);
    let has_local_clip = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::PushLayer {
                local_clip: Some(_),
                ..
            }
        )
    });
    assert!(
        has_local_clip,
        "nested/rotated rounded overflow:hidden must emit PushLayer.local_clip"
    );

    render_cpu_and_optional_gpu(&scene, |tag, fb, w| {
        let corner = px_at(fb, w, 34, 34);
        assert!(
            corner[0] > 230 && corner[2] > 230,
            "{tag}: clipped outer corner must stay white: {corner:?}"
        );
        let center = px_at(fb, w, 110, 110);
        assert!(
            center[1] > center[0].saturating_add(60),
            "{tag}: center green fill must survive: {center:?}"
        );
    });
}

#[test]
fn group_opacity_with_backdrop_blur_samples_blurred_background() {
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <style>
            #stage { position: relative; width: 220px; height: 140px; background: #ffffff; }
            #stripe { width: 110px; height: 140px; background: #ff0000; }
            #glass {
                position: absolute; top: 20px; left: 50px; width: 120px; height: 90px;
                opacity: 0.7; backdrop-filter: blur(8px); background: rgba(255, 255, 255, 0.2);
            }
        </style>
        <div id="stage">
            <div id="stripe"></div>
            <div id="glass"><span>Glass</span></div>
        </div>
        "#
    ));

    let (scene, _) = tree.build_paint_scene(220, 140);
    let blur_pos = scene.primitives.iter().position(|p| {
        matches!(p, DrawPrimitive::StyledBox { backdrop_blur_px, .. } if *backdrop_blur_px > 1.0)
    });
    let layer_pos = scene
        .primitives
        .iter()
        .position(|p| matches!(p, DrawPrimitive::PushLayer { opacity, .. } if *opacity < 0.99));
    assert!(blur_pos.is_some() && layer_pos.is_some() && blur_pos < layer_pos);

    render_cpu_and_optional_gpu(&scene, |tag, fb, w| {
        let seam = px_at(fb, w, 118, 65);
        assert!(
            seam[0] > 200 && seam[1] > 50,
            "{tag}: blurred transition across red seam expected: {seam:?}"
        );
    });
}

#[test]
fn horizontal_scroll_x_shifts_child_hit_testing() {
    let b1_hits = Signal::new(0u32);
    let b2_hits = Signal::new(0u32);
    let (h1, h2) = (b1_hits, b2_hits);

    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let c1 = Command::new(move || h1.update(|n| *n += 1));
        let c2 = Command::new(move || h2.update(|n| *n += 1));
        krui!(
            r#"
            <style>
                #scroller {
                    overflow-x: scroll; overflow-y: hidden;
                    display: flex; flex-direction: row; width: 120px; height: 60px;
                }
                #b1, #b2 { min-width: 100px; height: 50px; background: #4477aa; }
            </style>
            <div id="scroller">
                <button id="b1" onclick="{c1}">First</button>
                <button id="b2" onclick="{c2}">Second</button>
            </div>
            "#
        )
    });

    let _ = tree.build_paint_scene(160, 80);
    let scroller = tree.find_by_id("scroller").expect("scroller");
    assert!(tree.dispatch_click(30.0, 25.0));
    assert_eq!((b1_hits.get(), b2_hits.get()), (1, 0));

    assert!(tree.set_scroll_offsets(scroller, 100.0, 0.0));
    assert!(tree.dispatch_click(30.0, 25.0));
    assert_eq!((b1_hits.get(), b2_hits.get()), (1, 1));
}
