use std::str::FromStr;

const POSITIONAL: &str = "positional";
const NAMED: &str = "named";
const RAW: &str = "raw";

#[derive(Clone, Debug, PartialEq)]
pub enum ParamStyle {
	Positional,
	Named,
	Raw,
}

impl Default for ParamStyle {
	fn default() -> Self {
		Self::Positional
	}
}

impl FromStr for ParamStyle {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, String> {
		match s {
			POSITIONAL => Ok(Self::Positional),
			NAMED => Ok(Self::Named),
			RAW => Ok(Self::Raw),
			_ => Err(format!(
				"Invalid value for params key. Must be one of [{}, {}, {}]",
				POSITIONAL, NAMED, RAW
			)),
		}
	}
}
