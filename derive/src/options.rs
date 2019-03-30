use proc_macro::TokenStream;

#[derive(Debug)]
pub struct DeriveOptions {
	pub enable_client: bool,
	pub enable_server: bool,
}

impl DeriveOptions {
	pub fn new(enable_client: bool, enable_server: bool) -> Self {
		DeriveOptions {
			enable_client,
			enable_server,
		}
	}

	pub fn try_from(tokens: TokenStream) -> Result<Self, syn::Error> {
		if tokens.is_empty() {
			return Ok(Self::new(true, true));
		}
		let ident: syn::Ident = syn::parse::<syn::Ident>(tokens)?;
		let options = {
			let ident = ident.to_string();
			if ident == "client" {
				Some(Self::new(true, false))
			} else if ident == "server" {
				Some(Self::new(false, true))
			} else {
				None
			}
		};
		match options {
			Some(options) => Ok(options),
			None => Err(syn::Error::new(ident.span(), "Unknown attribute.")),
		}
	}
}
