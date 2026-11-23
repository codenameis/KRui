//! # `krui-macros`
//!
//! Compile-time procedural macros for KRui:
//! - `krui!("...")`: Compiles HTML + `<style>` CSS at build time into direct Rust struct literals
//!   and fine-grained signal slot bindings. Zero runtime CSS parser or selector matching ships in release binaries.
//! - `include_krui!("relative/path.krui")`: Compiles an external `.krui` file at build time.
//! - `#[view_model]`: Optional MVVM helper macro that generates a companion `*Signals` struct wrapping
//!   each model field in a `krui_signals::Signal<T>`.

#![forbid(unsafe_code)]

mod codegen;
mod codegen_control_flow;
mod codegen_events;
mod codegen_rich_text;
mod codegen_slots;
mod expr_vars;
mod media_codegen;
mod select_bindings;
mod view_model;

use codegen::expand_compiled_doc;
use krui_compiler::{compile_krui_source, compile_krui_source_from_path};
use proc_macro::TokenStream;
use syn::{parse_macro_input, LitStr};

/// Compiles inline HTML + CSS markup at build time into a `krui::ViewFragment` closure
/// that mounts pre-resolved `CompiledStyle` nodes and direct reactive slot bindings onto a `ViewTree`.
#[proc_macro]
pub fn krui(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let source = lit.value();
    let doc = match compile_krui_source(&source) {
        Ok(d) => d,
        Err(e) => {
            return syn::Error::new(lit.span(), e.to_string())
                .to_compile_error()
                .into()
        }
    };
    match expand_compiled_doc(&doc, lit.span()) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// Compiles an external `.krui` file (relative to `CARGO_MANIFEST_DIR`) at build time,
/// automatically loading any `<link rel="stylesheet" href="...css" />` or companion `<stem>.css` file.
#[proc_macro]
pub fn include_krui(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let rel_path = lit.value();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let full_path = std::path::Path::new(&manifest_dir).join(&rel_path);

    let source = match std::fs::read_to_string(&full_path) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!(
                "Failed to read `.krui` file at `{}`: {e}",
                full_path.display()
            );
            return syn::Error::new(lit.span(), msg).to_compile_error().into();
        }
    };

    let doc = match compile_krui_source_from_path(&full_path, &source) {
        Ok(d) => d,
        Err(e) => {
            return syn::Error::new(lit.span(), e.to_string())
                .to_compile_error()
                .into()
        }
    };

    match expand_compiled_doc(&doc, lit.span()) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// MVVM Ergonomics Macro `#[view_model]`.
#[proc_macro_attribute]
pub fn view_model(attr: TokenStream, item: TokenStream) -> TokenStream {
    view_model::expand_view_model(attr, item)
}

/// MVC / Store Controller Registry Macro `#[controller]`.
///
/// Can be applied to a `struct` (exposing its fields to `krui::Registry` when `preview` is enabled)
/// and/or to an `impl` block (exposing `&self` methods to `Registry::call` when `preview` is enabled).
#[proc_macro_attribute]
pub fn controller(attr: TokenStream, item: TokenStream) -> TokenStream {
    view_model::expand_controller(attr, item)
}

/// Store Message / Intent Registry Macro `#[store_msg]`.
///
/// Generates `StoreMsgRegistry` under `#[cfg(feature = "preview")]` so `store.on(Msg::Variant)`
/// and `store.on_input(Msg::Variant)` can be invoked dynamically by the live preview interpreter.
#[proc_macro_attribute]
pub fn store_msg(attr: TokenStream, item: TokenStream) -> TokenStream {
    view_model::expand_store_msg(attr, item)
}
