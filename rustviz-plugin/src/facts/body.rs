//! One pass per MIR body: dataflow (MaybeInit / MaybeUninit / EverInit,
//! NLL `Borrows`) and Polonius, each computed once, then read at the row
//! model's locations.

use super::places::{column_projection, Capture, Names};
use super::rows::{RowModel, Rows};
use super::schema::*;
use polonius_engine::{Algorithm, FactTypes, Output};
use rustc_borrowck::consumers::{
    places_conflict, BodyWithBorrowckFacts, BorrowIndex, Borrows, PlaceConflictBias, RustcFacts, TwoPhaseActivation,
};
use rustc_middle::mir::{
    AggregateKind, Body, BorrowKind, Local, Location, MutBorrowKind, Operand, Place, ProjectionElem, Rvalue,
    StatementKind, TerminatorKind, RETURN_PLACE,
};
use rustc_middle::ty::{TyCtxt, TyKind};
use rustc_mir_dataflow::impls::{EverInitializedPlaces, MaybeInitializedPlaces, MaybeUninitializedPlaces};
use rustc_mir_dataflow::move_paths::{LookupResult, MoveData, MovePathIndex};
use rustc_mir_dataflow::{Analysis, ResultsCursor};
use rustc_span::Span;
use std::collections::{BTreeMap, BTreeSet};

type Origin = <RustcFacts as FactTypes>::Origin;
type Point = <RustcFacts as FactTypes>::Point;

struct Cx<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    body: &'a Body<'tcx>,
    names: Names<'tcx>,
    rows: RowModel,
    r: Rows<'a, 'tcx>,
}

impl<'a, 'tcx> Cx<'a, 'tcx> {
    fn name(&self, p: Place<'tcx>) -> Option<String> {
        self.names.name(self.tcx, self.body, p)
    }

    fn is_user(&self, p: Place<'tcx>) -> bool {
        self.names.root(p).is_some()
    }

    fn row_of(&self, l: Location) -> usize {
        self.rows.row.get(&l).copied().unwrap_or(0)
    }

    /// The macro (as written, `println!`) whose expansion a span is in,
    /// if it isn't a desugaring.
    fn macro_of(&self, sp: Span) -> Option<String> {
        if !sp.from_expansion() || sp.desugaring_kind().is_some() {
            return None;
        }
        self.macro_name(sp)
    }

    /// The macro a location's code is in: its span is in the expansion,
    /// or it is a user-written argument (`s` in `println!("{}", s)`, or
    /// `list` in `"{list:?}"`) the expansion uses.
    fn macro_at(&self, l: Location) -> Option<String> {
        let sp = self.body.source_info(l).span;
        if let Some(m) = self.macro_of(sp) {
            return Some(m);
        }
        let hir_id = rustc_utils::BodyExt::location_to_hir_id(self.body, l);
        let nodes = std::iter::once(self.tcx.hir_node(hir_id)).chain(self.tcx.hir_parent_iter(hir_id).map(|(_, n)| n));
        for n in nodes.take(12) {
            let sp = match n {
                rustc_hir::Node::Expr(e) => e.span,
                rustc_hir::Node::Stmt(st) => st.span,
                rustc_hir::Node::Block(b) => b.span,
                rustc_hir::Node::LetStmt(ls) => ls.span,
                _ => return None,
            };
            if sp.from_expansion() && sp.desugaring_kind().is_none() {
                return self.macro_name(sp);
            }
        }
        None
    }

    fn macro_name(&self, sp: Span) -> Option<String> {
        let cs = sp.source_callsite();
        let t = self.tcx.sess.source_map().span_to_snippet(cs).ok()?;
        let i = t.find('!')?;
        Some(format!("{}!", t[..i].trim()))
    }

    /// The callee as the user wrote it: `String::from`, `push_str`,
    /// `println!`.
    fn call_name(&self, fn_span: Span) -> String {
        if let Some(m) = self.macro_of(fn_span) {
            return m;
        }
        let t = self.tcx.sess.source_map().span_to_snippet(fn_span).unwrap_or_default();
        let head = t.split('(').next().unwrap_or("").trim();
        let head = head.rsplit('.').next().unwrap_or(head).trim();
        if head.is_empty() { "<call>".into() } else { head.split("::<").next().unwrap_or(head).to_string() }
    }

