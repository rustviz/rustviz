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
cargo install rustviz
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
  `hide` hides nothing, and the plugin prints a warning on its stderr.

Run `mdbook build` (or `mdbook serve`) as usual. For verbose logs, set
`RUST_LOG=info`.

By default the preprocessor invokes the plugin via Docker. Set
`RV_RUNNER=local` to run it as a subprocess on the host instead — much
faster for local iteration; **don't** use `local` against untrusted
input (proc-macro expansion is arbitrary code execution).
