# Element ids: the `data-rv-*` attributes

Every element of a figure that shows a tooltip on hover, and every
numbered line of the code panel, carries structured `data-rv-*`
attributes. They let a client say exactly which element a reader
hovered (a telemetry event, a replay of a session) without parsing the
tooltip's English, which changes between versions. They change nothing
that is drawn and no tooltip text.

The plugin only writes these attributes. It records and sends nothing.

```svg
<circle cx="70" cy="215" r="5" data-hash="4" class="tooltip-trigger"
        data-tooltip-text="…"
        data-rv-id="s@L9:dot:move" data-rv-kind="dot" data-rv-event="move"
        data-rv-var="s" data-rv-line="9"/>
```

## Attributes

| Attribute | On | Value |
|---|---|---|
| `data-rv-id` | every element below | The element's id. Unique within one figure (one timeline panel, or one code panel). See [Ids](#ids). |
| `data-rv-kind` | every element | One of the [kinds](#kinds). |
| `data-rv-event` | `dot`, `call`, `fn`, `arrow` | The ownership events drawn there, space-separated, in the order they happen. See [Events](#events). |
| `data-rv-state` | `line`, `branch_line`, `bracket` | The column's state over those rows. See [States](#states). |
| `data-rv-var` | all but `code_line` | The column, by the name its header shows (`s`, `s₂`, `p.x`, `String::from`). For an arrow or `fn`, the column it starts at. |
| `data-rv-to` | `arrow`, `fn` | The column an arrow points to. `caller` for a value returned to the function's caller. |
| `data-rv-line` | all but `header` | The source line, as numbered in the code panel's gutter. For a segment, its first line. |
| `data-rv-line-end` | segments covering several lines | The segment's last line. |
| `data-rv-part` | some nodes of a `branch_line` or `stop_bar` | The node is part of an element drawn as several nodes. See [Parts](#parts). |
| `data-rv-drop` | `dot` | `true` on a dot drawn with the drop mark: the column's value is dropped there. |
| `data-rv-invalid` | the timeline's root `<svg>` | Only when something went wrong: each stretch of a column whose state the plugin couldn't compute (`State::Invalid`), as `var@L<line>[-<end>]`, space-separated. Nothing is drawn there. The corpus invariant checks (`rustviz-lib/tests/common/invariants.rs`) fail on it. |

Values are XML-escaped like any attribute (`String::from<T>` is written
`String::from&lt;T&gt;`); read them with `getAttribute` / `dataset`,
which decodes them.

Lines are those of the code panel's gutter. They match the source file's
except in a file with `// rustviz: hide` functions, whose hidden lines
the gutter skips. Rows the plugin adds to make room (blank rows around a
one-line conditional or before a function, rows arrows are stacked on)
have no number; an element on one takes the line above it.

## Kinds

| `data-rv-kind` | What |
|---|---|
| `header` | A column's name at the top of its column. |
| `dot` | An event dot on a column: one dot for every event on that line of that column (its tooltip has a bullet per event). Includes drop dots (`data-rv-drop="true"`) and a dot with a call icon on it. |
| `call` | A function-call icon (`f`) on a column where no event dot is drawn (the call reads from or writes to the variable). |
| `fn` | A function's logo at the end of an arrow to or from a function (`String::from`, `takes`, a function returning to its caller). |
| `arrow` | An arrow between two columns, or between a column and a function or the caller. |
| `line` | A stretch of a column in one state (solid, hollow, or dotted while mutably lent out). |
| `branch_line` | A stretch of a conditional arm's column, including the diagonals from the split and to the join. |
| `stop_bar` | The bar ending the column of an arm that never reaches the join (`return`, `panic!`, `break`, `continue`). |
| `bracket` | The bracket beside a reference's column while it holds a borrow. |
| `code_line` | A numbered line of the code panel (`<text class="code">`). Only `data-rv-id`, `data-rv-kind` and `data-rv-line`. |

## Ids

```text
id      := element [ "/" part ]
element := base [ "#" n ]                 n >= 2
base    := "hdr:" var                                        (header)
         | "code:L" line                                     (code_line)
         | var [ ">" to ] "@L" line [ "-" end ] ":" kind [ ":" detail ]
detail  := event { "+" event }            (dot, call, fn, arrow)
         | state                          (line, branch_line, bracket)
```

Examples:

| Id | Element |
|---|---|
| `hdr:s` | `s`'s column header |
| `code:L5` | line 5 of the code panel |
| `s@L6:dot:acquire` | `let s = String::from("hi");`: s gets its value |
| `s>t@L9:arrow:move` | `let t = s;`: the move arrow from s to t |
| `r@L8:dot:static_lend+static_reacquire+static_die+pass_by_static_reference` | `println!("{}", r);`, r's last use: one dot, four events |
| `r@L7-8:bracket:partial_privilege` | r's borrow bracket, lines 7 to 8 |
| `s@L4-7:line:full_privilege` | s's column, lines 4 to 7 |
| `t@L8:branch_line:full_privilege#2` | the second of two arm columns of t that would both get `t@L8:branch_line:full_privilege` |
| `t@L8:branch_line:full_privilege_gray` | an arm column of t drawn dashed (another arm runs on line 8) |
| `t@L8:branch_line:full_privilege/side1` | one side line of a hollow arm column (a part) |

When two elements would get the same base, the first in document order
keeps it and the next ones get `#2`, `#3`, … (two columns both named `s`
give `hdr:s` and `hdr:s#2`). Document order is deterministic, so the same
program rendered by the same plugin version always gets the same ids.
Treat an id as opaque outside one `(snippet, rustviz version)`: a later
version may draw an extra dot, or merge two, and renumber.

An id has no whitespace (a space in a name becomes `_` in the id, not in
`data-rv-var`) and is at most 128 characters: a base longer than 112
characters is cut before the `#n` is added, so it stays unique (the full
event list is still in `data-rv-event`).

### Parts

Most elements are one SVG node. A conditional arm's column is several:
each segment has a transparent hover polygon (the element, no `part`)
and its visible line(s) on which the pointer can also land. Those nodes
get the element's id plus `/part`, and `data-rv-part`:

| `data-rv-part` | Node |
|---|---|
| `line` | a solid or dotted segment's line |
| `side1`, `side2` | the two lines of a hollow segment |
| `bevel1`, `bevel2` | the short connectors where a hollow segment bends |
| `hit` | a stop bar's hover polygon |

To count hovers per element, strip `/part` from the id.

## Events

`data-rv-event` names the plugin's own event types in snake_case:
`Event` (one column's side) on `dot`; `ExternalEvent` (between two
columns) on `arrow`, `fn` and `call`. The same word can mean different
things on the two: a `move` dot is the column moving its value out,
while the receiving column's dot is `acquire`; a `move` arrow is the
transfer itself. A dot with a call icon also lists the call's
`pass_by_static_reference` / `pass_by_mutable_reference`.

On dots (`Event`):

| Event | Meaning on that column |
|---|---|
| `acquire` | gets a new value: from a constructor, a function, or moved from another variable |
| `copy` | gets a copy of another variable's value (a `Copy` type) |
| `duplicate` | its value is copied to another variable or function (a `Copy` type, or `.clone()`) |
| `move` | its value moves out (to a variable or function); the column ends unless it gets a new value |
| `static_lend` / `mutable_lend` | it is borrowed (`&` / `&mut`) |
| `static_borrow` / `mutable_borrow` | it (a reference) takes a borrow (`&` / `&mut`) |
| `static_reacquire` / `mutable_reacquire` | a borrow of it ends |
| `static_die` / `mutable_die` | its (a reference's) borrow ends |
| `ref_die` | a reference's borrow ends (not drawn as a dot today) |
| `owner_go_out_of_scope` | an owner's scope ends (dropped if it still holds a resource: `data-rv-drop`) |
| `ref_go_out_of_scope` | a reference's (or a non-owning variable's) scope ends; nothing dropped |
| `owner_drop_at_reassign` | the old value is dropped when a new one is assigned |
| `owner_drop_if_try_returns` | dropped here only if the `?` returns early |
| `owner_drop_unless_returned` | dropped here on the paths that don't return it |
| `manually_drop_by_hand` | `ManuallyDrop::drop`: the value inside is dropped by hand |
| `temporary_drop` | a temporary is dropped |
| `tuple_temp_drop` | the tuple a `match (x, y)` / `if let (x, y)` scrutinee builds is dropped, with what's left of x in it |
| `loop_iterator_drop` | the `for` loop's iterator, moved out of this column, is dropped |
| `loop_maybe_init` | in a loop, may or may not hold a value at this point |
| `loop_exit_kept` | at the end of a bare `loop`: holds a value at every `break` |
| `loop_exit_move` | after the loop, may have been moved by a branch that `break`s |
| `branch` | the split of a conditional (`if`, `match`, `if let`, loop) the column takes part in |
| `branch_join` | the join of that conditional (not an enum variant: the join dot of `branch`) |
| `or_alt_move` / `or_alt_lend` / `or_alt_borrow` | one of several places an or-pattern (`A(x) \| B(x)`) may bind from |
| `chain_fail_assign` / `chain_fail_use` | what a let-chain condition did before it failed |
| `tied_param_note` / `dead_else_note` / `dead_loan_note` | notes: nothing about ownership changes |
| `init_ref_param` | a function parameter gets its value from the caller |
| `closure_call` | an `FnOnce` closure is moved into its call |

On arrows, call icons and function logos (`ExternalEvent`):

| Event | Arrow |
|---|---|
| `move` | a value moves from `var` to `to` (`to` = `caller`: returned) |
| `copy` | a value is copied from `var` to `to` |
| `static_borrow` / `mutable_borrow` | `to` borrows `var` (`&` / `&mut`) |
| `static_die` / `mutable_die` | the borrow ends: the arrow returns from the reference `var` to the lender `to` |
| `pass_by_static_reference` / `pass_by_mutable_reference` | a function borrows the variable for a call (a `call` icon, or on a `dot`) |
| `init_ref_param` | a parameter's value comes from the caller (`var` = `caller`) |
| `bind`, `go_out_of_scope`, `ref_die`, `closure_call`, the drop and note events, `branch` | not drawn as arrows today; listed so the mapping is complete |

For coarse analysis, these groups are a reasonable start:

| Group | Events |
|---|---|
| move | `move`, `acquire` (from a move), `closure_call`, `or_alt_move`, `loop_exit_move` |
| copy | `copy`, `duplicate` |
| borrow | `static_borrow`, `static_lend`, `pass_by_static_reference` |
| mut_borrow | `mutable_borrow`, `mutable_lend`, `pass_by_mutable_reference` |
| borrow end | `static_die`, `mutable_die`, `static_reacquire`, `mutable_reacquire`, `ref_die` |
| scope end / drop | `owner_go_out_of_scope`, `ref_go_out_of_scope`, the `*_drop*` events (a drop is drawn where `data-rv-drop="true"`) |
| control flow | `branch`, `branch_join`, `loop_*`, `chain_fail_*` |

## States

`data-rv-state` is the column's `State` in snake_case, plus `_gray` for a
conditional arm that isn't the one running on those rows (drawn dashed)
or `_uninit` for a variable with no value yet:

| State | Column |
|---|---|
| `full_privilege` | holds its value and can use it (solid if `mut`, hollow if not) |
| `partial_privilege` | holds its value but it is borrowed (`&`), so it can only read (hollow) |
| `revoked_privilege` | mutably lent out (`&mut`): can't be used until the borrow ends (dotted) |
| `resource_moved` | a `&mut` reference that was moved (thin dashed) |

On a `bracket`, the state where the reference's borrow starts:
`full_privilege` (a `&mut` reference, or a `&` one the only reader) or
`partial_privilege`.

## Stability and tests

- `rustviz-lib/tests/corpus.rs`: every fixture's panels pass
  `common::svg::rv_problems` (every hover target has an id, ids are
  unique, each kind has the attributes it needs); `rv_ids_are_stable_across_runs`
  renders several fixtures twice and compares their ids;
  `rv_ids_pin_a_simple_program` pins the ids of `rv_element_ids.rs`.
  `rustviz-lib/tests/book.rs` runs the same check over the Book snippets.
- The attributes are written in `rustviz-plugin/src/svg_generator/svg_frontend/rv_attrs.rs`.
  A new `Event` / `ExternalEvent` variant must be named there (the
  match is exhaustive) and in the tables above.
- `scripts/probe-diff.sh` with `STRIP=' data-rv-[a-z-]+="[^"]*"'` checks
  that a change to these attributes changes nothing else.
