use std::collections::HashMap;
use proc_macro2::Span;
use quote::quote;
use syn::{
	parse_quote, Token, punctuated::Punctuated,
	fold::{self, Fold}, Result, Error, Ident
};
use crate::rpc_attr::{RpcMethodAttribute, PubSubMethodKind, AttributeKind};
use crate::to_delegate::{RpcMethod, MethodRegistration, generate_trait_item_method};

const METADATA_TYPE: &str = "Metadata";

const MISSING_SUBSCRIBE_METHOD_ERR: &str =
	"Can't find subscribe method, expected a method annotated with `subscribe` \
	e.g. `#[pubsub(subscription = \"hello\", subscribe, name = \"hello_subscribe\")]`";

const MISSING_UNSUBSCRIBE_METHOD_ERR: &str =
	"Can't find unsubscribe method, expected a method annotated with `unsubscribe` \
	e.g. `#[pubsub(subscription = \"hello\", unsubscribe, name = \"hello_unsubscribe\")]`";

const RPC_MOD_NAME_PREFIX: &str = "rpc_impl_";

struct RpcTrait {
	has_pubsub_methods: bool,
	methods: Vec<RpcMethod>,
	has_metadata: bool,
}

impl<'a> Fold for RpcTrait {
	fn fold_trait_item_method(&mut self, method: syn::TraitItemMethod) -> syn::TraitItemMethod {
		let mut foldable_method = method.clone();
		// strip rpc attributes
		foldable_method.attrs.retain(|a| {
			let rpc_method =
				self.methods.iter().find(|m| m.trait_item == method);
			rpc_method.map_or(true, |rpc| rpc.attr.attr != *a)
		});
		fold::fold_trait_item_method(self, foldable_method)
	}

	fn fold_trait_item_type(&mut self, ty: syn::TraitItemType) -> syn::TraitItemType {
		if ty.ident == METADATA_TYPE {
			self.has_metadata = true;
			let mut ty = ty;
			if self.has_pubsub_methods {
				ty.bounds.push(parse_quote!(_jsonrpc_pubsub::PubSubMetadata))
			} else {
				ty.bounds.push(parse_quote!(_jsonrpc_core::Metadata))
			}
			return ty;
		}
		ty
	}
}

fn generate_rpc_item_trait(item_trait: &syn::ItemTrait) -> Result<(syn::ItemTrait, bool)> {
	let methods_result: Result<Vec<_>> = item_trait.items
		.iter()
		.filter_map(|trait_item| {
			if let syn::TraitItem::Method(method) = trait_item {
				match RpcMethodAttribute::parse_attr(method) {
					Ok(Some(attr)) =>
						Some(Ok(RpcMethod::new(
							attr,
							method.clone(),
						))),
					Ok(None) => None, // non rpc annotated trait method
					Err(err) => Some(Err(syn::Error::new_spanned(method, err))),
				}
			} else {
				None
			}
		})
		.collect();
	let methods = methods_result?;
	let has_pubsub_methods = methods.iter().any(RpcMethod::is_pubsub);
	let mut rpc_trait = RpcTrait { methods: methods.clone(), has_pubsub_methods, has_metadata: false };
	let mut item_trait = fold::fold_item_trait(&mut rpc_trait, item_trait.clone());

	let mut pubsub_method_pairs: HashMap<String, (Option<RpcMethod>, Option<RpcMethod>)> = HashMap::new();
	let mut method_registrations: Vec<MethodRegistration> = Vec::new();

	for method in &methods {
		match &method.attr().kind {
			AttributeKind::Rpc { has_metadata } =>
				method_registrations.push(MethodRegistration::Standard {
					method: method.clone(),
					has_metadata: *has_metadata
				}),
			AttributeKind::PubSub { subscription_name, kind } => {
				let (ref mut sub, ref mut unsub) = pubsub_method_pairs
					.entry(subscription_name.clone())
					.or_insert((None, None));
				match kind {
					PubSubMethodKind::Subscribe => {
						if sub.is_none() {
							*sub = Some(method.clone())
						} else {
							return Err(syn::Error::new_spanned(&method.trait_item,
								format!("Subscription '{}' subscribe method is already defined", subscription_name)))
						}
					},
					PubSubMethodKind::Unsubscribe => {
						if unsub.is_none() {
							*unsub = Some(method.clone())
						} else {
							return Err(syn::Error::new_spanned(&method.trait_item,
								format!("Subscription '{}' unsubscribe method is already defined", subscription_name)))
						}
					},
				}
			},
		}
	}

	for (name, pair) in pubsub_method_pairs {
		match pair {
			(Some(subscribe), Some(unsubscribe)) =>
				method_registrations.push(MethodRegistration::PubSub {
					name: name.clone(),
					subscribe: subscribe.clone(),
					unsubscribe: unsubscribe.clone()
				}),
			(Some(method), None) => return Err(syn::Error::new_spanned(&method.trait_item,
				format!("subscription '{}'. {}", name, MISSING_UNSUBSCRIBE_METHOD_ERR))),
			(None, Some(method)) => return Err(syn::Error::new_spanned(&method.trait_item,
				format!("subscription '{}'. {}", name, MISSING_SUBSCRIBE_METHOD_ERR))),
			(None, None) => unreachable!(),
		}
	}

	let to_delegate_method =
		generate_trait_item_method(&method_registrations, &item_trait, rpc_trait.has_metadata, has_pubsub_methods)?;
	item_trait.items.push(syn::TraitItem::Method(to_delegate_method));

	let trait_bounds: Punctuated<syn::TypeParamBound, Token![+]> =
		parse_quote!(Sized + Send + Sync + 'static);
	item_trait.supertraits.extend(trait_bounds);

	Ok((item_trait, has_pubsub_methods))
}

fn rpc_wrapper_mod_name(rpc_trait: &syn::ItemTrait) -> syn::Ident {
	let name = rpc_trait.ident.clone();
	let mod_name = format!("{}{}", RPC_MOD_NAME_PREFIX, name.to_string());
	syn::Ident::new(&mod_name, proc_macro2::Span::call_site())
}

pub fn rpc_impl(input: syn::Item) -> Result<proc_macro2::TokenStream> {
	let rpc_trait = match input {
		syn::Item::Trait(item_trait) => item_trait,
		item => return Err(syn::Error::new_spanned(item, "The #[rpc] custom attribute only works with trait declarations")),
	};

	let (rpc_trait, has_pubsub_methods) = generate_rpc_item_trait(&rpc_trait)?;

	let name = rpc_trait.ident.clone();
	let mod_name_ident = rpc_wrapper_mod_name(&rpc_trait);

	let crate_name = |name| {
		proc_macro_crate::crate_name(name)
			.map(|name| Ident::new(&name, Span::call_site()))
			.map_err(|e| Error::new(Span::call_site(), &e))
	};

	let optional_pubsub_import =
		if has_pubsub_methods {
			crate_name("jsonrpc-pubsub")
				.map(|pubsub_name| quote!(use #pubsub_name as _jsonrpc_pubsub;))
		} else {
			Ok(quote!())
		}?;

	let core_name = crate_name("jsonrpc-core")?;
	let serde_name = crate_name("serde")?;

	Ok(quote!(
		mod #mod_name_ident {
			use #core_name as _jsonrpc_core;
			#optional_pubsub_import
			use #serde_name as _serde;
			use super::*;
			use self::_jsonrpc_core::futures as _futures;

			#rpc_trait
		}
		pub use self::#mod_name_ident::#name;
	))
}
