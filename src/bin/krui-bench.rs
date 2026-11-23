//! Controlled release benchmark for KRui (`krui-bench`).
//!
//! Measures cold first frame, warm first frame, and 300-frame animation telemetry
//! (after 30 warmup frames) across both `TinySkia` (CPU fallback) and `Wgpu` (GPU)
//! backends, enforcing the 120 FPS (`<= 8,333 us`) budget for both `p50` and `p99`
//! on both backends and waiting for GPU completion (`device.poll(Wait)`) on `Wgpu`.

#![forbid(unsafe_code)]

use krui::*;

const WARMUP_FRAMES: usize = 30;
const MEASURED_FRAMES: usize = 300;
const BENCH_WIDTH: u32 = 1024;
const BENCH_HEIGHT: u32 = 720;
const FRAME_DT_SECONDS: f32 = 1.0 / 120.0;

struct BenchResult {
    backend_name: &'static str,
    cold_first_frame_us: u64,
    warm_first_frame_us: u64,
    p50_us: u64,
    p99_us: u64,
    max_us: u64,
    relayouts_during_anim: u64,
}

struct BenchViewState {
    shake_trigger: Signal<bool>,
    spring_target_x: Signal<f32>,
    pulse_count: Signal<i32>,
}

fn mount_bench_view(tree: &mut ViewTree, clock: &mut FrameClock) -> BenchViewState {
    let (shake_tx, shake_trigger) = clock.shake(10.0, 22.0, 0.35);
    let spring_target_x = Signal::new(260.0_f32);
    let spring_x = clock.spring_to(0.0, spring_target_x, 180.0, 22.0);
    let pulse_count = Signal::new(0_i32);
    let card_opacity: Signal<f32> = Computed::new(move || {
        if spring_x.get().abs() > 12.0 {
            0.88
        } else {
            1.0
        }
    })
    .into();
    let on_activate = Command::new(move || pulse_count.update(|c| *c += 1));
    let summary = Computed::new(move || format!("Pulse #{} · 120Hz Fast-Path", pulse_count.get()));

    tree.mount_with(move || {
        krui!(
            r#"
            <style>
                .bench-shell { display: flex; flex-direction: column; width: 100%; height: 100%; padding: 24px; gap: 16px; background-color: #0f172a; color: #f8fafc; font-family: "Malgun Gothic", "Noto Sans KR", sans-serif; }
                .bench-header { display: flex; flex-direction: row; justify-content: space-between; align-items: center; padding: 16px 20px; background-color: #1e293b; border: 1px solid #334155; border-radius: 12px; }
                .bench-title { font-size: 20px; font-weight: 700; color: #f8fafc; }
                .bench-btn { padding: 10px 18px; background-color: #38bdf8; color: #082f49; border: 1px solid #7dd3fc; border-radius: 8px; font-size: 14px; font-weight: 700; }
                .bench-grid { display: flex; flex-direction: row; gap: 16px; }
                .bench-card { flex-grow: 1; display: flex; flex-direction: column; padding: 20px; gap: 10px; background-color: #1e293b; border: 1px solid #334155; border-radius: 12px; box-shadow: 0px 4px 12px 0px rgba(0, 0, 0, 0.25); }
                .card-heading { font-size: 16px; font-weight: 700; color: #e2e8f0; }
                .card-sub { font-size: 13px; color: #94a3b8; }
            </style>
            <div class="bench-shell">
                <div class="bench-header">
                    <span class="bench-title">KRui Stylo + Taffy 0.15 Release Benchmark</span>
                    <button id="bench-btn" class="bench-btn" onclick="{on_activate}">{summary}</button>
                </div>
                <div class="bench-grid">
                    <div class="bench-card" style="transform: {shake_tx}; opacity: {card_opacity}">
                        <span class="card-heading">Compositor Shake Track</span>
                        <span class="card-sub">GPU/SW SDF quad + 0-relayout PaintSlot</span>
                    </div>
                    <div class="bench-card" style="transform: translateX({spring_x}px)">
                        <span class="card-heading">Spring Motion Card</span>
                        <span class="card-sub">Zero Stylo restyle during transform ticks</span>
                    </div>
                </div>
            </div>
            "#
        )
    });

    BenchViewState {
        shake_trigger,
        spring_target_x,
        pulse_count,
    }
}

fn step_and_prepare_warm_session(
    preference: RenderBackendPreference,
) -> Option<(u64, u64, EngineSession, BenchViewState, u64)> {
    reset_reactive_runtime();

    let mut session = EngineSession::new(BENCH_WIDTH, BENCH_HEIGHT, preference);
    if matches!(preference, RenderBackendPreference::ForceWgpu)
        && session.renderer.backend_kind() != ActiveBackendKind::Wgpu
    {
        return None;
    }

    let _cold_state = mount_bench_view(&mut session.tree, &mut session.clock);
    let cold_first_frame_us = session
        .step_frame_with_gpu_wait(FRAME_DT_SECONDS)
        .total_frame_us;

    let mut warm_session = EngineSession::new(BENCH_WIDTH, BENCH_HEIGHT, preference);
    let warm_state = mount_bench_view(&mut warm_session.tree, &mut warm_session.clock);
    let warm_first_frame_us = warm_session
        .step_frame_with_gpu_wait(FRAME_DT_SECONDS)
        .total_frame_us;

    let _ = warm_session.tree.dispatch_key_tab(false);
    let _ = warm_session.tree.dispatch_key_activate();
    assert!(warm_state.pulse_count.get() >= 1);
    let post_text = warm_session.step_frame_with_gpu_wait(FRAME_DT_SECONDS);
    let relayout_baseline = post_text.total_relayout_passes;

    Some((
        cold_first_frame_us,
        warm_first_frame_us,
        warm_session,
        warm_state,
        relayout_baseline,
    ))
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(default)
}

fn run_backend_bench(
    backend_name: &'static str,
    preference: RenderBackendPreference,
) -> Option<BenchResult> {
    let (cold_first_frame_us, warm_first_frame_us, mut warm_session, state, relayout_baseline) =
        step_and_prepare_warm_session(preference)?;
    let warmup_frames = env_u64("KRUI_BENCH_WARMUP_FRAMES", WARMUP_FRAMES as u64) as usize;
    let measured_frames =
        (env_u64("KRUI_BENCH_MEASURED_FRAMES", MEASURED_FRAMES as u64) as usize).max(1);
    let inject_slow_us = env_u64("KRUI_BENCH_INJECT_SLOW_US", 0);

    for i in 0..warmup_frames {
        if i % 45 == 15 {
            state.shake_trigger.update(|v| *v = !*v);
            state
                .spring_target_x
                .update(|x| *x = if *x > 0.0 { 0.0 } else { 24.0 });
        }
        let _ = warm_session.step_frame_with_gpu_wait(FRAME_DT_SECONDS);
    }

    let mut samples = Vec::with_capacity(measured_frames);
    for i in 0..measured_frames {
        if i % 60 == 0 {
            state.shake_trigger.update(|v| *v = !*v);
            state
                .spring_target_x
                .update(|x| *x = if *x > 0.0 { 0.0 } else { 24.0 });
        }
        let telemetry = warm_session.step_frame_with_gpu_wait(FRAME_DT_SECONDS);
        samples.push(telemetry.total_frame_us.saturating_add(inject_slow_us));
    }

    let final_relayouts = warm_session
        .step_frame_with_gpu_wait(FRAME_DT_SECONDS)
        .total_relayout_passes;
    let relayouts_during_anim = final_relayouts.saturating_sub(relayout_baseline);
    samples.sort_unstable();
    let p99_idx = ((samples.len() * 99) / 100).min(samples.len() - 1);
    Some(BenchResult {
        backend_name,
        cold_first_frame_us,
        warm_first_frame_us,
        p50_us: samples[samples.len() / 2],
        p99_us: samples[p99_idx],
        max_us: *samples.last().unwrap_or(&0),
        relayouts_during_anim,
    })
}

fn validate_bench_result(res: &BenchResult) -> bool {
    println!(
        "[{:<8}] cold_1st={:>6}us | warm_1st={:>6}us | p50={:>5}us | p99={:>5}us | max={:>6}us | anim_relayouts={}",
        res.backend_name,
        res.cold_first_frame_us,
        res.warm_first_frame_us,
        res.p50_us,
        res.p99_us,
        res.max_us,
        res.relayouts_during_anim
    );
    let budget_us = std::env::var("KRUI_BENCH_BUDGET_US")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(FrameTelemetry::BUDGET_120FPS_US);
    let mut ok = true;
    if res.relayouts_during_anim != 0 {
        eprintln!(
            "[BENCH FAILED] {} triggered {} relayouts during transform/opacity animation!",
            res.backend_name, res.relayouts_during_anim
        );
        ok = false;
    }
    if res.p50_us > budget_us {
        eprintln!(
            "[BENCH FAILED] {} p50={}us exceeds 120 FPS budget ({}us)!",
            res.backend_name, res.p50_us, budget_us
        );
        ok = false;
    }
    if res.p99_us > budget_us {
        eprintln!(
            "[BENCH FAILED] {} p99={}us exceeds 120 FPS p99 budget ({}us)!",
            res.backend_name, res.p99_us, budget_us
        );
        ok = false;
    }
    ok
}

fn main() {
    println!("==================================================================");
    println!(
        " [KRui Release Benchmark] warmup={WARMUP_FRAMES} frames, measured={MEASURED_FRAMES} frames"
    );
    println!("==================================================================");

    let allow_missing_wgpu = std::env::args().any(|a| a == "--allow-missing-wgpu")
        || std::env::var("KRUI_BENCH_ALLOW_MISSING_WGPU")
            .ok()
            .is_some_and(|v| v == "1");
    let force_missing_wgpu = std::env::var("KRUI_BENCH_FORCE_MISSING_WGPU")
        .ok()
        .is_some_and(|v| v == "1");

    let mut failed = false;
    for (name, pref) in [
        ("TinySkia", RenderBackendPreference::ForceTinySkia),
        ("Wgpu", RenderBackendPreference::ForceWgpu),
    ] {
        let res_opt = if name == "Wgpu" && force_missing_wgpu {
            None
        } else {
            run_backend_bench(name, pref)
        };
        if let Some(res) = res_opt {
            if !validate_bench_result(&res) {
                failed = true;
            }
        } else if name == "Wgpu" && !allow_missing_wgpu {
            eprintln!(
                "[BENCH FAILED] {name} backend is unavailable! Pass --allow-missing-wgpu only on headless CI without GPU/software adapter."
            );
            failed = true;
        } else {
            println!("[{name:<8}] Backend unavailable on this machine (skipped)");
        }
    }

    if failed {
        std::process::exit(1);
    }
}
