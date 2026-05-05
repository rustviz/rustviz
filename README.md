# RustViz 2

**RustViz** generates interactive timeline visualizations of ownership and
borrowing for short Rust programs. It is meant as a teaching aid: paste a
snippet, see exactly when each binding becomes the resource owner, when
references go in and out of scope, and which lines those events correspond
to.

RustViz 2 is the compiler-integrated rewrite of the project. Earlier RustViz
read hand-annotated source; RustViz 2 plugs into `rustc` directly and walks
HIR/MIR, so the diagram reflects the real borrow checker's view of your
program rather than a hand-curated approximation.

RustViz is a project of the [Future of Programming Lab](http://fplab.mplse.org/)
at the University of Michigan.

> **Try it live:** <https://rustviz.github.io/playground/>
> (compile API at <https://rustviz-playground.fly.dev/>)

![screenshot placeholder](rustviz2-plugin/src/svg_generator/rv2_example.png)

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

The frontend is a static Vite bundle, hosted on GitHub Pages, so the page
loads instantly even when no one has visited recently. The compile API on
Fly is allowed to auto-stop and cold-start; that latency only shows up
after the user clicks "Generate Visualization", where a couple-second
delay is expected. CORS in `playground/src/main.rs` allows the Pages origin
to call `/submit-code`.

The same `playground` binary still serves the SPA + API from a single origin
in the all-in-one Fly deploy (and in local development), so neither
hosting mode is special-cased in the application code.

Workspace members:

| Crate                    | Role |
|--------------------------|------|
| **`rustviz2-plugin`**    | The rustc plugin. Built on Will Crichton's `rustc_plugin`/`rustc_utils` crates (same family as Flowistry/Aquascope). Provides `cargo rv-plugin`. |
| **`rustviz2`**           | Thin user-facing library. `Rustviz::new(code)` runs the plugin against `code` (in a sandboxed Docker container by default) and returns the rendered code-panel and timeline-panel SVGs. |
| **`mdbook-rustviz`**     | mdbook preprocessor that turns ` ```rv ``` ` fenced blocks into embedded SVGs. |
| **`playground`**           | Actix-web playground: serves the React/CodeMirror SPA and exposes `POST /submit-code`. |

For the playground's quick-start, deploy procedure, and Fly.io
operational notes, see [`playground/README.md`](playground/README.md).
The mdbook preprocessor's setup lives in
[`mdbook-rustviz/README.md`](mdbook-rustviz/README.md).

---

## Limitations

RustViz 2 is a research tool. It supports a meaningful subset of Rust but
not all of it. Currently unsupported (or known to misbehave):

- For-loops
- Conditional `let` bindings
- Borrows that occur inside conditionals
- Chained method calls (`x.get().get_mut()`)
- Lifetime annotations
- Borrows over struct members

The plugin has a TODO list with more detail in
[`rustviz2-plugin/README.md`](rustviz2-plugin/README.md).

---

## Security

The playground compiles untrusted Rust source. Proc-macro expansion in user
code is arbitrary code execution, so the plugin always runs inside a
sandboxed container. The full threat model and the operator checklist are
in [`SECURITY.md`](SECURITY.md). Report findings to `comar@umich.edu`.

---

## Contributing

Issues and PRs welcome. The project follows standard GitHub flow; keep
each PR focused on a single concern, and run `./setup.sh` plus
`cargo build --workspace --locked` before opening one.

---

## License

[MIT](LICENSE).

## Citing

If you use RustViz in academic work, please cite the
[VL/HCC 2022 paper](https://web.eecs.umich.edu/~comar/rustviz-vlhcc22.pdf).
