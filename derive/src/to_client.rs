use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::Result;
use syn::punctuated::Punctuated;
use crate::rpc_attr::AttributeKind;
use crate::to_delegate::{
	generate_where_clause_serialization_predicates,
	MethodRegistration,
};

pub fn generate_client_module(
	methods: &[MethodRegistration],
	item_trait: &syn::ItemTrait,
) -> Result<TokenStream> {
	let client_methods = generate_client_methods(methods)?;
	let generics = &item_trait.generics;
	let where_clause =
		generate_where_clause_serialization_predicates(&item_trait, true);
	let where_clause2 = where_clause.clone();
	let markers = generics.params.iter()
		.filter_map(|param| {
			match param {
				syn::GenericParam::Type(syn::TypeParam { ident, .. }) => Some(ident),
				_ => None,
			}
		})
		.enumerate()
		.map(|(i, ty)| {
			let field_name = "_".to_string() + &i.to_string();
			let field = Ident::new(&field_name, ty.span());
			(field, ty)
		});
	let (markers_decl, markers_impl): (Vec<_>, Vec<_>) = markers.map(|(field, ty)| {
		(
			quote! {
				#field: std::marker::PhantomData<#ty>
			},
			quote! {
				#field: std::marker::PhantomData
			}
		)
	}).unzip();
	Ok(quote! {
		/// The generated client module.
		pub mod gen_client {
			use super::*;
			use _jsonrpc_core::{
				Call, Error, ErrorCode, Id, MethodCall, Params, Request,
				Response, Version,
			};
			use _jsonrpc_core::futures::{future, Future, Sink};
			use _jsonrpc_core::futures::sync::oneshot;
			use _jsonrpc_core::serde_json::{self, Value};
			use jsonrpc_client::{RpcChannel, RpcError, RpcFuture, RpcMessage};

			/// The Client.
			#[derive(Clone)]
			pub struct Client#generics {
				sender: RpcChannel,
				#(#markers_decl),*
			}

			impl#generics Client#generics
			where
				#(#where_clause),*
			{
				/// Creates a new `Client`.
				pub fn new(sender: RpcChannel) -> Self {
					Client {
						sender,
						#(#markers_impl),*
					}
				}

				#(#client_methods)*

				fn call_method(
					&self,
					method: String,
					params: Params,
				) -> impl Future<Item=Value, Error=RpcError> {
					let (sender, receiver) = oneshot::channel();
					let msg = RpcMessage {
						method,
						params,
						sender,
					};
					self.sender
						.to_owned()
						.send(msg)
						.map_err(|error| RpcError::Other(error.into()))
						.and_then(|_| RpcFuture::new(receiver))
				}
			}

			impl#generics From<RpcChannel> for Client#generics
			where
				#(#where_clause2),*
			{
				fn from(channel: RpcChannel) -> Self {
					Client::new(channel)
				}
			}
		}
	})
}

fn generate_client_methods(
	methods: &[MethodRegistration],
) -> Result<Vec<syn::ImplItem>> {
	let mut client_methods = vec![];
	for method in methods {
		match method {
			MethodRegistration::Standard { method, .. } => {
				let attrs = get_doc_comments(&method.trait_item.attrs);
				let rpc_name = method.name();
				let name = &method.trait_item.sig.ident;
				let args = compute_args(&method.trait_item);
				let arg_names = compute_arg_identifiers(&args)?;
				let returns = match &method.attr.kind {
					AttributeKind::Rpc { returns, .. } => {
						compute_returns(&method.trait_item, returns)?
					},
					AttributeKind::PubSub { .. } => {
						continue;
					},
				};
				let client_method = generate_client_method(
					&attrs,
					rpc_name,
					name,
					&args,
					&arg_names,
					&returns,
				);
				client_methods.push(client_method);
			}
			MethodRegistration::PubSub { .. } => {
				println!("warning: pubsub methods are currently not supported in the generated client.")
			}
		}
	}
	Ok(client_methods)
}

