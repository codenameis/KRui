//! # `krui-style`
//!
//! Pure CSS value representation types shared across `krui-stylo-bridge`, `krui-layout`,
//! `krui-text`, `krui-core`, and `krui-renderer` (`LayoutStyle`, `TextStyle`, `PaintStyle`,
//! `KeyframesDef`, `FontFaceRule`, `HtmlTag`), plus BiDi text sanitization.

#![forbid(unsafe_code)]

pub mod affine;
pub mod box_edges;
pub mod class_patch;
pub mod clip_path;
pub mod color;
pub mod cursor;
pub mod easing;
pub mod filter;
pub mod font;
pub mod font_face;
pub mod gradient_radial;
pub mod gradient_stops;
pub mod grid;
pub mod homography3d;
pub mod html_tag;
pub mod keyframes;
pub mod layout_rect;
pub mod layout_types;
pub mod paint_types;
pub mod profile;
pub mod rich_span;
pub mod shadow_stack;
pub mod standalone_transform;
pub mod transform3d;
pub mod transition;
pub mod transition_clauses;
pub mod transition_layout;
pub mod transition_lerp;
pub mod typography;

pub use affine::{Affine2, Transform2D};
pub use box_edges::{
    BoxSide, ScrollbarWidth, UA_AUTO_ACCENT_COLOR, UA_IME_TARGET_BG_COLOR, UA_POPUP_BG_COLOR,
    UA_POPUP_BORDER_COLOR, UA_PRIMARY_TEXT_COLOR, UA_SCROLLBAR_THUMB_COLOR,
    UA_SECONDARY_TEXT_COLOR, UA_SELECTION_BG_COLOR, UA_SELECTION_BORDER_COLOR,
    UA_SLIDER_TRACK_COLOR, UA_TOOLTIP_BG_COLOR, UA_TOOLTIP_FG_COLOR,
};
pub use class_patch::{LayoutPatchOp, PaintPatchOp, TextPatchOp};
pub use clip_path::{
    ClipCoord, ClipFillRule, ClipPathCmd, ClipPathCmdsRef, ClipPolygonRef, ClipRadius, ClipShape,
};
pub use color::Color;
pub use cursor::CursorStyle;
pub use easing::{LinearCurveRef, LinearStop, StepPosition, TransitionCurve};
pub use filter::CssFilterEffect;
pub use font::FontFamilyStack;
pub use font_face::{
    FontContainerFormat, FontFaceMetricsOverride, FontFaceRule, FontFaceSource, FontStyleSlope,
    UnicodeRange,
};
pub use gradient_radial::{resolve_radial_geometry, PointerCoordPair, RadialCoord, RadialGradient};
pub use gradient_stops::{
    rasterize_lut_rgba8, sample_premul_rgba, ConicGradient, GradientStop, GradientStopsRef,
};
pub use grid::{
    AutoRepeatMode, GridAutoFlow, GridLinePair, GridMaxTrack, GridMinTrack, GridPlacementAtom,
    GridTrack, GridTrackList, MAX_GRID_TRACKS,
};
pub use homography3d::{
    compute_projective_quad, inverse_project_point, mat4_has_perspective, mat4_perspective_origin,
    ProjectiveQuad,
};
pub use html_tag::HtmlTag;
pub use keyframes::{
    sample_keyframes, sample_keyframes_with_timing, AnimationDirection, AnimationFillMode,
    AnimationListRef, AnimationSpec, KeyframeStop, KeyframesDef, StaticAnimationSpec,
};
pub use layout_rect::LayoutRect;
pub use layout_types::{
    AlignItems, BoxSizing, CornerRadii, Dimension, DisplayMode, Edges, FlexDirection, InsetEdges,
    JustifyContent, LayoutStyle, OverflowMode, PositionMode,
};
pub use paint_types::{BoxShadow, LinearGradient, ObjectFit, PaintStyle, UserSelect};
pub use profile::{
    ColorDepthBits, DamageRect, DamageTrackingMode, DisplayShape, DynamicSlotId, RootProfileAttrs,
    TierProfile,
};
pub use rich_span::{PseudoElementSpec, StyledTextSpan};
pub use shadow_stack::{ShadowStack, MAX_BOX_SHADOW_LAYERS};
pub use standalone_transform::{OriginAxis, StandaloneTransform, TranslateAxis};
pub use transform3d::{lerp_transform_list, Mat4, TransformLength, TransformListRef, TransformOp};
pub use transition::{
    apply_masked_lerp, interpolate_paint, lerp_color, lerp_f32, lerp_opt_shadow, lerp_transform,
    paint_differs_on_mask, paint_property_bit, parse_transition_delay_ms, parse_transition_time_ms,
    sample_paint_clauses, TransitionClause, TransitionMask, TransitionSpec,
};
pub use transition_clauses::{parse_transition_property_list, TransitionExtraClauses};
pub use transition_layout::{
    apply_layout_masked_lerp, layout_differs_on_mask, layout_property_bit, LayoutTransitionMask,
};
pub use typography::{
    parse_css_font_weight, parse_text_box_shorthand, FontVariation, FontVariationList, LineHeight,
    OverflowWrapMode, TextBoxEdge, TextBoxTrim, TextOverEdge, TextUnderEdge, WhiteSpaceMode,
    WordBreakMode, MAX_FONT_VARIATIONS,
};

