extern crate semver;
extern crate pulldown_cmark;
extern crate pulldown_cmark_to_cmark;

// mdbook 0.5 split its API into per-purpose crates. Preprocessors
// pull `Book`/`BookItem`/`Chapter` + the `Preprocessor` trait from
// `mdbook-preprocessor`; the markdown helpers (notably
// `new_cmark_parser`) moved into `mdbook-markdown`. Errors surface
// as `anyhow::Error` (re-exported under `errors::Error`).
//
// 0.4's `CmdPreprocessor::parse_input(stdin)` helper was retired:
// in 0.5 `CmdPreprocessor` is the *invocation*-side type (used by
// mdbook to spawn a preprocessor), not the parsing-side helper a
// preprocessor would use to read stdin. The wire format is just a
// JSON-encoded `(PreprocessorContext, Book)` tuple, so we
// deserialize that directly with serde_json.
use mdbook_preprocessor::book::{Book, BookItem, Chapter};
use mdbook_preprocessor::errors::Error;
use mdbook_preprocessor::{Preprocessor, PreprocessorContext};

use crate::Result;
use crate::cfg::Cfg;

use core::str;
use std::path::{Path, PathBuf};
use std::fs;
use rustviz_lib::Rustviz;

/// `<script>…</script>` block carrying the tooltip + cross-panel
/// highlighting glue. The JS itself lives in
/// `rustviz-lib/src/helpers.js` and is shared with the `rustviz` CLI's
/// `--html` mode — keep the SVG schema in sync with both consumers.
fn helpers_script() -> String {
    format!("<script>\n{}\n</script>", rustviz_lib::HELPERS_JS)
}

pub struct RustvizPlugin {
	src_dir: PathBuf
}

impl RustvizPlugin {
	pub fn new(path: &Path) -> RustvizPlugin {
		
		RustvizPlugin {
			src_dir: PathBuf::from(path).join("src"),
		}
	}

	pub fn handle_preprocessing(&self) -> Result {
		use std::io::{stdin, stdout};
		use semver::{Version, VersionReq};

		// Wire format is JSON: `[PreprocessorContext, Book]` — see
		// mdbook-driver/src/builtin_preprocessors/cmd.rs for the
		// matching `serde_json::to_writer(writer, &(ctx, book))`.
		let (ctx, book): (PreprocessorContext, Book) =
			serde_json::from_reader(stdin())?;
		let current = Version::parse(&ctx.mdbook_version)?;
		let built = VersionReq::parse(&format!("~{}", mdbook_preprocessor::MDBOOK_VERSION))?;

		if ctx.mdbook_version != mdbook_preprocessor::MDBOOK_VERSION && !built.matches(&current) {
			warn!(
			      "The {} plugin was built against version {} of mdbook, \
				      but we're being called from version {}, so may be incompatible.",
			      self.name(),
			      mdbook_preprocessor::MDBOOK_VERSION,
			      ctx.mdbook_version
			);
		}
		let processed_book = self.run(&ctx, book)?;
		serde_json::to_writer(stdout(), &processed_book)?;
		Ok(())
	}
}


impl Preprocessor for RustvizPlugin {
	fn name(&self) -> &str { "rustviz" }

	// 0.5 changed the return type from `bool` to `Result<bool>` so
	// preprocessors can fail cleanly during the renderer-compat
	// probe — we don't have a failure mode here, so just wrap.
	fn supports_renderer(&self, renderer: &str) -> mdbook_preprocessor::errors::Result<bool> {
		Ok(renderer != "not-supported")
	}


	fn run(&self, ctx: &PreprocessorContext, mut book: Book) -> Result<Book, Error> {
		use std::fs;
		let assets_dir = self.src_dir.join("examples");
		if let Err(err) = fs::create_dir(self.src_dir.join("examples")){
			warn!("Error creating assets directory: {}", err);
		}
		else {
			info!("Assets dir created successfully.")
		}

		
		let mut ex_counter :u32 = 0;
		
		// 0.5 dropped the `get_preprocessor` shortcut. `Config::get`
		// is now generic and serde-deserializes the value, taking a
		// dotted key path like `preprocessor.<name>`. Same end
		// result, just routed through serde directly.
		let cfg: Cfg = match ctx.config.get::<Cfg>(&format!("preprocessor.{}", self.name())) {
			Ok(Some(c)) => c,
			Ok(None) => Cfg::default(),
			Err(e) => {
				error!("{}", e);
				Cfg::default()
			}
		};

		book.for_each_mut(|item| {
			    if let BookItem::Chapter(chapter) = item {
				    let _ = process_code_blocks(chapter, &cfg, &assets_dir, &mut ex_counter).map(|s| {
					                                              chapter.content = s;
					                                              trace!("chapter '{}' processed", &chapter.name);
				                                              })
				                                              .map_err(|err| {
					                                              error!("I'm blue dabodee {}", err);
				                                              });
			    }
		    });

		// could add some callbacks here to delete the examples directory - not necessary though
		Ok(book)
	}
}




