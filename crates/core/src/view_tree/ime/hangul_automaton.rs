//! Embedded Korean 2-bulsik Hangul IME automaton (`Hangul2BulsikAutomaton`) for hosts without
//! an OS input method (`krui-kms` / bare Linux SPI framebuffer panels) and the built-in OSK.

const CHOSEONG: [char; 19] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ',
    'ㅌ', 'ㅍ', 'ㅎ',
];

const JUNGSEONG: [char; 21] = [
    'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ',
    'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ',
];

const JONGSEONG: [char; 28] = [
    '\0', 'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ',
    'ㅁ', 'ㅂ', 'ㅄ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

/// Result of feeding one key press or backspace into [`Hangul2BulsikAutomaton`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct HangulStep {
    /// Finalized character ready to append to committed text, if any.
    pub committed: Option<char>,
    /// Current active composing syllable/jamo displayed with underline, if any.
    pub composing: Option<char>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JamoInput {
    Consonant(char),
    Vowel(char),
}

/// Platform-neutral KS X 5002 2-bulsik Hangul syllable composition finite-state machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hangul2BulsikAutomaton {
    cho: Option<usize>,
    jung: Option<usize>,
    jong: Option<usize>,
}

impl Hangul2BulsikAutomaton {
    /// Creates an empty 2-bulsik Hangul automaton.
    pub const fn new() -> Self {
        Self {
            cho: None,
            jung: None,
            jong: None,
        }
    }

    /// Reconstructs automaton state losslessly from a single active composing Unicode scalar.
    pub fn from_composing_char(ch: Option<char>) -> Self {
        let Some(c) = ch else {
            return Self::new();
        };
        let code = c as u32;
        if (0xAC00..=0xD7A3).contains(&code) {
            let offset = code - 0xAC00;
            let jong = (offset % 28) as usize;
            let jung = ((offset / 28) % 21) as usize;
            let cho = (offset / (21 * 28)) as usize;
            return Self {
                cho: Some(cho),
                jung: Some(jung),
                jong: (jong != 0).then_some(jong),
            };
        }
        Self {
            cho: cho_index(c),
            jung: jung_index(c),
            jong: None,
        }
    }

    /// Returns the currently composing syllable or compatibility jamo, if any.
    pub fn composing_char(&self) -> Option<char> {
        match (self.cho, self.jung, self.jong) {
            (Some(c), Some(v), jong_opt) => {
                let f = jong_opt.unwrap_or(0);
                let code = 0xAC00 + (c as u32) * 21 * 28 + (v as u32) * 28 + (f as u32);
                char::from_u32(code)
            }
            (Some(c), None, None) => Some(CHOSEONG[c]),
            (None, Some(v), None) => Some(JUNGSEONG[v]),
            _ => None,
        }
    }

    /// Commits and clears any active composing character (used on focus loss or non-jamo keys).
    pub fn flush(&mut self) -> Option<char> {
        let out = self.composing_char();
        *self = Self::new();
        out
    }

    /// Feeds one US-QWERTY ASCII key (`'a'..='z'`, `'A'..='Z'`) into the 2-bulsik state machine.
    pub fn push_ascii_key(&mut self, ascii: char) -> HangulStep {
        let Some(jamo) = map_2bulsik_ascii(ascii) else {
            return HangulStep {
                committed: self.flush(),
                composing: None,
            };
        };
        match jamo {
            JamoInput::Consonant(ch) => self.push_consonant(ch),
            JamoInput::Vowel(ch) => self.push_vowel(ch),
        }
    }

    /// Feeds one Korean compatibility jamo character directly (used by OSK tap buttons).
    pub fn push_jamo(&mut self, jamo: char) -> HangulStep {
        if let Some(c) = cho_index(jamo) {
            return self.push_consonant(CHOSEONG[c]);
        }
        if let Some(v) = jung_index(jamo) {
            return self.push_vowel(JUNGSEONG[v]);
        }
        HangulStep {
            committed: self.flush(),
            composing: None,
        }
    }

    /// Decomposes one jamo step backward; returns `Some(step)` while composition still consumes
    /// the backspace, or `None` when the buffer was already empty (caller deletes committed char).
    pub fn backspace(&mut self) -> Option<HangulStep> {
        if let Some(f) = self.jong {
            self.jong = split_double_jong(JONGSEONG[f]).and_then(|(a, _)| jong_index(a));
            return Some(HangulStep {
                committed: None,
                composing: self.composing_char(),
            });
        }
        if let Some(v) = self.jung {
            self.jung = split_double_vowel(JUNGSEONG[v]).and_then(|(a, _)| jung_index(a));
            return Some(HangulStep {
                committed: None,
                composing: self.composing_char(),
            });
        }
        if self.cho.is_some() {
            self.cho = None;
            return Some(HangulStep {
                committed: None,
                composing: None,
            });
        }
        None
    }

