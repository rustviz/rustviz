# Plugin fixture corpus

This directory holds `.rs` snippets the plugin is run against during
development and CI.

## Status (per #140 triage, May 2026)

The `rustviz-lib/tests/corpus.rs` harness drives a curated subset of
the fixtures here through `RV_RUNNER=local`:

* **`EXPECTED_OK`** — regression floor. Plugin must produce
  well-formed SVG for every entry.
* **`EXPECTED_TOOLTIPS`** — behavior pinning. The listed
  `data-tooltip-text` strings must appear for the entry.

Fixtures **not** in `EXPECTED_OK` fall into a few buckets, kept here
intentionally rather than deleted:

* **Per-feature exploration variants** (`basic_if`, `basic_if2` …
  `basic_if12`, `basic_match2`–`basic_match4`, `basic_ref2`–`basic_ref5`,
  `basic_mutref2`, `basic_mutref3`, `basic_deref2`–`basic_deref6`,
  `advanced_deref`–`advanced_deref3`, `basic_alias`, `basic_struct`)
  — early iteration snippets used while landing the conditional /
  reference / struct features. Superseded by curated `if_else_*`,
  `match_*`, `nested_struct_*`, `reborrow`, etc. entries in
  `EXPECTED_OK`. Kept for history and as ad-hoc repro material when
  bisecting; not promoted to the regression floor because they'd
  add redundant coverage. Candidates for future deletion in a
  separate cleanup PR.
* **Loop variants** (`basic_for`, `basic_for2`, `basic_for3`,
  `basic_while`, `while_body`, `while_let_body`,
  `while_let_pattern_annotation`, `for_array_borrow`, `for_range`,
  `loop_body`, `loop_break_value`, `labeled_loop`) — all pass
  end-to-end (no panic) but render only a single iteration of the
  body, per the limitation tracked in #138. They aren't promoted to
  the regression floor because their pedagogical value is limited
  until per-iteration branching is implemented; some may become
  good fixtures once #138 lands.
* **Pre-RV2 artifacts** (`testBorrow`, `testScope`, `testShadow`,
  `returnValues`, `returningOwnership`, `calcLength`, `box`,
  `mutRef`, `mutableReferences`, `shadowing`, `variablesMutability`,
  `copy_owner_tooltip`, `struct_impl`, `loop_break_value`,
  `blocks_basic`, etc.) — predate the current RV2 plugin. Some have
  been promoted (`mutableReferences`, `mv_back`, `mutAssignOp`)
  where they add coverage the curated set misses; others duplicate
  existing entries and are kept only as historical reference.

Counts at last triage: 137 total fixtures, 76 in `EXPECTED_OK` (after
the #140 promotions). 0 panics on any fixture as of the post-triage
fix in `expr_visitor.rs` for the `?` operator's desugar.

When adding a new fixture:

* Put a short comment at the top explaining what it covers.
* Add the stem to `EXPECTED_OK` in `rustviz-lib/tests/corpus.rs`.
* If the rendering is load-bearing for a specific feature, also add
  a `TooltipExpect` entry pinning the relevant `data-tooltip-text`
  strings.
