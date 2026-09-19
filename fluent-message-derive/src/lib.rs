//! `#[derive(FluentMessage)]` - turns an enum into a Fluent message descriptor.
//!
//! See the `fluent-message` crate for documentation; this crate is an
//! implementation detail and is re-exported from there.

extern crate proc_macro;

use heck::ToKebabCase;
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Attribute, Data, DeriveInput, Error, Fields, LitStr, Path, Result, Variant,
};

#[proc_macro_derive(FluentMessage, attributes(fluent))]
pub fn derive_fluent_message(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

// ---------------------------------------------------------------------------
// attribute parsing
// ---------------------------------------------------------------------------

/// `#[fluent(...)]` options allowed on the enum itself.
struct ContainerOpts {
    /// Prepended to every message id: `prefix` + `separator` + id.
    prefix: Option<String>,
    separator: String,
    /// Path to the runtime crate, for re-exporting users.
    krate: Path,
}

impl ContainerOpts {
    fn parse(attrs: &[Attribute]) -> Result<Self> {
        let mut out = ContainerOpts {
            prefix: None,
            separator: "-".to_owned(),
            krate: syn::parse_quote!(::fluent_message),
        };
        for attr in attrs.iter().filter(|a| a.path().is_ident("fluent")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("prefix") {
                    out.prefix = Some(meta.value()?.parse::<LitStr>()?.value());
                } else if meta.path.is_ident("separator") {
                    out.separator = meta.value()?.parse::<LitStr>()?.value();
                } else if meta.path.is_ident("crate_path") {
                    out.krate = meta.value()?.parse::<LitStr>()?.parse()?;
                } else {
                    return Err(meta
                        .error("unknown option, expected `prefix`, `separator` or `crate_path`"));
                }
                Ok(())
            })?;
        }
        Ok(out)
    }
}

/// `#[fluent(...)]` options allowed on a variant.
#[derive(Default)]
struct VariantOpts {
    /// Message id, still subject to the container prefix.
    id: Option<String>,
    /// Message id, *not* subject to the container prefix.
    full_id: Option<String>,
}

impl VariantOpts {
    fn parse(attrs: &[Attribute]) -> Result<Self> {
        let mut out = VariantOpts::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("fluent")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("id") {
                    out.id = Some(meta.value()?.parse::<LitStr>()?.value());
                } else if meta.path.is_ident("full_id") {
                    out.full_id = Some(meta.value()?.parse::<LitStr>()?.value());
                } else {
                    return Err(meta.error("unknown option, expected `id` or `full_id`"));
                }
                Ok(())
            })?;
        }
        if out.id.is_some() && out.full_id.is_some() {
            return Err(Error::new(
                Span::call_site(),
                "`id` and `full_id` are mutually exclusive",
            ));
        }
        Ok(out)
    }
}

/// `#[fluent(...)]` options allowed on a field.
#[derive(Default)]
struct FieldOpts {
    /// Override the Fluent argument name.
    name: Option<String>,
    /// Do not pass this field to Fluent at all.
    skip: bool,
    /// Use `ToString` instead of `ToFluentValue`.
    display: bool,
}

impl FieldOpts {
    fn parse(attrs: &[Attribute]) -> Result<Self> {
        let mut out = FieldOpts::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("fluent")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    out.name = Some(meta.value()?.parse::<LitStr>()?.value());
                } else if meta.path.is_ident("skip") {
                    out.skip = true;
                } else if meta.path.is_ident("display") {
                    out.display = true;
                } else {
                    return Err(meta.error("unknown option, expected `name`, `skip` or `display`"));
                }
                Ok(())
            })?;
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// expansion
// ---------------------------------------------------------------------------

