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
use rustviz2::Rustviz;

/// `<script>…</script>` block carrying the tooltip + cross-panel
/// highlighting glue. The JS itself lives in
/// `rustviz2/src/helpers.js` and is shared with the `rustviz` CLI's
/// `--html` mode — keep the SVG schema in sync with both consumers.
fn helpers_script() -> String {
    format!("<script>\n{}\n</script>", rustviz2::HELPERS_JS)
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

	#[allow(dead_code)]
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




fn rustviz_handler(code_string: &str, a_dir: &PathBuf,  ex_counter: u32) -> String{
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
      <object type=\"image/svg+xml\" class=\"{} code_panel\" data=\"examples/{}/vis_code.svg\" style=\"flex-grow: 1\"></object>
      <object type=\"image/svg+xml\" class=\"{} tl_panel\" data=\"examples/{}/vis_timeline.svg\" style=\"width: auto; flex-grow: 0\" onmouseenter=\"helpers('{}')\"></object>
      </div>", rv.height(),example_dir_str, example_dir_str, example_dir_str, example_dir_str, example_dir_str);
    
      visualization_div
    }
    Err(e) =>{
      warn!("example {} failed with status: {:#?}", ex_counter, e);
      warn!("example code {}", code_string);
      format!("<p><b>Error generating visualization</b> {}</p>", e)
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
	let mut buf = format!("{}\n", helpers_script()); // inject the helpers script directly into the page
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
                          let res = rustviz_handler(text, &assets_dir, *ex_counter);
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
