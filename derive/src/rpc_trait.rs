use std::collections::HashSet;

use proc_macro;
use quote::quote;
use syn::{
	parse_macro_input, parse_quote, Token,
	punctuated::Punctuated, fold::{self, Fold}, visit::{self, Visit},
};

type Result<T> = std::result::Result<T, String>;

fn ident(s: &str) -> syn::Ident {
	syn::Ident::new(s, proc_macro2::Span::call_site())
}

#[derive(Clone, Debug)]
struct RpcMethod {
	rpc_attr: syn::Attribute,
	method: syn::TraitItemMethod,
	name: String,
	aliases: Vec<String>,
}

impl RpcMethod {
	fn try_from_trait_item_method(trait_item: &syn::TraitItemMethod) -> Result<Option<RpcMethod>> {
		struct VisitRpcAttribute {
			attr: Option<syn::Attribute>,
			name: Option<String>,
			aliases: Vec<String>,
		}
		impl<'a> Visit<'a> for VisitRpcAttribute {
			fn visit_attribute(&mut self, attr: &syn::Attribute) {
				if let Ok(ref meta) = attr.parse_meta() {
					if meta.name() == "rpc" {
						self.attr = Some(attr.clone());
						visit::visit_meta(self, meta);
					}
				}
			}
			fn visit_meta_name_value(&mut self, name_value: &syn::MetaNameValue) {
				if name_value.ident.to_string() == "name" {
					if let syn::Lit::Str(ref str) = name_value.lit {
						self.name = Some(str.value())
					}
				}
				visit::visit_meta_name_value(self, name_value);
			}
			fn visit_meta_list(&mut self, meta_list: &syn::MetaList) {
				if meta_list.ident.to_string() == "alias" {
					self.aliases = meta_list.nested
						.iter()
						.filter_map(|nm| {
							if let syn::NestedMeta::Literal(syn::Lit::Str(alias)) = nm {
								Some(alias.value())
							} else {
								None
							}
						})
						.collect();
				}
				visit::visit_meta_list(self,meta_list)
			}
		}
		let mut visitor = VisitRpcAttribute {
			attr: None,
			name: None,
			aliases: Vec::new(),
		};
		visitor.visit_trait_item_method(trait_item);

		match (visitor.attr, visitor.name) {
			(Some(attr), Some(name)) => {
				Ok(Some(RpcMethod {
					rpc_attr: attr.clone(),
					method: trait_item.clone(),
					aliases: visitor.aliases,
					name,
				}))
			},
			(None, None) => Ok(None),
			_ => Err("Expected rpc attribute with name argument".to_string())
		}
	}
}

struct RpcTrait {
	methods: Vec<RpcMethod>,
}

impl<'a> Fold for RpcTrait {
	fn fold_item_trait(&mut self, item_trait: syn::ItemTrait) -> syn::ItemTrait {
		// first visit the trait to collect the methods
		let mut item_trait = fold::fold_item_trait(self, item_trait);

		let to_delegate_method = self.generate_to_delegate_method(&item_trait);
		item_trait.items.push(syn::TraitItem::Method(to_delegate_method));

		let trait_bounds: Punctuated<syn::TypeParamBound, Token![+]> =
			parse_quote!(Sized + Send + Sync + 'static);
		item_trait.supertraits.extend(trait_bounds);

		item_trait
	}

	fn fold_trait_item_method(&mut self, method: syn::TraitItemMethod) -> syn::TraitItemMethod {
		let mut method = method.clone();
		match RpcMethod::try_from_trait_item_method(&method) {
			Ok(Some(rpc_method)) => {
				self.methods.push(rpc_method.clone());
				// remove the rpc attribute
				method.attrs.retain(|a| *a != rpc_method.rpc_attr);
			},
			Ok(None) => (), // non rpc annotated trait method
			Err(err) => panic!("Invalid rpc method attribute {}", err)
		}
		fold::fold_trait_item_method(self, method)
	}
}

impl RpcTrait {
	fn generate_to_delegate_method(&self, trait_item: &syn::ItemTrait) -> syn::TraitItemMethod {
		let add_methods: Vec<_> = self.methods
			.iter()
			.map(|rpc| {
				let rpc_name = &rpc.name;
				let method = &rpc.method.sig.ident;
				let arg_types = rpc.method.sig.decl.inputs
					.iter()
					.filter_map(|arg| {
						let ty =
							match arg {
								syn::FnArg::Captured(arg_captured) => Some(&arg_captured.ty),
								syn::FnArg::Ignored(ty) => Some(ty),
								// todo: [AJ] what about Inferred?
								_ => None,
							};
						ty.map(|t| quote! { #t })
					});
				let result = match rpc.method.sig.decl.output {
					// todo: [AJ] require Result type?
					syn::ReturnType::Type(_, ref output) => output,
					syn::ReturnType::Default => panic!("Return type required for RPC method signature")
				};
				let add_aliases: Vec<_> = rpc.aliases
					.iter()
					.map(|alias| {
						quote! {
							del.add_alias(#alias, #rpc_name);
						}
					})
					.collect();
				quote! {
					del.add_method(#rpc_name, move |base, params| {
						_jsonrpc_macros::WrapAsync::wrap_rpc(
							&(Self::#method as fn(&_ #(, #arg_types)*) -> #result),
							base,
							params
						)
					});
					#(#add_aliases)*
				}
			})
			.collect();

		let method: syn::TraitItemMethod =
			parse_quote! {
				fn to_delegate<M: _jsonrpc_core::Metadata>(self) -> _jsonrpc_macros::IoDelegate<Self, M>
				{
					let mut del = _jsonrpc_macros::IoDelegate::new(self.into());
					#(#add_methods)*
					del
				}
			};

		with_where_clause_serialization_bounds(&trait_item, &method)
	}
}

fn with_where_clause_serialization_bounds(
	item_trait: &syn::ItemTrait,
	method: &syn::TraitItemMethod,
) -> syn::TraitItemMethod {
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

	let predicates = item_trait.generics
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
		});

	let mut method = method.clone();
	method.sig.decl.generics
		.make_where_clause()
		.predicates
		.extend(predicates);
	method
}

fn impl_rpc(_args: syn::AttributeArgs, input: syn::Item) -> Result<proc_macro2::TokenStream> {
	let rpc_trait = match input {
		syn::Item::Trait(item_trait) => item_trait,
		_ => return Err("rpc_api trait only works with trait declarations".to_owned())
	};

	let mut visitor = RpcTrait { methods: Vec::new() };
	let rpc_trait = visitor.fold_item_trait(rpc_trait);

	let name = rpc_trait.ident.clone();

	let mod_name = format!("rpc_impl_{}", name.to_string());
	let mod_name_ident = ident(&mod_name);

	Ok(quote! {
		mod #mod_name_ident {
			extern crate jsonrpc_core as _jsonrpc_core;
			extern crate jsonrpc_macros as _jsonrpc_macros;
			extern crate serde as _serde;
			use super::*;

			#rpc_trait
		}
		pub use self::#mod_name_ident::#name;
	})
}

pub fn build_rpc_trait_impl(args: proc_macro::TokenStream, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
	let args_toks = parse_macro_input!(args as syn::AttributeArgs);
	let input_toks = parse_macro_input!(input as syn::Item);

	let output = match impl_rpc(args_toks, input_toks) {
		Ok(output) => output,
		Err(err) => panic!("[rpc_api] encountered error: {}", err),
	};

	output.into()
}
