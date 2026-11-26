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

    fn ev(&self, l: Location, fact: Fact) -> (usize, Ev) {
        let row = self.rows.event_row.get(&l).copied().unwrap_or_else(|| self.row_of(l));
        // An early exit's drop sits where the exit is: its column and arm.
        let at = self.rows.early_exit.get(&l).map_or(l, |(_, x)| *x);
        let mut order = self.rows.order(l);
        if matches!(fact, Fact::BorrowEnd { .. }) {
            order.2 = 1;
        }
        let ev = Ev { col: self.rows.col.get(&at).copied().unwrap_or(0), arm: self.rows.arm_of(at), fact, order };
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
    /// *b;`): nothing in it can be used, rustc calls `b` moved.
    payload_moved: bool,
    payload_maybe: bool,
}

impl InitState {
    fn word(&self) -> &'static str {
        match (self.may_init, self.may_uninit) {
            (true, false) if self.payload_moved => "moved",
            (true, false) if self.payload_maybe => "maybe",
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
        column_info.push(ColumnInfo { name: n.clone(), root, kind, copy, needs_drop });
        columns.push((mpi, mp.place, n));
    }
    // The other fields of a struct or tuple column some of whose fields
    // have move paths (`p.b` once `p.a` was moved): rustc tracks them
    // with their parent, so their state follows it.
    let mut synth: Vec<(MovePathIndex, Place<'tcx>, String)> = Vec::new();
    for (mpi, place, _) in &columns {
        let mut kids: BTreeSet<usize> = BTreeSet::new();
        let mut c = move_data.move_paths[*mpi].first_child;
        while let Some(ch) = c {
            if let Some(ProjectionElem::Field(f, _)) = move_data.move_paths[ch].place.projection.last() {
                kids.insert(f.index());
            }
            c = move_data.move_paths[ch].next_sibling;
        }
        // (A Box's payload `*b` gets its fields whenever it is a column, if
        // the source can name them `b.a`: a tuple or a struct of this crate.)
        let pty0 = place.ty(&body.local_decls, tcx).ty;
        let own_type = match pty0.kind() {
            TyKind::Adt(def, _) => def.did().is_local(),
            TyKind::Tuple(_) => true,
            _ => false,
        };
        if kids.is_empty() && !(place.projection.last() == Some(&ProjectionElem::Deref) && own_type) {
            continue;
        }
        let pty = place.ty(&body.local_decls, tcx).ty;
        let fields: Vec<rustc_middle::ty::Ty<'tcx>> = match pty.kind() {
            TyKind::Adt(def, args) if def.is_struct() => def.non_enum_variant().fields.iter().map(|fd| fd.ty(tcx, args)).collect(),
            TyKind::Tuple(tys) => tys.iter().collect(),
            _ => continue,
        };
        for (i, fty) in fields.into_iter().enumerate() {
            if kids.contains(&i) {
                continue;
            }
            let fp = tcx.mk_place_field(*place, rustc_abi::FieldIdx::from_usize(i), fty);
            let Some(n) = cx.name(fp) else { continue };
            let Some((root, _, _)) = cx.names.root(fp) else { continue };
            let (copy, needs_drop, _) = ty_info(fty);
            column_info.push(ColumnInfo { name: n.clone(), root, kind: "field", copy, needs_drop });
            synth.push((*mpi, fp, n));
        }
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
        let Some(lender) = lender_name else {
            // A loan of a MIR temporary (`&mut *_14` for `next()`, or a
            // lifetime-extended `&String::new()`): not a column.
            // (`&make_pair().0` borrows a field of the temporary.)
            temp_holders.entry(bp.local).or_default().extend(holders.iter().filter(|h| !h.contains('(')).cloned());
            continue;
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
            let there = inscope.get().clone();
            for b in &here {
                if there.contains(*b) {
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
                let holder = if let Some(u) = used.first() {
                    u.clone()
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
                    now.iter().any(|h| h == root) || (now.is_empty() && li.holders.first().is_some_and(|h| h == n))
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
                let f = Fact::BorrowEnd { loan: b.index(), lender: li.lender.clone(), holder, why, to_line, to_col };
                push(&mut evs, cx.ev(loc, f));
            }
        }
    }
    for l in loans.iter_mut() {
        l.end_lines.sort();
    }

    // ---- stores of references into struct fields (`h.r = &b`) ------------------
    // Per field: each place a store puts loans there, and which loans. The
    // struct literal stores the loans first held by the field; an
    // assignment `h.r = <rhs>` the loans its right-hand side takes, or those
    // a reference variable it copies holds (`h.r = s`).
    let mut field_stores: BTreeMap<String, Vec<(Location, Vec<usize>)>> = BTreeMap::new();
    for l in &loans {
        if let Some(f) = l.holders.first().filter(|h| h.contains('.')) {
            if let Some((bl, _)) = bwf.borrow_set.location_map().get_index(l.loan) {
                field_stores.entry(f.clone()).or_default().push((*bl, vec![l.loan]));
            }
        }
    }
    for (&loc, _) in &cx.rows.row {
        let Some(st) = body.basic_blocks[loc.block].statements.get(loc.statement_index) else { continue };
        let StatementKind::Assign(asg) = &st.kind else { continue };
        let (lhs, rv) = &**asg;
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
        let lmap = bwf.borrow_set.location_map();
        let mut new: Vec<usize> = loans
            .iter()
            .filter(|l| l.holders.first() == Some(&path) && lmap.get_index(l.loan).is_some_and(|(bl, _)| span.contains(cx.r.span_of(*bl))))
            .map(|l| l.loan)
            .collect();
        // And the loans the struct's variable holds right after the store
        // that it didn't hold just before (`h.r = s` carries s's loan in).
        if let (Some(t), Some(pol), Some(os)) = (table, pol.as_ref(), origins_of.get(&lhs.local)) {
            let held = |p: Point| -> BTreeSet<usize> {
                let mut v = BTreeSet::new();
                if let Some(m) = pol.origin_contains_loan_at.get(&p) {
                    for o in os {
                        v.extend(m.get(o).into_iter().flatten().map(|b| b.index()));
                    }
                }
                v
            };
            let (before, after) = (held(t.start_index(loc)), held(t.mid_index(loc)));
            for b in after.difference(&before) {
                if loans.iter().any(|l| l.loan == *b) && !new.contains(b) {
                    new.push(*b);
                }
            }
        }
        new.sort();
        // (Replaces the literal's entries for loans taken in this very
        // statement: those are this store's.)
        let v = field_stores.entry(path).or_default();
        v.retain(|(_, ls)| !(ls.len() == 1 && new.contains(&ls[0]) && span.contains(cx.r.span_of(lmap.get_index(ls[0]).map_or(loc, |(bl, _)| *bl)))));
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
                        // A reference stored in a field (`h.r = &mut y;`): the
                        // old loans are those the last store into `h.r` on the
                        // way here put there (and still in scope), the new ones
                        // those this store puts there.
                        if let Some(this) = field_stores.get(&path).and_then(|v| v.iter().find(|(l, _)| *l == loc)) {
                            inscope.seek_before_primary_effect(loc);
                            let live: BTreeSet<usize> = inscope.get().iter().map(|b| b.index()).collect();
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
                        LookupResult::Exact(mpi) => drop_how(&rt.state(&[loc], false, mpi)),
                        LookupResult::Parent(_) => "drops",
                    };
                    let shared = cx.rows.shared_exits.get(&loc).cloned().unwrap_or_default();
                    let also_on_exits: Vec<usize> = shared.iter().map(|(r, _)| *r).collect();
                    // The drop also runs on early exits that share it: each
                    // exit gets its own on its row, with what the place
                    // holds when that path leaves (Q2: on the exit's row).
                    if let LookupResult::Exact(mpi) = rt_md.rev_lookup.find(place.as_ref()) {
                        for (r, at) in &shared {
                            let how_there = drop_how(&rt.state(&[*at], true, mpi));
                            if how_there == "nothing_to_drop" {
                                continue;
                            }
                            let mut ev = cx.ev(loc, Fact::Drop {
                                path: path.clone(),
                                how: how_there,
                                at_reassign: false,
                                early_exit: Some(*r),
                                also_on_exits: Vec::new(),
                            });
                            ev.0 = *r;
                            ev.1.col = cx.rows.col.get(at).copied().unwrap_or(0);
                            ev.1.arm = cx.rows.arm_of(*at);
                            ev.1.order = cx.rows.order(*at);
                            push(&mut evs, ev);
                        }
                    }
                    // Some fields moved out (on some path): drop elaboration
                    // drops each field on its own, in declaration order, those
                    // that may hold a value behind a flag (`b.a` re-assigned
                    // in one arm). A field with no move path of its own holds
                    // a value exactly where its parent (as a whole) does.
                    let key = |p: &Place<'tcx>| -> Vec<usize> {
                        p.projection.iter().filter_map(|e| match e {
                            ProjectionElem::Field(f, _) => Some(f.index()),
                            _ => None,
                        }).collect()
                    };
                    let mut field_drops: Vec<(Vec<usize>, String, &'static str)> = Vec::new();
                    if let LookupResult::Exact(top) = rt_md.rev_lookup.find(place.as_ref()) {
                        let mut work = vec![top];
                        while let Some(m) = work.pop() {
                            let st = rt.state(&[loc], false, m);
                            if st.sub_moved.is_empty() && st.sub_maybe.is_empty() {
                                continue;
                            }
                            let mut c = rt_md.move_paths[m].first_child;
                            while let Some(ch) = c {
                                let cp = rt_md.move_paths[ch].place;
                                c = rt_md.move_paths[ch].next_sibling;
                                if !ty_info(cp.ty(&body.local_decls, tcx).ty).1 {
                                    continue;
                                }
                                let cst = rt.state(&[loc], false, ch);
                                // (A Box's payload: its fields, inside.)
                                if !cst.sub_moved.is_empty() || !cst.sub_maybe.is_empty() || cp.projection.last() == Some(&ProjectionElem::Deref) {
                                    work.push(ch);
                                    continue;
                                }
                                if let Some(n) = cx.name(cp) {
                                    field_drops.push((key(&cp), n, drop_how(&cst)));
                                }
                            }
                            let parent_how = match (st.may_init, st.may_uninit) {
                                (true, false) => Some("drops"),
                                (true, true) => Some("drops_if_init"),
                                _ => None,
                            };
                            let mname = cx.name(rt_md.move_paths[m].place);
                            for (parent, fp, n) in &synth {
                                if column_name(*parent) == mname && ty_info(fp.ty(&body.local_decls, tcx).ty).1 {
                                    if let Some(h) = parent_how {
                                        field_drops.push((key(fp), n.clone(), h));
                                    }
                                }
                            }
                        }
                    }
                    field_drops.sort();
                    // Only early exits reach this drop: each has its own above.
                    if !cx.rows.exit_only.contains(&loc) {
                        push(&mut evs, cx.ev(loc, Fact::Drop { path, how, at_reassign, early_exit, also_on_exits }));
                        for (_, n, h) in field_drops {
                            push(&mut evs, cx.ev(loc, Fact::Drop { path: n, how: h, at_reassign, early_exit, also_on_exits: Vec::new() }));
                        }
                    }
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
                st.cause.insert(name.clone(), if moved { "moved" } else { "unassigned" });
            }
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
        for (parent, _, name) in &synth {
            let Some(pn) = column_name(*parent) else { continue };
            let Some(pw) = st.state.get(&pn).copied() else { continue };
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
            // `v[_]`): listed under its own name too.
            if !l.columns.contains(&l.lender) {
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
        // evaluations of one argument are two events).
        fs.dedup_by(|a, b| a.fact == b.fact && a.order == b.order);
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
