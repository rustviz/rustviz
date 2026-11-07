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
    "tuple_field_read",     // `let a = t.0;`: Copy from t.0 (#189)
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
    //   `r.f = …` through a `&mut`, `v[i] = …`). These used to panic in
    //   `resource_of_lhs`. Field writes through a reference are now
    //   drawn on the reference's column (#147); index writes aren't.
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
    // A `move` closure copies the Copy values it captures (#311).
    "move_closure_copy_capture",
    // — A reference copied out of an `Option<&T>` / tuple of references
    //   by a pattern doesn't borrow the matched value (#304).
    "match_copy_ref_out_of_option",
    "match_copy_ref_out_of_tuple",
    "iflet_copy_ref_out_of_option",
    "match_ref_binding_borrows",
    // Copy values are "values" in copy / borrow tooltips too (#274).
    "copy_value_wording",
    // — An explicit `ref` / `ref mut` binding on an owned value borrows
    //   the matched place; it isn't a copy or a move (#338).
    "ref_binding_borrows_owned",
    // `if` without `else` whose body moves something: drawn like an
    // `if` with an empty `else` (#257).
    "if_no_else_partial_move",
    "if_let_no_else_move",
    "if_no_else_read_only",
    "if_else_empty_same_line",
    "if_no_else_nested_move",
    // A reference to a Copy value says "the borrowed value" (#312).
    "ref_copy_referent_wording",
    // Borrowing from a reference borrows what it points to (#313).
    "reborrow_through_ref_wording",
    // Comparison operators borrow their operands (#366); operator names
    // are escaped in tooltips (#376).
    "comparison_operand_borrows",
    "comparison_ref_operands",
    "operator_name_escaped",
    // Conditionals whose branches share lines (#117, #333, #354) and
    // an `else if` chain (#355).
    "if_one_line_move",
    "match_one_line",
    "if_else_next_line",
    "else_if_chain",
    // A fn parameter written as a pattern (`(a, b): (String, i32)`,
    // `P { name, .. }: P`) gives each of its names a column (#331).
    "param_patterns",
    // ... also when an if/match picks between literals, and for `&CONST`.
    "ref_static_lender_branches",
    "ref_static_lender_const",
    // Two refs borrowing the same variable on one line (#233).
    "two_refs_same_line",
    "two_refs_tuple",
    // A closure captures only the fields it uses (Rust 2021; #328).
    "closure_disjoint_field_capture",
    "closure_whole_struct_capture",
    // Same for tuple elements (#634).
    "closure_tuple_element_capture",
    // A Copy variable bound in every branch is "bound to a value" at
    // the merge, not "acquired ownership of a resource" (#316).
    "merge_bound_copy",
    // A write through `&mut` / `Box` says the old value is dropped.
    "deref_write_drops_old",
    // `*r = e` reads like a plain assignment (#474).
    "deref_assign_wording",
    // `drop(x)` states that it drops x.
    "mem_drop_states_drop",
    // Method chains: later calls act on temporaries, but their
    // arguments are still moved / borrowed (#132). Same for a receiver
    // with no column, like `self.items` (#250, #249).
    "method_chain_temporaries",
    "method_chain_args",
    "method_arg_unnamed_receiver",
    "method_call_in_unnamed_receiver_arg",
    // Arguments to a call of a local closure (or other callable
    // variable) go to the call, not into the closure's column
    // (#201, #210, #258, #284).
    "closure_call_mutref_arg",
    "closure_call_mutref_var",
    "closure_call_shared_ref_arg",
    "closure_call_copy_arg",
    "closure_call_move_arg",
    "generic_fn_param_call",
    "fn_item_local",
    "fn_pointer_local_call",
    "closure_call_result",
    // A struct bound from an overloaded operator's result (#314).
    "operator_result_struct_fields",
    // Reassigning a reference variable is an assignment (#414).
    "ref_reassign_wording",
    "long_ref_field_label",
    "long_ref_label_then_short",
    // Element / slice borrows through a reference are reborrows (#407).
    "index_reborrow_through_ref",
    // `ref` bindings in by-value parameters borrow the value the
    // function owns, not the caller's (#377).
    "param_ref_binding_own",
    // Inherent-method shapes (#136): one-field accessor, each receiver
    // kind, unit struct, nested struct consumed by a method (#319).
    "inherent_method_one_field",
    "inherent_method_receivers",
    "inherent_method_unit_struct",
    "nested_struct_moved_into_fn",
    // `ref` / `ref mut` in a `let` pattern borrows (#380).
    "let_ref_binding_borrows",
    // `let ref d = *r;` used to panic in find_lender (#422).
    "let_ref_deref_reborrow",
    // A match arm of type `!` (`_ => unreachable!()`, `panic!()`)
    // binds nothing and doesn't reach the merge (#296).
    "match_diverging_arm",
    // An arm that calls a `-> !` function diverges too (#296).
    "diverging_call_arm",
    // A diverging branch's column stops at its end (#317).
    "diverging_branch_stops",
    // — Element writes (#144): `v[i] = …` is drawn on v's column as a
    //   modification of v's resource, directly or through a reference.
    "index_assign",
    "index_assign_through_ref",
    // A write through a reference / Box stored in an element (#396).
    "write_through_element_ref",
    // Nested struct fields of fn params and pattern bindings get
    // columns (#239); a struct param's reference field borrows from
    // the caller (#351).
    "nested_fields_params_patterns",
    // Values still owned at a `return` in a branch are dropped there
    // (#363); a `let` in a match arm is dropped at the arm's end (#409).
    "return_in_arm_drops",
    "match_arm_with_let",
    // A pattern-bound struct's reference fields are references (#352).
    "pattern_struct_ref_fields",
    // A pattern-bound struct's fields live and die with it, inside its
    // `if let` / match arm / `while let` (#405).
    "pattern_struct_fields_scope",
    // A write to a place with no column states the old value's drop (#458).
    "place_without_column",
    // Tuples get a column per element, like struct fields (#189).
    "tuple_element_move",
    "tuple_create_copy",
    "tuple_destructure_var",
    "tuple_field_write",
    "tuple_through_calls",
    // A borrow stored in an element or other column-less place names
    // its borrower and ends (#282).
    "borrow_into_element",
    // Destructuring a struct variable moves each field into its own
    // name, not all of the struct (#326).
    "struct_destructure_let",
    "struct_destructure_match",
    // Compound assignment (`acc += x`, `s += &t`) isn't `a = b` (#260).
    "compound_assign",
    // Fields moved out on some paths only: the struct's scope end says
    // they're dropped only if no branch moved them (#395).
    "partial_move_one_arm_tuple",
    "partial_move_one_arm_struct",
    "partial_move_if_branches",
    // "resource" only for values that need drop (#484).
    "no_drop_iter",
    "non_copy_struct_move",
    "string_resource_wording",
    "sync_lock_resource",
    "no_drop_reassign_branch",
    "no_drop_partial_maybe",
    // A temporary a `let` borrows gets a column to the end of the block (#400).
    "extended_temporary",
    // A `&mut` passed to a generic `T` is moved, not reborrowed (#200);
    // a move into a call survives a borrow ending on the same line.
    "mutref_moved_into_generic",
    "move_into_call_same_line_borrow_end",
    // — Shadowing (#173, #253): each binding gets its own column.
    "shadow_after_move",
    "shadow_after_move_into_fn",
    "shadow_with_live_borrow",
    "shadow_in_block",
    "shadow_sibling_blocks",
    "shadow_shared_columns",
    "shadow_match_arm",
    "shadow_rebind_chain",
    // Shadowing matrix (#308 deep dive): re-lets, scopes, patterns,
    // closures.
    "shadow_matrix_let",
    "shadow_matrix_scopes",
    "shadow_header_names",
    "shadow_matrix_patterns",
    "shadow_matrix_closures",
    "shadow_closure_fate",
    // A call's result nothing takes is a temporary, dropped at the end of its statement (#457).
    "unbound_temporary",
    // A reference parameter's borrow end on a line shared with another
    // arrow (#416).
    "ref_param_end_shared_line",
    // A reference bound by an `if` starts its bracket at the join (#489).
    "branch_ref_bracket_if",
    // Hovering an arm's column underlines only that arm's own lines
    // (#369); the check is `arm_underline_off_its_lines`, run on every
    // case above.
    "if_else_arm_columns",
    "let_if_else_arm_columns",
    "match_three_arm_columns",
    "if_body_on_split_line",
    "else_on_join_line",
    // Loop variables and loop-body locals start and end (#205, #256,
    // #261).
    "for_var_call_result",
    "for_var_range",
    "loop_body_local_drop",
    "loop_var_in_branch",
    "for_var_string_drop",
    // A borrow by a reference declared in an arm ends in that arm
    // (#389); `borrow_end_in_arm_is_drawn_in_the_arm` checks where.
    "borrow_end_in_if_arm",
    "shared_borrow_end_in_if_arm",
    "borrow_end_in_match_arm",
    "borrow_end_in_nested_if",
    // One-line if / if let / match: condition events go on the split
    // dot, events after the conditional on the join dot (#492).
    "one_line_cond_events",
    // A scrutinee that moves a value is evaluated before the branch
    // (#450); one-line matches on a temporary used to panic (#459).
    "if_let_scrutinee_move",
    "if_let_scrutinee_move_else_move",
    "match_scrutinee_move",
    "match_on_temporary_one_line",
    "match_on_temporary_variants",
    // A borrow stored in a field through a local `&mut` (#512), or by a
    // call whose signature lets it store the reference (#513).
    "borrow_stored_through_local_ref",
    "borrow_stored_by_call",
    "match_arm_macro_ref_binding",
    "match_arm_call_mut_ref_binding",
    "iflet_one_line_ref_binding",
    // An `if let` / `while let` pattern that borrows, copies or moves
    // is drawn at the start of its arm's column (#382).
    "if_let_binding_borrow",
    "if_let_binding_copy",
    "if_let_binding_move",
    "while_let_binding_kinds",
    // A reference declared before an if/match whose last use is in an
    // arm: its borrow ends in each arm (#495);
    // `outer_borrow_end_is_drawn_in_each_arm` checks where.
    "outer_ref_end_in_match_arm",
    "outer_ref_end_in_if_arm",
    "outer_mut_ref_end_in_if_arm",
    "outer_ref_end_in_nested_if",
    // An arm written on the `if` / `match` line starts on that row (#508).
    "if_arm_on_split_line",
    "match_arm_on_split_line",
    "if_arm_on_split_line_cond_event",
    // Each segment of a branch column shows its own tooltip when
    // hovered (#388); the check is `hover_polygons_off_their_segment`,
    // run on every case above.
    "branch_hover_per_segment",
    "match_arm_hover_per_segment",
    // A reassignment after a conditional move drops the old value only
    // if it is still there (#335).
    "reassign_after_maybe_move",
    "reassign_after_maybe_move_no_else",
    "reassign_after_maybe_move_match",
    "reassign_after_nested_maybe_move",
    // A moved `&mut` ends no borrow (#209); one coerced into an
    // annotated `let` / struct field is reborrowed, not moved (#289).
    "mutref_moved_on",
    "mutref_coerced_reborrow",
    // Struct boxes widen to contain their end columns' header labels
    // (#477); pinned by `struct_box_contains_long_member_labels`.
    "struct_box_long_last_label",
    "struct_box_long_owner_label",
    // — Functions returning a reference (#171): the result borrows from
    //   the arguments its signature ties it to; tooltips pinned in
    //   EXPECTED_TOOLTIPS.
    "fn_ret_ref_first_word",
    "fn_ret_ref_longest",
    "fn_ret_ref_longest_literal",
    "fn_ret_ref_methods",
    "fn_ret_ref_mut",
    "fn_ret_ref_unrelated_arg",
    "fn_ret_ref_iter_next",
    "fn_ret_ref_reassign",
    "fn_ret_ref_struct_field",
    "fn_ret_ref_arg_lists",
    // An explicit `return v` moves `v` to the caller (#318).
    "return_early_in_if",
    "return_explicit_last",
    "return_in_match_arm",
    // A branch that moves a value and then `break`s (#344).
    "loop_break_after_move",
    // The merge after branches that moved a value and left early.
    "join_after_early_exit",
    // The same, for a value with nothing to drop (#484).
    "return_early_no_drop",
    // A call inside an arm adds no blank row (#419);
    // `arm_calls_add_no_blank_rows` checks.
    "arm_call_no_blank_row",
    "arm_call_borrow_end_no_blank_row",
    "match_arm_call_no_blank_row",
    // Writes to struct fields through a reference (`p.x = …` with
    // `p: &mut P`, `self.c = …`, `r.x = …`) are drawn on the
    // reference's column (#147), also inside if / match arms.
    "field_write_through_ref",
    "field_write_through_ref_branch",
    // `*r = e` reads like a plain assignment (#474).
    "deref_assign_wording",
    // `p.x += k` through a reference keeps "modified through", with no
    // "(copied from k)" (#260).
    "field_compound_assign_through_ref",
    // — User macros: one that forwards to println! still renders (#342).
    "user_macro_forwards",
    // — A reference returned by a call (or a borrow) in an `if` /
    //   `match` branch initializer borrows from every branch's lender
    //   (#337); tooltips pinned below.
    "branch_call_ref_if",
    "branch_call_ref_match",
    "branch_call_ref_method",
    "branch_ref_if_addr",
    // By-reference captures of a closure passed straight to a call (#298).
    "closure_arg_ref_captures",
    // A read through a reference / Box stored in an element (#438).
    "read_through_element_ref",
    // Thick `let mut` branch columns bend at splits and joins (#487).
    "mut_branch_kinks",
    "mut_branch_kinks_plain",
    // — A variable that holds a borrow without being a reference
    //   (`let it = v.iter();`, `let third = v.get(2);`, a struct with a
    //   lifetime returned by a call; #336); tooltips pinned below.
    "holder_iter_get",
    "holder_iter_mut",
    "holder_moved",
    "holder_consumed",
    "holder_reassign",
    "holder_struct_from_call",
    "holder_closure_capture",
    "holder_closure_ref",
    // A value moved into a call on a line with another arrow (#379).
    "move_into_call_shared_line",
    // A reassignment inside an if/else or match arm shows the previous
    // value being dropped (#267).
    "reassign_in_branch_drop",
    "reassign_through_ref_in_branch",
    // An `else if` chain / `else { match .. }` whose branches all
    // return diverges as a whole (#491).
    "else_if_chain_diverges",
    // — Collections of references (`vec![&s]`, `[&a, &b]`) and
    //   references copied out of them (`refs[0]`; #349).
    "refs_vec_literal",
    "refs_array_literal",
    "refs_vec_mut",
    "refs_index_untracked",
    // A value or reference that borrows through a reference parameter
    // (`v.iter()`, `first_word(s)` with `v: &Vec<i32>`, `s: &String`;
    // #393); tooltips pinned below.
    "holder_ref_param_iter",
    "holder_ref_param_iter_mut",
    "fn_ret_ref_through_param",
    // Copy values aren't resources: a `for` element of an owned Vec<i32>
    // (#504), a Copy parameter (#505); a reassignment says one thing (#506).
    "for_copy_elements",
    "copy_param_wording",
    "reassign_one_bullet",
    // — Destructuring assignment (#151): `(a, b) = (b, a)` draws each
    //   variable assigned from its part of the RHS; rustc's `lhs`
    //   temporaries get no column.
    "tuple_destructure_assign",
    "destructure_assign_parts",
    // A reference read out through a reference stored in an element (#453).
    "read_ref_through_element_ref",
    // Calling an `FnOnce` closure moves it (#271).
    "fnonce_call_drops_capture",
    "fnonce_call_returns_capture",
    "fnonce_call_conditional",
    "fnonce_call_captures",
    "fnonce_call_copy_result",
    "fnonce_closure_moved_to_binding",
    "fnmut_call_unchanged",
    // A built-in comparison (`t < lim`) reads both operands (#497).
    "comparison_reads",
    // — A collection filled by `collect()` or `push(&s)` holds the
    //   borrows of the references it stores (#392), and a reference
    //   passed for a by-value generic parameter is carried by the
    //   result (`id(&s)`, `Some(&s)`; #424); tooltips pinned below.
    "collect_push_borrows",
    "push_mut_refs",
    "generic_by_value_ref",
    "push_owned_not_holder",
    // Swapping references keeps both lenders borrowed (#291).
    "ref_swap_keeps_borrow",
    // A write through a local `&mut` drops the old value on the
    // lender's field column (`s.a`), also for `*w = S { .. }` (#152).
    "field_write_through_local_ref",
    // Rc / Arc / Weak: a drop lowers a count, clones share the value,
    // and the callee (not the name `clone`) decides (#373).
    "rc_handles",
    "rc_branch",
    "rc_clone_by_callee",
    // The same wording for an Rc/Arc that is a struct field, and for
    // Weak::clone (#373).
    "rc_fields",
    "rc_weak_clone",
    // An Rc/Arc/Weak inside Option/Box/Vec/HashMap/RefCell/a tuple (#373).
    "rc_inside",
    "rc_inside_flag",
    "rc_inside_tuple",
    // An `if`/`else` or `match` whose branches all `return` ends the
    // function there: no merge, no scope-end drops (#515).
    "if_else_both_return",
    "return_value_to_caller",
    // Array literals, `vec![…]` elements and copies out of an index
    // (`let first = v[0];`) (#297).
    "array_literal_moves",
    "vec_macro_moves",
    "index_copy_read",
    // A variable declared inside a branch, also in an inline `if` /
    // `else if let` / `for` nested in it, exists only there (#501).
    "else_if_let_binding",
    "match_arm_nested_if_let",
    "if_branch_nested_decls",
    "while_let_in_branch",
    // An `else if let` reusing the `if let` binding's name used to panic
    // in fetch_timeline (#525).
    "else_if_let_same_name",
    "else_if_let_same_name_else_let",
    // An arm's binding on the line where a nested `match` / `if` splits
    // holds its value in every inner branch (#502).
    "nested_match_arm_binding",
    "match_arm_binding_if_same_line",
    // A reference field overwritten directly (#531).
    "borrow_field_overwrite_direct",
    // Reads of fields through a reference (`let a = p.x;`, `&p.name`,
    // `take(p.x)`, `p.name.len()`, a returned `self.x`) are drawn on
    // the reference's column (#266).
    "field_read_through_ref",
    // Mutable borrows of fields through a `&mut` (`&mut r.a`,
    // `r.a.push_str(..)`) are reborrows on the reference's column, and
    // a write through them drops the owner's field (#202, #323).
    "field_mut_borrow_through_ref",
    // Copying out of an `if let` scrutinee doesn't branch it (#496).
    "if_let_copy_and_move_out_of_scrutinee",
    // A let chain's scrutinee temporary lives until the matched block
    // ends or the `else` runs (#604). Let chains need Rust 2024: Cargo
    // compiles each tests/*.rs as an integration test in the plugin
    // crate's edition (2021), but doesn't look in subdirectories. The
    // plugin builds user code in edition 2024.
    "edition2024/temporary_let_chain_scrutinee",
    // Or-pattern arms bind their names like a single pattern (#503).
    "or_pattern_arm",
    "or_pattern_arms",
    "or_pattern_if_let",
    "or_pattern_shadow",
    // — A reference a pattern copies out of a value that holds a
    //   borrow (`Some(x)` on `v.get(2)`) borrows what that value
    //   borrows (#391); tooltips pinned below.
    "holder_match_pattern",
    "holder_iflet_call",
    // A `let` inside a `match` arm lives in that arm only (#425).
    "match_arm_let_local",
    "match_arm_let_owned_move",
    "match_ref_param_reborrow",
    "match_mut_ref_param_reborrow",
    "match_local_ref_reborrow",
    // `if let` / `while let` chains (#198, #280). These need Rust 2024,
    // so they live in tests/edition2024/: Cargo compiles each
    // tests/*.rs as an integration test in the plugin crate's edition
    // (2021), which rejects let-chains, but doesn't look in
    // subdirectories. The plugin builds user code in edition 2024.
    "edition2024/if_let_chain",
    "edition2024/if_let_chain_move",
    "edition2024/if_let_chain_first_part",
    "edition2024/while_let_chain",
    "anonymous_copy_source",
    // Exits other than a `return` ending a branch drop what they leave
    // (#410): panics, `break` / `continue`, `return s.len()`.
    "exit_panic_drops",
    "exit_unreachable_else",
    "exit_break_continue_drops",
    "exit_continue_in_match",
    "exit_return_borrowed_value",
    "exit_return_after_nested_if",
    "exit_path_dependent_drops",
    "exit_return_in_let_if",
    // An exit drops an unbound temporary (#457) only while its
    // statement is running.
    "exit_temporary_drops",
    // A reference whose lender is unknown ends its borrow without naming one (#463).
    "ref_unknown_lender_end",
    // The `?` operator's early return drops what the function owns
    // there, on the error path only (#430).
    "try_drops_owned",
    "try_in_branch",
    "try_in_loop_two_on_a_line",
    // A whole struct overwritten drops its old value: the borrows its
    // reference fields held end at the write (#538).
    "borrow_struct_overwrite_whole",
    // References to `[i32]` / `str` point to values (#549).
    "slice_ref_value",
    // Copying `&T` / moving `&mut T` says "value" or "resource" by the
    // pointee (#551).
    "ref_copy_value_wording",
    // Elements a `let` pattern ignores (`..`, `_`) are still moved into
    // the literal's temporary, which has a `(temporary N)` column (#439).
    "let_pattern_literal_unbound_moves",
    "let_pattern_literal_temp_scopes",
    "edition2024/if_let_chain_cond_reads_x",
    "edition2024/else_if_let_chain_two_bindings",
    // A collection's drop says it drops what it holds (#542).
    "collection_drop_holds",
    "collection_drop_holds_flag",
    "collection_drop_holds_exit",
    // An array of Strings drops every String it holds (#544).
    "array_of_strings_drop",
    // The temporary a `ref` binding of a value extends, or a literal with
    // only fresh elements, is a column and its drop is stated (#483).
    "let_ref_temporary_value",
    "let_literal_fresh_elements",
    // A `let`-extended temporary dropped at a `return` says so (#553).
    "let_temp_dropped_at_return",
    "let_temp_dropped_at_return_only",
    // ... and at a `break` / `continue` or a panic (#553 with #410).
    "let_temp_dropped_at_break",
    "let_temp_dropped_at_panic",
    // `if let` with a tuple pattern binds its names; a tuple-literal
    // scrutinee moves its variables whatever the pattern does (#327,
    // #390).
    "iflet_tuple_pattern",
    "match_tuple_literal_moves",
    // Where the tuple a tuple-literal scrutinee builds is dropped, and
    // what of its variables' values goes with it (#327).
    "tuple_scrutinee_drops",
    // A `ref` binding out of `(x, y)` borrows the tuple, not x (#580).
    "tuple_scrutinee_ref",
    // An early exit nested in an arm / if branch drops a tuple
    // scrutinee at the exit (#327 on #410); `process::exit` drops nothing.
    "tuple_scrutinee_exit_drops",
    // A reference copied out of a holder gives its borrow back at its
    // last use while the holder still holds it (#575).
    "holder_ref_ends_first",
    // Same-named variables of sibling blocks share a column; each ends
    // at its own `}` (#589).
    "shared_column_scope_ends",
    // A bare block nested in a branch ends its locals at its `}` (#591).
    "nested_block_in_branch_scope_end",
    // A let-chain part that moves an outer variable before a part that
    // can still be false (#281).
    "edition2024/if_let_chain_else_move",
    "edition2024/if_let_chain_no_else_move",
    "edition2024/if_let_chain_call_move",
    "edition2024/if_let_chain_irrefutable_move",
    "edition2024/if_let_chain_else_reassign",
    "edition2024/if_let_chain_field_move",
    "edition2024/if_let_chain_closure_move",
    // A `ref` binding in a match arm keeps its borrow until its last use,
    // not to the end of the arm (#599); a guard that doesn't use a `ref`
    // binding used to panic (#611).
    "match_ref_binding_last_use",
    "match_ref_binding_last_use_arms",
    "match_ref_mut_guard_last_use",
    "match_ref_guard_unused",
    "match_shared_ref_binding_last_use",
    // A write through a `&mut` on the line its borrow ends (#587).
    "deref_write_ends_borrow",
    // Or-pattern arms that bind a name from different places (#533).
    "or_pattern_different_places",
    "or_pattern_different_values",
    "or_pattern_different_places_copy",
    "or_pattern_different_fields",
    "or_pattern_three_places",
    "or_pattern_tuple_scrutinee",
    // Let-chain copies stay before the split (#583); a chain's bindings
    // are drawn beside their sources, also in `while let` (#584).
    "edition2024/if_let_chain_copy_first",
    "edition2024/if_let_chain_pop_and_copy",
    "edition2024/while_let_chain_two_bindings",
    "edition2024/while_let_chain_two_sources",
    "box_field_move",
    "box_field_move_branch",
    "box_field_move_all",
    "box_field_move_param",
    // A whole struct overwritten by a call or a variable (#569).
    "borrow_struct_overwrite_call",
    "box_contents_move",
    "box_contents_move_string",
    "box_contents_move_pattern",
    "box_whole_move",
    // Or-pattern arms that bind a reference, tuple or struct from
    // different places (#565).
    "or_pattern_ref_different_places",
    "or_pattern_ref_last_use",
    "or_pattern_ref_mut_different_places",
    "or_pattern_ref_through_refs",
    "or_pattern_struct_different_places",
    "or_pattern_tuple_different_places",
    "or_pattern_shadowed_name",
    "or_pattern_ref_tuple_scrutinee",
    // A struct holding a borrow, moved to another variable (#637).
    "borrow_struct_moved_holder",
    // A struct field of a struct overwritten by a call (#638).
    "borrow_struct_field_overwrite_call",
    // A struct field of a struct overwritten by another variable (#655).
    "borrow_struct_field_overwrite_var",
    // A `?` in a let-chain part after the first, or in a branch of an
    // `if` with an `else`, draws the drops of its early return (#636).
    "edition2024/if_let_chain_else_try",
    "edition2024/if_let_chain_else_try_moved",
    "try_in_if_else",
    "try_in_match_arm",
    // `*r` returned with r a reference returns a copy of the value r
    // points to, not a reference (#670).
    "return_deref_copy",
    "return_deref_copy_early",
    "return_deref_copy_param",
    "return_deref_copy_ref",
    // Temporaries of std macros, literals, `if let` / `while let`
    // scrutinees and `for` heads are dropped and stated (#469).
    "temporary_drop_macros_literals",
    "temporary_drop_if_while_for",
    // A `(temporary)` column still alive at an early exit is dropped
    // there, with the exit's wording (#469 with #410).
    "temporary_drop_early_exit",
    // The same in `if let` / `while let` / `let` (#597).
    "or_pattern_if_let_different_places",
    "or_pattern_if_let_different_values_else",
    "or_pattern_while_let_different_places",
    "or_pattern_let_different_places",
    "or_pattern_let_different_fields_reassign",
    "or_pattern_let_different_places_copy",
    "or_pattern_if_let_tuple_scrutinee",
    "or_pattern_let_shadowed_name",
    // Exits in an `if` without `else` drop what they leave (#429).
    "exit_if_no_else_return",
    "exit_if_no_else_panic",
    "exit_if_no_else_break",
    "exit_if_no_else_nested",
    "exit_if_let_no_else_return",
    "exit_if_no_else_one_line",
    "exit_if_let_scrutinee_move_return",
    // A `?` in a branch drawn with columns keeps its dots (#430).
    "try_in_if_else_branch",
    // Same-named variables in separate scopes share a column only if
    // they are the same kind of value (#598); the first one used to panic.
    "ref_binding_name_reused",
    "ref_binding_name_reused_mut",
    "let_ref_then_owner_same_name",
    "match_macro_arm_name_reused",
    "same_name_copy_kinds_share",
    // An owner that needs drop and one that doesn't can't share a column:
    // the wording ("resource" / "value") follows the type (#642).
    "same_name_drop_kinds_split",
    "same_name_plain_owners_share",
    // A variable moved into a macro's or literal's temporary (#570).
    "temporary_moves_variable",
    // A for loop's iterator with drop glue is dropped when the loop ends (#482).
    "for_iterator_drop",
    // A let-chain part that assigns to an outer variable, calls an outer
    // FnOnce closure, or moves out of a Box, before a part that can
    // still be false (#579).
    "edition2024/if_let_chain_assign",
    "edition2024/if_let_chain_assign_owner",
    "edition2024/if_let_chain_assign_no_else",
    "edition2024/if_let_chain_fnonce_call",
    "edition2024/if_let_chain_box_move",
    "edition2024/if_let_chain_nested_effects",
    // A tuple-pattern for loop moves or borrows its head (#527, #576),
    // and a `break 'outer` drops the iterators of the loops it leaves
    // (#592).
    "for_tuple_pattern_move",
    "for_tuple_pattern_borrow",
    "for_iterator_break_outer",
    // `let r = loop { … break k; }` gives `r` the value (#431).
    "loop_break_value_moved",
    "loop_break_value_outer_var",
    "loop_break_value_labelled",
    "loop_break_value_receivers",
    "loop_break_value_in_branch",
];

/// Tooltip-level expectations per snippet. `must_contain` strings have to
/// appear verbatim as a `data-tooltip-text` value somewhere in the rendered
/// timeline; `must_not_contain` strings must not appear *anywhere inside*
/// any tooltip (a substring check, so a fragment like "(copied from" works;
/// #511). `must_contain` stays whole-tooltip equality. Adding new tooltips
/// downstream doesn't break the test — only removing a listed one (or
/// producing text that's been forbidden) does.
struct TooltipExpect {
    name: &'static str,
    must_contain: &'static [&'static str],
    must_not_contain: &'static [&'static str],
}

/// `must_not_contain` matching: a substring of any tooltip, unless the entry
/// starts with `=`, which means the whole tooltip must equal the rest. Use
/// `=` when the forbidden text is a full tooltip that is also a prefix or
/// suffix of a correct one (`len reads from s` vs `len reads from s
/// (implicitly borrowed)`, `f, immutable` vs `self, immutable`).
fn forbidden_present(forbid: &str, actual: &[String]) -> bool {
    match forbid.strip_prefix('=') {
        Some(whole) => actual.iter().any(|a| a == whole),
        None => actual.iter().any(|a| a.contains(forbid)),
    }
}

/// Forbidden fragments main still prints: (snippet, fragment, why). Pinned
/// so the suite is green while the bug is tracked; the test fails when the
/// text disappears, so the entry gets moved to `must_not_contain`.
const KNOWN_STILL_PRINTED: &[(&str, &str, &str)] = &[
    // (#517's if_let_no_else entry is gone: with #496/#517, `opt` is maybe-moved
    // after a single-arm `if let`, so "in a conditional expression" is true.)
];

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
            // (A Copy value is a "value", not a "resource", #274.)
            "y's value is copied to the caller",
        ],
        must_not_contain: &[
            "y's resource is moved to the caller",
            "y's resource is copied to the caller",
        ],
    },
    TooltipExpect {
        name: "copy_return_param",
        // i32 and bool returns are copies, never moves.
        must_contain: &[
            "x's value is copied to the caller",
            "big's value is copied to the caller",
        ],
        must_not_contain: &[
            "x's resource is moved to the caller",
            "big's resource is moved to the caller",
            "x's resource is copied to the caller",
            "big's resource is copied to the caller",
        ],
    },
    TooltipExpect {
        name: "return_shared_ref",
        // Returning a `&T` copies the reference (references are Copy),
        // never the String behind it. A returned String is still moved.
        // In the caller, `w` and `p` borrow `owned` (#171).
        must_contain: &[
            "a reference from s is returned to the caller; the data it points to is not copied",
            "a reference from r is returned to the caller; the data it points to is not copied",
            "t's resource is moved to the caller",
            "Immutable borrow from owned to w: first_word's signature gives the reference it returns the same lifetime as its argument",
            "Immutable borrow from owned to p: pick's signature gives the reference it returns the same lifetime as its argument",
        ],
        must_not_contain: &[
            ", through the reference first_word returned",
            ", through the reference pick returned",
            "s's resource is copied to the caller",
            "s's resource is moved to the caller",
            "r's resource is copied to the caller",
            "r's resource is moved to the caller",
            "Copy from first_word to w",
            "Copy from pick to p",
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
        // x is an i32: what's borrowed and returned is a value (#274).
        must_contain: &[
            "Immutable borrow from x to y",
            "Return immutably borrowed value from y to x",
            "x's value is immutably borrowed by y; x is read-only while the borrow is live",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from y to x",
            "x's resource is immutably borrowed by y; x is read-only while the borrow is live",
        ],
    },
    TooltipExpect {
        name: "basic_mutref",
        // `let y = &mut x; *y += 3;` — y's lifeline describes a
        // reference (not an owner), and the write through `*y`
        // modifies x's resource rather than acquiring one.
        must_contain: &[
            "Mutable borrow from x to y",
            "Return mutably borrowed value from y to x",
            "y holds a mutable reference; it can read and write the borrowed value",
            "the value y points to is modified through *y",
        ],
        must_not_contain: &[
            "the resource y points to is modified through *y",
            "y is the owner of the resource",
            "*y acquires ownership of a resource",
            "Return mutably borrowed resource from y to x",
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
            "i holds a mutable reference; it can read and write the borrowed value",
            "the value i points to is modified through *i",
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
            // `count` borrows `map` through or_insert's result (#171).
            "Mutable borrow from map to count: or_insert's signature gives the reference it returns the same lifetime as the value it was called on",
            "the value count points to is modified through *count",
            "text is bound to an immutable reference",
            "text holds an immutable reference; it can read the borrowed value",
        ],
        must_not_contain: &[
            ", through the reference or_insert returned",
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
            "the value x points to is modified through *x",
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
            "Copy from *w to *r",
            "*r is assigned a copy of y",
            "*r is assigned a copy of *w",
            "*r is assigned a new value",
            "r holds a mutable reference; it can read and write the borrowed value",
        ],
        must_not_contain: &[
            "(copied from",
            "(moved from",
            "the value r points to is modified through *r",
            "the resource r points to is modified through *r (copied from y)",
            "*r is initialized with a copy of y",
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
            "s holds an immutable reference; it can read the borrowed value",
            "t is initialized with a copy of s",
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
            "msg holds an immutable reference; it can read the borrowed value",
        ],
        must_not_contain: &[
            "msg acquired ownership of a resource (in all branches above)",
            "msg acquires ownership of a resource",
            "msg is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "cell_ref_return",
        // `r: &Cell<i32>` (returned by `pick(&c)`, so a shared borrow of
        // `c`, #171) is written through with `r.set(2)`. A shared
        // reference to a Cell/RefCell/Mutex can modify the referent, so
        // nothing may claim r can't modify it, that c is read-only
        // while r is live, or that r owns it.
        must_contain: &[
            "r holds an immutable reference; it can read the borrowed value",
            "Immutable borrow from c to r: pick's signature gives the reference it returns the same lifetime as its argument",
            // Cell<i32> needs no drop: a value, not a resource (#484).
            "c's value is immutably borrowed by r",
        ],
        must_not_contain: &[
            "c's value is immutably borrowed by r; c is read-only while the borrow is live",
            "r holds an immutable reference; it can read the borrowed resource",
            ", through the reference pick returned",
            "r holds an immutable reference; it can read but not modify the borrowed resource",
            "r is the owner of the resource",
            "c's resource is immutably borrowed by r; c is read-only while the borrow is live",
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
            // ra / rb own nothing: what's borrowed is what they point to
            // (#313). (rb2 = &mut rb also borrows rb itself; the dot
            // doesn't distinguish that, see the PR for #313.)
            "the resource ra points to is mutably borrowed by ra2 through ra; ra can't be used while the borrow is live",
            "ra2 no longer borrows the resource ra points to",
            "the resource rb points to is mutably borrowed by rb2 through rb; rb can't be used while the borrow is live",
            "c's resource is immutably borrowed by rc; c is read-only while the borrow is live",
            // A `&String` source was already read-only, so the
            // borrow adds no restriction worth stating.
            "the resource rc points to is immutably borrowed by rc2",
            "the resource rd points to is immutably borrowed by rd2",
        ],
        must_not_contain: &[
            "ra's resource is mutably borrowed by ra2; ra can't be used while the borrow is live",
            "*ra's resource is no longer mutably borrowed",
            "rc's resource is immutably borrowed by rc2",
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
            // `take(y)` into a generic `T`: y is moved, not reborrowed
            // (#200).
            "Move from y to take",
            // `show(x)` with x: `&String`: the reference is copied.
            "show reads from x",
            // x and y own nothing; the calls borrow what they point to
            // (#313). take consumes y, so nothing says y is usable again.
            "the resource x points to is immutably borrowed by show",
            "the resource y points to is mutably borrowed by push through y; y can't be used while the borrow is live",
            "push no longer borrows the resource y points to",
        ],
        must_not_contain: &[
            "x's resource is immutably borrowed by show",
            "y's resource is mutably borrowed by push; y can't be used while the borrow is live",
            "y's resource is no longer mutably borrowed",
            "first reads from/writes to y (mutable reborrow — y can't be used until first returns)",
            "take reads from/writes to y (mutable reborrow — y can't be used until take returns)",
            "take reads from/writes to y",
            "show reads from x (immutable reborrow — x is read-only until show returns)",
        ],
    },
    TooltipExpect {
        name: "interior_mut_lend",
        // `Cell::set`, `RefCell::borrow_mut`, `Mutex::lock` take
        // `&self` and mutate through it, so the lend dot must not
        // call the owner "read-only". A plain String still is.
        must_contain: &[
            "c's value is immutably borrowed by set",
            "r's resource is immutably borrowed by borrow_mut",
            "shared's resource is immutably borrowed by borrow",
            "m's resource is immutably borrowed by lock",
            "s's resource is immutably borrowed by len; s is read-only while the borrow is live",
        ],
        must_not_contain: &[
            "c's resource is immutably borrowed by set",
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
            "r is bound to a value",
            "r goes out of scope",
        ],
        must_not_contain: &[
            "r goes out of scope. Its resource is dropped.",
            "r acquires ownership of a resource",
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
        // The second `let s` declares a new variable, drawn as its own
        // column `s₂` (#173). Both Strings live, and are dropped, until
        // the end of `main`.
        must_contain: &[
            "Move from String::from to s",
            "Move from String::from to s₂",
            "s goes out of scope. Its resource is dropped.",
            "s₂ goes out of scope. Its resource is dropped.",
            "s₂, immutable. s₂ shadows s: it is a new variable named s, declared on line 8",
        ],
        must_not_contain: &[
            // Not "a different variable": the earlier one is still in scope.
            "s₂, immutable. s₂ is a different variable from s: the 2nd variable named s in this function",
        ],
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
        // owner regardless of branch. On one line, the branches used
        // to be drawn inline, with no merge (#117).
        must_contain: &[
            "Move from String::from to s",
            "s is live in a conditional expression ",
            "s acquired ownership of a resource (in all branches above)",
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
        // into r2. Reference counts aren't modelled, but the clone is
        // described as another Rc to the same value, not a copy (#373).
        must_contain: &[
            "Move from Rc::new to r",
            "Rc::clone makes another Rc to the same value (nothing is copied), now owned by r2",
            "r2 is another Rc to the same value (Rc::clone increases the reference count; nothing is copied)",
            "r2 goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped).",
            "Rc::clone reads from r",
        ],
        must_not_contain: &[
            "Move from Rc::clone to r2",
            "r2 acquires ownership of a resource",
            "r2 goes out of scope. Its resource is dropped.",
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
        // Single-arm if-let: destructure Move from opt to x, then a
        // borrow from x to show. The pattern moves the String out of
        // `opt` only when it matches, so `opt` is maybe-moved afterwards
        // (#257) and its column is drawn split at the `if let`, with the
        // implicit empty `else`: "opt is live in a conditional
        // expression" there is true, and the merge says opt was moved in
        // one branch (#517: this fixture used to claim there is no Branch
        // event).
        must_contain: &[
            "Move from Some to opt",
            "Move from opt to x",
            "show reads from x",
            "x goes out of scope. Its resource is dropped.",
            "opt was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource (including the String it holds, if any) is \
             dropped when opt goes out of scope.",
        ],
        must_not_contain: &[
            "may have been moved",
            "moved or dropped in every branch",
            "in branches where it was not",
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
        // Plain `if cond { body }` whose body moves `s`: `s` is moved
        // only if `cond` holds, so it's maybe-moved afterwards and the
        // drop at scope end is conditional. Drawn like an `if` with an
        // empty `else` (#257; it used to be an unconditional move).
        must_contain: &[
            "Move from s to consume",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "s goes out of scope. No resource is dropped.",
            "s was moved or dropped in every branch above",
            "may have been moved",
            "in branches where it was not",
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
            "n is initialized with a copy of parse",
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
            "n is initialized with a copy of map",
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
        // event. The guard `x > 0` reads `x` like any comparison
        // (#497). Both arms borrow `s`; merge is Unchanged.
        must_contain: &[
            "Copy from x to >",
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
            "y is initialized with a copy of x",
            "Copy from x to y",
        ],
        must_not_contain: &[
            // The const itself shouldn't get a column or events.
            "N is the owner",
            "N holds a copyable value",
            "N goes out of scope",
            // No Move from N — const reads aren't moves.
            "Move from N to x",
            // Old wording (maintainer chose "with a copy of").
            "initialized by copy from",
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
            "*p is assigned a new value",
        ],
        must_not_contain: &["*p is bound to a copyable value", "(copied from"],
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
            "u is assigned a copy of y; writing to a union never drops its old contents",
            // `u._b = 1.5;` — a literal has no source to name.
            "u is assigned a new value; writing to a union never drops its old contents",
            // `let x = unsafe { u._a };` — a copy out of u.
            "Copy from u to x",
            "x is initialized with a copy of u",
            "u's value is copied",
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
            "u is initialized with a copy of y",
            "Return immutably borrowed resource from r to *r",
        ],
    },
    TooltipExpect {
        name: "enum_param",
        // `fn area(s: Shape)` — the enum param is one owner column
        // that acquires from the caller; the match arms copy their
        // payloads out of it.
        must_contain: &[
            "s is bound to a value from the caller",
            "Partial copy from s to r",
            "Partial copy from s to w",
            "Move from c to area",
        ],
        must_not_contain: &[
            "s acquires ownership from the caller",
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
            "black is bound to a value",
            "Copy from black.0 to r",
            "r is initialized with a copy of black.0",
        ],
        must_not_contain: &[
            "black acquires ownership of a resource",
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
            "c goes out of scope",
        ],
        must_not_contain: &[
            "c goes out of scope. Its resource is dropped.",
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
            "Rc::clone makes another Rc to the same value (nothing is copied), now owned by a",
            "borrow_mut borrows value immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)", // #203
            "value goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped).",
            "a goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped).",
        ],
        must_not_contain: &[
            "value's resource is moved",
            "Move from Rc::clone to a",
            "value goes out of scope. Its resource is dropped.",
            "a goes out of scope. Its resource is dropped.",
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
            "map goes out of scope. Its resource is dropped (including every key it holds).",
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
            "=f, immutable",
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
            "c's value is copied",
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
        // is initialized with a copy of `count`, on the line where r's
        // borrow ends (#190).
        must_contain: &[
            "into_iter reads from r",
            "len reads from v (implicitly borrowed)",
            "v goes out of scope. Its resource is dropped.",
            "n is initialized with a copy of count",
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
            "=email goes out of scope. Its resource is dropped.",
            "=username goes out of scope. Its resource is dropped.",
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
            "q.x is initialized with a copy of p.x",
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
            "u.active's value is copied to the caller",
            "u.name goes out of scope. Its resource is dropped.",
            "u.email goes out of scope. No resource is dropped.",
            "u1.email goes out of scope. No resource is dropped.",
            "u2.email goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "=name goes out of scope. Its resource is dropped.",
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
            "n is initialized with a copy of opt",
            "s goes out of scope. Its resource is dropped.",
            "Partial copy from opt to n",
        ],
        must_not_contain: &[
            "s's resource is moved",
            "s goes out of scope. No resource is dropped.",
            // The arms' `n`s are never in scope together (#598).
            "n₂",
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
            "=t's resource is moved",
        ],
    },
    TooltipExpect {
        name: "match_if_let_guard",
        // `Some(n) if let Some(m) = n.checked_mul(2) =>`: `m` is
        // registered like an `if let` condition binding and scoped to
        // the arm. Its uses used to be skipped (unregistered local).
        must_contain: &[
            "Copy from checked_mul to m",
            "m is initialized with a copy of checked_mul",
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
            "words goes out of scope. Its resource is dropped (including every String it holds).",
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
            "Return immutably borrowed value from r to b",
            "Move from c to dbg!",
            "Copy from x to dbg!",
            "c goes out of scope",
            // `write!(f, …)` inside the Debug impl: f is borrowed
            // mutably by write_fmt, self.w is read.
            "write! reads from/writes to f through an implicit mutable reference, so f isn't moved",
            "write! reads from self through an implicit reference, so self isn't moved",
        ],
        must_not_contain: &[
            "c goes out of scope. No resource is dropped.",
            "Return immutably borrowed resource from r to b",
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
            "y is initialized with a copy of x",
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
            "v goes out of scope. Its resource is dropped (including every P it holds).",
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
            "Return immutably borrowed value from b to x",
        ],
        must_not_contain: &[
            "Return immutably borrowed value from c to x",
            "Return immutably borrowed value from a to x",
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
            "area is initialized with a copy of area",
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
            "n is initialized with a copy of len",
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
            "n is initialized with a copy of count",
            "k is initialized with a copy of sum",
            "w is initialized with a copy of count",
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
            "Return mutably borrowed value from r to x",
            "y's value is copied",
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
            "add_one is initialized with a copy of Anonymous resource",
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
            "*r is assigned a copy of y",
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
            "slot is assigned a new value; writing to a union never drops its old contents",
            "slot goes out of scope. A union's fields are never dropped automatically.",
        ],
        must_not_contain: &[
            "slot goes out of scope. Its resource is dropped.",
            "slot acquires ownership of a new resource; its previous resource is dropped",
            "slot is written to",
        ],
    },
    TooltipExpect {
        name: "union_whole_assign",
        must_contain: &[
            "u is assigned a copy of y; writing to a union never drops its old contents",
            "u acquires ownership of a resource; writing to a union never drops its old contents",
            "u goes out of scope. A union's fields are never dropped automatically.",
        ],
        must_not_contain: &[
            "u is initialized with a copy of y",
            "u acquires ownership of a new resource; its previous resource is dropped",
            "u goes out of scope. Its resource is dropped.",
            "u is written to",
            "(copied from",
            "(moved from",
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
            "a's reference is copied (not the value it points to)",
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
            "s's reference is copied (not the value it points to)",
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
            "b is assigned a new value; writing to a union never drops its old contents",
        ],
        must_not_contain: &[
            "p.x acquires ownership of a resource",
            "p.x is the owner of the resource",
            "p.y is initialized with a copy of z",
            "n is initialized with a copy of m",
            "b acquires ownership of a resource",
            "b is written to",
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
        // instead of carrying an empty tooltip. `self` is a reference
        // that the arm binding `s` reborrows through (#372), so its
        // borrow ends in each arm (#498), and its merge dot says that
        // rather than "isn't moved".
        must_contain: &[
            "opt isn't moved in any of the branches above",
            "self's borrow ended in each branch above",
            "opt goes out of scope. Its resource is dropped (including the String it holds, if any).",
        ],
        must_not_contain: &["="],
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
        // comparison on primitives (`x < y`) reads both operands the
        // same way (#497); a literal operand has no column.
        must_contain: &[
            "Copy from x to <",
            "Copy from y to <",
            "Copy from x to +",
            "Copy from y to +",
            "Copy from a.x to +",
            "Copy from a.y to +",
            "Copy from x to *",
            "x's value is copied",
            "a.y's value is copied",
            "Copy from a.x to d",
        ],
        must_not_contain: &[
            "Move from x to +",
            "x's resource is moved",
            "Copy from x to z",
            "Copy from x to bigger",
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
            "=len reads from s",
            "=push_str reads from/writes to s",
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
    // ─── Writes that overwrite a value state its drop ────────────────
    TooltipExpect {
        name: "deref_write_drops_old",
        // `*r = s` over a String drops the String that was there; so do
        // `*q = String::from("t")` and a Box's `*b = …`. The place has no
        // column, so the write's tooltip is where it's said, in the words
        // of a plain assignment (2026-11-05 decision). `*n = 5` (i32) is
        // "assigned a new value", `*rq = q2` (nothing to drop; #484: no
        // "resource" for a value with no drop glue) the same, and `*r += "x"`
        // (an operator call, nothing overwritten) keeps the update
        // wording, "modified through *r" (2026-11-06 decision), not an
        // operator reborrow of `r` (#260).
        must_contain: &[
            "*r acquires ownership of a new resource; its previous resource is dropped",
            "*q acquires ownership of a new resource; its previous resource is dropped",
            "*b acquires ownership of a new resource; its previous resource is dropped",
            "*n is assigned a new value",
            "*rq is assigned a new value",
            "the resource r points to is modified through *r",
        ],
        must_not_contain: &[
            "+= reads from/writes to r (mutable reborrow",
            "*rq acquires ownership of a resource",
            "its previous value is dropped",
            "(copied from",
            "(moved from",
            "the resource n points to is modified through *n",
            "the resource q points to is modified through *q",
            "the resource b points to is modified through *b",
            "the resource rq points to is modified through *rq",
        ],
    },
    TooltipExpect {
        name: "deref_assign_wording",
        // `*r = e` assigns to the place `*r` (2026-11-05 decision): a Copy
        // value is "assigned a copy of y" when the source is a named
        // place and "assigned a new value" otherwise, an owned value that
        // needs drop says its previous resource is dropped (a Box's too),
        // and a non-Copy value with nothing to drop (`Q` of an i32) is a
        // plain assignment too: it is a value, not a resource (#484). No
        // "(copied from" / "(moved from": the arrow says it. The in-place
        // update `*r += 1` / `*r += y` keeps "modified through".
        must_contain: &[
            "Copy from y to *r",
            "*r is assigned a copy of y",
            "*r is assigned a copy of *z",
            "*r is assigned a new value",
            "Move from t to *ra",
            "*ra acquires ownership of a new resource; its previous resource is dropped",
            "Move from u to *b",
            "*b acquires ownership of a new resource; its previous resource is dropped",
            "Move from q2 to *rq",
            "*rq is assigned a new value",
            "the value r points to is modified through *r",
            "Copy from *z to *r",
        ],
        must_not_contain: &[
            "(copied from",
            "(moved from",
            "the resource ra points to is modified through *ra",
            "the resource b points to is modified through *b",
            "the resource rq points to is modified through *rq",
            "*r acquires ownership",
            // Q has no drop glue: a value, not a resource (#484).
            "*rq acquires ownership of a resource",
            "*rq acquires ownership of a new resource",
        ],
    },
    TooltipExpect {
        name: "mem_drop_states_drop",
        // `drop(s)` / `std::mem::drop(v)` is where the value is dropped
        // (the arrow only says "Move from s to drop"). A Copy value has
        // nothing to drop, and a user function that happens to be named
        // `drop` isn't `std::mem::drop`.
        must_contain: &[
            "Move from s to drop",
            "s's resource is moved into drop, which drops it",
            "v's resource is moved into std::mem::drop, which drops it",
            "Copy from n to drop",
            "t's resource is moved",
        ],
        must_not_contain: &[
            "t's resource is moved into mine::drop, which drops it",
            "n's resource is moved into drop, which drops it",
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
            "b's value is copied",
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
            "Move from v into the closure passed to thread::spawn",
            "v's resource is moved into a closure passed to thread::spawn; it's dropped when that closure is dropped",
            "v goes out of scope. No resource is dropped.",
            "Copy from n into the closure passed to thread::spawn",
            "n's value is copied into a closure passed to thread::spawn",
            "Move from s into the closure passed to thread::spawn",
            "s goes out of scope. No resource is dropped.",
            "Move from p.name into the closure passed to thread::spawn",
            "p.name's resource is moved into a closure passed to thread::spawn; it's dropped when that closure is dropped",
            "Move from w into the closure passed to apply",
            "w's resource is moved into a closure passed to apply; it's dropped when that closure is dropped",
            "Move from t into the closure passed to map",
            "p.name goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "Move from v to thread::spawn",
            "Copy from n to thread::spawn",
            "Move from p.name to thread::spawn",
            "can still be used",
            "v goes out of scope. Its resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
            "p.name goes out of scope. Its resource is dropped.",
            "n's resource is moved",
            "Move from p to thread::spawn",
            "p.age's resource is moved",
        ],
    },
    TooltipExpect {
        name: "move_closure_copy_capture",
        // `move` copies Copy captures (m: i32, the field p.x, the
        // `&String` r) into the closure; only s is moved, so f drops
        // one captured resource and m / p.x / p.name stay usable.
        must_contain: &[
            "m's value is captured (copied) by closure f",
            "Closure capture (copy) from m to f",
            "p.x's value is captured (copied) by closure f",
            "Closure f captures: s (moved), m (copied), p.x (copied)",
            "f owns a closure which owns 1 resource via capture",
            "f goes out of scope. Its 1 captured resource is dropped.",
            "p.name goes out of scope. Its resource is dropped.",
            "r's reference is captured (copied) by closure g; the data it points to is not copied",
            "Closure g captures: r (copied)",
            "g owns a closure",
            "g goes out of scope",
        ],
        must_not_contain: &[
            "m's resource is captured (moved) by closure f",
            "Closure capture (move) from m to f",
            "p's resource is captured (moved) by closure f",
            "p.name's resource is moved",
            "f goes out of scope. Its 3 captured resources are dropped.",
            "f owns a closure which owns 3 resources via capture",
            "r's resource is captured (moved) by closure g",
            "g owns a closure which owns 1 resource via capture",
            "g goes out of scope. Its 1 captured resource is dropped.",
        ],
    },
    // ─── References copied out of a matched value (#304) ────────────
    TooltipExpect {
        name: "match_copy_ref_out_of_option",
        // Book ch. 8.1: `x` is a copy of the `&i32` stored in `third`.
        // It borrows an element of `v`, not `third`; nothing borrows
        // `third`. `third` holds a borrow of `v` (#336), so x's lender
        // is `v` (#391).
        must_contain: &[
            "x is initialized with a copy of third",
            "Partial copy from third to x",
            "Return immutably borrowed resource from x to v",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from x to *x",
            "Return immutably borrowed resource from x to third",
            "third's resource is no longer immutably borrowed",
        ],
    },
    TooltipExpect {
        name: "match_copy_ref_out_of_tuple",
        // `(x, y)` on a tuple of references copies each reference out;
        // they borrow `a` and `b`, not `pair`.
        must_contain: &[
            "Partial copy from pair to x",
            "Partial copy from pair to y",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from x to pair",
            "Return immutably borrowed resource from y to pair",
            "pair's resource is no longer immutably borrowed",
        ],
    },
    TooltipExpect {
        name: "iflet_copy_ref_out_of_option",
        // `if let Some(x) = head` / `if let Some(y) = v.get(1)`: the
        // bound reference borrows an element of `v`, not `head`, and
        // certainly not the function `get`.
        must_contain: &[
            "x is initialized with a copy of head",
            "y is initialized with a copy of get",
            "Return immutably borrowed resource from x to v",
            "Return immutably borrowed resource from y to v",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from x to *x",
            "Return immutably borrowed resource from y to *y",
            "Return immutably borrowed resource from x to head",
            "head's resource is no longer immutably borrowed",
            "Return immutably borrowed resource from y to get",
        ],
    },
    TooltipExpect {
        name: "match_ref_binding_borrows",
        // Bindings that *do* borrow the matched value keep it as their
        // lender: `Some(ref s)` on an owned Option, and `Some(t)` on
        // `&opt` (default binding modes).
        must_contain: &[
            "Return immutably borrowed resource from s to opt",
            "Partial immutable borrow from opt to t",
            "Return immutably borrowed resource from t to opt",
        ],
        must_not_contain: &[],
    },
    // ─── Copy values are "values" everywhere on their column (#274) ──
    TooltipExpect {
        name: "copy_value_wording",
        // m / k are i32 and p is a Copy struct: their columns say
        // "holds a value", and so do their copy, borrow and
        // borrow-return tooltips. The `&i32` r keeps reference wording.
        must_contain: &[
            "m's value is copied",
            "m's value is immutably borrowed by r; m is read-only while the borrow is live",
            "m's value is being shared by one or more variables",
            "m's value is no longer immutably borrowed",
            "r immutably borrows a value",
            "Return immutably borrowed value from r to m",
            "r's reference is copied (not the value it points to)",
            "r holds an immutable reference; it can read the borrowed value",
            "k's value is mutably borrowed by w; k can't be used while the borrow is live",
            "k's value is no longer mutably borrowed",
            "w mutably borrows a value",
            "Return mutably borrowed value from w to k",
            "k's value is captured (immutably borrowed) by closure f",
            "p's value is copied",
            "p.x's value is copied",
        ],
        must_not_contain: &[
            "m's resource is copied",
            "m's resource is immutably borrowed by r; m is read-only while the borrow is live",
            "m's resource is being shared by one or more variables",
            "m's resource is no longer immutably borrowed",
            "r immutably borrows a resource",
            "Return immutably borrowed resource from r to m",
            "r's resource is being shared by one or more variables",
            "r2's resource is being shared by one or more variables",
            "k's resource is mutably borrowed by w; k can't be used while the borrow is live",
            "k's resource is no longer mutably borrowed",
            "w mutably borrows a resource",
            "Return mutably borrowed resource from w to k",
            "k's resource is captured (immutably borrowed) by closure f",
            "p's resource is copied",
            "p.x's resource is copied",
        ],
    },
    // ─── Explicit `ref` bindings on an owned value (#338) ───────────
    TooltipExpect {
        name: "ref_binding_borrows_owned",
        // `Some(ref s)` on an `Option<String>` binds `s: &String`, a
        // shared borrow of the String inside `opt`: nothing is copied
        // and `opt` keeps its value. `Some(ref mut n)` is a mutable
        // borrow, not a move: `name` still owns (and drops) its
        // String. A Copy payload (`Some(ref k)` on `Option<i32>`) is
        // borrowed too.
        must_contain: &[
            "Partial immutable borrow from opt to s",
            "Return immutably borrowed resource from s to opt",
            "Mutable borrow from name to n",
            "Return mutably borrowed resource from n to name",
            "name goes out of scope. Its resource is dropped (including the String it holds, if any).",
            "Partial immutable borrow from num to k",
        ],
        must_not_contain: &[
            "Partial copy from opt to s",
            "s is initialized with a copy of opt",
            "opt's resource is copied",
            "Move from name to n",
            "name's resource is moved",
            "name goes out of scope. No resource is dropped.",
            "Partial copy from num to k",
        ],
    },
    // ─── `if` without `else` (#257) ─────────────────────────────────
    TooltipExpect {
        name: "if_no_else_partial_move",
        // `let n = p.name;` inside an `if` with no `else`: `p.name` is
        // moved only when `c` holds.
        must_contain: &[
            "Move from p.name to n",
            "p.name was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when p.name goes out of scope.",
            "p.name goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
            "p.nick goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "p.name goes out of scope. No resource is dropped.",
            "p.name was moved or dropped in every branch above",
        ],
    },
    TooltipExpect {
        name: "if_let_no_else_move",
        // `if let Some(x) = opt { take(x); take(s); }`: the pattern
        // moves out of `opt` and the body moves `s`, both only when
        // `opt` is `Some`.
        must_contain: &[
            "Move from opt to x",
            "Move from s to take",
            "opt was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource (including the String it holds, if any) is \
             dropped when opt goes out of scope.",
            "opt goes out of scope. Its resource (including the String it holds, if any) is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
            "x goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "opt goes out of scope. No resource is dropped.",
            "s goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "if_no_else_read_only",
        // Only reads `s`: nothing changes who owns it, so the body
        // stays inline (no branch columns, no merge dot).
        must_contain: &[
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            // (The plugin emits this one with a trailing space.)
            "s is live in a conditional expression ",
            "s isn't moved in any of the branches above",
        ],
    },
    TooltipExpect {
        name: "if_else_empty_same_line",
        // `} else {}`: the empty `else` column is one row tall and
        // used to count as having moved `s`.
        must_contain: &[
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "s was moved or dropped in every branch above",
            "s goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "if_no_else_nested_move",
        // Both arms of the inner `if` move `s` (its merge says so),
        // but the outer `if` has no `else`: maybe-moved after it.
        must_contain: &[
            "s was moved or dropped in every branch above",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "s goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "ref_copy_referent_wording",
        // y, z, n, k point to i32s, which their lenders' columns call
        // "a value"; s and q point to a String, still "the resource".
        must_contain: &[
            "y holds a mutable reference; it can read and write the borrowed value",
            "the value y points to is modified through *y",
            "z holds an immutable reference; it can read the borrowed value",
            "n holds a mutable reference; it can read and write the borrowed value",
            "k holds an immutable reference; it can read the borrowed value",
            // `*n += *k` is an operator call (#260): both are copied
            // into `+=`, not "assigned" from one to the other.
            "Copy from *k to +=",
            "the value n points to is modified through *n",
            "s holds a mutable reference; it can read and write the borrowed resource",
            "*s acquires ownership of a new resource; its previous resource is dropped",
            "q holds a mutable reference; it can read and write the borrowed resource",
        ],
        must_not_contain: &[
            "y holds a mutable reference; it can read and write the borrowed resource",
            "the resource y points to is modified through *y",
            "z holds an immutable reference; it can read the borrowed resource",
            "n holds a mutable reference; it can read and write the borrowed resource",
            "k holds an immutable reference; it can read the borrowed resource",
            "the value n points to is modified through *n (copied from *k)",
            "Copy from *k to *n",
            "(copied from",
            "the resource s points to is modified through *s",
            "(moved from",
        ],
    },
    TooltipExpect {
        name: "reborrow_through_ref_wording",
        // r / r2 / p own nothing: a borrow from them is of what they
        // point to, through them. The end of one doesn't end r's own
        // borrow of s, so it names the borrower whose borrow ended.
        must_contain: &[
            "the resource r points to is mutably borrowed by r2 through r; r can't be used while the borrow is live",
            "r2 no longer borrows the resource r points to",
            "the resource r2 points to is mutably borrowed by push through r2; r2 can't be used while the borrow is live",
            "push no longer borrows the resource r2 points to",
            "the resource r points to is mutably borrowed by push through r; r can't be used while the borrow is live",
            "the resource r points to is immutably borrowed by r3 through r; r is read-only while the borrow is live",
            "r holds a mutable reference, but the resource it points to is currently shared through it, so r can only read it",
            "r3 no longer borrows the resource r points to",
            "the value p points to is mutably borrowed by q through p; p can't be used while the borrow is live",
            "q mutably borrows a value",
            "q no longer borrows the value p points to",
            "Return mutably borrowed value from q to *p",
            "the value p points to is mutably borrowed by add through p; p can't be used while the borrow is live",
            "add no longer borrows the value p points to",
        ],
        must_not_contain: &[
            "r's resource is mutably borrowed by r2; r can't be used while the borrow is live",
            "*r's resource is no longer mutably borrowed",
            "r's resource is mutably borrowed by push; r can't be used while the borrow is live",
            "r's resource is no longer mutably borrowed",
            "r2's resource is mutably borrowed by push; r2 can't be used while the borrow is live",
            "r's resource is being shared by one or more variables",
            "r's resource is immutably borrowed by r3; r is read-only while the borrow is live",
            "*r's resource is no longer immutably borrowed",
            "p's resource is mutably borrowed by q; p can't be used while the borrow is live",
            "p's resource is mutably borrowed by add; p can't be used while the borrow is live",
            "Return mutably borrowed resource from q to *p",
        ],
    },
    TooltipExpect {
        name: "comparison_operand_borrows",
        // `a == b` on Strings is `PartialEq::eq(&a, &b)`, `a < b` is
        // `PartialOrd::lt(&a, &b)`: rustc inserts a `&` on each
        // operand, so both are borrowed for the call and stay usable
        // (they're printed at the end). Same for a derived PartialEq
        // struct and for fields. `i < n` on integers is built in: it
        // copies both operands into `<` and borrows nothing (#497).
        must_contain: &[
            "Copy from i to <",
            "Copy from n to <",
            "== reads from a (implicitly borrowed)",
            "== reads from b (implicitly borrowed)",
            "< reads from a (implicitly borrowed)",
            "< reads from b (implicitly borrowed)",
            "!= reads from a (implicitly borrowed)",
            "== reads from p (implicitly borrowed)",
            "== reads from q (implicitly borrowed)",
            "== reads from p.name (implicitly borrowed)",
            "== reads from q.name (implicitly borrowed)",
            "a's resource is immutably borrowed by ==; a is read-only while the borrow is live",
            "a goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "Move from a to ==",
            "a's resource is moved",
            "b's resource is moved",
            "Copy from a to ==",
            "< reads from i (implicitly borrowed)",
            "< reads from i",
        ],
    },
    TooltipExpect {
        name: "comparison_ref_operands",
        // `x == y` with `x: &String` borrows the reference, reading
        // through it; `*x < *y` reborrows `*x`. Neither borrows an
        // owner Rust picked for you, so no "implicitly" note. In
        // `r == &b` the `&b` is written.
        must_contain: &[
            "== reads from x",
            "== reads from y",
            "< reads from x",
            "< reads from y",
            "== reads from r",
            "== reads from b",
        ],
        must_not_contain: &[
            "== reads from x (implicitly borrowed)",
            "< reads from x (implicitly borrowed)",
            "== reads from r (implicitly borrowed)",
            "== reads from b (implicitly borrowed)",
        ],
    },
    TooltipExpect {
        name: "operator_name_escaped",
        // `&a & &b` is `BitAnd::bitand(&a, &b)`. The operator's name
        // must reach the tooltip as `&` (escaped twice in the SVG).
        must_contain: &[
            "& reads from a",
            "& reads from b",
        ],
        must_not_contain: &[
            "&amp; reads from a",
        ],
    },
    TooltipExpect {
        name: "if_one_line_move",
        // `if c { take(s); }` and `if c { take(t); } else { … }`, each
        // on one line (#333): the moves are conditional. They used to
        // be drawn inline, as unconditional moves.
        must_contain: &[
            "s is live in a conditional expression ",
            "s's resource is moved",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
            "t is live in a conditional expression ",
            "t's resource is moved",
            "t was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when t goes out of scope.",
            "t goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "s goes out of scope. No resource is dropped.",
            "t goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "match_one_line",
        // Every arm of a one-line `match` binds `s` (#117). The first
        // arm used to get all three binds and the others none, so the
        // merge said `s` was moved in some branch.
        must_contain: &[
            "s is live in a conditional expression ",
            "s acquired ownership of a resource (in all branches above)",
        ],
        must_not_contain: &[
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
        ],
    },
    TooltipExpect {
        name: "if_else_next_line",
        // Used to panic (#354).
        must_contain: &[
            "s is live in a conditional expression ",
            "s acquired ownership of a resource (in all branches above)",
        ],
        must_not_contain: &[],
    },
    // ─── Patterns in fn parameters (#331) ───────────────────────────
    TooltipExpect {
        name: "param_patterns",
        // Each name a parameter pattern binds is a parameter of its
        // own. `a` owns the caller's String and drops it at the end of
        // `first`; `s` in `peek((s, n): &(String, i32))` is bound by
        // reference (default binding mode), so it borrows from the
        // caller; `name` owns the String moved out of the caller's `P`.
        // On main none of these names had a column, so their uses drew
        // nothing.
        must_contain: &[
            "a acquires ownership from the caller",
            "len reads from a (implicitly borrowed)",
            "a goes out of scope. Its resource is dropped.",
            "b goes out of scope",
            "s is an immutable borrow from the caller",
            "len reads from s",
            // `x` is an i32 copied out of the caller's pair (#505).
            "x is a copy of the caller's value",
            "y goes out of scope",
            "name acquires ownership from the caller",
            "len reads from name (implicitly borrowed)",
            "name goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s acquires ownership from the caller",
            "s goes out of scope. Its resource is dropped.",
            "x acquires ownership from the caller",
        ],
    },
    TooltipExpect {
        name: "ref_static_lender_branches",
        // Every value-yielding arm is a string literal (a diverging arm
        // doesn't count): named. A literal mixed with a call result or
        // with a const's reference stays generic.
        must_contain: &[
            "a's borrow of the string literal ends (string literals live for the whole program)",
            "b's borrow of the string literal ends (string literals live for the whole program)",
            "d's borrow of the string literal ends (string literals live for the whole program)",
            "m's immutable borrow ends (what it borrows from isn't shown)",
            "n's immutable borrow ends (what it borrows from isn't shown)",
        ],
        must_not_contain: &[
            "a's immutable borrow ends (what it borrows from isn't shown)",
            "b's immutable borrow ends (what it borrows from isn't shown)",
            "d's immutable borrow ends (what it borrows from isn't shown)",
            "m's borrow of the string literal",
            "n's borrow of the string literal",
        ],
    },
    TooltipExpect {
        name: "ref_static_lender_const",
        // `&CONST` is promoted to a 'static only for a value with no
        // destructor and no interior mutability.
        must_contain: &[
            "p's borrow of the constant ANSWER ends (Rust stores the borrowed value for the whole program)",
            "q's borrow of the constant ORIGIN ends (Rust stores the borrowed value for the whole program)",
            "v's immutable borrow ends (what it borrows from isn't shown)",
        ],
        must_not_contain: &[
            "p's immutable borrow ends (what it borrows from isn't shown)",
            "q's immutable borrow ends (what it borrows from isn't shown)",
            "v's borrow of the constant",
        ],
    },
    // ─── Two refs to one variable on one line (#233) ───────────────
    // Each ref gets its own MIR loan (matched by the span of its `&x`).
    // `a` dies first while `b` still borrows `x`, so only `b` returns
    // the borrow. Before the fix both got `a`'s end line, and both
    // "returned" the borrow there while `b` was still in use.
    TooltipExpect {
        name: "two_refs_same_line",
        must_contain: &[
            "a's immutable borrow ends",
            "b's immutable borrow ends",
            "Return immutably borrowed resource from b to x",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from a to x",
        ],
    },
    TooltipExpect {
        name: "two_refs_tuple",
        must_contain: &[
            "a's immutable borrow ends",
            "b's immutable borrow ends",
            "Return immutably borrowed resource from b to x",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from a to x",
        ],
    },
    TooltipExpect {
        name: "closure_disjoint_field_capture",
        // Rust 2021 closures capture disjoint fields: `move ||
        // p.name.len()` moves only `p.name` into f; `p.nick` stays with
        // p, is printed afterwards, and is dropped at the end of main.
        // `|| q.nick.len()` borrows only `q.nick`, so `q.name` can be
        // moved out while g is alive.
        must_contain: &[
            "p.name's resource is captured (moved) by closure f",
            "Closure f captures: p.name (moved)",
            "Closure capture (move) from p.name to f",
            "p.nick goes out of scope. Its resource is dropped.",
            "q.nick's resource is captured (immutably borrowed) by closure g",
            "Return immutably borrowed resource from g to q.nick",
            "Move from q.name to r",
        ],
        must_not_contain: &[
            "p's resource is captured (moved) by closure f",
            "Closure f captures: p (moved)",
            "p.nick's resource is moved",
            "p.nick goes out of scope. No resource is dropped.",
            "q's resource is captured (immutably borrowed) by closure g",
            "Closure g captures: q (immutably borrowed)",
        ],
    },
    TooltipExpect {
        name: "closure_tuple_element_capture",
        // Rust 2021 closures capture disjoint tuple elements: `move ||
        // t.0.len()` moves only `t.0` into f; `t.1` stays with t, is
        // printed afterwards, and is dropped at the end of main.
        // `|| u.1.len()` borrows only `u.1`, so `u.0` can be moved out
        // while g is alive.
        must_contain: &[
            "t.0's resource is captured (moved) by closure f",
            "Closure f captures: t.0 (moved)",
            "Closure capture (move) from t.0 to f",
            "t.1 goes out of scope. Its resource is dropped.",
            "u.1's resource is captured (immutably borrowed) by closure g",
            "Return immutably borrowed resource from g to u.1",
            "Move from u.0 to r",
        ],
        must_not_contain: &[
            "t's resource is captured (moved) by closure f",
            "Closure f captures: t (moved)",
            "t.1's resource is moved",
            "t.1 goes out of scope. No resource is dropped.",
            "u's resource is captured (immutably borrowed) by closure g",
            "Closure g captures: u (immutably borrowed)",
        ],
    },
    TooltipExpect {
        name: "closure_whole_struct_capture",
        // `move || show(p)` uses all of p, so all of p (and both
        // fields) moves into f.
        must_contain: &[
            "p's resource is captured (moved) by closure f",
            "Closure f captures: p (moved)",
            "p.nick's resource is moved",
            "p.nick goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "p.nick goes out of scope. Its resource is dropped.",
        ],
    },
    // ─── Copy variable bound in every branch (#316) ─────────────────
    TooltipExpect {
        name: "merge_bound_copy",
        // `let n = match x { 0 => 1, _ => 2 };` and a `Copy` struct
        // bound by an if/else: no resource, no ownership. The String
        // `s` still acquires ownership.
        must_contain: &[
            "n is bound to a value (in all branches above)",
            "p is bound to a value (in all branches above)",
            "p.x is bound to a value (in all branches above)",
            "s acquired ownership of a resource (in all branches above)",
        ],
        must_not_contain: &[
            "n acquired ownership of a resource (in all branches above)",
            "p acquired ownership of a resource (in all branches above)",
            "p.x acquired ownership of a resource (in all branches above)",
            "p.y acquired ownership of a resource (in all branches above)",
        ],
    },
    // ─── Method chains (#132) ────────────────────────────────────────
    TooltipExpect {
        name: "method_chain_temporaries",
        // Only the first call in each chain acts on the variable. The
        // later calls (`push`, `map`, `collect`, `into_bytes`) act on
        // temporaries and get no event on `b` / `v` / `s`: in
        // particular `into_bytes` consumes the clone, not `s`.
        must_contain: &[
            "get_mut reads from/writes to b (implicitly mutably borrowed)",
            "iter reads from v (implicitly borrowed)",
            "clone reads from s (implicitly borrowed)",
            "Move from into_bytes to bytes",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "push reads from/writes to b",
            "map reads from v",
            "collect reads from v",
            "into_bytes reads from s",
            "Move from s to into_bytes",
            "s's resource is moved",
            "s goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "method_chain_args",
        // The receiver of `push` / `unwrap_or` / `contains` is a
        // temporary, but their arguments are moved / borrowed. Before,
        // `n` and `default` were said to drop a String they no longer
        // own.
        must_contain: &[
            "last_mut reads from/writes to names (implicitly mutably borrowed)",
            "Move from n to push",
            "n's resource is moved",
            "n goes out of scope. No resource is dropped.",
            "take reads from/writes to opt (implicitly mutably borrowed)",
            "Move from default to unwrap_or",
            "default goes out of scope. No resource is dropped.",
            "opt goes out of scope. Its resource is dropped (including the String it holds, if any).",
            "trim reads from text (implicitly borrowed)",
            "contains reads from needle",
        ],
        must_not_contain: &[
            "n goes out of scope. Its resource is dropped.",
            "default goes out of scope. Its resource is dropped.",
            "push reads from/writes to names",
            "opt's resource is moved",
            "Move from opt to unwrap_or",
            "contains reads from text",
        ],
    },
    TooltipExpect {
        name: "method_arg_unnamed_receiver",
        // #250: `self.items.push(item)` moves `item` even though
        // `self.items` has no column.
        must_contain: &[
            "Move from item to push",
            "item's resource is moved",
            "item goes out of scope. No resource is dropped.",
            "push_str reads from t",
        ],
        must_not_contain: &[
            "item goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "method_call_in_unnamed_receiver_arg",
        // #249: used to panic. `s.clone()` / `s.as_str()` in the
        // arguments only read `s`.
        must_contain: &[
            "clone reads from s",
            "as_str reads from s (implicitly borrowed)",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s's resource is moved",
        ],
    },
    // ─── Arguments of a closure call go to the call (#201, #210, #258, #284) ─
    // A call `f(args)` through a local variable used to send each
    // argument into `f`'s own column: the renderer read that as a
    // capture, an initialization, or (for `&x` / `&mut x` written at
    // the call) an arrow it couldn't place and crashed on.
    TooltipExpect {
        name: "closure_call_mutref_arg",
        // #210: used to crash in render_arrow.
        must_contain: &[
            "push reads from/writes to s",
            "s's resource is mutably borrowed by push; s can't be used while the borrow is live",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "captured (mutably borrowed) by closure push",
        ],
    },
    TooltipExpect {
        name: "closure_call_mutref_var",
        // #201: `ru` is reborrowed for the call, once; `g` captures
        // nothing.
        must_contain: &[
            "g reads from/writes to ru (mutable reborrow — ru can't be used until g returns)",
        ],
        must_not_contain: &[
            "ru's resource is captured (mutably borrowed) by closure g",
            "Pass by mutable reference from ru to g",
            "g's mutable borrow ends",
        ],
    },
    TooltipExpect {
        name: "closure_call_shared_ref_arg",
        // Used to crash like #210.
        must_contain: &[
            "print reads from s",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "captured (immutably borrowed) by closure print",
        ],
    },
    TooltipExpect {
        name: "closure_call_copy_arg",
        // #258: `5` is copied into the parameter `x`.
        must_contain: &[
            "Closure add_one is bound",
        ],
        must_not_contain: &[
            "add_one is initialized with a copy of Anonymous resource",
        ],
    },
    TooltipExpect {
        name: "closure_call_move_arg",
        // #284: `s` is moved into the call, not captured; `consume`
        // owns nothing.
        must_contain: &[
            "Move from s to consume",
            "s goes out of scope. No resource is dropped.",
            "consume owns a closure",
            "consume goes out of scope",
        ],
        must_not_contain: &[
            "s's resource is captured (moved) by closure consume",
            "Closure capture (move) from s to consume",
            "consume owns a closure which owns 1 resource via capture",
            "consume goes out of scope. Its 1 captured resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "generic_fn_param_call",
        // `f(1)` inside `apply` calls the closure parameter; it
        // doesn't initialize `f` (#179's "f is initialized by copy
        // from Anonymous resource").
        must_contain: &[
            "f acquires ownership from the caller",
        ],
        must_not_contain: &[
            "f is initialized with a copy of Anonymous resource",
        ],
    },
    TooltipExpect {
        name: "fn_item_local",
        must_contain: &[
            "f is bound to a copyable value",
            "f goes out of scope",
            "a's value is copied",
            "Copy from a to f",
        ],
        must_not_contain: &[
            "f is initialized with a copy of a",
            "f is initialized with a copy of helper",
        ],
    },
    TooltipExpect {
        name: "fn_pointer_local_call",
        must_contain: &[
            "f is bound to a copyable value",
            "s's resource is moved",
            "Move from s to f",
            "t acquires ownership of a resource",
            "Move from f to t",
        ],
        must_not_contain: &[
            "f's value is moved",
            "f's value is copied",
            "f acquires ownership",
        ],
    },
    TooltipExpect {
        name: "closure_call_result",
        must_contain: &[
            "Closure add_one is bound",
            "y is initialized with a copy of add_one",
            "Copy from add_one to y",
        ],
        must_not_contain: &[
            "add_one's value is copied",
        ],
    },
    TooltipExpect {
        name: "operator_result_struct_fields",
        // `let c = a + b;` (impl Add) and `let flipped = -wave;` (impl
        // Neg) bind a struct from an operator's result. Its fields hold
        // their parts of that value: `c.label` owns the concatenated
        // String and drops it at the end of main (#314).
        must_contain: &[
            "c.label acquires ownership of a resource",
            "c.label goes out of scope. Its resource is dropped.",
            "flipped.samples acquires ownership of a resource",
            "flipped.samples goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "c.label goes out of scope. No resource is dropped.",
            "flipped.samples goes out of scope. No resource is dropped.",
        ],
    },
    // ─── Reassigning a reference variable (#414) ────────────────────
    TooltipExpect {
        name: "ref_reassign_wording",
        // r, m and t already hold a reference when they are assigned
        // another one; `u = &a;` is u's first value (a borrow).
        must_contain: &[
            "r is assigned a copy of s's reference",
            "s's reference is copied (not the value it points to)",
            // `m = n;` assigns to an `&mut String` place, so rustc
            // reborrows `&mut *n` instead of moving n (#289); n is
            // usable again after m's last use.
            "Mutable reborrow from n to m",
            // `t = first(&y, &x)` reassigns t with a reference that
            // borrows both arguments (#171), so it is drawn as borrows.
            "Immutable borrow from y to t: first's signature gives the reference it returns the same lifetime as both arguments, so it may point into either, and both stay borrowed while t is in use",
            "Immutable borrow from a to u",
        ],
        must_not_contain: &[
            "r is initialized with a copy of s",
            "m is bound to a mutable reference",
            "n's mutable reference is moved",
            "t is initialized with a copy of first",
        ],
    },
    // ─── Element / slice borrows through a reference (#407) ─────────
    TooltipExpect {
        name: "index_reborrow_through_ref",
        // `&mut rv[0]`, `add(&mut rv[0])`, `rv[0].push(..)` and
        // `&r[1..]` borrow through the reference; `&mut w[0]` borrows
        // the owner w.
        must_contain: &[
            "Mutable reborrow from rv to e",
            "add reads from/writes to rv (mutable reborrow — rv can't be used until add returns)",
            "push reads from/writes to rv (mutable reborrow — rv can't be used until push returns)",
            "Mutable borrow from w to f",
            "Immutable reborrow from r to sl",
        ],
        must_not_contain: &[
            "Mutable borrow from rv to e",
            "=add reads from/writes to rv",
            "=push reads from/writes to rv",
            "Mutable reborrow from w to f",
            "Immutable borrow from r to sl",
            "(implicitly mutably borrowed) (mutable reborrow",
            "push reads from/writes to rv (implicitly mutably borrowed)",
        ],
    },
    // ─── `ref` bindings in by-value parameters (#377) ──────────────
    TooltipExpect {
        name: "param_ref_binding_own",
        // `fn whole(ref s: String)`: the caller's String is moved into
        // `whole`, which owns it; `s: &String` borrows that, so it is
        // not "a borrow from the caller" (rustc: "cannot return value
        // referencing function parameter"). Same for a `ref` inside a
        // tuple or struct parameter pattern (part of the value) and for
        // `ref mut`. Bindings reached through a reference, written
        // (`&(ref x, y)`) or implicit (`(w, z): &(String, i32)`), do
        // borrow the caller's data.
        must_contain: &[
            // The borrowed value has no column, so the tooltip states
            // its drop (with the parameter, when the function returns;
            // checked with a Drop-printing program). An i32 has none.
            "s is an immutable borrow of the value passed to whole, which whole owns; that value is dropped when whole returns",
            "a is an immutable borrow of part of the value passed to part, which part owns; that part is dropped when part returns",
            "v is a mutable borrow of the value passed to grow, which grow owns; that value is dropped when grow returns",
            "name is an immutable borrow of part of the value passed to field, which field owns; that part is dropped when field returns",
            "n is an immutable borrow of the value passed to small, which small owns",
            "x is an immutable borrow from the caller",
            "w is an immutable borrow from the caller",
            "Move from s1 to whole",
            "Move from t to part",
        ],
        must_not_contain: &[
            "=s is an immutable borrow of the value passed to whole, which whole owns",
            "=a is an immutable borrow of part of the value passed to part, which part owns",
            "=v is a mutable borrow of the value passed to grow, which grow owns",
            "=name is an immutable borrow of part of the value passed to field, which field owns",
            "n is an immutable borrow of the value passed to small, which small owns; that value is dropped when small returns",
            "s is an immutable borrow from the caller",
            "a is an immutable borrow from the caller",
            "v is a mutable borrow from the caller",
            "name is an immutable borrow from the caller",
            "x is an immutable borrow of",
            "w is an immutable borrow of",
        ],
    },
    TooltipExpect {
        name: "inherent_method_one_field",
        // `h.get()` with `fn get(&self) -> i32 { self.n }` on a
        // one-field struct used to panic (#74, #136). The call borrows
        // `h` for its duration and copies the i32 result out.
        must_contain: &[
            // Holder { n: i32 } isn't Copy but has nothing to drop: a
            // value, not a resource (#484).
            "h's value is immutably borrowed by get; h is read-only while the borrow is live",
            "get reads from h (implicitly borrowed)",
            "Copy from get to v",
            "h goes out of scope",
        ],
        must_not_contain: &[
            "h's resource is moved",
            "h's resource is immutably borrowed by get",
            "h goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "inherent_method_receivers",
        // One method per receiver kind (#136): `self` consumes `w`,
        // `self: &mut Self` / `self: &Self` borrow `c` (the second one
        // called as `Counter::get(&c)`), a `&'a str` result borrows
        // from `p`, and `&self` methods on a tuple struct and a
        // generic struct.
        must_contain: &[
            "Move from w to into_inner",
            "Move from into_inner to inner",
            "w goes out of scope. No resource is dropped.",
            "w.s goes out of scope. No resource is dropped.",
            "inner goes out of scope. Its resource is dropped.",
            "incr reads from/writes to c (implicitly mutably borrowed)",
            "Counter::get reads from c",
            "Immutable borrow from p to nm: name's signature gives the reference it returns the same lifetime as the value it was called on",
            "Return immutably borrowed resource from nm to p",
            "first reads from pair (implicitly borrowed)",
            "pair.0 goes out of scope. Its resource is dropped.",
            "x reads from pt (implicitly borrowed)",
        ],
        must_not_contain: &[
            "w goes out of scope. Its resource is dropped.",
            "c's resource is moved",
            "p's resource is moved",
        ],
    },
    TooltipExpect {
        name: "inherent_method_unit_struct",
        // `fn make() -> Marker { Marker }` returns a unit struct built
        // by its constructor; it used to panic in the renderer
        // (#136). The constructor value goes to the caller undrawn.
        must_contain: &[
            "Move from Marker to m",
            "Move from Marker::make to n",
            "name reads from m (implicitly borrowed)",
            "name reads from n (implicitly borrowed)",
        ],
        must_not_contain: &[
            "m's resource is moved",
        ],
    },
    TooltipExpect {
        name: "nested_struct_moved_into_fn",
        // Moving a struct into a consuming method or `drop` moves its
        // nested struct's fields along (#319): `o.inner.v` and
        // `q.inner.v` have nothing left to drop at the end of main.
        must_contain: &[
            "o.inner.v's resource is moved",
            "o.inner.v goes out of scope. No resource is dropped.",
            "q.inner.v's resource is moved",
            "q.inner.v goes out of scope. No resource is dropped.",
            "i.v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "o.inner.v goes out of scope. Its resource is dropped.",
            "q.inner.v goes out of scope. Its resource is dropped.",
        ],
    },
    // ─── `ref` / `ref mut` bindings in `let` (#380) ─────────────────
    TooltipExpect {
        name: "let_ref_binding_borrows",
        // `let ref r = s;` is `let r = &s;` (rustc: "cannot move out
        // of `s` because it is borrowed" if s is moved while r is
        // live), `let ref mut m = w;` is `let m = &mut w;`, and `let P
        // { ref name, .. } = p;` borrows `p.name`. Against a value
        // (`String::from("tmp")`) the binding borrows a temporary. In
        // `let (ref a, b) = (u, 1);` the tuple literal is a temporary
        // that `u` is moved into (rustc: "borrow of moved value: `u`"),
        // and `a` borrows the temporary, not `u`. (`a`'s borrow end
        // still reads "from a to *a"; that wording is #178.)
        must_contain: &[
            "Immutable borrow from s to r",
            "s's resource is immutably borrowed by r; s is read-only while the borrow is live",
            "Mutable borrow from w to m",
            "w goes out of scope. Its resource is dropped.",
            "Immutable borrow from p.name to name",
            "Return immutably borrowed resource from name to p.name",
            "Immutable borrow from x to k",
            "t immutably borrows a resource",
            "u's resource is moved",
            "u goes out of scope. No resource is dropped.",
            "a immutably borrows a resource",
        ],
        must_not_contain: &[
            "Copy from s to r",
            "r is initialized with a copy of s",
            "s's resource is copied",
            "Move from w to m",
            "w's resource is moved",
            "w goes out of scope. No resource is dropped.",
            "Copy from p to name",
            "p's resource is copied",
            "=Return immutably borrowed resource from name to p",
            "Copy from x to k",
            "Copy from String::from to t",
            "Copy from u to a",
            "u's resource is copied",
            "Return immutably borrowed resource from a to u",
            "u goes out of scope. Its resource is dropped.",
        ],
    },
    // `let ref d = *r;` is `let d = &*r;`, a reborrow of `s` through
    // `r`. It used to panic (#422).
    TooltipExpect {
        name: "let_ref_deref_reborrow",
        must_contain: &[
            "Immutable reborrow from *r to d",
            "Return immutably borrowed resource from r to s",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "match_diverging_arm",
        // #296: `_ => unreachable!()` never produces a value, so it
        // doesn't bind `s` (was "s acquires ownership of a resource"
        // on that arm's line), and the merge speaks of the arms that
        // reach it, naming the one that doesn't and how it left (#350).
        // `t` is moved in the only arm that reaches the
        // merge; the `panic!` arm never gets there, so this isn't the
        // "moved in some branches, dropped in the others" case.
        must_contain: &[
            "s acquired ownership of a resource in every branch that reaches this point (the _ arm panicked)",
            "t was moved or dropped in every branch above that reaches this point",
            "t goes out of scope. No resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s acquired ownership of a resource (in all branches above)",
            "s acquired ownership of a resource (in all branches above that reach this point)",
            "t was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.",
            "m is bound to a value",
        ],
    },
    TooltipExpect {
        name: "diverging_call_arm",
        // `fail()` has type `!` (a call to a `-> !` function), so like
        // `panic!()` that arm binds nothing and doesn't reach the merge.
        // Whether `fail` panics or exits isn't looked at: the merge says
        // only that the arm never finishes (#350).
        must_contain: &[
            "s acquired ownership of a resource in every branch that reaches this point (the _ arm never finishes)",
        ],
        must_not_contain: &[
            "s acquired ownership of a resource (in all branches above)",
            "s acquired ownership of a resource (in all branches above that reach this point)",
            "the _ arm panicked",
        ],
    },
    TooltipExpect {
        name: "diverging_branch_stops",
        // The `return` branch doesn't converge into the merge dot
        // (#317): `t`'s and `s`'s columns in it end at the `return`,
        // where both are dropped (#363), so there's no bar.
        // `return_drops_what_the_branch_owns` counts the drops.
        must_contain: &[
            "t goes out of scope. Its resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "match_diverging_arm",
        // #317: the `_ => unreachable!()` arm ends in a bar on `s`'s
        // column (`s` has no value yet there), and the `_ => panic!()`
        // arm on `m`'s. `s` and `t` are dropped at that `panic!`
        // (#410), which ends their columns there instead of a bar;
        // `return_drops_what_the_branch_owns` counts the drops.
        must_contain: &[
            "t goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "index_assign",
        // `v[0] = 9`, `v[1] += k`, `v[2] = v[0]`, `names[0] = s`,
        // `grid[1][0] = 7`, `ps[0] = p`: one element is overwritten;
        // the collection keeps its resource. Before #144 these writes
        // weren't drawn, and `p.name` still dropped a String that had
        // moved into `ps[0]` with `p`. An assignment reads like a plain
        // assignment with the place as subject (2026-11-05 decision);
        // `+=` modifies the element in place and keeps "modified
        // through".
        must_contain: &[
            // A literal has no source to name; a named place does.
            "v[0] is assigned a new value",
            "v's resource is modified through v[1]",
            // `v[2] = v[0]`: an element read is not a modelled source
            // (no arrow), so there is nothing to name.
            "v[2] is assigned a new value",
            "v[0] is assigned a copy of *z",
            "Move from s to names[0]",
            "names[0] acquires ownership of a new resource; its previous resource is dropped",
            // `+=` on a String calls `add_assign`: nothing is overwritten.
            "names's resource is modified through names[0]",
            "s's resource is moved",
            "s goes out of scope. No resource is dropped.",
            "Move from String::from to names[0]",
            "grid[1][0] is assigned a new value",
            // Nested collections read the same at any depth: the place
            // is the subject, not the outermost collection.
            "grid[0][1] is assigned a copy of y",
            "Copy from y to grid[0][1]",
            "grid's value is modified through grid[1][1]",
            "cube[0][0][1] is assigned a new value",
            "rows[0][0] acquires ownership of a new resource; its previous resource is dropped",
            "Move from p to ps[0]",
            "ps[0] acquires ownership of a new resource; its previous resource is dropped",
            "p.name's resource is moved",
            "p.name goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            // Treating `names[0] = …` as a write to `names` itself
            // (the first version of this change) said the whole Vec
            // was replaced and dropped.
            "names acquires ownership of a new resource; its previous resource is dropped",
            "v acquires ownership of a new resource; its previous resource is dropped",
            // `v[1] += k`: k is an operand, not the element's new value.
            "Copy from k to v[1]",
            "v[1] is assigned a new value",
            "v[2] is assigned a copy of",
            "p.name goes out of scope. Its resource is dropped.",
            // An assignment is not "modified through", and the arrow
            // carries the move / copy.
            "(copied from",
            "(moved from",
            "v's resource is modified through v[0]",
            "v's resource is modified through v[2]",
            "grid's resource is modified through grid[1][1]",
            "grid's resource is modified through grid[1][0]",
            "grid's resource is modified through grid[0][1]",
            "cube's resource is modified through cube[0][0][1]",
            "rows's resource is modified through rows[0][0]",
            "grid[1][1] is assigned",
            "ps's resource is modified through ps[0]",
            "the value previously in",
            // Integers have nothing to drop.
            "v[0] acquires ownership",
        ],
    },
    TooltipExpect {
        name: "index_assign_through_ref",
        // `xs[i] = x` (xs: &mut [i32]), `dst[0] = s` (dst: &mut Vec),
        // `r[0] = …` (r = &mut v): the write assigns to the place, which
        // reads like a plain assignment with the place as subject. The
        // parameters' borrows still end on the write's line (another
        // arrow on that line used to hide that event).
        must_contain: &[
            "Copy from x to xs[i]",
            "xs[i] is assigned a copy of x",
            "xs's mutable borrow ends",
            "Move from s to dst[0]",
            "dst[0] acquires ownership of a new resource; its previous resource is dropped",
            "dst's mutable borrow ends",
            "s goes out of scope. No resource is dropped.",
            "Move from String::from to r[0]",
            "r[0] acquires ownership of a new resource; its previous resource is dropped",
            "Return mutably borrowed resource from r to v",
        ],
        must_not_contain: &[
            "xs[i] is assigned a new value",
            "xs acquires ownership of a resource",
            "dst acquires ownership of a resource",
            "dst acquires ownership of a new resource; its previous resource is dropped",
            "r acquires ownership of a resource",
            "s goes out of scope. Its resource is dropped.",
            "(copied from",
            "(moved from",
            "the resource xs points to is modified through xs[i]",
            "the resource dst points to is modified through dst[0]",
            "the resource r points to is modified through r[0]",
            "the value previously in",
        ],
    },
    // ─── Writes through a reference stored in an element (#396) ─────
    TooltipExpect {
        name: "write_through_element_ref",
        // `*slots[0] += 10` writes through the `&mut i32` in slots[0];
        // slots is a Vec, not a reference, so "the resource slots
        // points to … through *slots" was false. An assignment
        // (`*names[0] = s`) reads like a plain assignment with the
        // place as the subject; `+=` keeps "modified through". What
        // the element points to is a "value" when it needs no drop
        // (an i32), a "resource" when it does (a String) (#484).
        must_contain: &[
            "the value slots[0] points to is modified through *slots[0]",
            "*slots[1] is assigned a new value",
            // A named Copy source is named (`*slots[1] = k`).
            "*slots[1] is assigned a copy of k",
            "Move from s to *names[0]",
            "the value hs[0].r points to is modified through *hs[0].r",
            "*boxes[0] is assigned a new value",
            // Overwriting a String through the stored reference drops the
            // old String; no column shows it, so the tooltip says it.
            "*names[0] acquires ownership of a new resource; its previous resource is dropped",
        ],
        must_not_contain: &[
            "the resource slots points to is modified through *slots",
            "the resource names points to is modified through *names",
            // `=`: whole tooltip. "Move from s to *names[0]" is correct (#396).
            "=Move from s to *names",
            "the resource boxes points to is modified through *boxes",
            // An i32 is a value, not a resource (#484).
            "the resource slots[0] points to is modified through *slots[0]",
            "the resource hs[0].r points to is modified through *hs[0].r",
            // An assignment is not "modified through"; the arrow carries
            // the move or copy.
            "*names[0] is assigned a copy of",
            "(copied from",
            "(moved from",
            "the value previously in",
            "the resource names[0] points to is modified through *names[0]",
            "the value slots[1] points to is modified through *slots[1]",
            "the value boxes[0] points to is modified through *boxes[0]",
        ],
    },
    TooltipExpect {
        name: "nested_fields_params_patterns",
        // Reading `r.tl.x` of a struct that came from a fn parameter or
        // a pattern binding copies out of its own column, like a
        // `let`-bound struct's. Before #239 these reads drew nothing
        // and the target was only "a goes out of scope. No resource is
        // dropped." `e.part` (a `&str` field of a parameter) used to
        // "acquire ownership" (#351).
        must_contain: &[
            "Copy from r.tl.x to a",
            "a is initialized with a copy of r.tl.x",
            "r.tl.x holds a copyable value",
            "Copy from m.tl.y to b",
            "b is initialized with a copy of m.tl.y",
            "Copy from n.tl.x to c",
            "Copy from t.tl.y to d",
            "e.part is an immutable borrow from the caller",
            "e.part's immutable borrow ends",
        ],
        must_not_contain: &[
            "a goes out of scope. No resource is dropped.",
            "b goes out of scope. No resource is dropped.",
            "e.part acquires ownership of a resource",
            "e.part is the owner of the resource",
        ],
    },
    // #363: leaving the function from a match arm drops what it
    // still owns, at the `return`.
    TooltipExpect {
        name: "return_in_arm_drops",
        must_contain: &["v goes out of scope. Its resource is dropped."],
        // w was moved into `keep` before the `return`.
        must_not_contain: &["w goes out of scope. No resource is dropped."],
    },
    // #409: the arm-local `s` owns its String until the arm's end.
    TooltipExpect {
        name: "match_arm_with_let",
        must_contain: &["s goes out of scope. Its resource is dropped."],
        must_not_contain: &["s goes out of scope. No resource is dropped."],
    },
    TooltipExpect {
        name: "pattern_struct_ref_fields",
        // `x.part` (a `&str` field of the struct bound by `if let
        // Some(x) = o`) is a reference, never an owner. It holds the
        // reference `o` held, a borrow of `s` that RustViz doesn't
        // carry through `o`, so no lender is named: no "from the
        // caller", no return arrow to x.part or *x.part. `q`, a copy of
        // that reference, borrows what x.part borrows, not x.part.
        must_contain: &[
            "x.part holds an immutable reference (it came with the value matched into x)",
            "Copy from x.part to q",
            "q's immutable borrow ends",
            "y.e.part's immutable borrow ends",
            "y.m's mutable borrow ends",
        ],
        must_not_contain: &[
            "x.part acquires ownership of a resource",
            "x.part is the owner of the resource",
            "x.part is an immutable borrow from the caller",
            "Return immutably borrowed resource from q to x.part",
            "Return immutably borrowed resource from x.part to *x.part",
            "y.e.part acquires ownership of a resource",
            "y.m acquires ownership of a resource",
            "y.m is the owner of the resource",
        ],
    },
    // ─── Fields of a pattern-bound struct (#405) ────────────────────
    TooltipExpect {
        name: "pattern_struct_fields_scope",
        // `x` (if let), `y` / `z` (match arms) and `w` (while let) are
        // structs bound by a pattern. Each field holds its part of the
        // value from the pattern line and goes out of scope with the
        // struct at the end of the arm; a String field is dropped
        // there. In a match arm the fields used to be drawn as
        // variables that existed before the match ("live in a
        // conditional expression" on the match's split dot, "has no
        // value yet"), losing the pattern line's events, among them
        // z.part's "holds an immutable reference" dot (#352).
        must_contain: &[
            "x.name goes out of scope. Its resource is dropped.",
            "x.n goes out of scope",
            "y.name acquires ownership of a resource",
            "y.name goes out of scope. Its resource is dropped.",
            "y.n goes out of scope",
            "z.part holds an immutable reference (it came with the value matched into z)",
            "z.part goes out of scope",
            "z.k goes out of scope",
            "w.name goes out of scope. Its resource is dropped.",
            "w.n goes out of scope",
        ],
        must_not_contain: &[
            "y.n has no value yet",
            "z.part has no value yet",
            "z.k has no value yet",
        ],
    },
    // ─── Writes to places with no column (#458) ──────────────────────
    TooltipExpect {
        name: "place_without_column",
        // `t.0 = …` and `v.last_mut().unwrap().name = …` overwrite a
        // String: the old one is dropped, and the tooltip on the
        // variable's column says so. `t.1 = 2` overwrites an integer,
        // which has nothing to drop.
        must_contain: &[
            "t.0 acquires ownership of a new resource; its previous resource is dropped",
            "t.1 is assigned a new value",
            // A named Copy source is named (`t.1 = n`).
            "t.1 is assigned a copy of n",
            "v.last_mut().unwrap().name acquires ownership of a new resource; its previous resource is dropped",
            "u.0 acquires ownership of a new resource; its previous resource is dropped",
            "Move from String::from to t.0",
            "Move from s to u.0",
        ],
        must_not_contain: &[
            "the value previously in t.1 is dropped",
            "t.0 is assigned a copy of",
            // An assignment reads like a plain assignment (2026-11-05).
            "t's resource is modified through t.0",
            "t's resource is modified through t.1",
            "v's resource is modified through v.last_mut().unwrap().name",
            "u's resource is modified through u.0",
            "the value previously in",
            "(moved from",
            "(copied from",
        ],
    },
    TooltipExpect {
        name: "for_over_iterator_var",
        // `for x in it` moves the iterator `it` into the loop; each `x` is
        // a reference `next()` hands out, not something `it` lends. It
        // used to read "Move from it to x" and "Return ... from x to it"
        // (#417). A collection by value (`for s in w`) still moves into
        // the loop variable's column. That `s` is a `String`, the first
        // a `&String`: it has a column of its own, also labelled `s`
        // (#598). Sharing the first's drew it as a reference ("the
        // resource s points to is immutably borrowed by println!") and
        // never dropped it.
        must_contain: &[
            // A slice iterator has nothing to drop: a value (#484).
            "it's value is moved",
            "x is bound to a mutable reference",
            "s is bound to an immutable reference",
            "Move from w to s",
            "s's resource is immutably borrowed by println!; s is read-only while the borrow is live",
            "s goes out of scope. Its resource is dropped.",
            "s, immutable. A different variable from the s declared on line 13 (separate scopes, so only one exists at a time)",
            "s, immutable. A different variable from the s declared on line 10 (separate scopes, so only one exists at a time)",
        ],
        must_not_contain: &[
            "it's resource is moved",
            "Move from it to x",
            "Return mutably borrowed resource from x to it",
            "Move from it2 to s",
            "Return immutably borrowed resource from s to it2",
            "s₂",
        ],
    },
    // ─── Tuples (#189) ────────────────────────────────────────────────
    // A tuple variable has a column per element (`t.0`, `t.1`), like a
    // struct's fields, so moving one element out leaves the others
    // owned by the tuple, and each element drops on its own.
    TooltipExpect {
        name: "tuple_element_move",
        must_contain: &[
            "Move from String::from to t.0",
            "Move from t.0 to first",
            "t.0's resource is moved",
            "first goes out of scope. Its resource is dropped.",
            "t.0 goes out of scope. No resource is dropped.",
            "t.1 goes out of scope. Its resource is dropped.",
            "t goes out of scope. t.0 was moved out earlier, so only t.1 is dropped.",
        ],
        must_not_contain: &[
            "first goes out of scope. No resource is dropped.",
            "t goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "tuple_create_copy",
        must_contain: &[
            "Move from String::from to t.0",
            "Copy from t.1 to n",
            "n is initialized with a copy of t.1",
            "t.0 goes out of scope. Its resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "t goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "tuple_destructure_var",
        // `let (a, _) = t;` takes t.0 only; `_` doesn't move t.1.
        // `let (x, .., z) = u;` takes u.0 and u.2.
        must_contain: &[
            "Move from t.0 to a",
            "t.0 goes out of scope. No resource is dropped.",
            "t.1 goes out of scope. Its resource is dropped.",
            "t goes out of scope. t.0 was moved out earlier, so only t.1 is dropped.",
            "Move from u.0 to x",
            "Move from u.2 to z",
            "u goes out of scope. u.0 and u.2 were moved out earlier, so no resource is dropped.",
        ],
        must_not_contain: &[
            "Move from t to a",
            "Move from u to x",
            "Move from u to z",
            "t goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "tuple_field_write",
        must_contain: &[
            "Move from s to t.0",
            "t.0 acquires ownership of a new resource; its previous resource is dropped",
            "s goes out of scope. No resource is dropped.",
            // `t = (w, 2);` drops the old tuple, element by element.
            "Move from w to t.0",
            "t acquires ownership of a new resource; its previous resource is dropped",
            "w goes out of scope. No resource is dropped.",
            "t.0 goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "w goes out of scope. Its resource is dropped.",
            "t goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "tuple_through_calls",
        must_contain: &[
            "s's resource is moved to the caller",
            "s goes out of scope. No resource is dropped.",
            "Move from m to show",
            "m goes out of scope. No resource is dropped.",
            "Move from t.0 to u.0",
            "t.0 goes out of scope. No resource is dropped.",
            "u.0 goes out of scope. Its resource is dropped.",
            "p.0 goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s goes out of scope. Its resource is dropped.",
            "m goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "tuple_field_read",
        must_contain: &[
            "Copy from t.0 to a",
            "a is initialized with a copy of t.0",
        ],
        must_not_contain: &[
            "a goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "match_tuple_destructure",
        // Each sub-pattern takes its own element of `pair`.
        must_contain: &[
            "Move from pair.0 to x",
            "Move from pair.1 to y",
            "pair goes out of scope. pair.0 and pair.1 were moved out earlier, so no resource is dropped.",
        ],
        must_not_contain: &[
            "Partial move from pair to x",
            "Partial move from pair to y",
        ],
    },
    // ─── A borrow stored in a column-less place (#282) ───────────────
    TooltipExpect {
        name: "borrow_into_element",
        // `v[0] = &u` / `w[0] = &mut c`: the element is the borrower,
        // and the borrow ends at v's / w's last use, after which `c`
        // owns (and drops) its String again. `h.r = &y` (h: &mut
        // Holder, y: &String) stores a reborrow of the String y points
        // to in the Holder h points to: h.r is the borrower, and the
        // borrow outlives `point`, so it doesn't end there (it used to
        // say "y's resource ... stored in a place that isn't drawn",
        // and end on the same line). A field of a call's result
        // (`hs.last_mut().unwrap().r = &z`) can't be named.
        must_contain: &[
            "u's resource is immutably borrowed by v[0]; u is read-only while the borrow is live",
            "u's resource is no longer immutably borrowed",
            "c's resource is mutably borrowed by w[0]; c can't be used while the borrow is live",
            "c's resource is no longer mutably borrowed",
            "c goes out of scope. Its resource is dropped.",
            "the resource y points to is immutably borrowed by h.r",
            "the resource z points to is immutably borrowed, and the reference is stored in a place that isn't drawn",
        ],
        must_not_contain: &[
            // y and z are references: they don't own the String.
            "y's resource is immutably borrowed, and the reference is stored in a place that isn't drawn",
            "z's resource is immutably borrowed, and the reference is stored in a place that isn't drawn",
            // The reborrow lives on in the caller's Holder / Vec.
            "the resource y points to is no longer borrowed through y",
            "the resource z points to is no longer borrowed through z",
            "h.r no longer borrows the resource y points to",
            "u's resource is immutably borrowed by Anonymous resource; u is read-only while the borrow is live",
            "c's resource is mutably borrowed by Anonymous resource; c can't be used while the borrow is live",
            "y's resource is immutably borrowed by Anonymous resource",
            "c goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "shadow_after_move",
        // `let t = s; let s = …;` used to panic ("Immutable variable s
        // cannot reacquire resources!", #253): both `s`es shared a RAP.
        must_contain: &[
            "Move from s to t",
            "s goes out of scope. No resource is dropped.",
            "Move from String::from to s₂",
            "s₂ goes out of scope. Its resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "shadow_after_move_into_fn",
        // `take(s); let s = …;`: same panic as above (#253).
        must_contain: &[
            "Move from s to take",
            "s goes out of scope. No resource is dropped.",
            "s₂ goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "shadow_with_live_borrow",
        // The first `s` is still borrowed by `r` after it's shadowed,
        // and is dropped at the end of `main` (#173). The merged column
        // said the first String was never dropped.
        must_contain: &[
            "Immutable borrow from s to r",
            "Return immutably borrowed resource from r to s",
            "s goes out of scope. Its resource is dropped.",
            "s₂ goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "shadow_in_block",
        // `let spaces = spaces.len();` binds a new `usize`, and the
        // inner block's `x` ends at the block's `}` (#173).
        must_contain: &[
            "len reads from spaces",
            "spaces₂ is initialized with a copy of len",
            "Copy from len to spaces₂",
            "x₂ is bound to a copyable value",
            "x₂ goes out of scope",
            "x₂, immutable. x₂ shadows x: it is a new variable named x, declared on line 10",
        ],
        must_not_contain: &[
            // Not "a different variable": the earlier one is still in scope.
            "x₂, immutable. x₂ is a different variable from x: the 2nd variable named x in this function",
            "spaces is initialized with a copy of len",
            "=Copy from len to spaces",
        ],
    },
    TooltipExpect {
        name: "shadow_sibling_blocks",
        // Two `let s` in sibling blocks: neither shadows the other (the
        // first is out of scope when the second is declared), so they
        // share one column `s`, with no subscript. The first block's `s`
        // is moved by `drop`; the second block's `s` drops its String
        // at the end of its block.
        must_contain: &[
            "Move from s to drop",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "Move from String::from to s₂",
            "s₂ goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "shadow_shared_columns",
        // A subscript means "shadows an earlier variable still in
        // scope". `s₂` shadows `s`; the two inner blocks each shadow `s₂`
        // and share the column `s₃`, whose header lists both lines. The
        // arms of an `if` shadow nothing: one column `x`, no `x₂`. The
        // second block's `let s = 6;` initializes a new variable in the
        // shared column; it isn't an assignment to the first block's.
        must_contain: &[
            "s₃ is bound to a copyable value",
            "s₂, immutable. s₂ shadows s: it is a new variable named s, declared on line 9",
            "s₃, immutable. s₃ shadows s₂: it is a new variable named s, declared on lines 11 and 15 (in different branches or blocks, so only one exists at a time)",
            "x, immutable",
        ],
        must_not_contain: &[
            "s₂, immutable. s₂ is a different variable from s: the 2nd variable named s in this function",
            "s₄, immutable",
            "x₂, immutable",
            "s₃ is assigned a new value",
        ],
    },
    TooltipExpect {
        name: "shadow_match_arm",
        // The arms' `n` shadows the outer `n` (#173); the two
        // `Some(n)` arms share the column `n₂`. The first arm's body is a
        // bare `println!`, whose span is inside the macro: that put its
        // `n` in scope to the end of the fn, so the second arm's `n` got
        // `n₃`, "shadowing" it (#598).
        must_contain: &[
            "n is bound to a copyable value",
            "n₂ is initialized with a copy of opt",
            "Partial copy from opt to n₂",
            "n₂, immutable. n₂ shadows n: it is a new variable named n, declared on lines 9 and 10 (in different branches or blocks, so only one exists at a time)",
        ],
        must_not_contain: &[
            "n is initialized with a copy of opt",
            "=Partial copy from opt to n",
            "n₃",
        ],
    },
    TooltipExpect {
        name: "shadow_rebind_chain",
        // `let s = s; let s = &s;` moves into `s₂`, then `s₃` borrows
        // `s₂` (#173). The merged column drew both as arrows from `s`
        // to itself.
        must_contain: &[
            "Move from s to s₂",
            "Immutable borrow from s₂ to s₃",
            "Return immutably borrowed resource from s₃ to s₂",
            "s₂ goes out of scope. Its resource is dropped.",
            "s₃, immutable. s₃ shadows s₂: it is a new variable named s, declared on line 9",
        ],
        must_not_contain: &[
            // Not "a different variable": the earlier one is still in scope.
            "s₃, immutable. s₃ is a different variable from s: the 3rd variable named s in this function",
            "=Move from s to s",
            "Immutable borrow from s to s",
            "Return immutably borrowed resource from s to s",
        ],
    },
    // ─── Destructuring a struct variable (#326) ─────────────────────
    TooltipExpect {
        name: "struct_destructure_let",
        // `let User { name, age, .. } = user;` moves `user.name` into
        // `name` and copies `user.age` into `age`; `user.email` stays in
        // `user` (still usable, and dropped with it). `let Pair(left,
        // right) = pair;` moves `pair.0` and `pair.1`. The field columns
        // that were moved out drop nothing.
        must_contain: &[
            "Move from user.name to name",
            "Copy from user.age to age",
            "user goes out of scope. user.name was moved out earlier, so only user.email is dropped.",
            "user.name goes out of scope. No resource is dropped.",
            "Move from pair.0 to left",
            "Move from pair.1 to right",
            "pair goes out of scope. pair.0 and pair.1 were moved out earlier, so no resource is dropped.",
            "pair.0 goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "Move from user to name",
            "Copy from user to age",
            "user.name goes out of scope. Its resource is dropped.",
            "Move from pair to left",
            "Move from pair to right",
            "pair.0 goes out of scope. Its resource is dropped.",
            "pair.1 goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "struct_destructure_match",
        // A struct pattern in a match arm, nested: each name takes its
        // own (nested) field of `o`.
        must_contain: &[
            "Move from o.inner.x to x",
            "Copy from o.inner.y to y",
            "Move from o.tag to tag",
            "o goes out of scope. o.inner.x and o.tag were moved out earlier, so no resource is dropped.",
            "o.inner.x goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "Move from o to x",
            "Copy from o to y",
            "Move from o to tag",
            "o.inner.x goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "compound_assign",
        // #260: `a op= b` used to be drawn as `a = b`. On i32s,
        // `acc += x` reads `acc` and `x` and writes the sum back:
        // both are copied into the operator and `acc` gets a new value.
        // On a String, `s += &t` is `AddAssign::add_assign(&mut s,
        // &t)`: `s` is mutably borrowed and appended to in place
        // (nothing dropped, no borrow stored in `s`), `t` is borrowed
        // for the call. Through a reference the update keeps its
        // wording (2026-11-06 decision): `*r += "c"` is "the resource r
        // points to is modified through *r", `*n += by` "the value n
        // points to is modified through *n", `v[i] += k` "modified
        // through v[0]"; the operator doesn't reborrow `r`.
        must_contain: &[
            "Copy from x to +=",
            "x's value is copied",
            "acc is assigned a new value",
            // The old value of `acc` is read too, as in `acc = acc + x`.
            "Copy from acc to +=",
            "acc's value is copied",
            // An i32 field: assigned a new value (#272), not "acquires ownership".
            "Copy from a.total to +=",
            "a.total is assigned a new value",
            "+= reads from/writes to s (implicitly mutably borrowed)",
            "s's resource is mutably borrowed by +=; s can't be used while the borrow is live",
            "+= reads from t",
            "t's resource is immutably borrowed by +=; t is read-only while the borrow is live",
            "the resource r points to is modified through *r",
            "the resource v points to is modified through v[0]",
            "Copy from by to +=",
            // `*n` is read too.
            "Copy from *n to +=",
            "the value n points to is modified through *n",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "acc is initialized with a copy of x",
            "Copy from x to acc",
            "s immutably borrows a resource",
            "s acquires ownership of a new resource; its previous resource is dropped",
            "t's resource is immutably borrowed by s; t is read-only while the borrow is live",
            "Immutable borrow from t to s",
            "a.total acquires ownership of a resource",
            "Copy from by to *n",
            "the resource n points to is modified through *n (copied from by)",
            "+= reads from/writes to r (mutable reborrow",
            "r's resource is mutably borrowed by +=",
        ],
    },
    // ─── Fields moved out on some paths only (#395) ─────────────────
    // A struct or tuple whose fields were moved out in some branches
    // only: at its scope end those fields are dropped only if the path
    // taken didn't move them (a runtime drop flag), which is what the
    // fields' own columns say (#179) and now the struct's too.
    TooltipExpect {
        name: "partial_move_one_arm_tuple",
        must_contain: &[
            "t goes out of scope. t.0 and t.1.1 were moved out in some of the branches above; \
             each is dropped here only if no branch above moved it (Rust tracks this with runtime \
             drop flags).",
            "t.1 goes out of scope. t.1.1 was moved out in at least one branch above, so it's \
             dropped here only if no branch above moved it (Rust tracks this with a runtime drop \
             flag).",
            "t.0 goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "t goes out of scope. t.0 and t.1.1 were moved out earlier, so no resource is dropped.",
            "t.1 goes out of scope. t.1.1 was moved out earlier, so no resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "partial_move_one_arm_struct",
        must_contain: &[
            "p goes out of scope. p.a was moved out in at least one branch above, so it's dropped \
             here only if no branch above moved it (Rust tracks this with a runtime drop flag). \
             p.b is dropped.",
            "p.b goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["p goes out of scope. p.a was moved out earlier, so only p.b is dropped."],
    },
    TooltipExpect {
        name: "partial_move_if_branches",
        must_contain: &[
            "p goes out of scope. p.a and p.b were moved out in some of the branches above; each \
             is dropped here only if no branch above moved it (Rust tracks this with runtime drop \
             flags).",
            "q goes out of scope. q.b was moved out earlier. q.a was moved out in at least one \
             branch above, so it's dropped here only if no branch above moved it (Rust tracks this \
             with a runtime drop flag).",
            // Moved in both branches: moved for sure.
            "r goes out of scope. r.a was moved out earlier, so only r.b is dropped.",
        ],
        must_not_contain: &[
            "p goes out of scope. p.a and p.b were moved out earlier, so no resource is dropped.",
            "q goes out of scope. q.a and q.b were moved out earlier, so no resource is dropped.",
        ],
    },
    // ─── "resource" only for values that need drop (#484) ────────────
    // `w.iter()` is a `slice::Iter`: not Copy, but no drop glue.
    TooltipExpect {
        name: "no_drop_iter",
        must_contain: &[
            // it2 holds an `Iter`, a value that borrows `w` (#336): its
            // column says so instead of the plain-value wording.
            "it2 holds a value that immutably borrows w's resource (it2 isn't a reference itself)",
            "it2 holds a value that contains a reference (it2 isn't a reference itself)",
            "it2 goes out of scope",
            "it2's value is immutably borrowed by len; it2 is read-only while the borrow is live",
            "w acquires ownership of a resource",
            "w goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "it2 acquires ownership of a resource",
            "it2 is the owner of the resource",
            "it2 goes out of scope. Its resource is dropped.",
            "it2's resource",
            "No resource is dropped",
        ],
    },
    // A struct of integers that doesn't derive Copy.
    TooltipExpect {
        name: "non_copy_struct_move",
        must_contain: &[
            "p is bound to a value from the caller",
            "p is bound to a value",
            "p's value is moved",
            "p.x's value is moved",
            "p holds a value",
            "p goes out of scope",
        ],
        must_not_contain: &[
            "p acquires ownership of a resource",
            "p acquires ownership from the caller",
            "p's resource is moved",
            "p.x's resource is moved",
            "p is the owner of the resource",
            "p goes out of scope. Its resource is dropped.",
            "p goes out of scope. No resource is dropped.",
        ],
    },
    // A String still needs drop, so keeps the resource wording.
    TooltipExpect {
        name: "string_resource_wording",
        must_contain: &[
            "s acquires ownership from the caller",
            "s acquires ownership of a resource",
            "s's resource is moved",
            "s is the owner of the resource",
            "s goes out of scope. Its resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s's value is moved",
            "s is bound to a value",
            "t is bound to a value",
        ],
    },
    // Overwriting, branches and merges for a no-drop struct: nothing is
    // dropped, so nothing says so.
    TooltipExpect {
        name: "no_drop_reassign_branch",
        must_contain: &[
            "p is bound to a value",
            "p is assigned a new value",
            "p goes out of scope",
            "r's value is moved",
            "r was moved in at least one branch above, so it can't be used after the branches (until it's assigned again).",
            "q is bound to a value (in all branches above)",
        ],
        must_not_contain: &[
            "p acquires ownership of a new resource; its previous resource is dropped",
            "p goes out of scope. Its resource is dropped.",
            "r's resource is moved",
            "its resource is dropped when r goes out of scope",
            "q acquired ownership of a resource (in all branches above)",
        ],
    },
    // std's lock types own an OS synchronization object. Their drop glue
    // depends on the host's std (pthread on macOS, futex / SRW on Linux
    // and Windows), so this wording must be the same on every platform:
    // always "resource", never "value". CI runs Linux, Windows and macOS.
    TooltipExpect {
        name: "sync_lock_resource",
        must_contain: &[
            "m acquires ownership of a resource",
            "m's resource is immutably borrowed by lock",
            "rw acquires ownership of a resource",
            "rw's resource is immutably borrowed by read",
            "cv acquires ownership of a resource",
            "sh acquires ownership of a resource",
            "m goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "m is bound to a value",
            "m's value is immutably borrowed by lock",
            "rw's value is immutably borrowed by read",
            "cv is bound to a value",
            "sh is bound to a value",
        ],
    },
    TooltipExpect {
        name: "no_drop_partial_maybe",
        // A struct of slice iterators (nothing to drop) with one field
        // moved in a branch: no "dropped" claims (#484 with #395).
        must_contain: &[
            "w goes out of scope. w.a was moved out in at least one branch above.",
            "it goes out of scope",
        ],
        must_not_contain: &[
            "dropped here only if no branch above moved it",
            "Its resource is dropped",
            "it goes out of scope. Its resource is dropped.",
        ],
    },
    // ─── Temporary lifetime extension (#400) ─────────────────────────
    TooltipExpect {
        name: "extended_temporary",
        // `let r = &String::from("a");`: the String is a temporary that
        // lives until the end of the block because r's `let` borrows
        // it. It gets a column, r borrows from it, and it's dropped at
        // scope end. `show(&String::from(..))` (dropped at the end of
        // its statement) and `&Point { x: 4 }` (nothing to drop) get
        // no extended-temporary column. With #457 the former still
        // gets an unbound `(temporary 4)` (one numbering for both
        // kinds), so inner's is `(temporary 5)`.
        must_contain: &[
            "Move from String::from to (temporary)",
            "(temporary) acquires ownership of a resource. This temporary value is borrowed by a let, so it lives until the end of the enclosing block, not just to the end of the statement",
            "Immutable borrow from (temporary) to r",
            "Return immutably borrowed resource from r to (temporary)",
            "(temporary) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
            "Move from Vec::new to (temporary 2)",
            "Mutable borrow from (temporary 2) to m",
            "(temporary 2) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
            "Immutable borrow from (temporary 3) to s",
            "(temporary 3) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
            "Immutable borrow from (temporary 5) to inner",
            "(temporary 5) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from r to String::from",
            "Immutable borrow from (temporary 4) to inner",
            "unbound temporary",
        ],
    },
    TooltipExpect {
        name: "mutref_moved_into_generic",
        // `push(y)` (param `&mut String`): rustc reborrows y, so y comes
        // back. `take(y)` (param `T`): no reborrow; the `&mut` itself is
        // moved into take, like an owner passed by value. It used to be
        // drawn as a borrow that y gets back.
        must_contain: &[
            "push reads from/writes to y (mutable reborrow — y can't be used until push returns)",
            "Move from y to take",
            "s's resource is no longer mutably borrowed",
        ],
        must_not_contain: &[
            "take reads from/writes to y",
            "y's resource is mutably borrowed by take; y can't be used while the borrow is live",
            "take reads from/writes to y (mutable reborrow — y can't be used until take returns)",
        ],
    },
    TooltipExpect {
        name: "move_into_call_same_line_borrow_end",
        // r's borrow of t ends on the `takes(s, r.len())` line, and s is
        // moved into takes there. Both arrows are drawn.
        must_contain: &[
            "Move from s to takes",
            "s goes out of scope. No resource is dropped.",
            "Return immutably borrowed resource from r to t",
        ],
        must_not_contain: &[
            // `=`: whole tooltip. `takes`'s parameter `_s` drops its String, and "_s goes out of scope. Its resource is dropped." is correct.
            "=s goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "shadow_matrix_let",
        // Re-binding a name with `let` declares a new variable; the
        // shadowed one keeps (and later drops) its value. Main crashes
        // on this file (#253); the false texts below are what main drew
        // for each fn on its own, where the two variables shared a
        // column.
        must_contain: &[
            // owned_twice: shadowing drops nothing; both drop at `}`.
            "a goes out of scope. Its resource is dropped.",
            "a₂ goes out of scope. Its resource is dropped.",
            // copies
            "Copy from x₂ to y",
            "y is initialized with a copy of x₂",
            // type_change
            "w₂ is initialized with a copy of len",
            // move_into_new
            "Move from m to m₂",
            "m goes out of scope. No resource is dropped.",
            "m₂ goes out of scope. Its resource is dropped.",
            // borrow_shadowed: r still owns its String, r₂ borrows it.
            "Immutable borrow from r to r₂",
            "Return immutably borrowed resource from r₂ to r",
            "r goes out of scope. Its resource is dropped.",
            // trim_shadowed
            "Immutable borrow from t to t₂: trim's signature gives the reference it returns the same lifetime as the value it was called on",
            "t goes out of scope. Its resource is dropped.",
            // mut_rebind
            "u₂, mutable. u₂ shadows u: it is a new variable named u, declared on line 52",
            "Move from u to u₂",
            "u₂ goes out of scope. Its resource is dropped.",
            // borrow_outlives_shadowing
            "Return immutably borrowed resource from p to q",
            "q goes out of scope. Its resource is dropped.",
            "q₂ is bound to a copyable value",
            // ref_shadowed
            "Immutable borrow from h to k₂",
            "Return immutably borrowed resource from k₂ to h",
            // three
            "Move from c₂ to +",
            "c₂ goes out of scope. No resource is dropped.",
            "c₃ goes out of scope. Its resource is dropped.",
            "c₃, immutable. c₃ shadows c₂: it is a new variable named c, declared on line 78",
            // deferred_init: `d = …` initializes d₂, not the first d.
            "Move from String::from to d₂",
            "d goes out of scope. Its resource is dropped.",
            "d₂ goes out of scope. Its resource is dropped.",
            // shorthand_field
            "Move from e₂ to w.e",
        ],
        must_not_contain: &[
            // Not "a different variable": the earlier one is still in scope.
            "c₃, immutable. c₃ is a different variable from c: the 3rd variable named c in this function",
            // Not "a different variable": the earlier one is still in scope.
            "u₂, mutable. u₂ is a different variable from u: the 2nd variable named u in this function",
            "=Move from m to m",
            "=Immutable borrow from r to r",
            "r's resource is immutably borrowed by r; r is read-only while the borrow is live",
            "r goes out of scope. No resource is dropped.",
            "Copy from trim to t",
            "t is initialized with a copy of trim",
            "=Move from u to u",
            "q goes out of scope. No resource is dropped.",
            "=Immutable borrow from h to k",
            "=Copy from len to w",
            "w is assigned the result of len",
            "Move from c to c",
        ],
    },
    TooltipExpect {
        name: "shadow_matrix_scopes",
        // An inner `let` ends with its block, and later uses go to the
        // outer column. Main crashes on this file (#253); the false
        // texts are main's for each fn on its own. Known gaps, not
        // asserted: `loop_body`'s l₂ never goes out of scope (#256);
        // `early_return` still drops r / r₂ at its unreachable `}`
        // (#515). Before `return`'s drops (#411) looked names up per
        // binding, `return r;` dropped r₂ there and not r.
        must_contain: &[
            // block_then_outer
            "a₂ is initialized with a copy of len",
            "a₂ goes out of scope",
            "Move from a to t",
            "a goes out of scope. No resource is dropped.",
            // if_arm
            "b₂ goes out of scope. Its resource is dropped.",
            "b goes out of scope. Its resource is dropped.",
            // loop_body
            "Move from String::from to l₂",
            "l goes out of scope. Its resource is dropped.",
            // param / ref_param
            "s₂'s resource is moved to the caller",
            "Move from to_string to v₂",
            "v₂ goes out of scope. Its resource is dropped.",
            // early_return: the first r is dropped at each `return`
            // (its drop dot ends its column, #410); r₂ is moved to the
            // caller. `branches_that_diverge_stop_short` checks that only
            // r₂'s column ends in a stop bar.
            "r goes out of scope. Its resource is dropped.",
            // main: its own s / s₂, unrelated to param's
            "Move from param to s₂",
            "s₂ goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "=Copy from len to a",
            "a is initialized with a copy of len",
            "b goes out of scope. No resource is dropped.",
            "Move from s to s",
        ],
    },
    TooltipExpect {
        name: "shadow_matrix_patterns",
        // Pattern bindings that reuse a name are new variables. Main
        // crashes on this file (#253). Known wrong, not asserted (same
        // as main with the bindings renamed): `for y in ys` says y₂
        // "acquires ownership of a resource" (#504) and
        // `let P { n, .. } = p` moves "from p" with p.n still dropping
        // (#326).
        must_contain: &[
            // if_let_same_name
            "Move from s to s₂",
            "s₂ goes out of scope. Its resource is dropped.",
            // match_same_name
            "Partial move from o to o₂",
            // while_let_outer
            "x₂ is initialized with a copy of next",
            "x is bound to a copyable value",
            // for_outer
            "Move from ys to y₂",
            "y goes out of scope. Its resource is dropped.",
            // else_if_let
            "z is initialized with a copy of a",
            "z is initialized with a copy of b",
            // tuple_swap
            "Move from g to f₂",
            "Move from f to g₂",
            "f goes out of scope. No resource is dropped.",
            "g₂ goes out of scope. Its resource is dropped.",
            // struct_pattern
            "n₂, immutable. n₂ shadows n: it is a new variable named n, declared on line 66",
            "n₂ goes out of scope. Its resource is dropped.",
            // else_if_let: the second `z` is in a different branch from the
            // first, so it doesn't shadow it and shares its column: no `z₂`,
            // no header note.
            // nested_match
            "w₂ is initialized with a copy of b",
        ],
        must_not_contain: &[
            "z₂",
            "z is assigned a copy of b",
            // Not "a different variable": the earlier one is still in scope.
            "n₂, immutable. n₂ is a different variable from n: the 2nd variable named n in this function",
            "=Move from s to s",
            "=s goes out of scope. No resource is dropped.",
            "=Copy from next to x",
            "x goes out of scope. No resource is dropped.",
            "=Move from ys to y",
            "Move from p to n",
        ],
    },
    TooltipExpect {
        name: "shadow_matrix_closures",
        // Closures capture the variable their name refers to at the
        // closure, not an earlier or later one with the same name. Main
        // crashes on this file (#253). The FnOnce call and the closure
        // argument (`fnonce_shadowed`, `closure_arg`) went to the first
        // `g` / `h` before the names were looked up per binding there.
        must_contain: &[
            // closure_param: `|s: &str|` leaves the outer s alone.
            "s goes out of scope. Its resource is dropped.",
            // captures_shadowing
            "Closure capture (immutable borrow) from c₂ to f",
            "c goes out of scope. Its resource is dropped.",
            // move_after_shadow
            "Closure capture (move) from m₂ to f",
            "Return immutably borrowed resource from t to m",
            "m goes out of scope. Its resource is dropped.",
            // captures_first
            "Closure capture (immutable borrow) from b to f",
            "Return immutably borrowed resource from f to b",
            "b₂ goes out of scope. Its resource is dropped.",
            // fnonce_shadowed
            "Move from g₂ to t",
            "g₂'s resource is moved",
            // closure_arg
            "Move from h₂ into the closure passed to apply",
            "h₂ goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "=Copy from len to c",
            "Closure capture (immutable borrow) from c to f",
            "m goes out of scope. No resource is dropped.",
            "Move from g to t",
            "g's resource is moved",
            "Move from h into the closure passed to apply",
        ],
    },
    // ─── Unbound temporaries (#457) ──────────────────────────────────
    TooltipExpect {
        name: "unbound_temporary",
        // `let _ = f();`, `f();`, `f().len()`, `show(&f())`, a match
        // scrutinee, an `if` condition, a block's tail expression, both
        // arms of an if/else and a `println!` argument each drop a
        // temporary, and the drop is stated (timing checked with a
        // Drop-printing program). Values that are moved on (`let kept =
        // make();`, `match Some(make()) { Some(s) => s, .. }`) are not
        // temporaries: no 12th column. The one-line `match` has a column
        // since #469 (it panicked before, #459).
        must_contain: &[
            "Move from String::from to (temporary)",
            "Move from make to (temporary 2)",
            "Move from make to (temporary 3)",
            "Move from String::from to (temporary 4)",
            "Move from Some to (temporary 5)",
            "Move from make to (temporary 6)",
            "Move from make to (temporary 7)",
            "Move from make to (temporary 8)",
            "Move from String::from to (temporary 9)",
            "Move from Some to (temporary 10)",
            "Move from make to (temporary 11)",
            "A temporary holds the value that make returns. Nothing binds it to a variable or moves it on, so Rust drops it as soon as its statement (or enclosing expression) is done",
            "A temporary holds the value that String::from returns. Nothing binds it to a variable or moves it on, so Rust drops it as soon as its statement (or enclosing expression) is done",
            "A temporary holds the value that Some returns. Nothing binds it to a variable or moves it on, so Rust drops it as soon as its statement (or enclosing expression) is done",
            "This temporary String is dropped at the end of this statement: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary String is dropped at the end of the block's final expression: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary String is dropped at the end of the condition: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary Option<String> is dropped at the end of this statement: nothing bound it to a variable or moved it on. Its resource is dropped",
        ],
        must_not_contain: &[
            "unbound temporary",
            "Move from make to (temporary 12)",
            "Move from Some to (temporary 12)",
            "Move from String::from to (temporary 12)",
        ],
    },
    TooltipExpect {
        name: "ref_param_end_shared_line",
        // `r`'s last use is also `v`'s last use. The end event (from the
        // parameter to the caller) isn't an arrow, so it used to be
        // dropped when another arrow shared the line (#416).
        must_contain: &[
            "v's immutable borrow ends",
            "v's mutable borrow ends",
        ],
        must_not_contain: &[],
    },
    // ─── Loop variables and loop-body locals (#205, #256, #261) ──────
    // Before #205 these columns had neither a start nor an end, so the
    // must_contain lines are what fails on the old plugin.
    TooltipExpect {
        name: "for_var_call_result",
        // `for c in s.chars()`: each iteration's `c` is a fresh char,
        // bound at the `for` line and out of scope at the closing `}`.
        must_contain: &[
            "c is bound to a copyable value",
            "c goes out of scope",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "c goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "for_var_range",
        must_contain: &[
            "i is bound to a copyable value",
            "i goes out of scope",
            "n goes out of scope",
        ],
        must_not_contain: &[
            "i goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "loop_body_local_drop",
        // Each iteration's `y` / `z` / `w` is dropped at the end of the
        // loop body; `moved` was moved into `consume` first.
        must_contain: &[
            "y goes out of scope. Its resource is dropped.",
            "z goes out of scope. Its resource is dropped.",
            "w goes out of scope. Its resource is dropped.",
            "moved goes out of scope. No resource is dropped.",
            "i goes out of scope",
        ],
        must_not_contain: &[
            "moved goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "for_var_string_drop",
        // `s` owns each String the loop hands it, so it is dropped at the
        // closing brace of every iteration.
        must_contain: &[
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "loop_var_in_branch",
        // `x` (a while-let binding) and `i` (a for variable) exist only
        // inside their loops, which sit in one branch of an if/else and
        // a match. They aren't live across the conditional, and
        // nothing is moved.
        must_contain: &[
            "x goes out of scope",
            "i is bound to a copyable value",
            "i goes out of scope",
        ],
        must_not_contain: &[
            "x is live in a conditional expression",
            "x was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.",
            "x goes out of scope. No resource is dropped.",
            "i is live in a conditional expression",
            "i was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.",
        ],
    },    TooltipExpect {
        name: "slice_ref_value",
        // `xs: &[i32]` and `s: &str` point to values (nothing to drop);
        // controls: `names: &[String]` and `st: &String` point to
        // resources.
        must_contain: &[
            "the value xs points to is immutably borrowed by first",
            "the value s points to is immutably borrowed by t",
            "the resource names points to is immutably borrowed by first",
            "the resource st points to is immutably borrowed by t",
        ],
        must_not_contain: &[
            "the resource xs points to",
            "the resource s points to",
        ],
    },
    TooltipExpect {
        name: "tuple_destructure_assign",
        // `(x, y) = (y, x)`, `(a, b) = (b, a)` (Strings), and
        // `(s, n) = pair()`. Before #151 the compiler's `lhs`
        // temporary had its own column and every arrow went through
        // it.
        must_contain: &[
            "Copy from y to x",
            "Copy from x to y",
            "Move from b to a",
            "Move from a to b",
            "a's resource is moved",
            "b's resource is moved",
            "a goes out of scope. Its resource is dropped.",
            "b goes out of scope. Its resource is dropped.",
            "Move from pair to s",
            "Copy from pair to n",
            "s acquires ownership of a new resource; its previous resource is dropped",
        ],
        must_not_contain: &[
            "lhs, immutable",
            "Copy from lhs to x",
            "Move from b to lhs",
            "Move from lhs to a",
            // The swap moves both old values out; neither is dropped.
            "a acquires ownership of a new resource; its previous resource is dropped",
            "b acquires ownership of a new resource; its previous resource is dropped",
            "a goes out of scope. No resource is dropped.",
            "b goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "destructure_assign_parts",
        // Rotation `(a, b, c) = (b, c, a)`; `(a, _) = (s, t)` moves t
        // into the temporary, which is dropped at the end of the
        // statement; `(p, q) = (q, p)` swaps structs and their fields
        // go along; `Pt { x, y } = Pt { x: 1, y: 2 }`.
        must_contain: &[
            "Move from b to a",
            "Move from c to b",
            "Move from a to c",
            "c goes out of scope. Its resource is dropped.",
            "Move from s to a",
            "a acquires ownership of a new resource; its previous resource is dropped",
            // t is moved into the tuple temporary, which is dropped at the
            // end of the statement; it has no column, so the move's
            // tooltip says so.
            "t's resource is moved into a temporary that is dropped at the end of this statement",
            "t goes out of scope. No resource is dropped.",
            "Move from q to p",
            "Move from p to q",
            "Move from q.name to p.name",
            "Move from p.name to q.name",
            "p.name goes out of scope. Its resource is dropped.",
            "q.name goes out of scope. Its resource is dropped.",
            "x is bound to a copyable value",
            "y is bound to a copyable value",
        ],
        must_not_contain: &[
            "lhs, immutable",
            // Before #151, the `_` part read as never moved.
            "t goes out of scope. Its resource is dropped.",
            // The drop is real and must be stated.
            // `=`: whole tooltip. The correct tooltip continues "... into a temporary that is dropped at the end of this statement".
            "=t's resource is moved",
            "b acquires ownership of a new resource; its previous resource is dropped",
            "c acquires ownership of a new resource; its previous resource is dropped",
            "p acquires ownership of a new resource; its previous resource is dropped",
            "p.name goes out of scope. No resource is dropped.",
        ],
    },
    // ─── Functions returning a reference (#171) ──────────────────────
    TooltipExpect {
        name: "fn_ret_ref_first_word",
        // Book ch. 4.3: `word` borrows `my_string` until word's last
        // use. It used to be drawn as an independent value copied out
        // of `first_word`, with my_string's borrow ending at the call.
        must_contain: &[
            "first_word reads from my_string",
            "Immutable borrow from my_string to word: first_word's signature gives the reference it returns the same lifetime as its argument",
            "my_string's resource is immutably borrowed by word; my_string is read-only while the borrow is live",
            "word immutably borrows a resource",
            "Return immutably borrowed resource from word to my_string",
        ],
        must_not_contain: &[
            ", through the reference first_word returned",
            "Copy from first_word to word",
            "word is initialized by copy from first_word",
            "Return immutably borrowed resource from word to *word",
        ],
    },
    TooltipExpect {
        name: "fn_ret_ref_longest",
        // Book ch. 10.3: one lifetime `'a` on both parameters and the
        // result, so `result` borrows from both arguments.
        must_contain: &[
            "Immutable borrow from string1 to result: longest's signature gives the reference it returns the same lifetime as both arguments, so it may point into either, and both stay borrowed while result is in use",
            "Immutable borrow from string2 to result: longest's signature gives the reference it returns the same lifetime as both arguments, so it may point into either, and both stay borrowed while result is in use",
            "string1's resource is immutably borrowed by result; string1 is read-only while the borrow is live",
            "string2's resource is immutably borrowed by result; string2 is read-only while the borrow is live",
            "Return immutably borrowed resource from result to string1",
            "Return immutably borrowed resource from result to string2",
        ],
        must_not_contain: &[
            ", through the reference longest returned",
            "Copy from longest to result",
            "result is initialized by copy from longest",
            "Return immutably borrowed resource from result to *result",
        ],
    },
    TooltipExpect {
        name: "fn_ret_ref_longest_literal",
        // Book ch. 10.3, with a string literal as second argument: it
        // has no column, and the result may point into it instead.
        must_contain: &[
            "Immutable borrow from string1 to result: longest's signature gives the reference it returns the same lifetime as both arguments (the other is a value with no column here), so string1 stays borrowed while result is in use",
            "Return immutably borrowed resource from result to string1",
        ],
        must_not_contain: &[
            ", through the reference longest returned",
            "Copy from longest to result",
            "Return immutably borrowed resource from result to *result",
        ],
    },
    TooltipExpect {
        name: "fn_ret_ref_methods",
        // `&self` methods: the result borrows the receiver, through
        // `.unwrap()` on the `Option<&T>` too. `get(&team)` borrows
        // `team` only for the call (the result's lifetime is self's).
        must_contain: &[
            "Immutable borrow from v to f: unwrap's signature gives the reference it returns the same lifetime as the value it was called on",
            "Immutable borrow from s to t: as_str's signature gives the reference it returns the same lifetime as the value it was called on",
            "Immutable borrow from scores to score: unwrap's signature gives the reference it returns the same lifetime as the value it was called on",
            "Return immutably borrowed resource from f to v",
            "Return immutably borrowed resource from score to scores",
            "get reads from team",
        ],
        must_not_contain: &[
            ", through the reference unwrap returned",
            ", through the reference as_str returned",
            "Immutable borrow from team to score",
        ],
    },
    TooltipExpect {
        name: "fn_ret_ref_mut",
        // `&mut` version: v can't be used until x's last use.
        must_contain: &[
            "Mutable borrow from v to x: first_mut's signature gives the reference it returns the same lifetime as its argument",
            "v's resource is mutably borrowed by x; v can't be used while the borrow is live",
            "x mutably borrows a resource",
            "Return mutably borrowed resource from x to v",
        ],
        must_not_contain: &[
            ", through the reference first_mut returned",
            "Move from first_mut to x",
            "x is bound to a mutable reference",
            "Return mutably borrowed resource from x to *x",
        ],
    },
    TooltipExpect {
        name: "fn_ret_ref_unrelated_arg",
        // `fn pick_first<'a, 'b>(x: &'a str, _y: &'b str) -> &'a str`:
        // only the `'a` argument stays borrowed.
        must_contain: &[
            "Immutable borrow from a to r: pick_first's signature gives the reference it returns the same lifetime as its first argument",
            "Return immutably borrowed resource from r to a",
            "pick_first reads from b",
        ],
        must_not_contain: &[
            ", through the reference pick_first returned",
            "Copy from pick_first to r",
            "Immutable borrow from b to r",
            "Return immutably borrowed resource from r to b",
        ],
    },
    TooltipExpect {
        name: "fn_ret_ref_iter_next",
        // `x` borrows `v` (through the iterator), not the iterator
        // `it`. `it` holds the borrow of `v` (#336), so x's lender is
        // `v`.
        must_contain: &[
            "Immutable borrow from v to x: unwrap's signature gives the reference it returns the same lifetime as the value it was called on",
            "Return immutably borrowed resource from x to v",
        ],
        must_not_contain: &[
            ", through the reference unwrap returned",
            "x is initialized by copy from unwrap",
            "Immutable borrow from it to x",
            "it's resource is immutably borrowed by x; it is read-only while the borrow is live",
            "Return immutably borrowed resource from x to it",
        ],
    },
    TooltipExpect {
        name: "fn_ret_ref_reassign",
        // Reassigning `r` ends its borrow of both of longest's
        // arguments and starts a borrow of `c`.
        must_contain: &[
            "Return immutably borrowed resource from r to a",
            "Return immutably borrowed resource from r to b",
            "Immutable borrow from c to r: first's signature gives the reference it returns the same lifetime as its argument",
            "Return immutably borrowed resource from r to c",
        ],
        must_not_contain: &[
            ", through the reference first returned",
            "Copy from longest to r",
            "Copy from first to r",
            "Return immutably borrowed resource from r to *r",
        ],
    },
    TooltipExpect {
        name: "fn_ret_ref_struct_field",
        // A struct field initialised from a call returning a reference.
        must_contain: &[
            "Immutable borrow from novel to e.part: first_sentence's signature gives the reference it returns the same lifetime as its argument",
            "Return immutably borrowed resource from e.part to novel",
        ],
        must_not_contain: &[
            ", through the reference first_sentence returned",
            "Copy from first_sentence to e.part",
            "Return immutably borrowed resource from e.part to *e.part",
        ],
    },
    TooltipExpect {
        name: "if_let_scrutinee_move",
        // `v.into_iter()` in the scrutinee moves `v` before the branch is
        // chosen (#450). It was drawn inside the first branch, which
        // returns: the merge ignored it and `v` was said to be dropped.
        must_contain: &[
            "v's resource is moved",
            "v goes out of scope. No resource is dropped.",
            // The `IntoIter` the scrutinee builds outlives the pattern
            // match (#469): the `return` in the first branch drops it
            // (#410), and the `else` path drops it before the `else` block.
            "This temporary IntoIter<String> is dropped here, where `return` leaves early: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary IntoIter<String> is dropped before the `else` block of this `if let` runs, or when the block it matched ends: nothing bound it to a variable or moved it on. Its resource is dropped",
        ],
        must_not_contain: &[
            "v is live in a conditional expression ",
            "v goes out of scope. Its resource is dropped.",
            "v goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
            "v was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when v goes out of scope.",
            "(temporary) goes out of scope at the end of the block: a let borrowed it",
            "dropped at the end of the condition",
            // Stated once, with #410's wording.
            "is left early, while this temporary is still alive",
        ],
    },
    TooltipExpect {
        name: "if_let_scrutinee_move_else_move",
        // `v` moved by the scrutinee (unconditional); `t` moved in the
        // `else` only (conditional).
        must_contain: &[
            "v's resource is moved",
            "v goes out of scope. No resource is dropped.",
            "t is live in a conditional expression ",
            "t's resource is moved into drop, which drops it",
            "t goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "v is live in a conditional expression ",
            "v goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
    },
    TooltipExpect {
        name: "match_scrutinee_move",
        must_contain: &[
            "v's resource is moved",
            "v goes out of scope. No resource is dropped.",
            "w's resource is moved",
            "w goes out of scope. No resource is dropped.",
            "Partial move from next to s",
        ],
        must_not_contain: &[
            // The two matches' `s`s are never in scope together (#598).
            "s₂",
            "v is live in a conditional expression ",
            "w is live in a conditional expression ",
            "v goes out of scope. Its resource is dropped.",
            "w goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "match_on_temporary_one_line",
        // Used to panic (#459). `c` is moved into the temporary on the
        // `match` line, whatever the arm.
        must_contain: &[
            "c's resource is moved",
            "c goes out of scope. No resource is dropped.",
            "d's resource is moved",
            "d goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "c is live in a conditional expression ",
            "c goes out of scope. Its resource is dropped.",
            "d goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "match_on_temporary_variants",
        // Used to panic (#459).
        must_contain: &[
            "a's resource is moved",
            "b's resource is moved",
            "c's resource is moved",
            "d's resource is moved",
            "x's resource is moved into drop, which drops it",
            "n is bound to a copyable value",
            "a goes out of scope. No resource is dropped.",
            "d goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "a goes out of scope. Its resource is dropped.",
            "b goes out of scope. Its resource is dropped.",
            "c goes out of scope. Its resource is dropped.",
            "d goes out of scope. Its resource is dropped.",
        ],
    },
    // ─── A borrow stored in a field through a local `&mut` (#512) ────
    TooltipExpect {
        name: "borrow_stored_through_local_ref",
        // `rh.r = &t` with `rh = &mut hold` stores `&t` in hold.r. It
        // drew nothing (the borrow was lost because rh's own borrow of
        // hold ends on that line). The borrow is named by the field it
        // is stored in, which has a column. Where it ends (hold's last
        // use, like s's) is checked in
        // `borrows_stored_in_a_struct_end_at_its_last_use`.
        must_contain: &[
            "t's resource is immutably borrowed by hold.r; t is read-only while the borrow is live",
            "t's resource is no longer immutably borrowed",
            "s's resource is no longer immutably borrowed",
            "b's resource is immutably borrowed by o.inner.r; b is read-only while the borrow is live",
            "b's resource is no longer immutably borrowed",
        ],
        must_not_contain: &[
            // The place as written, when the field has a column.
            "t's resource is immutably borrowed by rh.r; t is read-only while the borrow is live",
            "b's resource is immutably borrowed by (*ri).r; b is read-only while the borrow is live",
        ],
    },
    // ─── A reference field overwritten directly (#531) ───────────────
    TooltipExpect {
        name: "borrow_field_overwrite_direct",
        // `hold.r = &t` keeps s borrowed until hold's last use (rustc
        // rejects a `&mut s` in between), so s's borrow ends on the
        // println!'s row, with t's; see
        // `direct_field_overwrite_keeps_old_borrow_until_last_use`.
        // A reference variable (`r = &y`) is dead between the writes:
        // x's borrow ends there.
        must_contain: &[
            "t's resource is immutably borrowed by hold.r; t is read-only while the borrow is live",
            "s's resource is no longer immutably borrowed",
            "b's resource is immutably borrowed by o.inner.r; b is read-only while the borrow is live",
            "a's resource is no longer immutably borrowed",
            "x's resource is no longer immutably borrowed",
        ],
        must_not_contain: &[
            // The old borrow handed back at the write.
            "Return immutably borrowed resource from hold.r to s",
            "Return immutably borrowed resource from o.inner.r to a",
        ],
    },
    // ─── A borrow a call may store, per its signature (#513) ─────────
    TooltipExpect {
        name: "borrow_stored_by_call",
        // `point(&mut hold, &t)` with `fn point<'a>(h: &mut H<'a>, t:
        // &'a String)`: t stays borrowed while hold is in use. The call
        // dot used to say "point reads from t" and, on the same dot,
        // "t's resource is no longer immutably borrowed" (checked in
        // `borrows_stored_in_a_struct_end_at_its_last_use`). `look`
        // can't store its argument: u's borrow ends at that call.
        must_contain: &[
            "t's resource is immutably borrowed by point; t is read-only while the borrow is live",
            "point reads from t; point's signature lets it store this reference in hold, so t stays borrowed while hold is in use",
            "set reads from s; set's signature lets it store this reference in h2, so s stays borrowed while h2 is in use",
            "put reads from/writes to b; put's signature lets it store this reference in m, so b stays borrowed while m is in use",
            "look reads from u",
            "t's resource is no longer immutably borrowed",
            "b's resource is no longer mutably borrowed",
        ],
        must_not_contain: &[
            // Main's call dots, which also ended the borrow there.
            // `=`: whole tooltip. The correct tooltips continue "; point's signature lets it store this reference in ...".
            "=point reads from t",
            "=set reads from s",
            "=put reads from/writes to b",
        ],
    },
    // #371: the borrow and its end are on one line. The texts were
    // right before too; where the end is drawn is checked by
    // `same_line_borrow_ends_below_its_use`.
    TooltipExpect {
        name: "match_arm_macro_ref_binding",
        must_contain: &[
            "Partial immutable borrow from opt to s",
            "s's immutable borrow ends",
            "Return immutably borrowed resource from s to opt",
            "opt's resource is no longer immutably borrowed",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "match_arm_call_mut_ref_binding",
        must_contain: &[
            "Partial Mutable borrow from opt to s",
            "s's mutable borrow ends",
            "Return mutably borrowed resource from s to opt",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "iflet_one_line_ref_binding",
        must_contain: &[
            "Partial immutable borrow from opt to s",
            "s's immutable borrow ends",
            "Return immutably borrowed resource from s to opt",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "if_let_binding_borrow",
        // #382: `if let Some(a) = &x` borrows the String in `x`; nothing
        // is moved or copied out of it. The geometry is checked by
        // `if_let_binding_is_drawn_at_the_start_of_its_arm`.
        must_contain: &[
            "Partial immutable borrow from x to a",
            "x's resource is immutably borrowed by a; x is read-only while the borrow is live",
            "a immutably borrows a resource",
        ],
        must_not_contain: &["Move from x to a", "x's resource is moved", "Copy from x to a"],
    },
    TooltipExpect {
        name: "if_let_binding_copy",
        // #382: the i32 in `y: Option<i32>` is copied into `b`; `y`
        // keeps its value.
        must_contain: &["Copy from y to b", "y's value is copied"],
        must_not_contain: &["Move from y to b", "y's resource is moved"],
    },
    TooltipExpect {
        name: "if_let_binding_move",
        // #382: the String is moved out of `name` / `opt` into the
        // binding, only on the path where the pattern matched.
        must_contain: &[
            "Move from name to n",
            "name's resource is moved",
            "Move from opt to s",
            "opt's resource is moved",
        ],
        must_not_contain: &["Copy from name to n", "Copy from opt to s"],
    },
    TooltipExpect {
        name: "if_let_copy_and_move_out_of_scrutinee",
        // #496: `n` is copied out of `msg` and `s` is moved out of it,
        // only when the pattern matches. The copy leaves `msg` as it was;
        // the move is what makes `msg` maybe-moved afterwards.
        must_contain: &[
            "Copy from msg to n",
            "n is initialized with a copy of msg",
            "Move from msg to s",
            "msg's resource is moved",
            "msg was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when msg goes out of scope.",
        ],
        must_not_contain: &["Move from msg to n", "Copy from msg to s", "msg's resource is copied"],
    },
    TooltipExpect {
        name: "while_let_binding_kinds",
        // #382: the same three kinds of binding in `while let`.
        must_contain: &[
            "Partial immutable borrow from x to a",
            "Copy from y to b",
            "Move from o to s",
        ],
        must_not_contain: &["Move from x to a", "Move from y to b", "Copy from o to s"],
    },
    // #495: a reference declared before an if/match whose last use is
    // in one arm. Its borrow used to end on the parent columns, in the
    // middle of the diamond; that was about *where* the end was drawn,
    // so `outer_borrow_end_is_drawn_in_each_arm` pins it. These pin the
    // texts of the per-arm ends and of the reference's join.
    TooltipExpect {
        name: "outer_ref_end_in_match_arm",
        must_contain: &[
            "Return immutably borrowed resource from r to s",
            "r's borrow has ended on this path",
            "r's borrow ended in each branch above",
        ],
        // The join used to say only that r isn't moved, and r's column
        // ran on past it as if r still borrowed s.
        must_not_contain: &["r isn't moved in any of the branches above"],
    },
    TooltipExpect {
        name: "outer_ref_end_in_if_arm",
        must_contain: &[
            "Return immutably borrowed resource from r to s",
            "r's borrow has ended on this path",
            "r's borrow ended in each branch above",
        ],
        must_not_contain: &["r isn't moved in any of the branches above"],
    },
    TooltipExpect {
        name: "outer_mut_ref_end_in_if_arm",
        must_contain: &[
            "Return mutably borrowed resource from r to s",
            "s's resource is no longer mutably borrowed",
            "r's borrow has ended on this path",
            "r's borrow ended in each branch above",
        ],
        must_not_contain: &["r isn't moved in any of the branches above"],
    },
    TooltipExpect {
        name: "outer_ref_end_in_nested_if",
        must_contain: &[
            "Return immutably borrowed resource from r to s",
            "r's borrow has ended on this path",
            "r's borrow ended in each branch above",
        ],
        must_not_contain: &["r isn't moved in any of the branches above"],
    },
    TooltipExpect {
        name: "if_arm_on_split_line",
        // #508: the move happens only in the `if` arm, so the `else`
        // arm's use of `s` is a borrow, not a use after a move.
        // Geometry: `an_arm_whose_first_event_is_on_the_split_line_starts_there`.
        must_contain: &["Move from s to take", "s's resource is moved"],
        must_not_contain: &["Copy from s to take", "s's value is copied"],
    },
    TooltipExpect {
        name: "match_arm_on_split_line",
        must_contain: &["Move from s to take", "s's resource is moved"],
        must_not_contain: &["Copy from s to take", "s's value is copied"],
    },
    // ─── `if let` / `while let` chains (#198, #280) ────────────────
    // Before the fix the chain's bindings weren't registered: no
    // columns for them, and their uses in the body were skipped.
    TooltipExpect {
        name: "edition2024/if_let_chain",
        must_contain: &[
            "Partial immutable borrow from x to a",
            "Copy from y to b",
            "show reads from a",
            "Copy from b to take",
            "a goes out of scope",
            "b goes out of scope",
            "x goes out of scope. Its resource is dropped (including the String it holds, if any).",
        ],
        // Copying the i32 out of `y` changes nothing about `y`, so `y`
        // doesn't branch (#583, as #496 for a lone `if let`).
        must_not_contain: &[
            "=x goes out of scope. Its resource is dropped.",
            "y is live in a conditional expression",
            "y isn't moved in any of the branches above",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_move",
        // The last `let` moves the String out of `x` (into `a`, then into
        // `consume`) only when the whole condition holds. When `n > 0`
        // is false, `x` keeps its String and drops it at the end, so
        // neither "No resource is dropped" nor an unconditional drop is
        // right.
        must_contain: &[
            "Move from x to a",
            "Move from a to consume",
            "x goes out of scope. Its resource (including the String it holds, if any) is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "=x goes out of scope. Its resource is dropped.",
            "x goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_first_part",
        // The chain's first part, `wrap(s)`, runs whether or not the
        // rest holds, so `s`'s move is drawn before the split and `s`
        // has nothing to drop. (Before, the move was drawn in the if
        // branch and the program was refused; main shows no `w`.)
        must_contain: &[
            "Move from s to wrap",
            "s goes out of scope. No resource is dropped.",
            "Move from wrap to w",
            "w goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "edition2024/while_let_chain",
        must_contain: &[
            "Copy from pop to t",
            "Copy from t to take",
            "t goes out of scope",
        ],
        must_not_contain: &[],
    },
    // #335: a reassignment after a conditional that moved the value on
    // some paths only drops the old value only on the others (drop
    // flag), and the merge says the drop happens at that reassignment.
    TooltipExpect {
        name: "reassign_after_maybe_move",
        must_contain: &[
            "s acquires ownership of a new resource. Its previous resource is dropped here only if \
             no branch above moved it (Rust tracks this with a runtime drop flag).",
            "s was moved in at least one branch above, so it can't be used after the branches until \
             it's assigned again. If the branch that ran didn't move it, its resource is dropped \
             when s is assigned again.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s acquires ownership of a new resource; its previous resource is dropped",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
        ],
    },
    TooltipExpect {
        name: "reassign_after_maybe_move_no_else",
        must_contain: &[
            "s acquires ownership of a new resource. Its previous resource is dropped here only if \
             no branch above moved it (Rust tracks this with a runtime drop flag).",
            "s was moved in at least one branch above, so it can't be used after the branches until \
             it's assigned again. If the branch that ran didn't move it, its resource is dropped \
             when s is assigned again.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s acquires ownership of a new resource; its previous resource is dropped",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
        ],
    },
    TooltipExpect {
        name: "reassign_after_maybe_move_match",
        must_contain: &[
            "s acquires ownership of a new resource. Its previous resource is dropped here only if \
             no branch above moved it (Rust tracks this with a runtime drop flag).",
            "s was moved in at least one branch above, so it can't be used after the branches until \
             it's assigned again. If the branch that ran didn't move it, its resource is dropped \
             when s is assigned again.",
            "s goes out of scope. Its resource is dropped.",
            // u is moved in no arm: its old value is dropped.
            "u acquires ownership of a new resource; its previous resource is dropped",
        ],
        must_not_contain: &[
            "s acquires ownership of a new resource; its previous resource is dropped",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            // t is moved in every arm: there's nothing to drop.
            "t acquires ownership of a new resource; its previous resource is dropped",
            "t acquires ownership of a new resource. Its previous resource is dropped here only if \
             no branch above moved it (Rust tracks this with a runtime drop flag).",
        ],
    },
    TooltipExpect {
        name: "reassign_after_nested_maybe_move",
        must_contain: &[
            "s acquires ownership of a new resource. Its previous resource is dropped here only if \
             no branch above moved it (Rust tracks this with a runtime drop flag).",
            "s was moved in at least one branch above, so it can't be used after the branches until \
             it's assigned again. If the branch that ran didn't move it, its resource is dropped \
             when s is assigned again.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "s acquires ownership of a new resource; its previous resource is dropped",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
        ],
    },
    TooltipExpect {
        name: "mutref_moved_on",
        // r → r2 → r3 moves one `&mut`; only r3, the last holder,
        // ends the borrow and returns it to s. q is moved to q2, so
        // reassigning q later ends nothing of q's old borrow of t.
        must_contain: &[
            "Move from r to r2",
            "Move from r2 to r3",
            "Return mutably borrowed resource from r3 to s",
            "Move from q to q2",
            "Return mutably borrowed resource from q2 to t",
            "Return mutably borrowed resource from q to s",
        ],
        must_not_contain: &[
            "Return mutably borrowed resource from r to s",
            "r's mutable borrow ends",
            "r2's mutable borrow ends",
            "Return mutably borrowed resource from r2 to s",
            "Return mutably borrowed resource from q to t",
        ],
    },
    TooltipExpect {
        name: "mutref_coerced_reborrow",
        // `let r2: &mut String = r;` and `Holder { r: a }` reborrow
        // (rustc inserts `&mut *r`): r / a are usable again after the
        // new reference's last use, which gives the borrow back to
        // *r / *a, like the explicit `&mut *r`.
        must_contain: &[
            "Mutable reborrow from r to r2",
            "the resource r points to is mutably borrowed by r2 through r; r can't be used while the borrow is live",
            "Return mutably borrowed resource from r2 to *r",
            "Mutable reborrow from a to h.r",
            "Return mutably borrowed resource from h.r to *a",
        ],
        must_not_contain: &[
            "Move from r to r2",
            "r's resource is moved",
            "Move from a to h.r",
            "a's resource is moved",
            "Return mutably borrowed resource from h.r to t",
        ],
    },
    // ─── Reads through a reference stored in an element (#438) ──────
    TooltipExpect {
        name: "read_through_element_ref",
        // `let n = *slots[0];` copies the i32 the reference in slots[0]
        // points to; slots is a Vec, not a reference, so "Copy from
        // *slots to n" was false.
        must_contain: &[
            "Copy from *slots[0] to n",
            "n is initialized with a copy of *slots[0]",
            "the resource slots[0] points to is copied",
            "Copy from *hs[0].r to m",
            "m is initialized with a copy of *hs[0].r",
            "the resource hs[0].r points to is copied",
            "Copy from *boxes[0] to k",
            "k is initialized with a copy of *boxes[0]",
            "the resource boxes[0] points to is copied",
            "Copy from *grid[0][0] to total",
            "total is assigned a copy of *grid[0][0]",
            "the resource grid[0][0] points to is copied",
        ],
        must_not_contain: &[
            "Copy from *slots to n",
            "*slots's resource is copied",
            "m is initialized by copy from Anonymous resource",
            "Copy from *boxes to k",
            "*boxes's resource is copied",
            "Copy from *grid to total",
            "*grid's resource is copied",
        ],
    },
    TooltipExpect {
        name: "fn_ret_ref_arg_lists",
        // The arrow says which arguments the signature ties the result
        // to. Elision ties a `&self` method's result to the receiver
        // only, not to a reference argument too (`pick`); an explicit
        // lifetime can tie it to the argument only (`pick_arg`).
        must_contain: &[
            "Immutable borrow from w to r1: pick's signature gives the reference it returns the same lifetime as the value it was called on",
            "Immutable borrow from o to r2: pick_arg's signature gives the reference it returns the same lifetime as its argument",
            "Immutable borrow from w to r3: both's signature gives the reference it returns the same lifetime as the value it was called on and its argument, so it may point into either, and both stay borrowed while r3 is in use",
            "Immutable borrow from b to r4: three's signature gives the reference it returns the same lifetime as all three arguments, so it may point into any of them, and all of them stay borrowed while r4 is in use",
            "Immutable borrow from c to r5: first_third's signature gives the reference it returns the same lifetime as its first and third arguments, so it may point into either, and both stay borrowed while r5 is in use",
            "Immutable borrow from c to r6: three's signature gives the reference it returns the same lifetime as all three arguments (one of them is a value with no column here), so it may point into any of them, and a and c stay borrowed while r6 is in use",
        ],
        must_not_contain: &[
            ", through the reference pick returned",
            ", through the reference pick_arg returned",
            ", through the reference both returned",
            ", through the reference three returned",
            ", through the reference first_third returned",
            "Immutable borrow from o to r1",
            "Immutable borrow from b to r5",
            ", through the reference",
        ],
    },
    // ─── Explicit `return` (#318) and the merge after a branch that
    // leaves early (#344, #350) ──────────────────────────────────────
    TooltipExpect {
        name: "return_early_in_if",
        // `if n > 0 { return s; }`: `s` goes to the caller on that
        // path, which never reaches the end of `pick`; on the path
        // that does, `s` is still owned and dropped there. The merge
        // says why `s` is still owned after a branch that moved it
        // (was "isn't moved in any of the branches above [that reach
        // this point]", which reads as if no branch moved it).
        must_contain: &[
            "s's resource is moved to the caller",
            "s still owns its resource here: the branch that moved s returned before reaching this point",
            "s goes out of scope. Its resource is dropped.",
            "t's resource is moved to the caller",
        ],
        must_not_contain: &[
            "s isn't moved in any of the branches above",
            "s isn't moved in any of the branches above that reach this point",
            "s goes out of scope. No resource is dropped.",
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
        ],
    },
    TooltipExpect {
        name: "return_explicit_last",
        // `return s;` / `return n` / `return User { name, age };` as
        // the last statement behave like a tail expression.
        must_contain: &[
            "s's resource is moved to the caller",
            "s goes out of scope. No resource is dropped.",
            "n's value is copied to the caller",
            "name's resource is moved to the caller",
            "age's value is copied to the caller",
            "name goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "s goes out of scope. Its resource is dropped.",
            // `=`: whole tooltip. "u.name goes out of scope. Its resource is dropped." in main is correct.
            "=name goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "return_in_match_arm",
        // `0 => return s` moves `s` to the caller in that arm; the
        // other arm reaches the code after the match with `s` owned,
        // and the tail `s` moves it to the caller there. `k` is bound
        // in the one arm that reaches the merge.
        must_contain: &[
            "s's resource is moved to the caller",
            "s still owns its resource here: the branch that moved s returned before reaching this point",
            "k is bound to a value in every branch that reaches this point (the 0 arm returned)",
            "n isn't moved in any of the branches above",
            "s goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "s isn't moved in any of the branches above",
            "s isn't moved in any of the branches above that reach this point",
            "n isn't moved in any of the branches above that reach this point",
            "k is bound to a value (in all branches above that reach this point)",
        ],
    },
    TooltipExpect {
        name: "loop_break_after_move",
        // `if … { take(s); break; }` in a loop: that branch never
        // reaches the merge after its `if`, so there `s` is still
        // owned (was "s was moved in at least one branch above, so it
        // can't be used after the branches …", false inside the loop,
        // and on main "s isn't moved in any of the branches above" and
        // a plain drop at scope end). The move counts where the loop
        // ends: `s`, `t` (a bare `loop` whose only way out is that
        // `break`) are moved after it, so nothing is dropped and there
        // is no drop flag; `u` (a `while`) and `w` (a second `break`)
        // only may have been moved.
        must_contain: &[
            "s still owns its resource here: the branch that moved s broke out of the loop before reaching this point",
            "s was moved before the loop ended: the only way out of the loop is a break that \
             comes after s is moved, so s can't be used after the loop",
            "s goes out of scope. No resource is dropped.",
            "t still owns its resource here: the branch that moved t broke out of the loop before reaching this point",
            "t was moved before the loop ended: the only way out of the loop is a break that \
             comes after t is moved, so t can't be used after the loop",
            "t goes out of scope. No resource is dropped.",
            "u still owns its resource here: the branch that moved u broke out of the loop before reaching this point",
            "u may have been moved: the loop can end through a break that comes after u is \
             moved, so u can't be used after the loop",
            "u goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
            "w still owns its resource here: the branch that moved w broke out of the loop before reaching this point",
            "w may have been moved: the loop can end through a break that comes after w is \
             moved, so w can't be used after the loop",
            "w goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "s was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when s goes out of scope.",
            "t was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when t goes out of scope.",
            "u was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when u goes out of scope.",
            "w was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when w goes out of scope.",
            "s isn't moved in any of the branches above",
            "t isn't moved in any of the branches above",
            "u isn't moved in any of the branches above",
            "w isn't moved in any of the branches above",
            // `=`: whole tooltip. `take(_s)` drops its parameter: "_s goes out of scope. Its resource is dropped." is correct.
            "=s goes out of scope. Its resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
            "s goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
            "t goes out of scope. Its resource is dropped here only if no branch above moved it \
             (Rust tracks this with a runtime drop flag).",
            "u goes out of scope. Its resource is dropped.",
            "w goes out of scope. Its resource is dropped.",
            "u goes out of scope. No resource is dropped.",
            "w goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "join_after_early_exit",
        // The merge after branches that moved a value and then left
        // early says how they left: panicked, continued, returned
        // (two arms), return and break mixed; a `&mut` that was
        // returned is still held. All were "isn't moved in any of the
        // branches above", as if nothing had moved it.
        must_contain: &[
            "s still owns its resource here: the branch that moved s panicked before reaching this point",
            "c still owns its resource here: the branch that moved c continued to the next iteration before reaching this point",
            "t still owns its resource here: the branches that moved t returned before reaching this point",
            "m still owns its resource here: the branches that moved m left early (return, break) before reaching this point",
            "r still holds its mutable reference here: the branch that moved r returned before \
             reaching this point",
            "s goes out of scope. Its resource is dropped.",
            "total isn't moved in any of the branches above",
        ],
        must_not_contain: &[
            "s isn't moved in any of the branches above",
            "c isn't moved in any of the branches above",
            "t isn't moved in any of the branches above",
            "m isn't moved in any of the branches above",
            "r isn't moved in any of the branches above",
            "r still owns its resource here: the branch that moved r returned before reaching this point",
            "m was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when m goes out of scope.",
        ],
    },
    TooltipExpect {
        name: "return_early_no_drop",
        // `Pt` isn't Copy but has no drop glue: a value, not a resource (#484).
        must_contain: &[
            "p still owns its value here: the branch that moved p returned before reaching this point",
        ],
        must_not_contain: &[
            "p still owns its resource here",
        ],
    },
    TooltipExpect {
        name: "field_write_through_ref",
        // A field write through a reference is drawn on the reference's
        // column (owned by the caller / lender); the reference never
        // owns the field (#147). An assignment `p.x = e` reads like a
        // plain assignment with the place as the subject (2026-11-05
        // decision): "p.x is assigned a new value" for a Copy field
        // written from a literal or expression, "p.x is assigned a copy
        // of z" when it is copied from a named place,
        // "p.name acquires ownership of a new resource; its previous
        // resource is dropped" for a String. No "(moved from" /
        // "(copied from": the arrow says it. The in-place updates
        // `p.name += ..` / `p.inner.v += 1` keep "modified through".
        must_contain: &[
            "p.x is assigned a new value",
            "Copy from z to p.x",
            "p.x is assigned a copy of z",
            "Move from a to p.name",
            "p.name acquires ownership of a new resource; its previous resource is dropped",
            "the resource p points to is modified through p.name",
            "the resource p points to is modified through p.inner.v",
            "(*p).x is assigned a new value",
            "t.0 is assigned a new value",
            "Move from String::from to t.1",
            "t.1 acquires ownership of a new resource; its previous resource is dropped",
            "r.x is assigned a new value",
            "w.r.x is assigned a new value",
            "self.c is assigned a new value",
        ],
        must_not_contain: &[
            "p.x acquires ownership of a resource",
            "p.name acquires ownership of a resource",
            "self.c acquires ownership of a resource",
            "p.x is the owner of the resource",
            "self.c is the owner of the resource",
            "the resource p points to is modified through *p",
            // An assignment is not "modified through".
            "the resource p points to is modified through p.x",
            "the resource p points to is modified through (*p).x",
            "the resource t points to is modified through t.0",
            "the resource t points to is modified through t.1",
            "the resource r points to is modified through r.x",
            "the resource w.r points to is modified through w.r.x",
            "the resource self points to is modified through self.c",
            "its previous value is dropped",
            "(copied from",
            "(moved from",
        ],
    },
    TooltipExpect {
        name: "field_write_through_ref_branch",
        // The same writes inside if / match arms, plus `*s = …` in both
        // arms of an if/else, which used to panic ("Shouldn't be
        // happening" in fetch_timeline).
        must_contain: &[
            "p.x is assigned a new value",
            "Move from String::from to p.name",
            "Move from v to p.name",
            "p.name acquires ownership of a new resource; its previous resource is dropped",
            "Move from String::from to *s",
            "*s acquires ownership of a new resource; its previous resource is dropped",
        ],
        must_not_contain: &[
            "p.x acquires ownership of a resource",
            "p.name acquires ownership of a resource",
            "the resource p points to is modified through p.x",
            "its previous value is dropped",
            "the resource s points to is modified through *s",
            "(copied from",
            "(moved from",
        ],
    },
    TooltipExpect {
        name: "deref_assign_wording",
        // `*r = e` assigns to the place `*r` (2026-11-05 decision): a Copy
        // value is "assigned a new value", an owned value says its
        // previous resource is dropped (a Box's too), a non-Copy value
        // with nothing to drop just acquires the new one. No
        // "(copied from" / "(moved from": the arrow says it. The
        // in-place update `*r += 1` keeps "modified through".
        must_contain: &[
            "Copy from y to *r",
            "*r is assigned a copy of y",
            "*r is assigned a new value",
            "Move from t to *ra",
            "*ra acquires ownership of a new resource; its previous resource is dropped",
            "Move from u to *b",
            "*b acquires ownership of a new resource; its previous resource is dropped",
            "Move from q2 to *rq",
            // Q is a struct of integers: nothing to drop, so a plain
            // assignment, not a resource (#484).
            "*rq is assigned a new value",
            // r: &mut i32 points to a value (#312).
            "the value r points to is modified through *r",
        ],
        must_not_contain: &[
            "(copied from",
            "(moved from",
            "the resource r points to is modified through *r",
            "the resource ra points to is modified through *ra",
            "the resource b points to is modified through *b",
            "the resource rq points to is modified through *rq",
            "*r acquires ownership",
            "*rq acquires ownership of a resource",
        ],
    },
    TooltipExpect {
        name: "field_compound_assign_through_ref",
        // `p.x += k` / `r.x += k` / `p.name += "x"` through a reference
        // update the borrowed struct in place: "modified through", as
        // for `*r += k`. The copy of k is the arrow's job; the dot used
        // to add "(copied from k)" (#260). Nothing is assigned or
        // dropped.
        must_contain: &[
            // The operator is called with k, as for `*r += k` on main.
            "Copy from k to +=",
            "the resource p points to is modified through p.x",
            "the resource p points to is modified through p.name",
            "the resource r points to is modified through r.x",
        ],
        // (Tooltips are matched whole, so these are the full old texts.)
        must_not_contain: &[
            "the resource p points to is modified through p.x (copied from k)",
            "the resource r points to is modified through r.x (copied from k)",
            "p.x is assigned a copy of k",
            "r.x is assigned a copy of k",
            "p.name acquires ownership of a new resource; its previous resource is dropped",
        ],
    },

    // ─── User macros (#342) ──────────────────────────────────────────
    // A user macro that only forwards to a formatting macro still
    // renders (#206); one that touches no variable is untouched.
    TooltipExpect {
        name: "user_macro_forwards",
        must_contain: &[
            "show! reads from w through an implicit reference, so w isn't moved",
            "show_block! reads from v through an implicit reference, so v isn't moved",
            "println! reads from n through an implicit reference, so n isn't moved",
        ],
        must_not_contain: &[],
    },
    // ─── References chosen by an `if` / `match` (#337) ───────────────
    TooltipExpect {
        name: "branch_call_ref_if",
        // #337's repro: `t` stays borrowed until x's last use (line 6),
        // not just during the call (the `t.len()` in the condition is a
        // separate, short borrow).
        must_contain: &[
            "Immutable borrow from t to x: first_word's signature gives the reference it returns the same lifetime as its argument",
            "Return immutably borrowed resource from x to t",
        ],
        must_not_contain: &[
            ", through the reference first_word returned",
            "x is initialized by copy from first_word",
            "Copy from first_word to x",
            "Return immutably borrowed resource from x to *x",
        ],
    },
    TooltipExpect {
        name: "branch_call_ref_match",
        // Each arm draws its own borrow. After the match, rustc treats
        // `w` as possibly borrowing either, so both stay borrowed until
        // w's last use.
        must_contain: &[
            "Immutable borrow from a to w: first_word's signature gives the reference it returns the same lifetime as its argument",
            "Immutable borrow from b to w: first_word's signature gives the reference it returns the same lifetime as its argument",
            "Return immutably borrowed resource from w to a",
            "Return immutably borrowed resource from w to b",
        ],
        must_not_contain: &[
            ", through the reference first_word returned",
            "w is initialized by copy from first_word",
            "Return immutably borrowed resource from w to *w",
        ],
    },
    TooltipExpect {
        name: "branch_call_ref_method",
        // Method chains in a branch (`v.first().unwrap()`, `s.as_str()`)
        // next to a literal / promoted constant in the other branch.
        must_contain: &[
            "Immutable borrow from v to f: unwrap's signature gives the reference it returns the same lifetime as the value it was called on",
            "Immutable borrow from s to t: as_str's signature gives the reference it returns the same lifetime as the value it was called on",
            "Return immutably borrowed resource from f to v",
            "Return immutably borrowed resource from t to s",
        ],
        must_not_contain: &[
            ", through the reference unwrap returned",
            ", through the reference as_str returned",
            "f is initialized by copy from Anonymous resource",
            "t is initialized by copy from Anonymous resource",
            "Return immutably borrowed resource from f to *f",
            "Return immutably borrowed resource from t to *t",
        ],
    },
    TooltipExpect {
        name: "branch_ref_if_addr",
        // `if c { &a } else { &b }`: the borrows were drawn, but r's end
        // gave nothing back.
        must_contain: &[
            "Immutable borrow from a to r",
            "Immutable borrow from b to r",
            "Return immutably borrowed resource from r to a",
            "Return immutably borrowed resource from r to b",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from r to *r",
        ],
    },
    TooltipExpect {
        name: "closure_arg_ref_captures",
        // A closure literal passed to a call borrows what it captures
        // by reference for as long as the closure lives (#298). For
        // `apply`, `run` and `Option::map` the closure is gone when
        // the call returns, so the borrow is drawn like a `&k`
        // argument. `keep` hands the closure back (it lives on in
        // `g`), so the borrow of `s` doesn't end at the call and
        // isn't drawn.
        must_contain: &[
            // k and count are i32: a value, not a resource (#484).
            "k's value is immutably borrowed by the closure passed to apply; k is read-only while the borrow is live",
            "the closure passed to apply reads from k",
            "count's value is mutably borrowed by the closure passed to run; count can't be used while the borrow is live",
            "the closure passed to run reads from/writes to count",
            "the closure passed to map reads from k",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "k's resource is immutably borrowed by the closure passed to apply; k is read-only while the borrow is live",
            "count's resource is mutably borrowed by the closure passed to run; count can't be used while the borrow is live",
            "the closure passed to keep reads from s",
            "s's resource is immutably borrowed by the closure passed to keep; s is read-only while the borrow is live",
            "k's resource is moved",
            "count's resource is moved",
        ],
    },
    // ─── Values that hold a borrow (#336) ────────────────────────────
    TooltipExpect {
        name: "holder_iter_get",
        // `it` (an Iter) and `third` (an Option<&i32>) aren't
        // references, but each holds a borrow of `v` until its last use
        // (rustc rejects `v.push(4)` before line 6). `x` is a copy of a
        // reference `it` hands out, so it borrows `v` too.
        must_contain: &[
            "Immutable borrow from v to it: iter's signature gives the value it returns the same lifetime as the value it was called on; it isn't a reference, but its value holds one",
            "Immutable borrow from v to third: get's signature gives the value it returns the same lifetime as the value it was called on; third isn't a reference, but its value holds one",
            "Immutable borrow from v to x: unwrap's signature gives the reference it returns the same lifetime as the value it was called on",
            "it holds a value that immutably borrows v's resource (it isn't a reference itself)",
            "it holds a value that contains a reference (it isn't a reference itself)",
            "Return immutably borrowed resource from it to v",
            "Return immutably borrowed resource from third to v",
            "Return immutably borrowed resource from x to v",
            // An `Iter` needs no drop, so nothing is claimed dropped (#484).
            "it goes out of scope",
        ],
        must_not_contain: &[
            "it goes out of scope. Its value is dropped, but not the data it borrowed.",
            ", through the value iter returned",
            ", through the value get returned",
            ", through the reference unwrap returned",
            "Move from iter to it",
            "Copy from get to third",
            "x is initialized by copy from unwrap",
            "Return immutably borrowed resource from x to *x",
            "it is the owner of the resource",
            "it goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "holder_iter_mut",
        // iter_mut takes `&mut v`: `it` holds a mutable borrow, and so
        // does `first`, until line 5.
        must_contain: &[
            "Mutable borrow from v to it: iter_mut's signature gives the value it returns the same lifetime as the value it was called on; it isn't a reference, but its value holds one",
            "Mutable borrow from v to first: unwrap's signature gives the reference it returns the same lifetime as the value it was called on",
            "Return mutably borrowed resource from it to v",
            "Return mutably borrowed resource from first to v",
        ],
        must_not_contain: &[
            ", through the value iter_mut returned",
            ", through the reference unwrap returned",
            "Move from iter_mut to it",
            "Return mutably borrowed resource from first to *first",
        ],
    },
    TooltipExpect {
        name: "holder_moved",
        // Moving a holder moves the borrow with it; it ends when it2 is
        // moved into `consume`.
        must_contain: &[
            "it2 takes over it's value, and the borrow it holds",
            "Move from it2 to consume",
            "Return immutably borrowed resource from it2 to v",
            "it2 goes out of scope",
        ],
        must_not_contain: &[
            "it2 goes out of scope. No resource is dropped.",
            "Return immutably borrowed resource from it to v",
            "it2 goes out of scope. Its value is dropped, but not the data it borrowed.",
        ],
    },
    TooltipExpect {
        name: "holder_consumed",
        // `it.sum()` takes the iterator by value: it is moved on the
        // line its borrow ends.
        must_contain: &[
            "Immutable borrow from v to it: iter's signature gives the value it returns the same lifetime as the value it was called on; it isn't a reference, but its value holds one",
            "Move from it to sum",
            "Return immutably borrowed resource from it to v",
            "it goes out of scope",
        ],
        must_not_contain: &[
            "it goes out of scope. No resource is dropped.",
            ", through the value iter returned",
            "Move from iter to it",
        ],
    },
    TooltipExpect {
        name: "holder_reassign",
        // `it = b.iter();` ends the borrow of `a` and starts one of `b`.
        must_contain: &[
            "Immutable borrow from a to it: iter's signature gives the value it returns the same lifetime as the value it was called on; it isn't a reference, but its value holds one",
            "Return immutably borrowed resource from it to a",
            "Immutable borrow from b to it: iter's signature gives the value it returns the same lifetime as the value it was called on; it isn't a reference, but its value holds one",
            "Return immutably borrowed resource from it to b",
        ],
        must_not_contain: &[
            ", through the value iter returned",
            "Move from iter to it",
        ],
    },
    TooltipExpect {
        name: "holder_struct_from_call",
        // `Excerpt::new(&novel)` returns an `Excerpt<'a>` whose `part`
        // field borrows `novel` (the signature ties both to `'a`).
        must_contain: &[
            "Immutable borrow from novel to ex.part: Excerpt::new's signature gives the struct it returns the same lifetime as its argument",
            "Return immutably borrowed resource from ex.part to novel",
        ],
        must_not_contain: &[
            ", through the struct Excerpt::new returned",
            "Return immutably borrowed resource from ex.part to *ex.part",
        ],
    },
    TooltipExpect {
        name: "holder_closure_capture",
        // `map` stores the closure, which captures `k` by reference, so
        // `lens` holds borrows of both `s` and `k`.
        must_contain: &[
            "Immutable borrow from s to c: chars's signature gives the value it returns the same lifetime as the value it was called on; c isn't a reference, but its value holds one",
            "Immutable borrow from k to lens: map's signature gives the value it returns the same lifetime as the value it was called on and its argument, so it may borrow from either, and both stay borrowed while lens is in use; lens isn't a reference, but its value holds one",
            "Return immutably borrowed resource from lens to s",
            "Return immutably borrowed value from lens to k",
            "Move from lens to sum",
        ],
        must_not_contain: &[
            ", through the value chars returned",
            ", through the value map returned",
            "Move from map to lens",
            "other data that has no column here",
        ],
    },
    TooltipExpect {
        name: "holder_closure_ref",
        must_contain: &[
            "Immutable borrow from extra to it: map's signature gives the value it returns the same lifetime as the value it was called on and its argument, so it may borrow from either, and both stay borrowed while it is in use; it isn't a reference, but its value holds one",
            "Return immutably borrowed resource from it to extra",
        ],
        must_not_contain: &[
            ", through the value map returned",
            "Move from map to it",
        ],
    },
    TooltipExpect {
        name: "move_into_call_shared_line",
        // `take(b)` shares line 5 with `let c = a;` (#379).
        must_contain: &[
            "Move from b to take",
            "b's resource is moved",
            "b goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "b goes out of scope. Its resource is dropped.",
        ],
    },
    // ─── Reassignment inside a branch (#267) ────────────────────────
    TooltipExpect {
        name: "reassign_in_branch_drop",
        // `x = …` in both arms, `y = …` in one match arm, a whole
        // struct `s = S { … }` (drops `s.a`/`s.b` too) and `s.a = …`.
        // These dots used to be plain "acquires ownership of a
        // resource" with no drop.
        must_contain: &[
            "x acquires ownership of a new resource; its previous resource is dropped",
            "y acquires ownership of a new resource; its previous resource is dropped",
            "s acquires ownership of a new resource; its previous resource is dropped",
            "s.a acquires ownership of a new resource; its previous resource is dropped",
            "s.b acquires ownership of a new resource; its previous resource is dropped",
        ],
        must_not_contain: &[
            // `z` was moved into `take` just before, in the same arm:
            // nothing is left to drop at `z = …`.
            "z acquires ownership of a new resource; its previous resource is dropped",
        ],
    },
    TooltipExpect {
        name: "reassign_through_ref_in_branch",
        // `*r = …` in both arms: `t` (r's lender) gets a branched
        // column with its old String dropped in each arm. Before, `t`
        // had no branch at all (and before #148 this panicked, #264).
        must_contain: &[
            "t acquires ownership of a new resource; its previous resource is dropped",
            "*r acquires ownership of a new resource; its previous resource is dropped",
        ],
        must_not_contain: &[
            "the resource r points to is modified through *r",
            "its previous value is dropped",
            "(copied from",
            "(moved from",
        ],
    },
    // #491: every branch of the `else if` chain returns, so only the
    // `if c` path (which moved `s`) reaches the end: nothing is dropped
    // there, and Rust needs no runtime drop flag for it to say so.
    TooltipExpect {
        name: "else_if_chain_diverges",
        must_contain: &[
            "s was moved or dropped in every branch above that reaches this point",
            "s goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "Its resource is dropped here only if no branch above moved it",
            "s was moved in at least one branch above, so it can't be used after the branches",
        ],
    },
    // ─── Collections of references (#349) ────────────────────────────
    TooltipExpect {
        name: "refs_vec_literal",
        // #349's repro. `refs` holds a borrow of `s`; `r` is a copy of
        // that reference, so it borrows `s`, not `refs`.
        must_contain: &[
            "Immutable borrow from s to refs",
            "refs holds a value that immutably borrows s's resource (refs isn't a reference itself)",
            "Immutable borrow from s to r: r is a reference copied out of refs",
            "Return immutably borrowed resource from r to s",
            "Return immutably borrowed resource from refs to s",
        ],
        must_not_contain: &[
            ", through the reference copied out of",
            "Return immutably borrowed resource from r to refs",
            "refs's resource is no longer immutably borrowed",
        ],
    },
    TooltipExpect {
        name: "refs_array_literal",
        // `pair[0]` may be either element as far as rustc is concerned,
        // so `first` keeps both `a` and `b` borrowed. `pair` (a Copy
        // array of `&String`) is copied into `drop`, not dropped.
        must_contain: &[
            "Immutable borrow from a to pair",
            "Immutable borrow from b to pair",
            "Immutable borrow from a to first: first is a reference copied out of pair, which holds references to a and b, so it may point into either, and both stay borrowed while first is in use",
            "Return immutably borrowed resource from first to a",
            "Return immutably borrowed resource from first to b",
        ],
        must_not_contain: &[
            ", through the reference copied out of",
            "Return immutably borrowed resource from first to pair",
            "pair's resource is no longer immutably borrowed",
        ],
    },
    TooltipExpect {
        name: "refs_vec_mut",
        // `vec![&mut x, &mut y]` holds two mutable borrows until line 5;
        // `x += 1` on line 6 is fine.
        must_contain: &[
            "Mutable borrow from x to slots",
            "Mutable borrow from y to slots",
            "Return mutably borrowed value from slots to x",
            "Return mutably borrowed value from slots to y",
        ],
        must_not_contain: &[
            "slots goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "refs_index_untracked",
        // `words` (filled by `collect()`) and `refs` (filled by `push`)
        // hold borrows of `text` and `s` (#392), so the references
        // copied out of them borrow `text` and `s`, never the
        // collection itself.
        must_contain: &[
            "Immutable borrow from text to first: first is a reference copied out of words",
            "Immutable borrow from s to last: last is a reference copied out of refs",
            "Return immutably borrowed resource from first to text",
            "Return immutably borrowed resource from last to s",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from first to *first",
            "Return immutably borrowed resource from last to *last",
            "Return immutably borrowed resource from first to words",
            "Return immutably borrowed resource from last to refs",
            "words's resource is no longer immutably borrowed",
        ],
    },
    // ─── Borrows through a reference parameter (#393) ────────────────
    TooltipExpect {
        name: "holder_ref_param_iter",
        // `it` borrows the caller's Vec through the reference parameter
        // `v`; v's column stands for that data, as for `&v[0]`.
        must_contain: &[
            "Immutable borrow from v to it: iter's signature gives the value it returns the same lifetime as the value it was called on; it isn't a reference, but its value holds one",
            "it holds a value that immutably borrows the data v points to (it isn't a reference itself)",
            "Return immutably borrowed resource from it to v",
            // An `Iter` needs no drop, so nothing is claimed dropped (#484).
            "it goes out of scope",
        ],
        must_not_contain: &[
            "it goes out of scope. Its value is dropped, but not the data it borrowed.",
            ", through the value iter returned",
            "Move from iter to it",
            "it is the owner of the resource",
            "it acquires ownership of a resource",
            "it goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "holder_ref_param_iter_mut",
        // iter_mut reborrows `*v` mutably: `it`, and `first` taken out
        // of it, keep that borrow until line 4 (rustc rejects using
        // `it` after `v.push(0)`).
        must_contain: &[
            "Mutable borrow from v to it: iter_mut's signature gives the value it returns the same lifetime as the value it was called on; it isn't a reference, but its value holds one",
            "it holds a value that mutably borrows the data v points to (it isn't a reference itself)",
            "Mutable borrow from v to first: unwrap's signature gives the reference it returns the same lifetime as the value it was called on",
            "Return mutably borrowed resource from it to v",
            "Return mutably borrowed resource from first to v",
        ],
        must_not_contain: &[
            ", through the value iter_mut returned",
            ", through the reference unwrap returned",
            "Move from iter_mut to it",
            "it is the owner of the resource",
            "first is initialized by copy from unwrap",
        ],
    },
    TooltipExpect {
        name: "fn_ret_ref_through_param",
        // A reference returned by a call on a reference parameter
        // borrows what the parameter points to, like `&s[..1]` does.
        must_contain: &[
            "Immutable borrow from s to w: first_word's signature gives the reference it returns the same lifetime as its argument",
            "Immutable borrow from v to q: unwrap's signature gives the reference it returns the same lifetime as the value it was called on",
            "Return immutably borrowed resource from w to s",
            "Return immutably borrowed resource from q to v",
        ],
        must_not_contain: &[
            ", through the reference first_word returned",
            ", through the reference unwrap returned",
            "Copy from first_word to w",
            "w is initialized by copy from first_word",
            "Copy from unwrap to q",
            "q is initialized by copy from unwrap",
            "Return immutably borrowed resource from w to *w",
        ],
    },
    // #504: `x` is an i32 copied out of the Vec; `xs` is what is moved.
    TooltipExpect {
        name: "for_copy_elements",
        must_contain: &[
            "x is bound to a copyable value",
            "xs acquires ownership of a resource",
        ],
        must_not_contain: &[
            "x acquires ownership of a resource",
            "x is bound to a value",
        ],
    },
    // #505: a Copy parameter is a copy of the caller's value; a String
    // parameter acquires ownership.
    TooltipExpect {
        name: "copy_param_wording",
        must_contain: &[
            "c is a copy of the caller's value",
            "a is a copy of the caller's value",
            "s acquires ownership from the caller",
        ],
        must_not_contain: &[
            "c acquires ownership from the caller",
            "a acquires ownership from the caller",
            "c is bound to a value from the caller",
        ],
    },
    TooltipExpect {
        name: "reassign_one_bullet",
        must_contain: &["s acquires ownership of a new resource; its previous resource is dropped"],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "read_ref_through_element_ref",
        // `let r = *rr[0];` (rr: Vec<&&i32>) used to panic in
        // find_lender (rr isn't a borrower). It copies the `&i32` the
        // element points to; rr is a Vec, so "Copy from *rr to r" would
        // be false.
        must_contain: &[
            "Copy from *rr[0] to r",
            "r is initialized with a copy of *rr[0]",
            "the resource rr[0] points to is copied",
            "Copy from *rrr[0][0] to q",
            "the resource rrr[0][0] points to is copied",
            "Copy from *b to s",
        ],
        must_not_contain: &[
            "Copy from *rr to r",
            "*rr's resource is copied",
            "Copy from *rrr to q",
        ],
    },

    // ─── Calling an `FnOnce` closure moves it (#271) ─────────────────
    TooltipExpect {
        name: "fnonce_call_drops_capture",
        // `f()` is `FnOnce::call_once(f)`: f is moved into the call, the
        // body drops `s`, and so f has nothing to drop at its scope end.
        must_contain: &[
            "f is moved into the call (an FnOnce closure can only be called once). \
             s, which f captured, is dropped during the call",
            "f goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "f goes out of scope. Its 1 captured resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "fnonce_call_returns_capture",
        // The body returns `s`, so it leaves f as the call's result and
        // `t` owns it: f drops nothing and the call doesn't drop it.
        must_contain: &[
            "Move from f to t",
            "f is moved into the call (an FnOnce closure can only be called once). \
             s, which f captured, is moved out of the closure as the call's result",
            "f goes out of scope. No resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "f goes out of scope. Its 1 captured resource is dropped.",
            "f is moved into the call (an FnOnce closure can only be called once). \
             s, which f captured, is dropped during the call",
        ],
    },
    TooltipExpect {
        name: "fnonce_call_conditional",
        // Called on one path only: f is maybe-moved, so a drop flag
        // decides at scope end whether its capture is dropped.
        must_contain: &[
            "f is moved into the call (an FnOnce closure can only be called once). \
             s, which f captured, is dropped during the call",
            "f goes out of scope. Its captured resources are dropped here only if no branch above \
             called it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "f goes out of scope. Its 1 captured resource is dropped.",
            "f goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "fnonce_call_captures",
        // Three captures, three fates: `a` is passed on (all we claim is
        // that the call uses it up), `b` is only read so it's dropped
        // with the closure's environment, and the Copy `n` isn't listed.
        must_contain: &[
            "f is moved into the call (an FnOnce closure can only be called once). \
             a, which f captured, is moved on or dropped by the call. \
             b, which f captured, is dropped during the call",
            "f goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "f goes out of scope. Its 2 captured resources are dropped.",
        ],
    },
    TooltipExpect {
        name: "fnonce_call_copy_result",
        // The call's i32 result is a new value, not a copy out of `g`.
        must_contain: &[
            "g is moved into the call (an FnOnce closure can only be called once). \
             v, which g captured, is dropped during the call",
            "g goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "g's value is copied",
            "g goes out of scope. Its 1 captured resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "fnonce_closure_moved_to_binding",
        // `let g = f;` moves the closure; `g` is then moved into `drop`.
        // Neither is left holding the captured String at its scope end.
        must_contain: &[
            "f goes out of scope. No resource is dropped.",
            "g goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "f goes out of scope. Its 1 captured resource is dropped.",
            "g goes out of scope. Its 1 captured resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "fnmut_call_unchanged",
        // An `FnMut` closure is only borrowed by its calls: still owned,
        // and still dropping its capture, at the end of `main`.
        must_contain: &[
            "f goes out of scope. Its 1 captured resource is dropped.",
        ],
        must_not_contain: &[
            "f is moved into the call (an FnOnce closure can only be called once)",
            "f goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "comparison_reads",
        // #497: `t < lim` on i32s reads both operands, as `t + lim`
        // does (#241): each is copied into the operator, and the result
        // (a bool) is a new value that isn't `t` or `lim` (`z` is bound
        // to a copyable value, not copied from either). The same in an
        // `if` condition (`==`) and with a literal (`t >= 3`: only `t`
        // names a column).
        must_contain: &[
            "Copy from t to <",
            "Copy from lim to <",
            "Copy from t to +",
            "Copy from lim to +",
            "Copy from t to ==",
            "Copy from lim to ==",
            "Copy from t to >=",
            "t's value is copied",
            "lim's value is copied",
            "z is bound to a copyable value",
        ],
        must_not_contain: &[
            "Copy from lim to z",
            "Copy from t to z",
            "Copy from lim to >=",
            "Copy from t to big",
        ],
    },
    // An if-let arm's struct-field columns say one thing per row (#486): they
    // used to draw "has no value yet" over "holds a copyable value" on the
    // pattern's rows. (Fixed on main by #405; `well_formed` now checks every
    // fixture for stacked segments.)
    TooltipExpect {
        name: "if_let_struct_fields_unused",
        must_contain: &[
            "p.x is bound to a copyable value",
            "p.x holds a copyable value",
            "p.y is bound to a copyable value",
            "p.y holds a copyable value",
        ],
        must_not_contain: &[
            "p.x has no value yet",
            "p.y has no value yet",
        ],
    },
    // ─── Collections filled by collect() / push (#392) ───────────────
    TooltipExpect {
        name: "collect_push_borrows",
        // #392's repro. `words` and `refs` hold borrows of `text` and
        // `s` until line 7: rustc rejects `drop(s)` or moving `text`
        // before then.
        must_contain: &[
            "Immutable borrow from text to words: collect's signature gives the value it returns the same lifetime as the value it was called on; words isn't a reference, but its value holds one",
            "Return immutably borrowed resource from words to text",
            "Immutable borrow from s to refs, stored in refs by push; refs isn't a reference, but its value now holds one",
            "refs holds a value that immutably borrows s's resource (refs isn't a reference itself)",
            "refs owns a value that can hold references (refs isn't a reference itself)",
            "Return immutably borrowed resource from refs to s",
            "refs goes out of scope. Its value is dropped, but not the data it borrowed.",
        ],
        must_not_contain: &[
            ", through the value collect returned",
            "Move from collect to words",
            "words is the owner of the resource",
            "refs is the owner of the resource",
            "refs goes out of scope. Its resource is dropped.",
            // `Vec::new()` holds no reference yet.
            "refs holds a value that contains a reference (refs isn't a reference itself)",
        ],
    },
    TooltipExpect {
        name: "push_mut_refs",
        // Two `&mut` pushed into one Vec: both x and y stay mutably
        // borrowed until slots's last use (line 8), before `x += 1`.
        must_contain: &[
            "Mutable borrow from x to slots, stored in slots by push; slots isn't a reference, but its value now holds one",
            "Mutable borrow from y to slots, stored in slots by push; slots isn't a reference, but its value now holds one",
            "Return mutably borrowed value from slots to x",
            "Return mutably borrowed value from slots to y",
        ],
        must_not_contain: &[
            "slots is the owner of the resource",
            "slots goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "generic_by_value_ref",
        // #424: `id(&s)` returns the reference it was given, and
        // `Some(&s)` stores it; both keep `s` borrowed until line 8.
        must_contain: &[
            "Immutable borrow from s to r: id's signature gives the reference it returns the same lifetime as its argument",
            "Immutable borrow from s to o: Some's signature gives the value it returns the same lifetime as its argument; o isn't a reference, but its value holds one",
            "Return immutably borrowed resource from r to s",
            "Return immutably borrowed resource from o to s",
        ],
        must_not_contain: &[
            ", through the reference id returned",
            ", through the value Some returned",
            "Copy from id to r",
            "Copy from Some to o",
            "Return immutably borrowed resource from r to *r",
        ],
    },
    TooltipExpect {
        name: "push_owned_not_holder",
        // Guard, passes before and after: a Vec<String> holds no
        // borrow, and `retain` doesn't store its closure.
        must_contain: &[
            "names is the owner of the resource",
            "nums is the owner of the resource",
            "names goes out of scope. Its resource is dropped (including every String it holds).",
        ],
        must_not_contain: &[
            "names owns a value that can hold references (names isn't a reference itself)",
            "nums owns a value that can hold references (nums isn't a reference itself)",
            "Immutable borrow from k to nums",
        ],
    },
    TooltipExpect {
        name: "ref_swap_keeps_borrow",
        // `(r1, r2) = (r2, r1)`, the same swap through a temporary
        // (`let t = p; p = q; q = t;`), and a swap of `&mut`s: each
        // lender stays borrowed by the other reference, so its borrow
        // is returned only at that reference's last use (#291).
        must_contain: &[
            "r1's immutable borrow ends",
            "Return immutably borrowed resource from r2 to x",
            "Return immutably borrowed resource from r1 to y",
            "t's immutable borrow ends",
            "Return immutably borrowed resource from q to a",
            "Return immutably borrowed resource from p to b",
            "m1's mutable borrow ends",
            "Return mutably borrowed resource from m2 to u",
            "Return mutably borrowed resource from m1 to w",
            // Reads like `m1 = m2` (#650); the arrow names the source.
            "m1 is assigned m2's mutable reference",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from r1 to x",
            "Return immutably borrowed resource from t to a",
            "Return mutably borrowed resource from m1 to u",
            "(moved out of m2)",
        ],
    },
    TooltipExpect {
        name: "field_write_through_local_ref",
        // `r.a = …` after `let r = &mut s;` overwrites s's field: the
        // old String is dropped on `s.a`'s column, not on a column of
        // the reference's own (#152). Overwriting the whole struct
        // through `*w` drops the fields' old values too (#268).
        must_contain: &[
            "Move from String::from to r.a",
            "r.a acquires ownership of a new resource; its previous resource is dropped",
            // (r.n: a Copy integer, nothing to drop.)
            "r.n is assigned a new value",
            "r.inner.v acquires ownership of a new resource; its previous resource is dropped",
            "q.v acquires ownership of a new resource; its previous resource is dropped",
            "s.a acquires ownership of a new resource; its previous resource is dropped",
            "s.inner.v acquires ownership of a new resource; its previous resource is dropped",
            "s.inner acquires ownership of a new resource; its previous resource is dropped",
            "*w acquires ownership of a new resource; its previous resource is dropped",
        ],
        must_not_contain: &[
            "r.a acquires ownership of a resource",
            "r.a is the owner of the resource",
            "r.n acquires ownership of a resource",
            "the resource r points to is modified through r.a",
            "the resource r points to is modified through r.n",
            "the resource q points to is modified through q.v",
            "the resource w points to is modified through *w",
            "its previous value is dropped",
            "(copied from",
            "(moved from",
            "s.n acquires ownership of a new resource; its previous resource is dropped",
        ],
    },
    // ─── Rc / Arc / Weak (#373) ──────────────────────────────────────
    // Only the last Rc/Arc frees the value, and RustViz doesn't count
    // references, so every drop of one says what is always true. Clones
    // are found from the resolved callee, not from the name `clone`.
    TooltipExpect {
        name: "rc_handles",
        must_contain: &[
            "a goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped).",
            "b goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped).",
            "c goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped).",
            "s goes out of scope. Its resource is dropped (for an Arc, that lowers the reference count; the shared value is freed only when its last Arc is dropped).",
            "t goes out of scope. Its resource is dropped (for an Arc, that lowers the reference count; the shared value is freed only when its last Arc is dropped).",
            "w goes out of scope. Its resource is dropped (a Weak doesn't keep the value alive; dropping it frees only the Weak itself).",
            "t acquires ownership of a new resource; its previous resource is dropped (for an Arc, that lowers the reference count; the shared value is freed only when its last Arc is dropped)",
            "b is another Rc to the same value (Rc::clone increases the reference count; nothing is copied)",
            "c is another Rc to the same value (Rc::clone increases the reference count; nothing is copied)",
            "t is another Arc to the same value (Arc::clone increases the reference count; nothing is copied)",
            "w is a Weak pointing to the same value (Rc::downgrade increases the weak count; nothing is copied)",
            "Rc::clone makes another Rc to the same value (nothing is copied), now owned by b",
            "clone makes another Rc to the same value (nothing is copied), now owned by c",
            "Arc::clone makes another Arc to the same value (nothing is copied), now owned by t",
            "Rc::downgrade makes a Weak pointing to the same value (Rc's strong count is unchanged; nothing is copied), now owned by w",
        ],
        must_not_contain: &[
            "a goes out of scope. Its resource is dropped.",
            "b goes out of scope. Its resource is dropped.",
            "c goes out of scope. Its resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
            "w goes out of scope. Its resource is dropped.",
            // Whole-tooltip match (#518): the correct text continues with the Arc note.
            "=t acquires ownership of a new resource; its previous resource is dropped",
            "handle",
            "Move from Rc::clone to b",
            "Move from clone to c",
            "Move from Arc::clone to t",
            "Move from Rc::downgrade to w",
            "b acquires ownership of a resource",
            "w acquires ownership of a resource",
        ],
    },
    TooltipExpect {
        name: "rc_branch",
        must_contain: &[
            "a was moved in at least one branch above, so it can't be used after the branches (until it's assigned again). If the branch that ran didn't move it, its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped) when a goes out of scope.",
            "a goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped) here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "r goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped).",
        ],
        must_not_contain: &[
            "its resource is dropped when a goes out of scope.",
            "a goes out of scope. Its resource is dropped here only if no branch above moved it",
            "r goes out of scope. Its resource is dropped.",
            "handle",
        ],
    },
    TooltipExpect {
        name: "rc_clone_by_callee",
        // `Maker::clone` and `String::clone` aren't `Rc::clone`, even
        // though a method named `clone` returns an `Rc` in one of them.
        must_contain: &[
            "r goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped).",
            "s goes out of scope. Its resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
            "Move from clone to r",
            "Move from clone to t",
            "a's resource is moved into drop, which drops it (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped)",
        ],
        must_not_contain: &[
            "r is another Rc to the same value",
            "t is another Rc to the same value",
            "makes another Rc to the same value (nothing is copied), now owned by r",
        ],
    },
    TooltipExpect {
        name: "rc_fields",
        // An Rc/Arc that is a struct field says the same as a local one;
        // a struct whose only droppable fields are Rcs says so; a struct
        // that also owns a String keeps main's sentence.
        must_contain: &[
            "p.shared goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped).",
            "m.shared goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is freed only when its last Rc is dropped).",
            "c.hits goes out of scope. Its resource is dropped (for an Arc, that lowers the reference count; the shared value is freed only when its last Arc is dropped).",
            "p goes out of scope. Its resource is dropped (its Rc fields only lower their reference counts).",
            "c goes out of scope. Its resource is dropped (its Arc fields only lower their reference counts).",
            "m goes out of scope. Its resource is dropped.",
            "m.name goes out of scope. Its resource is dropped.",
            "p.shared is another Rc to the same value (Rc::clone increases the reference count; nothing is copied)",
            "m.shared is another Rc to the same value (Rc::clone increases the reference count; nothing is copied)",
            "q is another Rc to the same value (Rc::clone increases the reference count; nothing is copied)",
            "r is another Rc to the same value (Rc::clone increases the reference count; nothing is copied)",
            "Rc::clone makes another Rc to the same value (nothing is copied), now owned by q",
            "clone makes another Rc to the same value (nothing is copied), now owned by r",
            "clone makes another Rc to the same value (nothing is copied), now owned by m.shared",
        ],
        must_not_contain: &[
            "p.shared goes out of scope. Its resource is dropped.",
            "m.shared goes out of scope. Its resource is dropped.",
            "c.hits goes out of scope. Its resource is dropped.",
            "p goes out of scope. Its resource is dropped.",
            "c goes out of scope. Its resource is dropped.",
            "m goes out of scope. Its resource is dropped (",
            "m.name goes out of scope. Its resource is dropped (",
            "p.shared acquires ownership of a resource",
            "Move from Rc::clone to q",
            "Move from clone to r",
            "Move from Rc::clone to p.shared",
            "Move from clone to m.shared",
        ],
    },
    TooltipExpect {
        name: "rc_weak_clone",
        // `Weak::clone` makes another Weak and raises the weak count.
        must_contain: &[
            "w2 is another Weak pointing to the same value (Weak::clone increases the weak count; nothing is copied)",
            "w3 is another Weak pointing to the same value (Weak::clone increases the weak count; nothing is copied)",
            "clone makes another Weak pointing to the same value (nothing is copied), now owned by w2",
            "Weak::clone makes another Weak pointing to the same value (nothing is copied), now owned by w3",
            "w2 goes out of scope. Its resource is dropped (a Weak doesn't keep the value alive; dropping it frees only the Weak itself).",
        ],
        must_not_contain: &[
            "Move from clone to w2",
            "Move from Weak::clone to w3",
            "w2 acquires ownership of a resource",
            "w3 acquires ownership of a resource",
            "w2 goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "rc_inside",
        // A value whose only droppable contents are Rcs says the Rc inside
        // only lowers a count; a collection adds that its own buffer is
        // freed. A Vec of (Rc, String), a HashMap keyed by String and a
        // struct with its own Drop impl also own other data, so they get no
        // Rc sentence (the Vec and HashMap say what they hold instead, #542).
        must_contain: &[
            "o goes out of scope. Its resource is dropped (the Rc inside only lowers its reference count).",
            "b goes out of scope. Its resource is dropped (the Rc inside only lowers its reference count).",
            "c goes out of scope. Its resource is dropped (the Rc inside only lowers its reference count).",
            "ao goes out of scope. Its resource is dropped (the Arc inside only lowers its reference count).",
            "wo goes out of scope. Its resource is dropped (the Weak inside doesn't keep its value alive).",
            "v goes out of scope. Its resource is dropped (each Rc in it only lowers its reference count; its own buffer is freed).",
            "h goes out of scope. Its resource is dropped (each Rc in it only lowers its reference count; its own buffer is freed).",
            "d's resource is moved into drop, which drops it (the Rc inside only lowers its reference count)",
            "mixed goes out of scope. Its resource is dropped (including every (Rc<i32>, String) it holds).",
            "keyed goes out of scope. Its resource is dropped (including every key and value it holds).",
            "n goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "o goes out of scope. Its resource is dropped.",
            "b goes out of scope. Its resource is dropped.",
            "c goes out of scope. Its resource is dropped.",
            "ao goes out of scope. Its resource is dropped.",
            "wo goes out of scope. Its resource is dropped.",
            "v goes out of scope. Its resource is dropped.",
            "h goes out of scope. Its resource is dropped.",
            "d's resource is moved into drop, which drops it.",
            "mixed goes out of scope. Its resource is dropped.",
            "mixed goes out of scope. Its resource is dropped (the Rc inside only lowers its reference count).",
            "keyed goes out of scope. Its resource is dropped.",
            "keyed goes out of scope. Its resource is dropped (the Rc inside only lowers its reference count).",
            "n goes out of scope. Its resource is dropped (",
        ],
    },
    TooltipExpect {
        name: "rc_inside_flag",
        // The moved-on-some-paths wording (drop flag) and `drop(x)` carry
        // the same parenthetical.
        must_contain: &[
            "o goes out of scope. Its resource is dropped (the Rc inside only lowers its reference count) here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "v goes out of scope. Its resource is dropped (each Rc in it only lowers its reference count; its own buffer is freed) here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "o was moved in at least one branch above, so it can't be used after the branches (until it's assigned again). If the branch that ran didn't move it, its resource is dropped (the Rc inside only lowers its reference count) when o goes out of scope.",
            "v was moved in at least one branch above, so it can't be used after the branches (until it's assigned again). If the branch that ran didn't move it, its resource is dropped (each Rc in it only lowers its reference count; its own buffer is freed) when v goes out of scope.",
            "o's resource is moved into drop, which drops it (the Rc inside only lowers its reference count)",
            "v's resource is moved into drop, which drops it (each Rc in it only lowers its reference count; its own buffer is freed)",
        ],
        must_not_contain: &[
            "o goes out of scope. Its resource is dropped here only if",
            "v goes out of scope. Its resource is dropped here only if",
            "its resource is dropped when o goes out of scope.",
            "its resource is dropped when v goes out of scope.",
            "o's resource is moved into drop, which drops it.",
            "v's resource is moved into drop, which drops it.",
        ],
    },
    TooltipExpect {
        name: "rc_inside_tuple",
        // A tuple parameter whose only droppable part is an Rc.
        must_contain: &[
            "t goes out of scope. Its resource is dropped (the Rc inside only lowers its reference count).",
        ],
        must_not_contain: &[
            "t goes out of scope. Its resource is dropped.",
        ],
    },
    // #515: `r` / `a` are dropped at each `return` (the drop count is in
    // `both_branches_return_end_the_function`); `q` / `b` are moved to the
    // caller on both paths, so nothing is dropped for them, and nothing
    // is "not moved in any of the branches above": no path reaches the
    // merge or the closing brace.
    TooltipExpect {
        name: "if_else_both_return",
        must_contain: &[
            "q's resource is moved to the caller",
            "b's resource is moved to the caller",
            "r goes out of scope. Its resource is dropped.",
            "a goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "q isn't moved in any of the branches above",
            "r isn't moved in any of the branches above",
            "b isn't moved in any of the branches above",
            "a isn't moved in any of the branches above",
            "q goes out of scope. Its resource is dropped.",
            "b goes out of scope. Its resource is dropped.",
        ],
    },
    // #515: `return s;` moves `s` to the caller, `return n;` copies `n`.
    TooltipExpect {
        name: "return_value_to_caller",
        must_contain: &[
            "s's resource is moved to the caller",
            "n's value is copied to the caller",
        ],
        must_not_contain: &[],
    },
    // ─── Array literals, vec! and index reads (#297) ──────────────────
    TooltipExpect {
        name: "array_literal_moves",
        // `[s, t]` moves both Strings into `a`; `[x, 2, 3]` copies `x`;
        // `[p]` moves `p` and its field.
        must_contain: &[
            "Move from s to a",
            "Move from t to a",
            "s goes out of scope. No resource is dropped.",
            "a goes out of scope. Its resource is dropped (including every String it holds).",
            "Copy from x to nums",
            "nums is initialized with a copy of x",
            "zeros is bound to a copyable value",
            "Move from p to ps",
            "p.name goes out of scope. No resource is dropped.",
            "ps goes out of scope. Its resource is dropped (including every P it holds).",
        ],
        must_not_contain: &[
            // Whole-tooltip match (#518): "ps goes out of scope. Its resource is dropped." is correct.
            "=s goes out of scope. Its resource is dropped.",
            "a goes out of scope. No resource is dropped.",
            "p.name goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "vec_macro_moves",
        must_contain: &[
            "Move from s to v",
            "Move from t to v",
            "s goes out of scope. No resource is dropped.",
            "t goes out of scope. No resource is dropped.",
            "Move from w to h",
            "w goes out of scope. No resource is dropped.",
            "Move from m to keep",
            "m goes out of scope. No resource is dropped.",
            "q's resource is moved to the caller",
            "q goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "s goes out of scope. Its resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
            "w goes out of scope. Its resource is dropped.",
            "m goes out of scope. Its resource is dropped.",
            "q goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "index_copy_read",
        // A copy of one element; `v` keeps its whole resource.
        must_contain: &[
            "first is initialized with a copy of v[0]",
            "v[0] is copied out of v's resource",
            "cell is initialized with a copy of grid[1][0]",
            // `[[i32; 2]; 2]` has nothing to drop: a value (#484).
            "grid[1][0] is copied out of grid's value",
            "h is initialized with a copy of xs[0]",
            // `xs: &[i32]` points to a slice that needs no drop: a value (#549).
            "xs[0] is copied out of the value xs points to",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "first goes out of scope. No resource is dropped.",
            "v's resource is copied",
            "grid[1][0] is copied out of grid's resource",
            "xs[0] is copied out of the resource xs points to",
        ],
    },
    // ─── Variables declared in a nested construct (#501) ─────────────
    TooltipExpect {
        name: "else_if_let_binding",
        // `y` exists only in the `else if let` branch: one column, no
        // split dot of its own, no "no value yet" strips over the `if`
        // branch. It's an i32: no drop text.
        must_contain: &[
            "Copy from b to y",
            "y is initialized with a copy of b",
            "y goes out of scope",
            "x goes out of scope",
        ],
        must_not_contain: &[
            "y is live in a conditional expression ",
            "y has no value yet",
            "y goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "match_arm_nested_if_let",
        // `y`, bound by an `if let` inside the `Some(x)` arm, exists only
        // in that `if let`, not across the `match`.
        must_contain: &[
            "y immutably borrows a resource",
            "y's immutable borrow ends",
            "y goes out of scope",
            "x goes out of scope",
        ],
        must_not_contain: &[
            "y is live in a conditional expression ",
            "y has no value yet",
        ],
    },
    TooltipExpect {
        name: "if_branch_nested_decls",
        // `z` (a `let` in an inner `if`) and `n` (a `for` variable) are
        // declared inside the `if` branch; `s` is the variable that
        // really is live across the `if`.
        must_contain: &[
            "z acquires ownership of a resource",
            "z goes out of scope. Its resource is dropped.",
            "n immutably borrows a resource",
            "n's immutable borrow ends",
            "s is live in a conditional expression ",
        ],
        must_not_contain: &[
            "z is live in a conditional expression ",
            "n is live in a conditional expression ",
            "z has no value yet",
            "n has no value yet",
        ],
    },
    TooltipExpect {
        name: "while_let_in_branch",
        // #261: a `while let` binding in one branch of an `if` exists only
        // in the loop body. It's an i32: no drop text at its end (its
        // end used to be ordered before its use in the body).
        must_contain: &[
            "x is initialized with a copy of pop",
            "x goes out of scope",
            "v is live in a conditional expression ",
        ],
        must_not_contain: &[
            "x is live in a conditional expression ",
            "x has no value yet",
            "x goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "else_if_let_same_name",
        // #525, #308: the `z` of the `else if let` is a new variable in
        // a different branch from the first `z`, so it doesn't shadow it:
        // both share one column (no `z₂`, no "different variable" note)
        // and the second binding is an initialization, not an assignment.
        must_contain: &[
            "z is initialized with a copy of a",
            "z is initialized with a copy of b",
            "Copy from b to z",
            "z goes out of scope",
        ],
        must_not_contain: &[
            "z₂",
            "z is assigned a copy of b",
            "z is assigned a copy of a",
            "is a different variable from",
            "z is live in a conditional expression ",
            "z has no value yet",
        ],
    },
    TooltipExpect {
        name: "else_if_let_same_name_else_let",
        // The same, ending in an `else` that declares `let z`: every `z`
        // is a new variable of its own branch, so initializing it is
        // never an assignment ("z is assigned a new value" was the false
        // text for the last one).
        must_contain: &[
            "z is initialized with a copy of a",
            "z is initialized with a copy of b",
            "z is bound to a copyable value",
        ],
        must_not_contain: &[
            "z₂",
            "z is assigned a copy of b",
            "z is assigned a new value",
            "is a different variable from",
        ],
    },
    // ─── An arm binding on the line its nested branch splits on (#502) ──
    TooltipExpect {
        name: "nested_match_arm_binding",
        // `x` is bound by `Some(x)` on the line the inner `match b`
        // splits on; it holds 1 in both inner arms, including the
        // `Some(y)` arm that doesn't use it.
        must_contain: &[
            "x is initialized with a copy of a",
            "x is live in a conditional expression ",
            "x holds a copyable value",
        ],
        must_not_contain: &["x has no value yet"],
    },
    TooltipExpect {
        name: "match_arm_binding_if_same_line",
        // `Some(x) => if c {`: x owns the String in both branches of the
        // `if`; the `else` branch moves it into `drop`.
        must_contain: &[
            "x acquires ownership of a resource",
            "x is the owner of the resource",
            "x's resource is moved into drop, which drops it",
        ],
        must_not_contain: &["x has no value yet"],
    },
    TooltipExpect {
        name: "field_read_through_ref",
        // A field read through a reference `p` copies out of the
        // resource p points to; `&p.name` reborrows the field through
        // p. Before #266 none of these reads drew anything, `n` was
        // shown returning its borrow to itself (`*n`), and a match on
        // `self.n` bound `v` "by copy from Anonymous resource".
        must_contain: &[
            "the resource p points to is read through p.x (copied to a)",
            "Copy from p.x to a",
            "a is initialized with a copy of p.x",
            "Immutable reborrow from p.name to n",
            "p.name's resource is immutably borrowed by n",
            "Return immutably borrowed resource from n to p.name",
            "the resource p points to is read through p.inner.v (copied to v)",
            "the resource p points to is read through (*p).x (copied to w)",
            "Copy from p.x to take",
            "look reads from p.name",
            "len reads from p.name (implicitly borrowed)",
            "the resource self points to is read through self.x (copied to the caller)",
            "the resource r points to is read through r.x (copied to b)",
            "Copy from r.x to b",
            "the resource h.r points to is read through h.r.x (copied to c)",
            "Partial copy from self.n to v",
            "v is initialized with a copy of self.n",
            "Partial immutable borrow from self.label to s",
            "Return immutably borrowed resource from s to self.label",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from n to *n",
            "Return immutably borrowed resource from s to *s",
            "v is initialized by copy from Anonymous resource",
            "v is initialized with a copy of Anonymous resource",
            "self's resource is copied to the caller",
            "p.x acquires ownership of a resource",
            "p.x is the owner of the resource",
        ],
    },
    TooltipExpect {
        name: "field_mut_borrow_through_ref",
        // `&mut r.a` with r a `&mut S` reborrows the field through r.
        // Before #202 nothing was drawn and f returned its borrow to
        // itself (`*f`). A write through f (or through a reborrow
        // chain) drops the old String on the owner's column `s.a`;
        // before #323 a write through `r2 = &mut *r1` said the
        // reference r1 "acquires ownership".
        must_contain: &[
            "Mutable reborrow from r.a to f",
            "r.a's resource is mutably borrowed by f; r.a can't be used while the borrow is live",
            "Return mutably borrowed resource from f to r.a",
            "s.a acquires ownership of a new resource; its previous resource is dropped",
            "push_str reads from/writes to r.a (mutable reborrow — r.a can't be used until push_str returns)",
            "push reads from/writes to r.inner.v (mutable reborrow — r.inner.v can't be used until push returns)",
            "Mutable reborrow from r.inner to q",
            "Mutable reborrow from q.v to g",
            "s.inner.v acquires ownership of a new resource; its previous resource is dropped",
            "Mutable reborrow from p.a to f",
            "push_str reads from/writes to p.a (mutable reborrow — p.a can't be used until push_str returns)",
            "t acquires ownership of a new resource; its previous resource is dropped",
            "u.a acquires ownership of a new resource; its previous resource is dropped",
            // `*f = String::from("b")` reads like a plain assignment on
            // f's column too (2026-11-05 decision).
            "*f acquires ownership of a new resource; its previous resource is dropped",
        ],
        must_not_contain: &[
            "the resource f points to is modified through *f",
            "(copied from",
            "(moved from",
            "Return mutably borrowed resource from f to *f",
            "Return mutably borrowed resource from g to *g",
            "r acquires ownership of a new resource; its previous resource is dropped",
            "r1 acquires ownership of a new resource; its previous resource is dropped",
            "q acquires ownership of a new resource; its previous resource is dropped",
            "w acquires ownership of a new resource; its previous resource is dropped",
            "r.a acquires ownership of a resource",
        ],
    },
    // A shadowed variable captured by an FnOnce closure is named by its
    // own column in the call's tooltip (#173).
    TooltipExpect {
        name: "shadow_closure_fate",
        must_contain: &[
            "g2 is moved into the call (an FnOnce closure can only be called once). u₂, which g2 captured, is dropped during the call",
        ],
        must_not_contain: &[
            "g2 is moved into the call (an FnOnce closure can only be called once). u, which g2 captured, is dropped during the call",
        ],
    },
    // ─── Or-pattern arms (#503) ──────────────────────────────────────
    TooltipExpect {
        name: "or_pattern_arm",
        // `Ok(s) | Err(s) =>` moves the String out of `r` whichever
        // variant it is; `s` drops it at the end of the arm, so `r` has
        // nothing left to drop. (main drew no `s` at all and had `r`
        // drop the String.)
        must_contain: &[
            "Partial move from r to s",
            "r's resource is moved",
            "s acquires ownership of a resource",
            "s goes out of scope. Its resource is dropped.",
            "r goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &["r goes out of scope. Its resource is dropped."],
    },
    TooltipExpect {
        name: "or_pattern_arms",
        // Nested (`Some(Ok(s) | Err(s))`), through a reference
        // (`match &r { Ok(t) | Err(t) => … }` borrows), and with a guard
        // on Copy values (`Ok(k) | Err(k) if k > 0`).
        must_contain: &[
            "Partial move from o to s",
            "s goes out of scope. Its resource is dropped.",
            "o goes out of scope. Its resource (including the Result<String, String> it holds, if any) is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "Partial immutable borrow from r to t",
            "t immutably borrows a resource",
            "r goes out of scope. Its resource is dropped.",
            "Partial copy from n to k",
            "k is initialized with a copy of n",
        ],
        must_not_contain: &["o goes out of scope. Its resource is dropped."],
    },
    TooltipExpect {
        name: "or_pattern_if_let",
        // `if let Ok(s) | Err(s) = r` binds `s` too.
        must_contain: &[
            "Move from r to s",
            "s acquires ownership of a resource",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["r goes out of scope. Its resource is dropped."],
    },
    TooltipExpect {
        name: "or_pattern_shadow",
        // An or-pattern binding that shadows an outer `s` gets its own
        // column `s₂` (#253, #503); the outer `s` is still live after.
        must_contain: &[
            "Partial move from r to s₂",
            "s₂ goes out of scope. Its resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["r goes out of scope. Its resource is dropped."],
    },
    // ─── References a pattern copies out of a borrow holder (#391) ───
    TooltipExpect {
        name: "holder_match_pattern",
        // #391's repro (Book ch. 8.1). `x` is a copy of the `&i32` in
        // `third`, which borrows `v`: so x borrows `v`, until line 5.
        must_contain: &[
            "Partial copy from third to x",
            "Immutable borrow from v to x: x is a reference copied out of third",
            "Return immutably borrowed resource from x to v",
            "Return immutably borrowed resource from third to v",
        ],
        must_not_contain: &[
            ", through the reference copied out of",
            ", through the reference moved out of",
            "Return immutably borrowed resource from x to *x",
            "Return immutably borrowed resource from x to third",
        ],
    },
    TooltipExpect {
        name: "holder_iflet_call",
        // `if let` / `while let` on a call: the bound reference is the
        // one stored in the call's result, so it borrows `v` (mutably,
        // for `get_mut`), and v stays borrowed past the call.
        must_contain: &[
            "Immutable borrow from v to y: y is a reference copied out of the value get returned",
            "Return immutably borrowed resource from y to v",
            "Mutable borrow from v to x: x is a reference moved out of the value get_mut returned",
            "Return mutably borrowed resource from x to v",
            "Immutable borrow from v to n: n is a reference copied out of the value next returned",
            // n's borrow ends at its last use, `println!` (#575); `it`
            // still borrows v until the loop ends.
            "Return immutably borrowed resource from n to v",
            "v's resource is no longer borrowed by n, but is still immutably borrowed by it",
        ],
        must_not_contain: &[
            ", through the reference copied out of",
            ", through the reference moved out of",
            "Return immutably borrowed resource from y to *y",
            "Return mutably borrowed resource from x to *x",
            "Return immutably borrowed resource from n to *n",
            "Return immutably borrowed resource from y to get",
        ],
    },
    TooltipExpect {
        name: "match_arm_let_local",
        // `first` is declared in the `_` arm: no branched column, no
        // "no value yet" strips, no split dot, no false merge text (#425).
        must_contain: &[
            "first immutably borrows a resource",
            "first goes out of scope",
        ],
        must_not_contain: &[
            "first is live in a conditional expression ",
            "first holds an immutable reference; it can read the borrowed resource",
            "first was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when first goes out of scope.",
        ],
    },
    TooltipExpect {
        name: "match_arm_let_owned_move",
        // `s` is moved in one arm, so its merge dot may say so; `t` lives
        // in the other arm only, so it never does, and it is dropped there.
        must_contain: &[
            "s is live in a conditional expression ",
            "t goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "t is live in a conditional expression ",
            "t was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again). If the branch that ran didn't move it, its resource is \
             dropped when t goes out of scope.",
            "t goes out of scope. No resource is dropped.",
        ],
    },
    // #372: a pattern binding reborrows through a reference; the
    // reference's own borrow ends no earlier than the binding's.
    // `ref_borrow_outlasts_pattern_reborrow` checks where.
    TooltipExpect {
        name: "match_ref_param_reborrow",
        must_contain: &[
            "o is an immutable borrow from the caller",
            "Partial immutable borrow from o to s",
            "Return immutably borrowed resource from s to o",
            "o's immutable borrow ends",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "match_mut_ref_param_reborrow",
        must_contain: &[
            "o is a mutable borrow from the caller",
            "Return mutably borrowed resource from s to o",
            "o's mutable borrow ends",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "match_local_ref_reborrow",
        must_contain: &[
            "Return immutably borrowed resource from s to r",
            "Return immutably borrowed resource from r to x",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "anonymous_copy_source",
        // #547: `n` is a `usize` copied out of the match's result. That
        // value isn't in any variable, so there is no source to name;
        // "Anonymous resource" is an internal placeholder (and a usize
        // isn't a resource).
        must_contain: &[
            "n is bound to a copyable value",
        ],
        must_not_contain: &[
            "n is initialized with a copy of Anonymous resource",
        ],
    },
    // #410: a panic drops what the function owns, where it panics.
    // `return_drops_what_the_branch_owns` counts the drops per arm.
    TooltipExpect {
        name: "exit_panic_drops",
        must_contain: &[
            "t goes out of scope. Its resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "exit_unreachable_else",
        must_contain: &["t goes out of scope. Its resource is dropped."],
        // The `else` ends in the drops, not a bar
        // (`branches_that_diverge_stop_short`).
        must_not_contain: &[],
    },
    // #410: `break 'outer` drops `c` and `b`, `continue` drops `c`.
    TooltipExpect {
        name: "exit_break_continue_drops",
        must_contain: &[
            "b goes out of scope. Its resource is dropped.",
            "c goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    // #410: the guessing game's `Err(_) => continue` drops `guess`,
    // and `parse(&guess)`'s Result, whose statement it leaves.
    TooltipExpect {
        name: "exit_continue_in_match",
        must_contain: &[
            "guess goes out of scope. Its resource is dropped.",
            "This temporary Result<u32, String> is dropped here, where `continue` leaves early: \
             nothing bound it to a variable or moved it on. Its resource is dropped",
        ],
        // The Result is an unbound temporary, dropped at the `continue`
        // or at the end of its statement; no `let` borrowed it, and it
        // isn't dropped again at the end of the loop body.
        must_not_contain: &[
            "(temporary) goes out of scope at the end of the block: a let borrowed it, so it \
             was kept alive past the end of its statement. Its resource is dropped.",
        ],
    },
    // #410: `return s.len()` / `return format!("{}!", u)` drop `s` / `u`.
    TooltipExpect {
        name: "exit_return_borrowed_value",
        must_contain: &[
            "s goes out of scope. Its resource is dropped.",
            "u goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    // #410: a nested if/else before the `return` no longer hides it.
    TooltipExpect {
        name: "exit_return_after_nested_if",
        must_contain: &[
            "s goes out of scope. Its resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    // #410: a `return`'s drops follow the path to it; see the counts
    // in `return_drops_what_the_branch_owns`.
    TooltipExpect {
        name: "exit_path_dependent_drops",
        must_contain: &["t goes out of scope. Its resource is dropped."],
        must_not_contain: &[],
    },
    // #410: `s` is dropped at the end only, not at the `return` that
    // leaves before `let s = if …` binds it; see the counts in
    // `return_drops_what_the_branch_owns`.
    TooltipExpect {
        name: "exit_return_in_let_if",
        must_contain: &["t goes out of scope. Its resource is dropped."],
        must_not_contain: &[],
    },
    // #410 with #457: `g`'s `return` leaves while `parse(&s)`'s Result
    // is alive and drops it; `f`'s `return` comes after `make()`'s
    // String was dropped and doesn't drop it again (it used to, worded
    // as a `let`-borrowed temporary).
    TooltipExpect {
        name: "exit_temporary_drops",
        must_contain: &[
            "This temporary Result<u32, String> is dropped here, where `return` leaves early: \
             nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary Result<u32, String> is dropped at the end of this statement: \
             nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary String is dropped at the end of this statement: nothing bound it \
             to a variable or moved it on. Its resource is dropped",
        ],
        must_not_contain: &[
            "(temporary) goes out of scope at the end of the block: a let borrowed it, so it \
             was kept alive past the end of its statement. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "ref_unknown_lender_end",
        // `let r = pick();`, `let r = *rr[0];` (rr: Vec<&&i32>) and
        // `let s = *b;` (b: Box<&i32>) borrow from something the plugin
        // can't name, so there is no column to return the borrow to.
        // The end must not read as r returned to *r, itself. This
        // already holds on main (the wording from #178 covers these
        // shapes); the entry pins it.
        must_contain: &[
            "Copy from pick to r",
            "r's immutable borrow ends (what it borrows from isn't shown)",
            "Copy from *rr[0] to r",
            "Copy from *b to s",
            "s's immutable borrow ends (what it borrows from isn't shown)",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from r to *r",
            "Return immutably borrowed resource from s to *s",
            "Return immutably borrowed value from r to *r",
            "Return immutably borrowed value from s to *s",
        ],
    },
    // #598: two variables named `x` whose scopes don't overlap shared one
    // column, registered by the first: a `&mut String` here, so the
    // second `x`, a `String`, was looked up as a reference whose borrow
    // had ended (a panic). A variable of another kind (reference, owner,
    // Copy value) now gets a column of its own, labelled `x` too: it
    // shadows nothing, so no subscript (#308); its header says it is a
    // different variable. `same_name_kinds_get_separate_columns` checks
    // there are two `x` columns.
    TooltipExpect {
        name: "ref_binding_name_reused",
        must_contain: &[
            "x, immutable. A different variable from the x declared on line 8 (separate scopes, so only one exists at a time)",
            "x, immutable. A different variable from the x declared on line 4 (separate scopes, so only one exists at a time)",
            "Partial Mutable borrow from t.0 to x",
            "Partial move from r to x",
            "x acquires ownership of a resource",
            "x goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "x₂",
        ],
    },
    // The second `x` is a `&mut String`, the first a `&String`: the
    // second was drawn in the first's column, as a `&T` that is itself
    // borrowed by push_str.
    TooltipExpect {
        name: "ref_binding_name_reused_mut",
        must_contain: &[
            "Partial immutable borrow from t.0 to x",
            "Partial Mutable borrow from t.0 to x",
            "the resource x points to is mutably borrowed by push_str through x; x can't be used while the borrow is live",
            "x, immutable. A different variable from the x declared on line 9 (separate scopes, so only one exists at a time)",
        ],
        must_not_contain: &[
            "=x is mutably borrowed by push_str; x can't be used while the borrow is live",
            "x₂",
        ],
    },
    // The same with two `let`s in sibling blocks: the `String` was drawn
    // as a reference.
    TooltipExpect {
        name: "let_ref_then_owner_same_name",
        must_contain: &[
            "x, immutable. A different variable from the x declared on line 8 (separate scopes, so only one exists at a time)",
            "x, immutable. A different variable from the x declared on line 4 (separate scopes, so only one exists at a time)",
            "Move from String::from to x",
            "x acquires ownership of a resource",
            "x goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "x is bound to an immutable reference",
            "x₂",
        ],
    },
    // Sibling blocks with an `i32` and a `u8` `x`: both Copy values,
    // one column (#598 review).
    TooltipExpect {
        name: "same_name_copy_kinds_share",
        must_contain: &[
            "x is bound to a copyable value",
        ],
        must_not_contain: &[
            "A different variable from",
            "x₂",
        ],
    },
    // #642: a `String` `x` and a later `Light` `x` (no drop glue) shared a
    // column, registered by the first, so the enum was called a
    // "resource". Now each has its own column.
    TooltipExpect {
        name: "same_name_drop_kinds_split",
        must_contain: &[
            "x acquires ownership of a resource",
            "x goes out of scope. Its resource is dropped.",
            "x holds a value",
            "x is bound to a value",
            "x's value is moved",
            "x, immutable. A different variable from the x declared on line 9 (separate scopes, so only one exists at a time)",
            "x, immutable. A different variable from the x declared on line 5 (separate scopes, so only one exists at a time)",
        ],
        must_not_contain: &[
            // The enum (no drop glue) is not a resource; #619's plugin
            // said this of its `x`, which shared the `String`'s column.
            "x's resource is moved",
            "x₂",
        ],
    },
    // Two non-Copy owners without drop glue still share one column.
    TooltipExpect {
        name: "same_name_plain_owners_share",
        must_contain: &[
            "x is bound to a value",
            "x's value is moved",
        ],
        must_not_contain: &[
            "A different variable from",
            "x₂",
            "resource",
        ],
    },
    // A bare macro call as an arm's body (`Some(x) => println!(..)`) has
    // a span inside the macro, which put `x` in scope to the end of the
    // fn: the next match's `x` was said to shadow it.
    TooltipExpect {
        name: "match_macro_arm_name_reused",
        must_contain: &[
            "Partial move from a to x",
            "Partial move from b to x",
        ],
        must_not_contain: &[
            "x₂ shadows x",
            "Partial move from b to x₂",
        ],
    },
    // ─── The `?` operator's early return (#430) ─────────────────────
    TooltipExpect {
        name: "try_drops_owned",
        // `b` and `c` are owned at both `?`. `a` was moved into `take`
        // before them, `n` and `opt` are `Copy`: no dot for those.
        must_contain: &[
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and b's \
             resource is dropped. If the `?` succeeds, nothing happens to b here.",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and c's \
             resource is dropped. If the `?` succeeds, nothing happens to c here.",
        ],
        must_not_contain: &[
            "=Only if the `?` here returns early (on an `Err` or `None`): the function returns and a's \
             resource is dropped. If the `?` succeeds, nothing happens to a here.",
            "=Only if the `?` here returns early (on an `Err` or `None`): the function returns and n's \
             resource is dropped. If the `?` succeeds, nothing happens to n here.",
            "=Only if the `?` here returns early (on an `Err` or `None`): the function returns and opt's \
             resource is dropped. If the `?` succeeds, nothing happens to opt here.",
        ],
    },
    TooltipExpect {
        name: "try_in_branch",
        must_contain: &[
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and t's \
             resource is dropped. If the `?` succeeds, nothing happens to t here.",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and u's \
             resource is dropped. If the `?` succeeds, nothing happens to u here.",
            // `v` is a `Vec<String>`: its drop says what it holds (#542).
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and v's \
             resource (including every String it holds) is dropped. If the `?` succeeds, nothing \
             happens to v here.",
        ],
        must_not_contain: &[
            "the function returns and v's resource is dropped. If",
            // `s` is a reference.
            "=Only if the `?` here returns early (on an `Err` or `None`): the function returns and s's \
             resource is dropped. If the `?` succeeds, nothing happens to s here.",
        ],
    },
    // The `?` dots go on the branch columns once the `if` is drawn with
    // them (an `if`/`else`, or an `if` whose body returns, #429).
    TooltipExpect {
        name: "try_in_if_else_branch",
        must_contain: &[
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and t's \
             resource is dropped. If the `?` succeeds, nothing happens to t here.",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and u's \
             resource is dropped. If the `?` succeeds, nothing happens to u here.",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and v's \
             resource (including every String it holds) is dropped. If the `?` succeeds, nothing \
             happens to v here.",
        ],
        must_not_contain: &[
            "=Only if the `?` here returns early (on an `Err` or `None`): the function returns and s's \
             resource is dropped. If the `?` succeeds, nothing happens to s here.",
        ],
    },
    TooltipExpect {
        name: "try_in_loop_two_on_a_line",
        must_contain: &[
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and label's \
             resource is dropped. If the `?` succeeds, nothing happens to label here.",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and note's \
             resource is dropped. If the `?` succeeds, nothing happens to note here.",
        ],
        must_not_contain: &[
            // `total` is a `Copy`; `pair` and `items` are references.
            "=Only if the `?` here returns early (on an `Err` or `None`): the function returns and total's \
             resource is dropped. If the `?` succeeds, nothing happens to total here.",
            "=Only if the `?` here returns early (on an `Err` or `None`): the function returns and pair's \
             resource is dropped. If the `?` succeeds, nothing happens to pair here.",
        ],
    },
    // ─── A whole struct overwritten ends its old borrows (#538) ──────
    TooltipExpect {
        name: "borrow_struct_overwrite_whole",
        // `hold = H { r: &t };` drops hold's old value, so s's borrow
        // ends there (rustc accepts the `&mut s` right after; unlike
        // `hold.r = &t`, #531). Main drew no end and no return arrow
        // for s, x, y or u. `used_before` ends a's borrow at the
        // println! that last uses it; see
        // `whole_struct_overwrite_ends_old_borrow_at_the_write`.
        must_contain: &[
            "s's resource is no longer immutably borrowed",
            "Return immutably borrowed resource from hold.r to s",
            "a's resource is no longer immutably borrowed",
            "x's resource is no longer immutably borrowed",
            "y's resource is no longer immutably borrowed",
            "Return immutably borrowed resource from p.a to x",
            "Return immutably borrowed resource from p.b to y",
            "u's resource is no longer mutably borrowed",
            "Return mutably borrowed resource from h.r to u",
            "s's resource is no longer mutably borrowed",
        ],
        // Main's bug was an omission (no end, no return arrow), so
        // there is no false text to forbid; the must_contain entries
        // above fail on main.
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "ref_copy_value_wording",
        // Copying `&i32` / `&str` copies the reference, not a value;
        // moving `&mut i32` likewise. Controls: `&String` / `&mut String`
        // point to resources.
        must_contain: &[
            "r1's reference is copied (not the value it points to)",
            "w1's reference is copied (not the value it points to)",
            "m1's mutable reference is moved (not the value it points to)",
            "q1's reference is copied (not the resource it points to)",
            "u1's mutable reference is moved (not the resource it points to)",
        ],
        must_not_contain: &[
            "r1's reference is copied (not the resource it points to)",
            "w1's reference is copied (not the resource it points to)",
            "m1's mutable reference is moved (not the resource it points to)",
            "q1's reference is copied (not the value it points to)",
            "u1's mutable reference is moved (not the value it points to)",
        ],
    },
    // ─── `let` patterns that ignore parts of a literal (#439) ───────
    TooltipExpect {
        name: "let_pattern_literal_unbound_moves",
        // In `let (ref a, ..) = (s, t);` the tuple literal is a temporary
        // that both `s` and `t` are moved into (rustc: "borrow of moved
        // value" on either); `..` and `_` don't leave them behind. The
        // same for `let (x, _) = (m, n);`, a struct literal's omitted
        // field, and an array's `..`. `let _ = z;` moves nothing. The
        // temporary has a `(temporary N)` column, like #400's and #457's;
        // it is dropped at the end of the statement, or with a `ref`
        // binding at the end of the block (rustc, Drop-printing program),
        // and its tooltips say which. It holds what was moved into it,
        // so its drop is "Its resource is dropped", never "No resource".
        must_contain: &[
            "(temporary) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
            "(temporary 2) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
            "(temporary 6) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
            "The temporary that the literal on the right of this let builds takes the value of s. A `ref` binding borrows from it, so it lives until the end of the enclosing block, not just to the end of the statement",
            "The temporary that the literal on the right of this let builds takes the value of t. A `ref` binding borrows from it, so it lives until the end of the enclosing block, not just to the end of the statement",
            "The temporary that the literal on the right of this let builds takes the value of n. No `ref` binding borrows from it, so Rust drops it at the end of this statement",
            "This temporary (String, String), built by the literal on the right of the let, is dropped at the end of this statement. What the pattern didn't bind by value stays in it and is dropped with it. Its resource is dropped",
            "Move from s to (temporary)",
            "Move from t to (temporary)",
            "Move from n to (temporary 4)",
            "Move from m to x",
            "Immutable borrow from (temporary) to a",
            "s's resource is moved",
            "t's resource is moved",
            "c's resource is moved",
            "d's resource is moved",
            "f's resource is moved",
            "g's resource is moved",
            "h's resource is moved",
            "m's resource is moved",
            "n's resource is moved",
            "q's resource is moved",
            "r's resource is moved",
            "u's resource is moved",
            "w's resource is moved",
            "z goes out of scope. Its resource is dropped.",
            "a immutably borrows a resource",
            "e immutably borrows a resource",
            "i immutably borrows a resource",
            "name immutably borrows a resource",
            "k immutably borrows a resource",
            "x goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            // A `let`'s literal isn't one that nothing binds (#570's wording).
            "This temporary takes the value of",
            "(temporary) goes out of scope. No resource is dropped.",
            "(temporary 2) goes out of scope. No resource is dropped.",
            "(temporary 3) goes out of scope. No resource is dropped.",
            "(temporary 5) goes out of scope. No resource is dropped.",
            "(temporary 6) goes out of scope. No resource is dropped.",
            "Move from n to x",
            "s goes out of scope. Its resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
            "c goes out of scope. Its resource is dropped.",
            "d goes out of scope. Its resource is dropped.",
            "f goes out of scope. Its resource is dropped.",
            "g goes out of scope. Its resource is dropped.",
            "h goes out of scope. Its resource is dropped.",
            "m goes out of scope. Its resource is dropped.",
            "n goes out of scope. Its resource is dropped.",
            "q goes out of scope. Its resource is dropped.",
            "r goes out of scope. Its resource is dropped.",
            "u goes out of scope. Its resource is dropped.",
            "w goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "let_pattern_literal_temp_scopes",
        // The temporary lives in the block of its `let`: dropped at the
        // end of the statement (`(x, _) = (a, b)`, b), or with a `ref`
        // binding at the end of the block (`(_, ref y) = (c, d)`). A
        // literal of Copy values (`(i, j)`) owns nothing to drop, so it
        // gets no column and nothing claims a drop.
        must_contain: &[
            "Move from b to (temporary)",
            "Move from a to x",
            "The temporary that the literal on the right of this let builds takes the value of b. No `ref` binding borrows from it, so Rust drops it at the end of this statement",
            "Move from c to (temporary 2)",
            "Move from d to (temporary 2)",
            "(temporary 2) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
            "j's value is copied",
        ],
        must_not_contain: &[
            "Move from b to x",
            "b goes out of scope. Its resource is dropped.",
            "d goes out of scope. Its resource is dropped.",
            "Move from i to (temporary 3)",
            "(temporary 3)",
        ],
    },
    TooltipExpect {
        name: "collection_drop_holds",
        // Dropping a collection / Option / Box also drops what it holds;
        // the element type is known, the count isn't, so none is given.
        // Elements that need no drop (Vec<i32>, Option<i32>) add nothing.
        must_contain: &[
            "words goes out of scope. Its resource is dropped (including every String it holds).",
            "pair goes out of scope. Its resource is dropped (including every String it holds).",
            "queue goes out of scope. Its resource is dropped (including every String it holds).",
            "slice goes out of scope. Its resource is dropped (including every String it holds).",
            "maybe goes out of scope. Its resource is dropped (including the String it holds, if any).",
            "boxed goes out of scope. Its resource is dropped (including the String it holds).",
            "both goes out of scope. Its resource is dropped (including every key and value it holds).",
            "keys goes out of scope. Its resource is dropped (including every key it holds).",
            "vals goes out of scope. Its resource is dropped (including every value it holds).",
            "rows goes out of scope. Its resource is dropped (including every Vec<String> it holds).",
            "again acquires ownership of a new resource; its previous resource (including every String it holds) is dropped",
            "gone's resource is moved into drop, which drops it (including every String it holds)",
            "nums goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "its previous resource is dropped (including",
            "=words goes out of scope. Its resource is dropped.",
            "=pair goes out of scope. No resource is dropped.",
            "=queue goes out of scope. Its resource is dropped.",
            "=slice goes out of scope. Its resource is dropped.",
            "=maybe goes out of scope. Its resource is dropped.",
            "=boxed goes out of scope. Its resource is dropped.",
            "=both goes out of scope. Its resource is dropped.",
            "=keys goes out of scope. Its resource is dropped.",
            "=vals goes out of scope. Its resource is dropped.",
            "=rows goes out of scope. Its resource is dropped.",
            "=again acquires ownership of a new resource; its previous resource is dropped",
            "=gone's resource is moved into drop, which drops it",
            "nums goes out of scope. Its resource is dropped (including every i32 it holds).",
            "=maybe_num goes out of scope. Its resource is dropped.",
            "maybe_num goes out of scope. Its resource is dropped (including the i32 it holds, if any).",
        ],
    },
    TooltipExpect {
        name: "collection_drop_holds_flag",
        // The drop-flag scope end, the branch merge and `drop(x)` carry the
        // same parenthetical.
        must_contain: &[
            "words goes out of scope. Its resource (including every String it holds) is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "maybe goes out of scope. Its resource (including the String it holds, if any) is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "words was moved in at least one branch above, so it can't be used after the branches (until it's assigned again). If the branch that ran didn't move it, its resource (including every String it holds) is dropped when words goes out of scope.",
            "maybe's resource is moved into drop, which drops it (including the String it holds, if any)",
            "_v goes out of scope. Its resource is dropped (including every String it holds).",
        ],
        must_not_contain: &[
            "Its resource is dropped (including every String it holds) here only if",
            "Its resource is dropped (including the String it holds, if any) here only if",
            "its resource is dropped (including every String it holds) when words goes out of scope",
            "words goes out of scope. Its resource is dropped here only if",
            "maybe goes out of scope. Its resource is dropped here only if",
            "its resource is dropped when words goes out of scope.",
            "=maybe's resource is moved into drop, which drops it",
            "=_v goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "collection_drop_holds_exit",
        // The drops at early exits (#410: `return`, panic, `break`;
        // #430: `?`) carry the same parenthetical as the scope end.
        must_contain: &[
            "v goes out of scope. Its resource is dropped (including every String it holds).",
            "o goes out of scope. Its resource is dropped (including the String it holds, if any).",
            "m goes out of scope. Its resource is dropped (including every key it holds).",
            "w goes out of scope. Its resource is dropped (including every String it holds).",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and names's \
             resource (including every String it holds) is dropped. If the `?` succeeds, nothing happens \
             to names here.",
        ],
        must_not_contain: &[
            "resource is dropped (including every String it holds). If the `?`",
            "=v goes out of scope. Its resource is dropped.",
            "=o goes out of scope. Its resource is dropped.",
            "=m goes out of scope. Its resource is dropped.",
            "=w goes out of scope. Its resource is dropped.",
            "the function returns and names's resource is dropped. If",
        ],
    },
    TooltipExpect {
        name: "array_of_strings_drop",
        // `[String; N]` owns its Strings: dropping the array drops each
        // one (#544). Strings moved into an array literal are owned by
        // the array, so they don't go out of scope with a drop of their
        // own. A `[i32; 3]` has nothing to drop.
        must_contain: &[
            "names goes out of scope. Its resource is dropped (including every String it holds).",
            "moved goes out of scope. Its resource is dropped (including every String it holds).",
            "first goes out of scope. No resource is dropped.",
            "second goes out of scope. No resource is dropped.",
            "again acquires ownership of a new resource; its previous resource (including every String it holds) is dropped",
            "again goes out of scope. Its resource is dropped (including every String it holds).",
            "gone's resource is moved into drop, which drops it (including every String it holds)",
            "gone goes out of scope. No resource is dropped.",
            "maybe goes out of scope. Its resource (including every String it holds) is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "its previous resource is dropped (including",
            "Its resource is dropped (including every String it holds) here only if",
            "=names goes out of scope. No resource is dropped.",
            "=moved goes out of scope. No resource is dropped.",
            "first goes out of scope. Its resource is dropped",
            "second goes out of scope. Its resource is dropped",
            "nums goes out of scope. Its resource is dropped (including every i32 it holds).",
        ],
    },
    // ─── `ref` bindings of values and literals of fresh values (#483) ─
    TooltipExpect {
        name: "let_ref_temporary_value",
        // `let ref t = String::from("tmp");` borrows a temporary that
        // lives, and is dropped, at the end of the block; so do `ref
        // mut m = make()` and the part `ref name` borrows of `make_p()`.
        // Before, `t` borrowed nothing drawn and no tooltip said the
        // String was dropped.
        must_contain: &[
            "Move from String::from to (temporary)",
            "Move from make to (temporary 2)",
            "Move from make_p to (temporary 3)",
            "Immutable borrow from (temporary) to t",
            "Mutable borrow from (temporary 2) to m",
            "Immutable borrow from (temporary 3) to name",
            "(temporary) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
            "(temporary 2) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
            "(temporary 3) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
        ],
        must_not_contain: &[
            "t's immutable borrow ends (what it borrows from isn't shown)",
            "m's mutable borrow ends (what it borrows from isn't shown)",
            "name's immutable borrow ends (what it borrows from isn't shown)",
            "(temporary 4)",
        ],
    },
    TooltipExpect {
        name: "let_literal_fresh_elements",
        // The tuple `(make(), make())` and the array of fresh values
        // are temporaries even though no variable is moved into them:
        // with `ref a` / `ref k` they live to the end of the block, and
        // `let (x, _) = ..` drops its literal at the end of the
        // statement with what `_` left in it.
        must_contain: &[
            "Immutable borrow from (temporary) to a",
            "Immutable borrow from (temporary 3) to k",
            "(temporary) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
            "(temporary 3) goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
            "(temporary) acquires ownership of a resource. This temporary value is borrowed by a let, so it lives until the end of the enclosing block, not just to the end of the statement",
            "(temporary 3) acquires ownership of a resource. This temporary value is borrowed by a let, so it lives until the end of the enclosing block, not just to the end of the statement",
            "This temporary (String, String), built by the literal on the right of the let, is dropped at the end of this statement. What the pattern didn't bind by value stays in it and is dropped with it. Its resource is dropped",
            "The literal on the right of this let builds a temporary. No `ref` binding borrows from it, so Rust drops it at the end of this statement, with whatever the pattern didn't bind by value",
        ],
        must_not_contain: &[
            "a's immutable borrow ends (what it borrows from isn't shown)",
            "k's immutable borrow ends (what it borrows from isn't shown)",
            "(temporary 4)",
        ],
    },
    TooltipExpect {
        name: "let_temp_dropped_at_return",
        // `return 0` leaves `f` while the `let`-extended temporary is
        // alive and drops it there. The path that reaches the end of
        // the block still says so (the tooltips are told apart by
        // position; the second fixture has no such path).
        must_contain: &[
            "(temporary) goes out of scope here, where `return` leaves early: a let borrowed it, so it \
             lived past its statement. Its resource is dropped.",
            "(temporary) goes out of scope at the end of the block: a let borrowed it, so it was kept \
             alive past the end of its statement. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "let_temp_dropped_at_return_only",
        // Every path out of `f` is a `return`: the temporary is never
        // dropped at the end of the block.
        must_contain: &[
            "(temporary) goes out of scope here, where `return` leaves early: a let borrowed it, so it \
             lived past its statement. Its resource is dropped.",
        ],
        must_not_contain: &["goes out of scope at the end of the block"],
    },
    TooltipExpect {
        name: "let_temp_dropped_at_break",
        // The early exits #410 added drop the `let`-extended temporary
        // declared in the loop body too, and say where. The `_ => {}`
        // path reaches the end of the body (that drop keeps "at the end
        // of the block", so it can't be forbidden by text here).
        must_contain: &[
            "(temporary) goes out of scope here, where `break` leaves early: a let borrowed it, so it \
             lived past its statement. Its resource is dropped.",
            "(temporary) goes out of scope here, where `continue` leaves early: a let borrowed it, so it \
             lived past its statement. Its resource is dropped.",
            "(temporary) goes out of scope at the end of the block: a let borrowed it, so it was kept \
             alive past the end of its statement. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "let_temp_dropped_at_panic",
        // Every path out of `g` panics or returns: the temporary is
        // dropped as the panic unwinds, or at the `return`.
        must_contain: &[
            "(temporary) goes out of scope here, as the panic unwinds: a let borrowed it, so it lived \
             past its statement. Its resource is dropped.",
            "(temporary) goes out of scope here, where `return` leaves early: a let borrowed it, so it \
             lived past its statement. Its resource is dropped.",
        ],
        must_not_contain: &["goes out of scope at the end of the block"],
    },
    // ─── Tuple patterns in `if let`; tuple-literal scrutinees (#327) ─
    TooltipExpect {
        name: "iflet_tuple_pattern",
        // `if let (Some(a), Some(b)) = (x, y)` binds `a` / `b` from the
        // tuple's elements. Building the tuple moves `x` and `y` into
        // it, so they own nothing afterwards, even when the pattern
        // doesn't bind from them (`Some(_)`) or doesn't match (the else
        // branch). `Some((key, n))` on an `Option<(String, i32)>` binds
        // both names.
        must_contain: &[
            "Move from x to a",
            "Move from y to b",
            "x goes out of scope. No resource is dropped.",
            "y goes out of scope. No resource is dropped.",
            "Move from q to r",
            "p was moved or dropped in every branch above",
            "q was moved or dropped in every branch above",
            "p goes out of scope. No resource is dropped.",
            "Move from entry to key",
            "Copy from entry to n",
            "key goes out of scope. Its resource is dropped.",
            // The tuple (p, q) is dropped (edition 2024, checked with a
            // Drop-printing program) at the end of the if branch, with
            // p's value in it (q's String went to r), or, when the
            // pattern fails, before the else branch runs, with both.
            // It has no column, so the drop is stated on p's / q's.
            "p's resource is moved into the temporary tuple (p, q)",
            "The temporary tuple (p, q) is dropped here, at the end of the if branch, and with it p's resource",
            "The pattern didn't match, so the temporary tuple (p, q) is dropped here, before the else branch runs, and with it p's and q's resources",
            // No else: a failed match drops (x, y) and both values; a
            // match moves both Strings out, so nothing is left in it.
            "The pattern didn't match, so the temporary tuple (x, y) is dropped here, and with it x's and y's resources",
        ],
        must_not_contain: &[
            "=x goes out of scope. Its resource is dropped.",
            "=y goes out of scope. Its resource is dropped.",
            "=p goes out of scope. Its resource is dropped.",
            "=q goes out of scope. Its resource is dropped.",
            "=entry goes out of scope. Its resource is dropped.",
            // q's value isn't in the tuple on the if path.
            "=The temporary tuple (p, q) is dropped here, at the end of the if branch, and with it p's and q's resources",
            "The temporary tuple (x, y) is dropped here, at the end of the if branch",
        ],
    },
    TooltipExpect {
        name: "match_tuple_literal_moves",
        // `match (x, y)` moves both into the tuple, whatever arm runs:
        // the `_` arm shows them moved too (#390). A nested tuple
        // pattern (`(first, (n, second))`) on a tuple variable binds
        // every name from its own element.
        must_contain: &[
            "Move from x to a",
            "x was moved or dropped in every branch above",
            "y was moved or dropped in every branch above",
            "Move from t.0 to first",
            "Copy from t.1.0 to n",
            "Move from t.1.1 to second",
            "second goes out of scope. Its resource is dropped.",
            // The `_` arm takes nothing out of the tuple (x, y): it is
            // dropped after the arm, with both values in it.
            "x's resource is moved into the temporary tuple (x, y)",
            "The temporary tuple (x, y) is dropped after this arm, when the match ends, and with it x's and y's resources",
        ],
        must_not_contain: &[
            "x was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.",
            "y was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.",
            "Partial move from t to first",
            // The first arm moves both Strings out: nothing is left.
            "and with it x's resource",
            "and with it y's resource",
        ],
    },
    TooltipExpect {
        name: "tuple_scrutinee_drops",
        // The tuple a tuple-literal scrutinee builds has no column, so
        // where it's dropped is stated on the columns of the variables
        // whose values are in it (#327). Timing is edition 2024's,
        // checked with a Drop-printing program: a match keeps it to the
        // end of the match (or until an arm leaves early); an if let
        // to the end of the if branch (or its early `return`).
        must_contain: &[
            // `early`: arm 1 moved x's String out, y's value is left.
            "The temporary tuple (x, y) is dropped after this arm, when the match ends, and with it y's resource",
            // `_ => return 0` drops the tuple as it returns.
            "The temporary tuple (x, y) is dropped when this arm leaves the match early, and with it x's and y's resources",
            "x goes out of scope. No resource is dropped.",
            // `early_if`: p's value goes when the branch returns.
            "The temporary tuple (p, q) is dropped when the if branch leaves early, and with it p's resource",
            "The pattern didn't match, so the temporary tuple (p, q) is dropped here, and with it p's and q's resources",
            // A one-arm match is drawn inline: the drop is on the
            // match's closing line.
            "The temporary tuple (x, y) is dropped here, at the end of the match, and with it x's and y's resources",
            // `E::Two(c, _)` moves one String out of e's value; the
            // other is dropped with the tuple.
            "The temporary tuple (e, f) is dropped here, at the end of the if branch, and with it the rest of e's resource",
            "The pattern didn't match, so the temporary tuple (e, f) is dropped here, and with it e's and f's resources",
        ],
        must_not_contain: &[
            // x's value went into the tuple; it isn't x's to drop at
            // the `return`.
            "=x goes out of scope. Its resource is dropped.",
            "=y goes out of scope. Its resource is dropped.",
            "=p goes out of scope. Its resource is dropped.",
            "=e goes out of scope. Its resource is dropped.",
            "=f goes out of scope. Its resource is dropped.",
            // g took f's String: nothing of f's is left in the tuple.
            "the rest of e's resource and f's resource",
        ],
    },
    TooltipExpect {
        name: "tuple_scrutinee_ref",
        // `(Some(ref a), _)` on `(x, y)`: x's value was moved into the
        // temporary tuple, and `a` borrows the String in that tuple,
        // not x. The String stays in the tuple and is dropped with it
        // after the arm, so x never owns a value again (#580). Same
        // for `ref mut s` in an `if let`: the tuple, holding p's
        // value, is dropped at the end of the if branch.
        must_contain: &[
            "x's resource is moved into the temporary tuple (x, y)",
            "The temporary tuple (x, y) is dropped after this arm, when the match ends, and with it x's and y's resources",
            "x was moved or dropped in every branch above",
            "x goes out of scope. No resource is dropped.",
            "p's resource is moved into the temporary tuple (p, q)",
            "The temporary tuple (p, q) is dropped here, at the end of the if branch, and with it p's resource",
            "p was moved or dropped in every branch above",
            "p goes out of scope. No resource is dropped.",
            // The `ref mut s` end dot keeps its text (#658); the arrow
            // that used to carry the "(what it borrows from isn't
            // shown)" text is gone (see `ref_mut_out_of_temporary_tuple_ends_without_arrow`).
            "s's mutable borrow ends",
        ],
        must_not_contain: &[
            "(what it borrows from isn't shown)",
            "Immutable borrow from x to a",
            "Return immutably borrowed resource from a to x",
            "x's resource is immutably borrowed by a",
            "Mutable borrow from p to s",
            "Return mutably borrowed resource from s to p",
            "x was moved in at least one branch above",
            "p was moved in at least one branch above",
            "x goes out of scope. Its resource is dropped here only if no branch above moved it",
            "p goes out of scope. Its resource is dropped here only if no branch above moved it",
        ],
    },
    TooltipExpect {
        name: "tuple_scrutinee_exit_drops",
        // A `return` that leaves a match arm or an `if let` branch from
        // inside a nested `if` drops the tuple scrutinee right there, as
        // it drops the function's other values (#410), checked with a
        // Drop-printing program: at `return 0` the arm's own values go
        // first, then the tuple (holding y's value), then the locals
        // declared before the match. `std::process::exit` runs no
        // destructors, so that arm drops nothing.
        must_contain: &[
            "The temporary tuple (x, y) is dropped when this arm leaves the match early, and with it y's resource",
            "The temporary tuple (x, y) is dropped after this arm, when the match ends, and with it y's resource",
            "The temporary tuple (s, t) is dropped when this arm leaves the match early, and with it s's and t's resources",
            "The temporary tuple (p, q) is dropped when the if branch leaves early, and with it p's resource",
            "The temporary tuple (p, q) is dropped here, at the end of the if branch, and with it p's resource",
            "m's resource is moved into the temporary tuple (m, n)",
        ],
        must_not_contain: &[
            // `_ => std::process::exit(1)`: the process ends without
            // dropping anything.
            "The temporary tuple (m, n) is dropped when this arm leaves the match early",
            "=y goes out of scope. Its resource is dropped.",
            "=p goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "holder_ref_ends_first",
        // #575: `x` (a `&mut i32` moved out of what `it.next()` returns)
        // and `t` (a `&i32` copied out of `third`) stop borrowing `v` at
        // their last use, while `it` / `third` go on holding the borrow.
        // Each gives its borrow back there, and `v` stays borrowed.
        must_contain: &[
            "Mutable borrow from v to x: x is a reference moved out of the value next returned",
            "Return mutably borrowed resource from x to v",
            "v's resource is no longer borrowed by x, but is still mutably borrowed by it",
            "Return mutably borrowed resource from it to v",
            "Immutable borrow from v to t: t is a reference copied out of third",
            "Return immutably borrowed resource from t to v",
            "v's resource is no longer borrowed by t, but is still immutably borrowed by third",
            "Return immutably borrowed resource from third to v",
        ],
        must_not_contain: &[
            "Return mutably borrowed resource from x to *x",
            "Return immutably borrowed resource from t to *t",
        ],
    },
    TooltipExpect {
        name: "shadow_sibling_blocks",
        // #589: the first block's `s` was moved by `drop`, so nothing is
        // dropped at its `}`; on main that `}` said "Its resource is
        // dropped" (a false claim) and the second block's `}` said nothing.
        must_contain: &[
            "s goes out of scope. No resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "nested_block_in_branch_scope_end",
        // #591: each inner block's String is dropped at that block's `}`.
        must_contain: &[
            "u goes out of scope. Its resource is dropped.",
            "w goes out of scope. Its resource is dropped.",
            "m goes out of scope. Its resource is dropped.",
            "k goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    // ─── A let-chain that moves, then fails (#281) ─────────────────
    // A part of the condition that moves an outer variable before a part
    // that can still be false: the else branch (or the path that skips
    // the body) starts with the variable maybe-moved. Before, the
    // plugin refused these; main (before #198) drew no move and said
    // `x`'s resource is dropped at the end.
    TooltipExpect {
        name: "edition2024/if_let_chain_else_move",
        must_contain: &[
            "Move from x to a",
            "Move from a to consume",
            "x may have been moved: when the condition moves x to a and then turns out false, the else branch runs after that move, so x can't be used here. When the condition fails after that move, a goes out of scope and its resource is dropped.",
            "x goes out of scope. Its resource (including the String it holds, if any) is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            // The condition, not a branch, may have moved it (maintainer, #581).
            "only if no branch above moved it",
            // main before #198: no move, unconditional drop.
            "=x goes out of scope. Its resource is dropped.",
            "=x goes out of scope. Its resource is dropped (including the String it holds, if any).",
            "=x goes out of scope. No resource is dropped.",
            "x isn't moved in any of the branches above",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_no_else_move",
        must_contain: &[
            "Move from x to a",
            "x may have been moved: when the condition moves x to a and then turns out false, the body is skipped but the move has happened, so x can't be used after the if. When the condition fails after that move, a goes out of scope and its resource is dropped.",
            "x goes out of scope. Its resource (including the String it holds, if any) is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            // The condition, not a branch, may have moved it (maintainer, #581).
            "only if no branch above moved it",
            "=x goes out of scope. Its resource is dropped.",
            "=x goes out of scope. Its resource is dropped (including the String it holds, if any).",
            "=x goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_call_move",
        // `take` gets `s`; what it does with it is its own business, so
        // nothing is said to be dropped on the way into the else.
        must_contain: &[
            "Move from s to take",
            "s may have been moved: when the condition moves s to take and then turns out false, the else branch runs after that move, so s can't be used here",
            "s goes out of scope. Its resource is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            // The condition, not a branch, may have moved it (maintainer, #581).
            "only if no branch above moved it",
            "=s goes out of scope. Its resource is dropped.",
            "=s goes out of scope. No resource is dropped.",
            "take goes out of scope",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_irrefutable_move",
        // `(p, q)` always matches: the else branch always runs after the
        // move, so it's "was moved", not "may have been".
        must_contain: &[
            "pair.0 was moved: the condition moves pair.0 to p before it can turn out false, so the else branch runs after that move, and pair.0 can't be used here. When the condition fails after that move, p goes out of scope and its resource is dropped.",
            "pair.1 was moved: the condition moves pair.1 to q before it can turn out false, so the else branch runs after that move, and pair.1 can't be used here. When the condition fails after that move, q goes out of scope and its resource is dropped.",
            "pair goes out of scope. pair.0 and pair.1 were moved out earlier, so no resource is dropped.",
        ],
        must_not_contain: &[
            "pair.0 may have been moved",
            "pair.1 may have been moved",
            "pair.0 goes out of scope. Its resource is dropped",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_else_reassign",
        // The old value is still in `x` only if the condition didn't move
        // it, so its drop at the reassignment is decided by a drop flag.
        must_contain: &[
            "x goes out of scope. Its resource (including the String it holds, if any) is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
            "x acquires ownership of a new resource. Its previous resource is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            // The condition, not a branch, may have moved it (maintainer, #581).
            "only if no branch above moved it",
            "x acquires ownership of a new resource; its previous resource is dropped",
        ],
    },
    // The drop-flag wording where the condition may have moved a field /
    // a closure (#581 review): "nothing above", not "no branch above".
    TooltipExpect {
        name: "edition2024/if_let_chain_field_move",
        must_contain: &[
            "p.name may have been moved: when the condition moves p.name to a and then turns out false, the else branch runs after that move, so p.name can't be used here. When the condition fails after that move, a goes out of scope and its resource is dropped.",
            "p goes out of scope. p.name may have been moved out above, so it's dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag). p.id is dropped.",
            "p.name goes out of scope. Its resource (including the String it holds, if any) is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &["only if no branch above moved it", "moved out in at least one branch above"],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_closure_move",
        must_contain: &[
            "f may have been moved: when the condition moves f to run and then turns out false, the else branch runs after that move, so f can't be used here",
            "f goes out of scope. Its captured resources are dropped here only if nothing above moved or called it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &["only if no branch above called it"],
    },
    // #599: a `ref` binding's borrow ends at its last use, where the
    // borrow checker ends it, so the lender can be used again later in
    // the arm. `match_ref_binding_borrow_ends_at_last_use` checks the
    // rows (the texts are the same as before, on other rows).
    TooltipExpect {
        name: "match_ref_binding_last_use",
        must_contain: &[
            "t.0's resource is immutably borrowed by x; t.0 is read-only while the borrow is live",
            "t.0's resource is no longer immutably borrowed",
            "t.0's resource is mutably borrowed by push_str; t.0 can't be used while the borrow is live",
            "x's immutable borrow ends",
            "x goes out of scope",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "match_ref_binding_last_use_arms",
        must_contain: &[
            "o's resource is immutably borrowed by s; o is read-only while the borrow is live",
            "o's resource is no longer immutably borrowed",
            "o's resource is mutably borrowed by take; o can't be used while the borrow is live",
            "s's immutable borrow ends",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "match_ref_mut_guard_last_use",
        must_contain: &[
            "o's resource is mutably borrowed by s; o can't be used while the borrow is live",
            "o's resource is no longer mutably borrowed",
            "o's resource is immutably borrowed by println!; o is read-only while the borrow is live",
            "s's mutable borrow ends",
        ],
        must_not_contain: &[],
    },
    // The same through a shared reference: `x` reborrows through `o`,
    // with no loan of its own; `o`'s borrow of `t` lasts while `x` is
    // live (#372), so it too ends after the `println!`.
    TooltipExpect {
        name: "match_shared_ref_binding_last_use",
        must_contain: &[
            "t's resource is immutably borrowed by o; t is read-only while the borrow is live",
            "t's resource is no longer immutably borrowed",
            "o's immutable borrow ends",
            "x's immutable borrow ends",
        ],
        must_not_contain: &[],
    },
    // #611: used to panic in `gather_borrow_data` (the guard's loan of
    // `s` is never live).
    TooltipExpect {
        name: "match_ref_guard_unused",
        must_contain: &[
            "o's resource is immutably borrowed by s; o is read-only while the borrow is live",
            "o's resource is no longer immutably borrowed",
            "s's immutable borrow ends",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "deref_write_ends_borrow",
        // #587: `*x += 1;` is x's last use, so x's borrow of `v` ends
        // there, after the write (pinned in order by
        // `write_through_ref_comes_before_its_borrow_end`). The write
        // used to get a dot of its own on a blank row below the return,
        // and x's column came back as a live `&mut` from there to its
        // scope end. Same for `y`, out of `it.next()` (#575's return).
        must_contain: &[
            "the value x points to is modified through *x",
            "x's mutable borrow ends",
            "Return mutably borrowed resource from x to v",
            "the value y points to is modified through *y",
            "Return mutably borrowed resource from y to v",
            "v's resource is no longer borrowed by y, but is still mutably borrowed by it",
        ],
        // What was false on main is the order and the extra segment, not
        // a tooltip's text (bullets are matched one by one here); the
        // test named above fails on main.
        must_not_contain: &[],
    },
    // ─── Or-pattern arms that bind from different places (#533) ──────
    TooltipExpect {
        name: "or_pattern_different_places",
        // `(x, _) | (_, x)` on `t`: `x` is `t.0` or `t.1` depending on
        // which alternative matches. Each of them is maybe-moved, so
        // each is dropped at scope end only if nothing moved it. (main
        // refused the program; drawing a definite `Move from t.0 to x`
        // would say `t.1` is still dropped when `x` came from it.)
        must_contain: &[
            "x is bound from t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.0, t.0's resource is moved into x here; otherwise t.0 still owns it (Rust tracks this with a runtime drop flag).",
            "x is bound from t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.1, t.1's resource is moved into x here; otherwise t.1 still owns it (Rust tracks this with a runtime drop flag).",
            "x acquires ownership of a resource",
            "t.0 goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "t.1 goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "t goes out of scope. t.0 and t.1 were moved out in some of the branches above; each is dropped here only if no branch above moved it (Rust tracks this with runtime drop flags).",
        ],
        must_not_contain: &[
            "Move from t.0 to x",
            "Move from t.1 to x",
            "=t.0 goes out of scope. No resource is dropped.",
            "=t.0 goes out of scope. Its resource is dropped.",
            "=t.1 goes out of scope. Its resource is dropped.",
            "=t goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "or_pattern_different_values",
        // `(Some(x), _) | (_, Some(x))` on `(a, b)`. Building the tuple
        // moves a and b into it whatever arm runs (#327), so neither
        // owns anything afterwards: x is bound from the tuple's copy of
        // a's or b's value, and what x didn't take is dropped with the
        // tuple after the arm (edition 2024). #566 first said a and b
        // still owned it and were dropped at their scope end only if
        // nothing moved them (a drop flag), which is false here.
        must_contain: &[
            "a's resource is moved into the temporary tuple (a, b)",
            "x is bound from a or b, whichever alternative of the pattern matches. If it's the alternative that binds x from a, a's resource is moved out of the temporary tuple (a, b) into x here; otherwise it stays in the tuple",
            "x is bound from a or b, whichever alternative of the pattern matches. If it's the alternative that binds x from b, b's resource is moved out of the temporary tuple (a, b) into x here; otherwise it stays in the tuple",
            "The temporary tuple (a, b) is dropped after this arm, when the match ends, and with it whatever of a's and b's resources x didn't take",
            // The `_` arm takes nothing out of the tuple.
            "The temporary tuple (a, b) is dropped after this arm, when the match ends, and with it a's and b's resources",
            "a was moved or dropped in every branch above",
            "a goes out of scope. No resource is dropped.",
            "b goes out of scope. No resource is dropped.",
            "x goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "Move from a to x",
            "Move from b to x",
            "otherwise a still owns it",
            "otherwise b still owns it",
            "drop flag",
            "a was moved in at least one branch above",
            "b was moved in at least one branch above",
            "a goes out of scope. Its resource is dropped here only if no branch above moved it",
            "b goes out of scope. Its resource is dropped here only if no branch above moved it",
            "=a goes out of scope. Its resource is dropped.",
            "=b goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "or_pattern_different_places_copy",
        // A copied `x` moves nothing: `t` and its parts stay whole.
        must_contain: &[
            "x is bound to a copyable value",
            "t.0 goes out of scope",
        ],
        must_not_contain: &["moved into x", "drop flag", "Move from t.0 to x"],
    },
    TooltipExpect {
        name: "or_pattern_different_fields",
        // `P { a: x, .. } | P { b: x, .. }`: `p.a` or `p.b`.
        must_contain: &[
            "x is bound from p.a or p.b, whichever alternative of the pattern matches. If it's the alternative that binds x from p.a, p.a's resource is moved into x here; otherwise p.a still owns it (Rust tracks this with a runtime drop flag).",
            "x is bound from p.a or p.b, whichever alternative of the pattern matches. If it's the alternative that binds x from p.b, p.b's resource is moved into x here; otherwise p.b still owns it (Rust tracks this with a runtime drop flag).",
            "p.a goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "p.b goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "p goes out of scope. p.a and p.b were moved out in some of the branches above; each is dropped here only if no branch above moved it (Rust tracks this with runtime drop flags).",
        ],
        must_not_contain: &[
            "Move from p.a to x",
            "=p.a goes out of scope. No resource is dropped.",
            "=p.b goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "or_pattern_three_places",
        // Three alternatives: all three places are listed.
        must_contain: &[
            "x is bound from t.0, t.1 or t.2, whichever alternative of the pattern matches. If it's the alternative that binds x from t.0, t.0's resource is moved into x here; otherwise t.0 still owns it (Rust tracks this with a runtime drop flag).",
            "x is bound from t.0, t.1 or t.2, whichever alternative of the pattern matches. If it's the alternative that binds x from t.2, t.2's resource is moved into x here; otherwise t.2 still owns it (Rust tracks this with a runtime drop flag).",
            "t.2 goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &["Move from t.0 to x", "=t.2 goes out of scope. Its resource is dropped."],
    },
    TooltipExpect {
        name: "or_pattern_tuple_scrutinee",
        // `match (a, b)` / `match (a, b, c)` with an or-pattern arm that
        // binds x from a or b: a, b and c are moved into the temporary
        // tuple, x takes a's or b's value out of it, and the tuple's
        // drop takes the rest (when the match ends: a one-arm match is
        // drawn inline, so "here, at the end of the match"). Nothing is
        // dropped at a's, b's or c's scope end, and no drop flag is
        // involved (#566 first said there was).
        must_contain: &[
            "x is bound from a or b, whichever alternative of the pattern matches. If it's the alternative that binds x from a, a's resource is moved out of the temporary tuple (a, b) into x here; otherwise it stays in the tuple",
            "The temporary tuple (a, b) is dropped here, at the end of the match, and with it whatever of a's and b's resources x didn't take",
            "x is bound from a or b, whichever alternative of the pattern matches. If it's the alternative that binds x from b, b's resource is moved out of the temporary tuple (a, b, c) into x here; otherwise it stays in the tuple",
            // c isn't bound by the arm: all of it goes with the tuple.
            "The temporary tuple (a, b, c) is dropped after this arm, when the match ends, and with it c's resource and whatever of a's and b's resources x didn't take",
            "The temporary tuple (a, b, c) is dropped after this arm, when the match ends, and with it a's, b's and c's resources",
            "a goes out of scope. No resource is dropped.",
            "b goes out of scope. No resource is dropped.",
            "c goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "still owns it",
            "drop flag",
            "only if no branch above moved it",
            "was moved in at least one branch above",
            "=a goes out of scope. Its resource is dropped.",
            "=b goes out of scope. Its resource is dropped.",
            "=c goes out of scope. Its resource is dropped.",
            // c's whole value is in the tuple whichever alternative
            // matched.
            "whatever of a's, b's and c's resources",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_pop_and_copy",
        // #583: the copy out of `y` leaves `y` as it was, so `y` has no
        // arms and no join.
        must_contain: &[
            "Copy from y to u",
            "u is initialized with a copy of y",
            "y's value is copied",
            "Copy from pop to t",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "y is live in a conditional expression",
            "y isn't moved in any of the branches above",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_copy_first",
        must_contain: &[
            "Copy from y to b",
            "b is initialized with a copy of y",
            "Partial immutable borrow from x to a",
            "x goes out of scope. Its resource is dropped (including the String it holds, if any).",
        ],
        must_not_contain: &["y is live in a conditional expression"],
    },
    TooltipExpect {
        name: "edition2024/while_let_chain_two_bindings",
        must_contain: &[
            "Copy from y to u",
            "u is initialized with a copy of y",
            "Copy from pop to t",
            "t goes out of scope",
            "u goes out of scope",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "edition2024/while_let_chain_two_sources",
        must_contain: &[
            "Copy from y to b",
            "b is initialized with a copy of y",
            "Copy from z to c",
            "c is initialized with a copy of z",
        ],
        must_not_contain: &[],
    },
    // #602: a field moved out of a Box (`take(b.name)`) is drawn like a field
    // moved out of a plain struct, and the Box is partially moved: its
    // remaining fields and its heap allocation are still freed at scope end.
    TooltipExpect {
        name: "box_field_move",
        must_contain: &[
            "b.name's resource is moved",
            "Move from b.name to take",
            "b goes out of scope. b.name was moved out earlier, so only b.tag is dropped. The Box's heap allocation is freed.",
        ],
        must_not_contain: &[
            "b goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "box_field_move_all",
        must_contain: &[
            "b.name's resource is moved",
            "b goes out of scope. b.name was moved out earlier, so no field is dropped. The Box's heap allocation is freed.",
        ],
        must_not_contain: &[
            "b goes out of scope. Its resource is dropped.",
            // The box allocation is freed, so "no resource" would be false.
            "so no resource is dropped",
        ],
    },
    TooltipExpect {
        name: "box_field_move_branch",
        must_contain: &[
            "b.name's resource is moved",
            "b goes out of scope. b.name was moved out in at least one branch above, so it's dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag). b.tag is dropped. The Box's heap allocation is freed.",
        ],
        must_not_contain: &[
            "b goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "box_field_move_param",
        must_contain: &[
            "b.name's resource is moved",
            "b goes out of scope. b.name was moved out earlier, so no field is dropped. The Box's heap allocation is freed.",
        ],
        must_not_contain: &[
            "b goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "borrow_struct_overwrite_call",
        // `hold = make(&t);` and `hold = other;` drop hold's old value,
        // so s's / a's borrow ends at the write, as for a struct literal
        // (#538); rustc accepts the `&mut s` / `&mut a` right after. The
        // field then borrows what the new value borrows. Main drew
        // `make`'s (and `M::new`'s) result borrowing the old lender
        // again, and never returned t, d or v. `o.inner = H { r: &f };`
        // overwrites a field of `o`, which stays live, so e stays
        // borrowed until o's last use (#531); #568 returned e's borrow
        // at the write. See
        // `struct_overwrite_by_call_or_variable_ends_old_borrow_at_the_write`.
        must_contain: &[
            "Return immutably borrowed resource from hold.r to s",
            "Immutable borrow from t to hold.r: make's signature gives the struct it returns the same lifetime as its argument",
            "Return immutably borrowed resource from hold.r to t",
            "Return immutably borrowed resource from hold.r to a",
            "Immutable borrow from d to hold.r: make's signature gives the struct it returns the same lifetime as its argument",
            "Return immutably borrowed resource from hold.r to d",
            "Return mutably borrowed resource from h.r to u",
            "Mutable borrow from v to h.r: M::new's signature gives the struct it returns the same lifetime as its argument",
            "Return mutably borrowed resource from h.r to v",
            "e's resource is no longer immutably borrowed",
        ],
        must_not_contain: &[
            // The new value's borrow drawn from the old lender.
            "Immutable borrow from s to hold.r: make's signature",
            "Immutable borrow from c to hold.r: make's signature",
            "Mutable borrow from u to h.r: M::new's signature",
            // A field of a struct handed its borrow back at the write.
            "Return immutably borrowed resource from o.inner.r to e",
        ],
    },
    // #609: the whole contents of a Box moved out (`take(*b)`): the contents'
    // drop isn't b's, but the Box's heap allocation is still freed at b's
    // scope end, so "No resource is dropped" would be false.
    TooltipExpect {
        name: "box_contents_move",
        must_contain: &[
            "b goes out of scope. Its contents were moved out earlier, so only the Box's heap allocation is freed.",
        ],
        must_not_contain: &[
            "=b goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "box_contents_move_string",
        must_contain: &[
            "b goes out of scope. Its contents were moved out earlier, so only the Box's heap allocation is freed.",
        ],
        must_not_contain: &[
            "=b goes out of scope. No resource is dropped.",
        ],
    },
    // A Box bound by a pattern.
    TooltipExpect {
        name: "box_contents_move_pattern",
        must_contain: &[
            "b goes out of scope. Its contents were moved out earlier, so only the Box's heap allocation is freed.",
        ],
        must_not_contain: &[
            "=b goes out of scope. No resource is dropped.",
        ],
    },
    // The Box itself moved (not its contents): b's scope end frees nothing.
    TooltipExpect {
        name: "box_whole_move",
        must_contain: &[
            "b goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "heap allocation is freed",
        ],
    },
    // ─── Or-pattern arms that bind a reference, tuple or struct from
    // different places (#565) ─────────────────────────────────────────
    TooltipExpect {
        name: "or_pattern_ref_different_places",
        // `(ref x, _) | (_, ref x)`: x borrows t.0 or t.1, whichever
        // alternative matches. The borrow checker can't tell which, so
        // both are borrowed until x's borrow ends. (#566 refused the
        // program; a plain borrow arrow from t.0 would say x always
        // borrows t.0 and that t.1 is never borrowed.)
        must_contain: &[
            "x immutably borrows t.0 or t.1, whichever alternative of the pattern matches",
            "x borrows t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.0, t.0's resource is immutably borrowed by x here. Which alternative matches is decided at run time, so the borrow checker treats t.0 as borrowed either way: t.0 is read-only while x's borrow is live.",
            "x borrows t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.1, t.1's resource is immutably borrowed by x here. Which alternative matches is decided at run time, so the borrow checker treats t.1 as borrowed either way: t.1 is read-only while x's borrow is live.",
            "x's borrow ends here, so t.0's resource is no longer immutably borrowed (the borrow checker counted it as borrowed whichever alternative of the pattern matched)",
            "x's borrow ends here, so t.1's resource is no longer immutably borrowed (the borrow checker counted it as borrowed whichever alternative of the pattern matched)",
            "x's immutable borrow ends",
            "t.0 goes out of scope. Its resource is dropped.",
            "t.1 goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "Partial immutable borrow from t.0 to x",
            "Partial immutable borrow from t.1 to x",
            "Return immutably borrowed resource from x to t.0",
            "=t.0's resource is immutably borrowed by x; t.0 is read-only while the borrow is live",
            "=x immutably borrows a resource",
            "x is bound to an immutable reference",
            "drop flag",
        ],
    },
    TooltipExpect {
        name: "or_pattern_ref_last_use",
        // x's last use is the `println!`: the borrow checker ends both
        // possible borrows there, so `t.1.push_str` on the next line
        // compiles. They end on that line, not at the end of the arm.
        must_contain: &[
            "x's borrow ends here, so t.1's resource is no longer immutably borrowed (the borrow checker counted it as borrowed whichever alternative of the pattern matched)",
            "t.1's resource is immutably borrowed by x if the alternative that binds x from t.1 matched; either way, t.1 is read-only until x's borrow ends",
            "t.1's resource is mutably borrowed by push_str; t.1 can't be used while the borrow is live",
            "x's immutable borrow ends",
        ],
        must_not_contain: &[
            "Partial immutable borrow from t.0 to x",
            "=t.0's resource is being shared by one or more variables",
            "=t.1's resource is being shared by one or more variables",
        ],
    },
    TooltipExpect {
        name: "or_pattern_ref_mut_different_places",
        // `ref mut`: neither place can be used while x's borrow is
        // live; `println!("{}", t.0)` comes after x's last use.
        must_contain: &[
            "x mutably borrows t.0 or t.1, whichever alternative of the pattern matches",
            "x borrows t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.0, t.0's resource is mutably borrowed by x here. Which alternative matches is decided at run time, so the borrow checker treats t.0 as borrowed either way: t.0 can't be used while x's borrow is live.",
            "x borrows t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.1, t.1's resource is mutably borrowed by x here. Which alternative matches is decided at run time, so the borrow checker treats t.1 as borrowed either way: t.1 can't be used while x's borrow is live.",
            "x's borrow ends here, so t.0's resource is no longer mutably borrowed (the borrow checker counted it as borrowed whichever alternative of the pattern matched)",
            "x's mutable borrow ends",
            "t.0's resource is immutably borrowed by println!; t.0 is read-only while the borrow is live",
        ],
        must_not_contain: &[
            "Partial Mutable borrow from t.0 to x",
            "Return mutably borrowed resource from x to t.0",
            "=t.0's resource is mutably borrowed by x; t.0 can't be used while the borrow is live",
        ],
    },
    TooltipExpect {
        name: "or_pattern_ref_through_refs",
        // `match (&a, &b) { (Some(x), _) | (_, Some(x)) }`: x borrows a's
        // or b's String through the reference being matched. A Copy
        // place's reference borrows a value.
        must_contain: &[
            "x immutably borrows a or b, whichever alternative of the pattern matches",
            "x borrows a or b, whichever alternative of the pattern matches. If it's the alternative that binds x from b, b's resource is immutably borrowed by x here. Which alternative matches is decided at run time, so the borrow checker treats b as borrowed either way: b is read-only while x's borrow is live.",
            "k borrows n.0 or n.1, whichever alternative of the pattern matches. If it's the alternative that binds k from n.0, n.0's value is immutably borrowed by k here. Which alternative matches is decided at run time, so the borrow checker treats n.0 as borrowed either way: n.0 is read-only while k's borrow is live.",
            "k's borrow ends here, so n.1's value is no longer immutably borrowed (the borrow checker counted it as borrowed whichever alternative of the pattern matched)",
            "a goes out of scope. Its resource is dropped (including the String it holds, if any).",
        ],
        must_not_contain: &[
            "Immutable borrow from a to x",
            "n.0's resource",
            "moved into",
        ],
    },
    TooltipExpect {
        name: "or_pattern_struct_different_places",
        // A struct `x` bound from t.0 or t.1 moves its fields with it:
        // each field of each place is maybe-moved too. A plain (no
        // drop) struct moves a value, with no drop flag; a Copy tuple
        // is just copied.
        must_contain: &[
            "x is bound from t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.0, t.0's resource is moved into x here; otherwise t.0 still owns it (Rust tracks this with a runtime drop flag).",
            "x is bound from t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.1, t.1.n's resource is moved into x.n here; otherwise t.1.n still owns it (Rust tracks this with a runtime drop flag).",
            "x is bound from t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.0, t.0.k's value is moved into x.k here; otherwise t.0.k still holds it.",
            "t.0.n goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "x.n acquires ownership of a resource",
            "x.k is bound to a copyable value",
            "y is bound from u.0 or u.1, whichever alternative of the pattern matches. If it's the alternative that binds y from u.0, u.0's value is moved into y here; otherwise u.0 still holds it.",
            "z is bound to a copyable value",
            "z.0 is bound to a copyable value",
        ],
        must_not_contain: &[
            "Move from t.0 to x",
            "Move from t.0.n to x.n",
            "=t.0.n goes out of scope. Its resource is dropped.",
            "=t.1.n goes out of scope. Its resource is dropped.",
            "u.0's resource",
            "moved into z",
        ],
    },
    TooltipExpect {
        name: "or_pattern_tuple_different_places",
        // A tuple `x` bound from t.0 or t.1: each element moves with it.
        must_contain: &[
            "x is bound from t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.1, t.1.0's resource is moved into x.0 here; otherwise t.1.0 still owns it (Rust tracks this with a runtime drop flag).",
            "t.1.1 goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "x.0 acquires ownership of a resource",
            "x.1 acquires ownership of a resource",
        ],
        must_not_contain: &[
            "Move from t.0 to x",
            "Move from t.0.0 to x.0",
            "=t.1.0 goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "or_pattern_shadowed_name",
        // The three `x`s are in separate match arms: none shadows
        // another, so all are labelled plain `x` (#598). The first two
        // share a column; the `ref mut x` is a reference, so it has a
        // column of its own, still named `x`. Each is bound from a
        // different place per alternative. (#566 drew the second as a
        // definite `Move from u.0 to x₂`; #619's first rebase lost the
        // or-pattern borrow of the `ref mut x`, drawing it as a definite
        // borrow of w.0.)
        must_contain: &[
            "x is bound from u.0 or u.1, whichever alternative of the pattern matches. If it's the alternative that binds x from u.1, u.1's resource is moved into x here; otherwise u.1 still owns it (Rust tracks this with a runtime drop flag).",
            "x mutably borrows w.0 or w.1, whichever alternative of the pattern matches",
            "u.1 goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "Move from u.0 to x",
            "Partial Mutable borrow from w.0 to x",
            "=w.0's resource is mutably borrowed by x; w.0 can't be used while the borrow is live",
            "=x mutably borrows a resource",
            "=u.1 goes out of scope. Its resource is dropped.",
            "x₂",
            "x₃",
        ],
    },
    TooltipExpect {
        name: "or_pattern_ref_tuple_scrutinee",
        // `match (a, b) { (Some(ref x), _) | (_, Some(ref x)) }`: a and b
        // are moved into the temporary tuple, and x borrows the tuple
        // whichever alternative matched, as without an or-pattern (#580).
        // Not a borrow of a or b, and nothing about a or b is maybe.
        must_contain: &[
            "x immutably borrows a resource",
            "The temporary tuple (a, b) is dropped after this arm, when the match ends, and with it a's and b's resources",
            "a goes out of scope. No resource is dropped.",
            "b goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "x borrows a or b",
            "a's resource is immutably borrowed by x",
            "Immutable borrow from a to x",
            "drop flag",
        ],
    },
    TooltipExpect {
        name: "borrow_struct_moved_holder",
        // `let hold = other;` moves other's reference field with it: the
        // loan stays live while hold.r holds the reference, so hold.r
        // gives the lender back at its last use. Main drew the borrow
        // returned from other.r (which no longer holds it) and hold.r's
        // borrow from something "not shown", although the lender is.
        // The field's new move dot must not say "other.r's resource is
        // moved" (the generic move text): a reference owns no resource. See
        // `moved_struct_returns_its_borrow_from_the_new_holder`.
        must_contain: &[
            "Move from other.r to hold.r",
            "other.r's immutable reference is moved (not the resource it points to)",
            "hold.r is bound to an immutable reference",
            "Return immutably borrowed resource from hold.r to t",
            "Return immutably borrowed resource from hold.r to a",
            "Return immutably borrowed resource from hold.r to b",
            // `hold = other;` reads like a plain assignment.
            "hold.r is assigned other.r's immutable reference",
            "other.r's mutable reference is moved (not the resource it points to)",
            "Return mutably borrowed resource from hold.r to u",
            "Move from o.inner.r to o2.inner.r",
            "Return immutably borrowed resource from o2.inner.r to e",
            "Return immutably borrowed resource from hold.r to f",
            "Copy from c1.r to c2.r",
            "c2.r is initialized with a copy of c1.r",
            "Return immutably borrowed resource from c2.r to g",
            "Return immutably borrowed resource from hold.r to p",
            "Return immutably borrowed resource from other.r to q",
            "Return immutably borrowed resource from hold.r to w",
        ],
        must_not_contain: &[
            // The moved-from field returning a borrow it no longer holds.
            "Return immutably borrowed resource from other.r to t",
            "Return immutably borrowed resource from other.r to b",
            "Return mutably borrowed resource from other.r to u",
            "Return immutably borrowed resource from o.inner.r to e",
            "Return immutably borrowed resource from other.r to f",
            "Return immutably borrowed resource from other.r to p",
            // The new holder's lender, which is shown.
            "isn't shown",
            // A reference doesn't own what it points to.
            "other.r's resource is moved",
            "o.inner.r's resource is moved",
            // A place assignment has no "(moved out of …)" parenthetical;
            // the arrow says where the reference came from.
            "moved out of other.r",
        ],
    },
    TooltipExpect {
        name: "borrow_struct_field_overwrite_call",
        // `o.inner = mk(&t);` overwrites a struct field of `o` with a
        // call's result: o.inner.r borrows t, as mk's signature says, and
        // gives it back at its last use. The old borrow of s lasts until
        // o's last use (a partial write; o's type has one lifetime, so a
        // `s.push` between the write and the println! is E0502), and is
        // ended on s's column, not handed back from o.inner.r, which no
        // longer points to s. #569's plugin drew no borrow of t (or v, w,
        // b) and returned o.inner.r's borrow to s (p.inner.r's to u,
        // o.inner.r's to a). See
        // `struct_field_overwrite_by_call_borrows_per_signature`.
        must_contain: &[
            "Immutable borrow from t to o.inner.r: mk's signature gives the struct it returns the same lifetime as its argument",
            "Return immutably borrowed resource from o.inner.r to t",
            "s's resource is no longer immutably borrowed",
            "Mutable borrow from v to p.inner.r: M::new's signature gives the struct it returns the same lifetime as its argument",
            "Return mutably borrowed resource from p.inner.r to v",
            "u's resource is no longer mutably borrowed",
            "Immutable borrow from w to o.inner.r: view's signature gives the struct it returns the same lifetime as the value it was called on",
            "Return immutably borrowed resource from o.inner.r to w",
            "a's resource is no longer immutably borrowed",
            "Immutable borrow from b to o.inner.r: mk's signature gives the struct it returns the same lifetime as its argument",
            "Return immutably borrowed resource from o.inner.r to b",
        ],
        must_not_contain: &[
            // The old lender handed back from a field that now points
            // to the new value's lender.
            "Return immutably borrowed resource from o.inner.r to s",
            "Return mutably borrowed resource from p.inner.r to u",
            "Return immutably borrowed resource from o.inner.r to a",
            // `O { inner: mk(&b), .. }`: b is shown.
            "o.inner.r's immutable borrow ends (what it borrows from isn't shown)",
        ],
    },
    TooltipExpect {
        name: "borrow_struct_field_overwrite_var",
        // `o.inner = other;` overwrites a struct field of `o` with
        // another variable: other.r's reference, and the loan of t it
        // holds, move to o.inner.r, which gives t back at its last use
        // (as `let hold = other;` does, #637). The old borrow of s lasts
        // until o's last use (a partial write; o's type has one
        // lifetime, so a `s.push` between the write and the println! is
        // E0502), ended on s's column, not handed back from o.inner.r,
        // which no longer points to s (as for `o.inner = mk(&t)`, #638).
        // #654 + #650's plugin drew no end of s's (u's, a's, e's)
        // borrow at all, so s stayed borrowed to the end of main, and u
        // was said to drop nothing at its scope end. See
        // `struct_field_overwrite_by_variable_moves_the_loan`.
        must_contain: &[
            "Move from other.r to o.inner.r",
            // Reads like `o.inner.r = other.r` (#650); the arrow names
            // the source.
            "o.inner.r is assigned other.r's immutable reference",
            "p.inner.r is assigned m.r's mutable reference",
            "o.inner.r is assigned o2.inner.r's immutable reference",
            "Return immutably borrowed resource from o.inner.r to t",
            "s's resource is no longer immutably borrowed",
            "Move from m.r to p.inner.r",
            "Return mutably borrowed resource from p.inner.r to v",
            "u's resource is no longer mutably borrowed",
            "u goes out of scope. Its resource is dropped.",
            "Move from o2.inner.r to o.inner.r",
            "Return immutably borrowed resource from o.inner.r to b",
            "a's resource is no longer immutably borrowed",
            "Copy from c.r to q.inner.r",
            "e's resource is no longer immutably borrowed",
        ],
        must_not_contain: &[
            // The old lender handed back from a field that now points
            // to the new value's lender.
            "Return immutably borrowed resource from o.inner.r to s",
            "Return mutably borrowed resource from p.inner.r to u",
            "Return immutably borrowed resource from o.inner.r to a",
            "Return immutably borrowed resource from q.inner.r to e",
            // The moved-from field returning a borrow it no longer holds.
            "Return immutably borrowed resource from other.r to t",
            "Return mutably borrowed resource from m.r to v",
            "Return immutably borrowed resource from o2.inner.r to b",
            // u owns its String to the end.
            "u goes out of scope. No resource is dropped.",
            "isn't shown",
            // A place assignment has no "(moved out of …)" parenthetical.
            "(moved out of",
        ],
    },
    // ─── `?` in a let-chain part / an if-else branch (#636) ─────────
    // The plugin drew none of these dots: a branch's columns dropped
    // `?` events (a let-chain's parts after the first are drawn in the
    // if branch). The bug was an omission (no false text), so the
    // must_contain entries are what fail on #581's head; the
    // must_not_contain ones pin owners that have nothing to drop.
    TooltipExpect {
        name: "edition2024/if_let_chain_else_try",
        must_contain: &[
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and w's \
             resource is dropped. If the `?` succeeds, nothing happens to w here.",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and x's \
             resource (including the String it holds, if any) is dropped. If the `?` succeeds, \
             nothing happens to x here.",
        ],
        must_not_contain: &[
            // `a` is a reference.
            "=Only if the `?` here returns early (on an `Err` or `None`): the function returns and a's \
             resource is dropped. If the `?` succeeds, nothing happens to a here.",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_else_try_moved",
        // `let Some(a) = x` moved the String into `a` before the `?`.
        must_contain: &[
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and w's \
             resource is dropped. If the `?` succeeds, nothing happens to w here.",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and a's \
             resource is dropped. If the `?` succeeds, nothing happens to a here.",
        ],
        must_not_contain: &[
            "=Only if the `?` here returns early (on an `Err` or `None`): the function returns and x's \
             resource is dropped. If the `?` succeeds, nothing happens to x here.",
        ],
    },
    TooltipExpect {
        name: "try_in_if_else",
        must_contain: &[
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and t's \
             resource is dropped. If the `?` succeeds, nothing happens to t here.",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and u's \
             resource is dropped. If the `?` succeeds, nothing happens to u here.",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and v's \
             resource (including every String it holds) is dropped. If the `?` succeeds, nothing \
             happens to v here.",
        ],
        must_not_contain: &[
            // `s` is a reference, bound after the `?`.
            "=Only if the `?` here returns early (on an `Err` or `None`): the function returns and s's \
             resource is dropped. If the `?` succeeds, nothing happens to s here.",
        ],
    },
    TooltipExpect {
        name: "try_in_match_arm",
        must_contain: &[
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and s's \
             resource is dropped. If the `?` succeeds, nothing happens to s here.",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and t's \
             resource is dropped. If the `?` succeeds, nothing happens to t here.",
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns and v's \
             resource (including every String it holds) is dropped. If the `?` succeeds, nothing \
             happens to v here.",
        ],
        must_not_contain: &[
            // The arm's pattern moved o's String into `s`.
            "=Only if the `?` here returns early (on an `Err` or `None`): the function returns and o's \
             resource is dropped. If the `?` succeeds, nothing happens to o here.",
        ],
    },
    // #670: `*r` with r: &i32 is an i32, which is Copy: the caller gets a
    // copy of the value r points to. No reference is returned, and the
    // caller gets no reference to x.
    TooltipExpect {
        name: "return_deref_copy",
        must_contain: &["a copy of the value r points to is returned to the caller"],
        must_not_contain: &["a reference from r is returned to the caller"],
    },
    TooltipExpect {
        name: "return_deref_copy_early",
        must_contain: &["a copy of the value r points to is returned to the caller"],
        must_not_contain: &["a reference from r is returned to the caller"],
    },
    // `*r` from a `&i32` parameter, and `*m` from a `&mut i32` one: what is
    // copied is the i32 m points to, not m (a reference) or m's "value".
    TooltipExpect {
        name: "return_deref_copy_param",
        must_contain: &[
            "a copy of the value r points to is returned to the caller",
            "a copy of the value m points to is returned to the caller",
        ],
        must_not_contain: &[
            "a reference from r is returned to the caller",
            "m's value is copied to the caller",
        ],
    },
    // `*r` with r: &&String copies out the `&String` r points to (a
    // reference is a Copy value too); `**y` with y: &&i32 copies the i32.
    TooltipExpect {
        name: "return_deref_copy_ref",
        must_contain: &[
            "a copy of the value r points to is returned to the caller",
            "a copy of **y, the value behind the reference y points to, is returned to the caller",
        ],
        must_not_contain: &[
            "a reference from r is returned to the caller",
            "a reference from y is returned to the caller",
        ],
    },
    // ─── Temporaries of macros, literals, if-let / for heads (#469) ──
    TooltipExpect {
        name: "edition2024/temporary_let_chain_scrutinee",
        // In a let chain (edition 2024) a `let`'s scrutinee temporary is
        // not dropped at the end of its `&&` operand: it lives until the
        // block the chain matched ends, or until the `else` runs if the
        // chain fails, and in a `while` to the end of the iteration
        // (checked with a Drop-printing program). A plain operand's
        // temporary (`label()`) is dropped at the end of that operand.
        // Once rebased onto main's let-chain drawing (#198), the later
        // parts sit in the if-branch; the temporary must still get no
        // second, function-end drop ("left early").
        must_contain: &[
            "Move from make to (temporary)",
            "Move from label to (temporary 2)",
            "Move from make to (temporary 3)",
            "Move from make to (temporary 4)",
            "This temporary Option<String> is dropped before the `else` block of this `if` runs, or when the block its let chain matched ends: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary Option<String> is dropped at the end of each iteration of this `while let` loop: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary String is dropped at the end of this `&&` / `||` operand: nothing bound it to a variable or moved it on. Its resource is dropped",
        ],
        must_not_contain: &[
            "This temporary Option<String> is dropped at the end of this `&&` / `||` operand",
            "is left early",
            "(temporary 5)",
        ],
    },
    TooltipExpect {
        name: "temporary_drop_macros_literals",
        // `vec![..];`, `let _ = P { .. };`, `P { .. }.n`, `format!(..).len()`,
        // `(.., ..).1` and `[.., ..].len()` each build a temporary that
        // Rust drops at the end of the statement (timing checked with a
        // Drop-printing program). The `make()` results moved into the
        // literals are not temporaries of their own.
        must_contain: &[
            "Move from vec! to (temporary)",
            "Move from P { .. } to (temporary 2)",
            "Move from P { .. } to (temporary 3)",
            "Move from format! to (temporary 4)",
            "Move from (..) to (temporary 5)",
            "Move from [..] to (temporary 6)",
            "A temporary holds the value that the vec! macro produces. Nothing binds it to a variable or moves it on, so Rust drops it as soon as its statement (or enclosing expression) is done",
            "A temporary holds the value that the format! macro produces. Nothing binds it to a variable or moves it on, so Rust drops it as soon as its statement (or enclosing expression) is done",
            "A temporary holds the value that the literal P { .. } builds. Nothing binds it to a variable or moves it on, so Rust drops it as soon as its statement (or enclosing expression) is done",
            "A temporary holds the value that the literal (..) builds. Nothing binds it to a variable or moves it on, so Rust drops it as soon as its statement (or enclosing expression) is done",
            "A temporary holds the value that the literal [..] builds. Nothing binds it to a variable or moves it on, so Rust drops it as soon as its statement (or enclosing expression) is done",
            "This temporary Vec<String> is dropped at the end of this statement: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary P is dropped at the end of this statement: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary String is dropped at the end of this statement: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary (String, i32) is dropped at the end of this statement: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary [String; 2] is dropped at the end of this statement: nothing bound it to a variable or moved it on. Its resource is dropped",
        ],
        must_not_contain: &[
            "the value that vec! returns",
            "the value that P { .. } returns",
            "Move from make to (temporary",
            "Move from String::new to (temporary",
            "(temporary 7)",
        ],
    },
    TooltipExpect {
        name: "temporary_drop_if_while_for",
        // The scrutinee `Some(make())` of an `if let` is dropped before
        // the `else` block runs, or when the matched block ends (edition
        // 2024, which `cargo new` gives the corpus; in 2021 it lives to
        // the end of the whole statement, after the `else` block; both
        // checked with a Drop-printing program), a `while let`'s to the end of each iteration, and the iterator a
        // `for` makes from its head (`make_vec()`, `v`) to the end of
        // the loop; `make().chars()` keeps its String that long too.
        must_contain: &[
            "Move from Some to (temporary)",
            "Move from Some to (temporary 2)",
            "Move from Some to (temporary 3)",
            "Move from into_iter to (temporary 4)",
            "Move from into_iter to (temporary 5)",
            "Move from make to (temporary 6)",
            "This temporary Option<String> is dropped before the `else` block of this `if let` runs, or when the block it matched ends: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary Option<String> is dropped at the end of each iteration of this `while let` loop: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary IntoIter<String> is dropped when the `for` loop ends: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary String is dropped when the `for` loop ends: nothing bound it to a variable or moved it on. Its resource is dropped",
            "A temporary holds the value that into_iter returns. Nothing binds it to a variable or moves it on, so Rust drops it as soon as its statement (or enclosing expression) is done",
        ],
        must_not_contain: &[
            "This temporary Option<String> is dropped at the end of the condition",
            "This temporary Option<String> is dropped at the end of this statement",
            "Move from make to (temporary 7)",
            "(temporary 7)",
        ],
    },
    TooltipExpect {
        name: "temporary_drop_early_exit",
        // A `while let` scrutinee's iterator, a `for` loop's iterator, an
        // `if let` scrutinee and `match` scrutinees (`vec![..]`, a tuple)
        // are still alive at the `return` / `break` in their branch: each
        // is dropped there on its column, stated once with the exit's
        // wording (#410), and again where it ends on the other paths.
        must_contain: &[
            "This temporary IntoIter<String> is dropped here, where `return` leaves early: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary IntoIter<String> is dropped here, where `break` leaves early: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary IntoIter<String> is dropped at the end of each iteration of this `while let` loop: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary IntoIter<String> is dropped when the `for` loop ends: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary Vec<String> is dropped here, where `return` leaves early: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary Vec<String> is dropped before the `else` block of this `if let` runs, or when the block it matched ends: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary (String, usize) is dropped here, where `return` leaves early: nothing bound it to a variable or moved it on. Its resource is dropped",
        ],
        must_not_contain: &[
            // #469's own wording for the same drop, before #410 merged.
            "is left early, while this temporary is still alive",
            "(temporary) goes out of scope at the end of the block: a let borrowed it",
            "(temporary 2) goes out of scope at the end of the block: a let borrowed it",
        ],
    },
    // ─── The same in `if let` / `while let` / `let` (#597) ───────────
    TooltipExpect {
        name: "or_pattern_if_let_different_places",
        // `if let (x, _) | (_, x) = u`: drawn like such a match arm. The
        // old plugin drew no `x` at all, and `u.0` / `u.1` as both
        // still owning and dropping their String, though one of them
        // was moved into `x`.
        must_contain: &[
            "x acquires ownership of a resource",
            "x goes out of scope. Its resource is dropped.",
            "x is bound from u.0 or u.1, whichever alternative of the pattern matches. If it's the alternative that binds x from u.0, u.0's resource is moved into x here; otherwise u.0 still owns it (Rust tracks this with a runtime drop flag).",
            "x is bound from u.0 or u.1, whichever alternative of the pattern matches. If it's the alternative that binds x from u.1, u.1's resource is moved into x here; otherwise u.1 still owns it (Rust tracks this with a runtime drop flag).",
            "u.0 goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "u.1 goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "u goes out of scope. u.0 and u.1 were moved out in some of the branches above; each is dropped here only if no branch above moved it (Rust tracks this with runtime drop flags).",
        ],
        must_not_contain: &[
            "Move from u.0 to x",
            "Move from u.1 to x",
            "Move from u to x",
            "=u.0 goes out of scope. Its resource is dropped.",
            "=u.1 goes out of scope. Its resource is dropped.",
            "=u goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "or_pattern_if_let_different_values_else",
        // `if let (Some(x), _) | (_, Some(x)) = t { … } else { … }`.
        must_contain: &[
            "x acquires ownership of a resource",
            "x is bound from t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.0, t.0's resource is moved into x here; otherwise t.0 still owns it (Rust tracks this with a runtime drop flag).",
            "x is bound from t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x from t.1, t.1's resource is moved into x here; otherwise t.1 still owns it (Rust tracks this with a runtime drop flag).",
            "t.0 goes out of scope. Its resource (including the String it holds, if any) is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
            "t.1 goes out of scope. Its resource (including the String it holds, if any) is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "Move from t.0 to x",
            "Move from t to x",
            "=t.0 goes out of scope. Its resource is dropped.",
            "=t.1 goes out of scope. Its resource is dropped.",
            "=t goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "or_pattern_while_let_different_places",
        // `while let (x, _) | (_, x) = u { …; break; }`.
        must_contain: &[
            "x acquires ownership of a resource",
            "x is bound from u.0 or u.1, whichever alternative of the pattern matches. If it's the alternative that binds x from u.0, u.0's resource is moved into x here; otherwise u.0 still owns it (Rust tracks this with a runtime drop flag).",
            "u.1 goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "Move from u.0 to x",
            "=u.0 goes out of scope. Its resource is dropped.",
            "=u.1 goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "or_pattern_let_different_places",
        // `let ((x, _) | (_, x)) = u;` is no branch: the drop-flag
        // wording says "nothing above". The old plugin drew `Move from
        // u to x` and still dropped both `u.0` and `u.1`.
        must_contain: &[
            "x acquires ownership of a resource",
            "x goes out of scope. Its resource is dropped.",
            "x is bound from u.0 or u.1, whichever alternative of the pattern matches. If it's the alternative that binds x from u.0, u.0's resource is moved into x here; otherwise u.0 still owns it (Rust tracks this with a runtime drop flag).",
            "x is bound from u.0 or u.1, whichever alternative of the pattern matches. If it's the alternative that binds x from u.1, u.1's resource is moved into x here; otherwise u.1 still owns it (Rust tracks this with a runtime drop flag).",
            "u.0 goes out of scope. Its resource is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
            "u.1 goes out of scope. Its resource is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
            "u goes out of scope. u.0 and u.1 may have been moved out above, so each is dropped here only if nothing above moved it (Rust tracks this with runtime drop flags).",
        ],
        must_not_contain: &[
            "Move from u to x",
            "Move from u.0 to x",
            "no branch above",
            "branches above",
            "=u.0 goes out of scope. Its resource is dropped.",
            "=u.1 goes out of scope. Its resource is dropped.",
            "=u goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "or_pattern_let_different_fields_reassign",
        // `let (P { a: x, .. } | P { b: x, .. }) = p;` then `p.a = …`:
        // the old `p.a`, if it's still there, is dropped at the
        // assignment; `p.b` at scope end, only if nothing moved it.
        must_contain: &[
            "x is bound from p.a or p.b, whichever alternative of the pattern matches. If it's the alternative that binds x from p.b, p.b's resource is moved into x here; otherwise p.b still owns it (Rust tracks this with a runtime drop flag).",
            "p.a acquires ownership of a new resource. Its previous resource is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
            "p.b goes out of scope. Its resource is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
            "p goes out of scope. p.b may have been moved out above, so it's dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag). p.a is dropped.",
        ],
        must_not_contain: &[
            "Move from p to x",
            "branch above",
            "p.a acquires ownership of a new resource; its previous resource is dropped",
            "=p.b goes out of scope. Its resource is dropped.",
            "=p goes out of scope. No resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "or_pattern_let_different_places_copy",
        // A copied `x` / `y` moves nothing; `x` is a copy of `t.0` or
        // `t.1`, not of the tuple `t` (the old plugin drew `Copy from t
        // to x`, and no `y`).
        must_contain: &[
            "x is bound to a copyable value",
            "y is bound to a copyable value",
            "t.0 goes out of scope",
        ],
        must_not_contain: &["Copy from t to x", "x is initialized with a copy of t", "moved into", "drop flag"],
    },
    TooltipExpect {
        name: "or_pattern_if_let_tuple_scrutinee",
        // `if let (Some(x), _) | (_, Some(x)) = (a, b)`: as in `match (a,
        // b)`, a and b are moved into the temporary tuple whether or not
        // the pattern matches, x may take a's or b's value out of it, and
        // the tuple's drop takes the rest. a and b own nothing afterwards,
        // so no drop flag on them.
        must_contain: &[
            "a's resource is moved into the temporary tuple (a, b)",
            "x is bound from a or b, whichever alternative of the pattern matches. If it's the alternative that binds x from a, a's resource is moved out of the temporary tuple (a, b) into x here; otherwise it stays in the tuple",
            "x is bound from a or b, whichever alternative of the pattern matches. If it's the alternative that binds x from b, b's resource is moved out of the temporary tuple (a, b) into x here; otherwise it stays in the tuple",
            "The temporary tuple (a, b) is dropped here, at the end of the if branch, and with it whatever of a's and b's resources x didn't take",
            "The pattern didn't match, so the temporary tuple (a, b) is dropped here, before the else branch runs, and with it a's and b's resources",
            "a goes out of scope. No resource is dropped.",
            "b goes out of scope. No resource is dropped.",
            "x acquires ownership of a resource",
        ],
        must_not_contain: &[
            "Move from a to x",
            "Move from b to x",
            "a still owns it",
            "drop flag",
            "=a goes out of scope. Its resource is dropped.",
        ],
    },
    // A shadowing name (`x₂`) bound from different places by an `if let` or
    // a `let` or-pattern: the conflict is the written name `x`, which the
    // `let` path didn't match, so it drew `Move from t.0 to x₂` and dropped
    // t.1 unconditionally.
    TooltipExpect {
        name: "or_pattern_let_shadowed_name",
        must_contain: &[
            "x₂ is bound from u.0 or u.1, whichever alternative of the pattern matches. If it's the alternative that binds x₂ from u.0, u.0's resource is moved into x₂ here; otherwise u.0 still owns it (Rust tracks this with a runtime drop flag).",
            "x₂ is bound from t.0 or t.1, whichever alternative of the pattern matches. If it's the alternative that binds x₂ from t.1, t.1's resource is moved into x₂ here; otherwise t.1 still owns it (Rust tracks this with a runtime drop flag).",
            "t.0 goes out of scope. Its resource is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
            "t.1 goes out of scope. Its resource is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
            "u.1 goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "Move from u.0 to x₂",
            "Move from t.0 to x₂",
            "=t.0 goes out of scope. No resource is dropped.",
            "=t.1 goes out of scope. Its resource is dropped.",
            "=u.1 goes out of scope. Its resource is dropped.",
        ],
    },
    // ─── Exits in an `if` without `else` (#429) ─────────────────────
    TooltipExpect {
        name: "exit_if_no_else_return",
        // The `return s;` in the else-less `if` ends its column in the
        // drop of `t`; `t` is returned on the path that goes on. (Drawn
        // before #429 too: `return s` moves `s`, #350, which makes the
        // `if` a branch, #257.)
        must_contain: &[
            "t goes out of scope. Its resource is dropped.",
            "t isn't moved in any of the branches above",
            "t's resource is moved to the caller",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "exit_if_no_else_panic",
        // The message reads `v` first (#449: the drop used to come
        // before the read and say nothing was dropped).
        must_contain: &[
            "v goes out of scope. Its resource is dropped.",
            "v isn't moved in any of the branches above",
        ],
        must_not_contain: &["v goes out of scope. No resource is dropped."],
    },
    TooltipExpect {
        name: "exit_if_no_else_break",
        must_contain: &[
            "u goes out of scope. Its resource is dropped.",
            // (The plugin emits this one with a trailing space.)
            "u is live in a conditional expression ",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "exit_if_no_else_nested",
        must_contain: &[
            "t goes out of scope. Its resource is dropped.",
            "u goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "exit_if_let_no_else_return",
        must_contain: &[
            "t goes out of scope. Its resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
            // `opt` is moved into `s` in the branch that returns (its
            // column ends at that move), so after the `if` it still
            // owns its value. (Drawn before #429 too: the pattern's
            // move makes the `if` a branch, #257.)
            "opt still owns its resource here: the branch that moved opt returned before \
             reaching this point",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "exit_if_no_else_one_line",
        // On one line too, the `if` gets its own columns (#360), and the
        // `return` ends the `if` column in `t`'s drop.
        must_contain: &[
            "t goes out of scope. Its resource is dropped.",
            "t is live in a conditional expression ",
            "t isn't moved in any of the branches above",
        ],
        must_not_contain: &[],
    },
    TooltipExpect {
        name: "exit_if_let_scrutinee_move_return",
        // `v.into_iter()` moves `v` before the branch, on both paths
        // (#466): nothing is left to drop at its scope end. The `return`
        // drops `t` (counted in `return_drops_what_the_branch_owns`).
        must_contain: &[
            "v goes out of scope. No resource is dropped.",
            "t isn't moved in any of the branches above",
        ],
        must_not_contain: &["v goes out of scope. Its resource is dropped"],
    },
    TooltipExpect {
        name: "temporary_moves_variable",
        // `vec![s];`, `let _ = P { name: t, n: 1 };`, `(u, 1).1` and
        // `[a, b].len()` move their variables into the temporary the
        // macro or literal builds (rustc: a later use is "borrow of moved
        // value"); the temporary, not the variable, drops the Strings at
        // the end of the statement. Before (#570) nothing was drawn and
        // each variable "went out of scope. Its resource is dropped."
        must_contain: &[
            "Move from s to (temporary)",
            "Move from t to (temporary 2)",
            "Move from u to (temporary 3)",
            "Move from a to (temporary 4)",
            "Move from b to (temporary 4)",
            "s's resource is moved into a temporary that is dropped at the end of this statement",
            "t's resource is moved into a temporary that is dropped at the end of this statement",
            "u's resource is moved into a temporary that is dropped at the end of this statement",
            "a's resource is moved into a temporary that is dropped at the end of this statement",
            "s goes out of scope. No resource is dropped.",
            "u goes out of scope. No resource is dropped.",
            "This temporary takes the value of s, which is moved into it. Nothing binds the temporary to a variable or moves it on, so Rust drops it, and what it holds, as soon as its statement (or enclosing expression) is done",
            "This temporary Vec<String> is dropped at the end of this statement: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary P is dropped at the end of this statement: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary (String, i32) is dropped at the end of this statement: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary [String; 2] is dropped at the end of this statement: nothing bound it to a variable or moved it on. Its resource is dropped",
        ],
        must_not_contain: &[
            "=s goes out of scope. Its resource is dropped.",
            "=t goes out of scope. Its resource is dropped.",
            "=u goes out of scope. Its resource is dropped.",
            "=a goes out of scope. Its resource is dropped.",
            "=b goes out of scope. Its resource is dropped.",
            "the value that s returns",
            // These temporaries are no `let`'s literal (#439's wording).
            "The temporary that the literal on the right of this let builds",
        ],
    },
    TooltipExpect {
        name: "for_iterator_drop",
        // #482: a `for` loop owns its iterator and drops it when the
        // loop ends (running out or a `break`), or at a `return` inside
        // it, with whatever the iterator still owns (the elements it
        // hasn't handed out, a `vec::IntoIter`'s buffer).
        // - `for s in v` (v: Vec<String>) moves v into the iterator
        //   `into_iter` builds, a `(temporary)` column that #469 drops
        //   at the loop's end.
        // - `for s in it3` (it3: vec::IntoIter<String>): `into_iter`
        //   hands it3's value back unchanged, so it3's value is the
        //   loop's iterator. No temporary: the drop is a drop dot on
        //   it3's own column at the loop's closing brace (its line is
        //   pinned by `loop_iterator_drop_dots_sit_where_the_loop_is_left`).
        // - `for word in words` with a `return` inside: dropped at the
        //   `return` too.
        // `slice::Iter` (`for s in &q`, `for s in it`) has no drop glue:
        // nothing to say, and `q` is only borrowed.
        must_contain: &[
            "v's resource is moved into the `for` loop's iterator",
            "n's resource is moved into the `for` loop's iterator",
            "This temporary IntoIter<String> is dropped when the `for` loop ends: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary IntoIter<i32> is dropped when the `for` loop ends: nothing bound it to a variable or moved it on. Its resource is dropped",
            "it3's resource is moved into the `for` loop, which uses it as its iterator",
            "it3's former resource, the `for` loop's iterator, is dropped when the `for` loop ends, along with whatever it still owns (such as elements it hasn't handed out yet)",
            "words's resource is moved into the `for` loop, which uses it as its iterator",
            "words's former resource, the `for` loop's iterator, is dropped here: the function returns from inside the loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)",
            "words's former resource, the `for` loop's iterator, is dropped when the `for` loop ends, along with whatever it still owns (such as elements it hasn't handed out yet)",
            "it3 goes out of scope. No resource is dropped.",
            "v goes out of scope. No resource is dropped.",
            // A slice iterator has nothing to drop.
            "it's value is moved",
        ],
        must_not_contain: &[
            // The move used to forecast the drop instead of drawing it.
            "The iterator is dropped when the loop ends",
            // it3 / words are the iterator: no temporary is built from
            // them (only v's and n's loops have one).
            "(temporary 3)",
            "Move from into_iter to (temporary 3)",
            "q's resource is moved into the",
            "it's resource is moved into the",
            "z's resource is moved into the",
            "w's resource is moved into the",
            // it3 / words don't take part in the `if` around the `return`.
            "words is live in a conditional expression",
            "words was moved or dropped in every branch",
        ],
    },
    // ─── A let-chain that assigns / calls / moves out of a Box, then fails (#579) ───
    // These were refused. An assignment in the condition lasts on the
    // path into the else branch: the variable holds its old value or the
    // new one there (it isn't moved). A call of an FnOnce closure and a
    // move out of `*b` are moves, drawn like #281's.
    TooltipExpect {
        name: "edition2024/if_let_chain_assign",
        must_contain: &[
            "z is assigned a copy of a",
            "z may have been assigned a new value: when the condition assigns to z and then turns out false, the else branch runs after that assignment, so here z holds either its previous value or the new one.",
        ],
        must_not_contain: &[
            // An assignment isn't a move, and z has no resource.
            "z may have been moved",
            "z was moved",
            "z holds either its previous resource",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_assign_owner",
        must_contain: &[
            "z may have been assigned a new resource: when the condition assigns to z and then turns out false, the else branch runs after that assignment, so here z holds either its previous resource or the new one. When the assignment runs, it drops z's previous resource.",
            "y may have been moved: when the condition moves y to a and then turns out false, the else branch runs after that move, so y can't be used here",
            // z holds a String on every path: no drop flag.
            "z goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "z may have been moved",
            "z goes out of scope. Its resource is dropped here only if",
            "only if no branch above moved it",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_assign_no_else",
        must_contain: &[
            "s may have been moved: when the condition moves s to z and then turns out false, the body is skipped but the move has happened, so s can't be used after the if",
            "z may have been assigned a new resource: when the condition assigns to z and then turns out false, the body is skipped but the assignment has happened, so after the if z holds either its previous resource or the new one. When the assignment runs, it drops z's previous resource.",
            "s goes out of scope. Its resource is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
            "z goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "=s goes out of scope. Its resource is dropped.",
            "z goes out of scope. Its resource is dropped here only if",
            "only if no branch above moved it",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_fnonce_call",
        must_contain: &[
            "f may have been moved: when the condition calls f (it can only be called once, so the call moves it) and then turns out false, the else branch runs after that call, so f can't be used here",
            "f goes out of scope. Its captured resources are dropped here only if nothing above moved or called it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            // The call may have dropped them already.
            "=f goes out of scope. Its 1 captured resource is dropped.",
            "only if no branch above called it",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_box_move",
        must_contain: &[
            "*b may have been moved: when the condition moves *b to take and then turns out false, the else branch runs after that move, so *b can't be used here",
            "b goes out of scope. Its resource (including the String it holds) is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            // The String in the box may have been moved to take.
            "=b goes out of scope. Its resource is dropped.",
            "=b goes out of scope. Its resource is dropped (including the String it holds).",
            "only if no branch above moved it",
        ],
    },
    TooltipExpect {
        name: "edition2024/if_let_chain_nested_effects",
        // Effects inside an `if` within a chain part last too. Before,
        // the else column showed s and z as untouched.
        must_contain: &[
            "s may have been moved: when the condition moves s to take and then turns out false, the else branch runs after that move, so s can't be used here",
            "z may have been assigned a new value: when the condition assigns to z and then turns out false, the else branch runs after that assignment, so here z holds either its previous value or the new one.",
            "s goes out of scope. Its resource is dropped here only if nothing above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &["s goes out of scope. Its resource is dropped here only if no branch above moved it"],
    },
    TooltipExpect {
        name: "for_tuple_pattern_move",
        // #527: `for (n, s) in pairs` moves `pairs` into the loop's
        // iterator just like `for s in pairs` does; the iterator is
        // #571's `(temporary)` column, dropped when the loop ends (after
        // the `break`, the elements it hasn't handed out). The
        // composite-pattern path drew no move, so `pairs` read "goes out
        // of scope. Its resource is dropped.", dropped a second time.
        // `it` is already an iterator: its drop is a drop dot on its own
        // column, as for `for s in it3` (#482).
        must_contain: &[
            "pairs's resource is moved into the `for` loop's iterator",
            "ids's resource is moved into the `for` loop's iterator",
            "it's resource is moved into the `for` loop, which uses it as its iterator",
            "it's former resource, the `for` loop's iterator, is dropped when the `for` loop ends, along with whatever it still owns (such as elements it hasn't handed out yet)",
            "This temporary IntoIter<(i32, String)> is dropped when the `for` loop ends: nothing bound it to a variable or moved it on. Its resource is dropped",
            "This temporary IntoIter<(i32, i32)> is dropped when the `for` loop ends: nothing bound it to a variable or moved it on. Its resource is dropped",
            "pairs goes out of scope. No resource is dropped.",
            "ids goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "pairs goes out of scope. Its resource is dropped.",
            "ids goes out of scope. Its resource is dropped.",
            "it goes out of scope. Its resource is dropped.",
            "The iterator is dropped when the loop ends",
        ],
    },
    TooltipExpect {
        name: "for_tuple_pattern_borrow",
        // #576: `for (n, s) in &pairs` borrows `pairs` for the loop, and
        // `&mut m` borrows `m` mutably (so `m` can't be used inside). The
        // composite-pattern path drew no borrow of the head: `n` and `s`
        // were references "bound to" nothing, `pairs` untouched.
        must_contain: &[
            "pairs's resource is immutably borrowed by n; pairs is read-only while the borrow is live",
            "pairs's resource is immutably borrowed by s; pairs is read-only while the borrow is live",
            "m's resource is mutably borrowed by n; m can't be used while the borrow is live",
            "m's resource is mutably borrowed by s; m can't be used while the borrow is live",
            "m's resource is no longer mutably borrowed",
        ],
        must_not_contain: &[
            "n is bound to an immutable reference",
            "s is bound to an immutable reference",
            "n's mutable borrow ends (what it borrows from isn't shown)",
            "s's mutable borrow ends (what it borrows from isn't shown)",
        ],
    },
    TooltipExpect {
        name: "for_iterator_break_outer",
        // #592: `break 'outer` leaves the inner `for` loop too, so its
        // iterator (what `row` still holds) is dropped at the `break`:
        // the `(temporary 2)` column of `for s in row`, and the variable
        // iterator `it` of the second function. Drawn in the `if`/`else`
        // branch that leaves (a `break` in an `if` without `else` isn't
        // drawn, like a `return`).
        must_contain: &[
            "This temporary IntoIter<String> is dropped here, where `break` leaves early: nothing bound it to a variable or moved it on. Its resource is dropped",
            "it's former resource, the `for` loop's iterator, is dropped here: this `break` leaves the `for` loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)",
            // #596: `break 'outer` leaves the target loop too, so its own
            // iterator is dropped there as well: the `(temporary)` of
            // `for row in rows` (still holding `vec!["b"]`), and the
            // variable `outer_it` of the third function.
            "This temporary IntoIter<Vec<String>> is dropped here, where `break` leaves early: nothing bound it to a variable or moved it on. Its resource is dropped",
            "outer_it's former resource, the `for` loop's iterator, is dropped here: this `break` leaves the `for` loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)",
        ],
        must_not_contain: &[
            "the function returns from inside the loop",
            "(temporary 3)",
            // Stated once, with #410's wording.
            "is left early, while this temporary is still alive",
            "(temporary 2) goes out of scope at the end of the block: a let borrowed it",
        ],
    },
    // ─── `let r = loop { … break k; }` (#431) ───────────────────────
    TooltipExpect {
        name: "loop_break_value_moved",
        must_contain: &[
            "Move from k to r",
            "r acquires ownership of a resource",
            "r goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["=r goes out of scope. No resource is dropped."],
    },
    TooltipExpect {
        name: "loop_break_value_outer_var",
        // `break s` (in an `if`) moves `s` out of the loop: `r` has it
        // where the loop ends (#350's approach), so `s` has nothing to
        // drop at the end of `main` and `r` does. `break n * 2` copies.
        must_contain: &[
            "Move from s to r",
            "r goes out of scope. Its resource is dropped.",
            "s goes out of scope. No resource is dropped.",
            "doubled is bound to a copyable value",
        ],
        must_not_contain: &[
            "=r goes out of scope. No resource is dropped.",
            "=s goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "loop_break_value_in_branch",
        // `break a` in a match arm, `break b` in an `if` / `else`: the
        // values reach `r` and `q` at the end of their loops.
        must_contain: &[
            "Move from a to r",
            "Move from b to q",
            "r goes out of scope. Its resource is dropped.",
            "q goes out of scope. Its resource is dropped.",
            "a goes out of scope. No resource is dropped.",
            "b goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &[
            "=r goes out of scope. No resource is dropped.",
            "=q goes out of scope. No resource is dropped.",
            "=a goes out of scope. Its resource is dropped.",
            "=b goes out of scope. Its resource is dropped.",
        ],
    },
    TooltipExpect {
        name: "loop_break_value_labelled",
        must_contain: &[
            "Move from String::from to w",
            "w goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["=w goes out of scope. No resource is dropped."],
    },
    TooltipExpect {
        name: "loop_break_value_receivers",
        must_contain: &[
            "Move from k to r",
            "Move from x to a",
            "Move from y to b",
            "Move from z to take",
            "a goes out of scope. Its resource is dropped.",
            "b goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "=a goes out of scope. No resource is dropped.",
            "=b goes out of scope. No resource is dropped.",
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
    // "Anonymous resource" is the plugin's internal name for a value
    // with no place of its own; no tooltip may show it to a student
    // (#547).
    if let Some(t) = timeline_tooltips(&timeline).iter().find(|t| t.contains("Anonymous")) {
        panic!("{}: tooltip leaks the internal placeholder name: {:?}", name, t);
    }
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
    let stacked = stacked_segments(&timeline);
    assert!(
        stacked.is_empty(),
        "{}: column segments with different tooltips drawn over the same rows: {:?}",
        name,
        stacked
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
    let stray = arm_underline_off_its_lines(&code, &timeline);
    assert!(
        stray.is_empty(),
        "{}: {} branch column element(s) would underline another arm's lines: {:?}",
        name,
        stray.len(),
        &stray[..stray.len().min(5)]
    );
    let mislabelled = hover_polygons_off_their_segment(&timeline);
    assert!(
        mislabelled.is_empty(),
        "{}: {} branch column hover polygon(s) show another segment's tooltip: {:?}",
        name,
        mislabelled.len(),
        &mislabelled[..mislabelled.len().min(5)]
    );
}

/// #388: hovering a branch column shows the tooltip of the segment
/// under the mouse. The transparent hover `<polygon>`s sit on top of
/// the column's `<line>`s, so they're what the mouse hits; each must
/// cover lines (one segment's: one centreline, or a hollow strip's two
/// sides) whose tooltip is its own. One polygon used to span a whole
/// run of segments and show the first one's text over all of them.
/// Returns each polygon that breaks that. Bevel lines (the short
/// connectors at a hollow strip's corners) are ignored.
fn hover_polygons_off_their_segment(timeline_svg: &str) -> Vec<String> {
    // Is `p` inside the polygon `poly` or within 2px of its edge? A
    // hollow strip's two sides lie on its hover polygon's edges.
    fn on(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
        let mut inside = false;
        let mut near = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let ((xi, yi), (xj, yj)) = (poly[i], poly[j]);
            if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            // Distance from `p` to the edge (i, j).
            let (dx, dy) = (xj - xi, yj - yi);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 { (((p.0 - xi) * dx + (p.1 - yi) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            if (p.0 - (xi + t * dx)).hypot(p.1 - (yi + t * dy)) <= 2.0 {
                near = true;
            }
            j = i;
        }
        inside || near
    }
    // A tooltip without its markup (`&lt;span …&gt;s&lt;/span&gt; is …`).
    let short = |s: &str| {
        let mut text = String::new();
        let mut rest = s;
        while let Some(i) = rest.find("&lt;") {
            text.push_str(&rest[..i]);
            rest = rest[i..].find("&gt;").map_or("", |j| &rest[i + j + 4..]);
        }
        text.push_str(rest);
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let mut out = Vec::new();
    for group in timeline_svg.split("<g class=\"branch-group\"").skip(1) {
        let group = &group[..group.find("</g>").unwrap_or(group.len())];
        let tags = |kind: &str| -> Vec<&str> {
            group.split(kind).skip(1).map(|t| &t[..t.find('>').unwrap_or(t.len())]).collect()
        };
        // (midpoint, tooltip, endpoints) of each line longer than a bevel.
        let all_lines: Vec<((f64, f64), &str, [i64; 4])> = tags("<line ").into_iter().filter_map(|tag| {
            let n = |a: &str| tag_attr(tag, a).and_then(|v| v.parse::<f64>().ok());
            let (x1, y1, x2, y2) = (n("x1")?, n("y1")?, n("x2")?, n("y2")?);
            if (x2 - x1).hypot(y2 - y1) < 4.0 { return None; }
            // Endpoints in a direction-free order, to 0.1px.
            let q = |v: f64| (v * 10.0).round() as i64;
            let (p, r) = ((q(x1), q(y1)), (q(x2), q(y2)));
            let (p, r) = if p <= r { (p, r) } else { (r, p) };
            Some((((x1 + x2) / 2.0, (y1 + y2) / 2.0), tag_attr(tag, "data-tooltip-text")?, [p.0, p.1, r.0, r.1]))
        }).collect();
        // Two segments with different tooltips drawn on exactly the same
        // spot can't be told apart by position (the old padding of an
        // `if let` arm's struct-field columns, `if_let_struct_fields_unused`,
        // draws "has no value yet" and "holds a copyable value" over the
        // same rows). Leave them out: no polygon can be right about them.
        let stacked = |i: usize| all_lines.iter().enumerate()
            .any(|(j, l)| j != i && l.2 == all_lines[i].2 && l.1 != all_lines[i].1);
        let lines: Vec<((f64, f64), &str)> = all_lines.iter().enumerate()
            .filter(|(i, _)| !stacked(*i))
            .map(|(_, l)| (l.0, l.1))
            .collect();
        let stacked_mids: Vec<(f64, f64)> = all_lines.iter().enumerate()
            .filter(|(i, _)| stacked(*i))
            .map(|(_, l)| l.0)
            .collect();
        for tag in tags("<polygon ") {
            let Some(title) = tag_attr(tag, "data-tooltip-text") else { continue };
            let poly: Vec<(f64, f64)> = tag_attr(tag, "points").unwrap_or("")
                .split_whitespace()
                .filter_map(|p| {
                    let (x, y) = p.split_once(',')?;
                    Some((x.parse().ok()?, y.parse().ok()?))
                })
                .collect();
            if poly.len() < 3 { continue; }
            let covered: Vec<&str> = lines.iter()
                .filter(|(m, _)| on(*m, &poly))
                .map(|(_, t)| *t)
                .collect();
            if covered.is_empty() && stacked_mids.iter().any(|m| on(*m, &poly)) {
                continue;
            }
            if covered.is_empty() || covered.iter().any(|t| *t != title) {
                out.push(format!(
                    "polygon {:?} over lines {:?}",
                    short(title),
                    covered.iter().map(|t| short(t)).collect::<Vec<_>>()
                ));
            }
        }
    }
    out
}

/// The value of attribute `name` in the tag `tag` (`<line ... y1="..." ...>`).
fn tag_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {}=\"", name);
    let start = tag.find(&key)? + key.len();
    Some(&tag[start..start + tag[start..].find('"')?])
}

/// #369: hovering an arm of an `if`/`else` or `match` underlines only
/// that arm's own lines. Its column is also drawn beside the other
/// arms' code (dashed), and the diagonals from the split and to the
/// join pass those lines; none of that may underline them. Each arm's
/// `<g class="branch-group">` names the code panel rows of its first and
/// last line in `data-arm-rows`. This returns every `<line>` and hover
/// `<polygon>` in one that has no `data-underline-y` inside those rows
/// and doesn't opt out of underlining (`data-underline="none"`), and
/// every diagonal that doesn't opt out.
fn arm_underline_off_its_lines(code_svg: &str, timeline_svg: &str) -> Vec<String> {
    // Row number -> y the timeline draws it at: the k-th
    // `<text class="code" .. y="Y">` of the code panel, drawn at Y - 5.
    let row_y: Vec<f64> = code_svg
        .split("<text class=\"code\"")
        .skip(1)
        .filter_map(|chunk| tag_attr(&chunk[..chunk.find('>').unwrap_or(chunk.len())], "y"))
        .filter_map(|y| y.parse::<f64>().ok())
        .map(|y| y - 5.0)
        .collect();
    let y_of = |row: &usize| row.checked_sub(1).and_then(|i| row_y.get(i));
    const TOL: f64 = 2.0;
    let mut out = Vec::new();
    for group in timeline_svg.split("<g class=\"branch-group\"").skip(1) {
        let group = &group[..group.find("</g>").unwrap_or(group.len())];
        let head = &group[..group.find('>').unwrap_or(group.len())];
        let rows: Vec<usize> = tag_attr(head, "data-arm-rows")
            .map(|r| r.split(' ').filter_map(|n| n.parse().ok()).collect())
            .unwrap_or_default();
        let (Some(first), Some(last)) = (rows.first().and_then(y_of), rows.get(1).and_then(y_of)) else {
            // An arm with no lines of its own (an implicit `else`):
            // everything in it must opt out.
            for tag in group.split(|c| c == '<').skip(1) {
                let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
                if (tag.starts_with("line ") || tag.starts_with("polygon "))
                    && tag_attr(tag, "data-underline") != Some("none")
                {
                    out.push(format!("arm without own lines: <{}", tag));
                }
            }
            continue;
        };
        for el in ["line ", "polygon "] {
            for seg in group.split(&format!("<{}", el)).skip(1) {
                let tag = &seg[..seg.find('>').unwrap_or(seg.len())];
                let range: Vec<f64> = tag_attr(tag, "data-underline-y")
                    .map(|r| r.split(' ').filter_map(|v| v.parse().ok()).collect())
                    .unwrap_or_default();
                let n = |a: &str| tag_attr(tag, a).and_then(|v| v.parse::<f64>().ok());
                let diagonal = el == "line "
                    && matches!((n("x1"), n("x2")), (Some(x1), Some(x2)) if (x1 - x2).abs() > 0.01
                        && (n("y1") != n("y2")));
                let ok = match range[..] {
                    [a, b] => !diagonal && *first - TOL <= a.min(b) && a.max(b) <= *last + 5.0 + TOL,
                    _ => tag_attr(tag, "data-underline") == Some("none"),
                };
                if !ok {
                    out.push(format!(
                        "arm on rows {}-{} (y {}..{}): <{}{}",
                        rows[0], rows[1], first, last, el, tag
                    ));
                }
            }
        }
    }
    out
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

/// Vertical state-line pieces of a timeline panel that lie over the same
/// rows of the same column but carry different tooltips, as
/// `(column hash, x, from y, to y, tooltip A, tooltip B)`. Only the top
/// one can be hovered, so a column drawn like that says two things about
/// one row (#486: a pattern-bound field's column drew "has no value yet"
/// and "holds a copyable value" over the same rows).
fn stacked_segments(svg: &str) -> Vec<String> {
    let mut by_column: BTreeMap<(String, String), Vec<(f64, f64, String)>> = BTreeMap::new();
    for (i, _) in svg.match_indices(" data-tooltip-text=\"") {
        let start = match svg[..i].rfind('<') {
            Some(s) => s,
            None => continue,
        };
        let tag_end = start + svg[start..].find('>').unwrap_or(svg.len() - start);
        let tag = &svg[start..tag_end];
        let tip = match attr(tag, "data-tooltip-text") {
            Some(t) => t.to_owned(),
            None => continue,
        };
        let lines: Vec<&str> = if tag.starts_with("<line ") {
            vec![tag]
        } else if tag.starts_with("<g ") {
            let body = &svg[start..start + svg[start..].find("</g>").unwrap_or(svg.len() - start)];
            start_tags(body, "line")
        } else {
            continue;
        };
        for l in lines {
            let (x1, x2) = (attr(l, "x1"), attr(l, "x2"));
            if x1.is_none() || x1 != x2 {
                continue;
            }
            let ys: Option<(f64, f64)> = attr(l, "y1").zip(attr(l, "y2"))
                .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)));
            if let (Some((a, b)), Some(h)) = (ys, attr(l, "data-hash")) {
                by_column.entry((h.to_owned(), x1.unwrap().to_owned()))
                    .or_default()
                    .push((a.min(b), a.max(b), tip.clone()));
            }
        }
    }
    let mut out = Vec::new();
    for ((hash, x), segs) in by_column {
        for (i, a) in segs.iter().enumerate() {
            for b in &segs[i + 1..] {
                if a.2 != b.2 && a.1.min(b.1) - a.0.max(b.0) > 0.5 {
                    out.push(format!("hash {} x={}: y {}..{} {:?} / y {}..{} {:?}", hash, x, a.0, a.1, a.2, b.0, b.1, b.2));
                }
            }
        }
    }
    out
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
/// `(x1, y1, x2, y2)` of every `<line>` in a timeline panel.
fn svg_lines(svg: &str) -> Vec<(f64, f64, f64, f64)> {
    let num = |tag: &str, name: &str| -> Option<f64> {
        let key = format!(" {}=\"", name);
        let start = tag.find(&key)? + key.len();
        tag[start..start + tag[start..].find('"')?].parse().ok()
    };
    svg.match_indices("<line ")
        .filter_map(|(i, _)| {
            let tag = &svg[i..i + svg[i..].find('>')?];
            Some((num(tag, "x1")?, num(tag, "y1")?, num(tag, "x2")?, num(tag, "y2")?))
        })
        .collect()
}

/// #317: a branch of type `!` (`return`, `panic!()`, …) used to be
/// drawn converging into the merge dot like the others. It now ends in
/// a short bar, and nothing leaves its column below that bar. The bar's
/// message doesn't name the column it sits on, so this test checks the
/// columns: each bar's `data-hash` is its column's.
#[test]
fn branches_that_diverge_stop_short() {
    const BAR: &str = "This branch stops here: it returns, panics, breaks or continues, so it \
                       never reaches the end of the conditional";
    // A column that ends in a drop at the exit (#363, #410) gets no bar:
    // the drop dot ends it.
    for (name, columns) in [
        // The returning branch's `s` and `t` are dropped at the `return`.
        ("diverging_branch_stops", &[][..]),
        // `unreachable!()` on `s` (no value yet) and `panic!()` on `m`;
        // the `panic!` arm drops `s` and `t`.
        ("match_diverging_arm", &["s", "m"][..]),
        // `process::exit` runs no destructors: its arm keeps the bars.
        ("exit_panic_drops", &["s", "t"][..]),
        ("exit_unreachable_else", &[][..]),
        // `b` lives on past the inner `continue`; `c` is dropped there.
        ("exit_break_continue_drops", &["b"][..]),
        ("exit_continue_in_match", &["n"][..]),
        // `s` and `u` are dropped at their `return`s.
        ("exit_return_borrowed_value", &["v"][..]),
        ("exit_return_after_nested_if", &[][..]),
        // `pick`'s `s` has no value on the `_ => return 0` path: a bar,
        // not a drop. `f`'s `t` is dropped at its `return`; `g`'s `m`
        // is path-dependent and draws neither.
        ("exit_path_dependent_drops", &["s"][..]),
        ("exit_return_in_let_if", &[][..]),
        // The temporary Result is dropped at the `return`.
        ("exit_temporary_drops", &["n"][..]),
        // early_return: r is dropped at each `return`; r₂ moves out.
        ("shadow_matrix_scopes", &["r₂"][..]),
        // #429: an `if` without `else` whose body exits ends its column
        // in the drops there, not in a bar.
        ("exit_if_no_else_return", &[][..]),
        ("exit_if_no_else_panic", &[][..]),
        ("exit_if_no_else_break", &[][..]),
        ("exit_if_no_else_nested", &[][..]),
        ("exit_if_let_no_else_return", &[][..]),
        ("exit_if_no_else_one_line", &[][..]),
        ("exit_if_let_scrutinee_move_return", &[][..]),
    ] {
        let rv = run_with_local_backend(name);
        let timeline = rv.timeline_panel_string();
        let bar_tags: Vec<&str> = timeline
            .match_indices("<line ")
            .map(|(i, _)| &timeline[i..=i + timeline[i..].find('>').unwrap()])
            .filter(|t| t.contains("This branch stops here"))
            .collect();
        let label_of = |hash: &str| -> String {
            let key = format!("data-hash=\"{}\"", hash);
            let t = timeline
                .split("<text ")
                .skip(1)
                .find(|t| t.split_once('>').unwrap().0.contains("class=\"label") && t.split_once('>').unwrap().0.contains(&key))
                .unwrap_or_else(|| panic!("{}: no header label for hash {}", name, hash));
            let body = &t[t.find('>').unwrap() + 1..t.find("</text>").unwrap()];
            let mut text = String::new();
            let mut in_tag = false;
            for c in body.chars() {
                match c {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => text.push(c),
                    _ => {}
                }
            }
            text.split('|').next().unwrap().to_string()
        };
        let mut got: Vec<String> = bar_tags
            .iter()
            .map(|tag| {
                assert!(
                    tag.contains(&format!("data-tooltip-text=\"{}\"", BAR)),
                    "{}: stop bar message: {}",
                    name, tag
                );
                let h = &tag[tag.find("data-hash=\"").unwrap() + 11..];
                label_of(&h[..h.find('"').unwrap()])
            })
            .collect();
        got.sort();
        let mut want: Vec<String> = columns.iter().map(|c| c.to_string()).collect();
        want.sort();
        assert_eq!(got, want, "{}: columns that end in a stop bar", name);
        // The geometry check only fits conditionals outside loops: in a
        // loop the column legitimately continues below the bar once the
        // loop is over (`b` in `exit_break_continue_drops`).
        if !["diverging_branch_stops", "match_diverging_arm", "exit_panic_drops"].contains(&name) {
            continue;
        }
        for tag in bar_tags {
            let (x1, y, x2, _) = svg_lines(tag)[0];
            let x = (x1 + x2) / 2.0;
            for (ax, ay, bx, by) in svg_lines(&timeline) {
                for ((nx, _), (fx, fy)) in [((ax, ay), (bx, by)), ((bx, by), (ax, ay))] {
                    assert!(
                        !((nx - x).abs() <= 2.0 && (fx - x).abs() > 5.0 && fy > y),
                        "{}: a line leaves the diverging branch at x={} below its end y={}: ({}, {}) -> ({}, {})",
                        name, x, y, ax, ay, bx, by
                    );
                }
            }
        }
    }
}

/// `(cx, cy)` of every dot in a timeline panel whose tooltip is
/// exactly `text` (plain circles, and drop dots: a tooltip `<g>`
/// around a circle).
fn dots_with_tooltip(svg: &str, text: &str) -> Vec<(f64, f64)> {
    dots_where(svg, |lines| lines == [text])
}

/// Like `dots_with_tooltip`, for a dot that may share its event with
/// others (`• event<br>• event`): any of its lines is `text`.
fn dots_listing_tooltip(svg: &str, text: &str) -> Vec<(f64, f64)> {
    dots_where(svg, |lines| lines.iter().any(|l| l == text))
}

fn dots_where(svg: &str, want: impl Fn(&[String]) -> bool) -> Vec<(f64, f64)> {
    let num = |tag: &str, name: &str| -> Option<f64> {
        let key = format!(" {}=\"", name);
        let start = tag.find(&key)? + key.len();
        tag[start..start + tag[start..].find('"')?].parse().ok()
    };
    let mut out = Vec::new();
    for (i, _) in svg.match_indices(" data-tooltip-text=\"") {
        let Some(start) = svg[..i].rfind('<') else { continue };
        let rest = &svg[start..];
        let circle = if rest.starts_with("<circle ") {
            rest
        } else if rest.starts_with("<g ") {
            match rest.find("<circle ") {
                Some(c) if c < rest.find("</g>").unwrap_or(usize::MAX) => &rest[c..],
                _ => continue,
            }
        } else {
            continue;
        };
        let tag = &circle[..circle.find('>').unwrap_or(circle.len())];
        if !want(&timeline_tooltips(&svg[start..start + svg[start..].find('>').unwrap_or(0) + 1])) {
            continue;
        }
        if let (Some(x), Some(y)) = (num(tag, "cx"), num(tag, "cy")) {
            out.push((x, y));
        }
    }
    out
}

/// #117: a conditional whose branches share lines used to draw its
/// arms crossing into an X, with the split, the arms and the join all
/// on one row. Now the split is above every arm's dot and the join
/// below, like a conditional spread over several lines.
#[test]
fn branches_on_shared_lines_open_and_close_around_their_dots() {
    for (name, arm_text, arms) in [
        ("match_one_line", "s acquires ownership of a resource", 3),
        ("if_else_next_line", "s acquires ownership of a resource", 2),
        ("if_as_let_rhs", "s acquires ownership of a resource", 2),
    ] {
        let rv = run_with_local_backend(name);
        let timeline = rv.timeline_panel_string();
        let split = dots_with_tooltip(&timeline, "s is live in a conditional expression ");
        let join = dots_with_tooltip(&timeline, "s acquired ownership of a resource (in all branches above)");
        let arm_dots = dots_with_tooltip(&timeline, arm_text);
        assert_eq!((split.len(), join.len(), arm_dots.len()), (1, 1, arms), "{}: dots", name);
        for (x, y) in &arm_dots {
            assert!(split[0].1 < *y && *y < join[0].1,
                "{}: arm dot ({}, {}) not between split {:?} and join {:?}", name, x, y, split[0], join[0]);
        }
        let mut xs: Vec<f64> = arm_dots.iter().map(|d| d.0).collect();
        xs.dedup();
        assert_eq!(xs.len(), arms, "{}: arms share a column: {:?}", name, arm_dots);
    }
}

/// #389: a reference declared inside an `if` / `match` arm stops
/// borrowing inside that arm, so the lender gets its value back on the
/// arm's column, where the borrow started. That dot used to be drawn
/// on the lender's parent column, which then ran beside the arm's code
/// from there to the join, as if the conditional weren't there.
#[test]
fn borrow_end_in_arm_is_drawn_in_the_arm() {
    for (name, var, kind) in [
        ("borrow_end_in_if_arm", "s", "mutably"),
        ("shared_borrow_end_in_if_arm", "s", "immutably"),
        ("borrow_end_in_match_arm", "v", "mutably"),
        ("borrow_end_in_nested_if", "s", "mutably"),
    ] {
        let rv = run_with_local_backend(name);
        let timeline = rv.timeline_panel_string();
        let restriction = if kind == "mutably" {
            format!("{var} can't be used while the borrow is live")
        } else {
            format!("{var} is read-only while the borrow is live")
        };
        let lent = dots_with_tooltip(&timeline, &format!("{var}'s resource is {kind} borrowed by r; {restriction}"));
        let back = dots_with_tooltip(&timeline, &format!("{var}'s resource is no longer {kind} borrowed"));
        let mut splits = dots_with_tooltip(&timeline, &format!("{var} is live in a conditional expression "));
        splits.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        assert_eq!((lent.len(), back.len()), (1, 1), "{}: dots {:?} {:?}", name, lent, back);
        assert!(!splits.is_empty(), "{}: no split dot", name);
        assert_eq!(
            back[0].0, lent[0].0,
            "{}: {var} gets its value back at x={}, not on the arm's column (x={}) where it lent it",
            name, back[0].0, lent[0].0,
        );
        assert!(
            back[0].1 > lent[0].1,
            "{}: the borrow ends at y={}, above where it starts (y={})", name, back[0].1, lent[0].1,
        );
        assert_ne!(back[0].0, splits[0].0, "{}: {var} gets its value back on its parent column", name);
    }
}

/// #575: `n` in `while let Some(n) = it.next()` is a reference copied out
/// of what `next` returned, so it borrows `v`; that borrow ends at n's
/// last use, `println!`, while `it` goes on borrowing `v` until the loop
/// ends. n's return arrow is on the `println!` row, `v` says there that
/// `it` still borrows it, and `v` is back to unborrowed only once, below.
#[test]
fn reference_copied_out_of_a_holder_returns_at_its_last_use() {
    let rv = run_with_local_backend("holder_iflet_call");
    let (timeline, code) = (rv.timeline_panel_string(), rv.code_panel_string());
    let row = code_row_y(&code, "println!(\"{}\", n);");
    let arrows = arrow_points(&timeline, "Return immutably borrowed resource from n to v");
    assert_eq!(arrows.len(), 1, "n's return arrows: {:?}", arrows);
    assert!(
        arrows[0].iter().all(|p| (p.1 - row).abs() < 1.0),
        "n's return arrow {:?} isn't on the println! row (y={})", arrows[0], row,
    );
    let still = dots_with_tooltip(&timeline, "v's resource is no longer borrowed by n, but is still immutably borrowed by it");
    assert_eq!(still.len(), 1, "v's dots where n's borrow ends: {:?}", still);
    assert!((still[0].1 - row).abs() < 1.0, "v's dot {:?} isn't on the println! row (y={})", still[0], row);
    // Below the `while let` row (y's borrow, above it, ends at line 4).
    let loop_row = code_row_y(&code, "while let Some(n) = it.next() {");
    let back: Vec<(f64, f64)> = dots_with_tooltip(&timeline, "v's resource is no longer immutably borrowed")
        .into_iter()
        .filter(|d| d.1 > loop_row)
        .collect();
    assert_eq!(back.len(), 1, "v is unborrowed at {:?} after the loop starts", back);
    assert!(back[0].1 > row, "v is unborrowed at {:?}, not below n's last use (y={}); it still borrows v", back[0], row);
}

/// #587: `*x += 1;` as x's last use reads `*x`, writes through it, and
/// then x's borrow ends. All three are on x's dot on that line, in that
/// order, and x's column isn't drawn again below it: `x` holds a live
/// `&mut` only from its `let` to that line (one segment).
#[test]
fn write_through_ref_comes_before_its_borrow_end() {
    let rv = run_with_local_backend("deref_write_ends_borrow");
    let timeline = rv.timeline_panel_string();
    let tips = tooltip_lines(&timeline);
    for r in ["x", "y"] {
        let end = format!("{r}'s mutable borrow ends");
        let dots: Vec<&Vec<String>> = tips.iter().filter(|t| t.contains(&end)).collect();
        assert_eq!(dots.len(), 1, "{r}: dots ending its borrow: {:?}", dots);
        let want = [
            format!("*{r}'s value is copied"),
            format!("the value {r} points to is modified through *{r}"),
            end.clone(),
        ];
        assert!(dots[0].ends_with(&want), "{r}: the dot ending its borrow reads {:?}, not ending with {:?}", dots[0], want);
        let live = format!("{r} holds a mutable reference; it can read and write the borrowed value");
        let segments = tips.iter().filter(|t| **t == [live.clone()]).count();
        assert_eq!(segments, 1, "{r}: {segments} segments read {live:?}; its column comes back after its borrow ends");
    }
}

/// The timeline y of the code-panel row showing source line `src`
/// (compared with whitespace collapsed). A line's text sits 5px below its
/// timeline row.
fn code_row_y(code_svg: &str, src: &str) -> f64 {
    let rows = code_rows_y(code_svg, src);
    assert_eq!(rows.len(), 1, "code rows showing {:?}: {:?}", src, rows);
    rows[0]
}

/// `code_row_y` for a line that appears more than once, in file order.
fn code_rows_y(code_svg: &str, src: &str) -> Vec<f64> {
    let want = src.split_whitespace().collect::<Vec<_>>().join(" ");
    let rows: Vec<f64> = start_tags(code_svg, "text")
        .into_iter()
        .filter_map(|tag| {
            let at = code_svg.find(tag)?;
            let body = &code_svg[at + tag.len()..];
            let body = &body[..body.find("</text>")?];
            let mut text = String::new();
            let mut in_tag = false;
            for ch in body.chars() {
                match ch {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => text.push(ch),
                    _ => {}
                }
            }
            let text = text.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
            let words: Vec<&str> = text.split_whitespace().collect();
            // The first word is the line number.
            (words.len() > 1 && words[1..].join(" ") == want).then(|| attr(tag, "y")?.parse::<f64>().ok())?
        })
        .collect();
    rows.into_iter().map(|y| y - 5.0).collect()
}

/// The points of the gray arrow shaft whose tooltip is exactly `text`.
fn arrow_points(svg: &str, text: &str) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    for (i, _) in svg.match_indices("<g class=\"tooltip-trigger\" data-tooltip-text=\"") {
        let group = &svg[i..i + svg[i..].find("</g>").unwrap_or(svg.len() - i)];
        let open = &group[..group.find('>').unwrap_or(group.len()) + 1];
        if timeline_tooltips(open) != [text] {
            continue;
        }
        let shaft = start_tags(group, "polyline")
            .into_iter()
            .filter_map(|t| attr(t, "points"))
            .chain(start_tags(group, "path").into_iter().filter_map(|t| attr(t, "d")))
            .next();
        let Some(shaft) = shaft else { continue };
        let nums: Vec<f64> = shaft
            .split(|c: char| c == ' ' || c == ',')
            .filter_map(|t| t.parse().ok())
            .collect();
        out.push(nums.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect());
    }
    out
}

/// An arm whose first event is on the split line (a `Move`, `Copy` or
/// borrow dot of `var`, with the arrow `arrow_text` leaving along that
/// row) is drawn starting there: the split dot is within the half row above
/// the row of `line`, the arm's diagonal ends at the event's dot, and the
/// arm's column doesn't run above it (no Z), and no arrow hops over a dot.
fn assert_arm_starts_on_split_line(name: &str, var: &str, line: &str, bind_text: &str, arrow_text: &str) {
    let rv = run_with_local_backend(name);
    let (timeline, code) = (rv.timeline_panel_string(), rv.code_panel_string());
    let row = code_row_y(&code, line);
    let bind = dots_with_tooltip(&timeline, bind_text);
    let arrow = arrow_points(&timeline, arrow_text);
    // (`dots_listing`: a condition's event on the line shares the split dot, #509.)
    let split = dots_listing(&timeline, &format!("{} is live in a conditional expression ", var));
    assert_eq!((bind.len(), arrow.len(), split.len()), (1, 1, 1), "{} {}: dots / arrow", name, var);
    let (bind, split) = (bind[0], split[0]);
    // The binding is on its own source row, and its arrow runs level
    // along it from the event dot.
    assert_eq!(bind.1, row, "{} {}: event dot not on the split row", name, var);
    assert!(arrow[0].len() == 2 && arrow[0].iter().all(|p| p.1 == row),
        "{} {}: {:?} isn't a straight arrow along the split row y={}: {:?}", name, var, arrow_text, row, arrow[0]);
    // The split dot is above that row, within the half row above it.
    assert!(row - 30.0 < split.1 && split.1 < row,
        "{} {}: split dot at y={} is not between the row above and the split row y={}", name, var, split.1, row);
    // The arm's diagonal ends at the event dot, and the arm's
    // column doesn't run above it.
    let lines = svg_lines(&timeline);
    let near = |a: f64, b: f64| (a - b).abs() <= 3.0;
    let diagonals: Vec<_> = lines.iter()
        .filter(|(x1, y1, x2, y2)| {
            (near(*x1, split.0) && near(*y1, split.1) && near(*x2, bind.0))
                || (near(*x2, split.0) && near(*y2, split.1) && near(*x1, bind.0))
        })
        .collect();
    assert!(!diagonals.is_empty(), "{} {}: no diagonal from the split dot to the arm", name, var);
    for &&(x1, y1, x2, y2) in &diagonals {
        let end_y = if near(x1, bind.0) { y1 } else { y2 };
        assert!(near(end_y, bind.1),
            "{} {}: the arm's diagonal ends at y={}, not at the event dot y={}", name, var, end_y, bind.1);
    }
    for &(x1, y1, x2, y2) in &lines {
        // (Vertical lines only: a double line's bevel joins the
        // diagonal's offset ends to the column's.)
        if near(x1, bind.0) && x1 == x2 {
            assert!(y1.min(y2) >= bind.1 - 1e-6,
                "{} {}: the arm's column runs above its event dot: ({}, {}) -> ({}, {})", name, var, x1, y1, x2, y2);
        }
    }
    // Nothing to hop over: the event's arrow is straight.
    assert_eq!(arrows_crossing_dots(&timeline), Vec::<String>::new(), "{}", name);
    let hops: Vec<&str> = start_tags(&timeline, "path")
        .into_iter()
        .filter(|t| attr(t, "stroke") == Some("gray") && attr(t, "d").is_some_and(|d| d.contains(" A ")))
        .collect();
    assert_eq!(hops, Vec::<&str>::new(), "{} {}: an arrow hops over a dot", name, var);
}

/// #496: `if let Some(b) = y { take(b); } else { take(0); }` copies the
/// `i32` out of `y`. That doesn't change `y`, so its column used to split
/// into an `if` arm and an `else` arm that both read "y holds a copyable
/// value" down to the join, for nothing. `y` is now one column with the
/// copy on it.
#[test]
fn copying_out_of_an_if_let_scrutinee_doesnt_branch_it() {
    let rv = run_with_local_backend("if_let_binding_copy");
    let timeline = rv.timeline_panel_string();
    for text in [
        "y is live in a conditional expression",
        "y isn't moved in any of the branches above",
    ] {
        assert!(!timeline.contains(text), "y has a branch: {:?}", text);
    }
    // The copy is still drawn on y, and b still gets the value.
    assert_eq!(dots_with_tooltip(&timeline, "y's value is copied").len(), 1);
    assert_eq!(arrow_points(&timeline, "Copy from y to b").len(), 1);
    assert_eq!(dots_with_tooltip(&timeline, "b is initialized with a copy of y").len(), 1);
}

/// #382: an `if let` whose pattern borrows, copies or moves out of a
/// variable used to draw that binding on the split row, beside the split
/// dot: the `if` arm's diagonal reached its column a row lower, so the
/// column ran back up to the dot (a Z), and the binding's arrow hopped
/// over the split dot. The binding now sits on the `if let` row where the
/// arm's column starts, and the split dot is half a row above it (no row
/// is added). A `while let` body is drawn inline, with the binding on the
/// `while let` row; that part is a regression guard (it passed before).
#[test]
fn if_let_binding_is_drawn_at_the_start_of_its_arm() {
    // (fixture, variable, `if let` line, binding dot on the variable's
    // `if` arm, the binding's arrow)
    let if_lets = [
        ("if_let_binding_borrow", "x", "if let Some(a) = &x {",
         "x's resource is immutably borrowed by a; x is read-only while the borrow is live",
         "Partial immutable borrow from x to a"),
        // (`if_let_binding_copy` isn't here: a copy changes nothing about
        // `y`, so `y` has no arms; see
        // `copying_out_of_an_if_let_scrutinee_doesnt_branch_it`.)
        ("if_let_binding_move", "name", "if let Some(n) = name {", "name's resource is moved", "Move from name to n"),
        ("if_let_binding_move", "opt", "if let Some(s) = opt {", "opt's resource is moved", "Move from opt to s"),
    ];
    for (name, var, line, bind_text, arrow_text) in if_lets {
        assert_arm_starts_on_split_line(name, var, line, bind_text, arrow_text);
    }

    let rv = run_with_local_backend("while_let_binding_kinds");
    let (timeline, code) = (rv.timeline_panel_string(), rv.code_panel_string());
    assert!(!timeline.contains("is live in a conditional expression"), "a `while let` body is drawn inline");
    for (line, bind_text, arrow_text) in [
        ("while let Some(a) = &x {",
         "x's resource is immutably borrowed by a; x is read-only while the borrow is live",
         "Partial immutable borrow from x to a"),
        ("while let Some(b) = y {", "y's value is copied", "Copy from y to b"),
        ("while let Some(s) = o {", "o's resource is moved", "Move from o to s"),
    ] {
        let row = code_row_y(&code, line);
        let bind = dots_with_tooltip(&timeline, bind_text);
        let arrow = arrow_points(&timeline, arrow_text);
        assert_eq!((bind.len(), arrow.len()), (1, 1), "{}: dot / arrow", line);
        assert_eq!(bind[0].1, row, "{}: binding not on its row", line);
        assert!(arrow[0].iter().all(|p| p.1 == row), "{}: arrow not along its row: {:?}", line, arrow[0]);
    }
}

/// #495: a reference declared before an `if` / `match` whose last use
/// is in one arm stops borrowing in that arm, and on the paths through
/// the other arms it already has. The lender gets its value back once
/// per path, on an arm's column between the split and the join. It used
/// to get it back once, on its parent column in the middle of the
/// diamond (or, when the lender wasn't used in the conditional, beside
/// it), and its arms stayed "shared" to the join, so `s.push('y')` in
/// the other arm was drawn mutating a shared `s`.
#[test]
fn outer_borrow_end_is_drawn_in_each_arm() {
    for (name, kind, paths) in [
        ("outer_ref_end_in_match_arm", "immutably", 2),
        ("outer_ref_end_in_if_arm", "immutably", 2),
        ("outer_mut_ref_end_in_if_arm", "mutably", 2),
        ("outer_ref_end_in_nested_if", "immutably", 3),
    ] {
        let timeline = run_with_local_backend(name).timeline_panel_string();
        let mut splits = dots_with_tooltip(&timeline, "s is live in a conditional expression ");
        splits.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        let mut joins = dots_with_tooltip(&timeline, "s isn't moved in any of the branches above");
        joins.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        assert!(!splits.is_empty() && !joins.is_empty(),
            "{}: s has no column in the conditional (splits {:?}, joins {:?})", name, splits, joins);
        let (split, join) = (splits[0], joins[joins.len() - 1]);
        let back = dots_with_tooltip(&timeline, &format!("s's resource is no longer {kind} borrowed"));
        assert_eq!(back.len(), paths, "{}: s gets its value back {:?}, not once per path", name, back);
        for (x, y) in &back {
            assert!(*x != split.0, "{}: s gets its value back on its parent column at ({}, {})", name, x, y);
            assert!(split.1 < *y && *y < join.1,
                "{}: s gets its value back at ({}, {}), not between split {:?} and join {:?}", name, x, y, split, join);
        }
    }
}

/// #508: the same for a plain `if` or `match` whose first arm is written
/// on the split line (`if c { take(s); } else {`): the arm's column used to
/// start at the split row while the diagonal reached it a row lower, so it
/// ran back up to the move's dot.
#[test]
fn an_arm_whose_first_event_is_on_the_split_line_starts_there() {
    assert_arm_starts_on_split_line(
        "if_arm_on_split_line", "s", "if c { take(s); } else {",
        "s's resource is moved", "Move from s to take");
    assert_arm_starts_on_split_line(
        "match_arm_on_split_line", "s", "match n { 0 => take(s),",
        "s's resource is moved", "Move from s to take");
}

/// #509: when the condition also has an event of the column's own on the
/// split line (`if s.len() > 1 { take(s); } else {`, or a let-chain whose
/// first part reads the variable the later `let` binds from: `if
/// x.is_some() && let Some(a) = &x {`), that event runs before either
/// arm. It's drawn on the split dot, raised half a row above the `if`
/// row, and the `if` arm starts on the row as without it. These columns
/// used to keep the old drawing: the arm ran back up to its first event's
/// dot (a Z), and that event's arrow hopped over the split dot.
#[test]
fn a_condition_event_on_the_split_line_is_on_the_raised_split_dot() {
    for (name, var, line, bind_text, arrow_text, cond) in [
        ("if_arm_on_split_line_cond_event", "s", "if s.len() > 1 { take(s); } else {",
         "s's resource is moved", "Move from s to take", "len reads from s (implicitly borrowed)"),
        ("edition2024/if_let_chain_cond_reads_x", "x", "if x.is_some() && let Some(a) = &x {",
         "x's resource is immutably borrowed by a; x is read-only while the borrow is live",
         "Partial immutable borrow from x to a", "is_some reads from x (implicitly borrowed)"),
    ] {
        assert_arm_starts_on_split_line(name, var, line, bind_text, arrow_text);
        let timeline = run_with_local_backend(name).timeline_panel_string();
        let split = dots_listing(&timeline, &format!("{} is live in a conditional expression ", var));
        assert_eq!(dots_listing(&timeline, cond), split, "{}: {:?} is not on the split dot", name, cond);
    }
}

/// #557: `if let Msg::Pair(n, s) = msg` copies `n` out of `msg` before the
/// split and moves `s` out of it as the `if` arm's first event, both on
/// the `if let` row. `msg`'s `if` arm used to start at the split dot on
/// that row, run its diagonal to the arm's column a row lower, and come
/// back up to the move's dot: a zero-height, backwards stub (a V) at the
/// split. The arm starts on the `if let` row with the move's dot, and the
/// split dot (which also carries the copy, recorded before the split) is
/// half a row above it. (The copy's arrow to `n` runs down from that dot
/// to `n`'s, so it isn't level.)
///
/// This test pins the arm's geometry; it fails on the plugin before the fix.
#[test]
fn an_if_let_arm_that_moves_out_on_the_split_row_starts_there() {
    let rv = run_with_local_backend("if_let_copy_and_move_out_of_scrutinee");
    let (timeline, code) = (rv.timeline_panel_string(), rv.code_panel_string());
    let row = code_row_y(&code, "if let Msg::Pair(n, s) = msg {");
    let moved = dots_with_tooltip(&timeline, "msg's resource is moved");
    let split = dots_listing_tooltip(&timeline, "msg is live in a conditional expression ");
    let copied = dots_listing_tooltip(&timeline, "msg's value is copied");
    assert_eq!((moved.len(), split.len(), copied.len()), (1, 1, 1), "dots: {:?} {:?} {:?}", moved, split, copied);
    let (moved, split) = (moved[0], split[0]);
    // The move is the arm's first event, on the `if let` row; the split
    // dot is within the half row above it and holds the copy as well.
    assert_eq!(moved.1, row, "the move's dot is not on the `if let` row");
    assert!(row - 30.0 < split.1 && split.1 < row, "split dot at y={} is not just above the row y={}", split.1, row);
    assert_eq!(copied[0], split, "the copy isn't on the split dot");
    assert!(moved.0 != split.0, "the arm's dot is on the parent column");

    let near = |a: f64, b: f64| (a - b).abs() <= 3.0;
    let lines = svg_lines(&timeline);
    // The arm's diagonal runs from the split dot to the move's dot.
    assert!(lines.iter().any(|&(x1, y1, x2, y2)| near(x1, split.0) && near(y1, split.1) && near(x2, moved.0) && near(y2, moved.1)
        || near(x2, split.0) && near(y2, split.1) && near(x1, moved.0) && near(y1, moved.1)),
        "no diagonal from the split dot {:?} to the arm's dot {:?}", split, moved);
    // Nothing else is drawn on the arm's column from the split: the arm
    // has no event after the move, so its column has no vertical segment
    // at all, in particular none of zero height or running back up.
    let stubs: Vec<_> = lines.iter()
        .filter(|&&(x1, y1, x2, _)| near(x1, moved.0) && near(x2, moved.0) && y1 >= row - 15.0 && y1 <= row + 30.0)
        .collect();
    assert!(stubs.is_empty(), "a column segment is drawn from the split on msg's `if` arm: {:?}", stubs);
    // And nothing runs backwards or has no height on the whole column.
    for &(x1, y1, x2, y2) in &lines {
        if near(x1, moved.0) && near(x2, moved.0) && (x1 - x2).abs() < 1.0 {
            assert!(y2 > y1, "a zero-height or backwards segment on the arm's column: ({}, {}) -> ({}, {})", x1, y1, x2, y2);
        }
    }
}

/// #510: two pattern bindings on one if-let-chain line, `if let Some(a) =
/// &x && let Some(b) = y {`. Their columns came after both sources (x, y,
/// a, b), so the arrows overlapped: `x -> a` ran along the row and hopped
/// over y's binding dot, and `y -> b` was a trapezoid through a blank row
/// added below the line, which pushed the body a row away from its code.
/// Each binding's column now comes right after its source's (x, a, y, b),
/// and both arrows run straight along the `if let` row.
#[test]
fn two_bindings_on_one_let_chain_line_are_drawn_side_by_side() {
    // (fixture, the line, its arrows, the next line, the bindings' dots
    // on the sources' arms)
    for (name, line, arrows, next, binds) in [
        ("edition2024/if_let_chain", "if let Some(a) = &x && let Some(b) = y {",
         ["Partial immutable borrow from x to a", "Copy from y to b"], "show(a);",
         ["x's resource is immutably borrowed by a; x is read-only while the borrow is live", "y's value is copied"]),
        ("edition2024/else_if_let_chain_two_bindings", "} else if let Some(a) = &x && let Some(b) = &y {",
         ["Partial immutable borrow from x to a", "Partial immutable borrow from y to b"], "show(a);",
         ["x's resource is immutably borrowed by a; x is read-only while the borrow is live",
          "y's resource is immutably borrowed by b; y is read-only while the borrow is live"]),
    ] {
        let rv = run_with_local_backend(name);
        let (timeline, code) = (rv.timeline_panel_string(), rv.code_panel_string());
        let xs: Vec<f64> = ["x", "a", "y", "b"].iter().map(|l| label_x(&timeline, l)).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{}: columns x, a, y, b at {:?}", name, xs);
        let row = code_row_y(&code, line);
        for text in arrows {
            let arrow = arrow_points(&timeline, text);
            assert_eq!(arrow.len(), 1, "{}: {:?}", name, text);
            assert!(arrow[0].len() == 2 && arrow[0].iter().all(|p| p.1 == row),
                "{}: {:?} isn't a straight arrow along the `if let` row y={}: {:?}", name, text, row, arrow[0]);
        }
        assert_eq!(arrows_crossing_dots(&timeline), Vec::<String>::new(), "{}", name);
        let hops: Vec<&str> = start_tags(&timeline, "path")
            .into_iter()
            .filter(|t| attr(t, "stroke") == Some("gray") && attr(t, "d").is_some_and(|d| d.contains(" A ")))
            .collect();
        assert_eq!(hops, Vec::<&str>::new(), "{}: an arrow hops over a dot", name);
        // No row added below the line: the body stays on the rows of its code.
        assert_eq!(code_row_y(&code, next), row + 30.0, "{}: a row is added below the `if let` line", name);
        for text in binds {
            let bind = dots_with_tooltip(&timeline, text);
            assert_eq!(bind.len(), 1, "{}: {}", name, text);
            assert_eq!(bind[0].1, row, "{}: {:?} isn't on the `if let` row", name, text);
        }
    }
}

/// #583: `if let Some(a) = &x && let Some(b) = y {` copies the `i32`
/// out of `y`. That doesn't change `y`, but `y`'s column used to split
/// into an `if` arm and an `else` arm that both read "y holds a copyable
/// value" down to a join ("y isn't moved in any of the branches
/// above"), for nothing. As for a lone `if let` (#496), the copy is now
/// drawn on `y`'s one column, and the binding beside its source still
/// gets a straight arrow.
#[test]
fn copying_out_of_a_let_chain_scrutinee_doesnt_branch_it() {
    // (fixture, the `if let` line, the copy, columns left to right)
    for (name, line, copy, cols) in [
        ("edition2024/if_let_chain", "if let Some(a) = &x && let Some(b) = y {",
         "Copy from y to b", ["x", "a", "y", "b"]),
        ("edition2024/if_let_chain_pop_and_copy", "if let Some(t) = v.pop() && let Some(u) = y {",
         "Copy from y to u", ["v", "y", "u", "t"]),
    ] {
        let rv = run_with_local_backend(name);
        let (timeline, code) = (rv.timeline_panel_string(), rv.code_panel_string());
        for text in ["y is live in a conditional expression", "y isn't moved in any of the branches above"] {
            assert!(!timeline.contains(text), "{}: y has a branch: {:?}", name, text);
        }
        assert_eq!(dots_with_tooltip(&timeline, "y's value is copied").len(), 1, "{}", name);
        let xs: Vec<f64> = cols.iter().map(|l| label_x(&timeline, l)).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{}: columns {:?} at {:?}", name, cols, xs);
        let row = code_row_y(&code, line);
        let arrow = arrow_points(&timeline, copy);
        assert_eq!(arrow.len(), 1, "{}: {:?}", name, copy);
        assert!(arrow[0].len() == 2 && arrow[0].iter().all(|p| p.1 == row),
            "{}: {:?} isn't a straight arrow along the `if let` row y={}: {:?}", name, copy, row, arrow[0]);
        assert_eq!(arrows_crossing_dots(&timeline), Vec::<String>::new(), "{}", name);
        let hops: Vec<&str> = start_tags(&timeline, "path")
            .into_iter()
            .filter(|t| attr(t, "stroke") == Some("gray") && attr(t, "d").is_some_and(|d| d.contains(" A ")))
            .collect();
        assert_eq!(hops, Vec::<&str>::new(), "{}: an arrow hops over a dot", name);
    }
}

/// #584: a let-chain whose loop body or `if` body is drawn inline (no
/// arms), so #510's side-by-side layout for an arm's pattern line didn't
/// apply. In `while let Some(t) = v.pop() && let Some(u) = y {` the
/// columns were v, y, t, u and `Copy from y to u` hopped over `t`'s dot;
/// with two bindings from variables (`while let Some(b) = y && let
/// Some(c) = z {`, or an `if let` chain without an `else` whose body
/// only reads) the second arrow was a trapezoid through a blank row
/// added below the line. Each binding made from a variable now gets its
/// column right after that variable's, and the arrows run straight along
/// the line's own row.
#[test]
fn a_let_chains_bindings_are_drawn_beside_their_sources_inline() {
    // (fixture, the line, its arrows between columns, columns left to
    // right, the next line)
    for (name, line, arrows, cols, next) in [
        ("edition2024/while_let_chain_two_bindings", "while let Some(t) = v.pop() && let Some(u) = y {",
         &["Copy from y to u"][..], ["v", "y", "u", "t"], "take(t);"),
        ("edition2024/while_let_chain_two_sources", "while let Some(b) = y && let Some(c) = z {",
         &["Copy from y to b", "Copy from z to c"][..], ["y", "b", "z", "c"], "take(b);"),
        ("edition2024/if_let_chain_copy_first", "if let Some(b) = y && let Some(a) = &x {",
         &["Copy from y to b", "Partial immutable borrow from x to a"][..], ["x", "a", "y", "b"], "show(a);"),
    ] {
        let rv = run_with_local_backend(name);
        let (timeline, code) = (rv.timeline_panel_string(), rv.code_panel_string());
        let xs: Vec<f64> = cols.iter().map(|l| label_x(&timeline, l)).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{}: columns {:?} at {:?}", name, cols, xs);
        let row = code_row_y(&code, line);
        for text in arrows {
            let arrow = arrow_points(&timeline, text);
            assert_eq!(arrow.len(), 1, "{}: {:?}", name, text);
            assert!(arrow[0].len() == 2 && arrow[0].iter().all(|p| p.1 == row),
                "{}: {:?} isn't a straight arrow along the row y={}: {:?}", name, text, row, arrow[0]);
        }
        assert_eq!(arrows_crossing_dots(&timeline), Vec::<String>::new(), "{}", name);
        let hops: Vec<&str> = start_tags(&timeline, "path")
            .into_iter()
            .filter(|t| attr(t, "stroke") == Some("gray") && attr(t, "d").is_some_and(|d| d.contains(" A ")))
            .collect();
        assert_eq!(hops, Vec::<&str>::new(), "{}: an arrow hops over a dot", name);
        assert_eq!(code_row_y(&code, next), row + 30.0, "{}: a row is added below the line", name);
    }
}

/// Rows of the code panel that carry no source line number: the blank
/// rows the renderer inserts, below a line with stacked arrows, around a
/// conditional whose branches share lines, between functions.
fn unnumbered_code_rows(code: &str) -> usize {
    let mut n = 0;
    let mut rest = code;
    let open = r##"<text class="code" "##;
    let gutter = r##"<tspan fill="#AAA">"##;
    while let Some(i) = rest.find(open) {
        rest = &rest[i + open.len()..];
        let Some(g) = rest.find(gutter) else { break };
        let after = &rest[g + gutter.len()..];
        let end = after.find("</tspan>").unwrap_or(0);
        if after[..end].trim().is_empty() {
            n += 1;
        }
    }
    n
}

/// #419: inside an `if` / `match` arm, a line whose call takes or returns
/// a value got a blank row below it. The arm's line map counted the arrows
/// to and from the function as stacked; at the top level they aren't
/// counted (`event_line_map` holds variable-to-variable arrows only), and
/// nothing is drawn in the row. These fixtures have no other reason for a
/// blank row.
#[test]
fn arm_calls_add_no_blank_rows() {
    for name in [
        "arm_call_no_blank_row",
        "arm_call_borrow_end_no_blank_row",
        "match_arm_call_no_blank_row",
    ] {
        let rv = run_with_local_backend(name);
        let blanks = unnumbered_code_rows(&rv.code_panel_string());
        assert_eq!(blanks, 0, "{}: {} blank row(s) in the code panel", name, blanks);
    }
}

/// #355: the inner `if` of an `else if` belongs to the outer `else`
/// branch, which is drawn to the right of the parent column. It used
/// to be drawn in the outer `if` branch, on the left.
#[test]
fn else_if_is_drawn_in_the_else_branch() {
    let rv = run_with_local_backend("else_if_chain");
    let timeline = rv.timeline_panel_string();
    let mut splits = dots_with_tooltip(&timeline, "s is live in a conditional expression ");
    splits.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    assert_eq!(splits.len(), 2, "{:?}", splits);
    let (outer, inner) = (splits[0], splits[1]);
    assert!(inner.0 > outer.0, "inner `if` at x={} is not right of the parent column x={}", inner.0, outer.0);
}

/// #450: a scrutinee's move happens before the conditional, so its dot is
/// on the parent column, not in an arm's column (arms are drawn to the
/// side of the parent, and the other arm would otherwise show the value
/// still held).
#[test]
fn scrutinee_move_is_on_the_parent_column() {
    for (name, var, born) in [
        ("if_let_scrutinee_move", "v", "v acquires ownership from the caller"),
        ("if_let_scrutinee_move_else_move", "v", "v acquires ownership of a resource"),
        ("match_scrutinee_move", "w", "w acquires ownership of a resource"),
    ] {
        let rv = run_with_local_backend(name);
        let timeline = rv.timeline_panel_string();
        let parent = dots_with_tooltip(&timeline, born);
        let moved = dots_with_tooltip(&timeline, &format!("{}'s resource is moved", var));
        assert_eq!((parent.len(), moved.len()), (1, 1), "{}: dots {:?} {:?}", name, parent, moved);
        assert_eq!(parent[0].0, moved[0].0,
            "{}: the move at {:?} is not on {}'s own column x={}", name, moved[0], var, parent[0].0);
    }
}

/// `(cx, cy)` of every dot in a timeline panel one of whose tooltip
/// lines (see `tooltip_lines`) is `text`: a plain circle, or a tooltip
/// `<g>` around a circle (a drop dot, a dot with a call icon).
fn dots_listing(svg: &str, text: &str) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for (i, _) in svg.match_indices(" data-tooltip-text=\"") {
        let Some(start) = svg[..i].rfind('<') else { continue };
        let tag = &svg[start..start + svg[start..].find('>').unwrap_or(svg.len() - start) + 1];
        let circle = if tag.starts_with("<circle ") {
            Some(tag)
        } else if tag.starts_with("<g ") {
            let body = &svg[start..start + svg[start..].find("</g>").unwrap_or(svg.len() - start)];
            start_tags(body, "circle").first().copied()
        } else {
            None
        };
        let Some((x, y)) = circle.and_then(|c| attr(c, "cx").zip(attr(c, "cy"))) else { continue };
        if tooltip_lines(tag).iter().flatten().any(|l| l == text) {
            out.push((x.parse().unwrap(), y.parse().unwrap()));
        }
    }
    out
}

/// #492: when an `if` / `if let` / `match` shares a line with its arms,
/// its arms are drawn as a diamond around that line, from the split dot
/// above it to the join dot below. The condition or scrutinee runs
/// before any arm, but its event was drawn at the line's own y: in the
/// middle of the diamond, between the arms. It goes on the split dot
/// now, and an event after the conditional on the join dot.
#[test]
fn one_line_condition_events_are_drawn_before_the_split() {
    let timeline = run_with_local_backend("one_line_cond_events").timeline_panel_string();
    let one = |text: &str| -> (f64, f64) {
        let dots = dots_listing(&timeline, text);
        assert_eq!(dots.len(), 1, "dots listing {:?}: {:?}", text, dots);
        dots[0]
    };
    for (var, cond) in [
        ("t", "len reads from t (implicitly borrowed)"),
        ("u", "chars reads from u (implicitly borrowed)"),
        ("w", "len reads from w (implicitly borrowed)"),
    ] {
        let split = one(&format!("{} is live in a conditional expression ", var));
        let join = one(&format!("{} isn't moved in any of the branches above", var));
        let event = one(cond);
        assert_eq!(event.0, split.0, "{:?} is not on {}'s column", cond, var);
        assert!(event.1 <= split.1,
            "{:?} at y={} is below {}'s split dot at y={} (join at y={})", cond, event.1, var, split.1, join.1);
    }
    // `if x.is_empty() { v.len() } else { 0 } + v.capacity()`: the
    // `capacity` call runs after the `if`.
    let join = one("v isn't moved in any of the branches above");
    let after = one("capacity reads from v (implicitly borrowed)");
    assert_eq!(after.0, join.0);
    assert!(after.1 >= join.1, "capacity call at y={} is above v's join dot at y={}", after.1, join.1);
}

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
                // The tooltip is HTML (the page sets it as innerHTML), so a
                // name like `<` is an entity in it (#376): decode it as the
                // browser would.
                let clean = clean
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
                    .replace("&amp;", "&");
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
            "*r is assigned a copy of y",
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
fn shared_reborrow_keeps_param_column_to_its_end() {
    // `fn f(v: &Vec<i32>) { let r = &v[0]; println!("{}", r); }`: `r` is a
    // shared reborrow through `v`, so `v` stays readable until `r`'s last
    // use (line 3). v's column has to run down to the dot that says
    // "v's immutable borrow ends"; it used to stop at line 2, where `r`
    // is created (#416).
    let rv = run_with_local_backend("ref_param_end_shared_line");
    let timeline = rv.timeline_panel_string();
    let end_y: f64 = start_tags(&timeline, "circle")
        .into_iter()
        .find(|t| attr(t, "data-tooltip-text").is_some_and(|x| x.contains("immutable borrow ends")))
        .map(|t| num_attr(t, "cy"))
        .expect("a dot for v's borrow end");
    let first_y: f64 = start_tags(&timeline, "circle")
        .into_iter()
        .find(|t| attr(t, "data-tooltip-text").is_some_and(|x| x.contains("is an immutable borrow from the caller")))
        .map(|t| num_attr(t, "cy"))
        .expect("a dot for v's start");
    // The column's state lines (all of `f`'s v column: same x as the
    // first dot, between its first and last dots) must reach the end dot
    // without a gap.
    let mut spans: Vec<(f64, f64)> = start_tags(&timeline, "line")
        .into_iter()
        .filter(|t| attr(t, "class").is_some_and(|c| c.contains("hollow")))
        .filter(|t| num_attr(t, "y1") >= first_y && num_attr(t, "y2") <= end_y)
        .map(|t| (num_attr(t, "y1"), num_attr(t, "y2")))
        .collect();
    spans.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut reached = first_y;
    for (y1, y2) in &spans {
        if *y1 <= reached { reached = reached.max(*y2); }
    }
    assert_eq!(reached, end_y, "v's column stops at y={} but its borrow ends at y={}", reached, end_y);
    // `end_y` is line 3, one row below the borrow on line 2.
    assert!(end_y - first_y >= 60.0, "v's borrow ends too early: {} .. {}", first_y, end_y);
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
            // #308: a numbered header stops at "declared on line N"; the
            // long "While s₂ is in scope the name s refers to s₂ ..."
            // sentence was dropped as noise.
            assert!(
                !actual.iter().any(|t| t.contains(" is in scope the name ") || t.contains("still exists until its own scope ends")),
                "{}: numbered header still has the trailing scope sentence: {:#?}", expect.name, actual
            );

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
                .filter(|forbid| forbidden_present(forbid, &actual))
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
    for (name, frag, why) in KNOWN_STILL_PRINTED {
        let rv = run_with_local_backend(name);
        let actual = timeline_tooltips(&rv.timeline_panel_string());
        if !actual.iter().any(|a| a.contains(frag)) {
            failures.push(format!(
                "{}: {:?} is no longer printed; move it from KNOWN_STILL_PRINTED to must_not_contain ({})",
                name, frag, why
            ));
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
        .filter(|t| t.starts_with("Return immutably borrowed value from"))
        .collect();
    assert_eq!(
        returns,
        [
            "Return immutably borrowed value from y to x",
            "Return immutably borrowed value from z to x",
            "Return immutably borrowed value from a to x",
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

/// #589: same-named variables of sibling blocks share one column (#308),
/// and each one goes out of scope at its own closing brace. Only the
/// first block's `}` had the scope-end dot.
#[test]
fn sibling_blocks_sharing_a_column_each_end_it() {
    // (snippet, tooltip line, scope-end dots expected)
    for (name, text, count) in [
        ("shadow_shared_columns", "s₃ goes out of scope", 2),
        ("shared_column_scope_ends", "t goes out of scope. Its resource is dropped.", 2),
        ("shared_column_scope_ends", "r goes out of scope", 3),
        ("shadow_sibling_blocks", "s goes out of scope. No resource is dropped.", 1),
        ("shadow_sibling_blocks", "s goes out of scope. Its resource is dropped.", 1),
        // #591: a bare block in an `if` / `else` / match arm / loop.
        ("nested_block_in_branch_scope_end", "u goes out of scope. Its resource is dropped.", 1),
        ("nested_block_in_branch_scope_end", "w goes out of scope. Its resource is dropped.", 1),
        ("nested_block_in_branch_scope_end", "m goes out of scope. Its resource is dropped.", 1),
        ("nested_block_in_branch_scope_end", "k goes out of scope. Its resource is dropped.", 1),
    ] {
        let rv = run_with_local_backend(name);
        let dots = dots_listing(&rv.timeline_panel_string(), text);
        assert_eq!(dots.len(), count, "{}: dots saying {:?}: {:?}", name, text, dots);
        let mut ys: Vec<f64> = dots.iter().map(|d| d.1).collect();
        ys.dedup();
        assert_eq!(ys.len(), count, "{}: the dots share a row: {:?}", name, dots);
    }
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
/// #512 / #513: a borrow stored in a struct (through a local `&mut`,
/// `rh.r = &t`, or by a call whose signature allows it, `point(&mut
/// hold, &t)`) ends where the borrow checker ends it: at the struct's
/// last use, on the same row as the end of s's borrow by `hold.r`. Not
/// on the write's or the call's own dot.
#[test]
fn borrows_stored_in_a_struct_end_at_its_last_use() {
    for (name, pairs) in [
        ("borrow_stored_through_local_ref", &[("t", "s"), ("b", "a")][..]),
        ("borrow_stored_by_call", &[("t", "s")][..]),
    ] {
        let svg = run_with_local_backend(name).timeline_panel_string();
        for (stored, held) in pairs {
            let end = |v: &str| dots_with_tooltip(&svg, &format!("{}'s resource is no longer immutably borrowed", v));
            let (stored_end, held_end) = (end(stored), end(held));
            assert_eq!(stored_end.len(), 1, "{}: {}'s borrow should end on a dot of its own, got {:?}", name, stored, stored_end);
            assert!(held_end.iter().any(|(_, y)| *y == stored_end[0].1),
                "{}: {}'s borrow should end on the row where {}'s does ({:?}), got {:?}", name, stored, held, held_end, stored_end);
        }
        // No call dot that may store a reference also ends its borrow.
        for lines in tooltip_lines(&svg) {
            if lines.iter().any(|l| l.contains("'s signature lets it store this reference")) {
                assert!(!lines.iter().any(|l| l.contains("no longer")),
                    "{}: a stored borrow ends at the call: {:?}", name, lines);
            }
        }
    }
}

/// #531: after `hold.r = &t`, s's borrow (held in the same struct) ends at
/// hold's last use, on t's row, not at the write. For a reference
/// variable the old borrow ends at the reassignment, before t's.
#[test]
fn direct_field_overwrite_keeps_old_borrow_until_last_use() {
    let svg = run_with_local_backend("borrow_field_overwrite_direct").timeline_panel_string();
    let end = |v: &str| dots_with_tooltip(&svg, &format!("{}'s resource is no longer immutably borrowed", v));
    for (new, old) in [("t", "s"), ("b", "a")] {
        let (new_end, old_end) = (end(new), end(old));
        assert_eq!(new_end.len(), 1, "{}: {:?}", new, new_end);
        assert_eq!(old_end.len(), 1, "{}: {:?}", old, old_end);
        assert_eq!(old_end[0].1, new_end[0].1,
            "{}'s borrow should end where {}'s does (hold's last use), got {:?} vs {:?}", old, new, old_end, new_end);
    }
    let (x_end, y_end) = (end("x"), end("y"));
    assert_eq!((x_end.len(), y_end.len()), (1, 1));
    assert!(x_end[0].1 < y_end[0].1, "x's borrow ends at the reassignment, before y's: {:?} {:?}", x_end, y_end);
}

/// #538: `hold = H { r: &t };` drops the old struct, so s's borrow ends
/// on the overwrite's row, the row where t's borrow starts, and before
/// the later `&mut s`.
#[test]
fn whole_struct_overwrite_ends_old_borrow_at_the_write() {
    let svg = run_with_local_backend("borrow_struct_overwrite_whole").timeline_panel_string();
    let s_end = dots_with_tooltip(&svg, "s's resource is no longer immutably borrowed");
    let s_mut_end = dots_with_tooltip(&svg, "s's resource is no longer mutably borrowed");
    assert_eq!((s_end.len(), s_mut_end.len()), (1, 1), "{:?} {:?}", s_end, s_mut_end);
    let mut write = dots_with_tooltip(&svg, "hold is assigned a new value");
    write.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert!(!write.is_empty());
    assert_eq!(s_end[0].1, write[0].1,
        "s's borrow should end on the write's row, with t's start: {:?} vs {:?}", s_end, write);
    assert!(s_end[0].1 < s_mut_end[0].1, "{:?} {:?}", s_end, s_mut_end);
}

/// #569: `hold = make(&t);` and `hold = other;` end the old borrow on the
/// write's row, like a struct literal (#538), before the later `&mut`.
/// `o.inner = H { r: &f };` overwrites a field of `o`, which stays live:
/// e's borrow ends at o's last use, on the println!'s row (#531). When the
/// new value borrows what the old one did (`hold = make(hold.r)`), x stays
/// borrowed: its borrow ends once.
#[test]
fn struct_overwrite_by_call_or_variable_ends_old_borrow_at_the_write() {
    let rv = run_with_local_backend("borrow_struct_overwrite_call");
    let (svg, code) = (rv.timeline_panel_string(), rv.code_panel_string());
    let end = |v: &str| dots_with_tooltip(&svg, &format!("{}'s resource is no longer immutably borrowed", v));
    for (v, line) in [
        ("s", "hold = make(&t);"),
        ("a", "hold = other;"),
        ("e", "println!(\"{} {}\", o.inner.r, o.n);"),
    ] {
        let ends = end(v);
        assert_eq!(ends.len(), 1, "{}: {:?}", v, ends);
        assert_eq!(ends[0].1, code_row_y(&code, line), "{}'s borrow should end on the row of `{}`: {:?}", v, line, ends);
    }
    for (v, later) in [("s", "let m = &mut s;"), ("a", "let m = &mut a;")] {
        assert!(end(v)[0].1 < code_row_y(&code, later), "{}'s borrow should end before `{}`", v, later);
    }
    assert_eq!(end("x").len(), 1, "{:?}", end("x"));
}

/// #638: `o.inner = mk(&t);` is a partial write, so s's old borrow ends
/// at o's last use (the println!), as for `o.inner = I { r: &f };`
/// (#531), while the borrow of t that o.inner.r now holds starts at the
/// write. Same for the `&mut` version (u, v) and a method call (a, w).
#[test]
fn struct_field_overwrite_by_call_borrows_per_signature() {
    let rv = run_with_local_backend("borrow_struct_field_overwrite_call");
    let (svg, code) = (rv.timeline_panel_string(), rv.code_panel_string());
    for (tip, line) in [
        ("s's resource is no longer immutably borrowed", "println!(\"{} {}\", o.inner.r, o.n);"),
        ("t's resource is immutably borrowed by o.inner.r; t is read-only while the borrow is live", "o.inner = mk(&t);"),
        ("u's resource is no longer mutably borrowed", "println!(\"{}\", p.n);"),
        ("v's resource is mutably borrowed by p.inner.r; v can't be used while the borrow is live", "p.inner = M::new(&mut v);"),
        ("a's resource is no longer immutably borrowed", "println!(\"{} {}\", o.n, o.inner.r);"),
        ("w's resource is immutably borrowed by o.inner.r; w is read-only while the borrow is live", "o.inner = w.view();"),
    ] {
        let dots = dots_listing_tooltip(&svg, tip);
        assert_eq!(dots.len(), 1, "{}: {:?}", tip, dots);
        assert_eq!(dots[0].1, code_row_y(&code, line), "`{}` should be on the row of `{}`: {:?}", tip, line, dots);
    }
}

/// #637: a struct moved to another variable takes its field's borrow
/// along. The lender is given back once, at the new holder's last use:
/// the loan is live while any value holding the reference is (the line
/// after it uses the lender again, which rustc accepts). Moved inside an
/// arm, the borrow ends at hold's last use in that arm, and as the other
/// arm starts (other.r isn't used there): once per path.
#[test]
fn moved_struct_returns_its_borrow_from_the_new_holder() {
    let rv = run_with_local_backend("borrow_struct_moved_holder");
    let (svg, code) = (rv.timeline_panel_string(), rv.code_panel_string());
    let end = |v: &str, kind: &str| dots_with_tooltip(&svg, &format!("{}'s resource is no longer {} borrowed", v, kind));
    for (v, kind, line) in [
        ("t", "immutably", "println!(\"{}\", hold.r);"),
        ("b", "immutably", "println!(\"assigned {}\", hold.r);"),
        ("u", "mutably", "hold.r.push('1');"),
        ("e", "immutably", "println!(\"{} {}\", o2.inner.r, o2.n);"),
        ("f", "immutably", "println!(\"call {}\", hold.r);"),
        ("p", "immutably", "println!(\"source {}\", hold.r);"),
        ("q", "immutably", "println!(\"source {}\", other.r);"),
    ] {
        let ends = end(v, kind);
        assert_eq!(ends.len(), 1, "{}: {:?}", v, ends);
        assert_eq!(ends[0].1, code_row_y(&code, line), "{}'s borrow should end on the row of `{}`: {:?}", v, line, ends);
    }
    let mut w: Vec<f64> = end("w", "immutably").into_iter().map(|d| d.1).collect();
    w.sort_by(f64::total_cmp);
    assert_eq!(w.len(), 2, "w: {:?}", w);
    assert_eq!(w[0], code_row_y(&code, "println!(\"branch {}\", hold.r);"), "w: {:?}", w);
    assert!(w[1] > w[0] && w[1] < code_row_y(&code, "w.push('!');"), "w's other end should be at the if's end: {:?}", w);
}

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

/// What an or-pattern arm that binds a name from different places
/// still can't draw (#565) is refused with a message that says which
/// case it is, rather than drawn as taken from the first place: a
/// binding through different references, a `&mut` moved out of one of
/// the places, and a struct moved into the tuple being matched.
#[test]
fn or_pattern_binding_from_different_places_refusals() {
    env::set_var("RV_RUNNER", "local");
    for (src, what) in [
        ("fn main() {\n    let a = Some(String::from(\"a\"));\n    let b = Some(String::from(\"b\"));\n    let (r1, r2) = (&a, &b);\n    match (r1, r2) {\n        (Some(x), _) | (_, Some(x)) => println!(\"{}\", x),\n        _ => {}\n    }\n}\n",
         "or-patterns that bind `x` through different references being matched"),
        ("fn main() {\n    let mut a = String::from(\"a\");\n    let mut b = String::from(\"b\");\n    let t = (Some(&mut a), Some(&mut b));\n    match t {\n        (Some(x), _) | (_, Some(x)) => x.push('!'),\n        _ => {}\n    }\n}\n",
         "or-patterns that bind the mutable reference `x` from different places in the value being matched"),
        // A struct moved into the tuple being matched: the tuple isn't
        // drawn for it (#327), so x would look taken from a itself.
        ("struct P { n: String }\nfn main() {\n    let a = P { n: String::from(\"a\") };\n    let b = P { n: String::from(\"b\") };\n    match (a, b) {\n        (x, _) | (_, x) => println!(\"{}\", x.n),\n    }\n}\n",
         "or-patterns that bind `x` from different values of a tuple being matched, when a value is a struct or tuple"),
    ] {
        let err = Rustviz::new(src).expect_err("an or-pattern binding of this kind must not render");
        let msg = err.to_string();
        assert!(msg.contains(what), "expected the or-pattern refusal {:?}, got:\n{}", what, msg);
    }
}

/// The same refusal for an `if let` / `let` or-pattern (#597): a
/// reference or a tuple / struct name bound from different places.
#[test]
fn or_pattern_let_binding_a_reference_or_struct_from_different_places_is_refused() {
    env::set_var("RV_RUNNER", "local");
    for (src, what) in [
        ("fn main() {\n    let t = (String::from(\"a\"), String::from(\"b\"));\n    if let (ref x, _) | (_, ref x) = t {\n        println!(\"{}\", x);\n    }\n}\n",
         "the reference `x`"),
        ("fn main() {\n    let t = (String::from(\"a\"), String::from(\"b\"));\n    let ((ref x, _) | (_, ref x)) = t;\n    println!(\"{}\", x);\n}\n",
         "the reference `x`"),
        ("struct P { n: String }\nfn main() {\n    let t = (P { n: String::from(\"a\") }, P { n: String::from(\"b\") });\n    if let (x, _) | (_, x) = t {\n        println!(\"{}\", x.n);\n    }\n}\n",
         "the struct or tuple `x`"),
        ("struct P { n: String }\nfn main() {\n    let t = (P { n: String::from(\"a\") }, P { n: String::from(\"b\") });\n    let ((x, _) | (_, x)) = t;\n    println!(\"{}\", x.n);\n}\n",
         "the struct or tuple `x`"),
    ] {
        let err = Rustviz::new(src).expect_err("an or-pattern binding of this kind must not render");
        let msg = err.to_string();
        assert!(msg.contains(&format!("or-patterns that bind {} from different places in the value being matched", what)),
            "expected the or-pattern refusal, got:\n{}", msg);
    }
}

/// A `return` inside a branch drops every value the function still owns
/// right there (#363), so those values get a second drop dot, on the
/// returning branch, besides the one at the end of the function. A
/// value moved away before the `return` gets none there.
///
/// #410: the same for the other ways of leaving early. A panic drops
/// everything (`panic!`, `todo!`, `unreachable!`); `break` / `continue`
/// drop what the loop's iteration declared; `return s.len()` drops the
/// borrowed `s`. What a `return` drops follows the path to it: nothing
/// that only another arm gives a value, and nothing that a conditional
/// above moved on some paths only (`m`, whose drop depends on a runtime
/// drop flag). A var written as `"m:No"` counts "m goes out of scope.
/// No resource is dropped." instead, and `"m:Flag"` the scope-end
/// wording for a value moved on some paths only (#179).
#[test]
fn return_drops_what_the_branch_owns() {
    for (name, expected) in [
        ("diverging_branch_stops", &[("s", 2), ("t", 1)][..]),
        ("return_in_arm_drops", &[("v", 2), ("w", 1)][..]),
        // `consume`'s parameter `s` is dropped once more.
        ("match_diverging_arm", &[("s", 3), ("t", 1)][..]),
        ("exit_panic_drops", &[("s", 3), ("t", 3)][..]),
        ("exit_unreachable_else", &[("s", 2), ("t", 2)][..]),
        // `b`: at the `break 'outer` and at the end of the outer body
        // (#256); `c`: at the `break 'outer`, the `continue` and the end
        // of the inner body.
        ("exit_break_continue_drops", &[("a", 1), ("b", 2), ("c", 3)][..]),
        // `guess`: at the `continue`, at the `break` in the `if` without
        // `else` (#429) and at the end of the loop body (#256).
        ("exit_continue_in_match", &[("guess", 3), ("secret", 1)][..]),
        ("exit_return_borrowed_value", &[("s", 2), ("t", 2), ("u", 2), ("v", 2)][..]),
        ("exit_return_after_nested_if", &[("s", 2), ("t", 2)][..]),
        // `m` (moved on one path above): no drop of any kind at the
        // `return`, and the drop-flag wording once, at the end of `g`.
        // (Before #179's wording reached this branch, that scope-end
        // dot said "No resource is dropped.", false on the `else` path.)
        ("exit_path_dependent_drops", &[("s", 1), ("t", 1), ("m", 0), ("m:No", 0), ("m:Flag", 1)][..]),
        // `s` has no value yet at the `return`: dropped only at the end.
        ("exit_return_in_let_if", &[("s", 1), ("s:No", 0), ("t", 2)][..]),
        // #429: exits in an `if` without `else`. `t` is dropped at the
        // `return` and then returned (so its scope end drops nothing).
        ("exit_if_no_else_return", &[("t", 1), ("t:No", 1)][..]),
        ("exit_if_no_else_panic", &[("v", 2)][..]),
        // `u`: at the `break`, at the `continue` and at the end of the
        // loop body (#256).
        ("exit_if_no_else_break", &[("u", 3), ("s", 1)][..]),
        ("exit_if_no_else_nested", &[("t", 2), ("u", 2)][..]),
        ("exit_if_let_no_else_return", &[("t", 2), ("s", 1)][..]),
        // On one line too (#360): at the `return` and at the end.
        ("exit_if_no_else_one_line", &[("t", 2)][..]),
        // `v` was moved by the scrutinee: no drop at the `return` or at
        // its scope end.
        ("exit_if_let_scrutinee_move_return", &[("t", 2), ("v", 0), ("v:No", 1)][..]),
        // The `if` body returns: `t` and `v` are dropped at the `return`
        // and at the end; `u` (declared in the body) once (#630). `v` is
        // a Vec of Strings, so its drop says so (#542).
        ("try_in_branch", &[("t", 2), ("v:Strings", 2), ("u", 1)][..]),
    ] {
        let rv = run_with_local_backend(name);
        let tooltips = timeline_tooltips(&rv.timeline_panel_string());
        for (var, count) in expected {
            let drop = if let Some(v) = var.strip_suffix(":No") {
                format!("{} goes out of scope. No resource is dropped.", v)
            } else if let Some(v) = var.strip_suffix(":Strings") {
                format!("{} goes out of scope. Its resource is dropped (including every String it holds).", v)
            } else if let Some(v) = var.strip_suffix(":Flag") {
                format!(
                    "{} goes out of scope. Its resource is dropped here only if no branch above \
                     moved it (Rust tracks this with a runtime drop flag).",
                    v
                )
            } else {
                format!("{} goes out of scope. Its resource is dropped.", var)
            };
            assert_eq!(
                tooltips.iter().filter(|t| **t == drop).count(),
                *count,
                "{}: expected {:?} {} time(s), tooltips were:\n  {}",
                name, drop, count, tooltips.join("\n  ")
            );
        }
    }
}

/// What a let-chain still can't draw (#579): a part that assigns a new
/// reference to, writes through, or gives a first value to a variable
/// declared outside the condition, followed by a part that can be false.
/// Each lasts on the path into the `else`, where RustViz can't show
/// which borrow is live, or that the variable may hold no value.
#[test]
fn if_let_chain_lasting_effects_still_refused() {
    env::set_var("RV_RUNNER", "local");
    let cases = [
        ("fn main() {\n\
          \x20   let s = String::from(\"a\");\n\
          \x20   let t = String::from(\"b\");\n\
          \x20   let y = Some(1);\n\
          \x20   let mut r = &t;\n\
          \x20   if let Some(_n) = y && { r = &s; r.len() > 3 } {\n\
          \x20       println!(\"{}\", r);\n\
          \x20   } else {\n\
          \x20       println!(\"{}\", r);\n\
          \x20   }\n\
          }\n",
         "`if let` chains that assign a new reference to a variable declared outside the condition (`r`, line 6)"),
        ("fn main() {\n\
          \x20   let y = Some(5);\n\
          \x20   let mut n = 0;\n\
          \x20   let r = &mut n;\n\
          \x20   if let Some(a) = y && { *r = a; a > 3 } {\n\
          \x20       println!(\"big\");\n\
          \x20   }\n\
          \x20   println!(\"{}\", r);\n\
          }\n",
         "`if let` chains that assign to a place reached through a variable declared outside the condition (`*r`, line 5)"),
        ("fn main() {\n\
          \x20   let y = Some(String::from(\"a\"));\n\
          \x20   let mut z = String::from(\"z\");\n\
          \x20   drop(z);\n\
          \x20   if let Some(a) = y && { z = a; z.len() > 3 } {\n\
          \x20       println!(\"{}\", z);\n\
          \x20   }\n\
          }\n",
         "`if let` chains that assign to a variable declared outside the condition that may hold no value before (`z`, line 5)"),
    ];
    for (src, want) in cases {
        let err = Rustviz::new(src).expect_err("a let-chain with this lasting effect must not render");
        let msg = err.to_string();
        assert!(msg.contains(want), "expected {:?}, got:\n{}", want, msg);
    }
}

/// Column header labels in one row as (x, visible text), left to right.
/// A reference column's label is `r|*r` (the `|` sits in a `<tspan>`).
fn header_labels(timeline: &str) -> Vec<(f64, f64, String)> {
    let mut out = Vec::new();
    for t in timeline.split("<text ").skip(1) {
        let (tag, rest) = t.split_once('>').unwrap();
        if !tag.contains("class=\"label") {
            continue;
        }
        let body = &rest[..rest.find("</text>").unwrap()];
        let mut text = String::new();
        let mut in_tag = false;
        for c in body.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => text.push(c),
                _ => {}
            }
        }
        out.push((num_attr(tag, "x"), num_attr(tag, "y"), text));
    }
    out.sort_by(|a, b| (a.1, a.0).partial_cmp(&(b.1, b.0)).unwrap());
    out
}

/// Neighbouring header labels don't overlap, and the last one fits in the
/// panel. Labels are centred, monospace at 14px (about 8.4px a glyph).
/// `h.e.part|*h.e.part` used to run into `h.e.n` (#361).
#[test]
fn long_header_labels_do_not_overlap() {
    const GLYPH: f64 = 8.4;
    for name in ["long_ref_field_label", "long_ref_label_then_short"] {
        let t = run_with_local_backend(name).timeline_panel_string();
        let labels = header_labels(&t);
        for w in labels.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if a.1 != b.1 {
                continue;
            }
            let a_right = a.0 + a.2.chars().count() as f64 * GLYPH / 2.0;
            let b_left = b.0 - b.2.chars().count() as f64 * GLYPH / 2.0;
            assert!(
                a_right < b_left,
                "{}: label {:?} (x={}) overlaps {:?} (x={})",
                name, a.2, a.0, b.2, b.0
            );
        }
        let svg_tag = &t[t.find("<svg").unwrap() + 4..];
        let width: f64 = {
            let k = " width=\"";
            let v = &svg_tag[svg_tag.find(k).unwrap() + k.len()..];
            v[..v.find('"').unwrap()].trim_end_matches("px").parse().unwrap()
        };
        let last = labels.iter().map(|l| l.0 + l.2.chars().count() as f64 * GLYPH / 2.0).fold(0.0, f64::max);
        assert!(last <= width, "{}: rightmost label ends at {} past the panel width {}", name, last, width);
    }
}

/// A struct box contains every header label of its own columns, and no
/// label outside the struct touches it (#477). The box used to span a
/// fixed 20px / 40px around the first / last column, so a long last
/// member (`self.samples`) stuck out on the right.
#[test]
fn struct_box_contains_long_member_labels() {
    const GLYPH: f64 = 8.4;
    for name in ["struct_box_long_last_label", "struct_box_long_owner_label"] {
        let t = run_with_local_backend(name).timeline_panel_string();
        let labels = header_labels(&t);
        let mut checked = 0;
        for r in t.split("<rect ").skip(1).map(|r| &r[..r.find('>').unwrap()]).filter(|r| r.contains("rx=\"20\"")) {
            let (left, y) = (num_attr(r, "x"), num_attr(r, "y"));
            let right = left + num_attr(r, "width");
            for (x, ly, text) in labels.iter().filter(|l| l.1 == y + 20.0) {
                let half = text.chars().count() as f64 * GLYPH / 2.0;
                if left < *x && *x < right {
                    // inside the box: the whole label must be
                    assert!(left <= x - half && x + half <= right,
                        "{}: label {:?} (x={}, y={}) sticks out of box {}..{}", name, text, x, ly, left, right);
                    checked += 1;
                } else {
                    // outside the box: must not overlap it
                    assert!(x + half <= left || right <= x - half,
                        "{}: label {:?} (x={}) collides with box {}..{}", name, text, x, left, right);
                }
            }
        }
        assert!(checked >= 2, "{}: expected labels inside a struct box", name);
    }
}

/// #342: a user-defined macro whose body does something to a variable it
/// was given (`drop($e)`, `$e.len()`, `let t = $e;`) used to draw nothing,
/// so the variable was later said to drop a String it had given away.
/// Such programs are refused. (A macro that only forwards to `println!`
/// still renders: see the `user_macro_forwards` fixture.)
#[test]
fn user_macro_that_uses_a_variable_is_refused() {
    env::set_var("RV_RUNNER", "local");
    let cases = [
        // the expansion is a call
        "macro_rules! take { ($e:expr) => { drop($e) }; }\n\
         fn main() {\n\
         \x20   let z = String::from(\"f\");\n\
         \x20   take!(z);\n\
         }\n",
        // the expansion is a method call
        "macro_rules! len { ($e:expr) => { $e.len() }; }\n\
         fn main() {\n\
         \x20   let z = String::from(\"f\");\n\
         \x20   let n = len!(z);\n\
         }\n",
        // the expansion is a statement list with a `let`
        "macro_rules! mk { ($e:expr) => { let t = $e; drop(t); }; }\n\
         fn main() {\n\
         \x20   let z = String::from(\"f\");\n\
         \x20   mk!(z);\n\
         }\n",
    ];
    for src in cases {
        let err = Rustviz::new(src).expect_err("a user macro that uses a variable must not render");
        let msg = err.to_string();
        assert!(msg.contains("user-defined macros that use a variable (`z`, line 4) aren't supported yet"),
            "expected the user-macro refusal for:\n{}\ngot:\n{}", src, msg);
        assert!(msg.starts_with("RustViz can't visualize this program yet"), "{}", msg);
    }
}

/// #515: when every branch of an `if` / `else` (or arm of a `match`)
/// returns, nothing follows the conditional: `r` is dropped once at each
/// `return`, and not again at the closing brace.
#[test]
fn both_branches_return_end_the_function() {
    let rv = run_with_local_backend("if_else_both_return");
    let tooltips = timeline_tooltips(&rv.timeline_panel_string());
    for (var, count) in [("r", 2), ("a", 2), ("q", 0), ("b", 0)] {
        let drop = format!("{} goes out of scope. Its resource is dropped.", var);
        assert_eq!(
            tooltips.iter().filter(|t| **t == drop).count(),
            count,
            "expected {:?} {} time(s), tooltips were:\n  {}",
            drop, count, tooltips.join("\n  ")
        );
    }
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

/// #472: arrow shafts start or end at the centre of a dot or function
/// icon, so the arrows group must come before the events group in the
/// SVG: later siblings paint (and hit-test) on top.
#[test]
fn arrows_are_drawn_beneath_dots_and_function_icons() {
    let rv = run_with_local_backend("arrow_hops_over_dots");
    let timeline = rv.timeline_panel_string();
    let arrows = timeline.find("<g id=\"arrows\">").expect("arrows group");
    let events = timeline.find("<g id=\"events\">").expect("events group");
    assert!(arrows < events, "g#arrows must precede g#events");
    assert!(timeline.matches("<g id=\"arrows\">").count()
        == timeline.matches("<g id=\"events\">").count());
}

/// Every tooltip attribute is escaped: no bare `<`, and every `&` starts
/// an entity. An operator named `&` or `<` used to be written raw, and
/// the timeline SVG was not valid XML (#376).
#[test]
fn tooltip_attributes_are_escaped() {
    for name in ["operator_name_escaped", "comparison_operand_borrows"] {
        let t = run_with_local_backend(name).timeline_panel_string();
        for chunk in t.split("data-tooltip-text=\"").skip(1) {
            let raw = &chunk[..chunk.find('"').unwrap()];
            assert!(!raw.contains('<'), "{}: bare '<' in tooltip attribute {:?}", name, raw);
            for (i, _) in raw.match_indices('&') {
                let rest = &raw[i..];
                assert!(
                    ["&lt;", "&gt;", "&quot;", "&amp;", "&#"].iter().any(|e| rest.starts_with(e)),
                    "{}: bare '&' in tooltip attribute {:?}", name, raw
                );
            }
        }
    }
}

/// #308: a numbered column header names the variable several times
/// (`b₂ shadows b: it is a new variable named b ...`). The playground's
/// helpers.js used to re-split the tooltip HTML on spaces to wrap it and
/// cut inside the `<span style="...monospace !important;">` wrappers, so
/// the second occurrence leaked markup. The plugin's text is pinned here
/// (every header reads as the expected plain string, and the raw
/// attribute is nothing but well-formed name spans); the JS is pinned by
/// `tooltip_wrapping_is_css_not_word_splitting`.
#[test]
fn numbered_headers_are_clean_markup() {
    let rv = run_with_local_backend("shadow_header_names");
    let timeline = rv.timeline_panel_string();
    let tooltips = timeline_tooltips(&timeline);
    for v in ["s", "n", "e", "a", "i", "b"] {
        let want = format!(
            "{v}₂, immutable. {v}₂ shadows {v}: it is a new variable named {v}, declared on line {line}",
            line = match v { "s" => 7, "n" => 9, "e" => 11, "a" => 13, "i" => 15, _ => 17 },
        );
        assert!(tooltips.contains(&want), "missing header {:?} in {:#?}", want, tooltips);
        let old = format!("{want}. While {v}₂ is in scope the name {v} refers to {v}₂, but {v} still exists until its own scope ends");
        assert!(!tooltips.contains(&old), "old long header still present: {:?}", old);
        assert!(!tooltips.iter().any(|t| t.contains(&format!("While {v}₂ is in scope"))), "trailing sentence in {:#?}", tooltips);
    }
    // Raw attributes: remove each well-formed name span (as the plugin
    // writes it, entity-escaped) and nothing of the markup may remain.
    let span_open = "&lt;span style=&quot;font-family: 'Source Code Pro',";
    for chunk in timeline.split("data-tooltip-text=\"").skip(1) {
        let raw = &chunk[..chunk.find('"').unwrap()];
        let mut rest = raw.to_string();
        while let Some(i) = rest.find(span_open) {
            let close = rest[i..].find("&lt;/span&gt;").expect("unclosed span") + i + "&lt;/span&gt;".len();
            let open_end = rest[i..].find("&gt;").unwrap() + i + 4;
            let name = &rest[open_end..close - "&lt;/span&gt;".len()];
            assert!(!name.contains("&lt;") && !name.contains("&quot;") && !name.contains("span"),
                "name span holds markup: {:?}", raw);
            rest.replace_range(i..close, "");
        }
        // (a dot shared by several events joins them with `<br>`)
        let rest = rest.replace("&lt;br&gt;", "");
        for leak in ["monospace", "!important", "&quot;", "&lt;", "&gt;", "style="] {
            assert!(!rest.contains(leak), "{:?} leaks outside a name span in {:?}", leak, raw);
        }
    }
}

/// The playground's and mdbook's copies of helpers.js must wrap
/// tooltips with CSS, as `rustviz-lib/src/helpers.js` does. Their old
/// `breakText` split the tooltip's HTML on spaces and cut through name
/// spans, leaking `monospace !important;">` into long tooltips.
#[test]
fn tooltip_wrapping_is_css_not_word_splitting() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    for rel in [
        "rustviz-lib/src/helpers.js",
        "playground/frontend/public/ex-assets/helpers.js",
        "mdbook-rustviz/test-book/mdbook_plugin/helpers.js",
    ] {
        let js = fs::read_to_string(root.join(rel)).unwrap();
        assert!(!js.contains("breakText("), "{} still word-splits tooltip HTML", rel);
        assert!(js.contains("overflow-wrap"), "{} doesn't wrap tooltips in CSS", rel);
    }
}

/// A reference bound by an `if` only exists once the branches have joined,
/// so its dotted "holds a reference" bracket must start below the branch
/// diamond, not between its arms (#489). On main the bracket started at the
/// join line itself (y=235), cutting through the diamond that spans
/// y=205..265.
#[test]
fn reference_bracket_starts_below_the_branch_diamond() {
    let t = run_with_local_backend("branch_ref_bracket_if").timeline_panel_string();
    // The bracket: `<path ... class="staticref ..." d="M x y ..."` for `x`.
    let bracket = t
        .lines()
        .find(|l| l.contains("class=\"staticref") && l.contains("&gt;x&lt;/span&gt; holds a"))
        .expect("bracket for x");
    let d = &bracket[bracket.find("d=\"M ").unwrap() + 5..];
    let mut nums = d.split(|c: char| c == ' ' || c == '"');
    let _x: f64 = nums.next().unwrap().parse().unwrap();
    let y: f64 = nums.next().unwrap().parse().unwrap();
    // The `x` column's branch dots: the apex ("live in a conditional") and
    // the arm's initialization dot. The diamond's bottom corner is the
    // lowest arm dot's line plus half a row.
    let mut arm_ys = Vec::new();
    for c in t.lines().filter(|l| l.contains("<circle") && l.contains("data-hash=\"4\"")
        && (l.contains("live in a conditional") || l.contains("initialized by")
            || l.contains("initialized with a copy of") || l.contains("immutably borrows a resource"))) {
        let cy = c.split("cy=\"").nth(1).unwrap().split('"').next().unwrap();
        arm_ys.push(cy.parse::<f64>().unwrap());
    }
    let join = arm_ys.iter().cloned().fold(f64::MIN, f64::max);
    assert!(
        y >= join + 30.0,
        "x's bracket starts at y={} inside the branch diamond (arm dots up to y={}, \
         diamond ends at y={})",
        y, join, join + 30.0
    );
}

/// #506: the dot of `s = String::from("b")` says the new resource is
/// acquired and the old one dropped; it doesn't also say, as a separate
/// bullet, that `s` "acquires ownership of a resource".
#[test]
fn reassignment_dot_has_one_line() {
    let rv = run_with_local_backend("reassign_one_bullet");
    let timeline = rv.timeline_panel_string();
    let dot = tooltip_lines(&timeline).into_iter()
        .find(|l| l.iter().any(|t| t.contains("its previous resource is dropped")))
        .expect("a reassignment dot");
    assert_eq!(dot, vec![
        "s acquires ownership of a new resource; its previous resource is dropped".to_owned(),
    ]);
}

/// #482: the iterator a `for` loop takes from a variable (`for s in
/// it3`) is dropped where the loop is left: a drop dot (circle and
/// triangle) on the variable's column at the loop's closing brace, and
/// at a `return` inside the loop.
#[test]
fn loop_iterator_drop_dots_sit_where_the_loop_is_left() {
    let rv = run_with_local_backend("for_iterator_drop");
    let (timeline, code) = (rv.timeline_panel_string(), rv.code_panel_string());
    let row = |src: &str| code_row_y(&code, src);
    let step = row("for s in it3 {") - row("let it3 = z.into_iter();");
    let drop_dot = |text: &str, want_y: f64, var_line: &str| {
        let dots = dots_with_tooltip(&timeline, text);
        assert_eq!(dots.len(), 1, "one dot {:?}: {:?}", text, dots);
        let (x, y) = dots[0];
        assert_eq!(y, want_y, "{:?} is at y={}, not on the row y={}", text, y, want_y);
        // A drop dot: the tooltip `<g>` wraps a circle and the triangle.
        let at = timeline.find(&format!("<circle cx=\"{}\" cy=\"{}\"", x, y)).expect("its circle");
        let g = &timeline[timeline[..at].rfind("<g ").unwrap()..];
        assert!(g[..g.find("</g>").unwrap()].contains("<polygon points="), "{:?} isn't a drop dot", text);
        // On the variable's own column (its move dot's x).
        let moved = dots_with_tooltip(&timeline, var_line);
        assert!(moved.iter().any(|&(mx, _)| (mx - x).abs() < 30.0), "{:?} at x={} is off the column of {:?} ({:?})", text, x, var_line, moved);
    };
    // `for s in it3 {` .. `}`: the closing brace is two rows down.
    drop_dot(
        "it3's former resource, the `for` loop's iterator, is dropped when the `for` loop ends, along with whatever it still owns (such as elements it hasn't handed out yet)",
        row("for s in it3 {") + 2.0 * step,
        "it3's resource is moved into the `for` loop, which uses it as its iterator",
    );
    drop_dot(
        "words's former resource, the `for` loop's iterator, is dropped here: the function returns from inside the loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)",
        row("return word.len();"),
        "words's resource is moved into the `for` loop, which uses it as its iterator",
    );
    // `for word in words {` .. `}`: six rows down.
    drop_dot(
        "words's former resource, the `for` loop's iterator, is dropped when the `for` loop ends, along with whatever it still owns (such as elements it hasn't handed out yet)",
        row("for word in words {") + 6.0 * step,
        "words's resource is moved into the `for` loop, which uses it as its iterator",
    );
}

/// #592: the drops at a `break 'outer` sit on the `break`'s row, not at
/// the closing brace of the loop the iterator belongs to.
#[test]
fn break_outer_iterator_drops_sit_on_the_break() {
    let rv = run_with_local_backend("for_iterator_break_outer");
    let (timeline, code) = (rv.timeline_panel_string(), rv.code_panel_string());
    let rows: Vec<f64> = code_rows_y(&code, "break 'outer;");
    assert_eq!(rows.len(), 2, "two `break 'outer;` rows: {:?}", rows);
    for (text, row) in [
        ("This temporary IntoIter<String> is dropped here, where `break` leaves early: nothing bound it to a variable or moved it on. Its resource is dropped", rows[0]),
        ("it's former resource, the `for` loop's iterator, is dropped here: this `break` leaves the `for` loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)", code_row_y(&code, "break 'scan;")),
    ] {
        let ys = dots_with_tooltip(&timeline, text);
        assert_eq!(ys.len(), 1, "one dot {:?}: {:?}", text, ys);
        assert_eq!(ys[0].1, row, "{:?} isn't on its `break` row", text);
    }
    // #596: the target loop's own iterator is dropped on the `break 'outer`
    // row too (the first one in the file, the second function's is the
    // variable `outer_it`'s), and its column ends there on the break path:
    // the end-of-loop drop stays for the paths that don't break.
    for (text, row) in [
        ("This temporary IntoIter<Vec<String>> is dropped here, where `break` leaves early: nothing bound it to a variable or moved it on. Its resource is dropped", rows[0]),
        ("outer_it's former resource, the `for` loop's iterator, is dropped here: this `break` leaves the `for` loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)", rows[1]),
    ] {
        let ys = dots_with_tooltip(&timeline, text);
        assert_eq!(ys.len(), 1, "one dot {:?}: {:?}", text, ys);
        assert_eq!(ys[0].1, row, "{:?} isn't on its `break 'outer;` row", text);
    }
}

/// cy of every timeline dot whose tooltip contains `needle` (the dot
/// on a row shared by several events lists them as bullets).
fn dot_ys(timeline: &str, needle: &str) -> Vec<f64> {
    let mut out: Vec<f64> = timeline
        .split("<circle ")
        .skip(1)
        .map(|c| &c[..c.find("/>").unwrap_or(c.len())])
        .filter(|c| timeline_tooltips(c).iter().any(|t| t.contains(needle)))
        .map(|c| num_attr(c, "cy"))
        .collect();
    // A function's dot (`println!`, `push`) is a `<g>` that carries the
    // tooltip and wraps the `<circle>`.
    for g in timeline.split("<g class=\"tooltip-trigger\"").skip(1) {
        let Some(open_end) = g.find('>') else { continue };
        let Some(circle) = g[open_end..].find("<circle ") else { continue };
        if !g[open_end..open_end + circle].trim_start_matches('>').trim().is_empty() {
            continue;
        }
        if timeline_tooltips(&g[..open_end]).iter().any(|t| t.contains(needle)) {
            let c = &g[open_end + circle..];
            out.push(num_attr(&c[..c.find("/>").unwrap_or(c.len())], "cy"));
        }
    }
    out
}

/// A reference bound and last used on the same line (#371): `Some(s) =>
/// println!("{}", s),` on `&opt`, or a one-line `if let`. The borrow's
/// end must be drawn below its start and no higher than the use, on
/// both columns. It used to sit on the line's first row, next to the
/// borrow, while the use landed on the row below.
#[test]
fn same_line_borrow_ends_below_its_use() {
    for (name, kind, user) in [
        ("match_arm_macro_ref_binding", "immutabl", "println!"),
        ("match_arm_call_mut_ref_binding", "mutabl", "push"),
        ("iflet_one_line_ref_binding", "immutabl", "println!"),
    ] {
        let rv = run_with_local_backend(name);
        let tl = rv.timeline_panel_string();
        // The first such dot: `opt` is borrowed again after the arm.
        let one = |needle: String| -> f64 {
            let ys = dot_ys(&tl, &needle);
            assert!(!ys.is_empty(), "{}: no dot {:?}", name, needle);
            ys.into_iter().fold(f64::INFINITY, f64::min)
        };
        let starts = one(format!("s {}y borrows a resource", kind));
        let used = one(format!("the resource s points to is {}y borrowed by {}", kind, user));
        let ends = one(format!("s's {}e borrow ends", kind));
        let lent = one(format!("opt's resource is {}y borrowed by s", kind));
        let back = one(format!("opt's resource is no longer {}y borrowed", kind));
        assert!(starts < ends && used <= ends,
            "{}: s's borrow starts at y={}, is used at y={}, ends at y={}", name, starts, used, ends);
        assert!(lent < back && back == ends,
            "{}: opt is lent at y={}, gets it back at y={} (s's end: y={})", name, lent, back, ends);
    }
}

/// A thick (mutable) branch column is drawn as butt-capped segments, so
/// every bend would show a wedge-shaped notch at its outer corner. Each
/// bend gets a zero-length round-capped line that closes it (#487).
#[test]
fn mutable_branch_bends_have_round_joints() {
    let t = run_with_local_backend("mut_branch_kinks").timeline_panel_string();
    let dots = start_tags(&t, "line")
        .into_iter()
        .filter(|l| attr(l, "class").map_or(false, |c| c.starts_with("solid")))
        .filter(|l| attr(l, "x1") == attr(l, "x2") && attr(l, "y1") == attr(l, "y2"))
        .count();
    // if/else: 2 arms x (split + join); match: 2 bent arms x (split +
    // join) (the middle arm is straight).
    assert!(dots == 8, "expected a round joint at each bend, found {}", dots);
    assert!(t.contains("stroke-linecap: round"));
    // The joints are decorative: a bend point can lie on another arm's
    // row, so they must not be hoverable arm elements.
    assert!(start_tags(&t, "line").into_iter()
        .filter(|l| l.contains("stroke-linecap: round"))
        .all(|l| !l.contains("tooltip-trigger") && !l.contains("data-tooltip-text")));
    // The plain fixture has no dot or icon on its solid bends, so the
    // joints are what closes the notch: 2 arms x (split + join).
    let plain = run_with_local_backend("mut_branch_kinks_plain").timeline_panel_string();
    let plain_dots = start_tags(&plain, "line")
        .into_iter()
        .filter(|l| attr(l, "class").map_or(false, |c| c.starts_with("solid")))
        .filter(|l| attr(l, "x1") == attr(l, "x2") && attr(l, "y1") == attr(l, "y2"))
        .count();
    assert!(plain_dots == 4, "expected 4 round joints, found {}", plain_dots);
    // Immutable (double-line) columns are untouched.
    let imm = run_with_local_backend("if_no_else").timeline_panel_string();
    assert!(!imm.contains("stroke-linecap"));
}

/// #519: the dot of `t = (w, 2)` (and of `t.0 = s`) says the new resource
/// is acquired and the old one dropped, and nothing else. It doesn't also
/// list "t acquires ownership of a resource" as a bullet. (The initial
/// `let mut t = ...` dot does say that, so this can't be a
/// `must_not_contain`: it checks the shape of the reassignment dots.)
#[test]
fn tuple_reassignment_dots_have_one_line() {
    let rv = run_with_local_backend("tuple_field_write");
    let timeline = rv.timeline_panel_string();
    let dots: Vec<Vec<String>> = tooltip_lines(&timeline).into_iter()
        .filter(|l| l.iter().any(|t| t.contains("its previous resource is dropped")))
        .collect();
    let mut got: Vec<String> = dots.iter().map(|l| l.join(" | ")).collect();
    got.sort();
    assert_eq!(got, vec![
        "t acquires ownership of a new resource; its previous resource is dropped".to_owned(),
        "t.0 acquires ownership of a new resource; its previous resource is dropped".to_owned(),
        "t.0 acquires ownership of a new resource; its previous resource is dropped".to_owned(),
    ]);
}

/// (cx, cy) of the topmost timeline dot whose tooltip starts with `prefix`.
fn first_dot(timeline: &str, prefix: &str) -> (f64, f64) {
    timeline
        .split("<circle ")
        .skip(1)
        .map(|c| &c[..c.find("/>").unwrap_or(c.len())])
        .filter(|c| timeline_tooltips(c).iter().any(|t| t.starts_with(prefix)))
        .map(|c| (num_attr(c, "cx"), num_attr(c, "cy")))
        .fold(None, |m: Option<(f64, f64)>, d| match m {
            Some(m) if m.1 <= d.1 => Some(m),
            _ => Some(d),
        })
        .unwrap_or_else(|| panic!("no dot {:?}", prefix))
}

/// cy of the topmost timeline dot whose tooltip starts with `prefix`.
fn first_dot_y(timeline: &str, prefix: &str) -> f64 {
    first_dot(timeline, prefix).1
}

/// A match arm binding that reborrows through a reference (`Some(s)`
/// on `o: &Option<String>`) keeps the reference's borrow alive (#372).
/// The reference's "borrow ends" dot must not be above the row where
/// `s` gives its borrow back to it; it used to be on the `match` line.
#[test]
fn ref_borrow_outlasts_pattern_reborrow() {
    for (name, outer, kind) in [
        ("match_ref_param_reborrow", "o", "immutabl"),
        ("match_mut_ref_param_reborrow", "o", "mutabl"),
        ("match_local_ref_reborrow", "r", "immutabl"),
    ] {
        let rv = run_with_local_backend(name);
        let tl = rv.timeline_panel_string();
        let lent = first_dot_y(&tl, &format!("the resource {} points to is {}y borrowed by s", outer, kind));
        let back = first_dot_y(&tl, &format!("s no longer borrows the resource {} points to", outer));
        let ends = first_dot_y(&tl, &format!("{}'s {}e borrow ends", outer, kind));
        assert!(lent < back && back <= ends,
            "{}: {} lends to s at y={}, gets it back at y={}, its own borrow ends at y={}",
            name, outer, lent, back, ends);
        // And it is drawn on the arm where it happens (the column `s`
        // gives its borrow back in), not on the reference's own column
        // in the middle of the match diamond, where the return arrow
        // from `s` would cross it.
        let (ends_x, _) = first_dot(&tl, &format!("{}'s {}e borrow ends", outer, kind));
        let (back_x, _) = first_dot(&tl, &format!("s no longer borrows the resource {} points to", outer));
        let (split_x, _) = first_dot(&tl, &format!("{} is live in a conditional expression", outer));
        assert!(ends_x == back_x && ends_x != split_x,
            "{}: {}'s borrow-end dot is at x={}; the arm's column is x={}, the match diamond's x={}",
            name, outer, ends_x, back_x, split_x);
    }
}

/// #655: `o.inner = other;` is a partial write, so s's old borrow ends
/// at o's last use (the println!), as for `o.inner = mk(&t);` (#638)
/// and `o.inner.r = &t;` (#531); the loan of t that other.r held moves
/// to o.inner.r and ends at its last use too. Same for a `&mut` field
/// (u, v), a field of another struct (a, b) and a Copy struct (e, f).
#[test]
fn struct_field_overwrite_by_variable_moves_the_loan() {
    let rv = run_with_local_backend("borrow_struct_field_overwrite_var");
    let (svg, code) = (rv.timeline_panel_string(), rv.code_panel_string());
    for (tip, line) in [
        ("s's resource is no longer immutably borrowed", "println!(\"{} {}\", o.inner.r, o.n);"),
        ("t's resource is no longer immutably borrowed", "println!(\"{} {}\", o.inner.r, o.n);"),
        ("u's resource is no longer mutably borrowed", "println!(\"{}\", p.n);"),
        ("v's resource is no longer mutably borrowed", "println!(\"{}\", p.n);"),
        ("a's resource is no longer immutably borrowed", "println!(\"{} {}\", o.inner.r, o2.n);"),
        ("b's resource is no longer immutably borrowed", "println!(\"{} {}\", o.inner.r, o2.n);"),
        ("e's resource is no longer immutably borrowed", "println!(\"{} {}\", q.inner.r, c.r);"),
    ] {
        let dots = dots_listing_tooltip(&svg, tip);
        assert_eq!(dots.len(), 1, "{}: {:?}", tip, dots);
        assert_eq!(dots[0].1, code_row_y(&code, line), "`{}` should be on the row of `{}`: {:?}", tip, line, dots);
    }
}

/// #658: `ref mut s` out of the temporary tuple `(p, q)` ends on s's
/// column with a plain end dot, as `ref a` does in the match above it.
/// The tuple has no column, so the end used to be drawn as a short arrow
/// pointing at nothing. The dot's tooltip still says the borrow ends.
#[test]
fn ref_mut_out_of_temporary_tuple_ends_without_arrow() {
    let rv = run_with_local_backend("tuple_scrutinee_ref");
    let timeline = rv.timeline_panel_string();
    // Arrows are `tooltip-trigger` groups holding a gray shaft.
    let mut arrows = Vec::new();
    for (i, _) in timeline.match_indices("<g class=\"tooltip-trigger\" data-tooltip-text=\"") {
        let group = &timeline[i..i + timeline[i..].find("</g>").unwrap_or(timeline.len() - i)];
        if group.contains("stroke=\"gray\"") {
            arrows.extend(timeline_tooltips(&group[..group.find('>').unwrap_or(group.len()) + 1]));
        }
    }
    let ends: Vec<&String> = arrows.iter().filter(|t| t.contains("borrow ends")).collect();
    assert!(ends.is_empty(), "borrow-end arrow(s) drawn to the temporary tuple: {:?}", ends);
    assert!(
        timeline_tooltips(&timeline).iter().any(|t| t.contains("s's mutable borrow ends")),
        "the end dot lost its text"
    );
}

/// #578: on `if let Some(w) = wrap(s) && w.len() > 1` the dot where `w`
/// is initialized by `wrap` also carries the icon of the `w.len()` call.
/// The `f` glyph reaches about 6px right of the dot's centre, and the
/// arrow coming in from `wrap` used to stop at the dot's own edge, 5.25px
/// out, so its head's tip sat on the glyph. It now stops outside the icon.
#[test]
fn arrowhead_stops_before_a_call_icon_on_its_dot() {
    let tl = run_with_local_backend("edition2024/if_let_chain_first_part").timeline_panel_string();
    let arrows = arrow_points(&tl, "Move from wrap to w");
    assert_eq!(arrows.len(), 1, "{:?}", arrows);
    // The shaft is written source first and ends 12.75px short of the
    // head's tip; the arrow runs left, into `w`.
    let &(end_x, y) = arrows[0].last().unwrap();
    let tip_x = end_x - 12.75;
    let icons: Vec<f64> = start_tags(&tl, "use")
        .into_iter()
        .filter(|t| attr(t, "xlink:href") == Some("#functionDot") && num_attr(t, "y") == y)
        .map(|t| num_attr(t, "x"))
        .collect();
    assert_eq!(icons.len(), 1, "one call icon on w's row: {:?}", icons);
    assert!(tip_x - icons[0] >= 7.0,
        "the head's tip is at x={}, {}px from the call icon's centre (the glyph reaches 6px)",
        tip_x, tip_x - icons[0]);
}

/// Level straight gray arrows as (tail x, head tip x, y). A shaft is written
/// source first and stops 12.75px short of the head's tip.
fn level_arrows(svg: &str) -> Vec<(f64, f64, f64)> {
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
        let dir = if pts[2] > pts[0] { 1.0 } else { -1.0 };
        out.push((pts[0], pts[2] + 12.75 * dir, pts[1]));
    }
    out
}

/// Pairs of level arrows that run into each other: one's head lands within
/// 15px of where the next starts, in the same direction on the same row. They
/// read as one long arrow passing through the dot between them (#577).
fn chained_level_arrows(svg: &str) -> Vec<String> {
    let arrows = level_arrows(svg);
    let mut out = Vec::new();
    for (i, &(tail, tip, y)) in arrows.iter().enumerate() {
        for (j, &(tail2, tip2, y2)) in arrows.iter().enumerate() {
            let same_way = (tip - tail) * (tip2 - tail2) > 0.0;
            if i != j && y == y2 && same_way && (tail2 - tip).abs() < 15.0 {
                out.push(format!("an arrow ends at x={} where another starts, both on y={}", tip, y));
            }
        }
    }
    out
}

/// #577: a reborrow's end and its lender's end on one row (`s` gives back
/// to `r`, `r` to `x`, both on `s`'s last use, the `println!`, since #599)
/// used to be drawn level, the second starting where the first ends, so
/// they read as one long arrow through `r`'s dot. The second dips half a
/// row instead.
#[test]
fn chained_return_arrows_are_visibly_separate() {
    for name in [
        "match_local_ref_reborrow",
        "match_ref_param_reborrow",
        "match_mut_ref_param_reborrow",
    ] {
        let tl = run_with_local_backend(name).timeline_panel_string();
        assert_eq!(chained_level_arrows(&tl), Vec::<String>::new(), "{}", name);
    }
    let rv = run_with_local_backend("match_local_ref_reborrow");
    let tl = rv.timeline_panel_string();
    let code = rv.code_panel_string();
    let first = arrow_points(&tl, "Return immutably borrowed resource from s to r");
    let second = arrow_points(&tl, "Return immutably borrowed resource from r to x");
    assert_eq!((first.len(), second.len()), (1, 2), "{:?} {:?}", first, second);
    let y = first[0][0].1;
    // Both returns are on `s`'s last use (#599), so they still chain.
    assert_eq!(y, code_row_y(&code, "println!(\"{}\", s);"), "s's return isn't on its last use: {:?}", first);
    // The arm's return from `r` to `x` (the other one is the `None` arm's, a
    // row lower) starts on `s`'s row and goes below it, not along it.
    let arm = second
        .iter()
        .find(|p| p[0].1 - y < 20.0 && p[0].1 >= y - 1.0)
        .expect("r's return in the arm");
    assert!(arm.len() > 2 && arm.iter().any(|&(_, py)| py > y + 5.0),
        "r's return should leave the row s's return ends on: {:?}", arm);
    // The dip is the only change: half a row at most below the line (not a
    // full row), no row is added to the code panel for it, and a later
    // line's arrow (the `None` arm's return) is still on its own line.
    let lowest = arm.iter().map(|&(_, py)| py).fold(f64::MIN, f64::max);
    assert!(lowest <= y + 15.0 + 0.5, "r's return dips to y={}, more than half a row below y={}", lowest, y);
    let none_arm = second.iter().find(|p| p[0].1 > y + 20.0).expect("r's return in the None arm");
    assert_eq!(none_arm[0].1, code_row_y(&code, "None => {}"), "the None arm's line moved: {:?}", none_arm);
    assert_eq!(unnumbered_code_rows(&code), 0, "a blank row was added for the dip");
}

/// A `ref` / `ref mut` binding in a match arm returns its borrow at its
/// last use, where the borrow checker ends it, not at the end of the arm
/// (#599). The lender is used again later in the same arm (which only
/// compiles because the borrow has ended); the borrow must end on a row
/// above that use, and the lender's "being shared" segment must not
/// reach it.
#[test]
fn match_ref_binding_borrow_ends_at_last_use() {
    // (fixture, lender, who it lends to, the pattern binding,
    // mutability, the lender's later use)
    for (name, lender, borrower, binding, m, later) in [
        ("match_ref_binding_last_use", "t.0", "x", "x", "immutabl",
         "t.0's resource is mutably borrowed by push_str; t.0 can't be used while the borrow is live"),
        ("match_ref_binding_last_use_arms", "o", "s", "s", "immutabl",
         "o's resource is mutably borrowed by take; o can't be used while the borrow is live"),
        ("match_ref_mut_guard_last_use", "o", "s", "s", "mutabl",
         "o's resource is immutably borrowed by println!; o is read-only while the borrow is live"),
        // `x` reborrows through `o` (`o: &(String, String)`).
        ("match_shared_ref_binding_last_use", "t", "o", "x", "immutabl",
         "t.0's resource is mutably borrowed by push_str; t.0 can't be used while the borrow is live"),
    ] {
        let rv = run_with_local_backend(name);
        let tl = rv.timeline_panel_string();
        let y = |text: &str| -> f64 {
            dots_listing_tooltip(&tl, text).into_iter().map(|d| d.1).reduce(f64::min)
                .unwrap_or_else(|| panic!("{}: no dot {:?}", name, text))
        };
        let only = if m == "mutabl" { "can't be used" } else { "is read-only" };
        let lent = y(&format!("{0}'s resource is {1}y borrowed by {2}; {0} {3} while the borrow is live", lender, m, borrower, only));
        let back = y(&format!("{}'s resource is no longer {}y borrowed", lender, m));
        let used = y(later);
        assert!(lent < back && back < used,
            "{}: {} is lent at y={}, gets it back at y={}, is used again at y={}",
            name, lender, lent, back, used);
        // The binding's own column says so on the same row, before it
        // goes out of scope at the arm's end.
        let ends = y(&format!("{}'s {}e borrow ends", binding, m));
        let gone = y(&format!("{} goes out of scope", binding));
        assert!(ends == back && ends < gone,
            "{}: {}'s borrow ends at y={} (the lender gets it back at y={}), it goes out of scope at y={}",
            name, binding, ends, back, gone);
        let lent_state = format!("{}'s resource is being shared", lender);
        let reach = start_tags(&tl, "line")
            .into_iter()
            .filter(|t| timeline_tooltips(t).iter().any(|x| x.starts_with(&lent_state)))
            .map(|t| num_attr(t, "y2"))
            .fold(0.0, f64::max);
        assert!(reach <= back,
            "{}: {}'s \"being shared\" segment runs to y={}, past the borrow's end at y={}",
            name, lender, reach, back);
    }
}

/// The `(temporary)` columns whose drop dot says the temporary is
/// dropped before an `if let`'s `else` block runs (#469, #604): each
/// column's `data-hash` and the drop dot's y.
fn else_path_temporary_drops(timeline: &str) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    for tag in start_tags(timeline, "g") {
        let Some(hash) = attr(tag, "data-hash") else { continue };
        let lines = timeline_tooltips(&format!("{}>", tag));
        if !lines.iter().any(|l| l.starts_with("This temporary") && l.contains("before the `else` block")) {
            continue;
        }
        let at = timeline.find(tag).unwrap() + tag.len();
        let body = &timeline[at..at + timeline[at..].find("</g>").unwrap()];
        let Some(cy) = start_tags(body, "circle").first().and_then(|c| attr(c, "cy")?.parse::<f64>().ok()) else { continue };
        out.push((hash.to_owned(), cy));
    }
    out
}

/// The lowest y that an element of the column `hash` (segment line,
/// hover polygon or dot) reaches.
fn column_bottom(timeline: &str, hash: &str) -> f64 {
    let mut bottom = f64::MIN;
    for kind in ["line", "polygon", "circle"] {
        for tag in start_tags(timeline, kind) {
            if attr(tag, "data-hash") != Some(hash) {
                continue;
            }
            let ys: Vec<f64> = match kind {
                "line" => ["y1", "y2"].iter().filter_map(|k| attr(tag, k)?.parse().ok()).collect(),
                "circle" => attr(tag, "cy").and_then(|v| v.parse().ok()).into_iter().collect(),
                _ => attr(tag, "points").unwrap_or("").split_whitespace()
                    .filter_map(|p| p.split(',').nth(1)?.parse().ok()).collect(),
            };
            for y in ys {
                bottom = bottom.max(y);
            }
        }
    }
    bottom
}

/// #571 follow-up: an `if let`'s scrutinee temporary is dropped before
/// the `else` block runs (edition 2024), so it isn't alive in the else
/// branch. Its column used to run on from that drop into the else
/// branch and down to a join dot ("(temporary) isn't moved in any of
/// the branches above"), its segments saying "(temporary) is the owner
/// of the resource" there. Now nothing of the column is drawn below the
/// drop, and it gets no join.
#[test]
fn if_let_scrutinee_temporary_ends_at_its_else_drop() {
    for name in ["if_let_scrutinee_move", "temporary_drop_early_exit"] {
        let rv = run_with_local_backend(name);
        let timeline = rv.timeline_panel_string();
        let drops = else_path_temporary_drops(&timeline);
        assert!(!drops.is_empty(), "{}: no drop dot before an `else` block", name);
        for (hash, y) in drops {
            let bottom = column_bottom(&timeline, &hash);
            assert!(bottom <= y + 6.0,
                "{}: the temporary column {} runs to y={}, below its drop before the `else` block at y={}",
                name, hash, bottom, y);
            let joins: Vec<String> = start_tags(&timeline, "circle").into_iter()
                .filter(|t| attr(t, "data-hash") == Some(hash.as_str()))
                .flat_map(|t| timeline_tooltips(&format!("{}>", t)))
                .filter(|t| t.contains("branches above"))
                .collect();
            assert!(joins.is_empty(), "{}: the temporary column {} has a join dot: {:?}", name, hash, joins);
        }
    }
}

/// Same-named variables in separate scopes that are different kinds of
/// value (here a reference and a `String`) get a column each (#598), both
/// labelled with the plain name: the later one shadows nothing, so it
/// has no subscript (#308). Told apart by their hash. Same-kind ones
/// (an `i32` and a `u8`) still share one.
#[test]
fn same_name_kinds_get_separate_columns() {
    for (name, label, columns) in [
        ("ref_binding_name_reused", "x", 2),
        ("ref_binding_name_reused_mut", "x", 2),
        ("let_ref_then_owner_same_name", "x", 2),
        ("for_over_iterator_var", "s", 2),
        ("same_name_copy_kinds_share", "x", 1),
        ("same_name_drop_kinds_split", "x", 2),
        ("same_name_plain_owners_share", "x", 1),
        ("match_macro_arm_name_reused", "x", 1),
    ] {
        let t = run_with_local_backend(name).timeline_panel_string();
        let mut hashes: Vec<&str> = t.split("<text ").skip(1)
            .filter_map(|x| x.split_once('>'))
            .filter(|(tag, rest)| tag.contains("class=\"label") && rest.split('<').next() == Some(label))
            .map(|(tag, _)| attr(tag, "data-hash").expect("a label has a data-hash"))
            .collect();
        hashes.sort();
        hashes.dedup();
        assert_eq!(hashes.len(), columns, "{}: columns labelled {:?}: {:?}", name, label, hashes);
        let labels = header_labels(&t);
        assert!(!labels.iter().any(|l| l.2.starts_with(label) && l.2[label.len()..].starts_with(|c: char| "₀₁₂₃₄₅₆₇₈₉".contains(c))),
            "{}: a subscripted {:?} column: {:?}", name, label, labels);
    }
}
