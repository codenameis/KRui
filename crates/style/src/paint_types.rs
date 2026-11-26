//! Visual paint, compositing, shadow, gradient, and pseudo-state style types.

use crate::affine::{Affine2, Transform2D};
use crate::clip_path::ClipShape;
use crate::color::Color;
use crate::filter::CssFilterEffect;
use crate::gradient_radial::{RadialCoord, RadialGradient};
use crate::gradient_stops::{ConicGradient, GradientStopsRef};
use crate::keyframes::AnimationListRef;
use crate::layout_types::{CornerRadii, Edges, LayoutStyle};
use crate::shadow_stack::ShadowStack;
use crate::standalone_transform::StandaloneTransform;
use crate::transform3d::{Mat4, TransformListRef};
use crate::transition::TransitionSpec;

/// Default primary text RGB channels `(240, 244, 248)`.
const DEFAULT_TEXT_COLOR: Color = Color::rgb(240, 244, 248);

/// Elevation or inner shadow specification (`box-shadow`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct BoxShadow {
    /// Horizontal shadow offset in logical pixels.
    pub offset_x: f32,
    /// Vertical shadow offset in logical pixels.
    pub offset_y: f32,
    /// Gaussian blur radius in logical pixels.
    pub blur_radius: f32,
    /// Shadow expansion/contraction radius in logical pixels.
    pub spread_radius: f32,
    /// Shadow RGBA color.
    pub color: Color,
    /// `true` for inner (`inset`) shadows, `false` for outer drop shadows.
    pub inset: bool,
}

