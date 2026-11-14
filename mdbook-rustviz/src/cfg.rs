use serde::Deserialize;
use toml::value::{Value, Table};


pub const DEFAULT_CODE_BLOCK: &str = "rv";
pub const DEFAULT_PLAYGROUND_URL: &str = "https://rustviz.github.io/";


#[derive(Deserialize)]
pub struct Cfg {
	#[serde(alias = "code-block")]
	#[serde(default = "Cfg::default_code_block")]
	pub code_block: String,

	/// Playground that the "Open in Playground" link under each figure
	/// points at. The empty string disables the links.
	#[serde(alias = "playground-url")]
	#[serde(default = "Cfg::default_playground_url")]
	pub playground_url: String,
}



impl Cfg {
	#[inline]
	fn default_code_block() -> String { DEFAULT_CODE_BLOCK.into() }
	#[inline]
	fn default_playground_url() -> String { DEFAULT_PLAYGROUND_URL.into() }
}

impl Default for Cfg {
	// This using defaults defined above for serde, just to not repeat.
	fn default() -> Self {
		Value::from(Table::new()).try_into()
		                         .expect("empty table with serde-defined default values")
	}
}

impl TryFrom<Table> for Cfg {
	type Error = toml::de::Error;

	fn try_from(map: Table) -> Result<Self, Self::Error> {
		let value: Value = map.into();
		value.try_into()
	}
}

impl TryFrom<&'_ Table> for Cfg {
	type Error = toml::de::Error;

	fn try_from(map: &'_ Table) -> Result<Self, Self::Error> {
		let value: Value = map.to_owned().into();
		value.try_into()
	}
}



