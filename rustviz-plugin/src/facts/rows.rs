//! The row model: which source row each MIR location belongs to, the
//! locations a row's state is read at, early-exit drop chains and the
//! arms of one-line branches.
//!
//! Rules (RustViz 3 spike, SPIKE.md "Where MIR → source mapping got
//! hard"; Aquascope's `stepper/hir_steps.rs` makes the same calls for
//! its steps):
//!
//! 1. **CFG-monotone rows.** A location's row is never above the row of
//!    the code that runs just before it on its only way in. MIR gives
//!    `drop(h); h = move tmp;` for `h = if c {..} else {..};` the span of
//!    `h`, the first row, though it runs after the arms.
//! 2. **Multi-row statements** take effect on their last row (an
//!    assignment, a call, a drop; an assignment expression's whole span
//!    counts, not just its place's); desugared code (`for`'s `next()`,
//!    `?`) and everything else on its first.
//! 3. **State after row L** is read at the row's exits that go on to the
//!    next row of the same path: not a `?`'s or `break`'s jump further
//!    down, nor the way out of a loop head. Loans are read where the
//!    next row starts (NLL kills a loan before the next location).
//! 4. **Early exits.** The drops on a `return` / `break` / `continue` /
//!    `?` path carry the span of the scope's `}`; a drop that only an
//!    exit reaches belongs to the exit's row, for its events and for the
//!    state rows (so the `};` of a `let ... else { return }` shows the
//!    path that goes on, not the one that returned).
//! 5. **Several arms on one row** (`if c { a } else { b }`): each arm's
//!    locations and exits are recorded with its source columns.

use rustc_hir as hir;
use rustc_middle::mir::{BasicBlock, Body, Location, StatementKind, TerminatorKind};
use rustc_middle::ty::TyCtxt;
use rustc_span::Span;
use rustc_utils::{BodyExt, SpanExt};
use std::collections::{BTreeMap, BTreeSet};

pub struct ArmSpec {
    /// The `SwitchInt` terminator.
    pub switch: Location,
    pub branch: usize,
    pub arm: usize,
    pub parent: Option<usize>,
    pub locs: BTreeSet<Location>,
    pub exits: Vec<Location>,
    pub col_lo: usize,
    pub col_hi: usize,
}

pub struct RowModel {
    /// Reverse postorder index of each reachable block (evaluation order).
    pub rpo: BTreeMap<BasicBlock, usize>,
    /// Row of each non-cleanup location, for states (rule 1).
    pub row: BTreeMap<Location, usize>,
    /// Row an event at the location is shown on (rule 4).
    pub event_row: BTreeMap<Location, usize>,
    /// The exit (its row and its own location) whose path alone runs the
    /// location (rule 4).
    pub early_exit: BTreeMap<Location, (usize, Location)>,
    /// Early exits (row, the exit's own location) that share the
    /// location with the normal path or with each other: the drop runs
    /// on each of those paths too.
    pub shared_exits: BTreeMap<Location, Vec<(usize, Location)>>,
    /// Source column (1-based) the location's span starts at.
    pub col: BTreeMap<Location, usize>,
    /// Per row: the locations its state is read after (rule 3).
    pub exits: BTreeMap<usize, Vec<Location>>,
    /// Per row: arms of one-line branches (rule 5).
    pub arms: BTreeMap<usize, Vec<ArmSpec>>,
}

pub struct Rows<'a, 'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub body: &'a Body<'tcx>,
}

impl<'a, 'tcx> Rows<'a, 'tcx> {
    pub fn span_of(&self, l: Location) -> Span {
        let sp = self.body.source_info(l).span;
        sp.as_local(self.body.span).unwrap_or(sp)
    }

    pub fn line_lo(&self, sp: Span) -> usize {
        self.tcx.sess.source_map().lookup_char_pos(sp.source_callsite().lo()).line
    }

    pub fn line_hi(&self, sp: Span) -> usize {
        self.tcx.sess.source_map().lookup_char_pos(sp.source_callsite().hi()).line
    }

    pub fn col_lo(&self, sp: Span) -> usize {
        self.tcx.sess.source_map().lookup_char_pos(sp.source_callsite().lo()).col.0 + 1
    }

    pub fn col_hi(&self, sp: Span) -> usize {
        self.tcx.sess.source_map().lookup_char_pos(sp.source_callsite().hi()).col.0 + 1
    }