/// Default body font size in logical pixels.
pub const DEFAULT_FONT_SIZE_PX: f32 = 16.0;

/// Normal CSS font weight (`400`).
pub const DEFAULT_FONT_WEIGHT: u16 = 400;

/// Default unitless line-height multiplier (`1.4`).
pub const DEFAULT_LINE_HEIGHT: f32 = 1.4;

/// Active light or dark media preference (`@media (prefers-color-scheme)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum ColorScheme {
    /// Light mode (`prefers-color-scheme: light`).
    #[default]
    Light,
    /// Dark mode (`prefers-color-scheme: dark`).
    Dark,
}

/// Host/user color-scheme override policy (`System` follows OS theme, or force `Light` / `Dark`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum ColorSchemePreference {
    /// Follow the operating system theme (or default light when headless).
    #[default]
    System,
    /// Always evaluate `@media (prefers-color-scheme: light)`.
    Light,
    /// Always evaluate `@media (prefers-color-scheme: dark)`.
    Dark,
}

/// Host/user reduced-motion override policy (`@media (prefers-reduced-motion)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum ReducedMotionPreference {
    /// Follow the operating system setting (or `false` when headless).
    #[default]
    System,
    /// Force reduced motion (`prefers-reduced-motion: reduce`).
    Reduce,
    /// Allow full motion (`prefers-reduced-motion: no-preference`).
    NoPreference,
}

/// Horizontal text alignment inside an element box (`text-align: left | center | right`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TextAlign {
    /// Left / start alignment.
    #[default]
    Left,
    /// Horizontal center alignment.
    Center,
    /// Right / end alignment.
    Right,
}

/// Base paragraph direction for BiDi shaping and alignment (`dir="auto|ltr|rtl"` / `direction: ltr | rtl`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TextDirection {
    /// First-strong character BiDi detection.
    #[default]
    Auto,
    /// Left-to-right base direction.
    Ltr,
    /// Right-to-left base direction.
    Rtl,
}

