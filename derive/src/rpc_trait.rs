use quote::quote;
use syn::{
	parse_quote, Token, punctuated::Punctuated,
	fold::{self, Fold},
};
use rpc_attr::{RpcTraitAttribute, RpcMethodAttribute};
use to_delegate_fn::{RpcMethod, ToDelegateFunction};

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
					&attr.name,
					attr.aliases.clone(),
					attr.has_metadata,
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
		if ty.ident.to_string() == "Metadata" {
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
					Ok(ToDelegateFunction::Standard(visitor.methods))
				} else {
					Err("No rpc annotated trait items found")
				}
			},
			RpcTraitAttribute::PubSubTrait { name } => {
				let subscribe = visitor.methods
					.iter()
					.find(|m| m.name_contains("subscribe") && !m.name_contains("unsubscribe"));
				let unsubscribe = visitor.methods
					.iter()
					.find(|m| m.name_contains("unsubscribe"));

				match (subscribe, unsubscribe) {
					(Some(sub), Some(unsub)) => {
						// todo: [AJ] validate subscribe/unsubscribe args
//						let sub_arg_types = sub.get_method_arg_types();

						Ok(ToDelegateFunction::PubSub {
							name,
							subscribe: sub.clone(),
							unsubscribe: unsub.clone()
						})
					},
					(Some(_), None) => Err("Missing required subscribe method"),
					(None, Some(_)) => Err("Missing required unsubscribe method"),
					(None, None) => Err("Missing both subscribe and unsubscribe methods"),
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
	let mod_name = format!("rpc_impl_{}", name.to_string());
	syn::Ident::new(&mod_name, proc_macro2::Span::call_site())
}

pub fn rpc_impl(args: syn::AttributeArgs, input: syn::Item) -> Result<proc_macro2::TokenStream> {
	let rpc_trait = match input {
		syn::Item::Trait(item_trait) => item_trait,
		_ => return Err("rpc_api trait only works with trait declarations".to_owned())
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

			// todo: [AJ] sort all these out, see to_delegate method
			use self::_jsonrpc_core::futures as _futures;
			use self::_futures::{Future, IntoFuture};

			#rpc_trait
		}
		pub use self::#mod_name_ident::#name;
	})
}
