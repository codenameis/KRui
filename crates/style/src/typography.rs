//! CSS Inline Layout 3 (`line-height`, `text-box-trim`, `text-box-edge`, `text-box`) and
//! CSS Fonts 4 (`font-weight: 1..1000`, `font-variation-settings`) `Copy` domain types.

use crate::DEFAULT_LINE_HEIGHT;

/// Maximum number of OpenType variation axes (`fvar`) stored inline in [`FontVariationList`].
pub const MAX_FONT_VARIATIONS: usize = 6;

/// CSS `line-height` value preserving W3C inheritance semantics (`normal`, unitless factor,
/// computed pixel length, or pre-cascade font-relative `em`/`%` expression).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum LineHeight {
    /// `line-height: normal` — uses the font's typographic `ascent + descent + line_gap` and inherits as `Normal`.
    Normal,
    /// Unitless multiplier (`line-height: 1.5`) — inherits as `Factor(1.5)` and multiplies each descendant's `font-size`.
    Factor(f32),
    /// Computed absolute length in logical pixels (`20px`, `1.25rem`, or resolved `em`/`%`) — inherits as fixed `Px`.
    Px(f32),
    /// Element-relative `em` / `%` (plus optional `px` offset from `calc(1.2em + 4px)`) resolved against
    /// the declaring element's final `font-size` before inheritance.
    FontRelative {
        /// Multiplier of the declaring element's computed `font-size` (`150%` -> `1.5`, `1.2em` -> `1.2`).
        em_factor: f32,
        /// Fixed logical-pixel additive term (`0.0` for plain `em`/`%`).
        px_offset: f32,
    },
}

impl Default for LineHeight {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl LineHeight {
    /// Default KRui line-height (`Factor(1.4)`).
    pub const DEFAULT: Self = Self::Factor(DEFAULT_LINE_HEIGHT);

    /// Resolves [`Self::FontRelative`] into [`Self::Px`] using the declaring element's final `font-size`,
    /// preserving [`Self::Normal`], [`Self::Factor`], and [`Self::Px`] for W3C child inheritance.
    #[inline]
    pub fn resolve_on_element(self, element_font_size: f32) -> Self {
        match self {
            Self::FontRelative {
                em_factor,
                px_offset,
            } => Self::Px((element_font_size.max(0.0) * em_factor + px_offset).max(0.0)),
            other => other,
        }
    }

    /// Evaluates the concrete line-box height in logical pixels for a given `font_size` and font `normal_px`.
    #[inline]
    pub fn resolve_px(self, font_size: f32, normal_px: f32) -> f32 {
        match self {
            Self::Normal => normal_px.max(0.0),
            Self::Factor(f) => (font_size.max(0.0) * f.max(0.0)).max(0.0),
            Self::Px(px) => px.max(0.0),
            Self::FontRelative {
                em_factor,
                px_offset,
            } => (font_size.max(0.0) * em_factor + px_offset).max(0.0),
        }
    }

    /// Parses non-`calc()` CSS `line-height` tokens (`normal`, `<number>`, `px`, `rem`, `pt`, `em`, `%`).
    pub fn parse_css_simple(raw: &str, rem_px: f32) -> Option<Self> {
        let s = raw.trim();
        if s.is_empty() {
            return None;
        }
        let lower = s.to_ascii_lowercase();
        if lower == "normal" {
            return Some(Self::Normal);
        }
        if let Some(v) = lower.strip_suffix("rem") {
            let n = v
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|n| n.is_finite() && *n >= 0.0)?;
            return Some(Self::Px(n * rem_px));
        }
        if let Some(v) = lower.strip_suffix("px") {
            let n = v
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|n| n.is_finite() && *n >= 0.0)?;
            return Some(Self::Px(n));
        }
        if let Some(v) = lower.strip_suffix("pt") {
            let n = v
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|n| n.is_finite() && *n >= 0.0)?;
            return Some(Self::Px(n * (96.0 / 72.0)));
        }
        if let Some(v) = lower.strip_suffix("em") {
            let n = v
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|n| n.is_finite() && *n >= 0.0)?;
            return Some(Self::FontRelative {
                em_factor: n,
                px_offset: 0.0,
            });
        }
        if let Some(v) = lower.strip_suffix('%') {
            let n = v
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|n| n.is_finite() && *n >= 0.0)?;
            return Some(Self::FontRelative {
                em_factor: n * 0.01,
                px_offset: 0.0,
            });
        }
        let n = lower
            .parse::<f32>()
            .ok()
            .filter(|n| n.is_finite() && *n >= 0.0)?;
        Some(Self::Factor(n))
    }
}

