//! CSS Fonts 4 `@font-face` rule descriptors (`ascent-override`, `descent-override`,
//! `line-gap-override`, weight ranges `1..=1000`, `unicode-range`, and local/embedded sources).

use crate::typography::parse_css_font_weight;
use std::sync::Arc;

/// CSS `@font-face` `font-style` slope classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum FontStyleSlope {
    /// Upright `normal`.
    #[default]
    Normal,
    /// Cursive `italic`.
    Italic,
    /// Sloped `oblique`.
    Oblique,
}

impl FontStyleSlope {
    /// Parses a CSS `font-style` descriptor value.
    pub fn parse_css(raw: &str) -> Option<Self> {
        let first = raw.split_whitespace().next()?.to_ascii_lowercase();
        match first.as_str() {
            "normal" => Some(Self::Normal),
            "italic" => Some(Self::Italic),
            "oblique" => Some(Self::Oblique),
            _ => None,
        }
    }
}

/// Inclusive Unicode codepoint interval (`U+start-end`) in a `@font-face` `unicode-range` descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct UnicodeRange {
    /// Inclusive starting Unicode scalar value.
    pub start: u32,
    /// Inclusive ending Unicode scalar value (`<= 0x10FFFF`).
    pub end: u32,
}

impl UnicodeRange {
    /// Returns `true` when `cp` lies within `[start, end]`.
    #[inline]
    pub const fn contains(self, cp: u32) -> bool {
        cp >= self.start && cp <= self.end
    }

    /// Parses a comma-separated CSS `unicode-range` list (`U+0041`, `U+0000-00FF`, `U+00??`).
    pub fn parse_css_list(raw: &str) -> Option<Vec<Self>> {
        let mut out = Vec::new();
        for item in raw.split(',') {
            let tok = item.trim();
            if tok.is_empty() {
                return None;
            }
            out.push(Self::parse_single(tok)?);
        }
        (!out.is_empty()).then_some(out)
    }

    fn parse_single(tok: &str) -> Option<Self> {
        let body = tok.strip_prefix("U+").or_else(|| tok.strip_prefix("u+"))?;
        if body.is_empty() {
            return None;
        }
        if let Some((lo_s, hi_s)) = body.split_once('-') {
            if lo_s.is_empty() || hi_s.is_empty() || lo_s.len() > 6 || hi_s.len() > 6 {
                return None;
            }
            let start = u32::from_str_radix(lo_s, 16).ok()?;
            let end = u32::from_str_radix(hi_s, 16).ok()?;
            if start > end || end > 0x10FFFF {
                return None;
            }
            return Some(Self { start, end });
        }
        if body.len() > 6 {
            return None;
        }
        let q_count = body.bytes().rev().take_while(|&b| b == b'?').count();
        let hex_prefix = &body[..body.len() - q_count];
        if hex_prefix.bytes().any(|b| b == b'?') {
            return None;
        }
        if q_count == 0 {
            let cp = u32::from_str_radix(body, 16).ok()?;
            return (cp <= 0x10FFFF).then_some(Self { start: cp, end: cp });
        }
        let low_hex = format!("{hex_prefix}{}", "0".repeat(q_count));
        let high_hex = format!("{hex_prefix}{}", "F".repeat(q_count));
        let start = u32::from_str_radix(&low_hex, 16).ok()?;
        let end = u32::from_str_radix(&high_hex, 16).ok()?.min(0x10FFFF);
        (start <= end).then_some(Self { start, end })
    }
}

/// Optional `@font-face` vertical metric overrides (`ascent-override`, `descent-override`, `line-gap-override`),
/// stored as non-negative em-relative fractions (`90%` -> `Some(0.90)`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct FontFaceMetricsOverride {
    /// Overridden ascent above baseline as a fraction of `font-size` (`None` = `normal`).
    pub ascent_pct: Option<f32>,
    /// Overridden descent below baseline as a fraction of `font-size` (`None` = `normal`).
    pub descent_pct: Option<f32>,
    /// Overridden line gap as a fraction of `font-size` (`None` = `normal`).
    pub line_gap_pct: Option<f32>,
}

impl FontFaceMetricsOverride {
    /// Default `normal` metric overrides (all `None`).
    pub const EMPTY: Self = Self {
        ascent_pct: None,
        descent_pct: None,
        line_gap_pct: None,
    };

    /// Returns `true` when at least one metric override is specified.
    #[inline]
    pub const fn has_any(self) -> bool {
        self.ascent_pct.is_some() || self.descent_pct.is_some() || self.line_gap_pct.is_some()
    }

