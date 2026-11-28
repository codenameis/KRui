//! OS tray icon lifecycle, `tray-icon`/`muda` native menu builder, and [`WindowCommand`] executor.

use crate::tray::{TrayConfig, TrayMenuItem, WindowCommand};
use crate::window_helpers::{KruiWinitEvent, WindowEventHooks};
use crate::window_loop::StandardWindowHost;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};

#[cfg(feature = "tray")]
use crate::tray::{TrayClickButton, TrayEvent};
#[cfg(feature = "tray")]
use tray_icon::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
#[cfg(feature = "tray")]
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Owned native system tray handle dropped cleanly on exit.
pub(crate) struct ActiveTrayHost {
    #[cfg(feature = "tray")]
    tray: TrayIcon,
}

#[cfg(feature = "tray")]
fn map_mouse_button(button: MouseButton) -> TrayClickButton {
    match button {
        MouseButton::Left => TrayClickButton::Left,
        MouseButton::Right => TrayClickButton::Right,
        MouseButton::Middle => TrayClickButton::Middle,
    }
}

/// Builds a native OS [`tray_icon::menu::Menu`] from declarative [`TrayMenuItem`] descriptors.
#[cfg(feature = "tray")]
pub fn build_native_menu(items: &[TrayMenuItem]) -> Menu {
    let menu = Menu::new();
    for item in items {
        if let Some(built) = build_menu_entry(item) {
            let _ = menu.append(&*built);
        }
    }
    menu
}

#[cfg(feature = "tray")]
fn build_menu_entry(item: &TrayMenuItem) -> Option<Box<dyn IsMenuItem>> {
    match item {
        TrayMenuItem::Action { id, label, enabled } => Some(Box::new(MenuItem::with_id(
            id.to_string(),
            label,
            *enabled,
            None,
        ))),
        TrayMenuItem::Check {
            id,
            label,
            checked,
            enabled,
        } => Some(Box::new(CheckMenuItem::with_id(
            id.to_string(),
            label,
            *enabled,
            *checked,
            None,
        ))),
        TrayMenuItem::Separator => Some(Box::new(PredefinedMenuItem::separator())),
        TrayMenuItem::Submenu {
            label,
            enabled,
            items,
        } => {
            let sub = Submenu::new(label, *enabled);
            for child in items {
                if let Some(built) = build_menu_entry(child) {
                    let _ = sub.append(&*built);
                }
            }
            Some(Box::new(sub))
        }
    }
}

/// Installs OS menu/tray event forwarders into `EventLoopProxy<KruiWinitEvent>`.
pub(crate) fn install_tray_event_forwarders(proxy: &EventLoopProxy<KruiWinitEvent>) {
    #[cfg(feature = "tray")]
    {
        let menu_proxy = proxy.clone();
        MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
            if let Ok(id) = ev.id.as_ref().parse::<u32>() {
                let _ = menu_proxy.send_event(KruiWinitEvent::Tray(TrayEvent::MenuSelected(id)));
            }
        }));
        let icon_proxy = proxy.clone();
        TrayIconEvent::set_event_handler(Some(move |ev: TrayIconEvent| {
            if let Some(mapped) = map_tray_icon_event(ev) {
                let _ = icon_proxy.send_event(KruiWinitEvent::Tray(mapped));
            }
        }));
    }
    #[cfg(not(feature = "tray"))]
    let _ = proxy;
}

#[cfg(feature = "tray")]
fn map_tray_icon_event(ev: TrayIconEvent) -> Option<TrayEvent> {
    match ev {
        TrayIconEvent::Click {
            button,
            button_state: MouseButtonState::Up,
            position,
            ..
        } => Some(TrayEvent::IconClicked {
            button: map_mouse_button(button),
            x: position.x,
            y: position.y,
        }),
        TrayIconEvent::DoubleClick {
            button, position, ..
        } => Some(TrayEvent::IconDoubleClicked {
            button: map_mouse_button(button),
            x: position.x,
            y: position.y,
        }),
        _ => None,
    }
}

impl ActiveTrayHost {
    pub(crate) fn try_new(config: &TrayConfig) -> Option<Self> {
        #[cfg(feature = "tray")]
        {
            let tray_icon = config.icon.to_tray_icon()?;
            let menu = build_native_menu(&config.menu_items);
            let builder = TrayIconBuilder::new();
            #[cfg(target_os = "macos")]
            let builder = if config.icon_is_template {
                builder.with_icon_templated(tray_icon)
            } else {
                builder.with_icon(tray_icon)
            };
            #[cfg(not(target_os = "macos"))]
            let builder = builder.with_icon(tray_icon);
            let tray = builder
                .with_tooltip(&config.tooltip)
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(config.show_menu_on_left_click)
                .build()
                .ok()?;
            Some(Self { tray })
        }
        #[cfg(not(feature = "tray"))]
        {
            let _ = config;
            None
        }
    }

    pub(crate) fn set_tooltip(&self, tooltip: &str) {
        #[cfg(feature = "tray")]
        let _ = self.tray.set_tooltip(Some(tooltip));
        #[cfg(not(feature = "tray"))]
        let _ = tooltip;
    }

    pub(crate) fn set_menu(&self, items: &[TrayMenuItem]) {
        #[cfg(feature = "tray")]
        self.tray.set_menu(Some(Box::new(build_native_menu(items))));
        #[cfg(not(feature = "tray"))]
        let _ = items;
    }
}

fn reveal_host_window<H: WindowEventHooks>(host: &mut StandardWindowHost<H>) {
    host.is_window_hidden = false;
    if let Some(win) = &host.window {
        win.set_visible(true);
        win.set_minimized(false);
        win.focus_window();
        win.request_redraw();
    }
}

fn hide_host_window<H: WindowEventHooks>(host: &mut StandardWindowHost<H>) {
    host.is_window_hidden = true;
    host.is_window_focused = false;
    host.is_left_mouse_down = false;
    host.hook_pointer_buttons = 0;
    if let Some(win) = &host.window {
        win.set_visible(false);
    }
}

/// Applies a [`WindowCommand`] returned by a tray handler or window close policy.
pub(crate) fn apply_window_command_to_host<H: WindowEventHooks>(
    host: &mut StandardWindowHost<H>,
    event_loop: &ActiveEventLoop,
    cmd: WindowCommand,
) {
    match cmd {
        WindowCommand::None => {}
        WindowCommand::ShowWindow => reveal_host_window(host),
        WindowCommand::HideToTray => hide_host_window(host),
        WindowCommand::ToggleWindowVisibility => {
            if host.is_window_hidden {
                reveal_host_window(host);
            } else {
                hide_host_window(host);
            }
        }
        WindowCommand::Quit => {
            host.tray_host = None;
            event_loop.exit();
        }
        WindowCommand::SetCloseBehavior(behavior) => {
            host.options.close_behavior = behavior;
        }
        WindowCommand::SetTrayTooltip(tip) => {
            if let Some(tray) = &host.tray_host {
                tray.set_tooltip(&tip);
            }
        }
        WindowCommand::RebuildTrayMenu(items) => {
            if let Some(tray) = &host.tray_host {
                tray.set_menu(&items);
            }
        }
        WindowCommand::UpdateCloseBehaviorAndMenu {
            close_behavior,
            menu_items,
        } => {
            host.options.close_behavior = close_behavior;
            if let Some(tray) = &host.tray_host {
                tray.set_menu(&menu_items);
            }
        }
    }
}
