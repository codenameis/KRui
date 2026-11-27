//! # `krui-compiler`
//!
//! Build-time HTML, template-expression, component, and stylesheet collector for KRui.
//!
//! Produces a slim DOM + CSS stylesheet IR (`CompiledDocument`) consumed identically by
//! compile-time `krui-macros` codegen and the dev-time `krui-preview` interpreter; Stylo handles
//! runtime CSS parsing, cascade, selectors, `@media`, `@keyframes`, `@font-face`, and `var()`.

#![forbid(unsafe_code)]

mod builtins;
#[cfg(feature = "preview")]
pub mod codec;
mod compile_nodes;
mod component_slots;
mod component_subst;
mod components;
mod control_attrs;
mod css_collect;
mod css_collect_scan;
mod css_media_rewrite;
mod css_profile_lint;
mod element_attrs;
pub mod expr;
mod html;
mod html_control_flow;
mod html_lexer;
mod inline_style_slots;
mod interpolation;
pub mod ir;
mod media_attrs;
mod rich_inline;
mod source_loader;
pub mod svg_compile;
pub mod svg_path;
mod svg_shapes;

#[cfg(feature = "preview")]
pub use codec::{
    decode_compiled_document, encode_compiled_document, encode_compiled_document_with_version,
    IrCodecError, VersionedIrBundle, IR_FORMAT_VERSION, IR_WIRE_MAGIC,
};
use compile_nodes::compile_expanded_roots;
use components::ComponentExpander;
use css_collect::CollectedStylesheets;
pub use expr::{parse_template_expr, BinaryOp, TemplateExpr, TemplateLit, UnaryOp};
use html::parse_html_document;
use html_lexer::extract_style_and_link_blocks;
pub use interpolation::parse_interpolated_text_strict;
pub use ir::{
    CompiledAttrBinding, CompiledBoolAttr, CompiledButtonType, CompiledDocument,
    CompiledDynamicSelectOption, CompiledFor, CompiledIf, CompiledIfBranch, CompiledInputType,
    CompiledNode, CompiledNodeKind, CompiledRichSpan, CompiledSelectChild, CompiledSelectFor,
    CompiledSelectOption, CompiledTextPart, HtmlTag, LayoutSlot, LayoutSlotBinding, PaintSlot,
    PaintSlotBinding,
};
pub use source_loader::VirtualBufferMap;
use source_loader::{load_linked_and_companion_css, read_text_with_virtual};
pub use svg_compile::{
    CompiledFillRule, CompiledLineCap, CompiledLineJoin, CompiledSvgColor, CompiledSvgData,
    CompiledSvgOp,
};
pub use svg_path::CompiledPathSegment;

/// Classification of a [`CompileWarning`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompileWarningKind {
    /// CSS property, transition, or animation that triggers runtime Taffy relayouts.
    #[default]
    LayoutPerformanceCliff,
    /// Unsupported CSS declaration, value, or selector dropped under W3C forward compatibility.
    DroppedCssDeclaration,
    /// Unknown or unsupported CSS `@rule` ignored under W3C forward compatibility.
    UnsupportedAtRule,
    /// Unknown lowercase HTML tag or custom web-component element mapped to a generic container.
    UnknownHtmlElement,
    /// CSS feature falls outside GPU/CPU shader representation limits (`docs/ENGINE_LIMITS.toml`).
    EngineRepresentationLimit,
    /// Effect triggers an expensive offscreen blur or unsupported backdrop blur on `kiosk-spi`.
    CpuTierHeavyEffect,
}

/// Optional compiler behavior toggles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompileOptions {
    /// Kept for API compatibility; CSS diagnostics are reported by Stylo at runtime.
    pub strict_css: bool,
    /// Marks inline/root stylesheet context as an external stylesheet (suppresses drop warnings).
    pub external_stylesheet: bool,
    /// Explicitly selects minimal browser-UA baseline defaults.
    pub unstyled: bool,
    /// Target tier profile enabling profile-specific warnings (`CpuTierHeavyEffect`).
    pub target_profile: Option<krui_style::TierProfile>,
    /// Preserves raw `@media (prefers-reduced-motion)` rules without attribute rewriting (`web` DOM).
    pub preserve_raw_media_queries: bool,
}

