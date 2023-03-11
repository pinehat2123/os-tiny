extern crate proc_macro;

use proc_macro::TokenStream;

#[proc_macro_attribute] pub fn move_function(attr: TokenStream, item: TokenStream) -> TokenStream {
    let _metadata = attr;
    let _ast = item;
    "// function item was moved".parse().unwrap()
}