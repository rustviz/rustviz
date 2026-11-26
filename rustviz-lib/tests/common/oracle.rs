//! rustc as ground truth for what a timeline says about each variable
//! (`RV_ORACLE=1`, test `corpus_oracle_holds` in `corpus.rs`).
//!
//! The invariant checks (`invariants.rs`) look for pictures that
//! contradict themselves. This one asks the borrow checker. Between two
//! source lines a column is drawn in one state: holding its value and
//! free to use (solid or hollow), immutably borrowed (hollow: read
//! only), mutably borrowed (dotted: can't be used), or moved / not
//! assigned yet (no line, or dashed). Each of those is a claim rustc can
//! check: insert a statement that uses the variable right after the line
//! and compile.
//!
//! | drawn after line L                 | probe after L     | rustc must          | check name          |
//! |------------------------------------|-------------------|---------------------|---------------------|
//! | holds its value, not borrowed      | `let _ = &mut x;` | accept it           | `oracle-usable`     |
//! | immutably borrowed (read only)     | `let _ = &x;`     | accept it           | `oracle-readable`   |
//! | immutably borrowed (read only)     | `let _ = &mut x;` | reject: E0502/E0505 | `oracle-borrowed`   |
//! | mutably borrowed (can't be used)   | `let _ = &x;`     | reject: E0502/E0503 | `oracle-borrowed`   |
//! | moved, or no value yet             | `let _ = &x;`     | reject: E0382/E0381 | `oracle-moved`      |
//!
//! "Accept" ignores E0596 (`&mut` of a variable not declared `mut`):
//! rustc still reports a borrow conflict next to it, so a probe of a
//! non-`mut` variable tests the borrow all the same.
//!
//! **Where a probe goes.** Only after a statement that ends on line L,
//! where the next statement (or the block's `}`) starts on a later line,
//! found with `syn` (statement spans, not text guessing). The probe is a
//! new statement in the same block, so it changes no control flow and
//! takes nothing: `let _ = &mut x;` borrows for that statement only.
//! Never inside a closure (a use there would change what the closure
//! captures), a macro's arguments, or a block's tail expression. The
//! drawn state is read in the gap between rows L and L+1 of the
//! timeline; if the plugin added rows there (stacked arrows, a one-line
//! conditional), the point is skipped.
//!
//! **What is skipped** (counted, never reported): points inside closures
//! aren't collected at all; of the rest, a column whose name
//! isn't a variable or field path (`*r`, `v[0]`, `String::from`); a name
//! bound more than once in the function (shadowing: the probe could name
//! the other binding); a gap where only another arm's dashed line runs,
//! or the column is drawn in two states at once; a program with
//! `// rustviz: hide` / `skip` (its gutter or events differ from the
//! source) or a `macro_rules!` (a macro can bind names syn can't see);
//! a `&mut` probe of a place behind a `&` reference; and a probe rustc can't type-check (the variable is out of
//! scope there). A claim inside a loop is checked like any other (the
//! probe is still one point of the program) and its message says
//! "in a loop".
//!
//! Probes are compiled in two batches per program (moves apart: rustc
//! reports one use of a moved value per move, so a missing E0382 is
//! re-checked with that probe alone) as an edition-2024 lib with the
//! pinned nightly (`rust-toolchain.toml`), like `tools/progen`. Every
//! compile is cached by the hash of its source and toolchain under
//! `$RV_ORACLE_CACHE` (default `<target>/tmp/rv-oracle-cache`).
//!
//! `KNOWN_ORACLE_FAILURES` lists what main gets wrong today, each with
//! its issue, like `KNOWN_INVARIANT_FAILURES`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

use syn::spanned::Spanned;
use syn::visit::{self, Visit};

