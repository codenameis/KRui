//! Platform-neutral `evdev` input adapter for touch panels, rotary encoders, D-pads,
//! and built-in 2-bulsik Hangul IME routing (`krui-kms`).

use krui_core::{
    next_spatial_focus, FocusableCandidate, Hangul2BulsikAutomaton, ImeEvent, NodeId,
    SpatialNavDirection, TouchCalibrationMatrix, TouchPhaseKind, ViewTree,
};
use krui_signals::FrameClock;

/// Abstracted Linux `evdev` input events consumed by [`KioskInputRouter`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EvdevInputEvent {
    /// Capacitive/resistive finger down in raw panel coordinates.
    TouchDown { slot: u64, raw_x: f32, raw_y: f32 },
    /// Finger motion in raw panel coordinates with frame delta seconds.
    TouchMove {
        slot: u64,
        raw_x: f32,
        raw_y: f32,
        dt_secs: f32,
    },
    /// Finger lifted from touch panel.
    TouchUp { slot: u64, raw_x: f32, raw_y: f32 },
    /// Directional or rotary navigation action.
    SpatialNav(SpatialNavDirection),
    /// Encoder wheel detent steps (positive = clockwise / `Next`, negative = `Prev`).
    RotaryDelta(i32),
    /// Physical push-button activation on the currently focused node.
    ActivateFocused,
    /// Printable ASCII key input (routed through 2-bulsik Hangul when enabled).
    AsciiKey(char),
    /// Backspace key (decomposes active Hangul syllable before deleting committed chars).
    Backspace,
    /// Space bar / Enter flushing active Hangul composition and activating or inserting space.
    CommitAndSpace,
}

/// Stateful kiosk input router combining touch affine calibration and embedded 2-bulsik Hangul IME.
pub struct KioskInputRouter {
    calibration: TouchCalibrationMatrix,
    hangul_enabled: bool,
    hangul: Hangul2BulsikAutomaton,
}

impl Default for KioskInputRouter {
    fn default() -> Self {
        Self::new(TouchCalibrationMatrix::identity(), false)
    }
}

impl KioskInputRouter {
    /// Creates a router with `calibration` and optional `hangul_enabled` 2-bulsik IME.
    pub const fn new(calibration: TouchCalibrationMatrix, hangul_enabled: bool) -> Self {
        Self {
            calibration,
            hangul_enabled,
            hangul: Hangul2BulsikAutomaton::new(),
        }
    }

    /// Toggles embedded 2-bulsik Hangul composition, flushing any pending syllable cleanly.
    pub fn set_hangul_enabled(&mut self, enabled: bool, tree: &mut ViewTree) {
        if self.hangul_enabled && !enabled {
            self.flush_hangul_to_tree(tree);
        }
        self.hangul_enabled = enabled;
    }

    /// Updates touch panel affine calibration matrix.
    pub fn set_calibration(&mut self, matrix: TouchCalibrationMatrix) {
        self.calibration = matrix;
    }

    /// Routes `event` into `tree`, returning `true` if `tree` state or visual focus changed.
    pub fn dispatch(
        &mut self,
        tree: &mut ViewTree,
        clock: &mut FrameClock,
        event: EvdevInputEvent,
        focusables: &[FocusableCandidate],
        current_focus: Option<NodeId>,
    ) -> bool {
        match event {
            EvdevInputEvent::TouchDown { slot, raw_x, raw_y } => {
                let (x, y) = self.calibration.map_point(raw_x, raw_y);
                tree.dispatch_touch(clock, slot, TouchPhaseKind::Started, x, y, 0.0)
            }
            EvdevInputEvent::TouchMove {
                slot,
                raw_x,
                raw_y,
                dt_secs,
            } => {
                let (x, y) = self.calibration.map_point(raw_x, raw_y);
                tree.dispatch_touch(clock, slot, TouchPhaseKind::Moved, x, y, dt_secs)
            }
            EvdevInputEvent::TouchUp { slot, raw_x, raw_y } => {
                let (x, y) = self.calibration.map_point(raw_x, raw_y);
                tree.dispatch_touch(clock, slot, TouchPhaseKind::Ended, x, y, 0.0)
            }
            EvdevInputEvent::SpatialNav(dir) => {
                self.route_spatial(tree, focusables, current_focus, dir)
            }
            EvdevInputEvent::RotaryDelta(steps) => {
                self.route_rotary(tree, focusables, current_focus, steps)
            }
            EvdevInputEvent::ActivateFocused => {
                self.flush_hangul_to_tree(tree);
                tree.dispatch_key_activate()
            }
            EvdevInputEvent::AsciiKey(ch) => self.route_ascii(tree, ch),
            EvdevInputEvent::Backspace => self.route_backspace(tree),
            EvdevInputEvent::CommitAndSpace => {
                self.flush_hangul_to_tree(tree);
                tree.dispatch_key_space()
            }
        }
    }