    fn push_consonant(&mut self, ch: char) -> HangulStep {
        let Some(c_idx) = cho_index(ch) else {
            return HangulStep::default();
        };
        match (self.cho, self.jung, self.jong) {
            (None, None, _) => {
                self.cho = Some(c_idx);
                HangulStep {
                    committed: None,
                    composing: self.composing_char(),
                }
            }
            (Some(_), None, None) | (None, Some(_), None) => {
                let committed = self.flush();
                self.cho = Some(c_idx);
                HangulStep {
                    committed,
                    composing: self.composing_char(),
                }
            }
            (Some(_), Some(_), None) => {
                if let Some(f_idx) = jong_index(ch) {
                    self.jong = Some(f_idx);
                    HangulStep {
                        committed: None,
                        composing: self.composing_char(),
                    }
                } else {
                    let committed = self.flush();
                    self.cho = Some(c_idx);
                    HangulStep {
                        committed,
                        composing: self.composing_char(),
                    }
                }
            }
            (Some(_), Some(_), Some(f_idx)) => {
                if let Some(merged) = merge_double_jong(JONGSEONG[f_idx], ch).and_then(jong_index) {
                    self.jong = Some(merged);
                    HangulStep {
                        committed: None,
                        composing: self.composing_char(),
                    }
                } else {
                    let committed = self.flush();
                    self.cho = Some(c_idx);
                    HangulStep {
                        committed,
                        composing: self.composing_char(),
                    }
                }
            }
            (Some(_), None, Some(_)) | (None, Some(_), Some(_)) => {
                let committed = self.flush();
                self.cho = Some(c_idx);
                HangulStep {
                    committed,
                    composing: self.composing_char(),
                }
            }
        }
    }

    fn push_vowel(&mut self, ch: char) -> HangulStep {
        let Some(v_idx) = jung_index(ch) else {
            return HangulStep::default();
        };
        match (self.cho, self.jung, self.jong) {
            (None, None, _) | (Some(_), None, _) => {
                self.jung = Some(v_idx);
                HangulStep {
                    committed: None,
                    composing: self.composing_char(),
                }
            }
            (None, Some(cur_v), _) | (Some(_), Some(cur_v), None) => {
                if let Some(merged) = merge_double_vowel(JUNGSEONG[cur_v], ch).and_then(jung_index)
                {
                    self.jung = Some(merged);
                    HangulStep {
                        committed: None,
                        composing: self.composing_char(),
                    }
                } else {
                    let committed = self.flush();
                    self.jung = Some(v_idx);
                    HangulStep {
                        committed,
                        composing: self.composing_char(),
                    }
                }
            }
            (Some(_), Some(_), Some(f_idx)) => {
                let jong_ch = JONGSEONG[f_idx];
                let (stay_jong, move_cho) = match split_double_jong(jong_ch) {
                    Some((first, second)) => (jong_index(first), cho_index(second)),
                    None => (None, cho_index(jong_ch)),
                };
                self.jong = stay_jong;
                let committed = self.flush();
                self.cho = move_cho;
                self.jung = Some(v_idx);
                HangulStep {
                    committed,
                    composing: self.composing_char(),
                }
            }
        }
    }
}

fn cho_index(ch: char) -> Option<usize> {
    CHOSEONG.iter().position(|&c| c == ch)
}

fn jung_index(ch: char) -> Option<usize> {
    JUNGSEONG.iter().position(|&c| c == ch)
}

fn jong_index(ch: char) -> Option<usize> {
    JONGSEONG
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(i, &c)| (c == ch).then_some(i))
}

fn merge_double_vowel(first: char, second: char) -> Option<char> {
    match (first, second) {
        ('ㅗ', 'ㅏ') => Some('ㅘ'),
        ('ㅗ', 'ㅐ') => Some('ㅙ'),
        ('ㅗ', 'ㅣ') => Some('ㅚ'),
        ('ㅜ', 'ㅓ') => Some('ㅝ'),
        ('ㅜ', 'ㅔ') => Some('ㅞ'),
        ('ㅜ', 'ㅣ') => Some('ㅟ'),
        ('ㅡ', 'ㅣ') => Some('ㅢ'),
        _ => None,
    }
}

fn split_double_vowel(v: char) -> Option<(char, char)> {
    match v {
        'ㅘ' => Some(('ㅗ', 'ㅏ')),
        'ㅙ' => Some(('ㅗ', 'ㅐ')),
        'ㅚ' => Some(('ㅗ', 'ㅣ')),
        'ㅝ' => Some(('ㅜ', 'ㅓ')),
        'ㅞ' => Some(('ㅜ', 'ㅔ')),
        'ㅟ' => Some(('ㅜ', 'ㅣ')),
        'ㅢ' => Some(('ㅡ', 'ㅣ')),
        _ => None,
    }
}

fn merge_double_jong(first: char, second: char) -> Option<char> {
    match (first, second) {
        ('ㄱ', 'ㅅ') => Some('ㄳ'),
        ('ㄴ', 'ㅈ') => Some('ㄵ'),
        ('ㄴ', 'ㅎ') => Some('ㄶ'),
        ('ㄹ', 'ㄱ') => Some('ㄺ'),
        ('ㄹ', 'ㅁ') => Some('ㄻ'),
        ('ㄹ', 'ㅂ') => Some('ㄼ'),
        ('ㄹ', 'ㅅ') => Some('ㄽ'),
        ('ㄹ', 'ㅌ') => Some('ㄾ'),
        ('ㄹ', 'ㅍ') => Some('ㄿ'),
        ('ㄹ', 'ㅎ') => Some('ㅀ'),
        ('ㅂ', 'ㅅ') => Some('ㅄ'),
        _ => None,
    }
}