/// CSS Inline Layout 3 `text-box-trim` property (`none | trim-start | trim-end | trim-both`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TextBoxTrim {
    /// Keep standard half-leading above the first line and below the last line.
    #[default]
    None,
    /// Trim leading above the first formatted line to [`TextBoxEdge::over`].
    TrimStart,
    /// Trim leading below the last formatted line to [`TextBoxEdge::under`].
    TrimEnd,
    /// Trim both over-edge of the first line and under-edge of the last line.
    TrimBoth,
}

impl TextBoxTrim {
    /// Parses a CSS `text-box-trim` keyword.
    pub fn parse_css(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "trim-start" => Some(Self::TrimStart),
            "trim-end" => Some(Self::TrimEnd),
            "trim-both" => Some(Self::TrimBoth),
            _ => None,
        }
    }
}

/// Over-edge reference metric for CSS `text-box-edge`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TextOverEdge {
    /// `auto` (behaves as `text` ascent bound).
    #[default]
    Auto,
    /// Font ascent bound (`text`).
    Text,
    /// Capital-letter top bound (`cap`, OpenType `OS/2.sCapHeight`).
    Cap,
    /// Lowercase x-height top bound (`ex`, OpenType `OS/2.sxHeight`).
    Ex,
    /// CJK ideographic em-box top bound (`ideographic`).
    Ideographic,
    /// CJK ideographic ink top bound (`ideographic-ink`).
    IdeographicInk,
}

/// Under-edge reference metric for CSS `text-box-edge`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TextUnderEdge {
    /// `auto` (behaves as `text` descent bound).
    #[default]
    Auto,
    /// Font descent bound (`text`).
    Text,
    /// Alphabetic baseline (`alphabetic`, zero distance below baseline).
    Alphabetic,
    /// CJK ideographic em-box bottom bound (`ideographic`).
    Ideographic,
    /// CJK ideographic ink bottom bound (`ideographic-ink`).
    IdeographicInk,
}

/// CSS Inline Layout 3 `text-box-edge` pair (`<over-edge> <under-edge>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct TextBoxEdge {
    /// Over-edge metric applied when trimming block-start.
    pub over: TextOverEdge,
    /// Under-edge metric applied when trimming block-end.
    pub under: TextUnderEdge,
}

impl TextBoxEdge {
    /// Initial CSS value (`auto`).
    pub const AUTO: Self = Self {
        over: TextOverEdge::Auto,
        under: TextUnderEdge::Auto,
    };

    /// Parses a CSS `text-box-edge` value (`auto`, single keyword, or `<over> <under>` pair).
    pub fn parse_css(raw: &str) -> Option<Self> {
        let toks: Vec<String> = raw
            .split_whitespace()
            .map(|t| t.to_ascii_lowercase())
            .collect();
        match toks.as_slice() {
            [single] => match single.as_str() {
                "auto" | "leading" => Some(Self::AUTO),
                "text" => Some(Self {
                    over: TextOverEdge::Text,
                    under: TextUnderEdge::Text,
                }),
                "cap" => Some(Self {
                    over: TextOverEdge::Cap,
                    under: TextUnderEdge::Text,
                }),
                "ex" => Some(Self {
                    over: TextOverEdge::Ex,
                    under: TextUnderEdge::Text,
                }),
                "ideographic" => Some(Self {
                    over: TextOverEdge::Ideographic,
                    under: TextUnderEdge::Ideographic,
                }),
                "ideographic-ink" => Some(Self {
                    over: TextOverEdge::IdeographicInk,
                    under: TextUnderEdge::IdeographicInk,
                }),
                _ => None,
            },
            [over_raw, under_raw] => {
                let over = match over_raw.as_str() {
                    "text" => TextOverEdge::Text,
                    "cap" => TextOverEdge::Cap,
                    "ex" => TextOverEdge::Ex,
                    "ideographic" => TextOverEdge::Ideographic,
                    "ideographic-ink" => TextOverEdge::IdeographicInk,
                    _ => return None,
                };
                let under = match under_raw.as_str() {
                    "text" => TextUnderEdge::Text,
                    "alphabetic" => TextUnderEdge::Alphabetic,
                    "ideographic" => TextUnderEdge::Ideographic,
                    "ideographic-ink" => TextUnderEdge::IdeographicInk,
                    _ => return None,
                };
                Some(Self { over, under })
            }
            _ => None,
        }
    }
}

