# Known bugs

Minimal repros of drawings main gets wrong. Each compiles and renders; none
pins its wrong output as expected. They run through the corpus-wide invariant
checks and the rustc oracle (`RV_ORACLE=1`), and the violations main has on
them are listed in `KNOWN_INVARIANT_FAILURES` and `KNOWN_ORACLE_FAILURES`
by issue (a fix deletes its entries). Files are named `<issue>_<what>.rs`.

## Programs kept from the closed 2.x fix PRs

2.x fixes stopped at 2.1.1; RustViz 3 rebuilds the drawing on the borrow
checker's MIR facts. These programs came from the 2.x fix branches that were
closed unmerged (#1600 #1602 #1603 #1610 #1616 #1619 #1646 #1667 #1696 #1746
#1754 #1777, #1807 #1808 #1820 #1821 #1822, and the `cx/cx-text-a`,
`cx/cx-c18`, `cx/cx-c04`, `cx/cx-c09` branches). Only programs main still
draws wrong were kept. The file's first lines name its source branch. The
"what is wrong" column is the issue's title (the issue has the details). "How
main was found wrong" says what flagged it: the rustc oracle, the invariant
checks, a tooltip the fix branch's test forbids that main prints ("false
text"), a tooltip the branch's test requires that main lacks ("pins"), or only
the open issue ("issue"). RustViz 3's fact layer should run its oracle check
over all of these.

