//! Plugin regression tests against the corpus of `rustviz-plugin/tests/*.rs`.
//!
//! Each .rs file in the corpus is a Rust program the plugin should be able
//! to visualize. We don't pin exact SVG output (it's noisy and changes with
//! benign tweaks); instead we assert on three layers:
//!
//!   * `corpus_expected_ok_produce_well_formed_svg` — the regression floor.
//!     Every `EXPECTED_OK` snippet returns well-formed SVG (`<svg ...`) for
//!     both panels and the timeline carries at least one `tooltip-trigger`
//!     element (i.e. the plugin produced *some* annotations).
//!
//!   * `corpus_expected_tooltips_present` — the behaviour bar. For every
//!     entry in `EXPECTED_TOOLTIPS`, the listed `must_contain` strings must
//!     appear verbatim as `data-tooltip-text` values in the rendered
//!     timeline, and the `must_not_contain` strings must *not* appear.
//!     Containment semantics on both sides — extra tooltips are fine, only
//!     the listed ones are load-bearing.
//!
//!   * For features known to be unsupported (for-loops, conditional borrows,
//!     smart pointers, etc. — see README "Limitations") snippets stay off
//!     these lists; failures there aren't regressions until the feature is
//!     implemented.
//!
//! These tests use the `RV_RUNNER=local` backend, which requires
//! `cargo install --path rustviz-plugin --locked` to have been run first.
//! `setup.sh` does this for you. CI runs `setup.sh` before `cargo test`.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use rustviz_lib::Rustviz;

/// Curated subset that should always succeed end-to-end. Each entry is the
/// stem of a file in `../rustviz-plugin/tests/`. Keep this list small and
/// representative; it's the regression floor.
///
/// Snippets are grouped by feature area in source order so a glance at
/// the list shows which categories are covered.
const EXPECTED_OK: &[&str] = &[
    // — Existing canon: ownership / move / copy / basic refs.
    "testMove",
    "testNum",
    "basic_ref",
    "basic_mutref",
    "copy2Caller",
    "ownershipFunctions",
    "testStaticB",
    "testMutB",
    // — Field-projection cases (added with #84).
    "nested_struct_borrow",
    "nested_struct_move",
    "nested_struct_passref",
    "field_method_call",
    "field_through_ref",
    // — References: more nuanced shapes.
    "reborrow",
    "reborrow_kinds",
    "borrow_moved_on",
    "call_reborrow_kinds",
    "interior_mut_lend",
    "nll_two_borrows",
    // — Reassignment / move-out / take-and-return.
    "reassign_with_drop",
    "move_return",
    "take_and_return",
    // — Stdlib method calls.
    "string_push_str",
    "string_len",
    // — Stdlib indexing / slicing (#75).
    "vec_slice",
    // — User-defined inherent methods.
    "inherent_method_rectangle",
    // — Lifetime annotations.
    "lifetime_excerpt",
    // — Generics.
    "generic_fn",
    // — Shadowing.
    "shadowing_basic",
    // — Marker comments (skip on let, hide on fn).
    "skip_marker_let",
    "hide_marker_fn",
    // — Conditionals as expression RHS (the supported subset).
    "if_as_let_rhs",
    // — Smart-pointer wrappers (#76): rendered as opaque single-owner
    //   columns rather than recursing into Unique/NonNull/PhantomData
    //   internals.
    "box_string",
    "rc_clone",
    "box_dyn",
    // — Closure captures (#79): capture arrows render for both
    //   `move` and borrow closures.
    "closure_move_single",
    "closure_move_multi",
    "closure_borrow_imm",
    "closure_borrow_mut",
    // — Ref-arg call sites inside `println!` (#74): the visitor now
    //   descends through synthetic macro scaffolding to user-spanned
    //   subexpressions, so an inline `r.method()` / `f(&r)` inside a
    //   formatter macro emits its call-site arrow.
    "method_call_in_println",
    "freefn_call_in_println",
    // — Tuple destructuring (#86): each tuple-literal element pairs
    //   with its sub-pattern as if it were its own `let`. The slice
    //   variant takes the same path against an array literal RHS.
    "tuple_destructure",
    "slice_destructure",
    // — Conditionals (#87, #108): match-as-rhs renders move arrows
    //   per arm; arm labels show source-form (`0`, `_`); merge dot
    //   carries a join-state tooltip; nested conditionals propagate;
    //   no-else `if` doesn't synthesize an "Else" label.
    "match_as_let_rhs",
    "if_else_move_join",
    "if_else_rebind_join",
    "nested_if_move_join",
    "if_no_else",
    "if_else_mut_reassign",
    "if_as_let_rhs_multiline",
    "if_else_move_both",
    "deep_nested_if",
    "match_three_arms",
    "if_let_no_else",
    "match_one_arm",
    "match_tuple_destructure",
    // — Conditionals composed with iteration / capture. Verifies
    //   the merge classifier from #116 picks the right wording
    //   when the branches are non-trivially shaped (a closure
    //   capture in one arm, a for-loop borrow in another, etc.).
    //   Tooltip-level pinning lives below in EXPECTED_TOOLTIPS.
    "cond_with_for_loop",
    "cond_with_closure",
    "if_inside_for",
    "closure_with_cond",
    "if_let_inside_for",
    "cond_with_move_closure",
    "match_with_closure_arms",
    // — Borrowing the unit value of an empty block (#146). Used to
    //   panic in fetch_mutability; pinned here so the no-crash
    //   floor doesn't regress.
    "borrow_empty_block",
    // — Triaged from the previously-untracked fixture set (#140).
    //   These are shapes the curated entries above don't directly
    //   cover.
    "question_mark",        // `?` desugar (used to panic)
    "question_mark_chain",  // chained `?`
    "aliasing1",            // multi-level reference aliasing (`&&&x`)
    "anonymous_call",       // fn returning &'static str fed into a method
    "vec_macro",            // `let v = vec![…]` RHS
    "assert_call_dot",      // call inside `assert!(…)` emits its event
    "mv_back",              // shadow-after-move pattern
    "mutAssignOp",          // `x += 5` compound assignment
    "mutableReferences",    // `change(&mut s)` minimal mut-ref fn call
    // `if let` guards on match arms (stable since 1.95). Used to crash on
    // the guard-bound variable; tooltips pinned in EXPECTED_TOOLTIPS.
    "match_if_let_guard",
    // `:::` in a comment and a string literal (#181): used to be taken as
    // the plugin's panel separator, truncating the code panel mid-line.
    "triple_colon_source",
    // — Coverage gaps closed (#139): patterns the visitor has arms
    //   for but which lacked corpus pinning, plus const/static and
    //   unsafe blocks which were silently untested.
    "match_range_pattern",
    "match_with_guard",
    "const_basic",
    "static_basic",
    "unsafe_raw_ptr",
    "static_mut_write",     // `COUNTER += inc` / `LOG = v` (used to panic)
    // — Union (#131): rendered as an opaque single-owner column,
    //   same as enums. Pre-fix the plugin panicked at the
    //   AdtKind::Union arm with "union not implemented yet".
    "union_basic",
    // — Enum-typed fn parameter (#131): one opaque owner column, no
    //   phantom per-field (`s.0`) column.
    "enum_param",
    // — Tuple-field projections (`x.0`) used to panic in
    //   annotated_source_gen; fixed alongside #131.
    "tuple_struct_field",
    "tuple_field_read",     // renders; the read itself isn't shown (known gap)
    // — Assignment through a deref of a method-call temporary
    //   (`*c.borrow_mut() += 1`, `*v.last_mut().unwrap() = 5`). Used
    //   to panic looking up a RAP for the temporary.
    "refcell_deref_assign_op",
    "refcell_deref_assign_move",
    "rc_refcell_deref_assign",
    "deref_method_result_assign",
    // — The same fn / tuple-variant constructor receiving more than one
    //   value: nested `Cons(1, Box::new(Cons(2, ..)))`, or `id(s1)` then
    //   `id(s2)`. Used to panic in calc_state on the shared Function RAP.
    "cons_list_nested",
    "cons_list_qualified_nested",
    "fn_called_twice",
    // — Chained field assignment (#143). `o.inner.x = …` now
    //   resolves through `expr_to_rap_name` to the nested field's
    //   pre-registered RAP and emits an Acquire event on that column.
    "chained_field_assign",
    "chained_field_assign_move",
    // — Assignment to a place with no column of its own (`self.f = …`,
    //   `r.f = …` through a `&mut`, `v[i] = …`). The write isn't drawn;
    //   these used to panic in `resource_of_lhs`.
    "assign_unmodelled_place",
    "assign_field_of_method_result",
    // — References never own their referent: `&mut` / `&` lifelines,
    //   writes through `*r`, and `&str` bindings use reference
    //   wording rather than "owner" / "acquires ownership".
    "mutref_param_push_str",
    "mutref_after_shared_borrows",
    "mutref_self_method",
    "vec_for_mut_deref",
    "hashmap_entry_count",
    "iter_mut_deref",
    "deref_assign_copy",
    "move_mutref",
    "box_deref_assign",
    "str_literal_ref",
    "match_str_literal_arms",
    "cell_ref_return",
    // — Compiler-generated code stays out of the diagram: derived
    //   impls draw no timelines, macro-bodied match arms end on the
    //   arm's own line, Copy returns read as copies.
    "derive_debug",
    "match_arm_macro_body",
    "copy_return_param",
    "return_shared_ref",
    // Receivers consumed by a `self` method, and the left operand of
    // `String + &str`, are moved into the call (#172).
    "self_method_into_iter",
    "self_method_option",
    "self_method_user",
    "self_method_ref_receiver",
    "string_add_in_fn",
    // Struct fields follow whole-struct moves / copies, struct update
    // syntax and field-init shorthand (#174).
    "struct_move_drop",
    "struct_move_let",
    "struct_move_fn_param",
    "struct_update_syntax",
    "struct_field_init_shorthand",
    "struct_nested_whole_move",
    "struct_copy_whole",
    "struct_return_whole",
    "struct_reassign_whole",
    "struct_update_return",
    "struct_partial_move_two",
    "struct_literal_same_line_arrows",
    // — Match guards (#176): reads/borrows in `pat if cond =>`, guard
    //   reads of the arm's own bindings, `if let ... && cond` guards.
    "match_guard_borrow",
    "match_guard_reads_binding",
    "match_guard_let_chain",
    // — Macro arguments (#177): formatting macros, `dbg!`, `write!`,
    //   `assert_eq!` and `panic!` show the borrow / move of each
    //   argument; a brace-less match arm whose body is a macro call.
    "macro_format_args_borrow",
    "macro_dbg",
    "macro_match_arm_body",
    "macro_write_assert_eq",
    // Struct boxes in the timeline header (#192): a `String` bound by a
    // pattern used to become an empty struct group drawn with
    // width="-80"; back-to-back structs shared one box; a struct with no
    // field columns got a negative-width box. Box geometry is pinned by
    // `struct_boxes_span_their_own_columns`.
    "while_let_pop_string",
    "struct_box_adjacent",
    "if_let_struct_fields_unused",
    // Unit / empty structs get a label-only box each (#348).
    "unit_struct_box",
    // Deferred initialization, `let x;` then `x = …` (#234). Used to
    // panic in resource_of_lhs.
    "deferred_init_copy",
    "deferred_init_string",
    "deferred_init_if_else",
    "deferred_init_match",
    "deferred_init_then_reassign",
    "deferred_init_one_branch",
    // A String moved into a field of a method call's result, and a
    // nested field write through one (from closed PR #186).
    "field_of_method_result_assign",
    // A struct item between two fns (#231): main's variables used to
    // reuse helper's RAP hashes, and here the renderer panicked.
    "struct_after_fn_panic",
    // Assigning to a plain tuple's field (`t.0 = 5`) used to panic in
    // resource_of_lhs (#235). Renders; the write isn't drawn (#189).
    "tuple_field_assign",
    // A Copy result of a method chain on a reference is a plain Copy
    // binding, also on the line where the reference's borrow ends (#190).
    "method_chain_ref_copy_result",
    // Straight arrows hop over the dots they cross (#252); the
    // crossing check itself runs on every snippet in this list.
    "arrow_hops_over_dots",
    // Arrow tails start at the edge of their source dot (#269); the
    // tail check runs on every snippet in this list.
    "arrow_tail_off_source_dot",
    // A borrow of a temporary whose borrow ends on the same line (#204).
    "borrow_of_temporary_same_line",
    // Odd arm counts are laid out symmetrically, the middle arm on the
    // column (#119); the layout itself is pinned by unit tests in
    // timeline_panel.rs, these keep the shapes rendering on-canvas.
    "match_five_arms",
    "match_three_arms_nested_middle",
    // More columns than the 12-colour palette: every column still gets
    // a stroke colour (#279).
    "many_columns_palette",
    // Events on one line of one column share one dot (#309); the
    // shared-point check runs on every snippet in this list.
    "same_line_events_one_dot",
    // A drop and a call on one merged dot: the drop mark sits below it.
    "drop_and_call_one_dot",
    // A partially moved struct drops only its remaining fields (#207).
    "struct_partial_move_all",
    "struct_partial_move_nested",
    // Union writes and scope end don't drop fields (#191).
    "union_manually_drop",
    "union_whole_assign",
    // Reference moves/copies and Copy assignments are worded as such (#178).
    "ref_move_copy_wording",
    "copy_field_assign",
    // A borrow of a string literal or a static names it when it ends.
    "ref_static_lender",
    "ref_static_lender_mut",
    // Merge dots of variables no branch moves get a tooltip (#179).
    "match_unchanged_merge",
    // A closure capturing through a reference reborrows `*r` (#277).
    "closure_capture_through_ref",
    // A ManuallyDrop local / field / param never drops its value (#283).
    "manually_drop_local",
    // `ManuallyDrop::drop` / `take` by hand (#283).
    "manually_drop_by_hand",
    // `&self` calls on interior-mutable values aren't "reads" (#203).
    "interior_mut_calls",
    // The RefCell runtime-check sentence follows the callee's identity,
    // not the method name `borrow_mut` (#203).
    "interior_mut_user_borrow_mut",
    // Operands of built-in arithmetic operators are copied in (#241).
    "binop_operand_copies",
    "method_autoref_receiver",
    "method_autoref_ref_receiver",
    // `(&mut s).method()` is a fresh borrow of s, not a reborrow (#367).
    "method_addrof_receiver",
    // A moved-out reference's state isn't about ownership (#292).
    "moved_ref_state",
    // Operands of `-x` / `!b` are copied in, or moved for an
    // overloaded `Neg` on a non-Copy type (#262).
    "unary_operand_copies",
    // By-value captures of a closure passed straight to a call (#230).
    "closure_arg_move_captures",
];

/// Tooltip-level expectations per snippet. `must_contain` strings have to
/// appear verbatim as a `data-tooltip-text` value somewhere in the rendered
/// timeline; `must_not_contain` strings must not. Both checks are
/// containment, so adding new tooltips downstream doesn't break the test —
/// only removing a listed one (or producing one that's been forbidden) does.
struct TooltipExpect {
    name: &'static str,
    must_contain: &'static [&'static str],
    must_not_contain: &'static [&'static str],
}

