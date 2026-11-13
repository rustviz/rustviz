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
  Cargo resolves `cargo rv-plugin` from `$CARGO_HOME/bin` *before* `PATH`,
  so with the per-clone `.envrc` (`CARGO_HOME=.cargo-home`) a plugin
  installed there shadows one you put on `PATH`. To test a specific build,
  also use a `CARGO_HOME` whose `bin` has no plugin (or `--root` into it),
  and check `cargo rv-plugin --version` / `which`.

## Known gaps are issues, not README text

The README doesn't list limitations (maintainer decision, 2026-11-07); its
Limitations section only links to the issue searches. What RustViz can't
draw correctly yet is tracked as GitHub issues labelled `limitation`
([search](https://github.com/rustviz/rustviz/issues?q=is%3Aissue+is%3Aopen+label%3Alimitation)):

- A PR that fixes a gap closes its issue ("Fixes #N"); one that fixes
  only part of it says "Part of #N" and leaves the issue open.
- A newly found gap (wrong or missing output that isn't a crash) is filed
  as an issue labelled `limitation`: minimal repro, current output,
  expected output. Don't add limitation text to `README.md`.
- `playground/frontend/src/App.tsx` → the **Known limitations** panel
  students see in the playground (a short, student-facing list) is still
  kept in sync with what `main` gets wrong, in the same PR, for now; its
  future is decided at release prep.

`rustviz-plugin/README.md` only links to the issue searches.

**Crashes are tracked only as issues labelled `crash`**, not `limitation`,
and are listed in neither the README nor the playground panel. A plugin
panic on valid Rust gets an issue labelled `crash` (minimal repro, and
the file:line of the panic); the README and the panel just link to
[that issue search](https://github.com/rustviz/rustviz/issues?q=is%3Aissue+is%3Aopen+label%3Acrash).
A PR that fixes a crash closes its issue ("Fixes #N") and adds the repro as
a fixture in `rustviz-plugin/tests/` with a corpus entry in
`rustviz-lib/tests/corpus.rs`, so the crash stays fixed. If the program
then renders with something wrong, that's a new `limitation` issue (and a
panel entry, if students would hit it).

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
   `CARGO_HOME=$(mktemp -d) PATH=<new>/bin:$PATH cargo test --workspace
   --locked` (corpus tests run the *installed* plugin, and `$CARGO_HOME/bin`
   beats `PATH`; see the note under "Tests must pin correct behaviour").
2. Check exit codes: a snippet that doesn't compile must make
   `cargo rv-plugin` exit non-zero with rustc's diagnostic on stderr
   (`compile_errors_are_reported_not_swallowed` covers this).
3. Probe diff: run both builds over a set of Book-style snippets (at
   least `rustviz-plugin/tests/*.rs`; #170 used ~130) and diff the sorted
   `data-tooltip-text` attributes of each timeline SVG. Every changed
   snippet needs an explanation. `scripts/probe-diff.sh <old-root>
   <new-root>` does this for every `rustviz-plugin/tests/**/*.rs` fixture
   and Book snippet: it compares the sorted tooltips, a hash of each
   timeline's geometry and the code panel, and any new failure, prints
   the snippets that differ and exits 1. A refactor that must not change
   output passes when it prints `identical`.
4. Browser check: run the playground against the new plugin and render
   every dropdown example plus a failing snippet (the error card must
   show rustc's message, not "Unexpected output format").
5. CI's `runner-image` job builds the runner Dockerfile on native arm64
   for PRs touching it; this is the only place the aarch64 LLVM build is
   exercised (see #182). Its amd64 leg also renders one program through
   the docker backend (`RV_RUNNER=docker`) with the image it built.

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
