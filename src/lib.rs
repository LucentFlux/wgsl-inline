#![doc = include_str!("../README.md")]

mod result;
mod source;

use quote::ToTokens;
use source::Sourcecode;

#[proc_macro]
pub fn wgsl(shader: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let mut sourcecode = Sourcecode::new();
    sourcecode.append_tokens(shader);

    let mut result = sourcecode.complete();

    result.validate();

    let mut tokens = proc_macro2::TokenStream::new();
    for item in result.to_items() {
        item.to_tokens(&mut tokens);
    }
    tokens.into()
}