    fn route_spatial(
        &mut self,
        tree: &mut ViewTree,
        focusables: &[FocusableCandidate],
        current_focus: Option<NodeId>,
        dir: SpatialNavDirection,
    ) -> bool {
        let Some(target) = next_spatial_focus(focusables, current_focus, dir) else {
            return false;
        };
        self.flush_hangul_to_tree(tree);
        tree.set_focus_to_node(target)
    }

    fn route_rotary(
        &mut self,
        tree: &mut ViewTree,
        focusables: &[FocusableCandidate],
        mut focus: Option<NodeId>,
        steps: i32,
    ) -> bool {
        let count = steps.unsigned_abs().min(32);
        let dir = if steps >= 0 {
            SpatialNavDirection::Next
        } else {
            SpatialNavDirection::Prev
        };
        let mut moved = false;
        for _ in 0..count {
            if let Some(next_id) = next_spatial_focus(focusables, focus, dir) {
                focus = Some(next_id);
                moved |= tree.set_focus_to_node(next_id);
            }
        }
        moved
    }

    fn route_ascii(&mut self, tree: &mut ViewTree, ch: char) -> bool {
        if !self.hangul_enabled {
            let mut buf = [0_u8; 4];
            let s = ch.encode_utf8(&mut buf);
            return tree.dispatch_key_text(s);
        }
        let step = self.hangul.push_ascii_key(ch);
        let mut changed = false;
        if let Some(committed) = step.committed {
            let mut buf = [0_u8; 4];
            let s = committed.encode_utf8(&mut buf);
            changed |= tree.dispatch_ime_event(ImeEvent::Commit(s.to_string()));
        }
        if let Some(comp) = step.composing {
            let mut buf = [0_u8; 4];
            let s = comp.encode_utf8(&mut buf).to_string();
            let len = s.len();
            changed |= tree.dispatch_ime_event(ImeEvent::Preedit {
                text: s,
                cursor_offset: Some((len, len)),
            });
        } else {
            changed |= tree.dispatch_ime_event(ImeEvent::Preedit {
                text: String::new(),
                cursor_offset: None,
            });
        }
        changed
    }

    fn route_backspace(&mut self, tree: &mut ViewTree) -> bool {
        if self.hangul_enabled {
            if let Some(step) = self.hangul.backspace() {
                if let Some(comp) = step.composing {
                    let mut buf = [0_u8; 4];
                    let s = comp.encode_utf8(&mut buf).to_string();
                    let len = s.len();
                    return tree.dispatch_ime_event(ImeEvent::Preedit {
                        text: s,
                        cursor_offset: Some((len, len)),
                    });
                }
                return tree.dispatch_ime_event(ImeEvent::Preedit {
                    text: String::new(),
                    cursor_offset: None,
                });
            }
        }
        tree.dispatch_backspace()
    }

    /// Commits any pending Hangul composition into `tree`.
    pub fn flush_hangul_to_tree(&mut self, tree: &mut ViewTree) {
        if let Some(ch) = self.hangul.flush() {
            let mut buf = [0_u8; 4];
            let s = ch.encode_utf8(&mut buf).to_string();
            let _ = tree.dispatch_ime_event(ImeEvent::Commit(s));
        }
    }
}
