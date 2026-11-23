//! Compositor-level CSS `transition` parsing and non-layout paint interpolation.

pub use crate::easing::{LinearCurveRef, LinearStop, StepPosition, TransitionCurve};
use crate::transition_clauses::TransitionExtraClauses;
use crate::transition_layout::{layout_property_bit, LayoutTransitionMask};
pub use crate::transition_lerp::{
    apply_masked_lerp, interpolate_paint, lerp_color, lerp_f32, lerp_opt_shadow, lerp_transform,
    paint_differs_on_mask, sample_paint_clauses, solve_cubic_bezier,
};

/// Bitmask of visual `PaintStyle` properties eligible for zero-relayout transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct TransitionMask(pub u16);

impl TransitionMask {
    /// No transitioned properties.
    pub const NONE: Self = Self(0);
    /// `transform` transition bit.
    pub const TRANSFORM: u16 = 1 << 0;
    /// `opacity` transition bit.
    pub const OPACITY: u16 = 1 << 1;
    /// `background` / `background-color` transition bit.
    pub const BG_COLOR: u16 = 1 << 2;
    /// `color` transition bit.
    pub const TEXT_COLOR: u16 = 1 << 3;
    /// `border-color` transition bit.
    pub const BORDER_COLOR: u16 = 1 << 4;
    /// `box-shadow` transition bit.
    pub const BOX_SHADOW: u16 = 1 << 5;
    /// `backdrop-filter` transition bit.
    pub const BACKDROP_FILTER: u16 = 1 << 6;
    /// `border-radius` transition bit.
    pub const BORDER_RADIUS: u16 = 1 << 7;
    /// `border-width` / `border-bottom-width` transition bit.
    pub const BORDER_WIDTH: u16 = 1 << 8;
    /// `outline-*` / `border-bottom-color` transition bit.
    pub const OUTLINE: u16 = 1 << 9;
    /// `background-image` / `radial-gradient` / `background-size` transition bit.
    pub const RADIAL_GRADIENT: u16 = 1 << 10;
    /// All compositor transition bits combined (`all`).
    pub const ALL: Self = Self(0x07FF);

    /// Returns `true` if `bit` is enabled in this mask.
    #[inline]
    pub const fn contains(self, bit: u16) -> bool {
        (self.0 & bit) != 0
    }

    /// Combines two masks via bitwise OR.
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Parses a comma-separated CSS `transition-property` list.
    pub fn parse_css_list(raw: &str) -> Option<Self> {
        let mut mask = 0_u16;
        for part in split_top_level_commas(raw) {
            let p = part.trim().to_ascii_lowercase();
            if p == "none" {
                return Some(Self::NONE);
            }
            mask |= property_bit(&p)?;
        }
        (mask != 0).then_some(Self(mask))
    }
}

/// Maximum independent per-property timing clauses stored inline in a [`TransitionSpec`].
pub const MAX_TRANSITION_CLAUSES: usize = 4;

/// Single property-group timing track inside a [`TransitionSpec`].
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct TransitionClause {
    /// Tier-1 paint properties participating in this clause.
    pub mask: TransitionMask,
    /// Tier-2 layout & typography properties participating in this clause.
    pub layout_mask: LayoutTransitionMask,
    /// Clause duration in milliseconds (`0..=65535`).
    pub duration_ms: u16,
    /// Optional clause start delay in milliseconds (negative starts mid-transition immediately).
    pub delay_ms: i32,
    /// Easing or stepped timing curve.
    pub curve: TransitionCurve,
}

impl TransitionClause {
    /// Empty disabled transition clause.
    pub const EMPTY: Self = Self {
        mask: TransitionMask::NONE,
        layout_mask: LayoutTransitionMask::NONE,
        duration_ms: 0,
        delay_ms: 0,
        curve: TransitionCurve::Ease,
    };

    /// Returns `true` when this clause has non-zero transition activity.
    #[inline]
    pub const fn is_active(self) -> bool {
        (self.mask.0 != 0 || self.layout_mask.0 != 0) && self.duration_ms > 0
    }

    /// Returns effective clause duration in milliseconds.
    #[inline]
    pub const fn effective_duration_ms(self) -> f32 {
        self.duration_ms as f32
    }

    /// Returns effective clause completion time (`(delay + duration).max(0.0)`).
    #[inline]
    pub fn effective_total_ms(self) -> f32 {
        (self.delay_ms as f32 + self.duration_ms as f32).max(0.0)
    }
}