    /// Parses a CSS metric-override value (`normal` -> `Some(None)`, `92%` -> `Some(Some(0.92))`).
    pub fn parse_descriptor(raw: &str) -> Option<Option<f32>> {
        let s = raw.trim();
        if s.eq_ignore_ascii_case("normal") {
            return Some(None);
        }
        let pct_s = s.strip_suffix('%')?.trim();
        let pct = pct_s
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)?;
        Some(Some(pct * 0.01))
    }
}

/// Resolved `@font-face` font data source (`local(...)` system font name or embedded binary bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontFaceSource {
    /// System-installed font matched via `local("Font Name")`.
    Local(String),
    /// Embedded TTF/OTF binary bytes resolved at compile time from `url(...)` or `data:`.
    Embedded(Arc<[u8]>),
}

#[cfg(feature = "preview")]
#[derive(serde::Serialize, serde::Deserialize)]
enum FontFaceSourceWire {
    Local(String),
    Embedded(Vec<u8>),
}

#[cfg(feature = "preview")]
impl serde::Serialize for FontFaceSource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            Self::Local(name) => FontFaceSourceWire::Local(name.clone()),
            Self::Embedded(bytes) => FontFaceSourceWire::Embedded(bytes.to_vec()),
        };
        wire.serialize(serializer)
    }
}

#[cfg(feature = "preview")]
impl<'de> serde::Deserialize<'de> for FontFaceSource {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match FontFaceSourceWire::deserialize(deserializer)? {
            FontFaceSourceWire::Local(name) => Ok(Self::Local(name)),
            FontFaceSourceWire::Embedded(bytes) => Ok(Self::Embedded(Arc::from(bytes))),
        }
    }
}

/// Font binary container format hint for `@font-face` sources (`SFNT` vs compressed `WOFF2`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum FontContainerFormat {
    /// Uncompressed OpenType/TrueType SFNT font table bytes.
    #[default]
    Sfnt,
    /// WOFF2-compressed font container bytes (decoded at compile/preview load time).
    Woff2,
}

/// Compiled CSS `@font-face` rule shared identically between `krui-macros` and `krui-preview`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct FontFaceRule {
    /// Declared CSS `font-family` name (quotes stripped).
    pub family: String,
    /// Ordered fallback sources (`local(...)` and/or embedded binary bytes).
    pub sources: Vec<FontFaceSource>,
    /// Container format hint (`Sfnt` or `Woff2`).
    pub container_format: FontContainerFormat,
    /// Inclusive lower bound of `font-weight` (`1..=1000`, default `400`).
    pub weight_min: u16,
    /// Inclusive upper bound of `font-weight` (`1..=1000`, default `400`).
    pub weight_max: u16,
    /// Declared `font-style` slope.
    pub style: FontStyleSlope,
    /// Allowed Unicode codepoint intervals (empty = `U+0000-10FFFF`).
    pub unicode_ranges: Vec<UnicodeRange>,
    /// Optional `ascent-override`, `descent-override`, and `line-gap-override` descriptors.
    pub metrics_override: FontFaceMetricsOverride,
}

impl FontFaceRule {
    /// Returns `true` when `cp` is covered by `unicode_ranges` (or when `unicode_ranges` is empty).
    #[inline]
    pub fn matches_codepoint(&self, cp: u32) -> bool {
        self.unicode_ranges.is_empty() || self.unicode_ranges.iter().any(|r| r.contains(cp))
    }

    /// Returns the absolute distance from `requested_weight` (`1..=1000`) to `[weight_min, weight_max]` (`0` when covered).
    #[inline]
    pub fn weight_distance(&self, requested_weight: u16) -> u16 {
        if requested_weight < self.weight_min {
            self.weight_min - requested_weight
        } else {
            requested_weight.saturating_sub(self.weight_max)
        }
    }

    /// Parses CSS Fonts 4 `@font-face` `font-weight` descriptor (`normal`, `bold`, `500`, or `100 900` range).
    pub fn parse_weight_range(raw: &str) -> Option<(u16, u16)> {
        let toks: Vec<&str> = raw.split_whitespace().collect();
        match toks.as_slice() {
            ["auto"] => Some((1, 1000)),
            [single] => {
                let w = parse_css_font_weight(single)?;
                Some((w, w))
            }
            [a, b] => {
                let w0 = parse_css_font_weight(a)?;
                let w1 = parse_css_font_weight(b)?;
                Some((w0.min(w1), w0.max(w1)))
            }
            _ => None,
        }
    }
}
