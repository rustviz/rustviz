# RustViz: notes for agents and contributors

RustViz draws ownership/borrow timelines for **students learning Rust**. A
tooltip that renders but states something false about Rust is worse than a
crash: students believe it. Check every message you add or change against
real Rust semantics (moves vs copies, references never own, `&T` is `Copy`,
writes through `&mut` mutate the referent, drops happen at scope end).

## Tests must pin correct behaviour, not current output

- Corpus tests live in `rustviz-lib/tests/corpus.rs` (fixtures in
  `rustviz-plugin/tests/*.rs`). When you add an `EXPECTED_TOOLTIPS` entry,
  include `must_not_contain` for the false text the bug produced, and
  confirm the new assertions **fail on the old plugin** before claiming a
  fix. Never copy whatever the plugin prints into `must_contain` without
  reading it as a Rust teacher would.
- If a fixture deliberately pins known-wrong output, say so in a comment
  next to it.
- The Rust Book corpus (`rustviz-lib/tests/book.rs`, snippets in
  `rustviz-plugin/tests/book/`) tracks what main gets right and wrong on
  127 Book-style programs; a fix PR moves its snippets' `known_wrong`
  strings to `must_not_contain` (see `rustviz-plugin/tests/book/README.md`).
- Corpus tests run the *installed* plugin (`cargo rv-plugin` on PATH), not
  the source tree. Reinstall after plugin changes:
  `cargo install --locked --path rustviz-plugin`, or the tests silently
  exercise a stale build.

## Keep the limitations lists in sync

What RustViz can't do is documented in two places. A PR that changes what
the plugin supports (fixes wrong output, adds a construct, or discovers a
new gap) updates both in the same PR:

1. `README.md` → **Limitations**, the canonical list. It describes `main`
   exactly, so each fix PR deletes or rewrites the bullets it fixes.
2. `playground/frontend/src/App.tsx` → the **Known limitations** panel
   students see in the playground (a shorter, student-facing version).

`rustviz-plugin/README.md` only links to the README list.

**Crashes are not listed in either place.** A plugin panic on valid Rust
is tracked only as a GitHub issue labelled `crash` (minimal repro, and the
file:line of the panic). Both lists just link to
[that issue search](https://github.com/rustviz/rustviz/issues?q=is%3Aissue+is%3Aopen+label%3Acrash).
A PR that fixes a crash closes its issue ("Fixes #N") and adds the repro as
a fixture in `rustviz-plugin/tests/` with a corpus entry in
`rustviz-lib/tests/corpus.rs`, so the crash stays fixed. A crash fix
doesn't touch the limitations lists, unless the program now renders with
something wrong; that goes in the lists as usual.

## Bumping the toolchain

The plugin links `rustc_private`, so it only builds on the one nightly that
Will Crichton's `rustc_plugin` / `rustc_utils` (cognitive-engineering-lab)
target. Each release names its nightly in the version, e.g.
`0.15.2-nightly-YYYY-MM-DD`; upstream ships one every ~3-8 months. Find
the newest at <https://crates.io/crates/rustc_plugin/versions> (rustc_utils
releases in lockstep with the same version string).

**Pin sites.** `scripts/bump-nightly.sh <rustc_plugin version>` rewrites
all of them and refreshes `Cargo.lock`; check with `git grep -n <old
nightly>` afterwards. They are: `rust-toolchain.toml` (workspace root,
`rustviz-lib/`, `rustviz-plugin/`; `rustviz-lib` embeds its copy via
`include_str!` and a unit test checks it matches the root one),
`ARG RUST_NIGHTLY` in `playground/runner/Dockerfile` and
`playground/cloudrun/Dockerfile`, the heredoc in
`playground/runner/rv-runner.sh`, the `rustc_utils` / `rustc_plugin`
versions in `rustviz-plugin/Cargo.toml`, and prose in `README.md`,
`rustviz-plugin/README.md` and a doc comment in `rustviz-lib/src/lib.rs`.
(`scripts/bump-version.sh` is for RustViz's own release version and does
not touch the nightly.)

**Porting.** Compiler errors show you the rustc-internal API changes
(the 0.14 -> 0.15 bump in #170 hit 5 sites, e.g. `hir::PatKind::Ref`
gained a pinnedness field, `hir::QPath::LangItem` disappeared). Changes in
`rustc_plugin`'s *own* API can be silent: read its changelog / diff
between the two tags. In 0.15, `cli_main` / `driver_main` started
*returning* an `ExitCode`; our `main` dropped it, so every compile error
and panic exited 0 with empty output. New nightlies also accept new
syntax (e.g. `if let` guards), which the visitor may not handle yet.

**Verifying.**

1. Build the old plugin from `main` and the new one into separate roots
   (`cargo install --locked --path rustviz-plugin --root <dir>`), then
   `PATH=<new>/bin:$PATH cargo test --workspace --locked` (corpus tests
   run the *installed* plugin).
2. Check exit codes: a snippet that doesn't compile must make
   `cargo rv-plugin` exit non-zero with rustc's diagnostic on stderr
   (`compile_errors_are_reported_not_swallowed` covers this).
3. Probe diff: run both builds over a set of Book-style snippets (at
   least `rustviz-plugin/tests/*.rs`; #170 used ~130) and diff the sorted
   `data-tooltip-text` attributes of each timeline SVG. Every changed
   snippet needs an explanation.
4. Browser check: run the playground against the new plugin and render
   every dropdown example plus a failing snippet (the error card must
   show rustc's message, not "Unexpected output format").
5. CI's `runner-image` job builds the runner Dockerfile on native arm64
   for PRs touching it; this is the only place the aarch64 LLVM build is
   exercised (see #182).

## Practicalities

- Every plugin PR appends to the tails of `EXPECTED_OK` /
  `EXPECTED_TOOLTIPS`, so parallel PRs conflict there. Resolve by keeping
  both sides.
- The playground's compile API runs on Google Cloud Run (project
  `rustviz-playground`), deployed by the `deploy-cloudrun` CI job on every
  push to `main`; see `playground/cloudrun/` and `playground/SECURITY.md`.
  Idle cost is $0. `--max-instances` in `playground/cloudrun/deploy.sh` is
  the spend ceiling, so don't raise it casually.
- Per-clone cargo state lives in `.cargo-home/` via direnv (`.envrc`); see
  README "Working from multiple checkouts in parallel".