    /// Rule 2, before rule 1 is applied.
    fn raw_row(&self, l: Location) -> usize {
        let sp = self.span_of(l);
        let data = &self.body.basic_blocks[l.block];
        // The drop of the old value before `x = <rhs>;` runs once the
        // right-hand side is done: on the assignment's row.
        if l.statement_index == data.statements.len() {
            if let TerminatorKind::Drop { place, target, .. } = &data.terminator().kind {
                let t = &self.body.basic_blocks[*target];
                if matches!(t.statements.first().map(|s| &s.kind), Some(StatementKind::Assign(asg)) if asg.0 == *place) {
                    return self.raw_row(Location { block: *target, statement_index: 0 });
                }
            }
        }
        // An assignment takes effect when its whole expression is done:
        // MIR gives `h = move tmp` (and the drop of the old `h` before it)
        // the span of `h`, but in `h = if c {..} else {..};` it runs
        // after the arms, on the statement's last row.
        if let Some(StatementKind::Assign(..)) = data.statements.get(l.statement_index).map(|s| &s.kind) {
            let hir_id = self.body.location_to_hir_id(l);
            if let hir::Node::Expr(e @ hir::Expr { kind: hir::ExprKind::Assign(..) | hir::ExprKind::AssignOp(..), .. }) = self.tcx.hir_node(hir_id) {
                if !e.span.from_expansion() {
                    return self.line_hi(e.span).max(self.line_lo(sp));
                }
            }
        }
        let desugared = self.body.source_info(l).span.desugaring_kind().is_some();
        let effect_at_end = !desugared
            && match data.statements.get(l.statement_index) {
                Some(st) => matches!(st.kind, StatementKind::Assign(..)),
                None => matches!(data.terminator().kind, TerminatorKind::Call { .. } | TerminatorKind::Drop { .. }),
            };
        if effect_at_end { self.line_hi(sp) } else { self.line_lo(sp) }
    }

    fn locations(&self, bb: BasicBlock) -> impl Iterator<Item = Location> {
        let n = self.body.basic_blocks[bb].statements.len();
        (0..=n).map(move |i| Location { block: bb, statement_index: i })
    }

    fn succs(&self, l: Location) -> Vec<Location> {
        let data = &self.body.basic_blocks[l.block];
        if l.statement_index < data.statements.len() {
            vec![Location { block: l.block, statement_index: l.statement_index + 1 }]
        } else {
            data.terminator()
                .successors()
                .filter(|s| !self.body.basic_blocks[*s].is_cleanup && !is_unreachable(self.body, *s))
                .map(|s| Location { block: s, statement_index: 0 })
                .collect()
        }
    }

    /// A location that only ends scopes: `StorageDead`, a `Drop`, a
    /// `Goto` or the `return` (what MIR runs on the way out of a scope).
    fn is_scope_end(&self, l: Location) -> bool {
        let data = &self.body.basic_blocks[l.block];
        match data.statements.get(l.statement_index) {
            Some(st) => matches!(st.kind, StatementKind::StorageDead(_) | StatementKind::Nop),
            None => matches!(data.terminator().kind, TerminatorKind::Drop { .. } | TerminatorKind::Goto { .. } | TerminatorKind::Return),
        }
    }

