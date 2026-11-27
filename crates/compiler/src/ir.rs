//! Intermediate representation (IR) types for a compiled `.krui` document (`CompiledDocument`,
//! `CompiledNode`, `PaintSlot`, `LayoutSlot`, etc.).
//!
//! Shared identically between compile-time `krui-macros` code generation and the dev-time
//! `krui-preview` interpreter; Stylo handles all CSS cascade, selectors, and variable resolution.

pub use krui_style::HtmlTag;

use crate::{CompileWarning, TemplateExpr};
use std::path::PathBuf;

/// Direct compositor-only paint slot targeted by a `{signal}` interpolation hole.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum PaintSlot {
    TranslateX,
    TranslateY,
    ScaleX,
    ScaleY,
    ScaleUniform,
    RotateDeg,
    Opacity,
    /// Binds a `Signal<FastPathTransform>` directly to the node's 2D transform.
    FastTransform,
}

/// Direct layout-triggering slot targeted by a `{signal}` interpolation hole.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum LayoutSlot {
    WidthPx,
    HeightPx,
    TopPx,
    LeftPx,
    RightPx,
    BottomPx,
    MarginPx,
    PaddingPx,
    GapPx,
}

/// A compiled direct binding from a template expression to a specific numeric paint slot.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct PaintSlotBinding {
    pub slot: PaintSlot,
    pub signal_ident: String,
    pub expr: TemplateExpr,
}

/// A compiled binding from a template expression to a layout-triggering slot.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct LayoutSlotBinding {
    pub slot: LayoutSlot,
    pub signal_ident: String,
    pub expr: TemplateExpr,
}

/// A segment of text inside a text node or attribute after compile-time `{signal}` splitting.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledTextPart {
    Static(String),
    Dynamic { raw: String, expr: TemplateExpr },
}

/// Compiled boolean attribute (`disabled`, `disabled="true"`, or `disabled="{!vm.can_save}"`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledBoolAttr {
    Static(bool),
    Dynamic { raw: String, expr: TemplateExpr },
}

/// Supported `<input type="...">` variants preserved in the compiled IR.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledInputType {
    #[default]
    Text,
    Password,
    Number,
    Email,
    Search,
    Checkbox,
    Radio,
    Range,
}

/// Supported `<button type="...">` variants preserved in the compiled IR.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledButtonType {
    #[default]
    Auto,
    Button,
    Submit,
    Reset,
}

/// Compiled static `<option value="..." selected>` child inside a `<select>` element.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledSelectOption {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

/// Compiled dynamic `<option>` template inside `<select>` (either standalone or inside `<for>`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledDynamicSelectOption {
    pub value_parts: Vec<CompiledTextPart>,
    pub label_parts: Vec<CompiledTextPart>,
    pub selected: bool,
}

/// Compiled `<for each="{opt in list}" key="{opt.value}"><option ...>...</option></for>` inside `<select>`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledSelectFor {
    pub item_var: String,
    pub list_expr: String,
    pub list: TemplateExpr,
    pub key_expr: String,
    pub key: TemplateExpr,
    pub options: Vec<CompiledDynamicSelectOption>,
}

/// Ordered child entry of a `<select>` containing `<for>` or interpolated `<option>`s.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledSelectChild {
    Option(CompiledDynamicSelectOption),
    For(CompiledSelectFor),
}

/// A single `<if>` / `<else-if>` / `<else>` branch in a compiled conditional construct.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledIfBranch {
    pub condition_expr: Option<String>,
    pub condition: Option<TemplateExpr>,
    pub children: Vec<CompiledNode>,
}

/// Compiled `<if test="{...}"> ... <else-if test="{...}"> ... <else> ... </if>` construct.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledIf {
    pub branches: Vec<CompiledIfBranch>,
}

/// Compiled `<for each="{item in list}" key="{item.id}"> ... </for><empty>...</empty>` construct.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledFor {
    pub item_var: String,
    pub list_expr: String,
    pub list: TemplateExpr,
    pub key_expr: String,
    pub key: TemplateExpr,
    pub body: Vec<CompiledNode>,
    pub empty_body: Vec<CompiledNode>,
}

/// Dynamic runtime-interpolated HTML attribute binding (`open="{...}"`, `data-*="{...}"`, `aria-*="{...}"`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledAttrBinding {
    pub name: String,
    pub parts: Vec<CompiledTextPart>,
}

/// Discriminates between standard HTML elements and virtual `<if>` / `<for>` control-flow constructs.
#[derive(Clone, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledNodeKind {
    #[default]
    Element,
    If(CompiledIf),
    For(Box<CompiledFor>),
}

