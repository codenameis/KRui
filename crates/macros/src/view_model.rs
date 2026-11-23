//! `#[view_model]`, `#[controller]`, and `#[store_msg]` procedural macro implementations.
//!
//! When `#[cfg(feature = "preview")]` is active on `krui-macros`, these macros also emit
//! `krui::Registry` / `krui::StoreMsgRegistry` implementations so the live preview interpreter
//! can resolve paths, signals, commands, and controller/store methods dynamically at runtime.
//! In release builds (`default` features), zero registry code is generated.

mod registry_codegen;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use registry_codegen::{emit_impl_dispatch_helper, emit_store_msg_impl, emit_struct_registry_impl};
use syn::{parse_macro_input, Item, ItemEnum, ItemStruct};

pub(crate) fn expand_view_model(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let syn::Fields::Named(named_fields) = &input.fields else {
        return syn::Error::new_spanned(
            &input,
            "#[view_model] requires a struct with named fields",
        )
        .to_compile_error()
        .into();
    };

    let has_mvvm_field_attrs = named_fields.named.iter().any(|f| {
        f.attrs.iter().any(|a| {
            a.path().is_ident("state")
                || a.path().is_ident("computed")
                || a.path().is_ident("command")
        })
    });

    let field_idents: Vec<&syn::Ident> = named_fields
        .named
        .iter()
        .filter_map(|f| f.ident.as_ref())
        .collect();

    if has_mvvm_field_attrs {
        expand_annotated_view_model(&input, named_fields, &field_idents)
    } else {
        expand_plain_view_model(&input, named_fields, &field_idents)
    }
}

fn transform_annotated_vm_field(
    field: &syn::Field,
    transformed_fields: &mut Vec<proc_macro2::TokenStream>,
    helper_methods: &mut Vec<proc_macro2::TokenStream>,
) {
    let Some(fname) = field.ident.as_ref() else {
        return;
    };
    let ftype = &field.ty;
    let fvis = &field.vis;
    let mut is_state = false;
    let mut is_computed = false;
    let mut kept_attrs = Vec::new();

    for attr in &field.attrs {
        if attr.path().is_ident("state") {
            is_state = true;
        } else if attr.path().is_ident("computed") {
            is_computed = true;
        } else if !attr.path().is_ident("command") {
            kept_attrs.push(attr);
        }
    }

    if is_state {
        transformed_fields.push(quote! { #(#kept_attrs)* #fvis #fname: krui::Signal<#ftype> });
        let get_ident = format_ident!("get_{}", fname);
        let set_ident = format_ident!("set_{}", fname);
        helper_methods.push(quote! {
            #[inline]
            pub fn #get_ident(&self) -> #ftype { self.#fname.get() }
            #[inline]
            pub fn #set_ident(&self, value: #ftype) { self.#fname.set(value); }
        });
    } else if is_computed {
        transformed_fields.push(quote! { #(#kept_attrs)* #fvis #fname: krui::Computed<#ftype> });
        let get_ident = format_ident!("get_{}", fname);
        helper_methods.push(quote! {
            #[inline]
            pub fn #get_ident(&self) -> #ftype { self.#fname.get() }
        });
    } else {
        transformed_fields.push(quote! { #(#kept_attrs)* #fvis #fname: #ftype });
    }
}

fn expand_annotated_view_model(
    input: &ItemStruct,
    named_fields: &syn::FieldsNamed,
    field_idents: &[&syn::Ident],
) -> TokenStream {
    let struct_name = &input.ident;
    let vis = &input.vis;
    let attrs = &input.attrs;
    let mut transformed_fields = Vec::new();
    let mut helper_methods = Vec::new();
    for field in &named_fields.named {
        transform_annotated_vm_field(field, &mut transformed_fields, &mut helper_methods);
    }

    let mut has_derive_clone = false;
    for attr in attrs {
        if attr.path().is_ident("derive") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("Clone") {
                    has_derive_clone = true;
                }
                Ok(())
            });
        }
    }
    let derive_tokens = if has_derive_clone {
        quote! {}
    } else {
        quote! { #[derive(Clone)] }
    };
    let preview_registry_tokens = emit_struct_registry_impl(struct_name, field_idents);

    quote! {
        #(#attrs)*
        #derive_tokens
        #vis struct #struct_name {
            #(#transformed_fields),*
        }
        impl #struct_name {
            #(#helper_methods)*
        }
        #preview_registry_tokens
    }
    .into()
}

fn expand_plain_view_model(
    input: &ItemStruct,
    named_fields: &syn::FieldsNamed,
    field_idents: &[&syn::Ident],
) -> TokenStream {
    let struct_name = &input.ident;
    let vis = &input.vis;
    let vm_name = format_ident!("{}ViewModel", struct_name);
    let mut vm_fields = Vec::new();
    let mut init_fields = Vec::new();
    let mut snapshot_fields = Vec::new();

    for field in &named_fields.named {
        let Some(fname) = field.ident.as_ref() else {
            continue;
        };
        let ftype = &field.ty;
        let fvis = &field.vis;
        vm_fields.push(quote! { #fvis #fname: krui::Signal<#ftype> });
        init_fields.push(quote! { #fname: krui::Signal::new(self.#fname) });
        snapshot_fields.push(quote! { #fname: self.#fname.get() });
    }

    let preview_plain_registry = emit_struct_registry_impl(struct_name, field_idents);
    let preview_vm_registry = emit_struct_registry_impl(&vm_name, field_idents);

    quote! {
        #input
        #[derive(Clone, Copy)]
        #vis struct #vm_name {
            #(#vm_fields),*
        }
        impl #struct_name {
            /// Converts this plain state struct into a reactive `#vm_name` with per-field `Signal<T>`s.
            pub fn into_view_model(self) -> #vm_name {
                #vm_name { #(#init_fields),* }
            }
        }
        impl #vm_name {
            /// Reads and clones a snapshot of all reactive fields back into `#struct_name`.
            pub fn snapshot(&self) -> #struct_name {
                #struct_name { #(#snapshot_fields),* }
            }
        }
        #preview_plain_registry
        #preview_vm_registry
    }
    .into()
}

pub(crate) fn expand_controller(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(item as Item);
    match parsed {
        Item::Struct(item_struct) => {
            let struct_name = &item_struct.ident;
            let field_idents: Vec<&syn::Ident> = match &item_struct.fields {
                syn::Fields::Named(named) => named
                    .named
                    .iter()
                    .filter_map(|f| f.ident.as_ref())
                    .collect(),
                _ => Vec::new(),
            };
            let preview_tokens = emit_struct_registry_impl(struct_name, &field_idents);
            quote! {
                #item_struct
                impl #struct_name {
                    #[doc(hidden)]
                    #[inline]
                    pub fn __krui_template_callable(&self) -> &Self {
                        self
                    }
                }
                #preview_tokens
            }
            .into()
        }
        Item::Impl(item_impl) => {
            let preview_tokens = emit_impl_dispatch_helper(&item_impl);
            quote! {
                #item_impl
                #preview_tokens
            }
            .into()
        }
        other => syn::Error::new_spanned(
            other,
            "#[controller] can only be applied to a `struct` or `impl` block",
        )
        .to_compile_error()
        .into(),
    }
}

pub(crate) fn expand_store_msg(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_enum = parse_macro_input!(item as ItemEnum);
    let preview_tokens = emit_store_msg_impl(&item_enum);
    quote! {
        #item_enum
        #preview_tokens
    }
    .into()
}
