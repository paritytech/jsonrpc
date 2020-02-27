use crate::options::DeriveOptions;
use crate::params_style::ParamStyle;
use crate::rpc_attr::{AttributeKind, PubSubMethodKind, RpcMethodAttribute};
use crate::to_client::generate_client_module;
use crate::to_delegate::{generate_trait_item_method, MethodRegistration, RpcMethod};
use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use std::collections::HashMap;
use syn::{
	fold::{self, Fold},
	parse_quote,
	punctuated::Punctuated,
	Error, Ident, Result, Token,
};

const METADATA_TYPE: &str = "Metadata";

const MISSING_SUBSCRIBE_METHOD_ERR: &str = "Can't find subscribe method, expected a method annotated with `subscribe` \
	 e.g. `#[pubsub(subscription = \"hello\", subscribe, name = \"hello_subscribe\")]`";

const MISSING_UNSUBSCRIBE_METHOD_ERR: &str =
	"Can't find unsubscribe method, expected a method annotated with `unsubscribe` \
	 e.g. `#[pubsub(subscription = \"hello\", unsubscribe, name = \"hello_unsubscribe\")]`";

pub const USING_NAMED_PARAMS_WITH_SERVER_ERR: &str =
	"`params = \"named\"` can only be used to generate a client (on a trait annotated with #[rpc(client)]). \
	 At this time the server does not support named parameters.";

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
			let rpc_method = self.methods.iter().find(|m| m.trait_item == method);
			rpc_method.map_or(true, |rpc| rpc.attr.attr != *a)
		});
		fold::fold_trait_item_method(self, foldable_method)
	}

	fn fold_trait_item_type(&mut self, ty: syn::TraitItemType) -> syn::TraitItemType {
		if ty.ident == METADATA_TYPE {
			self.has_metadata = true;
			let mut ty = ty.clone();
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

fn compute_method_registrations(item_trait: &syn::ItemTrait) -> Result<(Vec<MethodRegistration>, Vec<RpcMethod>)> {
	let methods_result: Result<Vec<_>> = item_trait
		.items
		.iter()
		.filter_map(|trait_item| {
			if let syn::TraitItem::Method(method) = trait_item {
				match RpcMethodAttribute::parse_attr(method) {
					Ok(Some(attr)) => Some(Ok(RpcMethod::new(attr, method.clone()))),
					Ok(None) => None, // non rpc annotated trait method
					Err(err) => Some(Err(syn::Error::new_spanned(method, err))),
				}
			} else {
				None
			}
		})
		.collect();
	let methods = methods_result?;

	let mut pubsub_method_pairs: HashMap<String, (Vec<RpcMethod>, Option<RpcMethod>)> = HashMap::new();
	let mut method_registrations: Vec<MethodRegistration> = Vec::new();

	for method in methods.iter() {
		match &method.attr().kind {
			AttributeKind::Rpc {
				has_metadata,
				is_notification,
				..
			} => {
				if *is_notification {
					method_registrations.push(MethodRegistration::Notification {
						method: method.clone(),
						has_metadata: *has_metadata,
					})
				} else {
					method_registrations.push(MethodRegistration::Standard {
						method: method.clone(),
						has_metadata: *has_metadata,
					})
				}
			}
			AttributeKind::PubSub {
				subscription_name,
				kind,
			} => {
				let (ref mut sub, ref mut unsub) = pubsub_method_pairs
					.entry(subscription_name.clone())
					.or_insert((vec![], None));
				match kind {
					PubSubMethodKind::Subscribe => sub.push(method.clone()),
					PubSubMethodKind::Unsubscribe => {
						if unsub.is_none() {
							*unsub = Some(method.clone())
						} else {
							return Err(syn::Error::new_spanned(
								&method.trait_item,
								format!(
									"Subscription '{}' unsubscribe method is already defined",
									subscription_name
								),
							));
						}
					}
				}
			}
		}
	}

	for (name, pair) in pubsub_method_pairs {
		match pair {
			(subscribers, Some(unsubscribe)) => {
				if subscribers.is_empty() {
					return Err(syn::Error::new_spanned(
						&unsubscribe.trait_item,
						format!("subscription '{}'. {}", name, MISSING_SUBSCRIBE_METHOD_ERR),
					));
				}

				let mut subscriber_args = subscribers.iter().filter_map(|s| s.subscriber_arg());
				if let Some(subscriber_arg) = subscriber_args.next() {
					for next_method_arg in subscriber_args {
						if next_method_arg != subscriber_arg {
							return Err(syn::Error::new_spanned(
								&next_method_arg,
								format!(
									"Inconsistent signature for 'Subscriber' argument: {}, previously defined: {}",
									next_method_arg.clone().into_token_stream(),
									subscriber_arg.clone().into_token_stream()
								),
							));
						}
					}
				}

				method_registrations.push(MethodRegistration::PubSub {
					name: name.clone(),
					subscribes: subscribers.clone(),
					unsubscribe: unsubscribe.clone(),
				});
			}
			(_, None) => {
				return Err(syn::Error::new_spanned(
					&item_trait,
					format!("subscription '{}'. {}", name, MISSING_UNSUBSCRIBE_METHOD_ERR),
				));
			}
		}
	}

	Ok((method_registrations, methods))
}

fn generate_server_module(
	method_registrations: &[MethodRegistration],
	item_trait: &syn::ItemTrait,
	methods: &[RpcMethod],
) -> Result<TokenStream> {
	let has_pubsub_methods = methods.iter().any(RpcMethod::is_pubsub);

	let mut rpc_trait = RpcTrait {
		methods: methods.to_owned(),
		has_pubsub_methods,
		has_metadata: false,
	};
	let mut rpc_server_trait = fold::fold_item_trait(&mut rpc_trait, item_trait.clone());

	let to_delegate_method = generate_trait_item_method(
		&method_registrations,
		&rpc_server_trait,
		rpc_trait.has_metadata,
		has_pubsub_methods,
	)?;

	rpc_server_trait.items.push(syn::TraitItem::Method(to_delegate_method));

	let trait_bounds: Punctuated<syn::TypeParamBound, Token![+]> = parse_quote!(Sized + Send + Sync + 'static);
	rpc_server_trait.supertraits.extend(trait_bounds);

	let optional_pubsub_import = if has_pubsub_methods {
		crate_name("jsonrpc-pubsub").map(|pubsub_name| quote!(use #pubsub_name as _jsonrpc_pubsub;))
	} else {
		Ok(quote!())
	}?;

	let rpc_server_module = quote! {
		/// The generated server module.
		pub mod gen_server {
			#optional_pubsub_import
			use self::_jsonrpc_core::futures as _futures;
			use super::*;

			#rpc_server_trait
		}
	};

	Ok(rpc_server_module)
}

fn rpc_wrapper_mod_name(rpc_trait: &syn::ItemTrait) -> syn::Ident {
	let name = rpc_trait.ident.clone();
	let mod_name = format!("{}{}", RPC_MOD_NAME_PREFIX, name.to_string());
	syn::Ident::new(&mod_name, proc_macro2::Span::call_site())
}

fn has_named_params(methods: &[RpcMethod]) -> bool {
	methods
		.iter()
		.any(|method| method.attr.params_style == Some(ParamStyle::Named))
}

pub fn crate_name(name: &str) -> Result<Ident> {
	proc_macro_crate::crate_name(name)
		.map(|name| Ident::new(&name, Span::call_site()))
		.map_err(|e| Error::new(Span::call_site(), &e))
}

pub fn rpc_impl(input: syn::Item, options: &DeriveOptions) -> Result<proc_macro2::TokenStream> {
	let rpc_trait = match input {
		syn::Item::Trait(item_trait) => item_trait,
		item => {
			return Err(syn::Error::new_spanned(
				item,
				"The #[rpc] custom attribute only works with trait declarations",
			));
		}
	};

	let (method_registrations, methods) = compute_method_registrations(&rpc_trait)?;

	let name = rpc_trait.ident.clone();
	let mod_name_ident = rpc_wrapper_mod_name(&rpc_trait);

	let core_name = crate_name("jsonrpc-core")?;

	let mut submodules = Vec::new();
	let mut exports = Vec::new();
	if options.enable_client {
		let rpc_client_module = generate_client_module(&method_registrations, &rpc_trait, options)?;
		submodules.push(rpc_client_module);
		exports.push(quote! {
			pub use self::#mod_name_ident::gen_client;
		});
	}
	if options.enable_server {
		if has_named_params(&methods) {
			return Err(syn::Error::new_spanned(rpc_trait, USING_NAMED_PARAMS_WITH_SERVER_ERR));
		}
		let rpc_server_module = generate_server_module(&method_registrations, &rpc_trait, &methods)?;
		submodules.push(rpc_server_module);
		exports.push(quote! {
			pub use self::#mod_name_ident::gen_server::#name;
		});
	}
	Ok(quote!(
		mod #mod_name_ident {
			use #core_name as _jsonrpc_core;
			use super::*;

			#(#submodules)*
		}
		#(#exports)*
	))
}