/// Typography and text-shaping properties that affect intrinsic text measurement and Taffy layout.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct TextStyle {
    /// Font size in logical pixels.
    pub font_size: f32,
    /// Numeric CSS Fonts 4 font weight (`1..=1000`).
    pub font_weight: u16,
    /// CSS `line-height` (`normal`, unitless factor, or computed pixels).
    pub line_height: LineHeight,
    /// Resolved font family stack.
    pub font_family: FontFamilyStack,
    /// Horizontal text alignment.
    pub text_align: TextAlign,
    /// `true` when `text-align` was explicitly authored or inherited from an authored rule.
    pub text_align_explicit: bool,
    /// Base BiDi paragraph direction.
    pub direction: TextDirection,
    /// CSS Inline Layout 3 `text-box-trim` (not inherited).
    pub text_box_trim: TextBoxTrim,
    /// CSS Inline Layout 3 `text-box-edge` (inherited).
    pub text_box_edge: TextBoxEdge,
    /// CSS Fonts 4 `font-variation-settings` OpenType `fvar` overrides (inherited).
    pub variations: FontVariationList,
    /// Italic/oblique font posture (`font-style: italic | oblique`, inherited).
    pub italic: bool,
    /// Additional inter-character advance in logical pixels (`letter-spacing`, inherited).
    pub letter_spacing_px: f32,
    /// CSS Text 3 `white-space` collapsing and wrapping mode (inherited).
    pub white_space: WhiteSpaceMode,
    /// CSS Text 3 `word-break` mode (inherited).
    pub word_break: WordBreakMode,
    /// CSS Text 3 `overflow-wrap` mode (inherited).
    pub overflow_wrap: OverflowWrapMode,
    /// `true` when `text-overflow: ellipsis` is active on this box (not inherited).
    pub text_overflow_ellipsis: bool,
    /// Maximum visible line count from `line-clamp` / `-webkit-line-clamp` (not inherited).
    pub line_clamp: Option<u16>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE_PX,
            font_weight: DEFAULT_FONT_WEIGHT,
            line_height: LineHeight::DEFAULT,
            font_family: FontFamilyStack::FluentUI,
            text_align: TextAlign::Left,
            text_align_explicit: false,
            direction: TextDirection::Auto,
            text_box_trim: TextBoxTrim::None,
            text_box_edge: TextBoxEdge::AUTO,
            variations: FontVariationList::EMPTY,
            italic: false,
            letter_spacing_px: 0.0,
            white_space: WhiteSpaceMode::PreWrap,
            word_break: WordBreakMode::Normal,
            overflow_wrap: OverflowWrapMode::Normal,
            text_overflow_ellipsis: false,
            line_clamp: None,
        }
    }
}

/// Optional CSS-inheritable text and foreground color properties propagated from parent to child.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct InheritedTextProps {
    pub color: Option<Color>,
    pub font_size: Option<f32>,
    pub font_weight: Option<u16>,
    pub line_height: Option<LineHeight>,
    pub font_family: Option<FontFamilyStack>,
    pub text_align: Option<TextAlign>,
    pub direction: Option<TextDirection>,
    pub text_box_edge: Option<TextBoxEdge>,
    pub variations: Option<FontVariationList>,
    pub italic: Option<bool>,
    pub letter_spacing_px: Option<f32>,
    pub white_space: Option<WhiteSpaceMode>,
    pub word_break: Option<WordBreakMode>,
    pub overflow_wrap: Option<OverflowWrapMode>,
    pub caret_color: Option<Color>,
    pub caret_uses_current_color: Option<bool>,
    pub selection_background: Option<Color>,
    pub selection_color: Option<Color>,
    pub accent_color: Option<Option<Color>>,
    pub author_accent_color: Option<bool>,
    pub scrollbar_color: Option<Option<(Color, Color)>>,
}

impl InheritedTextProps {
    /// Empty inherited property set (no explicit overrides).
    pub const EMPTY: Self = Self {
        color: None,
        font_size: None,
        font_weight: None,
        line_height: None,
        font_family: None,
        text_align: None,
        direction: None,
        text_box_edge: None,
        variations: None,
        italic: None,
        letter_spacing_px: None,
        white_space: None,
        word_break: None,
        overflow_wrap: None,
        caret_color: None,
        caret_uses_current_color: None,
        selection_background: None,
        selection_color: None,
        accent_color: None,
        author_accent_color: None,
        scrollbar_color: None,
    };

