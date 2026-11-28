//! Ideally we would like to do all of our analysis in the MIR since it is a lot simpler than the HIR
//! since events like drops, borrows and moves are stated explicitly. Additionally, the MIR is less complicated.
//! If you'd like to work on converting more and more of the RustViz logic from the HIR to the MIR I would recommend reading:
//! https://rustc-dev-guide.rust-lang.org/ especially the sections on the MIR, dataflow analysis, borrow checker, drop elaboration
//! Also read through this: https://github.com/rust-lang/polonius/blob/master/book/src/SUMMARY.md
//! Unfortunately though, I don't believe it would be possible to do everything in the MIR, for example: how would 
//! you figure out when a copy occurs? You can't use the assignment operator - since a single HIR instr can be compiled down into
//! multiple MIR instructions.

use crate::{expr_visitor::*, expr_visitor_utils::{span_end_line, span_to_line}};
use log::info;
use rustc_middle::mir::{BorrowKind, Location, Local, Operand, Place, PlaceElem, ProjectionElem, Rvalue, StatementKind, TerminatorKind};
use rustc_middle::mir::visit::{PlaceContext, Visitor};
use rustc_middle::ty::{GenericArgKind, RegionVid, Ty, TyKind};
use rustc_hir::intravisit;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use rustc_utils::mir::{body::BodyExt, place::PlaceExt};
use rustc_utils::SpanExt;
use rustc_borrowck::consumers::{BodyWithBorrowckFacts, BorrowIndex, Borrows, RustcFacts};
use rustc_mir_dataflow::Analysis;
use rustc_span::Span;
use polonius_engine::{Algorithm, Output, FactTypes};
type Loan = <RustcFacts as FactTypes>::Loan;
type Point = <RustcFacts as FactTypes>::Point;
type Origin = <RustcFacts as FactTypes>::Origin;

/// Why a write to a lender is allowed at a line where a loan of it is
/// drawn live (#796): on the write's path the loan is already dead.
#[derive(Clone, Debug)]
pub struct DeadLoanWrite {
    pub location: Location,
    /// RAP name of the reference that held the loan.
    pub borrower: String,
    /// Where `borrower` is given a new reference on the write's path,
    /// and whether that is before the write; None when it isn't used
    /// again on that path.
    pub repoint: Option<(usize, bool)>,
}

/// Finds a use of `local` (not a `StorageLive` / `StorageDead`).
struct LocalUse { local: Local, found: bool }

impl<'tcx> Visitor<'tcx> for LocalUse {
    fn visit_local(&mut self, local: Local, ctx: PlaceContext, _loc: Location) {
        if local == self.local && !matches!(ctx, PlaceContext::NonUse(_)) {
            self.found = true;
        }
    }
}

/// Whether places with projections `a` and `b` of the same local can
/// overlap: not when they name different fields at the same depth.
fn projections_overlap(a: &[PlaceElem<'_>], b: &[PlaceElem<'_>]) -> bool {
    !a.iter().zip(b).any(|(x, y)| matches!((x, y), (ProjectionElem::Field(f, _), ProjectionElem::Field(g, _)) if f != g))
}

/// A loan a call left in a variable (see `loans_kept_after_call`).
#[derive(Clone, Debug)]
pub struct KeptLoan {
    /// Index of the loan in rustc's borrow set (`MIRBorrowData::borrow_idx`).
    pub loan: usize,
    /// RAP name of the variable that holds the loan once the call returns.
    pub holder: String,
    /// RAP name of the variable borrowed; with `through_ref`, the
    /// reference variable whose referent (`*r`) is reborrowed.
    pub lender: String,
    pub through_ref: bool,
    pub is_mut: bool,
    /// Last line the loan is live on (`MIRBorrowData::region.1`).
    pub end_line: usize,
    /// Source span of the borrow that issued the loan (for a closure's
    /// capture, the closure literal).
    pub span: Span,
}

pub struct MIRBorrowData {
    pub borrow_idx: usize,
    pub region: (usize, usize),
    pub borrowed_place: Option<String>,
    pub assigned_place: Option<String>,
    /// Source span of the borrow expression (`&x`) this loan was issued
    /// at; see `RefData::borrow_span`.
    pub span: Span,
    // could add more useful members if necessary like kind of mutable borrow, regionvid, etc...

}


/// Finds a move of a field out of `*local` (`move ((*_1).0)`).
struct BoxFieldMoves { local: Local, found: bool }

impl<'tcx> Visitor<'tcx> for BoxFieldMoves {
    fn visit_operand(&mut self, op: &Operand<'tcx>, loc: Location) {
        if let Operand::Move(place) = op {
            if place.local == self.local
                && matches!(place.projection.first(), Some(ProjectionElem::Deref))
                && matches!(place.projection.get(1), Some(ProjectionElem::Field(..)))
            {
                self.found = true;
            }
        }
        self.super_operand(op, loc);
    }
}

/// Finds a move of the whole contents of `*local` (`move (*_1)`).
struct BoxContentsMoves { local: Local, found: bool }

impl<'tcx> Visitor<'tcx> for BoxContentsMoves {
    fn visit_operand(&mut self, op: &Operand<'tcx>, loc: Location) {
        if let Operand::Move(place) = op {
            if place.local == self.local && matches!(place.projection.as_slice(), [ProjectionElem::Deref]) {
                self.found = true;
            }
        }
        self.super_operand(op, loc);
    }
}

/// A constant projection of an array operand. A subslice may be the last step.
struct ArrayStep {
    indices: std::ops::Range<u64>,
    len: u64,
    slice: bool,
}

fn array_steps<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, body: &rustc_middle::mir::Body<'tcx>, p: Place<'tcx>, prefix: usize) -> Option<Vec<ArrayStep>> {
    let mut steps = Vec::new();
    for (base, elem) in p.iter_projections().skip(prefix) {
        // Moving the sole field of an enum variant takes the resource in
        // that array element (e.g. [Some(s), _]), as the element tooltip says.
        if let [ProjectionElem::Downcast(_, variant), ProjectionElem::Field(field, _)] = &p.projection[base.projection.len()..] {
            if base.ty(&body.local_decls, tcx).ty.ty_adt_def().is_some_and(|d|
                d.is_enum() && d.variant(*variant).fields.len() == 1 && field.index() == 0) { break; }
        }
        let TyKind::Array(_, n) = base.ty(&body.local_decls, tcx).ty.kind() else { return None };
        let len = n.try_to_target_usize(tcx)?;
        let index = |n, from_end| if from_end { len.checked_sub(n) } else { Some(n) };
        let (indices, slice) = match elem {
            ProjectionElem::ConstantIndex { offset, from_end, .. } => {
                let i = index(offset, from_end)?;
                (i..i + 1, false)
            }
            ProjectionElem::Subslice { from, to, from_end } if base.projection.len() + 1 == p.projection.len() =>
                (from..index(to, from_end)?, true),
            _ => return None,
        };
        steps.push(ArrayStep { indices, len, slice });
    }
    (!steps.is_empty()).then_some(steps)
}

pub(crate) fn array_operand_name<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, body: &rustc_middle::mir::Body<'tcx>, p: Place<'tcx>, prefix: usize, mut name: String) -> Option<String> {
    use std::fmt::Write;
    for step in array_steps(tcx, body, p, prefix)? {
        if step.slice { write!(name, "[{}..{}]", step.indices.start, step.indices.end).ok()?; }
        else { write!(name, "[{}]", step.indices.start).ok()?; }
    }
    Some(name)
}

struct ArrayElemMoves<'a, 'tcx> {
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    body: &'a rustc_middle::mir::Body<'tcx>,
    local: Local,
    /// Length of each array whose immediate elements a move distinguishes.
    lengths: BTreeMap<Vec<u64>, u64>,
    moved: BTreeSet<Vec<u64>>,
    payloads: BTreeSet<Vec<u64>>,
    whole_slots: BTreeSet<Vec<u64>>,
    whole: bool,
    assigns: usize,
    at: Vec<(Location, Vec<u64>)>,
}

impl<'tcx> Visitor<'tcx> for ArrayElemMoves<'_, 'tcx> {
    fn visit_operand(&mut self, op: &Operand<'tcx>, loc: Location) {
        if let Operand::Move(p) = op {
            if p.local != self.local { return; }
            if let Some(steps) = array_steps(self.tcx, self.body, *p, 0) {
                let mut prefix = Vec::new();
                for (depth, step) in steps.iter().enumerate() {
                    self.lengths.insert(prefix.clone(), step.len);
                    if depth + 1 == steps.len() {
                        for i in step.indices.clone() {
                            let mut path = prefix.clone();
                            path.push(i);
                            if steps.len() < p.projection.len() { self.payloads.insert(path.clone()); }
                            else { self.whole_slots.insert(path.clone()); }
                            self.moved.insert(path.clone());
                            self.at.push((loc, path));
                        }
                    } else {
                        prefix.push(step.indices.start);
                    }
                }
            } else {
                self.whole = true;
            }
        }
        self.super_operand(op, loc);
    }
    fn visit_assign(&mut self, place: &Place<'tcx>, rv: &Rvalue<'tcx>, loc: Location) {
        if place.local == self.local && place.projection.is_empty() {
            self.assigns += 1;
        }
        self.super_assign(place, rv, loc);
    }
}

