# rustviz-cli

Command-line interface for [RustViz](https://github.com/rustviz/rustviz).
Renders a single Rust source file to a code-panel + timeline-panel SVG
pair, or to a single self-contained HTML page.

```sh
cargo install rustviz-cli
rustviz init                       # one-time: nightly toolchain + plugin

rustviz svg foo.rs                 # writes foo.code.svg + foo.timeline.svg
rustviz html foo.rs                # writes foo.html (both SVGs + tooltip JS inlined)
```

## Subcommands

| Command | What it writes |
|---|---|
| `rustviz svg <file>`  | Two SVGs side-by-side (`<stem>.code.svg`, `<stem>.timeline.svg`) for embedding into your own HTML/Markdown workflow. |
| `rustviz html <file>` | One self-contained HTML page with both SVGs + the tooltip JS embedded. Opens in any browser; no server, no external assets. |
| `rustviz init`        | Installs the pinned nightly toolchain + the rustc plugin (`rustviz-plugin`). Run once after `cargo install rustviz-cli`. Pass `--dry-run` to preview, or `--plugin-git` / `--plugin-rev` to install from a fork. |

## Library use

If you need programmatic access to the SVG generation rather than a
one-shot CLI, the underlying API lives in
[`rustviz-lib`](https://crates.io/crates/rustviz-lib):

```rust
use rustviz_lib::Rustviz;

let rv = Rustviz::new(code)?;
fs::write("code.svg", rv.code_panel_string())?;
fs::write("timeline.svg", rv.timeline_panel_string())?;
```

## Why two install steps?

The actual visualization work happens inside a `rustc_private` plugin
(`rustviz-plugin`), which has to be built against the exact nightly
toolchain it was developed against. `rustviz init` does that bootstrap
in one go: `rustup toolchain install nightly-… --component …` followed
by `cargo install --locked rustviz-plugin`. After that the `rustviz`
binary uses the matching nightly automatically.
