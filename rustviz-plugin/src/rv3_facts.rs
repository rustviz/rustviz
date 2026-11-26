//! RustViz 3 spike (not for merge): a MIR fact layer.
//!
//! Computes, per source row and per user variable (and field path), what
//! a timeline needs to show, from rustc's own MIR and borrow-check facts
//! only, with no HIR control-flow inference:
//!
//! - ownership state: owns / moved / maybe-moved / partially moved /
//!   unassigned (MaybeInit, MaybeUninit, EverInit dataflow);
//! - lent state: shared / mutably borrowed, by which loans (NLL `Borrows`,
//!   the dataflow rustc's borrowck uses);
//! - events: init / reassign, move (to where), copy, borrow (lender ->
//!   holder, kind), borrow end (per CFG path), re-point, drop (definite /
//!   conditional / nothing left), scope end, temporary drop.
//!
//! The HIR is used for names only (the same `s`, `s₂` names the plugin
//! gives its columns). Enabled by `RV3_FACTS=<file.json>`; the normal
//! rendering is unchanged.
#![allow(clippy::disallowed_methods, clippy::iter_over_hash_type)]

use crate::expr_visitor_utils::{binding_names, written_name};
use polonius_engine::{Algorithm, FactTypes, Output};
use rustc_borrowck::consumers::{BodyWithBorrowckFacts, BorrowIndex, Borrows, RustcFacts};
use rustc_middle::mir::{
    AggregateKind, BasicBlock, Body, BorrowKind, Local, Location, MutBorrowKind, Operand, Place,
    ProjectionElem, Rvalue, StatementKind, TerminatorKind, VarDebugInfoContents,
};
use rustc_middle::ty::{TyCtxt, TyKind};
use rustc_mir_dataflow::impls::{EverInitializedPlaces, MaybeInitializedPlaces, MaybeUninitializedPlaces};
use rustc_utils::SpanExt;
use rustc_mir_dataflow::move_paths::{LookupResult, MoveData, MovePathIndex};
use rustc_mir_dataflow::Analysis;
use rustc_span::Span;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

type Origin = <RustcFacts as FactTypes>::Origin;
type Point = <RustcFacts as FactTypes>::Point;

#[derive(Serialize)]
pub struct VarInfo {
    pub name: String,
    pub local: usize,
    pub decl_line: usize,
    /// "owner" | "shared_ref" | "mut_ref"
    pub kind: &'static str,
    pub copy: bool,
    pub needs_drop: bool,
    pub ty: String,
}

#[derive(Serialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "ev")]
pub enum Fact {
    /// `path` gets a value (first time on every path: `init`; otherwise
    /// `reassign`, with `maybe_had` when it may have had none before).
    Init { path: String, from: String, reassign: bool, maybe_had: bool },
    Move { path: String, to: String },
    Copy { path: String, to: String },
    Borrow { loan: usize, lender: String, kind: String, holder: String },
    /// The loan is no longer in scope for rustc's borrowck after this
    /// row, on the path through `to_line` (the next row of that path).
    BorrowEnd { loan: usize, lender: String, holder: String, why: String, to_line: usize },
    /// `holder` (a reference) gets a new loan while it held `old`.
    Repoint { holder: String, old: Vec<usize>, new: Vec<usize> },
    /// A `Drop` of `path`: "drops" (always runs), "drops_if_init" (a drop
    /// flag decides), "drops_rest" (some fields were moved out).
    Drop { path: String, how: String, at_reassign: bool },
    /// `StorageDead` of a variable with nothing left to drop.
    ScopeEnd { path: String, note: String },
    TempDrop { what: String },
}

#[derive(Serialize, Default)]
pub struct Row {
    pub line: usize,
    /// path -> "own" | "moved" | "maybe" | "partly" | "unassigned"
    pub state: BTreeMap<String, String>,
    /// lender path -> ["shared:<loan>->holder", "mut:<loan>->holder"]
    pub lent: BTreeMap<String, Vec<String>>,
    pub facts: Vec<Fact>,
}

#[derive(Serialize)]
pub struct BodyFacts {
    pub func: String,
    /// No `SwitchInt`: the MIR-fed render (`rv3_events`) handles it.
    pub straight_line: bool,
    pub first_line: usize,
    pub last_line: usize,
    pub vars: Vec<VarInfo>,
    pub loans: Vec<LoanInfo>,
    pub rows: Vec<Row>,
    pub notes: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct LoanInfo {
    pub loan: usize,
    pub line: usize,
    pub lender: String,
    pub kind: String,
    pub holders: Vec<String>,
    pub end_lines: Vec<usize>,
}

struct Cx<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    bwf: &'a BodyWithBorrowckFacts<'tcx>,
    body: &'a Body<'tcx>,
    names: BTreeMap<Local, String>,
    rows: std::collections::HashMap<Location, usize>,
}

