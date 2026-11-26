# RustViz 3 issue tests

One case per open `bug` issue that has a runnable repro: what the RustViz 3
fact layer must say about the program, as **facts and states, not tooltip
wording** (RustViz 3 rewords tooltips). The harness is
`rustviz-lib/tests/rv3_issues.rs`.

A case is two files:

- `<issue>_<slug>.rs`: the repro, exactly as in the issue where possible
  (rows are its line numbers). It must compile as an edition-2024 lib on the
  pinned nightly. Don't add a header comment: it would shift the rows.
- `<issue>_<slug>.expect.toml`: the assertions, described below.

## `expect.toml`

```toml
issue = 1675
cluster = "C05"                # from maintainer-tools g4/BUG-CLUSTERS.md
title = "one line: what 2.x gets wrong"
fn = "main"                    # body the rows belong to (default main); per assertion: fn = ".."
places = ["p.a"]               # field paths the oracle should probe besides variables (optional)
xfail = "P2"                   # RustViz 3 doesn't pass it yet: the PLAN.md phase that should (optional)
duplicate_of = 1653            # optional
note = "..."                   # optional
beyond_facts = "P3: the join dot has no title"   # optional: the part of the issue facts can't show, and its phase

[[state]]                      # the place's state after a row, i.e. as the next row sees it
row = 6
place = "x"
is = "own"                     # see the states table
src = "issue"
why = "..."                    # optional, recommended: the rustc check that proves it

[[lent]]                       # a loan restricts the place after the row
row = 8
place = "x"
kind = "shared"                # shared | mut | unique | exclusive (= mut or unique); optional
holder = "c"                   # the reference/variable holding the loan; optional; "_" = any
absent = false                 # true: no such loan
src = "issue"

[[loan]]                       # a borrow taken on `row`
row = 4
lender = "x"                   # the borrowed place as written: x, p.a, *b, v[_]
kind = "shared"                # optional
holder = "c"                   # optional; must be among the loan's holders
ends = [5, 9]                  # optional: exactly these end rows, one per path's end
ends_include = [5]             # optional
ends_exclude = [8]             # optional
absent = false                 # true: no such loan
src = "issue"

[[drop]]                       # a destructor runs (or not) for the place on the row
row = 10
place = "x"
how = "definite"               # definite | conditional (drop flag) | rest (fields left) | none | any
absent = false                 # true: no such drop on this row
src = "issue"

[[event]]                      # an event on the row (or its absence)
row = 6
ev = "move s -> v"
absent = false
src = "issue"

[[order]]                      # events on one row, in this evaluation order (a subsequence)
row = 5
events = ["end x -> c", "borrow-mut x"]
src = "issue"
```

**States** (`is`): `own` (holds its value, nothing borrows it), `shared`
(holds it, only shared loans), `mut` (a mutable or unique loan),
`moved`, `maybe-moved` (moved on some paths), `partly` (some fields moved),
`unassigned` (no value on any path), `maybe-unassigned` (assigned on some
paths only), `absent` (no column/state). The oracle writes `moved*`,
`partly*` and `unassigned*`: rustc says "moved" for a definite and a maybe
move alike, and E0381's wording ("isn't initialized" / "possibly-uninitialized")
doesn't follow the paths, so these accept either.

**Events** (`ev`): `<kind> <place>[ -> <target>]`, `_` matches anything.
Kinds: `init`, `reassign`, `move`, `copy` (place `-> to`), `borrow`,
`borrow-shared`, `borrow-mut` (lender `-> holder`), `end` (a loan of the
lender ends; `-> holder`), `repoint` (the reference), `drop`, `scope-end`,
`tempdrop` (place: text in the temporary's description, or `_`).

**Rows.** "After row L" is the state the next row of that path sees (where
rustc's probe `let _ = &x;` inserted after L sees it). A row with no MIR
location (`} else {`, blank) carries the row above. A drop the fact layer
gives the `}` span but that only an early exit reaches counts on the exit's
row. When rows have several paths (an `if` row), state the path the issue
is about with a row inside that arm, or use `[[loan]] ends` (one end per path).

**Sources** (`src`). `oracle`: derived by rustc probes, written only by the
generator into the block at the end of the file. `issue`: hand-written from
the issue and checked by you against rustc (a probe, a compile error, a
Drop-printing type for drop timing, `-Zdump-mir` / the fact dump for loan
ends). Write `issue` assertions only for what the oracle can't decide, or
for the exact point the issue is about (repeat it even if the oracle has it:
it names the bug).

## The generated block

```
RV3_ISSUES_GEN=1 RV3_ISSUES_ONLY=1675_ cargo test -p rustviz-lib --test rv3_issues rv3_issue_cases_gen -- --nocapture
```

probes `&x` / `&mut x` after every statement for every variable bound once
in its function (plus `places`), compiles with the pinned nightly, and
rewrites everything below the `# ---- generated ...` marker. It prints
`CONTRADICTED` for a hand-written state rustc disagrees with: fix the case.
`rv3_issue_cases_well_formed` (always run) fails when the block is stale.

## Running against the fact layer

`rv3_issue_cases_against_facts` runs each program through the installed
plugin with `RV3_FACTS=<file>` (or reads `$RV3_FACTS_DIR/<case>.facts.json`;
`RV3_FACTS_SAVE=<dir>` keeps the dumps) and prints `N of M issue tests
pass`, per source and per `xfail` phase. A plugin that writes no dump (2.x)
skips it. With `RV3_ISSUES_STRICT=1` it fails on a case that fails without
`xfail`, or passes with one (delete the `xfail`). The dump's shape is read
by `load_facts` (the one adapter point): RustViz 3's
`{"version": 1, "bodies": [..]}` and the spike's bare array.
