# RustViz

**RustViz** generates interactive timeline visualizations of ownership and
borrowing for short Rust programs. It is meant as a teaching aid: paste a
snippet and see exactly when each binding becomes the resource owner, when
references go in and out of scope, and which lines those events correspond
to.

This repository ("RustViz 2") is the compiler-integrated rewrite. Earlier
versions of RustViz read hand-annotated source; this one plugs into `rustc`
directly and walks HIR/MIR, so the diagram reflects the real borrow
checker's view of the program rather than a hand-curated approximation.

RustViz is a project of the [Future of Programming Lab](https://fplab.mplse.org/)
at the University of Michigan.

> **Try it live:** <https://rustviz.github.io/playground/>

![screenshot placeholder](rustviz2-plugin/src/svg_generator/rv2_example.png)

---

## Four ways to use RustViz

### 1. The playground

The hosted playground lets you paste a snippet into a CodeMirror editor and
get back the visualization with no local install. Available at
<https://rustviz.github.io/playground/>; the SPA loads from GitHub Pages
and the compile API is on Fly.io. You can also run the same playground
locally — it's the [`playground/`](playground/) crate in this workspace.

See [`playground/README.md`](playground/README.md) for the local quick-start
and the operational notes for the production deploy.

### 2. The mdbook preprocessor

Embed RustViz visualizations directly in an [mdBook](https://rust-lang.github.io/mdBook/)
by tagging code blocks with ` ```rv ` instead of ` ```rust `. The
preprocessor compiles each snippet through the plugin at build time and
inlines the resulting SVGs into the rendered HTML, with tooltip glue for
hover-driven exploration.

```toml
# book.toml
[preprocessor.rustviz]
```

````markdown
```rv
fn main() {
    let s = String::from("hello");
    println!("{}", s);
}
```
````

See [`mdbook-rustviz/README.md`](mdbook-rustviz/README.md) for setup
instructions and the bundled `test-book/` for a small worked example.
The hands-on Rust tutorial at <https://github.com/rustviz/tutorial>
(deployed at <https://rustviz.github.io/tutorial/>) is a full-scale
example built this way.

### 3. The command-line interface

For one-shot rendering of a single `.rs` file:

```sh
cargo install rustviz2          # gets the lib + the `rustviz` binary
rustviz init                    # one-time: install nightly + plugin

rustviz svg foo.rs              # writes foo.code.svg + foo.timeline.svg
rustviz html foo.rs             # writes one self-contained HTML page
```

`svg` is for embedding into your own HTML/Markdown workflow. `html`
produces a single self-contained file with both SVGs inlined and the
tooltip JS embedded — opens in any browser, no server, no external
assets. See `rustviz init --help` for the bootstrap flags.

### 4. The Rust library

For programmatic SVG generation (e.g. wiring RustViz into your own
authoring pipeline), the same `rustviz2` crate exposes a small Rust API:

```rust
use rustviz2::Rustviz;

let rv = Rustviz::new(code)?;     // calls the plugin under the hood
fs::write("code.svg", rv.code_panel_string())?;
fs::write("timeline.svg", rv.timeline_panel_string())?;
```

A runnable example lives at
[`rustviz2/examples/render_to_files.rs`](rustviz2/examples/render_to_files.rs).
The crate's [API docs](rustviz2/src/lib.rs) cover the backend split
(local subprocess vs. sandboxed Docker — `local` is the default for
library use; the playground binary opts into `docker` for untrusted
input).

---

## Architecture at a glance

```
              GET /  (CDN, instant)
   browser ─────────────────────▶  GitHub Pages
                                   rustviz.github.io/playground/
                                   (Vite SPA, ex-assets)

              POST /submit-code (cold start ~10s
                                  after Fly auto-stop,
                                  cached afterward)
   browser ─────────────────────▶  playground (Actix-web on Fly)
                                          │
                                          │ docker run --network=none --read-only …
                                          ▼
                                   ┌────────────────────────┐
                                   │  rustviz-runner image  │  ephemeral container per request
                                   │  (nightly + plugin)    │  tmpfs /work, capped CPU/RAM/PIDs
                                   └──────────┬─────────────┘
                                              │ cargo rv-plugin
                                              ▼
                                   ┌────────────────────────┐
                                   │   rustviz2-plugin      │  rustc plugin: walks HIR/MIR,
                                   │   (rustc_private)      │  emits two SVGs on stdout
                                   └────────────────────────┘
```

Workspace members:

| Crate                   | Role |
|-------------------------|------|
| **`rustviz2-plugin`**   | The rustc plugin. Built on Will Crichton's `rustc_plugin`/`rustc_utils` crates (same family as Flowistry/Aquascope). Provides `cargo rv-plugin`. |
| **`rustviz2`**          | User-facing library + the `rustviz` CLI. `Rustviz::new(code)` runs the plugin against `code` and returns the two rendered SVGs; the binary wraps that with file I/O and an HTML output mode. |
| **`mdbook-rustviz`**    | mdbook preprocessor that turns ` ```rv ``` ` fenced blocks into embedded SVGs. |
| **`playground`**        | Actix-web playground: serves the React/CodeMirror SPA and exposes `POST /submit-code`. |

---

## Limitations

RustViz 2 is a research tool. It supports a meaningful subset of Rust but
not all of it. Currently unsupported (or known to misbehave):

- For-loops
- Conditional `let` bindings
- Borrows that occur inside conditionals
- Lifetime annotations beyond the simple cases the
  [`Excerpt<'a>`](https://github.com/rustviz/tutorial/blob/master/src/structs.md)
  tutorial example covers
- Some borrows over struct members

The plugin has a TODO list with more detail in
[`rustviz2-plugin/README.md`](rustviz2-plugin/README.md).

---

## Security

The playground compiles untrusted Rust source. Proc-macro expansion in user
code is arbitrary code execution, so the playground runs the plugin inside
a sandboxed container by default. The full threat model and operator
checklist are in [`SECURITY.md`](SECURITY.md). Report findings to
`comar@umich.edu`.

---

## Contributing

Issues and PRs welcome. Keep each PR focused on a single concern; for
local-dev setup run `./setup.sh` then `cargo build --workspace --locked`.

## License

[MIT](LICENSE).

## Citing

If you use RustViz in academic work, please cite the
[VL/HCC 2022 paper](https://web.eecs.umich.edu/~comar/rustviz-vlhcc22.pdf).
