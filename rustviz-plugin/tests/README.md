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
  end-to-end (no panic). A loop body is drawn once, as straight-line
  code, which is intended: RustViz visualizes what the borrow checker
  sees, and that doesn't depend on how many times the body runs
  (see #138, closed). Not promoted because the curated
  `cond_with_for_loop`, `if_inside_for` and `if_let_inside_for`
  entries already keep loops in the regression floor.
* **Pre-RV2 artifacts** (`testBorrow`, `testScope`, `testShadow`,
  `returnValues`, `returningOwnership`, `calcLength`, `box`,
  `mutRef`, `shadowing`, `variablesMutability`, `copy_owner_tooltip`,
  `struct_impl`, `blocks_basic`, etc.) — predate the current RV2
  plugin. Some have been promoted (`mutableReferences`, `mv_back`,
  `mutAssignOp`) where they add coverage the curated set misses; others duplicate
  existing entries and are kept only as historical reference.
* **Pending deletion** (`hatra2`) — an orphan RV1 fixture that the
  limitations refresh (#130) deletes, so it isn't promoted here.

Not yet sorted into a bucket: `anonymous_copy2caller`,
`anonymous_fnparam`, `basic_match`, `deref_basic`,
`if_borrow_in_body`, `if_let_inside_body`, `macro_rhs_acquire_line`,
`match_arm_with_let`. #140 tracks finishing that.

Counts at last triage: 138 total fixtures, 69 in `EXPECTED_OK` (after
the #140 promotions). 0 panics on any fixture as of the `?` operator
fix in `expr_visitor.rs` (`match_rhs` now unwraps the
`MatchSource::TryDesugar` match to its operand).

When adding a new fixture:

* Put a short comment at the top explaining what it covers.
* Add the stem to `EXPECTED_OK` in `rustviz-lib/tests/corpus.rs`.
* If the rendering is load-bearing for a specific feature, also add
  a `TooltipExpect` entry pinning the relevant `data-tooltip-text`
  strings.
