#![recursion_limit = "256"]

extern crate proc_macro;
extern crate proc_macro2;
extern crate quote;
extern crate syn;

use proc_macro::TokenStream;
use syn::parse_macro_input;

mod rpc_attr;
mod rpc_trait;
mod to_delegate_fn;

// todo: [AJ] docs
#[proc_macro_attribute]
pub fn rpc(args: TokenStream, input: TokenStream) -> TokenStream {
	let args_toks = parse_macro_input!(args as syn::AttributeArgs);
	let input_toks = parse_macro_input!(input as syn::Item);

	let output = match rpc_trait::rpc_impl(args_toks, input_toks) {
		Ok(output) => output,
		Err(err) => panic!("[rpc] encountered error: {}", err),
	};

	output.into()
}
