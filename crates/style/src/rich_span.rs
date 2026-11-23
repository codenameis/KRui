//! Multi-span inline paragraph model (`StyledTextSpan`) and generated pseudo-element specification (`PseudoElementSpec`).

use crate::color::Color;
use crate::font::FontFamilyStack;
use crate::typography::FontVariationList;
use crate::CompiledStyle;

/// Shared rich-text inline run stored on `krui-layout::TextContext` and shaped by `krui-text`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct StyledTextSpan {
    /// Text content of this inline run.
    pub text: String,
    /// Numeric font weight (`1..=1000`).
    pub font_weight: u16,
    /// Italic slant flag (`true` for `<em>`, `<i>`, or `font-style: italic`).
    pub italic: bool,
    /// Resolved font family stack for this run.
    pub font_family: FontFamilyStack,
    /// Optional explicit foreground color overriding parent paragraph `text_color`.
    pub color: Option<Color>,
    /// OpenType variable font axis overrides.
    pub variations: FontVariationList,
}

/// Compiled generated pseudo-element (`::before` / `::after`) box specification.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct PseudoElementSpec {
    /// Generated text `content` (`Some("")` or `Some("...")` creates the pseudo box; `None` disables it).
    pub content: Option<String>,
    /// Resolved box, typography, and paint style for the generated pseudo box.
    pub style: CompiledStyle,
}
