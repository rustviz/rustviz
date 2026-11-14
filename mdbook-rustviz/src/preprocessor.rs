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
				    if chapter.content.contains("rustviz") || (cfg.aquascope_tabs && chapter.content.contains("```aquascope")) {
					    let (s, _) = add_rustviz_blocks(&chapter.content, &assets_dir, &mut ex_counter, &cfg.playground_url, cfg.aquascope_tabs);
					    chapter.content = s;
				    }
				    // Only round-trip chapters that actually contain `rv` blocks.
				    if !chapter.content.contains(&format!("```{}", cfg.code_block)) {
					    return;
				    }
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

// ---------------------------------------------------------------------
// RustViz next to Aquascope, chosen per block by fence flags.
//
//   ```aquascope,interpreter,rustviz          tabs, Aquascope shown first
//   ```aquascope,interpreter,rustviz-default  tabs, RustViz shown first
//   ```aquascope,interpreter,rustviz-only     RustViz replaces Aquascope
//   ```rust,rustviz / rustviz-default / rustviz-only   same for listings
//   (no flag)                                 untouched: identical to upstream
//
// Prototype-only: `rustviz-combo-hover`, `rustviz-combo-rows` (combined
// views, see assets/viz.js).
//
// Text-level pass (like mdbook-aquascope's own parser): only the flagged
// fences are rewritten, the rest of the chapter is left byte-for-byte.
// The ```aquascope fence (minus our flags) is kept for mdbook-aquascope,
// which must run *after* this preprocessor.
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RvMode { Tabs { rv_first: bool }, Only, ComboHover, ComboRows }

pub fn rv_mode(flag: &str) -> Option<RvMode> {
	match flag {
		"rustviz" => Some(RvMode::Tabs { rv_first: false }),
		"rustviz-default" => Some(RvMode::Tabs { rv_first: true }),
		"rustviz-only" => Some(RvMode::Only),
		"rustviz-combo-hover" => Some(RvMode::ComboHover),
		"rustviz-combo-rows" => Some(RvMode::ComboRows),
		_ => None,
	}
}

/// Aquascope source -> plain Rust: `#`-prefixed lines are shown (they
/// are only hidden in Aquascope's view), `\#` is a literal `#`, and the
/// `` `[..]` ``, `` `(..)` ``, `` `{..}` `` markers are removed. Line
/// numbers are preserved (the combined views rely on that). When the
/// block hides a `fn main() {` ... `}` wrapper, the body is indented.
pub fn aquascope_to_rust(body: &str) -> String {
	let mut out: Vec<(bool, String)> = Vec::new();
	for line in body.trim().lines() {
		if let Some(rest) = line.strip_prefix('#') {
			out.push((true, rest.to_string()));
		} else if let Some(rest) = line.strip_prefix("\\#") {
			out.push((false, format!("#{}", rest)));
		} else {
			out.push((false, strip_markers(line)));
		}
	}
	let open = out.iter().position(|(h, l)| *h && l.trim() == "fn main() {");
	let close = out.iter().rposition(|(h, l)| *h && l.trim() == "}");
	if let (Some(o), Some(c)) = (open, close) {
		if o < c {
			for (_, l) in out[o + 1..c].iter_mut() {
				if !l.trim().is_empty() {
					*l = format!("    {}", l);
				}
			}
		}
	}
	let mut s = out.into_iter().map(|(_, l)| l).collect::<Vec<_>>().join("\n");
	s.push('\n');
	s
}

/// rustdoc-style source (```rust blocks, `{{#rustdoc_include}}` output)
/// -> the full program: `# ` / `#` hidden lines are shown, attributes
/// (`#[..]`, `#![..]`) are left alone.
pub fn rustdoc_to_rust(body: &str) -> String {
	let mut s = body
		.lines()
		.map(|l| {
			let t = l.trim_start();
			let indent = &l[..l.len() - t.len()];
			if t == "#" {
				String::new()
			} else if let Some(rest) = t.strip_prefix("# ") {
				format!("{}{}", indent, rest)
			} else {
				l.to_string()
			}
		})
		.collect::<Vec<_>>()
		.join("\n");
	s.push('\n');
	s
}