    /// The location is the jump of a `return`, `break`, `continue` or
    /// `?` (rule 4).
    /// Whether the location belongs to an early exit: `Some(true)` for
    /// one that leaves the function (`return`, `?`), `Some(false)` for a
    /// `break` / `continue`.
    fn exit_kind(&self, l: Location) -> Option<bool> {
        let sp = self.body.source_info(l).span;
        // (`?` desugars to `match Try::branch(x) { .., Break(r) => return
        // from_residual(r) }`: only the `return` arm exits, found below by
        // its `Ret` node; in a `try` block it is a `break` instead.)
        // The location computes a `return` / `break` value or is the jump:
        // its HIR node is, or is inside, the `return` / `break` /
        // `continue` expression (before any statement or block).
        let hir_id = self.body.location_to_hir_id(l);
        // Not the `break` a `for` / `while` desugars to: leaving the loop
        // that way is its normal end.
        let jump = |n: hir::Node<'_>| match n {
            hir::Node::Expr(hir::Expr { kind: hir::ExprKind::Ret(_), .. }) => Some(true),
            hir::Node::Expr(e @ hir::Expr { kind: hir::ExprKind::Break(..) | hir::ExprKind::Continue(_), .. }) => {
                (!matches!(e.span.desugaring_kind(), Some(rustc_span::DesugaringKind::ForLoop | rustc_span::DesugaringKind::WhileLoop)))
                    .then_some(false)
            }
            _ => None,
        };
        if let Some(k) = jump(self.tcx.hir_node(hir_id)) {
            return Some(k);
        }
        for (_, n) in self.tcx.hir_parent_iter(hir_id) {
            if let Some(k) = jump(n) {
                return Some(k);
            }
            if !matches!(n, hir::Node::Expr(_)) {
                break;
            }
            if let hir::Node::Expr(e) = n {
                if matches!(e.kind, hir::ExprKind::Block(..) | hir::ExprKind::Loop(..) | hir::ExprKind::Closure(..)) {
                    break;
                }
            }
        }
        if sp.from_expansion() {
            return None;
        }
        let text = self.tcx.sess.source_map().span_to_snippet(sp).unwrap_or_default();
        let kw = |k: &str| text.strip_prefix(k).is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'));
        if kw("return") {
            Some(true)
        } else if kw("break") || kw("continue") {
            Some(false)
        } else {
            None
        }
    }

    pub fn build(&self) -> RowModel {
        let body = self.body;
        let rpo_v: Vec<BasicBlock> = body.basic_blocks.reverse_postorder().to_vec();
        let rpo: BTreeMap<BasicBlock, usize> = rpo_v.iter().enumerate().map(|(i, b)| (*b, i)).collect();
        let preds = body.basic_blocks.predecessors();

        // Rule 1 and 2.
        let mut row: BTreeMap<Location, usize> = BTreeMap::new();
        // The floor each block passes on (the last row that counts in it).
        let mut floor_out: BTreeMap<BasicBlock, usize> = BTreeMap::new();
        for &bb in &rpo_v {
            let data = &body.basic_blocks[bb];
            if data.is_cleanup {
                continue;
            }
            let fwd: Vec<BasicBlock> = preds[bb]
                .iter()
                .copied()
                .filter(|p| !body.basic_blocks[*p].is_cleanup && rpo.get(p).is_some_and(|o| *o < rpo[&bb]))
                .collect();
            let mut floor = 0;
            if fwd.len() == 1 && preds[bb].len() == 1 {
                floor = floor_out.get(&fwd[0]).copied().unwrap_or(0);
            }
            for loc in self.locations(bb) {
                let r = self.raw_row(loc).max(floor);
                row.insert(loc, r);
                // A `StorageDead` / `StorageLive` does nothing a row shows
                // and may carry a later span than the code after it (the
                // let-else scrutinee's temporary dies at `};` before the
                // `else` block runs): it doesn't push what follows down.
                // Nor does the drop of a temporary (the let-else scrutinee's
                // `args()` is dropped at `};` before the `else` block runs).
                let data = &body.basic_blocks[bb];
                let storage = match data.statements.get(loc.statement_index) {
                    Some(st) => matches!(st.kind, StatementKind::StorageDead(_) | StatementKind::StorageLive(_)),
                    None => matches!(&data.terminator().kind, TerminatorKind::Drop { place, .. }
                        if place.projection.is_empty() && !body.local_decls[place.local].is_user_variable()
                            && place.local.index() > body.arg_count),
                };
                if !storage {
                    floor = r;
                }
            }
            floor_out.insert(bb, floor);
        }
        let col: BTreeMap<Location, usize> = row.keys().map(|l| (*l, self.col_lo(self.span_of(*l)))).collect();

        // Rule 4: which exit rows reach each scope-ending location. MIR
        // often merges an exit's jump into its block (`_0 = None;
        // StorageDead(..); drop(o)` for `return None`), so this works on
        // locations: a scope-ending one inherits the origins of the
        // locations just before it.
        const NORMAL: usize = usize::MAX;
        // Added to the row of a `break` / `continue` (it stays in the
        // function), to tell it from a `return` / `?`.
        const LOOP_EXIT: usize = 1 << 40;
        let mut origin: BTreeMap<Location, BTreeSet<usize>> = BTreeMap::new();
        // The exits' own locations, per scope-ending location.
        let mut origin_at: BTreeMap<Location, BTreeSet<(usize, Location)>> = BTreeMap::new();
        let mut changed = true;
        while changed {
            changed = false;
            for &bb in &rpo_v {
                if body.basic_blocks[bb].is_cleanup {
                    continue;
                }
                for loc in self.locations(bb) {
                    if !self.is_scope_end(loc) {
                        continue;
                    }
                    let before: Vec<Location> = if loc.statement_index > 0 {
                        vec![Location { block: bb, statement_index: loc.statement_index - 1 }]
                    } else {
                        preds[bb]
                            .iter()
                            .filter(|p| !body.basic_blocks[**p].is_cleanup)
                            .map(|p| Location { block: *p, statement_index: body.basic_blocks[*p].statements.len() })
                            .collect()
                    };
                    let mut set: BTreeSet<usize> = BTreeSet::new();
                    let mut at: BTreeSet<(usize, Location)> = BTreeSet::new();
                    for p in before {
                        if self.is_scope_end(p) {
                            if let Some(s) = origin.get(&p) {
                                set.extend(s.iter().copied());
                            }
                            if let Some(s) = origin_at.get(&p) {
                                at.extend(s.iter().copied());
                            }
                        }
                        if let Some(leaves) = self.exit_kind(p) {
                            if let Some(r) = row.get(&p) {
                                set.insert(if leaves { *r } else { *r + LOOP_EXIT });
                                at.insert((*r, p));
                            }
                        } else if !self.is_scope_end(p) {
                            set.insert(NORMAL);
                        }
                    }
                    if origin.get(&loc) != Some(&set) || origin_at.get(&loc) != Some(&at) {
                        origin.insert(loc, set);
                        origin_at.insert(loc, at);
                        changed = true;
                    }
                }
            }
        }
        let mut event_row = row.clone();
        let mut early_exit = BTreeMap::new();
        let mut shared_exits: BTreeMap<Location, Vec<(usize, Location)>> = BTreeMap::new();
        // Locations only a `return` / `?` reaches leave the function: their
        // state is not what the `}` row shows (`let ... else { return }`).
        // A `break`'s path goes on after the loop, so only its events move.
        let mut leaves_fn: BTreeSet<Location> = BTreeSet::new();
        for (loc, set) in &origin {
            let exits: Vec<usize> = set.iter().copied().filter(|r| *r != NORMAL).map(|r| r % LOOP_EXIT).collect();
            if exits.is_empty() {
                continue;
            }
            if set.contains(&NORMAL) || exits.len() > 1 {
                // One exit location per exit row (its first).
                let mut per_row: BTreeMap<usize, Location> = BTreeMap::new();
                for (r, l) in origin_at.get(loc).into_iter().flatten() {
                    per_row.entry(*r).or_insert(*l);
                }
                shared_exits.insert(*loc, per_row.into_iter().collect());
            } else if exits[0] < row[loc] {
                event_row.insert(*loc, exits[0]);
                // The exit's first location (`_0 = ..` of `return`, the
                // `break` value), not a jump with no span of its own.
                let at = origin_at.get(loc).and_then(|a| a.iter().next().map(|(_, l)| *l)).unwrap_or(*loc);
                early_exit.insert(*loc, (exits[0], at));
                if set.iter().all(|r| *r < LOOP_EXIT) {
                    leaves_fn.insert(*loc);
                }
            }
        }

        for l in &leaves_fn {
            row.insert(*l, early_exit[l].0);
        }

        // Rule 3.
        let mut row_exits_all: BTreeMap<usize, Vec<(Location, Vec<usize>)>> = BTreeMap::new();
        for (&loc, &r) in &row {
            let succ_rows: Vec<usize> = self.succs(loc).iter().filter_map(|s| row.get(s).copied()).collect();
            if succ_rows.is_empty() || succ_rows.iter().any(|x| *x != r) {
                row_exits_all.entry(r).or_default().push((loc, succ_rows));
            }
        }
        let mut exits: BTreeMap<usize, Vec<Location>> = BTreeMap::new();
        for (r, ex) in &row_exits_all {
            let nxt = ex.iter().flat_map(|(_, rs)| rs.iter().copied()).filter(|x| x > r).min();
            let keep: Vec<Location> = match nxt {
                Some(n) => ex.iter().filter(|(_, rs)| rs.contains(&n)).map(|(l, _)| *l).collect(),
                None => ex.iter().map(|(l, _)| *l).collect(),
            };
            exits.insert(*r, keep);
        }

        // Rule 5.
        let doms = body.basic_blocks.dominators();
        let mut arms: BTreeMap<usize, Vec<ArmSpec>> = BTreeMap::new();
        for (&loc, &r) in &row {
            let data = &body.basic_blocks[loc.block];
            if loc.statement_index != data.statements.len() {
                continue;
            }
            let TerminatorKind::SwitchInt { .. } = data.terminator().kind else { continue };
            let mut targets: Vec<BasicBlock> = Vec::new();
            for s in data.terminator().successors() {
                if !body.basic_blocks[s].is_cleanup && !targets.contains(&s) {
                    targets.push(s);
                }
            }
            if targets.len() < 2 {
                continue;
            }
            let sw_span = self.span_of(loc);
            let mut specs: Vec<ArmSpec> = Vec::new();
            for (i, t) in targets.iter().enumerate() {
                if preds[*t].len() != 1 {
                    continue;
                }
                let locs: BTreeSet<Location> = row
                    .iter()
                    .filter(|(l, lr)| **lr == r && doms.dominates(*t, l.block))
                    .map(|(l, _)| *l)
                    .collect();
                if locs.is_empty() {
                    continue;
                }
                let arm_exits: Vec<Location> = locs
                    .iter()
                    .copied()
                    .filter(|l| {
                        let ss = self.succs(*l);
                        ss.is_empty() || ss.iter().any(|s| !locs.contains(s))
                    })
                    .collect();
                // The arm's own code: spans on this row that don't cover
                // the whole branch (MIR gives some gotos the `if`'s span).
                let mut lo = usize::MAX;
                let mut hi = 0;
                for l in &locs {
                    let sp = self.span_of(*l);
                    if sp.contains(sw_span) || self.line_lo(sp) != r || self.line_hi(sp) != r {
                        continue;
                    }
                    lo = lo.min(self.col_lo(sp));
                    hi = hi.max(self.col_hi(sp));
                }
                if lo == usize::MAX {
                    lo = 0;
                }
                specs.push(ArmSpec { switch: loc, branch: 0, arm: i, parent: None, locs, exits: arm_exits, col_lo: lo, col_hi: hi });
            }
            if specs.len() >= 2 {
                arms.entry(r).or_default().extend(specs);
            }
        }
        for specs in arms.values_mut() {
            // Branches in evaluation order; nested ones get their parent.
            specs.sort_by_key(|s| (rpo.get(&s.switch.block).copied().unwrap_or(usize::MAX), s.arm));
            let mut branch_of: Vec<Location> = Vec::new();
            for s in specs.iter_mut() {
                if !branch_of.contains(&s.switch) {
                    branch_of.push(s.switch);
                }
                s.branch = branch_of.iter().position(|x| *x == s.switch).unwrap();
            }
            let n = specs.len();
            for i in 0..n {
                let sw = specs[i].switch;
                // The innermost other arm containing this branch's switch.
                let parent = (0..n)
                    .filter(|&j| specs[j].switch != sw && specs[j].locs.contains(&sw))
                    .min_by_key(|&j| specs[j].locs.len());
                specs[i].parent = parent;
            }
        }

        RowModel { rpo, row, event_row, early_exit, shared_exits, col, exits, arms }
    }
}

/// A block MIR marks unreachable (the fall-through of a slice pattern's
/// length test): no path runs it.
pub fn is_unreachable(body: &Body<'_>, bb: BasicBlock) -> bool {
    let data = &body.basic_blocks[bb];
    matches!(data.terminator().kind, TerminatorKind::Unreachable)
        && data.statements.iter().all(|s| {
            matches!(
                s.kind,
                StatementKind::StorageDead(_) | StatementKind::StorageLive(_) | StatementKind::Nop | StatementKind::FakeRead(..) | StatementKind::PlaceMention(..)
            )
        })
}

impl RowModel {
    /// The innermost arm (index into the row's arms) a location is in.
    pub fn arm_of(&self, l: Location) -> Option<usize> {
        let r = self.row.get(&l)?;
        let specs = self.arms.get(r)?;
        specs.iter().enumerate().filter(|(_, s)| s.locs.contains(&l)).min_by_key(|(_, s)| s.locs.len()).map(|(i, _)| i)
    }

    /// Evaluation order of a location: (reverse postorder of its block,
    /// statement index).
    pub fn order(&self, l: Location) -> (usize, usize, u8) {
        (self.rpo.get(&l.block).copied().unwrap_or(usize::MAX), l.statement_index, 0)
    }
}
