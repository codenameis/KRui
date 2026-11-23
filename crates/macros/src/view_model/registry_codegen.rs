//! Code generation helpers for `krui::Registry` and `krui::StoreMsgRegistry` (`preview` feature only).

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{FnArg, ImplItem, ItemEnum, ItemImpl, Pat, ReturnType, Type};

fn emit_registry_get_and_signal(
    field_names: &[String],
    field_idents: &[&syn::Ident],
) -> (TokenStream2, TokenStream2) {
    let get_fn = quote! {
        fn get(&self, path: &str) -> ::std::option::Option<krui::Value> {
            #[allow(unused_imports)]
            use krui::RegistryFieldExt as _;
            let (head, tail) = match path.split_once(".") {
                Some((h, t)) => (h, Some(t)),
                None => (path, None),
            };
            match head {
                #(
                    #field_names => {
                        let val = (&self.#field_idents).registry_field_get()?;
                        if let Some(rest) = tail {
                            if let krui::Value::Record(sub) = val {
                                return sub.get(rest);
                            }
                            return None;
                        }
                        Some(val)
                    }
                )*
                _ => None,
            }
        }
    };
    let signal_fn = quote! {
        fn signal(&self, path: &str) -> ::std::option::Option<krui::DynSignal> {
            #[allow(unused_imports)]
            use krui::RegistryFieldExt as _;
            let (head, tail) = match path.split_once(".") {
                Some((h, t)) => (h, Some(t)),
                None => (path, None),
            };
            match head {
                #(
                    #field_names => {
                        if let Some(rest) = tail {
                            if let Some(krui::Value::Record(sub)) = (&self.#field_idents).registry_field_get() {
                                return sub.signal(rest);
                            }
                            return None;
                        }
                        (&self.#field_idents).registry_field_signal()
                    }
                )*
                _ => None,
            }
        }
    };
    (get_fn, signal_fn)
}

fn emit_core_delegate_tokens(has_core_field: bool) -> (TokenStream2, TokenStream2) {
    if !has_core_field {
        return (quote! {}, quote! {});
    }
    let call_tok = quote! {
        if matches!(name, "on" | "emit" | "send" | "on_input" | "emit_input") {
            if let Some(krui::Value::Record(core_reg)) = (&self.core).registry_field_get() {
                return core_reg.call(name, args);
            }
        }
    };
    let val_tok = quote! {
        if matches!(name, "on" | "emit" | "send" | "on_input" | "emit_input") {
            if let Some(krui::Value::Record(core_reg)) = (&self.core).registry_field_get() {
                return core_reg.validate_call(name, args);
            }
        }
    };
    (call_tok, val_tok)
}

fn emit_registry_call_and_validate(has_core_field: bool) -> (TokenStream2, TokenStream2) {
    let (core_delegate_tokens, core_validate_tokens) = emit_core_delegate_tokens(has_core_field);
    let call_fn = quote! {
        fn call(&self, name: &str, args: &[krui::Value]) -> ::std::result::Result<krui::Value, krui::CallError> {
            #[allow(unused_imports)]
            use krui::{RegistryFieldExt as _, RegistryMethodDispatchFallback as _};
            if let Some((head, tail)) = name.split_once(".") {
                if let Some(krui::Value::Record(sub)) = self.get(head) {
                    return sub.call(tail, args);
                }
            }
            if let Some(res) = self.__krui_dispatch_method(name, args) { return res; }
            #core_delegate_tokens
            if let Some(krui::Value::Command(cmd)) = self.get(name) {
                if !args.is_empty() {
                    return Err(krui::CallError::ArityMismatch { method: name.to_string(), expected: 0, actual: args.len() });
                }
                cmd.execute();
                return Ok(krui::Value::Unit);
            }
            Err(krui::CallError::UnknownMethod(name.to_string()))
        }
    };
    let validate_fn = quote! {
        fn validate_call(&self, name: &str, args: &[krui::Value]) -> ::std::result::Result<krui::Value, krui::CallError> {
            #[allow(unused_imports)]
            use krui::{RegistryFieldExt as _, RegistryMethodDispatchFallback as _};
            if let Some((head, tail)) = name.split_once(".") {
                if let Some(krui::Value::Record(sub)) = self.get(head) {
                    return sub.validate_call(tail, args);
                }
            }
            if let Some(res) = self.__krui_validate_method(name, args) { return res; }
            #core_validate_tokens
            if let Some(krui::Value::Command(_)) = self.get(name) {
                if !args.is_empty() {
                    return Err(krui::CallError::ArityMismatch { method: name.to_string(), expected: 0, actual: args.len() });
                }
                return Ok(krui::Value::Unit);
            }
            Err(krui::CallError::UnknownMethod(name.to_string()))
        }
    };
    (call_fn, validate_fn)
}

pub(crate) fn emit_struct_registry_impl(
    struct_name: &syn::Ident,
    field_idents: &[&syn::Ident],
) -> TokenStream2 {
    if !cfg!(feature = "preview") {
        return quote! {};
    }
    let field_names: Vec<String> = field_idents.iter().map(|id| id.to_string()).collect();
    let has_core_field = field_names.iter().any(|n| n == "core");
    let (get_fn, signal_fn) = emit_registry_get_and_signal(&field_names, field_idents);
    let (call_fn, validate_fn) = emit_registry_call_and_validate(has_core_field);

    quote! {
        impl krui::Registry for #struct_name {
            #get_fn
            #signal_fn
            #call_fn
            #validate_fn
        }

        impl krui::RegistryFieldExt for #struct_name {
            fn registry_field_get(&self) -> ::std::option::Option<krui::Value> {
                Some(krui::Value::Record(::std::rc::Rc::new(self.clone())))
            }
        }

        impl krui::IntoRegistryValue for #struct_name {
            fn into_registry_value(self) -> krui::Value {
                krui::Value::Record(::std::rc::Rc::new(self))
            }
        }
    }
}