fn strip_markers(line: &str) -> String {
	let mut s = line.to_string();
	'outer: loop {
		for (open, close) in [("`[", "]`"), ("`(", ")`"), ("`{", "}`")] {
			if let Some(i) = s.find(open) {
				let after = &s[i + open.len()..];
				let closer = close.chars().next().unwrap();
				if let Some(j) = after.find(close) {
					if !after[..j].contains(closer) {
						s.replace_range(i..i + open.len() + j + close.len(), "");
						continue 'outer;
					}
				}
			}
		}
		break;
	}
	s
}

const VIZ_CSS: &str = include_str!("assets/viz.css");
const VIZ_JS: &str = include_str!("assets/viz.js");

struct Fence<'a> {
	/// byte range of the whole block (opening line .. end of closing line)
	start: usize,
	end: usize,
	lang: &'a str,
	flags: Vec<&'a str>,
	body: &'a str,
	/// raw text of the opening fence line, without the newline
	open_line: &'a str,
	commented: bool,
}

/// Line-based scan for ```aquascope / ```rust fences.
fn scan_fences(content: &str) -> Vec<Fence<'_>> {
	let mut out = Vec::new();
	let mut pos = 0;
	let mut open: Option<(usize, usize, &str)> = None; // (start, body_start, open_line)
	for line in content.split_inclusive('\n') {
		let t = line.trim_end_matches(['\n', '\r']);
		let ts = t.trim_start();
		match open {
			None => {
				// `<!-- ```aquascope` (commented-out blocks) are still fences for
				// mdbook-aquascope's text parser; recognise them so we skip them.
				let (commented, f) = match ts.strip_prefix("<!--") {
					Some(r) => (true, r.trim_start()),
					None => (false, ts),
				};
				if let Some(info) = f.strip_prefix("```") {
					if info.starts_with("aquascope") || info.starts_with("rust") {
						open = Some((pos, pos + line.len(), t));
						let _ = commented;
					}
				}
			}
			Some((start, body_start, open_line)) => {
				if ts.starts_with("```") {
					let info = open_line.trim_start();
					let commented = info.starts_with("<!--");
					let info = info.trim_start_matches("<!--").trim_start().trim_start_matches("```");
					let mut parts = info.split(',').map(|p| p.trim());
					let lang = parts.next().unwrap_or("");
					out.push(Fence {
						start,
						end: pos + line.len(),
						lang,
						flags: parts.collect(),
						body: &content[body_start..pos],
						open_line,
						commented,
					});
					open = None;
				}
			}
		}
		pos += line.len();
	}
	out
}

