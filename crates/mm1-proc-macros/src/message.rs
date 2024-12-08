use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Item};

pub(crate) fn message(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as Item);

    quote! {
        #[derive(Clone, ::serde::Serialize, ::serde::Deserialize)]
        #input
    }
    .into()
}
