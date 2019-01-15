use syn::visit::{self, Visit};

pub enum RpcTraitAttribute {
	PubSubTrait { name: String },
	RpcTrait,
}

#[derive(Clone, Debug)]
pub struct RpcMethodAttribute {
	pub attr: syn::Attribute,
	pub name: String,
	pub has_metadata: bool,
	pub is_subscribe: bool,
	pub is_unsubscribe: bool,
	pub aliases: Vec<String>,
}

#[derive(Default)]
struct RpcAttributeVisitor {
	attr: Option<syn::Attribute>,
	name: Option<String>,
	meta_words: Vec<String>,
	aliases: Vec<String>,
}

impl RpcTraitAttribute {
	pub fn try_from_trait_attribute(args: &syn::AttributeArgs) -> Result<RpcTraitAttribute, String> {
		let mut visitor = RpcAttributeVisitor::default();
		for nested_meta in args {
			visitor.visit_nested_meta(nested_meta);
		}

		if visitor.meta_words.contains(&"pubsub".into()) {
			if let Some(name) = visitor.name {
				Ok(RpcTraitAttribute::PubSubTrait { name })
			} else {
				Err("rpc pubsub trait attribute should have a name".into())
			}
		} else {
			Ok(RpcTraitAttribute::RpcTrait)
		}
	}
}

impl RpcMethodAttribute {
	pub fn try_from_trait_item_method(method: &syn::TraitItemMethod) -> Result<Option<RpcMethodAttribute>, String> {
		let mut visitor = RpcAttributeVisitor::default();
		visitor.visit_trait_item_method(method);

		match (visitor.attr, visitor.name) {
			(Some(attr), Some(name)) => {
				Ok(Some(RpcMethodAttribute {
					attr: attr.clone(),
					aliases: visitor.aliases,
					has_metadata: visitor.meta_words.contains(&"meta".into()),
					is_subscribe: visitor.meta_words.contains(&"subscribe".into()),
					is_unsubscribe: visitor.meta_words.contains(&"unsubscribe".into()),
					name,
				}))
			},
			(None, None) => Ok(None),
			_ => Err("Expected rpc attribute with name argument".into())
		}
	}
}

impl<'a> Visit<'a> for RpcAttributeVisitor {
	fn visit_attribute(&mut self, attr: &syn::Attribute) {
		match attr.parse_meta() {
			Ok(ref meta) => {
				if meta.name() == "rpc" {
					self.attr = Some(attr.clone());
					visit::visit_meta(self, meta);
				}
			},
			// todo [AJ]: add to errors list instead of panicking?
			Err(err) => panic!("Failed to parse attribute: {}", err)
		}
	}
	fn visit_meta(&mut self, meta: &syn::Meta) {
		if let syn::Meta::Word(w) = meta {
			self.meta_words.push(w.to_string())
		}
		visit::visit_meta(self, meta);
	}
	fn visit_meta_name_value(&mut self, name_value: &syn::MetaNameValue) {
		if name_value.ident == "name" {
			if let syn::Lit::Str(ref str) = name_value.lit {
				self.name = Some(str.value())
			}
		}
		visit::visit_meta_name_value(self, name_value);
	}
	fn visit_meta_list(&mut self, meta_list: &syn::MetaList) {
		if meta_list.ident == "alias" {
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