    /// Merges `self` over `parent`, preserving child overrides while filling unset fields from `parent`.
    pub const fn inherit_from(&self, parent: &Self) -> Self {
        macro_rules! inh {
            ($f:ident) => {
                if self.$f.is_some() {
                    self.$f
                } else {
                    parent.$f
                }
            };
        }
        let own_caret = self.caret_uses_current_color.is_some() || self.caret_color.is_some();
        Self {
            color: inh!(color),
            font_size: inh!(font_size),
            font_weight: inh!(font_weight),
            line_height: inh!(line_height),
            font_family: inh!(font_family),
            text_align: inh!(text_align),
            direction: inh!(direction),
            text_box_edge: inh!(text_box_edge),
            variations: inh!(variations),
            italic: inh!(italic),
            letter_spacing_px: inh!(letter_spacing_px),
            white_space: inh!(white_space),
            word_break: inh!(word_break),
            overflow_wrap: inh!(overflow_wrap),
            caret_color: if own_caret {
                self.caret_color
            } else {
                parent.caret_color
            },
            caret_uses_current_color: if own_caret {
                self.caret_uses_current_color
            } else {
                parent.caret_uses_current_color
            },
            selection_background: inh!(selection_background),
            selection_color: inh!(selection_color),
            accent_color: inh!(accent_color),
            author_accent_color: inh!(author_accent_color),
            scrollbar_color: inh!(scrollbar_color),
        }
    }

    /// Applies resolved inherited properties onto `text` and `paint`.
    pub fn apply_to_style(&self, text: &mut TextStyle, paint: &mut PaintStyle) {
        if let Some(c) = self.color {
            paint.text_color = c;
        }
        if let Some(sz) = self.font_size {
            text.font_size = sz;
        }
        if let Some(wt) = self.font_weight {
            text.font_weight = wt;
        }
        if let Some(lh) = self.line_height {
            text.line_height = lh;
        }
        if let Some(ff) = self.font_family {
            text.font_family = ff;
        }
        if let Some(ta) = self.text_align {
            text.text_align = ta;
            text.text_align_explicit = true;
        }
        if let Some(dir) = self.direction {
            text.direction = dir;
        }
        if let Some(edge) = self.text_box_edge {
            text.text_box_edge = edge;
        }
        if let Some(vars) = self.variations {
            text.variations = vars;
        }
        if let Some(it) = self.italic {
            text.italic = it;
        }
        if let Some(ls) = self.letter_spacing_px {
            text.letter_spacing_px = ls;
        }
        if let Some(ws) = self.white_space {
            text.white_space = ws;
        }
        if let Some(wb) = self.word_break {
            text.word_break = wb;
        }
        if let Some(ow) = self.overflow_wrap {
            text.overflow_wrap = ow;
        }
        if let Some(cur) = self.caret_uses_current_color {
            paint.caret_uses_current_color = cur;
            paint.caret_color = self.caret_color;
        } else if let Some(cc) = self.caret_color {
            paint.caret_color = Some(cc);
            paint.caret_uses_current_color = false;
        }
        if let Some(sbg) = self.selection_background {
            paint.selection_background = Some(sbg);
        }
        if let Some(sfg) = self.selection_color {
            paint.selection_color = Some(sfg);
        }
        if let Some(ac) = self.accent_color {
            paint.accent_color = ac;
        }
        if let Some(auth) = self.author_accent_color {
            paint.author_accent_color = auth;
        }
        if let Some(sc) = self.scrollbar_color {
            paint.scrollbar_color = sc;
        }
    }
}

/// Combined element style bundle (`LayoutStyle`, `TextStyle`, `PaintStyle`) with interactive
/// states computed directly by `krui-stylo-bridge`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledStyle {
    /// Box layout properties.
    pub layout: LayoutStyle,
    /// Typography and shaping properties.
    pub text: TextStyle,
    /// Resolved paint and compositing properties.
    pub paint: PaintStyle,
}

/// Returns `true` if `ch` is a Unicode BiDi directional override or isolate control character.
pub const fn is_bidi_control_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{061C}'
    )
}

/// Sanitizes dynamically interpolated or untrusted text before rendering.
pub fn sanitize_untrusted_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if is_bidi_control_char(ch) {
            continue;
        }
        if ch == '\0' || (ch.is_control() && ch != '\n' && ch != '\t') {
            continue;
        }
        out.push(ch);
    }
    out
}