impl<'a, 'tcx> Cx<'a, 'tcx> {
    fn span_of(&self, l: Location) -> Span {
        let sp = self.body.source_info(l).span;
        sp.as_local(self.body.span).unwrap_or(sp)
    }

    fn line_lo(&self, sp: Span) -> usize {
        self.tcx.sess.source_map().lookup_char_pos(sp.source_callsite().lo()).line
    }

    fn line_hi(&self, sp: Span) -> usize {
        self.tcx.sess.source_map().lookup_char_pos(sp.source_callsite().hi()).line
    }

    /// The row a location belongs to. An assignment or call whose span
    /// covers several rows takes effect when its right-hand side is done:
    /// its last row. Everything else (conditions, gotos, drops) its first.
    fn row_of(&self, l: Location) -> usize {
        self.rows.get(&l).copied().unwrap_or_else(|| self.raw_row(l))
    }

    /// Rows along the CFG: a location's own row, but never above the row
    /// of the code that runs just before it on its only way in. MIR gives
    /// `drop(h); h = move tmp;` for `h = if c {..} else {..};` the span of
    /// `h` (the first row), though it runs after the arms.
    fn compute_rows(&self) -> std::collections::HashMap<Location, usize> {
        let body = self.body;
        let rpo: Vec<BasicBlock> = body.basic_blocks.reverse_postorder().to_vec();
        let order: std::collections::HashMap<BasicBlock, usize> = rpo.iter().enumerate().map(|(i, b)| (*b, i)).collect();
        let preds = body.basic_blocks.predecessors();
        let mut rows: std::collections::HashMap<Location, usize> = std::collections::HashMap::new();
        for bb in rpo {
            let data = &body.basic_blocks[bb];
            if data.is_cleanup {
                continue;
            }
            let fwd: Vec<BasicBlock> = preds[bb].iter().copied()
                .filter(|p| !body.basic_blocks[*p].is_cleanup && order.get(p).is_some_and(|o| *o < order[&bb]))
                .collect();
            let mut floor = 0;
            if fwd.len() == 1 && preds[bb].len() == 1 {
                let p = fwd[0];
                floor = rows.get(&Location { block: p, statement_index: body.basic_blocks[p].statements.len() }).copied().unwrap_or(0);
            }
            for i in 0..=data.statements.len() {
                let loc = Location { block: bb, statement_index: i };
                let r = self.raw_row(loc).max(floor);
                rows.insert(loc, r);
                floor = r;
            }
        }
        rows
    }

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
        // Desugarings (`for`, `?`, `while let`) carry the whole construct's
        // span: their calls happen at its start.
        let desugared = self.body.source_info(l).span.desugaring_kind().is_some();
        let effect_at_end = !desugared && match data.statements.get(l.statement_index) {
            Some(st) => matches!(st.kind, StatementKind::Assign(..)),
            None => matches!(data.terminator().kind, TerminatorKind::Call { .. } | TerminatorKind::Drop { .. }),
        };
        if effect_at_end { self.line_hi(sp) } else { self.line_lo(sp) }
    }

    /// The callee as the user wrote it (the name the HIR layer gives its
    /// function column): `String::from`, `push_str`, `println!`.
    fn call_name(&self, fn_span: Span) -> String {
        let sm = self.tcx.sess.source_map();
        if fn_span.from_expansion() && fn_span.desugaring_kind().is_none() {
            let cs = fn_span.source_callsite();
            let t = sm.span_to_snippet(cs).unwrap_or_default();
            if let Some(i) = t.find('!') {
                return t[..=i].trim().to_string();
            }
        }
        let t = sm.span_to_snippet(fn_span).unwrap_or_default();
        let head = t.split('(').next().unwrap_or("").trim();
        let head = head.rsplit('.').next().unwrap_or(head).trim();
        if head.is_empty() { "<call>".into() } else { head.split("::<").next().unwrap_or(head).to_string() }
    }

    fn place_name(&self, place: Place<'tcx>) -> Option<String> {
        let mut name = self.names.get(&place.local)?.clone();
        let mut ty = self.body.local_decls[place.local].ty;
        for elem in place.projection {
            match elem {
                ProjectionElem::Field(f, fty) => {
                    match ty.kind() {
                        TyKind::Adt(def, _) if def.is_struct() => {
                            name = format!("{}.{}", name, def.non_enum_variant().fields[f].name)
                        }
                        TyKind::Tuple(_) => name = format!("{}.{}", name, f.index()),
                        TyKind::Closure(..) => name = format!("{}.<cap{}>", name, f.index()),
                        _ => name = format!("{}.{}", name, f.index()),
                    }
                    ty = fty;
                }
                ProjectionElem::Deref => {
                    name = format!("*{}", name);
                    ty = ty.builtin_deref(true).unwrap_or(ty);
                }
                ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. } | ProjectionElem::Subslice { .. } => {
                    name = format!("{}[_]", name);
                    ty = match ty.kind() {
                        TyKind::Array(t, _) | TyKind::Slice(t) => *t,
                        _ => ty,
                    };
                }
                ProjectionElem::Downcast(Some(sym), _) => name = format!("({} as {})", name, sym),
                _ => {}
            }
        }
        Some(name)
    }

    fn is_user(&self, l: Local) -> bool {
        self.names.contains_key(&l)
    }

    /// The user-visible destination of a value stored into `place` at
    /// `loc`: a user variable, or — through MIR temporaries on the same
    /// row — the variable / call / closure / return that consumes it.
    fn dest_name(&self, place: Place<'tcx>, loc: Location, depth: usize) -> String {
        if let Some(n) = self.place_name(place) {
            return n;
        }
        if place.local == rustc_middle::mir::RETURN_PLACE {
            return "<return>".into();
        }
        if depth > 6 || !place.projection.is_empty() {
            return "<temp>".into();
        }
        // Follow the temporary forward to its consumer.
        let tmp = place.local;
        let row = self.row_of(loc);
        let mut best: Option<String> = None;
        for (bb, data) in self.body.basic_blocks.iter_enumerated() {
            if data.is_cleanup {
                continue;
            }
            for (i, st) in data.statements.iter().enumerate() {
                let here = Location { block: bb, statement_index: i };
                if let StatementKind::Assign(asg) = &st.kind {
                    let (lhs, rv) = &**asg;
                    if lhs.local == tmp {
                        continue;
                    }
                    let uses = match rv {
                        Rvalue::Use(Operand::Move(p) | Operand::Copy(p)) => p.local == tmp,
                        Rvalue::Aggregate(_, ops) => ops.iter().any(|o| matches!(o, Operand::Move(p) | Operand::Copy(p) if p.local == tmp)),
                        Rvalue::Ref(_, _, p) => p.local == tmp && p.projection.first() == Some(&ProjectionElem::Deref),
                        Rvalue::Cast(_, Operand::Move(p) | Operand::Copy(p), _) => p.local == tmp,
                        _ => false,
                    };
                    if uses && self.row_of(here).abs_diff(row) <= 3 {
                        let what = match rv {
                            Rvalue::Aggregate(k, _) if matches!(**k, AggregateKind::Closure(..)) => "closure ",
                            _ => "",
                        };
                        let d = self.dest_name(*lhs, here, depth + 1);
                        best = Some(if what.is_empty() { d } else { format!("{}{}", what, d) });
                    }
                }
            }
            let tl = Location { block: bb, statement_index: data.statements.len() };
            if let TerminatorKind::Call { args, fn_span, .. } = &data.terminator().kind {
                if args.iter().any(|a| matches!(&a.node, Operand::Move(p) | Operand::Copy(p) if p.local == tmp)) && best.is_none() {
                    let f = self.call_name(*fn_span);
                    let _ = tl;
                    best = Some(format!("{}()", f));
                }
            }
        }
        best.unwrap_or_else(|| "<temp>".into())
    }
}

