use std::collections::HashSet;

use quote::quote;
use syn::{
	parse_quote, Token, punctuated::Punctuated,
	fold::{self, Fold}, visit::{self, Visit},
};
use rpc_attr::{RpcTraitAttribute, RpcMethodAttribute};

type Result<T> = std::result::Result<T, String>;

fn ident(s: &str) -> syn::Ident {
	syn::Ident::new(s, proc_macro2::Span::call_site())
}

enum RpcTraitMethods {
	Standard(Vec<RpcMethod>),
	PubSub {
		name: String,
		subscribe: RpcMethod,
		unsubscribe: RpcMethod,
	}
}

impl RpcTraitMethods {
	fn generate_to_delegate_method(
		&self,
		trait_item: &syn::ItemTrait,
		has_metadata: bool,
	) -> Result<syn::TraitItemMethod> {
		let delegate_registration =
			match self {
				RpcTraitMethods::Standard(methods) => {
					let add_methods: Vec<_> = methods
						.iter()
						.map(RpcMethod::generate_delegate_method_registration)
						.collect();
					quote! { #(#add_methods)* }
				},
				RpcTraitMethods::PubSub { name, subscribe, unsubscribe } => {
					let mod_name = rpc_wrapper_mod_name(&trait_item);

					let sub_name = &subscribe.attr.name;
					let sub_method = &subscribe.sig.ident;
					let sub_aliases = subscribe.add_aliases();
					let sub_arg_types = &subscribe.get_method_arg_types();

					let unsub_name = &unsubscribe.attr.name;
					let unsub_method = &unsubscribe.sig.ident;
					let unsub_aliases = unsubscribe.add_aliases();

					quote! {
						del.add_subscription(
							#name,
							(#sub_name, move |base, params, meta, subscriber| {
								_jsonrpc_macros::WrapSubscribe::wrap_rpc(
									&(Self::#sub_method as fn(&_ #(, #sub_arg_types)*)),
									base,
									params,
									meta,
									subscriber,
								)
							}),
							(#unsub_name, move |base, id| {
								use #mod_name::_jsonrpc_core::futures::{IntoFuture, Future};
								Self::#unsub_method(base, id).into_future()
									.map(_jsonrpc_macros::to_value)
								.map_err(Into::into)
							}),
						);
						#sub_aliases
						#unsub_aliases
					}
				},
			};

		let to_delegate_body =
			quote! {
				let mut del = _jsonrpc_macros::IoDelegate::new(self.into());
				#delegate_registration
				del
			};

		// todo: [AJ] check that pubsub has metadata
		let method: syn::TraitItemMethod =
			if has_metadata {
				parse_quote! {
					fn to_delegate(self) -> _jsonrpc_macros::IoDelegate<Self, Self::Metadata>
					{
						#to_delegate_body
					}
				}
			} else {
				parse_quote! {
					fn to_delegate<M: _jsonrpc_core::Metadata>(self) -> _jsonrpc_macros::IoDelegate<Self, M>
					{
						#to_delegate_body
					}
				}
			};

		let predicates = generate_where_clause_serialization_predicates(&trait_item);

		let mut method = method.clone();
		method.sig.decl.generics
			.make_where_clause()
			.predicates
			.extend(predicates);
		Ok(method)
	}
}

#[derive(Clone, Debug)]
struct RpcMethod {
	attr: RpcMethodAttribute,
	sig: syn::MethodSig,
}

impl RpcMethod {
	fn try_from_trait_item_method(trait_item: &syn::TraitItemMethod) -> Result<Option<RpcMethod>> {
		let attr = RpcMethodAttribute::try_from_trait_item_method(trait_item);

		attr.map(|a| a.map(|a|
			RpcMethod { attr: a, sig: trait_item.sig.clone() }
		))
	}

	fn generate_delegate_method_registration(&self) -> proc_macro2::TokenStream {
		let rpc_name = &self.attr.name;
		let method = &self.sig.ident;
		let arg_types = self.get_method_arg_types();
		let result = match self.sig.decl.output {
			// todo: [AJ] require Result type?
			syn::ReturnType::Type(_, ref output) => output,
			syn::ReturnType::Default => panic!("Return type required for RPC method signature")
		};
		let add_aliases = self.add_aliases();
		let add_method =
			if self.attr.has_metadata {
				quote! {
					del.add_method_with_meta(#rpc_name, move |base, params, meta| {
						_jsonrpc_macros::WrapMeta::wrap_rpc(
							&(Self::#method as fn(&_ #(, #arg_types)*) -> #result),
							base,
							params,
							meta
						)
					});
				}
			} else {
				quote! {
					del.add_method(#rpc_name, move |base, params| {
						_jsonrpc_macros::WrapAsync::wrap_rpc(
							&(Self::#method as fn(&_ #(, #arg_types)*) -> #result),
							base,
							params
						)
					});
				}
			};
		quote! {
			#add_method
			#add_aliases
		}
	}

