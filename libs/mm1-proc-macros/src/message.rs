use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{Generics, Ident, Item, LitBool, Path, PredicateType, Token, parse_macro_input};

struct MessageArgs {
    base_path:          Option<Path>,
    derive_serialize:   bool,
    derive_deserialize: bool,
}
pub(crate) fn message(attr: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as MessageArgs);
    let input = parse_macro_input!(input as Item);

    let (ident, mut generics): (Ident, Generics) = match input {
        Item::Struct(ref s) => (s.ident.clone(), s.generics.clone()),
        Item::Enum(ref e) => (e.ident.clone(), e.generics.clone()),
        _ => {
            return syn::Error::new_spanned(
                input,
                "#[message] can only be applied to a struct or an enum",
            )
            .to_compile_error()
            .into()
        },
    };

    let where_clause = generics.make_where_clause();
    where_clause.predicates.push(
        PredicateType {
            lifetimes:   None,
            bounded_ty:  syn::parse_quote!(Self),
            colon_token: Default::default(),
            bounds:      {
                let mut b = Punctuated::new();
                b.push(syn::parse_quote!(::serde::Serialize));
                b.push(syn::parse_quote!(Send));
                b.push(syn::parse_quote!('static));
                b
            },
        }
        .into(),
    );
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let base_path: Path = args
        .base_path
        .unwrap_or_else(|| syn::parse_quote!(::mm1::proto));

    let derive_serialize = args
        .derive_serialize
        .then_some(quote!( #[derive(::serde::Serialize)] ));
    let derive_deserialize = args
        .derive_deserialize
        .then_some(quote!( #[derive(::serde::Deserialize)] ));

    let automatically_derived = quote! { #[automatically_derived] };

    quote! {
        #derive_serialize
        #derive_deserialize
        #input

        #automatically_derived
        impl #impl_generics #base_path :: Message for #ident #ty_generics #where_clause {}
    }
    .into()
}

enum MessageArg {
    BasePath(Path),
    DeriveSerialize(bool),
    DeriveDeserialize(bool),
}

impl Parse for MessageArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(Self {
                base_path:          None,
                derive_serialize:   true,
                derive_deserialize: true,
            });
        }

        let mut base_path = None;
        let mut derive_serialize = true;
        let mut derive_deserialize = true;
        let args = Punctuated::<MessageArg, Comma>::parse_terminated(input)?;

        for arg in args {
            match arg {
                MessageArg::BasePath(path) => {
                    base_path = Some(path);
                },
                MessageArg::DeriveSerialize(value) => {
                    derive_serialize = value;
                },
                MessageArg::DeriveDeserialize(value) => {
                    derive_deserialize = value;
                },
            }
        }

        Ok(Self {
            base_path,
            derive_serialize,
            derive_deserialize,
        })
    }
}

impl Parse for MessageArg {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let ident: Ident = input.parse()?;
        let _eq_token: Token![=] = input.parse()?;

        if ident == "base_path" {
            let path: Path = input.parse()?;
            Ok(MessageArg::BasePath(path))
        } else if ident == "derive_serialize" {
            let derive_serialize: LitBool = input.parse()?;
            Ok(MessageArg::DeriveSerialize(derive_serialize.value))
        } else if ident == "derive_deserialize" {
            let derive_deserialize: LitBool = input.parse()?;
            Ok(MessageArg::DeriveDeserialize(derive_deserialize.value))
        } else {
            Err(syn::Error::new_spanned(
                ident,
                "unexpected argument, expected `base_path`",
            ))
        }
    }
}
