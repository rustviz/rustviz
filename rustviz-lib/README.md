# rustviz-lib

The Rust API for [RustViz](https://github.com/rustviz/rustviz). Renders a
single Rust source file through the RustViz rustc plugin and returns the
resulting code-panel + timeline-panel SVGs.

```rust
use rustviz_lib::Rustviz;

let code = r#"
fn main() {
    let s = String::from("hello");
    let t = s;
    println!("{}", t);
}
"#;

let rv = Rustviz::new(code).expect("plugin invocation failed");
std::fs::write("code.svg", rv.code_panel_string())?;
std::fs::write("timeline.svg", rv.timeline_panel_string())?;
```

The plugin runs out-of-process. By default the lib shells out to
`cargo rv-plugin` against a generated tempdir crate (the `local`
backend), which means it needs the matching nightly toolchain + the
plugin installed:

```sh
cargo install rustviz-cli   # bundles the bootstrap helper
rustviz init                # rustup install + cargo install rustviz-plugin
```

For a sandboxed backend appropriate for *untrusted* input (e.g. a public
web playground), set `RV_RUNNER=docker` and ensure the
`rustviz/rustviz-runner` image is on the host. See
[playground/SECURITY.md](https://github.com/rustviz/rustviz/blob/main/playground/SECURITY.md)
for the full sandboxing contract.

If you just want a CLI rather than a programmatic API, install
[`rustviz-cli`](https://crates.io/crates/rustviz-cli) and use
`rustviz svg` / `rustviz html`.
