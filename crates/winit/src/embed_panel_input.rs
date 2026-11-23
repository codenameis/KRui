//! Input translation helpers mapping [`PanelInputEvent`] into live `EngineSession` + `ViewTree`.

use crate::session::EngineSession;
use crate::window_helpers::WindowEventHooks;
use krui_core::{ImeEvent, KeyInfo, Mods};
use krui_preview::{DeviceFramePreset, PanelInputEvent};

/// Converts a browser DOM `KeyboardEvent.key` string into a platform-neutral [`KeyInfo`].
pub(crate) fn key_info_from_panel_key(key: &str) -> KeyInfo {
    match key {
        "Escape" => KeyInfo::Escape,
        "Tab" => KeyInfo::Tab,
        "Backspace" => KeyInfo::Backspace,
        "Delete" => KeyInfo::Delete,
        "ArrowLeft" => KeyInfo::ArrowLeft,
        "ArrowRight" => KeyInfo::ArrowRight,
        "ArrowUp" => KeyInfo::ArrowUp,
        "ArrowDown" => KeyInfo::ArrowDown,
        "Home" => KeyInfo::Home,
        "End" => KeyInfo::End,
        "PageUp" => KeyInfo::PageUp,
        "PageDown" => KeyInfo::PageDown,
        "Enter" => KeyInfo::Enter,
        " " | "Spacebar" => KeyInfo::Space,
        other => single_printable_or_other(other),
    }
}

fn single_printable_or_other(key: &str) -> KeyInfo {
    let mut chars = key.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if !ch.is_control() {
            return KeyInfo::Character {
                logical: key.to_string(),
                text: Some(key.to_string()),
            };
        }
    }
    KeyInfo::Other(key.to_string())
}

fn map_panel_point(
    session: &EngineSession,
    frame: Option<DeviceFramePreset>,
    x: f32,
    y: f32,
    allow_unclamped_capture: bool,
) -> Option<(f32, f32)> {
    let Some(preset) = frame else {
        return Some((x, y));
    };
    if allow_unclamped_capture && session.tree.captured_pointer_node().is_some() {
        Some(preset.map_window_to_screen_point_unclamped(x, y))
    } else {
        preset.map_window_to_screen_point(x, y)
    }
}

fn apply_key_down_event<H: WindowEventHooks>(
    session: &mut EngineSession,
    hooks: &mut H,
    key: &str,
    mods: Mods,
    repeat: bool,
) -> Option<Option<String>> {
    let key_info = key_info_from_panel_key(key);
    if let Some(effect) = hooks.on_key_down(session, &key_info, mods) {
        return Some(effect);
    }
    let _ = session.handle_key_down(&key_info, mods, repeat);
    None
}

struct PointerDownInfo {
    frame: Option<DeviceFramePreset>,
    x: f32,
    y: f32,
    button: u8,
    mods: Mods,
    time_ms: u64,
}

fn apply_pointer_down<H: WindowEventHooks>(
    session: &mut EngineSession,
    hooks: &mut H,
    is_left_down: &mut bool,
    info: PointerDownInfo,
) -> Option<Option<String>> {
    *is_left_down = false;
    let (mx, my) = map_panel_point(session, info.frame, info.x, info.y, false)?;
    if let Some(effect) = hooks.on_pointer_button(session, mx, my, info.button, true, info.mods) {
        return Some(effect);
    }
    if info.button != 0 {
        return None;
    }
    *is_left_down = true;
    session.reactive_session.enter(|| {
        let _ =
            session
                .tree
                .dispatch_pointer_down_with_mods_at_time(mx, my, info.mods, info.time_ms);
    });
    hooks.on_tick(session)
}

fn apply_pointer_move<H: WindowEventHooks>(
    session: &mut EngineSession,
    hooks: &mut H,
    frame: Option<DeviceFramePreset>,
    is_left_down: bool,
    x: f32,
    y: f32,
) {
    let mapped = map_panel_point(session, frame, x, y, true);
    session.reactive_session.enter(|| {
        if is_left_down {
            if let Some((mx, my)) = mapped {
                let _ = session.tree.dispatch_pointer_drag(mx, my);
            }
        }
        let _ = match mapped {
            Some((mx, my)) => session.tree.dispatch_pointer_move(mx, my),
            None => session.tree.dispatch_pointer_move(-10_000.0, -10_000.0),
        };
    });
    if let Some((mx, my)) = mapped {
        let _ = hooks.on_pointer_moved(session, mx, my, Mods::default());
    }
}