impl BoxShadow {
    /// Composites multiple CSS `box-shadow` layers into a balanced single shadow primitive.
    pub fn composite_layers(layers: &[Option<Self>]) -> Option<Self> {
        let active: Vec<Self> = layers.iter().copied().flatten().collect();
        if active.is_empty() {
            return None;
        }
        if active.len() == 1 {
            return Some(active[0]);
        }
        let inset = active.iter().all(|s| s.inset);
        let subset: Vec<Self> = active
            .iter()
            .copied()
            .filter(|s| s.inset == inset)
            .collect();
        let (mut sum_a, mut ox, mut oy, mut spr, mut max_blur) =
            (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
        let (mut r_acc, mut g_acc, mut b_acc) = (0.0_f32, 0.0_f32, 0.0_f32);
        for sh in &subset {
            let w = (sh.color.a as f32 / 255.0).max(0.01);
            sum_a += sh.color.a as f32 / 255.0;
            ox += sh.offset_x * w;
            oy += sh.offset_y * w;
            spr += sh.spread_radius * w;
            max_blur = max_blur.max(sh.blur_radius);
            r_acc += sh.color.r as f32 * w;
            g_acc += sh.color.g as f32 * w;
            b_acc += sh.color.b as f32 * w;
        }
        let norm_w: f32 = subset
            .iter()
            .map(|s| (s.color.a as f32 / 255.0).max(0.01))
            .sum();
        let combined_a = ((sum_a * 0.62).clamp(0.0, 1.0) * 255.0).round() as u8;
        Some(Self {
            offset_x: ox / norm_w,
            offset_y: oy / norm_w,
            blur_radius: max_blur,
            spread_radius: spr / norm_w,
            color: Color::rgba(
                (r_acc / norm_w).round().clamp(0.0, 255.0) as u8,
                (g_acc / norm_w).round().clamp(0.0, 255.0) as u8,
                (b_acc / norm_w).round().clamp(0.0, 255.0) as u8,
                combined_a,
            ),
            inset,
        })
    }

    /// Parses a CSS `box-shadow` value (`none` or comma-separated `[inset] <ox> <oy> [<blur>] [<spread>] <color>` layers).
    pub fn parse_css(raw: &str) -> Option<Option<Self>> {
        let (primary, _) = Self::parse_css_pair(raw)?;
        Some(primary)
    }

    /// Parses up to two `box-shadow` layers cleanly (preserving key + ambient elevation shadows),
    /// compositing additional `>= 3` layers into `secondary_shadow`.
    pub fn parse_css_pair(raw: &str) -> Option<(Option<Self>, Option<Self>)> {
        let trimmed = raw.trim();
        if trimmed.eq_ignore_ascii_case("none") {
            return Some((None, None));
        }
        let mut layers = Vec::new();
        for part in split_shadow_commas(trimmed) {
            let p = part.trim();
            if p.is_empty() {
                return None;
            }
            layers.push(parse_single_box_shadow(p)?);
        }
        let active: Vec<Self> = layers.into_iter().flatten().collect();
        match active.len() {
            0 => Some((None, None)),
            1 => Some((Some(active[0]), None)),
            2 => Some((Some(active[0]), Some(active[1]))),
            _ => {
                let tail: Vec<Option<Self>> = active[1..].iter().copied().map(Some).collect();
                Some((Some(active[0]), Self::composite_layers(&tail)))
            }
        }
    }
}

fn split_shadow_commas(s: &str) -> Vec<&str> {
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

fn parse_single_box_shadow(trimmed: &str) -> Option<Option<BoxShadow>> {
    if trimmed.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    let mut inset = false;
    let mut color: Option<Color> = None;
    let mut lengths: Vec<f32> = Vec::with_capacity(4);
    let mut saw_lengths = false;
    for tok in split_shadow_tokens(trimmed) {
        if !inset && tok.eq_ignore_ascii_case("inset") {
            inset = true;
        } else if !saw_lengths && lengths.len() < 4 && parse_shadow_px(tok).is_some() {
            lengths.push(parse_shadow_px(tok)?);
        } else if color.is_none() {
            saw_lengths = !lengths.is_empty();
            color = Some(Color::parse_css(tok)?);
        } else {
            return None;
        }
    }
    if lengths.len() < 2 || lengths.get(2).is_some_and(|&b| b < 0.0) {
        return None;
    }
    Some(Some(BoxShadow {
        offset_x: lengths[0],
        offset_y: lengths[1],
        blur_radius: lengths.get(2).copied().unwrap_or(0.0),
        spread_radius: lengths.get(3).copied().unwrap_or(0.0),
        color: color?,
        inset,
    }))
}

fn parse_shadow_px(tok: &str) -> Option<f32> {
    if tok == "0" || tok == "0.0" || tok == "-0" {
        return Some(0.0);
    }
    let num = tok
        .strip_suffix("px")
        .or_else(|| tok.strip_suffix("PX"))?
        .trim()
        .parse::<f32>()
        .ok()?;
    num.is_finite().then_some(num)
}

fn split_shadow_tokens(s: &str) -> Vec<&str> {
    let (mut out, mut start, mut depth) = (Vec::new(), None, 0_usize);
    for (i, ch) in s.char_indices() {
        if ch == '(' {
            start.get_or_insert(i);
            depth += 1;
        } else if ch == ')' {
            depth = depth.saturating_sub(1);
        } else if ch.is_whitespace() && depth == 0 {
            if let Some(st) = start.take() {
                out.push(&s[st..i]);
            }
        } else {
            start.get_or_insert(i);
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

/// Two- or three-stop linear CSS gradient (`linear-gradient(<angle>deg, <stops...>)`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct LinearGradient {
    /// CSS gradient angle in degrees (`0deg` = bottom-to-top, `90deg` = left-to-right, `180deg` = top-to-bottom).
    pub angle_deg: f32,
    /// Starting stop RGBA color.
    pub start_color: Color,
    /// Ending stop RGBA color.
    pub end_color: Color,
    /// Optional middle stop RGBA color for 3+ stop gradients.
    pub mid_color: Option<Color>,
    /// Normalized stop offsets `[start, mid, end]` in `0.0..=1.0`.
    pub stop_positions: [f32; 3],
    /// Full stop list when > 3 stops or repeating linear gradient is authored.
    pub extra_stops: GradientStopsRef,
    /// `true` for `repeating-linear-gradient(...)`.
    pub repeating: bool,
}

impl Default for LinearGradient {
    fn default() -> Self {
        Self {
            angle_deg: 180.0,
            start_color: Color::TRANSPARENT,
            end_color: Color::TRANSPARENT,
            mid_color: None,
            stop_positions: [0.0, 0.5, 1.0],
            extra_stops: GradientStopsRef::Empty,
            repeating: false,
        }
    }
}

impl LinearGradient {
    /// Builds a standard two-stop linear gradient (`0%` -> `100%`).
    #[inline]
    pub const fn two_stop(angle_deg: f32, start_color: Color, end_color: Color) -> Self {
        Self {
            angle_deg,
            start_color,
            end_color,
            mid_color: None,
            stop_positions: [0.0, 0.5, 1.0],
            extra_stops: GradientStopsRef::Empty,
            repeating: false,
        }
    }
}

/// Image replacement sizing mode (`object-fit: fill | contain | cover | scale-down | none`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum ObjectFit {
    /// Stretches or squashes the source image to fill the box (CSS default).
    #[default]
    Fill,
    /// Scales the source uniformly to fit completely within the box, preserving aspect ratio.
    Contain,
    /// Scales the source uniformly to cover the entire box, cropping overflow at the center.
    Cover,
    /// Displays at natural size or `Contain`, whichever is smaller.
    ScaleDown,
    /// Displays at natural 1:1 logical pixel size centered inside the box, cropping overflow.
    None,
}

/// Controls whether the user can drag-select text on an element (`user-select: auto | none | text | all`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum UserSelect {
    /// Default element-dependent selection behavior.
    #[default]
    Auto,
    /// Disallows drag text selection.
    None,
    /// Enables text range selection.
    Text,
    /// Selects full text content on click.
    All,
}

/// Paint and compositing-only properties.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct PaintStyle {
    /// Solid background fill color.
    pub background_color: Color,
    /// Optional linear background gradient rendered over `background_color`.
    pub background_gradient: Option<LinearGradient>,
    /// Foreground text / icon color.
    pub text_color: Color,
    /// Border stroke color.
    pub border_color: Color,
    /// Computed border stroke width in logical pixels (`0.0` when `border_style_mask == 0`).
    pub border_width: f32,
    /// Authored uniform/max border width in logical pixels before `border_style_mask` gating.
    pub raw_border_width: f32,
    /// Per-side W3C `border-*-style` active mask (`bit 0=top, 1=right, 2=bottom, 3=left`; initial `0` = `none`).
    pub border_style_mask: u8,
    /// Uniform corner radius shorthand in logical pixels.
    pub border_radius: f32,
    /// Per-corner radii `[top_left, top_right, bottom_right, bottom_left]` in logical pixels.
    pub corner_radii: CornerRadii,
    /// Legacy unused bottom indicator width (`0.0`; `border-bottom` uses standard per-side borders).
    pub border_bottom_width: f32,
    /// Legacy unused bottom indicator color.
    pub border_bottom_color: Color,
    /// Optional paint-state per-side bottom border width override (`:focus` / `:disabled`).
    pub border_bottom_override: Option<f32>,
    /// Optional per-side border colors `[top, right, bottom, left]`.
    pub border_side_colors: Option<[Color; 4]>,
    /// Non-layout outer focus/accent ring stroke thickness in logical pixels.
    pub outline_width: f32,
    /// Gap in logical pixels between box outer edge and the outline stroke (negative draws inset).
    pub outline_offset: f32,
    /// Non-layout outline stroke color.
    pub outline_color: Color,
    /// Composited subtree opacity (`0.0..=1.0`).
    pub opacity: f32,
    /// Fast-path 2D affine transform (`translate` / `scale` / `rotate`) around box center.
    pub transform: Transform2D,
    /// Primary drop or inset box shadow.
    pub box_shadow: Option<BoxShadow>,
    /// Secondary layered drop or inset box shadow (dual-shadow M3 elevations).
    pub secondary_shadow: Option<BoxShadow>,
    /// Compositor-level CSS `transition` configuration (0-relayout animations).
    pub transition: TransitionSpec,
    /// Frosted-glass backdrop blur radius in logical pixels.
    pub backdrop_blur_px: f32,
    /// CSS `filter` pipeline effect applied across this element's subtree.
    pub filter: CssFilterEffect,
    /// Optional CSS `clip-path` (`polygon()`, `path()`, `circle()`, `ellipse()`, `inset()`).
    pub clip_path: Option<ClipShape>,
    /// User text-selection policy.
    pub user_select: UserSelect,
    /// Computed CSS `cursor` property (`Auto` preserves semantic cursor fallback).
    pub cursor: crate::cursor::CursorStyle,
    /// Replaced element (`<img>` / `ExternalTexture`) object-fit mode.
    pub object_fit: ObjectFit,
    /// Explicit CSS `caret-color` (`None` = `auto`, using theme caret token unless `caret_uses_current_color` is set).
    pub caret_color: Option<Color>,
    /// Whether `caret-color: currentColor` was specified so caret tracks resolved `text_color`.
    pub caret_uses_current_color: bool,
    /// Explicit CSS `::selection` highlight `background-color` (`None` = theme selection background).
    pub selection_background: Option<Color>,
    /// Explicit CSS `::selection` foreground glyph `color` (`None` = preserve unselected text color).
    pub selection_color: Option<Color>,
    /// Explicit CSS `::placeholder` glyph `color` (`None` = falls back to element `text_color`).
    pub placeholder_color: Option<Color>,
    /// Explicit CSS `::placeholder` glyph `opacity` (`0.0..=1.0`, default `1.0`).
    pub placeholder_opacity: f32,
    /// Explicit or inherited CSS `accent-color` (`None` = `auto`).
    pub accent_color: Option<Color>,
    /// `true` when `accent-color` was authored outside system `.krui-sys` presets.
    pub author_accent_color: bool,
    /// Explicit or inherited CSS `scrollbar-color` `(thumb_color, track_color)` (`None` = `auto`).
    pub scrollbar_color: Option<(Color, Color)>,
    /// Whether this node dynamically inherits `text_color` from an ancestor text container at paint time.
    pub inherits_text_color: bool,
    /// Whether this node provides its resolved `text_color` to inheriting descendants at paint time.
    pub provides_inherited_text_color: bool,
    /// Optional CSS `radial-gradient(...)` fill (supports pointer-bound coordinates).
    pub radial_gradient: Option<RadialGradient>,
    /// Optional CSS `conic-gradient(...)` fill (supports pointer-bound coordinates).
    pub conic_gradient: Option<ConicGradient>,
    /// Optional CSS `background-size` scale factors `(scale_x, scale_y)`.
    pub bg_size_scale: Option<(f32, f32)>,
    /// Optional CSS `background-position` center/tile offset coordinates.
    pub bg_position: Option<(RadialCoord, RadialCoord)>,
    /// Whether `appearance: none` stripped native widget indicator chrome.
    pub appearance_none: bool,
    /// Multi-layer CSS `box-shadow` stack (up to 5 active layers in declaration order).
    pub shadow_stack: ShadowStack,
    /// Standalone CSS Transforms Level 2 properties (`translate`, `rotate`, `scale`, `transform-origin`).
    pub standalone_transform: StandaloneTransform,
    /// Ordered CSS `transform` function list (evaluated around `transform-origin`).
    pub transform_list: TransformListRef,
    /// Cascaded CSS `animation` specification list evaluated at runtime.
    pub animations: AnimationListRef,
}

impl PaintStyle {
    /// Returns effective 4-corner radii (`corner_radii` if non-zero, else uniform `[border_radius; 4]`).
    #[inline]
    pub fn effective_corner_radii(&self) -> CornerRadii {
        if self.corner_radii == CornerRadii::ZERO && self.border_radius > 0.0 {
            CornerRadii::all(self.border_radius)
        } else {
            self.corner_radii
        }
    }