/// Compiled DOM node representation emitted at build time for Stylo + `ViewTree`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledNode {
    pub kind: CompiledNodeKind,
    pub tag: HtmlTag,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub class_parts: Vec<CompiledTextPart>,
    pub static_inline_style: Option<String>,
    pub dynamic_inline_style_parts: Vec<CompiledTextPart>,
    pub paint_bindings: Vec<PaintSlotBinding>,
    pub layout_bindings: Vec<LayoutSlotBinding>,
    pub text_parts: Vec<CompiledTextPart>,
    pub placeholder_parts: Vec<CompiledTextPart>,
    pub title_parts: Vec<CompiledTextPart>,
    pub img_src: Vec<CompiledTextPart>,
    pub img_alt: Vec<CompiledTextPart>,
    pub link_href: Vec<CompiledTextPart>,
    pub svg_data: Option<crate::CompiledSvgData>,
    pub on_click: Option<TemplateExpr>,
    pub on_focus: Option<TemplateExpr>,
    pub on_blur: Option<TemplateExpr>,
    pub on_pointer_down: Option<TemplateExpr>,
    pub on_pointer_up: Option<TemplateExpr>,
    pub on_pointer_move: Option<TemplateExpr>,
    #[cfg_attr(feature = "preview", serde(default))]
    pub on_context_menu: Option<TemplateExpr>,
    #[cfg_attr(feature = "preview", serde(default))]
    pub on_wheel: Option<TemplateExpr>,
    pub on_scroll: Option<TemplateExpr>,
    pub on_input: Option<TemplateExpr>,
    pub on_change: Option<TemplateExpr>,
    pub on_keydown: Option<TemplateExpr>,
    pub on_submit: Option<TemplateExpr>,
    pub on_reset: Option<TemplateExpr>,
    pub bind_value: Option<TemplateExpr>,
    pub canvas_draw: Option<TemplateExpr>,
    pub input_type: CompiledInputType,
    pub button_type: CompiledButtonType,
    pub is_secret_input: bool,
    pub disabled: Option<CompiledBoolAttr>,
    pub required: Option<CompiledBoolAttr>,
    pub checked: Option<CompiledBoolAttr>,
    pub bind_checked: Option<TemplateExpr>,
    pub label_for: Option<String>,
    pub control_name: Option<String>,
    pub range_bounds: Option<(f32, f32, f32)>,
    pub select_options: Vec<CompiledSelectOption>,
    pub select_children: Vec<CompiledSelectChild>,
    pub aria_label: Option<String>,
    pub aria_label_parts: Vec<CompiledTextPart>,
    pub static_attrs: Vec<(String, String)>,
    pub dynamic_attrs: Vec<CompiledAttrBinding>,
    pub custom_tag: Option<String>,
    /// 1-based source line of this element's opening tag (`0` for virtual control-flow nodes).
    pub source_line: u32,
    /// 1-based source column of this element's opening tag (`0` for virtual control-flow nodes).
    pub source_column: u32,
    /// `true` for compiler-synthesized inline `<span>` text-run wrappers.
    #[cfg_attr(feature = "preview", serde(default))]
    pub is_synthetic_text_run: bool,
    pub children: Vec<CompiledNode>,
    pub rich_spans: Vec<CompiledRichSpan>,
}

/// Compiled inline span belonging to a collapsed rich-text paragraph ([`CompiledNode::rich_spans`]).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledRichSpan {
    pub parts: Vec<CompiledTextPart>,
    pub font_weight: u16,
    pub italic: bool,
    pub font_family: krui_style::FontFamilyStack,
    pub color: Option<krui_style::Color>,
}

impl CompiledNode {
    /// Creates a `CompiledNode` with default DOM/event/style fields for `kind`.
    pub fn from_kind(kind: CompiledNodeKind) -> Self {
        Self {
            kind,
            tag: HtmlTag::Div,
            id: None,
            classes: Vec::new(),
            class_parts: Vec::new(),
            static_inline_style: None,
            dynamic_inline_style_parts: Vec::new(),
            paint_bindings: Vec::new(),
            layout_bindings: Vec::new(),
            text_parts: Vec::new(),
            placeholder_parts: Vec::new(),
            title_parts: Vec::new(),
            img_src: Vec::new(),
            img_alt: Vec::new(),
            link_href: Vec::new(),
            svg_data: None,
            on_click: None,
            on_focus: None,
            on_blur: None,
            on_pointer_down: None,
            on_pointer_up: None,
            on_pointer_move: None,
            on_context_menu: None,
            on_wheel: None,
            on_scroll: None,
            on_input: None,
            on_change: None,
            on_keydown: None,
            on_submit: None,
            on_reset: None,
            bind_value: None,
            canvas_draw: None,
            input_type: CompiledInputType::Text,
            button_type: CompiledButtonType::Auto,
            is_secret_input: false,
            disabled: None,
            required: None,
            checked: None,
            bind_checked: None,
            label_for: None,
            control_name: None,
            range_bounds: None,
            select_options: Vec::new(),
            select_children: Vec::new(),
            aria_label: None,
            aria_label_parts: Vec::new(),
            static_attrs: Vec::new(),
            dynamic_attrs: Vec::new(),
            custom_tag: None,
            source_line: 0,
            source_column: 0,
            is_synthetic_text_run: false,
            children: Vec::new(),
            rich_spans: Vec::new(),
        }
    }
}