/// Violations main has today: (snippet, check, line, column, issue), one
/// per drawn claim (the line the probe follows, the column's place name).
pub const KNOWN_ORACLE_FAILURES: &[(&str, &str, u32, &str, u32)] = &[
    // #1671: A field is borrowed; the struct is drawn a free owner.
    ("borrow_stored_through_local_ref", "oracle-usable", 27, "o", 1671),
    ("closure_disjoint_field_capture", "oracle-usable", 12, "q", 1671),
    ("closure_tuple_element_capture", "oracle-usable", 10, "u", 1671),
    ("copy_part_borrow_value_1233", "oracle-usable", 13, "p", 1671),
    ("copy_part_borrow_value_1233", "oracle-usable", 15, "p", 1671),
    ("field_borrow_through_ref_1073", "oracle-usable", 55, "s", 1671),
    ("field_borrow_through_ref_1073", "oracle-usable", 56, "s", 1671),
    ("field_through_ref", "oracle-usable", 9, "r", 1671),
    ("field_write_through_local_ref", "oracle-usable", 24, "s", 1671),
    ("for_iter_method_borrow_1347", "oracle-usable", 15, "q", 1671),
    ("get_mut_write_861", "oracle-usable", 26, "b", 1671),
    ("let_ref_binding_borrows", "oracle-usable", 10, "p", 1671),
    ("let_ref_binding_borrows", "oracle-usable", 11, "p", 1671),
    ("let_ref_binding_borrows", "oracle-usable", 12, "p", 1671),
    ("let_ref_binding_borrows", "oracle-usable", 13, "p", 1671),
    ("let_ref_binding_borrows", "oracle-usable", 14, "p", 1671),
    ("nested_struct_borrow", "oracle-usable", 9, "r", 1671),
    ("nested_struct_borrow", "oracle-usable", 9, "r.a", 1671),
    ("ref_write_through_local_mut_ref_1361", "oracle-usable", 48, "o", 1671),
    ("struct_last_move_in_arms_1666", "oracle-usable", 26, "p", 1671),
    ("whole_struct_borrow_fields_1130_1193", "oracle-usable", 39, "o", 1671),
    ("whole_struct_borrow_fields_1130_1193", "oracle-usable", 40, "o", 1671),
    ("whole_struct_borrow_fields_1130_1193", "oracle-usable", 41, "o", 1671),
    // #1672: Whole struct borrowed, written through the reference in an arm: fields drawn free.
    ("field_write_through_ref_column", "oracle-usable", 26, "s.inner", 1672),
    ("field_write_through_ref_column", "oracle-usable", 26, "s.inner.n", 1672),
    ("field_write_through_ref_column", "oracle-usable", 26, "s.inner.v", 1672),
    ("field_write_through_ref_column", "oracle-usable", 26, "s.n", 1672),
    // #1673: A Box's contents moved out: "b is the owner of the resource".
    ("box_deref_write_1345", "oracle-usable", 16, "b2", 1673),
    ("box_deref_write_1345", "oracle-usable", 17, "b2", 1673),
    ("box_field_move", "oracle-usable", 5, "b", 1673),
    ("box_field_move_all", "oracle-usable", 5, "b", 1673),
    ("box_field_move_branch", "oracle-usable", 7, "b", 1673),
    ("box_field_move_param", "oracle-usable", 4, "b", 1673),
    ("box_partial_move_890", "oracle-usable", 22, "b2", 1673),
    ("box_partial_move_890", "oracle-usable", 25, "b2", 1673),
    ("box_partial_move_890", "oracle-usable", 26, "b2", 1673),
    ("box_partial_move_890", "oracle-usable", 26, "bp", 1673),
    ("box_partial_move_890", "oracle-usable", 29, "b2", 1673),
    ("box_partial_move_890", "oracle-usable", 29, "bp", 1673),
    ("box_partial_move_890", "oracle-usable", 30, "b2", 1673),
    ("box_partial_move_890", "oracle-usable", 30, "bp", 1673),
    ("box_partial_move_890", "oracle-usable", 30, "bq", 1673),
    ("box_partial_move_890", "oracle-usable", 33, "b2", 1673),
    ("box_partial_move_890", "oracle-usable", 33, "bp", 1673),
    ("box_partial_move_890", "oracle-usable", 33, "bq", 1673),
    ("box_partial_move_890", "oracle-usable", 34, "b2", 1673),
    ("box_partial_move_890", "oracle-usable", 34, "b4", 1673),
    ("box_partial_move_890", "oracle-usable", 34, "bp", 1673),
    ("box_partial_move_890", "oracle-usable", 34, "bq", 1673),
    ("box_partial_move_890", "oracle-usable", 37, "b2", 1673),
    ("box_partial_move_890", "oracle-usable", 37, "b4", 1673),
    ("box_partial_move_890", "oracle-usable", 37, "bp", 1673),
    ("box_partial_move_890", "oracle-usable", 37, "bq", 1673),
    ("box_partial_move_890", "oracle-usable", 38, "b2", 1673),
    ("box_partial_move_890", "oracle-usable", 38, "b4", 1673),
    ("box_partial_move_890", "oracle-usable", 38, "bo", 1673),
    ("box_partial_move_890", "oracle-usable", 38, "bp", 1673),
    ("box_partial_move_890", "oracle-usable", 38, "bq", 1673),
    ("box_partial_move_890", "oracle-usable", 39, "b2", 1673),
    ("box_partial_move_890", "oracle-usable", 39, "b4", 1673),
    ("box_partial_move_890", "oracle-usable", 39, "bo", 1673),
    ("box_partial_move_890", "oracle-usable", 39, "bp", 1673),
    ("box_partial_move_890", "oracle-usable", 39, "bq", 1673),
    ("box_partial_move_match_892", "oracle-usable", 14, "b7", 1673),
    ("box_partial_move_match_892", "oracle-usable", 17, "b7", 1673),
    ("box_partial_move_match_892", "oracle-usable", 22, "b2", 1673),
    ("box_partial_move_match_892", "oracle-usable", 22, "b7", 1673),
    ("box_partial_move_match_892", "oracle-usable", 28, "b2", 1673),
    ("box_partial_move_match_892", "oracle-usable", 28, "b3", 1673),
    ("box_partial_move_match_892", "oracle-usable", 28, "b7", 1673),
    ("box_partial_move_match_892", "oracle-usable", 33, "b2", 1673),
    ("box_partial_move_match_892", "oracle-usable", 33, "b3", 1673),
    ("box_partial_move_match_892", "oracle-usable", 33, "b7", 1673),
    ("box_partial_move_match_892", "oracle-usable", 33, "bp", 1673),
    ("box_partial_move_match_892", "oracle-usable", 35, "b2", 1673),
    ("box_partial_move_match_892", "oracle-usable", 35, "b3", 1673),
    ("box_partial_move_match_892", "oracle-usable", 35, "b7", 1673),
    ("box_partial_move_match_892", "oracle-usable", 35, "bp", 1673),
    ("box_partial_move_match_892", "oracle-usable", 35, "bq", 1673),
    ("box_payload_reassign_893", "oracle-usable", 17, "bm", 1673),
    ("box_payload_reassign_893", "oracle-usable", 22, "b2", 1673),
    ("box_payload_reassign_893", "oracle-usable", 23, "b2", 1673),
    ("box_payload_reassign_893", "oracle-usable", 28, "b3", 1673),
    ("box_payload_reassign_893", "oracle-usable", 33, "bp", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 16, "b1", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 23, "b1", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 23, "b2", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 24, "b1", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 24, "b2", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 26, "b1", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 26, "b2", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 27, "b1", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 27, "b2", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 29, "b1", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 29, "b2", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 29, "b3", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 30, "b1", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 30, "b2", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 33, "b1", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 33, "b2", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 33, "b3", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 35, "b1", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 35, "b2", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 35, "b3", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 36, "b1", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 36, "b2", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 36, "b3", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 38, "b1", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 38, "b2", 1673),
    ("box_payload_whole_move_898", "oracle-usable", 38, "b3", 1673),
    ("edition2024/box_partial_move_chain_892", "oracle-usable", 6, "b5", 1673),
    ("let_else_exit_drops_906", "oracle-usable", 37, "b", 1673),
    ("let_else_nested_part_899", "oracle-usable", 27, "b", 1673),
    ("tuple_scrutinee_var_field_977", "oracle-usable", 14, "y", 1673),
    ("tuple_scrutinee_var_field_977", "oracle-usable", 16, "y", 1673),
    ("tuple_scrutinee_var_field_977", "oracle-usable", 27, "q", 1673),
    ("tuple_scrutinee_var_field_977", "oracle-usable", 32, "b", 1673),
    // #1674: Partial moves in arms that #1646 doesn't cover (or-patterns, destructuring assignment, loops, tuple scrutinees).
    ("destructure_assign_parts_1412", "oracle-usable", 59, "r", 1674),
    ("edition2024/loop_always_breaks_1467", "oracle-usable", 186, "p", 1674),
    ("edition2024/loop_always_breaks_1467", "oracle-usable", 191, "p", 1674),
    ("edition2024/loop_always_breaks_1467", "oracle-usable", 192, "p", 1674),
    ("loop_drop_flags_1026", "oracle-usable", 68, "x", 1674),
    ("loop_struct_line_1031", "oracle-usable", 91, "x", 1674),
    ("loop_struct_line_1031", "oracle-usable", 110, "y", 1674),
    ("loop_struct_line_1031", "oracle-usable", 111, "y", 1674),
    ("or_pattern_if_let_different_places", "oracle-usable", 4, "u", 1674),
    ("or_pattern_if_let_different_values_else", "oracle-usable", 4, "t", 1674),
    ("or_pattern_if_let_struct_different_places", "oracle-usable", 5, "t", 1674),
    ("or_pattern_let_shadowed_name", "oracle-usable", 6, "u", 1674),
    ("or_pattern_nested_if_let", "oracle-usable", 4, "t", 1674),
    ("or_pattern_nested_if_let", "oracle-usable", 4, "t.1", 1674),
    // #1675: A reference re-pointed in an arm: the old lender drawn borrowed on that path.
    ("basic_if7", "oracle-borrowed", 7, "c", 1675),
    ("borrow_end_join_one_branch", "oracle-borrowed", 10, "x", 1675),
    ("branch_mut_lend_move_1132", "oracle-borrowed", 9, "other", 1675),
    ("branch_mut_lend_move_1132", "oracle-borrowed", 33, "other", 1675),
    ("dead_loan_write_796", "oracle-borrowed", 30, "a", 1675),
    ("dead_loan_write_796", "oracle-borrowed", 74, "s", 1675),
    ("dead_loan_write_796", "oracle-borrowed", 74, "v", 1675),
    ("dead_loan_write_796", "oracle-borrowed", 75, "s", 1675),
    ("dead_loan_write_796", "oracle-borrowed", 75, "v", 1675),
    ("dead_loan_write_796", "oracle-borrowed", 76, "s", 1675),
    ("dead_loan_write_796", "oracle-borrowed", 77, "s", 1675),
    ("dead_loan_write_near_796", "oracle-borrowed", 26, "a", 1675),
    ("else_if_join_1294", "oracle-borrowed", 19, "x", 1675),
    ("else_if_join_1294", "oracle-borrowed", 21, "x", 1675),
    ("else_if_join_1294", "oracle-borrowed", 37, "t", 1675),
    ("else_if_join_1294", "oracle-borrowed", 39, "s", 1675),
    ("known_bugs/1132_mut_borrow_vs_drop_arms", "oracle-borrowed", 8, "other", 1675),
    ("mut_lend_move_join_1297", "oracle-borrowed", 14, "other", 1675),
    ("mut_lend_move_join_1297", "oracle-borrowed", 26, "other", 1675),
    ("mut_lend_move_join_1297", "oracle-borrowed", 51, "other", 1675),
    ("ref_reassigned_other_ref_dead_1439", "oracle-borrowed", 121, "s9", 1675),
    ("repoint_chain_one_return_1479", "oracle-borrowed", 13, "x", 1675),
    ("repoint_chain_one_return_1479", "oracle-borrowed", 15, "x", 1675),
    ("repoint_chain_one_return_1479", "oracle-borrowed", 30, "x", 1675),
    ("repoint_chain_one_return_1479", "oracle-borrowed", 32, "x", 1675),
    ("repoint_chain_one_return_1479", "oracle-borrowed", 49, "x", 1675),
    ("repoint_chain_one_return_1479", "oracle-borrowed", 64, "x", 1675),
    ("repoint_chain_one_return_1479", "oracle-borrowed", 66, "x", 1675),
    ("repoint_ref_cond_call_loan_1557", "oracle-borrowed", 45, "a3", 1675),
    // #1676: A holder overwritten before it is read: its first loan drawn live to the overwrite.
    ("borrow_struct_moved_holder", "oracle-borrowed", 29, "a", 1676),
    ("borrow_struct_moved_holder", "oracle-borrowed", 30, "a", 1676),
    ("borrow_struct_moved_over", "oracle-borrowed", 13, "a", 1676),
    ("borrow_struct_moved_over", "oracle-borrowed", 14, "a", 1676),
    ("borrow_struct_overwrite_call", "oracle-borrowed", 21, "s", 1676),
    ("borrow_struct_overwrite_call", "oracle-borrowed", 33, "a", 1676),
    ("borrow_struct_overwrite_call", "oracle-borrowed", 34, "a", 1676),
    ("borrow_struct_overwrite_whole", "oracle-borrowed", 15, "s", 1676),
    ("borrow_struct_overwrite_whole", "oracle-borrowed", 50, "x", 1676),
    ("borrow_struct_overwrite_whole", "oracle-borrowed", 50, "y", 1676),
    ("holder_borrows_1094", "oracle-borrowed", 57, "h", 1676),
    // #1678: A closure's shared capture given back by a later read's borrow end.
    ("book/c13_12_closure_borrow_then_use", "oracle-usable", 4, "list", 1678),
    // #1679: A loan stored into a Vec through iter_mut() isn't drawn.
    ("iter_mut_holder_1399", "oracle-usable", 102, "t1", 1679),
    ("iter_mut_holder_1399", "oracle-usable", 103, "t1", 1679),
    ("iter_mut_holder_1399", "oracle-usable", 104, "t1", 1679),
    ("iter_mut_holder_1399", "oracle-usable", 105, "t1", 1679),
    ("iter_mut_holder_1399", "oracle-usable", 106, "t1", 1679),
    ("iter_mut_holder_1399", "oracle-usable", 107, "t1", 1679),
    // #1681: Two `e` bindings (last_mut / iter_mut): v drawn mutably borrowed after the block.
    ("same_name_bindings_1400", "oracle-borrowed", 59, "v", 1681),
    ("same_name_bindings_1400", "oracle-borrowed", 62, "v", 1681),
    ("same_name_bindings_1400", "oracle-borrowed", 63, "v", 1681),
    ("same_name_bindings_1400", "oracle-borrowed", 63, "w", 1681),
    // #1727: `for item in &mut iter`: iter drawn mutably borrowed past the loop.
    ("try_block_drops_1656", "oracle-borrowed", 45, "iter", 1727),
    ("try_block_drops_1656", "oracle-borrowed", 48, "iter", 1727),
    ("try_block_drops_1656", "oracle-borrowed", 49, "iter", 1727),
    ("try_block_drops_1656", "oracle-borrowed", 51, "iter", 1727),
    // #853: Loops are drawn as one pass: a loan carried into the next iteration isn't shown.
    ("copied_ref_field_end_1568", "oracle-usable", 58, "e", 853),
    ("shared_iter_loop_1081", "oracle-usable", 46, "p", 853),
    // #1595: A struct's line through a conditional says "owner" in the arm after a field moved (PR #1646).
    ("assign_branch_reborrow_holder_1482", "oracle-usable", 129, "w", 1595),
    ("edition2024/if_let_chain_field_move", "oracle-usable", 10, "p", 1595),
    ("edition2024/if_let_chain_field_move", "oracle-usable", 12, "p", 1595),
    ("edition2024/if_let_chain_irrefutable_move", "oracle-usable", 9, "pair", 1595),
    ("edition2024/if_let_chain_irrefutable_move", "oracle-usable", 11, "pair", 1595),
    ("edition2024/struct_inner_move_chain_1711", "oracle-usable", 6, "q", 1595),
    ("edition2024/struct_inner_move_chain_1711", "oracle-usable", 12, "r", 1595),
    ("fields_maybe_all_moved_1583", "oracle-usable", 43, "x", 1595),
    ("fields_maybe_all_moved_1583", "oracle-usable", 54, "y", 1595),
    ("if_no_else_partial_move", "oracle-usable", 12, "p", 1595),
    ("if_no_else_partial_move", "oracle-usable", 13, "p", 1595),
    ("manually_drop_maybe_moved_1147", "oracle-usable", 80, "t", 1595),
    ("manually_drop_maybe_moved_1147", "oracle-usable", 87, "u", 1595),
    ("manually_drop_maybe_moved_1147", "oracle-usable", 94, "v", 1595),
    ("no_drop_partial_maybe", "oracle-usable", 11, "w", 1595),
    ("partial_move_if_branches", "oracle-usable", 19, "p", 1595),
    ("partial_move_if_branches", "oracle-usable", 21, "p", 1595),
    ("partial_move_if_branches", "oracle-usable", 28, "q", 1595),
    ("partial_move_if_branches", "oracle-usable", 38, "r", 1595),
    ("partial_move_if_branches", "oracle-usable", 40, "r", 1595),
    ("partial_move_one_arm_struct", "oracle-usable", 19, "p", 1595),
    ("struct_inner_moves_1711", "oracle-usable", 11, "p", 1595),
    ("struct_inner_moves_1711", "oracle-usable", 12, "p", 1595),
    ("struct_inner_moves_1711", "oracle-usable", 13, "p", 1595),
    ("struct_nested_maybe_moves_1728", "oracle-usable", 17, "p", 1595),
    ("struct_nested_maybe_moves_1728", "oracle-usable", 17, "p.i", 1595),
    ("struct_nested_maybe_moves_1728", "oracle-usable", 18, "p", 1595),
    ("struct_nested_maybe_moves_1728", "oracle-usable", 18, "p.i", 1595),
    ("struct_nested_maybe_moves_1728", "oracle-usable", 19, "p", 1595),
    ("struct_nested_maybe_moves_1728", "oracle-usable", 19, "p.i", 1595),
    ("struct_parts_moved_out_1157", "oracle-usable", 42, "q", 1595),
    ("struct_parts_moved_out_1157", "oracle-usable", 43, "q", 1595),
    ("struct_refilled_partial_drop_1577", "oracle-usable", 25, "p", 1595),
    ("struct_refilled_partial_drop_1577", "oracle-usable", 26, "p", 1595),
    ("struct_refilled_partial_drop_1577", "oracle-usable", 36, "p", 1595),
    ("struct_refilled_partial_drop_1577", "oracle-usable", 37, "p", 1595),
    ("try_partial_move_1184", "oracle-usable", 31, "r", 1595),
];


