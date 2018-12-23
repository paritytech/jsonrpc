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

enum ToDelegateFunction {
	Standard(Vec<RpcMethod>),
	PubSub {
		name: String,
		subscribe: RpcMethod,
		unsubscribe: RpcMethod,
	}
}

impl ToDelegateFunction {
	fn register_rpc_method(method: &RpcMethod) -> proc_macro2::TokenStream {
		let rpc_name = &method.attr.name;

		let add_method =
			if method.attr.has_metadata {
				ident("add_method_with_meta")
			} else {
				ident("add_method")
			};

		let closure = method.generate_delegate_closure();
		let add_aliases = method.add_aliases();

		quote! {
			del.#add_method(#rpc_name, #closure);
			#add_aliases
		}
	}

	fn register_pubsub_methods(
		name: &str,
		subscribe: &RpcMethod,
		unsubscribe: &RpcMethod,
	) -> proc_macro2::TokenStream {
		let sub_name = &subscribe.attr.name;
		let sub_closure = subscribe.generate_delegate_closure();
		let sub_aliases = subscribe.add_aliases();

		let unsub_name = &unsubscribe.attr.name;
		let unsub_closure = unsubscribe.generate_delegate_closure();
		let unsub_aliases = unsubscribe.add_aliases();

		quote! {
			del.add_subscription(
				#name,
				(#sub_name, #sub_closure),
				(#unsub_name, #unsub_closure),
			);
			#sub_aliases
			#unsub_aliases
		}
	}

	fn quote(
		&self,
		trait_item: &syn::ItemTrait,
		has_metadata: bool,
	) -> syn::TraitItemMethod {
		let delegate_registration =
			match self {
				ToDelegateFunction::Standard(methods) => {
					let add_methods: Vec<_> = methods
						.iter()
						.map(Self::register_rpc_method)
						.collect();
					quote! { #(#add_methods)* }
				},
				ToDelegateFunction::PubSub { name, subscribe, unsubscribe } => {
					Self::register_pubsub_methods(name, subscribe, unsubscribe)
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
		method
	}
}

#[derive(Clone, Debug)]
struct RpcMethod {
	attr: RpcMethodAttribute,
	sig: syn::MethodSig,
	special_args: Vec<(syn::Ident, syn::Type)>,
	rpc_param_types: Vec<syn::Type>,
	meta_arg: Option<syn::Type>,
	subscriber_arg: Option<syn::Type>,
	trailing_arg: Option<syn::Type>,
	return_type: syn::Type,
}

impl RpcMethod {
	fn try_from_trait_item_method(trait_item: &syn::TraitItemMethod) -> Result<Option<RpcMethod>> {
		let attr = RpcMethodAttribute::try_from_trait_item_method(trait_item);

		let mut rpc_param_types: Vec<_> =
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

		let meta_arg =
			rpc_param_types.first().and_then(|ty|
				if *ty == parse_quote!(Self::Metadata) { Some(ty.clone()) } else { None });

		let subscriber_arg = rpc_param_types.iter().nth(1).and_then(|ty| {
			if let syn::Type::Path(path) = ty {
				if path.path.segments.iter().any(|s| s.ident == "Subscriber") {
					Some(ty.clone())
				} else {
					None
				}
			} else {
				None
			}
		});

		let mut special_args = Vec::new();
		if let Some(ref meta) = meta_arg {
			rpc_param_types.retain(|ty| ty != meta );
			special_args.push((ident("meta"), meta.clone()));
		}
		if let Some(ref subscriber) = subscriber_arg {
			rpc_param_types.retain(|ty| ty != subscriber);
			special_args.push((ident("subscriber"), subscriber.clone()));
		}

		let is_option = |arg: &syn::Type| {
			if let syn::Type::Path(path) = arg {
				path.path.segments
					.first()
					.and_then(|t| {
						if t.value().ident == "Option" { Some(arg.clone()) } else { None }
					})
			} else {
				None
			}
		};

		// if the last argument is an `Option` then it can be made an optional 'trailing' argument
		let trailing_arg = rpc_param_types.iter().last().and_then(is_option);

		let return_type = match trait_item.sig.decl.output {
			// todo: [AJ] require Result type?
			syn::ReturnType::Type(_, ref output) => Ok(*output.clone()),
			syn::ReturnType::Default => Err("Return type required for RPC method signature".to_string())
		}?;

		attr.and_then(|attr|
			match attr {
				Some(attr) => {
//					if attr.has_metadata {
//						let metadata: &syn::Type = &parse_quote!(Self::Metadata);
//						if Some(metadata) != arg_types.get(0) {
//							return Err("Method with metadata expected Self::Metadata argument after self".into())
//						}
//						special_args.push((ident("meta"), metadata));
//						// remove Self::Metadata arg
//						let mut rpc_param_types = arg_types;
//						rpc_param_types.retain(|arg| arg != metadata);
//					}
					let sig = trait_item.sig.clone();
					Ok(Some(RpcMethod {
						attr,
						sig,
						special_args,
						meta_arg,
						subscriber_arg,
						rpc_param_types,
						trailing_arg,
						return_type,
					}))
				},
				None => Ok(None)
			})
	}

	fn generate_delegate_closure(&self) -> proc_macro2::TokenStream {
		let method = &self.sig.ident;
		let arg_types = &self.rpc_param_types;
		let num = &self.rpc_param_types.len();
		let result = &self.return_type;

		let tuple_fields : &Vec<_> =
			&(0..arg_types.len() as u8)
				.map(|x| ident(&((x + 'a' as u8) as char).to_string()))
				.collect();

		let parse_params =
			if let Some(ref trailing) = self.trailing_arg {
				let arg_types_no_trailing: &Vec<_> =
					&arg_types.iter().filter(|arg| *arg != trailing).collect();
				let tuple_fields_no_trailing: &Vec<_> =
					&tuple_fields.iter().take(tuple_fields.len() - 1).collect();
				quote! {
					let params_len = match params {
						_jsonrpc_core::Params::Array(ref v) => Ok(v.len()),
						_jsonrpc_core::Params::None => Ok(0),
						_ => Err(Error::invalid_params("`params` should be an array"))
					};

					let params = params_len.and_then(|len| {
						match len - #num {
							0 => params.parse::<(#(#arg_types_no_trailing), *)>()
								.map( |(#(#tuple_fields_no_trailing), *)| (#(#tuple_fields_no_trailing, )* None)).map_err(Into::into),
							1 => params.parse::<(#(#arg_types), *) > ()
								.map( |(#(#tuple_fields_no_trailing, )* id)| (#(#tuple_fields_no_trailing, )* id)).map_err(Into::into),
							x if x < 0 => Err(Error::invalid_params(format!("`params` should have at least {} argument(s)", #num))),
							_ => Err(Error::invalid_params_with_details(format!("Expected {} or {} parameters.", #num, #num + 1), format!("Got: {}", len))),
						}
					});
				}
			} else if arg_types.is_empty() {
				quote! { let params = params.expect_no_params(); }
			} else {
				quote! { let params = params.parse::<(#(#arg_types), *)>(); }
			};

		let extra_closure_args: &Vec<_> = &self.special_args.iter().cloned().map(|arg| arg.0).collect();
		let extra_method_types: &Vec<_> = &self.special_args.iter().cloned().map(|arg| arg.1).collect();

		let closure_args = quote! { base, params, #(#extra_closure_args), * };
		let method_sig = quote! { fn(&Self, #(#extra_method_types, ) * #(#arg_types), *) };
		let method_call = quote! { (base, #(#extra_closure_args, )* #(#tuple_fields), *) };

		quote! {
			move |#closure_args| {
				let method = &(Self::#method as #method_sig -> #result);
				#parse_params
				match params {
					Ok((#(#tuple_fields), *)) => {
						let fut = (method)#method_call
							.into_future()
							.map(|value| _jsonrpc_core::to_value(value).expect("Expected always-serializable type; qed"))
							.map_err(Into::into as fn(_) -> _jsonrpc_core::Error);
						_jsonrpc_core::futures::future::Either::A(fut)
					},
					Err(e) => _jsonrpc_core::futures::future::Either::B(futures::failed(e)),
				}
			}
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

	let to_delegate =
		match visitor.attr {
			RpcTraitAttribute::RpcTrait => {
				if !visitor.methods.is_empty() {
					Ok(ToDelegateFunction::Standard(visitor.methods.clone()))
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
		to_delegate.quote(&item_trait, visitor.has_metadata);
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
			use self::_jsonrpc_core::futures::{Future, IntoFuture};

			#rpc_trait
		}
		pub use self::#mod_name_ident::#name;
	})
}
