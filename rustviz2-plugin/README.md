# rustviz2-plugin

The compiler-integrated heart of RustViz. A `rustc_private` plugin that
walks HIR/MIR for a single-file crate and emits a code panel + timeline
panel as SVGs on stdout. Built on Will Crichton's
[`rustc_plugin`](https://crates.io/crates/rustc_plugin) /
[`rustc_utils`](https://crates.io/crates/rustc_utils) crates (same
family as Flowistry / Aquascope).

End users typically don't touch this crate directly — they use one of:

- the **`rustviz` CLI** in [`../rustviz2/`](../rustviz2/) (`rustviz svg
  foo.rs`, `rustviz html foo.rs`),
- the **`mdbook-rustviz` preprocessor** in
  [`../mdbook-rustviz/`](../mdbook-rustviz/) (` ```rv ` fenced blocks),
- the **playground** in [`../playground/`](../playground/), or
- the **`rustviz2` Rust library** in [`../rustviz2/`](../rustviz2/).

Each of those routes calls into the binaries this crate produces
(`cargo-rv-plugin`, `rv-plugin-driver`).

![](./src/svg_generator/rv2_example.png)

## Installing

The standard install path is via the `rustviz` CLI's bootstrap, which
also installs the matching nightly toolchain:

```sh
cargo install rustviz2
rustviz init
```

To install from source against a local checkout:

```sh
rustup toolchain install nightly-2025-08-20 \
    --profile minimal \
    --component rust-src,rustc-dev,llvm-tools-preview
cargo install --path . --locked
```

## Limitations

RustViz is a teaching tool — it supports a meaningful subset of Rust,
not all of it. Currently unsupported (or known to misbehave):

- For-loops
- Conditional `let` bindings
- Borrows that occur inside conditionals
- Some borrows over struct members
- Lifetime annotations beyond what the
  [`Excerpt<'a>`](https://github.com/rustviz/tutorial/blob/master/src/structs.md)
  tutorial example covers

### To fix / implement

- [x] Handle owners that are declared inside conditional blocks
- [x] Typecheck function ctxt to determine what type of return annotation to make
- [x] Implement new state calculation system
- [ ] Remove struct members that are not utilized from the timeline
- [ ] Implement hoverable anonymous owner interactions in code panel
- [ ] Weird phantom annotated src bug that seems to appear when there are `\t` characters
- [ ] Add highlighting for passbyref events
- [ ] Implement For-loops (really just desugared match expr)
- [x] last (black) data-hash doesn't render properly
- [x] Fix resource dropping (breaks with conditionals)
- [x] Reference aliasing
- [x] Fix annotated source gen to handle `<` `/` `>` characters
- [ ] Let-if / match expressions (new conditional move event)
- [x] Conditional lifetime logic and visualization
- [x] Bad stuff happens when you don't put a semi-colon at the end of a void stmt at the end of a block
- [ ] Chained method calls (goes hand in hand with anonymous owner interactions, e.g. `.get().get_mut()`)
- [ ] Lifetimes that are 'captured' by conditional statements (use MIR)
- [ ] Struct box kind of buggy
- [ ] JSONify the output