/// Where the drawing deliberately follows what runs rather than rustc's
/// borrow checker, which is conservative: (snippet, check, line, column, why). The
/// test ignores these, and fails if one no longer happens.
pub const ORACLE_EXEMPT: &[(&str, &str, u32, &str, &str)] = &[
    // A pattern that can't fail (`if let x = f`, `Some(x)` on `Some(f)`):
    // its `else` block, and the code after a body that always returns,
    // never run, so the drawing shows nothing there. rustc still checks
    // them as if they could run (#987, #995, #1004, #1013, #949).
    // `ManuallyDrop::take(&mut m)` moves the value out (#283); rustc only
    // sees a `&mut` borrow.
    ("edition2024/dead_else_987", "oracle-moved", 13, "f", "irrefutable pattern: dead else"),
    ("edition2024/dead_else_987", "oracle-moved", 35, "a", "irrefutable pattern: dead else"),
    ("edition2024/dead_else_987", "oracle-moved", 35, "s1", "irrefutable pattern: dead else"),
    ("edition2024/dead_else_987", "oracle-moved", 45, "b", "irrefutable pattern: dead else"),
    ("edition2024/dead_else_987", "oracle-usable", 13, "g", "irrefutable pattern: dead else"),
    ("edition2024/dead_else_987", "oracle-usable", 23, "r", "irrefutable pattern: dead else"),
    ("edition2024/dead_else_987", "oracle-usable", 35, "s2", "irrefutable pattern: dead else"),
    ("edition2024/dead_else_987", "oracle-usable", 36, "s2", "irrefutable pattern: dead else"),
    ("edition2024/dead_else_987", "oracle-usable", 37, "s2", "irrefutable pattern: dead else"),
    ("edition2024/irrefutable_iflet_inline_995", "oracle-moved", 14, "f", "irrefutable pattern: code after never runs"),
    ("edition2024/irrefutable_iflet_inline_995", "oracle-moved", 34, "p", "irrefutable pattern: code after never runs"),
    ("edition2024/irrefutable_iflet_inline_995", "oracle-moved", 35, "p", "irrefutable pattern: code after never runs"),
    ("inline_exit_1004", "oracle-moved", 11, "f", "irrefutable pattern: code after never runs"),
    ("inline_exit_nested_1013", "oracle-moved", 13, "f", "irrefutable pattern: code after never runs"),
    ("inline_exit_nested_1013", "oracle-moved", 65, "s", "irrefutable pattern: code after never runs"),
    ("inline_exit_nested_1013", "oracle-moved", 66, "s", "irrefutable pattern: code after never runs"),
    ("manually_drop_by_hand", "oracle-moved", 14, "m2", "ManuallyDrop::take moves out behind a &mut"),
    ("manually_drop_by_hand", "oracle-moved", 15, "m2", "ManuallyDrop::take moves out behind a &mut"),
    ("manually_drop_by_hand", "oracle-moved", 16, "m2", "ManuallyDrop::take moves out behind a &mut"),
    ("manually_drop_by_hand", "oracle-moved", 17, "m2", "ManuallyDrop::take moves out behind a &mut"),
    ("manually_drop_by_hand", "oracle-moved", 18, "m2", "ManuallyDrop::take moves out behind a &mut"),
];


pub const ORACLE_CHECKS: &[(&str, &str)] = &[
    ("oracle-usable",
     "the column is drawn holding its value with no borrow of it after this line, but rustc reports a \
      borrow conflict, a move or a missing value for `&mut x` there"),
    ("oracle-readable",
     "the column is drawn immutably borrowed (read only) after this line, but rustc reports a borrow \
      conflict, a move or a missing value for `&x` there"),
    ("oracle-borrowed",
     "the column is drawn borrowed after this line (read only, or lent out mutably), but rustc reports no \
      borrow conflict for the use the drawing forbids (`&mut x` for read only, `&x` for lent out mutably)"),
    ("oracle-moved",
     "the column is drawn moved, partially moved or not assigned yet after this line, but rustc reports \
      no move or missing value for `&x` there"),
    ("oracle-error",
     "the probe programs couldn't be built or compiled (an error in the oracle or its setup, not in the \
      drawing); never listed as known"),
];

/// Borrow conflicts: the place is borrowed where the probe uses it.
const CONFLICT: &[&str] = &["E0499", "E0500", "E0501", "E0502", "E0503", "E0505", "E0506", "E0510", "E0524"];
/// The place has no value (moved, or not assigned yet).
const NO_VALUE: &[&str] = &["E0381", "E0382"];
/// Mutability of the binding, not a borrow: says nothing either way.
const IGNORED: &[&str] = &["E0596", "E0594"];
/// Recorded for a conflict whose loan is used later only by a probe.
const SELF_CONFLICT: &str = "self-conflict";
/// Recorded for a `&mut` probe rustc refuses for another reason than the
/// binding's mutability (behind a `&` reference, inside an `Rc`).
const BEHIND_REF: &str = "E0596-behind-ref";

/// `// rustviz: skip` / `// rustviz: hide` (the colon's spacing is free),
/// as the plugin reads them (`rustviz-plugin/src/utils.rs`).
fn has_marker(src: &str) -> bool {
    src.lines().any(|line| {
        let Some(at) = line.find("//") else { return false };
        let Some(rest) = line[at + 2..].trim_start().strip_prefix("rustviz") else { return false };
        let Some(rest) = rest.trim_start().strip_prefix(':') else { return false };
        let rest = rest.trim_start();
        ["skip", "hide"].iter().any(|kw| {
            rest.strip_prefix(kw).is_some_and(|a| a.is_empty() || a.starts_with(char::is_whitespace))
        })
    })
}

// ─── Statement boundaries (syn) ─────────────────────────────────────

#[derive(Debug, Clone)]
struct Point {
    /// The last line of the statement the probe follows.
    line: u32,
    func: usize,
    in_loop: bool,
    /// The blocks the statement is in (indices into `Points::blocks`),
    /// outermost first.
    ancestors: Vec<usize>,
}

#[derive(Debug, Clone)]
struct BlockSpan {
    open: u32,
    close: u32,
    func: usize,
    /// (first, last) line of each statement.
    stmts: Vec<(u32, u32)>,
}

impl BlockSpan {
    /// Whether line `l`'s events belong inside this block. Not on the
    /// `{` line: that line may hold the arm before it (`} else {`,
    /// `true => drop(x), false => {`) or the pattern that leads in.
    fn holds(&self, l: u32) -> bool {
        l > self.open && l <= self.close
    }
}

#[derive(Default)]
struct Points {
    points: Vec<Point>,
    /// Per function: binding name -> how many patterns bind it.
    bindings: Vec<HashMap<String, u32>>,
    /// Per function: (first line, last line, the line after which a
    /// declaration starts the body, if its first statement starts on a
    /// later line than the `{`).
    spans: Vec<(u32, u32, Option<u32>)>,
    /// The function (index) each item level is in; None in a const,
    /// static or other non-fn item.
    stack: Vec<Option<usize>>,
    loops: u32,
    closures: u32,
    /// The source's lines.
    src: Vec<String>,
    blocks: Vec<BlockSpan>,
    block_stack: Vec<usize>,
    /// Per function: inserting lines could change its meaning or land
    /// in a comment (block comments, `line!()` / `column!()` /
    /// `file!()`, a name like the sentinels').
    unprobeable: Vec<bool>,
    /// Inside a call's or method call's arguments or receiver, an index,
    /// a compound assignment's right-hand side (a two-phase `&mut` may be
    /// reserved there), or a const context (array length, const generic
    /// argument): no points.
    no_points: u32,
    /// Every item name in the file (fns, consts, statics, types, enum
    /// variants, `use` names).
    items: HashSet<String>,
}

fn line_of(s: proc_macro2::Span) -> (u32, u32) {
    (s.start().line as u32, s.end().line as u32)
}

impl Points {
    fn func(&self) -> Option<usize> {
        self.stack.last().copied().flatten()
    }
    fn enter_fn(&mut self, sig: proc_macro2::Span, body: Option<&syn::Block>) {
        self.bindings.push(HashMap::new());
        let open = body.and_then(|b| {
            let o = b.brace_token.span.open().start().line as u32;
            let first = b.stmts.first().map_or(b.brace_token.span.close().start().line as u32, |s| line_of(s.span()).0);
            // Declarations go before the first statement (after any
            // inner attribute such as `#![allow(..)]`).
            (first > o).then_some(first - 1)
        });
        let end = body.map_or(line_of(sig).1, |b| b.brace_token.span.close().start().line as u32);
        let text: String = self.src.iter().skip(line_of(sig).0 as usize - 1).take((end + 1 - line_of(sig).0) as usize)
            .map(|l| l.as_str()).collect::<Vec<_>>().join("\n");
        self.unprobeable.push(["/*", "*/", "__rv_"].iter().any(|t| text.contains(t)));
        self.spans.push((line_of(sig).0, end, open));
        self.stack.push(Some(self.bindings.len() - 1));
    }
    fn body<F: FnOnce(&mut Self)>(&mut self, f: F) {
        let (loops, closures, no_points) = (self.loops, self.closures, self.no_points);
        self.loops = 0;
        self.closures = 0;
        self.no_points = 0;
        f(self);
        self.loops = loops;
        self.closures = closures;
        self.no_points = no_points;
        self.stack.pop();
    }
}

