use std::collections::HashSet;

use quote::quote;
use syn::{
	parse_quote, Token, punctuated::Punctuated,
	visit::{self, Visit},
};

pub enum ToDelegateFunction {
	Standard(Vec<RpcMethod>),
	PubSub {
		name: String,
		subscribe: RpcMethod,
		unsubscribe: RpcMethod,
	}
}

impl ToDelegateFunction {
	pub fn generate_trait_item_method(
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
				let mut del = _jsonrpc_core::IoDelegate::new(self.into());
				#delegate_registration
				del
			};

		// todo: [AJ] check that pubsub has metadata
		let method: syn::TraitItemMethod =
			if has_metadata {
				parse_quote! {
					fn to_delegate(self) -> _jsonrpc_core::IoDelegate<Self, Self::Metadata>
					{
						#to_delegate_body
					}
				}
			} else {
				parse_quote! {
					fn to_delegate<M: _jsonrpc_core::Metadata>(self) -> _jsonrpc_core::IoDelegate<Self, M>
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

	fn register_rpc_method(method: &RpcMethod) -> proc_macro2::TokenStream {
		let rpc_name = &method.name();

		let add_method =
			if method.has_metadata {
				ident("add_method_with_meta")
			} else {
				ident("add_method")
			};

		let closure = method.generate_delegate_closure(false);
		let add_aliases = method.generate_add_aliases();

//		println!("{}", closure);

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
		let sub_name = &subscribe.name();
		let sub_closure = subscribe.generate_delegate_closure(true);
		let sub_aliases = subscribe.generate_add_aliases();

		let unsub_name = &unsubscribe.name();
		let unsub_closure = unsubscribe.generate_delegate_closure(false);
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
	}
}

#[derive(Clone)]
pub struct RpcMethod {
	name: String,
	aliases: Vec<String>,
	has_metadata: bool,
	trait_item: syn::TraitItemMethod,
}

impl RpcMethod {
	pub fn new(
		name: &str,
		aliases: Vec<String>,
		has_metadata: bool,
		trait_item: syn::TraitItemMethod
	) -> RpcMethod {
		RpcMethod { name: name.to_string(), aliases, has_metadata, trait_item }
	}

	pub fn name_contains(&self, s: &str) -> bool {
		self.name.contains(s)
	}

	pub fn name(&self) -> &str {
		&self.name
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

		let meta_arg =
			param_types.first().and_then(|ty|
				if *ty == parse_quote!(Self::Metadata) { Some(ty.clone()) } else { None });

		let subscriber_arg = param_types.iter().nth(1).and_then(|ty| {
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
			param_types.retain(|ty| ty != meta );
			special_args.push((ident("meta"), meta.clone()));
		}
		if let Some(ref subscriber) = subscriber_arg {
			param_types.retain(|ty| ty != subscriber);
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

//		println!("{:?}", &self.trait_item.sig.decl.output);

//		let result = match &self.trait_item.sig.decl.output {
//			// todo: [AJ] require Result type?
//			syn::ReturnType::Type(_, ref output) => Ok(*output.clone()),
//			// todo: [AJ] default
//			syn::ReturnType::Default => Err("Return type required for RPC method signature".to_string())
//		}?;
		let result = &self.trait_item.sig.decl.output;

		let tuple_fields : &Vec<_> =
			&(0..param_types.len() as u8)
				.map(|x| ident(&((x + 'a' as u8) as char).to_string()))
				.collect();

		let param_types = &param_types;

		let parse_params =
			// if the last argument is an `Option` then it can be made an optional 'trailing' argument
			if let Some(ref trailing) = param_types.iter().last().and_then(is_option) {
				let num = param_types.len();
				let param_types_no_trailing: Vec<_> =
					param_types.iter().cloned().filter(|arg| arg != trailing).collect();
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
							0 => params.parse::<(#(#param_types_no_trailing), *)>()
								.map( |(#(#tuple_fields_no_trailing), *)| (#(#tuple_fields_no_trailing, )* None)).map_err(Into::into),
							1 => params.parse::<(#(#param_types), *) > ()
								.map( |(#(#tuple_fields_no_trailing, )* id)| (#(#tuple_fields_no_trailing, )* id)).map_err(Into::into),
							x if x < 0 => Err(Error::invalid_params(format!("`params` should have at least {} argument(s)", #num))),
							_ => Err(Error::invalid_params_with_details(format!("Expected {} or {} parameters.", #num, #num + 1), format!("Got: {}", len))),
						}
					});
				}
			} else if param_types.is_empty() {
				quote! { let params = params.expect_no_params(); }
			} else {
				quote! { let params = params.parse::<(#(#param_types), *)>(); }
			};

		let name = &self.trait_item.sig.ident;
		let extra_closure_args: &Vec<_> = &special_args.iter().cloned().map(|arg| arg.0).collect();
		let extra_method_types: &Vec<_> = &special_args.iter().cloned().map(|arg| arg.1).collect();

		let closure_args = quote! { base, params, #(#extra_closure_args), * };
		let method_sig = quote! { fn(&Self, #(#extra_method_types, ) * #(#param_types), *) #result };
		let method_call = quote! { (base, #(#extra_closure_args, )* #(#tuple_fields), *) };
		let on_error =
			if is_subscribe {
				quote! ( {
					let _ = subscriber.reject(e);
					return
				}, )
			} else {
				quote! ( _futures::future::Either::B(_futures::failed(e)), )
			};

//		println!("method sig: {}", method_sig);

		quote! {
			move |#closure_args| {
//				let method = &(Self::#method as #method_sig -> #result);
				let method = &(Self::#name as #method_sig);
				#parse_params
				match params {
					Ok((#(#tuple_fields), *)) => {
						let fut = (method)#method_call
							.into_future()
							.map(|value| _jsonrpc_core::to_value(value).expect("Expected always-serializable type; qed"))
							.map_err(Into::into as fn(_) -> _jsonrpc_core::Error);
						_futures::future::Either::A(fut)
					},
					Err(e) => #on_error
				}
			}
		}
	}

	fn generate_add_aliases(&self) -> proc_macro2::TokenStream {
		let name = &self.name;
		let add_aliases: Vec<_> = self.aliases
			.iter()
			.map(|alias| quote! { del.add_alias(#alias, #name); })
			.collect();
		quote!{ #(#add_aliases)* }
	}
}

fn ident(s: &str) -> syn::Ident {
	syn::Ident::new(s, proc_macro2::Span::call_site())
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