/// User-variable names of a body's locals, the same names the plugin uses
/// for its columns (`s`, `s₂`).
fn local_names<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> BTreeMap<Local, String> {
    let owner = tcx.local_def_id_to_hir_id(body.source.def_id().expect_local()).owner;
    let names = binding_names(&tcx, owner);
    let mut res: BTreeMap<Local, String> = BTreeMap::new();
    for vdi in body.var_debug_info.iter() {
        let VarDebugInfoContents::Place(place) = vdi.value else { continue };
        if !place.projection.is_empty() {
            continue;
        }
        let span = vdi.source_info.span;
        if span.from_expansion() && span.desugaring_kind().is_none() || span.desugaring_kind().is_some_and(|k| !matches!(k, rustc_span::DesugaringKind::ForLoop) ) {
            continue;
        }
        // Only bindings the user wrote (`val` of a `?`, `iter` of a `for`,
        // `args` of `format_args!` are MIR's).
        let written = tcx.sess.source_map().span_to_snippet(span).unwrap_or_default();
        let w = written.trim_start_matches("ref ").trim_start_matches("mut ").trim();
        if w != vdi.name.as_str() && !(w == "self" || w.ends_with("self")) {
            continue;
        }
        let ident = vdi.name.as_str();
        let name = names
            .by_span
            .iter()
            .find(|(sp, n)| span.contains(*sp) && written_name(n) == ident)
            .map_or_else(|| ident.to_owned(), |(_, n)| n.clone());
        res.entry(place.local).or_insert(name);
    }
    res
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

pub fn body_facts<'tcx>(tcx: TyCtxt<'tcx>, bwf: &BodyWithBorrowckFacts<'tcx>, func: String) -> BodyFacts {
    let body = &bwf.body;
    let names = local_names(tcx, body);
    let mut cx = Cx { tcx, bwf, body, names, rows: Default::default() };
    cx.rows = cx.compute_rows();
    let cx = cx;
    let mut notes: Vec<String> = Vec::new();
    let def_id = body.source.def_id();
    let typing_env = rustc_middle::ty::TypingEnv::post_analysis(tcx, def_id);

    // ---- variables -------------------------------------------------------
    let mut vars: Vec<VarInfo> = Vec::new();
    for (local, name) in &cx.names {
        let decl = &body.local_decls[*local];
        let ty = tcx.erase_and_anonymize_regions(decl.ty);
        let kind = match ty.kind() {
            TyKind::Ref(_, _, rustc_middle::mir::Mutability::Mut) => "mut_ref",
            TyKind::Ref(..) => "shared_ref",
            _ => "owner",
        };
        vars.push(VarInfo {
            name: name.clone(),
            local: local.index(),
            decl_line: cx.line_lo(decl.source_info.span),
            kind,
            copy: tcx.type_is_copy_modulo_regions(typing_env, ty),
            needs_drop: ty.needs_drop(tcx, typing_env),
            ty: format!("{}", ty),
        });
    }

    // ---- dataflow --------------------------------------------------------
    let move_data = MoveData::gather_moves(body, tcx, |_| true);
    let mut init = MaybeInitializedPlaces::new(tcx, body, &move_data)
        .iterate_to_fixpoint(tcx, body, None)
        .into_results_cursor(body);
    let mut uninit = MaybeUninitializedPlaces::new(tcx, body, &move_data)
        .iterate_to_fixpoint(tcx, body, None)
        .into_results_cursor(body);
    let mut ever = EverInitializedPlaces::new(body, &move_data)
        .iterate_to_fixpoint(tcx, body, None)
        .into_results_cursor(body);
    let borrows = Borrows::new(tcx, body, &bwf.region_inference_context, &bwf.borrow_set);
    let mut inscope = borrows.iterate_to_fixpoint(tcx, body, None).into_results_cursor(body);

    // Move paths of user variables and their field paths, with names.
    let mut paths: Vec<(MovePathIndex, String)> = Vec::new();
    for (mpi, mp) in move_data.move_paths.iter_enumerated() {
        if !cx.is_user(mp.place.local) {
            continue;
        }
        if mp.place.projection.iter().any(|e| !matches!(e, ProjectionElem::Field(..))) {
            continue;
        }
        if let Some(n) = cx.place_name(mp.place) {
            paths.push((mpi, n));
        }
    }
    let descendants = |mpi: MovePathIndex| -> Vec<MovePathIndex> {
        let mut out = Vec::new();
        let mut work = vec![mpi];
        while let Some(m) = work.pop() {
            out.push(m);
            let mut c = move_data.move_paths[m].first_child;
            while let Some(ch) = c {
                work.push(ch);
                c = move_data.move_paths[ch].next_sibling;
            }
        }
        out
    };
    let ancestors = |mpi: MovePathIndex| -> Vec<MovePathIndex> {
        let mut out = vec![mpi];
        let mut p = move_data.move_paths[mpi].parent;
        while let Some(x) = p {
            out.push(x);
            p = move_data.move_paths[x].parent;
        }
        out
    };

    // ---- rows and row exits ---------------------------------------------
    // A row's exits: its locations with a successor on another row (or a
    // `return`). The state shown for a row is the union over its exits,
    // so rows inside an arm only see that arm's paths.
    let mut row_exits_all: BTreeMap<usize, Vec<(Location, Vec<usize>)>> = BTreeMap::new();
    let mut all_rows: BTreeSet<usize> = BTreeSet::new();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        if data.is_cleanup {
            continue;
        }
        for i in 0..=data.statements.len() {
            let loc = Location { block: bb, statement_index: i };
            let row = cx.row_of(loc);
            all_rows.insert(row);
            let succ_rows: Vec<usize> = if i < data.statements.len() {
                vec![cx.row_of(Location { block: bb, statement_index: i + 1 })]
            } else {
                data.terminator()
                    .successors()
                    .filter(|s| !body.basic_blocks[*s].is_cleanup)
                    .map(|s| cx.row_of(Location { block: s, statement_index: 0 }))
                    .collect()
            };
            if succ_rows.is_empty() || succ_rows.iter().any(|r| *r != row) {
                row_exits_all.entry(row).or_default().push((loc, succ_rows));
            }
        }
    }
    // The state shown after a row is the one the next row sees (as the
    // oracle's probe inserted after it would): only the exits that go on to
    // the nearest later row, not a `?`'s or `break`'s jump further down,
    // nor the way out of a loop head. If every exit goes back up (a loop's
    // last row), all of them.
    let mut row_exits: BTreeMap<usize, Vec<Location>> = BTreeMap::new();
    for (row, exits) in &row_exits_all {
        let nxt = exits.iter().flat_map(|(_, rs)| rs.iter().copied()).filter(|r| *r > *row).min();
        let keep: Vec<Location> = match nxt {
            Some(n) => exits.iter().filter(|(_, rs)| rs.contains(&n)).map(|(l, _)| *l).collect(),
            None => exits.iter().map(|(l, _)| *l).collect(),
        };
        row_exits.insert(*row, keep);
    }
    let first_line = cx.line_lo(body.span);
    let last_line = cx.line_hi(body.span);

    let state_at = |init: &mut rustc_mir_dataflow::ResultsCursor<'_, 'tcx, MaybeInitializedPlaces<'_, 'tcx>>,
                    uninit: &mut rustc_mir_dataflow::ResultsCursor<'_, 'tcx, MaybeUninitializedPlaces<'_, 'tcx>>,
                    ever: &mut rustc_mir_dataflow::ResultsCursor<'_, 'tcx, EverInitializedPlaces<'_, 'tcx>>,
                    locs: &[Location],
                    after: bool,
                    mpi: MovePathIndex|
     -> &'static str {
        let (mut may_init, mut may_uninit, mut sub_uninit, mut ever_init) = (false, false, false, false);
        let anc = ancestors(mpi);
        let desc = descendants(mpi);
        for &l in locs {
            if after {
                init.seek_after_primary_effect(l);
                uninit.seek_after_primary_effect(l);
                ever.seek_after_primary_effect(l);
            } else {
                init.seek_before_primary_effect(l);
                uninit.seek_before_primary_effect(l);
                ever.seek_before_primary_effect(l);
            }
            if init.get().contains(mpi) {
                may_init = true;
            }
            if uninit.get().contains(mpi) {
                may_uninit = true;
            }
            if desc.iter().skip(1).any(|d| uninit.get().contains(*d)) {
                sub_uninit = true;
            }
            for i in ever.get().iter() {
                if anc.contains(&move_data.inits[i].path) || desc.contains(&move_data.inits[i].path) {
                    ever_init = true;
                }
            }
        }
        match (may_init, may_uninit) {
            (true, false) if sub_uninit => "partly",
            (true, false) => "own",
            (true, true) => "maybe",
            (false, _) if ever_init => "moved",
            (false, _) => "unassigned",
        }
    };

    // ---- loans ------------------------------------------------------------
    let facts = bwf.input_facts.as_ref();
    let table = bwf.location_table.as_ref();
    let pol = facts.map(|f| Output::compute(f, Algorithm::Naive, true));
    let mut origins_of: BTreeMap<Local, Vec<Origin>> = BTreeMap::new();
    if let Some(f) = facts {
        for (local, origin) in f.use_of_var_derefs_origin.iter().chain(f.drop_of_var_derefs_origin.iter()) {
            if cx.is_user(*local) {
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
                out.push(cx.names[local].clone());
            }
        }
        out
    };
    let mut holders_any: BTreeMap<BorrowIndex, Vec<(usize, String)>> = BTreeMap::new();
    if let (Some(pol), Some(t)) = (pol.as_ref(), table) {
        for (p, m) in pol.origin_contains_loan_at.iter() {
            let row = cx.row_of(t.to_location(*p));
            for (local, os) in &origins_of {
                for o in os {
                    for l in m.get(o).into_iter().flatten() {
                        let v = holders_any.entry(*l).or_default();
                        match v.iter_mut().find(|(_, n)| *n == cx.names[local]) {
                            Some(e) => e.0 = e.0.min(row),
                            None => v.push((row, cx.names[local].clone())),
                        }
                    }
                }
            }
        }
        for v in holders_any.values_mut() {
            v.sort();
        }
    }
    let kind_str = |k: BorrowKind| -> String {
        match k {
            BorrowKind::Shared => "shared".into(),
            BorrowKind::Fake(_) => "fake".into(),
            BorrowKind::Mut { kind: MutBorrowKind::TwoPhaseBorrow } => "mut(two-phase)".into(),
            BorrowKind::Mut { kind: MutBorrowKind::ClosureCapture } => "unique(closure)".into(),
            BorrowKind::Mut { .. } => "mut".into(),
        }
    };

    let mut loans: Vec<LoanInfo> = Vec::new();
    let mut by_row: BTreeMap<usize, Vec<Fact>> = BTreeMap::new();
    for (idx, (loc, bd)) in bwf.borrow_set.location_map().iter().enumerate() {
        let bi = BorrowIndex::from_usize(idx);
        let kind = kind_str(bd.kind());
        if kind == "fake" {
            continue;
        }
        let lender = cx.place_name(bd.borrowed_place()).unwrap_or_else(|| format!("{:?}", bd.borrowed_place()));
        let row = cx.row_of(*loc);
        // Holder: the assigned place's user variable (through temporaries
        // on the row), else whoever's type holds the loan right after.
        let mut holders: Vec<String> = Vec::new();
        let mut direct = cx.dest_name(bd.assigned_place(), *loc, 0);
        if direct.starts_with('<') {
            let sp = body.source_info(*loc).span;
            if sp.from_expansion() && sp.desugaring_kind().is_none() {
                direct = format!("{}()", cx.call_name(sp));
            }
        }
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
        // Anyone whose type holds the loan at some point (Polonius'
        // `origin_contains_loan_at`), in order of the first row they do.
        if let Some(hs) = holders_any.get(&bi) {
            for (_, h) in hs {
                if !holders.contains(h) && *h != lender {
                    holders.push(h.clone());
                }
            }
        }
        if !cx.is_user(bd.borrowed_place().local) {
            // a loan of a MIR temporary (`&mut *_14` for `next()`): not a column
            continue;
        }
        // Skip MIR's own reborrows for a call whose result keeps nothing
        // and that no user variable holds: those are drawn as the call's
        // "passes a reference" event, not a column.
        let holder = if holders.is_empty() { format!("{}", direct) } else { holders.join("|") };
        by_row.entry(row).or_default().push(Fact::Borrow { loan: idx, lender: lender.clone(), kind: kind.clone(), holder });
        loans.push(LoanInfo { loan: idx, line: row, lender, kind, holders, end_lines: Vec::new() });
    }

    // Borrow ends: a location where a loan is in scope, with a successor
    // where it isn't. Per CFG edge, so each arm gets its own end.
    let mut seen_end: BTreeSet<(usize, usize, usize)> = BTreeSet::new();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        if data.is_cleanup {
            continue;
        }
        for i in 0..=data.statements.len() {
            let loc = Location { block: bb, statement_index: i };
            inscope.seek_before_primary_effect(loc);
            let here: Vec<BorrowIndex> = inscope.get().iter().collect();
            if here.is_empty() {
                continue;
            }
            let succs: Vec<Location> = if i < data.statements.len() {
                vec![Location { block: bb, statement_index: i + 1 }]
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
                    let row = cx.row_of(loc);
                    let to_line = cx.row_of(*s);
                    // Why: the holder is overwritten (re-point), goes out of
                    // scope, is dropped, or simply isn't used again.
                    let why = {
                        let sdata = &body.basic_blocks[s.block];
                        let mut w = "last use".to_string();
                        let st = sdata.statements.get(s.statement_index);
                        if let Some(st) = st {
                            match &st.kind {
                                StatementKind::Assign(asg) if cx.is_user(asg.0.local) && li.holders.contains(cx.names.get(&asg.0.local).unwrap()) => {
                                    w = "holder overwritten".into()
                                }
                                StatementKind::StorageDead(l) if cx.is_user(*l) => w = format!("{} out of scope", cx.names[l]),
                                _ => {}
                            }
                        } else if let TerminatorKind::Drop { place, .. } = &sdata.terminator().kind {
                            if let Some(n) = cx.place_name(*place) {
                                w = format!("{} dropped", n);
                            }
                        }
                        if let TerminatorKind::Drop { place, .. } = &data.terminator().kind {
                            if i == data.statements.len() {
                                if let Some(n) = cx.place_name(*place) {
                                    w = format!("{} dropped", n);
                                }
                            }
                        }
                        w
                    };
                    if seen_end.insert((b.index(), row, to_line)) {
                        if !li.end_lines.contains(&row) {
                            li.end_lines.push(row);
                        }
                        by_row.entry(row).or_default().push(Fact::BorrowEnd {
                            loan: b.index(),
                            lender: li.lender.clone(),
                            holder: li.holders.join("|"),
                            why,
                            to_line,
                        });
                    }
                }
            }
        }
    }

    // ---- moves, copies, inits, re-points, drops, scope ends ---------------
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        if data.is_cleanup {
            continue;
        }
        for (i, st) in data.statements.iter().enumerate() {
            let loc = Location { block: bb, statement_index: i };
            let row = cx.row_of(loc);
            match &st.kind {
                StatementKind::Assign(asg) => {
                    let (lhs, rv) = &**asg;
                    // moves / copies out of user places
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
                            Operand::Move(p) if cx.is_user(p.local) => {
                                let to = cx.dest_name(*lhs, loc, 0);
                                let path = cx.place_name(*p).unwrap_or_default();
                                if !path.starts_with('*') || body.local_decls[p.local].ty.is_box() {
                                    by_row.entry(row).or_default().push(Fact::Move { path, to });
                                }
                            }
                            Operand::Copy(p) if cx.is_user(p.local) => {
                                let to = cx.dest_name(*lhs, loc, 0);
                                if to != "<temp>" {
                                    by_row.entry(row).or_default().push(Fact::Copy { path: cx.place_name(*p).unwrap_or_default(), to });
                                }
                            }
                            _ => {}
                        }
                    }
                    // inits / reassigns / re-points of user places
                    if cx.is_user(lhs.local) && lhs.projection.iter().all(|e| matches!(e, ProjectionElem::Field(..))) {
                        let path = cx.place_name(*lhs).unwrap_or_default();
                        let from = match rv {
                            Rvalue::Use(Operand::Move(p) | Operand::Copy(p)) => cx.place_name(*p).unwrap_or_else(|| "<value>".into()),
                            Rvalue::Ref(_, k, p) => format!("&{}{}", if matches!(k, BorrowKind::Mut { .. }) { "mut " } else { "" }, cx.place_name(*p).unwrap_or_else(|| "<place>".into())),
                            Rvalue::Aggregate(..) => "<aggregate>".into(),
                            Rvalue::Use(Operand::Constant(_)) => "<const>".into(),
                            _ => "<value>".into(),
                        };
                        let (reassign, maybe_had) = match move_data.rev_lookup.find(lhs.as_ref()) {
                            LookupResult::Exact(mpi) => {
                                let at = before_reassign_drop(body, loc, *lhs);
                                let s = state_at(&mut init, &mut uninit, &mut ever, &at, false, mpi);
                                (s != "unassigned", s == "maybe" || s == "moved")
                            }
                            LookupResult::Parent(_) => (false, false),
                        };
                        let ty = body.local_decls[lhs.local].ty;
                        if ty.is_ref() && reassign && lhs.projection.is_empty() {
                            // the loans this reference held just before, and after
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
                                let old = held(t.start_index(loc));
                                let new = held(t.mid_index(loc));
                                by_row.entry(row).or_default().push(Fact::Repoint { holder: path.clone(), old, new });
                            }
                        }
                        by_row.entry(row).or_default().push(Fact::Init { path, from, reassign, maybe_had });
                    }
                }
                StatementKind::StorageDead(l) if cx.is_user(*l) => {
                    let ty = tcx.erase_and_anonymize_regions(body.local_decls[*l].ty);
                    if !ty.needs_drop(tcx, typing_env) {
                        let note = if tcx.type_is_copy_modulo_regions(typing_env, ty) { "copy: nothing to drop" } else { "no drop glue" };
                        by_row.entry(row).or_default().push(Fact::ScopeEnd { path: cx.names[l].clone(), note: note.into() });
                    }
                    // (a needs-drop variable always has a `Drop` terminator at
                    // its scope end in this MIR; it is classified there.)
                }
                _ => {}
            }
        }
        // terminators: calls (moves into args), drops
        let loc = Location { block: bb, statement_index: data.statements.len() };
        let row = cx.row_of(loc);
        match &data.terminator().kind {
            TerminatorKind::Call { args, destination, fn_span, .. } => {
                let f = cx.call_name(*fn_span);
                for a in args.iter() {
                    match &a.node {
                        Operand::Move(p) if cx.is_user(p.local) => {
                            by_row.entry(row).or_default().push(Fact::Move { path: cx.place_name(*p).unwrap_or_default(), to: format!("{}()", f) });
                        }
                        Operand::Copy(p) if cx.is_user(p.local) => {
                            by_row.entry(row).or_default().push(Fact::Copy { path: cx.place_name(*p).unwrap_or_default(), to: format!("{}()", f) });
                        }
                        _ => {}
                    }
                }
                if cx.is_user(destination.local) && destination.projection.is_empty() {
                    let path = cx.place_name(*destination).unwrap_or_default();
                    let (reassign, maybe_had) = match move_data.rev_lookup.find(destination.as_ref()) {
                        LookupResult::Exact(mpi) => {
                            let s = state_at(&mut init, &mut uninit, &mut ever, &[loc], false, mpi);
                            (s != "unassigned", s == "maybe" || s == "moved")
                        }
                        LookupResult::Parent(_) => (false, false),
                    };
                    by_row.entry(row).or_default().push(Fact::Init { path, from: format!("{}()", f), reassign, maybe_had });
                }
            }
            TerminatorKind::Drop { place, target, .. } => {
                // Is this the drop before an assignment (`s = ...;`)?
                let tdata = &body.basic_blocks[*target];
                let at_reassign = matches!(tdata.statements.first().map(|s| &s.kind),
                    Some(StatementKind::Assign(asg)) if asg.0 == *place);
                if cx.is_user(place.local) {
                    let path = cx.place_name(*place).unwrap_or_default();
                    let how = match move_data.rev_lookup.find(place.as_ref()) {
                        LookupResult::Exact(mpi) => match state_at(&mut init, &mut uninit, &mut ever, &[loc], false, mpi) {
                            "own" => "drops",
                            "partly" => "drops_rest",
                            "maybe" => "drops_if_init",
                            _ => "nothing_to_drop",
                        },
                        LookupResult::Parent(_) => "drops",
                    };
                    by_row.entry(row).or_default().push(Fact::Drop { path, how: how.into(), at_reassign });
                } else if place.local != rustc_middle::mir::RETURN_PLACE && place.projection.is_empty()
                    && !body.local_decls[place.local].source_info.span.from_expansion()
                    && matches!(move_data.rev_lookup.find(place.as_ref()), LookupResult::Exact(mpi)
                        if matches!(state_at(&mut init, &mut uninit, &mut ever, &[loc], false, mpi), "own" | "maybe" | "partly"))
                {
                    let decl = &body.local_decls[place.local];
                    let sp = decl.source_info.span;
                    let what = tcx.sess.source_map().span_to_snippet(sp.source_callsite()).unwrap_or_else(|_| "?".into());
                    let what: String = what.chars().take(40).collect();
                    by_row.entry(row).or_default().push(Fact::TempDrop { what: format!("{} : {}", what, decl.ty) });
                }
            }
            _ => {}
        }
    }

    // ---- per-row state -----------------------------------------------------
    let mut rows: Vec<Row> = Vec::new();
    let lines: BTreeSet<usize> = all_rows.iter().copied().filter(|l| *l >= first_line && *l <= last_line).collect();
    for line in lines {
        let mut row = Row { line, ..Default::default() };
        if let Some(exits) = row_exits.get(&line) {
            for (mpi, name) in &paths {
                let s = state_at(&mut init, &mut uninit, &mut ever, exits, true, *mpi);
                if s != "unassigned" || name.contains('.') {
                    row.state.insert(name.clone(), s.to_string());
                }
            }
            // lent: loans in scope after the row's exits
            let mut live: BTreeSet<usize> = BTreeSet::new();
            for e in exits {
                inscope.seek_after_primary_effect(*e);
                live.extend(inscope.get().iter().map(|b| b.index()));
            }
            for l in &loans {
                if live.contains(&l.loan) {
                    let k = if l.kind.starts_with("mut") || l.kind.starts_with("unique") { "mut" } else { "shared" };
                    row.lent.entry(l.lender.clone()).or_default().push(format!("{}:L{}->{}", k, l.loan, l.holders.join("|")));
                }
            }
        }
        let mut fs = by_row.remove(&line).unwrap_or_default();
        fs.sort();
        fs.dedup();
        row.facts = fs;
        rows.push(row);
    }
    for (line, fs) in by_row {
        if !fs.is_empty() {
            notes.push(format!("{} facts on row {} outside the body's rows", fs.len(), line));
        }
    }
    let _ = BasicBlock::from_usize(0);
    let straight_line = body.basic_blocks.iter().all(|d| d.is_cleanup || !matches!(d.terminator().kind, TerminatorKind::SwitchInt { .. }));
    BodyFacts { func, straight_line, first_line, last_line, vars, loans, rows, notes }
}

/// Entry point from `plugin::rv_visitor`: append one body's facts.
pub fn collect<'tcx>(tcx: TyCtxt<'tcx>, def_id: rustc_span::def_id::LocalDefId, out: &mut Vec<BodyFacts>) {
    let bwf = rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, def_id);
    let func = tcx.def_path_str(def_id.to_def_id());
    out.push(body_facts(tcx, bwf, func));
}

pub fn write(out: &[BodyFacts]) {
    if let Some(path) = std::env::var_os("RV3_FACTS").filter(|v| !v.is_empty()) {
        if let Ok(s) = serde_json::to_string_pretty(out) {
            let _ = std::fs::write(path, s);
        }
    }
}

pub fn enabled() -> bool {
    std::env::var_os("RV3_FACTS").is_some_and(|v| !v.is_empty())
}
