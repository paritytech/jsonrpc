use syn::visit::{self, Visit};

pub struct RpcTraitAttribute {
	pub is_pubsub: bool,
}

#[derive(Clone, Debug)]
pub struct RpcMethodAttribute {
	pub attr: syn::Attribute,
	pub name: String,
	pub has_metadata: bool,
	pub pubsub: Option<PubSubMethod>,
	pub aliases: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PubSubMethod {
	pub name: String,
	pub kind: PubSubMethodKind,
}

impl PubSubMethod {
	fn is_subscribe(&self) -> bool {
		match self.kind {
			PubSubMethodKind::Subscribe => true,
			PubSubMethodKind::Unsubscribe => false,
		}
	}
	fn is_unsubscribe(&self) -> bool {
		match self.kind {
			PubSubMethodKind::Subscribe => false,
			PubSubMethodKind::Unsubscribe => true,
		}
	}
}

#[derive(Clone, Debug)]
pub enum PubSubMethodKind {
	Subscribe,
	Unsubscribe,
}

#[derive(Default)]
struct RpcAttributeVisitor {
	attr: Option<syn::Attribute>,
	pubsub: Option<String>,
	name: Option<String>,
	meta_words: Vec<String>,
	aliases: Vec<String>,
}

const RPC_ATTR_NAME: &'static str = "rpc";
const RPC_NAME_KEY: &'static str = "name";
const RPC_ALIASES_KEY: &'static str = "aliases";
const PUB_SUB_META_WORD: &'static str = "pubsub";
const METADATA_META_WORD: &'static str = "meta";
const SUBSCRIBE_META_WORD: &'static str = "subscribe";
const UNSUBSCRIBE_META_WORD: &'static str = "unsubscribe";

impl From<syn::AttributeArgs> for RpcTraitAttribute {
	fn from(args: syn::AttributeArgs) -> RpcTraitAttribute {
		let mut visitor = RpcAttributeVisitor::default();
		for nested_meta in args {
			visitor.visit_nested_meta(&nested_meta);
		}
		let is_pubsub = visitor.meta_words.contains(&PUB_SUB_META_WORD.into());
		RpcTraitAttribute { is_pubsub }
	}
}

impl RpcMethodAttribute {
	pub fn try_from_trait_item_method(method: &syn::TraitItemMethod) -> Result<Option<RpcMethodAttribute>, String> {
		let mut visitor = RpcAttributeVisitor::default();
		visitor.visit_trait_item_method(method);

		match (visitor.attr, visitor.name) {
			(Some(attr), Some(name)) => {
				let pubsub =
					visitor.pubsub.map(|pubsub| {
						if visitor.meta_words.contains(&SUBSCRIBE_META_WORD.into()) {
							Ok(PubSubMethod { name, kind: PubSubMethodKind::Subscribe })
						} else if visitor.meta_words.contains(&UNSUBSCRIBE_META_WORD.into()) {
							Ok(PubSubMethod { name, kind: PubSubMethodKind::Unsubscribe })
						} else {
							Err(format!("Rpc methods with `pubsub` should be annotated either `{}` or `{}`",
								SUBSCRIBE_META_WORD, UNSUBSCRIBE_META_WORD))
						}
					});

				Ok(Some(RpcMethodAttribute {
					attr: attr.clone(),
					aliases: visitor.aliases,
					has_metadata: visitor.meta_words.contains(&METADATA_META_WORD.into()),
					pubsub: pubsub.map_or(Ok(None), |r| r.map(Some))?,
					name,
				}))
			},
			(None, None) => Ok(None),
			_ => Err("Expected rpc attribute with name argument".into())
		}
	}

	pub fn is_subscribe(&self) -> bool {
		self.attr.pubsub.map_or(false, PubSubMethod::is_subscribe)
	}

	pub fn is_unsubscribe(&self) -> bool {
		self.attr.pubsub.map_or(false, PubSubMethod::is_unsubscribe)
	}
}

impl<'a> Visit<'a> for RpcAttributeVisitor {
	fn visit_attribute(&mut self, attr: &syn::Attribute) {
		match attr.parse_meta() {
			Ok(ref meta) => {
				if meta.name() == RPC_ATTR_NAME {
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
		if name_value.ident == RPC_NAME_KEY {
			if let syn::Lit::Str(ref str) = name_value.lit {
				self.name = Some(str.value())
			}
		}
		visit::visit_meta_name_value(self, name_value);
	}
	fn visit_meta_list(&mut self, meta_list: &syn::MetaList) {
		if meta_list.ident == RPC_ALIASES_KEY {
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
