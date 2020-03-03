use std::str::FromStr;

use crate::params_style::ParamStyle;
use crate::rpc_attr::path_eq_str;

const CLIENT_META_WORD: &str = "client";
const SERVER_META_WORD: &str = "server";
const PARAMS_META_KEY: &str = "params";

#[derive(Debug)]
pub struct DeriveOptions {
	pub enable_client: bool,
	pub enable_server: bool,
	pub params_style: ParamStyle,
}

impl DeriveOptions {
	pub fn new(enable_client: bool, enable_server: bool, params_style: ParamStyle) -> Self {
		DeriveOptions {
			enable_client,
			enable_server,
			params_style,
		}
	}

	pub fn try_from(args: syn::AttributeArgs) -> Result<Self, syn::Error> {
		let mut options = DeriveOptions::new(false, false, ParamStyle::default());
		for arg in args {
			if let syn::NestedMeta::Meta(meta) = arg {
				match meta {
					syn::Meta::Path(ref p) => {
						match p
							.get_ident()
							.ok_or(syn::Error::new_spanned(
								p,
								format!("Expecting identifier `{}` or `{}`", CLIENT_META_WORD, SERVER_META_WORD),
							))?
							.to_string()
							.as_ref()
						{
							CLIENT_META_WORD => options.enable_client = true,
							SERVER_META_WORD => options.enable_server = true,
							_ => {}
						};
					}
					syn::Meta::NameValue(nv) => {
						if path_eq_str(&nv.path, PARAMS_META_KEY) {
							if let syn::Lit::Str(ref lit) = nv.lit {
								options.params_style = ParamStyle::from_str(&lit.value())
									.map_err(|e| syn::Error::new_spanned(nv.clone(), e))?;
							}
						} else {
							return Err(syn::Error::new_spanned(nv, "Unexpected RPC attribute key"));
						}
					}
					_ => return Err(syn::Error::new_spanned(meta, "Unexpected use of RPC attribute macro")),
				}
			}
		}
		if !options.enable_client && !options.enable_server {
			// if nothing provided default to both
			options.enable_client = true;
			options.enable_server = true;
		}
		if options.enable_server && options.params_style == ParamStyle::Named {
			// This is not allowed at this time
			panic!("Server code generation only supports `params = \"positional\"` (default) or `params = \"raw\" at this time.")
		}
		Ok(options)
	}
}