impl<'ast> Visit<'ast> for Points {
    fn visit_item(&mut self, i: &'ast syn::Item) {
        // Every item name in the file: a probe of `x` could resolve to an
        // item `x` in scope instead of the local.
        let name = match i {
            syn::Item::Fn(f) => Some(&f.sig.ident),
            syn::Item::Const(c) => Some(&c.ident),
            syn::Item::Static(c) => Some(&c.ident),
            syn::Item::Struct(c) => Some(&c.ident),
            syn::Item::Enum(c) => Some(&c.ident),
            syn::Item::Union(c) => Some(&c.ident),
            syn::Item::Type(c) => Some(&c.ident),
            syn::Item::Mod(c) => Some(&c.ident),
            syn::Item::Trait(c) => Some(&c.ident),
            syn::Item::Macro(c) => c.ident.as_ref(),
            syn::Item::ExternCrate(c) => Some(c.rename.as_ref().map_or(&c.ident, |r| &r.1)),
            _ => None,
        };
        if let Some(n) = name {
            self.items.insert(n.to_string().trim_start_matches("r#").to_owned());
        }
        match i {
            syn::Item::Fn(_) | syn::Item::Impl(_) | syn::Item::Trait(_) | syn::Item::Mod(_) => visit::visit_item(self, i),
            _ => {
                self.stack.push(None);
                visit::visit_item(self, i);
                self.stack.pop();
            }
        }
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.enter_fn(f.sig.span(), Some(&f.block));
        self.body(|s| visit::visit_item_fn(s, f));
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.enter_fn(f.sig.span(), Some(&f.block));
        self.body(|s| visit::visit_impl_item_fn(s, f));
    }
    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        self.enter_fn(f.sig.span(), f.default.as_ref());
        self.body(|s| visit::visit_trait_item_fn(s, f));
    }
    fn visit_impl_item_const(&mut self, c: &'ast syn::ImplItemConst) {
        self.stack.push(None);
        visit::visit_impl_item_const(self, c);
        self.stack.pop();
    }
    fn visit_pat_ident(&mut self, p: &'ast syn::PatIdent) {
        if let Some(f) = self.func() {
            // `r#x` and `x` are one name.
            *self.bindings[f].entry(p.ident.to_string().trim_start_matches("r#").to_owned()).or_default() += 1;
        }
        visit::visit_pat_ident(self, p);
    }
    fn visit_variant(&mut self, v: &'ast syn::Variant) {
        self.items.insert(v.ident.to_string().trim_start_matches("r#").to_owned());
        visit::visit_variant(self, v);
    }
    fn visit_use_name(&mut self, u: &'ast syn::UseName) {
        self.items.insert(u.ident.to_string().trim_start_matches("r#").to_owned());
    }
    fn visit_use_rename(&mut self, u: &'ast syn::UseRename) {
        self.items.insert(u.rename.to_string().trim_start_matches("r#").to_owned());
    }
    fn visit_foreign_item_fn(&mut self, f: &'ast syn::ForeignItemFn) {
        self.items.insert(f.sig.ident.to_string());
        visit::visit_foreign_item_fn(self, f);
    }
    fn visit_foreign_item_static(&mut self, f: &'ast syn::ForeignItemStatic) {
        self.items.insert(f.ident.to_string());
        visit::visit_foreign_item_static(self, f);
    }
    fn visit_type_array(&mut self, a: &'ast syn::TypeArray) {
        self.visit_type(&a.elem);
        self.no_points += 1;
        self.visit_expr(&a.len);
        self.no_points -= 1;
    }
    fn visit_receiver(&mut self, r: &'ast syn::Receiver) {
        if let Some(f) = self.func() {
            *self.bindings[f].entry("self".to_owned()).or_default() += 1;
        }
        visit::visit_receiver(self, r);
    }
    fn visit_expr_closure(&mut self, c: &'ast syn::ExprClosure) {
        self.closures += 1;
        visit::visit_expr_closure(self, c);
        self.closures -= 1;
    }
    fn visit_expr_async(&mut self, a: &'ast syn::ExprAsync) {
        self.closures += 1;
        visit::visit_expr_async(self, a);
        self.closures -= 1;
    }
    fn visit_expr_const(&mut self, c: &'ast syn::ExprConst) {
        self.closures += 1;
        visit::visit_expr_const(self, c);
        self.closures -= 1;
    }
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        self.no_points += 1;
        visit::visit_expr_method_call(self, m);
        self.no_points -= 1;
    }
    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        self.no_points += 1;
        visit::visit_expr_call(self, c);
        self.no_points -= 1;
    }
    fn visit_expr_index(&mut self, i: &'ast syn::ExprIndex) {
        self.no_points += 1;
        visit::visit_expr_index(self, i);
        self.no_points -= 1;
    }
    fn visit_expr_binary(&mut self, b: &'ast syn::ExprBinary) {
        use syn::BinOp::*;
        let compound = matches!(b.op, AddAssign(_) | SubAssign(_) | MulAssign(_) | DivAssign(_) | RemAssign(_)
            | BitXorAssign(_) | BitAndAssign(_) | BitOrAssign(_) | ShlAssign(_) | ShrAssign(_));
        self.no_points += compound as u32;
        visit::visit_expr_binary(self, b);
        self.no_points -= compound as u32;
    }
    fn visit_expr_repeat(&mut self, r: &'ast syn::ExprRepeat) {
        self.visit_expr(&r.expr);
        self.no_points += 1;
        self.visit_expr(&r.len);
        self.no_points -= 1;
    }
    fn visit_generic_argument(&mut self, g: &'ast syn::GenericArgument) {
        let c = matches!(g, syn::GenericArgument::Const(_)) as u32;
        self.no_points += c;
        visit::visit_generic_argument(self, g);
        self.no_points -= c;
    }
    fn visit_expr_loop(&mut self, l: &'ast syn::ExprLoop) {
        self.loops += 1;
        visit::visit_expr_loop(self, l);
        self.loops -= 1;
    }
    fn visit_expr_while(&mut self, l: &'ast syn::ExprWhile) {
        self.loops += 1;
        visit::visit_expr_while(self, l);
        self.loops -= 1;
    }
    fn visit_expr_for_loop(&mut self, l: &'ast syn::ExprForLoop) {
        self.loops += 1;
        visit::visit_expr_for_loop(self, l);
        self.loops -= 1;
    }
    fn visit_block(&mut self, b: &'ast syn::Block) {
        let Some(func) = self.func() else {
            visit::visit_block(self, b);
            return;
        };
        let close = b.brace_token.span.close().start().line as u32;
        let open_at = b.brace_token.span.open().start();
        let open = open_at.line as u32;
        let stmts = b.stmts.iter().map(|st| line_of(st.span())).collect();
        self.blocks.push(BlockSpan { open, close, func, stmts });
        self.block_stack.push(self.blocks.len() - 1);
        if self.closures == 0 && self.no_points == 0 {
            let n = b.stmts.len();
            for (i, st) in b.stmts.iter().enumerate() {
                let tail = i + 1 == n && matches!(st, syn::Stmt::Expr(_, None) | syn::Stmt::Macro(syn::StmtMacro { semi_token: None, .. }));
                if tail || matches!(st, syn::Stmt::Item(_)) {
                    continue;
                }
                let (_, end) = line_of(st.span());
                let next = b.stmts.get(i + 1).map_or(close, |s| line_of(s.span()).0);
                // A probe goes on a new line after `end`: not if a block
                // comment opened on that line runs on past it.
                let open_comment = self.src.get(end as usize - 1)
                    .is_some_and(|l| l.rfind("/*").is_some_and(|a| !l[a..].contains("*/")));
                if next > end && !open_comment && !self.unprobeable[func] {
                    self.points.push(Point { line: end, func, in_loop: self.loops > 0, ancestors: self.block_stack.clone() });
                }
            }
        }
        visit::visit_block(self, b);
        self.block_stack.pop();
    }
    /// The alternatives of an or-pattern bind the same names once.
    fn visit_pat_or(&mut self, p: &'ast syn::PatOr) {
        let Some(f) = self.func() else {
            visit::visit_pat_or(self, p);
            return;
        };
        let before = self.bindings[f].clone();
        let mut merged = before.clone();
        for case in &p.cases {
            self.bindings[f] = before.clone();
            self.visit_pat(case);
            for (k, v) in &self.bindings[f] {
                let m = merged.entry(k.clone()).or_default();
                *m = (*m).max(*v);
            }
        }
        self.bindings[f] = merged;
    }
}

impl Points {
    /// Whether line `l`'s events happen on every path to point `p`: they
    /// are in a block `p` is in, and not in the statement of that block
    /// that holds `p`'s nested block (a sibling arm, a pattern on the
    /// `match` / `if let` line).
    fn on_path(&self, p: &Point, l: u32) -> bool {
        let Some(b) = self.block_of(p.func, l) else { return false };
        let Some(at) = p.ancestors.iter().position(|&a| a == b) else { return false };
        match p.ancestors.get(at + 1) {
            None => true,
            Some(&child) => {
                let open = self.blocks[child].open;
                !self.blocks[b].stmts.iter().any(|&(s, e)| s <= open && open <= e && s <= l && l <= e)
            }
        }
    }
    /// The innermost block of function `f` that line `l`'s events are in.
    fn block_of(&self, f: usize, l: u32) -> Option<usize> {
        (0..self.blocks.len())
            .filter(|&i| self.blocks[i].func == f && self.blocks[i].holds(l))
            .min_by_key(|&i| self.blocks[i].close - self.blocks[i].open)
    }
}

// ─── The drawing ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Dot {
    hash: String,
    var: String,
    line: u32,
    x: f64,
    y: f64,
    events: Vec<String>,
    tip: String,
}

#[derive(Debug, Clone)]
struct Seg {
    hash: String,
    var: String,
    state: String,
    /// A conditional arm's leg (`branch_line`), not the column's own line.
    branch: bool,
    tip: String,
    /// (x1, y1, x2, y2) of each drawn line.
    lines: Vec<(f64, f64, f64, f64)>,
}

struct Drawing {
    dots: Vec<Dot>,
    segs: Vec<Seg>,
    /// hash -> column name.
    columns: BTreeMap<String, String>,
    /// hash -> x of the column's header (its line outside conditionals).
    header_x: HashMap<String, f64>,
    /// Columns of references (they have a bracket): their line's style
    /// says what kind of reference they hold, not that they're borrowed.
    refs: HashSet<String>,
    /// Source line -> y of its timeline row.
    row: HashMap<u32, f64>,
}