/// Whether the temporary `local` is used only to reborrow through it
/// what the reference it points to points to (`&(*(*_4))`, a place
/// starting with two derefs), apart from being assigned.
struct OnlyReborrowedThrough { local: Local, reborrowed: bool, other: bool }

impl<'tcx> Visitor<'tcx> for OnlyReborrowedThrough {
    fn visit_rvalue(&mut self, rv: &Rvalue<'tcx>, loc: Location) {
        if let Rvalue::Ref(_, _, place) = rv {
            if place.local == self.local && matches!(place.projection.as_slice(), [ProjectionElem::Deref, ProjectionElem::Deref, ..]) {
                self.reborrowed = true;
                return;
            }
        }
        self.super_rvalue(rv, loc);
    }
    fn visit_local(&mut self, local: Local, ctx: PlaceContext, _loc: Location) {
        use rustc_middle::mir::visit::MutatingUseContext;
        if local == self.local && !matches!(ctx, PlaceContext::NonUse(_) | PlaceContext::MutatingUse(MutatingUseContext::Store)) {
            self.other = true;
        }
    }
}

impl <'a, 'tcx> ExprVisitor<'a, 'tcx> {
    /// Whether the loan `b` is the borrow of a shared reference variable
    /// x that `&x` coerced to `&T` (x: `&T`) makes only to reborrow `*x`
    /// through it (`&**(&x)`, #939): `_4 = &x; _3 = &(*(*_4));`. That
    /// reborrow goes through a shared reference, so rustc tracks no loan
    /// for it, and the loan of x can end at once; what the result
    /// borrows is what x points to, as with the written `&*x`, and the
    /// result lives to its own last use (#1369). So it is no loan a
    /// reference holds. (Through a `&mut` x, the loan of x does last as
    /// long as the result, and is kept.)
    fn is_shared_autoref_loan(&self, b: &rustc_borrowck::consumers::BorrowData<'tcx>, body: &rustc_middle::mir::Body<'tcx>) -> bool {
        let (bp, ap) = (b.borrowed_place(), b.assigned_place());
        if !bp.projection.is_empty() || !ap.projection.is_empty()
            || !matches!(body.local_decls[bp.local].ty.kind(), TyKind::Ref(_, _, rustc_middle::mir::Mutability::Not))
            || body.local_decls[ap.local].is_user_variable()
        {
            return false;
        }
        let mut v = OnlyReborrowedThrough { local: ap.local, reborrowed: false, other: false };
        v.visit_body(body);
        v.reborrowed && !v.other
    }
}

