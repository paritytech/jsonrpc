extern crate proc_macro;
extern crate proc_macro2;

#[macro_use]
extern crate syn;

#[macro_use]
extern crate quote;

use proc_macro::TokenStream;
use proc_macro2::Span;
use syn::{
	Generics, GenericParam, punctuated::Punctuated, TypeParamBound, TraitItemMethod
};

type Result<T> = std::result::Result<T, String>;

/// Arguments given to the `rpc` attribute macro
struct RpcArgs {
	name: String,
	aliases: Vec<String>,
}

fn ident(s: &str) -> syn::Ident {
	syn::Ident::new(s, Span::call_site())
}

impl RpcArgs {
	pub fn from_attribute(attr: syn::Attribute) -> Result<RpcArgs> {
		if let Ok(syn::Meta::List(list)) = attr.parse_meta() {
			let rpc_name = list.nested
				.into_iter()
				.find_map(|nm| {
					if let syn::NestedMeta::Meta(
						syn::Meta::NameValue(
							syn::MetaNameValue {
								ident: i,
								lit: syn::Lit::Str(str),
								eq_token: _
							}
						)
					) = nm {
						if i == ident("name") { Some(str.value()) } else { None }
					} else {
						None
					}
				});

			if let Some(name) = rpc_name {
				Ok(RpcArgs {
					name,
					aliases: vec![],
				})
			} else {
				Err("Missing required rpc name field".to_string())
			}
		} else {
			Err("Expected valid attribute Meta".to_string())
		}
	}
}

/// Marker attribute for rpc trait methods, handled in `rpc_api`
#[proc_macro_attribute]
pub fn rpc(_args: TokenStream, input: proc_macro::TokenStream) -> TokenStream {
	input
}

#[proc_macro_attribute]
pub fn rpc_api(args: TokenStream, input: TokenStream) -> TokenStream {
	let args_toks = parse_macro_input!(args as syn::AttributeArgs);
	let input_toks = parse_macro_input!(input as syn::Item);

	let output = match impl_rpc(args_toks, input_toks) {
		Ok(output) => output,
		Err(err) => panic!("[rpc_api] encountered error: {}", err),
	};

	output.into()
}

fn impl_rpc(_args: syn::AttributeArgs, input: syn::Item) -> Result<proc_macro2::TokenStream> {
	let rpc_trait = match input {
		syn::Item::Trait(item_trait) => item_trait,
		_ => return Err("rpc_api trait only works with trait declarations".to_owned())
	};
	// todo: [AJ] extract this and other to struct
	let name = rpc_trait.ident.clone();

	let mod_name = format!("rpc_impl_{}", name.to_string());
	let mod_name_ident = ident(&mod_name);
	let generics = &rpc_trait.generics;

	let rpc_methods = rpc_trait_methods(&rpc_trait.items);
	let methods : Vec<proc_macro2::TokenStream> = rpc_methods
		.iter()
		.map(|(_, method)| quote! { #method }) // todo: [AJ] impl model to to_tokens?
		.collect();
	let to_delegate = generate_to_delegate_method(generics, &rpc_methods);

	Ok(quote! {
		mod #mod_name_ident {
			extern crate jsonrpc_core as _jsonrpc_core;
			extern crate jsonrpc_macros as _jsonrpc_macros;
			extern crate serde as _serde;
			use super::*;
			pub trait #name #generics : Sized + Send + Sync + 'static {
				#(#methods)*
				#to_delegate
			}
		}
		pub use self::#mod_name_ident::#name;
	})
}