/// Compact `Copy` two-tier CSS `transition` configuration stored on `PaintStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct TransitionSpec {
    /// Combined mask of Tier-1 paint properties participating in interpolation.
    pub mask: TransitionMask,
    /// Combined mask of Tier-2 layout & typography properties participating in interpolation.
    pub layout_mask: LayoutTransitionMask,
    /// Maximum transition duration across clauses in milliseconds (`0..=65535`).
    pub duration_ms: u16,
    /// Maximum start delay across clauses in milliseconds (supports negative delays).
    pub delay_ms: i32,
    /// Last/primary easing curve.
    pub curve: TransitionCurve,
    /// First four independent per-property timing tracks stored inline.
    pub clauses: [TransitionClause; MAX_TRANSITION_CLAUSES],
    /// Total number of clauses (clamped to `u8::MAX`; `0` uses summary fields as a single track).
    pub clause_count: u8,
    /// Interned/static overflow storage for clauses beyond `4`.
    pub extra: TransitionExtraClauses,
    /// Whether `transition-property` (or shorthand) has been explicitly specified.
    pub explicit_props: bool,
}

impl TransitionSpec {
    /// Disabled transition (`transition: none`).
    pub const NONE: Self = Self {
        mask: TransitionMask::NONE,
        layout_mask: LayoutTransitionMask::NONE,
        duration_ms: 0,
        delay_ms: 0,
        curve: TransitionCurve::Ease,
        clauses: [TransitionClause::EMPTY; MAX_TRANSITION_CLAUSES],
        clause_count: 0,
        extra: TransitionExtraClauses::Empty,
        explicit_props: false,
    };

    /// Builds a single-track Tier-1 `TransitionSpec` preserving summary fields.
    pub const fn single(
        mask: TransitionMask,
        duration_ms: u16,
        delay_ms: i32,
        curve: TransitionCurve,
    ) -> Self {
        Self::single_with_layout(
            mask,
            LayoutTransitionMask::NONE,
            duration_ms,
            delay_ms,
            curve,
        )
    }

    /// Builds a single-track Tier-1/Tier-2 `TransitionSpec`.
    pub const fn single_with_layout(
        mask: TransitionMask,
        layout_mask: LayoutTransitionMask,
        duration_ms: u16,
        delay_ms: i32,
        curve: TransitionCurve,
    ) -> Self {
        let mut clauses = [TransitionClause::EMPTY; MAX_TRANSITION_CLAUSES];
        clauses[0] = TransitionClause {
            mask,
            layout_mask,
            duration_ms,
            delay_ms,
            curve,
        };
        Self {
            mask,
            layout_mask,
            duration_ms,
            delay_ms,
            curve,
            clauses,
            clause_count: 1,
            extra: TransitionExtraClauses::Empty,
            explicit_props: true,
        }
    }

    /// Combines multiple independent clauses into a single `TransitionSpec` (no 4-clause loss).
    pub fn from_clauses(items: &[TransitionClause]) -> Self {
        Self::from_clause_vec(items)
    }

    /// Appends one timing clause into this spec, interning overflow beyond 4 clauses cleanly.
    pub fn push_clause(&mut self, cl: TransitionClause) {
        self.mask.0 |= cl.mask.0;
        self.layout_mask.0 |= cl.layout_mask.0;
        self.duration_ms = self.duration_ms.max(cl.duration_ms);
        self.delay_ms = if self.clause_count == 0 {
            cl.delay_ms
        } else {
            self.delay_ms.max(cl.delay_ms)
        };
        self.curve = cl.curve;
        let idx = self.clause_count as usize;
        if idx < MAX_TRANSITION_CLAUSES {
            self.clauses[idx] = cl;
            self.clause_count += 1;
        } else {
            let mut extra_vec = self.extra.with_extra(|s| s.to_vec());
            extra_vec.push(cl);
            self.extra = TransitionExtraClauses::from_vec(extra_vec);
            self.clause_count = (MAX_TRANSITION_CLAUSES + self.extra.with_extra(|s| s.len()))
                .min(u8::MAX as usize) as u8;
        }
    }

