//! Cross-platform system tray configuration, menu descriptors, and idle/close policy helpers.

use crate::app_icon::{AppIcon, CloseBehavior};
use crate::window_helpers::compute_window_idle_control_flow;
use std::time::{Duration, Instant};
use winit::event_loop::ControlFlow;

/// Mouse button associated with a system tray icon click event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayClickButton {
    /// Primary (left) mouse button.
    Left,
    /// Secondary (right) mouse button.
    Right,
    /// Middle mouse button.
    Middle,
}

/// System tray user interaction delivered to [`crate::WindowEventHooks::on_tray_event`].
#[derive(Debug, Clone, PartialEq)]
pub enum TrayEvent {
    /// A native context-menu item with the given numeric `id` was activated.
    MenuSelected(u32),
    /// The tray icon itself received a mouse click release at physical `(x, y)`.
    IconClicked {
        /// Which button was clicked.
        button: TrayClickButton,
        /// Physical horizontal cursor coordinate.
        x: f64,
        /// Physical vertical cursor coordinate.
        y: f64,
    },
    /// The tray icon received a double click at physical `(x, y)` (Windows).
    IconDoubleClicked {
        /// Which button was double-clicked.
        button: TrayClickButton,
        /// Physical horizontal cursor coordinate.
        x: f64,
        /// Physical vertical cursor coordinate.
        y: f64,
    },
}

/// Declarative item inside a native OS tray context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayMenuItem {
    /// Standard clickable menu item emitting [`TrayEvent::MenuSelected`]`(id)`.
    Action {
        /// Application-defined item identifier returned in [`TrayEvent::MenuSelected`].
        id: u32,
        /// Visible localized label text.
        label: String,
        /// Whether the item can be clicked.
        enabled: bool,
    },
    /// Checkable toggle menu item emitting [`TrayEvent::MenuSelected`]`(id)`.
    Check {
        /// Application-defined item identifier returned in [`TrayEvent::MenuSelected`].
        id: u32,
        /// Visible localized label text.
        label: String,
        /// Current checked state displayed by the OS menu.
        checked: bool,
        /// Whether the item can be clicked.
        enabled: bool,
    },
    /// Native horizontal divider line.
    Separator,
    /// Nested native submenu container.
    Submenu {
        /// Visible parent menu label.
        label: String,
        /// Whether the submenu can be opened.
        enabled: bool,
        /// Child menu entries.
        items: Vec<TrayMenuItem>,
    },
}

impl TrayMenuItem {
    /// Creates an enabled clickable menu item.
    pub fn action(id: u32, label: impl Into<String>) -> Self {
        Self::Action {
            id,
            label: label.into(),
            enabled: true,
        }
    }

    /// Creates an enabled checkable menu item with initial `checked` state.
    pub fn check(id: u32, label: impl Into<String>, checked: bool) -> Self {
        Self::Check {
            id,
            label: label.into(),
            checked,
            enabled: true,
        }
    }

    /// Creates a native menu separator.
    pub fn separator() -> Self {
        Self::Separator
    }

    /// Creates an enabled submenu holding `items`.
    pub fn submenu(label: impl Into<String>, items: Vec<TrayMenuItem>) -> Self {
        Self::Submenu {
            label: label.into(),
            enabled: true,
            items,
        }
    }
}

/// Configuration used to create and update the OS system tray icon and context menu.
#[derive(Debug, Clone)]
pub struct TrayConfig {
    /// Tray icon bitmap (typically 16x16–64x64 RGBA8).
    pub icon: AppIcon,
    /// Hover tooltip text displayed by Windows and macOS trays.
    pub tooltip: String,
    /// Native context menu entries attached to the tray icon.
    pub menu_items: Vec<TrayMenuItem>,
    /// Whether left-clicking the tray icon also opens the native context menu.
    pub show_menu_on_left_click: bool,
    /// Draws the tray icon as a monochrome template image on macOS menu bars.
    pub icon_is_template: bool,
}

