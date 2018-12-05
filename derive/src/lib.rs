extern crate proc_macro;
extern crate proc_macro2;
extern crate quote;
extern crate syn;

use proc_macro::TokenStream;

mod rpc_trait;

// todo: [AJ] docs
#[proc_macro_attribute]
pub fn rpc(args: TokenStream, input: TokenStream) -> TokenStream {
	rpc_trait::build_rpc_trait_impl(args, input)
}