    /// Synchronizes computed `border_width` from `raw_border_width` and `border_style_mask`.
    #[inline]
    pub fn sync_computed_border_width(&mut self) {
        self.border_width = if (self.border_style_mask & 0x0F) != 0 {
            self.raw_border_width
        } else {
            0.0
        };
    }

    /// Sets `raw_border_width` and updates computed `border_width` under `border_style_mask`.
    #[inline]
    pub fn set_raw_border_width(&mut self, px: f32) -> bool {
        let prev_raw = self.raw_border_width;
        let prev_comp = self.border_width;
        self.raw_border_width = px;
        self.sync_computed_border_width();
        (self.raw_border_width - prev_raw).abs() > f32::EPSILON
            || (self.border_width - prev_comp).abs() > f32::EPSILON
    }

    /// Sets `border_style_mask` and updates computed `border_width`.
    #[inline]
    pub fn set_border_style_mask(&mut self, mask: u8) -> bool {
        let prev_mask = self.border_style_mask;
        let prev_comp = self.border_width;
        self.border_style_mask = mask & 0x0F;
        self.sync_computed_border_width();
        self.border_style_mask != prev_mask || (self.border_width - prev_comp).abs() > f32::EPSILON
    }

    /// Computes effective per-side border widths combining `layout.border_edges` / `self.raw_border_width` and `self.border_style_mask`.
    #[inline]
    pub const fn effective_border_edges(&self, layout: &LayoutStyle) -> Edges {
        let base = if self.raw_border_width > 0.0 || self.border_width > 0.0 {
            if (self.border_style_mask & 0x0F) != 0 && self.raw_border_width > 0.0 {
                self.raw_border_width
            } else {
                self.border_width
            }
        } else {
            layout.border_width
        };
        let mut raw = match layout.border_edges {
            Some(edges) => edges,
            None => Edges::all(base),
        };
        if let Some(ov_b) = self.border_bottom_override {
            raw.bottom = ov_b;
        }
        let m = self.border_style_mask;
        Edges {
            top: if (m & 1) != 0 { raw.top } else { 0.0 },
            right: if (m & 2) != 0 { raw.right } else { 0.0 },
            bottom: if (m & 4) != 0 { raw.bottom } else { 0.0 },
            left: if (m & 8) != 0 { raw.left } else { 0.0 },
        }
    }

