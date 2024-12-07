use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

pub(crate) fn derive_message(item: TokenStream) -> TokenStream {
    let _input = parse_macro_input!(item as DeriveInput);
    quote! {}.into()
}
