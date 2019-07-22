use syn::{
	visit::{self, Visit},
	Error, Result,
};

#[derive(Clone, Debug)]
pub struct RpcMethodAttribute {
	pub attr: syn::Attribute,
	pub name: String,
	pub aliases: Vec<String>,
	pub kind: AttributeKind,
	pub raw_params: bool,
}

#[derive(Clone, Debug)]
pub enum AttributeKind {
	Rpc {
		has_metadata: bool,
		returns: Option<String>,
		is_notification: bool,
	},
	PubSub {
		subscription_name: String,
		kind: PubSubMethodKind,
	},
}

#[derive(Clone, Debug)]
pub enum PubSubMethodKind {
	Subscribe,
	Unsubscribe,
}

const RPC_ATTR_NAME: &str = "rpc";
const RPC_NAME_KEY: &str = "name";
const SUBSCRIPTION_NAME_KEY: &str = "subscription";
const ALIASES_KEY: &str = "alias";
const PUB_SUB_ATTR_NAME: &str = "pubsub";
const METADATA_META_WORD: &str = "meta";
const RAW_PARAMS_META_WORD: &str = "raw_params";
const SUBSCRIBE_META_WORD: &str = "subscribe";
const UNSUBSCRIBE_META_WORD: &str = "unsubscribe";
const RETURNS_META_WORD: &str = "returns";

const MULTIPLE_RPC_ATTRIBUTES_ERR: &str = "Expected only a single rpc attribute per method";
const INVALID_ATTR_PARAM_NAMES_ERR: &str = "Invalid attribute parameter(s):";
const MISSING_NAME_ERR: &str = "rpc attribute should have a name e.g. `name = \"method_name\"`";
const MISSING_SUB_NAME_ERR: &str = "pubsub attribute should have a subscription name";
const BOTH_SUB_AND_UNSUB_ERR: &str = "pubsub attribute annotated with both subscribe and unsubscribe";
const NEITHER_SUB_OR_UNSUB_ERR: &str = "pubsub attribute not annotated with either subscribe or unsubscribe";

impl RpcMethodAttribute {
	pub fn parse_attr(method: &syn::TraitItemMethod) -> Result<Option<RpcMethodAttribute>> {
		let output = &method.sig.decl.output;
		let attrs = method
			.attrs
			.iter()
			.filter_map(|attr| Self::parse_meta(attr, &output))
			.collect::<Result<Vec<_>>>()?;

		if attrs.len() <= 1 {
			Ok(attrs.first().cloned())
		} else {
			Err(Error::new_spanned(method, MULTIPLE_RPC_ATTRIBUTES_ERR))
		}
	}

	fn parse_meta(attr: &syn::Attribute, output: &syn::ReturnType) -> Option<Result<RpcMethodAttribute>> {
		match attr.parse_meta().and_then(validate_attribute_meta) {
			Ok(ref meta) => {
				let attr_kind = match meta.name().to_string().as_ref() {
					RPC_ATTR_NAME => Some(Self::parse_rpc(meta, output)),
					PUB_SUB_ATTR_NAME => Some(Self::parse_pubsub(meta)),
					_ => None,
				};
				attr_kind.map(|kind| {
					kind.and_then(|kind| {
						get_meta_list(meta)
							.and_then(|ml| get_name_value(RPC_NAME_KEY, ml))
							.map_or(Err(Error::new_spanned(attr, MISSING_NAME_ERR)), |name| {
								let aliases = get_meta_list(&meta).map_or(Vec::new(), |ml| get_aliases(ml));
								let raw_params =
									get_meta_list(meta).map_or(false, |ml| has_meta_word(RAW_PARAMS_META_WORD, ml));
								Ok(RpcMethodAttribute {
									attr: attr.clone(),
									name,
									aliases,
									kind,
									raw_params,
								})
							})
					})
				})
			}
			Err(err) => Some(Err(err)),
		}
	}

	fn parse_rpc(meta: &syn::Meta, output: &syn::ReturnType) -> Result<AttributeKind> {
		let has_metadata = get_meta_list(meta).map_or(false, |ml| has_meta_word(METADATA_META_WORD, ml));
		let returns = get_meta_list(meta).map_or(None, |ml| get_name_value(RETURNS_META_WORD, ml));
		let is_notification = match output {
			syn::ReturnType::Default => true,
			syn::ReturnType::Type(_, ret) => match **ret {
				syn::Type::Tuple(ref tup) if tup.elems.empty_or_trailing() => true,
				_ => false,
			},
		};

		if is_notification && returns.is_some() {
			return Err(syn::Error::new_spanned(output, &"Notifications must return ()"));
		}

		Ok(AttributeKind::Rpc {
			has_metadata,
			returns,
			is_notification,
		})
	}

