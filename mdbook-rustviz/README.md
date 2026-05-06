# mdbook-rustviz

An [mdBook](https://rust-lang.github.io/mdBook/) preprocessor that turns
` ```rv ` fenced code blocks into embedded RustViz visualizations.
Compiles each snippet through the rustc plugin at build time and inlines
the resulting SVGs (plus the tooltip JS) into the rendered HTML.

The [RustViz tutorial](https://github.com/rustviz/tutorial) is a
full-scale book built this way; `test-book/` here is a small
worked example.

## Setup

The preprocessor needs the rustc plugin (`cargo rv-plugin`) installed
against the matching nightly toolchain. The fastest path is the CLI's
bootstrap:

```sh
cargo install rustviz-cli
rustviz init
```

That installs the `rustviz` binary, then runs the toolchain + plugin
install. Then install the preprocessor itself:

```sh
cargo install mdbook-rustviz
```

## Usage

Enable the preprocessor in your book's `book.toml`:

```toml
[preprocessor.rustviz]
```

Then tag code blocks you'd like visualized with ` ```rv ` instead of
` ```rust `:

````markdown
```rv
fn main() {
    let mut s = String::from("hello");
    s.push_str(" world");
}
```
````

Run `mdbook build` (or `mdbook serve`) as usual. For verbose logs, set
`RUST_LOG=info`.

By default the preprocessor invokes the plugin via Docker. Set
`RV_RUNNER=local` to run it as a subprocess on the host instead — much
faster for local iteration; **don't** use `local` against untrusted
input (proc-macro expansion is arbitrary code execution).