/// Rewrites the flagged fences. Returns the new chapter text and whether
/// any visualization was added (so the caller can inject the assets once).
pub fn add_rustviz_blocks(content: &str, assets_dir: &PathBuf, ex_counter: &mut u32, playground_url: &str, all_aquascope: bool) -> (String, bool) {
	let mut out = String::with_capacity(content.len());
	let mut last = 0;
	let mut added = false;
	for f in scan_fences(content) {
		if f.commented || !(f.lang == "aquascope" || f.lang == "rust") {
			continue;
		}
		let mut mode = None;
		let mut kept: Vec<&str> = Vec::new();
		for fl in &f.flags {
			match rv_mode(fl) {
				Some(m) => mode = Some(m),
				None => kept.push(fl),
			}
		}
		let should_fail = kept.contains(&"shouldFail") || kept.contains(&"does_not_compile");
		if mode.is_none() && all_aquascope && f.lang == "aquascope" && !should_fail {
			mode = Some(RvMode::Tabs { rv_first: false });
		}
		let Some(mode) = mode else { continue };
		let is_aq = f.lang == "aquascope";
		if !is_aq && matches!(mode, RvMode::ComboHover | RvMode::ComboRows) {
			warn!("rustviz-combo-* needs an ```aquascope,interpreter block; ignored");
			continue;
		}
		// The original block with our flags removed, so mdbook-aquascope /
		// mdBook see exactly what upstream would have written.
		let indent = &f.open_line[..f.open_line.len() - f.open_line.trim_start().len()];
		let header = std::iter::once(f.lang).chain(kept.iter().copied()).collect::<Vec<_>>().join(",");
		let close_line = content[f.start..f.end].trim_end().rsplit('\n').next().unwrap_or("```");
		let original = format!("{}```{}\n{}{}\n", indent, header, f.body, close_line.trim_start());
		let code = if is_aq { aquascope_to_rust(f.body) } else { rustdoc_to_rust(f.body) };
		let name = format!("example-{}", *ex_counter);
		*ex_counter += 1;
		let fig = match rustviz_figure(&code, assets_dir, &name, playground_url) {
			Ok(fig) => fig,
			Err(e) => {
				warn!("{}: block not drawn by RustViz: {}", name, e.lines().next().unwrap_or(""));
				continue; // leave the block exactly as written
			}
		};
		out.push_str(&content[last..f.start]);
		let other = if is_aq { "Aquascope" } else { "Code" };
		let html = match mode {
			RvMode::Only => format!("<div class=\"rv-viz rv-viz-single\" data-rv=\"{name}\">\n{fig}\n</div>\n"),
			RvMode::Tabs { rv_first } => {
				let (a, b) = if rv_first { (("RustViz", None), (other, Some(&original))) } else { ((other, Some(&original)), ("RustViz", None)) };
				let pane = |i: usize, p: &(&str, Option<&String>)| -> String {
					let hidden = if i == 0 { "false" } else { "true" };
					match p.1 {
						Some(orig) => format!("<div class=\"rv-viz-pane\" role=\"tabpanel\" data-pane=\"{i}\" aria-hidden=\"{hidden}\">\n\n{orig}\n</div>\n"),
						None => format!("<div class=\"rv-viz-pane\" role=\"tabpanel\" data-pane=\"{i}\" aria-hidden=\"{hidden}\">\n{fig}\n</div>\n"),
					}
				};
				format!(
					"<div class=\"rv-viz rv-viz-tabs\" data-rv=\"{name}\">\n<div class=\"rv-viz-bar\" role=\"tablist\">\
					<button type=\"button\" class=\"rv-viz-tab\" role=\"tab\" aria-selected=\"true\" data-pane=\"0\">{}</button>\
					<button type=\"button\" class=\"rv-viz-tab\" role=\"tab\" aria-selected=\"false\" data-pane=\"1\">{}</button></div>\n\
					<div class=\"rv-viz-panes\">\n{}{}</div>\n</div>\n",
					a.0, b.0, pane(0, &a), pane(1, &b)
				)
			}
			RvMode::ComboHover | RvMode::ComboRows => {
				let kind = if mode == RvMode::ComboHover { "hover" } else { "rows" };
				format!(
					"<div class=\"rv-viz rv-combo rv-combo-{kind}\" data-rv=\"{name}\" data-combo=\"{kind}\">\n\
					<div class=\"rv-combo-rv\">\n{fig}\n</div>\n\
					<div class=\"rv-combo-aq\">\n\n{original}\n</div>\n</div>\n"
				)
			}
		};
		out.push_str(&html);
		added = true;
		last = f.end;
	}
	out.push_str(&content[last..]);
	if added {
		// process_code_blocks injects helpers.js itself when the chapter
		// also has `rv` blocks; a second copy would redeclare `const SVG`.
		let helpers = if out.contains("```rv") { String::new() } else { helpers_script() };
		(format!("<style>\n{}\n</style>\n<script>\n{}\n</script>\n{}\n\n{}", VIZ_CSS, VIZ_JS, helpers, out), true)
	} else {
		(out, false)
	}
}

