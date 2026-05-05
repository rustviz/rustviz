# RustViz

**RustViz** generates interactive timeline visualizations of ownership and
borrowing for short Rust programs. It is meant as a teaching aid: paste a
snippet and see exactly when each binding becomes the resource owner, when
references go in and out of scope, and which lines those events correspond
to.

This repository is now the compiler-integrated rewrite. Earlier
versions of RustViz, deployed in the classroom and described in [our VL/HCC 2022 paper](https://web.eecs.umich.edu/~comar/rustviz-vlhcc22.pdf), read hand-annotated source; this one plugs into `rustc`
directly and walks HIR/MIR, so the diagram reflects the real borrow
checker's view of the program rather than a hand-curated approximation.

> Looking for the original (annotation-based) RustViz? It lives on
> the [`rv1-final`](https://github.com/rustviz/rustviz/tree/rv1-final)
> branch and tag in this repo, with a matching
> [GitHub Release](https://github.com/rustviz/rustviz/releases/tag/rv1-final).

RustViz is a project of the [Future of Programming Lab](https://fplab.mplse.org/)
at the University of Michigan.

> **Try it live:** <https://rustviz.github.io/>

![screenshot placeholder](rustviz2-plugin/src/svg_generator/rv2_example.png)

---

## Local setup

The CLI, mdbook preprocessor, and library all share a runtime
dependency on the rustc plugin built from this workspace, which links
against `rustc_private` and so requires a specific nightly toolchain.
The fastest way to get a working install is:

```sh
cargo install rustviz2     # gets the lib + the `rustviz` CLI binary
rustviz init               # installs nightly-2025-08-20 + the plugin
```

`rustviz init` runs the underlying `rustup toolchain install` + `cargo
install` against the canonical RustViz repo; pass `--dry-run` to see
exactly what it would do, or `--plugin-git` / `--plugin-rev` to install
from a fork. See `rustviz init --help` for the full flag list.

**Building from this checkout instead** (e.g. you're contributing):

```sh
git clone https://github.com/rustviz/rustviz
cd rustviz
./setup.sh                 # toolchain + plugin install + frontend build + runner image
```

`./setup.sh` is the canonical bootstrap for working *on* RustViz; it
sets up everything you need to run any of the four entry points below
plus the playground's React frontend. To undo it, run `./uninstall.sh`
(see `--help` for what it touches and what it leaves alone — by
default it spares the rustup toolchain and the cargo `target/` tree).

---

## Four ways to use RustViz

### 1. The playground

The hosted playground lets you paste a snippet into a CodeMirror editor and
get back the visualization with no local install. Available at
<https://rustviz.github.io/>; the SPA loads from GitHub Pages
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
instructions and `mdbook-rustviz/test-book/` for a small worked
example. The hands-on Rust tutorial at
<https://github.com/rustviz/tutorial> (deployed at
<https://rustviz.github.io/tutorial/>) is a full-scale example built
this way.

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

## Repository layout

The workspace is organized around the four entry points above. Each
top-level directory:

| Path                  | Contents |
|-----------------------|----------|
| **`rustviz2-plugin/`**| The rustc plugin — the heart of the project. Walks HIR/MIR for a single-file crate and emits a code-panel + timeline-panel SVG on stdout. Built on Will Crichton's `rustc_plugin` / `rustc_utils` crates (same family as Flowistry / Aquascope). Produces the `cargo-rv-plugin` and `rv-plugin-driver` binaries. Pinned to nightly-2025-08-20 via `rust-toolchain.toml`. |
| **`rustviz2/`**       | User-facing Rust library + the `rustviz` CLI. The library exposes `Rustviz::new(code)` which shells out to the plugin; the CLI (`rustviz svg`, `rustviz html`, `rustviz init`) wraps it with file I/O and self-contained-HTML output. The shared tooltip JS for hover behavior also lives here, exported as `rustviz2::HELPERS_JS`. |
| **`mdbook-rustviz/`** | An [mdBook](https://rust-lang.github.io/mdBook/) preprocessor. Replaces ` ```rv ` fenced code blocks with embedded RustViz SVGs at build time. Includes a `test-book/` worked example. The full hands-on tutorial that uses it is in a [separate repo](https://github.com/rustviz/tutorial). |
| **`playground/`**     | The web playground: Actix-web backend + a Vite/React/CodeMirror frontend. Hosted at <https://rustviz.github.io/> with the compile API at <https://rustviz-playground.fly.dev/>. The same binary works as an all-in-one server for local dev. The per-request Docker sandbox image, deploy artifacts, and security threat model all live alongside it under `playground/`. |
| `setup.sh`            | One-shot dev bootstrap: installs the toolchain, builds the plugin, builds the frontend, builds the runner image. Run once after cloning. |
| `uninstall.sh`        | Reverse of `setup.sh`. Removes the cargo-installed binaries + the local docker runner image + frontend artifacts; spares the rustup toolchain and `target/` by default (override with `--toolchain` / `--target` / `--everything`). |

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
checklist are in [`playground/SECURITY.md`](playground/SECURITY.md). Report findings to
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
