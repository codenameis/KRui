//! Interactive desktop window entrypoints (`run_app_window*`).

use crate::app_icon::WindowOptions;
use crate::ime_adapter::WinitImeStateCache;
#[cfg(feature = "preview")]
use crate::preview;
use crate::session::EngineSession;
use crate::tray_host::install_tray_event_forwarders;
use crate::window_helpers::{
    run_headless_window_session, KruiWinitEvent, NoopWindowHooks, WindowEventHooks,
    WinitProxyUiWaker,
};
use crate::window_loop::StandardWindowHost;
use krui_core::{HoveredCursorKind, Mods};
use std::sync::Arc;
use std::time::Instant;
use winit::event_loop::EventLoop;

/// Launches an interactive native desktop window for `session` (or runs headless verification
/// if `--headless` is passed on the command line).
pub fn run_app_window(title: &str, session: EngineSession, hot_reload_krui_path: Option<&str>) {
    run_app_window_with_hooks(title, session, hot_reload_krui_path, NoopWindowHooks);
}

/// Launches an interactive native desktop window with custom [`WindowEventHooks`].
pub fn run_app_window_with_hooks<H: WindowEventHooks>(
    title: &str,
    session: EngineSession,
    hot_reload_krui_path: Option<&str>,
    hooks: H,
) {
    run_app_window_with_options(
        title,
        session,
        hot_reload_krui_path,
        hooks,
        WindowOptions::default(),
    );
}

/// Launches an interactive native desktop window with [`WindowEventHooks`] and [`WindowOptions`].
pub fn run_app_window_with_options<H: WindowEventHooks>(
    title: &str,
    mut session: EngineSession,
    _hot_reload_krui_path: Option<&str>,
    hooks: H,
    options: WindowOptions,
) {
    if std::env::args().any(|a| a == "--headless") {
        run_headless_window_session(title, &mut session);
        return;
    }
    #[cfg(feature = "preview")]
    if crate::embed_panel::is_embed_panel_mode() {
        use crate::embed_panel::run_embedded_panel_session as run_panel;
        run_panel(title, session, _hot_reload_krui_path, hooks);
        return;
    }
    let Ok(event_loop) = EventLoop::<KruiWinitEvent>::with_user_event().build() else {
        let _ = session.step_frame(1.0 / 120.0);
        return;
    };
    let proxy = event_loop.create_proxy();
    if options.tray.is_some() {
        install_tray_event_forwarders(&proxy);
    }
    session.reactive_session.enter(|| {
        krui_signals::set_ui_waker(Some(Arc::new(WinitProxyUiWaker::new(proxy.clone()))));
    });
    #[cfg(feature = "preview")]
    let hot_reloader = preview::init_window_hot_reloader(_hot_reload_krui_path, &proxy);
    let now = Instant::now();
    let start_hidden = options.start_hidden;
    let mut host = StandardWindowHost {
        base_title: title.to_string(),
        session,
        hooks,
        options,
        window: None,
        presenter: None,
        a11y_adapter: None,
        tray_host: None,
        proxy,
        cursor_pos: (0.0, 0.0),
        is_left_mouse_down: false,
        hook_pointer_buttons: 0,
        is_window_focused: !start_hidden,
        is_window_hidden: start_hidden,
        mods: Mods::default(),
        current_cursor_kind: HoveredCursorKind::Default,
        pointer_lock_active: false,
        ime_cache: WinitImeStateCache::default(),
        epoch: now,
        last_frame: now,
        last_touch_time: now,
        last_caret_activity: now,
        #[cfg(feature = "preview")]
        last_hot_reload_check: now,
        #[cfg(feature = "preview")]
        hot_reloader,
    };
    let _ = event_loop.run_app(&mut host);
    host.tray_host = None;
    host.session
        .reactive_session
        .enter(|| krui_signals::set_ui_waker(None));
}
