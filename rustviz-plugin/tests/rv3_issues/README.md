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
why = "..."                    # required on every hand-written assertion: the rustc check that proves it

[[lent]]                       # a loan restricts the place after the row
row = 8
place = "a"                    # a column: a variable or field path (`a`, not `*a`)
kind = "shared"                # shared | reserved | mut | unique | exclusive (= mut or unique); optional
holder = "c"                   # the reference/variable holding the loan; optional; "_" = any
lender = "*a"                  # optional: the loan's borrowed place (a reborrow through a)
absent = false                 # true: no such loan
src = "issue"
why = "Borrowck rejects a write through a (E0506) while c, a shared reborrow of *a, is still used; reads through a stay allowed."

[[holds]]                      # a variable's value keeps a loan borrowed after the row, as the
                               #   borrow checker counts it (v4 dumps; can be more than what it
                               #   points to: `h.r = &b` leaves `a`'s loan in `h`)
row = 6
place = "c"                    # a variable column (a reference, or a struct holding one: `h`, not `h.r`)
lender = "y"                   # the loan's borrowed place as the dump names it; "_" = any
absent = false                 # true: holds no such loan (a dead or moved-out reference holds none)
src = "issue"

[[loan]]                       # a borrow taken on `row` (a loan of rustc's borrow set: not the dump's
                               #   v3 `source` "caller" / "static" / "untracked" loans)
row = 4
lender = "x"                   # the borrowed place as written: x, p.a, *b, v[_]
kind = "shared"                # optional: shared | mut | unique | exclusive
holder = "c"                   # optional; must be among the loan's holders
ends = [[5, 6], 9]             # optional: exactly these ends, one per CFG edge that ends the loan:
                               #   [5, 6] = on row 5, on the path that next runs row 6 (any row: a loop's
                               #   back edge goes up); 9 = on row 9 (any edge); [4, 5, 16] = also the column the path continues at (the dump's `to_col`). Ends on one row pair with different columns are different ends. The loan record's
                               #   end rows must be the same set.
ends_include = [5]             # optional
ends_exclude = [8]             # optional
col = 5                        # optional: the source column (1-based) the dump records for the borrow
absent = false                 # true: no such loan
src = "issue"
why = "The taken arm replaces c on row 6. Only the other path still needs the old loan at the read on row 9."

[[drop]]                       # a destructor runs (or not) for the place on the row
row = 10
place = "x"
how = "definite"               # definite | conditional (drop flag) | rest (fields left) | none | any
absent = false                 # true: no such drop on this row
src = "issue"
why = "Both successors reach the MIR drop of the initialized x."

[[event]]                      # an event on the row (or its absence)
row = 6
ev = "move s -> v"
absent = false
src = "issue"
why = "MIR moves the String s into v."

