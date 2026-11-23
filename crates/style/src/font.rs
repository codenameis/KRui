//! Multilingual font family stacks and thread-safe custom CSS `font-family` interning.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Maximum number of distinct custom font family stacks interned in a process.
const MAX_CUSTOM_FONT_STACKS: usize = u16::MAX as usize;

/// Multilingual font family stack matched to KRui design systems or interned custom CSS font lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontFamilyStack {
    /// **Fluent 2 (Windows 11)**: `"Segoe UI"` + `"Malgun Gothic"` / `"맑은 고딕"` + `"Yu Gothic UI"`.
    #[default]
    FluentUI,
    /// **Material Design 3 (Google)**: `"Roboto"` / `"Trebuchet MS"` + `"Noto Sans KR"`.
    MaterialSans,
    /// **Liquid Glass (Apple Spatial UI Sans)**: `"SF Pro Display"` / `"Inter"` / `"Segoe UI Variable"` + `"Apple SD Gothic Neo"` / `"Malgun Gothic"`.
    LiquidGlassSans,
    /// **Cosmic (Flat Cybernetic HUD)**: `"Cascadia Code"` / `"Consolas"` + `"D2Coding"`.
    CosmicMono,
    /// **Explicit Editorial Serif (`font-family: Georgia, serif`)**: `"Georgia"` / `"Sitka Text"` + `"Batang"` / `"AppleMyungjo"`.
    Serif,
    /// **Interned Custom CSS Font Stack**: arbitrary user-declared `font-family` list (e.g. `"Pretendard", "Inter", sans-serif`).
    Custom(u16),
}

#[derive(Default)]
struct FontInterner {
    entries: Vec<&'static [&'static str]>,
    lookup: HashMap<Vec<String>, u16>,
}

fn font_interner() -> &'static RwLock<FontInterner> {
    static INTERNER: OnceLock<RwLock<FontInterner>> = OnceLock::new();
    INTERNER.get_or_init(|| RwLock::new(FontInterner::default()))
}

fn clean_family_token(raw: &str) -> &str {
    raw.trim().trim_matches(|c| c == '"' || c == '\'').trim()
}

fn is_builtin_serif_family(lower: &str) -> bool {
    matches!(
        lower,
        "georgia"
            | "sitka text"
            | "cambria"
            | "times new roman"
            | "times"
            | "batang"
            | "바탕"
            | "applemyungjo"
            | "serif"
            | "ui-serif"
    )
}

impl FontFamilyStack {
    /// Interns a custom ordered CSS `font-family` list (deduplicating identical lists)
    /// and returns a `Copy` [`FontFamilyStack::Custom`] handle.
    pub fn intern_custom<S: AsRef<str>>(families: &[S]) -> Self {
        let cleaned: Vec<String> = families
            .iter()
            .map(|s| clean_family_token(s.as_ref()))
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if cleaned.is_empty() {
            return Self::FluentUI;
        }
        if let Ok(guard) = font_interner().read() {
            if let Some(&id) = guard.lookup.get(&cleaned) {
                return Self::Custom(id);
            }
        }
        let Ok(mut guard) = font_interner().write() else {
            return Self::FluentUI;
        };
        if let Some(&id) = guard.lookup.get(&cleaned) {
            return Self::Custom(id);
        }
        if guard.entries.len() >= MAX_CUSTOM_FONT_STACKS {
            return Self::FluentUI;
        }
        let leaked_strs: Vec<&'static str> = cleaned
            .iter()
            .map(|s| Box::leak(s.clone().into_boxed_str()) as &'static str)
            .collect();
        let leaked_slice: &'static [&'static str] = Box::leak(leaked_strs.into_boxed_slice());
        let id = guard.entries.len() as u16;
        guard.entries.push(leaked_slice);
        guard.lookup.insert(cleaned, id);
        Self::Custom(id)
    }

