use std::collections::HashSet;

use crate::params_style::ParamStyle;
use crate::rpc_attr::RpcMethodAttribute;
use quote::quote;
use syn::{
	parse_quote,
	punctuated::Punctuated,
	visit::{self, Visit},
	Result, Token,
};

pub enum MethodRegistration {
	Standard {
		method: RpcMethod,
		has_metadata: bool,
	},
	PubSub {
		name: String,
		subscribes: Vec<RpcMethod>,
		unsubscribe: RpcMethod,
	},
	Notification {
		method: RpcMethod,
		has_metadata: bool,
	},
}

impl MethodRegistration {
	fn generate(&self) -> Result<proc_macro2::TokenStream> {
		match self {
			MethodRegistration::Standard { method, has_metadata } => {
				let rpc_name = &method.name();
				let add_method = if *has_metadata {
					quote!(add_method_with_meta)
				} else {
					quote!(add_method)
				};
				let closure = method.generate_delegate_closure(false)?;
				let add_aliases = method.generate_add_aliases();

				Ok(quote! {
					del.#add_method(#rpc_name, #closure);
					#add_aliases
				})
			}
			MethodRegistration::PubSub {
				name,
				subscribes,
				unsubscribe,
			} => {
				let unsub_name = unsubscribe.name();
				let unsub_method_ident = unsubscribe.ident();
				let unsub_closure = quote! {
					move |base, id, meta| {
						use self::_futures::{FutureExt, TryFutureExt};
						self::_jsonrpc_core::WrapFuture::into_future(
							Self::#unsub_method_ident(base, meta, id)
						)
							.map_ok(|value| _jsonrpc_core::to_value(value)
									.expect("Expected always-serializable type; qed"))
							.map_err(Into::into)
					}
				};

				let mut add_subscriptions = proc_macro2::TokenStream::new();

				for subscribe in subscribes.iter() {
					let sub_name = subscribe.name();
					let sub_closure = subscribe.generate_delegate_closure(true)?;
					let sub_aliases = subscribe.generate_add_aliases();

					add_subscriptions = quote! {
						#add_subscriptions
						del.add_subscription(
							#name,
							(#sub_name, #sub_closure),
							(#unsub_name, #unsub_closure),
						);
						#sub_aliases
					};
				}

				let unsub_aliases = unsubscribe.generate_add_aliases();

				Ok(quote! {
					#add_subscriptions
					#unsub_aliases
				})
			}
			MethodRegistration::Notification { method, has_metadata } => {
				let name = &method.name();
				let add_notification = if *has_metadata {
					quote!(add_notification_with_meta)
				} else {
					quote!(add_notification)
				};
				let closure = method.generate_delegate_closure(false)?;
				let add_aliases = method.generate_add_aliases();

				Ok(quote! {
					del.#add_notification(#name, #closure);
					#add_aliases
				})
			}
		}
	}
}

const SUBSCRIBER_TYPE_IDENT: &str = "Subscriber";
const METADATA_CLOSURE_ARG: &str = "meta";
const SUBSCRIBER_CLOSURE_ARG: &str = "subscriber";

// tuples are limited to 16 fields: the maximum supported by `serde::Deserialize`
const TUPLE_FIELD_NAMES: [&str; 16] = [
	"a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
];

pub fn generate_trait_item_method(
	methods: &[MethodRegistration],
	trait_item: &syn::ItemTrait,
	has_metadata: bool,
	has_pubsub_methods: bool,
) -> Result<syn::TraitItemMethod> {
	let io_delegate_type = if has_pubsub_methods {
		quote!(_jsonrpc_pubsub::IoDelegate)
	} else {
		quote!(_jsonrpc_core::IoDelegate)
	};
	let add_methods = methods
		.iter()
		.map(MethodRegistration::generate)
		.collect::<Result<Vec<_>>>()?;
	let to_delegate_body = quote! {
		let mut del = #io_delegate_type::new(self.into());
		#(#add_methods)*
		del
	};

	let method: syn::TraitItemMethod = if has_metadata {
		parse_quote! {
			/// Create an `IoDelegate`, wiring rpc calls to the trait methods.
			fn to_delegate(self) -> #io_delegate_type<Self, Self::Metadata> {
				#to_delegate_body
			}
		}
	} else {
		parse_quote! {
			/// Create an `IoDelegate`, wiring rpc calls to the trait methods.
			fn to_delegate<M: _jsonrpc_core::Metadata>(self) -> #io_delegate_type<Self, M> {
				#to_delegate_body
			}
		}
	};

