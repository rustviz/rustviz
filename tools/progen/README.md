# progen: feature-combination programs for the corpus checks

Most RustViz bugs live in **combinations** of features that no fixture
has: a `&mut` moved into a generic call, a let-else that moves, a holder
moved by a pattern inside a match arm. `progen` writes such programs at
random, keeps the ones rustc accepts, runs RustViz's corpus checks on
them, shrinks every failure to a small program and groups the failures so
that one root cause is one bucket (#1218).

It adds no checking code of its own. The checks are the ones every
fixture already goes through: `corpus_invariants_hold` in
`rustviz-lib/tests/corpus.rs`, i.e. the ownership invariants in
`rustviz-lib/tests/common/invariants.rs` (including `State::Invalid`,
`data-rv-invalid`) and the drawing checks in `geometry.rs`, pointed at the
generated files with `RV_INVARIANTS_EXTRA=<dir> RV_INVARIANTS_ONLY=<dir>`.
A plugin panic comes back from that test as a failed snippet and is
bucketed as `crash` by the panic's `file:line`.

## Running it

```sh
cargo install --locked --path rustviz-plugin --root /tmp/rv   # the plugin to test
PROGEN_PLUGIN_ROOT=/tmp/rv tools/progen/run.sh 3000 42 /tmp/progen-42
```

`run.sh N SEED [OUT]` generates programs `0..N` of seed `SEED` (the same
seed always gives the same programs), builds the corpus test binary,
and writes to `OUT` (default `progen-out/SEED`):

| File | What |
|---|---|
| `summary.md` | counts, one row per bucket (check, signature, how many programs, smallest repro), and each bucket's minimal program |
| `summary.json` | the same, plus rustc's rejection codes and how often each grammar template was used |
| `buckets/NN-<check>/min.rs` | the shrunk program: still compiles, still fails the same check with the same signature |
| `buckets/NN-<check>/original.rs`, `failure.txt` | the generated program it came from and the check's full message |
| `work/` | every compiling program (`programs/pNNNNN.rs`) and the shrinker's candidates; safe to delete |

Environment:

- `PROGEN_PLUGIN_ROOT`: a `cargo install --root` to test. Its `bin/` goes
  first on `PATH` and `CARGO_HOME` points at an empty directory, because
  cargo looks for `cargo-rv-plugin` in `$CARGO_HOME/bin` before `PATH`
  (an old plugin there would silently be the one tested). Without it,
  whatever `cargo rv-plugin` resolves to is tested.
- `PROGEN_TIME_LIMIT`: seconds for the whole run. Generation stops at about
  55% of it; shrinking stops at the limit and reports the smallest program
  found so far.
- `PROGEN_TEST_BIN`: a prebuilt `corpus` test binary, to skip the build.

`progen.py gen --count N --seed S --out DIR` only writes programs, and
`progen.py check DIR` runs the checks on a directory of `.rs` files (any
programs, e.g. a batch of bug repros).

`--oracle` (on `run` and `check`) also runs `corpus_oracle_holds`, which
compares each variable's drawn state with rustc's borrow checker (see
"Checking drawings against rustc" in the top-level README). Its failures
are bucketed as `oracle-usable` / `oracle-readable` / `oracle-borrowed` /
`oracle-moved` by the variable's type, the drawn state and rustc's error
code. It compiles a few probe programs per generated program, so a run
takes about twice as long.

Needs Python 3.8+ (standard library only) and the toolchain from
`rust-toolchain.toml` (rustc's borrow checker is the filter). 3,000
programs take about 3 minutes to generate and check on 18 cores; shrinking
takes most of a run (about a minute per bucket, four buckets at a time).

## How programs are made (`gen.py`)

A program is the helper items it uses plus one generated function: `fn
main()`, or `fn run(<params>, flag: bool) -> Option<usize>` (where `?` and
`return None` work). The body starts with one or two `String`s and one
other value, then 3–8 statements drawn from the templates, nested up to two
levels, over these variables:

`String`, `&String`, `&mut String`, `&str`, `Vec<String>`, `Vec<&str>`,
`Holder<'a> { r: &'a String, n: i32 }` (a reference field), `Pair { a:
String, b: String }` (owned fields), `Box<String>`, `Option<String>`,
`(String, i32)`, `[String; 2]`, `i32`, and closures (borrowing `Fn`,
`FnMut`, `move` `FnOnce` returning the captured `String`, `move` `Fn`).

The templates cover:

- **let**: fresh values, moves, copies, clones, `&`/`&mut` borrows,
  reborrows (`&*m`, `&mut *m`), shadowing, `let _ = s` (no move), `let x;`
  initialized in both arms of an `if`, values of `if`, `match` and
  labeled blocks (`break 'blk s`).
- **assignment**: whole variables (including re-initializing a moved one),
  fields (`p.a = ..` after `p.a` was moved, `h.r = &t`), elements
  (`v[0]`, `arr[1]`), derefs (`*m = ..`, `*b = ..`, `*m += ".."`),
  destructuring assignment (`(a, b) = (b, a)`, `(s, n) = split(t)`,
  `Pair { a: x, b: y } = p`), `std::mem::{swap, take, replace}`.
- **calls**: by value, `&`, `&mut` (implicit reborrow of a `&mut` variable,
  and moving it into a generic `T`), concrete and generic functions
  returning owned values, references (`first`, `longest`, `downgrade:
  &mut -> &`, `wrap_mut`), structs, tuples and `Option`, closures passed
  to a generic `FnOnce`.
- **method calls**: `push`/`push_str`/`clear`, `Vec::push` of a moved
  value or a borrow, `iter_mut`, `last_mut`, `first`/`last`/`get`,
  `as_ref`/`as_mut`, `take`/`replace`, `unwrap`, `?`, and `&self` / `&mut
  self` / `self` methods on `Pair`.
- **control flow**: `if` / `else if` / `else`, `match` (guards,
  or-patterns, ranges, `ref` bindings, one-line arms), `if let`, `while
  let`, let-else (diverging by `return`, `break` or `continue`), let-chains
  (edition 2024), labeled blocks, `loop`, `while`, `for` over `&v`,
  `v.iter_mut()`, `v` and ranges, nested loops with `break 'outer`,
  `break`/`continue`, early `return`, a move followed by `break`.
- **literals and patterns**: struct, tuple and array literals, struct
  update (`..h` copying a reference, `..p` moving a field), struct, tuple
  and array patterns with `ref`/`ref mut`, `..`, slice patterns (`[x, rest
  @ ..]`).
- **reads**: `println!` of one or two variables, `.len()`, conditions.

To keep most programs compiling, the generator tracks for each variable
whether it holds a value (moved, maybe moved after a branch, partly moved,
not yet assigned) and which loans it depends on, with a counter per
variable of its last read and last write: a reference may be used only if
nothing wrote to its lender since it was made (for `&mut`, nothing read
it either). Branches are merged (moved in one arm = maybe moved), outer
values aren't moved inside loops except right before a `break`, and a loop
whose body breaks a loan it also uses is dropped. The model is
approximate; rustc decides (`rustc --edition 2024 --crate-type lib
--emit=metadata`, about 96% get through).

## Buckets and shrinking (`progen.py`)

Each failure has a check (the invariant's name, `crash`, `plugin-error`
or `timeout`) and a signature: for the ownership checks, the elements the
message names with variable names replaced by their types and line numbers
dropped (`RS:dot:static_die`, `P.a:line:partial_privilege / P:dot:move`);
for the drawing checks, only the kinds of elements (`branch_line / dot`;
`outside-figure` by axis), since one layout bug shows up on whichever
columns are there; for `invalid-state`, the types of the columns that went
invalid; for a crash, the panic's `file:line`. A program counts once per
check (its smallest signature).

The smallest program of each bucket is shrunk: repeatedly try every
single edit (delete a statement, replace a compound statement by one of
its blocks, drop an `else`, drop a parameter), keep the smallest result
that still compiles and still fails with the same check and signature;
then the same with single lines and single call arguments. Each round
checks all candidates in one run of the test binary.

## Triage

A bucket is a lead, not a bug report. For each one, render `min.rs`
(`cargo rv-plugin` in a crate holding it, or the playground), read the
tooltips as a careful Rust teacher, and compare with what rustc says
about the program. A real bug gets an issue (one per root cause, with the
minimal program, what RustViz shows and what it should show; "Found by the
program generator (#1218)"). A bucket where RustViz is right and the check
is wrong is a checker false positive: note it for a change to the checks,
don't paper over it here.

## CI

The batch is run by hand, not on a schedule: the Actions "Run workflow"
button on `.github/workflows/progen.yml` (inputs `count` and `seed`), or
`tools/progen/run.sh N SEED` locally. The workflow installs the plugin from
the commit, runs a 20-minute batch (the date as the seed unless one is
given), shows `summary.md` on the run page and uploads it with
`summary.json`, `log.txt` and `buckets/` as the `progen-<seed>` artifact.
Findings never fail the job. On pull requests that touch `tools/progen/`
it only runs the generator's own tests:

```sh
python3 -m unittest discover -s tools/progen
```

(determinism per seed, grammar coverage, ≥80% of programs compile,
shrinking keeps the failure and reaches a minimum, reading the test's
output, signatures).

## Extending the grammar

Add a `t_*` method to `Gen` in `gen.py` and list it in `templates()`. A
template picks its variables (`self.pick`, `self.pick_ty`), returns `None`
if there's nothing to work on, and otherwise records what it does to them
(`self.move`, `self.touch`, `self.borrow` for a new reference's loans,
`self.new_var`) before returning a `Stmt` with a tag. Statements with
blocks build them with `self.block(...)` from a `self.snap()` and join the
arms with `self.merge(...)`. Add the tag to `test_grammar_coverage`.
