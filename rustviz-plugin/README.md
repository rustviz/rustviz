# rustviz-plugin

The compiler-integrated heart of [RustViz](https://github.com/rustviz/rustviz).
A `rustc_private` plugin that walks HIR/MIR for a single-file crate and
emits a code panel + timeline panel as SVGs on stdout. Built on Will
Crichton's [`rustc_plugin`](https://crates.io/crates/rustc_plugin) /
[`rustc_utils`](https://crates.io/crates/rustc_utils) crates (same
family as Flowistry / Aquascope).

End users typically don't touch this crate directly — they use one of:

- the **`rustviz` CLI** in [`rustviz-cli`](https://crates.io/crates/rustviz-cli) (`rustviz svg foo.rs`, `rustviz html foo.rs`),
- the **`mdbook-rustviz` preprocessor** in [`mdbook-rustviz`](https://crates.io/crates/mdbook-rustviz) (` ```rv ` fenced blocks),
- the **playground** in [`../playground/`](../playground/), or
- the **`rustviz-lib` Rust library** in [`rustviz-lib`](https://crates.io/crates/rustviz-lib).

Each of those routes calls into the binaries this crate produces
(`cargo-rv-plugin`, `rv-plugin-driver`).

![](./src/svg_generator/rv2_example.png)

## Installing

The standard install path is via the `rustviz` CLI's bootstrap, which
also installs the matching nightly toolchain:

```sh
cargo install rustviz-cli
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

RustViz is a teaching tool — it supports a meaningful subset of Rust;
some features render in a deliberately simplified form, others aren't
yet handled.

**Rendered as opaque single-owner columns** — the wrapper has a column,
the wrapped value doesn't get a separate timeline:

- Smart-pointer wrappers: `Box`, `Rc`, `Arc`, `RefCell`, `Mutex`, and
  trait objects (`Box<dyn T>`). Shared-ownership semantics (refcount
  sharing for `Rc` / `Arc`, runtime borrow tracking for `RefCell`)
  aren't visualized.
- Closures: capture events into the closure binding are visualized;
  events inside the closure body aren't separately traced.

**Rendered as a single iteration:**

- `for` / `while` / `loop` bodies show one execution. Per-iteration
  branching across iterations isn't visualized.

**Not supported:**

- Indexing into collections (`v[i]`). Slicing (`&v[..]` on a `Vec`,
  `&s[..]` on a `String`) does work.
- `async` / `await`, futures, and other heavily-desugared async
  constructs.
- Custom procedural or declarative macros — only standard-library
  macros (`println!`, `vec!`, `assert!`, `?`) are unwrapped through
  their desugaring; user-defined macros are invisible.
- Method chains where each call's return is itself the receiver of the
  next call (`a.get_mut().push(x)`). The first call renders, the rest
  of the chain doesn't yet.
- `unsafe` blocks aren't tested; raw-pointer behavior is undefined for
  the visualization.
- Union types (`union U { … }`) panic the plugin.

**Limited / fragile:**

- Inherent methods on user structs work for the canonical
  Rectangle/area shape but minor variants (e.g. `fn get(&self) -> i32
  { self.n }` on a one-field struct) can crash.
- Range patterns in match arms (`0..=9 => …`) and pattern guards (`pat
  if cond => …`) are walked but not extensively tested.
- `const` and `static` items aren't tested.

### To fix / implement

- [x] Handle owners that are declared inside conditional blocks
- [x] Typecheck function ctxt to determine what type of return annotation to make
- [x] Implement new state calculation system
- [ ] Remove struct members that are not utilized from the timeline
- [ ] Implement hoverable anonymous owner interactions in code panel
- [ ] Weird phantom annotated src bug that seems to appear when there are `\t` characters
- [ ] Add highlighting for passbyref events
- [x] Implement for-loops, while, while-let, loop, if-let — all loop / pattern-binding constructs
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
