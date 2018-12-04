extern crate proc_macro;
extern crate proc_macro2;
extern crate quote;
extern crate syn;

use proc_macro::TokenStream;

mod rpc_trait;

/// Marker attribute for rpc trait methods, handled in `rpc_api`
#[proc_macro_attribute]
pub fn rpc(_args: TokenStream, input: proc_macro::TokenStream) -> TokenStream {
	input
}

// todo: [AJ] docs
#[proc_macro_attribute]
pub fn rpc_api(args: TokenStream, input: TokenStream) -> TokenStream {
	rpc_trait::build_rpc_trait_impl(args, input)
}
