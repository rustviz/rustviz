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
- Corpus tests run the *installed* plugin (`cargo rv-plugin` on PATH), not
  the source tree. Reinstall after plugin changes:
  `cargo install --locked --path rustviz-plugin`, or the tests silently
  exercise a stale build.

## Keep the limitations lists in sync

What RustViz can't do is documented in three places. A PR that changes what
the plugin supports (fixes a crash, adds a construct, or discovers a new
gap) updates all three in the same PR:

1. `README.md` → **Limitations**, the canonical list.
2. `rustviz-plugin/README.md` → the plugin's TODO and limitations detail.
3. `playground/frontend/src/App.tsx` → the **Known limitations** panel
   students see in the playground.

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
