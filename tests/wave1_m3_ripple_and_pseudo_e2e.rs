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

fn find_radial_near_box(scene: &PaintScene, r: LayoutRect) -> Option<(f32, f32, f32, f32)> {
    scene.primitives.iter().find_map(|p| match p {
        DrawPrimitive::StyledBox {
            rect,
            radial_gradient: Some(rg),
            ..
        } if (rect.x - r.x).abs() < 2.0 && (rect.y - r.y).abs() < 2.0 => {
            Some((rg.center_dx, rg.center_dy, rg.radius_x, rg.radius_y))
        }
        _ => None,
    })
}

#[allow(deprecated)]
fn mount_ripple_harness() -> ViewTree {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                .stage { display: flex; flex-direction: column; gap: 16px; padding: 20px; background-color: #ffffff; }
                .rip-btn, .hover-btn, .author-btn, .unshadowed-author-btn {
                    position: relative; overflow: hidden;
                    width: 200px; height: 56px; border-radius: 28px;
                    background-color: #201a30; color: #ff5522; border: 0px solid transparent;
                }
                .rip-btn::before {
                    content: ""; position: absolute; inset: 0px; opacity: 0;
                    background-image: radial-gradient(circle at var(--krui-pointer-x, 50%) var(--krui-pointer-y, 50%), currentColor 18%, transparent 19%);
                    background-size: 1000%;
                    transition: background-size 400ms linear, opacity 480ms linear;
                }
                .rip-btn:active::before {
                    background-size: 60%; opacity: 0.85; transition: 0s;
                }
                .hover-btn::before {
                    content: ""; position: absolute; inset: 0px; opacity: 0.70;
                    background-image: radial-gradient(circle at var(--krui-hover-x, 50%) var(--krui-hover-y, 50%), currentColor 18%, transparent 19%);
                    background-size: 80%;
                }
                .author-btn { --_x: 12px; }
                .author-btn::before {
                    content: ""; position: absolute; inset: 0px; opacity: 0.80;
                    background-image: radial-gradient(circle at var(--_x, 50%) var(--_y, 50%), currentColor 18%, transparent 19%);
                    background-size: 80%;
                }
                .unshadowed-author-btn::before {
                    content: ""; position: absolute; inset: 0px; opacity: 0.80;
                    background-image: radial-gradient(circle at var(--_x, 25%) var(--_y, 25%), currentColor 18%, transparent 19%);
                    background-size: 100%;
                }
            </style>
            <div class="stage">
                <button id="rip" class="rip-btn">Ripple</button>
                <button id="hov" class="hover-btn">Hover</button>
                <button id="auth" class="author-btn">Author</button>
                <button id="unsh" class="unshadowed-author-btn">Unsh</button>
            </div>
            "#
        )
    });
    tree
}

