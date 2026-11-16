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
cargo install rustviz       # bundles the bootstrap helper
rustviz init                # rustup install + cargo install rustviz-plugin
```

**Don't use the `local` backend on untrusted input**: compiling a snippet
can run arbitrary code (proc-macro expansion). For a sandboxed backend
(e.g. a public web playground), set `RV_RUNNER=docker`. That runs the
plugin inside the `rustviz/rustviz-runner:latest` image (override with
`RV_RUNNER_IMAGE`) with no network, a read-only filesystem and capped
memory, CPU, processes and time, so the host needs Docker and the image,
but not the plugin. The image is built from `main` and published on
GHCR:

```sh
docker pull ghcr.io/rustviz/rustviz-runner:latest
docker tag ghcr.io/rustviz/rustviz-runner:latest rustviz/rustviz-runner:latest
```

or build it from a checkout with `docker build -t
rustviz/rustviz-runner:latest -f playground/runner/Dockerfile .` (what
`cargo xtask setup` does). A third backend, `RV_RUNNER=sandbox`, is what
the hosted playground uses on Cloud Run; see the [crate
docs](https://github.com/rustviz/rustviz/blob/main/rustviz-lib/src/lib.rs). See
[playground/SECURITY.md](https://github.com/rustviz/rustviz/blob/main/playground/SECURITY.md)
for the full sandboxing contract.

If you just want a CLI rather than a programmatic API, install
[`rustviz`](https://crates.io/crates/rustviz) and use
`rustviz svg` / `rustviz html`.

Working on RustViz itself? See [Building from
source](https://github.com/rustviz/rustviz#building-from-source) in the
main README.
