# RustViz 3 issue tests

Cases made from open `bug` issues that have a runnable repro (the goal is
one or more per such issue; see maintainer-tools `docs/rustviz3/ISSUE-TESTS.md`
for coverage): what the RustViz 3 fact layer must say about the program, as **facts and states, not tooltip
wording** (RustViz 3 rewords tooltips). The harness is
`rustviz-lib/tests/rv3_issues.rs`.

A case is two files:

- `<issue>_<slug>.rs`: the repro, exactly as in the issue where possible
  (rows are its line numbers). It must compile as an edition-2024 lib on the
  pinned nightly. Don't add a header comment: it would shift the rows. If
  the repro needs a change to mean what the issue says (a branch that can
  never run, a loop that always runs), make it without moving rows and say
  so in `note`.
- `<issue>_<slug>.expect.toml`: the assertions, described below.

## `expect.toml`

```toml
issue = 1675
cluster = "C05"                # from maintainer-tools g4/BUG-CLUSTERS.md
title = "one line: what 2.x gets wrong"
fn = "main"                    # body the rows belong to (default main; a method is `Type::name`); per assertion: fn = ".."
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
place = "x"                    # a column: a variable or field path (`a`, not `*a`)
kind = "shared"                # shared | mut | unique | exclusive (= mut or unique); optional
holder = "c"                   # the reference/variable holding the loan; optional; "_" = any
lender = "*a"                  # optional: the loan's borrowed place (a reborrow through a)
absent = false                 # true: no such loan
src = "issue"

[[loan]]                       # a borrow taken on `row`
row = 4
lender = "x"                   # the borrowed place as written: x, p.a, *b, v[_]
kind = "shared"                # optional: shared | mut | unique | exclusive
holder = "c"                   # optional; must be among the loan's holders
ends = [[5, 6], 9]             # optional: exactly these ends, one per CFG edge that ends the loan:
                               #   [5, 6] = on row 5, on the path that next runs row 6 (any row: a loop's
                               #   back edge goes up); 9 = on row 9 (any edge). The loan record's
                               #   end rows must be the same set.
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
(holds it, only shared loans), `mut` (a mutable loan or a closure's unique
immutable one: nothing else may use it), `moved`, `maybe-moved` (moved on
some paths; the dump's `maybe` counts only if something moves the place or
a parent in the body), `partly` (some fields moved), `unassigned` (no value
on any path), `maybe-unassigned` (assigned on some paths only; the dump's
`maybe` counts only if nothing moves the place), `absent` (no state). The oracle writes `moved*`,
`partly*` and `unassigned*`: rustc says "moved" for a definite and a maybe
move alike, and E0381's wording ("isn't initialized" / "possibly-uninitialized")
doesn't follow the paths, so these accept either.

**Kinds.** The dump's spellings are normalised (`mut(two-phase)` is
`mut`); `unique` (a closure's unique immutable capture) is not `mut`.
`exclusive` matches either. A row's `lent` entry takes its kind from the
loan's record. A two-phase loan counts as exclusive (state `mut`) only
from its activation row (`activation_line` in the loan record); before
that, or when the dump doesn't say, it is a reservation and counts as
shared.

**Events** (`ev`): `<kind> <place>[ -> <target>]`, `_` matches anything.
Kinds: `init`, `reassign`, `move`, `copy` (place `-> to`), `borrow`,
`borrow-shared`, `borrow-mut`, `borrow-unique` (lender `-> holder`), `end`
(a loan of the lender ends; `-> holder`), `repoint` (the reference), `drop`
(a destructor runs: not a `Drop` with nothing to drop), `scope-end`,
`tempdrop` (place: text in the temporary's description, or `_`). A target
matches any one of a fact's holders (`r|h`: a copied reference).

**Rows.** "After row L" is the state the next row of that path sees (where
rustc's probe `let _ = &x;` inserted after L sees it), as the dump gives it
for row L. A row the dump gives no state is "no state": the harness never
borrows a state from a textually earlier row (it can be another path's: a
loop body, a sibling arm). A drop the dump marks `early_exit = N` (MIR span
`}`, run only by the exit on row N) counts on row N and not on the `}` row;
one marked `also_on_exits` counts on its row and on each exit's row. An
`[[order]]` must hold on one path through the row: events outside any arm
plus one arm's (and its parents'), by the dump's `arm` index. When
a row has several paths (an `if` row), state the path the issue is about
with a row inside that arm, or use `[[loan]] ends` with `[row, next]`
edges. In an edge, `next` is the first row that path runs after the end;
rows holding only braces / `else` in between may be skipped, so `[3, 7]`
also matches an end whose dump says the path continues at a `}` on row 6.

**Sources** (`src`). `oracle`: derived by rustc probes, written only by the
generator into the block at the end of the file. `issue`: hand-written from
the issue and checked against rustc (a probe, a compile error, a run with a
Drop-printing type for drop timing, `--emit=mir` for moves and drops).
Give the check in `why` (not every case has one yet; new ones should). Write `issue` assertions only for what the oracle can't decide, or
for the exact point the issue is about (repeat it even if the oracle has it:
it names the bug).

**`[[oracle_skip]]`** (`place`, optional `row` / `fn`, `why`) leaves
matching generated states unchecked, for a rustc verdict that isn't what
the program does (borrowck's view of an unreachable or-pattern
alternative). Pin what MIR does with `issue` assertions next to it.

**Producer errors.** A dump whose top-level `errors` isn't empty is a read
failure: the case fails in strict mode whatever its `xfail`.

## The generated block

```
RV3_ISSUES_GEN=1 RV3_ISSUES_ONLY=1675_ cargo test -p rustviz-lib --test rv3_issues rv3_issue_cases_gen -- --nocapture
```

probes `&x` / `&mut x` at the oracle's probe points (after a statement
that is followed by another statement or its block's `}` on a later line;
not inside closures, call arguments or tail expressions, nor in dead code)
for every variable bound once in its function whose name doesn't start
with `_` (plus `places`), compiles with the pinned nightly, and rewrites
everything below the `# ---- generated ...` marker. `own` means the
`&mut x` probe raised no borrow conflict and no missing value (E0596, x
not declared `mut`, is ignored). It prints
`CONTRADICTED` for a hand-written state rustc disagrees with: fix the case.
If rustc or a probe fails on a case that already has states, the old block
is kept and the run fails. A method's states are named `Type::name`, a trait
impl's `<Type as Trait>::name`, a module's fn `module::name`.
`rv3_issue_cases_well_formed` (always run) fails when the block is stale
(the program, `places` or the toolchain changed) or has a malformed line.

## Running against the fact layer

`rv3_issue_cases_against_facts` runs each program through the installed
plugin with `RV3_FACTS=<file>` (or reads `$RV3_FACTS_DIR/<case>.facts.json`;
`RV3_FACTS_SAVE=<dir>` keeps the dumps) and prints `N of M issue tests
pass`, per source and per `xfail` phase. A plugin that writes no dump (2.x)
skips it. With `RV3_ISSUES_STRICT=1` it fails on a case that fails without
`xfail`, or passes with one (delete the `xfail`), on any case whose facts
couldn't be read (a missing `RV3_FACTS_DIR` or dump file, a plugin failure:
`xfail` doesn't cover those), and when no case was checked. The dump's shape is read
by `load_facts` (the one adapter point): RustViz 3's
`{"version": 1, "bodies": [..]}` and the spike's bare array.
