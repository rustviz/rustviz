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
    /// Locations only early exits reach (several of them): no event of
    /// their own on the `}` row, only each exit's.
    pub exit_only: BTreeSet<Location>,
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
        // A `for` loop's own drops on its way out (its hidden iterator, its
        // last `next()` value): the desugaring spans them at the loop's `}`,
        // whose call site is the whole loop.
        let drop = l.statement_index == data.statements.len() && matches!(data.terminator().kind, TerminatorKind::Drop { .. });
        if drop && sp.desugaring_kind() == Some(rustc_span::DesugaringKind::ForLoop) {
            return self.tcx.sess.source_map().lookup_char_pos(sp.hi()).line;
        }
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
        // A binding a `let .. else { .. };` makes (`x2 = (o2 as Some).0`)
        // takes effect when the statement is done, past its `else` block: on
        // its `};` row, which so shows the path that goes on (rule 4).
        if let Some(StatementKind::Assign(asg)) = data.statements.get(l.statement_index).map(|s| &s.kind) {
            let hir_id = self.body.location_to_hir_id(l);
            let binding = asg.0.projection.is_empty() && self.body.local_decls[asg.0.local].is_user_variable();
            if binding && matches!(self.tcx.hir_node(hir_id), hir::Node::LetStmt(ls) if ls.els.is_some() && !ls.span.from_expansion()) {
                if let hir::Node::LetStmt(ls) = self.tcx.hir_node(hir_id) {
                    return self.line_hi(ls.span).max(self.line_lo(sp));
                }
            }
            if binding && matches!(self.tcx.hir_node(hir_id), hir::Node::Pat(_)) {
                for (_, n) in self.tcx.hir_parent_iter(hir_id).take(16) {
                    match n {
                        hir::Node::LetStmt(ls) if ls.els.is_some() && !ls.span.from_expansion() => {
                            return self.line_hi(ls.span).max(self.line_lo(sp));
                        }
                        hir::Node::Pat(_) | hir::Node::PatField(_) | hir::Node::PatExpr(_) => {}
                        _ => break,
                    }
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
        // A closure's captures are borrowed where its expression starts (its
        // `|..|` head): its body's rows don't run here. (Its value is made at
        // the expression's end, like any multi-row assignment.)
        if self.is_capture(l) {
            return self.line_lo(sp);
        }
        let desugared = self.body.source_info(l).span.desugaring_kind().is_some();
        let effect_at_end = !desugared
            && match data.statements.get(l.statement_index) {
                Some(st) => matches!(st.kind, StatementKind::Assign(..)),
                None => matches!(data.terminator().kind, TerminatorKind::Call { .. } | TerminatorKind::Drop { .. }),
            };
        if effect_at_end { self.line_hi(sp) } else { self.line_lo(sp) }
    }

    /// The construct an early exit at `l` leaves: the loop or block a
    /// `break` / `continue` (or a `?` in a `try` block) targets, else the
    /// whole body.
    fn exit_scope(&self, l: Location) -> Span {
        let hir_id = self.body.location_to_hir_id(l);
        let nodes = std::iter::once(self.tcx.hir_node(hir_id)).chain(self.tcx.hir_parent_iter(hir_id).map(|(_, n)| n));
        for n in nodes.take(12) {
            match n {
                hir::Node::Expr(hir::Expr { kind: hir::ExprKind::Break(dest, _) | hir::ExprKind::Continue(dest), .. }) => {
                    if let Ok(target) = dest.target_id {
                        let sp = self.tcx.hir_span(target);
                        return sp.as_local(self.body.span).unwrap_or(sp);
                    }
                    break;
                }
                hir::Node::Expr(hir::Expr { kind: hir::ExprKind::Ret(_) | hir::ExprKind::Closure(..), .. }) => break,
                hir::Node::Expr(_) | hir::Node::Arm(_) | hir::Node::Stmt(_) | hir::Node::LetStmt(_) | hir::Node::Block(_) => {}
                _ => break,
            }
        }
        self.body.span
    }

    /// A closure's capture borrow (`_4 = &mut _2` for the closure
    /// expression, before its aggregate).
    fn is_capture(&self, l: Location) -> bool {
        let data = &self.body.basic_blocks[l.block];
        let Some(StatementKind::Assign(asg)) = data.statements.get(l.statement_index).map(|s| &s.kind) else { return false };
        matches!(asg.1, rustc_middle::mir::Rvalue::Ref(..))
            && matches!(self.tcx.hir_node(self.body.location_to_hir_id(l)), hir::Node::Expr(hir::Expr { kind: hir::ExprKind::Closure(..), .. }))
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

    /// The successors a path can run: `succs` without borrowck's imaginary
    /// `FalseEdge` targets.
    fn real_succs(&self, l: Location) -> Vec<Location> {
        let data = &self.body.basic_blocks[l.block];
        let imaginary = match &data.terminator().kind {
            TerminatorKind::FalseEdge { imaginary_target, real_target } if l.statement_index == data.statements.len() && imaginary_target != real_target => Some(*imaginary_target),
            _ => None,
        };
        self.succs(l).into_iter().filter(|s| Some(s.block) != imaginary).collect()
    }

    /// A location that shows nothing of its own on a row: a `StorageDead`
    /// / `StorageLive`, or the drop of a temporary.
    pub fn is_quiet(&self, l: Location) -> bool {
        let body = self.body;
        let data = &body.basic_blocks[l.block];
        match data.statements.get(l.statement_index) {
            Some(st) => matches!(st.kind, StatementKind::StorageDead(_) | StatementKind::StorageLive(_) | StatementKind::Nop),
            None => matches!(&data.terminator().kind, TerminatorKind::Drop { place, .. }
                if place.projection.is_empty() && !body.local_decls[place.local].is_user_variable()
                    && place.local.index() > body.arg_count),
        }
    }

    /// A location that does nothing on any row: the unit value of a block
    /// stored in a MIR temporary (`_5 = const ()` of an empty `{ }` arm,
    /// spanned over the arm's braces). The path's next row is past it.
    fn is_silent(&self, l: Location) -> bool {
        let body = self.body;
        match body.basic_blocks[l.block].statements.get(l.statement_index).map(|s| &s.kind) {
            Some(StatementKind::Assign(asg)) => {
                let (lhs, rv) = &**asg;
                // (Not the return place: `_0 = const ()` is a `return;`'s
                // own code, on its row.)
                lhs.projection.is_empty()
                    && lhs.local != rustc_middle::mir::RETURN_PLACE
                    && !body.local_decls[lhs.local].is_user_variable()
                    && matches!(rv, rustc_middle::mir::Rvalue::Use(rustc_middle::mir::Operand::Constant(c)) if c.ty().is_unit())
            }
            _ => false,
        }
    }

    /// A plain jump (`goto`; MIR gives it the span of the whole branch or
    /// loop it leaves, or a row its floor pushed it to): no row of its own.
    fn is_jump(&self, l: Location) -> bool {
        let data = &self.body.basic_blocks[l.block];
        l.statement_index == data.statements.len()
            && matches!(data.terminator().kind, TerminatorKind::Goto { .. } | TerminatorKind::FalseEdge { .. } | TerminatorKind::FalseUnwind { .. })
    }

    /// The next row of the path from `start` (a successor of a location
    /// on row `from`): the first row other than `from` its code reaches,
    /// past quiet locations (temporaries' drops, `StorageDead`s); `from`
    /// if the path never leaves it.
    pub fn next_row(&self, rows: &RowModel, start: Location, from: usize) -> usize {
        // A quiet location counts unless it is out of order: the let-else
        // scrutinee's temporaries die at `};` before the `else` block's
        // first row runs; a `} else {` row's StorageDead is in order.
        let any = self.next_row_by(rows, start, from, false);
        let loud = self.next_row_by(rows, start, from, true);
        if any != from && (any <= loud || loud == from) { any } else { loud }
    }

    fn next_row_by(&self, rows: &RowModel, start: Location, from: usize, skip_quiet: bool) -> usize {
        let mut seen: BTreeSet<Location> = BTreeSet::new();
        let mut work = vec![start];
        let mut best: Option<usize> = None;
        while let Some(l) = work.pop() {
            if !seen.insert(l) || seen.len() > 4096 {
                continue;
            }
            let Some(&r) = rows.row.get(&l) else { continue };
            // An empty arm's unit value is where its block starts: the arm's
            // own row (`None => {}`), or the branch's row itself (`if a {`
            // with its `}` on the `} else if b {` row: the path goes on to
            // where the branch rejoins).
            let silent = self.is_silent(l);
            let r = if silent { self.line_lo(self.span_of(l)) } else { r };
            let counts = if silent { r > from } else { r != from && !self.is_jump(l) && !(skip_quiet && self.is_quiet(l)) };
            if counts {
                best = Some(best.map_or(r, |b: usize| if (r > from) == (b > from) { b.min(r) } else if r > from { r } else { b }));
                continue;
            }
            work.extend(self.real_succs(l));
        }
        best.unwrap_or(from)
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
        // `?` desugars to `match Try::branch(x) { .., Break(r) => return
        // from_residual(r) }` (in a `try` block, `break 'try
        // <residual>(r)`, with `residual_into_try_type` or, for a typed
        // block, `from_residual`): only that arm exits (the `branch` call
        // runs on both paths). Whether it leaves the function or the block
        // is the HIR's `return` or `break` around the call (the value's
        // destination can't tell: a `try` block in tail position writes the
        // return place too).
        if sp.desugaring_kind() == Some(rustc_span::DesugaringKind::QuestionMark) {
            let data = &self.body.basic_blocks[l.block];
            let jump = || {
                let hir_id = self.body.location_to_hir_id(l);
                let nodes = std::iter::once(self.tcx.hir_node(hir_id)).chain(self.tcx.hir_parent_iter(hir_id).map(|(_, n)| n));
                for n in nodes.take(8) {
                    match n {
                        hir::Node::Expr(hir::Expr { kind: hir::ExprKind::Ret(_), .. }) => return Some(true),
                        hir::Node::Expr(hir::Expr { kind: hir::ExprKind::Break(..), .. }) => return Some(false),
                        hir::Node::Expr(hir::Expr { kind: hir::ExprKind::Match(..) | hir::ExprKind::Block(..) | hir::ExprKind::Closure(..), .. }) => return None,
                        hir::Node::Expr(_) | hir::Node::Arm(_) => {}
                        _ => return None,
                    }
                }
                None
            };
            return match data.statements.get(l.statement_index) {
                Some(st) => match &st.kind {
                    StatementKind::Assign(asg) if asg.0.local == rustc_middle::mir::RETURN_PLACE => Some(jump().unwrap_or(true)),
                    _ => None,
                },
                None => match &data.terminator().kind {
                    TerminatorKind::Call { func, destination, .. }
                        if func.const_fn_def().is_some_and(|(d, _)| {
                            matches!(self.tcx.item_name(d).as_str(), "from_residual" | "residual_into_try_type")
                        }) =>
                    {
                        Some(jump().unwrap_or(destination.local == rustc_middle::mir::RETURN_PLACE))
                    }
                    _ => None,
                },
            };
        }
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
                // (A capture borrow is on the closure's first row even when the
                // closure's own `FakeRead`s of matched places, spanned inside
                // its body, run just before it.)
                let r = if self.is_capture(loc) { self.raw_row(loc) } else { self.raw_row(loc).max(floor) };
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
        // Rule 1 from below: a temporary's drop or storage end whose span is
        // a later row than the code it runs before (a failing `let ... else`
        // drops its scrutinee's temporaries, spanned at `};`, before the
        // `else` block runs) happens on the row of the code that ran just
        // before it, when every real way in agrees on that row.
        let temp_quiet = |l: Location| {
            let data = &body.basic_blocks[l.block];
            let temp = |local: rustc_middle::mir::Local| !body.local_decls[local].is_user_variable() && local.index() > body.arg_count;
            match data.statements.get(l.statement_index) {
                Some(st) => matches!(st.kind, StatementKind::StorageDead(x) | StatementKind::StorageLive(x) if temp(x)),
                None => matches!(&data.terminator().kind, TerminatorKind::Drop { place, .. } if place.projection.is_empty() && temp(place.local)),
            }
        };
        let real_preds = |bb: BasicBlock| -> Vec<BasicBlock> {
            preds[bb]
                .iter()
                .copied()
                .filter(|p| {
                    !body.basic_blocks[*p].is_cleanup
                        && !matches!(&body.basic_blocks[*p].terminator().kind,
                            TerminatorKind::FalseEdge { imaginary_target, real_target } if *imaginary_target == bb && *real_target != bb)
                })
                .collect()
        };
        // Locations that do nothing on a row: storage, unit values, jumps
        // (a `goto` carries the span of the whole branch or loop).
        let passes = |l: Location| {
            temp_quiet(l)
                || self.is_silent(l)
                || self.is_quiet(l)
                || self.is_jump(l)
        };
        let mut lowered: BTreeMap<Location, usize> = BTreeMap::new();
        for &bb in &rpo_v {
            if body.basic_blocks[bb].is_cleanup {
                continue;
            }
            for l in self.locations(bb) {
                if !temp_quiet(l) {
                    continue;
                }
                // The next rows this location's code goes on to, on the paths
                // that run (no imaginary edges) and go forward (a loop's back
                // edge goes up by design: it tells nothing). A search that
                // can't finish decides nothing.
                let mut next: Option<usize> = None;
                let mut complete = true;
                let mut seen: BTreeSet<Location> = BTreeSet::new();
                let mut work: Vec<(Location, Location)> = self.real_succs(l).into_iter().map(|s| (l, s)).collect();
                while let Some((from, s)) = work.pop() {
                    if s.block != from.block && rpo.get(&s.block).is_none_or(|o| *o <= rpo[&from.block]) {
                        continue;
                    }
                    if !seen.insert(s) {
                        continue;
                    }
                    if seen.len() > 4096 {
                        complete = false;
                        break;
                    }
                    if passes(s) {
                        work.extend(self.real_succs(s).into_iter().map(|t| (s, t)));
                        continue;
                    }
                    if let Some(r) = row.get(&s) {
                        next = Some(next.map_or(*r, |n| n.max(*r)));
                    }
                }
                let Some(next) = next.filter(|_| complete) else { continue };
                if next >= row[&l] {
                    continue;
                }
                // The row of the code before it, on every real way in (not a
                // loop's back edge).
                let mut before: BTreeSet<usize> = BTreeSet::new();
                let mut seen: BTreeSet<Location> = BTreeSet::new();
                let mut work = vec![l];
                while let Some(x) = work.pop() {
                    if !seen.insert(x) {
                        continue;
                    }
                    if seen.len() > 4096 {
                        complete = false;
                        break;
                    }
                    let prev: Vec<Location> = if x.statement_index > 0 {
                        vec![Location { block: x.block, statement_index: x.statement_index - 1 }]
                    } else {
                        real_preds(x.block)
                            .into_iter()
                            .filter(|p| rpo.get(p).is_some_and(|o| *o < rpo[&x.block]))
                            .map(|p| Location { block: p, statement_index: body.basic_blocks[p].statements.len() })
                            .collect()
                    };
                    for p in prev {
                        if passes(p) {
                            match lowered.get(&p) {
                                Some(r) => {
                                    before.insert(*r);
                                }
                                None => work.push(p),
                            }
                        } else if let Some(r) = row.get(&p) {
                            before.insert(*r);
                        }
                    }
                }
                if !complete {
                    continue;
                }
                if before.len() == 1 {
                    let b = *before.iter().next().unwrap();
                    if b <= next && b < row[&l] {
                        lowered.insert(l, b);
                    }
                }
            }
        }
        row.extend(lowered);
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
                            // An exit's path runs this scope end as the exit's
                            // own only inside the construct it leaves (a `break`'s
                            // loop, a `try` block a `?` leaves): past it, the
                            // path goes on like the normal one (the fn's `}`
                            // after a `try` block in tail position). A jump with
                            // no span of its own passes it on.
                            let here = self.span_of(loc);
                            let inherited: Vec<(usize, Location)> = origin_at.get(&p).into_iter().flatten().copied().collect();
                            let kept: Vec<(usize, Location)> =
                                inherited.iter().copied().filter(|(_, x)| here.is_dummy() || self.exit_scope(*x).contains(here)).collect();
                            if kept.len() == inherited.len() {
                                if let Some(s) = origin.get(&p) {
                                    set.extend(s.iter().copied());
                                }
                            } else {
                                if origin.get(&p).is_some_and(|s| s.contains(&NORMAL)) || kept.len() < inherited.len() {
                                    set.insert(NORMAL);
                                }
                                for (r, x) in &kept {
                                    set.insert(if self.exit_kind(*x) == Some(false) { *r + LOOP_EXIT } else { *r });
                                }
                            }
                            at.extend(kept);
                        }
                        if let Some(leaves) = self.exit_kind(p) {
                            if let Some(r) = row.get(&p) {
                                set.insert(if leaves { *r } else { *r + LOOP_EXIT });
                                // One location per exit: its first (`_0 = ..`
                                // of `return 0`, not also the jump after it).
                                let inherited = self.is_scope_end(p) && origin_at.get(&p).is_some_and(|a| !a.is_empty());
                                if !inherited {
                                    at.insert((*r, p));
                                }
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
        let mut exit_only: BTreeSet<Location> = BTreeSet::new();
        // Locations only (several) `return`s / `?`s reach.
        let mut leaves_fn_shared: BTreeSet<Location> = BTreeSet::new();
        for (loc, set) in &origin {
            let exits: Vec<usize> = set.iter().copied().filter(|r| *r != NORMAL).map(|r| r % LOOP_EXIT).collect();
            if exits.is_empty() {
                continue;
            }
            let all_at: Vec<(usize, Location)> = origin_at.get(loc).into_iter().flatten().copied().collect();
            if set.contains(&NORMAL) || all_at.len() > 1 {
                // Every exit that runs it (two returns on one row are two).
                shared_exits.insert(*loc, all_at.clone());
                if !set.contains(&NORMAL) {
                    // Only exits run it: no path reaches it otherwise. Its
                    // events go on the exits' rows (`exit_only`); its state
                    // row stays (moving it onto one exit's row would put
                    // another exit's path into that row's state).
                    let last = exits.iter().copied().max().unwrap_or(row[loc]);
                    exit_only.insert(*loc);
                    if set.iter().all(|r| *r < LOOP_EXIT) {
                        leaves_fn_shared.insert(*loc);
                    }
                    if last < row[loc] {
                        event_row.insert(*loc, last);
                    }
                }
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
            // Not into code only a `break` / `continue` runs (its drops keep
            // the `}` row for states, rule 4): that path leaves the row's
            // scope, it doesn't go on to the next row (`if c { break l.name; }`).
            let succ_rows: Vec<usize> = self.succs(loc).iter()
                .filter(|s| early_exit.contains_key(&loc) || !early_exit.contains_key(*s))
                .filter_map(|s| row.get(s).copied())
                .collect();
            if succ_rows.is_empty() || succ_rows.iter().any(|x| *x != r) {
                row_exits_all.entry(r).or_default().push((loc, succ_rows));
            }
        }
        // A path that leaves the function on this row (`let ... else {
        // return; };` on one line) has no next row: the state after the row
        // is the continuing paths' (Q3), when there are any.
        // (Drops several returns share count too.)
        let leaves = |l: &Location| leaves_fn.contains(l) || leaves_fn_shared.contains(l);
        for ex in row_exits_all.values_mut() {
            if ex.iter().any(|(l, _)| !leaves(l)) {
                ex.retain(|(l, _)| !leaves(l));
            }
        }
        let mut exits: BTreeMap<usize, Vec<Location>> = BTreeMap::new();
        for (r, ex) in &row_exits_all {
            let nxt = ex.iter().flat_map(|(_, rs)| rs.iter().copied()).filter(|x| x > r).min();
            let keep: Vec<Location> = match nxt {
                Some(n) => ex.iter().filter(|(_, rs)| rs.contains(&n)).map(|(l, _)| *l).collect(),
                // Every exit goes back up from a row with nothing but the
                // loop body's closing `}` on it (its `StorageDead`s and the
                // jump back): what the next row sees is the loop's way
                // out, reached from here (`s` may be unassigned after `for
                // _ in v { s = .. }`, #1789). A row of code in the body
                // keeps the back edge's state: the next iteration is its
                // next row.
                None if row.iter().filter(|(_, rr)| *rr == r).all(|(l, _)| {
                    let data = &self.body.basic_blocks[l.block];
                    self.is_quiet(*l)
                        || match data.statements.get(l.statement_index).map(|s| &s.kind) {
                            // (`_ = ()`: the loop's own value.)
                            Some(StatementKind::Assign(asg)) => {
                                matches!(&asg.1, rustc_middle::mir::Rvalue::Use(rustc_middle::mir::Operand::Constant(c)) if c.ty().is_unit())
                            }
                            Some(_) => false,
                            None => matches!(data.terminator().kind, TerminatorKind::Goto { .. } | TerminatorKind::FalseUnwind { .. }),
                        }
                }) => {
                    let mut seen: BTreeSet<Location> = BTreeSet::new();
                    let mut work: Vec<Location> = ex.iter().map(|(l, _)| *l).collect();
                    let mut out: Vec<(usize, Location)> = Vec::new();
                    while let Some(l) = work.pop() {
                        if !seen.insert(l) || seen.len() > 4096 {
                            continue;
                        }
                        for sc in self.succs(l) {
                            match row.get(&sc) {
                                Some(sr) if sr > r => out.push((*sr, l)),
                                Some(_) => work.push(sc),
                                None => {}
                            }
                        }
                    }
                    match out.iter().map(|(sr, _)| *sr).min() {
                        Some(n) => out.iter().filter(|(sr, _)| *sr == n).map(|(_, l)| *l).collect(),
                        None => ex.iter().map(|(l, _)| *l).collect(),
                    }
                }
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
                if !body.basic_blocks[s].is_cleanup && !is_unreachable(body, s) && !targets.contains(&s) {
                    targets.push(s);
                }
            }
            if targets.len() < 2 {
                continue;
            }
            let sw_span = self.span_of(loc);
            let mut specs: Vec<ArmSpec> = Vec::new();
            for (i, t) in targets.iter().enumerate() {
                // Entered from this switch only (an earlier arm's imaginary
                // `FalseEdge` into a match arm doesn't run).
                let real_preds = preds[*t]
                    .iter()
                    .filter(|p| !matches!(&body.basic_blocks[**p].terminator().kind,
                        TerminatorKind::FalseEdge { imaginary_target, .. } if imaginary_target == t))
                    .count();
                if real_preds != 1 {
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

        RowModel { rpo, row, event_row, early_exit, shared_exits, exit_only, col, exits, arms }
    }
}

/// A block from which every path ends in MIR's `unreachable` (the
/// fall-through of a slice pattern's length test, a match's impossible
/// `otherwise`), with nothing on the way but bookkeeping: no path runs it.
pub fn is_unreachable(body: &Body<'_>, bb: BasicBlock) -> bool {
    fn dead(body: &Body<'_>, bb: BasicBlock, depth: usize) -> bool {
        let data = &body.basic_blocks[bb];
        let quiet = data.statements.iter().all(|s| {
            matches!(
                s.kind,
                StatementKind::StorageDead(_) | StatementKind::StorageLive(_) | StatementKind::Nop | StatementKind::FakeRead(..) | StatementKind::PlaceMention(..)
            )
        });
        if !quiet || depth > 8 {
            return false;
        }
        match &data.terminator().kind {
            TerminatorKind::Unreachable => true,
            TerminatorKind::Goto { target } => dead(body, *target, depth + 1),
            TerminatorKind::FalseEdge { real_target, .. } => dead(body, *real_target, depth + 1),
            _ => false,
        }
    }
    dead(body, bb, 0)
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
