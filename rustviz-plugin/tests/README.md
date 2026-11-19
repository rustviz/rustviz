# Plugin fixture corpus

This directory holds `.rs` snippets the plugin is run against during
development and CI.

## How the fixtures are run

`corpus_invariants_hold` (in `rustviz-lib/tests/corpus.rs`, checks in
`rustviz-lib/tests/common/invariants.rs`) also checks every fixture,
Book snippet and `known_bugs/` repro against properties every picture
must have (no column used after it ended, no drop where nothing is held,
borrow kinds consistent, ...). `known_bugs/` holds minimal repros of open
bugs, named `<issue>_<what>.rs`; their violations, and today's in the
corpus, are listed in `KNOWN_INVARIANT_FAILURES` by issue. A fix PR
deletes its entries (the test fails while they're listed and no longer
happen).

`rustviz-lib/tests/corpus.rs` renders **every** `.rs` file here (and in
`edition2024/`) through `RV_RUNNER=local` and requires well-formed SVG
(`corpus_all_fixtures_produce_well_formed_svg`). Add a file and it is
run; no list to edit. A fixture that must be skipped goes in
`SKIP_WELL_FORMED` with a reason (a crash also gets a `crash` issue, see
CLAUDE.md).

`EXPECTED_TOOLTIPS` pins behaviour: the listed `data-tooltip-text`
strings must (or must not) appear for the entry. Some fixtures have
no entry and serve only as the no-crash floor, such as the early
per-feature variants (`basic_if*`, `basic_match*`, `basic_ref*`,
`basic_deref*`, `advanced_deref*`), the loop variants (`basic_for*`,
`basic_while`, `loop_body`, ...) and pre-RV2 artifacts (`testBorrow`,
`calcLength`, `box`, ...). They are kept for history and as repro
material; candidates for deletion in a separate cleanup PR.

When adding a new fixture:

* Put a short comment at the top explaining what it covers.
* If the rendering is load-bearing for a specific feature, add a
  `TooltipExpect` entry in `rustviz-lib/tests/corpus.rs` pinning the
  relevant `data-tooltip-text` strings.
