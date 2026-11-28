//! Static UI descriptors emitted as `.rodata` slices by `krui-macros` and shared between
//! `krui-core` (`ViewTree`) and `krui-web` (`WebDomTree`) (`AGENTS.md` Rule 2).

/// Node flavor inside a [`StaticDocumentDesc`] / static subtree table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticNodeKind {
    /// Standard HTML/SVG element node.
    Element,
    /// Invisible control-flow anchor (`<if>` / `<for>`).
    ControlFlowAnchor,
}

/// Static `<button type="...">` classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticButtonKind {
    /// Default `<button>`.
    Auto,
    /// `<button type="button">`.
    Button,
    /// `<button type="submit">`.
    Submit,
    /// `<button type="reset">`.
    Reset,
}

/// Static `<input type="...">` classification.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StaticInputKind {
    /// Not an `<input>` element.
    None,
    /// `<input type="checkbox">`.
    Checkbox,
    /// `<input type="radio">`.
    Radio,
    /// `<input type="range">` with `(min, max, step)`.
    Range(f32, f32, f32),
    /// `<input type="text">`.
    Text,
    /// `<input type="password">`.
    Password,
    /// `<input type="number">`.
    Number,
    /// `<input type="email">`.
    Email,
    /// `<input type="search">`.
    Search,
}

/// Static HTML attribute `(name, value)` pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaticAttrDesc {
    /// Attribute name.
    pub name: &'static str,
    /// Attribute value.
    pub value: &'static str,
}

/// Static `<option>` entry inside `<select>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaticSelectOptDesc {
    /// Option `value` attribute.
    pub value: &'static str,
    /// Visible option text label.
    pub label: &'static str,
    /// Whether this option is initially selected.
    pub selected: bool,
}

/// Pre-order static descriptor for a compiled element or control-flow anchor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StaticNodeDesc {
    /// Node flavor (element vs control-flow anchor).
    pub kind: StaticNodeKind,
    /// Index of the parent node in the same pre-order slice, or `None` for roots.
    pub parent_slot: Option<usize>,
    /// Normalized HTML tag name (`"div"`, `"button"`, `"input"`, etc.).
    pub tag: &'static str,
    /// Original custom tag name if non-standard.
    pub custom_tag: Option<&'static str>,
    /// Static `id` attribute, if present.
    pub id: Option<&'static str>,
    /// Static `aria-label` attribute, if present.
    pub aria_label: Option<&'static str>,
    /// Static `aria-label` when compiled as a text-part slot.
    pub static_aria_label_slot: Option<&'static str>,
    /// `true` when `tag == "button"`.
    pub is_button: bool,
    /// `true` when `tag == "input"`.
    pub is_input: bool,
    /// `true` when `tag == "canvas"`.
    pub is_canvas: bool,
    /// `true` when `<input type="password">`.
    pub is_secret_input: bool,
    /// `true` when this node represents an unwrapped inline text run inside mixed children.
    pub is_synthetic_text_run: bool,
    /// Static `disabled` boolean attribute, if fixed at compile time.
    pub disabled: Option<bool>,
    /// Static `required` boolean attribute, if fixed at compile time.
    pub required: Option<bool>,
    /// Static `checked` boolean attribute, if fixed at compile time.
    pub checked: Option<bool>,
    /// Static CSS class tokens.
    pub classes: &'static [&'static str],
    /// Static custom attributes.
    pub attrs: &'static [StaticAttrDesc],
    /// Static inline CSS `style="..."` text.
    pub inline_style: Option<&'static str>,
    /// Static `<label for="...">` target id.
    pub label_for: Option<&'static str>,
    /// Static `name="..."` form control name.
    pub control_name: Option<&'static str>,
    /// Static `<button type="...">` classification.
    pub button_kind: StaticButtonKind,
    /// Static `<input type="...">` classification.
    pub input_kind: StaticInputKind,
    /// Static `<option>` list for `<select>`.
    pub select_options: &'static [StaticSelectOptDesc],
    /// Static inner text or initial input `value`.
    pub static_text: Option<&'static str>,
    /// Static `placeholder` text.
    pub placeholder: Option<&'static str>,
    /// Static `title` tooltip text.
    pub title: Option<&'static str>,
    /// Static `<a href="...">` URL.
    pub link_href: Option<&'static str>,
    /// Source `.krui` line number (1-based, preview metadata).
    pub source_line: u32,
    /// Source `.krui` column number (1-based, preview metadata).
    pub source_column: u32,
}

impl StaticNodeDesc {
    /// Blank default element descriptor for concise `..StaticNodeDesc::BLANK` codegen literals.
    pub const BLANK: Self = Self {
        kind: StaticNodeKind::Element,
        parent_slot: None,
        tag: "div",
        custom_tag: None,
        id: None,
        aria_label: None,
        static_aria_label_slot: None,
        is_button: false,
        is_input: false,
        is_canvas: false,
        is_secret_input: false,
        is_synthetic_text_run: false,
        disabled: None,
        required: None,
        checked: None,
        classes: &[],
        attrs: &[],
        inline_style: None,
        label_for: None,
        control_name: None,
        button_kind: StaticButtonKind::Auto,
        input_kind: StaticInputKind::None,
        select_options: &[],
        static_text: None,
        placeholder: None,
        title: None,
        link_href: None,
        source_line: 0,
        source_column: 0,
    };

    /// Blank control-flow anchor descriptor for concise `StaticNodeDesc::ANCHOR` codegen.
    pub const ANCHOR: Self = Self {
        kind: StaticNodeKind::ControlFlowAnchor,
        tag: "",
        ..Self::BLANK
    };
}

/// Top-level document static descriptor slice plus document stylesheet metadata.
#[derive(Clone, Copy, Debug)]
pub struct StaticDocumentDesc {
    /// Whether `krui://preset/unstyled` was selected.
    pub unstyled: bool,
    /// Ordered CSS stylesheets collected at compile time.
    pub stylesheets: &'static [&'static str],
    /// Pre-order slice of top-level template nodes.
    pub nodes: &'static [StaticNodeDesc],
}