fn apply_pointer_up<H: WindowEventHooks>(
    session: &mut EngineSession,
    hooks: &mut H,
    frame: Option<DeviceFramePreset>,
    is_left_down: &mut bool,
    x: f32,
    y: f32,
) -> Option<Option<String>> {
    let was_dom_drag = *is_left_down;
    *is_left_down = false;
    let mapped = map_panel_point(session, frame, x, y, true);
    if !was_dom_drag {
        if let Some((mx, my)) = mapped {
            if let Some(effect) =
                hooks.on_pointer_button(session, mx, my, 0, false, Mods::default())
            {
                return Some(effect);
            }
        }
    }
    session.reactive_session.enter(|| {
        if let Some((mx, my)) = mapped {
            let _ = session.tree.dispatch_pointer_up_at(mx, my);
        }
        session.tree.end_pointer_drag();
    });
    hooks.on_tick(session)
}

fn apply_pointer_event<H: WindowEventHooks>(
    session: &mut EngineSession,
    hooks: &mut H,
    frame: Option<DeviceFramePreset>,
    is_left_down: &mut bool,
    event: PanelInputEvent,
) -> Option<Option<String>> {
    match event {
        PanelInputEvent::PointerDown {
            x,
            y,
            button,
            shift,
            ctrl,
            alt,
            time_ms,
        } => apply_pointer_down(
            session,
            hooks,
            is_left_down,
            PointerDownInfo {
                frame,
                x,
                y,
                button,
                mods: Mods {
                    ctrl,
                    alt,
                    shift,
                    super_key: false,
                },
                time_ms,
            },
        ),
        PanelInputEvent::PointerMove { x, y } => {
            apply_pointer_move(session, hooks, frame, *is_left_down, x, y);
            None
        }
        PanelInputEvent::PointerUp { x, y } => {
            apply_pointer_up(session, hooks, frame, is_left_down, x, y)
        }
        PanelInputEvent::PointerLeave => {
            let _ = session.tree.dispatch_pointer_leave();
            None
        }
        PanelInputEvent::Wheel { x, y, delta_y } => {
            if let Some((mx, my)) = map_panel_point(session, frame, x, y, false) {
                if let Some(effect) =
                    hooks.on_mouse_wheel(session, mx, my, -delta_y, Mods::default())
                {
                    return Some(effect);
                }
                session.reactive_session.enter(|| {
                    let _ = session.tree.dispatch_scroll(mx, my, -delta_y);
                });
            }
            None
        }
        _ => None,
    }
}

pub(crate) struct PanelEventOutcome {
    pub(crate) force_refresh: bool,
    pub(crate) hook_effect: Option<Option<String>>,
}

fn apply_panel_resize(
    session: &mut EngineSession,
    frame: Option<DeviceFramePreset>,
    base_size: &mut (u32, u32),
    width: u16,
    height: u16,
) -> PanelEventOutcome {
    *base_size = (
        (width as u32).clamp(240, 2560),
        (height as u32).clamp(200, 2160),
    );
    if frame.is_none() {
        session.width = base_size.0;
        session.height = base_size.1;
    }
    PanelEventOutcome {
        force_refresh: true,
        hook_effect: None,
    }
}

fn dispatch_ime_panel_event(session: &mut EngineSession, event: PanelInputEvent) {
    session.reactive_session.enter(|| match event {
        PanelInputEvent::ImePreedit {
            text,
            cursor_start,
            cursor_end,
        } => {
            let _ = session.tree.dispatch_ime_event(ImeEvent::Enabled);
            let _ = session.tree.dispatch_ime_event(ImeEvent::Preedit {
                text,
                cursor_offset: Some((cursor_start, cursor_end)),
            });
        }
        PanelInputEvent::ImeCommit { text } => {
            let _ = session.tree.dispatch_ime_event(ImeEvent::Commit(text));
        }
        _ => {}
    });
}

pub(crate) fn apply_panel_input_event<H: WindowEventHooks>(
    session: &mut EngineSession,
    hooks: &mut H,
    frame: Option<DeviceFramePreset>,
    base_size: &mut (u32, u32),
    is_left_down: &mut bool,
    event: PanelInputEvent,
) -> PanelEventOutcome {
    match event {
        PanelInputEvent::RequestRefresh => PanelEventOutcome {
            force_refresh: true,
            hook_effect: None,
        },
        PanelInputEvent::Resize { width, height } => {
            apply_panel_resize(session, frame, base_size, width, height)
        }
        PanelInputEvent::KeyDown {
            key,
            shift,
            ctrl,
            alt,
            repeat,
        } => PanelEventOutcome {
            force_refresh: false,
            hook_effect: apply_key_down_event(
                session,
                hooks,
                &key,
                Mods {
                    ctrl,
                    alt,
                    shift,
                    super_key: false,
                },
                repeat,
            ),
        },
        ev @ (PanelInputEvent::ImePreedit { .. } | PanelInputEvent::ImeCommit { .. }) => {
            dispatch_ime_panel_event(session, ev);
            PanelEventOutcome {
                force_refresh: false,
                hook_effect: None,
            }
        }
        other => PanelEventOutcome {
            force_refresh: false,
            hook_effect: apply_pointer_event(session, hooks, frame, is_left_down, other),
        },
    }
}