impl crate::facts::BodyBindings {
    /// For `name`, an array `[T; N]` whose `T` needs drop, from which
    /// patterns move some elements out but leave others (`let [a, _] =
    /// arr;`, `[first, ..] =>`): which elements are moved and which are
    /// left in it and dropped at its scope end (#1144, #1166). None if
    /// none is moved, or the array is also moved or assigned as a whole,
    /// or one pattern moves every element (`let [a, b] = arr;`: a move
    /// of all of it). Several patterns that together move every element
    /// (`let [a, _] = arr; let [_, b] = arr;`, #1302) leave nothing in
    /// it: `ElemsLeft::last` names the elements whose move empties it.
    pub(crate) fn array_elems_left<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, body: &rustc_middle::mir::Body<'tcx>, local: Local, name: &str, ty: Ty<'tcx>) -> Option<crate::svg_generator::data::ElemsLeft> {
        let TyKind::Array(elem, _) = ty.kind() else { return None };
        if !crate::expr_visitor_utils::needs_drop_in(tcx, *elem, rustc_middle::ty::TypingEnv::fully_monomorphized()) {
            return None;
        }
        let mut v = ArrayElemMoves { tcx, body, local, lengths: BTreeMap::new(), moved: BTreeSet::new(), payloads: BTreeSet::new(), whole_slots: BTreeSet::new(), whole: false, assigns: 0, at: Vec::new() };
        v.visit_body(body);
        if v.whole || v.assigns > 1 || v.moved.is_empty() {
            return None;
        }
        let label = |path: &Vec<u64>| path.iter().fold(name.to_owned(), |s, i| format!("{s}[{i}]"));
        let mut left = BTreeSet::new();
        for (prefix, len) in &v.lengths {
            // A different path moves this entire inner array.
            if v.moved.contains(prefix) { return None; }
            for i in 0..*len {
                let mut path = prefix.clone();
                path.push(i);
                if !v.moved.contains(&path) && !v.lengths.contains_key(&path) { left.insert(path); }
            }
        }
        let mut last = Vec::new();
        if left.is_empty() {
            // The final pattern empties the array only when all other moves dominate it.
            // Several bindings in one pattern share the enclosing HIR pattern node.
            let pattern = |loc: Location| {
                let StatementKind::Assign(asg) = &body.basic_blocks[loc.block].statements.get(loc.statement_index)?.kind else { return None };
                let id = body.local_decls[asg.0.local].source_info.scope.lint_root(&body.source_scopes)?;
                Some(tcx.hir_parent_iter(id).take_while(|(_, n)| matches!(n, rustc_hir::Node::Pat(_) | rustc_hir::Node::PatField(_)))
                    .last().map_or(id, |(p, _)| p))
            };
            let doms = body.basic_blocks.dominators();
            let mut groups = BTreeMap::<_, Vec<_>>::new();
            for (loc, index) in &v.at { groups.entry(pattern(*loc)?.local_id.as_u32()).or_default().push((*loc, index)); }
            if groups.len() == 1 { return None; }
            for (id, tail) in &groups {
                if groups.iter().filter(|(k, _)| *k != id).flat_map(|(_, moves)| moves)
                    .all(|(r, _)| tail.iter().all(|(t, _)| r.dominates(*t, doms))) {
                    last = tail.iter().map(|(_, i)| *i).collect::<BTreeSet<_>>().into_iter().map(label).collect();
                    break;
                }
            }
        }
        Some(crate::svg_generator::data::ElemsLeft {
            payloads: v.payloads.iter().map(|p| (label(p), !v.whole_slots.contains(p))).collect(),
            moved: v.moved.iter().map(label).collect(), left: left.iter().map(label).collect(), last,
        })
    }

    /// Is `name`, a `Box<T>`, emptied by a move of its whole contents
    /// somewhere in this body (`take(*b)`)? Its heap allocation is
    /// still freed at scope end (#609).
    pub(crate) fn box_contents_moved_out<'tcx>(body: &rustc_middle::mir::Body<'tcx>, local: Local, ty: Ty<'tcx>) -> bool {
        if !ty.is_box() { return false; }
        let mut v = BoxContentsMoves { local, found: false };
        v.visit_body(body);
        v.found
    }
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
    /// For a `Box<T>` variable `name` whose `T` has field columns and
    /// from which a field is moved out somewhere in this body
    /// (`take(b.name)`): `T`. Rust allows that move out of a `Box`
    /// only, not out of other pointers (#602).
    pub fn box_with_field_moved_out(&self, name: &str, ty: Ty<'tcx>) -> Option<Ty<'tcx>> {
        if !ty.is_box() { return None; }
        let inner = ty.boxed_ty()?;
        if !self.has_member_columns(inner) { return None; }
        let local = self.raps.local(name)?;
        let mut v = BoxFieldMoves { local, found: false };
        v.visit_body(&self.bwf.body);
        v.found.then_some(inner)
    }

    // these are just helper functions copied from aquascope permissionctxt
    fn point_to_location(&self, p: Point, body_with_facts: &BodyWithBorrowckFacts) -> Location {
        body_with_facts.location_table.as_ref().unwrap().to_location(p)
    }

    fn location_to_span(&self, l: Location, body_with_facts: &BodyWithBorrowckFacts) -> Span {
        let span = body_with_facts.body.source_info(l).span;
        span.as_local(body_with_facts.body.span).unwrap_or(span)
    }

    fn location_to_line(&self, l: Location, body_with_facts: &BodyWithBorrowckFacts) -> usize {
        let line = span_to_line(&self.location_to_span(l, body_with_facts), &self.tcx);
        line.max(self.loop_arg_end_line(l, body_with_facts, line).unwrap_or(0))
    }

    /// A call at `l` with a `loop` argument (`take(loop { … break ra;
    /// … })`) runs once the loop has ended: on the loop's last line,
    /// where the visitor pairs the `break` values in its branches with
    /// the argument (#452, #561). A loan live at the call is live on
    /// that line, not just on the call's first one (#662). `line`: the
    /// call's first line. Decided from the HIR: the argument is a `loop`
    /// expression.
    fn loop_arg_end_line(&self, l: Location, body_with_facts: &BodyWithBorrowckFacts, line: usize) -> Option<usize> {
        let body = &body_with_facts.body;
        let block = &body.basic_blocks[l.block];
        if l.statement_index != block.statements.len() {
            return None;
        }
        let TerminatorKind::Call { args, .. } = &block.terminator().kind else { return None };
        let ends: Vec<(Span, usize)> = args.iter()
            .filter_map(|a| a.span.as_local(body.span))
            .map(|sp| (sp, span_end_line(&sp, &self.tcx)))
            .filter(|(_, end)| *end > line)
            .collect();
        if ends.is_empty() {
            return None;
        }
        struct Loops(Vec<Span>);
        impl<'tcx> intravisit::Visitor<'tcx> for Loops {
            fn visit_expr(&mut self, e: &'tcx rustc_hir::Expr<'tcx>) {
                if let rustc_hir::ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _) = e.kind {
                    self.0.push(e.span);
                }
                intravisit::walk_expr(self, e);
            }
        }
        let mut loops = Loops(Vec::new());
        intravisit::Visitor::visit_expr(&mut loops, self.tcx.hir_body_owned_by(body.source.def_id().as_local()?).value);
        ends.into_iter().filter(|(sp, _)| loops.0.contains(sp)).map(|(_, end)| end).max()
    }

    /// The last line at which each loan is in scope for rustc's NLL
    /// borrow checker (#644): the `Borrows` dataflow rustc's borrowck
    /// runs, whose state before a statement is the set of loans an
    /// access there conflicts with. Polonius' `loan_live_at` is more
    /// precise (location-sensitive subset relations) and can end a loan
    /// earlier than rustc does, which would draw a lender free where
    /// rustc still rejects using it.
    ///
    /// Cleanup blocks (the unwind path of a call that may panic) are
    /// left out: no code of the program runs there, only drops, and
    /// their span is the end of the scope. A `Box<dyn Fn + '_>` that
    /// is called and then moved into `drop` is still dropped on the
    /// unwind path out of the call, so the loan its closure holds is
    /// in scope there, but the program can use the lender right after
    /// `drop(g)` (#1104).
    fn nll_loan_end_lines(&self, body_with_facts: &BodyWithBorrowckFacts<'tcx>) -> HashMap<BorrowIndex, usize> {
        let body = &body_with_facts.body;
        let borrows = Borrows::new(self.tcx, body, &body_with_facts.region_inference_context, &body_with_facts.borrow_set);
        let mut cursor = borrows.iterate_to_fixpoint(self.tcx, body, None).into_results_cursor(body);
        let mut ends: HashMap<BorrowIndex, usize> = HashMap::new();
        for (block, data) in body.basic_blocks.iter_enumerated() {
            if data.is_cleanup {
                continue;
            }
            for statement_index in 0..=data.statements.len() {
                let location = Location { block, statement_index };
                cursor.seek_before_primary_effect(location);
                if cursor.get().is_empty() { continue; }
                let line = self.location_to_line(location, body_with_facts);
                for b in cursor.get().iter() {
                    let e = ends.entry(b).or_insert(line);
                    if line > *e { *e = line; }
                }
            }
        }
        ends
    }

    /// The last line, before `write_line`, where rustc's borrow checker
    /// has the loan `borrow_idx` in scope on a path that reaches
    /// `write_line`: where the loan ends for the code that writes there.
    /// `nll_loan_end_lines` gives the last line over all paths, which can
    /// be on another path (#770): in `if c { x += 1; r = &y; }` the loan
    /// of `x` that `r` holds is dead on the way to `r = &y;` (`r` is
    /// overwritten before it is read), though it lives on to a later read
    /// of `r` on the path that skips the `if`. None when the loan is in
    /// scope at the write itself or later on a path back to it (a
    /// loop), or never before it.
    ///
    /// `write`: the span of the write, when known. Only the locations on
    /// `write_line` inside it are the write: in a one-line `if c { r =
    /// &y; }` the condition, on the same line, is read on both paths, and
    /// the loan can be in scope there (#1188). The end is then that line.
    pub fn loan_end_on_path_to(&self, borrow_idx: usize, write_line: usize, write: Option<Span>) -> Option<usize> {
        let body_with_facts = self.bwf;
        let body = &body_with_facts.body;
        let borrows = Borrows::new(self.tcx, body, &body_with_facts.region_inference_context, &body_with_facts.borrow_set);
        let mut cursor = borrows.iterate_to_fixpoint(self.tcx, body, None).into_results_cursor(body);
        let mut in_scope: Vec<(Location, usize)> = Vec::new();
        let mut targets: Vec<Location> = Vec::new();
        for (block, data) in body.basic_blocks.iter_enumerated() {
            for statement_index in 0..=data.statements.len() {
                let location = Location { block, statement_index };
                let line = self.location_to_line(location, body_with_facts);
                let is_target = line == write_line
                    && write.is_none_or(|w| w.contains(self.location_to_span(location, body_with_facts)));
                if is_target {
                    targets.push(location);
                }
                cursor.seek_before_primary_effect(location);
                if cursor.get().iter().any(|b| b.index() == borrow_idx) {
                    if is_target {
                        return None;
                    }
                    in_scope.push((location, line));
                }
            }
        }
        // The locations from which the write is reachable.
        let preds = body.basic_blocks.predecessors();
        let mut reaches: std::collections::HashSet<Location> = targets.iter().copied().collect();
        let mut work = targets;
        while let Some(l) = work.pop() {
            let before: Vec<Location> = if l.statement_index > 0 {
                vec![Location { block: l.block, statement_index: l.statement_index - 1 }]
            } else {
                preds[l.block].iter()
                    .map(|&p| Location { block: p, statement_index: body.basic_blocks[p].statements.len() })
                    .collect()
            };
            for b in before {
                if reaches.insert(b) {
                    work.push(b);
                }
            }
        }
        in_scope.into_iter()
            .filter(|(l, _)| reaches.contains(l))
            .map(|(_, line)| line)
            .max()
            .filter(|&end| end < write_line || (write.is_some() && end == write_line))
    }

    /// Where rustc's borrow checker has the loan `borrow_idx` in scope
    /// around a conditional `whole` (#603): for each of `parts` (spans
    /// of its arms, or of a condition and its body), the last line of a
    /// location inside it where the loan is in scope; and whether it is
    /// in scope anywhere after `whole` (a location that starts past its
    /// end), where the paths through the parts meet again.
    pub fn loan_scope_in(&self, borrow_idx: usize, parts: &[Span], whole: Span) -> (Vec<Option<usize>>, bool) {
        let body_with_facts = self.bwf;
        let body = &body_with_facts.body;
        let borrows = Borrows::new(self.tcx, body, &body_with_facts.region_inference_context, &body_with_facts.borrow_set);
        let mut cursor = borrows.iterate_to_fixpoint(self.tcx, body, None).into_results_cursor(body);
        let mut last: Vec<Option<usize>> = vec![None; parts.len()];
        let mut past = false;
        for (block, data) in body.basic_blocks.iter_enumerated() {
            for statement_index in 0..=data.statements.len() {
                let location = Location { block, statement_index };
                cursor.seek_before_primary_effect(location);
                if !cursor.get().iter().any(|b| b.index() == borrow_idx) {
                    continue;
                }
                // Only where code runs: a temporary's `StorageDead` at the
                // failing path's start has the span of the body's `}`.
                let bookkeeping = match data.statements.get(location.statement_index) {
                    Some(st) => matches!(st.kind, StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop),
                    None => matches!(data.terminator().kind, TerminatorKind::Goto { .. } | TerminatorKind::FalseEdge { .. } | TerminatorKind::FalseUnwind { .. }),
                };
                if bookkeeping {
                    continue;
                }
                let span = self.location_to_span(location, body_with_facts);
                if span.lo() >= whole.hi() {
                    past = true;
                }
                let line = self.location_to_line(location, body_with_facts);
                for (i, part) in parts.iter().enumerate() {
                    if part.contains(span) {
                        last[i] = Some(last[i].map_or(line, |l: usize| l.max(line)));
                    }
                }
            }
        }
        (last, past)
    }

    /// A write to the variable `lender` at `line` (an assignment to it or
    /// a part of it, or a `&mut` borrow of it, as `v.push(..)` takes)
    /// where a loan of it is dead though the borrow checker keeps it in
    /// scope on another path, past the write (#796): `let mut c = &x; if
    /// cond { x = 2; c = &y; } println!("{}", c);` compiles because on
    /// the path through the `if`, `c` is given a new reference before it
    /// is used again. The diagram gives each loan one end (#778), so x is
    /// drawn borrowed across the write; this says why rustc accepts it.
    /// None when no such loan, or no reference variable holding it, is
    /// found.
    pub fn dead_loan_at_write(&self, x: Local, site: Span, line: usize) -> Option<DeadLoanWrite> {
        let bwf = self.bwf;
        let body = &bwf.body;
        let names = &self.raps.bindings.locals;
        let at_site = |loc| {
            let span = self.location_to_span(loc, bwf);
            site.contains(span) || span.contains(site)
        };
        let no_deref = |p: &Place<'tcx>| !p.projection.iter().any(|e| matches!(e, ProjectionElem::Deref));
        // Candidate writes to this local within the emitting expression.
        let mut writes: Vec<(Location, Place<'tcx>)> = Vec::new();
        for (block, data) in body.basic_blocks.iter_enumerated() {
            for (statement_index, st) in data.statements.iter().enumerate() {
                let loc = Location { block, statement_index };
                let StatementKind::Assign(assign) = &st.kind else { continue };
                let (lhs, rv) = &**assign;
                let place = match rv {
                    Rvalue::Ref(_, BorrowKind::Mut { .. }, p) if p.local == x => *p,
                    _ if lhs.local == x => *lhs,
                    _ => continue,
                };
                if no_deref(&place) && at_site(loc) {
                    writes.push((loc, place));
                }
            }
            let loc = Location { block, statement_index: data.statements.len() };
            if let TerminatorKind::Call { destination, .. } = &data.terminator().kind {
                if destination.local == x && no_deref(destination) && at_site(loc) {
                    writes.push((loc, *destination));
                }
            }
        }
        if writes.is_empty() {
            return None;
        }
        // The loans of x the writes conflict with, issued before the line.
        let loans: Vec<(BorrowIndex, Location, Local)> = bwf.borrow_set.location_map().iter().enumerate()
            .filter(|(_, (issued, b))| {
                let bp = b.borrowed_place();
                bp.local == x && no_deref(&bp)
                    && writes.iter().any(|(_, w)| projections_overlap(bp.projection, w.projection))
                    && self.location_to_line(**issued, bwf) < line
            })
            .map(|(i, (issued, b))| (BorrowIndex::from_usize(i), *issued, b.assigned_place().local))
            .collect();
        if loans.is_empty() {
            return None;
        }
        // Each loan must be out of scope at the writes (rustc accepted
        // them) and in scope at or after the line on some other path.
        let borrows = Borrows::new(self.tcx, body, &bwf.region_inference_context, &bwf.borrow_set);
        let mut cursor = borrows.iterate_to_fixpoint(self.tcx, body, None).into_results_cursor(body);
        let mut live_later: Vec<BorrowIndex> = Vec::new();
        let mut at_write: Vec<BorrowIndex> = Vec::new();
        for (block, data) in body.basic_blocks.iter_enumerated() {
            for statement_index in 0..=data.statements.len() {
                let location = Location { block, statement_index };
                cursor.seek_before_primary_effect(location);
                let is_write = writes.iter().any(|(l, _)| *l == location);
                let l_line = self.location_to_line(location, bwf);
                for (b, _, _) in loans.iter() {
                    if !cursor.get().contains(*b) { continue; }
                    if is_write { at_write.push(*b); }
                    // A use before every write on this very row is on the
                    // same path. It cannot justify a note about another path.
                    if l_line >= line && !(l_line == line && writes.iter().all(|(w, _)| {
                        if location.block == w.block { location.statement_index < w.statement_index }
                        else { body.basic_blocks.dominators().dominates(location.block, w.block) }
                    })) { live_later.push(*b); }
                }
            }
        }
        // `local = ...` at `loc`, the whole variable.
        let stmt_assigns = |loc: Location, local: Local| -> bool {
            body.basic_blocks[loc.block].statements.get(loc.statement_index).is_some_and(|st|
                matches!(&st.kind, StatementKind::Assign(a) if a.0.local == local && a.0.projection.is_empty()))
        };
        for (b, issued, assigned) in loans {
            if at_write.contains(&b) || !live_later.contains(&b) {
                continue;
            }
            // The reference variable the loan is stored in: the place it
            // is assigned to, or the variable a temporary holding it is
            // moved or reborrowed into.
            let (mut holder, mut holder_at) = (assigned, issued);
            for _ in 0..4 {
                if names.contains_key(&holder) { break; }
                let next = body.basic_blocks.iter_enumerated().find_map(|(block, data)| {
                    data.statements.iter().enumerate().find_map(|(statement_index, st)| {
                        let StatementKind::Assign(a) = &st.kind else { return None };
                        let reads = match &a.1 {
                            Rvalue::Use(Operand::Copy(p) | Operand::Move(p)) => p.local == holder && p.projection.is_empty(),
                            Rvalue::Ref(_, _, p) => p.local == holder && matches!(p.projection.as_slice(), [ProjectionElem::Deref]),
                            _ => false,
                        };
                        (reads && a.0.projection.is_empty()).then(|| (a.0.local, Location { block, statement_index }))
                    })
                });
                let Some((h, at)) = next else { break };
                (holder, holder_at) = (h, at);
            }
            let Some(borrower) = names.get(&holder) else { continue };
            if !body.local_decls[holder].ty.is_ref() {
                continue;
            }
            let succ = |l: Location| -> Vec<Location> {
                let data = &body.basic_blocks[l.block];
                if l.statement_index < data.statements.len() {
                    vec![Location { block: l.block, statement_index: l.statement_index + 1 }]
                } else {
                    data.terminator().successors().map(|block| Location { block, statement_index: 0 }).collect()
                }
            };
            // The loan has to be issued on a path that reaches the write: a
            // loan of the other branch (`if c { r = &mut s; } else { r =
            // &mut s; }`, the write being the else's own borrow) never
            // existed on this one (#1153).
            let mut seen: std::collections::HashSet<Location> = std::collections::HashSet::new();
            let mut work: Vec<Location> = succ(issued);
            let mut reaches_write = false;
            while let Some(l) = work.pop() {
                if !seen.insert(l) { continue; }
                if writes.iter().any(|(w, _)| *w == l) {
                    reaches_write = true;
                    break;
                }
                work.extend(succ(l));
            }
            if !reaches_write {
                continue;
            }
            let uses = |l: Location| -> bool {
                let data = &body.basic_blocks[l.block];
                let mut v = LocalUse { local: holder, found: false };
                match data.statements.get(l.statement_index) {
                    Some(st) => v.visit_statement(st, l),
                    None => v.visit_terminator(data.terminator(), l),
                }
                v.found
            };
            // Back from the writes: was the reference given a new value on
            // every path that reaches them, after it took the loan? Then
            // that is where the loan's use on this path ended.
            let preds = body.basic_blocks.predecessors();
            let pred = |l: Location| -> Vec<Location> {
                if l.statement_index > 0 {
                    vec![Location { block: l.block, statement_index: l.statement_index - 1 }]
                } else {
                    preds[l.block].iter().map(|&p| Location { block: p, statement_index: body.basic_blocks[p].statements.len() }).collect()
                }
            };
            let mut seen: std::collections::HashSet<Location> = std::collections::HashSet::new();
            let mut work: Vec<Location> = writes.iter().flat_map(|(l, _)| pred(*l)).collect();
            let (mut earlier, mut old_value) = (BTreeSet::new(), false);
            while let Some(l) = work.pop() {
                if !seen.insert(l) { continue; }
                if stmt_assigns(l, holder) {
                    if l == holder_at { old_value = true; } else { earlier.insert(self.location_to_line(l, bwf)); }
                    continue;
                }
                let before = pred(l);
                if before.is_empty() { old_value = true; }
                work.extend(before);
            }
            // Else forward from the writes: is the reference given a new
            // value before its next use, or not used again?
            let mut seen: std::collections::HashSet<Location> = std::collections::HashSet::new();
            let mut work: Vec<Location> = writes.iter().flat_map(|(l, _)| succ(*l)).collect();
            let (mut later, mut used) = (BTreeSet::new(), false);
            while let Some(l) = work.pop() {
                if !seen.insert(l) { continue; }
                let data = &body.basic_blocks[l.block];
                if let Some(st) = data.statements.get(l.statement_index) {
                    if matches!(st.kind, StatementKind::StorageDead(d) if d == holder) { continue; }
                }
                if stmt_assigns(l, holder) {
                    later.insert(self.location_to_line(l, bwf));
                    continue;
                }
                if uses(l) {
                    used = true;
                    continue;
                }
                work.extend(succ(l));
            }
            // Only a single line is named: with several (`if a { c = &y; }
            // else { c = &z; }`) no one line is the reason.
            let repoint = match (old_value, earlier.len(), used, later.len()) {
                (false, 1, _, _) => Some((*earlier.first()?, true)),
                (true, _, false, 0) => None,
                (true, _, false, 1) => Some((*later.first()?, false)),
                _ => continue,
            };
            return Some(DeadLoanWrite { location: writes[0].0, borrower: borrower.clone(), repoint });
        }
        None
    }

    // To get this to really work we would need to do a dataflow analysis to establish which temporaries 
    // refer to the same piece of data. Since the way the MIR is SSA.
    // For example:
    //  Let mut a = &b;
    //  a = &c;
    // compiles to: (where  _1: "b", _2: "c", "_3: a")
    // _3 = &'?2 _1; <-- let mut a = &b
    // _5 = &'?3 _2;     <- |
    // _4 = &'?4 (*_5);     |
    // _3 = move _4;     <- | All these instructions are used for a = &c 
    // notice how we generate new temporaries (_4 and _5)
    // Consequently, when we get the borrow data we see a borrow between (_5 and _2)
    // Which doesn't allow us to trace it back to the borrowed and assigned place in the source code (c, a)
    pub fn gather_borrow_data(&self, body_with_facts: &BodyWithBorrowckFacts<'tcx>) -> Vec<MIRBorrowData>{
        // compute output facts using polonius
        let p_output = Output::compute(&body_with_facts.input_facts.as_ref().unwrap(), Algorithm::Naive, true);

        // compute local to name map
        let loc_to_source_name = &self.raps.bindings.locals;
        
        
        // compute the loan regions
        let mut loan_regions: HashMap<Loan, (usize, usize)> = HashMap::new();
        // `loan_live_at` is an FxHashMap (fixed but arbitrary order).
        #[expect(clippy::disallowed_methods, reason = "keeps a min/max line per loan; order doesn't matter")]
        p_output.loan_live_at.iter().for_each(|(point, loans)| {
          loans.iter().for_each(|loan| {
            loan_regions.entry(*loan).and_modify(|(l1, l2)|{
              let l = self.point_to_location(*point, &body_with_facts);
              let line = self.location_to_line(l, &body_with_facts);
              if line < *l1 { *l1 = line }
              else if line > *l2 { *l2 = line}
            })
            .or_insert_with(|| {
              let l = self.point_to_location(*point, &body_with_facts);
              let line = self.location_to_line(l, &body_with_facts);
              (line, line)
            });
          });
        });
        
        let nll_ends = self.nll_loan_end_lines(body_with_facts);
        let mut res: Vec<MIRBorrowData> = Vec::new();

        // refine loan regions (when testing they seem to be off sometimes) by looping through loan assignments
        for (_region, b_idx, location_idx) in body_with_facts.input_facts.as_ref().unwrap().loan_issued_at.iter() {
            let location = body_with_facts.location_table.as_ref().unwrap().to_location(*location_idx);
            let assignment_line = self.location_to_line(location, &body_with_facts);
            match body_with_facts.borrow_set.location_map().get(&location) {
              Some(b_data) if self.is_shared_autoref_loan(b_data, &body_with_facts.body) => {}
              Some(b_data) => {
                // Where rustc's borrow checker ends the loan (#644);
                // Polonius' end only for a loan NLL never has in scope
                // after the statement that makes it.
                let end_line = nll_ends.get(b_idx).copied()
                  .unwrap_or_else(|| loan_regions.get(b_idx).map_or(assignment_line, |r| r.1));
                // println!("borrow_data for location {:#?} : {:#?}", location, b_data);
                // For a borrow of `r.a.b`, MIR's `borrowed_place`
                // is `r` with projections [Field(a), Field(b)].
                // `local_or_deref_local()` returns None for any
                // projection that isn't a single deref, which is why
                // ref-lifetime refinement used to skip every borrow
                // of a struct field. Use the base `.local` directly
                // so we always get the source name; `borrow_match`
                // matches against either the exact lender name or a
                // path prefix terminated by `.`.
                let b_place = Some(b_data.borrowed_place().local);
                let a_place = Some(b_data.assigned_place().local);
                let b_name = b_place.and_then(|l| loc_to_source_name.get(&l).cloned());
                let a_name = a_place.and_then(|l| loc_to_source_name.get(&l).cloned());

                // (A loan that is never live, like the `&s` MIR takes of a
                // `ref s` binding for a guard that doesn't use `s`, ends
                // where it's issued: `assignment_line` above.)
                res.push(MIRBorrowData {borrow_idx: b_idx.index(), 
                                        region: (assignment_line, end_line),
                                        borrowed_place: b_name, 
                                        assigned_place: a_name,
                                        span: self.location_to_span(location, &body_with_facts)});
              }
              None => { }
            }
          }

            // debugging info
            info!("locals map {:#?}", loc_to_source_name);
            info!("body string {}", self.bwf.body.to_string(self.tcx).unwrap());
            for (region, b_idx, location_idx) in body_with_facts.input_facts.as_ref().unwrap().loan_issued_at.iter() {
                info!("loan issued at {:#?}", (region, b_idx, location_idx));
                let location = body_with_facts.location_table.as_ref().unwrap().to_location(*location_idx);
                match body_with_facts.borrow_set.location_map().get(&location) {
                    Some(b_data) => {
                    info!("borrow_data for location {:#?} : {:#?}", location, b_data);
                    let b_place = b_data.borrowed_place().local_or_deref_local();
                    let a_place = b_data.assigned_place().local_or_deref_local();
                    // Log places with their Debug form, not rustc_utils'
                    // `PlaceExt::to_string`: that helper hits
                    // `unimplemented!("Union")` on a union-field projection
                    // (`&b.f` with `b: SomeUnion`), and because the logger
                    // runs at Info this debug line would panic the plugin.
                    info!("borrowed_place: {:?}, as local: {:#?}", b_data.borrowed_place(), b_place);
                    info!("is source visible? {}", b_data.borrowed_place().is_source_visible(self.tcx, &body_with_facts.body));
                    // RegionInferenceContext's per-region accessors are pub(crate) in 1.91;
                    // log just the region id here. If we need the NLL origin, we can lift
                    // this through rustc_utils' consumer wrappers later.
                    info!("region inference context: region {:?}", region);
                    if b_place.is_some() { 
                        let b_loc = body_with_facts.body.local_decls.get(b_place.unwrap()).unwrap();
                        info!("local decl {:#?}", b_loc); 
                        //println!("span {}", b_loc.source_info.span.to_string(self.tcx));
                    }
                    info!("assigned place {:?}, as local {:#?}", b_data.assigned_place(), a_place);
                    info!("is source visible? {}", b_data.assigned_place().is_source_visible(self.tcx, &body_with_facts.body));
                    if a_place.is_some() { 
                        let a_loc =  body_with_facts.body.local_decls.get(a_place.unwrap()).unwrap();
                        info!("local decl {:#?}", a_loc);
                        //println!("span {}", a_loc.source_info.span.to_string(self.tcx));
                    }

                    // origins live at
                    // println!("origins live at {:#?}", p_output.origins_live_at(*location_idx));
                    // println!("loans in scope at {:#?}", p_output.loans_in_scope_at(*location_idx));
                    // println!("origin contains loans at {:#?}", p_output.origin_contains_loan_at(*location_idx));
                    // let expr = self.tcx.hir_expect_expr(borrow_data.body.location_to_hir_id(location.clone()));
                    // println!("line {}\n\n", self.span_to_line(&expr.span));
                    }
                    None => { println!("no borrow data found for location"); }
                }
            }

          res
    }


    /// The loans a call leaves in a variable that keeps them after it
    /// returns: the closure `keep` hands back into `g` in `let g =
    /// keep(|| s.len());` holds its capture's loan of `s` (#686), and
    /// `refs.extend([&s])` stores `&s` in `refs` (#682), which a
    /// signature only says through a trait bound (`I: IntoIterator<Item
    /// = T>`).
    ///
    /// Read from rustc's own facts, not the callee's signature: right
    /// after the call returns, a variable keeps loan L if L is in one
    /// of the regions of its type there (Polonius' `origin_contains_loan_at`,
    /// which only holds while that variable is live). A loan counts as
    /// handed over by the call if it was issued inside the call (`&s` in
    /// its arguments, a closure argument's capture) or if the variable
    /// didn't hold it before the call. Only loans of a named variable
    /// (or of what a reference variable points to, `*r`) are returned.
    pub fn loans_kept_after_call(&self, call: Span) -> Vec<KeptLoan> {
        let bwf = self.bwf;
        let (Some(facts), Some(table)) = (bwf.input_facts.as_ref(), bwf.location_table.as_ref()) else { return Vec::new() };
        let body = &bwf.body;
        // The call's terminator, and the first point after it returns.
        let mut sites: Vec<(Location, Location)> = Vec::new();
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            let term = data.terminator();
            if let TerminatorKind::Call { target: Some(target), fn_span, .. } = &term.kind {
                if term.source_info.span == call || *fn_span == call {
                    sites.push((Location { block: bb, statement_index: data.statements.len() },
                                Location { block: *target, statement_index: 0 }));
                }
            }
        }
        if sites.is_empty() {
            return Vec::new();
        }
        let names = &self.raps.bindings.locals;
        // Ordered by local, so the result doesn't depend on hash order.
        let mut origins: std::collections::BTreeMap<Local, Vec<Origin>> = std::collections::BTreeMap::new();
        for (local, origin) in facts.use_of_var_derefs_origin.iter().chain(facts.drop_of_var_derefs_origin.iter()) {
            if names.contains_key(local) {
                let os = origins.entry(*local).or_default();
                if !os.contains(origin) {
                    os.push(*origin);
                }
            }
        }
        let out = Output::compute(facts, Algorithm::Naive, true);
        let held_at = |p: Point, os: &[Origin]| -> Vec<Loan> {
            let mut ls: Vec<Loan> = Vec::new();
            if let Some(m) = out.origin_contains_loan_at.get(&p) {
                for o in os {
                    for l in m.get(o).into_iter().flatten() {
                        if !ls.contains(l) {
                            ls.push(*l);
                        }
                    }
                }
            }
            ls
        };
        let ends: HashMap<usize, usize> = self.gather_borrow_data(bwf).iter().map(|b| (b.borrow_idx, b.region.1)).collect();
        let mut res: Vec<KeptLoan> = Vec::new();
        for (term, after) in sites {
            for (local, os) in origins.iter() {
                let before = held_at(table.start_index(term), os);
                for loan in held_at(table.start_index(after), os) {
                    let Some((issued, data)) = bwf.borrow_set.location_map().get_index(loan.index()) else { continue };
                    let span = self.location_to_span(*issued, bwf);
                    if before.contains(&loan) && !call.contains(span) {
                        continue;
                    }
                    let place = data.borrowed_place();
                    let through_ref = match place.projection.first() {
                        Some(ProjectionElem::Deref) => {
                            // `*r` with `r` a reference variable: a reborrow
                            // of what r points to. Any deeper deref (`**r`, a
                            // Box) has no column to draw on.
                            if place.projection.iter().skip(1).any(|p| matches!(p, ProjectionElem::Deref))
                                || !body.local_decls[place.local].ty.is_ref() {
                                continue;
                            }
                            true
                        }
                        _ if place.projection.iter().any(|p| matches!(p, ProjectionElem::Deref)) => continue,
                        _ => false,
                    };
                    let (Some(lender), Some(holder)) = (names.get(&place.local), names.get(local)) else { continue };
                    if place.local == *local {
                        continue;
                    }
                    if res.iter().any(|k| k.loan == loan.index() && k.holder == *holder) {
                        continue;
                    }
                    res.push(KeptLoan {
                        loan: loan.index(),
                        holder: holder.clone(),
                        lender: lender.clone(),
                        through_ref,
                        is_mut: matches!(data.kind(), BorrowKind::Mut { .. }),
                        end_line: ends.get(&loan.index()).copied().unwrap_or_else(|| self.location_to_line(*issued, bwf)),
                        span,
                    });
                }
            }
        }
        res
    }

    /// rustc's loans issued for the borrow expression at `span` (`&s`):
    /// indices into its borrow set, as in `KeptLoan::loan`.
    pub fn loans_issued_in(&self, span: Span) -> Vec<usize> {
        let bwf = self.bwf;
        bwf.borrow_set.location_map().iter().enumerate()
            .filter(|(_, (loc, _))| span.contains(self.location_to_span(**loc, bwf)))
            .map(|(i, _)| i)
            .collect()
    }

    /// The loan issued at this expression's MIR source span. Coercions
    /// can add a second loan through the first loan's argument temporary.
    pub(crate) fn argument_loan(&self, span: Span) -> Option<&crate::facts::schema::LoanInfo> {
        let span = span.with_parent(None);
        let candidates: Vec<_> = self.body_facts?.loans.iter().filter_map(|loan| {
            let loc = loan.issue?;
            let stmt = self.mir_body.basic_blocks[loc.block].statements.get(loc.statement_index)?;
            if self.location_to_span(loc, self.bwf).with_parent(None) != span { return None; }
            let StatementKind::Assign(assignment) = &stmt.kind else { return None };
            let Rvalue::Ref(_, _, place) = &assignment.1 else { return None };
            Some((loan, assignment.0, *place))
        }).collect();
        let mut issued = candidates.iter().filter(|(_, _, place)| {
            place.projection.first() != Some(&ProjectionElem::Deref) || !candidates.iter().any(|(_, dest, _)|
                dest.as_local() == Some(place.local) && !self.mir_body.local_decls[place.local].is_user_variable())
        });
        let (loan, ..) = issued.next()?;
        issued.next().is_none().then_some(*loan)
    }

    /// The variables that hold one of `loans` at some point, by rustc's
    /// borrow checker: a variable whose type has a lifetime the loan is
    /// in (Polonius' `origin_contains_loan_at`) while the variable is
    /// live. `let w: Vec<&String> = v.iter().copied().collect();` after
    /// `keepv(&mut v, &s)` gives w (and v): the signatures along the
    /// chain tie w's lifetime to v's (#1015).
    pub fn names_holding_loans(&self, loans: &[usize]) -> BTreeSet<String> {
        let bwf = self.bwf;
        let mut out: BTreeSet<String> = BTreeSet::new();
        let Some(facts) = bwf.input_facts.as_ref() else { return out };
        if loans.is_empty() {
            return out;
        }
        let names = &self.raps.bindings.locals;
        let mut of_origin: BTreeMap<Origin, Vec<Local>> = BTreeMap::new();
        for (local, origin) in facts.use_of_var_derefs_origin.iter().chain(facts.drop_of_var_derefs_origin.iter()) {
            if names.contains_key(local) {
                let ls = of_origin.entry(*origin).or_default();
                if !ls.contains(local) {
                    ls.push(*local);
                }
            }
        }
        let p_output = Output::compute(facts, Algorithm::Naive, true);
        #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "collected into a BTreeSet")]
        for origins in p_output.origin_contains_loan_at.values() {
            for (origin, held) in origins.iter() {
                let Some(locals) = of_origin.get(origin) else { continue };
                if held.iter().any(|l| loans.contains(&l.index())) {
                    out.extend(locals.iter().filter_map(|l| names.get(l).cloned()));
                }
            }
        }
        out
    }

    /// Whether compiler MIR has any executable edge back to this head.
    pub fn loop_never_rounds(&self, id: rustc_hir::HirId) -> Option<bool> {
        let loop_state = self.body_facts?.loops.get(&id)?;
        Some(!loop_state.edges.iter().any(|e| e.runs && e.kind == crate::facts::LoopEdgeKind::Backedge))
    }

    /// Whether a loan issued inside `within` (a closure literal passed
    /// to the call at `call`: its captures' borrows) is still in scope
    /// for rustc's borrow checker right after the call returns, i.e.
    /// whether anything (the call's result) keeps the closure's borrows.
    /// `self.items.iter().find(|x| **x as usize == self.n)` returns an
    /// `Option<&i32>` that holds only the borrow of `self.items`: the
    /// closure's borrow of `self.n` ends with the call (#1101). Uses the
    /// `Borrows` dataflow rustc's borrowck runs, like
    /// `nll_loan_end_lines`. None when the call isn't found in the MIR.
    pub fn loans_issued_in_kept_after_call(&self, call: Span, within: Span) -> Option<bool> {
        let bwf = self.bwf;
        let body = &bwf.body;
        let mut afters: Vec<Location> = Vec::new();
        for data in body.basic_blocks.iter() {
            let term = data.terminator();
            if let TerminatorKind::Call { target: Some(target), fn_span, .. } = &term.kind {
                if term.source_info.span == call || *fn_span == call {
                    afters.push(Location { block: *target, statement_index: 0 });
                }
            }
        }
        if afters.is_empty() {
            return None;
        }
        let borrows = Borrows::new(self.tcx, body, &bwf.region_inference_context, &bwf.borrow_set);
        let mut cursor = borrows.iterate_to_fixpoint(self.tcx, body, None).into_results_cursor(body);
        for after in afters {
            cursor.seek_before_primary_effect(after);
            for b in cursor.get().iter() {
                let Some((issued, _)) = bwf.borrow_set.location_map().get_index(b.index()) else { continue };
                if within.contains(self.location_to_span(*issued, bwf)) {
                    return Some(true);
                }
            }
        }
        Some(false)
    }

    /// The last line in `lines` (first, last) on which the variable
    /// `name` (its RAP column) is live, from
    /// Polonius' liveness: its last use, or the end of a loop it's used
    /// in.
    pub(crate) fn local_last_live_line(&self, local: Local, lines: (usize, usize)) -> Option<usize> {
        let bwf = self.bwf;
        let p_output = Output::compute(bwf.input_facts.as_ref().unwrap(), Algorithm::Naive, true);
        #[expect(clippy::disallowed_methods, reason = "only takes the greatest line")]
        let last = p_output.var_live_on_entry.iter()
            .filter(|(_, vars)| vars.contains(&local))
            .map(|(p, _)| self.location_to_line(self.point_to_location(*p, bwf), bwf))
            .filter(|l| lines.0 <= *l && *l <= lines.1)
            .max();
        last
    }

    /// The variables whose loans rustc's borrow checker puts in the
    /// lifetimes of `name`'s type, or of its field `name.f.g` when
    /// `fields` is `["f", "g"]`: what a reference stored there may
    /// point into. `if let Some(x) = pick(&a, &b)` with `pick<'a>(&'a
    /// String, &'a String) -> Option<&'a String>` gives `{a, b}`: the
    /// signature makes both loans flow into x's lifetime (#681). A
    /// loan is named by the base variable of the place it borrows
    /// (`s` for `&s`, `&s.f` and `&*s`), with whether one of its loans
    /// there is mutable.
    ///
    /// `None` when this body has no MIR local named `name` (a binding
    /// inside a closure), or a field in `fields` isn't found.
    pub fn loans_held_by(&self, name: &str, fields: &[&str]) -> Option<BTreeMap<String, bool>> {
        self.loans_held_in(self.raps.local(name), fields, None)
    }

    /// `loans_held_by`, for only the loans issued inside `within` (a
    /// reference's initializer): the kind of the loans that this
    /// assignment puts in `name`'s lifetimes. That kind is not always
    /// the reference's own: `let r: &String = &mut s;` and `let r =
    /// downgrade(&mut s);` (`fn(&mut String) -> &str`) keep a mutable
    /// loan of s in a shared reference, and `let r = &mut &x;` keeps a
    /// shared loan of x in a mutable one (#1122, #1131).
    /// (`name.f.g` when `fields` is `["f", "g"]`, as in `loans_held_by`.)
    pub fn loans_held_from(&self, name: &str, fields: &[&str], within: Span) -> Option<BTreeMap<String, bool>> {
        self.loans_held_in(self.raps.local(name), fields, Some(within))
    }

    pub(crate) fn loans_held_in(&self, local: Option<Local>, fields: &[&str], within: Option<Span>) -> Option<BTreeMap<String, bool>> {
        let bwf = self.bwf;
        let names = &self.raps.bindings.locals;
        let held = self.loans_in_type_of(local?, fields)?;
        let mut out: BTreeMap<String, bool> = BTreeMap::new();
        for loan in held {
            let b = &bwf.borrow_set[loan];
            if let Some(w) = within {
                let sp = self.location_to_span(b.reserve_location(), bwf);
                if !w.contains(sp) {
                    continue;
                }
            }
            if let Some(n) = names.get(&b.borrowed_place().local).cloned() {
                *out.entry(n).or_insert(false) |= b.kind().mutability().is_mut();
            }
        }
        Some(out)
    }

    /// The loans (indices into rustc's borrow set, as
    /// `MIRBorrowData::borrow_idx`) the variable `name` (its part
    /// `fields`) holds at some point: those in a lifetime of its type
    /// (Polonius' `origin_contains_loan_at`). A holder that got a
    /// reference variable's value (`y` in `let y = if c { let r = &mut
    /// s1; Some(r) } else { .. };`) holds r's loan though no loan was
    /// issued for y itself (#1475). A field's column (`w.o`) names the
    /// field of its variable's type (#1482). None when this body has no
    /// local named `name`.
    pub fn loan_indices_held_by(&self, name: &str) -> Option<BTreeSet<usize>> {
        let mut parts = name.split('.');
        let root = parts.next()?;
        let fields: Vec<&str> = parts.collect();
        Some(self.loans_in_type_of(self.raps.local(root)?, &fields)?.into_iter().map(|l| l.index()).collect())
    }

    fn loans_in_type_of(&self, local: Local, fields: &[&str]) -> Option<BTreeSet<Loan>> {
        let bwf = self.bwf;
        let body = &bwf.body;
        let mut regions: BTreeSet<RegionVid> = BTreeSet::new();
        let mut ty = body.local_decls[local].ty;
        for f in fields {
            ty = self.field_ty_named(ty, f)?;
        }
        for arg in ty.walk() {
            if let GenericArgKind::Lifetime(r) = arg.kind() {
                if r.is_var() {
                    regions.insert(r.as_var());
                }
            }
        }
        let facts = bwf.input_facts.as_ref()?;
        let p_output = Output::compute(facts, Algorithm::Naive, true);
        let mut held: BTreeSet<Loan> = BTreeSet::new();
        #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "collected into a BTreeSet")]
        for origins in p_output.origin_contains_loan_at.values() {
            for (origin, loans) in origins.iter() {
                if regions.contains(&RegionVid::from(*origin)) {
                    held.extend(loans.iter().copied());
                }
            }
        }
        Some(held)
    }

    /// `loans_held_by`, split by where in `name`'s type the loans are
    /// (#1399). `own`: the variables (with whether a loan is mutable)
    /// whose loans are in a lifetime of the value itself, and not only
    /// there because they are in a lifetime of something it reaches
    /// through a borrow. `through`: the variables whose loans are only
    /// in what it reaches that way. `let el = refs.iter_mut().next()
    /// .unwrap();` (refs: `Vec<&String>`) gives el the type `&'e mut
    /// &'x String`: its own lifetime `'e` holds the mutable loan of refs,
    /// and the loan of a1 that is in `'x` (the `&String` el points to,
    /// in refs) too, since `'x` outlives `'e`. a1 is only reached through
    /// el: `own` is `{refs: mut}`, `through` is `{a1}`. Likewise for the
    /// `IterMut<'r, &'x String>` that `refs.iter_mut()` returns, whose
    /// `T` the iterator reaches only through its `'r` (see
    /// `params_behind_borrows`). `None` when `loans_held_by` is.
    pub fn loans_split_by(&self, name: &str, fields: &[&str]) -> Option<(BTreeMap<String, bool>, BTreeSet<String>)> {
        let bwf = self.bwf;
        let body = &bwf.body;
        let local = self.raps.local(name)?;
        let mut own: BTreeSet<RegionVid> = BTreeSet::new();
        let mut through: BTreeSet<RegionVid> = BTreeSet::new();
        let mut ty = body.local_decls[local].ty;
        for f in fields {
            ty = self.field_ty_named(ty, f)?;
        }
        self.split_regions(ty, false, &mut own, &mut through, 0);
        let facts = bwf.input_facts.as_ref()?;
        let p_output = Output::compute(facts, Algorithm::Naive, true);
        let mut own_loans: BTreeSet<Loan> = BTreeSet::new();
        let mut through_loans: BTreeSet<Loan> = BTreeSet::new();
        #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "collected into BTreeSets")]
        for origins in p_output.origin_contains_loan_at.values() {
            for (origin, loans) in origins.iter() {
                let r = RegionVid::from(*origin);
                if own.contains(&r) {
                    own_loans.extend(loans.iter().copied());
                }
                if through.contains(&r) {
                    through_loans.extend(loans.iter().copied());
                }
            }
        }
        let names = &self.raps.bindings.locals;
        let name_of = |loan: Loan| names.get(&bwf.borrow_set[loan].borrowed_place().local).cloned();
        let mut out_own: BTreeMap<String, bool> = BTreeMap::new();
        for loan in own_loans.difference(&through_loans) {
            if let Some(n) = name_of(*loan) {
                *out_own.entry(n).or_insert(false) |= bwf.borrow_set[*loan].kind().mutability().is_mut();
            }
        }
        let out_through: BTreeSet<String> = through_loans.iter()
            .filter_map(|l| name_of(*l))
            .filter(|n| !out_own.contains_key(n))
            .collect();
        Some((out_own, out_through))
    }

    /// Sorts the lifetimes in `ty` into those of the value itself
    /// (`own`) and those of what it reaches through a borrow
    /// (`through`): in `&'e mut &'x String`, `'e` is el's own and `'x`
    /// is the `&String` it points to. A struct's type arguments are its
    /// own (`Option<&'a String>` holds the reference), except the ones
    /// it holds only behind one of its lifetimes (`T` in `IterMut<'a,
    /// T>`, see `params_behind_borrows`).
    fn split_regions(&self, ty: Ty<'tcx>, behind: bool, own: &mut BTreeSet<RegionVid>, through: &mut BTreeSet<RegionVid>, depth: usize) {
        fn add(r: rustc_middle::ty::Region<'_>, behind: bool, own: &mut BTreeSet<RegionVid>, through: &mut BTreeSet<RegionVid>) {
            if r.is_var() {
                if behind { through.insert(r.as_var()); } else { own.insert(r.as_var()); }
            }
        }
        if depth > 8 {
            for g in ty.walk() {
                if let GenericArgKind::Lifetime(r) = g.kind() { add(r, behind, own, through); }
            }
            return;
        }
        match ty.kind() {
            TyKind::Ref(r, inner, _) => {
                add(*r, behind, own, through);
                self.split_regions(*inner, true, own, through, depth + 1);
            }
            TyKind::Adt(def, args) => {
                let inside = self.params_behind_borrows(*def);
                for (i, g) in args.iter().enumerate() {
                    match g.kind() {
                        GenericArgKind::Lifetime(r) => add(r, behind, own, through),
                        GenericArgKind::Type(t) => {
                            let b = behind || inside.get(i).copied().unwrap_or(false);
                            self.split_regions(t, b, own, through, depth + 1);
                        }
                        GenericArgKind::Const(_) => {}
                    }
                }
            }
            TyKind::Tuple(tys) => {
                for t in tys.iter() {
                    self.split_regions(t, behind, own, through, depth + 1);
                }
            }
            TyKind::Array(t, _) | TyKind::Slice(t) => self.split_regions(*t, behind, own, through, depth + 1),
            _ => {
                for g in ty.walk() {
                    if let GenericArgKind::Lifetime(r) = g.kind() { add(r, behind, own, through); }
                }
            }
        }
    }

    /// For each generic parameter of the struct or enum `def` (by
    /// position): whether its values hold that parameter only behind a
    /// borrow of one of their lifetimes. `slice::IterMut<'a, T>` holds
    /// raw pointers to the `T`s and a `PhantomData<&'a mut T>`, so `T` is
    /// behind `'a`; `Vec<T>` (`PhantomData<T>`), `Option<T>` and
    /// `str::Split<'a, P>` hold theirs by value. Raw pointers say neither
    /// (their `PhantomData` does); a type with no lifetime parameter holds
    /// nothing behind one.
    pub(crate) fn params_behind_borrows(&self, def: rustc_middle::ty::AdtDef<'tcx>) -> Vec<bool> {
        let tcx = self.tcx;
        let generics = tcx.generics_of(def.did());
        let n = generics.count();
        let has_lifetime = generics.own_params.iter()
            .any(|p| matches!(p.kind, rustc_middle::ty::GenericParamDefKind::Lifetime));
        if !has_lifetime {
            return vec![false; n];
        }
        let mut by_value = vec![false; n];
        let mut behind = vec![false; n];
        let args = rustc_middle::ty::GenericArgs::identity_for_item(tcx, def.did());
        let mut visiting = vec![def.did()];
        for f in def.all_fields() {
            self.param_uses(f.ty(tcx, args), false, &mut by_value, &mut behind, &mut visiting);
        }
        (0..n).map(|i| behind[i] && !by_value[i]).collect()
    }

    /// Records in `by_value` / `behind` (see `params_behind_borrows`)
    /// how the type `ty` of a field (or of a part of one) holds the
    /// parameters it mentions.
    fn param_uses(&self, ty: Ty<'tcx>, under: bool, by_value: &mut Vec<bool>, behind: &mut Vec<bool>, visiting: &mut Vec<rustc_span::def_id::DefId>) {
        let tcx = self.tcx;
        let mark = |i: u32, under: bool, by_value: &mut Vec<bool>, behind: &mut Vec<bool>| {
            let slot = if under { behind } else { by_value };
            if let Some(x) = slot.get_mut(i as usize) { *x = true; }
        };
        match ty.kind() {
            TyKind::Param(p) => mark(p.index, under, by_value, behind),
            TyKind::Ref(_, inner, _) => self.param_uses(*inner, true, by_value, behind, visiting),
            TyKind::RawPtr(..) => {}
            // `pattern_type!(*const T is !null)` in `NonNull<T>`.
            TyKind::Pat(base, _) => self.param_uses(*base, under, by_value, behind, visiting),
            TyKind::Adt(d, a) if d.is_phantom_data() || d.is_box() => {
                for t in a.types() {
                    self.param_uses(t, under, by_value, behind, visiting);
                }
            }
            TyKind::Adt(d, a) => {
                if visiting.contains(&d.did()) || visiting.len() > 6 {
                    // (A recursive type, or one nested too deep: as by value.)
                    for g in ty.walk() {
                        if let GenericArgKind::Type(t) = g.kind() {
                            if let TyKind::Param(p) = t.kind() { mark(p.index, under, by_value, behind); }
                        }
                    }
                    return;
                }
                visiting.push(d.did());
                for f in d.all_fields() {
                    self.param_uses(f.ty(tcx, a), under, by_value, behind, visiting);
                }
                visiting.pop();
            }
            TyKind::Tuple(tys) => {
                for t in tys.iter() {
                    self.param_uses(t, under, by_value, behind, visiting);
                }
            }
            TyKind::Array(t, _) | TyKind::Slice(t) => self.param_uses(*t, under, by_value, behind, visiting),
            _ => {
                for g in ty.walk() {
                    if let GenericArgKind::Type(t) = g.kind() {
                        if let TyKind::Param(p) = t.kind() { mark(p.index, under, by_value, behind); }
                    }
                }
            }
        }
    }

    /// Whether this body has a MIR local for the variable `name` (its
    /// RAP column): a borrow of it is one of
    /// rustc's loans here.
    pub fn is_body_local(&self, name: &str) -> bool {
        self.raps.bindings.columns.contains_key(name)
    }

    /// The type of field `f` (a name, or a tuple index) of a struct or
    /// tuple type `ty`, with `ty`'s own lifetimes.
    pub(crate) fn field_ty_named(&self, ty: Ty<'tcx>, f: &str) -> Option<Ty<'tcx>> {
        match ty.kind() {
            TyKind::Adt(adt, args) if adt.is_struct() => adt.non_enum_variant().fields.iter()
                .find(|fd| fd.name.as_str() == f)
                .map(|fd| fd.ty(self.tcx, args)),
            TyKind::Tuple(tys) => f.parse::<usize>().ok().and_then(|i| tys.get(i).copied()),
            _ => None,
        }
    }


    /// The loan issued at exactly the `&x` expression `r` was assigned
    /// from (#233). Only the kill line is used, as in `borrow_match`.
    pub fn borrow_span_match(r: &RefData, b: &MIRBorrowData) -> bool {
        r.borrow_span.is_some_and(|sp| sp == b.span)
    }

    // Helper function to help refine loan regions that we compute in HIR
    pub fn borrow_match(r: &RefData, b: &MIRBorrowData) -> Option<usize> {
        let mir_place = b.borrowed_place.as_ref()?;
        // A reference that took over a call's loan (#664) holds the
        // one issued on that call's line, not on its own.
        if b.region.0 != r.loan_line.unwrap_or(r.assigned_at) {
            return None;
        }
        let lender_name = r.lender.real_name();
        // MIR's `borrowed_place().local_or_deref_local()` only gives
        // back the base Local for a place — for `&r.a.b` it returns
        // the name `r`, not `r.a.b`. Our lender name is the full
        // dotted path (`r.a.b`). Accept a match when MIR's name is
        // either exactly equal OR is a path prefix terminated by `.`
        // (so `r` matches `r.a.b` but not `roar.a.b`). Without this
        // any borrow rooted at a struct field skips MIR refinement
        // and the lifetime stays pinned to the assignment line —
        // which makes the dotted ref-line render with zero height
        // when the only use of the borrower is inside a macro
        // (where visit_expr can't reach the Path to extend it
        // through update_rap).
        if *mir_place == lender_name {
            return Some(b.region.1);
        }
        if lender_name.starts_with(mir_place)
            && lender_name.as_bytes().get(mir_place.len()) == Some(&b'.')
        {
            return Some(b.region.1);
        }
        None
    }

}