/// Parses CSS Inline Layout 3 `text-box` shorthand (`normal | <text-box-trim> || <text-box-edge>`).
pub fn parse_text_box_shorthand(raw: &str) -> Option<(TextBoxTrim, TextBoxEdge)> {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("normal") {
        return Some((TextBoxTrim::None, TextBoxEdge::AUTO));
    }
    let toks: Vec<&str> = trimmed.split_whitespace().collect();
    if toks.is_empty() || toks.len() > 3 {
        return None;
    }
    if let Some(t) = TextBoxTrim::parse_css(toks[0]) {
        if toks.len() == 1 {
            return Some((t, TextBoxEdge::AUTO));
        }
        let edge = TextBoxEdge::parse_css(&toks[1..].join(" "))?;
        return Some((t, edge));
    }
    if let Some(&last) = toks.last() {
        if let Some(t) = TextBoxTrim::parse_css(last) {
            let edge = TextBoxEdge::parse_css(&toks[..toks.len() - 1].join(" "))?;
            return Some((t, edge));
        }
    }
    let edge = TextBoxEdge::parse_css(trimmed)?;
    Some((TextBoxTrim::TrimBoth, edge))
}

/// Single OpenType `fvar` variation axis setting (`tag` + numeric coordinate).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct FontVariation {
    /// Four-byte ASCII OpenType axis tag (e.g. `*b"wght"`, `*b"FILL"`, `*b"GRAD"`, `*b"opsz"`).
    pub tag: [u8; 4],
    /// User-space variation coordinate value.
    pub value: f32,
}

/// Fixed-capacity `Copy` list of CSS `font-variation-settings` axis overrides.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct FontVariationList {
    /// Inline storage of axis settings (`0..len` active).
    pub items: [FontVariation; MAX_FONT_VARIATIONS],
    /// Number of active entries in `items`.
    pub len: u8,
}

impl FontVariationList {
    /// Empty `font-variation-settings: normal` list.
    pub const EMPTY: Self = Self {
        items: [FontVariation {
            tag: [b' '; 4],
            value: 0.0,
        }; MAX_FONT_VARIATIONS],
        len: 0,
    };

    /// Active slice of OpenType variation settings.
    #[inline]
    pub fn as_slice(&self) -> &[FontVariation] {
        &self.items[..(self.len as usize).min(MAX_FONT_VARIATIONS)]
    }

    /// Returns `true` when no explicit `font-variation-settings` are active.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Parses CSS `font-variation-settings` (`normal` or comma-separated `"TAG" <number>` pairs).
    pub fn parse_css(raw: &str) -> Option<Self> {
        let s = raw.trim();
        if s.is_empty() {
            return None;
        }
        if s.eq_ignore_ascii_case("normal") {
            return Some(Self::EMPTY);
        }
        let mut out = Self::EMPTY;
        for entry in s.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            let (tag, rest) = parse_quoted_axis_tag(entry)?;
            let val = rest.trim().parse::<f32>().ok().filter(|v| v.is_finite())?;
            out.upsert(tag, val);
        }
        Some(out)
    }

    /// Inserts or replaces an axis tag setting in declaration order.
    pub fn upsert(&mut self, tag: [u8; 4], value: f32) {
        let n = (self.len as usize).min(MAX_FONT_VARIATIONS);
        for item in &mut self.items[..n] {
            if item.tag == tag {
                item.value = value;
                return;
            }
        }
        if n < MAX_FONT_VARIATIONS {
            self.items[n] = FontVariation { tag, value };
            self.len = (n + 1) as u8;
        }
    }
}