    /// The user-visible destination of a value stored into `place` at
    /// `loc`: a user place, or (through MIR temporaries on nearby rows)
    /// the variable, struct field, call or closure that consumes it.
    fn dest_name(&self, place: Place<'tcx>, loc: Location, depth: usize) -> String {
        if let Some(n) = self.name(place) {
            return n;
        }
        if place.local == RETURN_PLACE {
            return "<return>".into();
        }
        if depth > 6 || !place.projection.is_empty() {
            return "<temp>".into();
        }
        let tmp = place.local;
        let row = self.row_of(loc);
        let mut best: Option<String> = None;
        for (bb, data) in self.body.basic_blocks.iter_enumerated() {
            if data.is_cleanup {
                continue;
            }
            for (i, st) in data.statements.iter().enumerate() {
                let here = Location { block: bb, statement_index: i };
                let StatementKind::Assign(asg) = &st.kind else { continue };
                let (lhs, rv) = &**asg;
                if lhs.local == tmp {
                    continue;
                }
                let is_tmp = |o: &Operand<'tcx>| matches!(o, Operand::Move(p) | Operand::Copy(p) if p.local == tmp && p.projection.is_empty());
                let field = match rv {
                    Rvalue::Use(o) | Rvalue::Cast(_, o, _) => is_tmp(o).then_some(None),
                    Rvalue::Aggregate(k, ops) => ops.iter_enumerated().find(|(_, o)| is_tmp(o)).map(|(f, _)| match &**k {
                        AggregateKind::Adt(did, v, ..) => {
                            let adt = self.tcx.adt_def(*did);
                            Some(adt.variant(*v).fields.get(f).map_or_else(|| f.index().to_string(), |fd| fd.name.to_string()))
                        }
                        AggregateKind::Tuple => Some(f.index().to_string()),
                        _ => None,
                    }),
                    Rvalue::Ref(_, _, p) => (p.local == tmp && p.projection.first() == Some(&ProjectionElem::Deref)).then_some(None),
                    _ => None,
                };
                let Some(field) = field else { continue };
                if self.row_of(here).abs_diff(row) > 3 {
                    continue;
                }
                let d = self.dest_name(*lhs, here, depth + 1);
                let closure = matches!(rv, Rvalue::Aggregate(k, _) if matches!(**k, AggregateKind::Closure(..)));
                best = Some(match field {
                    _ if closure => format!("closure {}", d),
                    Some(f) if !d.starts_with('<') && !d.ends_with("()") => format!("{}.{}", d, f),
                    _ => d,
                });
            }
            if let TerminatorKind::Call { args, fn_span, .. } = &data.terminator().kind {
                if best.is_none() && args.iter().any(|a| matches!(&a.node, Operand::Move(p) | Operand::Copy(p) if p.local == tmp)) {
                    best = Some(format!("{}()", self.call_name(*fn_span)));
                }
            }
        }
        best.unwrap_or_else(|| "<temp>".into())
    }

    fn ev(&self, l: Location, fact: Fact) -> (usize, Ev) {
        let row = self.rows.event_row.get(&l).copied().unwrap_or_else(|| self.row_of(l));
        let ev = Ev { col: self.rows.col.get(&l).copied().unwrap_or(0), arm: self.rows.arm_of(l), fact, order: self.rows.order(l) };
        (row, ev)
    }
}

/// Where to read what `place` held before the assignment at `loc`: if
/// MIR drops the old value first (`drop(s) -> bb; bb: s = ..`), before
/// those drops; else at `loc`.
fn before_reassign_drop<'tcx>(body: &Body<'tcx>, loc: Location, place: Place<'tcx>) -> Vec<Location> {
    if loc.statement_index != 0 {
        return vec![loc];
    }
    let preds = &body.basic_blocks.predecessors()[loc.block];
    let drops: Vec<Location> = preds
        .iter()
        .filter(|p| matches!(&body.basic_blocks[**p].terminator().kind, TerminatorKind::Drop { place: dp, .. } if *dp == place))
        .map(|p| Location { block: *p, statement_index: body.basic_blocks[*p].statements.len() })
        .collect();
    if drops.is_empty() { vec![loc] } else { drops }
}

struct Flow<'a, 'mir, 'tcx> {
    md: &'a MoveData<'tcx>,
    init: ResultsCursor<'mir, 'tcx, MaybeInitializedPlaces<'a, 'tcx>>,
    uninit: ResultsCursor<'mir, 'tcx, MaybeUninitializedPlaces<'a, 'tcx>>,
    ever: ResultsCursor<'mir, 'tcx, EverInitializedPlaces<'a, 'tcx>>,
}

/// Raw init state of one move path over a set of locations.
struct InitState {
    may_init: bool,
    may_uninit: bool,
    ever_init: bool,
    /// Descendant paths that are maybe-uninit / definitely uninit.
    sub_maybe: Vec<MovePathIndex>,
    sub_moved: Vec<MovePathIndex>,
}

impl InitState {
    fn word(&self) -> &'static str {
        match (self.may_init, self.may_uninit) {
            (true, false) if !self.sub_maybe.is_empty() || !self.sub_moved.is_empty() => "partly",
            (true, false) => "own",
            (true, true) => "maybe",
            (false, _) if self.ever_init => "moved",
            (false, _) => "unassigned",
        }
    }
}

