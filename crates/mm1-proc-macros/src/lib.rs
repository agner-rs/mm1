// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

use proc_macro::TokenStream;

mod derive_message;
mod derive_traversable;
mod dispatch;

#[proc_macro_derive(Message)]
pub fn derive_message(item: TokenStream) -> TokenStream {
    derive_message::derive_message(item)
}

#[proc_macro_derive(Traversable)]
pub fn derive_traversable(item: TokenStream) -> TokenStream {
    derive_traversable::derive_traversable(item)
}

#[proc_macro]
pub fn dispatch(input: TokenStream) -> TokenStream {
    dispatch::dispatch(input)
}