const EXPECTED_TOOLTIPS: &[TooltipExpect] = &[
    // ─── Ownership / moves / copies ──────────────────────────────────
    TooltipExpect {
        name: "testMove",
        must_contain: &[
            "Move from String::from to s",
            "Move from s to takes_and_drops",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "ownershipFunctions",
        must_contain: &[
            "Move from String::from to s",
            "Move from s to takes_ownership",
            "Copy from x to makes_copy",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "copy2Caller",
        // `y: u32` is Copy: returning it copies the value out. This
        // entry used to pin the arrow's "moved to the caller" text.
        must_contain: &[
            "Copy from f to x",
            // `y: u32` is Copy: returning it copies (the arrow's
            // tooltip used to say "moved" regardless).
            "y's resource is copied to the caller",
        ],
        must_not_contain: &[
            "y's resource is moved to the caller",
        ],
    },
    TooltipExpect {
        name: "copy_return_param",
        // i32 and bool returns are copies, never moves.
        must_contain: &[
            "x's resource is copied to the caller",
            "big's resource is copied to the caller",
        ],
        must_not_contain: &[
            "x's resource is moved to the caller",
            "big's resource is moved to the caller",
        ],
    },
    TooltipExpect {
        name: "return_shared_ref",
        // Returning a `&T` copies the reference (references are Copy),
        // never the String behind it. A returned String is still moved.
        must_contain: &[
            "a reference from s is returned to the caller; the data it points to is not copied",
            "a reference from r is returned to the caller; the data it points to is not copied",
            "t's resource is moved to the caller",
        ],
        must_not_contain: &[
            "s's resource is copied to the caller",
            "s's resource is moved to the caller",
            "r's resource is copied to the caller",
            "r's resource is moved to the caller",
        ],
    },
    TooltipExpect {
        name: "reassign_with_drop",
        must_contain: &[
            "Move from x to y",
            "y acquires ownership of a new resource; its previous resource is dropped",
        ],
        must_not_contain: &[],
    },

    // ─── References: borrow + return ─────────────────────────────────
    TooltipExpect {
        name: "basic_ref",
        must_contain: &[
            "Immutable borrow from x to y",
            "Return immutably borrowed resource from y to x",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "basic_mutref",
        // `let y = &mut x; *y += 3;` — y's lifeline describes a
        // reference (not an owner), and the write through `*y`
        // modifies x's resource rather than acquiring one.
        must_contain: &[
            "Mutable borrow from x to y",
            "Return mutably borrowed resource from y to x",
            "y holds a mutable reference; it can read and write the borrowed resource",
            "the resource y points to is modified through *y",
        ],
        must_not_contain: &[
            "y is the owner of the resource",
            "*y acquires ownership of a resource",
        ],
    },
    TooltipExpect {
        name: "mutref_param_push_str",
        // Rust Book 4.2 `fn change(some_string: &mut String)`.
        must_contain: &[
            "some_string is a mutable borrow from the caller",
            "some_string holds a mutable reference; it can read and write the borrowed resource",
            "push_str reads from/writes to some_string (mutable reborrow — some_string can't be used until push_str returns)",
        ],
        must_not_contain: &[
            "some_string is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "mutref_after_shared_borrows",
        // Rust Book 4.2 NLL example: r1/r2 end, then `let r3 = &mut s`.
        must_contain: &[
            "Mutable borrow from s to r3",
            "r3 holds a mutable reference; it can read and write the borrowed resource",
        ],
        must_not_contain: &[
            "r3 is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "mutref_self_method",
        // `fn push(&mut self, ..)` — self is a mutable borrow.
        must_contain: &[
            "self is a mutable borrow from the caller",
            "self holds a mutable reference; it can read and write the borrowed resource",
        ],
        must_not_contain: &[
            "self is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "vec_for_mut_deref",
        // Rust Book 8.1 `for i in &mut v { *i += 50; }`.
        must_contain: &[
            "Mutable borrow from v to i",
            "i holds a mutable reference; it can read and write the borrowed resource",
            "the resource i points to is modified through *i",
        ],
        must_not_contain: &[
            "i is the owner of the resource",
            "*i acquires ownership of a resource",
        ],
    },
    TooltipExpect {
        name: "hashmap_entry_count",
        // Rust Book 8.3 word count: `*count += 1` through the
        // `&mut` returned by `or_insert`; `text` is a `&str`.
        must_contain: &[
            "Mutable borrow from map to count",
            "the resource count points to is modified through *count",
            "text is bound to an immutable reference",
            "text holds an immutable reference; it can read the borrowed resource",
        ],
        must_not_contain: &[
            "*count acquires ownership of a resource",
            "text acquires ownership of a resource",
            "text is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "iter_mut_deref",
        // `for x in v.iter_mut() { *x *= 2; }`.
        must_contain: &[
            "iter_mut reads from/writes to v (implicitly mutably borrowed)",
            "the resource x points to is modified through *x",
        ],
        must_not_contain: &[
            "*x acquires ownership of a resource",
        ],
    },
    TooltipExpect {
        name: "deref_assign_copy",
        // `*r = y;` with y: i32 overwrites x through r; nothing is
        // initialized.
        must_contain: &[
            "Copy from y to *r",
            "the resource r points to is modified through *r (copied from y)",
            "r holds a mutable reference; it can read and write the borrowed resource",
        ],
        must_not_contain: &[
            "*r is initialized by copy from y",
            "r is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "move_mutref",
        // `let r2 = r;` moves the `&mut` itself; r2 holds the borrow,
        // it doesn't own s's resource.
        must_contain: &[
            "Move from r to r2",
            "r2 is bound to a mutable reference",
            "r2 holds a mutable reference; it can read and write the borrowed resource",
        ],
        must_not_contain: &[
            "r2 acquires ownership of a resource",
            "r2 is the owner of the resource",
            "r is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "box_deref_assign",
        // `*b += 1` modifies the heap value b owns; *b doesn't
        // acquire a new resource.
        must_contain: &[
            "b acquires ownership of a resource",
            "the resource b points to is modified through *b",
        ],
        must_not_contain: &[
            "*b acquires ownership of a resource",
        ],
    },
    TooltipExpect {
        name: "str_literal_ref",
        // `let s: &str = "hello";` — s borrows static string data.
        must_contain: &[
            "s is bound to an immutable reference",
            "s holds an immutable reference; it can read the borrowed resource",
            "t is initialized by copy from s",
        ],
        must_not_contain: &[
            "s acquires ownership of a resource",
            "s is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "match_str_literal_arms",
        // Every arm binds `msg: &'static str`; the join dot must not
        // claim msg acquired ownership.
        must_contain: &[
            "msg is bound to an immutable reference",
            "msg is bound to an immutable reference (in all branches above)",
            "msg holds an immutable reference; it can read the borrowed resource",
        ],
        must_not_contain: &[
            "msg acquired ownership of a resource (in all branches above)",
            "msg acquires ownership of a resource",
            "msg is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "cell_ref_return",
        // `r: &Cell<i32>` (returned by a fn, so its lifeline is in
        // FullPrivilege) is written through with `r.set(2)`. A shared
        // reference to a Cell/RefCell/Mutex can modify the referent, so
        // the lifeline must not claim r can't modify it, nor that r owns it.
        must_contain: &[
            "r holds an immutable reference; it can read the borrowed resource",
        ],
        must_not_contain: &[
            "r holds an immutable reference; it can read but not modify the borrowed resource",
            "r is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "testStaticB",
        must_contain: &[
            "Move from String::from to my_string",
            "Immutable borrow from my_string to my_str",
            "Return immutably borrowed resource from my_str to my_string",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "testMutB",
        must_contain: &[
            "Move from String::from to greeting",
            "Mutable borrow from greeting to r1",
            "Return mutably borrowed resource from r1 to greeting",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "reborrow",
        must_contain: &[
            "Mutable borrow from s to r",
            // r is itself `&mut String`, so the borrow into r2 is a
            // reborrow — `let r2 = &mut *r` — not a fresh borrow off
            // the owner. The wording reflects that.
            "Mutable reborrow from r to r2",
            "Return mutably borrowed resource from r2 to *r",
            "Return mutably borrowed resource from r to s",
            // r2 is itself `&mut String`, so push_str's call-site
            // dot picks up the implicit-reborrow suffix.
            "push_str reads from/writes to r2 (mutable reborrow — r2 can't be used until push_str returns)",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "reborrow_kinds",
        // Reborrow-ness is decided by the syntax at the `&` site:
        // only `&mut *r` / `&*r` borrow *through* r. `&mut r` /
        // `&r` borrow the reference variable itself — same source
        // RAP, different meaning, so the arrow labels must differ.
        must_contain: &[
            "Mutable reborrow from ra to ra2",
            "Mutable borrow from rb to rb2",
            "Immutable reborrow from rc to rc2",
            "Immutable borrow from rd to rd2",
            // Lend dots: a `&mut` source can't be used while the
            // borrow is live; an owner lent immutably is read-only.
            "ra's resource is mutably borrowed by ra2; ra can't be used while the borrow is live",
            "rb's resource is mutably borrowed by rb2; rb can't be used while the borrow is live",
            "c's resource is immutably borrowed by rc; c is read-only while the borrow is live",
            // A `&String` source was already read-only, so the
            // borrow adds no restriction worth stating.
            "rc's resource is immutably borrowed by rc2",
            "rd's resource is immutably borrowed by rd2",
        ],
        must_not_contain: &[
            "Mutable reborrow from rb to rb2",
            "Immutable borrow from rc to rc2",
            "Immutable reborrow from rd to rd2",
            "rc's resource is immutably borrowed by rc2; rc is read-only while the borrow is live",
        ],
    },
    TooltipExpect {
        name: "borrow_moved_on",
        // `let z = y;` moves the `&mut` on (and `let b = a;` copies
        // the `&`), so the lender stays unusable / read-only past the
        // first holder's last use. The lend-dot wording must not
        // tie the restriction to y's / a's last use.
        must_contain: &[
            "s's resource is mutably borrowed by y; s can't be used while the borrow is live",
            "t's resource is immutably borrowed by a; t is read-only while the borrow is live",
            "Return mutably borrowed resource from z to s",
            "Return immutably borrowed resource from b to t",
        ],
        must_not_contain: &[
            "s's resource is mutably borrowed by y; s can't be used until y's last use",
            "t's resource is immutably borrowed by a; t is read-only until a's last use",
        ],
    },
    TooltipExpect {
        name: "call_reborrow_kinds",
        // Call-site dot suffix depends on what the call does to the
        // reference argument, read off rustc's adjustments and the
        // call's result type rather than the argument's type alone.
        must_contain: &[
            // `push(y)`, push: `&mut String` → `()`: implicit
            // reborrow that ends when push returns.
            "push reads from/writes to y (mutable reborrow — y can't be used until push returns)",
            // `let w = first(y)` returns a `&mut String` derived
            // from y: the reborrow lives on in w after first returns.
            "first reads from/writes to y (mutable reborrow — y can't be used while the reborrow is live)",
            // `take(y)` into a generic `T`: y is moved, not reborrowed.
            "take reads from/writes to y",
            // `show(x)` with x: `&String`: the reference is copied.
            "show reads from x",
            "x's resource is immutably borrowed by show",
            "y's resource is mutably borrowed by push; y can't be used while the borrow is live",
        ],
        must_not_contain: &[
            "first reads from/writes to y (mutable reborrow — y can't be used until first returns)",
            "take reads from/writes to y (mutable reborrow — y can't be used until take returns)",
            "show reads from x (immutable reborrow — x is read-only until show returns)",
        ],
    },
    TooltipExpect {
        name: "interior_mut_lend",
        // `Cell::set`, `RefCell::borrow_mut`, `Mutex::lock` take
        // `&self` and mutate through it, so the lend dot must not
        // call the owner "read-only". A plain String still is.
        must_contain: &[
            "c's resource is immutably borrowed by set",
            "r's resource is immutably borrowed by borrow_mut",
            "shared's resource is immutably borrowed by borrow",
            "m's resource is immutably borrowed by lock",
            "s's resource is immutably borrowed by len; s is read-only while the borrow is live",
        ],
        must_not_contain: &[
            "c is read-only",
            "r is read-only",
            "shared is read-only",
            "m is read-only",
        ],
    },
    TooltipExpect {
        name: "nll_two_borrows",
        // Two consecutive &mut borrows, with the loan released between
        // them. The renderer should draw both borrow-and-return pairs
        // and a fn-call interaction at each `world(...)` site.
        must_contain: &[
            "Mutable borrow from x to y",
            "Return mutably borrowed resource from y to x",
            "Mutable borrow from x to z",
            "Return mutably borrowed resource from z to x",
            // y and z are both `&mut String`, so the call-site
            // dot picks up the implicit-reborrow suffix.
            "world reads from/writes to y (mutable reborrow — y can't be used until world returns)",
            "world reads from/writes to z (mutable reborrow — z can't be used until world returns)",
        ],
        must_not_contain: &[],
    },

    // ─── Field-projection cases (regression bar for #71/#72/#73) ─────
    TooltipExpect {
        name: "nested_struct_borrow",
        must_contain: &[
            "Move from String::from to r.a.b",
            "Immutable borrow from r.a.b to p",
            "Return immutably borrowed resource from p to r.a.b",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "nested_struct_move",
        must_contain: &[
            "Move from String::from to r.a.b",
            "Move from r.a.b to x",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "nested_struct_passref",
        must_contain: &[
            "Move from String::from to r.a.b",
            "read_a reads from r.a",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "field_method_call",
        must_contain: &[
            "Move from String::from to r.s",
            "push_str reads from/writes to r.s (implicitly mutably borrowed)",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "field_through_ref",
        must_contain: &[
            "Move from String::from to r.s",
            "Immutable borrow from r.s to p",
            "Return immutably borrowed resource from p to r.s",
        ],
        must_not_contain: &[],
    },

    // ─── Function calls / returns ────────────────────────────────────
    TooltipExpect {
        name: "move_return",
        must_contain: &[
            "Move from String::from to s",
            "s's resource is moved to the caller",
            "Move from make to r",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "take_and_return",
        must_contain: &[
            "Move from s to take_and_return",
            "Move from take_and_return to s",
        ],
        must_not_contain: &[],
    },

    // ─── Stdlib method calls ────────────────────────────────────────
    TooltipExpect {
        name: "string_push_str",
        // `&mut self` method → PassByMutableReference — rendered as
        // "X reads from/writes to Y" on the fn-icon dot.
        must_contain: &[
            "Move from String::from to s",
            "push_str reads from/writes to s (implicitly mutably borrowed)",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "vec_slice",
        // `let v = vec![..]` desugars through a macro into a Call whose
        // function never gets registered as a RAP. The fix in #75 emits a
        // plain Bind ("v acquires ownership") instead of crashing. `&v[..]`
        // attributes the borrow to `v` (Vec collapses via #76's non-local
        // ADT rule), matching how `&s` works for `s: String`.
        must_contain: &[
            "v acquires ownership of a resource",
            "Immutable borrow from v to p",
            "Return immutably borrowed resource from p to v",
        ],
        must_not_contain: &[
            // Vec internals shouldn't leak into the timeline as separate
            // columns (regression guard for `ty_is_special_owner`).
            "v.buf, immutable",
            "v.len, immutable",
        ],
    },
    TooltipExpect {
        name: "string_len",
        // `&self` method → PassByStaticReference — "X reads from Y" on
        // the fn-icon dot. The Copy return value lands in n.
        must_contain: &[
            "Move from String::from to s",
            "len reads from s (implicitly borrowed)",
            "Copy from len to n",
        ],
        must_not_contain: &[],
    },

    TooltipExpect {
        name: "chained_field_assign",
        // The chained write lands on `o.inner.x`'s column. Writing to
        // an i32 field is an assignment, not an acquisition (#178).
        must_contain: &[
            "o.inner.x, mutable",
            "o.inner.x is assigned a new value",
        ],
        must_not_contain: &[
            "o.inner.x acquires ownership of a resource",
        ],
    },
    TooltipExpect {
        name: "chained_field_assign_move",
        must_contain: &[
            "Move from s to p.name.first",
            "p.name.first acquires ownership of a new resource; its previous resource is dropped",
            "s's resource is moved",
            "s goes out of scope. No resource is dropped.",
            "p.name.first goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "assign_field_of_method_result",
        must_contain: &[
            "last_mut reads from/writes to v (implicitly mutably borrowed)",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "assign_unmodelled_place",
        // `a`, `b`, `c` are each moved into a place that has no column
        // (`self.name`, `r.name`, `v[0]`). `r.name = b` shares its line
        // with the end of r's borrow, which used to swallow b's move.
        must_contain: &[
            "a's resource is moved",
            "a goes out of scope. No resource is dropped.",
            "b's resource is moved",
            "b goes out of scope. No resource is dropped.",
            "c's resource is moved",
            "c goes out of scope. No resource is dropped.",
            "rename reads from/writes to u (implicitly mutably borrowed)",
        ],
        must_not_contain: &[
            "a goes out of scope. Its resource is dropped.",
            "b goes out of scope. Its resource is dropped.",
            "c goes out of scope. Its resource is dropped.",
        ],
    },

    // ─── User-defined inherent methods ──────────────────────────────
    TooltipExpect {
        name: "inherent_method_rectangle",
        // The Rectangle/area pattern is the canonical "method on a
        // user struct" shape. After #74's macro-descent fix the
        // inline `rect.area()` inside `print_area`'s `println!` body
        // now emits its own call-site arrow alongside the outer
        // `print_area(&r)` arrow.
        must_contain: &[
            "print_area reads from r",
            // `print_area(rect: &Rectangle)` calls `rect.area()`.
            // rect is a `&Rectangle`, and passing a shared reference
            // just copies it — rect is read-only before, during and
            // after the call — so no "reborrow / read-only until
            // area returns" note.
            "area reads from rect",
            "r acquires ownership of a resource",
            "r goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "area reads from rect (immutable reborrow — rect is read-only until area returns)",
        ],
    },

    // ─── Lifetimes ──────────────────────────────────────────────────
    TooltipExpect {
        name: "lifetime_excerpt",
        // Excerpt<'a> { p: &'a str } — `e.p` is rendered as a borrower
        // of `s`. Borrow + matching return both appear.
        must_contain: &[
            "Move from String::from to s",
            "Immutable borrow from s to e.p",
            "Return immutably borrowed resource from e.p to s",
        ],
        must_not_contain: &[],
    },

    // ─── Generics ───────────────────────────────────────────────────
    TooltipExpect {
        name: "generic_fn",
        must_contain: &[
            "Move from s to id",
            "Move from id to t",
        ],
        must_not_contain: &[],
    },

    // ─── Shadowing ──────────────────────────────────────────────────
    TooltipExpect {
        name: "shadowing_basic",
        // Both `let s = ...` lines should bind a String to `s`. The
        // first `s` is dropped at the shadow site.
        must_contain: &[
            "Move from String::from to s",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },

    // ─── Marker comments (skip / hide) ──────────────────────────────
    TooltipExpect {
        name: "skip_marker_let",
        // `let q = ...; // rustviz: skip` — every event touching `q`
        // is dropped. The rendered timeline must mention `s` but never
        // `q`.
        must_contain: &[
            "Move from String::from to s",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "q acquires ownership of a resource",
            "q goes out of scope. Its resource is dropped.",
            "Move from String::from to q",
        ],
    },
    TooltipExpect {
        name: "hide_marker_fn",
        // `// rustviz: hide` on `helper` — the call-site `Move s ->
        // helper` arrow still fires (Function RAP created at the call
        // site), but `helper`'s body isn't traversed so its
        // `some_string` parameter never appears as a column / state.
        must_contain: &[
            "Move from String::from to s",
            "Move from s to helper",
        ],
        must_not_contain: &[
            "some_string acquires ownership from the caller",
            "some_string goes out of scope. Its resource is dropped.",
        ],
    },

    // ─── Conditionals as expression RHS ─────────────────────────────
    TooltipExpect {
        name: "if_as_let_rhs",
        // `let s = if cond { String::from(..) } else { String::from(..) };`
        // Both branches' moves into `s` show up; `s` ends up the
        // owner regardless of branch.
        must_contain: &[
            "Move from String::from to s",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },

    // ─── Smart-pointer wrappers (#76) ───────────────────────────────
    // Box / Rc / Box<dyn T> render as a single owning column, not as
    // their internal struct internals. The `must_not_contain` list
    // pins the regression: if any of these strings come back, #84's
    // recursive struct-field walker is leaking wrapper internals
    // into the timeline again.
    TooltipExpect {
        name: "box_string",
        must_contain: &[
            "Move from Box::new to b",
        ],
        must_not_contain: &[
            "b.0, immutable",
            "b.0.pointer, immutable",
            "b.0.pointer.pointer, immutable",
            "b.0._marker, immutable",
            "b.1, immutable",
        ],
    },
    TooltipExpect {
        name: "rc_clone",
        // `Rc::clone(&r)` is just a regular function call: `&r` is
        // a static borrow into Rc::clone, the return value moves
        // into r2. Shared-ownership semantics aren't visualized
        // (per the issue's "out of scope for the panic fix" note).
        must_contain: &[
            "Move from Rc::new to r",
            "Move from Rc::clone to r2",
            "Rc::clone reads from r",
        ],
        must_not_contain: &[
            "r.ptr, immutable",
            "r.ptr.pointer, immutable",
            "r.phantom, immutable",
            "r.alloc, immutable",
            "r2.ptr, immutable",
            "r2.alloc, immutable",
        ],
    },
    TooltipExpect {
        name: "box_dyn",
        must_contain: &[
            "Move from Box::new to b",
        ],
        must_not_contain: &[
            "b.0, immutable",
            "b.0.pointer, immutable",
            "b.0._marker, immutable",
            "b.1, immutable",
        ],
    },

    // ─── Closure captures (#79) ──────────────────────────────────────
    //
    // Move/borrow events into a closure binding render with
    // capture-flavoured arrow labels (`Closure capture (move)` /
    // `… (immutable borrow)` / `… (mutable borrow)`) instead of the
    // generic `Move` / `Immutable borrow` / `Mutable borrow`. The
    // closure binding's scope-end also gets a "captured resources
    // are dropped" message in place of the generic "resource is
    // dropped" suffix.
    TooltipExpect {
        name: "closure_move_single",
        must_contain: &[
            "Move from String::from to s",
            "Closure capture (move) from s to f",
            "Closure f captures: s (moved)",
            // Scope-end and timeline tooltips both report the
            // exact count of move-captured resources, with
            // singular grammar at N == 1.
            "f goes out of scope. Its 1 captured resource is dropped.",
            "f owns a closure which owns 1 resource via capture",
        ],
        must_not_contain: &[
            // Generic Move-arrow label would obscure the capture.
            "Move from s to f",
            // Per-capture closure-side dots are suppressed in
            // favour of the combined Bind dot.
            "Closure f captures (moves) s's resource",
            // Generic owner state message would obscure the
            // closure-vs-resource distinction.
            "f is the owner of the resource",
            // Wrong pluralization guards.
            "owns 1 resources",
            "1 captured resources are dropped",
            // The previous count-less wording would slip through
            // a substring match if we didn't pin the new one.
            "Its captured resources are dropped",
        ],
    },
    TooltipExpect {
        name: "closure_move_multi",
        // Each captured upvar produces its own capture arrow on
        // the source side; on the closure side, a single Bind dot
        // enumerates every capture so neither tooltip masks the
        // other (#79 follow-up).
        must_contain: &[
            "Closure capture (move) from s to f",
            "Closure capture (move) from t to f",
            "Closure f captures: s (moved), t (moved)",
            "f goes out of scope. Its 2 captured resources are dropped.",
            "f owns a closure which owns 2 resources via capture",
        ],
        must_not_contain: &[
            "Move from s to f",
            "Move from t to f",
            // Per-capture closure-side dots suppressed.
            "Closure f captures (moves) s's resource",
            "Closure f captures (moves) t's resource",
            "f is the owner of the resource",
            "owns 2 resource via capture",
            "2 captured resource is dropped",
            "Its captured resources are dropped",
        ],
    },
    TooltipExpect {
        name: "closure_borrow_imm",
        // Non-`move` closure that reads its upvar → static borrow.
        // No move captures, so f's scope-end gets the plain owner
        // OOS message — there are no resources to drop. The borrow
        // returns at f's NLL last use (the call on the line after
        // the closure literal), not at the closure's lexical scope.
        must_contain: &[
            "Move from String::from to s",
            "Closure capture (immutable borrow) from s to f",
            "Closure f captures: s (immutably borrowed)",
            "Return immutably borrowed resource from f to s",
            "f's immutable borrow ends",
            "s's resource is no longer immutably borrowed",
            "f goes out of scope",
            // Borrow-only closure → f owns the closure value but
            // not any captured resource.
            "f owns a closure",
        ],
        must_not_contain: &[
            // Critically: the closure should *not* be modeled as a
            // move — `s` is still usable after `f` runs.
            "Move from s to f",
            "Closure capture (move) from s to f",
            "Immutable borrow from s to f",
            "Closure f captures an immutable reference to s",
            // No move captures → no "captured resources are dropped".
            "f goes out of scope. Its 1 captured resource is dropped.",
            "f goes out of scope. Its 2 captured resources are dropped.",
            "f goes out of scope. Its captured resources are dropped.",
            // Borrow-only closure → no upgrade to "owns N resources via capture".
            "f owns a closure which owns",
            "f is the owner of the resource",
        ],
    },
    // ─── Tuple destructuring (#86) ──────────────────────────────────
    TooltipExpect {
        name: "tuple_destructure",
        // `let (a, b) = (String::from("x"), String::from("y"));` —
        // pairs sub-pat to sub-expr so each element renders as its
        // own move into its own column, with its own scope-end drop.
        must_contain: &[
            "Move from String::from to a",
            "Move from String::from to b",
            "a goes out of scope. Its resource is dropped.",
            "b goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "slice_destructure",
        // `let [a, b] = [String::from("x"), String::from("y")];` —
        // same element-wise pairing as tuple_destructure, but
        // against an array literal RHS.
        must_contain: &[
            "Move from String::from to a",
            "Move from String::from to b",
            "a goes out of scope. Its resource is dropped.",
            "b goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },

    TooltipExpect {
        name: "closure_borrow_mut",
        // Non-`move` closure that mutates its upvar → mutable borrow.
        // Same as the immutable case for scope-end: no resources
        // are dropped at the closure binding's OOS, and the
        // borrow returns at the closure's NLL last use.
        must_contain: &[
            "Move from String::from to s",
            "Closure capture (mutable borrow) from s to f",
            "Closure f captures: s (mutably borrowed)",
            "Return mutably borrowed resource from f to s",
            "f's mutable borrow ends",
            "s's resource is no longer mutably borrowed",
            "f goes out of scope",
            "f owns a closure",
        ],
        must_not_contain: &[
            "Move from s to f",
            "Closure capture (move) from s to f",
            "Mutable borrow from s to f",
            "Closure f captures a mutable reference to s",
            "f goes out of scope. Its 1 captured resource is dropped.",
            "f goes out of scope. Its 2 captured resources are dropped.",
            "f goes out of scope. Its captured resources are dropped.",
            "f owns a closure which owns",
            "f is the owner of the resource",
        ],
    },

    // ─── Ref-arg call sites inside `println!` (#74) ─────────────────
    // The inline `r.method()` / `f(&r)` inside `println!("{}", …)`
    // emits its call-site arrow once the visitor descends through
    // synthetic macro scaffolding. Before #74's fix, the outer
    // synthetic block's expansion span discarded the entire arg
    // subtree before the MethodCall / Call arms could fire.
    TooltipExpect {
        name: "method_call_in_println",
        must_contain: &[
            "get reads from r (implicitly borrowed)",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "freefn_call_in_println",
        must_contain: &[
            "get reads from r",
        ],
        must_not_contain: &[],
    },

    // ─── Conditionals (#87, #108) ────────────────────────────────────
    TooltipExpect {
        name: "match_as_let_rhs",
        // `let s = match n { 0 => ..., _ => ... };`. Pre-fix: zero
        // Move arrows (match_rhs had no Match arm), and arm labels
        // came out as `Int(Pu128(0), Unsuffixed)` / `Wild`. We pin
        // both the move-arrow recursion and the source-form arm
        // labels here.
        must_contain: &[
            "Move from String::from to s",
        ],
        must_not_contain: &[
            "Int(Pu128(0), Unsuffixed)",
            "Bool(",
            "Wild",
        ],
    },
    TooltipExpect {
        name: "if_else_move_join",
        // Variable `s` consumed in if branch, only borrowed in else
        // branch. Mixed move/alive merge: `s` is maybe-moved, and a
        // runtime drop flag decides at scope end whether it's dropped
        // (#179; it used to say "dropped at the branch's end").
        must_contain: &[
            "Move from s to consume",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &["merge"],
    },
    TooltipExpect {
        name: "if_else_rebind_join",
        // Both branches consume `s` and reassign it from a fresh
        // String::from. After the conditional `s` owns a freshly-bound
        // resource regardless of branch.
        must_contain: &[
            "s acquired ownership of a resource (in all branches above)",
        ],
        must_not_contain: &[
            "merge",
            "may have been moved",
        ],
    },
    TooltipExpect {
        name: "nested_if_move_join",
        // Inner if: consume on one inner branch, borrow on the
        // other → mixed. Outer if: the else consumes, and the then-arm
        // ends maybe-moved, so the outer merge is mixed too (#179).
        must_contain: &[
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            // #179: a path keeps `s` (inner branch that only borrows),
            // so the outer merge is maybe-moved, not all-moved.
            "s was moved or dropped in every branch above",
            "merge",
        ],
    },
    TooltipExpect {
        name: "if_as_let_rhs_multiline",
        // Multi-line `let s = if cond { ... } else { ... };`. Both
        // arms acquire `s`, so the merge is BoundHere. The pre-
        // first-acquire rows in each arm are now Gray-Full so the
        // rendered column stays continuous from the leading
        // converge into the acquire event without a visible gap.
        must_contain: &[
            "s acquired ownership of a resource (in all branches above)",
        ],
        must_not_contain: &[
            "may have been moved",
            "moved or dropped in every branch",
        ],
    },
    TooltipExpect {
        name: "if_else_mut_reassign",
        // mut binding consumed and reassigned in each arm. At the
        // merge `s` is bound again (every branch rebound) so the
        // join message is BoundHere, not MovedAfter.
        must_contain: &[
            "Move from s to consume",
            "Move from String::from to s",
            "s acquired ownership of a resource (in all branches above)",
        ],
        must_not_contain: &[
            "may have been moved",
            "moved or dropped in every branch",
        ],
    },
    TooltipExpect {
        name: "if_else_move_both",
        // Both arms consume → merge is all-moved (no may-have-been
        // hedge, no drop dot since there's no didn't-move branch
        // to insert an implicit drop into).
        must_contain: &[
            "Move from s to consume",
            "s was moved or dropped in every branch above",
        ],
        must_not_contain: &[
            "may have been moved",
            "in branches where it was not",
        ],
    },
    TooltipExpect {
        name: "deep_nested_if",
        // Three-level nesting. Each merge classifies on its own
        // branches' end states; a branch ending maybe-moved (after a
        // nested mixed merge) counts as one that may still hold `s`,
        // so every merge here is mixed (#179).
        must_contain: &[
            "Move from s to consume",
            // At least one inner merge is the mixed implicit-drop
            // case (consume on one path, borrow on the other).
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            // #179: a path keeps `s` (inner branch that only borrows),
            // so the outer merge is maybe-moved, not all-moved.
            "s was moved or dropped in every branch above",
            // The outer merge no longer hedges with "at least one";
            // the deepest mixed merges still do, but the corpus
            // pins exact strings so an outer regression to the
            // hedge wording is caught here too.
            "may have been moved (consumed in at least one branch above)",
        ],
    },
    TooltipExpect {
        name: "if_let_no_else",
        // Single-arm if-let inlines: destructure Move from opt to
        // x, then a borrow from x to show. No Branch event, so no
        // merge tooltip wording.
        must_contain: &[
            "Move from Some to opt",
            "Move from opt to x",
            "show reads from x",
            "x goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "may have been moved",
            "moved or dropped in every branch",
            "in branches where it was not",
            "in a conditional expression",
        ],
    },
    TooltipExpect {
        name: "match_tuple_destructure",
        // Tuple pattern over a single tuple-typed scrutinee. Each
        // inner binding destructures out of the same single parent
        // (`pair`); pre-fix this panicked with index-out-of-bounds.
        // Single-arm match also inlines.
        must_contain: &[
            "show reads from x",
            "show reads from y",
            "x goes out of scope. Its resource is dropped.",
            "y goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "may have been moved",
            "moved or dropped in every branch",
            "in branches where it was not",
            "in a conditional expression",
        ],
    },
    TooltipExpect {
        name: "match_one_arm",
        // Single-arm match with an irrefutable binding. Body shows
        // inline; the pattern's Move (s → x) emits on the
        // destructure line; show borrows x; x drops at arm end.
        // No Branch event, no merge tooltip.
        must_contain: &[
            "Move from s to x",
            "show reads from x",
            "x goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "may have been moved",
            "moved or dropped in every branch",
            "in branches where it was not",
            "in a conditional expression",
        ],
    },
    TooltipExpect {
        name: "match_three_arms",
        // 3-arm match: consume, borrow, borrow. Mixed merge →
        // implicit-drop wording. Each arm gets its own column;
        // even/odd N just affects placement, not classification.
        must_contain: &[
            "Move from s to consume",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "may have been moved (consumed in at least one branch above)",
            "moved or dropped in every branch",
        ],
    },
    TooltipExpect {
        name: "if_no_else",
        // Plain `if cond { body }`: the Branch event is now
        // skipped (single-arm conditional). The Move from `s` to
        // `consume` shows inline on the parent timeline, with no
        // merge tooltip and no "live in a conditional expression"
        // labelling.
        must_contain: &[
            "Move from s to consume",
        ],
        must_not_contain: &[
            "If",
            "Else",
            "merge",
            "may have been moved",
            "moved or dropped in every branch",
            "in branches where it was not",
            "in a conditional expression",
            "acquired ownership of a resource (in all branches above)",
        ],
    },

    // ─── Conditionals composed with iteration / capture ──────────────
    TooltipExpect {
        name: "cond_with_for_loop",
        // for-loop body in if-arm borrows `s`; else consumes.
        // Mixed merge → drop dot + implicit-drop wording.
        must_contain: &[
            "Move from s to consume",
            "show reads from s",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "may have been moved (consumed in at least one branch above)",
            "moved or dropped in every branch",
        ],
    },
    TooltipExpect {
        name: "cond_with_closure",
        // Borrow-only closure captures `s` inside if-arm; else
        // consumes. Mixed merge → drop dot.
        must_contain: &[
            "Closure capture (immutable borrow) from s to f",
            "Move from s to consume",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "may have been moved (consumed in at least one branch above)",
            "moved or dropped in every branch",
        ],
    },
    TooltipExpect {
        name: "cond_with_move_closure",
        // `move` closure captures `s` (consuming it) in if-arm;
        // else consumes directly. Both arms end without `s` →
        // all-moved wording.
        must_contain: &[
            "Closure capture (move) from s to f",
            "Move from s to consume",
            "s was moved or dropped in every branch above",
        ],
        must_not_contain: &[
            "may have been moved",
            "in branches where it was not",
        ],
    },
    TooltipExpect {
        name: "if_inside_for",
        // if/else inside a for-loop body, both inner arms borrow
        // the loop variable. Inner merge classifies as Unchanged,
        // so no merge wording surfaces.
        must_contain: &[
            // `for x in &xs` makes x: `&String`; passing it to show
            // copies the shared reference, so no reborrow note.
            "show reads from x",
        ],
        must_not_contain: &[
            "show reads from x (immutable reborrow — x is read-only until show returns)",
            "may have been moved",
            "moved or dropped in every branch",
            "in branches where it was not",
        ],
    },
    TooltipExpect {
        name: "closure_with_cond",
        // Closure body wraps an if/else over a captured variable;
        // both inner arms borrow it. The closure-capture event is
        // what surfaces at the outer scope (the if's body events
        // live inside the closure, not on the parent timeline).
        // No merge wording — the outer scope sees a regular
        // closure capture + return, not a Branch.
        must_contain: &[
            "Closure capture (immutable borrow) from s to f",
            "Return immutably borrowed resource from f to s",
        ],
        must_not_contain: &[
            "may have been moved",
            "moved or dropped in every branch",
            "in branches where it was not",
        ],
    },
    TooltipExpect {
        name: "if_let_inside_for",
        // Single-arm if-let inlines per #116 — no Branch event,
        // no merge wording. Loop body's per-iteration destructure
        // shows as inline events.
        must_contain: &[
            // `if let Some(inner) = x` where x: `&Option<String>`
            // gives inner: `&String`; passing it to show copies the
            // shared reference, so no reborrow note.
            "show reads from inner",
        ],
        must_not_contain: &[
            "show reads from inner (immutable reborrow — inner is read-only until show returns)",
            "may have been moved",
            "moved or dropped in every branch",
            "in branches where it was not",
            "in a conditional expression",
        ],
    },
    TooltipExpect {
        name: "match_with_closure_arms",
        // Each arm declares its own closure that borrows `s`.
        // The capture events are per-arm. The shared scrutinee
        // `s` stays alive throughout — no may-have-been-moved
        // wording for it.
        must_contain: &[
            "Closure capture (immutable borrow) from s to f",
            "Closure capture (immutable borrow) from s to g",
        ],
        must_not_contain: &[
            "may have been moved (consumed in at least one branch above)",
        ],
    },

    // ─── `?` operator (#140) ────────────────────────────────────────
    TooltipExpect {
        name: "question_mark",
        // `let n: i32 = "5".parse()?;` — the `?` desugar's Match is
        // unwrapped to its operand, so `n` is a copy out of the
        // `parse` call. Pre-fix match_rhs walked the desugar's
        // synthetic arms and emitted two anonymous "n is bound to a
        // value" Binds on line 2 with no arrow from `parse`.
        must_contain: &[
            "Copy from parse to n",
            "n is initialized by copy from parse",
            "Copy from n to Ok",
        ],
        must_not_contain: &[
            "n is bound to a copyable value",
        ],
    },
    TooltipExpect {
        name: "question_mark_chain",
        // `let n = "5".parse::<i32>().map(|x| x + 1)?;` — same
        // shape, with the source being the outermost call in the
        // chain (`map`).
        must_contain: &[
            "Copy from map to n",
            "n is initialized by copy from map",
        ],
        must_not_contain: &[
            "n is bound to a copyable value",
        ],
    },
    TooltipExpect {
        name: "triple_colon_source",
        // The mis-split (#181) produced no false tooltip, only a truncated
        // code panel and a timeline starting mid-`<text>`; the panel
        // contents are pinned by `source_with_triple_colon_splits_correctly`.
        must_contain: &[
            "Move from String::from to s",
            "Move from s to t",
        ],
        must_not_contain: &[],
    },
    // ─── Coverage gaps closed (#139) ───────────────────────────────
    TooltipExpect {
        name: "match_range_pattern",
        // Range patterns in match arms (`0..=4`, `5..=9`, `_`).
        // All three arms borrow `s`; merge classifies as Unchanged,
        // so no merge wording. PatKind::Range is walked silently —
        // no per-arm event from the range itself.
        must_contain: &[
            "Move from String::from to s",
            "show reads from s",
        ],
        must_not_contain: &[
            "may have been moved",
            "moved or dropped in every branch",
        ],
    },
    TooltipExpect {
        name: "match_with_guard",
        // Pattern guard (`x if x > 0`). The pattern `x` binds from
        // the scrutinee (`n`, Copy) — that's the "Copy from n to x"
        // event. The guard expression itself is never visited, so
        // nothing is asserted about it. Both arms borrow `s`; merge
        // is Unchanged.
        must_contain: &[
            "Move from String::from to s",
            "Copy from n to x",
            "show reads from s",
        ],
        must_not_contain: &[
            "may have been moved",
            "moved or dropped in every branch",
        ],
    },
    TooltipExpect {
        name: "const_basic",
        // `let x = N;` where N is a const i32 → Copy treated as a
        // Bind from Anonymous (the const has no column). Then
        // `let y = x` is a regular Copy from x to y.
        must_contain: &[
            "x is bound to a copyable value",
            "y is initialized by copy from x",
            "Copy from x to y",
        ],
        must_not_contain: &[
            // The const itself shouldn't get a column or events.
            "N is the owner",
            "N holds a copyable value",
            "N goes out of scope",
            // No Move from N — const reads aren't moves.
            "Move from N to x",
        ],
    },
    TooltipExpect {
        name: "static_basic",
        // `let s = GREETING;` where GREETING is a `&'static str`.
        // The static itself isn't a function-local RAP; `s` borrows
        // from an Anonymous source (external memory). show(s) reads
        // through s.
        must_contain: &[
            "s holds a reference",
            "show reads from s",
        ],
        must_not_contain: &[
            // GREETING shouldn't get a column.
            "GREETING is the owner",
            "GREETING holds",
            "GREETING goes out of scope",
        ],
    },
    TooltipExpect {
        name: "unsafe_raw_ptr",
        // Raw-pointer code through an `unsafe` block. The plugin
        // doesn't model raw pointers (#194): the `let p = &mut x as
        // *mut i32` line shows no borrow of x, and p's lifeline isn't
        // tied to x. The write `*p = 2` is described correctly, as a
        // modification through *p (it used to read "*p is bound to a
        // value", as if *p were a fresh binding; #167's wording fixed
        // that). Mainly guards against a future change panicking on
        // raw pointers; update these strings when #194 is fixed.
        must_contain: &[
            "x is bound to a copyable value",
            "the resource p points to is modified through *p",
        ],
        must_not_contain: &["*p is bound to a copyable value"],
    },
    TooltipExpect {
        name: "static_mut_write",
        // `COUNTER += inc` and `LOG = v` write to `static mut`s. Both
        // used to panic in resource_of_lhs. The statics get no column;
        // `v` is moved into LOG, so it must not be shown as dropped at
        // the end of main.
        must_contain: &[
            "v's resource is moved",
            "v goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "v goes out of scope. Its resource is dropped.",
            "COUNTER",
            "LOG",
        ],
    },
    // ─── Unions (#131) ──────────────────────────────────────────────
    TooltipExpect {
        name: "union_basic",
        // Opaque-owner treatment: `u` gets a single column. The
        // construction is a Bind (acquires); field writes, the
        // `unsafe { u._a }` read, and the `&u._b` borrow are all
        // events on `u`'s own column; passing `u` to show is a
        // Move, and the post-move OOS reports no resource dropped.
        // No per-field timelines for `_a` / `_b` — unions don't
        // have a statically-known active field.
        must_contain: &[
            "u acquires ownership of a resource",
            // `u._a = y;` — the field write lands on u's column. It's
            // a write, not an initialization (#191).
            "Copy from y to u",
            "u is written to (copied from y); writing to a union never drops its old contents",
            // `let x = unsafe { u._a };` — a copy out of u.
            "Copy from u to x",
            "x is initialized by copy from u",
            "u's resource is copied",
            // `let r = unsafe { &u._b };` — u is the lender.
            "Immutable borrow from u to r",
            "Return immutably borrowed resource from r to u",
            "u's resource is moved",
            "Move from u to show",
            "u goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            // Pre-fix the AdtKind::Union arm panicked; a renderer
            // that treated unions as structs would create field
            // RAPs `u._a` / `u._b` with their own labels — guard
            // against either regression. (Matching is exact, so
            // name the column-label tooltips explicitly.)
            "u._a",
            "u._b",
            "u._a, mutable",
            "u._b, mutable",
            // The read / borrow must not lose their source.
            "x is bound to a copyable value",
            "u is initialized by copy from y",
            "Return immutably borrowed resource from r to *r",
        ],
    },
    TooltipExpect {
        name: "enum_param",
        // `fn area(s: Shape)` — the enum param is one owner column
        // that acquires from the caller; the match arms copy their
        // payloads out of it.
        must_contain: &[
            "s acquires ownership from the caller",
            "Partial copy from s to r",
            "Partial copy from s to w",
            "Move from c to area",
        ],
        must_not_contain: &[
            // Pre-#131 the param was registered like a struct and
            // got a phantom column for the first variant's field.
            "s.0, immutable",
            "s.0 goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "tuple_struct_field",
        // `struct Color(u8, u8, u8)`: one column per field, so a read
        // of `black.0` is a copy out of that field's column, and
        // `black.1 = 10` lands on `black.1`. Pre-fix: panic.
        must_contain: &[
            "Move from Color to black",
            "black acquires ownership of a resource",
            "Copy from black.0 to r",
            "r is initialized by copy from black.0",
        ],
        must_not_contain: &[
            // The read must not lose its source.
            "r is bound to a copyable value",
        ],
    },
    // ─── Assignment through a deref of a temporary ───────────────────
    TooltipExpect {
        name: "refcell_deref_assign_op",
        // `*c.borrow_mut() += 1`: the method call still produces its
        // normal event on `c`; the temporary `RefMut` gets nothing, and
        // `c` keeps its resource.
        must_contain: &[
            "Move from RefCell::new to c",
            "borrow_mut borrows c immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)", // #203
            "c goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "c's resource is moved",
            "c goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "refcell_deref_assign_move",
        // `*cell.borrow_mut() = v` moves `v` into the cell; `cell`
        // itself is only borrowed by the call.
        must_contain: &[
            "borrow_mut borrows cell immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)", // #203
            "v's resource is moved",
            "v goes out of scope. No resource is dropped.",
            "cell goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "cell's resource is moved",
            "cell goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "rc_refcell_deref_assign",
        // Book ch15 Rc<RefCell<T>>: `*value.borrow_mut() += 10`.
        must_contain: &[
            "Rc::clone borrows value immutably (a shared borrow); value has interior mutability, so its contents can change even through a shared borrow", // #203
            "Move from Rc::clone to a",
            "borrow_mut borrows value immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)", // #203
            "value goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "value's resource is moved",
        ],
    },
    TooltipExpect {
        name: "deref_method_result_assign",
        // `*v.last_mut().unwrap() = 5` and `*map.entry(k).or_insert(0) += 1`:
        // the base receiver's call is shown; `k` is moved into `entry`.
        must_contain: &[
            "last_mut reads from/writes to v (implicitly mutably borrowed)",
            "entry reads from/writes to map (implicitly mutably borrowed)",
            "Move from k to entry",
            "v goes out of scope. Its resource is dropped.",
            "map goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "v's resource is moved",
            "map's resource is moved",
        ],
    },
    // ─── One Function RAP receiving several values ───────────────────
    TooltipExpect {
        name: "cons_list_nested",
        // Book ch15 cons list. The outer constructor's value moves into
        // `list`; the inner `Cons` / `Box::new` calls are fn-to-fn
        // hand-offs, which (as for any nested call) draw no arrow.
        must_contain: &[
            "Move from Cons to list",
            "Move from list to _l",
            "list goes out of scope. No resource is dropped.",
            "_l goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "list goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "cons_list_qualified_nested",
        must_contain: &[
            "Move from List::Cons to list",
            "Move from list to _l",
            "_l goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "list goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "fn_called_twice",
        must_contain: &[
            "Move from s1 to id",
            "Move from id to a",
            "Move from s2 to id",
            "Move from id to b",
            "a goes out of scope. Its resource is dropped.",
            "b goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s1 goes out of scope. Its resource is dropped.",
            "s2 goes out of scope. Its resource is dropped.",
        ],
    },
    // ─── Compiler-generated code ─────────────────────────────────────
    TooltipExpect {
        name: "derive_debug",
        // `#[derive(Debug, Clone, PartialEq)]` generates `fmt(&self,
        // f: &mut Formatter)`, `clone(&self)` and `eq(&self, other)`.
        // None of that is user code, so no `f` / `other` columns.
        // The hand-written `impl Rectangle` and `impl Display` still
        // render (`out` is the Display impl's own parameter).
        must_contain: &[
            "self is an immutable borrow from the caller",
            "out is a mutable borrow from the caller",
            "area reads from rect1 (implicitly borrowed)",
        ],
        must_not_contain: &[
            "f, immutable",
            "f holds a mutable reference",
            "f is a mutable borrow from the caller",
            "other, immutable",
            "other is an immutable borrow from the caller",
        ],
    },
    TooltipExpect {
        name: "match_arm_macro_body",
        // Bare-macro arm bodies (`Some(s) => println!(..)`): the
        // bindings drop at the end of their arm. Position is checked
        // by `match_arm_macro_body_stays_on_canvas`.
        must_contain: &[
            "Partial move from opt to s",
            "s goes out of scope. Its resource is dropped.",
            "n goes out of scope",
            "e goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },

    // ─── Receivers consumed by `self` methods; `String +` (#172) ────
    TooltipExpect {
        name: "self_method_into_iter",
        // `IntoIterator::into_iter` takes `self`, and `String + &str`
        // is `Add::add(self, &str)`: `v` and `s1` are moved into the
        // call, so nothing is left for them to drop at the end of
        // `main` (the old output claimed both were dropped there, a
        // double drop). `&s2` is only borrowed.
        must_contain: &[
            "Move from v to into_iter",
            "v's resource is moved",
            "v goes out of scope. No resource is dropped.",
            "Move from s1 to +",
            "s1's resource is moved",
            "s1 goes out of scope. No resource is dropped.",
            "+ reads from s2",
            "s2 goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "v goes out of scope. Its resource is dropped.",
            "s1 goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "self_method_option",
        // `Option::expect` / `Result::unwrap_or_else` take `self`:
        // moves. `Option<i32>::unwrap_or` takes `self` too, but the
        // receiver is Copy, so it's a copy and `c` stays usable.
        // `s.clone()` / `c.is_some()` take `&self`: borrows only.
        must_contain: &[
            "Move from maybe to expect",
            "maybe goes out of scope. No resource is dropped.",
            "Move from r to unwrap_or_else",
            "r goes out of scope. No resource is dropped.",
            "Copy from c to unwrap_or",
            "c's resource is copied",
            "clone reads from s (implicitly borrowed)",
            "is_some reads from c (implicitly borrowed)",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "maybe goes out of scope. Its resource is dropped.",
            "r goes out of scope. Its resource is dropped.",
            "Move from c to unwrap_or",
            "c's resource is moved",
            "Move from s to clone",
            "s's resource is moved",
        ],
    },
    TooltipExpect {
        name: "self_method_user",
        // A user `fn into_name(self)` consumes `p`; UFCS
        // `Vec::into_iter(v)` consumes `v` (that one already worked
        // through the Call path). `p.name` is still shown as owning
        // and dropped after the whole-struct move: that's #174.
        must_contain: &[
            "Move from p to into_name",
            "p's resource is moved",
            "p goes out of scope. No resource is dropped.",
            "Move from v to Vec::into_iter",
            "v goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "p goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "self_method_ref_receiver",
        // `r.into_iter()` with `r: &Vec<i32>` is `<&Vec as
        // IntoIterator>::into_iter(self)`: `self` is the reference,
        // so nothing is moved out of `v`, which still owns (and
        // drops) its vector. `v.len()` takes `&self`. `n` (a usize)
        // is initialized by copy from `count`, on the line where r's
        // borrow ends (#190).
        must_contain: &[
            "into_iter reads from r",
            "len reads from v (implicitly borrowed)",
            "v goes out of scope. Its resource is dropped.",
            "n is initialized by copy from count",
            "Copy from count to n",
        ],
        must_not_contain: &[
            "Move from r to into_iter",
            "Move from v to into_iter",
            "v's resource is moved",
            "v goes out of scope. No resource is dropped.",
            "n goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "string_add_in_fn",
        // `s + "!"` as a fn's tail expression moves the parameter `s`
        // into `+` (the old output said `s` was dropped at the end of
        // `shout`). `a == b` takes both operands by reference: no
        // move, no drop change.
        must_contain: &[
            "Move from s to +",
            "s goes out of scope. No resource is dropped.",
            "Move from a to +",
            "a goes out of scope. No resource is dropped.",
            "+ reads from b",
            "b goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "a goes out of scope. Its resource is dropped.",
            "Move from b to +",
            "b's resource is moved",
            "b goes out of scope. No resource is dropped.",
        ],
    },
    // ─── Struct fields follow whole-struct transfers (#174) ─────────
    TooltipExpect {
        name: "struct_move_drop",
        // `drop(c)` moves `c` and with it `c.data`; nothing is left
        // for `c.data` to drop at the end of main.
        must_contain: &[
            "Move from c to drop",
            "c.data's resource is moved",
            "c.data goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "c.data goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_move_let",
        // `let d = c;` moves each field into the matching field of `d`.
        must_contain: &[
            "Move from c to d",
            "Move from c.data to d.data",
            "Move from c.n to d.n",
            "c.data goes out of scope. No resource is dropped.",
            "d.data goes out of scope. Its resource is dropped.",
            // Copy-typed field: plain scope end, nothing to drop.
            "d.n goes out of scope",
        ],
        must_not_contain: &[
            "c.data goes out of scope. Its resource is dropped.",
            "d.data goes out of scope. No resource is dropped.",
            "d.n goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_move_fn_param",
        // Caller side: `c.data` moves with `c`. Callee side: the
        // by-value param's field owns the String and drops it (both
        // fns name their struct `c`, so only the moved-out message
        // tells the two `c.data` columns apart here).
        must_contain: &[
            "Move from c to take",
            "c.data's resource is moved",
            "c.data goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "struct_update_syntax",
        // `..user1` moves `username`, copies `active`, and leaves
        // `user1.email` (overridden) owned by user1.
        must_contain: &[
            "Move from user1.username to user2.username",
            "Copy from user1.active to user2.active",
            "user1.username goes out of scope. No resource is dropped.",
            "user1.email goes out of scope. Its resource is dropped.",
            "user2.username goes out of scope. Its resource is dropped.",
            "user2.active goes out of scope",
        ],
        must_not_contain: &[
            "user1.username goes out of scope. Its resource is dropped.",
            "user2.username goes out of scope. No resource is dropped.",
            "user2.active goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_field_init_shorthand",
        // `User { active: true, username, email }` as build_user's
        // result: the shorthand fields move the parameters out.
        must_contain: &[
            "email's resource is moved to the caller",
            "username's resource is moved to the caller",
            "email goes out of scope. No resource is dropped.",
            "u.email goes out of scope. Its resource is dropped.",
            "u.username goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "email goes out of scope. Its resource is dropped.",
            "username goes out of scope. Its resource is dropped.",
            "u.email goes out of scope. No resource is dropped.",
            "u.username goes out of scope. No resource is dropped.",
            "u.active goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_nested_whole_move",
        must_contain: &[
            "Move from r to s",
            "Move from r.a to s.a",
            "Move from r.a.b to s.a.b",
            "r.a.b goes out of scope. No resource is dropped.",
            "s.a.b goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "r.a.b goes out of scope. Its resource is dropped.",
            "s.a.b goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_copy_whole",
        // Copy struct: the fields are copied, the source stays usable.
        must_contain: &[
            "Copy from p to q",
            "Copy from p.x to q.x",
            "q.x is initialized by copy from p.x",
            "q.x goes out of scope",
        ],
        must_not_contain: &[
            "q.x goes out of scope. No resource is dropped.",
            "p.x's resource is moved",
        ],
    },
    TooltipExpect {
        name: "struct_return_whole",
        must_contain: &[
            "c's resource is moved to the caller",
            "c.data's resource is moved to the caller",
            "c.data goes out of scope. No resource is dropped.",
            "x.data acquires ownership of a resource",
            "x.data goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "c.data goes out of scope. Its resource is dropped.",
            "x.data goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_reassign_whole",
        // `d = c;` drops the old `d.data` and moves `c.data` in.
        must_contain: &[
            "d acquires ownership of a new resource; its previous resource is dropped",
            "d.data acquires ownership of a new resource; its previous resource is dropped",
            "Move from c.data to d.data",
            "c.data goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "c.data goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_update_return",
        // `User { name, ..u }` returned: `name`, `u.email` move and
        // `u.active` copies to the caller; the overridden `u.name`
        // is dropped with `u`. In main, `u1` (moved into rename) has
        // nothing left and `u2`'s fields own the result.
        must_contain: &[
            "name's resource is moved to the caller",
            "u.email's resource is moved to the caller",
            "u.active's resource is copied to the caller",
            "u.name goes out of scope. Its resource is dropped.",
            "u.email goes out of scope. No resource is dropped.",
            "u1.email goes out of scope. No resource is dropped.",
            "u2.email goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "name goes out of scope. Its resource is dropped.",
            "u.name goes out of scope. No resource is dropped.",
            "u1.email goes out of scope. Its resource is dropped.",
            "u2.email goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_partial_move_two",
        // Partial moves keep working per field (unchanged by #174).
        must_contain: &[
            "Move from p.name to n",
            "Copy from p.age to a",
            "p.name goes out of scope. No resource is dropped.",
            "p.nick goes out of scope. Its resource is dropped.",
            "p.age goes out of scope",
        ],
        must_not_contain: &[
            "p.name goes out of scope. Its resource is dropped.",
            "p.nick goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_literal_same_line_arrows",
        // Both arrows on the `let s = S { … }` line render.
        must_contain: &[
            "Move from String::from to s.b",
            "Move from t to s.c",
            "s.b goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s.b goes out of scope. No resource is dropped.",
        ],
    },
    // ─── Match guards (#176) ─────────────────────────────────────────
    // A guard's events are drawn in its own arm's column: they run
    // after the pattern matched and before the body, so "if this arm
    // is taken, this happened" holds. (Guards that move or assign an
    // outer variable are refused; see `guard_that_moves_is_refused`.)
    TooltipExpect {
        name: "match_guard_borrow",
        // `Some(n) if check(&s) =>`: the guard lends `s` to `check`.
        // Used to produce no event at all for the guard.
        must_contain: &[
            "check reads from s",
            "n is initialized by copy from opt",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s's resource is moved",
            "s goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "match_guard_reads_binding",
        // `Some(t) if t.len() > 2 =>`: the guard reads the arm's own
        // binding.
        must_contain: &[
            "len reads from t (implicitly borrowed)",
            "Partial move from opt to t",
            "t goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "t's resource is moved",
        ],
    },
    TooltipExpect {
        name: "match_if_let_guard",
        // `Some(n) if let Some(m) = n.checked_mul(2) =>`: `m` is
        // registered like an `if let` condition binding and scoped to
        // the arm. Its uses used to be skipped (unregistered local).
        must_contain: &[
            "Copy from checked_mul to m",
            "m is initialized by copy from checked_mul",
            "m goes out of scope",
            "describe reads from s",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s's resource is moved",
        ],
    },
    TooltipExpect {
        name: "match_guard_let_chain",
        // `n if let Some(w) = words.last() && check(&s) =>`: both parts
        // of the chain produce events; `w` is scoped to the arm.
        must_contain: &[
            "last reads from words (implicitly borrowed)",
            "Copy from last to w",
            "check reads from s",
            "w goes out of scope",
            "words goes out of scope. Its resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "words's resource is moved",
            "s's resource is moved",
        ],
    },

    // ─── Macro arguments (#177) ──────────────────────────────────────
    // `format_args!` (behind println!/format!/eprintln!/write!/panic!)
    // takes every argument by shared reference, so each named
    // argument is read for that line. Before #177 none of these
    // lines produced any event. A read through a reference is named
    // after the reference (`r.w`, `*p` read through r / p), not "*p".
    TooltipExpect {
        name: "macro_format_args_borrow",
        must_contain: &[
            "println! reads from s through an implicit reference, so s isn't moved",
            "println! reads from n through an implicit reference, so n isn't moved",   // inline `{n}`
            "format! reads from s through an implicit reference, so s isn't moved",
            "eprintln! reads from t through an implicit reference, so t isn't moved",
            "println! reads from r through an implicit reference, so r isn't moved",   // `r.w` through a reference
            "println! reads from p through an implicit reference, so p isn't moved",   // `*p`
        ],
        must_not_contain: &[
            // println! only borrows; it never takes ownership.
            "Move from s to println!",
            "println! reads from/writes to s",
            "println! reads from *p",
            "*p's resource is immutably borrowed",
            // `assert!(cond, "{}", t)` formats t only if the assertion
            // fails, so t isn't shown as read unconditionally.
            "assert! reads from t through an implicit reference, so t isn't moved",
        ],
    },
    // `dbg!(e)` takes e by value and returns it: `dbg!(&a)` lends a,
    // a statement `dbg!(c);` moves c into dbg! (the returned value is
    // dropped there), and `let b = dbg!(a)` is a move from a to b. On
    // main all of these showed nothing, so `a` and `c` were wrongly
    // "dropped" at the end of main and `let r = dbg!(&b)` made r an
    // owner of a resource instead of a borrow of b.
    TooltipExpect {
        name: "macro_dbg",
        must_contain: &[
            "dbg! reads from a",
            "Move from a to b",
            "Immutable borrow from b to r",
            "Return immutably borrowed resource from r to b",
            "Move from c to dbg!",
            "Copy from x to dbg!",
            "c goes out of scope. No resource is dropped.",
            // `write!(f, …)` inside the Debug impl: f is borrowed
            // mutably by write_fmt, self.w is read.
            "write! reads from/writes to f through an implicit mutable reference, so f isn't moved",
            "write! reads from self through an implicit reference, so self isn't moved",
        ],
        must_not_contain: &[
            "r acquires ownership of a resource",
            "Return immutably borrowed resource from r to *r",
            // `dbg!(&a)`: the user wrote the `&`, so no "implicit" note.
            "dbg! reads from a through an implicit reference, so a isn't moved",
            "c goes out of scope. Its resource is dropped.",
            "a goes out of scope. Its resource is dropped.",
        ],
    },
    // `Some(s) => println!("{}", s),`: the arm body is a macro call
    // whose span lives in std's macros.rs. The match handler must map
    // it back to the arm's line, or the arm's events fall outside the
    // arm and s is reported as holding nothing when it goes out of
    // scope.
    TooltipExpect {
        name: "macro_match_arm_body",
        must_contain: &[
            "println! reads from s through an implicit reference, so s isn't moved",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "macro_write_assert_eq",
        must_contain: &[
            "write! reads from/writes to out through an implicit mutable reference, so out isn't moved",
            "write! reads from v through an implicit reference, so v isn't moved",
            "assert_eq! reads from v through an implicit reference, so v isn't moved",
            "assert_eq! reads from w through an implicit reference, so w isn't moved",
            "panic! reads from msg through an implicit reference, so msg isn't moved",
        ],
        must_not_contain: &[
            // `v.len()` is a call, not a place: its own event ("len
            // reads from v (implicitly borrowed)") is the read, not a
            // read of `len`.
            "panic! reads from len through an implicit reference, so len isn't moved",
            "assert_eq! reads from len through an implicit reference, so len isn't moved",
        ],
    },
    // ─── Deferred initialization (#234) ─────────────────────────────
    // `let x;` owns nothing; the first `x = …` initializes x with the
    // same wording as `let x = …`, and nothing is dropped there.
    TooltipExpect {
        name: "deferred_init_copy",
        must_contain: &[
            "x is bound to a copyable value",
            "Copy from x to y",
            "y is initialized by copy from x",
        ],
        must_not_contain: &[
            "x's resource is moved",
            "Move from x to y",
        ],
    },
    TooltipExpect {
        name: "deferred_init_string",
        must_contain: &[
            "Move from String::from to s",
            "s acquires ownership of a resource",
        ],
        must_not_contain: &[
            "s acquires ownership of a new resource; its previous resource is dropped",
        ],
    },
    TooltipExpect {
        name: "deferred_init_if_else",
        // Each branch initializes s, so after the if s owns a String.
        // Before its branch assigns it, s's column says it has no value.
        must_contain: &[
            "Move from String::from to s",
            "s acquires ownership of a resource",
            "s acquired ownership of a resource (in all branches above)",
            "s has no value yet",
        ],
        must_not_contain: &[
            "s acquires ownership of a new resource; its previous resource is dropped",
            "s may have been moved (consumed in at least one branch above)",
            "s was moved or dropped in every branch above",
        ],
    },
    TooltipExpect {
        name: "deferred_init_match",
        must_contain: &[
            "Move from String::from to s",
            "s acquires ownership of a resource",
            "s acquired ownership of a resource (in all branches above)",
        ],
        must_not_contain: &[
            "s acquires ownership of a new resource; its previous resource is dropped",
            "s may have been moved (consumed in at least one branch above)",
            "s was moved or dropped in every branch above",
        ],
    },
    TooltipExpect {
        name: "deferred_init_then_reassign",
        // The reassignments drop the old String: after straight-line
        // init (s) and after init in both branches of an if/else (t).
        // `deferred_init_drops_only_at_reassignment` checks the
        // initializing assignments don't.
        must_contain: &[
            "s acquires ownership of a new resource; its previous resource is dropped",
            "t acquired ownership of a resource (in all branches above)",
            "t acquires ownership of a new resource; its previous resource is dropped",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "deferred_init_one_branch",
        // s is assigned in the if branch only. On the else path it has
        // no value, so there is nothing to drop there and it wasn't
        // moved. `unused` is never assigned and gets no column.
        must_contain: &[
            "Move from String::from to s",
            "s acquires ownership of a resource",
            "s has no value yet",
        ],
        must_not_contain: &[
            "s was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.",
            "s may have been moved (consumed in at least one branch above)",
            "unused goes out of scope",
        ],
    },
    // `v.last_mut().unwrap().s = t`: the temporary `&mut P` has no
    // column, so the write isn't drawn, but t is moved into the field
    // and must not be shown dropping it (from closed PR #186).
    TooltipExpect {
        name: "field_of_method_result_assign",
        must_contain: &[
            "last_mut reads from/writes to v (implicitly mutably borrowed)",
            "first_mut reads from/writes to v (implicitly mutably borrowed)",
            "t's resource is moved",
            "t goes out of scope. No resource is dropped.",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "t goes out of scope. Its resource is dropped.",
        ],
    },

    // ─── Overlapping borrows (#211) ─────────────────────────────────
    TooltipExpect {
        name: "overlapping_borrow_regions",
        // a, c and b borrow x with chained live ranges (a ends while c
        // is live, b starts before c ends), so x is borrowed without a
        // break until b's last use, and only b gives the borrow back.
        // Before #211 the borrows were grouped in HashSet order: on
        // some runs c "returned" the borrow while b still held it, and
        // x read as "no longer immutably borrowed" one line early.
        must_contain: &[
            "Return immutably borrowed resource from b to x",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from c to x",
            "Return immutably borrowed resource from a to x",
        ],
    },

    // ─── Struct item between fns (#231) ─────────────────────────────
    TooltipExpect {
        name: "struct_after_fn_hashes",
        // main's s and t have their own columns. Before #231 they
        // shared helper's b and c columns (helper's ninth binding
        // pushed the RAP hash counter past 9, and the struct's field
        // wrapped it back), so main read as b and c, including a
        // borrow by helper's i32 `a` that the program never makes.
        must_contain: &[
            "s is the owner of the resource",
            "t is the owner of the resource",
            "Move from s to t",
            "t goes out of scope. Its resource is dropped.",
            "s goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "a immutably borrows a resource",
            "b's resource is moved",
            "c acquires ownership of a resource",
            "b goes out of scope. No resource is dropped.",
        ],
    },

    // ─── Variable and function with the same name (#232) ────────────
    TooltipExpect {
        name: "var_named_like_fn",
        // `let area = area(&rect);`: the variable has its own column
        // and gets the fn's return value.
        must_contain: &[
            "area holds a copyable value",
            "area is initialized by copy from area",
            "area goes out of scope",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "var_named_like_method",
        // `let len = s.len();`: the variable has its own column.
        must_contain: &[
            "len holds a copyable value",
            "len goes out of scope",
            "len reads from s (implicitly borrowed)",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "method_named_like_var",
        // `let len = 3; let n = s.len();`: the call is `String::len`,
        // not the i32 variable. Before #232 the variable was shown
        // being lent s's resource and copied into n.
        must_contain: &[
            "len holds a copyable value",
            "len reads from s (implicitly borrowed)",
            "n is initialized by copy from len",
        ],
        must_not_contain: &[
            "Pass by immutable reference from s to len",
            "len immutably borrows a resource",
            "len's resource is copied",
        ],
    },
    // ─── Tuple field assignment (#235) ──────────────────────────────
    TooltipExpect {
        name: "tuple_field_assign",
        // `t.0 = s;` moves `s` into the tuple. The write to `t.0` has
        // no column to land on (#189), but `s` must still be shown as
        // moved, not as dropping a String it no longer owns.
        must_contain: &[
            "s's resource is moved",
            "s goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "s goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "method_chain_ref_copy_result",
        // `k` and `w` are `i32` / `usize` results of chains on a
        // reference, on the line of that reference's last use (where
        // its borrow ends). They're initialized by copy like `m`, and
        // end with the plain Copy-value scope end. Before #208 the
        // renderer dropped the "Copy from sum/count" arrow whenever a
        // borrow ended on the same line, leaving no creation event and
        // "No resource is dropped." (the moved-out owner wording).
        must_contain: &[
            "n is initialized by copy from count",
            "k is initialized by copy from sum",
            "w is initialized by copy from count",
            "Copy from sum to k",
            "Copy from count to w",
            "k goes out of scope",
            "w goes out of scope",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "n goes out of scope. No resource is dropped.",
            "k goes out of scope. No resource is dropped.",
            "w goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "arrow_hops_over_dots",
        // The arrows and the dots they cross (#252). Tooltip text is
        // unchanged by the hop; `arrows_hop_over_dots_they_cross`
        // checks the geometry. Listed so the fixture keeps
        // exercising a crossing: if a dot disappears, this fails.
        must_contain: &[
            "Return mutably borrowed resource from r to x",
            "y's resource is copied",
            "Move from p to q",
            "p.name's resource is moved",
            "p.email's resource is moved",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "arrow_tail_off_source_dot",
        // The source dots whose middles the arrows used to cover
        // (#269). Tooltip text is unchanged; the geometry is checked
        // by `arrow_tails_start_at_the_dot_edge`. Listed so the
        // fixture keeps drawing these dots and arrows.
        must_contain: &[
            "s's resource is moved",
            "Move from s to t",
            "Immutable borrow from a to b",
            "Immutable borrow from a to c",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "borrow_of_temporary_same_line",
        // `let r = &Point { .. };` with r never used: the borrow starts
        // and ends on line 9. The start (a borrow from a temporary) isn't
        // in the renderer's line map, so it used to be dropped whenever
        // the end shared its line, leaving r with an end dot and no
        // borrow (#204). Calling the closure on line 12 also emits an
        // off-map event that's still left out on purpose: its text would
        // be false (the call doesn't initialise add_one).
        must_contain: &[
            "r immutably borrows a resource",
            "r holds an immutable reference",
            "r's immutable borrow ends",
            "m mutably borrows a resource",
            "m holds a mutable reference",
            "m's mutable borrow ends",
        ],
        must_not_contain: &[
            "add_one is initialized by copy from Anonymous resource",
        ],
    },
    TooltipExpect {
        name: "same_line_events_one_dot",
        // Each event on a shared dot (#309) is still listed; the text
        // was right before, but only the top dot could be hovered.
        // `same_line_events_share_one_dot` checks they share one dot,
        // in order.
        must_contain: &[
            "a's resource is immutably borrowed by b; a is read-only while the borrow is live",
            "a's resource is immutably borrowed by c; a is read-only while the borrow is live",
            "the resource r points to is modified through *r (copied from y)",
            "r's mutable borrow ends",
            "s's resource is immutably borrowed by show; s is read-only while the borrow is live",
            "show reads from s",
            "s's resource is no longer immutably borrowed",
            "t acquires ownership of a resource",
            "t goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    // ─── Partially moved structs (#207) ──────────────────────────────
    // A struct with fields moved out isn't dropped as a whole; its
    // scope-end tooltip names what was moved and what is dropped.
    TooltipExpect {
        name: "struct_partial_move_two",
        must_contain: &[
            "p goes out of scope. p.name was moved out earlier, so only p.nick is dropped.",
        ],
        must_not_contain: &[
            "p goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_update_syntax",
        must_contain: &[
            "user1 goes out of scope. user1.username was moved out earlier, so only user1.email is dropped.",
        ],
        must_not_contain: &[
            "user1 goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_partial_move_all",
        // Only the Copy `p.age` is left: nothing to drop.
        must_contain: &[
            "p goes out of scope. p.name and p.nick were moved out earlier, so no resource is dropped.",
            "p.age goes out of scope",
        ],
        must_not_contain: &[
            "p goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_partial_move_nested",
        // A moved-out field's own fields went with it (`q.inner.x` is
        // not listed separately), and the partially moved `o.inner`
        // gets the same treatment as `o`.
        must_contain: &[
            "o goes out of scope. o.inner.x was moved out earlier, so only o.inner.y and o.z are dropped.",
            "o.inner goes out of scope. o.inner.x was moved out earlier, so only o.inner.y is dropped.",
            "o.inner.y goes out of scope. Its resource is dropped.",
            "q goes out of scope. q.inner was moved out earlier, so only q.z is dropped.",
            "q.inner goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "o goes out of scope. Its resource is dropped.",
            "o.inner goes out of scope. Its resource is dropped.",
            "q goes out of scope. Its resource is dropped.",
        ],
    },
    // ─── Unions never drop their fields (#191) ──────────────────────
    TooltipExpect {
        name: "union_manually_drop",
        must_contain: &[
            "slot acquires ownership of a resource",
            "slot is written to; writing to a union never drops its old contents",
            "slot goes out of scope. A union's fields are never dropped automatically.",
        ],
        must_not_contain: &[
            "slot goes out of scope. Its resource is dropped.",
            "slot acquires ownership of a new resource; its previous resource is dropped",
        ],
    },
    TooltipExpect {
        name: "union_whole_assign",
        must_contain: &[
            "u is written to (copied from y); writing to a union never drops its old contents",
            "u is written to; writing to a union never drops its old contents",
            "u goes out of scope. A union's fields are never dropped automatically.",
        ],
        must_not_contain: &[
            "u is initialized by copy from y",
            "u acquires ownership of a new resource; its previous resource is dropped",
            "u goes out of scope. Its resource is dropped.",
        ],
    },
    // ─── Wording on references and Copy assignments (#178) ──────────
    TooltipExpect {
        name: "ref_move_copy_wording",
        // Moving / returning a `&mut` and copying a `&T` move or copy
        // the reference; `s` and `x` keep their resources.
        must_contain: &[
            "r's mutable reference is moved (not the resource it points to)",
            "r's mutable reference is moved to the caller (not the resource it points to)",
            "a's reference is copied (not the resource it points to)",
            "Move from r to r2",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "r's resource is moved",
            "r's resource is moved to the caller",
            "a's resource is copied",
        ],
    },
    TooltipExpect {
        name: "str_literal_ref",
        // The string literal has no column: the borrow isn't
        // "returned" from s to *s, and the arrow says what s (and t, a
        // copy of s) borrowed.
        must_contain: &[
            "s's borrow of the string literal ends (string literals live for the whole program)",
            "t's borrow of the string literal ends (string literals live for the whole program)",
            "s's reference is copied (not the resource it points to)",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from s to *s",
            "Return immutably borrowed resource from t to *t",
            "s's immutable borrow ends (what it borrows from isn't shown)",
            "t's immutable borrow ends (what it borrows from isn't shown)",
            "s's resource is copied",
        ],
    },
    TooltipExpect {
        name: "hashmap_entry_count",
        // `map` owns its HashMap to the end (already right on main;
        // pinned because #178 reported "No resource is dropped").
        must_contain: &[
            "map goes out of scope. Its resource is dropped.",
            "text's borrow of the string literal ends (string literals live for the whole program)",
        ],
        must_not_contain: &[
            "map goes out of scope. No resource is dropped.",
            "Return immutably borrowed resource from text to *text",
            "text's immutable borrow ends (what it borrows from isn't shown)",
        ],
    },
    TooltipExpect {
        name: "copy_field_assign",
        // Copy fields hold values, not resources; writing to a Copy
        // place that already holds a value is an assignment.
        must_contain: &[
            "p.x holds a copyable value",
            "p.x is bound to a copyable value",
            "p.x is assigned a new value",
            "p.y is assigned a copy of z",
            "Copy from z to p.y",
            "n is assigned a new value",
            "n is assigned a copy of m",
            "b is written to; writing to a union never drops its old contents",
        ],
        must_not_contain: &[
            "p.x acquires ownership of a resource",
            "p.x is the owner of the resource",
            "p.y is initialized by copy from z",
            "n is initialized by copy from m",
            "b acquires ownership of a resource",
            "b is assigned a new value",
        ],
    },
    TooltipExpect {
        name: "ref_static_lender",
        // The lender has no column but is 'static data: a literal, the
        // static COUNT (`&COUNT`), or what a static / const of reference
        // type points to (`GREETING`, `NAME` are copied, not borrowed).
        // A call result (`w`) still gets the generic text.
        must_contain: &[
            "s's borrow of the string literal ends (string literals live for the whole program)",
            "t's borrow of the string literal ends (string literals live for the whole program)",
            "b's borrow of the byte string literal ends (byte string literals live for the whole program)",
            "g's borrow of the static COUNT ends (statics live for the whole program)",
            "h's borrow of what the static GREETING points to ends (it lives for the whole program)",
            "c's borrow of what the constant NAME points to ends (it lives for the whole program)",
            "w's immutable borrow ends (what it borrows from isn't shown)",
        ],
        must_not_contain: &[
            "s's immutable borrow ends (what it borrows from isn't shown)",
            "t's immutable borrow ends (what it borrows from isn't shown)",
            "b's immutable borrow ends (what it borrows from isn't shown)",
            "g's immutable borrow ends (what it borrows from isn't shown)",
            "h's immutable borrow ends (what it borrows from isn't shown)",
            "c's immutable borrow ends (what it borrows from isn't shown)",
            "Return immutably borrowed resource from s to *s",
            "Return immutably borrowed resource from g to *g",
            "Return immutably borrowed resource from h to *h",
        ],
    },
    TooltipExpect {
        name: "ref_static_lender_mut",
        must_contain: &[
            "r's mutable borrow of the static COUNTER ends (statics live for the whole program)",
        ],
        must_not_contain: &[
            "r's mutable borrow ends (what it borrows from isn't shown)",
            "Return mutably borrowed resource from r to *r",
        ],
    },
    // ─── Merge-point tooltips (#179) ────────────────────────────────
    TooltipExpect {
        name: "match_unchanged_merge",
        // No branch moves `opt` / `self`: the merge dot says so
        // instead of carrying an empty tooltip.
        must_contain: &[
            "opt isn't moved in any of the branches above",
            "self isn't moved in any of the branches above",
            "opt goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[""],
    },
    TooltipExpect {
        name: "if_else_move_join",
        // Rust doesn't drop `s` at the end of the branch that didn't
        // move it; a drop flag decides at scope end.
        must_contain: &[],
        must_not_contain: &[
            "s was moved in at least one branch above; \
             in branches where it was not, its resource is dropped at the branch's end.",
            "s goes out of scope. No resource is dropped.",
        ],
    },
    // ─── Closure capture through a reference (#277) ──────────────────
    TooltipExpect {
        name: "closure_capture_through_ref",
        // `|| r.push('b')` / `|| r.len()` / `|| *p += 1` capture `*r` /
        // `*p`: a reborrow of what the reference points to. r and p
        // own nothing. `t` is captured as before.
        must_contain: &[
            "the resource r points to is captured by closure f (a mutable reborrow through r); r can't be used while the borrow is live",
            "Closure f captures: *r (mutably reborrowed)",
            "Closure capture (mutable reborrow) from r to f",
            "the resource r points to is captured by closure len (an immutable reborrow through r); r is read-only while the borrow is live",
            "Closure len captures: *r (immutably reborrowed)",
            "Closure capture (immutable reborrow) from r to len",
            "the resource p points to is captured by closure g (a mutable reborrow through p); p can't be used while the borrow is live",
            "Closure h captures: t (mutably borrowed)",
            "t's resource is captured (mutably borrowed) by closure h",
        ],
        must_not_contain: &[
            "r's resource is captured (mutably borrowed) by closure f",
            "r's resource is captured (immutably borrowed) by closure len",
            "p's resource is captured (mutably borrowed) by closure g",
            "Closure f captures: r (mutably borrowed)",
            "Closure len captures: r (immutably borrowed)",
            "Closure capture (mutable borrow) from r to f",
        ],
    },
    // ─── ManuallyDrop never drops what it wraps (#283) ───────────────
    TooltipExpect {
        name: "manually_drop_local",
        must_contain: &[
            "md goes out of scope. Nothing is dropped: a ManuallyDrop never drops the value inside it automatically.",
            "h.a goes out of scope. Nothing is dropped: a ManuallyDrop never drops the value inside it automatically.",
            "h.b goes out of scope. Its resource is dropped.",
            "md2 goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "md goes out of scope. Its resource is dropped.",
            "h.a goes out of scope. Its resource is dropped.",
            // `md = ManuallyDrop::new(..)` leaks the old String.
            "md acquires ownership of a new resource; its previous resource is dropped",
        ],
    },
    // ─── Dropping a ManuallyDrop by hand (#283) ──────────────────────
    TooltipExpect {
        name: "manually_drop_by_hand",
        must_contain: &[
            "ManuallyDrop::drop drops the value inside md (by hand)",
            "ManuallyDrop::drop drops the value inside h.a (by hand)",
            // The borrow information stays.
            "md's resource is mutably borrowed by ManuallyDrop::drop; md can't be used while the borrow is live",
            // Still nothing dropped at scope end.
            "md goes out of scope. Nothing is dropped: a ManuallyDrop never drops the value inside it automatically.",
            "h.a goes out of scope. Nothing is dropped: a ManuallyDrop never drops the value inside it automatically.",
            "h.b goes out of scope. Its resource is dropped.",
            // `take` moves the value out into the result.
            "Move from m2 to ManuallyDrop::take",
            "Move from ManuallyDrop::take to s",
            "m2 goes out of scope. No resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "md goes out of scope. Its resource is dropped.",
            // `take` is a move out of m2, not a borrow.
            "m2's resource is mutably borrowed by ManuallyDrop::take",
            "ManuallyDrop::take reads from/writes to m2",
            "m2 goes out of scope. Nothing is dropped: a ManuallyDrop never drops the value inside it automatically.",
        ],
    },
    // ─── Interior mutability: call dots (#203) ──────────────────────
    TooltipExpect {
        name: "interior_mut_lend",
        must_contain: &[
            "set borrows c immutably (a shared borrow); c has interior mutability, so its contents can change even through a shared borrow",
            "borrow_mut borrows r immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)",
            "lock borrows m immutably (a shared borrow); m has interior mutability, so its contents can change even through a shared borrow",
            "len reads from s (implicitly borrowed)",
        ],
        must_not_contain: &[
            "set reads from c",
            "borrow_mut reads from r",
            "lock reads from m",
        ],
    },
    TooltipExpect {
        name: "interior_mut_calls",
        must_contain: &[
            "get borrows c immutably (a shared borrow); c has interior mutability, so its contents can change even through a shared borrow",
            "set borrows c immutably (a shared borrow); c has interior mutability, so its contents can change even through a shared borrow",
            "borrow_mut borrows cell immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)",
            "fetch_add borrows hits immutably (a shared borrow); hits has interior mutability, so its contents can change even through a shared borrow",
            "len reads from s (implicitly borrowed)",
        ],
        must_not_contain: &[
            "set reads from c",
            "borrow_mut reads from cell",
            "fetch_add reads from hits",
        ],
    },
    TooltipExpect {
        name: "interior_mut_user_borrow_mut",
        must_contain: &[
            "borrow_mut borrows cell immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)",
            "borrow_mut borrows counter immutably (a shared borrow); counter has interior mutability, so its contents can change even through a shared borrow",
        ],
        must_not_contain: &[
            "borrow_mut borrows counter immutably (a shared borrow); exclusive access to its contents is checked at runtime",
        ],
    },
    TooltipExpect {
        name: "binop_operand_copies",
        // `x + y` on i32 is `Add::add(x, y)`: both operands are taken
        // by value, i.e. copied, and stay usable. Field operands
        // (`a.x + a.y`) are copied out of their own columns. A
        // comparison (`x < y`) reads through references (`lt(&x, &y)`)
        // and isn't drawn as a copy; a literal operand has no column.
        must_contain: &[
            "Copy from x to +",
            "Copy from y to +",
            "Copy from a.x to +",
            "Copy from a.y to +",
            "Copy from x to *",
            "x's resource is copied",
            "a.y's resource is copied",
            "Copy from a.x to d",
        ],
        must_not_contain: &[
            "Move from x to +",
            "x's resource is moved",
            "Copy from x to z",
            "Copy from x to <",
            "Copy from y to <",
        ],
    },
    TooltipExpect {
        name: "method_autoref_receiver",
        // rustc auto-references an owner receiver: `s.len()` is
        // `String::len(&s)`. Without the note the dot read like a move.
        must_contain: &[
            "len reads from s (implicitly borrowed)",
            "push_str reads from/writes to s (implicitly mutably borrowed)",
            "push reads from/writes to v (implicitly mutably borrowed)",
            // Through a Box: `String::len(&**b)` still borrows b.
            "len reads from b (implicitly borrowed)",
            // A written `&`: no note.
            "is_empty reads from s",
            "String::len reads from s",
            "String::push_str reads from/writes to s",
        ],
        must_not_contain: &[
            "len reads from s",
            "push_str reads from/writes to s",
            "is_empty reads from s (implicitly borrowed)",
            "String::len reads from s (implicitly borrowed)",
            "String::push_str reads from/writes to s (implicitly mutably borrowed)",
            // The macro wording is only for macro arguments.
            "len reads from s through an implicit reference, so s isn't moved",
        ],
    },
    TooltipExpect {
        name: "method_autoref_ref_receiver",
        // A reference receiver is dereffed and reborrowed, not
        // auto-referenced: no "(implicitly borrowed)" note, and the
        // `&mut` case keeps only its reborrow note.
        must_contain: &[
            "len reads from r",
            "push_str reads from/writes to w (mutable reborrow — w can't be used until push_str returns)",
            "is_empty reads from r",
        ],
        must_not_contain: &[
            "len reads from r (implicitly borrowed)",
            "push_str reads from/writes to w (implicitly mutably borrowed)",
            "push_str reads from/writes to w (implicitly mutably borrowed) (mutable reborrow — w can't be used until push_str returns)",
            "is_empty reads from r (implicitly borrowed)",
        ],
    },
    TooltipExpect {
        name: "method_addrof_receiver",
        // `(&mut s).push_str` borrows s afresh, like the explicit
        // `String::push_str(&mut s, ..)`; only `(&mut *r)` / `r.` on a
        // `&mut String` r are reborrows.
        must_contain: &[
            "push_str reads from/writes to s",
            "push_str reads from/writes to s (implicitly mutably borrowed)",
            "String::push_str reads from/writes to s",
            "push_str reads from/writes to r (mutable reborrow — r can't be used until push_str returns)",
        ],
        must_not_contain: &[
            "push_str reads from/writes to s (mutable reborrow — s can't be used until push_str returns)",
        ],
    },
    // ─── A moved-out reference's state (#292) ────────────────────────
    TooltipExpect {
        name: "moved_ref_state",
        // q's `&mut` is moved into q2; q is usable again once it's
        // assigned a new reference. It never owned anything.
        must_contain: &[
            "q's mutable reference was moved out, so q can't be used until it's assigned again",
            "q's mutable reference is moved (not the resource it points to)",
        ],
        must_not_contain: &[
            "q's resource was moved, so q no longer has ownership",
            "q's resource was moved, so q no longer has ownership; cannot access data.",
        ],
    },
    TooltipExpect {
        name: "unary_operand_copies",
        // `-x` is `Neg::neg(x)` and `!b` is `Not::not(b)`: the operand
        // is taken by value. A primitive operand (local, field, `*r`)
        // is copied and stays usable. An overloaded `Neg` on a
        // non-Copy struct moves its operand, which then has nothing
        // left to drop. A literal (`-5`) has no column.
        must_contain: &[
            "Copy from x to -",
            "Copy from b to !",
            "Copy from p.y to -",
            "Copy from *r to -",
            "b's resource is copied",
            "Move from wave to -",
            "wave's resource is moved",
            "wave goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "Move from x to -",
            "Move from b to !",
            "wave goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "closure_arg_move_captures",
        // The closures own what they capture by value, and are handed
        // to thread::spawn: v, s and p.name are moved into the call and
        // dropped by the spawned thread, not at the end of main. n is
        // Copy (copied in); p.age isn't captured and stays with p.
        must_contain: &[
            "Move from v to thread::spawn",
            "v goes out of scope. No resource is dropped.",
            "Copy from n to thread::spawn",
            "Move from s to thread::spawn",
            "s goes out of scope. No resource is dropped.",
            "Move from p.name to thread::spawn",
            "p.name goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "v goes out of scope. Its resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
            "p.name goes out of scope. Its resource is dropped.",
            "n's resource is moved",
            "Move from p to thread::spawn",
            "p.age's resource is moved",
        ],
    },
];

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("rustviz-plugin")
        .join("tests")
}

fn read_corpus(name: &str) -> String {
    let path = corpus_dir().join(format!("{}.rs", name));
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {:?}: {}", path, e))
}

fn ensure_plugin_installed() {
    let ok = Command::new("cargo")
        .args(["rv-plugin", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(
        ok,
        "`cargo rv-plugin` not on PATH. Run `cargo install --path rustviz-plugin --locked` \
         (or `./setup.sh`) before `cargo test`."
    );
}

fn run_with_local_backend(name: &str) -> Rustviz {
    ensure_plugin_installed();
    // Tests share env between threads; setting this once per test is fine
    // because we only ever set it to the same value. The library reads
    // RV_RUNNER on each Rustviz::new call.
    env::set_var("RV_RUNNER", "local");
    let src = read_corpus(name);
    Rustviz::new(&src).unwrap_or_else(|e| panic!("{} failed: {}", name, e))
}

fn assert_well_formed(name: &str, rv: &Rustviz) {
    let code = rv.code_panel_string();
    let timeline = rv.timeline_panel_string();
    assert!(
        code.trim_start().starts_with("<svg"),
        "{}: code panel is not SVG (starts with: {:?})",
        name,
        code.chars().take(40).collect::<String>()
    );
    assert!(
        timeline.trim_start().starts_with("<svg"),
        "{}: timeline panel is not SVG (starts with: {:?})",
        name,
        timeline.chars().take(40).collect::<String>()
    );
    assert!(
        timeline.contains("tooltip-trigger"),
        "{}: timeline has no tooltip-trigger elements (annotations missing)",
        name
    );
    let crossed = arrows_crossing_dots(&timeline);
    assert!(
        crossed.is_empty(),
        "{}: straight arrow(s) drawn across event dots, hiding their tooltips: {:?}",
        name,
        crossed
    );
    let shared = shared_dot_points(&timeline);
    assert!(
        shared.is_empty(),
        "{}: several dots drawn at one point, only the top one can be hovered: {:?}",
        name,
        shared
    );
    let covered = arrow_tails_on_dots(&timeline);
    assert!(
        covered.is_empty(),
        "{}: arrow tail(s) drawn inside their source dot, hiding the dot's tooltip: {:?}",
        name,
        covered
    );
    for (panel, svg) in [("code", &code), ("timeline", &timeline)] {
        let uncoloured = hashes_without_colour(svg);
        assert!(
            uncoloured.is_empty(),
            "{}: {} panel uses data-hash value(s) {:?} with no fill+stroke colour rule \
             (in the timeline, those columns' state lines have no stroke and are invisible)",
            name,
            panel,
            uncoloured
        );
        let off = off_canvas_ys(svg);
        assert!(
            off.is_empty(),
            "{}: {} panel draws {} element(s) below the SVG height: {:?}",
            name,
            panel,
            off.len(),
            &off[..off.len().min(5)]
        );
    }
    assert_no_negative_sizes(name, &timeline);
}

/// SVG rejects negative `width` / `height` (browsers log a console error
/// and skip the element). The timeline used to draw empty struct boxes
/// with width="-80" (#192).
fn assert_no_negative_sizes(name: &str, timeline: &str) {
    for attr in ["width=\"-", "height=\"-"] {
        if let Some(i) = timeline.find(attr) {
            let start = timeline[..i].rfind('<').unwrap_or(0);
            let end = timeline[i..].find('>').map_or(timeline.len(), |e| i + e + 1);
            panic!("{}: negative size in timeline: {}", name, &timeline[start..end]);
        }
    }
}

/// Every hash (from 1 up) that the panel's elements use, as
/// `data-hash="N"` or a function name's `hash="N"` (the hover script
/// copies it into `data-hash`), whose `[data-hash="N"]` rule in the
/// panel's stylesheet is missing or lacks a `fill` or `stroke`. A
/// column without one draws its state lines with no stroke (#279).
/// Hash 0 is the code panel's function-name colour and is fill-only.
fn hashes_without_colour(svg: &str) -> Vec<u64> {
    let (style, body) = match svg.find("]]>") {
        Some(i) => (&svg[..i], &svg[i..]),
        None => return vec![u64::MAX],
    };
    let mut used: Vec<u64> = body
        .split("hash=\"")
        .skip(1)
        .filter_map(|r| r.split('"').next()?.parse().ok())
        .filter(|&h| h > 0)
        .collect();
    used.sort();
    used.dedup();
    used.into_iter()
        .filter(|h| {
            let sel = format!("[data-hash=\"{}\"]", h);
            match style.find(&sel) {
                Some(i) => {
                    let rule = &style[i + sel.len()..];
                    let rule = &rule[..rule.find('}').unwrap_or(rule.len())];
                    !(rule.contains("fill:") && rule.contains("stroke:"))
                }
                None => true,
            }
        })
        .collect()
}

/// Every y coordinate (`y`, `y1`, `y2`, `cy`, and the y half of each
/// `points` pair) that lies outside `0..=height` of the panel's root
/// `<svg height="..">`. Elements out there are never visible: a
/// macro-expansion span once put match-arm drops at y=4375 in a
/// 370px panel.
fn off_canvas_ys(svg: &str) -> Vec<String> {
    let num = |v: &str| v.trim_end_matches("px").parse::<f64>().ok();
    let root = &svg[..svg.find('>').unwrap_or(svg.len())];
    let height = match root
        .split("height=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .and_then(num)
    {
        Some(h) => h,
        None => return vec!["<svg> has no height".to_string()],
    };
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(eq) = rest.find("=\"") {
        let attr_start = rest[..eq]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .map_or(0, |i| i + 1);
        let attr = &rest[attr_start..eq];
        let after = &rest[eq + 2..];
        let end = after.find('"').unwrap_or(after.len());
        let value = &after[..end];
        let ys: Vec<f64> = match attr {
            "y" | "y1" | "y2" | "cy" => num(value).into_iter().collect(),
            "points" => value
                .split(|c: char| c == ' ' || c == ',')
                .filter(|t| !t.is_empty())
                .filter_map(num)
                .skip(1)
                .step_by(2)
                .collect(),
            _ => Vec::new(),
        };
        for y in ys {
            if !(0.0..=height).contains(&y) {
                out.push(format!("{}={} (height {})", attr, y, height));
            }
        }
        rest = &after[end.min(after.len())..];
    }
    out
}

/// Value of attribute `name` in the start tag `tag` (`<circle cx=".." ..`).
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {}=\"", name);
    let start = tag.find(&key)? + key.len();
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

/// Start tags of every `<kind ...>` element in `svg`.
fn start_tags<'a>(svg: &'a str, kind: &str) -> Vec<&'a str> {
    let open = format!("<{} ", kind);
    svg.match_indices(&open)
        .map(|(i, _)| &svg[i..i + svg[i..].find('>').unwrap_or(svg.len() - i)])
        .collect()
}

/// Centres of the event dots in a timeline panel (every `<circle>`;
/// drop dots are a circle plus a triangle).
fn dot_centres(svg: &str) -> Vec<(f64, f64)> {
    start_tags(svg, "circle")
        .into_iter()
        .filter_map(|t| Some((attr(t, "cx")?.parse().ok()?, attr(t, "cy")?.parse().ok()?)))
        .collect()
}

/// Straight horizontal arrows (gray two-point `<polyline>` shafts) that
/// run through the centre of an event dot strictly between their two
/// ends. Arrows are drawn after dots, so such an arrow covers the dot
/// and steals its hover: the dot's tooltip can't be read (#252).
fn arrows_crossing_dots(svg: &str) -> Vec<String> {
    let dots = dot_centres(svg);
    let mut out = Vec::new();
    for t in start_tags(svg, "polyline") {
        if attr(t, "stroke") != Some("gray") {
            continue;
        }
        let pts: Vec<f64> = attr(t, "points")
            .unwrap_or("")
            .split(|c: char| c == ' ' || c == ',')
            .filter_map(|v| v.parse().ok())
            .collect();
        if pts.len() != 4 || pts[1] != pts[3] {
            continue;
        }
        let (lo, hi, y) = (pts[0].min(pts[2]), pts[0].max(pts[2]), pts[1]);
        for &(cx, cy) in &dots {
            if cy == y && lo < cx && cx < hi {
                out.push(format!("arrow {}..{} at y={} crosses dot at x={}", lo, hi, y, cx));
            }
        }
    }
    out
}

/// First point of every gray arrow shaft (`<polyline>` or hop `<path>`).
/// Shafts are written source first, so this is the arrow's tail.
fn arrow_tails(svg: &str) -> Vec<(f64, f64)> {
    let polylines = start_tags(svg, "polyline")
        .into_iter()
        .filter(|t| attr(t, "stroke") == Some("gray"))
        .filter_map(|t| attr(t, "points"));
    let paths = start_tags(svg, "path")
        .into_iter()
        .filter(|t| attr(t, "stroke") == Some("gray"))
        .filter_map(|t| attr(t, "d"))
        .map(|d| d.trim_start_matches('M'));
    polylines
        .chain(paths)
        .filter_map(|pts| {
            let mut v = pts
                .split(|c: char| c == ' ' || c == ',')
                .filter(|t| !t.is_empty())
                .map(|t| t.parse::<f64>());
            Some((v.next()?.ok()?, v.next()?.ok()?))
        })
        .collect()
}

/// Arrow tails that start inside an event dot (closer than its 5px
/// radius to the centre). Arrows are drawn after dots, so such a tail
/// covers the middle of the dot and hovering there shows the arrow's
/// tooltip instead of the dot's (#269).
fn arrow_tails_on_dots(svg: &str) -> Vec<String> {
    let dots = dot_centres(svg);
    let mut out = Vec::new();
    for (x, y) in arrow_tails(svg) {
        for &(cx, cy) in &dots {
            if (x - cx).hypot(y - cy) < 5.0 - 1e-9 {
                out.push(format!("tail ({}, {}) inside dot at ({}, {})", x, y, cx, cy));
            }
        }
    }
    out
}

/// #269: arrows used to start at their source dot's centre. Each tail
/// must sit on the edge of a drawn dot instead, for straight arrows and
/// for trapezoids that leave the dot diagonally.
#[test]
fn arrow_tails_start_at_the_dot_edge() {
    let rv = run_with_local_backend("arrow_tail_off_source_dot");
    let timeline = rv.timeline_panel_string();
    assert_eq!(arrow_tails_on_dots(&timeline), Vec::<String>::new());
    let dots = dot_centres(&timeline);
    let (mut straight, mut diagonal) = (0, 0);
    for (x, y) in arrow_tails(&timeline) {
        let on_edge = dots
            .iter()
            .any(|&(cx, cy)| ((x - cx).hypot(y - cy) - 5.25).abs() < 1e-6);
        if !on_edge {
            continue; // e.g. a `String::from` arrow, whose tail is at the `f`
        }
        if dots.iter().any(|&(_, cy)| cy == y) { straight += 1 } else { diagonal += 1 }
    }
    // `let t = s;` and `let b = &a;` are straight; `let c = &a;`
    // (second arrow on its line) is a trapezoid.
    assert!(straight >= 2 && diagonal >= 1,
        "expected tails on dot edges (straight {}, diagonal {}): {:?}",
        straight, diagonal, arrow_tails(&timeline));
}

/// A straight arrow that would cross event dots is drawn as a `<path>`
/// with a half-circle hop over each of them (#252). The hops must sit
/// on the crossed dots and bulge upward, whichever way the arrow runs.
#[test]
fn arrows_hop_over_dots_they_cross() {
    let rv = run_with_local_backend("arrow_hops_over_dots");
    let timeline = rv.timeline_panel_string();
    assert_eq!(arrows_crossing_dots(&timeline), Vec::<String>::new());
    let dots = dot_centres(&timeline);
    let hop_paths: Vec<&str> = start_tags(&timeline, "path")
        .into_iter()
        .filter(|t| attr(t, "stroke") == Some("gray"))
        .filter_map(|t| attr(t, "d"))
        .filter(|d| d.contains(" A "))
        .collect();
    // `*r = y;` runs leftwards over one dot; `let q = p;` runs
    // rightwards over two.
    let (mut left, mut right) = (0, 0);
    for d in &hop_paths {
        let tok: Vec<&str> = d.split_whitespace().collect();
        let num = |i: usize| tok[i].parse::<f64>().unwrap();
        let (mut x, mut y) = (num(1), num(2));
        let mut i = 3;
        while i < tok.len() {
            match tok[i] {
                "L" => { x = num(i + 1); y = num(i + 2); i += 3; }
                "A" => {
                    // A rx ry rotation large-arc sweep x y
                    let (sweep, end_x) = (tok[i + 5], num(i + 6));
                    let rightwards = end_x > x;
                    if rightwards { right += 1 } else { left += 1 }
                    // SVG's y axis points down: sweep 1 is clockwise, so
                    // left -> top -> right bulges up when going right.
                    assert_eq!(sweep, if rightwards { "1" } else { "0" }, "hop bulges downward: {}", d);
                    let mid = (x + end_x) / 2.0;
                    assert!(dots.contains(&(mid, y)), "hop at x={} y={} isn't over a dot: {}", mid, y, d);
                    x = end_x;
                    i += 8;
                }
                other => panic!("unexpected path command {:?} in {}", other, d),
            }
        }
    }
    assert_eq!((left, right), (1, 2), "hops (leftwards, rightwards) in {:?}", hop_paths);
}

/// Extract every tooltip text from a rendered timeline panel.
///
/// The renderer attaches `data-tooltip-text="..."` to many element
/// kinds: arrows in `<g id="arrows">`, dots in `<g id="events">`,
/// state-message vertical lines, column labels, function-icon dots,
/// etc. We pull all of them and let the per-snippet expectations
/// pick out the ones that matter — that way a test for "the
/// `len reads from s` interaction" doesn't have to know whether
/// the renderer puts that on a dot vs. an arrow.
///
/// The renderer HTML-escapes the tooltip body and embeds nested
/// `<span>` markup for the variable-name color spans; both get
/// stripped here so comparisons are against the plain visible text.
///
/// A dot shared by several events on one line (#309) lists them as
/// `• event<br>• event`; each event is returned as its own tooltip.
fn timeline_tooltips(timeline_svg: &str) -> Vec<String> {
    tooltip_lines(timeline_svg).into_iter().flatten().collect()
}

/// Every tooltip in a timeline panel, as its list of lines (one line
/// unless several events share a dot).
fn tooltip_lines(timeline_svg: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for chunk in timeline_svg.split("data-tooltip-text=\"").skip(1) {
        let raw = match chunk.find('"') {
            Some(end) => &chunk[..end],
            None => continue,
        };
        let unescaped = raw
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
        let lines = unescaped
            .split("<br>")
            .map(|line| {
                let mut clean = String::with_capacity(line.len());
                let mut in_tag = false;
                for ch in line.chars() {
                    if ch == '<' { in_tag = true; continue; }
                    if ch == '>' { in_tag = false; continue; }
                    if !in_tag { clean.push(ch); }
                }
                clean.strip_prefix("• ").map(str::to_owned).unwrap_or(clean)
            })
            .collect();
        out.push(lines);
    }
    out
}

/// Points where more than one hoverable dot is drawn (a `<circle>` or
/// call icon `<use>` with a tooltip, or a tooltip `<g>` around a
/// circle). Only the top one can be hovered, so the others' tooltips
/// can't be read (#309).
fn shared_dot_points(svg: &str) -> Vec<String> {
    let mut seen: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (i, _) in svg.match_indices(" data-tooltip-text=\"") {
        let start = match svg[..i].rfind('<') {
            Some(s) => s,
            None => continue,
        };
        let tag = &svg[start..start + svg[start..].find('>').unwrap_or(svg.len() - start)];
        let centre = if tag.starts_with("<circle ") {
            attr(tag, "cx").zip(attr(tag, "cy"))
        } else if tag.starts_with("<use ") {
            attr(tag, "x").zip(attr(tag, "y"))
        } else if tag.starts_with("<g ") {
            let body = &svg[start..start + svg[start..].find("</g>").unwrap_or(svg.len() - start)];
            start_tags(body, "circle").first().and_then(|c| attr(c, "cx").zip(attr(c, "cy")))
        } else {
            None
        };
        if let Some((x, y)) = centre {
            *seen.entry((x.to_string(), y.to_string())).or_default() += 1;
        }
    }
    seen.into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|((x, y), n)| format!("{} dots at ({}, {})", n, x, y))
        .collect()
}

/// #309: two events on one line of a variable's column used to get two
/// dots at the same point, so only the top one's tooltip showed. They
/// now share one dot that lists every event, in order.
#[test]
fn same_line_events_share_one_dot() {
    let rv = run_with_local_backend("same_line_events_one_dot");
    let timeline = rv.timeline_panel_string();
    assert_eq!(shared_dot_points(&timeline), Vec::<String>::new());
    let tooltips = tooltip_lines(&timeline);
    for want in [
        &[
            "a's resource is immutably borrowed by b; a is read-only while the borrow is live",
            "a's resource is immutably borrowed by c; a is read-only while the borrow is live",
        ][..],
        &[
            "the resource r points to is modified through *r (copied from y)",
            "r's mutable borrow ends",
        ],
        // The call icon's line goes between the borrow and its end.
        &[
            "s's resource is immutably borrowed by show; s is read-only while the borrow is live",
            "show reads from s",
            "s's resource is no longer immutably borrowed",
        ],
        &[
            "t acquires ownership of a resource",
            "t goes out of scope. Its resource is dropped.",
        ],
    ] {
        assert!(
            tooltips.iter().any(|t| t == want),
            "no dot lists {:?}; multi-line tooltips: {:?}",
            want,
            tooltips.iter().filter(|t| t.len() > 1).collect::<Vec<_>>()
        );
    }
    // The shared call dot keeps its call icon, and t's keeps the drop
    // triangle.
    assert!(timeline.contains("<use xlink:href=\"#functionDot\""));
    assert!(timeline.contains("pointer-events: none;\"/>"));
}

#[test]
fn drop_on_a_call_dot_is_marked_below_it() {
    // `{ let s = String::from("a"); show(&s); }`: s's one dot has the
    // call icon (which covers the dot) and a drop, so the drop mark is a
    // separate triangle below the dot, inside the same hover group.
    let rv = run_with_local_backend("drop_and_call_one_dot");
    let timeline = rv.timeline_panel_string();
    let group = timeline
        .split("<g class=\"tooltip-trigger\"")
        .find(|g| g.contains("#functionDot") && g.contains("Its resource is dropped"))
        .expect("a call dot whose tooltip includes the drop");
    let group = &group[..group.find("</g>").unwrap_or(group.len())];
    assert!(group.contains("class=\"drop-mark\""), "no drop mark below the call dot: {}", group);
}

#[test]
fn corpus_expected_ok_produce_well_formed_svg() {
    let mut failures = Vec::new();
    for name in EXPECTED_OK {
        // Don't unwind across cases — collect all failures so a single broken
        // case doesn't mask others.
        match std::panic::catch_unwind(|| {
            let rv = run_with_local_backend(name);
            assert_well_formed(name, &rv);
        }) {
            Ok(()) => {}
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "<non-string panic>".to_string());
                failures.push(format!("{}: {}", name, msg));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} corpus cases regressed:\n  {}",
        failures.len(),
        EXPECTED_OK.len(),
        failures.join("\n  ")
    );
}

#[test]
fn corpus_expected_tooltips_present() {
    let mut failures = Vec::new();
    for expect in EXPECTED_TOOLTIPS {
        let res = std::panic::catch_unwind(|| {
            let rv = run_with_local_backend(expect.name);
            assert_no_negative_sizes(expect.name, &rv.timeline_panel_string());
            let actual = timeline_tooltips(&rv.timeline_panel_string());

            let missing: Vec<&str> = expect
                .must_contain
                .iter()
                .copied()
                .filter(|want| !actual.iter().any(|a| a == want))
                .collect();
            let unexpected: Vec<&str> = expect
                .must_not_contain
                .iter()
                .copied()
                .filter(|forbid| actual.iter().any(|a| a == forbid))
                .collect();

            if !missing.is_empty() || !unexpected.is_empty() {
                let mut msg = String::new();
                if !missing.is_empty() {
                    msg.push_str(&format!("missing {:?}; ", missing));
                }
                if !unexpected.is_empty() {
                    msg.push_str(&format!("unexpectedly present {:?}; ", unexpected));
                }
                msg.push_str(&format!(
                    "actual tooltips were:\n  {}",
                    actual.join("\n  ")
                ));
                panic!("{}", msg);
            }
        });
        if let Err(payload) = res {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "<non-string panic>".to_string());
            failures.push(format!("{}: {}", expect.name, msg));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} tooltip assertions failed:\n  {}",
        failures.len(),
        EXPECTED_TOOLTIPS.len(),
        failures.join("\n  ")
    );
}

/// Snippets whose output used to depend on HashMap/HashSet iteration
/// order (#211). std's hash collections are seeded per process, so
/// each `cargo rv-plugin` run could pick a different order.
///
///   * `str_literal_copy`, `basic_deref3`, `multiple_imm_borrows`:
///     several borrows end on the same line and their return arrows
///     were drawn in a different order (and at different rows) per run.
///   * `overlapping_borrow_regions`: grouping chained borrows into
///     regions depended on set order, which changed which return
///     arrows were drawn at all.
///   * `struct_after_fn_hashes` (#231): two RAPs shared a hash, and
///     the fn line kept for that hash (which places the column labels)
///     was whichever one the HashMap visited last.
const DETERMINISM_CASES: &[&str] = &[
    "str_literal_copy",
    "basic_deref3",
    "multiple_imm_borrows",
    "overlapping_borrow_regions",
    "struct_after_fn_hashes",
];
const DETERMINISM_RUNS: usize = 6;

/// Rendering the same program twice must give the same bytes: the
/// playground and Pages caches key on the source, and byte-level
/// regression diffs need a stable baseline (#211).
#[test]
fn plugin_output_is_deterministic() {
    let mut failures = Vec::new();
    for name in DETERMINISM_CASES {
        let first = run_with_local_backend(name);
        assert_well_formed(name, &first);
        for run in 2..=DETERMINISM_RUNS {
            let again = run_with_local_backend(name);
            if again.code_panel_string() != first.code_panel_string()
                || again.timeline_panel_string() != first.timeline_panel_string()
            {
                failures.push(format!("{}: run {} differs from run 1", name, run));
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "plugin output changed between identical runs:\n  {}",
        failures.join("\n  ")
    );
}

/// Return arrows that end on the same line are drawn in source order:
/// the ref borrowed first returns first (top row), then the next.
/// `basic_deref3`: `y = &x` (line 3), `z = y` (4), `a = z` (5), all
/// last used on line 6.
#[test]
fn same_line_return_arrows_follow_source_order() {
    let rv = run_with_local_backend("basic_deref3");
    let returns: Vec<String> = timeline_tooltips(&rv.timeline_panel_string())
        .into_iter()
        .filter(|t| t.starts_with("Return immutably borrowed resource from"))
        .collect();
    assert_eq!(
        returns,
        [
            "Return immutably borrowed resource from y to x",
            "Return immutably borrowed resource from z to x",
            "Return immutably borrowed resource from a to x",
        ],
    );
}

/// A match arm whose body is a bare macro call used to take its line
/// range from the macro's expansion span (a line inside libstd), so
/// the binding's drop and borrow-end were drawn far below the panel.
/// `assert_well_formed` checks the same bound for every EXPECTED_OK
/// snippet; this names the regression directly.
#[test]
fn match_arm_macro_body_stays_on_canvas() {
    let rv = run_with_local_backend("match_arm_macro_body");
    let timeline = rv.timeline_panel_string();
    let off = off_canvas_ys(&timeline);
    assert!(off.is_empty(), "elements below the timeline SVG: {:?}", off);
    let code = rv.code_panel_string();
    let off = off_canvas_ys(&code);
    assert!(off.is_empty(), "elements below the code SVG: {:?}", off);
}

/// Visible text of an SVG fragment: tags stripped, the three entities the
/// plugin emits decoded.
fn svg_text(svg: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in svg.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// The code panel must show the user's source verbatim. The annotation
/// pass used `[_` / `_]` as stand-ins for `<` / `>`, so `[_` in ordinary
/// Rust became raw markup, and array literals on lines with several
/// annotations were emitted once per annotation.
#[test]
fn code_panel_shows_source_verbatim() {
    env::set_var("RV_RUNNER", "local");
    let lines = [
        "fn main() {",
        "    let x = 1;",
        "    let y = 2;",
        "    let w = [x, y];",
        "    let z: [_; 2] = w;",
        "    let s = String::from(\"[_b_] \u{E000}tspan\u{E001}\");",
        "    let t = s;",
        "    println!(\"{:?} {}\", z, t);",
        "}",
    ];
    let src = lines.join("\n") + "\n";
    let rv = Rustviz::new(&src).expect("plugin run");
    let code = rv.code_panel_string();
    let text = svg_text(&code);
    for line in &lines[..5] {
        let want = line.trim();
        assert_eq!(
            text.matches(want).count(),
            1,
            "expected {:?} exactly once in code panel text:\n{}",
            want,
            text
        );
    }
    assert!(text.contains("[_b_]"), "string literal mangled:\n{}", text);
    assert!(
        !code.contains('\u{E000}') && !code.contains('\u{E001}'),
        "marker code points leaked into output"
    );
    assert!(!code.contains("<b_]"), "user text became markup:\n{}", code);
}

/// `:::` in user source must not be taken as the plugin's panel separator
/// (#181). On the old plugin the code panel stopped at the first `:::` (the
/// comment) and the timeline panel began with the rest of that line.
#[test]
fn source_with_triple_colon_splits_correctly() {
    let rv = run_with_local_backend("triple_colon_source");
    let code = rv.code_panel_string();
    let timeline = rv.timeline_panel_string();
    assert!(
        code.trim_end().ends_with("</svg>"),
        "code panel truncated; ends with: {:?}",
        &code[code.len().saturating_sub(120)..]
    );
    assert!(
        timeline.trim_start().starts_with("<svg"),
        "timeline panel starts mid-markup: {:?}",
        timeline.chars().take(80).collect::<String>()
    );
    let text = svg_text(&code);
    for want in [
        "user's source (this comment ::: and the string below) must not be taken",
        "let s = String::from(\"a:::b\");",
        "println!(\"{}\", t);",
    ] {
        assert!(text.contains(want), "expected {:?} in code panel text:\n{}", want, text);
    }
}

/// A failing compile must surface as an error carrying rustc's diagnostic,
/// not as empty output. rustc_plugin 0.15 changed `cli_main` /
/// `driver_main` to *return* an `ExitCode`; dropping it made every failure
/// (compile errors and plugin panics alike) exit 0 with empty stdout,
/// which reached users as "Unexpected output format".
#[test]
fn compile_errors_are_reported_not_swallowed() {
    env::set_var("RV_RUNNER", "local");
    let err = Rustviz::new("fn f() {\n    let x: i32 = \"not a number\";\n}\n")
        .expect_err("a type error must not render");
    let msg = err.to_string();
    assert!(msg.contains("mismatched types"), "expected rustc's diagnostic, got:\n{}", msg);
}

/// The initializing assignment of a `let s;` drops nothing; only the
/// later reassignments do (#234). One drop each for s and t.
#[test]
fn deferred_init_drops_only_at_reassignment() {
    let rv = run_with_local_backend("deferred_init_then_reassign");
    let tooltips = timeline_tooltips(&rv.timeline_panel_string());
    for name in ["s", "t"] {
        let drop = format!("{} acquires ownership of a new resource; its previous resource is dropped", name);
        assert_eq!(
            tooltips.iter().filter(|t| **t == drop).count(),
            1,
            "expected {:?} exactly once, tooltips were:\n  {}",
            drop,
            tooltips.join("\n  ")
        );
    }
}

/// Numeric attribute `key` of an SVG start tag (the text after `<name `).
fn num_attr(tag: &str, key: &str) -> f64 {
    let k = format!(" {}=\"", key);
    let tag = format!(" {}", tag);
    let v = &tag[tag.find(&k).unwrap_or_else(|| panic!("no {} in {}", key, tag)) + k.len()..];
    let v = &v[..v.find('"').unwrap()];
    v.parse().unwrap_or_else(|_| panic!("{}={:?} is not a number", key, v))
}

/// Struct boxes: the rounded `<rect>`s drawn in the timeline header
/// around a struct's label and its field labels, as (left, right) x.
fn struct_boxes(timeline: &str) -> Vec<(f64, f64)> {
    timeline
        .split("<rect ")
        .skip(1)
        .map(|r| &r[..r.find('>').unwrap()])
        .filter(|r| r.contains("rx=\"20\""))
        .map(|r| {
            let x = num_attr(r, "x");
            (x, x + num_attr(r, "width"))
        })
        .collect()
}

/// x of the column label whose text is exactly `label`.
fn label_x(timeline: &str, label: &str) -> f64 {
    for t in timeline.split("<text ").skip(1) {
        let (tag, rest) = t.split_once('>').unwrap();
        if tag.contains("class=\"label") && rest.split('<').next() == Some(label) {
            return num_attr(tag, "x");
        }
    }
    panic!("no column label {:?}", label)
}

/// A `String` bound by `while let Some(s)` is an owner, not a struct, so
/// it gets no struct box (#192: an empty box with width="-80").
#[test]
fn pattern_bound_string_has_no_struct_box() {
    let t = run_with_local_backend("while_let_pop_string").timeline_panel_string();
    assert_eq!(struct_boxes(&t), vec![], "while_let_pop_string: unexpected struct box");
}

/// Back-to-back structs get one box each; `a`'s box used to stretch over
/// `b`'s columns too, and `b` got none (#192).
#[test]
fn adjacent_structs_get_one_box_each() {
    let t = run_with_local_backend("struct_box_adjacent").timeline_panel_string();
    let boxes = struct_boxes(&t);
    assert_eq!(boxes.len(), 2, "struct_box_adjacent: expected a box per struct, got {:?}", boxes);
    for (b, labels) in boxes.iter().zip([["a", "a.x", "a.y"], ["b", "b.x", "b.y"]]) {
        for l in labels {
            let x = label_x(&t, l);
            assert!(b.0 < x && x < b.1, "struct_box_adjacent: {} (x={}) outside box {:?}", l, x, b);
        }
    }
    assert!(boxes[0].1 < label_x(&t, "b"), "struct_box_adjacent: a's box {:?} covers b", boxes[0]);
}

/// A struct none of whose fields has a column gets a box around its own
/// label (#192: width="-80").
#[test]
fn struct_without_field_columns_boxes_its_own_label() {
    let t = run_with_local_backend("if_let_struct_fields_unused").timeline_panel_string();
    let boxes = struct_boxes(&t);
    let x = label_x(&t, "p");
    assert!(
        boxes.len() == 1 && boxes[0].0 < x && x < boxes[0].1,
        "if_let_struct_fields_unused: expected one box around p (x={}), got {:?}",
        x,
        boxes
    );
}

/// A guard that moves an outer variable (`Some(_) if consume(s) =>`) moves
/// it whenever the guard runs, even when the guard is false and a later
/// arm is taken. Drawing the move in the guard's arm only would show the
/// later arms still owning `s`, so the plugin refuses instead. (main used
/// to render it with no move at all and `s` dropped at the end.)
#[test]
fn guard_that_moves_is_refused() {
    env::set_var("RV_RUNNER", "local");
    let src = "fn consume(s: String) -> bool { s.len() > 3 }\n\
               fn main() {\n\
               \x20   let s = String::from(\"hello\");\n\
               \x20   let opt = Some(5);\n\
               \x20   match opt {\n\
               \x20       Some(_) if consume(s) => println!(\"yes\"),\n\
               \x20       _ => println!(\"no\"),\n\
               \x20   }\n\
               }\n";
    let err = Rustviz::new(src).expect_err("a guard that moves must not render");
    let msg = err.to_string();
    assert!(msg.contains("match guards that move or assign a variable declared outside the guard (`s`"),
        "expected the guard-move refusal, got:\n{}", msg);
}

/// #279: past the 12th hash, columns had no colour rule, so their state
/// lines had no stroke and only their dots showed. The fixture has more
/// hashes than the palette; every column's lines must get a stroke.
#[test]
fn columns_past_the_palette_get_a_stroke() {
    let rv = run_with_local_backend("many_columns_palette");
    let timeline = rv.timeline_panel_string();
    let body = &timeline[timeline.find("]]>").unwrap()..];
    let line_hashes: Vec<u64> = body
        .split('<')
        .filter(|tag| {
            (tag.starts_with("line ") || tag.starts_with("path "))
                && (tag.contains("class=\"hollow") || tag.contains("class=\"solid"))
        })
        .filter_map(|tag| tag.split("data-hash=\"").nth(1)?.split('"').next()?.parse().ok())
        .collect();
    assert!(
        line_hashes.iter().any(|&h| h > 12),
        "fixture no longer has a column past the 12-colour palette (state-line hashes {:?}); \
         add variables so the test still covers #279",
        line_hashes
    );
    assert_eq!(hashes_without_colour(&timeline), Vec::<u64>::new());
    assert_eq!(hashes_without_colour(&rv.code_panel_string()), Vec::<u64>::new());
}
