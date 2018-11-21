extern crate proc_macro;
extern crate proc_macro2;

#[macro_use]
extern crate syn;

#[macro_use]
extern crate quote;

mod error;

use error::Result;

/// Arguments given to the `rpc` attribute macro
//struct RpcArgs {
//	name: String,
//}

//impl RpcArgs {
//	pub fn from_attribute_args(attr_args: syn::AttributeArgs) -> Result<RpcArgs> {
//		let name =
//			if let Some(syn::NestedMeta::Meta(syn::Meta::Word(ident))) = attr_args.get(0) {
//				Ok(ident.to_string())
//			} else {
//				Err("Expected `name` argument in rpc attribute".to_owned())
//			}?;
//		Ok(RpcArgs {
//			name,
//		})
//	}
//}

/// Marker attribute for rpc trait methods, handled in `rpc_api`
#[proc_macro_attribute]
pub fn rpc(_args: proc_macro::TokenStream, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
	input
}

#[proc_macro_attribute]
pub fn rpc_api(args: proc_macro::TokenStream, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
	let args_toks = parse_macro_input!(args as syn::AttributeArgs);
	let input_toks = parse_macro_input!(input as syn::Item);

//	println!("input_toks {:?}", input_toks);

	let output = match impl_rpc(args_toks, input_toks) {
		Ok(output) => output,
		Err(err) => panic!("[rpc_api] encountered error: {}", err),
	};

	let res: proc_macro::TokenStream = output.into();

	println!("Output: {:?}", res);

	res
}

fn impl_rpc(_args: syn::AttributeArgs, input: syn::Item) -> Result<proc_macro2::TokenStream> {

	let item_trait = match input {
		syn::Item::Trait(item_trait) => item_trait,
		_ => return Err("rpc_api trait only works with trait declarations".to_owned())
	};
	// todo: [AJ] extract this and other to struct
	let name = item_trait.ident;

	Ok(quote! {
		pub trait #name {

		}
	})
}

/*
input_toks Trait(ItemTrait { attrs: [], vis: Public(VisPublic { pub_token: Pub }), unsafety: None, auto_token: None, trait_token: Trait, ident: Ident(Rpc), generics: Generics { lt_token: None, params: [], gt_token: None, where_clause: None }, colon_token: None, supertraits: [], brace_token: Brace, items: [Method(TraitItemMethod { attrs: [Attribute { pound_token: Pound, style: Outer, bracket_token: Bracket, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(doc), arguments: None }] }, tts: TokenStream [Punct { op: '=', spacing: Alone }, Literal { lit: " Returns a protocol version" }] }, Attribute { pound_token: Pound, style: Outer, bracket_token: Bracket, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(rpc), arguments: None }] }, tts: TokenStream [Group { delimiter: Parenthesis, stream: TokenStream [Ident { sym: name }, Punct { op: '=', spacing: Alone }, Literal { lit: "protocolVersion" }] }] }], sig: MethodSig { constness: None, unsafety: None, asyncness: None, abi: None, ident: Ident(protocol_version), decl: FnDecl { fn_token: Fn, generics: Generics { lt_token: None, params: [], gt_token: None, where_clause: None }, paren_token: Paren, inputs: [SelfRef(ArgSelfRef { and_token: And, lifetime: None, mutability: None, self_token: SelfValue })], variadic: None, output: Type(RArrow, Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(Result), arguments: AngleBracketed(AngleBracketedGenericArguments { colon2_token: None, lt_token: Lt, args: [Type(Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(String), arguments: None }] } }))], gt_token: Gt }) }] } })) } }, default: None, semi_token: Some(Semi) }), Method(TraitItemMethod { attrs: [Attribute { pound_token: Pound, style: Outer, bracket_token: Bracket, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(doc), arguments: None }] }, tts: TokenStream [Punct { op: '=', spacing: Alone }, Literal { lit: " Adds two numbers and returns a result" }] }, Attribute { pound_token: Pound, style: Outer, bracket_token: Bracket, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(rpc), arguments: None }] }, tts: TokenStream [Group { delimiter: Parenthesis, stream: TokenStream [Ident { sym: name }, Punct { op: '=', spacing: Alone }, Literal { lit: "add" }] }] }], sig: MethodSig { constness: None, unsafety: None, asyncness: None, abi: None, ident: Ident(add), decl: FnDecl { fn_token: Fn, generics: Generics { lt_token: None, params: [], gt_token: None, where_clause: None }, paren_token: Paren, inputs: [SelfRef(ArgSelfRef { and_token: And, lifetime: None, mutability: None, self_token: SelfValue }), Comma, Ignored(Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(u64), arguments: None }] } })), Comma, Ignored(Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(u64), arguments: None }] } }))], variadic: None, output: Type(RArrow, Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(Result), arguments: AngleBracketed(AngleBracketedGenericArguments { colon2_token: None, lt_token: Lt, args: [Type(Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(u64), arguments: None }] } }))], gt_token: Gt }) }] } })) } }, default: None, semi_token: Some(Semi) })] })
*/

//fn generate_eth_endpoint_wrapper(
//	intf: &items::Interface,
//	endpoint_name: &str,
//) -> Result<proc_macro2::TokenStream> {
//	// FIXME: Code duplication with `generate_eth_endpoint_and_client_wrapper`
//	//        We might want to fix this, however it is not critical.
//	//        >>>
//	let name_ident_use = syn::Ident::new(intf.name(), Span::call_site());
//	let mod_name = format!("pwasm_abi_impl_{}", &intf.name().clone());
//	let mod_name_ident = syn::Ident::new(&mod_name, Span::call_site());
//	// FIXME: <<<
//
//	let endpoint_toks = generate_eth_endpoint(endpoint_name, intf);
//	let endpoint_ident = syn::Ident::new(endpoint_name, Span::call_site());
//
//	Ok(quote! {
//		#intf
//		#[allow(non_snake_case)]
//		mod #mod_name_ident {
//			extern crate pwasm_ethereum;
//			extern crate pwasm_abi;
//			use pwasm_abi::types::{H160, H256, U256, Address, Vec};
//			use super::#name_ident_use;
//			#endpoint_toks
//		}
//		pub use self::#mod_name_ident::#endpoint_ident;
//	})
//}


