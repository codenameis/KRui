//! Headless reactive on-screen virtual keyboard (`VirtualKeyboardController`) supporting
//! Hangul 2-bulsik composition, English QWERTY, and symbols for bare kiosk panels without OS IME.

use krui_core::{Hangul2BulsikAutomaton, HangulStep};
use krui_patterns::controller;
use krui_signals::{batch, Signal};

/// Active keymap layout layer on the on-screen virtual keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardLayer {
    /// Korean 2-bulsik Hangul keyboard backed by [`Hangul2BulsikAutomaton`].
    Hangul = 0,
    /// US-QWERTY English alphabetic layer.
    English = 1,
    /// Numeric and punctuation symbol layer.
    Symbols = 2,
}

impl KeyboardLayer {
    const fn from_index(idx: i32) -> Self {
        match idx {
            1 => Self::English,
            2 => Self::Symbols,
            _ => Self::Hangul,
        }
    }
}

/// Key press input emitted by on-screen keyboard key buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtualKeyKind {
    /// US-QWERTY key (mapped via 2-bulsik in Hangul mode) or direct character.
    Char(char),
    /// Direct Hangul compatibility jamo tap (used by explicit jamo buttons).
    Jamo(char),
    /// Delete composing jamo step-by-step or the last committed scalar.
    Backspace,
    /// Commit any composing Hangul syllable and insert an ASCII space.
    Space,
    /// Flush composition and increment submission count.
    Enter,
    /// Toggle uppercase / tense consonant shift state (`ㅂ->ㅃ`, `a->A`).
    Shift,
    /// Switch active keyboard layout layer.
    ToggleLayer(KeyboardLayer),
}

/// Pure-Rust headless OSK controller combining primitive `Signal`s with [`Hangul2BulsikAutomaton`].
#[controller]
#[derive(Clone)]
pub struct VirtualKeyboardController {
    /// Active keymap layer index (`0`=Hangul, `1`=English, `2`=Symbols).
    pub layer_index: Signal<i32>,
    /// Active Shift modifier state (tense Hangul consonants / uppercase ASCII).
    pub shift: Signal<bool>,
    /// Committed text buffer synchronized as keys commit.
    pub committed_text: Signal<String>,
    /// Currently composing Hangul syllable or jamo string (`""` when empty).
    pub composing_text: Signal<String>,
    /// Incremented whenever the user presses `Enter`.
    pub submit_count: Signal<i32>,
}

impl Default for VirtualKeyboardController {
    fn default() -> Self {
        Self::new(KeyboardLayer::Hangul, "")
    }
}

#[controller]
impl VirtualKeyboardController {
    /// Processes a single-character or string key press (`"".krui` template-bindable).
    pub fn press_text(&self, text: String) {
        for ch in text.chars() {
            self.handle_char_key(ch);
        }
    }

    /// Processes a direct Korean compatibility jamo string (`"ㅂ"`, `"ㅏ"`, etc.).
    pub fn press_jamo(&self, text: String) {
        for ch in text.chars() {
            self.apply_hangul_step(|auto| auto.push_jamo(ch));
        }
    }

    /// Switches keyboard layer by index (`0`=Hangul, `1`=English, `2`=Symbols).
    pub fn set_layer_index(&self, next_layer: i32) {
        self.flush();
        let clamped = next_layer.clamp(0, 2);
        batch(|| {
            self.layer_index.set(clamped);
            self.shift.set(false);
        });
    }

    /// Toggles uppercase / tense consonant shift state.
    pub fn toggle_shift(&self) {
        self.shift.update(|s| *s = !*s);
    }

    /// Commits active composition and inserts an ASCII space.
    pub fn press_space(&self) {
        self.flush();
        self.committed_text.update(|s| s.push(' '));
    }

    /// Commits active composition and increments `submit_count`.
    pub fn press_enter(&self) {
        self.flush();
        self.submit_count.update(|n| *n += 1);
    }

    /// Decomposes active Hangul syllable step-by-step or pops the last committed scalar.
    pub fn press_backspace(&self) {
        let mut auto = self.load_automaton();
        let step_opt = auto.backspace();
        batch(|| match step_opt {
            Some(step) => self.store_composing(step.composing),
            None => {
                self.composing_text.set(String::new());
                self.committed_text.update(|s| {
                    let _ = s.pop();
                });
            }
        });
    }