[[order]]                      # events on one row, in this evaluation order (a subsequence)
row = 5
events = ["end x -> c", "borrow-mut x"]
src = "issue"
why = "Taking the new mutable loan on this path requires the old loan to have ended."
```

**States.** `own` means a whole, initialized value with no live loans.
`shared` permits shared reads (shared loans or two-phase reservations).
`mut` requires an active mutable loan or a closure's unique immutable loan.
`moved` and `unassigned` apply on every path, `maybe-moved` and
`maybe-unassigned` on some paths that run: the producer's row-local `cause`
for that place says which (`moved` or `unassigned`), and a move on a path
that returned before the join cannot supply it. `maybe-imaginary` means borrowck counts the place as
possibly moved, but only on an edge that never runs, such as an or-pattern's
unreachable alternative: the dump's cause must be `imaginary`. `partly`
records a field move and `absent` means the place has no state on this row.
The oracle writes `moved*`, `partly*` and `unassigned*` because E0382 and
E0381 alone do not distinguish a definite move or missing assignment from a
possible one.

**Kinds.** `[[loan]]` checks the kind at issuance. `[[lent]]` checks the
phase on that row. Version 2 emits `reserved` until a two-phase mutable
loan activates. A spike loan record identifies the two-phase borrow, but
cannot establish its row phase. In that case a phase-dependent assertion
needs more evidence. `exclusive` matches an active `mut` or `unique` loan.

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
current dumps give each exit that drops a value its own fact. Its `arm`
indexes the exit row. `also_on_exits` on the scope-end summary identifies
shared MIR drops. A definite shared drop also proves the destructor runs on
each listed exit in an archived dump. Only positioned events can establish
evaluation order. A shared drop flag alone leaves the exit-specific result open.

An `[[order]]` is checked along a single path through a row. For nested
branches that includes the selected arm's parents. Spike dumps lack arm
membership and sort events by kind, so they cannot verify an order, and a
v1 early-exit drop's `arm` was taken at the wrong place, so v1 cannot either.
`exact_drops = true` additionally requires that the path runs no destructor
the order does not name (use it where a dump that drops both locals on a path
where one moved must fail). From v3 on, each exit that drops a value has its
own positioned event, so a shared scope-end summary no longer makes an absence
assertion uncertain; older dumps still do.
For a per-path loan end use `[row, next]`, optionally with a third element
for the successor column. The harness matches the producer's MIR successor
row exactly, including a join brace and a loop's back edge. It counts
distinct `(row, next, column)` triples for the same loan.

**Why a place is `maybe`.** A `maybe` state needs the producer's row-local
`cause` for that place: `moved`, `unassigned` or `imaginary`. The harness also
accepts those causes encoded directly in the state name. A missing cause
cannot prove either expectation.

**Sources** (`src`). `oracle`: derived by rustc probes, written only by the
generator into the block at the end of the file. `issue`: hand-written from
the issue and checked against rustc (a probe, a compile error, a run with a
Drop-printing type for drop timing, `--emit=mir` for moves and drops).
Give the check in a nonempty `why` on every hand-written assertion. Write `issue` assertions only for what the oracle can't decide, or
for the exact point the issue is about (repeat it even if the oracle has it:
it names the bug).

**Or-patterns.** Pin usability against borrowck, including its imaginary
edges. Pin moves and drops against executable MIR. In #1674 a read of
either tuple field after binding `x` produces E0382, while only `u.0`
actually moves and `u.1` drops at scope exit. The format also supports
`[[oracle_skip]]` (`place`, optional `row` / `fn`, `why`) for documented
exceptions to a generated check.

**Known crashes** (`[known_crash]`: `issue`, `panic_at`, `phase`). For a
crash issue whose program makes the plugin as a whole exit nonzero (a panic
in the 2.x drawing path) while the fact layer still writes its dump. In
strict mode the case passes only if the plugin fails with `panic_at` (a
substring of its stderr, e.g. `data.rs:5179`) AND the written dump satisfies
the assertions. If the plugin no longer fails, or fails somewhere else, the
case fails (remove the marker, or update `panic_at`). It never covers a
missing or unreadable dump, and it excludes `xfail`. `phase` is the PLAN.md
phase whose work replaces the crashing code (P2 for the drawing path). With
`RV3_FACTS_DIR` the plugin's stderr is read from `<case>.err` (what
`issue-facts.sh` writes).

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
(the program, probe inputs or toolchain changed) or has a malformed line.

A case may request an extra statement boundary inside a closure, a call
argument block or a block in a tail expression. For example, the following
entry in the nested-probes fixture checks a local after its move.

```toml
[[probe]]
fn = "run::{closure#0}"
row = 6
place = "local"
```

Take the body name from rustc MIR. The place must be a simple local
declared with `let` in the same block. The probe goes between complete
statements and preserves the block's tail value. This also keeps the probe
from introducing a capture. Generation requires a rustc verdict at each
requested point. Dead code gives no ownership verdict. Use executable
MIR to pin the absence of a move or drop after a diverging statement.

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
versioned object (v1, v2 or v3) and the spike's bare array.