/// The RustViz figure (code panel + timeline) sized to the text column;
/// reports failure instead of rendering an error box.
fn rustviz_figure(code: &str, a_dir: &PathBuf, name: &str, playground_url: &str) -> std::result::Result<String, String> {
	let dir = a_dir.join(name);
	let _ = fs::create_dir_all(&dir);
	let rv = Rustviz::new(code).map_err(|e| e.to_string())?;
	fs::write(dir.join("vis_code.svg"), rv.code_panel_string()).map_err(|e| e.to_string())?;
	fs::write(dir.join("vis_timeline.svg"), rv.timeline_panel_string()).map_err(|e| e.to_string())?;
	let link = if playground_url.is_empty() {
		String::new()
	} else {
		format!(
			"<a class=\"rv-playground-link\" href=\"{}#code={}\" target=\"_blank\" rel=\"noopener\">Open in Playground &#8599;</a>",
			html_attr_escape(playground_url),
			base64url(code.as_bytes())
		)
	};
	Ok(format!(
		"<div class=\"rv-fig\" style=\"height: {h}px\">\
<div class=\"rv-fig-code\"><object type=\"image/svg+xml\" class=\"{n} code_panel\" data=\"examples/{n}/vis_code.svg\"></object></div>\
<object type=\"image/svg+xml\" class=\"{n} tl_panel\" data=\"examples/{n}/vis_timeline.svg\" onmouseenter=\"helpers('{n}')\"></object>\
</div><div class=\"rv-fig-foot\">{link}</div>",
		h = rv.height(), n = name, link = link
	))
}

#[cfg(test)]
mod aq_tests {
	use super::*;

	#[test]
	fn strips_aquascope_markers() {
		let src = "#fn main() {\nlet x = 1;`(focus,paths:x)`\n`[]`let y = &x;`{}`\n#}";
		assert_eq!(aquascope_to_rust(src), "fn main() {\n    let x = 1;\n    let y = &x;\n}\n");
	}

	#[test]
	fn rustdoc_hidden_lines_shown_attributes_kept() {
		let src = "# #[derive(Debug)]\n#[derive(Clone)]\n# fn main() {\n    let x = 1;\n# }";
		assert_eq!(rustdoc_to_rust(src), "#[derive(Debug)]\n#[derive(Clone)]\nfn main() {\n    let x = 1;\n}\n");
	}

	#[test]
	fn modes() {
		assert_eq!(rv_mode("rustviz"), Some(RvMode::Tabs { rv_first: false }));
		assert_eq!(rv_mode("rustviz-default"), Some(RvMode::Tabs { rv_first: true }));
		assert_eq!(rv_mode("rustviz-only"), Some(RvMode::Only));
		assert_eq!(rv_mode("horizontal"), None);
	}

	#[test]
	fn unflagged_blocks_untouched() {
		let ch = "text\n```aquascope,interpreter\n#fn main() {\nlet x = 1;`[]`\n#}\n```\n\n```rust\nfn main() {}\n```\n";
		let (out, added) = add_rustviz_blocks(ch, &PathBuf::from("/nonexistent"), &mut 0, "", false);
		assert!(!added);
		assert_eq!(out, ch);
	}

	#[test]
	fn scan_finds_flags_and_bodies() {
		let ch = "a\n```aquascope,interpreter,rustviz\nlet x = 1;`[]`\n```\n<!-- ```aquascope,interpreter\nx\n``` -->\n";
		let fs = scan_fences(ch);
		assert_eq!(fs.len(), 2);
		assert_eq!(fs[0].flags, vec!["interpreter", "rustviz"]);
		assert_eq!(fs[0].body, "let x = 1;`[]`\n");
		assert!(fs[1].commented);
	}
}