	fn add_aliases(&self) -> proc_macro2::TokenStream {
		let name = &self.attr.name;
		let add_aliases: Vec<_> = self.attr.aliases
			.iter()
			.map(|alias| quote! { del.add_alias(#alias, #name); })
			.collect();
		quote!{ #(#add_aliases)* }
	}

	fn get_method_arg_types(&self) -> Vec<&syn::Type> {
		self.sig.decl.inputs
			.iter()
			.filter_map(|arg| {
				match arg {
					syn::FnArg::Captured(arg_captured) => Some(&arg_captured.ty),
					syn::FnArg::Ignored(ty) => Some(&ty),
					_ => None,
				}
			})
			.collect()
	}
}

struct RpcTrait {
	attr: RpcTraitAttribute,
	methods: Vec<RpcMethod>,
	has_metadata: bool,
}

impl<'a> Fold for RpcTrait {
	fn fold_trait_item_method(&mut self, method: syn::TraitItemMethod) -> syn::TraitItemMethod {
		let mut method = method.clone();
		match RpcMethod::try_from_trait_item_method(&method) {
			Ok(Some(rpc_method)) => {
				self.methods.push(rpc_method.clone());
				// remove the rpc attribute
				method.attrs.retain(|a| *a != rpc_method.attr.attr);
			},
			Ok(None) => (), // non rpc annotated trait method
			Err(err) => panic!("Invalid rpc method attribute {}", err)
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

fn generate_where_clause_serialization_predicates(item_trait: &syn::ItemTrait) -> Vec<syn::WherePredicate> {
	struct FindTyParams {
		trait_generics: HashSet<syn::Ident>,
		serialize_type_params: HashSet<syn::Ident>,
		deserialize_type_params: HashSet<syn::Ident>,
		visiting_return_type: bool,
		visiting_fn_arg: bool,
	}
	impl<'ast> Visit<'ast> for FindTyParams {
		fn visit_type_param(&mut self, ty_param: &'ast syn::TypeParam) {
			self.trait_generics.insert(ty_param.ident.clone());
		}

		fn visit_return_type(&mut self, return_type: &'ast syn::ReturnType) {
			self.visiting_return_type = true;
			visit::visit_return_type(self, return_type);
			self.visiting_return_type = false
		}

		fn visit_path_segment(&mut self, segment: &'ast syn::PathSegment) {
			if self.visiting_return_type && self.trait_generics.contains(&segment.ident) {
				self.serialize_type_params.insert(segment.ident.clone());
			}
			if self.visiting_fn_arg && self.trait_generics.contains(&segment.ident) {
				self.deserialize_type_params.insert(segment.ident.clone());
			}
			visit::visit_path_segment(self, segment)
		}

		fn visit_fn_arg(&mut self, arg: &'ast syn::FnArg) {
			self.visiting_fn_arg = true;
			visit::visit_fn_arg(self, arg);
			self.visiting_fn_arg = false;
		}
	}
	let mut visitor = FindTyParams {
		visiting_return_type: false,
		visiting_fn_arg: false,
		trait_generics: HashSet::new(),
		serialize_type_params: HashSet::new(),
		deserialize_type_params: HashSet::new(),
	};
	visitor.visit_item_trait(item_trait);

	item_trait.generics
		.type_params()
		.map(|ty| {
			let ty_path = syn::TypePath { qself: None, path: ty.ident.clone().into() };
			let mut bounds: Punctuated<syn::TypeParamBound, Token![+]> =
				parse_quote!(Send + Sync + 'static);
			// add json serialization trait bounds
			if visitor.serialize_type_params.contains(&ty.ident) {
				bounds.push(parse_quote!(_serde::Serialize))
			}
			if visitor.deserialize_type_params.contains(&ty.ident) {
				bounds.push(parse_quote!(_serde::de::DeserializeOwned))
			}
			syn::WherePredicate::Type(syn::PredicateType {
				lifetimes: None,
				bounded_ty: syn::Type::Path(ty_path),
				colon_token: <Token![:]>::default(),
				bounds,
			})
		})
		.collect()
}

fn generate_rpc_item_trait(attr_args: &syn::AttributeArgs, item_trait: &syn::ItemTrait) -> Result<syn::ItemTrait> {
	let trait_attr = RpcTraitAttribute::try_from_trait_attribute(&attr_args)?;
	let mut visitor = RpcTrait { attr: trait_attr, methods: Vec::new(), has_metadata: false };

	// first visit the trait to collect the methods
	let mut item_trait = fold::fold_item_trait(&mut visitor, item_trait.clone());
	let rpc_trait_methods =
		match visitor.attr {
			RpcTraitAttribute::RpcTrait => {
				if !visitor.methods.is_empty() {
					Ok(RpcTraitMethods::Standard(visitor.methods.clone()))
				} else {
					Err("No rpc annotated trait items found")
				}
			},
			RpcTraitAttribute::PubSubTrait { name } => {
				let method_name_contains = |m: &RpcMethod, s: &str| {
					m.sig.ident.to_string().contains(s)
				};
				let subscribe = visitor.methods
					.iter()
					.find(|m| method_name_contains(m, "subscribe") && !method_name_contains(*m, "unsubscribe"));
				let unsubscribe = visitor.methods
					.iter()
					.find(|m| method_name_contains(*m, "unsubscribe"));

				match (subscribe, unsubscribe) {
					(Some(sub), Some(unsub)) => {
						// todo: [AJ] validate subscribe/unsubscribe args
//						let sub_arg_types = sub.get_method_arg_types();

						Ok(RpcTraitMethods::PubSub {
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
		rpc_trait_methods.generate_to_delegate_method(&item_trait, visitor.has_metadata)?;
	item_trait.items.push(syn::TraitItem::Method(to_delegate_method));

	let trait_bounds: Punctuated<syn::TypeParamBound, Token![+]> =
		parse_quote!(Sized + Send + Sync + 'static);
	item_trait.supertraits.extend(trait_bounds);

	Ok(item_trait)
}

fn rpc_wrapper_mod_name(rpc_trait: &syn::ItemTrait) -> syn::Ident {
	let name = rpc_trait.ident.clone();
	let mod_name = format!("rpc_impl_{}", name.to_string());
	ident(&mod_name)
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
			extern crate jsonrpc_macros as _jsonrpc_macros; // todo: [AJ] remove/replace
			extern crate serde as _serde;
			use super::*;

			#rpc_trait
		}
		pub use self::#mod_name_ident::#name;
	})
}