fn plain(t: &str) -> String {
    let t = t.replace("<br>", " / ");
    let mut out = String::with_capacity(t.len());
    let mut in_tag = false;
    for c in t.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn num(n: roxmltree::Node, a: &str) -> f64 {
    n.attribute(a).and_then(|v| v.parse().ok()).unwrap_or(f64::NAN)
}

fn drawing(timeline: &str, code: &str) -> Result<Drawing, String> {
    let doc = roxmltree::Document::parse(timeline).map_err(|e| e.to_string())?;
    let mut d = Drawing { dots: vec![], segs: vec![], columns: BTreeMap::new(), header_x: HashMap::new(), refs: HashSet::new(), row: HashMap::new() };
    for n in doc.descendants().filter(|n| n.attribute("data-rv-id").is_some()) {
        let a = |k: &str| n.attribute(k).unwrap_or("").to_owned();
        let hash = n.descendants().find_map(|c| c.attribute("data-hash")).unwrap_or("").to_owned();
        match a("data-rv-kind").as_str() {
            "dot" => {
                let Some(c) = n.descendants().find(|c| c.has_tag_name("circle")) else { continue };
                d.dots.push(Dot {
                    hash, var: a("data-rv-var"),
                    line: a("data-rv-line").parse().unwrap_or(0),
                    x: num(c, "cx"),
                    y: num(c, "cy"),
                    events: a("data-rv-event").split(' ').filter(|e| !e.is_empty()).map(str::to_owned).collect(),
                    tip: plain(&a("data-tooltip-text")),
                });
            }
            kind @ ("line" | "branch_line") => {
                let part = n.attribute("data-rv-part");
                if kind == "branch_line" && !matches!(part, Some("line" | "side1" | "side2")) {
                    continue;
                }
                let mut tip = a("data-tooltip-text");
                if tip.is_empty() {
                    tip = n.parent().and_then(|p| p.attribute("data-tooltip-text")).unwrap_or("").to_owned();
                }
                let lines = n.descendants().filter(|c| c.has_tag_name("line"))
                    .map(|l| (num(l, "x1"), num(l, "y1"), num(l, "x2"), num(l, "y2"))).collect();
                d.segs.push(Seg { hash, var: a("data-rv-var"), state: a("data-rv-state"), tip: plain(&tip), lines, branch: kind == "branch_line" });
            }
            "header" => {
                d.header_x.entry(hash.clone()).or_insert(num(n, "x"));
                d.columns.insert(hash, a("data-rv-var"));
            }
            "bracket" => {
                d.refs.insert(hash);
            }
            _ => {}
        }
    }
    // Rows line up with the code panel's lines; a dot sits a fixed
    // offset above its line's text baseline.
    let cdoc = roxmltree::Document::parse(code).map_err(|e| e.to_string())?;
    let mut text_y = HashMap::new();
    for n in cdoc.descendants().filter(|n| n.attribute("data-rv-kind") == Some("code_line")) {
        if let (Some(l), y) = (n.attribute("data-rv-line").and_then(|l| l.parse::<u32>().ok()), num(n, "y")) {
            text_y.insert(l, y);
        }
    }
    let mut offsets: HashMap<i64, u32> = HashMap::new();
    for dot in &d.dots {
        if let Some(y) = text_y.get(&dot.line) {
            *offsets.entry((dot.y - y).round() as i64).or_default() += 1;
        }
    }
    let off = offsets.into_iter().max_by_key(|(o, n)| (*n, -o)).map_or(-5.0, |(o, _)| o as f64);
    d.row = text_y.into_iter().map(|(l, y)| (l, y + off)).collect();
    Ok(d)
}

// ─── Claims ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Drawn {
    Usable,
    Shared,
    MutLent,
    Moved,
    /// A struct with a field moved out ("p is partially moved").
    PartlyMoved,
    Unassigned,
}

#[derive(Debug, Clone)]
struct Claim {
    line: u32,
    func: usize,
    var: String,
    drawn: Drawn,
    in_loop: bool,
    /// The tooltip that says it: the segment's, or the moving dot's.
    tip: String,
}

/// A variable or a field path (`s`, `p.x`, `t.0`), as a probe can name it.
fn place_name(var: &str) -> Option<String> {
    let v: String = var.chars().filter(|c| !('₀'..='₉').contains(c)).collect();
    let ok = !v.is_empty()
        && v.split('.').enumerate().all(|(i, p)| {
            !p.is_empty()
                && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && (i > 0 || !p.starts_with(|c: char| c.is_ascii_digit()))
        });
    ok.then_some(v)
}

/// Where `seg` crosses height `y` (x), if it does.
fn crossing(seg: &Seg, y: f64) -> Option<f64> {
    seg.lines.iter().find_map(|&(x1, y1, x2, y2)| {
        (y1.min(y2) + 0.5 < y && y < y1.max(y2) - 0.5).then(|| x1 + (x2 - x1) * (y - y1) / (y2 - y1))
    })
}

const MOVE_EVENTS: &[&str] = &["move", "closure_call", "loop_exit_move", "or_alt_move"];
const VALUE_EVENTS: &[&str] = &[
    "acquire", "copy", "static_borrow", "mutable_borrow", "init_ref_param", "loop_maybe_init",
    "loop_exit_kept", "or_alt_borrow",
];
/// What a reference's line says (a deferred `let r;` has no bracket).
const HOLDS_REF: &[&str] = &["holds an immutable reference", "holds a mutable reference"];
/// What a struct's line says once a field was moved out of it.
const PARTLY_MOVED: &[&str] = &["is partially moved", "can't be used as a whole"];
const MAYBE_MOVED: &[&str] = &["moved in at least one branch", "may have been moved", "moved out in at least one branch"];

pub type Skips = BTreeMap<&'static str, usize>;

fn claims(pts: &Points, d: &Drawing, skips: &mut Skips) -> Vec<Claim> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for p in &pts.points {
        let (Some(&ya), Some(&yb)) = (d.row.get(&p.line), d.row.get(&(p.line + 1))) else { continue };
        if !seen.insert(p.line) {
            continue;
        }
        let (fn_start, _, _) = pts.spans[p.func];
        // The renderer stacks events on a blank (or comment-only) row
        // below a line: the gap's state isn't the one after line L.
        let next_text = pts.src.get(p.line as usize).map_or("", |l| l.trim());
        if next_text.is_empty() || next_text.starts_with("//") {
            continue;
        }
        let y = (ya + yb) / 2.0;
        let added_rows = d.dots.iter().any(|x| x.y > ya + 1.0 && x.y < yb - 1.0);
        for (hash, var) in &d.columns {
            // The column's last dot so far in this function. When it is
            // in a conditional's arm and in the point's own block, the
            // point is on that arm's path: other arms' legs (an `if`
            // without `else` draws its implicit `else` solid beside the
            // `then` arm) say nothing about it.
            let last = d.dots.iter()
                .filter(|x| &x.hash == hash && x.line >= fn_start && x.line <= p.line && x.y <= ya + 1.0)
                .max_by(|a, b| a.y.total_cmp(&b.y));
            let in_arm = last.is_some_and(|l| d.header_x.get(hash).is_some_and(|hx| (hx - l.x).abs() > 1.0));
            let local = last.is_some_and(|l| pts.on_path(p, l.line));
            // The column is forked into arm legs here. Only a leg the
            // point's own path is known to be on can be checked: the one
            // through the column's last dot, when that dot is in an arm and
            // on the point's path. Otherwise (the arm's first rows, the
            // start of an `else`, a move in a finished inner block) the
            // path isn't known: skip rather than guess.
            let forked = d.segs.iter().any(|s| &s.hash == hash && s.branch && crossing(s, y).is_some());
            let path_known = in_arm && local;
            if forked && !path_known {
                *skips.entry("arm leg: the point's path isn't known").or_default() += 1;
                continue;
            }
            let segs: Vec<&Seg> = d.segs.iter()
                .filter(|s| &s.hash == hash)
                .filter(|s| match (crossing(s, y), last) {
                    (None, _) => false,
                    (Some(x), Some(l)) if path_known => (x - l.x).abs() < 3.0,
                    _ => true,
                })
                .collect();
            let mut drawn: BTreeSet<Drawn> = BTreeSet::new();
            let mut gray = false;
            let mut skip: Option<&'static str> = None;
            let mut tip = String::new();
            for s in &segs {
                let st = s.state.as_str();
                let k = if st.ends_with("_gray") {
                    gray = true;
                    continue;
                } else if st.ends_with("_uninit") {
                    if d.refs.contains(hash) || s.tip.contains("borrow has ended") {
                        // A reference's dashed line after its borrow
                        // ended on one path: it still holds a value.
                        skip = Some("a reference's dashed line (borrow ended on a path)");
                        continue;
                    }
                    Drawn::Unassigned
                } else if PARTLY_MOVED.iter().any(|m| s.tip.contains(m)) {
                    // A struct with a field moved out: its line goes on
                    // (the other fields can be used), and says so.
                    Drawn::PartlyMoved
                } else {
                    match st {
                        // A reference's line: hollow holds a `&`, solid a `&mut`.
                        "full_privilege" | "partial_privilege" if d.refs.contains(hash) || HOLDS_REF.iter().any(|h| s.tip.contains(h)) => {
                            skip = Some("a reference's line (says what it holds)");
                            continue;
                        }
                        "full_privilege" => Drawn::Usable,
                        "partial_privilege" => Drawn::Shared,
                        "revoked_privilege" => Drawn::MutLent,
                        "resource_moved" => Drawn::Moved,
                        _ => continue,
                    }
                };
                // A loan dead on some paths only, drawn live through the
                // conditional with a note on the dot where it died
                // ("x stays borrowed only on paths that skip line 8"):
                // the line is every path's, the probe one path's.
                let noted = d.dots.iter().any(|x| &x.hash == hash && x.y < y
                    && x.events.iter().any(|e| e == "dead_loan_note")
                    && s.lines.iter().any(|&(x1, y1, x2, y2)| y1.min(y2) - 1.0 <= x.y && x.y <= y1.max(y2) + 1.0
                        && x1.min(x2) - 3.0 <= x.x && x.x <= x1.max(x2) + 3.0));
                if noted && matches!(k, Drawn::Shared | Drawn::MutLent) {
                    skip = Some("loan dead on this path only, with a note saying so");
                    continue;
                }
                drawn.insert(k);
                tip = s.tip.clone();
            }
            if segs.is_empty() {
                // No line: moved, if the column's last dot so far moved
                // it, on this path (not in another arm).
                let Some(last) = last else { continue };
                if in_arm && !local {
                    continue;
                }
                // `data-rv-event` lists each event name once, so a dot that
                // moves, gets a value and moves again reads `move acquire`:
                // with both kinds on the dot, the order says nothing.
                let has = |es: &[&str]| last.events.iter().any(|e| es.contains(&e.as_str()));
                if has(MOVE_EVENTS) && has(VALUE_EVENTS) {
                    *skips.entry("a dot that both moves and gives a value").or_default() += 1;
                    continue;
                }
                let moved = has(MOVE_EVENTS)
                    || (last.events.iter().any(|e| e == "branch_join") && MAYBE_MOVED.iter().any(|m| last.tip.contains(m)));
                if !moved {
                    continue;
                }
                if last.tip.split(" / ").any(|b| b.trim_start_matches("• ").starts_with('*')) {
                    // `let x = *b;`: what the Box points to moved; the
                    // drawing keeps the Box itself.
                    skip = Some("only what a Box points to was moved");
                } else {
                    drawn.insert(Drawn::Moved);
                    tip = last.tip.clone();
                }
            }
            let why = if drawn.is_empty() {
                skip.or(gray.then_some("only another arm's line runs here"))
            } else if added_rows {
                Some("rows added between the lines")
            } else if drawn.len() > 1 {
                Some("drawn in two states at once")
            } else {
                None
            };
            if let Some(why) = why {
                *skips.entry(why).or_default() += 1;
                continue;
            }
            if drawn.is_empty() {
                continue;
            }
            let Some(name) = place_name(var) else {
                *skips.entry("not a variable or field path").or_default() += 1;
                continue;
            };
            let root = name.split('.').next().unwrap();
            if pts.items.contains(root) {
                *skips.entry("an item of the same name exists").or_default() += 1;
                continue;
            }
            match pts.bindings[p.func].get(root) {
                Some(1) => {}
                Some(_) => {
                    *skips.entry("name bound more than once in the function").or_default() += 1;
                    continue;
                }
                None => {
                    *skips.entry("not bound in this function").or_default() += 1;
                    continue;
                }
            }
            let drawn = *drawn.iter().next().unwrap();
            out.push(Claim { line: p.line, func: p.func, var: name, drawn, in_loop: p.in_loop, tip });
        }
    }
    out
}

