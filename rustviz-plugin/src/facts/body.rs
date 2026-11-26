//! One pass per MIR body: NLL `Borrows`, Polonius (naive, once per
//! body), storage liveness, and the init analyses (MaybeInit /
//! MaybeUninit / EverInit) twice: on borrowck's CFG for states, and on
//! the CFG without its imaginary edges for what runs (drop kinds, which
//! events happen). Then read at the row model's locations.

use super::places::{column_projection, Capture, Names};
use super::rows::{is_unreachable, RowModel, Rows};
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
use rustc_mir_dataflow::impls::{
    always_storage_live_locals, EverInitializedPlaces, MaybeInitializedPlaces, MaybeStorageLive, MaybeUninitializedPlaces,
};
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
    fn call_name(&self, fn_span: Span, func: &Operand<'tcx>, at: Location) -> String {
        if let Some(m) = self.macro_of(fn_span) {
            return m;
        }
        // The call as the HIR has it: a method's name, a path's text, or the
        // callee expression (`factory()` in `factory()(s)`).
        let sm = self.tcx.sess.source_map();
        let hid = rustc_utils::BodyExt::location_to_hir_id(self.body, at);
        if let rustc_hir::Node::Expr(e) = self.tcx.hir_node(hid) {
            if !e.span.from_expansion() {
                match &e.kind {
                    rustc_hir::ExprKind::MethodCall(seg, ..) => return seg.ident.to_string(),
                    rustc_hir::ExprKind::Call(callee, _) => {
                        if let Ok(t) = sm.span_to_snippet(callee.span) {
                            let t = t.trim();
                            return t.split("::<").next().unwrap_or(t).to_string();
                        }
                    }
                    _ => {}
                }
            }
        }
        let t = self.tcx.sess.source_map().span_to_snippet(fn_span).unwrap_or_default();
        let head = t.split('(').next().unwrap_or("").trim();
        let head = head.rsplit('.').next().unwrap_or(head).trim();
        let head = head.split("::<").next().unwrap_or(head);
        // An operator (`v[..]`, `a + b`) or desugaring calls a trait
        // method the source doesn't name: the method's own name.
        let written = t.contains('(') && !head.is_empty() && head.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ':');
        if written {
            return head.to_string();
        }
        match func.const_fn_def() {
            Some((did, _)) => self.tcx.item_name(did).to_string(),
            None if head.is_empty() => "<call>".into(),
            None => head.to_string(),
        }
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
        // A MIR temporary is assigned once and used once: follow it to
        // that use (whatever the rows in between).
        let tmp = place.local;
        let _ = loc;
        let mut best: Option<String> = None;
        for &bb in self.body.basic_blocks.reverse_postorder() {
            let data = &self.body.basic_blocks[bb];
            if data.is_cleanup || best.is_some() {
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
                // (`c = move (_t.0)`: a field of the temporary, not what it
                // points to: `*_t` reads through a reference, it doesn't
                // pass the reference on.)
                let in_tmp = |o: &Operand<'tcx>| {
                    matches!(o, Operand::Move(p) | Operand::Copy(p)
                        if p.local == tmp && p.projection.iter().all(|e| matches!(e, ProjectionElem::Field(..))))
                };
                // An operand of arithmetic or a comparison: copied to
                // compute it, not into the result's place.
                let operand = match rv {
                    Rvalue::BinaryOp(_, ab) => is_tmp(&ab.0) || is_tmp(&ab.1),
                    Rvalue::UnaryOp(_, o) => is_tmp(o),
                    _ => false,
                };
                if operand {
                    best = Some("<operand>".into());
                    continue;
                }
                let field = match rv {
                    Rvalue::Use(o) => in_tmp(o).then_some(None),
                    Rvalue::Cast(_, o, _) => is_tmp(o).then_some(None),
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
                if best.is_some() {
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
            if let TerminatorKind::Call { args, fn_span, func, .. } = &data.terminator().kind {
                if best.is_none() && args.iter().any(|a| matches!(&a.node, Operand::Move(p) | Operand::Copy(p) if p.local == tmp)) {
                    let at = Location { block: bb, statement_index: data.statements.len() };
                    best = Some(format!("{}()", self.call_name(*fn_span, func, at)));
                }
            }
        }
        best.unwrap_or_else(|| "<temp>".into())
    }

    /// The span of the statement whose evaluation first assigns `local`
    /// (a temporary): its `let` / expression statement.
    fn stmt_span_of_local(&self, local: Local) -> Option<Span> {
        let body = self.body;
        let at = body.basic_blocks.reverse_postorder().iter().find_map(|bb| {
            let data = &body.basic_blocks[*bb];
            if data.is_cleanup {
                return None;
            }
            data.statements.iter().enumerate().find_map(|(i, st)| match &st.kind {
                StatementKind::Assign(asg) if asg.0.local == local => Some(Location { block: *bb, statement_index: i }),
                _ => None,
            })
            .or_else(|| match &data.terminator().kind {
                TerminatorKind::Call { destination, .. } if destination.local == local => {
                    Some(Location { block: *bb, statement_index: data.statements.len() })
                }
                _ => None,
            })
        })?;
        let hir_id = rustc_utils::BodyExt::location_to_hir_id(body, at);
        let nodes = std::iter::once(self.tcx.hir_node(hir_id)).chain(self.tcx.hir_parent_iter(hir_id).map(|(_, n)| n));
        for n in nodes {
            match n {
                rustc_hir::Node::Stmt(st) => return Some(st.span),
                rustc_hir::Node::LetStmt(ls) => return Some(ls.span),
                rustc_hir::Node::Expr(_) => {}
                _ => return None,
            }
        }
        None
    }

    /// The borrow at `l` is written in the source (`&x`, `&mut *r`, `ref
    /// x`), not inserted by the compiler (a method call's auto-ref, the
    /// reborrow of a reference passed on, a macro's).
    fn written_borrow(&self, l: Location) -> bool {
        let sp = self.body.source_info(l).span;
        if sp.from_expansion() {
            return false;
        }
        // rustc's reborrow of a written borrow, which has the written one's
        // span (`&*m` passed where `&String` is expected: `_4 = &(*_5)`
        // after `_5 = &(*m)`): a borrow through a MIR temporary that a
        // borrow with this very span made.
        let body = self.body;
        if let Some(StatementKind::Assign(asg)) = body.basic_blocks[l.block].statements.get(l.statement_index).map(|st| &st.kind) {
            if let Rvalue::Ref(_, _, p) = &asg.1 {
                let temp = p.projection.first() == Some(&ProjectionElem::Deref)
                    && !body.local_decls[p.local].is_user_variable()
                    && p.local != RETURN_PLACE
                    && p.local.index() > body.arg_count;
                let made_here = temp && body.basic_blocks.iter().flat_map(|d| d.statements.iter()).any(|st| match &st.kind {
                    StatementKind::Assign(a) => a.0.local == p.local && a.0.projection.is_empty() && matches!(a.1, Rvalue::Ref(..)) && st.source_info.span == sp,
                    _ => false,
                });
                if made_here {
                    return false;
                }
            }
        }
        let t = self.tcx.sess.source_map().span_to_snippet(sp).unwrap_or_default();
        let t = t.trim_start();
        t.starts_with('&') || t.starts_with("ref ") || t.starts_with("ref\t") || t.contains(" ref ")
    }

    fn ev(&self, l: Location, fact: Fact) -> (usize, Ev) {
        let row = self.rows.event_row.get(&l).copied().unwrap_or_else(|| self.row_of(l));
        // An early exit's drop sits where the exit is: its column and arm.
        let at = self.rows.early_exit.get(&l).map_or(l, |(_, x)| *x);
        let mut order = self.rows.order(l);
        if matches!(fact, Fact::BorrowEnd { .. }) {
            order.2 = 1;
        }
        let ev = Ev { col: self.rows.col.get(&at).copied().unwrap_or(0), arm: self.rows.arm_of(at), fact, order, site: None };
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

/// A loan borrowck doesn't track (see `body_facts`), before it becomes a
/// `LoanInfo`.
struct Untracked<'tcx> {
    /// "caller" | "static" | "untracked"
    source: &'static str,
    /// Where it is taken (none for a parameter's: it comes with the call).
    at: Option<Location>,
    /// The origins it starts in, at which points.
    issue: Vec<(Origin, Point)>,
    lender: String,
    kind: &'static str,
    /// The borrowed place (an untracked borrow).
    place: Option<Place<'tcx>>,
    /// Where the reference is put.
    assigned: Place<'tcx>,
    param: Option<Local>,
    /// The field of an aggregate it is put in (`text` of `P { text: "x" }`).
    field: Option<String>,
}

struct Flow<'a, 'mir, 'tcx> {
    body: &'mir Body<'tcx>,
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
    /// The path is a Box whose payload (`*b`) was moved out (`let x =
    /// *b;`): `b` is partly moved (its allocation is still owned and
    /// freed at its scope end; nothing in it can be used).
    payload_moved: bool,
    payload_maybe: bool,
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

/// The locals the MIR mentions at `loc` and before it in its block on
/// the same row, latest first.
fn used_on_row(body: &Body<'_>, rows: &RowModel, loc: Location) -> Vec<Local> {
    use rustc_middle::mir::visit::{PlaceContext, Visitor};
    struct Locals(Vec<Local>);
    impl<'tcx> Visitor<'tcx> for Locals {
        fn visit_local(&mut self, l: Local, _: PlaceContext, _: Location) {
            if !self.0.contains(&l) {
                self.0.push(l);
            }
        }
    }
    let row = rows.row.get(&loc).copied();
    let data = &body.basic_blocks[loc.block];
    let mut out = Vec::new();
    for i in (0..=loc.statement_index).rev() {
        let l = Location { block: loc.block, statement_index: i };
        if rows.row.get(&l).copied() != row {
            break;
        }
        let mut v = Locals(Vec::new());
        match data.statements.get(i) {
            // (A storage marker uses nothing: `StorageDead(r)` doesn't make
            // r the loan's last user.)
            Some(st) if matches!(st.kind, StatementKind::StorageDead(_) | StatementKind::StorageLive(_)) => {}
            Some(st) => v.visit_statement(st, l),
            None => v.visit_terminator(data.terminator(), l),
        }
        for x in v.0 {
            if !out.contains(&x) {
                out.push(x);
            }
        }
    }
    out
}

/// Which move paths a move-out may have emptied (and nothing re-assigned
/// since), at each block's entry: tells a `maybe` that may have been
/// moved from one that may never have been assigned.
fn maybe_moved_entry(body: &Body<'_>, md: &MoveData<'_>) -> BTreeMap<rustc_middle::mir::BasicBlock, BTreeSet<MovePathIndex>> {
    let mut entry: BTreeMap<rustc_middle::mir::BasicBlock, BTreeSet<MovePathIndex>> = BTreeMap::new();
    let rpo: Vec<_> = body.basic_blocks.reverse_postorder().to_vec();
    let mut changed = true;
    while changed {
        changed = false;
        for &bb in &rpo {
            let data = &body.basic_blocks[bb];
            if data.is_cleanup {
                continue;
            }
            let mut set = entry.get(&bb).cloned().unwrap_or_default();
            for i in 0..=data.statements.len() {
                moved_transfer(md, body, &mut set, Location { block: bb, statement_index: i });
            }
            for succ in data.terminator().successors() {
                let e = entry.entry(succ).or_default();
                let before = e.len();
                e.extend(set.iter().copied());
                changed |= e.len() != before;
            }
        }
    }
    entry
}

fn moved_transfer(md: &MoveData<'_>, body: &Body<'_>, set: &mut BTreeSet<MovePathIndex>, loc: Location) {
    // A variable's storage starting or ending forgets its moves: a `let`
    // in a loop body starts each iteration unassigned, not "moved" by the
    // previous iteration's scope end.
    if let Some(StatementKind::StorageLive(l) | StatementKind::StorageDead(l)) =
        body.basic_blocks[loc.block].statements.get(loc.statement_index).map(|s| &s.kind)
    {
        set.retain(|m| md.move_paths[*m].place.local != *l);
    }
    for mo in md.loc_map[loc].iter() {
        set.insert(md.moves[*mo].path);
    }
    for ii in md.init_loc_map[loc].iter() {
        for d in descendants(md, md.inits[*ii].path) {
            set.remove(&d);
        }
    }
}

/// What a `Drop` of a place does, from its init state where it runs:
/// a Box whose payload was moved out still frees its allocation.
fn drop_how(st: &InitState) -> &'static str {
    match (st.may_init, st.may_uninit) {
        (true, false) if st.payload_moved || st.payload_maybe || !st.sub_moved.is_empty() || !st.sub_maybe.is_empty() => "drops_rest",
        (true, false) => "drops",
        (true, true) => "drops_if_init",
        _ => "nothing_to_drop",
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
        let mut s = InitState {
            may_init: false,
            may_uninit: false,
            ever_init: false,
            sub_maybe: Vec::new(),
            sub_moved: Vec::new(),
            payload_moved: false,
            payload_maybe: false,
        };
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
            // A call's destination is initialized on its return edge, which
            // "after the call" in the cursor doesn't include.
            let returned: Option<MovePathIndex> = match &self.body.basic_blocks[l.block].terminator().kind {
                TerminatorKind::Call { destination, .. } if after && l.statement_index == self.body.basic_blocks[l.block].statements.len() => {
                    match self.md.rev_lookup.find(destination.as_ref()) {
                        LookupResult::Exact(d) => Some(d),
                        LookupResult::Parent(_) => None,
                    }
                }
                _ => None,
            };
            let set_by_call = |m: MovePathIndex| returned.is_some_and(|d| ancestors(self.md, m).contains(&d));
            if set_by_call(mpi) {
                s.may_init = true;
            } else {
                s.may_init |= self.init.get().contains(mpi);
                s.may_uninit |= self.uninit.get().contains(mpi);
            }
            for d in desc.iter().skip(1) {
                if set_by_call(*d) {
                    sub_init.insert(*d);
                    continue;
                }
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
            let payload = self.md.move_paths[d].parent == Some(mpi)
                && self.md.move_paths[d].place.projection.last() == Some(&ProjectionElem::Deref);
            if sub_init.contains(&d) {
                s.payload_maybe |= payload;
                s.sub_maybe.push(d)
            } else {
                s.payload_moved |= payload;
                s.sub_moved.push(d)
            }
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
                Capture::ByUniqueRef => "by_unique_ref",
            }),
        });
    }

    // ---- dataflow, once --------------------------------------------------------
    let move_data = MoveData::gather_moves(body, tcx, |_| true);
    let moved_entry = maybe_moved_entry(body, &move_data);
    let mut flow = Flow {
        body,
        md: &move_data,
        init: MaybeInitializedPlaces::new(tcx, body, &move_data).iterate_to_fixpoint(tcx, body, None).into_results_cursor(body),
        uninit: MaybeUninitializedPlaces::new(tcx, body, &move_data).iterate_to_fixpoint(tcx, body, None).into_results_cursor(body),
        ever: EverInitializedPlaces::new(body, &move_data).iterate_to_fixpoint(tcx, body, None).into_results_cursor(body),
    };
    // What runs: drop elaboration works on the CFG without borrowck's
    // imaginary edges (`FalseEdge`'s `else` of an irrefutable `if let`), so
    // a drop's kind (definite / flag) is read there.
    let mut rt_body = body.clone();
    for data in rt_body.basic_blocks_mut().iter_mut() {
        let t = data.terminator_mut();
        if let TerminatorKind::FalseEdge { real_target, .. } | TerminatorKind::FalseUnwind { real_target, .. } = t.kind {
            t.kind = TerminatorKind::Goto { target: real_target };
        }
    }
    // Blocks that run: borrowck checks the imaginary ones too (states keep
    // its view), but events only happen on code that runs.
    let mut runs: BTreeSet<rustc_middle::mir::BasicBlock> = BTreeSet::new();
    {
        let mut work = vec![rustc_middle::mir::START_BLOCK];
        while let Some(bb) = work.pop() {
            // (MIR's `unreachable` fall-throughs don't run either.)
            if !is_unreachable(&rt_body, bb) && runs.insert(bb) {
                work.extend(rt_body.basic_blocks[bb].terminator().successors());
            }
        }
    }
    let rt_md = MoveData::gather_moves(&rt_body, tcx, |_| true);
    let mut rt = Flow {
        body: &rt_body,
        md: &rt_md,
        init: MaybeInitializedPlaces::new(tcx, &rt_body, &rt_md).iterate_to_fixpoint(tcx, &rt_body, None).into_results_cursor(&rt_body),
        uninit: MaybeUninitializedPlaces::new(tcx, &rt_body, &rt_md).iterate_to_fixpoint(tcx, &rt_body, None).into_results_cursor(&rt_body),
        ever: EverInitializedPlaces::new(&rt_body, &rt_md).iterate_to_fixpoint(tcx, &rt_body, None).into_results_cursor(&rt_body),
    };
    let mut storage = MaybeStorageLive::new(std::borrow::Cow::Owned(always_storage_live_locals(body)))
        .iterate_to_fixpoint(tcx, body, None)
        .into_results_cursor(body);
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
        column_info.push(ColumnInfo { name: n.clone(), root, kind, copy, needs_drop, hidden_fields: false });
        columns.push((mpi, mp.place, n));
    }
    // Fields with no move path of their own (nothing moves or assigns them
    // alone): rustc tracks them with the nearest ancestor that has one, so
    // their state follows it (`p.b` once `p.a` was moved is `own`; `h.r` is
    // moved with `h`). Every field the source can name gets a column: of a
    // tuple or a struct of this crate (any struct some of whose fields have
    // move paths), nested ones too, and the fields of a struct a reference
    // variable points to (`self.a` through `&mut self`: its loans restrict
    // `self.a` alone, its state is the reference's).
    let mut synth: Vec<(MovePathIndex, Place<'tcx>, String)> = Vec::new();
    // Fields the source can't name from this body (private to another
    // module) get no column.
    let here = tcx.parent_module_from_def_id(def_id.expect_local()).to_def_id();
    let mut work: Vec<(MovePathIndex, Place<'tcx>, bool)> = columns.iter().map(|(m, p, _)| (*m, *p, true)).collect();
    while let Some((mpi, place, real)) = work.pop() {
        let mut kids: BTreeSet<usize> = BTreeSet::new();
        let mut payload = false;
        if real {
            let mut c = move_data.move_paths[mpi].first_child;
            while let Some(ch) = c {
                match move_data.move_paths[ch].place.projection.last() {
                    Some(ProjectionElem::Field(f, _)) => {
                        kids.insert(f.index());
                    }
                    Some(ProjectionElem::Deref) => payload = true,
                    _ => {}
                }
                c = move_data.move_paths[ch].next_sibling;
            }
        }
        let parent_name = cx.name(place);
        let mut pty = place.ty(&body.local_decls, tcx).ty;
        let mut place = place;
        // Auto-deref, as the source's `x.f` does: through a reference
        // variable (`self.a` of `&mut self`, `r.f` of `&&S`) and through a
        // Box whose contents have no move path of their own (`b.s` of a
        // `Box<H>`; else `*b`'s own column gives them).
        if real && !payload {
            loop {
                let inner = match pty.kind() {
                    TyKind::Ref(_, inner, _) if place.projection.iter().all(|e| e == ProjectionElem::Deref) && cx.names.locals.contains_key(&place.local) => *inner,
                    _ if pty.is_box() => pty.boxed_ty().unwrap_or(pty),
                    _ => break,
                };
                if inner == pty {
                    break;
                }
                place = tcx.mk_place_deref(place);
                pty = inner;
            }
        }
        let nameable = match pty.kind() {
            TyKind::Adt(def, _) => def.is_struct() && (def.did().is_local() || !kids.is_empty()),
            TyKind::Tuple(_) => true,
            _ => false,
        };
        if !nameable {
            continue;
        }
        let fields: Vec<(rustc_middle::ty::Ty<'tcx>, bool)> = match pty.kind() {
            TyKind::Adt(def, args) => def
                .non_enum_variant()
                .fields
                .iter()
                .map(|fd| (fd.ty(tcx, args), tcx.visibility(fd.did).is_accessible_from(here, tcx)))
                .collect(),
            TyKind::Tuple(tys) => tys.iter().map(|t| (t, true)).collect(),
            _ => continue,
        };
        if fields.iter().enumerate().any(|(i, (_, visible))| !visible && !kids.contains(&i)) {
            if let Some(c) = parent_name.as_ref().and_then(|pn| column_info.iter_mut().find(|c| c.name == *pn)) {
                c.hidden_fields = true;
            }
        }
        for (i, (fty, visible)) in fields.into_iter().enumerate() {
            if kids.contains(&i) || !visible {
                continue;
            }
            let fp = tcx.mk_place_field(place, rustc_abi::FieldIdx::from_usize(i), fty);
            let Some(n) = cx.name(fp) else { continue };
            let Some((root, _, _)) = cx.names.root(fp) else { continue };
            // (Two variables of one name in sibling scopes share a column
            // name; each keeps its own places.)
            if !column_info.iter().any(|c| c.name == n) {
                let (copy, needs_drop, _) = ty_info(fty);
                column_info.push(ColumnInfo { name: n.clone(), root, kind: "field", copy, needs_drop, hidden_fields: false });
            }
            synth.push((mpi, fp, n));
            // (Nested structs and tuples; never through a reference or a
            // Box stored in a field, so the type nesting bounds this.)
            work.push((mpi, fp, false));
        }
    }
    let column_name = |mpi: MovePathIndex| columns.iter().find(|(m, _, _)| *m == mpi).map(|(_, _, n)| n.clone());

    // ---- Polonius, once ---------------------------------------------------------
    let pfacts = bwf.input_facts.as_ref();
    let table = bwf.location_table.as_ref();
    let pol = pfacts.map(|f| Output::compute(f, Algorithm::Naive, true));
    let mut origins_of: BTreeMap<Local, Vec<Origin>> = BTreeMap::new();
    // Every local's (temporaries' and the return place's too).
    let mut all_origins: BTreeMap<Local, Vec<Origin>> = BTreeMap::new();
    if let Some(f) = pfacts {
        for (local, origin) in f.use_of_var_derefs_origin.iter().chain(f.drop_of_var_derefs_origin.iter()) {
            let os = all_origins.entry(*local).or_default();
            if !os.contains(origin) {
                os.push(*origin);
            }
            if cx.names.locals.contains_key(local) {
                let os = origins_of.entry(*local).or_default();
                if !os.contains(origin) {
                    os.push(*origin);
                }
            }
        }
    }
    // ---- loans borrowck doesn't track ---------------------------------------------
    // What a reference parameter holds (the caller's loan: `self` of `&mut
    // self`), a reference to a constant (`"hi"`, a promoted `&5`), and a
    // borrow borrowck ignores because nothing in the body can conflict with
    // it (a shared reborrow through a shared reference, `&*r` or the auto-ref
    // of `r.len()`; a borrow of an immutable parameter that is never moved).
    // They get the loan ids after borrowck's and a Polonius run of their own,
    // on the body's location-specific subset facts only: rustc relates every
    // parameter's and the return value's region to the fn's universal regions
    // at every point, which would carry one parameter's loan into every
    // other parameter of the same lifetime (`longest<'a>(x: &'a str, y: &'a
    // str)`) and keep it there for the whole body.
    let base = bwf.borrow_set.location_map().len();
    let mut untracked: Vec<Untracked<'tcx>> = Vec::new();
    if let Some(t) = table {
        for p in body.args_iter() {
            let Some(name) = cx.names.locals.get(&p) else { continue };
            let TyKind::Ref(region, _, m) = body.local_decls[p].ty.kind() else { continue };
            let Some(vid) = region_vid(*region) else { continue };
            untracked.push(Untracked {
                source: "caller",
                at: None,
                issue: vec![(Origin::from(vid), t.start_index(Location::START))],
                lender: format!("*{}", name),
                kind: if m.is_mut() { "mut" } else { "shared" },
                place: None,
                assigned: Place::from(p),
                param: Some(p),
                field: None,
            });
        }
        let lmap = bwf.borrow_set.location_map();
        for (&loc, _) in &cx.rows.row {
            if !runs.contains(&loc.block) {
                continue;
            }
            let Some(st) = body.basic_blocks[loc.block].statements.get(loc.statement_index) else { continue };
            let StatementKind::Assign(asg) = &st.kind else { continue };
            let (lhs, rv) = &**asg;
            match rv {
                Rvalue::Ref(region, bk, place) if !matches!(bk, BorrowKind::Fake(_)) && !lmap.contains_key(&loc) => {
                    // (A deref coercion reborrows through a MIR temporary:
                    // `let s: &String = &gx;` is `&**_t` with `_t = &gx`.)
                    let resolved = resolve_temp_deref(tcx, body, *place);
                    // (Resolved to a place not behind a reference, it borrows
                    // what the temporary's own loan does: no new loan.)
                    if resolved != *place && !resolved.projection.contains(&ProjectionElem::Deref) {
                        continue;
                    }
                    let place = &resolved;
                    let (Some(lender), Some(vid)) = (cx.name(*place), region_vid(*region)) else { continue };
                    untracked.push(Untracked {
                        source: "untracked",
                        at: Some(loc),
                        issue: vec![(Origin::from(vid), t.mid_index(loc))],
                        lender,
                        kind: if matches!(bk, BorrowKind::Mut { .. }) { "mut" } else { "shared" },
                        place: Some(*place),
                        assigned: *lhs,
                        param: None,
                        field: None,
                    });
                }
                Rvalue::Use(Operand::Constant(c)) | Rvalue::Cast(_, Operand::Constant(c), _) if c.ty().is_ref() => {
                    // Only a constant some variable keeps (`let s = "hi";`, a
                    // field `h.r = "x"`), not one passed straight to a call.
                    let d = cx.dest_name(*lhs, loc, 0);
                    if d.starts_with('<') || d.ends_with("()") || d.starts_with("closure ") {
                        continue;
                    }
                    // (Into the origins of the place it is stored in: `p.0`'s,
                    // not every origin of `p`.)
                    let os: Vec<Origin> = regions_of(decl_place_ty(tcx, body, *lhs));
                    if os.is_empty() {
                        continue;
                    }
                    let text = tcx.sess.source_map().span_to_snippet(c.span).unwrap_or_default();
                    untracked.push(Untracked {
                        source: "static",
                        at: Some(loc),
                        issue: os.into_iter().map(|o| (o, t.mid_index(loc))).collect(),
                        lender: text.trim_start_matches('&').trim().to_string(),
                        kind: const_kind(c.ty()),
                        place: None,
                        assigned: *lhs,
                        param: None,
                        field: None,
                    });
                }
                Rvalue::Aggregate(k, ops) => {
                    // A constant in a struct or tuple literal (`P { text: "x" }`).
                    for (f, o) in ops.iter_enumerated() {
                        let Operand::Constant(c) = o else { continue };
                        if !c.ty().is_ref() {
                            continue;
                        }
                        let Some(field) = aggregate_field(tcx, k, f) else { continue };
                        let d = cx.dest_name(*lhs, loc, 0);
                        if user_place(&d).is_none() {
                            continue;
                        }
                        // (Into the field's own origins: `"left"` of `("left",
                        // "right")` is held by `p.0` only.)
                        let os: Vec<Origin> = aggregate_field_ty(tcx, k, decl_place_ty(tcx, body, *lhs), f).map(regions_of).unwrap_or_default();
                        if os.is_empty() {
                            continue;
                        }
                        let text = tcx.sess.source_map().span_to_snippet(c.span).unwrap_or_default();
                        untracked.push(Untracked {
                            source: "static",
                            at: Some(loc),
                            issue: os.into_iter().map(|o| (o, t.mid_index(loc))).collect(),
                            lender: text.trim_start_matches('&').trim().to_string(),
                            kind: const_kind(c.ty()),
                            place: None,
                            assigned: *lhs,
                            param: None,
                            field: Some(field),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    let spol = pfacts.filter(|_| !untracked.is_empty()).zip(table).map(|(f, t)| {
        let mut g = f.clone();
        g.loan_issued_at.clear();
        g.loan_killed_at.clear();
        g.loan_invalidated_at.clear();
        g.placeholder.clear();
        g.known_placeholder_subset.clear();
        // The regions inside what a reference parameter points to (`'a` of
        // `h: &mut Hold<'a>` or `v: &mut Vec<&'a T>`) are the caller's: a
        // loan stored there (`h.r = t`, `v.push(r)`) goes back with the
        // call and doesn't end in the body. Polonius keeps a universal
        // region live at every point.
        g.universal_region = body
            .args_iter()
            .flat_map(|p| match body.local_decls[p].ty.kind() {
                TyKind::Ref(_, inner, _) => regions_of(*inner),
                _ => Vec::new(),
            })
            .collect();
        // (The move-path facts stay: they say which drops can run, and a
        // drop that may read a reference keeps its loans live, e.g. an
        // `impl Drop` reading a field that holds a parameter's reference.)
        // (rustc emits a location-independent constraint at every point.)
        let n = t.all_points().count();
        let mut count: BTreeMap<(Origin, Origin), usize> = BTreeMap::new();
        for (a, b, _) in &g.subset_base {
            *count.entry((*a, *b)).or_default() += 1;
        }
        g.subset_base.retain(|(a, b, _)| n < 2 || count[&(*a, *b)] < n);
        for (i, u) in untracked.iter().enumerate() {
            for (o, p) in &u.issue {
                g.loan_issued_at.push((*o, BorrowIndex::from_usize(base + i), *p));
            }
        }
        Output::compute(&g, Algorithm::Naive, true)
    });
    // The Polonius output that tracks a loan.
    let out_for = |loan: BorrowIndex| if loan.index() >= base { spol.as_ref() } else { pol.as_ref() };
    let holders_at = |p: Point, loan: BorrowIndex| -> Vec<String> {
        let mut out = Vec::new();
        let Some(pol) = out_for(loan) else { return out };
        let Some(m) = pol.origin_contains_loan_at.get(&p) else { return out };
        for (local, os) in &origins_of {
            if os.iter().any(|o| m.get(o).is_some_and(|ls| ls.contains(&loan))) {
                out.push(cx.names.locals[local].clone());
            }
        }
        out
    };
    // Who held the loan last on the way to `loc`: on each way back over the
    // CFG (borrowck's imaginary edges and cleanup excluded), the nearest
    // location where some variable's origin contains it; the union over
    // the ways (two arms can leave it in different variables). Empty when
    // the search can't finish (it decides nothing then).
    let preds_all = body.basic_blocks.predecessors();
    let last_holders = |loc: Location, loan: BorrowIndex| -> Vec<String> {
        let Some(t) = table else { return Vec::new() };
        let mut seen: BTreeSet<Location> = BTreeSet::new();
        let mut out: Vec<String> = Vec::new();
        let mut work = vec![loc];
        while let Some(l) = work.pop() {
            if !seen.insert(l) {
                continue;
            }
            if seen.len() > 4096 {
                return Vec::new();
            }
            let mut hs = holders_at(t.mid_index(l), loan);
            if hs.is_empty() {
                hs = holders_at(t.start_index(l), loan);
            }
            if !hs.is_empty() {
                for h in hs {
                    if !out.contains(&h) {
                        out.push(h);
                    }
                }
                continue;
            }
            if l.statement_index > 0 {
                work.push(Location { block: l.block, statement_index: l.statement_index - 1 });
            } else {
                for p in &preds_all[l.block] {
                    let pdata = &body.basic_blocks[*p];
                    let imaginary = matches!(&pdata.terminator().kind,
                        TerminatorKind::FalseEdge { imaginary_target, real_target } if *imaginary_target == l.block && *real_target != l.block);
                    if !pdata.is_cleanup && !imaginary {
                        work.push(Location { block: *p, statement_index: pdata.statements.len() });
                    }
                }
            }
        }
        out.sort();
        out
    };
    // Anyone whose type holds the loan at some point, with the first row.
    let mut holders_any: BTreeMap<BorrowIndex, Vec<(usize, String)>> = BTreeMap::new();
    for (pol, untracked) in [(pol.as_ref(), false), (spol.as_ref(), true)] {
        let (Some(pol), Some(t)) = (pol, table) else { continue };
        #[expect(clippy::disallowed_methods, reason = "sorted below")]
        let mut pts: Vec<(&Point, &BTreeMap<Origin, BTreeSet<BorrowIndex>>)> = pol.origin_contains_loan_at.iter().collect();
        pts.sort_by_key(|(p, _)| **p);
        for (p, m) in pts {
            let row = cx.row_of(t.to_location(*p));
            for (local, os) in &origins_of {
                for o in os {
                    for l in m.get(o).into_iter().flatten() {
                        // (The tracked run's placeholder loans share ids with
                        // the untracked ones.)
                        if (l.index() >= base) != untracked {
                            continue;
                        }
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
    }
    for v in holders_any.values_mut() {
        v.sort();
    }

    // ---- loans ------------------------------------------------------------------
    let mut loans: Vec<LoanInfo> = Vec::new();
    let mut evs: BTreeMap<usize, Vec<Ev>> = BTreeMap::new();
    let push = |evs: &mut BTreeMap<usize, Vec<Ev>>, (row, ev): (usize, Ev)| evs.entry(row).or_default().push(ev);
    // Where each two-phase loan is activated.
    let mut activation: BTreeMap<usize, Location> = BTreeMap::new();
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
        // Who holds it as it is taken (the Borrow event's holder); later
        // holders (`let s = r;`) join `holders` below.
        let at_issue = if holders.is_empty() { direct.clone() } else { holders.join("|") };
        if let TwoPhaseActivation::ActivatedAt(a) = bd.activation_location() {
            activation.insert(idx, a);
        }
        let lender_name = cx.name(bp);
        if let Some(hs) = holders_any.get(&bi) {
            for (_, h) in hs {
                if !holders.contains(h) && Some(h) != lender_name.as_ref() {
                    holders.push(h.clone());
                }
            }
        }
        let mut source = "borrow";
        let lender = match lender_name {
            Some(l) => l,
            None => {
                // A loan of a MIR temporary (`&mut *_14` for `next()`, or a
                // lifetime-extended `&String::new()`): not a column.
                // (`&make_pair().0` borrows a field of the temporary.)
                temp_holders.entry(bp.local).or_default().extend(holders.iter().filter(|h| !h.contains('(')).cloned());
                // Listed when a variable keeps it (`let t = &String::from("x");`),
                // named by the temporary's code.
                let kept = user_place(&direct).is_some() || holders.iter().any(|h| user_place(h).is_some());
                let decl = &body.local_decls[bp.local];
                if !kept || decl.is_user_variable() || bp.local == RETURN_PLACE || bp.projection.first() == Some(&ProjectionElem::Deref) {
                    continue;
                }
                let sm = tcx.sess.source_map();
                let text = sm.span_to_snippet(decl.source_info.span.source_callsite()).unwrap_or_else(|_| "?".into());
                let text: String = text.chars().take(40).collect();
                source = "temporary";
                // `(make_pair()).0`; a downcast wraps the temporary's code
                // itself: `(make_res() as Ok).0`.
                let n = cx.names.name_in_temp(body, bp, text.clone());
                match n.strip_prefix(text.as_str()) {
                    Some(rest) => format!("({}){}", text, rest),
                    None => n,
                }
            }
        };
        let to_call = direct.strip_suffix("()").filter(|_| holders.is_empty()).map(str::to_owned);
        let holder = at_issue;
        let cols: Vec<String> = columns
            .iter()
            .chain(synth.iter())
            .filter(|(_, cp, _)| places_conflict(tcx, body, bp, *cp, PlaceConflictBias::Overlap))
            .map(|(_, _, n)| n.clone())
            .collect();
        if runs.contains(&loc.block) {
            push(&mut evs, cx.ev(*loc, Fact::Borrow { loan: idx, lender: lender.clone(), kind, holder }));
        }
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
            source,
            made_through: Vec::new(),
            direct_holders: user_place(&direct).into_iter().collect(),
            via_call: Vec::new(),
            implicit: !cx.written_borrow(*loc),
        });
    }
    for (i, u) in untracked.iter().enumerate() {
        let id = base + i;
        let bi = BorrowIndex::from_usize(id);
        let (direct, in_macro) = match (u.param, u.at) {
            (Some(p), _) => (cx.names.locals[&p].clone(), None),
            (None, Some(loc)) => {
                let d = cx.dest_name(u.assigned, loc, 0);
                let d = match &u.field {
                    Some(f) if user_place(&d).is_some() => format!("{}.{}", d, f),
                    _ => d,
                };
                (d, cx.macro_at(loc))
            }
            (None, None) => continue,
        };
        let mut holders: Vec<String> = user_place(&direct).into_iter().collect();
        if let (Some(t), Some(loc)) = (table, u.at) {
            for h in holders_at(t.mid_index(loc), bi) {
                if !holders.contains(&h) {
                    holders.push(h);
                }
            }
        }
        let at_issue = if holders.is_empty() { direct.clone() } else { holders.join("|") };
        if let Some(hs) = holders_any.get(&bi) {
            for (_, h) in hs {
                if !holders.contains(h) && *h != u.lender {
                    holders.push(h.clone());
                }
            }
        }
        let to_call = direct.strip_suffix("()").filter(|_| holders.is_empty()).map(str::to_owned);
        if let (Some(loc), Some(_)) = (u.at, u.place) {
            if runs.contains(&loc.block) {
                push(&mut evs, cx.ev(loc, Fact::Borrow { loan: id, lender: u.lender.clone(), kind: u.kind, holder: at_issue }));
            }
        }
        let (line, col) = match (u.param, u.at) {
            (Some(p), _) => (cx.r.line_lo(body.local_decls[p].source_info.span), 0),
            (_, Some(loc)) => (cx.row_of(loc), cx.rows.col.get(&loc).copied().unwrap_or(0)),
            _ => (0, 0),
        };
        loans.push(LoanInfo {
            loan: id,
            line,
            col,
            lender: u.lender.clone(),
            columns: Vec::new(),
            kind: u.kind,
            two_phase: false,
            through: u.place.and_then(|bp| cx.names.through(tcx, body, bp)),
            holders,
            to_call,
            in_macro,
            end_lines: Vec::new(),
            source: u.source,
            made_through: Vec::new(),
            direct_holders: user_place(&direct).into_iter().collect(),
            via_call: Vec::new(),
            implicit: u.place.is_some() && u.at.is_some_and(|loc| !cx.written_borrow(loc)),
        });
    }
    // The variable holding the loan (`h` for `h.r`): the one a column's
    // segment names. `LoanInfo::holders` keeps the field.
    let loan_holder = |l: &LoanInfo| -> String {
        if let Some(h) = l.holders.iter().find(|h| !h.contains('.')).or(l.holders.first()) {
            h.clone()
        } else if let Some(c) = &l.to_call {
            format!("{}()", c)
        } else {
            "<temp>".into()
        }
    };

    // The untracked loans in use at a point: in some origin there (Polonius
    // keeps a loan in an origin only while the origin is live).
    // A reference parameter the body never uses: its caller's loan is given
    // back when the function returns (an end on each `return`, below).
    let unused: Vec<usize> = untracked
        .iter()
        .enumerate()
        .filter(|(_, u)| u.param.is_some_and(|p| pfacts.is_some_and(|f| !f.var_used_at.iter().any(|(v, _)| *v == p))))
        .map(|(i, _)| base + i)
        .collect();
    let untracked_at = |p: Point| -> BTreeSet<BorrowIndex> {
        let mut out: BTreeSet<BorrowIndex> = unused.iter().map(|l| BorrowIndex::from_usize(*l)).collect();
        if let Some(m) = spol.as_ref().and_then(|o| o.origin_contains_loan_at.get(&p)) {
            for ls in m.values() {
                out.extend(ls.iter().copied().filter(|l| l.index() >= base));
            }
        }
        out
    };
    // The CFG edges each loan ends on (for `HoldEnd`s below).
    let mut ended_edges: BTreeSet<(Location, Location, usize)> = BTreeSet::new();
    // Borrow ends: a location where a loan is in scope (before or after
    // its effect: a loan taken here and dead before the next location
    // counts) with a successor where it isn't. One per CFG edge, so each
    // arm gets its own end.
    for (&loc, &row) in &cx.rows.row {
        if !runs.contains(&loc.block) {
            continue;
        }
        let data = &body.basic_blocks[loc.block];
        inscope.seek_before_primary_effect(loc);
        let mut here: BTreeSet<BorrowIndex> = inscope.get().iter().collect();
        inscope.seek_after_primary_effect(loc);
        here.extend(inscope.get().iter());
        if let Some(t) = table {
            here.extend(untracked_at(t.start_index(loc)));
            here.extend(untracked_at(t.mid_index(loc)));
        }
        if here.is_empty() {
            continue;
        }
        // (Not borrowck's imaginary edge of a `FalseEdge`: no path takes it.)
        let real_only = |s: &rustc_middle::mir::BasicBlock| match &data.terminator().kind {
            TerminatorKind::FalseEdge { real_target, .. } => s == real_target,
            _ => true,
        };
        let succs: Vec<Location> = if loc.statement_index < data.statements.len() {
            vec![loc.successor_within_block()]
        } else {
            data.terminator()
                .successors()
                .filter(|s| !body.basic_blocks[*s].is_cleanup && !is_unreachable(body, *s) && real_only(s))
                .map(|s| Location { block: s, statement_index: 0 })
                .collect()
        };
        for s in &succs {
            if !runs.contains(&s.block) {
                continue;
            }
            inscope.seek_before_primary_effect(*s);
            let mut there: BTreeSet<BorrowIndex> = inscope.get().iter().collect();
            if let Some(t) = table {
                there.extend(untracked_at(t.start_index(*s)));
            }
            for b in &here {
                if there.contains(b) {
                    continue;
                }
                let Some(li) = loans.iter_mut().find(|l| l.loan == b.index()) else { continue };
                // The next row of this path (past a block start that carries
                // the branch's span, past temporaries' drops).
                let to_line = cx.r.next_row(&cx.rows, *s, row);
                let to_col = cx.rows.col.get(s).copied().unwrap_or(0);
                // Who holds it as it ends (Polonius, at this location), not
                // whoever held it first.
                let mut now: Vec<String> = Vec::new();
                if let Some(t) = table {
                    for p in [t.start_index(loc), t.mid_index(loc)] {
                        for h in holders_at(p, *b) {
                            if !now.contains(&h) {
                                now.push(h);
                            }
                        }
                    }
                }
                // Of several holders, the one this row uses last (`s` of
                // `n + s.len()` after `let s = r;`).
                let used: Vec<String> = used_on_row(body, &cx.rows, loc)
                    .into_iter()
                    .filter_map(|l| cx.names.locals.get(&l).cloned())
                    .filter(|n| now.contains(n) || li.holders.contains(n))
                    .collect();
                let first = loan_holder(li);
                // Nobody's origin holds it here (its last holder's last use
                // is behind): who held it last on the way here, not whoever
                // held it first (`vr` after `vr.push(r)`, not the dead `r`).
                let last = if now.is_empty() { last_holders(loc, *b) } else { Vec::new() };
                let holder = if let Some(u) = used.first() {
                    u.clone()
                } else if !last.is_empty() {
                    last.join("|")
                } else if now.is_empty() || now.contains(&first) {
                    first
                } else {
                    now[0].clone()
                };
                // Why, only when it is about a holder of the loan here: the
                // holder is overwritten, goes out of scope or is dropped;
                // else nothing uses the loan any more on this path.
                let holds = |n: &str| {
                    let root = n.split('.').next().unwrap_or(n);
                    now.iter().any(|h| h == root)
                        || (now.is_empty() && !last.is_empty() && last.iter().any(|h| h == root))
                        || (now.is_empty() && last.is_empty() && li.holders.first().is_some_and(|h| h == n))
                };
                let mut why = "no further use".to_string();
                let sdata = &body.basic_blocks[s.block];
                match sdata.statements.get(s.statement_index).map(|st| &st.kind) {
                    Some(StatementKind::Assign(asg)) if cx.name(asg.0).is_some_and(|n| holds(&n)) => {
                        why = "holder overwritten".into()
                    }
                    Some(StatementKind::StorageDead(l)) if cx.names.locals.get(l).is_some_and(|n| holds(n)) => {
                        why = format!("{} out of scope", cx.names.locals[l])
                    }
                    None => {
                        if let TerminatorKind::Drop { place, .. } = &sdata.terminator().kind {
                            if let Some(n) = cx.name(*place).filter(|n| holds(n)) {
                                why = format!("{} dropped", n);
                            }
                        }
                    }
                    _ => {}
                }
                if loc.statement_index == data.statements.len() {
                    if let TerminatorKind::Drop { place, .. } = &data.terminator().kind {
                        if let Some(n) = cx.name(*place).filter(|n| holds(n)) {
                            why = format!("{} dropped", n);
                        }
                    }
                }
                if !li.end_lines.contains(&row) {
                    li.end_lines.push(row);
                }
                ended_edges.insert((loc, *s, b.index()));
                let f = Fact::BorrowEnd { loan: b.index(), lender: li.lender.clone(), holder, why, to_line, to_col };
                push(&mut evs, cx.ev(loc, f));
            }
        }
    }
    for &id in &unused {
        let Some(li) = loans.iter_mut().find(|l| l.loan == id) else { continue };
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            if !runs.contains(&bb) || !matches!(data.terminator().kind, TerminatorKind::Return) {
                continue;
            }
            let loc = Location { block: bb, statement_index: data.statements.len() };
            let holder = li.holders.first().cloned().unwrap_or_default();
            let f = Fact::BorrowEnd { loan: id, lender: li.lender.clone(), holder, why: "unused, the function returns".into(), to_line: 0, to_col: 0 };
            // One end per way out: early `return`s that share this
            // `Return` each get theirs on their own row.
            for (r, at) in cx.rows.shared_exits.get(&loc).cloned().unwrap_or_default() {
                let mut ev = cx.ev(loc, f.clone());
                ev.0 = r;
                ev.1.col = cx.rows.col.get(&at).copied().unwrap_or(0);
                ev.1.arm = cx.rows.arm_of(at);
                ev.1.site = Some(cx.rows.order(at));
                if !li.end_lines.contains(&r) {
                    li.end_lines.push(r);
                }
                push(&mut evs, ev);
            }
            if cx.rows.exit_only.contains(&loc) {
                continue;
            }
            let (mut row, ev) = cx.ev(loc, f);
            if row == 0 {
                row = cx.r.line_hi(body.span);
            }
            if !li.end_lines.contains(&row) {
                li.end_lines.push(row);
            }
            push(&mut evs, (row, ev));
        }
    }
    for l in loans.iter_mut() {
        l.end_lines.sort();
    }

    // ---- reborrow chains and direct holders -------------------------------------
    // The loans a local's value holds at a point (tracked and untracked).
    let held_in = |os: &[Origin], p: Point| -> BTreeSet<usize> {
        let mut out = BTreeSet::new();
        for (o, untracked) in [(pol.as_ref(), false), (spol.as_ref(), true)] {
            let Some(m) = o.and_then(|o| o.origin_contains_loan_at.get(&p)) else { continue };
            for og in os {
                out.extend(m.get(og).into_iter().flatten().map(|l| l.index()).filter(|l| (*l >= base) == untracked));
            }
        }
        out
    };
    let held_by = |local: Local, p: Point| -> BTreeSet<usize> {
        held_in(all_origins.get(&local).map(Vec::as_slice).unwrap_or(&[]), p)
    };
    // What the value at a place holds (`k.1`'s reference, not `k.0`'s):
    // the origins of the place's own type.
    let held_by_place = |place: Place<'tcx>, p: Point| -> BTreeSet<usize> {
        let os = regions_of(decl_place_ty(tcx, body, place));
        if place.projection.is_empty() || os.is_empty() {
            return held_by(place.local, p);
        }
        held_in(&os, p)
    };
    let published: BTreeSet<usize> = loans.iter().map(|l| l.loan).collect();
    if let Some(t) = table {
        let lmap = bwf.borrow_set.location_map();
        for l in loans.iter_mut() {
            if l.through.is_none() {
                continue;
            }
            let at = if l.loan < base {
                lmap.get_index(l.loan).map(|(loc, bd)| (*loc, bd.borrowed_place()))
            } else {
                untracked.get(l.loan - base).and_then(|u| Some((u.at?, u.place?)))
            };
            let Some((loc, bp)) = at else { continue };
            // The reference it is reached through (`k.1` of `*k.1`): the loans
            // that reference holds, not its local's other fields'. (Only
            // loans listed here: a MIR temporary's isn't.)
            let last_deref = bp.projection.iter().rposition(|e| e == ProjectionElem::Deref).unwrap_or(0);
            let refp = Place { local: bp.local, projection: tcx.mk_place_elems(&bp.projection[..last_deref]) };
            let mut v: Vec<usize> = held_by_place(refp, t.start_index(loc)).into_iter().filter(|x| *x != l.loan && published.contains(x)).collect();
            v.sort();
            l.made_through = v;
        }
        // Copies and moves of a direct holder's reference (through MIR
        // temporaries), into a variable, a field (`h.r`, `o.0`), or a whole
        // struct that carries it (`s2` of `s1` gives `s2.r` of `s1.r`). Only
        // while the source holds the loan there (Polonius).
        let mut changed = true;
        let mut rounds = 0;
        while changed && rounds < 64 {
            changed = false;
            rounds += 1;
            for &bb in body.basic_blocks.reverse_postorder() {
                if !runs.contains(&bb) {
                    continue;
                }
                for (i, st) in body.basic_blocks[bb].statements.iter().enumerate() {
                    let loc = Location { block: bb, statement_index: i };
                    let StatementKind::Assign(asg) = &st.kind else { continue };
                    let (lhs, rv) = &**asg;
                    let srcs: Vec<(Place<'tcx>, Option<String>)> = match rv {
                        Rvalue::Use(Operand::Copy(p) | Operand::Move(p)) | Rvalue::Cast(_, Operand::Copy(p) | Operand::Move(p), _) => vec![(*p, None)],
                        Rvalue::Aggregate(k, ops) => ops
                            .iter_enumerated()
                            .filter_map(|(f, o)| match o {
                                Operand::Copy(p) | Operand::Move(p) => Some((*p, aggregate_field(tcx, k, f))),
                                _ => None,
                            })
                            .filter(|(_, f)| f.is_some())
                            .collect(),
                        _ => Vec::new(),
                    };
                    for (src, field) in srcs {
                        let Some(sname) = cx.name(src).map(|n| without_downcasts(&n)) else { continue };
                        let held = held_by(src.local, t.start_index(loc));
                        if held.is_empty() {
                            continue;
                        }
                        let d = cx.dest_name(*lhs, loc, 0);
                        let Some(d) = user_place(&d) else { continue };
                        let d = match &field {
                            Some(f) => format!("{}.{}", d, f),
                            None => d,
                        };
                        // (Only what the destination holds after: not a
                        // `usize` field copied out of a tuple with a reference.)
                        let now = held_by(lhs.local, t.mid_index(loc));
                        for l in loans.iter_mut() {
                            if !held.contains(&l.loan) || !now.contains(&l.loan) {
                                continue;
                            }
                            let mut add = Vec::new();
                            for h in &l.direct_holders {
                                if *h == sname {
                                    add.push(d.clone());
                                } else if let Some(rest) = h.strip_prefix(&format!("{}.", sname)) {
                                    add.push(format!("{}.{}", d, rest));
                                }
                            }
                            for a in add {
                                if !l.direct_holders.contains(&a) && a != l.lender {
                                    l.direct_holders.push(a);
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
        }
        // Holders by a call's result. Per loan, the locals that carry it as
        // their value (`copy`: the reference itself, through copies, moves
        // and field reads; `call`: out of a call's result), and those that
        // are a reference to a carrier (`&iter`, which a call like `next`
        // may read it out of). A reborrow through a carrier (`&*r`) is a
        // loan of its own (`made_through`), not carried.
        let lmap = bwf.borrow_set.location_map();
        for l in loans.iter_mut() {
            let first: Option<Local> = if l.loan < base {
                lmap.get_index(l.loan).map(|(_, bd)| bd.assigned_place().local)
            } else {
                untracked.get(l.loan - base).map(|u| u.param.unwrap_or(u.assigned.local))
            };
            let Some(first) = first else { continue };
            // 0: copy carrier, 1: call carrier, 2: reference to a carrier.
            let mut how: BTreeMap<Local, u8> = BTreeMap::new();
            // Where a call carrier got it (to name the fields that hold it).
            let mut got_at: BTreeMap<Local, Point> = BTreeMap::new();
            how.insert(first, 0);
            let mut changed = true;
            let mut rounds = 0;
            while changed && rounds < 64 {
                changed = false;
                rounds += 1;
                for &bb in body.basic_blocks.reverse_postorder() {
                    if !runs.contains(&bb) {
                        continue;
                    }
                    let data = &body.basic_blocks[bb];
                    let mut set = |how: &mut BTreeMap<Local, u8>, local: Local, k: u8, at: Point| {
                        // (A copy beats a call result, which beats a reference.)
                        let e = how.entry(local).or_insert(u8::MAX);
                        if k < *e {
                            *e = k;
                            changed = true;
                            got_at.insert(local, at);
                        }
                    };
                    for (i, st) in data.statements.iter().enumerate() {
                        let loc = Location { block: bb, statement_index: i };
                        let StatementKind::Assign(asg) = &st.kind else { continue };
                        let (lhs, rv) = &**asg;
                        let no_deref = |p: &Place<'tcx>| !p.projection.iter().any(|e| e == ProjectionElem::Deref);
                        let through_ref = |p: &Place<'tcx>| {
                            p.projection.iter().enumerate().any(|(i, e)| {
                                e == ProjectionElem::Deref && {
                                    let base = Place { local: p.local, projection: tcx.mk_place_elems(&p.projection[..i]) };
                                    !decl_place_ty(tcx, body, base).is_box()
                                }
                            })
                        };
                        let gets = held_by(lhs.local, t.mid_index(loc)).contains(&l.loan);
                        let carried = |how: &BTreeMap<Local, u8>, p: &Place<'tcx>| -> Option<u8> {
                            let k = *how.get(&p.local)?;
                            (gets && k < 2 && no_deref(p) && held_by(p.local, t.start_index(loc)).contains(&l.loan)).then_some(k)
                        };
                        match rv {
                            // (A store through a reference, `*z = &c`, puts it in
                            // what z points to; z's own value stays the same. A
                            // Box's contents are its own: `vec![&row]`.)
                            Rvalue::Use(Operand::Copy(p) | Operand::Move(p)) | Rvalue::Cast(_, Operand::Copy(p) | Operand::Move(p), _) if !through_ref(lhs) => {
                                if let Some(k) = carried(&how, p) {
                                    set(&mut how, lhs.local, k, t.mid_index(loc));
                                }
                            }
                            Rvalue::Aggregate(_, ops) if !through_ref(lhs) => {
                                for o in ops {
                                    if let Operand::Copy(p) | Operand::Move(p) = o {
                                        if let Some(k) = carried(&how, p) {
                                            set(&mut how, lhs.local, k, t.mid_index(loc));
                                        }
                                    }
                                }
                            }
                            Rvalue::Ref(_, _, p) if lhs.projection.is_empty() => {
                                if carried(&how, p).is_some() {
                                    set(&mut how, lhs.local, 2, t.mid_index(loc));
                                } else if p.projection.first() == Some(&ProjectionElem::Deref)
                                    && !cx.names.locals.contains_key(&p.local)
                                    && gets
                                    && held_by(p.local, t.start_index(loc)).contains(&l.loan)
                                {
                                    // A MIR temporary reborrowed to pass it on
                                    // (`holder(&*m)`'s argument) or a part of
                                    // what it points to (`[x, ..] =
                                    // v.as_slice()`): no loan of its own.
                                    if let Some(k) = how.get(&p.local).copied() {
                                        set(&mut how, lhs.local, k, t.mid_index(loc));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    if let TerminatorKind::Call { args, destination, target: Some(tb), .. } = &data.terminator().kind {
                        let loc = Location { block: bb, statement_index: data.statements.len() };
                        let given = args.iter().any(|a| match &a.node {
                            Operand::Copy(p) | Operand::Move(p) => {
                                how.contains_key(&p.local) && held_by(p.local, t.start_index(loc)).contains(&l.loan)
                            }
                            _ => false,
                        });
                        let after = Location { block: *tb, statement_index: 0 };
                        if given && held_by(destination.local, t.start_index(after)).contains(&l.loan) {
                            set(&mut how, destination.local, 1, t.start_index(after));
                        }
                        // Stored into a `&mut` argument's referent (`v.extend([&p.0])`,
                        // `v.push(r)`): the variable it points to holds it now.
                        if given {
                            for a in args.iter() {
                                let (Operand::Copy(p) | Operand::Move(p)) = &a.node else { continue };
                                if !p.projection.is_empty() || !matches!(body.local_decls[p.local].ty.kind(), TyKind::Ref(_, _, m) if m.is_mut()) {
                                    continue;
                                }
                                let target = resolve_temp_deref(tcx, body, tcx.mk_place_deref(*p));
                                // (`v.push(r)`: v; through a `&mut` variable
                                // `vr.push(r)`: vr, whose referent keeps it.)
                                let whole = target.projection.is_empty() || (target.projection.len() == 1 && target.projection[0] == ProjectionElem::Deref);
                                if target.local == p.local || !whole {
                                    continue;
                                }
                                let u = target.local;
                                if !held_by(u, t.start_index(loc)).contains(&l.loan) && held_by(u, t.start_index(after)).contains(&l.loan) {
                                    set(&mut how, u, 1, t.start_index(after));
                                }
                            }
                        }
                    }
                }
            }
            // A temporary that keeps a call's result past its row (a `for`
            // loop's hidden iterator over `s.chars()`): "<temp>".
            let anon = how.iter().any(|(local, k)| {
                *k == 1 && !cx.names.locals.contains_key(local) && *local != RETURN_PLACE && {
                    let from = got_at.get(local).map(|p| cx.row_of(t.to_location(*p)));
                    cx.rows.row.iter().any(|(loc, row)| Some(*row) != from && held_by(*local, t.start_index(*loc)).contains(&l.loan))
                }
            });
            // (Also when a variable holds the loan too: the `for` loop's
            // iterator over a parameter `xs` keeps it while `xs` does.)
            if anon && !l.via_call.iter().any(|v| v == "<temp>") {
                l.via_call.push("<temp>".into());
            }
            for (local, k) in &how {
                let Some(n) = cx.names.locals.get(local) else { continue };
                let root_direct = l.direct_holders.iter().any(|d| d == n || d.starts_with(&format!("{}.", n)));
                if root_direct || *n == l.lender {
                    continue;
                }
                // A copy through MIR temporaries the names above don't follow
                // (`if let Some(m) = Some(r)`).
                if *k == 0 && local != &first {
                    l.direct_holders.push(n.clone());
                    continue;
                }
                if *k != 1 {
                    continue;
                }
                // The fields whose types carry it (`t.0` and `t.1` of `pair(&mut
                // a, &mut b)`), else the variable.
                let mut names = Vec::new();
                if let Some(at) = got_at.get(local) {
                    let with: BTreeSet<Origin> = [pol.as_ref(), spol.as_ref()]
                        .into_iter()
                        .flatten()
                        .filter_map(|o| o.origin_contains_loan_at.get(at))
                        .flat_map(|m| m.iter().filter(|(_, ls)| ls.contains(&BorrowIndex::from_usize(l.loan))).map(|(o, _)| *o))
                        .collect();
                    field_paths_with(tcx, body.local_decls[*local].ty, n, &with, 0, &mut names);
                }
                if names.is_empty() {
                    names.push(n.clone());
                }
                for n in names {
                    if !l.via_call.contains(&n) {
                        l.via_call.push(n);
                    }
                }
            }
        }
    }

    // ---- per-holder ends ---------------------------------------------------------
    // Where a direct or via-call holder stops holding a loan that goes on.
    if let Some(t) = table {
        let local_of: BTreeMap<&str, Local> = cx.names.locals.iter().map(|(l, n)| (n.as_str(), *l)).collect();
        // The place a holder's name is (`h.0`, `h.inner.r`): its own field,
        // so `h.0` and `h.1` each have their own end.
        let place_of = |name: &str| -> Option<Place<'tcx>> {
            let mut segs = name.split('.');
            let mut place = Place::from(*local_of.get(segs.next()?)?);
            for seg in segs {
                let ty = decl_place_ty(tcx, body, place);
                let (i, fty) = match ty.kind() {
                    TyKind::Tuple(tys) => {
                        let i: usize = seg.parse().ok()?;
                        (i, *tys.get(i)?)
                    }
                    TyKind::Adt(def, args) if def.is_struct() => {
                        let (i, fd) = def.non_enum_variant().fields.iter().enumerate().find(|(_, fd)| fd.name.as_str() == seg)?;
                        (i, fd.ty(tcx, args))
                    }
                    _ => return Some(place),
                };
                place = tcx.mk_place_field(place, rustc_abi::FieldIdx::from_usize(i), fty);
            }
            Some(place)
        };
        let succs_of = |loc: Location| -> Vec<Location> {
            let data = &body.basic_blocks[loc.block];
            if loc.statement_index < data.statements.len() {
                return vec![loc.successor_within_block()];
            }
            data.terminator()
                .successors()
                .filter(|s| !body.basic_blocks[*s].is_cleanup && !is_unreachable(body, *s) && runs.contains(s))
                .filter(|s| match &data.terminator().kind {
                    TerminatorKind::FalseEdge { real_target, .. } => s == real_target,
                    _ => true,
                })
                .map(|s| Location { block: s, statement_index: 0 })
                .collect()
        };
        // Every way on from `s` ends the loan before it leaves `row` (a
        // temporary carried it to the row's end; the `BorrowEnd` says it).
        // A way that leaves the row with the loan going on (the loop-back
        // edge beside a `break` on one row) needs the holder's own end.
        let ends_on_row = |s: Location, row: usize, loan: usize| -> bool {
            let mut seen: BTreeSet<Location> = BTreeSet::new();
            let mut work = vec![s];
            while let Some(x) = work.pop() {
                if !seen.insert(x) {
                    continue;
                }
                if seen.len() > 1024 || cx.rows.row.get(&x).is_some_and(|r| *r != row) {
                    return false;
                }
                let next = succs_of(x);
                if next.is_empty() {
                    return false;
                }
                for y in next {
                    if !ended_edges.contains(&(x, y, loan)) {
                        work.push(y);
                    }
                }
            }
            true
        };
        for l in &loans {
            if unused.contains(&l.loan) {
                continue;
            }
            let bi = BorrowIndex::from_usize(l.loan);
            let mut seen: BTreeSet<&String> = BTreeSet::new();
            for h in l.direct_holders.iter().chain(l.via_call.iter()) {
                if !seen.insert(h) {
                    continue;
                }
                let Some(hp) = place_of(h) else { continue };
                for (&loc, &row) in &cx.rows.row {
                    if !runs.contains(&loc.block) {
                        continue;
                    }
                    let held_here = held_by_place(hp, t.start_index(loc)).contains(&l.loan) || held_by_place(hp, t.mid_index(loc)).contains(&l.loan);
                    if !held_here {
                        continue;
                    }
                    let succs = succs_of(loc);
                    for s in succs {
                        if held_by_place(hp, t.start_index(s)).contains(&l.loan) {
                            continue;
                        }
                        // (The loan itself ends here: its `BorrowEnd` says so.)
                        let goes_on = if l.loan >= base {
                            untracked_at(t.start_index(s)).contains(&bi)
                        } else {
                            inscope.seek_before_primary_effect(s);
                            inscope.get().contains(bi)
                        };
                        if !goes_on {
                            continue;
                        }
                        let to_line = cx.r.next_row(&cx.rows, s, row);
                        let to_col = cx.rows.col.get(&s).copied().unwrap_or(0);
                        let mut f = cx.ev(loc, Fact::HoldEnd { loan: l.loan, holder: h.clone(), to_line, to_col });
                        f.1.order.2 = 1;
                        if !ends_on_row(s, row, l.loan) {
                            push(&mut evs, f);
                        }
                    }
                }
            }
        }
    }

    // ---- stores of references into struct fields (`h.r = &b`) ------------------
    // Per field: each place a store puts loans there, and which loans. The
    // struct literal stores the loans first held by the field; an
    // assignment `h.r = <rhs>` the loans its right-hand side takes, or those
    // a reference variable it copies holds (`h.r = s`).
    let mut field_stores: BTreeMap<String, Vec<(Location, Vec<usize>)>> = BTreeMap::new();
    // Where each listed loan is taken (an untracked one's, from its record).
    let taken_at = |loan: usize| -> Option<Location> {
        if loan < base {
            bwf.borrow_set.location_map().get_index(loan).map(|(bl, _)| *bl)
        } else {
            untracked.get(loan - base).and_then(|u| u.at)
        }
    };
    for l in &loans {
        if let Some(f) = l.holders.first().filter(|h| h.contains('.')) {
            if let Some(bl) = taken_at(l.loan) {
                field_stores.entry(f.clone()).or_default().push((bl, vec![l.loan]));
            }
        }
    }
    // A literal stores what its fields' operands hold (`(a, a)` puts a's
    // loan in `h.0` and `h.1`; `R { r: s }` s's).
    if let Some(t) = table {
        for (&loc, _) in &cx.rows.row {
            let Some(st) = body.basic_blocks[loc.block].statements.get(loc.statement_index) else { continue };
            let StatementKind::Assign(asg) = &st.kind else { continue };
            let Rvalue::Aggregate(k, ops) = &asg.1 else { continue };
            let Some(d) = user_place(&cx.dest_name(asg.0, loc, 0)) else { continue };
            for (f, o) in ops.iter_enumerated() {
                let (Operand::Copy(p) | Operand::Move(p)) = o else { continue };
                let Some(field) = aggregate_field(tcx, k, f) else { continue };
                let ls: Vec<usize> = held_by_place(*p, t.start_index(loc)).into_iter().filter(|l| published.contains(l)).collect();
                if !ls.is_empty() {
                    field_stores.entry(format!("{}.{}", d, field)).or_default().push((loc, ls));
                }
            }
        }
    }
    for (&loc, _) in &cx.rows.row {
        let Some(st) = body.basic_blocks[loc.block].statements.get(loc.statement_index) else { continue };
        let StatementKind::Assign(asg) = &st.kind else { continue };
        let lhs = &asg.0;
        if lhs.projection.is_empty() || !lhs.ty(&body.local_decls, tcx).ty.is_ref() {
            continue;
        }
        let Some(path) = cx.name(*lhs).filter(|_| cx.names.root(*lhs).is_some_and(|(_, skip, _)| column_projection(body, *lhs, skip))) else { continue };
        let span = {
            let hid = rustc_utils::BodyExt::location_to_hir_id(body, loc);
            match tcx.hir_node(hid) {
                rustc_hir::Node::Expr(e) => e.span,
                _ => cx.r.span_of(loc),
            }
        };
        let mut new: Vec<usize> = loans
            .iter()
            .filter(|l| l.holders.first() == Some(&path) && taken_at(l.loan).is_some_and(|bl| span.contains(cx.r.span_of(bl))))
            .map(|l| l.loan)
            .collect();
        // And the loans the field holds right after the store that it didn't
        // hold just before (`h.r = s` carries s's loan in; `h.r = "y"`).
        if let Some(t) = table {
            let (before, after) = (held_by_place(*lhs, t.start_index(loc)), held_by_place(*lhs, t.mid_index(loc)));
            for b in after.difference(&before) {
                if published.contains(b) && !new.contains(b) {
                    new.push(*b);
                }
            }
        }
        new.sort();
        // (Replaces the literal's entries for loans taken in this very
        // statement: those are this store's.)
        let v = field_stores.entry(path).or_default();
        v.retain(|(_, ls)| !(ls.len() == 1 && new.contains(&ls[0]) && span.contains(cx.r.span_of(taken_at(ls[0]).unwrap_or(loc)))));
        v.push((loc, new));
    }
    // The stores whose loans can still be in the field at `loc`: those on
    // a way to it not overwritten by a later store on the same way
    // (dominance: a store dominated by another reaching one is the later).
    let doms = body.basic_blocks.dominators();
    let reaches = |a: Location, b: Location| -> bool {
        if a.block == b.block && a.statement_index < b.statement_index {
            return true;
        }
        let mut seen = BTreeSet::new();
        let mut work: Vec<_> = body.basic_blocks[a.block].terminator().successors().collect();
        while let Some(bb) = work.pop() {
            if bb == b.block {
                return true;
            }
            if seen.insert(bb) {
                work.extend(body.basic_blocks[bb].terminator().successors());
            }
        }
        false
    };
    let ldom = |a: Location, b: Location| if a.block == b.block { a.statement_index <= b.statement_index } else { doms.dominates(a.block, b.block) };
    let reaching_stores = |stores: &[(Location, Vec<usize>)], loc: Location| -> Vec<Vec<usize>> {
        let cands: Vec<&(Location, Vec<usize>)> = stores.iter().filter(|(l, _)| *l != loc && reaches(*l, loc)).collect();
        cands
            .iter()
            .filter(|(l, _)| !cands.iter().any(|(m, _)| m != l && ldom(*l, *m) && ldom(*m, loc)))
            .map(|(_, ls)| ls.clone())
            .collect()
    };

    // The parts of a place that drop elaboration drops on their own when some
    // of it was moved out (on some path), as (field key, depth, name, how):
    // each field in declaration order, those that may hold a value behind a
    // flag (`b.a` re-assigned in one arm); a Box inside (`*b`, or `*p.b`)
    // also frees its own allocation, after its contents (deeper parts sort
    // first). A field with no move path of its own holds a value exactly
    // where its parent (as a whole) does.
    let field_drops = |rt: &mut Flow<'_, '_, 'tcx>, at: &[Location], after: bool, top: MovePathIndex| -> Vec<(Vec<usize>, std::cmp::Reverse<usize>, String, &'static str)> {
        let key = |p: &Place<'tcx>| -> Vec<usize> {
            p.projection.iter().filter_map(|e| match e {
                ProjectionElem::Field(f, _) => Some(f.index()),
                _ => None,
            }).collect()
        };
        let mut out = Vec::new();
        let mut work = vec![top];
        while let Some(m) = work.pop() {
            let st = rt.state(at, after, m);
            if st.sub_moved.is_empty() && st.sub_maybe.is_empty() {
                continue;
            }
            let mut c = rt.md.move_paths[m].first_child;
            while let Some(ch) = c {
                let cp = rt.md.move_paths[ch].place;
                c = rt.md.move_paths[ch].next_sibling;
                if !ty_info(cp.ty(&body.local_decls, tcx).ty).1 {
                    continue;
                }
                let cst = rt.state(at, after, ch);
                let inner = !cst.sub_moved.is_empty() || !cst.sub_maybe.is_empty();
                // (A part some of whose own parts were moved: those, inside;
                // a Box among them still frees its allocation.)
                if inner {
                    work.push(ch);
                }
                if !inner || cp.ty(&body.local_decls, tcx).ty.is_box() {
                    if let Some(n) = cx.name(cp) {
                        // (After its own contents.)
                        let mut k = key(&cp);
                        if inner {
                            k.push(usize::MAX);
                        }
                        out.push((k, std::cmp::Reverse(cp.projection.len()), n, drop_how(&cst)));
                    }
                }
            }
            let parent_how = match (st.may_init, st.may_uninit) {
                (true, false) => Some("drops"),
                (true, true) => Some("drops_if_init"),
                _ => None,
            };
            // (Its own fields: not a field's fields, nor fields behind a
            // reference, nor another scope's variable of the same name.)
            let mplace = rt.md.move_paths[m].place;
            for (_, fp, n) in &synth {
                let direct = fp.local == mplace.local
                    && fp.projection.len() == mplace.projection.len() + 1
                    && fp.projection[..mplace.projection.len()] == mplace.projection[..];
                if direct && ty_info(fp.ty(&body.local_decls, tcx).ty).1 {
                    if let Some(h) = parent_how {
                        out.push((key(fp), std::cmp::Reverse(fp.projection.len()), n.clone(), h));
                    }
                }
            }
        }
        out.sort();
        out
    };
    // The hidden iterator of a `for` loop (`let mut iter =
    // IntoIterator::into_iter(<head>)` in the desugaring).
    let for_iter = |l: Local| {
        let decl = &body.local_decls[l];
        decl.is_user_variable() && decl.source_info.span.desugaring_kind() == Some(rustc_span::DesugaringKind::ForLoop)
    };
    // ---- moves, copies, inits, re-points, drops, scope ends ---------------------
    let mut temps: BTreeMap<Local, TempInfo> = BTreeMap::new();
    for (&loc, _) in &cx.rows.row {
        if !runs.contains(&loc.block) {
            continue;
        }
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
                            // A part of a temporary moved into a variable (`let
                            // Some(x) = make_o().opt else ..`): named by the
                            // temporary's source text.
                            Operand::Move(p)
                                if !p.projection.is_empty()
                                    && p.local != RETURN_PLACE
                                    && p.local.index() > body.arg_count
                                    && !body.local_decls[p.local].is_user_variable()
                                    && !body.local_decls[p.local].source_info.span.from_expansion()
                                    // (A `Copy` value is copied, not moved: `n = (_t.0)` of
                                    // a checked `+`'s `(usize, bool)` is arithmetic.)
                                    && !ty_info(p.ty(&body.local_decls, tcx).ty).0
                                    && cx.is_user(*lhs) =>
                            {
                                let sp = body.local_decls[p.local].source_info.span;
                                if let Ok(text) = tcx.sess.source_map().span_to_snippet(sp) {
                                    let path = cx.names.name_in_temp(body, *p, text.trim().to_string());
                                    let to = cx.dest_name(*lhs, loc, 0);
                                    push(&mut evs, cx.ev(loc, Fact::Move { path, to, in_macro: in_macro.clone() }));
                                }
                            }
                            Operand::Copy(p) if cx.is_user(*p) => {
                                // An operand of arithmetic or a comparison is
                                // copied to compute it, not into the result's
                                // place.
                                let to = if matches!(rv, Rvalue::BinaryOp(..) | Rvalue::UnaryOp(..)) {
                                    "<operand>".to_string()
                                } else {
                                    cx.dest_name(*lhs, loc, 0)
                                };
                                let path = cx.name(*p).unwrap_or_default();
                                push(&mut evs, cx.ev(loc, Fact::Copy { path, to, in_macro: in_macro.clone() }));
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
                            if let Some(t) = table {
                                // (Untracked loans too: `r = "y"`, `r6 = r5`.)
                                let held = |p: Point| -> Vec<usize> {
                                    held_by(lhs.local, p).into_iter().filter(|l| published.contains(l)).collect()
                                };
                                let f = Fact::Repoint { holder: path.clone(), old: held(t.start_index(loc)), new: held(t.mid_index(loc)) };
                                push(&mut evs, cx.ev(loc, f));
                            }
                        }
                        // A reference stored in a field (`h.r = &mut y;`): the
                        // old loans are those the last store into `h.r` on the
                        // way here put there (and still in scope), the new ones
                        // those this store puts there.
                        if let Some(this) = field_stores.get(&path).and_then(|v| v.iter().find(|(l, _)| *l == loc)) {
                            inscope.seek_before_primary_effect(loc);
                            let mut live: BTreeSet<usize> = inscope.get().iter().map(|b| b.index()).collect();
                            if let Some(t) = table {
                                live.extend(untracked_at(t.start_index(loc)).iter().map(|b| b.index()));
                            }
                            let mut old: Vec<usize> = reaching_stores(&field_stores[&path], loc)
                                .into_iter()
                                .flatten()
                                .filter(|l| live.contains(l) && !this.1.contains(l))
                                .collect();
                            old.sort();
                            old.dedup();
                            push(&mut evs, cx.ev(loc, Fact::Repoint { holder: path.clone(), old, new: this.1.clone() }));
                        }
                        push(&mut evs, cx.ev(loc, Fact::Init { path, from, reassign, maybe_had }));
                    } else if cx.is_user(*lhs) {
                        // A write through a reference or into an element
                        // (`*r = x`, `v[i] = x`): the place gets a new value
                        // (its old one was dropped just before); the
                        // reference still points to the same place.
                        let from = match rv {
                            Rvalue::Use(Operand::Move(p) | Operand::Copy(p)) => cx.name(*p).unwrap_or_else(|| "<value>".into()),
                            Rvalue::Use(Operand::Constant(_)) => "<const>".into(),
                            _ => "<value>".into(),
                        };
                        let path = cx.name(*lhs).unwrap_or_default();
                        push(&mut evs, cx.ev(loc, Fact::Init { path, from, reassign: true, maybe_had: false }));
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
            TerminatorKind::Call { args, destination, fn_span, func, .. } => {
                let f = cx.call_name(*fn_span, func, loc);
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
                let early_exit = cx.rows.early_exit.get(&loc).map(|(r, _)| *r);
                let state = match rt_md.rev_lookup.find(place.as_ref()) {
                    LookupResult::Exact(mpi) => Some(rt.state(&[loc], false, mpi).word()),
                    LookupResult::Parent(_) => None,
                };
                if cx.is_user(*place) {
                    let path = cx.name(*place).unwrap_or_default();
                    let how = match rt_md.rev_lookup.find(place.as_ref()) {
                        LookupResult::Exact(mpi) => {
                            let st = rt.state(&[loc], false, mpi);
                            let how = drop_how(&st);
                            // What is left needs no drop (every field that does was
                            // moved out on every path; `(bool, String, String)`
                            // with both Strings moved): nothing is dropped.
                            let left_needs_drop = || {
                                if !st.sub_maybe.is_empty() || st.payload_moved || st.payload_maybe {
                                    return true;
                                }
                                let mut moved: BTreeSet<usize> = BTreeSet::new();
                                for m in &st.sub_moved {
                                    let mp = &rt_md.move_paths[*m];
                                    match (mp.parent, mp.place.projection.last()) {
                                        (Some(p), Some(ProjectionElem::Field(f, _))) if p == mpi => {
                                            moved.insert(f.index());
                                        }
                                        _ => return true,
                                    }
                                }
                                let pty = place.ty(&body.local_decls, tcx).ty;
                                let fields: Vec<rustc_middle::ty::Ty<'tcx>> = match pty.kind() {
                                    TyKind::Tuple(tys) => tys.iter().collect(),
                                    TyKind::Adt(def, args) if def.is_struct() && !pty.is_box() => {
                                        def.non_enum_variant().fields.iter().map(|fd| fd.ty(tcx, args)).collect()
                                    }
                                    _ => return true,
                                };
                                fields.iter().enumerate().any(|(i, fty)| !moved.contains(&i) && ty_info(*fty).1)
                            };
                            if how == "drops_rest" && !left_needs_drop() { "nothing_to_drop" } else { how }
                        }
                        LookupResult::Parent(_) => "drops",
                    };
                    let shared = cx.rows.shared_exits.get(&loc).cloned().unwrap_or_default();
                    let also_on_exits: Vec<usize> = shared.iter().map(|(r, _)| *r).collect();
                    // A Box's contents are dropped before its allocation is
                    // freed; a struct's summary drop comes before its fields'.
                    let is_box = place.ty(&body.local_decls, tcx).ty.is_box();
                    // The drop also runs on early exits that share it: each
                    // exit gets its own on its row, with what the place (and
                    // each part of it) holds when that path leaves (Q2: on
                    // the exit's row).
                    if let LookupResult::Exact(mpi) = rt_md.rev_lookup.find(place.as_ref()) {
                        for (r, at) in &shared {
                            let how_there = drop_how(&rt.state(&[*at], true, mpi));
                            if how_there == "nothing_to_drop" {
                                continue;
                            }
                            let parts = field_drops(&mut rt, &[*at], true, mpi);
                            let mut items: Vec<(String, &'static str)> = parts.into_iter().map(|(_, _, n, h)| (n, h)).collect();
                            if is_box {
                                items.push((path.clone(), how_there));
                            } else {
                                items.insert(0, (path.clone(), how_there));
                            }
                            for (n, h) in items {
                                let mut ev = cx.ev(loc, Fact::Drop { path: n, how: h, at_reassign: false, early_exit: Some(*r), also_on_exits: Vec::new() });
                                ev.0 = *r;
                                ev.1.col = cx.rows.col.get(at).copied().unwrap_or(0);
                                ev.1.arm = cx.rows.arm_of(*at);
                                // In the order MIR runs it on that path: after
                                // the exit's own drops of inner scopes (`t`
                                // before `s` at `return`), which come before it
                                // in the CFG; one copy per exit site.
                                ev.1.order = cx.rows.order(loc);
                                ev.1.site = Some(cx.rows.order(*at));
                                push(&mut evs, ev);
                            }
                        }
                    }
                    let parts = match rt_md.rev_lookup.find(place.as_ref()) {
                        LookupResult::Exact(top) => field_drops(&mut rt, &[loc], false, top),
                        LookupResult::Parent(_) => Vec::new(),
                    };
                    // Only early exits reach this drop: each has its own above.
                    if !cx.rows.exit_only.contains(&loc) {
                        let whole = (path, how, also_on_exits);
                        let mut items: Vec<(String, &'static str, Vec<usize>)> = parts.into_iter().map(|(_, _, n, h)| (n, h, Vec::new())).collect();
                        if is_box {
                            items.push(whole);
                        } else {
                            items.insert(0, whole);
                        }
                        for (n, h, also) in items {
                            push(&mut evs, cx.ev(loc, Fact::Drop { path: n, how: h, at_reassign, early_exit, also_on_exits: also }));
                        }
                    }
                } else if place.local != RETURN_PLACE
                    && place.local.index() > body.arg_count
                    && place.projection.is_empty()
                    // (A macro's own locals aren't the user's; a `for` loop's
                    // hidden iterator is: it holds what the loop hasn't
                    // taken yet.)
                    && (!body.local_decls[place.local].source_info.span.from_expansion() || for_iter(place.local))
                    && matches!(state, Some("own" | "maybe" | "partly"))
                {
                    let decl = &body.local_decls[place.local];
                    let sp = decl.source_info.span;
                    // (The iterator's span is the loop's head expression, in
                    // the desugaring: not its call site, the whole loop.)
                    let sm = tcx.sess.source_map();
                    let what: String = if for_iter(place.local) {
                        format!("into_iter({})", sm.span_to_snippet(sp).unwrap_or_else(|_| "?".into()).trim())
                    } else {
                        sm.span_to_snippet(sp.source_callsite()).unwrap_or_else(|_| "?".into())
                    };
                    let what: String = what.chars().take(40).collect();
                    let created_line = if for_iter(place.local) { sm.lookup_char_pos(sp.hi()).line } else { cx.r.line_hi(sp) };
                    let drop_row = cx.rows.event_row.get(&loc).copied().unwrap_or(0);
                    let held_by = temp_holders.get(&place.local).cloned().unwrap_or_default();
                    // Extended: a variable holds a loan of it and it outlives the
                    // statement that made it (by source position, not rows).
                    // Extended: it outlives the statement that made it (by
                    // source position, not rows; `let _ = &String::new();`
                    // too, which no variable holds).
                    let extended = cx.stmt_span_of_local(place.local).is_some_and(|st| cx.r.span_of(loc).lo() >= st.hi());
                    let ti = temps.entry(place.local).or_insert_with(|| TempInfo {
                        what: what.clone(),
                        ty: format!("{}", tcx.erase_and_anonymize_regions(decl.ty)),
                        created_line,
                        drop_lines: Vec::new(),
                        extended: false,
                        held_by,
                    });
                    ti.extended |= extended;
                    // Early exits that share the drop run it too (two
                    // `return`s out of a `for` loop drop its iterator): one
                    // event on each exit's row, if it holds a value there.
                    let shared = cx.rows.shared_exits.get(&loc).cloned().unwrap_or_default();
                    if let LookupResult::Exact(mpi) = rt_md.rev_lookup.find(place.as_ref()) {
                        for (r, at) in &shared {
                            if drop_how(&rt.state(&[*at], true, mpi)) == "nothing_to_drop" {
                                continue;
                            }
                            let mut ev = cx.ev(loc, Fact::TempDrop { what: what.clone(), created_line, extended, early_exit: Some(*r) });
                            ev.0 = *r;
                            ev.1.col = cx.rows.col.get(at).copied().unwrap_or(0);
                            ev.1.arm = cx.rows.arm_of(*at);
                            ev.1.site = Some(cx.rows.order(*at));
                            if !ti.drop_lines.contains(r) {
                                ti.drop_lines.push(*r);
                            }
                            push(&mut evs, ev);
                        }
                    }
                    if !cx.rows.exit_only.contains(&loc) {
                        if !ti.drop_lines.contains(&drop_row) {
                            ti.drop_lines.push(drop_row);
                        }
                        push(&mut evs, cx.ev(loc, Fact::TempDrop { what, created_line, extended, early_exit }));
                    }
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
            // What runs, for the words (Q5 refined): moved on every real
            // execution, though borrowck's imaginary edges skip the move (an
            // or-pattern's second alternative after an irrefutable first,
            // `(x, _) | (_, x)`), reads "moved".
            let rt_here = match rt_md.rev_lookup.find(place.as_ref()) {
                LookupResult::Exact(rm) => Some(rt.state(exits, true, rm)),
                LookupResult::Parent(_) => None,
            };
            let w = match s.word() {
                "maybe" if rt_here.as_ref().is_some_and(|r| r.may_uninit && !r.may_init && r.ever_init) => "moved",
                w => w,
            };
            // A variable not assigned yet (and its fields): no column yet.
            let mut root_unassigned = || {
                columns.iter().any(|(m, rp, _)| {
                    rp.local == place.local && rp.projection.len() < place.projection.len() && flow.state(exits, true, *m).word() == "unassigned"
                })
            };
            // A variable is a column from its declaration (`let r;`: storage
            // live, no value yet) to its scope's end.
            let declared = |storage: &mut ResultsCursor<'_, 'tcx, MaybeStorageLive<'_>>| {
                exits.iter().any(|e| {
                    storage.seek_after_primary_effect(*e);
                    storage.get().contains(place.local)
                })
            };
            // Out of its scope (its storage dead): no column, whatever its
            // fields' history says.
            if !declared(&mut storage) {
                continue;
            }
            if w == "unassigned" && !place.projection.is_empty() && root_unassigned() {
                continue;
            }
            st.state.insert(name.clone(), w);
            if w == "maybe" {
                // Moved on some way here, or only never assigned?
                let moved = exits.iter().any(|e| {
                    let mut set = moved_entry.get(&e.block).cloned().unwrap_or_default();
                    for i in 0..=e.statement_index {
                        moved_transfer(&move_data, body, &mut set, Location { block: e.block, statement_index: i });
                    }
                    let (anc, desc) = (ancestors(&move_data, *mpi), descendants(&move_data, *mpi));
                    set.iter().any(|m| anc.contains(m) || desc.contains(m))
                });
                // Never moved at runtime, only on borrowck's imaginary edges
                // (the unreachable alternative): it still can't be used.
                let imaginary = rt_here.as_ref().is_some_and(|r| r.may_init && !r.may_uninit);
                st.cause.insert(name.clone(), if imaginary { "imaginary" } else if moved { "moved" } else { "unassigned" });
            }
            if (w == "partly" || w == "own") && (!s.sub_moved.is_empty() || !s.sub_maybe.is_empty()) {
                // A part borrowck counts as moved on some paths only is moved
                // for sure when only its imaginary edges skip the move (an
                // irrefutable `if let (p, q) = pair && ..`'s `else`): which
                // parts are gone is what runs.
                let mut surely_gone = |m: MovePathIndex| match rt_md.rev_lookup.find(move_data.move_paths[m].place.as_ref()) {
                    LookupResult::Exact(rm) => {
                        let r = rt.state(exits, true, rm);
                        r.may_uninit && !r.may_init
                    }
                    LookupResult::Parent(_) => false,
                };
                let (rt_moved, still_maybe): (Vec<MovePathIndex>, Vec<MovePathIndex>) =
                    s.sub_maybe.iter().copied().partition(|m| surely_gone(*m));
                let mut parts = Parts {
                    moved: s.sub_moved.iter().chain(rt_moved.iter()).filter_map(|m| column_name(*m)).collect(),
                    maybe: still_maybe.iter().filter_map(|m| column_name(*m)).collect(),
                };
                parts.moved.sort();
                parts.maybe.sort();
                st.parts.insert(name.clone(), parts);
            }
        }
        for (parent, fp, name) in &synth {
            let Some(pn) = column_name(*parent) else { continue };
            let Some(pw) = st.state.get(&pn).copied() else { continue };
            // A field reached through a reference (`self.a`, `r.a`) is a
            // place only while the reference holds one: after `let t = r;`
            // the fields are still the referent's, not moved.
            let through_ref = {
                let mut ty = body.local_decls[fp.local].ty;
                let mut any = false;
                for e in fp.projection.iter() {
                    if e == ProjectionElem::Deref && ty.is_ref() {
                        any = true;
                    }
                    ty = match e {
                        ProjectionElem::Field(_, t) => t,
                        ProjectionElem::Deref => ty.builtin_deref(true).unwrap_or(ty),
                        _ => ty,
                    };
                }
                any
            };
            if through_ref && pw != "own" {
                continue;
            }
            st.state.insert(name.clone(), if pw == "partly" { "own" } else { pw });
            if let Some(c) = st.cause.get(&pn).copied() {
                st.cause.insert(name.clone(), c);
            }
        }
        // Loans in scope where the next row starts on the same path: in
        // scope after the exit and still at the successor (a loan whose last
        // use is on this row is out of scope there; another path's loan
        // that joins at the successor is not on this one).
        let mut live: BTreeSet<usize> = BTreeSet::new();
        for e in exits {
            let line = cx.row_of(*e);
            let data = &body.basic_blocks[e.block];
            let succs: Vec<Location> = if e.statement_index < data.statements.len() {
                vec![e.successor_within_block()]
            } else {
                data.terminator()
                    .successors()
                    .filter(|s| !body.basic_blocks[*s].is_cleanup && !is_unreachable(body, *s))
                    .map(|s| Location { block: s, statement_index: 0 })
                    .collect()
            };
            let later: Vec<Location> = succs.iter().copied().filter(|s| cx.row_of(*s) > line).collect();
            let at = if later.is_empty() { succs } else { later };
            inscope.seek_after_primary_effect(*e);
            let after: BTreeSet<usize> = inscope.get().iter().map(|b| b.index()).collect();
            if at.is_empty() {
                live.extend(after.iter().copied());
            }
            for s in at {
                inscope.seek_before_primary_effect(s);
                live.extend(inscope.get().iter().map(|b| b.index()).filter(|b| after.contains(b)));
            }
        }
        for l in &loans {
            if !live.contains(&l.loan) {
                continue;
            }
            // Who holds it here (Polonius: whose origins contain it after
            // the row's exits); the call / macro it went to otherwise.
            let mut now: Vec<String> = Vec::new();
            if let Some(t) = table {
                for e in exits {
                    for h in holders_at(t.mid_index(*e), BorrowIndex::from_usize(l.loan)) {
                        if !now.contains(&h) {
                            now.push(h);
                        }
                    }
                }
            }
            let first = loan_holder(l);
            let holder = if now.is_empty() || now.contains(&first) { first } else { now[0].clone() };
            // A two-phase `&mut` before its activation only reserves the
            // place: it can still be read. Activated on some path to here
            // (its activation reaches the row's exit) is activated.
            let reached = |a: Location| {
                exits.iter().any(|e| {
                    if a.block == e.block && a.statement_index <= e.statement_index {
                        return true;
                    }
                    let mut seen = BTreeSet::new();
                    let mut work: Vec<_> = body.basic_blocks[a.block].terminator().successors().collect();
                    while let Some(bb) = work.pop() {
                        if bb == e.block {
                            return true;
                        }
                        if seen.insert(bb) {
                            work.extend(body.basic_blocks[bb].terminator().successors());
                        }
                    }
                    false
                })
            };
            let kind = match l.kind {
                "shared" => "shared",
                "unique" => "unique",
                _ if l.two_phase && activation.get(&l.loan).is_none_or(|a| !reached(*a)) => "reserved",
                _ => "mut",
            };
            for c in &l.columns {
                // Only columns that hold a value can be lent.
                if matches!(st.state.get(c), Some(&"own" | &"partly" | &"maybe")) {
                    st.lent.entry(c.clone()).or_default().push(Lent { loan: l.loan, kind, holder: holder.clone() });
                }
            }
            // A lender that is no column (`*r` reborrowed through `r`,
            // `v[_]`): listed under its own name too (not a temporary).
            if !l.columns.contains(&l.lender) && l.source == "borrow" {
                st.lent.entry(l.lender.clone()).or_default().push(Lent { loan: l.loan, kind, holder: holder.clone() });
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
        // Only the same fact from the same MIR location is a duplicate (two
        // evaluations of one argument are two events), on the same exit site
        // (a drop two early exits share is one event per exit).
        fs.dedup_by(|a, b| a.fact == b.fact && a.order == b.order && a.site == b.site);
        row.facts = fs;
        out_rows.push(row);
    }
    for (line, fs) in evs {
        notes.push(format!("{} facts on row {} outside the body's rows", fs.len(), line));
    }
    BodyFacts {
        func,
        kind: if tcx.is_closure_like(def_id) { "closure" } else { "fn" },
        parent: tcx.is_closure_like(def_id).then(|| tcx.def_path_str(tcx.parent(def_id))),
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

/// The region variable a body's region is (after borrowck's renumbering).
fn region_vid(r: rustc_middle::ty::Region<'_>) -> Option<rustc_middle::ty::RegionVid> {
    match r.kind() {
        rustc_middle::ty::RegionKind::ReVar(v) => Some(v),
        _ => None,
    }
}

/// The type of `place`, with the region variables of its local's declared
/// type: a projection's own field type has fresh ones after borrowck's
/// renumbering, which no origin of the local is.
fn decl_place_ty<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, place: Place<'tcx>) -> rustc_middle::ty::Ty<'tcx> {
    let mut ty = body.local_decls[place.local].ty;
    let mut variant: Option<rustc_abi::VariantIdx> = None;
    for e in place.projection.iter() {
        ty = match e {
            ProjectionElem::Deref => ty.builtin_deref(true).unwrap_or(ty),
            ProjectionElem::Field(f, fty) => match ty.kind() {
                TyKind::Tuple(tys) => tys.get(f.index()).copied().unwrap_or(fty),
                TyKind::Adt(def, args) => {
                    let v = def.variant(variant.unwrap_or(rustc_abi::FIRST_VARIANT));
                    v.fields.get(f).map_or(fty, |fd| fd.ty(tcx, args))
                }
                _ => fty,
            },
            ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. } => match ty.kind() {
                TyKind::Array(t, _) | TyKind::Slice(t) => *t,
                _ => ty,
            },
            ProjectionElem::Downcast(_, v) => {
                variant = Some(v);
                continue;
            }
            _ => ty,
        };
        variant = None;
    }
    ty
}

/// A constant reference's loan kind: `&mut` constants exist
/// (`const BUF: &mut [u8] = &mut [];`).
fn const_kind(ty: rustc_middle::ty::Ty<'_>) -> &'static str {
    if matches!(ty.kind(), TyKind::Ref(_, _, m) if m.is_mut()) { "mut" } else { "shared" }
}

/// The type of field `f` of an aggregate of kind `k` whose type is `ty`
/// (with `ty`'s region variables).
fn aggregate_field_ty<'tcx>(tcx: TyCtxt<'tcx>, k: &AggregateKind<'tcx>, ty: rustc_middle::ty::Ty<'tcx>, f: rustc_abi::FieldIdx) -> Option<rustc_middle::ty::Ty<'tcx>> {
    match (k, ty.kind()) {
        (AggregateKind::Tuple, TyKind::Tuple(tys)) => tys.get(f.index()).copied(),
        (AggregateKind::Adt(_, v, ..), TyKind::Adt(def, args)) => def.variant(*v).fields.get(f).map(|fd| fd.ty(tcx, args)),
        _ => None,
    }
}

/// The origins (region variables) a type has, in order.
fn regions_of(ty: rustc_middle::ty::Ty<'_>) -> Vec<Origin> {
    let mut out = Vec::new();
    for a in ty.walk() {
        if let rustc_middle::ty::GenericArgKind::Lifetime(r) = a.kind() {
            if let Some(v) = region_vid(r) {
                let o = Origin::from(v);
                if !out.contains(&o) {
                    out.push(o);
                }
            }
        }
    }
    out
}

/// A destination name that is a user place (`r`, `h.r`, the closure `c`
/// that captures it), not a call or a MIR temporary.
fn user_place(d: &str) -> Option<String> {
    let d = d.trim_start_matches("closure ");
    (!d.is_empty() && !d.starts_with('<') && !d.ends_with("()")).then(|| d.to_owned())
}

/// The name of an aggregate's field `f` (`r` of `H { r }`, `0` of a tuple or
/// of `Some(..)`), if the aggregate is a struct, enum variant or tuple.
fn aggregate_field<'tcx>(tcx: TyCtxt<'tcx>, k: &AggregateKind<'tcx>, f: rustc_abi::FieldIdx) -> Option<String> {
    match k {
        AggregateKind::Adt(did, v, ..) => {
            let adt = tcx.adt_def(*did);
            Some(adt.variant(*v).fields.get(f).map_or_else(|| f.index().to_string(), |fd| fd.name.to_string()))
        }
        AggregateKind::Tuple => Some(f.index().to_string()),
        _ => None,
    }
}

/// The field paths of a struct or tuple value (`t.0`, `h.inner.r`) whose
/// types have one of `with`'s regions; empty for any other type, or when
/// the value as a whole is the only place that has one.
fn field_paths_with<'tcx>(tcx: TyCtxt<'tcx>, ty: rustc_middle::ty::Ty<'tcx>, name: &str, with: &BTreeSet<Origin>, depth: usize, out: &mut Vec<String>) {
    let fields: Vec<(String, rustc_middle::ty::Ty<'tcx>)> = match ty.kind() {
        // (A struct of this crate: another crate's, `Box` among them, has
        // fields the source can't name.)
        TyKind::Adt(def, args) if def.is_struct() && def.did().is_local() => {
            def.non_enum_variant().fields.iter().map(|fd| (fd.name.to_string(), fd.ty(tcx, args))).collect()
        }
        TyKind::Tuple(tys) => tys.iter().enumerate().map(|(i, t)| (i.to_string(), t)).collect(),
        _ => return,
    };
    let has = |t: rustc_middle::ty::Ty<'tcx>| {
        t.walk().any(|a| match a.kind() {
            rustc_middle::ty::GenericArgKind::Lifetime(r) => region_vid(r).is_some_and(|v| with.contains(&Origin::from(v))),
            _ => false,
        })
    };
    for (f, fty) in fields {
        if !has(fty) {
            continue;
        }
        let path = format!("{}.{}", name, f);
        let before = out.len();
        if depth < 3 {
            field_paths_with(tcx, fty, &path, with, depth + 1, out);
        }
        if out.len() == before {
            out.push(path);
        }
    }
}

/// `place` with a leading dereference of a MIR temporary replaced by the
/// user place the temporary refers to: `**_t` is `*gx` when the body's only
/// assignment of `_t` is `_t = &gx`; `*_t` is `*r` when it is `_t = copy r`.
fn resolve_temp_deref<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, place: Place<'tcx>) -> Place<'tcx> {
    let mut place = place;
    for _ in 0..4 {
        if place.projection.first() != Some(&ProjectionElem::Deref) || body.local_decls[place.local].is_user_variable() || place.local == RETURN_PLACE || place.local.index() <= body.arg_count {
            return place;
        }
        let mut src: Option<Place<'tcx>> = None;
        let mut n = 0;
        for data in body.basic_blocks.iter() {
            for st in &data.statements {
                if let StatementKind::Assign(asg) = &st.kind {
                    if asg.0.local == place.local {
                        n += 1;
                        if !asg.0.projection.is_empty() {
                            return place;
                        }
                        src = match &asg.1 {
                            // `*_t` of `_t = &P` is P.
                            Rvalue::Ref(_, _, p) => Some(*p),
                            // `*_t` of `_t = copy r` is `*r`.
                            Rvalue::Use(Operand::Copy(p) | Operand::Move(p)) => Some(tcx.mk_place_deref(*p)),
                            _ => None,
                        };
                    }
                }
            }
            if let TerminatorKind::Call { destination, .. } = &data.terminator().kind {
                if destination.local == place.local {
                    return place;
                }
            }
        }
        let (Some(base), 1) = (src, n) else { return place };
        let mut proj: Vec<_> = base.projection.iter().collect();
        proj.extend(place.projection.iter().skip(1));
        place = Place { local: base.local, projection: tcx.mk_place_elems(&proj) };
    }
    place
}

/// A place name without its enum downcasts, as a field path: `o.0` for
/// `(o as Some).0` (how a pattern reads what `Some(r)` put in `o.0`).
fn without_downcasts(n: &str) -> String {
    let mut n = n.to_owned();
    while let Some(a) = n.find(" as ") {
        let (Some(open), Some(close)) = (n[..a].rfind('('), n[a..].find(')').map(|c| a + c)) else { break };
        n = format!("{}{}{}", &n[..open], &n[open + 1..a], &n[close + 1..]);
    }
    n
}