#[test]
fn offcenter_pointerdown_latches_ripple_origin_and_post_release_hover_does_not_move_it() {
    let mut tree = mount_ripple_harness();
    let _ = tree.build_paint_scene(320, 340);

    let rip_r = tree.node_layout(tree.find_by_id("rip").unwrap()).unwrap();
    let _ = tree.dispatch_pointer_down(rip_r.x + 24.0, rip_r.y + 28.0);
    let (left_scene, _) = tree.build_paint_scene(320, 340);
    let (left_dx, ..) = find_radial_near_box(&left_scene, rip_r).expect("left ripple primitive");
    assert!(left_dx < -50.0, "left press dx={left_dx}");
    render_cpu_and_optional_gpu(&left_scene, |label, fb, w| {
        let lx = px_at(fb, w, (rip_r.x + 24.0) as u32, (rip_r.y + 28.0) as u32);
        let rx = px_at(fb, w, (rip_r.x + 176.0) as u32, (rip_r.y + 28.0) as u32);
        assert!(
            lx[0] > rx[0] + 40,
            "[{label}] left ripple {lx:?} vs far side {rx:?}"
        );
    });

    let _ = tree.dispatch_pointer_up();
    let _ = tree.dispatch_pointer_move(rip_r.x + 180.0, rip_r.y + 28.0);
    let _ = tree.tick_transitions(0.05);
    let (release_scene, relayout) = tree.build_paint_scene(320, 340);
    assert!(!relayout, "release fade + hover move must not relayout");
    let (latched_dx, ..) =
        find_radial_near_box(&release_scene, rip_r).expect("latched ripple active");
    assert!(latched_dx < -50.0, "latched dx drifted to {latched_dx}");

    let hov_r = tree.node_layout(tree.find_by_id("hov").unwrap()).unwrap();
    let _ = tree.dispatch_pointer_move(hov_r.x + 180.0, hov_r.y + 28.0);
    let (hov_scene, _) = tree.build_paint_scene(320, 340);
    let (hov_dx, ..) = find_radial_near_box(&hov_scene, hov_r).expect("hover radial");
    assert!(hov_dx > 50.0, "expected live hover dx > 50, got {hov_dx}");

    let auth_r = tree.node_layout(tree.find_by_id("auth").unwrap()).unwrap();
    let _ = tree.dispatch_pointer_down(auth_r.x + 170.0, auth_r.y + 28.0);
    let (auth_scene, _) = tree.build_paint_scene(320, 340);
    let (auth_dx, ..) = find_radial_near_box(&auth_scene, auth_r).expect("author var radial");
    assert!(
        (auth_dx - (-88.0)).abs() < 2.0,
        "author --_x must stay 12px ({auth_dx})"
    );

    let unsh_id = tree.find_by_id("unsh").expect("unsh");
    let unsh_r = tree.node_layout(unsh_id).expect("unsh rect");
    let _ = tree.dispatch_pointer_down(unsh_r.x + 170.0, unsh_r.y + 28.0);
    let (unsh_scene, _) = tree.build_paint_scene(320, 340);
    let (unsh_dx, ..) = find_radial_near_box(&unsh_scene, unsh_r).expect("unshadowed --_x");
    assert!(
        (unsh_dx - (-50.0)).abs() < 2.0,
        "unshadowed author --_x must keep 25% fallback instead of pointer hijack ({unsh_dx})"
    );
}

#[test]
fn active_zero_transition_snaps_and_releases_with_zero_relayouts_over_120_frames() {
    let mut tree = mount_ripple_harness();
    let _ = tree.build_paint_scene(320, 340);
    let rip_r = tree.node_layout(tree.find_by_id("rip").unwrap()).unwrap();

    let baseline_relayouts = tree.relayout_passes();
    let _ = tree.dispatch_pointer_down(rip_r.x + 28.0, rip_r.y + 28.0);
    let _ = tree.dispatch_pointer_up();
    assert_eq!(tree.relayout_passes(), baseline_relayouts);

    let _ = tree.tick_transitions(1.0 / 120.0);
    let (early_scene, r0) = tree.build_paint_scene(320, 340);
    assert!(!r0);
    let (_, _, early_rx, _) = find_radial_near_box(&early_scene, rip_r).expect("early ripple");

    let mut mid_rx = early_rx;
    for frame in 1..120 {
        let _ = tree.tick_transitions(1.0 / 120.0);
        let (scene, did_relayout) = tree.build_paint_scene(320, 340);
        assert!(!did_relayout, "frame {frame} triggered relayout");
        assert_eq!(tree.relayout_passes(), baseline_relayouts);
        if frame == 25 {
            mid_rx = find_radial_near_box(&scene, rip_r).expect("mid ripple").2;
        }
    }
    assert!(
        mid_rx > early_rx * 2.0,
        "expected radial ripple radius growth: {early_rx} -> {mid_rx}"
    );
    assert!(
        !tree.has_active_transitions(),
        "transitions should settle cleanly after 1s"
    );
}