	let predicates = generate_where_clause_serialization_predicates(&trait_item, false);
	let mut method = method;
	method.sig.generics.make_where_clause().predicates.extend(predicates);
	Ok(method)
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

	pub fn attr(&self) -> &RpcMethodAttribute {
		&self.attr
	}

	pub fn name(&self) -> &str {
		&self.attr.name
	}

	pub fn ident(&self) -> &syn::Ident {
		&self.trait_item.sig.ident
	}

	pub fn is_pubsub(&self) -> bool {
		self.attr.is_pubsub()
	}

	pub fn subscriber_arg(&self) -> Option<syn::Type> {
		self.trait_item
			.sig
			.inputs
			.iter()
			.filter_map(|arg| match arg {
				syn::FnArg::Typed(ty) => Some(*ty.ty.clone()),
				_ => None,
			})
			.find(|ty| {
				if let syn::Type::Path(path) = ty {
					if path.path.segments.iter().any(|s| s.ident == SUBSCRIBER_TYPE_IDENT) {
						return true;
					}
				}
				false
			})
	}

	fn generate_delegate_closure(&self, is_subscribe: bool) -> Result<proc_macro2::TokenStream> {
		let mut param_types: Vec<_> = self
			.trait_item
			.sig
			.inputs
			.iter()
			.cloned()
			.filter_map(|arg| match arg {
				syn::FnArg::Typed(ty) => Some(*ty.ty),
				_ => None,
			})
			.collect();

		// special args are those which are not passed directly via rpc params: metadata, subscriber
		let special_args = Self::special_args(&param_types);
		param_types.retain(|ty| !special_args.iter().any(|(_, sty)| sty == ty));
		if param_types.len() > TUPLE_FIELD_NAMES.len() {
			return Err(syn::Error::new_spanned(
				&self.trait_item,
				&format!("Maximum supported number of params is {}", TUPLE_FIELD_NAMES.len()),
			));
		}
		let tuple_fields: &Vec<_> = &(TUPLE_FIELD_NAMES
			.iter()
			.take(param_types.len())
			.map(|name| ident(name))
			.collect());
		let param_types = &param_types;
		let parse_params = {
			// last arguments that are `Option`-s are optional 'trailing' arguments
			let trailing_args_num = param_types.iter().rev().take_while(|t| is_option_type(t)).count();

			if trailing_args_num != 0 {
				self.params_with_trailing(trailing_args_num, param_types, tuple_fields)
			} else if param_types.is_empty() {
				quote! { let params = params.expect_no_params(); }
			} else if self.attr.params_style == Some(ParamStyle::Raw) {
				quote! { let params: _jsonrpc_core::Result<_> = Ok((params,)); }
			} else if self.attr.params_style == Some(ParamStyle::Positional) {
				quote! { let params = params.parse::<(#(#param_types, )*)>(); }
			} else {
				unimplemented!("Server side named parameters are not implemented");
			}
		};

		let method_ident = self.ident();
		let result = &self.trait_item.sig.output;
		let extra_closure_args: &Vec<_> = &special_args.iter().cloned().map(|arg| arg.0).collect();
		let extra_method_types: &Vec<_> = &special_args.iter().cloned().map(|arg| arg.1).collect();

		let closure_args = quote! { base, params, #(#extra_closure_args), * };
		let method_sig = quote! { fn(&Self, #(#extra_method_types, ) * #(#param_types), *) #result };
		let method_call = quote! { (base, #(#extra_closure_args, )* #(#tuple_fields), *) };
		let match_params = if is_subscribe {
			quote! {
				Ok((#(#tuple_fields, )*)) => {
					let subscriber = _jsonrpc_pubsub::typed::Subscriber::new(subscriber);
					(method)#method_call
				},
				Err(e) => {
					let _ = subscriber.reject(e);
					return
				}
			}
		} else if self.attr.is_notification() {
			quote! {
				Ok((#(#tuple_fields, )*)) => {
					(method)#method_call
				},
				Err(_) => return,
			}
		} else {
			quote! {
				Ok((#(#tuple_fields, )*)) => {
					use self::_futures::{FutureExt, TryFutureExt};
					let fut = self::_jsonrpc_core::WrapFuture::into_future((method)#method_call)
						.map_ok(|value| _jsonrpc_core::to_value(value)
							.expect("Expected always-serializable type; qed"))
						.map_err(Into::into as fn(_) -> _jsonrpc_core::Error);
					_futures::future::Either::Left(fut)
				},
				Err(e) => _futures::future::Either::Right(_futures::future::ready(Err(e))),
			}
		};

		Ok(quote! {
			move |#closure_args| {
				let method = &(Self::#method_ident as #method_sig);
				#parse_params
				match params {
					#match_params
				}
			}
		})
	}

	fn special_args(param_types: &[syn::Type]) -> Vec<(syn::Ident, syn::Type)> {
		let meta_arg = param_types.first().and_then(|ty| {
			if *ty == parse_quote!(Self::Metadata) {
				Some(ty.clone())
			} else {
				None
			}
		});
		let subscriber_arg = param_types.get(1).and_then(|ty| {
			if let syn::Type::Path(path) = ty {
				if path.path.segments.iter().any(|s| s.ident == SUBSCRIBER_TYPE_IDENT) {
					Some(ty.clone())
				} else {
					None
				}
			} else {
				None
			}
		});

		let mut special_args = Vec::new();
		if let Some(meta) = meta_arg {
			special_args.push((ident(METADATA_CLOSURE_ARG), meta));
		}
		if let Some(subscriber) = subscriber_arg {
			special_args.push((ident(SUBSCRIBER_CLOSURE_ARG), subscriber));
		}
		special_args
	}

	fn params_with_trailing(
		&self,
		trailing_args_num: usize,
		param_types: &[syn::Type],
		tuple_fields: &[syn::Ident],
	) -> proc_macro2::TokenStream {
		let total_args_num = param_types.len();
		let required_args_num = total_args_num - trailing_args_num;

		let switch_branches = (0..=trailing_args_num)
			.map(|passed_trailing_args_num| {
				let passed_args_num = required_args_num + passed_trailing_args_num;
				let passed_param_types = &param_types[..passed_args_num];
				let passed_tuple_fields = &tuple_fields[..passed_args_num];
				let missed_args_num = total_args_num - passed_args_num;
				let missed_params_values = ::std::iter::repeat(quote! { None })
					.take(missed_args_num)
					.collect::<Vec<_>>();

				if passed_args_num == 0 {
					quote! {
						#passed_args_num => params.expect_no_params()
							.map(|_| (#(#missed_params_values, ) *))
							.map_err(Into::into)
					}
				} else {
					quote! {
						#passed_args_num => params.parse::<(#(#passed_param_types, )*)>()
							.map(|(#(#passed_tuple_fields,)*)|
								(#(#passed_tuple_fields, )* #(#missed_params_values, )*))
							.map_err(Into::into)
					}
				}
			})
			.collect::<Vec<_>>();

		quote! {
			let passed_args_num = match params {
				_jsonrpc_core::Params::Array(ref v) => Ok(v.len()),
				_jsonrpc_core::Params::None => Ok(0),
				_ => Err(_jsonrpc_core::Error::invalid_params("`params` should be an array"))
			};

			let params = passed_args_num.and_then(|passed_args_num| {
				match passed_args_num {
					_ if passed_args_num < #required_args_num => Err(_jsonrpc_core::Error::invalid_params(
						format!("`params` should have at least {} argument(s)", #required_args_num))),
					#(#switch_branches),*,
					_ => Err(_jsonrpc_core::Error::invalid_params_with_details(
						format!("Expected from {} to {} parameters.", #required_args_num, #total_args_num),
						format!("Got: {}", passed_args_num))),
				}
			});
		}
	}

	fn generate_add_aliases(&self) -> proc_macro2::TokenStream {
		let name = self.name();
		let add_aliases: Vec<_> = self
			.attr
			.aliases
			.iter()
			.map(|alias| quote! { del.add_alias(#alias, #name); })
			.collect();
		quote! { #(#add_aliases)* }
	}
}

fn ident(s: &str) -> syn::Ident {
	syn::Ident::new(s, proc_macro2::Span::call_site())
}

fn is_option_type(ty: &syn::Type) -> bool {
	if let syn::Type::Path(path) = ty {
		path.path.segments.first().map_or(false, |t| t.ident == "Option")
	} else {
		false
	}
}

pub fn generate_where_clause_serialization_predicates(
	item_trait: &syn::ItemTrait,
	client: bool,
) -> Vec<syn::WherePredicate> {
	#[derive(Default)]
	struct FindTyParams {
		trait_generics: HashSet<syn::Ident>,
		server_to_client_type_params: HashSet<syn::Ident>,
		client_to_server_type_params: HashSet<syn::Ident>,
		visiting_return_type: bool,
		visiting_fn_arg: bool,
		visiting_subscriber_arg: bool,
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
			self.visiting_subscriber_arg =
				self.visiting_subscriber_arg || (self.visiting_fn_arg && segment.ident == SUBSCRIBER_TYPE_IDENT);
			visit::visit_path_segment(self, segment);
			self.visiting_subscriber_arg = self.visiting_subscriber_arg && segment.ident != SUBSCRIBER_TYPE_IDENT;
		}
		fn visit_ident(&mut self, ident: &'ast syn::Ident) {
			if self.trait_generics.contains(&ident) {
				if self.visiting_return_type || self.visiting_subscriber_arg {
					self.server_to_client_type_params.insert(ident.clone());
				}
				if self.visiting_fn_arg && !self.visiting_subscriber_arg {
					self.client_to_server_type_params.insert(ident.clone());
				}
			}
		}
		fn visit_fn_arg(&mut self, arg: &'ast syn::FnArg) {
			self.visiting_fn_arg = true;
			visit::visit_fn_arg(self, arg);
			self.visiting_fn_arg = false;
		}
	}
	let mut visitor = FindTyParams::default();
	visitor.visit_item_trait(item_trait);

	let additional_where_clause = item_trait.generics.where_clause.clone();

	item_trait
		.generics
		.type_params()
		.map(|ty| {
			let ty_path = syn::TypePath {
				qself: None,
				path: ty.ident.clone().into(),
			};
			let mut bounds: Punctuated<syn::TypeParamBound, Token![+]> = parse_quote!(Send + Sync + 'static);
			// add json serialization trait bounds
			if client {
				if visitor.server_to_client_type_params.contains(&ty.ident) {
					bounds.push(parse_quote!(_jsonrpc_core::serde::de::DeserializeOwned))
				}
				if visitor.client_to_server_type_params.contains(&ty.ident) {
					bounds.push(parse_quote!(_jsonrpc_core::serde::Serialize))
				}
			} else {
				if visitor.server_to_client_type_params.contains(&ty.ident) {
					bounds.push(parse_quote!(_jsonrpc_core::serde::Serialize))
				}
				if visitor.client_to_server_type_params.contains(&ty.ident) {
					bounds.push(parse_quote!(_jsonrpc_core::serde::de::DeserializeOwned))
				}
			}

			// add the trait bounds specified by the user in where clause.
			if let Some(ref where_clause) = additional_where_clause {
				for predicate in where_clause.predicates.iter() {
					if let syn::WherePredicate::Type(where_ty) = predicate {
						if let syn::Type::Path(ref predicate) = where_ty.bounded_ty {
							if *predicate == ty_path {
								bounds.extend(where_ty.bounds.clone().into_iter());
							}
						}
					}
				}
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