/// The points rustc reaches: a never-assigned local per point, borrowed
/// there, is an error (E0381) only where the code is reachable. After a
/// `return`, `break`, `panic!` or `process::exit` nothing is
/// borrow-checked, so a probe there would pass whatever is drawn.
fn reachable(src: &str, pts: &Points, lines: &BTreeSet<(usize, u32)>) -> Result<HashSet<(usize, u32)>, String> {
    let mut ins: Vec<(u32, String)> = Vec::new();
    // For each inserted line, the point whose use it is.
    let mut use_of: Vec<Option<(usize, u32)>> = Vec::new();
    for (k, &(f, l)) in lines.iter().enumerate() {
        let Some(open) = pts.spans[f].2 else { continue };
        ins.push((open, format!("let __rv_reach_{}: u8;", k)));
        use_of.push(None);
        ins.push((l, format!("let _ = &__rv_reach_{};", k)));
        use_of.push(Some((f, l)));
    }
    let (text, at) = insert_lines(src, &ins);
    let mut out = HashSet::new();
    for (code, line, msg, _) in rustc_errors(&text)? {
        match (code.as_deref(), line.and_then(|l| at.get(&l)).and_then(|&k| use_of[k])) {
            (Some("E0381"), Some(point)) => {
                out.insert(point);
            }
            _ => return Err(format!("reachability probe: {:?} L{:?}: {}", code, line, msg)),
        }
    }
    Ok(out)
}

/// `src` with each (after line, text) on a new line after that line;
/// new line number -> index into `ins`.
fn insert_lines(src: &str, ins: &[(u32, String)]) -> (String, HashMap<u32, usize>) {
    let mut at: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, (l, _)) in ins.iter().enumerate() {
        at.entry(*l).or_default().push(i);
    }
    let mut out = String::new();
    let mut lines = HashMap::new();
    let mut n = 0u32;
    for (i, l) in src.lines().enumerate() {
        out.push_str(l);
        out.push('\n');
        n += 1;
        for &k in at.get(&(i as u32 + 1)).into_iter().flatten() {
            out.push_str(&ins[k].1);
            out.push('\n');
            n += 1;
            lines.insert(n, k);
        }
    }
    (out, lines)
}

// ─── rustc ──────────────────────────────────────────────────────────

fn toolchain() -> String {
    let t = include_str!("../../../rust-toolchain.toml");
    t.lines()
        .find_map(|l| l.trim().strip_prefix("channel").map(|r| r.trim_start_matches([' ', '=']).trim().trim_matches('"').to_owned()))
        .expect("channel in rust-toolchain.toml")
}

fn cache_dir() -> PathBuf {
    std::env::var_os("RV_ORACLE_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_TARGET_TMPDIR")).join("rv-oracle-cache"))
}

/// One error: (code, primary line, message, lines of the spans that say
/// where a borrow is "later used" / "might be used").
type Diag = (Option<String>, Option<u32>, String, Vec<u32>);

/// `rustc -vV` of the pinned toolchain (version, commit, host): part of
/// every cache key.
fn rustc_version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        Command::new("rustc").arg("-vV").env("RUSTUP_TOOLCHAIN", toolchain()).output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    })
}

/// rustc's errors for `src` as an edition-2024 lib, cached as a JSON list
/// of `[code, line, message]` keyed by the toolchain and source.
fn rustc_errors(src: &str) -> Result<Vec<Diag>, String> {
    let tc = toolchain();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ("rv-oracle-2", tc.as_str(), rustc_version(), src).hash(&mut h);
    let key = format!("{:016x}{:08x}", h.finish(), src.len());
    let dir = cache_dir();
    let cached = dir.join(format!("{}.json", key));
    if let Some(d) = std::fs::read_to_string(&cached).ok().and_then(|s| serde_json::from_str::<Vec<Diag>>(&s).ok()) {
        return Ok(d);
    }
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let file = tmp.path().join("probe.rs");
    std::fs::write(&file, src).map_err(|e| e.to_string())?;
    let out = Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "lib", "--emit=metadata", "--error-format=json",
               "-A", "warnings", "--crate-name", "rv_oracle_probe", "--out-dir"])
        .arg(tmp.path())
        .arg(&file)
        .env("RUSTUP_TOOLCHAIN", &tc)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .map_err(|e| format!("run rustc: {}", e))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    // A finished compile with errors exits 1 and says "aborting due to";
    // anything else (killed, crashed) gives no verdict.
    if !out.status.success() && (out.status.code() != Some(1) || !stderr.contains("aborting due to")) {
        return Err(format!("rustc didn't finish: {:?} {}", out.status, stderr.chars().take(400).collect::<String>()));
    }
    if !out.status.success() && !stderr.contains("\"level\":\"error\"") {
        return Err(format!("rustc failed without a diagnostic: {}", stderr.chars().take(400).collect::<String>()));
    }
    let mut diags = Vec::new();
    for l in stderr.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(l) else { continue };
        if v["level"] != "error" {
            continue;
        }
        let code = v["code"]["code"].as_str().map(str::to_owned);
        let line = v["spans"].as_array().and_then(|ss| {
            ss.iter().find(|s| s["is_primary"] == true).and_then(|s| s["line_start"].as_u64()).map(|l| l as u32)
        });
        let msg = v["message"].as_str().unwrap_or("").to_owned();
        if line.is_none() && code.is_none() {
            continue; // "aborting due to N previous errors"
        }
        let later: Vec<u32> = v["spans"].as_array().into_iter().flatten()
            .filter(|s| s["label"].as_str().is_some_and(|l| l.contains("later used") || l.contains("might be used")))
            .filter_map(|s| s["line_start"].as_u64().map(|l| l as u32))
            .collect();
        diags.push((code, line, msg, later));
    }
    if diags.iter().any(|(c, _, _, _)| c.as_deref() == Some("E0463")) {
        return Err("rustc can't find a crate (E0463): toolchain setup".to_owned());
    }
    if !out.status.success() && diags.is_empty() {
        return Err(format!("rustc failed: {}", stderr.chars().take(400).collect::<String>()));
    }
    let _ = std::fs::create_dir_all(&dir);
    let part = dir.join(format!("{}.{}.{:?}.tmp", key, std::process::id(), std::thread::current().id()));
    if serde_json::to_string(&diags).is_ok_and(|j| std::fs::write(&part, j).is_ok()) {
        let _ = std::fs::rename(&part, &cached);
    }
    Ok(diags)
}

#[derive(Debug, Clone)]
struct Probe {
    claim: usize,
    after: u32,
    mutable: bool,
}

impl Probe {
    fn text(&self, claims: &[Claim]) -> String {
        format!("let _ = &{}{}; // rv-oracle", if self.mutable { "mut " } else { "" }, claims[self.claim].var)
    }
}

/// `src` with each probe on a new line after its line; the probe's line.
fn with_probes(src: &str, probes: &[&Probe], claims: &[Claim]) -> (String, HashMap<u32, usize>) {
    let ins: Vec<(u32, String)> = probes.iter().map(|p| (p.after, p.text(claims))).collect();
    insert_lines(src, &ins)
}

/// What rustc said about one probe.
#[derive(Debug, Clone)]
enum Outcome {
    /// The probe compiled with these errors at its line: (code, message).
    Errors(BTreeSet<(String, String)>),
    /// rustc can't type-check it there (out of scope, no such field).
    NoTypecheck,
    /// It makes another line fail: using a reference (or a closure or
    /// iterator holding one) keeps its loan alive up to the probe.
    Disturbs,
}

/// rustc's verdict on each probe. Probes that don't type-check are left
/// out and the rest compiled again; probes that make another line fail
/// are found by halving the set.
fn run_probes(src: &str, probes: &[&Probe], claims: &[Claim]) -> Result<Vec<Outcome>, String> {
    let mut result: Vec<Outcome> = vec![Outcome::NoTypecheck; probes.len()];
    let mut active: Vec<usize> = (0..probes.len()).collect();
    for _ in 0..8 {
        if active.is_empty() {
            return Ok(result);
        }
        let ps: Vec<&Probe> = active.iter().map(|&i| probes[i]).collect();
        let (text, lines) = with_probes(src, &ps, claims);
        let errs = rustc_errors(&text)?;
        let mut got: HashMap<usize, BTreeSet<(String, String)>> = HashMap::new();
        let mut bad = HashSet::new();
        let mut elsewhere = false;
        for (code, line, msg, later) in errs {
            // E0596 for a binding not declared `mut` says nothing about
            // borrows (its span is the binding when reported for several
            // uses). For a place behind a `&` reference (`&mut p.a` with
            // `p: &S`) the `&mut` probe can't test anything: noted.
            if code.as_deref().is_some_and(|c| IGNORED.contains(&c)) {
                if !msg.contains("is not declared as mutable") {
                    if let Some(&k) = line.and_then(|l| lines.get(&l)) {
                        got.entry(k).or_default().insert((BEHIND_REF.to_owned(), msg));
                    }
                }
                continue;
            }
            match line.and_then(|l| lines.get(&l)) {
                Some(&k) => {
                    let c = code.clone().unwrap_or_default();
                    if CONFLICT.contains(&c.as_str()) && !later.iter().any(|l| !lines.contains_key(l)) {
                        // The loan is "later used" only at a probe line (or
                        // nowhere rustc says): the probe's own use of the
                        // borrowing value keeps it alive (`t.1 = Some(&t.0)`
                        // then `&mut t`). No verdict.
                        got.entry(k).or_default().insert((SELF_CONFLICT.to_owned(), msg));
                    } else if CONFLICT.contains(&c.as_str()) || NO_VALUE.contains(&c.as_str()) {
                        got.entry(k).or_default().insert((c, msg));
                    } else {
                        bad.insert(k);
                    }
                }
                None => elsewhere = true,
            }
        }
        if !bad.is_empty() {
            active = active.iter().enumerate().filter(|(k, _)| !bad.contains(k)).map(|(_, &i)| i).collect();
            continue;
        }
        if elsewhere {
            if active.len() == 1 {
                result[active[0]] = Outcome::Disturbs;
                return Ok(result);
            }
            let (a, b) = active.split_at(active.len() / 2);
            for half in [a, b] {
                let ps: Vec<&Probe> = half.iter().map(|&i| probes[i]).collect();
                for (k, o) in run_probes(src, &ps, claims)?.into_iter().enumerate() {
                    result[half[k]] = o;
                }
            }
            return Ok(result);
        }
        for (k, &i) in active.iter().enumerate() {
            result[i] = Outcome::Errors(got.remove(&k).unwrap_or_default());
        }
        return Ok(result);
    }
    Err("probes still don't type-check after 8 rounds".to_owned())
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub programs: usize,
    pub claims: usize,
    pub tested: usize,
    pub violations: usize,
    pub in_loop_violations: usize,
    pub skips: Skips,
}