fn parse_quoted_axis_tag(entry: &str) -> Option<([u8; 4], &str)> {
    let quote = entry.chars().next().filter(|&c| c == '"' || c == '\'')?;
    let after_open = &entry[quote.len_utf8()..];
    let close_idx = after_open.find(quote)?;
    let tag_str = &after_open[..close_idx];
    let bytes = tag_str.as_bytes();
    if bytes.len() != 4 || !bytes.iter().all(|b| (0x20..=0x7E).contains(b)) {
        return None;
    }
    let rest = &after_open[close_idx + quote.len_utf8()..];
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(([bytes[0], bytes[1], bytes[2], bytes[3]], rest))
}

/// CSS Text 3 `white-space` collapsing and wrapping mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum WhiteSpaceMode {
    /// Collapses ASCII whitespace runs and wraps lines (`white-space: normal`).
    Normal,
    /// Collapses ASCII whitespace runs and suppresses soft line wrapping (`white-space: nowrap`).
    NoWrap,
    /// Preserves spaces and newlines and suppresses soft line wrapping (`white-space: pre`).
    Pre,
    /// Preserves spaces and newlines and wraps lines (`white-space: pre-wrap`, raw Rust default).
    #[default]
    PreWrap,
    /// Collapses spaces/tabs, preserves newlines, and wraps lines (`white-space: pre-line`).
    PreLine,
    /// Preserves spaces/newlines and wraps at spaces (`white-space: break-spaces`).
    BreakSpaces,
}

impl WhiteSpaceMode {
    /// Returns `true` when soft line wrapping is permitted at container `max-width`.
    #[inline]
    pub const fn allows_wrap(self) -> bool {
        !matches!(self, Self::NoWrap | Self::Pre)
    }

    /// Returns `true` when runs of collapsible spaces/tabs should be collapsed.
    #[inline]
    pub const fn collapses_spaces(self) -> bool {
        matches!(self, Self::Normal | Self::NoWrap | Self::PreLine)
    }

    /// Returns `true` when explicit `\n` line breaks are preserved.
    #[inline]
    pub const fn preserves_newlines(self) -> bool {
        !matches!(self, Self::Normal | Self::NoWrap)
    }
}

/// CSS Text 3 `word-break` mode (`normal | break-all | keep-all`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum WordBreakMode {
    /// Standard UAX #14 word breaking (`word-break: normal`).
    #[default]
    Normal,
    /// Allows soft breaks between any two characters (`word-break: break-all`).
    BreakAll,
    /// Suppresses implicit mid-word CJK/Hangul breaks (`word-break: keep-all`).
    KeepAll,
}

/// CSS Text 3 `overflow-wrap` / `word-wrap` mode (`normal | break-word | anywhere`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum OverflowWrapMode {
    /// Only break at allowed word break points (`overflow-wrap: normal`).
    #[default]
    Normal,
    /// Break otherwise-unbreakable words if a line has no other break points (`break-word`).
    BreakWord,
    /// Break otherwise-unbreakable words anywhere if needed (`anywhere`).
    Anywhere,
}

/// Parses CSS Fonts 4 `font-weight` (`normal`, `medium`, `semibold`, `bold`, or numeric `1..=1000`).
pub fn parse_css_font_weight(raw: &str) -> Option<u16> {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "normal" => Some(400),
        "medium" => Some(500),
        "semibold" => Some(600),
        "bold" => Some(700),
        other => {
            let v = other.parse::<f32>().ok().filter(|n| n.is_finite())?;
            if !(1.0..=1000.0).contains(&v) {
                return None;
            }
            Some(v.round().clamp(1.0, 1000.0) as u16)
        }
    }
}