/// Compile-time diagnostic emitted by the KRui markup/slot compiler.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompileWarning {
    pub kind: CompileWarningKind,
    pub suppressed: bool,
    pub node_tag: String,
    pub property: String,
    pub signal_expr: String,
    pub message: String,
    pub suggested_fix: String,
}

/// Compile-time error when markup or expressions use invalid syntax or unknown attributes/components.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompileError {
    pub file: Option<std::path::PathBuf>,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl CompileError {
    /// Creates a `CompileError` at the given 1-based `line` and `column`.
    pub fn new(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            file: None,
            line: line.max(1),
            column: column.max(1),
            message: message.into(),
        }
    }

    /// Creates a `CompileError` by computing 1-based `(line, column)` from `byte_offset` in `source`.
    pub fn at_offset(source: &str, byte_offset: usize, message: impl Into<String>) -> Self {
        let (line, column) = line_col_at(source, byte_offset);
        Self::new(line, column, message)
    }

    /// Attaches a source file path to this `CompileError`.
    pub fn with_file(mut self, file: impl Into<std::path::PathBuf>) -> Self {
        self.file = Some(file.into());
        self
    }
}

pub(crate) fn line_col_at(source: &str, byte_offset: usize) -> (usize, usize) {
    let clamped = byte_offset.min(source.len());
    let mut line = 1_usize;
    let mut col = 1_usize;
    for (idx, ch) in source.char_indices() {
        if idx >= clamped {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else if ch != '\r' {
            col += 1;
        }
    }
    (line, col)
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.file {
            Some(path) => write!(
                f,
                "KRui Compile Error ({}:{}:{}): {}",
                path.display(),
                self.line,
                self.column,
                self.message
            ),
            None => write!(
                f,
                "KRui Compile Error (line {}, column {}): {}",
                self.line, self.column, self.message
            ),
        }
    }
}

impl std::error::Error for CompileError {}

/// Compiles a `.krui` source string into a slim [`CompiledDocument`].
pub fn compile_krui_source(source: &str) -> Result<CompiledDocument, CompileError> {
    compile_krui_with_options(source, None, None, CompileOptions::default())
}

/// Compiles a `.krui` source string with `strict_css` option enabled.
pub fn compile_krui_source_strict(source: &str) -> Result<CompiledDocument, CompileError> {
    compile_krui_source_with_options(
        source,
        CompileOptions {
            strict_css: true,
            ..CompileOptions::default()
        },
    )
}

/// Compiles a `.krui` source string using explicit [`CompileOptions`].
pub fn compile_krui_source_with_options(
    source: &str,
    options: CompileOptions,
) -> Result<CompiledDocument, CompileError> {
    compile_krui_with_options(source, None, None, options)
}

/// Compiles a `.krui` file from `krui_path` and its `source`.
pub fn compile_krui_source_from_path(
    krui_path: &std::path::Path,
    source: &str,
) -> Result<CompiledDocument, CompileError> {
    compile_krui_source_from_path_with_options(krui_path, source, CompileOptions::default())
}

/// Compiles a `.krui` file from `krui_path` using explicit [`CompileOptions`].
pub fn compile_krui_source_from_path_with_options(
    krui_path: &std::path::Path,
    source: &str,
    options: CompileOptions,
) -> Result<CompiledDocument, CompileError> {
    compile_krui_with_options(source, Some(krui_path), None, options).map_err(|mut err| {
        if err.file.is_none() {
            err.file = Some(krui_path.to_path_buf());
        }
        err
    })
}