fn descendants(md: &MoveData<'_>, mpi: MovePathIndex) -> Vec<MovePathIndex> {
    let mut out = Vec::new();
    let mut work = vec![mpi];
    while let Some(m) = work.pop() {
        out.push(m);
        let mut c = md.move_paths[m].first_child;
        while let Some(ch) = c {
            work.push(ch);
            c = md.move_paths[ch].next_sibling;
        }
    }
    out
}

fn ancestors(md: &MoveData<'_>, mpi: MovePathIndex) -> Vec<MovePathIndex> {
    let mut out = vec![mpi];
    let mut p = md.move_paths[mpi].parent;
    while let Some(x) = p {
        out.push(x);
        p = md.move_paths[x].parent;
    }
    out
}

impl Flow<'_, '_, '_> {
    fn state(&mut self, locs: &[Location], after: bool, mpi: MovePathIndex) -> InitState {
        let anc = ancestors(self.md, mpi);
        let desc = descendants(self.md, mpi);
        let mut s = InitState { may_init: false, may_uninit: false, ever_init: false, sub_maybe: Vec::new(), sub_moved: Vec::new() };
        let mut sub_init: BTreeSet<MovePathIndex> = BTreeSet::new();
        let mut sub_uninit: BTreeSet<MovePathIndex> = BTreeSet::new();
        for &l in locs {
            if after {
                self.init.seek_after_primary_effect(l);
                self.uninit.seek_after_primary_effect(l);
                self.ever.seek_after_primary_effect(l);
            } else {
                self.init.seek_before_primary_effect(l);
                self.uninit.seek_before_primary_effect(l);
                self.ever.seek_before_primary_effect(l);
            }
            s.may_init |= self.init.get().contains(mpi);
            s.may_uninit |= self.uninit.get().contains(mpi);
            for d in desc.iter().skip(1) {
                if self.uninit.get().contains(*d) {
                    sub_uninit.insert(*d);
                }
                if self.init.get().contains(*d) {
                    sub_init.insert(*d);
                }
            }
            for i in self.ever.get().iter() {
                let p = self.md.inits[i].path;
                if anc.contains(&p) || desc.contains(&p) {
                    s.ever_init = true;
                }
            }
        }
        for d in sub_uninit {
            if sub_init.contains(&d) { s.sub_maybe.push(d) } else { s.sub_moved.push(d) }
        }
        s
    }
}