const CLONE_WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

fn node_exceeds_depth(node: &CompiledNode, remaining_depth: usize) -> bool {
    if remaining_depth == 0 {
        return true;
    }
    let next = remaining_depth - 1;
    match &node.kind {
        CompiledNodeKind::If(cif) => cif
            .branches
            .iter()
            .any(|b| nodes_exceed_depth(&b.children, next)),
        CompiledNodeKind::For(cfor) => {
            nodes_exceed_depth(&cfor.body, next) || nodes_exceed_depth(&cfor.empty_body, next)
        }
        CompiledNodeKind::Element => nodes_exceed_depth(&node.children, next),
    }
}

pub(crate) fn nodes_exceed_depth(nodes: &[CompiledNode], remaining_depth: usize) -> bool {
    nodes.iter().any(|n| node_exceeds_depth(n, remaining_depth))
}

/// Clones a [`CompiledNode`] slice using a 16 MiB stack in debug builds when tree depth would risk
/// overflowing a 1 MiB main thread stack.
pub fn clone_compiled_nodes_stack_safe(nodes: &[CompiledNode]) -> Vec<CompiledNode> {
    if !(cfg!(debug_assertions) && nodes_exceed_depth(nodes, 2)) {
        return nodes.to_vec();
    }
    std::thread::scope(|scope| {
        let builder = std::thread::Builder::new()
            .name("krui-ir-clone".to_string())
            .stack_size(CLONE_WORKER_STACK_BYTES);
        match builder.spawn_scoped(scope, || nodes.to_vec()) {
            Ok(handle) => match handle.join() {
                Ok(cloned) => cloned,
                Err(payload) => std::panic::resume_unwind(payload),
            },
            Err(_) => nodes.to_vec(),
        }
    })
}

/// Complete compiled document artifact produced from a `.krui` document or `krui!` macro block.
#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledDocument {
    /// Whether `<link rel="krui-preset" href="krui://preset/unstyled">` or `CompileOptions::unstyled` was requested.
    pub unstyled: bool,
    /// Ordered CSS stylesheets to register in Stylo (`@layer component` sheets first, then author sheets).
    pub stylesheets: Vec<String>,
    /// Local `@font-face` files rewritten to `"krui://inline/<key>"` for compile-time `include_bytes!`.
    pub embedded_font_files: Vec<(String, PathBuf)>,
    pub roots: Vec<CompiledNode>,
    pub warnings: Vec<CompileWarning>,
    pub source_path: Option<PathBuf>,
    pub external_css_paths: Vec<PathBuf>,
    pub external_component_paths: Vec<PathBuf>,
}

impl Clone for CompiledDocument {
    fn clone(&self) -> Self {
        Self {
            unstyled: self.unstyled,
            stylesheets: self.stylesheets.clone(),
            embedded_font_files: self.embedded_font_files.clone(),
            roots: clone_compiled_nodes_stack_safe(&self.roots),
            warnings: self.warnings.clone(),
            source_path: self.source_path.clone(),
            external_css_paths: self.external_css_paths.clone(),
            external_component_paths: self.external_component_paths.clone(),
        }
    }
}

impl Drop for CompiledDocument {
    fn drop(&mut self) {
        if !nodes_exceed_depth(&self.roots, 24) {
            return;
        }
        let mut stack = std::mem::take(&mut self.roots);
        while let Some(mut node) = stack.pop() {
            stack.append(&mut node.children);
            match &mut node.kind {
                CompiledNodeKind::If(cif) => {
                    for br in &mut cif.branches {
                        stack.append(&mut br.children);
                    }
                }
                CompiledNodeKind::For(cfor) => {
                    stack.append(&mut cfor.body);
                    stack.append(&mut cfor.empty_body);
                }
                CompiledNodeKind::Element => {}
            }
        }
    }
}
