extern crate proc_macro;
extern crate proc_macro2;

#[macro_use]
extern crate syn;

#[macro_use]
extern crate quote;

mod error;

use proc_macro2::Span;
use error::Result;

/// Arguments given to the `rpc` attribute macro
struct RpcArgs {
	name: String,
	aliases: Vec<String>,
}

//impl RpcArgs {
//	pub fn from_attribute(attr: syn::Attribute) -> Result<RpcArgs> {
//		let meta = attr.parse_meta().map_error()?;
//
////			if let Some(syn::NestedMeta::Meta(syn::Meta::Word(ident))) = attr_args.get(0) {
////				Ok(ident.to_string())
////			} else {
////				Err("Expected `name` argument in rpc attribute".to_owned())
////			}?;
//		Ok(RpcArgs {
//			name,
//			aliases,
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
	let rpc_trait = match input {
		syn::Item::Trait(item_trait) => item_trait,
		_ => return Err("rpc_api trait only works with trait declarations".to_owned())
	};
	// todo: [AJ] extract this and other to struct
	let name = rpc_trait.ident.clone();

	let mod_name = format!("rpc_impl_{}", name.to_string());
	let mod_name_ident = syn::Ident::new(&mod_name, Span::call_site());

	let rpc_methods = rpc_trait_methods(&rpc_trait.items);
	let methods : Vec<proc_macro2::TokenStream> = rpc_methods
		.iter()
		.map(|(_, method)| quote! { #method }) // todo: [AJ] impl model to to_tokens?
		.collect();
	let to_delegate = generate_to_delegate_method(&rpc_methods);

	Ok(quote! {
		mod #mod_name_ident {
//			extern crate jsonrpc_core;
			pub trait #name : Sized + Send + Sync + 'static {
				#(#methods)*
				#to_delegate
			}
		}
	})
}

fn rpc_trait_methods(items: &[syn::TraitItem]) -> Vec<(syn::Attribute, syn::TraitItemMethod)> {
	items
		.iter()
		.filter_map(|item| {
			match item {
				syn::TraitItem::Method(method) => {
					let rpc_attr = method.attrs
						.iter()
						.cloned()
						.find(|a| attr_has_name(a, "rpc"));
					if let Some(rpc_attr) = rpc_attr {
						// strip rpc attribute from method
						let attrs_stripped = method.attrs
							.iter()
							.cloned()
							.filter(|a| *a != rpc_attr)
							.collect();
						let method_stripped = syn::TraitItemMethod {
							attrs: attrs_stripped,
							sig: method.sig.clone(),
							default: method.default.clone(),
							semi_token: method.semi_token,
						};
						Some((rpc_attr, method_stripped))
					} else {
						// todo: [AJ] should we really discard non annotated functions? check old behaviour
						None
					}
				},
				// todo: [AJ] what to do with other TraitItems? Const/Type/Macro/Verbatim.
				_ => None
			}
		})
		.collect()
}

fn generate_to_delegate_method(rpc_methods: &[(syn::Attribute, syn::TraitItemMethod)]) -> proc_macro2::TokenStream {
//	let add_methods = rpc_methods
//		.into_iter()
//		.map(|attr, method| quote! {
		// todo: [AJ] extract name from attr
//			del.add_method($name, move |base, params| {
//				$crate::WrapAsync::wrap_rpc(&(Self::$method as fn(&_ $(, $param)*) -> $result <$out $(, $error)*>), base, params)
//			});
//		});
	quote! {
		fn to_delegate<M: jsonrpc_core::Metadata>(self) -> jsonrpc_core::IoDelegate<Self, M>
//			where $(
//				$($simple_generics: Send + Sync + 'static + $crate::Serialize + $crate::DeserializeOwned ,)*
//				$($generics: Send + Sync + 'static $( + $bounds $( + $morebounds )* )* ),*
//			)*
		{
			let mut del = jsonrpc_core::IoDelegate::new(self.into());
//			$(
//				build_rpc_trait!(WRAP del =>
//					( $($t)* )
//					fn $m_name ( $($p)* ) -> $result <$out $(, $error)* >
//				);
//			)*
			del
		}
	}
}

fn attr_has_name(attr: &syn::Attribute, name: &str) -> bool {
	if let Some(first_seg) = attr.path.segments.first() {
		return first_seg.value().ident == name
	};
	false
}

/*
input_toks Trait(ItemTrait { attrs: [], vis: Public(VisPublic { pub_token: Pub }), unsafety: None, auto_token: None, trait_token: Trait, ident: Ident(Rpc), generics: Generics { lt_token: None, params: [], gt_token: None, where_clause: None }, colon_token: None, supertraits: [], brace_token: Brace, items: [Method(TraitItemMethod { attrs: [Attribute { pound_token: Pound, style: Outer, bracket_token: Bracket, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(doc), arguments: None }] }, tts: TokenStream [Punct { op: '=', spacing: Alone }, Literal { lit: " Returns a protocol version" }] }, Attribute { pound_token: Pound, style: Outer, bracket_token: Bracket, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(rpc), arguments: None }] }, tts: TokenStream [Group { delimiter: Parenthesis, stream: TokenStream [Ident { sym: name }, Punct { op: '=', spacing: Alone }, Literal { lit: "protocolVersion" }] }] }], sig: MethodSig { constness: None, unsafety: None, asyncness: None, abi: None, ident: Ident(protocol_version), decl: FnDecl { fn_token: Fn, generics: Generics { lt_token: None, params: [], gt_token: None, where_clause: None }, paren_token: Paren, inputs: [SelfRef(ArgSelfRef { and_token: And, lifetime: None, mutability: None, self_token: SelfValue })], variadic: None, output: Type(RArrow, Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(Result), arguments: AngleBracketed(AngleBracketedGenericArguments { colon2_token: None, lt_token: Lt, args: [Type(Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(String), arguments: None }] } }))], gt_token: Gt }) }] } })) } }, default: None, semi_token: Some(Semi) }), Method(TraitItemMethod { attrs: [Attribute { pound_token: Pound, style: Outer, bracket_token: Bracket, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(doc), arguments: None }] }, tts: TokenStream [Punct { op: '=', spacing: Alone }, Literal { lit: " Adds two numbers and returns a result" }] }, Attribute { pound_token: Pound, style: Outer, bracket_token: Bracket, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(rpc), arguments: None }] }, tts: TokenStream [Group { delimiter: Parenthesis, stream: TokenStream [Ident { sym: name }, Punct { op: '=', spacing: Alone }, Literal { lit: "add" }] }] }], sig: MethodSig { constness: None, unsafety: None, asyncness: None, abi: None, ident: Ident(add), decl: FnDecl { fn_token: Fn, generics: Generics { lt_token: None, params: [], gt_token: None, where_clause: None }, paren_token: Paren, inputs: [SelfRef(ArgSelfRef { and_token: And, lifetime: None, mutability: None, self_token: SelfValue }), Comma, Ignored(Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(u64), arguments: None }] } })), Comma, Ignored(Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(u64), arguments: None }] } }))], variadic: None, output: Type(RArrow, Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(Result), arguments: AngleBracketed(AngleBracketedGenericArguments { colon2_token: None, lt_token: Lt, args: [Type(Path(TypePath { qself: None, path: Path { leading_colon: None, segments: [PathSegment { ident: Ident(u64), arguments: None }] } }))], gt_token: Gt }) }] } })) } }, default: None, semi_token: Some(Semi) })] })
*/