    /// Resolves the effective caret color (`caret-color: auto` and `currentColor` both resolve to `resolved_text_color`).
    #[inline]
    pub fn resolve_caret_color(&self, resolved_text_color: Color) -> Color {
        if self.caret_uses_current_color {
            resolved_text_color
        } else {
            self.caret_color.unwrap_or(resolved_text_color)
        }
    }
}

impl Default for PaintStyle {
    fn default() -> Self {
        Self {
            background_color: Color::TRANSPARENT,
            background_gradient: None,
            text_color: DEFAULT_TEXT_COLOR,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            raw_border_width: 0.0,
            border_style_mask: 0,
            border_radius: 0.0,
            corner_radii: CornerRadii::ZERO,
            border_bottom_width: 0.0,
            border_bottom_color: Color::TRANSPARENT,
            border_bottom_override: None,
            border_side_colors: None,
            outline_width: 0.0,
            outline_offset: 0.0,
            outline_color: Color::TRANSPARENT,
            opacity: 1.0,
            transform: Transform2D::IDENTITY,
            box_shadow: None,
            secondary_shadow: None,
            transition: TransitionSpec::NONE,
            backdrop_blur_px: 0.0,
            filter: CssFilterEffect::IDENTITY,
            clip_path: None,
            user_select: UserSelect::Auto,
            cursor: crate::cursor::CursorStyle::Auto,
            object_fit: ObjectFit::Fill,
            caret_color: None,
            caret_uses_current_color: false,
            selection_background: None,
            selection_color: None,
            placeholder_color: None,
            placeholder_opacity: 1.0,
            accent_color: None,
            author_accent_color: false,
            scrollbar_color: None,
            inherits_text_color: false,
            provides_inherited_text_color: false,
            radial_gradient: None,
            conic_gradient: None,
            bg_size_scale: None,
            bg_position: None,
            appearance_none: false,
            shadow_stack: ShadowStack::EMPTY,
            standalone_transform: StandaloneTransform::IDENTITY,
            transform_list: TransformListRef::Empty,
            animations: AnimationListRef::Empty,
        }
    }
}

impl PaintStyle {
    /// Computes the local 2D affine matrix combining `transform`, `transform_list`, and `standalone_transform`.
    #[inline]
    pub fn effective_affine_for_rect(&self, x: f32, y: f32, width: f32, height: f32) -> Affine2 {
        self.standalone_transform.compose_with_list(
            self.transform,
            self.transform_list,
            x,
            y,
            width,
            height,
        )
    }

    /// Computes the full 4x4 transformation matrix combining `transform`, `transform_list`, and `standalone_transform`.
    #[inline]
    pub fn effective_mat4_for_rect(&self, x: f32, y: f32, width: f32, height: f32) -> Mat4 {
        self.standalone_transform.compose_mat4_with_list(
            self.transform,
            self.transform_list,
            x,
            y,
            width,
            height,
        )
    }

    /// Returns `true` when any transform property (`transform`, `transform_list`, or standalone) is non-identity.
    #[inline]
    pub fn has_non_identity_transform(&self) -> bool {
        self.transform != Transform2D::IDENTITY
            || !self.transform_list.is_empty()
            || !self.standalone_transform.is_identity()
    }
}
