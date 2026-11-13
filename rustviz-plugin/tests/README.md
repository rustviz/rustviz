# Plugin fixture corpus

This directory holds `.rs` snippets the plugin is run against during
development and CI.

## How the fixtures are run

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