| Program | Issue | What is wrong on main (per the issue) | How main was found wrong |
|---|---|---|---|
| `1566_arm_ref_copy_end.rs` | #1566 | Unused reference copy in a conditional arm: its "borrow ends" is drawn in a later arm, after its scope ended | issue |
| `1575_dead_loan_then_mut_borrow.rs` | #1575 | A dead first loan of a re-pointed reference (`let mut r2 = &s; .. r2 = &t;`) keeps a later `&mut s` in a loop or `if` from ever giving s back | rustc oracle, tooltip pins |
| `1578_match_scrutinee_ref_repointed.rs` | #1578 | `match` on a reference's use (`match r.as_str()`), r re-pointed in one arm: r's old loan ends only in that arm; the other arms use the lender while it is still drawn borrowed | invariant checks |
| `1580_repoint_later_use.rs` | #1580 | Reference re-pointed in an arm of a nested conditional (`else if` chain, `else { if .. }`) while its old loan is dead: inner join says "none of the branches above assigned it", outer join has no dot | rustc oracle, invariant checks, tooltip pins |
| `1580_repoint_nested_arm_dead_loan.rs` | #1580 | Reference re-pointed in an arm of a nested conditional (`else if` chain, `else { if .. }`) while its old loan is dead: inner join says "none of the branches above assigned it", outer join has no dot | invariant checks, false tooltip text |
| `1580_repoint_nested_arm_live_loan.rs` | #1580 | Reference re-pointed in an arm of a nested conditional (`else if` chain, `else { if .. }`) while its old loan is dead: inner join says "none of the branches above assigned it", outer join has no dot | rustc oracle, invariant checks, tooltip pins |
| `1581_nested_shared_ends.rs` | #1581 | Reference last used in an arm of a nested conditional (`else if` chain, `else { if .. }`) while another reference to the same lender is live: its borrow never ends in the nested arms | invariant checks, false tooltip text |
| `1582_maybe_held_assign_move_join.rs` | #1582 | A value that may have no value (moved, then reassigned in some branches) entering a later `if` whose arm assigns and moves it, or runs a `?`: the join has no dot and the leg stops in mid-air | invariant checks, false tooltip text |
| `1584_maybe_held_second_returning_if.rs` | #1584 | Second of two returning `if`s inside an arm, on a value that may have no value: the second `if` gets no split dot and its `then` leg starts in mid-air | invariant checks, false tooltip text |
| `1585_unused_ref_binding_nested_return.rs` | #1585 | Unused `ref` match binding inside an `if let` arm, followed by an `if .. { return; }`: the lender is drawn still shared through the nested `if`, and its trunk runs through the fork | rustc oracle, invariant checks, false tooltip text |
| `1586_cond_last_use_repointed_after.rs` | #1586 | Reference last read in an `if` condition and re-pointed after the `if`: the lender's arms are drawn "being shared" and its trunk runs through the fork | invariant checks |
| `1587_loop_two_conditionals.rs` | #1587 | Loop-assigned value through two later `if`s: the third assignment says "its previous resource is dropped" unconditionally, and a solid owner line follows a "may have no value" join | invariant checks, false tooltip text |
| `1587_maybe_held_reassign_after_loop.rs` | #1587 | Loop-assigned value through two later `if`s: the third assignment says "its previous resource is dropped" unconditionally, and a solid owner line follows a "may have no value" join | invariant checks, false tooltip text |
| `1588_field_repoint_in_arm.rs` | #1588 | Struct reference field holding a two-lender loan, re-pointed (`h.r = &t;`): the old loan ends twice and the co-lender stays "shared" to the end of the fn on the other path | rustc oracle, invariant checks, false tooltip text |
| `1594_let_else_one_line_binds_at_join.rs` | #1594 | One-line let-else drawn as a branch: the pattern's binding starts on the `else` sub-row, above the join | issue |
| `1595_struct_arm_parts_moved.rs` | #1595 | A struct whose fields are moved in an arm keeps one line through the conditional: inside the arm, after the move, it's still drawn as before it | rustc oracle, false tooltip text |
| `1596_downgraded_arm_loan.rs` | #1596 | Borrow made and ended in an arm where an outer `&mut` of the same lender is dead on that path (`let r3 = downgrade(&mut s2);` after `let n = m.len();`): the lender never gets it back | false tooltip text |
| `1597_chain_call_use_before_end.rs` | #1597 | One-line let-chain with no drawn conditional: a method call's read on x drawn before x's borrow (State::Invalid on x) | invariant checks |
| `1598_else_if_let_implicit_else_join.rs` | #1598 | Implicit else of an `else if let` sharing its join line with the outer if: the leg to the raised inner join starts at a corner with no dot | invariant checks |
| `1604_field_two_lender_repoint.rs` | #1604 | Two-lender field re-pointed straight-line: its old loan's end is drawn twice, on two rows ("of s or t ends", then "borrow ends") | rustc oracle |
| `1620_one_line_reassigns.rs` | #1620 | Several assignments to one variable on one line (`s = String::from("b"); s = String::from("c");`): their arrows and function logos are drawn on top of each other | tooltip pins |
| `1621_two_phase_reborrow.rs` | #1621 | `v.push(v.len())` (two-phase borrow): push's borrow is drawn starting and ending before len reads v | false tooltip text |
| `1622_closure_arg_taken_by_call.rs` | #1622 | Move closure passed to a function (`run(move \|\| s)`): s is said to be dropped "when that closure is dropped" even when the call consumes it and returns s | false tooltip text |
| `1623_fn_once_param_call.rs` | #1623 | Calling an `impl FnOnce` parameter (`fn run(f: impl FnOnce() -> String) -> String { f() }`): "f's resource is moved to the caller" | false tooltip text |
| `1624_mut_ref_arg_may_write.rs` | #1624 | Passing a `&mut` reference variable to a function (`ignore(r)`): "ignore reads and writes through r" even when it does neither | false tooltip text |
| `1625_repoint_live_reborrow.rs` | #1625 | Reference re-pointed while a reborrow of it is live (`let a = &mut *r; r = &mut y; *r = 1; *a = 2;`): r stays drawn "can't be used" and its new loan ends at a's last use | rustc oracle, invariant checks, tooltip pins |
| `1626_repoint_return_let_else.rs` | #1626 | let-else whose `else` re-points a reference and returns: after the let-else, a write through it drops the else-branch's target | false tooltip text |
| `1627_closure_captures_review.rs` | #1627, #1622, #1701 | Closure that both reads and writes through a captured `&mut` (`\|\| { println!("{}", r); *r += 1; }`): r drawn "read-only" instead of unusable (unique borrow) | rustc oracle, invariant checks, false tooltip text |
| `1627_closure_unique_capture.rs` | #1627 | Closure that both reads and writes through a captured `&mut` (`\|\| { println!("{}", r); *r += 1; }`): r drawn "read-only" instead of unusable (unique borrow) | rustc oracle, false tooltip text |
| `1632_extend_field_loan.rs` | #1632 | `v.extend([&p.0])` then `p.1 = 3`: p.1 drawn "read-only while the borrow is live" while its write is drawn on the same stretch | rustc oracle, false tooltip text |
| `1636_self_struct_update.rs` | #1636 | Struct update into the same variable (`p = Pair { a: new, ..p }`): p.a's old value isn't dropped | tooltip pins |
| `1637_drop_holder_return.rs` | #1637 | Value with a `Drop` impl holding `&mut n`, `return` in an `if` arm: n's borrow drawn ending at the arm's first line, before the destructor uses it | rustc oracle, invariant checks, tooltip pins |
| `1638_borrow_ref_scrutinee.rs` | #1638 | The single-arm `match &mut r` lends the slot on the scrutinee row. The `if let` and `while let` rows still mishandle the slot borrow. | rustc oracle, invariant checks, false tooltip text |
| `1638_chain_ref_scrutinee.rs` | #1638 | `match &mut r { s => **s += 1 }` with `r: &mut i32`: drawn "Move from r to s" and r unusable after | rustc oracle, invariant checks, false tooltip text |
| `1642_projected_branch_write_chain.rs` | #1642 | Writing through a Box in both arms (`if c { *b = 1 } else { *b = 2 }`): the join says "b acquired ownership of a resource (in all branches above)" | false tooltip text |
| `1642_projected_branch_writes.rs` | #1642 | Writing through a Box in both arms (`if c { *b = 1 } else { *b = 2 }`): the join says "b acquired ownership of a resource (in all branches above)" | rustc oracle, false tooltip text |
| `1643_join_before_return.rs` | #1643 | Maybe-moved / maybe-assigned join followed by an `if .. { return; }` and a reassignment: the join names the wrong place the value is dropped | false tooltip text |
| `1645_move_then_two_reassigns.rs` | #1645 | Checker: `drop-of-moved-value-at-reassign` false positive when a move and two reassignments share one line (`drop(s); s = ..; s = ..;`) | invariant checks |
| `1653_or_pattern_unreachable.rs` | #1653 | Or-pattern with an unreachable second alternative: "u.0 and u.1 may have been moved out" though only u.0 can move | rustc oracle, false tooltip text |
| `1660_try_after_diverging_assignment.rs` | #1660 | `?` after a conditional whose only assignment of a maybe-assigned `let x;` is in a returning branch says "x's resource is dropped" | false tooltip text |
| `1662_arm_loan_kinds.rs` | #1662 | Holder from a conditional value: each arm gets every arm's lenders and kinds; a conditional tuple element loses its lenders | rustc oracle, invariant checks, false tooltip text |
| `1664_stored_reference_slot.rs` | #1664 | `&mut r` stored through a binding (`u.push(slot)`): holder dot says it borrows "the data r points to" | invariant checks, false tooltip text |
| `1666_struct_last_move_in_arms.rs` | #1666 | Struct whose last field is moved in every arm: title says no field can be used before the arms run | rustc oracle, invariant checks, false tooltip text |
| `1676_unread_holder_overwrite.rs` | #1676 | Holder overwritten before its first value is read (`let mut hold = H { r: &a }; .. hold = other;`): a drawn borrowed until the overwrite, rustc ends the loan at once | rustc oracle, tooltip pins |
| `1677_drop_holder_rhs.rs` | #1677 | `h = if c { .. } else { .. };` on a holder with `Drop`: the old value's loan is drawn ended before the arms, but it lives until the assignment drops it | rustc oracle, tooltip pins |
| `1678_closure_shared_read.rs` | #1678 | Closure capture by shared reference, lender read before the closure's call (Book listing 13-4): the read's dot says "no longer immutably borrowed" and the line goes solid "owner" | rustc oracle, tooltip pins |
| `1680_nested_holder_copy.rs` | #1680 | `let g2 = Holder { r: g.r };` in an `else if` arm: the lender is drawn "owner" (free) while g2 still holds its loan | tooltip pins |
| `1681_reused_element.rs` | #1681 | `let o = v.last_mut(); if let Some(e) = o {..}` in a block, then `for e in v.iter_mut()`: v drawn mutably borrowed after the block | rustc oracle, invariant checks, tooltip pins |
| `1682_mixed_vec_elements.rs` | #1682 | `vec![s2, format!(..)]`: s2's move into the vec isn't drawn; its line stays "s2 is the owner of the resource" | rustc oracle, false tooltip text |
| `1682_vec_macro_element_move.rs` | #1682 | `vec![s2, format!(..)]`: s2's move into the vec isn't drawn; its line stays "s2 is the owner of the resource" | rustc oracle, tooltip pins |
| `1683_nested_if_let_join.rs` | #1683 | `if let Some(x) = v.first()` with a moving nested `if let` and a later read of x: v drawn "being shared" again after the outer `if let` | rustc oracle, tooltip pins |
| `1686_box_shared_across_read.rs` | #1686 | rustc oracle: six generated programs with drawn borrow/move states rustc contradicts, to minimize | rustc oracle, tooltip pins |
| `1686_let_else_unused_ref.rs` | #1686 | rustc oracle: six generated programs with drawn borrow/move states rustc contradicts, to minimize | rustc oracle, tooltip pins |
| `1686_match_unused_ref.rs` | #1686 | rustc oracle: six generated programs with drawn borrow/move states rustc contradicts, to minimize | rustc oracle, tooltip pins |
| `1686_move_after_loop.rs` | #1686 | rustc oracle: six generated programs with drawn borrow/move states rustc contradicts, to minimize | tooltip pins |
| `1686_nested_chain.rs` | #1686 | rustc oracle: six generated programs with drawn borrow/move states rustc contradicts, to minimize | tooltip pins |
| `1686_pattern_else_lender.rs` | #1686 | rustc oracle: six generated programs with drawn borrow/move states rustc contradicts, to minimize | rustc oracle, tooltip pins |
| `1689_kept_field_loans.rs` | #1689 | `extend` stores (rustc-facts path): one borrow kind for all lenders, field-path receivers not followed, reference-typed field drawn as the whole variable | rustc oracle, false tooltip text |
| `1694_nested_overlapping_loans.rs` | #1694 | Nested if let ending x's borrow of v while another reference still borrows v: "v's resource is no longer immutably borrowed" | rustc oracle, invariant checks, false tooltip text |
| `1699_try_projected_place.rs` | #1699 | `(*b)?`: the Box is dropped on the early-return path but no dot says so; other `?` shapes (`(**b)?`, Poll, Copy payload) | rustc oracle, false tooltip text |
| `1700_promoted_named_constant.rs` | #1700 | Promotion test misses named constants (`&Ok(N)` with `const N`); `<String as From<&str>>::from` and `Vec::<String>::new` are named `From::from` / `Vec::new` | false tooltip text |
| `1710_conditional_borrow_before_join.rs` | #1710 | Join after a conditional assignment says "r's borrow ended before the branches above" on paths where r was never assigned | false tooltip text |
| `1711_struct_inner_move_chain.rs` | #1711 | Struct line says a field was moved out when only an element or a Box's contents inside it moved | rustc oracle, false tooltip text |
| `1711_struct_inner_moves.rs` | #1711 | Struct line says a field was moved out when only an element or a Box's contents inside it moved | rustc oracle, false tooltip text |
| `1715_nested_downgrade.rs` | #1715 | `v.push(id(downgrade(&mut a)))`: a drawn immutably borrowed ("read-only") though its mutable loan stays live | invariant checks, false tooltip text |
| `1717_two_phase_multiline.rs` | #1717 | Two-phase borrows (#1621 follow-ups): reservation not drawn, aliases, and calls over several lines keep main's order | issue |
| `1720_inline_else_loans.rs` | #1720 | #1596's shape with the other arm on one line: r3's end gives s2 nothing back | false tooltip text |
| `1724_try_and_or_pattern_let_chains.rs` | #1724, #1653 | `?` in an `if` condition is merged with an assignment in the `if` body: "If the `?` succeeds, the assignment runs" | rustc oracle, false tooltip text |
| `1724_try_if_condition.rs` | #1724 | `?` in an `if` condition is merged with an assignment in the `if` body: "If the `?` succeeds, the assignment runs" | false tooltip text |
| `1728_struct_nested_maybe_moves.rs` | #1728 | Struct line names a field as moved out when only its own fields were, one per path | rustc oracle, false tooltip text |
| `1732_stored_loan_kinds.rs` | #1732 | One loan kind for all lenders of a stored value: extend with a mixed tuple, id(r) of a two-lender r, first(if c {&x} else {&y}), a storing method | rustc oracle, invariant checks, false tooltip text |
| `1736_try_call_arguments.rs` | #1736 | `s = make(o?, p?);`: only one `?` text; the second `?` returning early is described as the assignment running | false tooltip text |
| `1738_drop_after_later_conditional.rs` | #1738 | Maybe-held join says its value is dropped at scope end though a later assignment (after a returning `if`) drops it | false tooltip text |
| `1750_chain_mixed.rs` | #1750 | Holder bound from `if`/`match` arms with mixed `&`/`&mut` lenders (`if c { Some((&a, &mut b)) } else { None }`): the `&mut` loan drawn as shared | rustc oracle, false tooltip text |
| `1750_mixed_arm_holder.rs` | #1750 | Holder bound from `if`/`match` arms with mixed `&`/`&mut` lenders (`if c { Some((&a, &mut b)) } else { None }`): the `&mut` loan drawn as shared | rustc oracle, invariant checks, false tooltip text |
| `1752_dead_loan_one_line.rs` | #1752 | Several writes to a reference on one line: "stays borrowed only on paths that skip line N" in straight-line code | invariant checks, false tooltip text |
| `1753_initialized_reference_join.rs` | #1753 | Join after a reference initialized in every arm of an earlier if says it has no value | invariant checks, false tooltip text |
| `1761_try_field_and_whole_assignment.rs` | #1761 | `p.a = x?…; p = pair(); y?;`: "If the `?` succeeds, the assignment to p.a runs" for the `?` after both assignments | false tooltip text |
| `1789_join_after_arm_loop.rs` | #1789 | `let mut s; if c { for .. { s = ..; } }`: the if's join says each branch moved s away, though s may hold the loop's value | false tooltip text |
| `1791_short_circuit_reassignment.rs` | #1791 | Assignment in a short-circuit operand (`stop && { s = ..; true }`) counted as running on every path: join says "dropped when s is assigned again" | rustc oracle, false tooltip text |
| `1796_dead_loan_maybe_repoint.rs` | #1796 | Dead-loan note says "r is given a new reference on line N" on a path where r is only maybe reassigned | false tooltip text |
| `1823_shared_ref_of_mut_hedge.rs` | #1823 | Holder loan texts state a borrow kind rustc does not give (follow-ups of #1599) | rustc oracle, invariant checks, false tooltip text |
| `1823_shared_ref_of_mut_kinds.rs` | #1823 | Holder loan texts state a borrow kind rustc does not give (follow-ups of #1599) | rustc oracle, invariant checks, false tooltip text |
| `1823_shared_ref_of_mut_siblings.rs` | #1823 | Holder loan texts state a borrow kind rustc does not give (follow-ups of #1599) | rustc oracle, invariant checks, false tooltip text |
