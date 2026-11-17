# mdbook-rustviz

An [mdBook](https://rust-lang.github.io/mdBook/) preprocessor that turns
` ```rv ` fenced code blocks into embedded RustViz visualizations.
Compiles each snippet through the rustc plugin at build time, writes the
resulting SVGs into the book's `src/examples/example-<n>/`
(`vis_code.svg`, `vis_timeline.svg`), embeds them in the chapter, and
adds the tooltip JS to the page.

The [RustViz tutorial](https://github.com/rustviz/tutorial) is a
full-scale book built this way;
[`test-book/`](https://github.com/rustviz/rustviz/tree/main/mdbook-rustviz/test-book)
in the repository is a small worked example.

## Setup

The preprocessor needs the rustc plugin (`cargo rv-plugin`) installed
against the matching nightly toolchain. The fastest path is the CLI's
bootstrap:

```sh
cargo install rustviz
rustviz init
```

That installs the `rustviz` binary, then runs the toolchain + plugin
install. Then install the preprocessor itself:

```sh
cargo install mdbook-rustviz
```

It is built against mdBook 0.5.

Working on RustViz itself? See [Building from
source](https://github.com/rustviz/rustviz#building-from-source) in the
main README; it covers using a local build of the plugin and
preprocessor with a book.

## Usage

Enable the preprocessor in your book's `book.toml`:

```toml
[preprocessor.rustviz]
```

Then tag code blocks you'd like visualized with ` ```rv ` instead of
` ```rust ` (set `code-block = "..."` under `[preprocessor.rustviz]` to
use another tag):

````markdown
```rv
fn main() {
    let mut s = String::from("hello");
    s.push_str(" world");
}
```
````

### "Open in Playground" links

Under every figure the preprocessor adds a small right-aligned
"Open in Playground" link. It opens the [RustViz playground](https://rustviz.github.io/)
with that figure's code (the full text of the ` ```rv ` block, including any
`// rustviz: hide` / `skip` markers) loaded and visualized. To point at a
different playground, or to turn the links off with an empty string:

```toml
[preprocessor.rustviz]
playground-url = "https://rustviz.github.io/"   # default; "" disables the links
```

The link has class `rv-playground-link`, so a book's `additional-css` can restyle it.

### Themes

Figures follow the book's theme. Each SVG carries a palette for each of
mdBook's five themes (Light, Rust, Coal, Navy, Ayu), and a small script
the preprocessor adds to every chapter sets the reader's current theme
on each figure, again whenever they switch. A book opened from `file://`
can't reach into its figures, so they stay light (navy if the system
prefers dark). The colours are CSS custom properties (`--rv-*`), selected
by `data-rv-theme` on the SVG's root; see
[`themes.rs`](https://github.com/rustviz/rustviz/blob/main/rustviz-plugin/src/svg_generator/svg_frontend/themes.rs).

### Usage logging (off by default)

The preprocessor adds no analytics or logging of any kind unless you ask for
it. To log how readers use your figures to a collector you run (the RustViz
collector is `playground/events/` in the repository):

```toml
[preprocessor.rustviz]
telemetry-endpoint = "https://events.example.org"   # unset (default): off
telemetry-consent = "opt-in"     # default; or "notice": a banner, recording starts without a click
telemetry-privacy-url = "https://example.org/privacy/"   # linked from the banner
telemetry-surface = "book:example.org"   # default book:<host name>
telemetry-study-text = "…"   # optional: who runs the study, a contact; shown in the banner
```

With an endpoint set, every chapter loads `rv-telemetry.js`: a consent
banner, a "Your data" button (shows the reader's random ID; turns logging
off), and events such as figure views, tooltip hovers and pointer paths
over a figure. Do Not Track and Global Privacy Control turn it off
entirely. Figures get a `data-rv-figure-id`, and `src/examples/rv-figures.json`
maps each figure id to its snippet, so the snippets themselves are never
logged. Without `telemetry-endpoint` none of this is in the output.

### Keeping boilerplate out of the diagram

Two comment markers, written at the end of the line an item starts on, work
in ` ```rv ` blocks (the marker is stripped from the displayed code):

- `// rustviz: skip` on a `let` or `fn` line: the item stays in the source
  but is left out of the trace.
- `// rustviz: hide` removes the whole item from the code panel; the
  following lines move up. Supported items: `fn` (free fns and methods),
  `struct`, `enum`, `union`, `impl` (the whole block; its methods aren't
  traced), `trait`, `use`, `type`, `const` and `static`. For an item with
  attributes, put the marker on the item line or on an attribute line
  (`#[derive(..)]`); the attributes are hidden with it. Calls into a
  hidden fn still draw their arrows, and a hidden struct's fields still get
  columns where an instance is used. On any other line (a `let`, a `mod`)
  `hide` hides nothing.

Run `mdbook build` (or `mdbook serve`) as usual. For verbose logs, set
`RUST_LOG=info`.

By default the preprocessor runs the plugin as a subprocess on the host
(`cargo rv-plugin`, the `local` backend of
[`rustviz-lib`](https://crates.io/crates/rustviz-lib)), which is why the
plugin must be installed. That is fine for your own book, but
**don't** build a book from untrusted sources this way: compiling a
snippet can run arbitrary code (proc-macro expansion). To compile the
snippets in a sandbox instead, set `RV_RUNNER=docker`. That needs Docker
and the `rustviz/rustviz-runner:latest` image (which carries its own
toolchain and plugin, built from the repository's `main`):

```sh
docker pull ghcr.io/rustviz/rustviz-runner:latest
docker tag ghcr.io/rustviz/rustviz-runner:latest rustviz/rustviz-runner:latest
RV_RUNNER=docker mdbook build
```

`RV_RUNNER_IMAGE` names a different image. The docker backend is much
slower (one container per figure).