    /// Returns `true` when at least one Tier-1 or Tier-2 clause has non-zero duration.
    #[inline]
    pub fn is_active(self) -> bool {
        if self.clause_count > 0 {
            let inline_n = (self.clause_count as usize).min(MAX_TRANSITION_CLAUSES);
            if self.clauses[..inline_n]
                .iter()
                .copied()
                .any(TransitionClause::is_active)
            {
                return true;
            }
            return self
                .extra
                .with_extra(|ex| ex.iter().copied().any(TransitionClause::is_active));
        }
        (self.mask.0 != 0 || self.layout_mask.0 != 0) && self.duration_ms > 0
    }

    /// Returns `true` when any active clause targets a Tier-2 layout property.
    #[inline]
    pub fn has_layout_transition(self) -> bool {
        self.layout_mask.is_any() && self.duration_ms > 0
    }

    /// Returns the effective total end time (`(delay + duration).max(0.0)`) across active clauses.
    #[inline]
    pub fn effective_total_ms(self) -> f32 {
        if self.clause_count == 0 {
            return (self.delay_ms as f32 + self.effective_duration_ms()).max(0.0);
        }
        let inline_n = (self.clause_count as usize).min(MAX_TRANSITION_CLAUSES);
        let inline_max = self.clauses[..inline_n]
            .iter()
            .map(|c| c.effective_total_ms())
            .fold(0.0_f32, f32::max);
        let extra_max = self.extra.with_extra(|ex| {
            ex.iter()
                .map(|c| c.effective_total_ms())
                .fold(0.0_f32, f32::max)
        });
        inline_max.max(extra_max)
    }

    /// Returns the effective transition duration in milliseconds.
    #[inline]
    pub const fn effective_duration_ms(self) -> f32 {
        self.duration_ms as f32
    }

    /// Parses CSS `transition` shorthand (supports `none`, comma-separated clauses, Tier-1 & Tier-2
    /// properties, negative delays, `ease*`, `cubic-bezier(...)`, `steps(...)`, `linear(...)`).
    pub fn parse_css(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.eq_ignore_ascii_case("none") {
            return Some(Self::NONE);
        }
        let mut spec = Self::NONE;
        spec.explicit_props = true;
        for clause in split_top_level_commas(trimmed) {
            let single = parse_single_transition_clause(clause.trim())?;
            spec.push_clause(single);
        }
        Some(spec)
    }
}

/// Parses a CSS non-negative time duration token (`200ms`, `0.2s`, `0`) into milliseconds.
pub fn parse_transition_time_ms(raw: &str) -> Option<u16> {
    let ms = parse_transition_delay_ms(raw)?;
    (ms >= 0).then_some(ms.clamp(0, u16::MAX as i32) as u16)
}

/// Parses a CSS signed time token (`-150ms`, `0.2s`, `0`) into signed milliseconds.
pub fn parse_transition_delay_ms(raw: &str) -> Option<i32> {
    let lower = raw.trim().to_ascii_lowercase();
    if lower == "0" || lower == "0s" || lower == "0ms" {
        return Some(0);
    }
    let ms = if let Some(v) = lower.strip_suffix("ms") {
        v.trim().parse::<f32>().ok()?
    } else {
        let v = lower.strip_suffix('s')?;
        v.trim().parse::<f32>().ok()? * 1000.0
    };
    if !ms.is_finite() {
        return None;
    }
    Some(ms.round().clamp(-3_600_000.0, 3_600_000.0) as i32)
}

/// Maps a lowercase CSS property name onto a Tier-1 compositor [`TransitionMask`] bit.
pub fn paint_property_bit(prop: &str) -> Option<u16> {
    Some(match prop {
        "none" => 0,
        "all" => TransitionMask::ALL.0,
        "transform" | "translate" | "rotate" | "scale" | "transform-origin" => {
            TransitionMask::TRANSFORM
        }
        "opacity" => TransitionMask::OPACITY,
        "background" => TransitionMask::BG_COLOR | TransitionMask::RADIAL_GRADIENT,
        "background-color" => TransitionMask::BG_COLOR,
        "background-image" | "background-size" | "background-position" => {
            TransitionMask::RADIAL_GRADIENT
        }
        "backdrop-filter" | "-webkit-backdrop-filter" => TransitionMask::BACKDROP_FILTER,
        "color" => TransitionMask::TEXT_COLOR,
        "border-color" | "border-top-color" | "border-right-color" | "border-left-color" => {
            TransitionMask::BORDER_COLOR
        }
        "border" | "border-top" | "border-right" | "border-left" => {
            TransitionMask::BORDER_COLOR | TransitionMask::BORDER_WIDTH | TransitionMask::OUTLINE
        }
        "box-shadow" => TransitionMask::BOX_SHADOW,
        "border-radius"
        | "border-top-left-radius"
        | "border-top-right-radius"
        | "border-bottom-right-radius"
        | "border-bottom-left-radius" => TransitionMask::BORDER_RADIUS,
        "border-width"
        | "border-top-width"
        | "border-right-width"
        | "border-bottom-width"
        | "border-left-width" => TransitionMask::BORDER_WIDTH,
        "outline"
        | "outline-width"
        | "outline-color"
        | "outline-offset"
        | "border-bottom-color"
        | "border-bottom" => TransitionMask::OUTLINE,
        _ => return None,
    })
}