#[test]
fn elevated_button_renders_outer_box_shadow_and_clipped_ripple_on_cpu_and_gpu() {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                .wrap { padding: 36px; background-color: #fef7ff; }
                .elev-btn {
                    position: relative; overflow: hidden;
                    width: 200px; height: 56px; border-radius: 24px;
                    background-color: #ffffff; color: #6750a4; border: 0px solid transparent;
                    box-shadow: 0px 8px 12px 2px rgba(0, 0, 0, 0.50);
                }
                .elev-btn::before {
                    content: ""; position: absolute; inset: 0px; opacity: 0;
                    background-image: radial-gradient(circle at var(--krui-pointer-x, 50%) var(--krui-pointer-y, 50%), currentColor 22%, transparent 23%);
                    background-size: 60%;
                }
                .elev-btn:active::before { opacity: 0.85; }
            </style>
            <div class="wrap"><button id="elev" class="elev-btn">Elevated</button></div>
            "#
        )
    });
    let _ = tree.build_paint_scene(300, 160);
    let btn_r = tree.node_layout(tree.find_by_id("elev").unwrap()).unwrap();
    let _ = tree.dispatch_pointer_down(btn_r.x + 28.0, btn_r.y + 28.0);
    let (scene, _) = tree.build_paint_scene(300, 160);

    assert!(
        find_radial_near_box(&scene, btn_r).is_some(),
        "elevated ::before radial must emit"
    );
    render_cpu_and_optional_gpu(&scene, |label, fb, w| {
        let shadow_px = px_at(
            fb,
            w,
            (btn_r.x + 100.0) as u32,
            (btn_r.y + btn_r.height + 6.0) as u32,
        );
        assert!(
            shadow_px[1] < 220,
            "[{label}] expected outer drop shadow, got {shadow_px:?}"
        );
        let left_rip = px_at(fb, w, (btn_r.x + 28.0) as u32, (btn_r.y + 28.0) as u32);
        let right_white = px_at(fb, w, (btn_r.x + 176.0) as u32, (btn_r.y + 28.0) as u32);
        assert!(
            right_white[1] > left_rip[1] + 50,
            "[{label}] ripple {left_rip:?} vs bg {right_white:?}"
        );
        let outer_corner = px_at(fb, w, (btn_r.x + 2.0) as u32, (btn_r.y + 2.0) as u32);
        assert!(
            outer_corner[0] > 235 && outer_corner[2] > 235,
            "[{label}] corner clip leaked: {outer_corner:?}"
        );
    });
}

#[controller]
#[derive(Clone)]
struct ClipClickCtrl {
    count: Signal<i32>,
}

impl ClipClickCtrl {
    fn new() -> Self {
        Self {
            count: Signal::new(0),
        }
    }
    fn hit(&self) {
        self.count.update(|n| *n += 1);
    }
}

#[test]
fn asymmetric_four_corner_padding_box_clip_and_hit_test_isolation() {
    let ctrl = ClipClickCtrl::new();
    let ctrl_view = ctrl.clone();
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let ctrl = ctrl_view;
        krui!(
            r#"
            <style>
                .root { padding: 20px; background-color: #101018; }
                .card {
                    width: 200px; height: 120px; border-radius: 36px 4px 36px 4px;
                    border: 6px solid #224466; background-color: #101018; overflow: hidden;
                }
                .fill { width: 100%; height: 100%; background-color: #ee3344; }
            </style>
            <div class="root">
                <div id="card" class="card">
                    <div id="fill" class="fill" onclick="{ctrl.hit()}"></div>
                </div>
            </div>
            "#
        )
    });
    let (scene, _) = tree.build_paint_scene(260, 180);
    let cr = tree.node_layout(tree.find_by_id("card").unwrap()).unwrap();
    render_cpu_and_optional_gpu(&scene, |label, fb, w| {
        let tl_clipped = px_at(fb, w, (cr.x + 8.0) as u32, (cr.y + 8.0) as u32);
        assert!(
            tl_clipped[0] < 60,
            "[{label}] TL child should clip outside 36px corner: {tl_clipped:?}"
        );
        let tr_visible = px_at(fb, w, (cr.x + cr.width - 10.0) as u32, (cr.y + 10.0) as u32);
        assert!(
            tr_visible[0] > 200 && tr_visible[1] < 80,
            "[{label}] TR sharp corner clipped: {tr_visible:?}"
        );
        let border_px = px_at(fb, w, (cr.x + 3.0) as u32, (cr.y + 50.0) as u32);
        assert!(
            border_px[2] > 80 && border_px[0] < 60,
            "[{label}] border overwritten: {border_px:?}"
        );
    });
    let _ = tree.dispatch_click(cr.x + 1.0, cr.y + 1.0);
    assert_eq!(
        ctrl.count.get_untracked(),
        0,
        "clipped outer TL corner must not hit inner fill"
    );
    let _ = tree.dispatch_click(cr.x + 100.0, cr.y + 60.0);
    assert_eq!(
        ctrl.count.get_untracked(),
        1,
        "interior hit must fire onclick"
    );
}

