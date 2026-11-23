#![forbid(unsafe_code)]

use krui::*;

fn px_at(fb: &[u8], stride: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * stride + x) * 4) as usize;
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

fn tailwind_v4_test_view(shift_clicks: Signal<u32>) -> ViewFragment {
    let on_shift = Command::new(move || shift_clicks.update(|n| *n += 1));
    krui!(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            @layer theme, base, utilities;
            @property --tw-ring-shadow { syntax: "*"; inherits: false; initial-value: 0 0 #0000; }
            @property --tw-shadow { syntax: "*"; inherits: false; initial-value: 0 0 #0000; }
            @property --tw-ring-color { syntax: "*"; inherits: false; initial-value: #ef4444; }
            @layer theme {
                :root { --tw-bg: #0f172a; }
            }
            @layer utilities {
                .stage { display: flex; flex-direction: column; gap: 14px; padding: 16px; width: 460px; background: var(--tw-bg); }
                .w-1\/2 { width: 50%; height: 24px; }
                .bg-\[\#1e293b\] { background-color: #1e293b; }
                .rounded-full { border-radius: calc(infinity * 1px); }
                .appearance-none { appearance: none; width: 24px; height: 24px; padding: 0px; background-color: #334155; }
                .ring-and-shadow {
                    width: 96px; height: 28px; margin: 12px; background-color: #1e293b;
                    --tw-ring-shadow: 0 0 0 4px var(--tw-ring-color);
                    --tw-shadow: 0 12px 0 0 #f43f5e;
                    box-shadow: 0 0 #0000, 0 0 #0000, var(--tw-ring-shadow), var(--tw-shadow);
                }
                .peer-checked\:ring-green:is(:where(.peer):checked ~ *) { --tw-ring-color: #22c55e; }
                .peer-checked\:move:is(:where(.peer):checked ~ *) { translate: 30px 0px; background-color: #22c55e; }
                .group-wrap { display: flex; flex-direction: row; align-items: center; gap: 12px; padding: 8px; background: #1e293b; }
                .group-badge { width: 40px; height: 24px; background-color: #ef4444; transition: transform 150ms linear, background-color 150ms linear; }
                .group-hover\:lit:is(:where(.group):hover *) { background-color: #22c55e; scale: 1.1; }
                .shift-btn { width: 100px; height: 40px; translate: 50% 0px; rotate: 0deg; scale: 1.0; transform-origin: top left; background-color: #38bdf8; }
                .oklab-bar { width: 200px; height: 32px; background-image: linear-gradient(to right in oklab, #0000ff, #ffff00); }
                .knob { width: 20px; height: 20px; background-color: #94a3b8; }
            }
        </style>
        <div id="stage" class="stage">
            <div id="half" class="w-1/2 bg-[#1e293b] rounded-full"></div>
            <div class="group-wrap group" id="grp">
                <div id="grp-badge" class="group-badge group-hover:lit"></div>
            </div>
            <input id="peer-cb" type="checkbox" class="peer appearance-none" />
            <div id="peer-ring" class="ring-and-shadow peer-checked:ring-green"></div>
            <div id="knob" class="knob peer-checked:move"></div>
            <button id="shift-btn" class="shift-btn" onclick="{on_shift}">Shift</button>
            <div id="oklab-bar" class="oklab-bar"></div>
        </div>
        "#
    )
}

#[test]
fn tailwind_v4_dropin_selectors_shadows_transforms_and_oklab_cpu_gpu() {
    let shift_clicks = Signal::new(0u32);
    let mut tree = ViewTree::new();
    tree.mount_with(move || tailwind_v4_test_view(shift_clicks));
    let (scene0, _) = tree.build_paint_scene(480, 420);
    let base_relayouts = tree.relayout_passes();

    const EXPECTED_HALF_STAGE_WIDTH: f32 = (460.0 - 2.0 * 16.0) * 0.5;
    let half_r = tree.node_layout(tree.find_by_id("half").unwrap()).unwrap();
    assert!(
        (half_r.width - EXPECTED_HALF_STAGE_WIDTH).abs() < 1.5,
        "w-1/2 width={}",
        half_r.width
    );

    let ring_r = tree
        .node_layout(tree.find_by_id("peer-ring").unwrap())
        .unwrap();
    let oklab_r = tree
        .node_layout(tree.find_by_id("oklab-bar").unwrap())
        .unwrap();
    render_cpu_and_optional_gpu(&scene0, |tag, fb, w| {
        let top_ring = px_at(fb, w, (ring_r.x + 20.0) as u32, (ring_r.y - 2.0) as u32);
        let drop_sh = px_at(fb, w, (ring_r.x + 20.0) as u32, (ring_r.y + 34.0) as u32);
        let mid_oklab = px_at(fb, w, (oklab_r.x + 100.0) as u32, (oklab_r.y + 16.0) as u32);
        assert!(
            top_ring[0] > 180 && top_ring[1] < 110,
            "[{tag}] ring red={top_ring:?}"
        );
        assert!(
            drop_sh[0] > 180 && drop_sh[2] > 60,
            "[{tag}] shadow rose={drop_sh:?}"
        );
        assert!(
            mid_oklab[1] > 170,
            "[{tag}] in-oklab midpoint must stay bright green/teal, got {mid_oklab:?}"
        );
    });

    let shift_r = tree
        .node_layout(tree.find_by_id("shift-btn").unwrap())
        .unwrap();
    assert!(
        (shift_r.x - (half_r.x + 50.0)).abs() < 1.5,
        "50% translate must shift x by +50: {shift_r:?}"
    );
    assert!(!tree.dispatch_click(half_r.x + 10.0, shift_r.y + 20.0));
    assert!(tree.dispatch_click(shift_r.x + 20.0, shift_r.y + 20.0));
    assert_eq!(shift_clicks.get(), 1);

    assert_peer_checked_and_group_hover_compositor(&mut tree, ring_r, base_relayouts);
}

fn assert_peer_checked_and_group_hover_compositor(
    tree: &mut ViewTree,
    ring_r: LayoutRect,
    base_relayouts: u64,
) {
    let cb_r = tree
        .node_layout(tree.find_by_id("peer-cb").unwrap())
        .unwrap();
    assert!(tree.dispatch_click(cb_r.x + 12.0, cb_r.y + 12.0));
    let grp_r = tree.node_layout(tree.find_by_id("grp").unwrap()).unwrap();
    assert!(tree.dispatch_pointer_move(grp_r.x + 20.0, grp_r.y + 12.0));
    let _ = tree.build_paint_scene(480, 420);
    let anim_base = tree.relayout_passes();
    for _ in 0..120 {
        let _ = tree.tick_transitions(1.0 / 120.0);
    }
    let (scene1, _) = tree.build_paint_scene(480, 420);
    assert_eq!(
        tree.relayout_passes(),
        anim_base,
        "compositor hover/peer must trigger 0 relayouts"
    );
    assert!(base_relayouts >= 1);
    let badge_r = tree
        .node_layout(tree.find_by_id("grp-badge").unwrap())
        .unwrap();
    render_cpu_and_optional_gpu(&scene1, |tag, fb, w| {
        let top_ring = px_at(fb, w, (ring_r.x + 20.0) as u32, (ring_r.y - 2.0) as u32);
        let badge = px_at(fb, w, (badge_r.x + 20.0) as u32, (badge_r.y + 12.0) as u32);
        assert!(
            top_ring[1] > 160,
            "[{tag}] peer-checked green ring={top_ring:?}"
        );
        assert!(badge[1] > 160, "[{tag}] group-hover green badge={badge:?}");
    });
}

#[cfg(feature = "preview")]
const COMPOSITE_DROPIN_MARKUP: &str = r#"
<link rel="krui-preset" href="krui://preset/unstyled" />
<style>
    @layer base, utilities;
    @layer utilities {
        .panel { display: flex; flex-direction: column; gap: 10px; padding: 16px; width: 380px; background: #0f172a; }
        .w-1\/2 { width: 50%; height: 28px; background: #1e293b; border-radius: calc(infinity * 1px); box-shadow: 0 0 #0000, 0 0 0 2px #6366f1, 0 8px 12px -2px rgb(0 0 0 / 0.4); }
        .bar { width: 220px; height: 28px; background-image: linear-gradient(to right in oklab, #4f46e5, #06b6d4); translate: 12px 0px; }
        .micl-btn { width: 160px; height: 40px; border-radius: 20px; background: #3b82f6; color: #ffffff; }
    }
</style>
<div class="panel">
    <div class="w-1/2"></div>
    <div class="bar"></div>
    <button class="micl-btn">Unified Drop-in</button>
</div>
"#;

#[cfg(feature = "preview")]
#[test]
fn tailwind_beercss_micl_macro_and_preview_pixel_exact_parity() {
    let mut mac_tree = ViewTree::new();
    mac_tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                @layer base, utilities;
                @layer utilities {
                    .panel { display: flex; flex-direction: column; gap: 10px; padding: 16px; width: 380px; background: #0f172a; }
                    .w-1\/2 { width: 50%; height: 28px; background: #1e293b; border-radius: calc(infinity * 1px); box-shadow: 0 0 #0000, 0 0 0 2px #6366f1, 0 8px 12px -2px rgb(0 0 0 / 0.4); }
                    .bar { width: 220px; height: 28px; background-image: linear-gradient(to right in oklab, #4f46e5, #06b6d4); translate: 12px 0px; }
                    .micl-btn { width: 160px; height: 40px; border-radius: 20px; background: #3b82f6; color: #ffffff; }
                }
            </style>
            <div class="panel">
                <div class="w-1/2"></div>
                <div class="bar"></div>
                <button class="micl-btn">Unified Drop-in</button>
            </div>
            "#
        )
    });
    let doc = krui::preview::compile_krui_source(COMPOSITE_DROPIN_MARKUP).expect("preview compile");
    let mut prv_tree = ViewTree::new();
    let _ = krui::preview::mount_compiled_document(&mut prv_tree, &doc);
    let (s_mac, _) = mac_tree.build_paint_scene(400, 220);
    let (s_prv, _) = prv_tree.build_paint_scene(400, 220);
    let mut r_mac = TinySkiaRenderer::new(400, 220, GpuResourceLimits::default());
    let mut r_prv = TinySkiaRenderer::new(400, 220, GpuResourceLimits::default());
    r_mac.render_scene(&s_mac);
    r_prv.render_scene(&s_prv);
    assert_eq!(
        r_mac.framebuffer_rgba().unwrap(),
        r_prv.framebuffer_rgba().unwrap(),
        "macro and preview must produce exact RGBA parity for Tailwind+MICL styles"
    );
}

#[allow(deprecated)]
fn mount_beercss_micl_fixture() -> ViewTree {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                .root { display: flex; flex-direction: column; gap: 10px; padding: 12px; width: 340px; background: #ffffff; }
                input[type="text" i] { width: 180px; height: 32px; background: #e2e8f0; }
                input:read-only { background-color: #fde047; }
                .indet { width: 24px; height: 24px; background: #94a3b8; }
                .indet:indeterminate { background-color: #a855f7; }
                .card { display: flex; flex-direction: row; gap: 8px; padding: 8px; width: 220px; height: 38px; background: #ef4444; }
                .card:has(> input:checked) { background-color: #16a34a; }
                .has-cb { width: 20px; height: 20px; }
                .micl { position: relative; overflow: hidden; width: 180px; height: 48px; border-radius: 24px; background: #1e1b4b; color: #f97316; box-shadow: 0 8px 0 0 #0ea5e9; }
                .micl::before { content: ""; position: absolute; inset: 0px; opacity: 0; background-image: radial-gradient(circle at var(--krui-pointer-x, 50%) var(--krui-pointer-y, 50%), currentColor 22%, transparent 24%); background-size: 90%; transition: opacity 400ms linear; }
                .micl:active::before { opacity: 0.9; transition: 0s; }
                dialog { width: 140px; height: 48px; background: #0f172a; color: #fff; }
                dialog::backdrop { background: rgba(0, 0, 0, 0.5); }
                .pop { width: 120px; height: 36px; background: #14b8a6; }
            </style>
            <div class="root">
                <input id="icase-in" TYPE="TEXT" readonly value="RO" />
                <input id="indet-cb" type="checkbox" class="indet" indeterminate />
                <div id="has-card" class="card"><input id="has-cb" class="has-cb" type="checkbox" /></div>
                <details id="det"><summary id="sum">S</summary><div id="det-body" style="width:100px;height:20px;background:#333"></div></details>
                <button id="pop-trig" popovertarget="be-pop" style="width:80px;height:24px;">P</button>
                <div id="be-pop" popover class="pop"></div>
                <button id="micl-btn" class="micl">MICL</button>
                <dialog id="be-dlg" open modal>Modal</dialog>
            </div>
            "#
        )
    });
    tree
}

#[test]
fn beercss_and_micl_semantics_has_attr_icase_and_ripple_shadow_parity() {
    let mut tree = mount_beercss_micl_fixture();
    let (s0, _) = tree.build_paint_scene(360, 360);

    let ro_r = tree
        .node_layout(tree.find_by_id("icase-in").unwrap())
        .unwrap();
    let indet_r = tree
        .node_layout(tree.find_by_id("indet-cb").unwrap())
        .unwrap();
    let card_r = tree
        .node_layout(tree.find_by_id("has-card").unwrap())
        .unwrap();
    render_cpu_and_optional_gpu(&s0, |tag, fb, w| {
        let ro = px_at(fb, w, (ro_r.x + 8.0) as u32, (ro_r.y + 8.0) as u32);
        let ind = px_at(fb, w, (indet_r.x + 12.0) as u32, (indet_r.y + 12.0) as u32);
        let c0 = px_at(fb, w, (card_r.x + 160.0) as u32, (card_r.y + 18.0) as u32);
        assert!(
            ro[0] > 105 && ro[1] > 95 && ro[2] < 70,
            "[{tag}] dimmed [type=text i]:read-only={ro:?}"
        );
        assert!(
            ind[2] > 90 && ind[2] > ind[1] + 40,
            "[{tag}] dimmed :indeterminate={ind:?}"
        );
        assert!(
            c0[0] > 90 && c0[1] < 60,
            "[{tag}] dimmed unchecked card={c0:?}"
        );
    });

    let hcb_r = tree
        .node_layout(tree.find_by_id("has-cb").unwrap())
        .unwrap();
    assert!(tree.dispatch_key_escape());
    assert!(tree.dispatch_click(hcb_r.x + 10.0, hcb_r.y + 10.0));
    let sum_r = tree.node_layout(tree.find_by_id("sum").unwrap()).unwrap();
    assert!(tree.dispatch_click(sum_r.x + 10.0, sum_r.y + 10.0));
    let _ = tree.build_paint_scene(360, 360);
    let micl_r = tree
        .node_layout(tree.find_by_id("micl-btn").unwrap())
        .unwrap();
    let _ = tree.dispatch_pointer_down(micl_r.x + 20.0, micl_r.y + 24.0);

    let (s1, _) = tree.build_paint_scene(360, 360);
    render_cpu_and_optional_gpu(&s1, |tag, fb, w| {
        let ro1 = px_at(fb, w, (ro_r.x + 8.0) as u32, (ro_r.y + 8.0) as u32);
        let c1 = px_at(fb, w, (card_r.x + 160.0) as u32, (card_r.y + 18.0) as u32);
        let rip = px_at(fb, w, (micl_r.x + 20.0) as u32, (micl_r.y + 24.0) as u32);
        let sh = px_at(fb, w, (micl_r.x + 90.0) as u32, (micl_r.y + 52.0) as u32);
        assert!(
            ro1[0] > 200 && ro1[1] > 190 && ro1[2] < 120,
            "[{tag}] undimmed [type=text i]:read-only={ro1:?}"
        );
        assert!(
            c1[1] > 130 && c1[0] < 80,
            "[{tag}] :has(> input:checked) card green={c1:?}"
        );
        assert!(rip[0] > 180, "[{tag}] MICL offcenter ripple={rip:?}");
        assert!(sh[2] > 180, "[{tag}] MICL outer elevation shadow={sh:?}");
    });
}
