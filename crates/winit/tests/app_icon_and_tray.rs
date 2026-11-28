//! Unit & platform-safe tests for desktop `AppIcon`, `.ico`/`.png` assets, tray menu descriptors,
//! close-to-tray policy, and 0%-CPU hidden idle scheduling.

#![forbid(unsafe_code)]

use krui_winit::{
    compute_tray_aware_idle_control_flow, resolve_close_request_command, AppIcon, AppIconError,
    CloseBehavior, TrayClickButton, TrayConfig, TrayEvent, TrayIdleState, TrayMenuItem,
    WindowCommand, WindowOptions, WindowSpec, MAX_ICON_DIMENSION,
};
use std::time::{Duration, Instant};
use winit::event_loop::ControlFlow;

const BRAND_PNG: &[u8] = include_bytes!("../../../assets/krui_icon.png");
const BRAND_ICO: &[u8] = include_bytes!("../../../assets/krui_icon.ico");

#[test]
fn embedded_brand_png_and_ico_assets_are_valid_multi_size() {
    let icon = AppIcon::from_png_bytes(BRAND_PNG).expect("brand PNG should decode");
    assert_eq!((icon.width(), icon.height()), (256, 256));
    assert!(
        icon.rgba().as_chunks::<4>().0.iter().any(|px| px[3] > 200),
        "brand icon must contain opaque foreground pixels"
    );

    // Multi-size ICO directory header: reserved=0, type=1, image_count >= 4.
    assert!(BRAND_ICO.len() > 6 + 16 * 4);
    assert_eq!(&BRAND_ICO[0..4], &[0, 0, 1, 0]);
    let count = u16::from_le_bytes([BRAND_ICO[4], BRAND_ICO[5]]);
    assert_eq!(count, 4);
}

#[test]
fn app_icon_validation_and_winit_conversion() {
    assert_eq!(
        AppIcon::from_rgba(vec![], 0, 16),
        Err(AppIconError::InvalidDimensions {
            width: 0,
            height: 16
        })
    );
    assert_eq!(
        AppIcon::from_rgba(vec![], MAX_ICON_DIMENSION + 1, 16),
        Err(AppIconError::InvalidDimensions {
            width: MAX_ICON_DIMENSION + 1,
            height: 16
        })
    );
    assert_eq!(
        AppIcon::from_rgba(vec![0; 12], 2, 2),
        Err(AppIconError::BufferLengthMismatch {
            expected: 16,
            actual: 12
        })
    );
    assert!(matches!(
        AppIcon::from_png_bytes(b"not-a-png"),
        Err(AppIconError::DecodeFailed(_))
    ));

    let ok = AppIcon::from_rgba(vec![255; 16 * 16 * 4], 16, 16).expect("valid 16x16 RGBA");
    assert_eq!(ok.rgba().len(), 1024);

    let spec = WindowSpec::new("Test", 320, 200, |_| {}).with_icon(ok.clone());
    assert_eq!(spec.icon.as_ref().map(AppIcon::width), Some(16));
}

#[test]
fn close_request_policy_only_hides_when_tray_is_active() {
    assert_eq!(
        resolve_close_request_command(CloseBehavior::Exit, false),
        WindowCommand::Quit
    );
    assert_eq!(
        resolve_close_request_command(CloseBehavior::Exit, true),
        WindowCommand::Quit
    );
    assert_eq!(
        resolve_close_request_command(CloseBehavior::HideToTray, false),
        WindowCommand::Quit
    );
    assert_eq!(
        resolve_close_request_command(CloseBehavior::HideToTray, true),
        WindowCommand::HideToTray
    );
}

#[test]
fn hidden_to_tray_window_enforces_zero_cpu_wait_control_flow() {
    let now = Instant::now();
    let hidden_flow = compute_tray_aware_idle_control_flow(
        TrayIdleState {
            is_hidden: true,
            is_minimized: false,
            has_active_animations: true,
            is_paint_dirty: true,
            is_input_focused: true,
            is_window_focused: true,
            idle_since_input: Duration::from_millis(50),
            has_preview_watcher: false,
        },
        now,
    );
    assert_eq!(
        hidden_flow,
        ControlFlow::Wait,
        "hidden background tray window must sleep with ControlFlow::Wait (0% idle CPU)"
    );

    let visible_anim_flow = compute_tray_aware_idle_control_flow(
        TrayIdleState {
            is_hidden: false,
            is_minimized: false,
            has_active_animations: true,
            is_paint_dirty: true,
            is_input_focused: false,
            is_window_focused: true,
            idle_since_input: Duration::from_secs(1),
            has_preview_watcher: false,
        },
        now,
    );
    assert!(matches!(visible_anim_flow, ControlFlow::WaitUntil(_)));
}

#[test]
fn tray_config_and_window_options_builders_preserve_fields() {
    let icon = AppIcon::from_png_bytes(BRAND_PNG).unwrap();
    let menu = vec![
        TrayMenuItem::action(1, "Show"),
        TrayMenuItem::check(2, "Dark", true),
        TrayMenuItem::separator(),
        TrayMenuItem::submenu("More", vec![TrayMenuItem::action(3, "About")]),
    ];
    let tray = TrayConfig::new(icon.clone(), "KRui App")
        .with_menu(menu.clone())
        .with_menu_on_left_click(false)
        .with_template_icon(true);
    let opts = WindowOptions::default()
        .with_icon(icon)
        .with_close_behavior(CloseBehavior::HideToTray)
        .with_start_hidden(true)
        .with_tray(tray);

    assert!(opts.start_hidden);
    assert_eq!(opts.close_behavior, CloseBehavior::HideToTray);
    let tray_cfg = opts.tray.expect("tray config");
    assert_eq!(tray_cfg.tooltip, "KRui App");
    assert_eq!(tray_cfg.menu_items.len(), 4);
    assert!(!tray_cfg.show_menu_on_left_click);
    assert!(tray_cfg.icon_is_template);

    let ev = TrayEvent::IconClicked {
        button: TrayClickButton::Left,
        x: 100.0,
        y: 200.0,
    };
    assert!(matches!(
        ev,
        TrayEvent::IconClicked {
            button: TrayClickButton::Left,
            ..
        }
    ));
}

#[cfg(all(target_os = "windows", feature = "tray"))]
#[test]
fn windows_native_menu_builds_without_panic() {
    let items = vec![
        TrayMenuItem::action(1, "열기"),
        TrayMenuItem::check(2, "트레이에 상주", true),
        TrayMenuItem::separator(),
        TrayMenuItem::submenu("테마", vec![TrayMenuItem::action(3, "Material 3")]),
        TrayMenuItem::action(99, "종료"),
    ];
    let native_menu = krui_winit::build_native_menu(&items);
    assert_eq!(native_menu.items().len(), 5);
}
