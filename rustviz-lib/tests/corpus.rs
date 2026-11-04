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
            "iter_mut reads from/writes to v",
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
            "push_str reads from/writes to r.s",
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
            "push_str reads from/writes to s",
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
            "len reads from s",
            "Copy from len to n",
        ],
        must_not_contain: &[],
    },

    TooltipExpect {
        name: "chained_field_assign",
        // "acquires ownership" for an i32 field is the plugin's existing
        // wording for every struct-field write (flat `o.x = 5` says the
        // same on main); this entry pins that the chained write lands on
        // `o.inner.x`'s column, not that wording.
        must_contain: &[
            "o.inner.x, mutable",
            "o.inner.x acquires ownership of a resource",
        ],
        must_not_contain: &[],
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
            "last_mut reads from/writes to v",
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
            "rename reads from/writes to u",
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
            "get reads from r",
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
        // branch. Mixed move/alive merge → drop dot with the
        // implicit-drop explanation (Rust drops `s` at the end of
        // any branch that didn't move it so the post-state is
        // consistent across branches).
        must_contain: &[
            "Move from s to consume",
            "s was moved in at least one branch above; \
             in branches where it was not, its resource is dropped at the branch's end.",
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
        // other → mixed → inner merge gets the implicit-drop
        // tooltip. Outer if: both branches end without the
        // resource (else by direct consume, then-arm by the inner
        // merge's implicit drop) → outer merge says every branch.
        must_contain: &[
            "s was moved in at least one branch above; \
             in branches where it was not, its resource is dropped at the branch's end.",
            "s was moved or dropped in every branch above",
        ],
        must_not_contain: &["merge"],
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
        // branches' end states; the outermost ends up all-moved
        // because every path reaches a consume (some directly,
        // some via the chain of nested merges already inserting
        // implicit drops).
        must_contain: &[
            "Move from s to consume",
            "s was moved or dropped in every branch above",
            // At least one inner merge is the mixed implicit-drop
            // case (consume on one path, borrow on the other).
            "s was moved in at least one branch above; \
             in branches where it was not, its resource is dropped at the branch's end.",
        ],
        must_not_contain: &[
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
            "s was moved in at least one branch above; \
             in branches where it was not, its resource is dropped at the branch's end.",
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
            "s was moved in at least one branch above; \
             in branches where it was not, its resource is dropped at the branch's end.",
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
            "s was moved in at least one branch above; \
             in branches where it was not, its resource is dropped at the branch's end.",
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
            "n is bound to a value",
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
            "n is bound to a value",
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
            "x is bound to a value",
            "y is initialized by copy from x",
            "Copy from x to y",
        ],
        must_not_contain: &[
            // The const itself shouldn't get a column or events.
            "N is the owner",
            "N holds a value",
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
            "x is bound to a value",
            "the resource p points to is modified through *p",
        ],
        must_not_contain: &["*p is bound to a value"],
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
            // `u._a = y;` — the field write lands on u's column.
            "Copy from y to u",
            "u is initialized by copy from y",
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
            "x is bound to a value",
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
            "r is bound to a value",
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
            "borrow_mut reads from c",
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
            "borrow_mut reads from cell",
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
            "Rc::clone reads from value",
            "Move from Rc::clone to a",
            "borrow_mut reads from value",
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
            "last_mut reads from/writes to v",
            "entry reads from/writes to map",
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
            "area reads from rect1",
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
            "clone reads from s",
            "is_some reads from c",
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
        // drops) its vector. `v.len()` takes `&self`.
        must_contain: &[
            "into_iter reads from r",
            "len reads from v",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "Move from r to into_iter",
            "Move from v to into_iter",
            "v's resource is moved",
            "v goes out of scope. No resource is dropped.",
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
            "len reads from t",
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
            "last reads from words",
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
            // reads from v") is the read, not a read of `len`.
            "panic! reads from len through an implicit reference, so len isn't moved",
            "assert_eq! reads from len through an implicit reference, so len isn't moved",
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
    for (panel, svg) in [("code", &code), ("timeline", &timeline)] {
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
fn timeline_tooltips(timeline_svg: &str) -> Vec<String> {
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
        let mut clean = String::with_capacity(unescaped.len());
        let mut in_tag = false;
        for ch in unescaped.chars() {
            if ch == '<' { in_tag = true; continue; }
            if ch == '>' { in_tag = false; continue; }
            if !in_tag { clean.push(ch); }
        }
        out.push(clean);
    }
    out
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
