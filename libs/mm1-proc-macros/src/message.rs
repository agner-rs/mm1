use proc_macro::TokenStream;
use quote::quote;
use syn::{Item, parse_macro_input};

pub(crate) fn message(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as Item);

    quote! {
        // #[derive(::serde::Serialize, ::serde::Deserialize)]
        #input
    }
    .into()
}
