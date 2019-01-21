use std::collections::HashSet;

use quote::quote;
use syn::{
	parse_quote, Token, punctuated::Punctuated,
	visit::{self, Visit},
};
use crate::rpc_attr::RpcMethodAttribute;

pub enum MethodRegistration {
	Standard {
		method: RpcMethod,
		has_metadata: bool
	},
	PubSub {
		name: String,
		subscribe: RpcMethod,
		unsubscribe: RpcMethod,
	}
}

impl MethodRegistration {
	fn generate(&self) -> proc_macro2::TokenStream {
		match self {
			MethodRegistration::Standard { method, has_metadata } => {
				let rpc_name = &method.name();
				let add_method =
					if *has_metadata {
						quote!(add_method_with_meta)
					} else {
						quote!(add_method)
					};
				let closure = method.generate_delegate_closure(false);
				let add_aliases = method.generate_add_aliases();

				quote! {
					del.#add_method(#rpc_name, #closure);
					#add_aliases
				}
			},
			MethodRegistration::PubSub { name, subscribe, unsubscribe } => {
				let sub_name = subscribe.name();
				let sub_closure = subscribe.generate_delegate_closure(true);
				let sub_aliases = subscribe.generate_add_aliases();

				let unsub_name = unsubscribe.name();
				let unsub_method_ident = unsubscribe.ident();
				let unsub_closure =
					quote! {
						move |base, id, meta| {
							use self::_futures::{Future, IntoFuture};
							Self::#unsub_method_ident(base, meta, id).into_future()
								.map(|value| _jsonrpc_core::to_value(value)
										.expect("Expected always-serializable type; qed"))
								.map_err(Into::into)
						}
					};
				let unsub_aliases = unsubscribe.generate_add_aliases();

				quote! {
					del.add_subscription(
						#name,
						(#sub_name, #sub_closure),
						(#unsub_name, #unsub_closure),
					);
					#sub_aliases
					#unsub_aliases
				}
			},
		}
	}
}

const SUBCRIBER_TYPE_IDENT: &'static str = "Subscriber";
const METADATA_CLOSURE_ARG: &'static str = "meta";
const SUBSCRIBER_CLOSURE_ARG: &'static str = "subscriber";

pub fn generate_trait_item_method(
	methods: &[MethodRegistration],
	trait_item: &syn::ItemTrait,
	has_metadata: bool,
	has_pubsub_methods: bool,
) -> syn::TraitItemMethod {
	let io_delegate_type =
		if has_pubsub_methods {
			quote!(_jsonrpc_pubsub::IoDelegate)
		} else {
			quote!(_jsonrpc_core::IoDelegate)
		};
	let add_methods: Vec<_> = methods
		.iter()
		.map(MethodRegistration::generate)
		.collect();
	let to_delegate_body =
		quote! {
			let mut del = #io_delegate_type::new(self.into());
			#(#add_methods)*
			del
		};

	// todo: [AJ] check that pubsub has metadata
	let method: syn::TraitItemMethod =
		if has_metadata {
			parse_quote! {
				fn to_delegate(self) -> #io_delegate_type<Self, Self::Metadata> {
					#to_delegate_body
				}
			}
		} else {
			parse_quote! {
				fn to_delegate<M: _jsonrpc_core::Metadata>(self) -> #io_delegate_type<Self, M> {
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

#[derive(Clone)]
pub struct RpcMethod {
	pub attr: RpcMethodAttribute,
	pub trait_item: syn::TraitItemMethod,
}

impl RpcMethod {
	pub fn new(attr: RpcMethodAttribute, trait_item: syn::TraitItemMethod) -> RpcMethod {
		RpcMethod { attr, trait_item }
	}

	pub fn attr(&self) -> &RpcMethodAttribute { &self.attr }

	pub fn name(&self) -> &str {
		&self.attr.name
	}

	pub fn ident(&self) -> &syn::Ident {
		&self.trait_item.sig.ident
	}

	pub fn is_pubsub(&self) -> bool {
		self.attr.is_pubsub()
	}

	fn generate_delegate_closure(&self, is_subscribe: bool) -> proc_macro2::TokenStream {
		let mut param_types: Vec<_> =
			self.trait_item.sig.decl.inputs
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

		// special args are those which are not passed directly via rpc params: metadata, subscriber
		let special_args = Self::special_args(&param_types);
		param_types.retain(|ty|
			special_args.iter().find(|(_,sty)| sty == ty).is_none());

		let tuple_fields : &Vec<_> =
			&(0..param_types.len() as u8)
				.map(|x| ident(&((x + 'a' as u8) as char).to_string()))
				.collect();
		let param_types = &param_types;
		let parse_params =
			// if the last argument is an `Option` then it can be made an optional 'trailing' argument
			if let Some(ref trailing) = param_types.iter().last().and_then(try_get_option) {
				self.params_with_trailing(trailing, param_types, tuple_fields)
			} else if param_types.is_empty() {
				quote! { let params = params.expect_no_params(); }
			} else {
				quote! { let params = params.parse::<(#(#param_types), *)>(); }
			};

		let method_ident = self.ident();
		let result = &self.trait_item.sig.decl.output;
		let extra_closure_args: &Vec<_> = &special_args.iter().cloned().map(|arg| arg.0).collect();
		let extra_method_types: &Vec<_> = &special_args.iter().cloned().map(|arg| arg.1).collect();

		let closure_args = quote! { base, params, #(#extra_closure_args), * };
		let method_sig = quote! { fn(&Self, #(#extra_method_types, ) * #(#param_types), *) #result };
		let method_call = quote! { (base, #(#extra_closure_args, )* #(#tuple_fields), *) };
		let match_params =
			if is_subscribe {
				quote! {
					Ok((#(#tuple_fields), *)) => {
						let subscriber = _jsonrpc_pubsub::typed::Subscriber::new(subscriber);
						(method)#method_call
					},
					Err(e) => {
						let _ = subscriber.reject(e);
						return
					}
				}
			} else {
				quote! {
					Ok((#(#tuple_fields), *)) => {
						use self::_futures::{Future, IntoFuture};
						let fut = (method)#method_call
							.into_future()
							.map(|value| _jsonrpc_core::to_value(value)
								.expect("Expected always-serializable type; qed"))
							.map_err(Into::into as fn(_) -> _jsonrpc_core::Error);
						_futures::future::Either::A(fut)
					},
					Err(e) => _futures::future::Either::B(_futures::failed(e)),
				}
			};

		quote! {
			move |#closure_args| {
				let method = &(Self::#method_ident as #method_sig);
				#parse_params
				match params {
					#match_params
				}
			}
		}
	}

	fn special_args(param_types: &[syn::Type]) -> Vec<(syn::Ident, syn::Type)> {
		let meta_arg =
			param_types.first().and_then(|ty|
				if *ty == parse_quote!(Self::Metadata) { Some(ty.clone()) } else { None });
		let subscriber_arg = param_types.iter().nth(1).and_then(|ty| {
			if let syn::Type::Path(path) = ty {
				if path.path.segments.iter().any(|s| s.ident == SUBCRIBER_TYPE_IDENT) {
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
			special_args.push((ident(METADATA_CLOSURE_ARG), meta.clone()));
		}
		if let Some(ref subscriber) = subscriber_arg {
			special_args.push((ident(SUBSCRIBER_CLOSURE_ARG), subscriber.clone()));
		}
		special_args
	}

	fn params_with_trailing(
		&self,
		trailing: &syn::Type,
		param_types: &[syn::Type],
		tuple_fields: &[syn::Ident],
	) -> proc_macro2::TokenStream {
		let param_types_no_trailing: Vec<_> =
			param_types.iter().cloned().filter(|arg| arg != trailing).collect();
		let tuple_fields_no_trailing: &Vec<_> =
			&tuple_fields.iter().take(tuple_fields.len() - 1).collect();
		let num = param_types_no_trailing.len();
		quote! {
			let params_len = match params {
				_jsonrpc_core::Params::Array(ref v) => Ok(v.len()),
				_jsonrpc_core::Params::None => Ok(0),
				_ => Err(_jsonrpc_core::Error::invalid_params("`params` should be an array"))
			};

			let params = params_len.and_then(|len| {
				match len.checked_sub(#num) {
					Some(0) => params.parse::<(#(#param_types_no_trailing), *)>()
						.map( |(#(#tuple_fields_no_trailing), *)|
							(#(#tuple_fields_no_trailing, )* None))
						.map_err(Into::into),
					Some(1) => params.parse::<(#(#param_types), *) > ()
						.map( |(#(#tuple_fields_no_trailing, )* id)|
							(#(#tuple_fields_no_trailing, )* id))
						.map_err(Into::into),
					None => Err(_jsonrpc_core::Error::invalid_params(
						format!("`params` should have at least {} argument(s)", #num))),
					_ => Err(_jsonrpc_core::Error::invalid_params_with_details(
						format!("Expected {} or {} parameters.", #num, #num + 1),
						format!("Got: {}", len))),
				}
			});
		}
	}

	fn generate_add_aliases(&self) -> proc_macro2::TokenStream {
		let name = self.name();
		let add_aliases: Vec<_> = self.attr.aliases
			.iter()
			.map(|alias| quote! { del.add_alias(#alias, #name); })
			.collect();
		quote!{ #(#add_aliases)* }
	}
}

fn ident(s: &str) -> syn::Ident {
	syn::Ident::new(s, proc_macro2::Span::call_site())
}

fn try_get_option(ty: &syn::Type) -> Option<syn::Type> {
	if let syn::Type::Path(path) = ty {
		path.path.segments
			.first()
			.and_then(|t| {
				if t.value().ident == "Option" { Some(ty.clone()) } else { None }
			})
	} else {
		None
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