fn extract_dispatchable_method_params(sig: &syn::Signature) -> Option<Vec<&Type>> {
    let m_name = sig.ident.to_string();
    if m_name == "new" || m_name.starts_with("__krui") {
        return None;
    }
    let Some(FnArg::Receiver(recv)) = sig.inputs.first() else {
        return None;
    };
    if recv.mutability.is_some() {
        return None;
    }
    let mut param_types = Vec::new();
    for arg in sig.inputs.iter().skip(1) {
        match arg {
            FnArg::Typed(pat_ty) => {
                if matches!(&*pat_ty.ty, Type::Reference(_))
                    || !matches!(&*pat_ty.pat, Pat::Ident(_))
                {
                    return None;
                }
                param_types.push(&*pat_ty.ty);
            }
            FnArg::Receiver(_) => return None,
        }
    }
    Some(param_types)
}

fn build_method_dispatch_arms(sig: &syn::Signature) -> Option<(TokenStream2, TokenStream2)> {
    let param_types = extract_dispatchable_method_params(sig)?;
    let m_ident = &sig.ident;
    let m_name = m_ident.to_string();
    let expected_arity = param_types.len();
    let arg_extractions: Vec<TokenStream2> = param_types
        .iter()
        .enumerate()
        .map(|(idx, pty)| {
            quote! {
                <#pty as krui::TryFromRegistryValue>::try_from_registry_value(&args[#idx])
                    .ok_or_else(|| krui::CallError::TypeMismatch {
                        method: #m_name.to_string(),
                        arg_index: #idx,
                        expected: ::std::any::type_name::<#pty>(),
                        actual: ::std::format!("{:?}", args[#idx]),
                    })?
            }
        })
        .collect();
    let call_expr = match &sig.output {
        ReturnType::Default => quote! {
            self.#m_ident(#(#arg_extractions),*);
            Ok(krui::Value::Unit)
        },
        ReturnType::Type(_, _) => quote! {
            let res = self.#m_ident(#(#arg_extractions),*);
            Ok(krui::IntoRegistryValue::into_registry_value(res))
        },
    };
    let call_arm = quote! {
        #m_name => {
            if args.len() != #expected_arity {
                return Some(Err(krui::CallError::ArityMismatch { method: #m_name.to_string(), expected: #expected_arity, actual: args.len() }));
            }
            Some((|| -> ::std::result::Result<krui::Value, krui::CallError> { #call_expr })())
        }
    };
    let validate_arm = quote! {
        #m_name => {
            if args.len() != #expected_arity {
                return Some(Err(krui::CallError::ArityMismatch { method: #m_name.to_string(), expected: #expected_arity, actual: args.len() }));
            }
            Some((|| -> ::std::result::Result<krui::Value, krui::CallError> {
                #(let _ = #arg_extractions;)*
                Ok(krui::Value::Unit)
            })())
        }
    };
    Some((call_arm, validate_arm))
}

pub(crate) fn emit_impl_dispatch_helper(item_impl: &ItemImpl) -> TokenStream2 {
    if !cfg!(feature = "preview") {
        return quote! {};
    }
    let self_ty = &item_impl.self_ty;
    let mut arms = Vec::new();
    let mut validate_arms = Vec::new();

    for item in &item_impl.items {
        if let ImplItem::Fn(method) = item {
            if let Some((call_arm, validate_arm)) = build_method_dispatch_arms(&method.sig) {
                arms.push(call_arm);
                validate_arms.push(validate_arm);
            }
        }
    }

    quote! {
        impl #self_ty {
            #[doc(hidden)]
            #[allow(dead_code)]
            pub fn __krui_dispatch_method(
                &self,
                name: &str,
                args: &[krui::Value],
            ) -> ::std::option::Option<::std::result::Result<krui::Value, krui::CallError>> {
                match name {
                    #(#arms)*
                    _ => None,
                }
            }

            #[doc(hidden)]
            #[allow(dead_code)]
            pub fn __krui_validate_method(
                &self,
                name: &str,
                args: &[krui::Value],
            ) -> ::std::option::Option<::std::result::Result<krui::Value, krui::CallError>> {
                match name {
                    #(#validate_arms)*
                    _ => None,
                }
            }
        }
    }
}

pub(crate) fn emit_store_msg_impl(item_enum: &ItemEnum) -> TokenStream2 {
    if !cfg!(feature = "preview") {
        return quote! {};
    }
    let enum_ident = &item_enum.ident;
    let mut unit_arms = Vec::new();
    let mut string_arms = Vec::new();

    for variant in &item_enum.variants {
        let v_ident = &variant.ident;
        let v_str = v_ident.to_string();
        match &variant.fields {
            syn::Fields::Unit => {
                unit_arms.push(quote! {
                    #v_str => Some(Self::#v_ident),
                });
            }
            syn::Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
                let f_ty = &unnamed.unnamed[0].ty;
                let ty_str = quote!(#f_ty).to_string();
                if ty_str == "String" {
                    string_arms.push(quote! {
                        #v_str => Some(Self::#v_ident(input)),
                    });
                }
            }
            _ => {}
        }
    }

    quote! {
        impl krui::StoreMsgRegistry for #enum_ident {
            fn parse_unit_msg(variant: &str) -> ::std::option::Option<Self> {
                match variant {
                    #(#unit_arms)*
                    _ => None,
                }
            }

            fn parse_string_msg(variant: &str, input: ::std::string::String) -> ::std::option::Option<Self> {
                match variant {
                    #(#string_arms)*
                    _ => None,
                }
            }
        }
    }
}