fn rpc_trait_methods(items: &[syn::TraitItem]) -> Vec<(RpcArgs, TraitItemMethod)> {
	items
		.iter()
		.filter_map(|item| {
			if let syn::TraitItem::Method(method) = item {
				let rpc_attr = method.attrs
					.iter()
					.cloned()
					.find(|a| {
						if let Ok(meta) = a.parse_meta() {
							meta.name() == "rpc"
						} else {
							false
						}
					});
				if let Some(rpc_attr) = rpc_attr {
					// strip rpc attribute from method
					let attrs_stripped = method.attrs
						.iter()
						.cloned()
						.filter(|a| *a != rpc_attr)
						.collect();
					let method_stripped = TraitItemMethod {
						attrs: attrs_stripped,
						.. method.clone()
					};
					match RpcArgs::from_attribute(rpc_attr) {
						Ok(rpc_args) => Some((rpc_args, method_stripped)),
						Err(e) => panic!("Failed to parse rpc_args: {:?}", e),
					}
				} else {
					// todo: [AJ] should we really discard non annotated functions?
					// todo: [AJ] we could assume all methods should be rpc and infer names
					None
				}
			} else {
				// todo: [AJ] what to do with other TraitItems? Const/Type/Macro/Verbatim.
				None
			}
		})
		.collect()
}

fn generate_to_delegate_method(
	generics: &Generics,
	rpc_methods: &[(RpcArgs, TraitItemMethod)]
) -> TraitItemMethod {
	let add_methods: Vec<proc_macro2::TokenStream> = rpc_methods
		.into_iter()
		.map(|(attr, trait_method)| {
			let rpc_name = &attr.name;
			let method = &trait_method.sig.ident;
			let arg_types = trait_method.sig.decl.inputs
				.iter()
				.filter_map(|arg| {
					let ty =
						match arg {
							syn::FnArg::Captured(arg_captured) => Some(&arg_captured.ty),
							syn::FnArg::Ignored(ty) => Some(ty),
							// todo: [AJ] what about Inferred?
							_ => None,
						};
					ty.map(|t| quote! { #t })
				});
			let result = match trait_method.sig.decl.output {
				// todo: [AJ] require Result type?
				syn::ReturnType::Type(_, ref output) => output,
				syn::ReturnType::Default => panic!("Return type required for RPC method signature")
			};
			quote! {
				del.add_method(#rpc_name, move |base, params| {
					jsonrpc_macros::WrapAsync::wrap_rpc(&(Self::#method as fn(&_ #(, #arg_types)*) -> #result), base, params)
				});
			}
		})
		.collect();

	let method: syn::TraitItemMethod =
		parse_quote! {
			fn to_delegate<M: _jsonrpc_core::Metadata>(self) -> _jsonrpc_macros::IoDelegate<Self, M>
	//			where $(
	//				$($simple_generics: Send + Sync + 'static + $crate::Serialize + $crate::DeserializeOwned ,)*
	//				$($generics: Send + Sync + 'static $( + $bounds $( + $morebounds )* )* ),*
	//			)*
			{
				let mut del = _jsonrpc_macros::IoDelegate::new(self.into());
				#(#add_methods)*
				del
			}
		};

	// add default bounds where no bounds specified
	// todo: [AJ] add custom bounds
	let trait_bounds: Punctuated<TypeParamBound, Token![+]> = parse_quote!(
		'static
		+ Send
		+ Sync
		+ _serde::Serialize
		+ _serde::de::DeserializeOwned
	);

	let new_predicates = generics
		.type_params()
		.map(|ty| {
			let ty_path = syn::TypePath { qself: None, path: ty.ident.clone().into() };
			syn::WherePredicate::Type(syn::PredicateType {
				lifetimes: None,
				bounded_ty: syn::Type::Path(ty_path),
				colon_token: <Token![:]>::default(),
				bounds: trait_bounds.clone(),
			})
		});

	let mut method = method.clone();
	method.sig.decl.generics
		.make_where_clause()
		.predicates
		.extend(new_predicates);
	method
}

//fn add_trait_bounds(
//	mut generics: Generics,
//	bounds: Punctuated<TypeParamBound, Token![+]>
//) -> Generics {
//	for param in &mut generics.params {
//		if let GenericParam::Type(ref mut type_param) = *param {
//			type_param.bounds.extend(bounds.clone());
//		}
//	}
//	generics
//}