pub fn body_facts<'tcx>(tcx: TyCtxt<'tcx>, bwf: &BodyWithBorrowckFacts<'tcx>, func: String) -> BodyFacts {
    let body = &bwf.body;
    let def_id = body.source.def_id();
    let typing_env = rustc_middle::ty::TypingEnv::post_analysis(tcx, def_id);
    let r = Rows { tcx, body };
    let rows = r.build();
    let names = Names::new(tcx, body);
    let cx = Cx { tcx, body, names, rows, r };
    let mut notes: Vec<String> = Vec::new();

    // ---- variables ---------------------------------------------------------
    let ty_info = |ty: rustc_middle::ty::Ty<'tcx>| {
        let ty = tcx.erase_and_anonymize_regions(ty);
        (tcx.type_is_copy_modulo_regions(typing_env, ty), ty.needs_drop(tcx, typing_env), ty)
    };
    let kind_of = |ty: rustc_middle::ty::Ty<'tcx>| match ty.kind() {
        TyKind::Ref(_, _, rustc_middle::mir::Mutability::Mut) => "mut_ref",
        TyKind::Ref(..) => "shared_ref",
        _ => "owner",
    };
    let mut vars: Vec<VarInfo> = Vec::new();
    for (local, name) in &cx.names.locals {
        let decl = &body.local_decls[*local];
        let (copy, needs_drop, ty) = ty_info(decl.ty);
        vars.push(VarInfo {
            name: name.clone(),
            local: local.index(),
            decl_line: cx.r.line_lo(decl.source_info.span),
            kind: kind_of(ty),
            copy,
            needs_drop,
            ty: format!("{}", ty),
            capture: None,
        });
    }
    for (place, name, cap) in &cx.names.aliases {
        let pty = place.ty(&body.local_decls, tcx).ty;
        let (copy, needs_drop, ty) = ty_info(pty);
        vars.push(VarInfo {
            name: name.clone(),
            local: place.local.index(),
            decl_line: cx.r.line_lo(body.span),
            kind: kind_of(ty),
            copy,
            needs_drop,
            ty: format!("{}", ty),
            capture: Some(match cap {
                Capture::ByValue => "by_value",
                Capture::ByRef => "by_ref",
                Capture::ByMutRef => "by_mut_ref",
            }),
        });
    }

    // ---- dataflow, once --------------------------------------------------------
    let move_data = MoveData::gather_moves(body, tcx, |_| true);
    let mut flow = Flow {
        md: &move_data,
        init: MaybeInitializedPlaces::new(tcx, body, &move_data).iterate_to_fixpoint(tcx, body, None).into_results_cursor(body),
        uninit: MaybeUninitializedPlaces::new(tcx, body, &move_data).iterate_to_fixpoint(tcx, body, None).into_results_cursor(body),
        ever: EverInitializedPlaces::new(body, &move_data).iterate_to_fixpoint(tcx, body, None).into_results_cursor(body),
    };
    let borrows = Borrows::new(tcx, body, &bwf.region_inference_context, &bwf.borrow_set);
    let mut inscope = borrows.iterate_to_fixpoint(tcx, body, None).into_results_cursor(body);

    // Columns: move paths of user variables, their fields and Box payloads.
    let mut columns: Vec<(MovePathIndex, Place<'tcx>, String)> = Vec::new();
    let mut column_info: Vec<ColumnInfo> = Vec::new();
    for (mpi, mp) in move_data.move_paths.iter_enumerated() {
        let Some((root, skip, _)) = cx.names.root(mp.place) else { continue };
        if !column_projection(body, mp.place, skip) {
            continue;
        }
        let Some(n) = cx.name(mp.place) else { continue };
        let (copy, needs_drop, _) = ty_info(mp.place.ty(&body.local_decls, tcx).ty);
        let kind = if mp.place.projection.len() == skip {
            "var"
        } else if mp.place.projection.last() == Some(&ProjectionElem::Deref) {
            "box_payload"
        } else {
            "field"
        };
        column_info.push(ColumnInfo { name: n.clone(), root, kind, copy, needs_drop });
        columns.push((mpi, mp.place, n));
    }
    let column_name = |mpi: MovePathIndex| columns.iter().find(|(m, _, _)| *m == mpi).map(|(_, _, n)| n.clone());

    // ---- Polonius, once ---------------------------------------------------------
    let pfacts = bwf.input_facts.as_ref();
    let table = bwf.location_table.as_ref();
    let pol = pfacts.map(|f| Output::compute(f, Algorithm::Naive, true));
    let mut origins_of: BTreeMap<Local, Vec<Origin>> = BTreeMap::new();
    if let Some(f) = pfacts {
        for (local, origin) in f.use_of_var_derefs_origin.iter().chain(f.drop_of_var_derefs_origin.iter()) {
            if cx.names.locals.contains_key(local) {
                let os = origins_of.entry(*local).or_default();
                if !os.contains(origin) {
                    os.push(*origin);
                }
            }
        }
    }
    let holders_at = |p: Point, loan: BorrowIndex| -> Vec<String> {
        let mut out = Vec::new();
        let Some(pol) = pol.as_ref() else { return out };
        let Some(m) = pol.origin_contains_loan_at.get(&p) else { return out };
        for (local, os) in &origins_of {
            if os.iter().any(|o| m.get(o).is_some_and(|ls| ls.contains(&loan))) {
                out.push(cx.names.locals[local].clone());
            }
        }
        out
    };
    // Anyone whose type holds the loan at some point, with the first row.
    let mut holders_any: BTreeMap<BorrowIndex, Vec<(usize, String)>> = BTreeMap::new();
    if let (Some(pol), Some(t)) = (pol.as_ref(), table) {
        #[expect(clippy::disallowed_methods, reason = "sorted below")]
        let mut pts: Vec<(&Point, &BTreeMap<Origin, BTreeSet<BorrowIndex>>)> = pol.origin_contains_loan_at.iter().collect();
        pts.sort_by_key(|(p, _)| **p);
        for (p, m) in pts {
            let row = cx.row_of(t.to_location(*p));
            for (local, os) in &origins_of {
                for o in os {
                    for l in m.get(o).into_iter().flatten() {
                        let v = holders_any.entry(*l).or_default();
                        let n = &cx.names.locals[local];
                        match v.iter_mut().find(|(_, x)| x == n) {
                            Some(e) => e.0 = e.0.min(row),
                            None => v.push((row, n.clone())),
                        }
                    }
                }
            }
        }
        for v in holders_any.values_mut() {
            v.sort();
        }
    }

    // ---- loans ------------------------------------------------------------------
    let mut loans: Vec<LoanInfo> = Vec::new();
    let mut evs: BTreeMap<usize, Vec<Ev>> = BTreeMap::new();
    let push = |evs: &mut BTreeMap<usize, Vec<Ev>>, (row, ev): (usize, Ev)| evs.entry(row).or_default().push(ev);
    // Loans of temporaries, for `TempInfo::held_by`.
    let mut temp_holders: BTreeMap<Local, Vec<String>> = BTreeMap::new();
    for (idx, (loc, bd)) in bwf.borrow_set.location_map().iter().enumerate() {
        let bi = BorrowIndex::from_usize(idx);
        let (kind, two_phase) = match bd.kind() {
            BorrowKind::Fake(_) => continue,
            BorrowKind::Shared => ("shared", false),
            BorrowKind::Mut { kind: MutBorrowKind::TwoPhaseBorrow } => ("mut", true),
            BorrowKind::Mut { kind: MutBorrowKind::ClosureCapture } => ("unique", false),
            BorrowKind::Mut { .. } => ("mut", false),
        };
        let two_phase = two_phase && !matches!(bd.activation_location(), TwoPhaseActivation::NotTwoPhase);
        let bp = bd.borrowed_place();
        let in_macro = cx.macro_at(*loc);
        let mut direct = cx.dest_name(bd.assigned_place(), *loc, 0);
        if direct.starts_with('<') {
            if let Some(m) = &in_macro {
                direct = format!("{}()", m);
            }
        }
        let mut holders: Vec<String> = Vec::new();
        if !direct.starts_with('<') && !direct.ends_with("()") {
            holders.push(direct.trim_start_matches("closure ").to_string());
        }
        if let Some(t) = table {
            let next = loc.successor_within_block();
            let p = if next.statement_index <= body.basic_blocks[loc.block].statements.len() { t.start_index(next) } else { t.mid_index(*loc) };
            for h in holders_at(p, bi) {
                if !holders.contains(&h) {
                    holders.push(h);
                }
            }
        }
        let lender_name = cx.name(bp);
        if let Some(hs) = holders_any.get(&bi) {
            for (_, h) in hs {
                if !holders.contains(h) && Some(h) != lender_name.as_ref() {
                    holders.push(h.clone());
                }
            }
        }
        let Some(lender) = lender_name else {
            // A loan of a MIR temporary (`&mut *_14` for `next()`, or a
            // lifetime-extended `&String::new()`): not a column.
            if bp.projection.is_empty() {
                temp_holders.entry(bp.local).or_default().extend(holders.iter().filter(|h| !h.contains('(')).cloned());
            }
            continue;
        };
        let to_call = direct.strip_suffix("()").filter(|_| holders.is_empty()).map(str::to_owned);
        let holder = if holders.is_empty() { direct.clone() } else { holders.join("|") };
        let cols: Vec<String> = columns
            .iter()
            .filter(|(_, cp, _)| places_conflict(tcx, body, bp, *cp, PlaceConflictBias::Overlap))
            .map(|(_, _, n)| n.clone())
            .collect();
        push(&mut evs, cx.ev(*loc, Fact::Borrow { loan: idx, lender: lender.clone(), kind, holder }));
        loans.push(LoanInfo {
            loan: idx,
            line: cx.row_of(*loc),
            col: cx.rows.col.get(loc).copied().unwrap_or(0),
            lender,
            columns: cols,
            kind,
            two_phase,
            through: cx.names.through(tcx, body, bp),
            holders,
            to_call,
            in_macro,
            end_lines: Vec::new(),
        });
    }
    let loan_holder = |l: &LoanInfo| -> String {
        if !l.holders.is_empty() {
            l.holders.join("|")
        } else if let Some(c) = &l.to_call {
            format!("{}()", c)
        } else {
            "<temp>".into()
        }
    };

    // Borrow ends: a location where a loan is in scope with a successor
    // where it isn't. Per CFG edge, so each arm gets its own end.
    let mut seen_end: BTreeSet<(usize, usize, usize)> = BTreeSet::new();
    for (&loc, &row) in &cx.rows.row {
        let data = &body.basic_blocks[loc.block];
        inscope.seek_before_primary_effect(loc);
        let here: Vec<BorrowIndex> = inscope.get().iter().collect();
        if here.is_empty() {
            continue;
        }
        let succs: Vec<Location> = if loc.statement_index < data.statements.len() {
            vec![loc.successor_within_block()]
        } else {
            data.terminator()
                .successors()
                .filter(|s| !body.basic_blocks[*s].is_cleanup)
                .map(|s| Location { block: s, statement_index: 0 })
                .collect()
        };
        for s in &succs {
            inscope.seek_before_primary_effect(*s);
            let there = inscope.get().clone();
            for b in &here {
                if there.contains(*b) {
                    continue;
                }
                let Some(li) = loans.iter_mut().find(|l| l.loan == b.index()) else { continue };
                let to_line = cx.row_of(*s);
                let mut why = "last use".to_string();
                let sdata = &body.basic_blocks[s.block];
                match sdata.statements.get(s.statement_index).map(|st| &st.kind) {
                    Some(StatementKind::Assign(asg)) if cx.name(asg.0).is_some_and(|n| li.holders.contains(&n)) => {
                        why = "holder overwritten".into()
                    }
                    Some(StatementKind::StorageDead(l)) if cx.names.locals.contains_key(l) => {
                        why = format!("{} out of scope", cx.names.locals[l])
                    }
                    None => {
                        if let TerminatorKind::Drop { place, .. } = &sdata.terminator().kind {
                            if let Some(n) = cx.name(*place) {
                                why = format!("{} dropped", n);
                            }
                        }
                    }
                    _ => {}
                }
                if loc.statement_index == data.statements.len() {
                    if let TerminatorKind::Drop { place, .. } = &data.terminator().kind {
                        if let Some(n) = cx.name(*place) {
                            why = format!("{} dropped", n);
                        }
                    }
                }
                if seen_end.insert((b.index(), row, to_line)) {
                    if !li.end_lines.contains(&row) {
                        li.end_lines.push(row);
                    }
                    let f = Fact::BorrowEnd { loan: b.index(), lender: li.lender.clone(), holder: loan_holder(li), why, to_line };
                    push(&mut evs, cx.ev(loc, f));
                }
            }
        }
    }
    for l in loans.iter_mut() {
        l.end_lines.sort();
    }

    // ---- moves, copies, inits, re-points, drops, scope ends ---------------------
    let mut temps: BTreeMap<Local, TempInfo> = BTreeMap::new();
    for (&loc, _) in &cx.rows.row {
        let data = &body.basic_blocks[loc.block];
        if let Some(st) = data.statements.get(loc.statement_index) {
            let in_macro = cx.macro_at(loc);
            match &st.kind {
                StatementKind::Assign(asg) => {
                    let (lhs, rv) = &**asg;
                    let mut ops: Vec<&Operand<'tcx>> = Vec::new();
                    match rv {
                        Rvalue::Use(o) | Rvalue::Cast(_, o, _) | Rvalue::UnaryOp(_, o) | Rvalue::Repeat(o, _) => ops.push(o),
                        Rvalue::BinaryOp(_, ab) => {
                            ops.push(&ab.0);
                            ops.push(&ab.1);
                        }
                        Rvalue::Aggregate(_, os) => ops.extend(os.iter()),
                        _ => {}
                    }
                    for o in ops {
                        match o {
                            Operand::Move(p) if cx.is_user(*p) => {
                                let to = cx.dest_name(*lhs, loc, 0);
                                let path = cx.name(*p).unwrap_or_default();
                                // A move out of `*r` is a copy of a reference's target
                                // only for Box (others can't be moved out of).
                                let through_ref = p.projection.first() == Some(&ProjectionElem::Deref)
                                    && !body.local_decls[p.local].ty.is_box()
                                    && cx.names.locals.contains_key(&p.local);
                                if !through_ref {
                                    push(&mut evs, cx.ev(loc, Fact::Move { path, to, in_macro: in_macro.clone() }));
                                }
                            }
                            Operand::Copy(p) if cx.is_user(*p) => {
                                let to = cx.dest_name(*lhs, loc, 0);
                                if to != "<temp>" || in_macro.is_some() {
                                    let path = cx.name(*p).unwrap_or_default();
                                    push(&mut evs, cx.ev(loc, Fact::Copy { path, to, in_macro: in_macro.clone() }));
                                }
                            }
                            _ => {}
                        }
                    }
                    let lhs_col = cx.names.root(*lhs).is_some_and(|(_, skip, _)| column_projection(body, *lhs, skip));
                    if lhs_col {
                        let path = cx.name(*lhs).unwrap_or_default();
                        let from = match rv {
                            Rvalue::Use(Operand::Move(p) | Operand::Copy(p)) => cx.name(*p).unwrap_or_else(|| "<value>".into()),
                            Rvalue::Ref(_, k, p) => format!(
                                "&{}{}",
                                if matches!(k, BorrowKind::Mut { .. }) { "mut " } else { "" },
                                cx.name(*p).unwrap_or_else(|| "<temp>".into())
                            ),
                            Rvalue::Aggregate(..) => "<aggregate>".into(),
                            Rvalue::Use(Operand::Constant(_)) => "<const>".into(),
                            _ => "<value>".into(),
                        };
                        let (reassign, maybe_had) = match move_data.rev_lookup.find(lhs.as_ref()) {
                            LookupResult::Exact(mpi) => {
                                let at = before_reassign_drop(body, loc, *lhs);
                                let s = flow.state(&at, false, mpi).word();
                                (s != "unassigned", s == "maybe" || s == "moved")
                            }
                            LookupResult::Parent(_) => (false, false),
                        };
                        if body.local_decls[lhs.local].ty.is_ref() && reassign && lhs.projection.is_empty() {
                            if let (Some(t), Some(pol)) = (table, pol.as_ref()) {
                                let held = |p: Point| -> Vec<usize> {
                                    let mut v: Vec<usize> = Vec::new();
                                    if let (Some(m), Some(os)) = (pol.origin_contains_loan_at.get(&p), origins_of.get(&lhs.local)) {
                                        for o in os {
                                            for l in m.get(o).into_iter().flatten() {
                                                if !v.contains(&l.index()) {
                                                    v.push(l.index());
                                                }
                                            }
                                        }
                                    }
                                    v.sort();
                                    v
                                };
                                let f = Fact::Repoint { holder: path.clone(), old: held(t.start_index(loc)), new: held(t.mid_index(loc)) };
                                push(&mut evs, cx.ev(loc, f));
                            }
                        }
                        push(&mut evs, cx.ev(loc, Fact::Init { path, from, reassign, maybe_had }));
                    }
                }
                StatementKind::StorageDead(l) if cx.names.locals.contains_key(l) => {
                    let (copy, needs_drop, _) = ty_info(body.local_decls[*l].ty);
                    if !needs_drop {
                        let note = if copy { "copy: nothing to drop" } else { "no drop glue" };
                        push(&mut evs, cx.ev(loc, Fact::ScopeEnd { path: cx.names.locals[l].clone(), note }));
                    }
                }
                _ => {}
            }
            continue;
        }
        match &data.terminator().kind {
            TerminatorKind::Call { args, destination, fn_span, .. } => {
                let f = cx.call_name(*fn_span);
                let in_macro = cx.macro_of(*fn_span);
                for a in args.iter() {
                    match &a.node {
                        Operand::Move(p) if cx.is_user(*p) => {
                            let path = cx.name(*p).unwrap_or_default();
                            push(&mut evs, cx.ev(loc, Fact::Move { path, to: format!("{}()", f), in_macro: in_macro.clone() }));
                        }
                        Operand::Copy(p) if cx.is_user(*p) => {
                            let path = cx.name(*p).unwrap_or_default();
                            push(&mut evs, cx.ev(loc, Fact::Copy { path, to: format!("{}()", f), in_macro: in_macro.clone() }));
                        }
                        _ => {}
                    }
                }
                let dest_col = cx.names.root(*destination).is_some_and(|(_, skip, _)| column_projection(body, *destination, skip));
                if dest_col {
                    let path = cx.name(*destination).unwrap_or_default();
                    let (reassign, maybe_had) = match move_data.rev_lookup.find(destination.as_ref()) {
                        LookupResult::Exact(mpi) => {
                            let s = flow.state(&[loc], false, mpi).word();
                            (s != "unassigned", s == "maybe" || s == "moved")
                        }
                        LookupResult::Parent(_) => (false, false),
                    };
                    push(&mut evs, cx.ev(loc, Fact::Init { path, from: format!("{}()", f), reassign, maybe_had }));
                }
            }
            TerminatorKind::Drop { place, target, .. } => {
                let tdata = &body.basic_blocks[*target];
                let at_reassign = matches!(tdata.statements.first().map(|s| &s.kind), Some(StatementKind::Assign(asg)) if asg.0 == *place);
                let early_exit = cx.rows.early_exit.get(&loc).copied();
                let state = match move_data.rev_lookup.find(place.as_ref()) {
                    LookupResult::Exact(mpi) => Some(flow.state(&[loc], false, mpi).word()),
                    LookupResult::Parent(_) => None,
                };
                if cx.is_user(*place) {
                    let path = cx.name(*place).unwrap_or_default();
                    let how = match state {
                        Some("own") | None => "drops",
                        Some("partly") => "drops_rest",
                        Some("maybe") => "drops_if_init",
                        _ => "nothing_to_drop",
                    };
                    let also_on_exits = cx.rows.shared_exits.get(&loc).cloned().unwrap_or_default();
                    push(&mut evs, cx.ev(loc, Fact::Drop { path, how, at_reassign, early_exit, also_on_exits }));
                } else if place.local != RETURN_PLACE
                    && place.local.index() > body.arg_count
                    && place.projection.is_empty()
                    && !body.local_decls[place.local].source_info.span.from_expansion()
                    && matches!(state, Some("own" | "maybe" | "partly"))
                {
                    let decl = &body.local_decls[place.local];
                    let sp = decl.source_info.span;
                    let what: String = tcx.sess.source_map().span_to_snippet(sp.source_callsite()).unwrap_or_else(|_| "?".into());
                    let what: String = what.chars().take(40).collect();
                    let created_line = cx.r.line_hi(sp);
                    let drop_row = cx.rows.event_row.get(&loc).copied().unwrap_or(0);
                    let held_by = temp_holders.get(&place.local).cloned().unwrap_or_default();
                    let extended = !held_by.is_empty() && drop_row > created_line;
                    let ti = temps.entry(place.local).or_insert_with(|| TempInfo {
                        what: what.clone(),
                        ty: format!("{}", tcx.erase_and_anonymize_regions(decl.ty)),
                        created_line,
                        drop_lines: Vec::new(),
                        extended: false,
                        held_by,
                    });
                    ti.extended |= extended;
                    if !ti.drop_lines.contains(&drop_row) {
                        ti.drop_lines.push(drop_row);
                    }
                    push(&mut evs, cx.ev(loc, Fact::TempDrop { what, created_line, extended, early_exit }));
                }
            }
            _ => {}
        }
    }
    let temps: Vec<TempInfo> = temps
        .into_values()
        .map(|mut t| {
            t.drop_lines.sort();
            t.held_by.sort();
            t.held_by.dedup();
            t
        })
        .collect();

    // ---- per-row state ------------------------------------------------------------
    let first_line = cx.r.line_lo(body.span);
    let last_line = cx.r.line_hi(body.span);
    let mut state_after = |exits: &[Location]| -> State {
        let mut st = State::default();
        for (mpi, place, name) in &columns {
            let s = flow.state(exits, true, *mpi);
            let w = s.word();
            // A variable not assigned yet (and its fields): no column yet.
            let mut root_unassigned = || {
                columns.iter().any(|(m, rp, _)| {
                    rp.local == place.local && rp.projection.len() < place.projection.len() && flow.state(exits, true, *m).word() == "unassigned"
                })
            };
            if w == "unassigned" && (place.projection.is_empty() || root_unassigned()) {
                continue;
            }
            st.state.insert(name.clone(), w);
            if (w == "partly" || w == "own") && (!s.sub_moved.is_empty() || !s.sub_maybe.is_empty()) {
                let mut parts = Parts {
                    moved: s.sub_moved.iter().filter_map(|m| column_name(*m)).collect(),
                    maybe: s.sub_maybe.iter().filter_map(|m| column_name(*m)).collect(),
                };
                parts.moved.sort();
                parts.maybe.sort();
                st.parts.insert(name.clone(), parts);
            }
        }
        // Loans in scope where the next row starts: a loan whose last use
        // is on this row is out of scope there.
        let mut live: BTreeSet<usize> = BTreeSet::new();
        for e in exits {
            let line = cx.row_of(*e);
            let data = &body.basic_blocks[e.block];
            let succs: Vec<Location> = if e.statement_index < data.statements.len() {
                vec![e.successor_within_block()]
            } else {
                data.terminator()
                    .successors()
                    .filter(|s| !body.basic_blocks[*s].is_cleanup)
                    .map(|s| Location { block: s, statement_index: 0 })
                    .collect()
            };
            let later: Vec<Location> = succs.iter().copied().filter(|s| cx.row_of(*s) > line).collect();
            let at = if later.is_empty() { succs } else { later };
            if at.is_empty() {
                inscope.seek_after_primary_effect(*e);
                live.extend(inscope.get().iter().map(|b| b.index()));
            }
            for s in at {
                inscope.seek_before_primary_effect(s);
                live.extend(inscope.get().iter().map(|b| b.index()));
            }
        }
        for l in &loans {
            if !live.contains(&l.loan) {
                continue;
            }
            let kind = if l.kind == "shared" { "shared" } else { "mut" };
            for c in &l.columns {
                // Only columns that hold a value can be lent.
                if matches!(st.state.get(c), Some(&"own" | &"partly" | &"maybe")) {
                    st.lent.entry(c.clone()).or_default().push(Lent { loan: l.loan, kind, holder: loan_holder(l) });
                }
            }
        }
        st
    };
    let mut out_rows: Vec<Row> = Vec::new();
    let mut lines: BTreeSet<usize> = cx.rows.row.values().copied().collect();
    lines.extend(evs.keys().copied());
    for line in lines.into_iter().filter(|l| *l >= first_line && *l <= last_line) {
        let mut row = Row { line, ..Default::default() };
        if let Some(exits) = cx.rows.exits.get(&line) {
            row.after = state_after(exits);
        }
        if let Some(specs) = cx.rows.arms.get(&line) {
            for s in specs {
                row.arms.push(Arm {
                    branch: s.branch,
                    arm: s.arm,
                    parent: s.parent,
                    col_lo: s.col_lo,
                    col_hi: s.col_hi,
                    after: state_after(&s.exits),
                });
            }
        }
        let mut fs = evs.remove(&line).unwrap_or_default();
        fs.sort_by_key(|e| e.order);
        fs.dedup_by(|a, b| a.fact == b.fact && a.col == b.col);
        row.facts = fs;
        out_rows.push(row);
    }
    for (line, fs) in evs {
        notes.push(format!("{} facts on row {} outside the body's rows", fs.len(), line));
    }
    BodyFacts {
        func,
        kind: if tcx.is_closure_like(def_id) { "closure" } else { "fn" },
        parent: tcx.is_closure_like(def_id).then(|| tcx.def_path_str(tcx.typeck_root_def_id(def_id))),
        first_line,
        last_line,
        vars,
        columns: column_info,
        loans,
        rows: out_rows,
        temps,
        notes,
    }
}