fn html_escape(s: &str) -> String {
	s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Unpadded base64url (RFC 4648 section 5) of `bytes`. The playground's
/// decoder (`playground/frontend/src/linkCode.ts`) must stay in sync.
fn base64url(bytes: &[u8]) -> String {
	const ALPHABET: &[u8; 64] =
		b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
	let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
	for chunk in bytes.chunks(3) {
		let n = (chunk[0] as u32) << 16
			| (*chunk.get(1).unwrap_or(&0) as u32) << 8
			| *chunk.get(2).unwrap_or(&0) as u32;
		for i in 0..=chunk.len() {
			out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
		}
	}
	out
}

/// Right-aligned (to the figure's edge: same -75px side margins as the
/// figure block, so the link lines up with the box above it) small "Open in Playground" link placed under a figure;
/// empty when `playground_url` is empty (links disabled). `code` is the
/// figure's source exactly as written in the block.
fn playground_link(playground_url: &str, code: &str) -> String {
	if playground_url.is_empty() {
		return String::new();
	}
	format!(
		"\n<div class=\"rv-playground-row\" style=\"text-align: right; margin: 0.3em -75px 0 -75px; font-size: 0.85em\">\
		<a class=\"rv-playground-link\" href=\"{}#code={}\" target=\"_blank\" rel=\"noopener\" \
		style=\"color: inherit; opacity: 0.75\">Open in Playground &#8599;</a></div>",
		html_attr_escape(playground_url),
		base64url(code.as_bytes())
	)
}

fn html_attr_escape(s: &str) -> String {
	html_escape(s).replace('"', "&quot;")
}

fn rustviz_handler(code_string: &str, a_dir: &PathBuf,  ex_counter: u32, playground_url: &str) -> String{
	// create new example directory: src/assets/example-x/
	let example_dir_str = format!("example-{}", ex_counter);
	let example_dir = a_dir.join(example_dir_str.clone());

	if let Err(err) = fs::create_dir(example_dir.clone()){
		info!("Error creating example directory: {}", err);
	}  

  match Rustviz::new(code_string) {
    Ok(rv) => {
      // write strings to file
      match fs::write(example_dir.join("vis_code.svg"), rv.code_panel_string()) {
        Ok(_) => {}
        Err(e) => warn!("error writing code panel to file {:#?}", e)
      }

      match fs::write(example_dir.join("vis_timeline.svg"), rv.timeline_panel_string()) {
        Ok(_)  => {}
        Err(e) => warn!("error writing timeline panel to file {:#?}", e)
      }

      info!("successfully created visualization for example {}", ex_counter);
      let visualization_div = format!("<div class=\"flex-container vis_block\" 
      style=\"position:relative; margin-left:-75px; margin-right:-75px; display: flex; flex-direction: row; justify-content: flex-start; flex-wrap: nowrap; flex-shrink: 0; height: {}px\">
      <div style=\"flex: 1 1 auto; min-width: 0; overflow-x: auto; overflow-y: hidden\">
      <object type=\"image/svg+xml\" class=\"{} code_panel\" data=\"examples/{}/vis_code.svg\" style=\"display: block; min-width: 100%\"></object>
      </div>
      <object type=\"image/svg+xml\" class=\"{} tl_panel\" data=\"examples/{}/vis_timeline.svg\" style=\"width: auto; flex-grow: 0; flex-shrink: 0\" onmouseenter=\"helpers('{}')\"></object>
      </div>", rv.height(),example_dir_str, example_dir_str, example_dir_str, example_dir_str, example_dir_str);
    
      format!("{}{}", visualization_div, playground_link(playground_url, code_string))
    }
    Err(e) =>{
      warn!("example {} failed: {}", ex_counter, e);
      warn!("example code {}", code_string);
      // The message is multi-line rustc output full of `<`/`>` (generics,
      // `&str`), so escape it and keep its layout.
      format!(
        "<p><b>Error generating visualization</b></p>\n<pre>{}</pre>",
        html_escape(&e.to_string())
      )
    }
  }
}

fn process_code_blocks(
chapter: &Chapter,
cfg: &Cfg,
assets_dir: &PathBuf,
// pulldown-cmark-to-cmark 11+ widened its error type from
// `std::fmt::Error` to its own enum (so it can surface things like
// "wrote bad reference link" alongside formatter failures). Switch
// the return type accordingly; the caller already prints it via
// `Display`.
ex_counter: &mut u32) -> Result<String, pulldown_cmark_to_cmark::Error> {

	// pulldown-cmark 0.10+ split end-tags into a separate `TagEnd`
	// enum (start-tags carry data like the code-block language;
	// end-tags don't need it). We don't have to re-check the
	// language at the close because our state machine already
	// remembers we're inside an `rv` block.
	use pulldown_cmark::{CodeBlockKind, Event, CowStr, Tag, TagEnd};
	use pulldown_cmark_to_cmark::cmark;

	enum State {
		None,
		Open,
		Closing,
	}

	let mut state = State::None;
	// On narrow screens the -75px side margins would push the link past the
	// viewport edge, so keep it inside the text column there.
	let mut buf = format!(
		"<style>@media (max-width: 640px) {{ .rv-playground-row {{ margin-left: 0 !important; margin-right: 0 !important; }} }}</style>\n{}\n",
		helpers_script()
	); // inject the helpers script directly into the page
	// The curly_quotes setting is left at false so that people can
	// set it in book.toml (mdBook will apply the setting when it
	// parses our output). It is important to use new_cmark_parser so
	// that we parse things like tables consistently with mdBook.
	// Use mdbook-markdown's parser so we stay in lockstep with the
	// pulldown-cmark feature flags mdbook itself enables (tables,
	// footnotes, strikethrough, etc.) — drift here would parse a
	// chapter differently than the renderer that ultimately sees
	// our output. `MarkdownOptions::default()` mirrors mdbook 0.5's
	// own defaults (smart-punctuation, definition-lists, admonitions
	// all on); the prior `false` arg is gone in the 0.5 API.
	let parser = mdbook_markdown::new_cmark_parser(
		&chapter.content,
		&mdbook_markdown::MarkdownOptions::default(),
	);
	// Clippy false-positive issue:
	// https://github.com/rust-lang/rust-clippy/issues/9211#issuecomment-1335173323
	#[allow(clippy::unnecessary_filter_map)]
	let events = parser.filter_map(|e| {
		                use State::*;
		                use CowStr::*;
		                use CodeBlockKind::*;
		                use Tag::{CodeBlock, Paragraph};


						// info!("event {:#?}", e);

		                match (&e, &mut state) {
			                (Event::Start(CodeBlock(Fenced(Borrowed(mark)))), None) if mark == &cfg.code_block => {
			                   state = Open;
			                   Some(Event::Start(Paragraph))
		                   },

		                   (Event::Text(Borrowed(text)), Open) => {
                          state = Closing;
                          let res = rustviz_handler(text, &assets_dir, *ex_counter, &cfg.playground_url);
                          *ex_counter += 1;
                          Some(Event::Html(res.into()))
		                   },

		                   // 0.13: `Event::End` takes `TagEnd::CodeBlock`
		                   // (no language payload — the language was on
		                   // the matching `Tag::CodeBlock` start). We're
		                   // already gated by the `Closing` state set
		                   // when our `rv` opener fired, so the bare
		                   // `TagEnd::CodeBlock` is enough.
		                   (Event::End(TagEnd::CodeBlock), Closing) => {
                          state = None;
                          Some(Event::End(TagEnd::Paragraph))
		                   },
		                   _ => Some(e),
		                }
	                });
	cmark(events, &mut buf).map(|_| buf)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::cfg::DEFAULT_PLAYGROUND_URL;

	#[test]
	fn base64url_matches_known_vectors() {
		assert_eq!(base64url(b""), "");
		assert_eq!(base64url(b"a"), "YQ");
		assert_eq!(base64url(b"ab"), "YWI");
		assert_eq!(base64url(b"abc"), "YWJj");
		// bytes that are `+` and `/` in standard base64
		assert_eq!(base64url(&[0xfb, 0xff, 0xbf]), "-_-_");
		assert_eq!(base64url("héllo".as_bytes()), "aMOpbGxv");
	}

	#[test]
	fn link_encodes_source_verbatim() {
		let code = "// rustviz: hide\nlet s = \"h\\\"é\";\n";
		let html = playground_link("https://example.org/pg/", code);
		let expect = format!("href=\"https://example.org/pg/#code={}\"", base64url(code.as_bytes()));
		assert!(html.contains(&expect), "{}", html);
		assert!(html.contains("class=\"rv-playground-link\""));
		assert!(html.contains("rel=\"noopener\""));
	}

	#[test]
	fn empty_url_disables_link() {
		assert_eq!(playground_link("", "fn main() {}"), "");
	}

	#[test]
	fn cfg_defaults_and_override() {
		assert_eq!(Cfg::default().playground_url, DEFAULT_PLAYGROUND_URL);
		let t: toml::value::Table = toml::from_str("playground-url = \"\"").unwrap();
		assert_eq!(Cfg::try_from(t).unwrap().playground_url, "");
	}
}
