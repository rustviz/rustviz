//! Ideally we would like to do all of our analysis in the MIR since it is a lot simpler than the HIR
//! since events like drops, borrows and moves are stated explicitly. Additionally, the MIR is less complicated.
//! If you'd like to work on converting more and more of the RustViz logic from the HIR to the MIR I would recommend reading:
//! https://rustc-dev-guide.rust-lang.org/ especially the sections on the MIR, dataflow analysis, borrow checker, drop elaboration
//! Also read through this: https://github.com/rust-lang/polonius/blob/master/book/src/SUMMARY.md
//! Unfortunately though, I don't believe it would be possible to do everything in the MIR, for example: how would 
//! you figure out when a copy occurs? You can't use the assignment operator - since a single HIR instr can be compiled down into
//! multiple MIR instructions.

use crate::{expr_visitor::*, expr_visitor_utils::{binding_names, span_end_line, span_to_line, written_name}};
use crate::svg_generator::data::{ExternalEvent, ResourceAccessPoint, ScopeEndHolds};
use log::info;
use rustc_middle::mir::{BorrowKind, Location, Local, Operand, Place, PlaceElem, ProjectionElem, Rvalue, StatementKind, TerminatorKind, VarDebugInfoContents};
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

/// The elements of the array `local` (of length `len`) that are moved
/// out one by one (`move _1[0 of 2]`, `move _1[1..2]`), and whether the
/// whole array is moved or assigned somewhere (then the elements' fate
/// isn't one fixed set).
struct ArrayElemMoves { local: Local, len: u64, moved: BTreeSet<u64>, whole: bool, assigns: usize, at: Vec<(Location, u64)> }

