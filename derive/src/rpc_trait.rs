use quote::quote;
use syn::{
	parse_quote, Token, punctuated::Punctuated,
	fold::{self, Fold},
};
use crate::rpc_attr::{RpcTraitAttribute, RpcMethodAttribute};
use crate::to_delegate::{RpcMethod, ToDelegateMethod};

const METADATA_TYPE: &'static str = "Metadata";

const MISSING_SUBSCRIBE_METHOD_ERR: &'static str =
	"Can't find subscribe method, expected a method annotated with `subscribe` \
	e.g. `#[rpc(subscribe, name = \"hello_subscribe\")]`";

const MISSING_UNSUBSCRIBE_METHOD_ERR: &'static str =
	"Can't find unsubscribe method, expected a method annotated with `unsubscribe` \
	e.g. `#[rpc(unsubscribe, name = \"hello_unsubscribe\")]`";

const RPC_MOD_NAME_PREFIX: &'static str = "rpc_impl_";

type Result<T> = std::result::Result<T, String>;

struct RpcTrait {
	attr: RpcTraitAttribute,
	methods: Vec<RpcMethod>,
	has_metadata: bool,
	errors: Vec<String>,
}

impl<'a> Fold for RpcTrait {
	fn fold_trait_item_method(&mut self, method: syn::TraitItemMethod) -> syn::TraitItemMethod {
		let mut method = method.clone();
		match RpcMethodAttribute::try_from_trait_item_method(&method) {
			Ok(Some(attr)) => {
				self.methods.push(RpcMethod::new(
					attr.clone(),
					method.clone(),
				));
				// remove the rpc attribute
				method.attrs.retain(|a| *a != attr.attr);
			},
			Ok(None) => (), // non rpc annotated trait method
			Err(err) => self.errors.push(format!("{}: Invalid rpc method attribute: {}", method.sig.ident, err))
		}
		fold::fold_trait_item_method(self, method)
	}

	fn fold_trait_item_type(&mut self, ty: syn::TraitItemType) -> syn::TraitItemType {
		if ty.ident == METADATA_TYPE {
			self.has_metadata = true;
			let mut ty = ty.clone();
			match self.attr {
				RpcTraitAttribute::RpcTrait =>
					ty.bounds.push(parse_quote!(_jsonrpc_core::Metadata)),
				RpcTraitAttribute::PubSubTrait { name: _ } =>
					ty.bounds.push(parse_quote!(_jsonrpc_pubsub::PubSubMetadata)),
			}
			return ty;
		}
		ty
	}
}

fn generate_rpc_item_trait(attr_args: &syn::AttributeArgs, item_trait: &syn::ItemTrait) -> Result<syn::ItemTrait> {
	let trait_attr = RpcTraitAttribute::try_from_trait_attribute(&attr_args)?;
	let mut visitor = RpcTrait {
		attr: trait_attr,
		methods: Vec::new(),
		has_metadata: false,
		errors: Vec::new(),
	};

	// first visit the trait to collect the methods
	let mut item_trait = fold::fold_item_trait(&mut visitor, item_trait.clone());

	if !visitor.errors.is_empty() {
		let errs = visitor.errors.join("\n  ");
		return Err(format!("Invalid rpc trait:\n  {}", errs))
	}

	let to_delegate =
		match visitor.attr {
			RpcTraitAttribute::RpcTrait => {
				if !visitor.methods.is_empty() {
					Ok(ToDelegateMethod::Standard(visitor.methods))
				} else {
					Err("No rpc annotated trait items found".into())
				}
			},
			RpcTraitAttribute::PubSubTrait { name } => {
				let subscribe = visitor.methods
					.iter()
					.find(|m| m.attr().is_subscribe);
				let unsubscribe = visitor.methods
					.iter()
					.find(|m| m.attr().is_unsubscribe);

				match (subscribe, unsubscribe) {
					(Some(sub), Some(unsub)) => {
						// todo: [AJ] validate subscribe/unsubscribe args
//						let sub_arg_types = sub.get_method_arg_types();

						Ok(ToDelegateMethod::PubSub {
							name,
							subscribe: sub.clone(),
							unsubscribe: unsub.clone()
						})
					},
					(Some(_), None) => Err(MISSING_UNSUBSCRIBE_METHOD_ERR.into()),
					(None, Some(_)) => Err(MISSING_SUBSCRIBE_METHOD_ERR.into()),
					(None, None) => Err(format!("\n{}\n{}", MISSING_SUBSCRIBE_METHOD_ERR, MISSING_UNSUBSCRIBE_METHOD_ERR)),
				}
			}
		}?;

	let to_delegate_method =
		to_delegate.generate_trait_item_method(&item_trait, visitor.has_metadata);
	item_trait.items.push(syn::TraitItem::Method(to_delegate_method));

	let trait_bounds: Punctuated<syn::TypeParamBound, Token![+]> =
		parse_quote!(Sized + Send + Sync + 'static);
	item_trait.supertraits.extend(trait_bounds);

	Ok(item_trait)
}

fn rpc_wrapper_mod_name(rpc_trait: &syn::ItemTrait) -> syn::Ident {
	let name = rpc_trait.ident.clone();
	let mod_name = format!("{}{}", RPC_MOD_NAME_PREFIX, name.to_string());
	syn::Ident::new(&mod_name, proc_macro2::Span::call_site())
}

pub fn rpc_impl(args: syn::AttributeArgs, input: syn::Item) -> Result<proc_macro2::TokenStream> {
	let rpc_trait = match input {
		syn::Item::Trait(item_trait) => item_trait,
		_ => return Err("rpc_api trait only works with trait declarations".into())
	};

	let rpc_trait = generate_rpc_item_trait(&args, &rpc_trait)?;

	let name = rpc_trait.ident.clone();
	let mod_name_ident = rpc_wrapper_mod_name(&rpc_trait);

	Ok(quote! {
		mod #mod_name_ident {
			extern crate jsonrpc_core as _jsonrpc_core;
			extern crate jsonrpc_pubsub as _jsonrpc_pubsub;
			extern crate serde as _serde;
			use super::*;
			use self::_jsonrpc_core::futures as _futures;

			#rpc_trait
		}
		pub use self::#mod_name_ident::#name;
	})
}