fn property_bit(prop: &str) -> Option<u16> {
    paint_property_bit(prop)
}

fn parse_single_transition_clause(clause: &str) -> Option<TransitionClause> {
    let tokens = split_ws_preserving_parens(clause);
    if tokens.is_empty() {
        return None;
    }
    let (mut p_mask, mut l_mask) = (None::<u16>, None::<u16>);
    let (mut dur, mut delay, mut saw_dur, mut curve) = (None, 0_i32, false, TransitionCurve::Ease);
    for tok in tokens {
        if !saw_dur {
            if let Some(ms) = parse_transition_time_ms(tok) {
                (dur, saw_dur) = (Some(ms), true);
                continue;
            }
        }
        if let Some(d_ms) = parse_transition_delay_ms(tok) {
            if saw_dur {
                delay = d_ms;
                continue;
            }
            return None;
        }
        if let Some(c) = TransitionCurve::parse_css(tok) {
            curve = c;
        } else if p_mask.is_none() && l_mask.is_none() {
            let low = tok.to_ascii_lowercase();
            let pb = property_bit(&low).unwrap_or(0);
            let lb = layout_property_bit(&low).unwrap_or(0);
            if pb == 0 && lb == 0 && low != "none" {
                return None;
            }
            (p_mask, l_mask) = (Some(pb), Some(lb));
        } else {
            return None;
        }
    }
    let resolved_p = p_mask.unwrap_or(TransitionMask::ALL.0);
    let resolved_l = l_mask.unwrap_or(LayoutTransitionMask::ALL.0);
    let duration_ms = dur.unwrap_or(0);
    Some(TransitionClause {
        mask: TransitionMask(resolved_p),
        layout_mask: LayoutTransitionMask(resolved_l),
        duration_ms,
        delay_ms: delay,
        curve,
    })
}

pub(crate) fn split_top_level_commas(s: &str) -> Vec<&str> {
    let (mut out, mut start, mut depth) = (Vec::new(), 0_usize, 0_usize);
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn split_ws_preserving_parens(s: &str) -> Vec<&str> {
    let (mut out, mut start, mut depth) = (Vec::new(), None, 0_usize);
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => {
                start.get_or_insert(i);
                depth += 1;
            }
            ')' => depth = depth.saturating_sub(1),
            _ if ch.is_whitespace() && depth == 0 => {
                if let Some(st) = start.take() {
                    out.push(&s[st..i]);
                }
            }
            _ => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{StepPosition, TransitionCurve};

    #[test]
    fn default_is_w3c_ease_and_steps_linear_work() {
        assert_eq!(TransitionCurve::default(), TransitionCurve::Ease);
        assert!(TransitionCurve::parse_css("emphasized").is_none());
        assert!(TransitionCurve::parse_css("spring(380, 0.78)").is_none());

        let steps = TransitionCurve::parse_css("steps(4, start)").expect("steps(4, start)");
        assert_eq!(
            steps,
            TransitionCurve::Steps {
                count: 4,
                pos: StepPosition::JumpStart
            }
        );
        assert!((steps.sample(0.0) - 0.25).abs() < 1e-5);
        assert!((steps.sample(0.75) - 1.0).abs() < 1e-5);

        let jn = TransitionCurve::parse_css("steps(3, jump-none)").expect("jump-none");
        assert!((jn.sample(0.0) - 0.0).abs() < 1e-5);
        assert!((jn.sample(0.35) - 0.5).abs() < 1e-5);

        let lin = TransitionCurve::parse_css("linear(0, 0.5 25% 75%, 1)").expect("linear()");
        assert!((lin.sample(0.125) - 0.25).abs() < 1e-5);
        assert!((lin.sample(0.5) - 0.5).abs() < 1e-5);
        assert!((lin.sample(0.875) - 0.75).abs() < 1e-5);
    }
}
