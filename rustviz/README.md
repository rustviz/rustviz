# rustviz

[RustViz](https://github.com/rustviz/rustviz) draws ownership and borrow
timelines for Rust programs, aimed at students learning Rust.

```sh
cargo install rustviz
rustviz init                # one-time: pinned nightly toolchain + rustc plugin

rustviz svg foo.rs          # writes foo.code.svg + foo.timeline.svg
rustviz html foo.rs         # writes foo.html (both SVGs + tooltip JS inlined)
```

As a library, `rustviz` re-exports [`rustviz-lib`](https://crates.io/crates/rustviz-lib):

```rust
let rv = rustviz::Rustviz::new(code)?;
std::fs::write("code.svg", rv.code_panel_string())?;
std::fs::write("timeline.svg", rv.timeline_panel_string())?;
```

This crate supersedes the unrelated 2022 `rustviz` 0.1.0 (RustViz 1) and
the `rustviz-cli` crate (still published; same binary). The pieces are
[`rustviz-lib`](https://crates.io/crates/rustviz-lib),
[`rustviz-cli`](https://crates.io/crates/rustviz-cli),
[`rustviz-plugin`](https://crates.io/crates/rustviz-plugin) and
[`mdbook-rustviz`](https://crates.io/crates/mdbook-rustviz). See the
[repository](https://github.com/rustviz/rustviz) for documentation,
limitations and the web playground.

MIT licensed.
