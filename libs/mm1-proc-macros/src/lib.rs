// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

use proc_macro::TokenStream;

mod dispatch;
mod message;

#[proc_macro_attribute]
pub fn message(attr: TokenStream, item: TokenStream) -> TokenStream {
    message::message(attr, item)
}

#[proc_macro]
pub fn dispatch(input: TokenStream) -> TokenStream {
    dispatch::dispatch(input)
}
