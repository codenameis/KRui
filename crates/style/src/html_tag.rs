//! Supported semantic HTML5 tags in KRui (`HtmlTag`).

/// Supported semantic HTML tags in KRui's HTML5/MICL subset (with [`Self::Custom`] fallback for
/// unregistered lowercase/custom elements).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum HtmlTag {
    Html,
    Body,
    Div,
    Section,
    Header,
    Footer,
    Main,
    Nav,
    Aside,
    Article,
    Hgroup,
    Address,
    Search,
    Dialog,
    Details,
    Summary,
    Menu,
    Progress,
    Meter,
    Output,
    Hr,
    Br,
    Wbr,
    Form,
    Fieldset,
    Legend,
    Span,
    B,
    Strong,
    Em,
    I,
    Small,
    Mark,
    Sub,
    Sup,
    Kbd,
    Samp,
    Var,
    U,
    S,
    Q,
    Cite,
    Dfn,
    Abbr,
    Time,
    Data,
    Code,
    Pre,
    Blockquote,
    A,
    P,
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    Label,
    Button,
    Input,
    Textarea,
    Select,
    Optgroup,
    Datalist,
    Option,
    Ul,
    Ol,
    Li,
    Dl,
    Dt,
    Dd,
    Table,
    Thead,
    Tbody,
    Tfoot,
    Tr,
    Th,
    Td,
    Caption,
    Colgroup,
    Col,
    Figure,
    Figcaption,
    Picture,
    Video,
    Audio,
    Source,
    Canvas,
    Img,
    Svg,
    /// Fallback generic container for unknown lowercase tags and hyphenated custom elements (`<md-icon>`).
    Custom,
}

const HTML_TAG_NAMES: &[(HtmlTag, &str)] = &[
    (HtmlTag::Html, "html"),
    (HtmlTag::Body, "body"),
    (HtmlTag::Div, "div"),
    (HtmlTag::Section, "section"),
    (HtmlTag::Header, "header"),
    (HtmlTag::Footer, "footer"),
    (HtmlTag::Main, "main"),
    (HtmlTag::Nav, "nav"),
    (HtmlTag::Aside, "aside"),
    (HtmlTag::Article, "article"),
    (HtmlTag::Hgroup, "hgroup"),
    (HtmlTag::Address, "address"),
    (HtmlTag::Search, "search"),
    (HtmlTag::Dialog, "dialog"),
    (HtmlTag::Details, "details"),
    (HtmlTag::Summary, "summary"),
    (HtmlTag::Menu, "menu"),
    (HtmlTag::Progress, "progress"),
    (HtmlTag::Meter, "meter"),
    (HtmlTag::Output, "output"),
    (HtmlTag::Hr, "hr"),
    (HtmlTag::Br, "br"),
    (HtmlTag::Wbr, "wbr"),
    (HtmlTag::Form, "form"),
    (HtmlTag::Fieldset, "fieldset"),
    (HtmlTag::Legend, "legend"),
    (HtmlTag::Span, "span"),
    (HtmlTag::B, "b"),
    (HtmlTag::Strong, "strong"),
    (HtmlTag::Em, "em"),
    (HtmlTag::I, "i"),
    (HtmlTag::Small, "small"),
    (HtmlTag::Mark, "mark"),
    (HtmlTag::Sub, "sub"),
    (HtmlTag::Sup, "sup"),
    (HtmlTag::Kbd, "kbd"),
    (HtmlTag::Samp, "samp"),
    (HtmlTag::Var, "var"),
    (HtmlTag::U, "u"),
    (HtmlTag::S, "s"),
    (HtmlTag::Q, "q"),
    (HtmlTag::Cite, "cite"),
    (HtmlTag::Dfn, "dfn"),
    (HtmlTag::Abbr, "abbr"),
    (HtmlTag::Time, "time"),
    (HtmlTag::Data, "data"),
    (HtmlTag::Code, "code"),
    (HtmlTag::Pre, "pre"),
    (HtmlTag::Blockquote, "blockquote"),
    (HtmlTag::A, "a"),
    (HtmlTag::P, "p"),
    (HtmlTag::H1, "h1"),
    (HtmlTag::H2, "h2"),
    (HtmlTag::H3, "h3"),
    (HtmlTag::H4, "h4"),
    (HtmlTag::H5, "h5"),
    (HtmlTag::H6, "h6"),
    (HtmlTag::Label, "label"),
    (HtmlTag::Button, "button"),
    (HtmlTag::Input, "input"),
    (HtmlTag::Textarea, "textarea"),
    (HtmlTag::Select, "select"),
    (HtmlTag::Optgroup, "optgroup"),
    (HtmlTag::Datalist, "datalist"),
    (HtmlTag::Option, "option"),
    (HtmlTag::Ul, "ul"),
    (HtmlTag::Ol, "ol"),
    (HtmlTag::Li, "li"),
    (HtmlTag::Dl, "dl"),
    (HtmlTag::Dt, "dt"),
    (HtmlTag::Dd, "dd"),
    (HtmlTag::Table, "table"),
    (HtmlTag::Thead, "thead"),
    (HtmlTag::Tbody, "tbody"),
    (HtmlTag::Tfoot, "tfoot"),
    (HtmlTag::Tr, "tr"),
    (HtmlTag::Th, "th"),
    (HtmlTag::Td, "td"),
    (HtmlTag::Caption, "caption"),
    (HtmlTag::Colgroup, "colgroup"),
    (HtmlTag::Col, "col"),
    (HtmlTag::Figure, "figure"),
    (HtmlTag::Figcaption, "figcaption"),
    (HtmlTag::Picture, "picture"),
    (HtmlTag::Video, "video"),
    (HtmlTag::Audio, "audio"),
    (HtmlTag::Source, "source"),
    (HtmlTag::Canvas, "canvas"),
    (HtmlTag::Img, "img"),
    (HtmlTag::Svg, "svg"),
    (HtmlTag::Custom, "custom"),
];