    /// Returns the interned slice of family names if `self` is [`FontFamilyStack::Custom`], or `None` for built-in stacks.
    pub fn custom_families(self) -> Option<&'static [&'static str]> {
        let Self::Custom(id) = self else {
            return None;
        };
        let guard = font_interner().read().ok()?;
        guard.entries.get(id as usize).copied()
    }

    /// Parses a CSS `font-family` declaration into a built-in preset stack, [`Self::Serif`], or an interned [`Self::Custom`] stack.
    pub fn parse_css(raw: &str) -> Self {
        let items: Vec<&str> = raw
            .split(',')
            .map(clean_family_token)
            .filter(|s| !s.is_empty())
            .collect();
        if items.is_empty() {
            return Self::FluentUI;
        }
        for &item in &items {
            match item.to_ascii_lowercase().as_str() {
                "fluent-ui" => return Self::FluentUI,
                "material-sans" => return Self::MaterialSans,
                "liquid-glass-sans" | "liquid-glass" => return Self::LiquidGlassSans,
                "cosmic-mono" => return Self::CosmicMono,
                "liquid-glass-serif" => return Self::Serif,
                _ => {}
            }
        }
        if items.len() == 1 {
            match items[0].to_ascii_lowercase().as_str() {
                "sans-serif" => return Self::FluentUI,
                "serif" | "ui-serif" => return Self::Serif,
                "monospace" | "ui-monospace" => return Self::CosmicMono,
                _ => {}
            }
        }
        if items
            .iter()
            .all(|s| is_builtin_serif_family(&s.to_ascii_lowercase()))
        {
            return Self::Serif;
        }
        Self::intern_custom(&items)
    }

    /// Human-readable display label for diagnostics and `.notdef` glyph warnings.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::FluentUI => "Segoe UI + 맑은 고딕 (Fluent Neo-Grotesque)",
            Self::MaterialSans => "Roboto/Trebuchet + Noto Sans KR (Material Humanist)",
            Self::LiquidGlassSans => {
                "SF Pro/Inter + Apple SD 산돌고딕/맑은 고딕 (Liquid Glass Sans)"
            }
            Self::CosmicMono => "Cascadia Code + D2Coding (Cybernetic Monospace)",
            Self::Serif => "Georgia + 바탕/명조 (Editorial Serif)",
            Self::Custom(_) => self
                .custom_families()
                .and_then(|f| f.first().copied())
                .unwrap_or("Custom Font Stack"),
        }
    }
}

#[cfg(feature = "preview")]
#[derive(serde::Serialize, serde::Deserialize)]
enum FontFamilyWire {
    FluentUI,
    MaterialSans,
    LiquidGlassSans,
    CosmicMono,
    Serif,
    Custom(Vec<String>),
}

#[cfg(feature = "preview")]
impl serde::Serialize for FontFamilyStack {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match *self {
            Self::FluentUI => FontFamilyWire::FluentUI,
            Self::MaterialSans => FontFamilyWire::MaterialSans,
            Self::LiquidGlassSans => FontFamilyWire::LiquidGlassSans,
            Self::CosmicMono => FontFamilyWire::CosmicMono,
            Self::Serif => FontFamilyWire::Serif,
            Self::Custom(_) => {
                let list = self
                    .custom_families()
                    .unwrap_or(&[])
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect();
                FontFamilyWire::Custom(list)
            }
        };
        wire.serialize(serializer)
    }
}

#[cfg(feature = "preview")]
impl<'de> serde::Deserialize<'de> for FontFamilyStack {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match FontFamilyWire::deserialize(deserializer)? {
            FontFamilyWire::FluentUI => Ok(Self::FluentUI),
            FontFamilyWire::MaterialSans => Ok(Self::MaterialSans),
            FontFamilyWire::LiquidGlassSans => Ok(Self::LiquidGlassSans),
            FontFamilyWire::CosmicMono => Ok(Self::CosmicMono),
            FontFamilyWire::Serif => Ok(Self::Serif),
            FontFamilyWire::Custom(list) => Ok(Self::intern_custom(&list)),
        }
    }
}