	fn parse_pubsub(meta: &syn::Meta) -> Result<AttributeKind> {
		let name_and_list =
			get_meta_list(meta).and_then(|ml| get_name_value(SUBSCRIPTION_NAME_KEY, ml).map(|name| (name, ml)));

		name_and_list.map_or(Err(Error::new_spanned(meta, MISSING_SUB_NAME_ERR)), |(sub_name, ml)| {
			let is_subscribe = has_meta_word(SUBSCRIBE_META_WORD, ml);
			let is_unsubscribe = has_meta_word(UNSUBSCRIBE_META_WORD, ml);
			let kind = match (is_subscribe, is_unsubscribe) {
				(true, false) => Ok(PubSubMethodKind::Subscribe),
				(false, true) => Ok(PubSubMethodKind::Unsubscribe),
				(true, true) => Err(Error::new_spanned(meta, BOTH_SUB_AND_UNSUB_ERR)),
				(false, false) => Err(Error::new_spanned(meta, NEITHER_SUB_OR_UNSUB_ERR)),
			};
			kind.map(|kind| AttributeKind::PubSub {
				subscription_name: sub_name,
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

	pub fn is_notification(&self) -> bool {
		match self.kind {
			AttributeKind::Rpc { is_notification, .. } => is_notification,
			AttributeKind::PubSub { .. } => false,
		}
	}
}

fn validate_attribute_meta(meta: syn::Meta) -> Result<syn::Meta> {
	#[derive(Default)]
	struct Visitor {
		meta_words: Vec<String>,
		name_value_names: Vec<String>,
		meta_list_names: Vec<String>,
	}
	impl<'a> Visit<'a> for Visitor {
		fn visit_meta(&mut self, meta: &syn::Meta) {
			match meta {
				syn::Meta::List(list) => self.meta_list_names.push(list.ident.to_string()),
				syn::Meta::Word(ident) => self.meta_words.push(ident.to_string()),
				syn::Meta::NameValue(nv) => self.name_value_names.push(nv.ident.to_string()),
			}
		}
	}

	let mut visitor = Visitor::default();
	visit::visit_meta(&mut visitor, &meta);

	match meta.name().to_string().as_ref() {
		RPC_ATTR_NAME => {
			validate_idents(&meta, &visitor.meta_words, &[METADATA_META_WORD, RAW_PARAMS_META_WORD])?;
			validate_idents(&meta, &visitor.name_value_names, &[RPC_NAME_KEY, RETURNS_META_WORD])?;
			validate_idents(&meta, &visitor.meta_list_names, &[ALIASES_KEY])
		}
		PUB_SUB_ATTR_NAME => {
			validate_idents(
				&meta,
				&visitor.meta_words,
				&[SUBSCRIBE_META_WORD, UNSUBSCRIBE_META_WORD, RAW_PARAMS_META_WORD],
			)?;
			validate_idents(&meta, &visitor.name_value_names, &[SUBSCRIPTION_NAME_KEY, RPC_NAME_KEY])?;
			validate_idents(&meta, &visitor.meta_list_names, &[ALIASES_KEY])
		}
		_ => Ok(meta), // ignore other attributes - compiler will catch unknown ones
	}
}

fn validate_idents(meta: &syn::Meta, attr_idents: &[String], valid: &[&str]) -> Result<syn::Meta> {
	let invalid_meta_words: Vec<_> = attr_idents
		.iter()
		.filter(|w| !valid.iter().any(|v| v == w))
		.cloned()
		.collect();
	if invalid_meta_words.is_empty() {
		Ok(meta.clone())
	} else {
		let expected = format!("Expected '{}'", valid.join(", "));
		let msg = format!(
			"{} '{}'. {}",
			INVALID_ATTR_PARAM_NAMES_ERR,
			invalid_meta_words.join(", "),
			expected
		);
		Err(Error::new_spanned(meta, msg))
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
	ml.nested.iter().find_map(|nested| {
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
	})
}

fn has_meta_word(word: &str, ml: &syn::MetaList) -> bool {
	ml.nested.iter().any(|nested| {
		if let syn::NestedMeta::Meta(syn::Meta::Word(w)) = nested {
			w == word
		} else {
			false
		}
	})
}

fn get_aliases(ml: &syn::MetaList) -> Vec<String> {
	ml.nested
		.iter()
		.find_map(|nested| {
			if let syn::NestedMeta::Meta(syn::Meta::List(list)) = nested {
				if list.ident == ALIASES_KEY {
					Some(list)
				} else {
					None
				}
			} else {
				None
			}
		})
		.map_or(Vec::new(), |list| {
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
		})
}
