use syn::{Error, Result};

#[derive(Clone, Debug)]
pub struct RpcMethodAttribute {
	pub attr: syn::Attribute,
	pub name: String,
	pub aliases: Vec<String>,
	pub kind: AttributeKind,
}

#[derive(Clone, Debug)]
pub enum AttributeKind {
	Rpc { has_metadata: bool },
	PubSub { subscription_name: String, kind: PubSubMethodKind }
}

#[derive(Clone, Debug)]
pub enum PubSubMethodKind {
	Subscribe,
	Unsubscribe,
}

const RPC_ATTR_NAME: &'static str = "rpc";
const RPC_NAME_KEY: &'static str = "name";
const SUBSCRIPTION_NAME_KEY: &'static str = "subscription";
const ALIASES_KEY: &'static str = "alias";
const PUB_SUB_ATTR_NAME: &'static str = "pubsub";
const METADATA_META_WORD: &'static str = "meta";
const SUBSCRIBE_META_WORD: &'static str = "subscribe";
const UNSUBSCRIBE_META_WORD: &'static str = "unsubscribe";

const MULTIPLE_RPC_ATTRIBUTES_ERR: &'static str = "Expected only a single rpc attribute per method";
const MISSING_NAME_ERR: &'static str = "rpc attribute should have a name e.g. `name = \"method_name\"`";
const MISSING_SUB_NAME_ERR: &'static str = "pubsub attribute should have a subscription name";
const BOTH_SUB_AND_UNSUB_ERR: &'static str = "pubsub attribute annotated with both subscribe and unsubscribe";
const NEITHER_SUB_OR_UNSUB_ERR: &'static str = "pubsub attribute not annotated with either subscribe or unsubscribe";

impl RpcMethodAttribute {
	pub fn parse_attr(method: &syn::TraitItemMethod) -> Result<Option<RpcMethodAttribute>> {
		let attrs = method.attrs
			.iter()
			.filter_map(Self::parse_meta)
			.collect::<Result<Vec<_>>>()?;

		if attrs.len() <= 1 {
			Ok(attrs.first().cloned())
		} else {
			Err(Error::new_spanned(method, MULTIPLE_RPC_ATTRIBUTES_ERR))
		}
	}

	fn parse_meta(attr: &syn::Attribute) -> Option<Result<RpcMethodAttribute>> {
		let parse_result = attr.parse_meta();
		match parse_result {
			Ok(ref meta) => {
				let attr_kind =
					match meta.name().to_string().as_ref() {
						RPC_ATTR_NAME => {
							let has_metadata = get_meta_list(meta)
								.map_or(false, |ml| has_meta_word(METADATA_META_WORD, ml));
							Some(Ok(AttributeKind::Rpc { has_metadata }))
						},
						PUB_SUB_ATTR_NAME => Some(Self::parse_pubsub(meta)),
						_ => None,
					};
				attr_kind.map(|kind| kind.and_then(|kind| {
					get_meta_list(meta)
						.and_then(|ml| get_name_value(RPC_NAME_KEY, ml))
						.map_or(
							Err(Error::new_spanned(attr,MISSING_NAME_ERR)),
							|name| {
								let aliases = get_meta_list(meta)
									.map_or(Vec::new(), |ml| get_aliases(ml));
								Ok(RpcMethodAttribute {
									attr: attr.clone(),
									name: name.into(),
									aliases,
									kind
								})
							})
				}))
			},
			Err(err) => Some(Err(err)),
		}
	}

	fn parse_pubsub(meta: &syn::Meta) -> Result<AttributeKind> {
		let name_and_list = get_meta_list(meta)
			.and_then(|ml|
				get_name_value(SUBSCRIPTION_NAME_KEY, ml).map(|name| (name, ml))
			);

		name_and_list.map_or(
			Err(Error::new_spanned(meta, MISSING_SUB_NAME_ERR)),
			|(sub_name, ml)| {
				let is_subscribe = has_meta_word(SUBSCRIBE_META_WORD, ml);
				let is_unsubscribe = has_meta_word(UNSUBSCRIBE_META_WORD, ml);
				let kind = match (is_subscribe, is_unsubscribe) {
					(true, false) =>
						Ok(PubSubMethodKind::Subscribe),
					(false, true) =>
						Ok(PubSubMethodKind::Unsubscribe),
					(true, true) =>
						Err(Error::new_spanned(meta, BOTH_SUB_AND_UNSUB_ERR)),
					(false, false) =>
						Err(Error::new_spanned(meta,NEITHER_SUB_OR_UNSUB_ERR)),
				};
				kind.map(|kind| AttributeKind::PubSub {
					subscription_name: sub_name.into(),
					kind,
				})
			})
	}

	pub fn is_pubsub(&self) -> bool {
		match self.kind {
			AttributeKind::PubSub { .. } => true,
			AttributeKind::Rpc { .. } => false,
		}
	}
}

fn get_meta_list(meta: &syn::Meta) -> Option<&syn::MetaList> {
	if let syn::Meta::List(ml) = meta {
		Some(ml)
	} else {
		None
	}
}

fn get_name_value(key: &str, ml: &syn::MetaList) -> Option<String> {
	ml.nested
		.iter()
		.find_map(|nested|
			if let syn::NestedMeta::Meta(syn::Meta::NameValue(mnv)) = nested {
				if mnv.ident == key {
					if let syn::Lit::Str(ref lit) = mnv.lit {
						Some(lit.value())
					} else {
						None
					}
				} else {
					None
				}
			} else {
				None
			}
		)
}

fn has_meta_word(word: &str, ml: &syn::MetaList) -> bool {
	ml.nested
		.iter()
		.any(|nested|
			if let syn::NestedMeta::Meta(syn::Meta::Word(w)) = nested {
				word == w.to_string()
			} else {
				false
			}
		)
}

fn get_aliases(ml: &syn::MetaList) -> Vec<String> {
	ml.nested
		.iter()
		.find_map(|nested|
			if let syn::NestedMeta::Meta(syn::Meta::List(list)) = nested {
				if list.ident == ALIASES_KEY {
					Some(list)
				} else {
					None
				}
			} else {
				None
			}
		)
		.map_or(Vec::new(), |list|
			list.nested
				.iter()
				.filter_map(|nm| {
					if let syn::NestedMeta::Literal(syn::Lit::Str(alias)) = nm {
						Some(alias.value())
					} else {
						None
					}
				})
				.collect()
		)
}
