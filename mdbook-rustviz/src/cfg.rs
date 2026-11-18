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

	/// Usage logging (off by default). With a collector origin here, every
	/// chapter loads rv-telemetry.js, which posts to `<endpoint>/v1/events`
	/// under the consent rules below. Empty (the default): no telemetry
	/// script, no ids, nothing sent, and the book's output is unchanged.
	#[serde(alias = "telemetry-endpoint")]
	#[serde(default)]
	pub telemetry_endpoint: String,

	/// `"opt-in"` (default): nothing is recorded until the reader agrees.
	/// `"notice"`: no banner; the book's own text says what is recorded
	/// (link `<privacy-url>#your-data`), and recording starts without a click;
	/// the reader can turn it off in the privacy page's "Your data" section.
	#[serde(alias = "telemetry-consent")]
	#[serde(default = "Cfg::default_telemetry_consent")]
	pub telemetry_consent: String,

	/// Privacy page, linked from the opt-in prompt. Its "Your data" section
	/// (`<privacy-url>#your-data`) shows the reader's ID and turns logging off.
	#[serde(alias = "telemetry-privacy-url")]
	#[serde(default)]
	pub telemetry_privacy_url: String,

	/// The `surface` events carry: `"tutorial"` for the RustViz tutorial,
	/// default `book:<host name>`.
	#[serde(alias = "telemetry-surface")]
	#[serde(default)]
	pub telemetry_surface: String,

	/// `true` (default): a browser that sends Global Privacy Control or Do Not
	/// Track gets no logging at all. `false`: those signals are ignored and
	/// logging stops only when the reader turns it off in the privacy page.
	/// Set `false` only if the book never sells or shares the data and says so.
	#[serde(alias = "telemetry-respect-privacy-signals")]
	#[serde(default = "Cfg::default_true")]
	pub telemetry_respect_privacy_signals: bool,

	/// A sentence about the research study (who runs it, a contact) shown
	/// in the opt-in prompt. Empty: none shown.
	#[serde(alias = "telemetry-study-text")]
	#[serde(default)]
	pub telemetry_study_text: String,
}



impl Cfg {
	#[inline]
	fn default_code_block() -> String { DEFAULT_CODE_BLOCK.into() }
	#[inline]
	fn default_playground_url() -> String { DEFAULT_PLAYGROUND_URL.into() }
	#[inline]
	fn default_true() -> bool { true }
	#[inline]
	fn default_telemetry_consent() -> String { "opt-in".into() }
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