impl<'tcx> Visitor<'tcx> for ArrayElemMoves {
    fn visit_operand(&mut self, op: &Operand<'tcx>, loc: Location) {
        if let Operand::Move(place) = op {
            if place.local == self.local {
                match place.projection.first() {
                    None => self.whole = true,
                    Some(ProjectionElem::ConstantIndex { offset, from_end, .. }) => {
                        let i = if *from_end { self.len.saturating_sub(*offset) } else { *offset };
                        self.moved.insert(i);
                        self.at.push((loc, i));
                    }
                    Some(ProjectionElem::Subslice { from, to, from_end }) => {
                        let end = if *from_end { self.len.saturating_sub(*to) } else { *to };
                        self.moved.extend(*from..end);
                        self.at.extend((*from..end).map(|i| (loc, i)));
                    }
                    _ => {}
                }
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

    /// For `name`, an array `[T; N]` whose `T` needs drop, from which
    /// patterns move some elements out but leave others (`let [a, _] =
    /// arr;`, `[first, ..] =>`): which elements are moved and which are
    /// left in it and dropped at its scope end (#1144, #1166). None if
    /// none is moved, or the array is also moved or assigned as a whole,
    /// or one pattern moves every element (`let [a, b] = arr;`: a move
    /// of all of it). Several patterns that together move every element
    /// (`let [a, _] = arr; let [_, b] = arr;`, #1302) leave nothing in
    /// it: `ElemsLeft::last` names the elements whose move empties it.
    pub fn array_elems_left(&self, name: &str, ty: Ty<'tcx>) -> Option<crate::svg_generator::data::ElemsLeft> {
        let TyKind::Array(elem, len) = ty.kind() else { return None };
        let len = len.try_to_target_usize(self.tcx)?;
        if !crate::expr_visitor_utils::needs_drop_in(self.tcx, *elem, rustc_middle::ty::TypingEnv::fully_monomorphized()) {
            return None;
        }
        let names = self.mir_local_names(self.bwf);
        #[expect(clippy::disallowed_methods, reason = "a name is bound to one local; min() makes order irrelevant")]
        let local = names.iter().filter(|(_, n)| n.as_str() == name).map(|(l, _)| *l).min()?;
        let mut v = ArrayElemMoves { local, len, moved: BTreeSet::new(), whole: false, assigns: 0, at: Vec::new() };
        v.visit_body(&self.bwf.body);
        if v.whole || v.assigns > 1 || v.moved.is_empty() {
            return None;
        }
        let elem_name = |i: u64| format!("{}[{}]", name, i);
        let mut last = Vec::new();
        if v.moved.len() as u64 >= len {
            // Every element is moved, by patterns on more than one line
            // (else it is one pattern taking all of it). The array is
            // empty once the last of them has run: the moves on the last
            // line, if every other move comes before them on every path
            // to them (in a `match`'s arms, no move empties it).
            let body = &self.bwf.body;
            let line_of = |l: Location| span_to_line(&self.location_to_span(l, self.bwf), &self.tcx);
            let lines: BTreeSet<usize> = v.at.iter().map(|(l, _)| line_of(*l)).collect();
            let last_line = *lines.last()?;
            if lines.len() < 2 {
                return None;
            }
            let doms = body.basic_blocks.dominators();
            let (tail, rest): (Vec<_>, Vec<_>) = v.at.iter().partition(|(l, _)| line_of(*l) == last_line);
            if rest.iter().all(|(r, _)| tail.iter().all(|(t, _)| r.dominates(*t, doms))) {
                last = tail.iter().map(|(_, i)| *i).collect::<BTreeSet<u64>>().into_iter().map(elem_name).collect();
            }
        }
        Some(crate::svg_generator::data::ElemsLeft {
            moved: v.moved.iter().map(|&i| elem_name(i)).collect(),
            left: (0..len).filter(|i| !v.moved.contains(i)).map(elem_name).collect(),
            last,
        })
    }

    /// Is `name`, a `Box<T>`, emptied by a move of its whole contents
    /// somewhere in this body (`take(*b)`)? Its heap allocation is
    /// still freed at scope end (#609).
    pub fn box_contents_moved_out(&self, name: &str, ty: Ty<'tcx>) -> bool {
        if !ty.is_box() { return false; }
        let names = self.mir_local_names(self.bwf);
        #[expect(clippy::disallowed_methods, reason = "a name is bound to one local; min() makes order irrelevant")]
        let Some(local) = names.iter().filter(|(_, n)| n.as_str() == name).map(|(l, _)| *l).min() else { return false };
        let mut v = BoxContentsMoves { local, found: false };
        v.visit_body(&self.bwf.body);
        v.found
    }

    /// For a `Box<T>` variable `name` whose `T` has field columns and
    /// from which a field is moved out somewhere in this body
    /// (`take(b.name)`): `T`. Rust allows that move out of a `Box`
    /// only, not out of other pointers (#602).
    pub fn box_with_field_moved_out(&self, name: &str, ty: Ty<'tcx>) -> Option<Ty<'tcx>> {
        if !ty.is_box() { return None; }
        let inner = ty.boxed_ty()?;
        if !self.has_member_columns(inner) { return None; }
        let names = self.mir_local_names(self.bwf);
        #[expect(clippy::disallowed_methods, reason = "a name is bound to one local; min() makes order irrelevant")]
        let local = names.iter().filter(|(_, n)| n.as_str() == name).map(|(l, _)| *l).min()?;
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

    fn src_name_of_local(&self, l: Option<Local>, l_map: &HashMap<Local, String>) -> Option<String> {
        match l {
            Some(loc) => {
                match l_map.get(&loc) {
                    Some(name) => Some(name.clone()),
                    None => None
                }
            }
            None => None
        }
    }


    /// Name of each user variable's MIR local, as used for its RAP
    /// (`s₂` for a second `let s`, see `binding_name`). rustc_utils'
    /// `debug_info_name_map` is keyed by the source name, so it kept
    /// only one of two shadowing `s`es; here each local is matched to
    /// its binding through the binding's span.
    fn mir_local_names(&self, body_with_facts: &BodyWithBorrowckFacts) -> HashMap<Local, String> {
        let body = &body_with_facts.body;
        let owner = self.tcx.local_def_id_to_hir_id(body.source.def_id().expect_local()).owner;
        let names = binding_names(&self.tcx, owner);
        let mut res: HashMap<Local, String> = HashMap::new();
        for vdi in body.var_debug_info.iter() {
            let VarDebugInfoContents::Place(place) = vdi.value else { continue };
            let span = vdi.source_info.span;
            let ident = vdi.name.as_str();
            let name = names.by_span.iter()
                .find(|(sp, n)| span.contains(*sp)
                    && written_name(n) == ident)
                .map_or_else(|| ident.to_owned(), |(_, n)| n.clone());
            // If two names share a Local (closure captures all live in
            // `_1`), the same one wins on every run (#211): the greatest,
            // as before.
            match res.get(&place.local) {
                Some(prev) if *prev >= name => {}
                _ => { res.insert(place.local, name); }
            }
        }
        res
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
    pub fn dead_loan_at_write(&self, lender: &str, line: usize) -> Option<DeadLoanWrite> {
        let bwf = self.bwf;
        let body = &bwf.body;
        let names = self.mir_local_names(bwf);
        let x = body.local_decls.indices().find(|l| names.get(l).is_some_and(|n| n == lender))?;
        let no_deref = |p: &Place<'tcx>| !p.projection.iter().any(|e| matches!(e, ProjectionElem::Deref));
        // The writes at `line`.
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
                if no_deref(&place) && self.location_to_line(loc, bwf) == line {
                    writes.push((loc, place));
                }
            }
            let loc = Location { block, statement_index: data.statements.len() };
            if let TerminatorKind::Call { destination, .. } = &data.terminator().kind {
                if destination.local == x && no_deref(destination) && self.location_to_line(loc, bwf) == line {
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
                    if l_line >= line { live_later.push(*b); }
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
            return Some(DeadLoanWrite { borrower: borrower.clone(), repoint });
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
        let loc_to_source_name = self.mir_local_names(body_with_facts);
        
        
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
                let b_name = self.src_name_of_local(b_place, &loc_to_source_name);
                let a_name = self.src_name_of_local(a_place, &loc_to_source_name);

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
        let names = self.mir_local_names(bwf);
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
        let names = self.mir_local_names(bwf);
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

    /// The user variables, and fields of them (RAP names, sorted), that
    /// hold their value on every way into the loop `loop_span` but may
    /// not where the loop comes back round to its start (#992): a move
    /// further down the body, or before a `continue`, that nothing
    /// after gave a new value. rustc's maybe-uninitialized places at
    /// the loop's head (its `FalseUnwind` block), split by the edges
    /// into it: from before the loop (not dominated by the head), and
    /// back from the body. Empty when the loop isn't in this body (a
    /// closure's).
    pub fn moved_round_loop(&self, loop_span: Span) -> Vec<String> {
        use rustc_mir_dataflow::impls::MaybeUninitializedPlaces;
        use rustc_mir_dataflow::move_paths::MoveData;
        let Some((entry, back)) = self.loop_head_edges(loop_span) else { return Vec::new() };
        let body = &self.bwf.body;
        let move_data = MoveData::gather_moves(body, self.tcx, |_| true);
        let mut cursor = MaybeUninitializedPlaces::new(self.tcx, body, &move_data)
            .iterate_to_fixpoint(self.tcx, body, None)
            .into_results_cursor(body);
        let mut out: Vec<String> = Vec::new();
        for (mpi, name) in self.move_path_names(&move_data) {
            let mut uninit_at_end = |bb: rustc_middle::mir::BasicBlock| {
                cursor.seek_to_block_end(bb);
                cursor.get().contains(mpi)
            };
            if !entry.iter().any(|&p| uninit_at_end(p)) && back.iter().any(|&p| uninit_at_end(p)) {
                out.push(name);
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// The mirror of `moved_round_loop` (#1001): the user variables,
    /// and fields of them, that may hold a value where the loop
    /// `loop_span` comes back round to its start: the body gave them a
    /// value that nothing after moved on every path. rustc's
    /// maybe-initialized places at the ends of the edges back to the
    /// loop's head. (The caller keeps those that hold none where the
    /// loop starts: moved before it. One on a way into the loop may
    /// hold one there too, from an outer loop's earlier time round.)
    pub fn given_round_loop(&self, loop_span: Span) -> Vec<String> {
        use rustc_mir_dataflow::impls::MaybeInitializedPlaces;
        use rustc_mir_dataflow::move_paths::MoveData;
        let Some((_, back)) = self.loop_head_edges(loop_span) else { return Vec::new() };
        let body = &self.bwf.body;
        let move_data = MoveData::gather_moves(body, self.tcx, |_| true);
        let mut cursor = MaybeInitializedPlaces::new(self.tcx, body, &move_data)
            .iterate_to_fixpoint(self.tcx, body, None)
            .into_results_cursor(body);
        let mut out: Vec<String> = Vec::new();
        for (mpi, name) in self.move_path_names(&move_data) {
            let mut init_at_end = |bb: rustc_middle::mir::BasicBlock| {
                cursor.seek_to_block_end(bb);
                cursor.get().contains(mpi)
            };
            if back.iter().any(|&p| init_at_end(p)) {
                out.push(name);
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// What each user variable (and field of one) holds where the loop
    /// `loop_span` comes back round to its start (#1012): `Held` when
    /// rustc's dataflow says it's initialized on every edge back to the
    /// loop's head, `None` when on none, `Maybe` otherwise. With each,
    /// whether the loop moves anything out of its variable (a `Drop`
    /// before an assignment isn't a move). Empty when the loop isn't in
    /// this body or never comes back round.
    pub fn round_states_loop(&self, loop_span: Span) -> Vec<(String, Ownership, bool)> {
        use rustc_mir_dataflow::impls::{MaybeInitializedPlaces, MaybeUninitializedPlaces};
        use rustc_mir_dataflow::move_paths::MoveData;
        let Some((_, back)) = self.loop_head_edges(loop_span) else { return Vec::new() };
        let body = &self.bwf.body;
        let move_data = MoveData::gather_moves(body, self.tcx, |_| true);
        // The loop's blocks: its head, and those that lead back to it
        // without passing it (the natural loop of its back edges).
        let Some(head) = body.basic_blocks.iter_enumerated().find_map(|(bb, data)| {
            let t = data.terminator.as_ref()?;
            (matches!(t.kind, TerminatorKind::FalseUnwind { .. }) && t.source_info.span == loop_span).then_some(bb)
        }) else { return Vec::new() };
        let preds = body.basic_blocks.predecessors();
        let mut in_loop: BTreeSet<rustc_middle::mir::BasicBlock> = back.iter().copied().collect();
        in_loop.insert(head);
        let mut work: Vec<rustc_middle::mir::BasicBlock> = back.clone();
        while let Some(bb) = work.pop() {
            for &p in &preds[bb] {
                if in_loop.insert(p) {
                    work.push(p);
                }
            }
        }
        // The variables (locals) the loop moves something out of: not a
        // `Drop` before an assignment, nor a `StorageDead`.
        let moved_locals: BTreeSet<Local> = move_data.moves.iter()
            .filter(|m| in_loop.contains(&m.source.block))
            .filter(|m| {
                let data = &body.basic_blocks[m.source.block];
                match data.statements.get(m.source.statement_index) {
                    Some(st) => !matches!(st.kind, rustc_middle::mir::StatementKind::StorageDead(_)),
                    None => !matches!(data.terminator().kind, TerminatorKind::Drop { .. }),
                }
            })
            .map(|m| move_data.move_paths[m.path].place.local)
            .collect();
        let mut init = MaybeInitializedPlaces::new(self.tcx, body, &move_data)
            .iterate_to_fixpoint(self.tcx, body, None)
            .into_results_cursor(body);
        let mut uninit = MaybeUninitializedPlaces::new(self.tcx, body, &move_data)
            .iterate_to_fixpoint(self.tcx, body, None)
            .into_results_cursor(body);
        let mut out: Vec<(String, Ownership, bool)> = Vec::new();
        for (mpi, name) in self.move_path_names(&move_data) {
            let moved = moved_locals.contains(&move_data.move_paths[mpi].place.local);
            let may_hold = back.iter().any(|&p| { init.seek_to_block_end(p); init.get().contains(mpi) });
            let may_lack = back.iter().any(|&p| { uninit.seek_to_block_end(p); uninit.get().contains(mpi) });
            let own = match (may_hold, may_lack) {
                (true, false) => Ownership::Held,
                (false, _) => Ownership::None,
                (true, true) => Ownership::Maybe,
            };
            if !out.iter().any(|(n, ..)| *n == name) {
                out.push((name, own, moved));
            }
        }
        out
    }

    /// What each user variable (and field of one) holds where the loop
    /// `loop_span` is entered from before it (#1019), as for
    /// `round_states_loop` but on the edges into its head that aren't
    /// back edges. For a loop inside another, that's every run of it:
    /// the outer loop's later times round come in this way too. Empty
    /// when the loop isn't in this body (not when it never comes back
    /// round, #1467).
    pub fn entry_states_loop(&self, loop_span: Span) -> Vec<(String, Ownership)> {
        use rustc_mir_dataflow::impls::{MaybeInitializedPlaces, MaybeUninitializedPlaces};
        use rustc_mir_dataflow::move_paths::MoveData;
        let Some((entry, _)) = self.loop_head_edges_all(loop_span).filter(|(e, _)| !e.is_empty()) else { return Vec::new() };
        let body = &self.bwf.body;
        let move_data = MoveData::gather_moves(body, self.tcx, |_| true);
        let mut init = MaybeInitializedPlaces::new(self.tcx, body, &move_data)
            .iterate_to_fixpoint(self.tcx, body, None)
            .into_results_cursor(body);
        let mut uninit = MaybeUninitializedPlaces::new(self.tcx, body, &move_data)
            .iterate_to_fixpoint(self.tcx, body, None)
            .into_results_cursor(body);
        let mut out: Vec<(String, Ownership)> = Vec::new();
        for (mpi, name) in self.move_path_names(&move_data) {
            let may_hold = entry.iter().any(|&p| { init.seek_to_block_end(p); init.get().contains(mpi) });
            let may_lack = entry.iter().any(|&p| { uninit.seek_to_block_end(p); uninit.get().contains(mpi) });
            let own = match (may_hold, may_lack) {
                (true, false) => Ownership::Held,
                (false, _) => Ownership::None,
                (true, true) => Ownership::Maybe,
            };
            if !out.iter().any(|(n, _)| *n == name) {
                out.push((name, own));
            }
        }
        out
    }

    /// What the user variable (or field of one) `name` holds where the
    /// assignment `assign` (its span; `lhs`, its left-hand side's)
    /// overwrites it (#1027): rustc's dataflow just before the `Drop` of
    /// the old value that the assignment starts with (MIR gives it the
    /// left-hand side's span), or, failing that, the write itself.
    /// `None` when no such place in the MIR is found.
    pub fn ownership_before_assign(&self, name: &str, lhs: Span, assign: Span) -> Option<Ownership> {
        use rustc_middle::mir::StatementKind;
        use rustc_mir_dataflow::impls::{MaybeInitializedPlaces, MaybeUninitializedPlaces};
        use rustc_mir_dataflow::move_paths::MoveData;
        let body = &self.bwf.body;
        let move_data = MoveData::gather_moves(body, self.tcx, |_| true);
        let paths = self.move_path_names(&move_data);
        let root = name.split('.').next().unwrap_or(name);
        let mpi = paths.iter().find(|(_, n)| n == name).or_else(|| paths.iter().find(|(_, n)| n == root))?.0;
        let local = move_data.move_paths[mpi].place.local;
        let blocks = || body.basic_blocks.iter_enumerated();
        let dropped = blocks().find_map(|(bb, data)| {
            let t = data.terminator.as_ref()?;
            matches!(&t.kind, TerminatorKind::Drop { place, .. } if place.local == local && t.source_info.span == lhs)
                .then(|| Location { block: bb, statement_index: data.statements.len() })
        });
        let at = dropped.or_else(|| blocks().find_map(|(bb, data)| {
            data.statements.iter().position(|st| matches!(&st.kind,
                StatementKind::Assign(b) if b.0.local == local && st.source_info.span == assign))
                .map(|i| Location { block: bb, statement_index: i })
        }));
        let at = at?;
        let mut init = MaybeInitializedPlaces::new(self.tcx, body, &move_data)
            .iterate_to_fixpoint(self.tcx, body, None)
            .into_results_cursor(body);
        let mut uninit = MaybeUninitializedPlaces::new(self.tcx, body, &move_data)
            .iterate_to_fixpoint(self.tcx, body, None)
            .into_results_cursor(body);
        init.seek_before_primary_effect(at);
        uninit.seek_before_primary_effect(at);
        Some(match (init.get().contains(mpi), uninit.get().contains(mpi)) {
            (true, false) => Ownership::Held,
            (false, _) => Ownership::None,
            (true, true) => Ownership::Maybe,
        })
    }

    /// Fill in, on the scope ends among `preprocessed_events[from..]`
    /// (this body's events, also those in a conditional's arms), what
    /// rustc's move data says the variable holds there
    /// (`ownership_at_scope_ends`, #1036, #1037). Only a plain owner's:
    /// not a struct's (its fields are drawn on their own), nor a
    /// reference's.
    pub fn fill_scope_end_holds(&mut self, from: usize) {
        fn queries(events: &[(usize, ExternalEvent)], out: &mut Vec<(String, usize)>) {
            for (line, e) in events {
                match e {
                    ExternalEvent::GoOutOfScope { ro: ro @ ResourceAccessPoint::Owner(_), .. } => {
                        let q = (ro.name().clone(), *line);
                        if !out.contains(&q) {
                            out.push(q);
                        }
                    }
                    ExternalEvent::Branch { branches, .. } => branches.iter().for_each(|b| queries(&b.e_data, out)),
                    _ => {}
                }
            }
        }
        fn fill(events: &mut [(usize, ExternalEvent)], answers: &BTreeMap<(String, usize), ScopeEndHolds>) {
            for (line, e) in events.iter_mut() {
                match e {
                    ExternalEvent::GoOutOfScope { ro: ro @ ResourceAccessPoint::Owner(_), holds, .. } => {
                        *holds = answers.get(&(ro.name().clone(), *line)).copied();
                    }
                    ExternalEvent::Branch { branches, .. } => branches.iter_mut().for_each(|b| fill(&mut b.e_data, answers)),
                    _ => {}
                }
            }
        }
        let mut qs = Vec::new();
        queries(&self.preprocessed_events[from..], &mut qs);
        let found = self.ownership_at_scope_ends(&qs);
        let answers: BTreeMap<(String, usize), ScopeEndHolds> = qs.into_iter().zip(found)
            .filter_map(|(q, own)| own.map(|o| (q, match o {
                Ownership::Held => ScopeEndHolds::Value,
                Ownership::Maybe => ScopeEndHolds::Maybe,
                Ownership::None => ScopeEndHolds::Nothing,
            })))
            .collect();
        fill(&mut self.preprocessed_events[from..], &answers);
    }

    /// What each variable of `queries` (its RAP name, and the line of
    /// the `}` where it goes out of scope) holds there, on the paths
    /// that get to that `}` through the code of the block it closes
    /// (#1036, #1037): rustc's dataflow just before the `Drop`s of it
    /// that MIR puts at that `}`. A `return` (or `?`), or a `break` or
    /// `continue` that leaves its block, drops it on the way out with a
    /// `Drop` MIR gives the same span, and so does a let-chain whose
    /// condition fails after binding it (`if let Some(a) = x && … {`,
    /// whose `}` it is): those are left out. `Held` when it holds a
    /// value on every path left, `None` when on none, `Maybe` when a
    /// runtime drop flag decides. No answer (`None`) when no such
    /// `Drop` is found (its type needs no drop, it's a closure's
    /// variable, or the `}` isn't a block's), more than one variable of
    /// that name is dropped there, something was moved out of a part of
    /// it (a field, a Box's contents), or a `Drop` there is reached both
    /// ways.
    pub fn ownership_at_scope_ends(&self, queries: &[(String, usize)]) -> Vec<Option<Ownership>> {
        use rustc_middle::mir::{BasicBlock, RETURN_PLACE, StatementKind};
        use rustc_mir_dataflow::impls::{MaybeInitializedPlaces, MaybeUninitializedPlaces};
        use rustc_mir_dataflow::move_paths::MoveData;
        if queries.is_empty() {
            return Vec::new();
        }
        let body = &self.bwf.body;
        // The body's blocks, and its `return`s, `break`s and
        // `continue`s, each with the loop or labeled block a `break` or
        // `continue` leaves (`None` for a `return`). Not those of a
        // closure inside.
        struct Hir<'tcx> { tcx: rustc_middle::ty::TyCtxt<'tcx>, blocks: Vec<Span>, exits: Vec<(Span, Option<Span>)> }
        impl<'tcx> intravisit::Visitor<'tcx> for Hir<'tcx> {
            fn visit_block(&mut self, b: &'tcx rustc_hir::Block<'tcx>) {
                self.blocks.push(b.span);
                intravisit::walk_block(self, b);
            }
            fn visit_expr(&mut self, e: &'tcx rustc_hir::Expr<'tcx>) {
                match e.kind {
                    rustc_hir::ExprKind::Ret(_) => self.exits.push((e.span, None)),
                    rustc_hir::ExprKind::Break(dest, _) | rustc_hir::ExprKind::Continue(dest) => {
                        if let Ok(id) = dest.target_id {
                            self.exits.push((e.span, Some(self.tcx.hir_span(id))));
                        }
                    }
                    _ => {}
                }
                intravisit::walk_expr(self, e);
            }
        }
        let mut hir = Hir { tcx: self.tcx, blocks: Vec::new(), exits: Vec::new() };
        intravisit::Visitor::visit_expr(&mut hir, self.hir_body.value);
        let preds = body.basic_blocks.predecessors();
        // Whether code at `sp` leaves the scope of a variable declared
        // at `decl`.
        let leaves = |sp: Span, decl: Span| hir.exits.iter().any(|(e, from)| e.contains(sp) && from.is_none_or(|f| f.contains(decl)));
        // A `?`'s early return: the call that makes the return value
        // from the error (its span is the whole `x?`, not the `?`'s).
        let residual = |t: &rustc_middle::mir::Terminator<'tcx>| matches!(&t.kind,
            TerminatorKind::Call { func, destination, .. } if destination.local == RETURN_PLACE
                && func.const_fn_def().is_some_and(|(def, _)| self.tcx.item_name(def).as_str() == "from_residual"));
        // How the `Drop` that ends block `bb` is reached, walking back
        // from it past what MIR adds on the way out (other `Drop`s,
        // `goto`s, `StorageDead`s) to the code before: (through the
        // code of `closes`, the block whose `}` it is; by a way out of
        // the scope of a variable declared at `decl`, or from code
        // outside that block).
        let reached = |bb: BasicBlock, decl: Span, closes: Span| -> (bool, bool) {
            let (mut inside, mut other) = (false, false);
            let mut seen: BTreeSet<BasicBlock> = BTreeSet::new();
            let mut work = vec![(bb, true)];
            while let Some((b, own)) = work.pop() {
                if !seen.insert(b) {
                    continue;
                }
                let data = &body.basic_blocks[b];
                let t = data.terminator();
                let code = if own {
                    None
                } else if residual(t) {
                    other = true;
                    continue;
                } else {
                    match t.kind {
                        TerminatorKind::Drop { .. } => None,
                        TerminatorKind::Goto { .. } => leaves(t.source_info.span, decl).then_some(t.source_info.span),
                        _ => Some(t.source_info.span),
                    }
                };
                let code = code.or_else(|| data.statements.iter().rev()
                    .find(|st| !matches!(st.kind, StatementKind::StorageDead(_) | StatementKind::StorageLive(_) | StatementKind::Nop))
                    .map(|st| st.source_info.span));
                match code {
                    Some(sp) if !leaves(sp, decl) && closes.contains(sp) => inside = true,
                    Some(_) => other = true,
                    None => {
                        let from: Vec<BasicBlock> = preds[b].iter().copied().filter(|p| !body.basic_blocks[*p].is_cleanup).collect();
                        if from.is_empty() {
                            other = true;
                        }
                        work.extend(from.into_iter().map(|p| (p, false)));
                    }
                }
            }
            (inside, other)
        };
        let names = self.mir_local_names(self.bwf);
        // What holds a value at run time, as drop elaboration sees it:
        // the borrow checker's `FalseEdge`s (its imaginary way past an
        // `if let` pattern that always matches, into a dead `else`;
        // #981, #987, #995) are gone by then (`CleanupPostBorrowck`), so
        // a value moved on every real path holds nothing at its `}`.
        let mut real = body.clone();
        for data in real.basic_blocks.as_mut().iter_mut() {
            if let Some(t) = data.terminator.as_mut() {
                if let TerminatorKind::FalseEdge { real_target, .. } = t.kind {
                    t.kind = TerminatorKind::Goto { target: real_target };
                }
            }
        }
        let real = &real;
        let move_data = MoveData::gather_moves(real, self.tcx, |_| true);
        let mut init = MaybeInitializedPlaces::new(self.tcx, real, &move_data)
            .iterate_to_fixpoint(self.tcx, real, None)
            .into_results_cursor(real);
        let mut uninit = MaybeUninitializedPlaces::new(self.tcx, real, &move_data)
            .iterate_to_fixpoint(self.tcx, real, None)
            .into_results_cursor(real);
        queries.iter().map(|(name, line)| {
            // The `Drop`s of a variable named `name` on that line.
            let drops: Vec<(BasicBlock, Local, Span)> = body.basic_blocks.iter_enumerated()
                .filter(|(_, data)| !data.is_cleanup)
                .filter_map(|(bb, data)| {
                    let t = data.terminator();
                    match &t.kind {
                        TerminatorKind::Drop { place, .. } if place.projection.is_empty()
                            && names.get(&place.local).is_some_and(|n| n == name)
                            && span_to_line(&t.source_info.span, &self.tcx) == *line => Some((bb, place.local, t.source_info.span)),
                        _ => None,
                    }
                })
                .collect();
            let local = drops.first()?.1;
            if drops.iter().any(|(_, l, _)| *l != local) {
                return None;
            }
            let mut paths = move_data.move_paths.iter_enumerated().filter(|(_, p)| p.place.local == local);
            let mpi = paths.next().filter(|(_, p)| p.place.projection.is_empty())?.0;
            if paths.next().is_some() {
                return None;
            }
            let decl = body.local_decls[local].source_info.span;
            let (mut may_hold, mut may_lack, mut found) = (false, false, false);
            for (bb, _, sp) in drops {
                // The innermost block whose `}` this is.
                let closes = hir.blocks.iter().filter(|b| b.hi() == sp.hi()).min_by_key(|b| b.hi() - b.lo())?;
                match reached(bb, decl, *closes) {
                    (true, true) => return None,
                    (true, false) => {}
                    (false, _) => continue,
                }
                let at = Location { block: bb, statement_index: body.basic_blocks[bb].statements.len() };
                init.seek_before_primary_effect(at);
                uninit.seek_before_primary_effect(at);
                may_hold |= init.get().contains(mpi);
                may_lack |= uninit.get().contains(mpi);
                found = true;
            }
            found.then_some(match (may_hold, may_lack) {
                (true, false) => Ownership::Held,
                (false, _) => Ownership::None,
                (true, true) => Ownership::Maybe,
            })
        }).collect()
    }

    /// The edges into the head of the loop `loop_span` (its
    /// `FalseUnwind` block): the blocks that enter it from before the
    /// loop (not dominated by the head), and those that come back to
    /// it from the body. `None` when the loop isn't in this body (a
    /// closure's) or either set is empty.
    fn loop_head_edges(&self, loop_span: Span) -> Option<(Vec<rustc_middle::mir::BasicBlock>, Vec<rustc_middle::mir::BasicBlock>)> {
        self.loop_head_edges_all(loop_span).filter(|(entry, back)| !entry.is_empty() && !back.is_empty())
    }

    /// Whether the loop `loop_span` never comes back round to its start
    /// (#1467): every pass through its body leaves it (a `break`, a
    /// `return`), so it runs its body at most once. `None` when the
    /// loop isn't in this body (a closure's).
    pub fn loop_never_rounds(&self, loop_span: Span) -> Option<bool> {
        self.loop_head_edges_all(loop_span).map(|(_, back)| back.is_empty())
    }

    /// `loop_head_edges`, also when either set is empty.
    fn loop_head_edges_all(&self, loop_span: Span) -> Option<(Vec<rustc_middle::mir::BasicBlock>, Vec<rustc_middle::mir::BasicBlock>)> {
        use rustc_middle::mir::BasicBlock;
        let body = &self.bwf.body;
        let head = body.basic_blocks.iter_enumerated().find_map(|(bb, data)| {
            let t = data.terminator.as_ref()?;
            (matches!(t.kind, TerminatorKind::FalseUnwind { .. }) && t.source_info.span == loop_span).then_some(bb)
        })?;
        let doms = body.basic_blocks.dominators();
        let (back, entry): (Vec<BasicBlock>, Vec<BasicBlock>) =
            body.basic_blocks.predecessors()[head].iter().partition(|&&p| doms.dominates(head, p));
        Some((entry, back))
    }

    /// The move paths of `move_data` that are a user variable, or a
    /// field of one (`p.name`, `t.0`), each with the name of its column.
    /// For each struct (or tuple) variable, and struct-typed field of one,
    /// some field of which is moved out somewhere in this body: on each
    /// source line, which of its fields rustc's dataflow says may be
    /// uninitialized (moved out) after that line's code, split into those
    /// surely uninitialized and those maybe (#1595). Read after the last
    /// MIR location of the line in each block, on the paths that run that
    /// line; a location whose span covers several lines (a conditional's
    /// join) counts on its last. The outermost such field is named (`p.i`
    /// when all of it is, not `p.i.x`). Not where the variable (or that
    /// field) itself may be moved as a whole: its own column says so.
    /// Cleanup blocks and code from other files are left out.
    pub fn struct_parts_uninit_by_line(&self) -> Vec<crate::svg_generator::data::PartsUninit> {
        use rustc_mir_dataflow::impls::{MaybeInitializedPlaces, MaybeUninitializedPlaces};
        use rustc_mir_dataflow::move_paths::{MoveData, MovePathIndex};
        let body = &self.bwf.body;
        let move_data = MoveData::gather_moves(body, self.tcx, |_| true);
        let names = self.move_path_names(&move_data);
        let moved: BTreeSet<MovePathIndex> = move_data.moves.iter().map(|m| m.path).collect();
        let fields: Vec<(MovePathIndex, String)> = names.iter().filter(|(_, n)| n.contains('.')).cloned().collect();
        let mut structs: BTreeSet<String> = BTreeSet::new();
        for (mpi, n) in &fields {
            if moved.contains(mpi) {
                let parts: Vec<&str> = n.split('.').collect();
                for k in 1..parts.len() {
                    structs.insert(parts[..k].join("."));
                }
            }
        }
        if structs.is_empty() {
            return Vec::new();
        }
        let path_of = |name: &str| names.iter().find(|(_, n)| n == name).map(|(m, _)| *m);
        let mut init = MaybeInitializedPlaces::new(self.tcx, body, &move_data)
            .iterate_to_fixpoint(self.tcx, body, None)
            .into_results_cursor(body);
        let mut uninit = MaybeUninitializedPlaces::new(self.tcx, body, &move_data)
            .iterate_to_fixpoint(self.tcx, body, None)
            .into_results_cursor(body);
        let sm = self.tcx.sess.source_map();
        let file = sm.lookup_char_pos(body.span.lo()).file.name.clone();
        // The source line a location's code is on (a span over several
        // lines, a conditional's join, counts on its last), or none for
        // what isn't code on a line: storage markers, drops (a scope's
        // cleanup, also on a `return`'s way out), and code from elsewhere.
        let line_of = |loc: Location| -> Option<usize> {
            let data = &body.basic_blocks[loc.block];
            match data.statements.get(loc.statement_index) {
                Some(st) if matches!(st.kind, StatementKind::StorageLive(_) | StatementKind::StorageDead(_)) => return None,
                None if matches!(data.terminator().kind, TerminatorKind::Drop { .. } | TerminatorKind::UnwindResume | TerminatorKind::Unreachable | TerminatorKind::Return) => return None,
                _ => {}
            }
            let span = body.source_info(loc).span.source_callsite();
            // (Not the signature's: a parameter's own code isn't on a line
            // of the body.)
            if span.is_dummy() || span.lo() < self.hir_body.value.span.lo() {
                return None;
            }
            let pos = sm.lookup_char_pos(span.hi());
            (pos.file.name == file).then_some(pos.line)
        };
        // The locations after which control leaves their line: the next
        // locations on a line (past those on none) are on another, or
        // there are none.
        let next_lines = |loc: Location| -> BTreeSet<Option<usize>> {
            let mut out = BTreeSet::new();
            let mut work = vec![loc];
            let mut seen: BTreeSet<Location> = BTreeSet::new();
            let mut first = true;
            while let Some(l) = work.pop() {
                if !first {
                    if !seen.insert(l) { continue; }
                    if let Some(line) = line_of(l) { out.insert(Some(line)); continue; }
                }
                first = false;
                let data = &body.basic_blocks[l.block];
                if l.statement_index < data.statements.len() {
                    work.push(Location { block: l.block, statement_index: l.statement_index + 1 });
                } else {
                    let succ: Vec<_> = data.terminator().successors().filter(|b| !body.basic_blocks[*b].is_cleanup).collect();
                    if succ.is_empty() { out.insert(None); }
                    work.extend(succ.into_iter().map(|b| Location { block: b, statement_index: 0 }));
                }
            }
            out
        };
        // And the code that control enters a line from: the code on another
        // line just before (past locations on none: storage markers, drops),
        // read after it (so a scope's drops in between don't count).
        let preds = body.basic_blocks.predecessors();
        let prev_code = |loc: Location| -> BTreeSet<(Option<usize>, Option<Location>)> {
            let mut out = BTreeSet::new();
            let mut work = vec![loc];
            let mut seen: BTreeSet<Location> = BTreeSet::new();
            let mut first = true;
            while let Some(l) = work.pop() {
                if !first {
                    if !seen.insert(l) { continue; }
                    if let Some(line) = line_of(l) { out.insert((Some(line), Some(l))); continue; }
                }
                first = false;
                if l.statement_index > 0 {
                    work.push(Location { block: l.block, statement_index: l.statement_index - 1 });
                } else {
                    let ps: Vec<_> = preds[l.block].iter().filter(|b| !body.basic_blocks[**b].is_cleanup).collect();
                    if ps.is_empty() { out.insert((None, None)); }
                    work.extend(ps.into_iter().map(|b| Location { block: *b, statement_index: body.basic_blocks[*b].statements.len() }));
                }
            }
            out
        };
        let body_end = sm.lookup_char_pos(self.hir_body.value.span.hi()).line;
        // The body's `return`s (also a `?`'s, desugared into one).
        struct Rets { spans: Vec<Span> }
        impl<'tcx> intravisit::Visitor<'tcx> for Rets {
            fn visit_expr(&mut self, e: &'tcx rustc_hir::Expr<'tcx>) {
                if let rustc_hir::ExprKind::Ret(_) = e.kind {
                    self.spans.push(e.span.source_callsite());
                }
                intravisit::walk_expr(self, e);
            }
        }
        let mut rets = Rets { spans: Vec::new() };
        intravisit::Visitor::visit_expr(&mut rets, self.hir_body.value);
        let in_return = |loc: Location| {
            let span = body.source_info(loc).span.source_callsite();
            rets.spans.iter().any(|r| r.contains(span))
        };
        // Exits that leave the function (`return`, `?`): what continues
        // below the line doesn't come from them, unless nothing does.
        let mut leaving: BTreeMap<usize, Vec<Location>> = BTreeMap::new();
        let mut exits: BTreeMap<usize, Vec<Location>> = BTreeMap::new();
        let mut entries: BTreeMap<usize, Vec<Location>> = BTreeMap::new();
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            if data.is_cleanup {
                continue;
            }
            for i in 0..=data.statements.len() {
                let loc = Location { block: bb, statement_index: i };
                let Some(line) = line_of(loc) else { continue };
                let next = next_lines(loc);
                if next.iter().any(|n| *n != Some(line)) {
                    // (A `return`'s, straight to the body's closing brace.)
                    if line < body_end && in_return(loc) && next.iter().all(|n| n.is_none_or(|n| n >= body_end)) {
                        leaving.entry(line).or_default().push(loc);
                    } else {
                        exits.entry(line).or_default().push(loc);
                        // Falling through to the body's end: the way into its
                        // closing brace (which may hold no code of its own).
                        if line < body_end && next.iter().any(|n| n.is_none_or(|n| n >= body_end)) {
                            let into_end = entries.entry(body_end).or_default();
                            if !into_end.contains(&loc) { into_end.push(loc); }
                        }
                    }
                }
                let prev = prev_code(loc);
                if prev.iter().any(|(n, _)| *n != Some(line)) {
                    let from = entries.entry(line).or_default();
                    for (n, l) in prev {
                        if let (true, Some(l)) = (n != Some(line), l) {
                            if !from.contains(&l) { from.push(l); }
                        }
                    }
                }
            }
        }
        for (line, locs) in leaving {
            exits.entry(line).or_insert(locs);
        }
        let mut out = Vec::new();
        for s in &structs {
            let prefix = format!("{}.", s);
            let own = path_of(s);
            let below: Vec<&(MovePathIndex, String)> = fields.iter().filter(|(_, n)| n.starts_with(&prefix)).collect();
            for (entry, line, locs) in exits.iter().map(|(l, v)| (false, l, v)).chain(entries.iter().map(|(l, v)| (true, l, v))) {
                if locs.is_empty() {
                    continue;
                }
                let mut whole = false;
                // Per field: may lack a value after some exit; lacks one after all.
                let mut may: Vec<bool> = vec![false; below.len()];
                let mut sure: Vec<bool> = vec![true; below.len()];
                for &loc in locs {
                    init.seek_after_primary_effect(loc);
                    uninit.seek_after_primary_effect(loc);
                    if own.is_some_and(|o| uninit.get().contains(o)) {
                        whole = true;
                    }
                    for (k, (m, _)) in below.iter().enumerate() {
                        let lack = uninit.get().contains(*m);
                        may[k] |= lack;
                        sure[k] &= lack && !init.get().contains(*m);
                    }
                }
                if whole {
                    continue;
                }
                // The outermost fields that may lack a value.
                let lacking: Vec<usize> = (0..below.len()).filter(|k| may[*k]).collect();
                let outer = |k: &usize| !lacking.iter().any(|j| below[*k].1.starts_with(&format!("{}.", below[*j].1)));
                let (mut certain, mut maybe) = (Vec::new(), Vec::new());
                for k in lacking.iter().filter(|k| outer(k)) {
                    if sure[*k] { certain.push(below[*k].1.clone()) } else { maybe.push(below[*k].1.clone()) }
                }
                out.push(crate::svg_generator::data::PartsUninit { name: s.clone(), line: *line, entry, certain, maybe });
            }
        }
        out
    }

    fn move_path_names(&self, move_data: &rustc_mir_dataflow::move_paths::MoveData<'tcx>) -> Vec<(rustc_mir_dataflow::move_paths::MovePathIndex, String)> {
        let body = &self.bwf.body;
        let names = self.mir_local_names(self.bwf);
        let mut out = Vec::new();
        for (mpi, path) in move_data.move_paths.iter_enumerated() {
            let Some(mut name) = names.get(&path.place.local).cloned() else { continue };
            let mut ty = body.local_decls[path.place.local].ty;
            let mut is_field_path = true;
            for elem in path.place.projection {
                let ProjectionElem::Field(f, fty) = elem else { is_field_path = false; break };
                match ty.kind() {
                    TyKind::Adt(def, _) if def.is_struct() => name = format!("{}.{}", name, def.non_enum_variant().fields[f].name),
                    TyKind::Tuple(_) => name = format!("{}.{}", name, f.index()),
                    _ => { is_field_path = false; break }
                }
                ty = fty;
            }
            if is_field_path {
                out.push((mpi, name));
            }
        }
        out
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
    /// `name` (its RAP name, see `mir_local_names`) is live, from
    /// Polonius' liveness: its last use, or the end of a loop it's used
    /// in.
    pub fn last_live_line(&self, name: &str, lines: (usize, usize)) -> Option<usize> {
        let bwf = self.bwf;
        let p_output = Output::compute(bwf.input_facts.as_ref().unwrap(), Algorithm::Naive, true);
        let names = self.mir_local_names(bwf);
        #[expect(clippy::disallowed_methods, reason = "only collects the locals named `name`; order doesn't matter")]
        let locals: Vec<Local> = names.iter().filter(|(_, n)| n.as_str() == name).map(|(l, _)| *l).collect();
        #[expect(clippy::disallowed_methods, reason = "only takes the greatest line")]
        let last = p_output.var_live_on_entry.iter()
            .filter(|(_, vars)| vars.iter().any(|v| locals.contains(v)))
            .map(|(p, _)| self.location_to_line(self.point_to_location(*p, bwf), bwf))
            .filter(|l| lines.0 <= *l && *l <= lines.1)
            .max();
        last
    }

    /// The last line on which dropping the variable `name` (its RAP
    /// name) may use a borrow its value holds, from rustc's own facts:
    /// the drop check says its drop reads a lifetime of its type (a
    /// `Box<dyn Fn() + 'a>` does: its destructor can't be checked, so
    /// it may use what the closure borrowed; a `Vec<&T>` doesn't), and
    /// Polonius' drop-liveness says where it's dropped while still
    /// initialized: the end of its scope, or nowhere after `drop(m)`
    /// moved it. Cleanup blocks are left out, as in
    /// `nll_loan_end_lines`. None when its drop uses no lifetime.
    ///
    /// rustc keeps a holder's borrow until then even when it records no
    /// loan for it: `boxed(|x| r.len() …)` reborrows `*r` through the
    /// shared reference r, which is a copy of r's lifetime, not a loan
    /// (#1107).
    pub fn last_drop_use_line(&self, name: &str) -> Option<usize> {
        let bwf = self.bwf;
        let facts = bwf.input_facts.as_ref()?;
        let names = self.mir_local_names(bwf);
        #[expect(clippy::disallowed_methods, reason = "only collects the locals named `name`; order doesn't matter")]
        let locals: Vec<Local> = names.iter()
            .filter(|(l, n)| n.as_str() == name && facts.drop_of_var_derefs_origin.iter().any(|(v, _)| v == *l))
            .map(|(l, _)| *l)
            .collect();
        if locals.is_empty() {
            return None;
        }
        let out = Output::compute(facts, Algorithm::Naive, true);
        #[expect(clippy::disallowed_methods, reason = "only takes the greatest line")]
        let last = out.var_drop_live_on_entry.iter()
            .filter(|(_, vars)| vars.iter().any(|v| locals.contains(v)))
            .map(|(p, _)| self.point_to_location(*p, bwf))
            .filter(|l| !bwf.body.basic_blocks[l.block].is_cleanup)
            .map(|l| self.location_to_line(l, bwf))
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
        self.loans_held_in(name, fields, None)
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
        self.loans_held_in(name, fields, Some(within))
    }

    fn loans_held_in(&self, name: &str, fields: &[&str], within: Option<Span>) -> Option<BTreeMap<String, bool>> {
        let bwf = self.bwf;
        let names = self.mir_local_names(bwf);
        let held = self.loans_in_type_of(name, fields)?;
        let mut out: BTreeMap<String, bool> = BTreeMap::new();
        for loan in held {
            let b = &bwf.borrow_set[loan];
            if let Some(w) = within {
                let sp = self.location_to_span(b.reserve_location(), bwf);
                if !w.contains(sp) {
                    continue;
                }
            }
            if let Some(n) = self.src_name_of_local(Some(b.borrowed_place().local), &names) {
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
        Some(self.loans_in_type_of(root, &fields)?.into_iter().map(|l| l.index()).collect())
    }

    fn loans_in_type_of(&self, name: &str, fields: &[&str]) -> Option<BTreeSet<Loan>> {
        let bwf = self.bwf;
        let body = &bwf.body;
        let names = self.mir_local_names(bwf);
        #[expect(clippy::disallowed_methods, reason = "only collects the locals named `name`; order doesn't matter")]
        let locals: Vec<Local> = names.iter().filter(|(_, n)| n.as_str() == name).map(|(l, _)| *l).collect();
        if locals.is_empty() {
            return None;
        }
        let mut regions: BTreeSet<RegionVid> = BTreeSet::new();
        for local in locals {
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
        let names = self.mir_local_names(bwf);
        #[expect(clippy::disallowed_methods, reason = "only collects the locals named `name`; order doesn't matter")]
        let locals: Vec<Local> = names.iter().filter(|(_, n)| n.as_str() == name).map(|(l, _)| *l).collect();
        if locals.is_empty() {
            return None;
        }
        let mut own: BTreeSet<RegionVid> = BTreeSet::new();
        let mut through: BTreeSet<RegionVid> = BTreeSet::new();
        for local in locals {
            let mut ty = body.local_decls[local].ty;
            for f in fields {
                ty = self.field_ty_named(ty, f)?;
            }
            self.split_regions(ty, false, &mut own, &mut through, 0);
        }
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
        let name_of = |loan: Loan| self.src_name_of_local(Some(bwf.borrow_set[loan].borrowed_place().local), &names);
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
    /// RAP name, see `mir_local_names`): a borrow of it is one of
    /// rustc's loans here.
    pub fn is_body_local(&self, name: &str) -> bool {
        #[expect(clippy::disallowed_methods, reason = "only asks whether any local has the name")]
        let found = self.mir_local_names(self.bwf).values().any(|n| n == name);
        found
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