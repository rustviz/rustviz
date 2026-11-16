# rustviz-cli

Command-line interface for [RustViz](https://github.com/rustviz/rustviz).
Renders a single Rust source file to a code-panel + timeline-panel SVG
pair, or to a single self-contained HTML page.

```sh
cargo install rustviz             # (`cargo install rustviz-cli` installs the same binary)
rustviz init                       # one-time: nightly toolchain + plugin

rustviz svg foo.rs                 # writes foo.code.svg + foo.timeline.svg
rustviz html foo.rs                # writes foo.html (both SVGs + tooltip JS inlined)
```

## Subcommands

| Command | What it writes |
|---|---|
| `rustviz svg <file>`  | Two SVGs side-by-side (`<stem>.code.svg`, `<stem>.timeline.svg`) for embedding into your own HTML/Markdown workflow. Written next to the input unless you pass `-o <DIR>`. |
| `rustviz html <file>` | One self-contained HTML page with both SVGs + the tooltip JS embedded. Opens in any browser; no server, no external assets. `-o <FILE>` sets the output path (default `<stem>.html` next to the input), `--title` the page title. |
| `rustviz init`        | Installs the pinned nightly toolchain + the rustc plugin (`rustviz-plugin`). Run once after `cargo install rustviz`. The plugin comes from the git tag matching this CLI's version (e.g. `v2.1.0`). Pass `--dry-run` to preview, `--plugin-rev` to install a different branch / tag / commit, `--plugin-git` to install from a fork (its default branch unless you also pass `--plugin-rev`), or `--skip-toolchain` / `--skip-plugin` to run one step only. |

## Library use

If you need programmatic access to the SVG generation rather than a
one-shot CLI, the underlying API lives in
[`rustviz-lib`](https://crates.io/crates/rustviz-lib):

```rust
use rustviz_lib::Rustviz;

let rv = Rustviz::new(code)?;
std::fs::write("code.svg", rv.code_panel_string())?;
std::fs::write("timeline.svg", rv.timeline_panel_string())?;
```

## Why two install steps?

The actual visualization work happens inside a `rustc_private` plugin
(`rustviz-plugin`), which has to be built against the exact nightly
toolchain it was developed against. `rustviz init` does that bootstrap
in one go: `rustup toolchain install nightly-2026-05-01 --profile minimal
--component rust-src,rustc-dev,llvm-tools-preview` followed by `cargo
+nightly-2026-05-01 install --git https://github.com/rustviz/rustviz
--tag v<version> --locked rustviz-plugin`, where `v<version>` is this
CLI's own release tag (e.g. `v2.1.0`), so the plugin matches the CLI;
`--plugin-rev` overrides it. After that the `rustviz` binary uses the matching
nightly automatically.

Working on RustViz itself? See [Building from
source](https://github.com/rustviz/rustviz#building-from-source) in the
main README.