#[cfg(feature = "preview")]
#[test]
fn cascade_layer_property_and_unstyled_mode_work_end_to_end() {
    use krui::dev_hot_reload::{
        compile_krui_source_with_options, mount_compiled_document, CompileOptions,
        CompileWarningKind,
    };

    let doc = compile_krui_source_with_options(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            @layer base, utilities;
            @property --tw-ring { syntax: "*"; inherits: false; initial-value: #112233; }
            @layer base {
                @media (min-width: 100px) { #box { background-color: #aa1111; } }
            }
            @layer utilities { #box { background-color: #11aa22; } }
            #parent { --tw-ring: #ff00ff; }
            #child { width: 40px; height: 20px; background-color: var(--tw-ring); }
            .flex { display: flex; }
            .flex-col { flex-direction: column; }
            md-icon { color: #12aa34; }
        </style>
        <div id="parent">
            <div id="box" class="flex-col flex"><div id="child"></div></div>
            <span id="inl">txt</span>
            <md-icon id="cust">x</md-icon>
        </div>
        "#,
        CompileOptions::default(),
    )
    .expect("unstyled + layer + @property + custom tag compile");

    assert!(doc
        .warnings
        .iter()
        .any(|w| w.kind == CompileWarningKind::UnknownHtmlElement && w.node_tag == "md-icon"));

    let mut tree = ViewTree::new();
    mount_compiled_document(&mut tree, &doc);
    let _ = tree.build_paint_scene(400, 300);
    assert_eq!(
        tree.node_effective_paint(tree.find_by_id("box").unwrap())
            .unwrap()
            .background_color,
        Color::rgb(0x11, 0xaa, 0x22)
    );
    assert_eq!(
        tree.node_effective_paint(tree.find_by_id("child").unwrap())
            .unwrap()
            .background_color,
        Color::rgb(0x11, 0x22, 0x33)
    );
    assert_eq!(
        tree.node_effective_paint(tree.find_by_id("cust").unwrap())
            .unwrap()
            .text_color,
        Color::rgb(0x12, 0xaa, 0x34)
    );

    let err_doc =
        compile_krui_source_with_options("<MissingComponent />", CompileOptions::default());
    assert!(
        err_doc.is_err(),
        "unknown PascalCase component must remain CompileError"
    );
}

#[cfg(feature = "preview")]
#[test]
fn unstyled_ua_display_flex_row_default_and_flex_col_order_independence() {
    use krui::dev_hot_reload::{
        compile_krui_source_with_options, mount_compiled_document, CompileOptions,
    };

    let doc = compile_krui_source_with_options(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            .flex { display: flex; }
            .flex-col { flex-direction: column; }
            .kid { width: 40px; height: 20px; }
        </style>
        <div id="ua-div"><span id="ua-span">hi</span></div>
        <div id="row" class="flex"><div id="r0" class="kid"></div><div id="r1" class="kid"></div></div>
        <div id="c-a" class="flex-col flex"><div id="a0" class="kid"></div><div id="a1" class="kid"></div></div>
        <div id="c-b" class="flex flex-col"><div id="b0" class="kid"></div><div id="b1" class="kid"></div></div>
        "#,
        CompileOptions::default(),
    )
    .expect("unstyled flex test compile");

    let mut tree = ViewTree::new();
    mount_compiled_document(&mut tree, &doc);
    let _ = tree.build_paint_scene(400, 300);
    let ua_div = tree
        .node_layout(tree.find_by_id("ua-div").unwrap())
        .unwrap();
    let ua_span = tree
        .node_layout(tree.find_by_id("ua-span").unwrap())
        .unwrap();
    assert!(
        ua_div.width > 300.0 && ua_span.width < 100.0,
        "div={ua_div:?} span={ua_span:?}"
    );
    let (r0, r1) = (
        tree.node_layout(tree.find_by_id("r0").unwrap()).unwrap(),
        tree.node_layout(tree.find_by_id("r1").unwrap()).unwrap(),
    );
    assert!(
        r1.x > r0.x + 30.0 && (r1.y - r0.y).abs() < 1.0,
        "display:flex must default to Row"
    );
    let (a0, a1) = (
        tree.node_layout(tree.find_by_id("a0").unwrap()).unwrap(),
        tree.node_layout(tree.find_by_id("a1").unwrap()).unwrap(),
    );
    let (b0, b1) = (
        tree.node_layout(tree.find_by_id("b0").unwrap()).unwrap(),
        tree.node_layout(tree.find_by_id("b1").unwrap()).unwrap(),
    );
    assert!(
        a1.y > a0.y + 15.0 && (a1.x - a0.x).abs() < 1.0,
        ".flex-col.flex must stack Column"
    );
    assert!(
        b1.y > b0.y + 15.0 && (b1.x - b0.x).abs() < 1.0,
        ".flex.flex-col must stack Column"
    );
}

#[cfg(feature = "preview")]
#[path = "../examples/component_gallery/controller/mod.rs"]
#[allow(dead_code)]
mod gallery_controller;

#[cfg(feature = "preview")]
fn verify_tailwind_disables_ripple(
    tree: &mut ViewTree,
    ctrl: &gallery_controller::GalleryController,
    elev_r: LayoutRect,
) {
    let switch_relayouts = tree.relayout_passes();
    ctrl.select_theme(1);
    let (_, rel_switch) = tree.build_paint_scene(1280, 840);
    assert!(
        !rel_switch && tree.relayout_passes() == switch_relayouts,
        "Material -> Tailwind switch on doc page 25 must stay 0-relayout"
    );
    let _ = tree.dispatch_pointer_down(elev_r.x + 12.0, elev_r.y + 14.0);
    let (tw_scene, rel_tw_press) = tree.build_paint_scene(1280, 840);
    assert!(
        !rel_tw_press && tree.relayout_passes() == switch_relayouts,
        "Tailwind button press must stay 0-relayout"
    );
    assert!(
        find_radial_near_box(&tw_scene, elev_r).is_none(),
        "Tailwind [data-theme=\"tailwind\"] must disable Material radial ripple"
    );
    let _ = tree.dispatch_pointer_up();
}

#[cfg(feature = "preview")]
#[test]
fn gallery_08_component_micl_and_elevated_button_pure_css_ripples_and_tailwind_toggle() {
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            use gallery_controller::GalleryController;
            use krui::dev_hot_reload::{
                compile_krui_source_from_path, mount_compiled_document_with_registry,
            };

            reset_reactive_runtime();
            let ctrl = GalleryController::new();
            let mut reg = ScopeRegistry::new();
            reg.insert_registry("controller", ctrl.clone());
            let mut tree = ViewTree::new();
            let path =
                std::path::Path::new("examples/component_gallery/views/component_gallery.krui");
            let src = std::fs::read_to_string(path).expect("read component_gallery.krui");
            let doc = compile_krui_source_from_path(path, &src).expect("compile component_gallery");
            let _ = mount_compiled_document_with_registry(&mut tree, &doc, std::rc::Rc::new(reg));

            ctrl.select_section(3);
            ctrl.select_doc_page(3);
            let _ = tree.build_paint_scene(1280, 840);
            let seg_r = tree
                .node_layout(tree.find_by_id("micl-seg-day").unwrap())
                .unwrap();
            let base_rel = tree.relayout_passes();
            let _ = tree.dispatch_pointer_down(seg_r.x + 8.0, seg_r.y + 12.0);
            let (micl_scene, rel0) = tree.build_paint_scene(1280, 840);
            assert!(
                !rel0
                    && tree.relayout_passes() == base_rel
                    && find_radial_near_box(&micl_scene, seg_r).is_some()
            );

            ctrl.select_doc_page(25);
            let _ = tree.build_paint_scene(1280, 840);
            let elev_r = tree
                .node_layout(tree.find_by_id("btn-demo-elevated").unwrap())
                .unwrap();
            let press_rel = tree.relayout_passes();
            let _ = tree.dispatch_pointer_down(elev_r.x + 12.0, elev_r.y + 14.0);
            let (press_scene, rel_p) = tree.build_paint_scene(1280, 840);
            assert!(
                !rel_p
                    && tree.relayout_passes() == press_rel
                    && find_radial_near_box(&press_scene, elev_r).is_some()
            );
            let _ = tree.dispatch_pointer_up();
            let _ = tree.build_paint_scene(1280, 840);
            let _ = tree.tick_transitions(1.0 / 60.0);
            let (elev_scene, _) = tree.build_paint_scene(1280, 840);
            assert!(find_radial_near_box(&elev_scene, elev_r).is_some());

            verify_tailwind_disables_ripple(&mut tree, &ctrl, elev_r);
        })
        .expect("spawn 2mb gallery test");
    handle.join().expect("gallery ripple e2e");
}