fn split_double_jong(f: char) -> Option<(char, char)> {
    match f {
        'ㄳ' => Some(('ㄱ', 'ㅅ')),
        'ㄵ' => Some(('ㄴ', 'ㅈ')),
        'ㄶ' => Some(('ㄴ', 'ㅎ')),
        'ㄺ' => Some(('ㄹ', 'ㄱ')),
        'ㄻ' => Some(('ㄹ', 'ㅁ')),
        'ㄼ' => Some(('ㄹ', 'ㅂ')),
        'ㄽ' => Some(('ㄹ', 'ㅅ')),
        'ㄾ' => Some(('ㄹ', 'ㅌ')),
        'ㄿ' => Some(('ㄹ', 'ㅍ')),
        'ㅀ' => Some(('ㄹ', 'ㅎ')),
        'ㅄ' => Some(('ㅂ', 'ㅅ')),
        _ => None,
    }
}

fn map_2bulsik_ascii(ascii: char) -> Option<JamoInput> {
    match ascii {
        'r' => Some(JamoInput::Consonant('ㄱ')),
        'R' => Some(JamoInput::Consonant('ㄲ')),
        's' => Some(JamoInput::Consonant('ㄴ')),
        'e' => Some(JamoInput::Consonant('ㄷ')),
        'E' => Some(JamoInput::Consonant('ㄸ')),
        'f' => Some(JamoInput::Consonant('ㄹ')),
        'a' => Some(JamoInput::Consonant('ㅁ')),
        'q' => Some(JamoInput::Consonant('ㅂ')),
        'Q' => Some(JamoInput::Consonant('ㅃ')),
        't' => Some(JamoInput::Consonant('ㅅ')),
        'T' => Some(JamoInput::Consonant('ㅆ')),
        'd' => Some(JamoInput::Consonant('ㅇ')),
        'w' => Some(JamoInput::Consonant('ㅈ')),
        'W' => Some(JamoInput::Consonant('ㅉ')),
        'c' => Some(JamoInput::Consonant('ㅊ')),
        'z' => Some(JamoInput::Consonant('ㅋ')),
        'x' => Some(JamoInput::Consonant('ㅌ')),
        'v' => Some(JamoInput::Consonant('ㅍ')),
        'g' => Some(JamoInput::Consonant('ㅎ')),
        'k' => Some(JamoInput::Vowel('ㅏ')),
        'o' => Some(JamoInput::Vowel('ㅐ')),
        'i' => Some(JamoInput::Vowel('ㅑ')),
        'O' => Some(JamoInput::Vowel('ㅒ')),
        'j' => Some(JamoInput::Vowel('ㅓ')),
        'p' => Some(JamoInput::Vowel('ㅔ')),
        'u' => Some(JamoInput::Vowel('ㅕ')),
        'P' => Some(JamoInput::Vowel('ㅖ')),
        'h' => Some(JamoInput::Vowel('ㅗ')),
        'y' => Some(JamoInput::Vowel('ㅛ')),
        'n' => Some(JamoInput::Vowel('ㅜ')),
        'b' => Some(JamoInput::Vowel('ㅠ')),
        'm' => Some(JamoInput::Vowel('ㅡ')),
        'l' => Some(JamoInput::Vowel('ㅣ')),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_seq(seq: &str) -> String {
        let mut auto = Hangul2BulsikAutomaton::new();
        let mut out = String::new();
        for ch in seq.chars() {
            let step = auto.push_ascii_key(ch);
            if let Some(c) = step.committed {
                out.push(c);
            }
        }
        if let Some(tail) = auto.flush() {
            out.push(tail);
        }
        out
    }

    #[test]
    fn composes_basic_word_complex_jongseong_and_syllable_split() {
        assert_eq!(type_seq("gksrmf"), "한글");
        assert_eq!(type_seq("ekfr"), "닭");
        assert_eq!(type_seq("qkfrdk"), "밝아");
        assert_eq!(type_seq("dhkqh"), "와보");
    }

    #[test]
    fn backspace_decomposes_double_jongseong_step_by_step() {
        let mut auto = Hangul2BulsikAutomaton::new();
        for ch in "ekfr".chars() {
            let _ = auto.push_ascii_key(ch);
        }
        assert_eq!(auto.composing_char(), Some('닭'));
        assert_eq!(auto.backspace().unwrap().composing, Some('달'));
        assert_eq!(auto.backspace().unwrap().composing, Some('다'));
        assert_eq!(auto.backspace().unwrap().composing, Some('ㄷ'));
        assert_eq!(auto.backspace().unwrap().composing, None);
        assert_eq!(auto.backspace(), None);
    }
}