    /// Commits any active Hangul syllable (e.g., before focus transfers) and clears composing state.
    pub fn flush(&self) {
        let mut auto = self.load_automaton();
        let tail = auto.flush();
        batch(|| {
            if let Some(ch) = tail {
                self.committed_text.update(|s| s.push(ch));
            }
            self.composing_text.set(String::new());
        });
    }

    /// Returns the live preview string (`committed_text` + `composing_text`).
    pub fn live_text(&self) -> String {
        let mut out = self.committed_text.get();
        out.push_str(&self.composing_text.get());
        out
    }
}

impl VirtualKeyboardController {
    /// Creates a virtual keyboard controller with initial `layer` and `initial_text`.
    pub fn new(initial_layer: KeyboardLayer, initial_text: impl Into<String>) -> Self {
        Self {
            layer_index: Signal::new(initial_layer as i32),
            shift: Signal::new(false),
            committed_text: Signal::new(initial_text.into()),
            composing_text: Signal::new(String::new()),
            submit_count: Signal::new(0),
        }
    }

    /// Returns the currently active typed [`KeyboardLayer`].
    pub fn layer(&self) -> KeyboardLayer {
        KeyboardLayer::from_index(self.layer_index.get())
    }

    /// Dispatches a strongly typed [`VirtualKeyKind`] action from Rust/embedded hosts.
    pub fn press_key(&self, key: VirtualKeyKind) {
        match key {
            VirtualKeyKind::ToggleLayer(next_layer) => self.set_layer_index(next_layer as i32),
            VirtualKeyKind::Shift => self.toggle_shift(),
            VirtualKeyKind::Space => self.press_space(),
            VirtualKeyKind::Enter => self.press_enter(),
            VirtualKeyKind::Backspace => self.press_backspace(),
            VirtualKeyKind::Char(ch) => self.handle_char_key(ch),
            VirtualKeyKind::Jamo(jamo) => self.apply_hangul_step(|auto| auto.push_jamo(jamo)),
        }
    }

    fn load_automaton(&self) -> Hangul2BulsikAutomaton {
        let comp = self.composing_text.get_untracked().chars().next();
        Hangul2BulsikAutomaton::from_composing_char(comp)
    }

    fn store_composing(&self, ch: Option<char>) {
        let next = match ch {
            Some(c) => {
                let mut buf = [0_u8; 4];
                c.encode_utf8(&mut buf).to_string()
            }
            None => String::new(),
        };
        self.composing_text.set(next);
    }

    fn handle_char_key(&self, ch: char) {
        let shifted = if self.shift.get_untracked() {
            ch.to_ascii_uppercase()
        } else {
            ch
        };
        if KeyboardLayer::from_index(self.layer_index.get_untracked()) == KeyboardLayer::Hangul {
            self.apply_hangul_step(|auto| auto.push_ascii_key(shifted));
        } else {
            self.flush();
            self.committed_text.update(|s| s.push(shifted));
        }
        if self.shift.get_untracked() {
            self.shift.set(false);
        }
    }

    fn apply_hangul_step(&self, f: impl FnOnce(&mut Hangul2BulsikAutomaton) -> HangulStep) {
        let mut auto = self.load_automaton();
        let step = f(&mut auto);
        batch(|| {
            if let Some(ch) = step.committed {
                self.committed_text.update(|s| s.push(ch));
            }
            self.store_composing(step.composing);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_keyboard_composes_hangul_and_switches_to_english() {
        let kb = VirtualKeyboardController::default();
        for c in ['g', 'k', 's', 'r', 'm', 'f'] {
            kb.press_key(VirtualKeyKind::Char(c));
        }
        assert_eq!(kb.live_text(), "한글");
        kb.press_key(VirtualKeyKind::Backspace);
        assert_eq!(kb.live_text(), "한그");
        kb.press_key(VirtualKeyKind::ToggleLayer(KeyboardLayer::English));
        kb.press_key(VirtualKeyKind::Space);
        kb.press_key(VirtualKeyKind::Shift);
        kb.press_key(VirtualKeyKind::Char('u'));
        kb.press_key(VirtualKeyKind::Char('i'));
        assert_eq!(kb.live_text(), "한그 Ui");
    }
}