fn is_disallowed_metadata_tag(lower: &str) -> bool {
    matches!(
        lower,
        "head" | "meta" | "link" | "base" | "script" | "noscript"
    )
}

fn is_valid_custom_tag_name(lower: &str) -> bool {
    let mut bytes = lower.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl HtmlTag {
    /// Parses a lowercase/case-insensitive HTML tag name into [`HtmlTag`].
    pub fn parse(tag: &str) -> Result<Self, String> {
        let lower = tag.to_ascii_lowercase();
        for &(variant, name) in HTML_TAG_NAMES {
            if variant != Self::Custom && name == lower {
                return Ok(variant);
            }
        }
        Err(format!(
            "Unsupported HTML element `<{lower}>` in KRui markup"
        ))
    }

    /// Parses an HTML element tag in `.krui` markup, allowing lowercase/custom tags to fall back
    /// to `(Self::Custom, true)` while keeping document metadata tags disallowed.
    pub fn parse_element_tag(tag: &str) -> Result<(Self, bool), String> {
        if let Ok(known) = Self::parse(tag) {
            return Ok((known, false));
        }
        let lower = tag.to_ascii_lowercase();
        if is_disallowed_metadata_tag(&lower) || !is_valid_custom_tag_name(&lower) {
            return Err(format!(
                "Unsupported HTML element `<{lower}>` in KRui markup"
            ));
        }
        Ok((Self::Custom, true))
    }

    /// Canonical lowercase HTML tag name.
    pub const fn as_str(self) -> &'static str {
        let mut idx = 0;
        while idx < HTML_TAG_NAMES.len() {
            if HTML_TAG_NAMES[idx].0 as u8 == self as u8 {
                return HTML_TAG_NAMES[idx].1;
            }
            idx += 1;
        }
        "custom"
    }

    /// Returns `true` for inline phrasing tags (`span`, `b`, `strong`, `em`, `i`, `code`, `a`, ...).
    pub const fn is_inline_element(self) -> bool {
        matches!(
            self,
            Self::Span
                | Self::B
                | Self::Strong
                | Self::Em
                | Self::I
                | Self::Small
                | Self::Mark
                | Self::Sub
                | Self::Sup
                | Self::Kbd
                | Self::Samp
                | Self::Var
                | Self::U
                | Self::S
                | Self::Q
                | Self::Cite
                | Self::Dfn
                | Self::Abbr
                | Self::Time
                | Self::Data
                | Self::Output
                | Self::Code
                | Self::A
                | Self::Wbr
        )
    }

    /// Returns `true` for text-flow containers that lay out inline-only children horizontally.
    pub const fn is_text_flow_container(self) -> bool {
        matches!(
            self,
            Self::P
                | Self::H1
                | Self::H2
                | Self::H3
                | Self::H4
                | Self::H5
                | Self::H6
                | Self::Span
                | Self::Label
                | Self::Summary
                | Self::Figcaption
                | Self::Legend
                | Self::B
                | Self::Strong
                | Self::Em
                | Self::I
                | Self::Small
                | Self::Mark
                | Self::Sub
                | Self::Sup
                | Self::Kbd
                | Self::Samp
                | Self::Var
                | Self::U
                | Self::S
                | Self::Q
                | Self::Cite
                | Self::Dfn
                | Self::Abbr
                | Self::Time
                | Self::Data
                | Self::Output
                | Self::Code
                | Self::Pre
                | Self::Blockquote
                | Self::A
                | Self::Dt
                | Self::Dd
                | Self::Th
                | Self::Td
                | Self::Caption
                | Self::Address
        )
    }

    /// Returns `true` for interactive form/button controls.
    pub const fn is_interactive_control(self) -> bool {
        matches!(
            self,
            Self::Button | Self::Input | Self::Textarea | Self::Select
        )
    }
}
