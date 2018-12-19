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
					let sub_arg_types = &subscribe.arg_types;

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
	arg_types: Vec<syn::Type>,
	trailing_arg: Option<syn::Type>,
	return_type: syn::Type,
}

impl RpcMethod {
	fn try_from_trait_item_method(trait_item: &syn::TraitItemMethod) -> Result<Option<RpcMethod>> {
		let attr = RpcMethodAttribute::try_from_trait_item_method(trait_item);

		let mut arg_types: Vec<_> =
			trait_item.sig.decl.inputs
				.iter()
				.cloned()
				.filter_map(|arg| {
					match arg {
						syn::FnArg::Captured(arg_captured) => Some(arg_captured.ty),
						syn::FnArg::Ignored(ty) => Some(ty),
						_ => None,
					}
				})
				.collect();

		// if the last argument is an `Option` then it can be made a 'trailing' argument
		let trailing_arg =
			arg_types
				.iter()
				.last()
				.and_then(|arg| {
					if let syn::Type::Path(path) = arg {
						path.path.segments
							.first()
							.and_then(|t| {
								if t.value().ident == "Option" { Some(arg) } else { None }
							})
					} else {
						None
					}
				})
				.collect();

		println!("Trailing {:?}", trailing_args);

		let return_type = match trait_item.sig.decl.output {
			// todo: [AJ] require Result type?
			syn::ReturnType::Type(_, ref output) => Ok(*output.clone()),
			syn::ReturnType::Default => Err("Return type required for RPC method signature".to_string())
		}?;

		attr.and_then(|attr|
			match attr {
				Some(attr) => {
					if attr.has_metadata {
						let metadata: &syn::Type = &parse_quote!(Self::Metadata);
						if Some(metadata) != arg_types.get(0) {
							return Err("Method with metadata expected Self::Metadata argument after self".into())
						}
						// remove Self::Metadata arg, it will be added again in the output
						arg_types.retain(|arg| arg != metadata);
					}
					let sig = trait_item.sig.clone();
					Ok(Some(RpcMethod { attr, sig, arg_types, trailing_args, return_type }))
				},
				None => Ok(None)
			})
	}

	fn generate_delegate_method_registration(&self) -> proc_macro2::TokenStream {
		let rpc_name = &self.attr.name;
		let method = &self.sig.ident;
		let arg_types = &self.arg_types;
		let result = &self.return_type;

		let tuple_fields : &Vec<_> =
			&(0..arg_types.len() as u8)
				.map(|x| ident(&((x + 'a' as u8) as char).to_string()))
				.collect();

		let add_aliases = self.add_aliases();

		let (add_method, closure_args, method_sig, method_call) =
			if self.attr.has_metadata {
				(
					quote! { add_method_with_meta },
					quote! { base, params, meta },
					quote! { fn(&Self, Self::Metadata, #(#arg_types), *) },
					quote! { (base, meta, #(#tuple_fields), *) }
				)
			} else {
				(
					quote! { add_method },
					quote! { base, params },
					quote! { fn(&Self, #(#arg_types), *) },
					quote! { (base, #(#tuple_fields), *) }
				)
			};

		let params_method =
			if !self.arg_types.is_empty() {
				let params =
					if let Some(trailing) = self.trailing_arg {
						quote! {
//							let len = match require_len(&params, $num) {
//								Ok(len) => len,
//								Err(e) => return Either::B(futures::failed(e)),
//							};
//
//							let params = match len - $num {
//								0 => params.parse::<($($x,)+)>()
//									.map(|($($x,)+)| ($($x,)+ None)).map_err(Into::into),
//								1 => params.parse::<($($x,)+ TRAILING)>()
//									.map(|($($x,)+ id)| ($($x,)+ Some(id))).map_err(Into::into),
//								_ => Err(invalid_params(&format!("Expected {} or {} parameters.", $num, $num + 1), format!("Got: {}", len))),
//							};
//
//							match params {
//								Ok(($($x,)+ id)) => Either::A(as_future((self)(base, meta, $($x,)+ Trailing(id)))),
//								Err(e) => Either::B(futures::failed(e)),
//							}
						}
					} else {
						quote! {
							let params = params.parse::<(#(#arg_types), *)>();
						}
					};
			} else {
				quote! { let params = params.expect_no_params(); }
			};

		let add_method =
			quote! {
				del.#add_method(#rpc_name,
					move |#closure_args| {
						let method = &(Self::#method as #method_sig -> #result);
						match params {
							Ok((#(#tuple_fields), *)) => Either::A(as_future((method)#method_call)),
							Err(e) => Either::B(futures::failed(e)),
						}
					}
				);
			};
		quote! {
			#add_method
//			#add_aliases
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
}

struct RpcTrait {
	attr: RpcTraitAttribute,
	methods: Vec<RpcMethod>,
	has_metadata: bool,
	errors: Vec<String>,
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

			// todo: [AJ] sort all these out, see to_delegate method
			use std::sync::Arc;
			use std::collections::HashMap;
			use self::_jsonrpc_core::futures::future::{self, Either};
			use self::_jsonrpc_core::futures::{self, Future, IntoFuture};

			type WrappedFuture<F, OUT, E> = future::MapErr<
				future::Map<F, fn(OUT) -> _jsonrpc_core::Value>,
				fn(E) -> Error
			>;
			type WrapResult<F, OUT, E> = Either<
				WrappedFuture<F, OUT, E>,
				future::FutureResult<_jsonrpc_core::Value, _jsonrpc_core::Error>,
			>;

			fn to_value<T>(value: T) -> _jsonrpc_core::Value where T: _serde::Serialize {
				_jsonrpc_core::to_value(value).expect("Expected always-serializable type.")
			}

			fn as_future<F, OUT, E, I>(el: I) -> WrappedFuture<F, OUT, E> where
				OUT: _serde::Serialize,
				E: Into<_jsonrpc_core::Error>,
				F: Future<Item = OUT, Error = E>,
				I: IntoFuture<Item = OUT, Error = E, Future = F>
			{
				el.into_future()
					.map(to_value as fn(OUT) -> _jsonrpc_core::Value)
					.map_err(Into::into as fn(E) -> _jsonrpc_core::Error)
			}

			#rpc_trait
		}
		pub use self::#mod_name_ident::#name;
	})
}