impl Stats {
    pub fn add(&mut self, o: &Stats) {
        self.programs += o.programs;
        self.claims += o.claims;
        self.tested += o.tested;
        self.violations += o.violations;
        self.in_loop_violations += o.in_loop_violations;
        for (k, v) in &o.skips {
            *self.skips.entry(k).or_default() += v;
        }
    }
}

fn describe(d: Drawn) -> &'static str {
    match d {
        Drawn::Usable => "holding its value, not borrowed",
        Drawn::Shared => "immutably borrowed (read only)",
        Drawn::MutLent => "mutably borrowed (can't be used)",
        Drawn::Moved => "moved (no value)",
        Drawn::PartlyMoved => "partially moved (can't be used as a whole)",
        Drawn::Unassigned => "not assigned yet",
    }
}

/// The violations of one program: (check, what), and the counts.
pub fn check(src: &str, timeline: &str, code: &str) -> (Vec<Violation>, Vec<(u32, String, &'static str)>, Stats) {
    let mut st = Stats { programs: 1, ..Default::default() };
    let mut out = Vec::new();
    if has_marker(src) {
        *st.skips.entry("program: rustviz: hide/skip").or_default() += 1;
        return (out, Vec::new(), st);
    }
    if ["line!", "column!", "file!", "env!", "option_env!", "include!", "include_str!", "include_bytes!", "module_path!"]
        .iter().any(|m| src.contains(m))
    {
        // Inserting lines changes what these expand to anywhere in the
        // file (and `env!` makes the program depend on the environment).
        *st.skips.entry("program: uses line!/column!/file!/env!/include!").or_default() += 1;
        return (out, Vec::new(), st);
    }
    if src.contains("macro_rules") {
        // A macro can bind a name syn never sees, so "bound once" can't
        // be trusted.
        *st.skips.entry("program: defines a macro").or_default() += 1;
        return (out, Vec::new(), st);
    }
    let d = match drawing(timeline, code) {
        Ok(d) => d,
        Err(e) => {
            out.push(infra(format!("couldn't read the panels: {}", e)));
            return (out, Vec::new(), st);
        }
    };
    let Ok(file) = syn::parse_file(src) else {
        *st.skips.entry("program: syn can't parse it").or_default() += 1;
        return (out, Vec::new(), st);
    };
    let mut pts = Points { src: src.lines().map(str::to_owned).collect(), ..Default::default() };
    pts.visit_file(&file);
    let claims = claims(&pts, &d, &mut st.skips);
    match rustc_errors(src) {
        Ok(e) if e.is_empty() => {}
        Ok(_) => {
            *st.skips.entry("program: doesn't compile as an edition-2024 lib").or_default() += 1;
            return (out, Vec::new(), st);
        }
        Err(e) => {
            out.push(infra(e));
            return (out, Vec::new(), st);
        }
    }
    let lines: BTreeSet<(usize, u32)> = claims.iter().map(|c| (c.func, c.line)).collect();
    let live = match reachable(src, &pts, &lines) {
        Ok(r) => r,
        Err(e) => {
            out.push(infra(format!("probe program failed: {}", e)));
            return (out, Vec::new(), st);
        }
    };
    let (claims, dead): (Vec<Claim>, Vec<Claim>) = claims.into_iter().partition(|c| live.contains(&(c.func, c.line)));
    for c in &dead {
        let why = if pts.spans[c.func].2.is_none() { "function body starts on its `{` line" } else { "dead code (after a return, break, panic)" };
        *st.skips.entry(why).or_default() += 1;
    }
    st.claims = claims.len() + st.skips.values().sum::<usize>();
    let mut probes = Vec::new();
    for (i, c) in claims.iter().enumerate() {
        match c.drawn {
            Drawn::Usable => probes.push(Probe { claim: i, after: c.line, mutable: true }),
            Drawn::Shared => {
                probes.push(Probe { claim: i, after: c.line, mutable: false });
                probes.push(Probe { claim: i, after: c.line, mutable: true });
            }
            Drawn::MutLent | Drawn::Moved | Drawn::PartlyMoved | Drawn::Unassigned => {
                probes.push(Probe { claim: i, after: c.line, mutable: false })
            }
        }
    }
    let moved = |p: &Probe| matches!(claims[p.claim].drawn, Drawn::Moved | Drawn::PartlyMoved | Drawn::Unassigned);
    let mut res: Vec<Outcome> = vec![Outcome::NoTypecheck; probes.len()];
    for want_moved in [false, true] {
        let idx: Vec<usize> = (0..probes.len()).filter(|&i| moved(&probes[i]) == want_moved).collect();
        let ps: Vec<&Probe> = idx.iter().map(|&i| &probes[i]).collect();
        match run_probes(src, &ps, &claims) {
            Ok(r) => {
                for (k, v) in r.into_iter().enumerate() {
                    res[idx[k]] = v;
                }
            }
            Err(e) => {
                out.push(infra(format!("probe program failed: {}", e)));
                return (out, Vec::new(), st);
            }
        }
    }
    // A batch only proves a probe clean. Another probe can make one fail
    // (using a reference after a probe of its lender keeps the loan
    // alive over it), and rustc reports one use of a moved value per
    // move. So every probe that failed, and every moved probe, is
    // compiled again on its own, and that verdict counts.
    for i in 0..probes.len() {
        let again = match &res[i] {
            Outcome::Errors(e) => !e.is_empty() || moved(&probes[i]),
            _ => false,
        };
        if again {
            match run_probes(src, &[&probes[i]], &claims) {
                Ok(r) => res[i] = r.into_iter().next().unwrap(),
                Err(e) => {
                    out.push(infra(format!("probe program failed: {}", e)));
                    return (out, Vec::new(), st);
                }
            }
        }
    }
    // rustc also reports a use of a moved value once per move across
    // different probes: once a probe of a variable came back moved, its
    // probes that came back clean are compiled alone too.
    let root = |p: &Probe| claims[p.claim].var.split('.').next().unwrap().to_owned();
    let moved_roots: HashSet<String> = probes.iter().zip(&res)
        .filter(|(_, r)| matches!(r, Outcome::Errors(e) if e.iter().any(|(c, _)| NO_VALUE.contains(&c.as_str()))))
        .map(|(p, _)| root(p))
        .collect();
    for i in 0..probes.len() {
        if matches!(&res[i], Outcome::Errors(e) if e.is_empty()) && moved_roots.contains(&root(&probes[i])) {
            match run_probes(src, &[&probes[i]], &claims) {
                Ok(r) => res[i] = r.into_iter().next().unwrap(),
                Err(e) => {
                    out.push(infra(format!("probe program failed: {}", e)));
                    return (out, Vec::new(), st);
                }
            }
        }
    }
    // Each probe's verdict: right, wrong (a violation), or inconclusive.
    let mut wrong: BTreeMap<usize, (&'static str, String)> = BTreeMap::new();
    let mut passed: Vec<(u32, String, &'static str)> = Vec::new();
    let mut unsure: BTreeMap<usize, &'static str> = BTreeMap::new();
    let code_is = |e: &BTreeSet<(String, String)>, codes: &[&str]| e.iter().find(|(c, _)| codes.contains(&c.as_str())).cloned();
    for (p, r) in probes.iter().zip(&res) {
        let errs = match r {
            Outcome::Errors(e) => e,
            Outcome::NoTypecheck => {
                unsure.entry(p.claim).or_insert("probe doesn't type-check there (out of scope?)");
                continue;
            }
            Outcome::Disturbs => {
                unsure.entry(p.claim).or_insert("probe makes another line fail (keeps a loan alive)");
                continue;
            }
        };
        if errs.iter().any(|(c, _)| c == BEHIND_REF) {
            unsure.entry(p.claim).or_insert("`&mut` refused for another reason than a borrow (behind `&`, in an `Rc`)");
            continue;
        }
        if errs.iter().any(|(c, _)| c == SELF_CONFLICT) {
            unsure.entry(p.claim).or_insert("the probe's own use keeps the loan alive");
            continue;
        }
        let c = &claims[p.claim];
        let conflict = code_is(errs, CONFLICT);
        let novalue = code_is(errs, NO_VALUE);
        let e0382 = errs.iter().find(|(c, _)| c == "E0382").cloned();
        let e0381 = errs.iter().find(|(c, _)| c == "E0381").cloned();
        let probe = p.text(&claims).trim_end_matches(" // rv-oracle").to_owned();
        let said = |e: &(String, String)| format!("rustc: error[{}]: {}", e.0, e.1);
        let none = format!("rustc reports no borrow conflict or move for `{}`", probe);
        // Ok(()) right, Err(Some(..)) wrong, Err(None) inconclusive.
        let v: Result<(), Option<(&'static str, String)>> = match (c.drawn, p.mutable) {
            (Drawn::Usable, _) => match conflict.or(novalue) {
                Some(e) => Err(Some(("oracle-usable", said(&e)))),
                None => Ok(()),
            },
            (Drawn::Shared, false) => match conflict.or(novalue) {
                Some(e) => Err(Some(("oracle-readable", said(&e)))),
                None => Ok(()),
            },
            // Drawn read only: `&mut x` must meet a *shared* loan. E0499
            // can be a two-phase `&mut` reservation (`v.push({ .. })`) or
            // a mutable loan: not this claim's to judge.
            (Drawn::Shared, true) => match (code_is(errs, &["E0502", "E0505", "E0506"]), conflict, novalue) {
                (Some(_), _, _) => Ok(()),
                (None, Some(_), _) | (None, None, Some(_)) => Err(None),
                (None, None, None) => Err(Some(("oracle-borrowed", none.clone()))),
            },
            (Drawn::MutLent, _) => match (code_is(errs, &["E0502", "E0503", "E0505"]), conflict, novalue) {
                (Some(_), _, _) => Ok(()),
                (None, Some(_), _) | (None, None, Some(_)) => Err(None),
                (None, None, None) => Err(Some(("oracle-borrowed", none.clone()))),
            },
            // Moved / partially moved: E0382 says "moved on some path"
            // (rustc gives the same message after `if c { drop(x) }`), so
            // it confirms only a drawing that says "may". A definite claim
            // can only be shown wrong: no E0381/E0382 means x has its
            // value on every path.
            (Drawn::Moved | Drawn::PartlyMoved, _) => {
                let maybe = MAYBE_MOVED.iter().chain(["may have been moved", "may have no value"].iter()).any(|m| c.tip.contains(m));
                let partly = c.drawn == Drawn::PartlyMoved;
                match (&e0382, &e0381) {
                    (Some(e), _) if maybe && e.1.contains("partially") == partly => Ok(()),
                    (Some(_), _) | (None, Some(_)) => Err(None),
                    (None, None) => Err(Some(("oracle-moved", conflict.as_ref().map_or(none.clone(), said)))),
                }
            }
            // "has no value yet" is E0381 isn't initialized, on every
            // path; "may have no value" is any E0381 / E0382.
            (Drawn::Unassigned, _) => {
                let maybe = c.tip.contains("may have no value");
                match (&e0381, &e0382) {
                    (Some(e), _) if maybe || !e.1.contains("possibly") => Ok(()),
                    (_, Some(_)) if maybe => Ok(()),
                    (Some(_), _) | (_, Some(_)) => Err(None),
                    (None, None) => Err(Some(("oracle-moved", conflict.as_ref().map_or(none.clone(), said)))),
                }
            }
        };
        let check_of = match (c.drawn, p.mutable) {
            (Drawn::Usable, _) => "oracle-usable",
            (Drawn::Shared, false) => "oracle-readable",
            (Drawn::Shared, true) | (Drawn::MutLent, _) => "oracle-borrowed",
            _ => "oracle-moved",
        };
        match v {
            Ok(()) => passed.push((c.line, c.var.clone(), check_of)),
            Err(Some(w)) => {
                wrong.entry(p.claim).or_insert(w);
            }
            Err(None) => {
                unsure.entry(p.claim).or_insert("rustc's verdict doesn't decide the drawn state");
            }
        }
    }
    // A probe that rustc contradicts is a violation even when another
    // probe of the same claim was inconclusive. A listed violation is
    // GONE only if its own check passed (`passed`).
    let mut tested = 0;
    for i in 0..claims.len() {
        if wrong.contains_key(&i) || !unsure.contains_key(&i) {
            tested += 1;
        } else if let Some(why) = unsure.get(&i) {
            *st.skips.entry(why).or_default() += 1;
        }
    }
    st.tested = tested;
    for (i, (check, verdict)) in wrong {
        let c = &claims[i];
        let src_line = src.lines().nth(c.line as usize - 1).unwrap_or("").trim();
        st.violations += 1;
        if c.in_loop {
            st.in_loop_violations += 1;
        }
        let tip: String = c.tip.chars().take(160).collect();
        out.push(Violation {
            check,
            line: c.line,
            var: c.var.clone(),
            text: format!(
                "{} after L{} `{}`{}: drawn {} ({:?}); {}",
                c.var, c.line, src_line, if c.in_loop { " (in a loop)" } else { "" }, describe(c.drawn), tip, verdict
            ),
        });
    }
    (out, passed, st)
}

/// One reported contradiction.
#[derive(Debug, Clone)]
pub struct Violation {
    pub check: &'static str,
    /// The line the probe follows and the column (0 and "" for an
    /// `oracle-error`).
    pub line: u32,
    pub var: String,
    pub text: String,
}

fn infra(text: String) -> Violation {
    Violation { check: "oracle-error", line: 0, var: String::new(), text }
}

/// One snippet's results: its violations and the (line, column, check)
/// probes that rustc confirmed.
pub type Checked = (Vec<Violation>, Vec<(u32, String, &'static str)>);

/// What differs from `KNOWN_ORACLE_FAILURES` / `ORACLE_EXEMPT`, in
/// `invariants::report`'s format (`tools/progen` parses its NEW lines).
/// An entry is GONE only when its (line, column) claim was tested and
/// passed; one whose claim wasn't tested this run is neither.
pub fn report(results: &BTreeMap<String, Checked>) -> String {
    let listed: HashSet<(&str, &str, u32, &str)> = KNOWN_ORACLE_FAILURES.iter().map(|(s, c, l, v, _)| (*s, *c, *l, *v))
        .chain(ORACLE_EXEMPT.iter().map(|(s, c, l, v, _)| (*s, *c, *l, *v)))
        .collect();
    let mut out = String::new();
    let mut new: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    for (snippet, (vs, _)) in results {
        for v in vs {
            if !listed.contains(&(snippet.as_str(), v.check, v.line, v.var.as_str())) {
                new.entry((snippet.as_str(), v.check)).or_default().push(&v.text);
            }
        }
    }
    for ((snippet, check), whats) in new {
        let why = ORACLE_CHECKS.iter().find(|(n, _)| *n == check).map_or("", |(_, w)| *w);
        out.push_str(&format!("\nNEW  {snippet}: {check} ({why})\n"));
        for w in whats {
            out.push_str(&format!("       {w}\n"));
        }
    }
    let gone = |s: &str, c: &str, l: u32, v: &str| {
        results.get(s).is_some_and(|(vs, passed)| {
            passed.iter().any(|(pl, pv, pc)| *pl == l && pv == v && *pc == c)
                && !vs.iter().any(|x| x.check == c && x.line == l && x.var == v)
        })
    };
    for (s, c, l, v, issue) in KNOWN_ORACLE_FAILURES {
        if gone(s, c, *l, v) {
            out.push_str(&format!(
                "\nGONE {s}: {c} {v} after L{l} (#{issue}) no longer happens. Fixed? Delete its entry from \
                 KNOWN_ORACLE_FAILURES (and check #{issue}).\n"));
        }
    }
    for (s, c, l, v, why) in ORACLE_EXEMPT {
        if gone(s, c, *l, v) {
            out.push_str(&format!(
                "\nGONE {s}: {c} {v} after L{l} (exempt: {why}) no longer happens. Delete its entry from ORACLE_EXEMPT.\n"));
        }
    }
    out
}

/// Listed entries whose claim wasn't tested this run (neither found nor
/// gone): reported, not failed.
pub fn untested_listed(results: &BTreeMap<String, Checked>) -> usize {
    KNOWN_ORACLE_FAILURES.iter().map(|(s, c, l, v, _)| (*s, *c, *l, *v))
        .chain(ORACLE_EXEMPT.iter().map(|(s, c, l, v, _)| (*s, *c, *l, *v)))
        .filter(|(s, c, l, v)| results.get(*s).is_none_or(|(vs, passed)| {
            !passed.iter().any(|(pl, pv, pc)| pl == l && pv == v && pc == c)
                && !vs.iter().any(|x| x.check == *c && x.line == *l && x.var == *v)
        }))
        .count()
}

/// Every entry names a check, and none is listed twice.
pub fn known_list_problems() -> Vec<String> {
    let names: HashSet<&str> = ORACLE_CHECKS.iter().map(|(n, _)| *n).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let both = KNOWN_ORACLE_FAILURES.iter().map(|(s, c, l, v, _)| (*s, *c, *l, *v))
        .chain(ORACLE_EXEMPT.iter().map(|(s, c, l, v, _)| (*s, *c, *l, *v)));
    for e @ (s, c, l, v) in both {
        if !names.contains(c) {
            out.push(format!("({s}, {c}, {l}, {v}) names no check"));
        }
        if c == "oracle-error" {
            out.push(format!("({s}, {c}): an oracle error is fixed, not listed"));
        }
        if !seen.insert(e) {
            out.push(format!("({s}, {c}, {l}, {v}) is listed twice"));
        }
    }
    out
}

#[cfg(test)]
mod unit {
    use super::*;

    fn points(src: &str) -> Vec<(u32, bool)> {
        let mut p = Points { src: src.lines().map(str::to_owned).collect(), ..Default::default() };
        p.visit_file(&syn::parse_file(src).unwrap());
        let mut v: Vec<(u32, bool)> = p.points.iter().map(|p| (p.line, p.in_loop)).collect();
        v.sort();
        v
    }

    #[test]
    fn statement_boundaries() {
        let src = "fn main() {\n    let a = 1; let b = 2;\n    let s = String::new();\n    if a > b {\n        let t = 1;\n    }\n    for i in 0..2 {\n        let _ = i;\n    }\n    let c = || {\n        let z = 1;\n    };\n    let d = {\n        5\n    };\n    d\n}\n";
        // Not after `let a` (b starts on its line), not in the closure,
        // not after a tail (`5`, `d`).
        assert_eq!(points(src), vec![(2, false), (3, false), (5, false), (6, false), (8, true), (9, false), (12, false), (15, false)]);
    }

    #[test]
    fn or_pattern_binds_once() {
        let src = "fn main() {\n    let u = (1, 2);\n    if let (x, _) | (_, x) = u {\n        let _ = x;\n    }\n}\n";
        let mut p = Points { src: src.lines().map(str::to_owned).collect(), ..Default::default() };
        p.visit_file(&syn::parse_file(src).unwrap());
        assert_eq!(p.bindings[0].get("x"), Some(&1));
    }

    #[test]
    fn no_points_where_inserting_is_unsound() {
        // Call / method arguments (a two-phase `&mut` reservation may be
        // live), an array length (a const context), a function with a
        // block comment, an array type's length (`line!()` and friends
        // skip the whole file, in `check`).
        let src = "fn main() {\n    let mut v = vec![1];\n    v.push({\n        let n = v.len();\n        n\n    });\n    let a = [0u8; {\n        let k = 2;\n        k\n    }];\n    let b = 1;\n    let _ = (a, b);\n}\nfn g() {\n    let x = 1; /* c */\n    let y = x;\n    let _ = y;\n}\nfn h() {\n    let t: [u8; {\n        let k = 1;\n        k\n    }] = [0];\n    let _ = t;\n}\n";
        assert_eq!(points(src), vec![(2, false), (6, false), (10, false), (11, false), (12, false), (23, false), (24, false)]);
    }

    #[test]
    fn item_names_are_collected() {
        let src = "const N: u8 = 1;\nenum E { A }\nuse std::mem::swap as sw;\nfn main() {\n    fn x() {}\n    let x = 1;\n}\n";
        let mut p = Points { src: src.lines().map(str::to_owned).collect(), ..Default::default() };
        p.visit_file(&syn::parse_file(src).unwrap());
        for n in ["N", "E", "A", "sw", "main", "x"] {
            assert!(p.items.contains(n), "{n}");
        }
    }

    #[test]
    fn raw_identifiers_are_the_same_name() {
        let src = "fn main() {\n    let x = 1;\n    let r#x = 2;\n    let _ = x;\n}\n";
        let mut p = Points { src: src.lines().map(str::to_owned).collect(), ..Default::default() };
        p.visit_file(&syn::parse_file(src).unwrap());
        assert_eq!(p.bindings[0].get("x"), Some(&2));
    }

    #[test]
    fn markers_as_the_plugin_reads_them() {
        assert!(has_marker("fn f() {} // rustviz:hide"));
        assert!(has_marker("// rustviz : skip — see #42"));
        assert!(!has_marker("fn f() { println!(\"rustviz: hide\"); }"));
        assert!(!has_marker("// rustviz: hidden"));
    }

    #[test]
    fn place_names() {
        assert_eq!(place_name("s₂").as_deref(), Some("s"));
        assert_eq!(place_name("p.x").as_deref(), Some("p.x"));
        assert_eq!(place_name("t.0").as_deref(), Some("t.0"));
        assert_eq!(place_name("*r"), None);
        assert_eq!(place_name("String::from"), None);
        assert_eq!(place_name("v[0]"), None);
        assert_eq!(place_name("0"), None);
    }
}
