//! `component_gallery` / `krui-showcase`용 앱 아이콘, 백그라운드 트레이 및 OS 컨텍스트 메뉴 설정.

use super::controller::GalleryController;
use krui::*;

pub(crate) const MENU_TOGGLE_WINDOW: u32 = 1;
pub(crate) const MENU_TOGGLE_DARK: u32 = 2;
pub(crate) const MENU_TOGGLE_HIDE_ON_CLOSE: u32 = 3;
pub(crate) const MENU_QUIT: u32 = 99;

const ICON_PNG_BYTES: &[u8] = include_bytes!("../../assets/krui_icon.png");

pub(crate) fn load_gallery_app_icon() -> AppIcon {
    AppIcon::from_png_bytes(ICON_PNG_BYTES)
        .expect("embedded assets/krui_icon.png must remain valid RGBA PNG")
}

pub(crate) fn build_gallery_tray_menu(dark_mode: bool, hide_on_close: bool) -> Vec<TrayMenuItem> {
    vec![
        TrayMenuItem::action(MENU_TOGGLE_WINDOW, "창 열기 / 숨기기"),
        TrayMenuItem::check(MENU_TOGGLE_DARK, "다크 테마 사용", dark_mode),
        TrayMenuItem::check(
            MENU_TOGGLE_HIDE_ON_CLOSE,
            "닫기 시 트레이로 숨기기",
            hide_on_close,
        ),
        TrayMenuItem::separator(),
        TrayMenuItem::action(MENU_QUIT, "KRui 종료"),
    ]
}

pub(crate) fn build_gallery_window_options(
    args: &[String],
    controller: &GalleryController,
) -> (WindowOptions, bool) {
    let start_hidden = args.iter().any(|a| a == "--start-hidden");
    let hide_on_close = start_hidden || args.iter().any(|a| a == "--tray" || a == "--background");
    let icon = load_gallery_app_icon();
    let dark_mode = controller.dark_mode.get_untracked();
    let tray = TrayConfig::new(icon.clone(), "KRui Component Gallery")
        .with_menu(build_gallery_tray_menu(dark_mode, hide_on_close))
        .with_template_icon(true);
    let close_behavior = if hide_on_close {
        CloseBehavior::HideToTray
    } else {
        CloseBehavior::Exit
    };
    let options = WindowOptions::default()
        .with_icon(icon)
        .with_close_behavior(close_behavior)
        .with_start_hidden(start_hidden)
        .with_tray(tray);
    (options, hide_on_close)
}

pub(crate) fn handle_gallery_tray_event(
    controller: &GalleryController,
    hide_on_close: &mut bool,
    session: &mut EngineSession,
    event: &TrayEvent,
) -> WindowCommand {
    match event {
        TrayEvent::IconClicked {
            button: TrayClickButton::Left,
            ..
        }
        | TrayEvent::MenuSelected(MENU_TOGGLE_WINDOW) => WindowCommand::ToggleWindowVisibility,
        TrayEvent::MenuSelected(MENU_TOGGLE_DARK) => {
            session.reactive_session.clone().enter(|| {
                controller.toggle_dark_mode();
            });
            let dark = controller.dark_mode.get_untracked();
            session
                .tree
                .set_color_scheme_preference(controller.current_color_scheme_pref());
            WindowCommand::RebuildTrayMenu(build_gallery_tray_menu(dark, *hide_on_close))
        }
        TrayEvent::MenuSelected(MENU_TOGGLE_HIDE_ON_CLOSE) => {
            *hide_on_close = !*hide_on_close;
            let behavior = if *hide_on_close {
                CloseBehavior::HideToTray
            } else {
                CloseBehavior::Exit
            };
            let dark = controller.dark_mode.get_untracked();
            WindowCommand::UpdateCloseBehaviorAndMenu {
                close_behavior: behavior,
                menu_items: build_gallery_tray_menu(dark, *hide_on_close),
            }
        }
        TrayEvent::MenuSelected(MENU_QUIT) => WindowCommand::Quit,
        _ => WindowCommand::None,
    }
}

pub(crate) fn take_gallery_window_command(
    controller: &GalleryController,
    session: &mut EngineSession,
) -> WindowCommand {
    if !controller.tray_hide_requested.get_untracked() {
        return WindowCommand::None;
    }
    session
        .reactive_session
        .clone()
        .enter(|| controller.tray_hide_requested.set(false));
    WindowCommand::HideToTray
}

pub(crate) fn run_gallery_window<H: WindowEventHooks>(
    session: EngineSession,
    hooks: H,
    options: WindowOptions,
) {
    run_app_window_with_options(
        "KRui [component_gallery] — 순수 CSS 컴포넌트 갤러리 (Material 3 · Tailwind v4)",
        session,
        Some("examples/component_gallery/views/component_gallery.krui"),
        hooks,
        options,
    );
}