fn expand(input: DeriveInput) -> Result<TokenStream2> {
    let data = match &input.data {
        Data::Enum(data) => data,
        _ => {
            return Err(Error::new(
                Span::call_site(),
                "#[derive(FluentMessage)] can only be applied to enums",
            ))
        }
    };

    let container = ContainerOpts::parse(&input.attrs)?;
    let krate = &container.krate;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    if data.variants.is_empty() {
        // An uninhabited enum: every method is unreachable.
        return Ok(quote! {
            #[automatically_derived]
            impl #impl_generics #krate::FluentMessage for #name #ty_generics #where_clause {
                fn msg_id(&self) -> &'static str { match *self {} }
                fn args(&self) -> #krate::FluentArgsMap<'_> { match *self {} }
                fn has_args(&self) -> bool { match *self {} }
            }
        });
    }

    let mut id_arms = Vec::new();
    let mut args_arms = Vec::new();
    let mut has_args_arms = Vec::new();

    for variant in &data.variants {
        let vname = &variant.ident;
        let id = message_id(&container, variant)?;

        let wildcard = match &variant.fields {
            Fields::Named(_) => quote!(Self::#vname { .. }),
            Fields::Unnamed(_) => quote!(Self::#vname(..)),
            Fields::Unit => quote!(Self::#vname),
        };
        id_arms.push(quote!(#wildcard => #id,));

        let (pattern, inserts) = variant_body(krate, variant)?;
        let len = inserts.len();
        let has = len > 0;
        has_args_arms.push(quote!(#wildcard => #has,));
        args_arms.push(quote! {
            #pattern => {
                let mut __args: #krate::FluentArgsMap<'_> =
                    ::std::collections::HashMap::with_capacity(#len);
                #(#inserts)*
                __args
            }
        });
    }

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #krate::FluentMessage for #name #ty_generics #where_clause {
            fn msg_id(&self) -> &'static str {
                match self { #(#id_arms)* }
            }

            fn args(&self) -> #krate::FluentArgsMap<'_> {
                match self { #(#args_arms)* }
            }

            fn has_args(&self) -> bool {
                match self { #(#has_args_arms)* }
            }
        }
    })
}

/// `full_id`, else `prefix` + kebab-cased variant name (or explicit `id`).
fn message_id(container: &ContainerOpts, variant: &Variant) -> Result<String> {
    let opts = VariantOpts::parse(&variant.attrs)?;
    if let Some(full) = opts.full_id {
        return Ok(full);
    }
    let base = opts
        .id
        .unwrap_or_else(|| variant.ident.to_string().to_kebab_case());
    Ok(match &container.prefix {
        Some(prefix) => format!("{}{}{}", prefix, container.separator, base),
        None => base,
    })
}

/// Builds the match pattern for a variant plus the `insert` statements.
fn variant_body(krate: &Path, variant: &Variant) -> Result<(TokenStream2, Vec<TokenStream2>)> {
    let vname = &variant.ident;
    let mut inserts = Vec::new();

    let pattern = match &variant.fields {
        Fields::Unit => quote!(Self::#vname),
        Fields::Named(fields) => {
            let mut pats = Vec::new();
            for field in &fields.named {
                let ident = field.ident.as_ref().expect("named field");
                let opts = FieldOpts::parse(&field.attrs)?;
                if opts.skip {
                    pats.push(quote!(#ident: _));
                    continue;
                }
                let key = opts
                    .name
                    .clone()
                    .unwrap_or_else(|| ident.to_string().trim_start_matches("r#").to_owned());
                inserts.push(insert_stmt(krate, &key, &quote!(#ident), &opts));
                pats.push(quote!(#ident));
            }
            quote!(Self::#vname { #(#pats),* })
        }
        Fields::Unnamed(fields) => {
            let mut pats = Vec::new();
            for (index, field) in fields.unnamed.iter().enumerate() {
                let opts = FieldOpts::parse(&field.attrs)?;
                if opts.skip {
                    pats.push(quote!(_));
                    continue;
                }
                let binding = format_ident!("__field{}", index);
                // Fluent identifiers must start with a letter, so `0` is not
                // usable as an argument name: default to `arg0`, `arg1`, ...
                let key = opts.name.clone().unwrap_or_else(|| format!("arg{index}"));
                inserts.push(insert_stmt(krate, &key, &quote!(#binding), &opts));
                pats.push(quote!(#binding));
            }
            quote!(Self::#vname(#(#pats),*))
        }
    };

    Ok((pattern, inserts))
}

fn insert_stmt(krate: &Path, key: &str, binding: &TokenStream2, opts: &FieldOpts) -> TokenStream2 {
    let value = if opts.display {
        quote!(#krate::FluentValue::from(::std::string::ToString::to_string(#binding)))
    } else {
        quote!(#krate::ToFluentValue::to_fluent_value(#binding))
    };
    quote! {
        __args.insert(::std::borrow::Cow::Borrowed(#key), #value);
    }
}