/// Compiles a `.krui` file from `krui_path` using unsaved `virtual_buffers` before disk fallback.
pub fn compile_krui_source_with_virtual_buffers(
    krui_path: &std::path::Path,
    source: Option<&str>,
    virtual_buffers: &VirtualBufferMap,
) -> Result<CompiledDocument, CompileError> {
    let loaded = match source {
        Some(s) => s.to_string(),
        None => read_text_with_virtual(krui_path, Some(virtual_buffers)).map_err(|e| {
            CompileError::new(
                1,
                1,
                format!("Failed to read `{}`: {e}", krui_path.display()),
            )
            .with_file(krui_path)
        })?,
    };
    compile_krui_with_options(
        &loaded,
        Some(krui_path),
        Some(virtual_buffers),
        CompileOptions::default(),
    )
    .map_err(|mut err| {
        if err.file.is_none() {
            err.file = Some(krui_path.to_path_buf());
        }
        err
    })
}

const COMPILE_WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

fn compile_krui_with_options(
    source: &str,
    krui_path: Option<&std::path::Path>,
    virtual_buffers: Option<&VirtualBufferMap>,
    options: CompileOptions,
) -> Result<CompiledDocument, CompileError> {
    std::thread::scope(|scope| {
        let builder = std::thread::Builder::new()
            .name("krui-compile".to_string())
            .stack_size(COMPILE_WORKER_STACK_BYTES);
        match builder.spawn_scoped(scope, || {
            compile_krui_with_options_inner(source, krui_path, virtual_buffers, options)
        }) {
            Ok(handle) => match handle.join() {
                Ok(result) => result,
                Err(payload) => std::panic::resume_unwind(payload),
            },
            Err(_) => compile_krui_with_options_inner(source, krui_path, virtual_buffers, options),
        }
    })
}

fn compile_krui_with_options_inner(
    source: &str,
    krui_path: Option<&std::path::Path>,
    virtual_buffers: Option<&VirtualBufferMap>,
    options: CompileOptions,
) -> Result<CompiledDocument, CompileError> {
    let (inline_styles, linked_hrefs, html_text) = extract_style_and_link_blocks(source)?;
    let mut collector = CollectedStylesheets::default();
    collector.unstyled = options.unstyled;
    collector.target_profile = options.target_profile;
    collector.preserve_raw_media_queries = options.preserve_raw_media_queries;
    load_linked_and_companion_css(
        source,
        &linked_hrefs,
        krui_path,
        virtual_buffers,
        None,
        &mut collector,
    )?;
    let (inline_templates, unexpanded_roots) = parse_html_document(&html_text)?;
    for (css_text, start) in &inline_styles {
        if let Some(tpl) = inline_templates
            .iter()
            .find(|t| *start >= t.tag_offset && *start < t.end_offset)
        {
            collector.push_component_css(
                source,
                *start,
                css_text,
                krui_path,
                Some(&tpl.name),
                virtual_buffers,
            )?;
        } else {
            collector.push_author_css(source, *start, css_text, krui_path, virtual_buffers)?;
        }
    }
    let mut external_component_paths = Vec::new();
    let raw_nodes = {
        let mut expander = ComponentExpander::new(
            source,
            inline_templates,
            krui_path,
            virtual_buffers,
            &mut collector,
            &mut external_component_paths,
        )?;
        expander.expand_root_nodes(source, unexpanded_roots)?
    };
    let mut warnings = std::mem::take(&mut collector.css_warnings);
    let roots = compile_expanded_roots(source, raw_nodes, &mut warnings)?;
    let unstyled = collector.unstyled;
    let (stylesheets, embedded_font_files, external_css_paths) = collector.into_stylesheets();
    let source_path =
        krui_path.map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()));
    Ok(CompiledDocument {
        unstyled,
        stylesheets,
        embedded_font_files,
        roots,
        warnings,
        source_path,
        external_css_paths,
        external_component_paths,
    })
}

/// Splits text containing zero or more `{signal}` interpolation holes into static and dynamic segments.
pub fn parse_interpolated_text(input: &str) -> Vec<CompiledTextPart> {
    parse_interpolated_text_strict(input, 0, input).unwrap_or_default()
}

#[cfg(test)]
mod lib_tests;