fn generate_client_method(
	attrs: &[syn::Attribute],
	rpc_name: &str,
	name: &syn::Ident,
	args: &Punctuated<syn::FnArg, syn::token::Comma>,
	arg_names: &[&syn::Ident],
	returns: &syn::Type,
) -> syn::ImplItem {
	syn::parse_quote! {
		#(#attrs)*
		pub fn #name(&self, #args) -> impl Future<Item=#returns, Error=RpcError> {
			let args_tuple = (#(#arg_names,)*);
			let args = serde_json::to_value(args_tuple)
				.expect("Only types with infallible serialisation can be used for JSON-RPC");
			let method = #rpc_name.to_owned();
			let params = match args {
				Value::Array(vec) => Some(Params::Array(vec)),
				Value::Null => Some(Params::None),
				_ => None,
			}.expect("should never happen");
			self.call_method(method, params)
				.and_then(|value: Value| {
					let result = serde_json::from_value::<#returns>(value)
						.map_err(|error| RpcError::Other(error.into()));
					future::done(result)
				})
		}
	}
}

fn get_doc_comments(attrs: &[syn::Attribute]) -> Vec<syn::Attribute> {
	let mut doc_comments = vec![];
	for attr in attrs {
		match attr {
			syn::Attribute {
				path: syn::Path { segments, .. },
				..
			} => {
				match &segments[0] {
					syn::PathSegment { ident, .. } => {
						if ident.to_string() == "doc" {
							doc_comments.push(attr.to_owned());
						}
					}
				}
			}
		}
	}
	doc_comments
}

fn compute_args(
	method: &syn::TraitItemMethod,
) -> Punctuated<syn::FnArg, syn::token::Comma> {
	let mut args = Punctuated::new();
	for arg in &method.sig.decl.inputs {
		let ty = match arg {
			syn::FnArg::Captured(syn::ArgCaptured { ty, .. }) => ty,
			_ => continue,
		};
		let segments = match ty {
			syn::Type::Path(syn::TypePath {
				path: syn::Path { segments, .. },
				..
			}) => segments,
			_ => continue,
		};
		let ident = match &segments[0] {
			syn::PathSegment { ident, .. } => ident,
		};
		if ident.to_string() == "Self" {
			continue;
		}
		args.push(arg.to_owned());
	}
	args
}

fn compute_arg_identifiers(
	args: &Punctuated<syn::FnArg, syn::token::Comma>,
) -> Result<Vec<&syn::Ident>> {
	let mut arg_names = vec![];
	for arg in args {
		let pat = match arg {
			syn::FnArg::Captured(syn::ArgCaptured { pat, .. }) => pat,
			_ => continue,
		};
		let ident = match pat {
			syn::Pat::Ident(syn::PatIdent { ident, .. }) => ident,
			syn::Pat::Wild(wild) => {
				let span = wild.underscore_token.spans[0];
				let msg = "No wildcard patterns allowed in rpc trait.";
				return Err(syn::Error::new(span, msg))
			},
			_ => continue,
		};
		arg_names.push(ident);
	}
	Ok(arg_names)
}

fn compute_returns(
	method: &syn::TraitItemMethod,
	returns: &Option<String>,
) -> Result<syn::Type> {
	let returns: Option<syn::Type> = match returns {
		Some(returns) => Some(syn::parse_str(returns)?),
		None => None,
	};
	let returns = match returns {
		None => try_infer_returns(&method.sig.decl.output),
		_ => returns,
	};
	let returns = match returns {
		Some(returns) => returns,
		None => {
			let span = method.attrs[0].pound_token.spans[0];
			let msg = "Missing returns attribute.";
			return Err(syn::Error::new(span, msg))
		}
	};
	Ok(returns)
}

fn try_infer_returns(output: &syn::ReturnType) -> Option<syn::Type> {
	match output {
		syn::ReturnType::Type(_, ty) => {
			match &**ty {
				syn::Type::Path(syn::TypePath {
					path: syn::Path { segments, .. },
					..
				}) => {
					match &segments[0] {
						syn::PathSegment {
							ident,
							arguments,
							..
						} => {
							if ident.to_string().ends_with("Result") {
								get_first_type_argument(arguments)
							} else {
								None
							}
						}
					}
				}
				_ => None,
			}
		}
		_ => None,
	}
}

fn get_first_type_argument(args: &syn::PathArguments) -> Option<syn::Type> {
	match args {
		syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
			args,
			..
		}) => {
			if args.len() > 0 {
				match &args[0] {
					syn::GenericArgument::Type(ty) => Some(ty.to_owned()),
					_ => None,
				}
			} else {
				None
			}
		}
		_ => None
	}
}