impl TrayConfig {
    /// Builds a [`TrayConfig`] with `icon` and `tooltip` (`show_menu_on_left_click` defaults to `false`).
    pub fn new(icon: AppIcon, tooltip: impl Into<String>) -> Self {
        Self {
            icon,
            tooltip: tooltip.into(),
            menu_items: Vec::new(),
            show_menu_on_left_click: false,
            icon_is_template: false,
        }
    }

    /// Attaches the native context menu entries shown on right-click (and optionally left-click).
    pub fn with_menu(mut self, menu_items: Vec<TrayMenuItem>) -> Self {
        self.menu_items = menu_items;
        self
    }

    /// Controls whether left-clicking opens the native menu instead of only emitting [`TrayEvent::IconClicked`].
    pub fn with_menu_on_left_click(mut self, enable: bool) -> Self {
        self.show_menu_on_left_click = enable;
        self
    }

    /// Enables monochrome template rendering in the macOS menu bar.
    pub fn with_template_icon(mut self, icon_is_template: bool) -> Self {
        self.icon_is_template = icon_is_template;
        self
    }
}

/// Lifecycle/window command returned by [`crate::WindowEventHooks::on_tray_event`] or close policy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum WindowCommand {
    /// No window or tray mutation needed beyond normal hook redraw handling.
    #[default]
    None,
    /// Reveals the window, clears minimized state, focuses it, and schedules a redraw.
    ShowWindow,
    /// Hides the window to the system tray and suspends rendering for 0% idle CPU.
    HideToTray,
    /// Shows the window if currently hidden, or hides it to tray if visible.
    ToggleWindowVisibility,
    /// Exits the event loop and removes any active tray icon.
    Quit,
    /// Updates how subsequent window close button presses behave.
    SetCloseBehavior(CloseBehavior),
    /// Updates the OS tray hover tooltip string.
    SetTrayTooltip(String),
    /// Replaces the native tray context menu with a new item list.
    RebuildTrayMenu(Vec<TrayMenuItem>),
    /// Updates close behavior and rebuilds the tray menu together.
    UpdateCloseBehaviorAndMenu {
        /// New close button policy.
        close_behavior: CloseBehavior,
        /// Replacement native menu entries.
        menu_items: Vec<TrayMenuItem>,
    },
}

/// Resolves the command executed when the user presses the native OS close button.
pub const fn resolve_close_request_command(
    behavior: CloseBehavior,
    tray_active: bool,
) -> WindowCommand {
    match (behavior, tray_active) {
        (CloseBehavior::HideToTray, true) => WindowCommand::HideToTray,
        _ => WindowCommand::Quit,
    }
}

/// Frame/idle inputs passed to [`compute_tray_aware_idle_control_flow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayIdleState {
    /// Window is hidden into the background tray (`set_visible(false)`).
    pub is_hidden: bool,
    /// Window is minimized or zero-sized.
    pub is_minimized: bool,
    /// Active frame-clock or CSS animations are running.
    pub has_active_animations: bool,
    /// Scene needs repaint.
    pub is_paint_dirty: bool,
    /// An editable text input currently holds focus.
    pub is_input_focused: bool,
    /// Native window currently holds OS focus.
    pub is_window_focused: bool,
    /// Elapsed time since last keyboard/pointer/IME input.
    pub idle_since_input: Duration,
    /// Preview file watcher requires periodic polling.
    pub has_preview_watcher: bool,
}

/// Computes idle `ControlFlow` while ensuring hidden-to-tray or minimized windows sleep (`ControlFlow::Wait`)
/// with 0% CPU usage even if compositor animations or focused inputs existed before hiding.
pub fn compute_tray_aware_idle_control_flow(state: TrayIdleState, now: Instant) -> ControlFlow {
    let suppressed = state.is_hidden || state.is_minimized;
    compute_window_idle_control_flow(
        !suppressed && state.has_active_animations,
        !suppressed && state.is_paint_dirty,
        !suppressed && state.is_input_focused,
        !suppressed && state.is_window_focused,
        state.idle_since_input,
        state.has_preview_watcher,
        now,
    )
}
