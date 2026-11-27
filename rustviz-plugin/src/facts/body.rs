//! One pass per MIR body: NLL `Borrows`, Polonius (naive, once per
//! body), storage liveness, and the init analyses (MaybeInit /
//! MaybeUninit / EverInit) twice: on borrowck's CFG for states, and on
//! the CFG without its imaginary edges for what runs (drop kinds, which
//! events happen). Then read at the row model's locations.

use super::places::{borrows_part_of, column_projection, Capture, Names};
use super::rows::{is_unreachable, real_successor_blocks, RowModel, Rows};
use super::schema::*;
use polonius_engine::{Algorithm, FactTypes, Output};
use rustc_borrowck::consumers::{
    places_conflict, BodyWithBorrowckFacts, BorrowIndex, Borrows, PlaceConflictBias, RustcFacts, TwoPhaseActivation,
};
use rustc_middle::mir::{
    AggregateKind, BasicBlock, Body, BorrowKind, Local, Location, MutBorrowKind, Operand, Place, ProjectionElem, Rvalue,
    StatementKind, TerminatorKind, UnwindAction, RETURN_PLACE,
};
use rustc_middle::ty::{TyCtxt, TyKind};
use rustc_mir_dataflow::impls::{
    always_storage_live_locals, EverInitializedPlaces, MaybeInitializedPlaces, MaybeStorageLive, MaybeUninitializedPlaces,
};
use rustc_mir_dataflow::move_paths::{LookupResult, MoveData, MovePathIndex};
use rustc_mir_dataflow::{Analysis, ResultsCursor};
use rustc_span::Span;
use rustc_utils::BodyExt;
use std::collections::{BTreeMap, BTreeSet};

type Origin = <RustcFacts as FactTypes>::Origin;
type Point = <RustcFacts as FactTypes>::Point;

struct Cx<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    body: &'a Body<'tcx>,
    names: Names<'tcx>,
    rows: RowModel,
    r: Rows<'a, 'tcx>,
    /// The call producing each expanded vec! (from a boxed array or a
    /// repeated element). Nested expansions have distinct compiler IDs.
    vec_results: std::collections::HashMap<rustc_span::hygiene::ExpnId, (Location, Place<'tcx>)>,
}

/// Use the MIR operand's function-item type, even for a call through a local.
fn manually_drop_take<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, func: &Operand<'tcx>) -> bool {
    let TyKind::FnDef(method, _) = *func.ty(&body.local_decls, tcx).kind() else { return false };
    tcx.item_name(method).as_str() == "take" && tcx.impl_of_assoc(method).is_some_and(|imp| {
        tcx.impl_opt_trait_ref(imp).is_none()
            && tcx.type_of(imp).skip_binder().ty_adt_def().is_some_and(|d| d.is_manually_drop())
    })
}

#[derive(Clone, Copy, Default)]
struct ExtractionPaths {
    taken: bool,
    intact: bool,
}

impl ExtractionPaths {
    const TAKEN: Self = Self { taken: true, intact: false };
    const INTACT: Self = Self { taken: false, intact: true };

    fn join(&mut self, other: Self) -> bool {
        let changed = (other.taken && !self.taken) || (other.intact && !self.intact);
        self.taken |= other.taken;
        self.intact |= other.intact;
        changed
    }
}

struct ManuallyTaken {
    column: String,
    inner: String,
    boundaries: BTreeMap<(Location, bool), ExtractionPaths>,
}

/// Track both possible contents states over the normal MIR CFG. The
/// wrapper's borrowck initialization remains independent of its contents.
fn manually_taken<'tcx>(
    cx: &Cx<'_, 'tcx>, calls: &BTreeSet<Location>,
) -> Vec<ManuallyTaken> {
    let Cx { tcx, body, .. } = *cx;
    let mut takes: Vec<(Place<'tcx>, BTreeSet<Location>)> = Vec::new();
    for &loc in calls {
        let TerminatorKind::Call { args, .. } = &body.basic_blocks[loc.block].terminator().kind else { continue };
        let [arg] = args.as_ref() else { continue };
        let Some(arg) = arg.node.place() else { continue };
        let place = resolve_temp_deref(tcx, body, tcx.mk_place_deref(arg));
        if place.projection.iter().all(|p| matches!(p, ProjectionElem::Field(..))) {
            if let Some((_, sites)) = takes.iter_mut().find(|(p, _)| *p == place) {
                sites.insert(loc);
            } else {
                takes.push((place, BTreeSet::from([loc])));
            }
        }
    }
    takes.into_iter().filter_map(|(place, takes)| {
        let TyKind::Adt(def, args) = *place.ty(&body.local_decls, tcx).ty.kind() else { return None };
        if !def.is_manually_drop() { return None; }
        let column = cx.name(place)?;
        let ty = args.type_at(0);
        let inner = match ty.kind() {
            TyKind::Adt(def, _) => tcx.item_name(def.did()).to_string(),
            TyKind::Closure(..) => "closure".to_owned(),
            TyKind::Ref(..) => "reference".to_owned(),
            TyKind::Tuple(..) if !ty.is_unit() => "tuple".to_owned(),
            TyKind::Array(..) => "array".to_owned(),
            _ if ty.is_primitive() => ty.to_string(),
            _ => "value".to_owned(),
        };
        let writes = |dest: Place<'tcx>| {
            let dest = resolve_write_deref(tcx, body, dest);
            dest.local == place.local && place.projection.starts_with(dest.projection)
        };
        let entry = Location { block: rustc_middle::mir::START_BLOCK, statement_index: 0 };
        let mut boundaries = BTreeMap::from([((entry, false), ExtractionPaths::INTACT)]);
        let mut work = vec![entry];
        while let Some(loc) = work.pop() {
            let data = &body.basic_blocks[loc.block];
            let replaced = if let Some(st) = data.statements.get(loc.statement_index) {
                match &st.kind {
                    StatementKind::Assign(a) => writes(a.0),
                    StatementKind::StorageLive(l) | StatementKind::StorageDead(l) => *l == place.local,
                    _ => false,
                }
            } else if let TerminatorKind::Call { func, args, destination, .. } = &data.terminator().kind {
                writes(*destination) || (matches!(func.ty(&body.local_decls, tcx).kind(), TyKind::FnDef(did, _)
                    if tcx.is_diagnostic_item(rustc_span::sym::ptr_write, *did))
                    && args.first().and_then(|a| a.node.place()).is_some_and(|p| writes(tcx.mk_place_deref(p))))
            } else {
                false
            };
            let after = if replaced {
                ExtractionPaths::INTACT
            } else if takes.contains(&loc) {
                ExtractionPaths::TAKEN
            } else {
                boundaries[&(loc, false)]
            };
            if boundaries.entry((loc, true)).or_default().join(after) {
                for next in cx.r.real_succs(loc) {
                    if boundaries.entry((next, false)).or_default().join(after) {
                        work.push(next);
                    }
                }
            }
        }
        Some(ManuallyTaken { column, inner, boundaries })
    }).collect()
}

fn after_call<'tcx>(body: &Body<'tcx>, loc: Location, predicate: impl Fn(&Operand<'tcx>) -> bool) -> bool {
    let dom = body.basic_blocks.dominators();
    let mut block = loc.block;
    while let Some(pred) = dom.immediate_dominator(block) {
        if let TerminatorKind::Call { func, target: Some(target), .. } = &body.basic_blocks[pred].terminator().kind {
            if dom.dominates(*target, loc.block) && predicate(func) {
                return true;
            }
        }
        block = pred;
    }
    false
}

impl BodyFacts {
    /// For a store through two references, follow the reaching MIR values
    /// back to the two address-of operations. A join, call, or intervening
    /// indirect store can change the referent; leave its name out then.
    pub(crate) fn reference_slot_write(&self, body: &Body<'_>, at: Location) -> Option<(String, String)> {
        let mut block = at.block;
        let mut index = at.statement_index;
        let StatementKind::Assign(write) = &body.basic_blocks[block].statements.get(index)?.kind else { return None };
        if write.0.projection.as_slice() != [ProjectionElem::Deref, ProjectionElem::Deref] { return None; }
        let mut local = write.0.local;
        let mut slot = None;
        let mut seen = BTreeSet::new();
        while seen.insert(block) {
            for stmt in body.basic_blocks[block].statements[..index].iter().rev() {
                match &stmt.kind {
                    StatementKind::Assign(assign) => {
                        let (dst, rhs) = &**assign;
                        if dst.is_indirect() { return None; }
                        if dst.local != local { continue; }
                        if !dst.projection.is_empty() { return None; }
                        local = match rhs {
                            Rvalue::Use(Operand::Move(p) | Operand::Copy(p)) => p.as_local()?,
                            Rvalue::Ref(_, _, p) => {
                                let borrowed = p.as_local()?;
                                let name = &self.vars.iter().find(|v| v.local == borrowed.index() && v.capture.is_none())?.name;
                                if let Some(slot) = slot { return Some((slot, name.clone())); }
                                slot = Some(name.clone());
                                borrowed
                            }
                            _ => return None,
                        };
                    }
                    StatementKind::StorageLive(_) | StatementKind::StorageDead(_) |
                    StatementKind::PlaceMention(_) | StatementKind::FakeRead(_) |
                    StatementKind::AscribeUserType(..) | StatementKind::Nop => {}
                    _ => return None,
                }
            }
            let &[pred] = body.basic_blocks.predecessors()[block].as_slice() else { return None };
            let data = &body.basic_blocks[pred];
            if !matches!(data.terminator().kind, TerminatorKind::Goto { target } | TerminatorKind::Assert { target, .. } if target == block) {
                return None;
            }
            block = pred;
            index = data.statements.len();
        }
        None
    }
}

/// A staged operand is destroyed on the early-return edge of `?` when
/// the residual conversion dominates the drop of that exact MIR temporary.
pub(crate) fn dropped_on_residual<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, facts: &BodyFacts, assignment: Location) -> bool {
    let Some(st) = body.basic_blocks[assignment.block].statements.get(assignment.statement_index) else { return false };
    let StatementKind::Assign(a) = &st.kind else { return false };
    facts.rows.iter().flat_map(|row| &row.facts).any(|event| {
        matches!(event.fact, Fact::TempDrop { .. }) &&
            matches!(body.basic_blocks[event.location.block].terminator().kind, TerminatorKind::Drop { place, .. } if place == a.0) &&
            after_call(body, event.location, |f| f.const_fn_def().is_some_and(|(d, _)| tcx.is_lang_item(d, rustc_hir::LangItem::TryTraitFromResidual)))
    })
}

fn called_closure<'tcx>(tcx: TyCtxt<'tcx>, func: &Operand<'tcx>) -> Option<&'tcx Body<'tcx>> {
    let (method, args) = func.const_fn_def()?;
    let trait_id = tcx.trait_of_assoc(method)?;
    if ![tcx.lang_items().fn_trait(), tcx.lang_items().fn_mut_trait(), tcx.lang_items().fn_once_trait()].contains(&Some(trait_id)) {
        return None;
    }
    let TyKind::Closure(id, _) = args.type_at(0).kind() else { return None };
    let local = id.as_local()?;
    Some(&rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, local).body)
}

fn closure_has_no_return<'tcx>(tcx: TyCtxt<'tcx>, func: &Operand<'tcx>) -> bool {
    called_closure(tcx, func).is_some_and(|body| !closure_can_return(tcx, body))
}

fn closure_can_return<'tcx>(tcx: TyCtxt<'tcx>, closure: &Body<'tcx>) -> bool {
    let rows = Rows { tcx, body: closure };
    let mut work = vec![rustc_middle::mir::START_BLOCK];
    let mut seen = BTreeSet::new();
    while let Some(bb) = work.pop() {
        if !seen.insert(bb) { continue; }
        let data = &closure.basic_blocks[bb];
        if matches!(data.terminator().kind, TerminatorKind::Return) { return true; }
        let end = Location { block: bb, statement_index: data.statements.len() };
        work.extend(rows.real_succs(end).into_iter().map(|next| next.block));
    }
    false
}

/// Work out which normal blocks become unreachable when local closures
/// with no MIR return stop execution on each path. Retain the initiating
/// call when all paths into a skipped block go through the same cut.
fn skipped_closure_returns<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> BTreeMap<BasicBlock, Option<Location>> {
    let mut reachable = BTreeSet::new();
    let mut cuts = Vec::new();
    let mut work = vec![rustc_middle::mir::START_BLOCK];
    while let Some(bb) = work.pop() {
        if !reachable.insert(bb) { continue; }
        if let TerminatorKind::Call { func, target: Some(target), .. } = &body.basic_blocks[bb].terminator().kind {
            if closure_has_no_return(tcx, func) {
                cuts.push((*target, Some(body.terminator_loc(bb))));
                continue;
            }
        }
        work.extend(real_successor_blocks(body, bb));
    }

    let mut skipped: BTreeMap<BasicBlock, Option<Location>> = BTreeMap::new();
    while let Some((bb, from)) = cuts.pop() {
        if reachable.contains(&bb) || is_unreachable(body, bb) { continue; }
        let from = match skipped.get(&bb) {
            Some(&prev) if prev == from || prev.is_none() => continue,
            Some(_) => None,
            None => from,
        };
        skipped.insert(bb, from);
        cuts.extend(real_successor_blocks(body, bb).into_iter().map(|target| (target, from)));
    }
    skipped
}

/// How a call with no normal successor uses this mutable argument. The
/// formatter case requires a direct MIR path from the parameter to panic_display.
pub(crate) fn terminal_argument<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, span: Span) -> Option<TerminalArgument> {
    let mut uses = body.basic_blocks.iter().filter_map(|bb| {
        let TerminatorKind::Call { func, args, target: None, .. } = &bb.terminator().kind else { return None };
        let mut indices = args.iter().enumerate().filter(|(_, a)| a.span.source_equal(span)).map(|(i, _)| i);
        let index = indices.next()?;
        let callee = func.const_fn_def().and_then(|(id, _)| id.as_local());
        Some(indices.next().is_none().then_some((callee, index)))
    });
    let argument = uses.next()?;
    let formatted = argument.filter(|_| uses.next().is_none()).is_some_and(|(callee, index)| {
        let Some(id) = callee.filter(|id| tcx.hir_maybe_body_owned_by(*id).is_some()) else { return false };
        let body = &rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, id).body;
        let first = &body[rustc_middle::mir::START_BLOCK];
        let TerminatorKind::Call { func, args, .. } = &first.terminator().kind else { return false };
        let Some(param) = body.args_iter().nth(index) else { return false };
        let [arg] = &args[..] else { return false };
        // The formatter must receive the incoming reference. A store or
        // address-taking operation could have retargeted the parameter.
        if first.statements.iter().any(|s|
            matches!(&s.kind, StatementKind::Assign(a) if a.0.local == param ||
                matches!(a.1, Rvalue::Ref(_, BorrowKind::Mut { .. }, p) | Rvalue::RawPtr(_, p) if p.local == param)))
        { return false; }
        func.const_fn_def().is_some_and(|(d, _)| tcx.is_lang_item(d, rustc_hir::LangItem::PanicDisplay))
            && arg.node.place().is_some_and(|p| resolve_write_deref(tcx, body, tcx.mk_place_deref(p)) == Place::from(param))
    });
    Some(if formatted { TerminalArgument::Formatted } else { TerminalArgument::Borrowed })
}

/// Look for a compiler panic entry point with a route out of the closure.
/// A generic call's MIR unwind edge alone says nothing about whether it
/// can panic: rustc can retain that edge even for process termination.
fn closure_has_panic_route<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> bool {
    use rustc_hir::LangItem::{BeginPanic, PanicDisplay, PanicFmt};

    let mut work = vec![rustc_middle::mir::START_BLOCK];
    let mut seen = BTreeSet::new();
    while let Some(bb) = work.pop() {
        if !seen.insert(bb) { continue; }
        if let TerminatorKind::Call { func, unwind, .. } = &body.basic_blocks[bb].terminator().kind {
            if func.const_fn_def().is_some_and(|(id, _)| [PanicFmt, PanicDisplay, BeginPanic].into_iter()
                .any(|item| tcx.is_lang_item(id, item))) {
                let propagates = match *unwind {
                    UnwindAction::Continue => true,
                    UnwindAction::Cleanup(start) => cleanup_path(body, start).last().is_some_and(|bb|
                        matches!(body.basic_blocks[bb].terminator().kind, TerminatorKind::UnwindResume)),
                    _ => false,
                };
                if propagates { return true; }
            }
            if closure_has_no_return(tcx, func) { continue; }
        }
        work.extend(real_successor_blocks(body, bb));
    }
    false
}

/// Borrowck MIR records pending drops on the cleanup edge. Read its
/// straight-line chain in execution order, following each drop's return.
fn cleanup_path<'a, 'tcx>(body: &'a Body<'tcx>, start: BasicBlock) -> impl Iterator<Item = BasicBlock> + use<'a, 'tcx> {
    let mut seen = BTreeSet::new();
    let mut next = Some(start);
    std::iter::from_fn(move || {
        let bb = next.take()?;
        let data = &body.basic_blocks[bb];
        if !seen.insert(bb) || !data.is_cleanup || !data.statements.iter().all(|s|
            matches!(s.kind, StatementKind::StorageDead(_) | StatementKind::Nop)) {
            return None;
        }
        next = match data.terminator().kind {
            TerminatorKind::Drop { target, .. } | TerminatorKind::Goto { target } => Some(target),
            _ => None,
        };
        Some(bb)
    })
}

fn cleanup_drops<'tcx>(body: &Body<'tcx>, start: BasicBlock) -> Vec<(Location, Place<'tcx>)> {
    let mut storage_dead = BTreeSet::new();
    cleanup_path(body, start).filter_map(|bb| {
        let data = &body.basic_blocks[bb];
        storage_dead.extend(data.statements.iter().filter_map(|s| match s.kind {
            StatementKind::StorageDead(local) => Some(local),
            _ => None,
        }));
        match data.terminator().kind {
            TerminatorKind::Drop { place, .. } if !storage_dead.contains(&place.local) => Some((body.terminator_loc(bb), place)),
            _ => None,
        }
    }).collect()
}

/// On entry to this while-let, real MIR edges lead straight to a pattern
/// binding before any test can skip the body. Unknown for branched entries.
pub(crate) fn loop_enters_pattern<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, facts: &BodyFacts, head: Span, pattern: Span) -> bool {
    let mut loc = match body.basic_blocks.iter_enumerated().find(|(_, data)| {
        let t = data.terminator();
        t.source_info.span == head && matches!(t.kind, TerminatorKind::FalseUnwind { .. })
    }) {
        Some((bb, _)) => body.terminator_loc(bb),
        None => return false,
    };
    let bindings: BTreeSet<_> = facts.binding_sources.iter().filter(|s| s.runs && pattern.contains(s.site))
        .map(|s| s.location).collect();
    let rows = Rows { tcx, body };
    let mut seen = BTreeSet::new();
    while seen.insert(loc) {
        if bindings.contains(&loc) { return true; }
        let succs = rows.real_succs(loc);
        let [next] = succs.as_slice() else { break };
        loc = *next;
    }
    false
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

    /// Follow the reaching definitions of the temporaries staging an aggregate
    /// operand. An implicit reborrow has its own issue location. A raw address,
    /// a scalar read through a reference, and a call result are not that loan.
    fn operand_loans(
        &self, operand: &Operand<'tcx>, at: Location, issued: &BTreeMap<Location, usize>,
        runs: &BTreeSet<rustc_middle::mir::BasicBlock>, carried: &impl Fn(Place<'tcx>, Location) -> Vec<usize>, out: &mut Vec<AggregateLoan>,
    ) {
        let Some(place) = operand.place() else { return };
        let construction = at.block;
        let doms = self.body.basic_blocks.dominators();
        // A reference can already exist before the branch that selects it.
        // Its use as an operand, as well as its issue, can be conditional.
        let mut record = |loan, issued, site: Location| {
            let conditional = !doms.dominates(site.block, construction);
            if let Some(existing) = out.iter_mut().find(|b| b.loan == loan && b.issued == issued) {
                existing.conditional &= conditional;
            } else {
                out.push(AggregateLoan { loan, issued, conditional });
            }
        };
        let mut work = vec![(place, at)];
        let mut seen = std::collections::HashSet::new();
        while let Some((place, at)) = work.pop() {
            let ty = decl_place_ty(self.tcx, self.body, place);
            if !seen.insert((place, at)) || ty.is_raw_ptr() || regions_of(ty).is_empty() {
                continue;
            }
            if self.is_user(place) {
                for id in carried(place, at) {
                    record(id, false, at);
                }
                continue;
            }
            if !place.projection.is_empty() { continue; }
            let block = &self.body.basic_blocks[at.block];
            let def = block.statements[..at.statement_index].iter().enumerate().rev().find(|(_, st)| match &st.kind {
                StatementKind::Assign(a) => a.0.local == place.local,
                StatementKind::StorageLive(l) | StatementKind::StorageDead(l) => *l == place.local,
                _ => false,
            });
            if let Some((i, st)) = def {
                let StatementKind::Assign(a) = &st.kind else { continue };
                if a.0 != place { continue; }
                let loc = Location { block: at.block, statement_index: i };
                match &a.1 {
                    // rustc can reborrow the reference it just staged for an
                    // explicit borrow. Follow only a dereference of that temp.
                    Rvalue::Ref(_, _, p) if !self.is_user(*p) && p.projection.as_slice() == [ProjectionElem::Deref]
                        && self.body.local_decls[p.local].ty.is_ref() => work.push((Place::from(p.local), loc)),
                    Rvalue::Ref(..) => {
                        if let Some(&id) = issued.get(&loc) {
                            record(id, true, loc);
                        }
                    }
                    Rvalue::Aggregate(_, ops) => work.extend(ops.iter().rev().filter_map(|o| o.place().map(|p| (p, loc)))),
                    Rvalue::Use(o) | Rvalue::Repeat(o, _) | Rvalue::Cast(_, o, _) => work.extend(o.place().map(|p| (p, loc))),
                    _ => {}
                }
            } else {
                for &pred in self.body.basic_blocks.predecessors()[at.block].iter().rev() {
                    let data = &self.body.basic_blocks[pred];
                    let loc = Location { block: pred, statement_index: data.statements.len() };
                    if !runs.contains(&pred) || data.is_cleanup || !self.r.real_succs(loc).iter().any(|s| s.block == at.block) { continue; }
                    if matches!(data.terminator().kind, TerminatorKind::Call { destination, .. } if destination.local == place.local) {
                        // The result may keep argument loans. They restrict
                        // access, but do not identify what the result points to.
                        for id in carried(place, Location { block: at.block, statement_index: 0 }) {
                            record(id, false, loc);
                        }
                    } else {
                        work.push((place, loc));
                    }
                }
            }
        }
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
        match self.destination(place, loc, depth) {
            (name, true) if name == "<temp>" => "<temporary Vec>".into(),
            (name, _) => name,
        }
    }

    /// The flag means the value is inside a vector element. Its fields
    /// aren't fields of the column that owns the vector.
    fn destination(&self, place: Place<'tcx>, loc: Location, depth: usize) -> (String, bool) {
        if let Some(n) = self.name(place) {
            return (n, false);
        }
        if place.local == RETURN_PLACE {
            return ("<return>".into(), false);
        }
        if depth > 6 || !place.projection.is_empty() {
            return ("<temp>".into(), false);
        }
        // A MIR temporary is assigned once and used once: follow it to
        // that use (whatever the rows in between).
        let tmp = place.local;
        let mut best = None;
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
                    best = Some(("<operand>".into(), false));
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
                // The array is initialized inside the allocation. The Vec
                // returned by this very expansion owns its elements. If
                // evaluating another element can leave before constructing
                // the array (e.g. `?`), only the move into a temp is certain.
                if matches!(rv, Rvalue::Aggregate(k, _) if matches!(**k, AggregateKind::Array(_))) {
                    if let Some(&(call, result)) = self.vec_results.get(&st.source_info.span.ctxt().outer_expn()) {
                        return if self.row_of(loc) == self.row_of(call) && !self.replaces_after_call(call) && self.reaches_without_branch(loc, here) {
                            (self.dest_name(result, call, depth + 1), true)
                        } else {
                            ("<temp>".into(), false)
                        };
                    }
                }
                let (d, element) = self.destination(*lhs, here, depth + 1);
                let closure = matches!(rv, Rvalue::Aggregate(k, _) if matches!(**k, AggregateKind::Closure(..)));
                best = Some((match field {
                    _ if closure => format!("closure {}", d),
                    Some(f) if !element && !d.starts_with('<') && !d.ends_with("()") => format!("{}.{}", d, f),
                    _ => d,
                }, element));
            }
            if let TerminatorKind::Call { args, fn_span, func, destination, .. } = &data.terminator().kind {
                if best.is_none() && args.iter().any(|a| matches!(&a.node, Operand::Move(p) | Operand::Copy(p) if p.local == tmp)) {
                    let at = Location { block: bb, statement_index: data.statements.len() };
                    let expansion = data.terminator().source_info.span.ctxt().outer_expn();
                    best = Some(if self.vec_results.get(&expansion).is_some_and(|(call, _)| *call == at)
                        && args.first().is_some_and(|a| a.node.place().is_some_and(|p| p.as_local() == Some(tmp))) {
                        // from_elem takes the staged first argument even when
                        // it returns an empty vector; count evaluation can exit.
                        let repeat = func.const_fn_def().is_some_and(|(d, _)| self.tcx.is_diagnostic_item(rustc_span::Symbol::intern("vec_from_elem"), d));
                        let count = args.get(1).and_then(|a| a.node.constant()).and_then(|c|
                            c.const_.try_eval_target_usize(self.tcx, self.body.typing_env(self.tcx)));
                        if repeat && (count.is_none_or(|n| n == 0) || self.row_of(loc) != self.row_of(at) || self.replaces_after_call(at) || !self.reaches_without_branch(loc, at)) {
                            ("<temp>".into(), false)
                        } else {
                            (self.dest_name(*destination, at, depth + 1), true)
                        }
                    } else {
                        (format!("{}()", self.call_name(*fn_span, func, at)), false)
                    });
                }
            }
        }
        best.unwrap_or_else(|| ("<temp>".into(), false))
    }

    fn replaces_after_call(&self, call: Location) -> bool {
        let TerminatorKind::Call { destination, target: Some(next), .. } = self.body.basic_blocks[call.block].terminator().kind else { return false };
        let TerminatorKind::Drop { place, target, .. } = self.body.basic_blocks[next].terminator().kind else { return false };
        self.body.basic_blocks[target].statements.iter().any(|s| matches!(&s.kind,
            StatementKind::Assign(a) if a.0 == place && matches!(&a.1, Rvalue::Use(Operand::Move(p)) if *p == destination)))
    }

    fn reaches_without_branch(&self, mut at: Location, target: Location) -> bool {
        let original = &self.body.basic_blocks[at.block];
        let source = original.statements.get(at.statement_index).and_then(|st| match &st.kind {
            StatementKind::Assign(asg) => match &asg.1 {
                Rvalue::Use(op) => op.place().filter(|p| self.is_user(*p)),
                _ => None,
            },
            _ => None,
        });
        let overwrites = |place| source.is_some_and(|p| places_conflict(self.tcx, self.body, p, place, PlaceConflictBias::Overlap));
        let mut start = at.statement_index + 1;
        let mut seen = BTreeSet::new();
        loop {
            let data = &self.body.basic_blocks[at.block];
            let stop = if at.block == target.block { target.statement_index } else { data.statements.len() };
            if data.statements.get(start..stop).is_some_and(|stmts| stmts.iter().any(|s|
                matches!(&s.kind, StatementKind::Assign(asg) if overwrites(asg.0)))) { return false; }
            if at.block == target.block {
                return at.statement_index <= stop;
            }
            if matches!(&data.terminator().kind, TerminatorKind::Call { destination, func, .. } if overwrites(*destination) || closure_has_no_return(self.tcx, func)) {
                return false;
            }
            start = 0;
            if !seen.insert(at.block) {
                return false;
            }
            at.statement_index = self.body.basic_blocks[at.block].statements.len();
            match self.r.real_succs(at).as_slice() {
                [next] => at = *next,
                _ => return false,
            }
        }
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

    fn place_ev(&self, l: Location, local: Local, fact: Fact) -> (usize, Ev) {
        let mut event = self.ev(l, fact);
        event.1.place_local = Some(local);
        event
    }

    fn ev(&self, l: Location, fact: Fact) -> (usize, Ev) {
        let row = self.rows.event_row.get(&l).copied().unwrap_or_else(|| self.row_of(l));
        // An early exit's drop sits where the exit is: its column and arm.
        let at = self.rows.early_exit.get(&l).map_or(l, |(_, x)| *x);
        let mut order = self.rows.order(l);
        if matches!(fact, Fact::BorrowEnd { .. }) {
            order.2 = 1;
        }
        let ev = Ev { location: l, display_location: at, place_local: None, drop_route: DropRoute::Normal, end_successor: None, end_places: vec![], caller_slot: None, transfer_paths: vec![], col: self.rows.col.get(&at).copied().unwrap_or(0), arm: self.rows.arm_of(at), fact, order, site: None };
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
    may_entry(body, BTreeSet::new(), &|set, loc| moved_transfer(md, body, set, loc))
}

/// Which move paths may never have been given a value (since their
/// variable's storage started), at each block's entry: a place that is
/// uninitialized on every path and in none of these was moved out on
/// every path.
fn maybe_unassigned_entry(body: &Body<'_>, md: &MoveData<'_>) -> BTreeMap<rustc_middle::mir::BasicBlock, BTreeSet<MovePathIndex>> {
    let start: BTreeSet<MovePathIndex> = md
        .move_paths
        .iter_enumerated()
        .filter(|(_, mp)| mp.place.local.index() > body.arg_count)
        .map(|(m, _)| m)
        .collect();
    may_entry(body, start, &|set, loc| unassigned_transfer(md, body, set, loc))
}

/// A forward "may" dataflow (union at joins) over the non-cleanup blocks,
/// as the set at each block's entry.
fn may_entry(
    body: &Body<'_>,
    start: BTreeSet<MovePathIndex>,
    transfer: &dyn Fn(&mut BTreeSet<MovePathIndex>, Location),
) -> BTreeMap<rustc_middle::mir::BasicBlock, BTreeSet<MovePathIndex>> {
    let mut entry: BTreeMap<rustc_middle::mir::BasicBlock, BTreeSet<MovePathIndex>> = BTreeMap::new();
    entry.insert(rustc_middle::mir::START_BLOCK, start);
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
                transfer(&mut set, Location { block: bb, statement_index: i });
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

/// The set of a [`may_entry`] dataflow after `loc`.
fn may_after(
    entry: &BTreeMap<rustc_middle::mir::BasicBlock, BTreeSet<MovePathIndex>>,
    transfer: &dyn Fn(&mut BTreeSet<MovePathIndex>, Location),
    loc: Location,
    after: bool,
) -> BTreeSet<MovePathIndex> {
    let mut set = entry.get(&loc.block).cloned().unwrap_or_default();
    for i in 0..loc.statement_index + usize::from(after) {
        transfer(&mut set, Location { block: loc.block, statement_index: i });
    }
    set
}

/// A variable's storage starting or ending leaves it (and its fields)
/// with no value; an assignment gives the place and its fields one.
fn unassigned_transfer(md: &MoveData<'_>, body: &Body<'_>, set: &mut BTreeSet<MovePathIndex>, loc: Location) {
    if let Some(StatementKind::StorageLive(l) | StatementKind::StorageDead(l)) =
        body.basic_blocks[loc.block].statements.get(loc.statement_index).map(|s| &s.kind)
    {
        if let LookupResult::Exact(root) = md.rev_lookup.find(Place::from(*l).as_ref()) {
            set.extend(descendants(md, root));
        }
    }
    for ii in md.init_loc_map[loc].iter() {
        for d in descendants(md, md.inits[*ii].path) {
            set.remove(&d);
        }
    }
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
    let mut names = Names::new(tcx, body);
    for (&local, name) in &names.bindings.locals {
        let ty = body.local_decls[local].ty;
        if let Some(parts) = BodyBindings::array_elems_left(tcx, body, local, name, ty) {
            names.bindings.arrays.insert(local, parts);
        }
        if BodyBindings::box_contents_moved_out(body, local, ty) { names.bindings.emptied_boxes.insert(local); }
    }
    let vec_results = body.basic_blocks.iter_enumerated().filter_map(|(bb, data)| {
        let term = data.terminator();
        let TerminatorKind::Call { func, destination, .. } = &term.kind else { return None };
        let (did, _) = func.const_fn_def()?;
        let expansion = term.source_info.span.ctxt().outer_expn();
        (super::is_vec_expansion(tcx, expansion)
            && (tcx.is_diagnostic_item(rustc_span::Symbol::intern("box_assume_init_into_vec_unsafe"), did)
                || tcx.is_diagnostic_item(rustc_span::Symbol::intern("vec_from_elem"), did)))
            .then_some((expansion, (Location { block: bb, statement_index: data.statements.len() }, *destination)))
    }).collect();
    let cx = Cx { tcx, body, names, rows, r, vec_results };
    let skipped_closure_returns = skipped_closure_returns(tcx, body);
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
    let option_ref_region = |ty: rustc_middle::ty::Ty<'tcx>| {
        let TyKind::Adt(def, args) = ty.kind() else { return None };
        if !tcx.is_diagnostic_item(rustc_span::sym::Option, def.did()) { return None; }
        let TyKind::Ref(region, ..) = args.type_at(0).kind() else { return None };
        Some(*region)
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
    let unassigned_entry = maybe_unassigned_entry(body, &move_data);
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
    let rt_moved_entry = maybe_moved_entry(&rt_body, &rt_md);
    let rt_unassigned_entry = maybe_unassigned_entry(&rt_body, &rt_md);
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

    let array_len = |place: Place<'tcx>| match place.ty(&body.local_decls, tcx).ty.kind() {
        TyKind::Array(_, len) => len.try_to_target_usize(tcx).and_then(|n| usize::try_from(n).ok()),
        _ => None,
    };
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
        column_info.push(ColumnInfo { local: mp.place.local, name: n.clone(), root, kind, copy, needs_drop, is_box: mp.place.ty(&body.local_decls, tcx).ty.is_box(), array_len: array_len(mp.place), hidden_fields: false });
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
                column_info.push(ColumnInfo { local: fp.local, name: n.clone(), root, kind: "field", copy, needs_drop, is_box: fty.is_box(), array_len: array_len(fp), hidden_fields: false });
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
    // The hidden iterator of a `for` loop (`let mut iter =
    // IntoIterator::into_iter(<head>)` in the desugaring).
    let for_iter = |l: Local| {
        let decl = &body.local_decls[l];
        decl.is_user_variable() && decl.source_info.span.desugaring_kind() == Some(rustc_span::DesugaringKind::ForLoop)
    };
    let mut iterator_origins = Vec::new();
    let mut origins_of: BTreeMap<Local, Vec<Origin>> = BTreeMap::new();
    // Every local's (temporaries' and the return place's too).
    let mut all_origins: BTreeMap<Local, Vec<Origin>> = BTreeMap::new();
    if let Some(f) = pfacts {
        for (local, origin) in f.use_of_var_derefs_origin.iter().chain(f.drop_of_var_derefs_origin.iter()) {
            let os = all_origins.entry(*local).or_default();
            if !os.contains(origin) {
                os.push(*origin);
            }
            if for_iter(*local) && !iterator_origins.contains(origin) {
                iterator_origins.push(*origin);
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
    let mut inline_holder_ends = Vec::new();
    let mut loans: Vec<LoanInfo> = Vec::new();
    let mut binding_loans = BTreeMap::new();
    let mut evs: BTreeMap<usize, Vec<Ev>> = BTreeMap::new();
    let push = |evs: &mut BTreeMap<usize, Vec<Ev>>, (row, ev): (usize, Ev)| evs.entry(row).or_default().push(ev);
    // Where each two-phase loan is activated.
    let mut activation: BTreeMap<usize, Location> = BTreeMap::new();
    // Loans of temporaries, for `TempInfo::held_by`.
    let mut temp_holders: BTreeMap<Local, Vec<String>> = BTreeMap::new();
    // A live reborrow can keep its parent loan alive after the parent's
    // last direct use. Match loan identities at the reborrow point.
    let mut reborrows = BTreeMap::<usize, Vec<(usize, String)>>::new();
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
        let mut part_of = Vec::new();
        let cols: Vec<String> = columns
            .iter()
            .chain(synth.iter())
            .filter(|(_, cp, _)| places_conflict(tcx, body, bp, *cp, PlaceConflictBias::Overlap))
            .map(|(_, cp, name)| {
                if borrows_part_of(bp, *cp) {
                    part_of.push(name.clone());
                }
                name.clone()
            })
            .collect();
        if let (Some(through), Some(t), Some(pol)) = (cx.names.through(tcx, body, bp), table, &pol) {
            if cx.names.locals.get(&bp.local) == Some(&through) {
                if let Some(at) = pol.origin_contains_loan_at.get(&t.start_index(*loc)) {
                    let parents: BTreeSet<_> = origins_of.get(&bp.local).into_iter().flatten()
                        .filter_map(|origin| at.get(origin)).flatten().collect();
                    for parent in parents {
                        reborrows.entry(parent.index()).or_default().push((idx, through.clone()));
                    }
                }
            }
        }
        let whole = columns.iter().chain(&synth).filter(|(_, cp, _)| {
            cp.local == bp.local && (cp.projection.starts_with(bp.projection)
                || (bp.projection.starts_with(cp.projection)
                    && bp.projection[cp.projection.len()..].iter().all(|p| *p == ProjectionElem::Deref)
                    && cx.names.through(tcx, body, bp).is_none()))
        }).map(|(_, _, n)| n.clone()).collect();
        if runs.contains(&loc.block) {
            push(&mut evs, cx.ev(*loc, Fact::Borrow { loan: idx, lender: lender.clone(), kind, holder }));
        }
        loans.push(LoanInfo {
            issue: Some(*loc),
            loan: idx,
            line: cx.row_of(*loc),
            col: cx.rows.col.get(loc).copied().unwrap_or(0),
            lender,
            projected: cx.names.projected(tcx, body, bp),
            columns: cols,
            whole,
            direct_holder: cx.name(bd.assigned_place()),
            holds_ref_slot: bp.ty(body, tcx).ty.is_ref() && columns.iter().any(|(_, place, name)| {
                name == &direct && matches!(place.ty(body, tcx).ty.kind(), TyKind::Ref(_, target, _) if target.is_ref())
            }),
            runs: runs.contains(&loc.block),
            part_of,
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
            issue: u.at,
            loan: id,
            line,
            col,
            lender: u.lender.clone(),
            projected: u.place.and_then(|p| cx.names.projected(tcx, body, p)),
            columns: Vec::new(),
            whole: BTreeSet::new(),
            direct_holder: None,
            holds_ref_slot: false,
            runs: u.at.is_none_or(|loc| runs.contains(&loc.block)),
            part_of: Vec::new(),
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
    let mut loan_cleanups = BTreeSet::new();
    let mut terminal_ends = BTreeSet::new();
    // The CFG edges each loan ends on (for `HoldEnd`s below).
    let mut restored_reborrows = BTreeSet::new();
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
        let diverges = loc.statement_index == data.statements.len()
            && matches!(data.terminator().kind, TerminatorKind::Call { target: None, .. });
        // A missing normal target is not an end. Even a cleanup successor
        // can be fictitious (process termination). Follow the unwind edge
        // with just the loans that entered it at this particular call.
        let mut edges = Vec::new();
        if diverges {
            let TerminatorKind::Call { func, unwind: UnwindAction::Cleanup(start), .. } = &data.terminator().kind else { continue };
            if !tcx.sess.panic_strategy().unwinds() { continue; }
            // An indirect callee may store its argument in caller-owned
            // storage before it panics. Local temporary liveness does not
            // prove that the caller's loan ends on that edge.
            let Some((callee, _)) = func.const_fn_def() else { continue };
            if ["process_exit", "process_abort"].into_iter()
                .any(|item| tcx.is_diagnostic_item(rustc_span::Symbol::intern(item), callee)) { continue; }
            // Caller-origin loans may be passed into a direct callee too.
            // Its signature can let them escape, even when no local holds
            // them on the cleanup edge. A panic entry point only formats
            // the arguments and starts unwinding.
            use rustc_hir::LangItem::{BeginPanic, PanicDisplay, PanicFmt, Panic};
            if ![Panic, PanicFmt, PanicDisplay, BeginPanic].iter().any(|item| tcx.is_lang_item(callee, *item)) {
                here.retain(|b| b.index() < base || untracked[b.index() - base].param.is_none());
            }
            edges.push((loc, start.start_location()));
            for bb in cleanup_path(body, *start) {
                let block = &body[bb];
                for i in 0..block.statements.len() {
                    let at = Location { block: bb, statement_index: i };
                    edges.push((at, at.successor_within_block()));
                }
                if let TerminatorKind::Drop { target, .. } | TerminatorKind::Goto { target } = block.terminator().kind {
                    edges.push((body.terminator_loc(bb), target.start_location()));
                }
            }
        } else {
            edges.extend(cx.r.real_succs(loc).into_iter().filter(|s| runs.contains(&s.block)).map(|s| (loc, s)));
        }
        let call = loc;
        for (loc, successor) in edges {
            let data = &body[loc.block];
            inscope.seek_before_primary_effect(successor);
            let mut there: BTreeSet<_> = inscope.get().iter().collect();
            if let Some(t) = table {
                there.extend(untracked_at(t.start_index(successor)));
            }
            let restricted: BTreeSet<_> = loans.iter().filter(|l| there.contains(&BorrowIndex::from_usize(l.loan)))
                .flat_map(|l| l.through.iter().chain(&l.columns)).cloned().collect();
            for b in &here {
                if there.contains(b) {
                    continue;
                }
                let Some(li) = loans.iter_mut().find(|l| l.loan == b.index()) else { continue };
                // The next row of this path (past a block start that carries
                // the branch's span, past temporaries' drops).
                let to_line = if diverges { 0 } else { cx.r.next_row(&cx.rows, successor, row) };
                let to_col = if diverges { 0 } else { cx.rows.col.get(&successor).copied().unwrap_or(0) };
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
                {
                    let sdata = &body.basic_blocks[successor.block];
                    match sdata.statements.get(successor.statement_index).map(|st| &st.kind) {
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
                if li.through.as_ref().is_some_and(|r| !restricted.contains(r)) {
                    restored_reborrows.insert((loc, successor, b.index()));
                }
                ended_edges.insert((loc, successor, b.index()));
                if diverges && loc == call {
                    terminal_ends.insert((loc, b.index()));
                    why = "non-returning call".into();
                }
                let f = Fact::BorrowEnd { loan: b.index(), lender: li.lender.clone(), holder, why, to_line, to_col };
                let mut event = cx.ev(call, f);
                if loc != call {
                    loan_cleanups.insert(call);
                    event.1.site = Some(cx.rows.order(call));
                    event.1.location = loc;
                    event.1.order = cx.rows.order(loc);
                    event.1.order.2 = 1;
                    event.1.drop_route = DropRoute::Unwind;
                }
                event.1.end_successor = Some(successor);
                push(&mut evs, event);
            }
            if diverges {
                here.retain(|b| there.contains(b));
                if here.is_empty() { break; }
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
                ev.1.location = at;
                ev.1.display_location = at;
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
    let mut closure_result_loans = std::collections::HashMap::new();
    let mut receiver_loans = std::collections::HashMap::new();
    let mut argument_stores = std::collections::HashMap::<_, Vec<_>>::new();
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
        let mut callees = std::collections::HashMap::new();
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            let term = data.terminator();
            let TerminatorKind::Call { func, args, target: Some(return_to), .. } = &term.kind else { continue };
            if !runs.contains(&bb) || args.len() < 2 { continue; }
            let loc = Location { block: bb, statement_index: data.statements.len() };
            let callee = match *func.ty(&body.local_decls, tcx).kind() {
                TyKind::FnDef(def, _) if args.iter().any(|arg| arg.node.place()
                    .is_some_and(|p| decl_place_ty(tcx, body, p).is_mutable_ptr())) => Some(def),
                _ => None,
            };
            if let Some(def) = callee {
                let writes = callees.entry(def).or_insert_with(|| super::call_stores::argument_stores(tcx, def));
                for &(dst, src) in writes.iter() {
                    let operand = |index: usize| args.get(index).and_then(|a| a.node.place());
                    let (Some(dst), Some(source)) = (operand(dst), operand(src)) else { continue };
                    let target = resolve_write_deref(tcx, body, tcx.mk_place_deref(dst));
                    let Some(holder) = cx.name(target).filter(|_| cx.names.root(target)
                        .is_some_and(|(_, skip, _)| column_projection(body, target, skip))) else { continue; };
                    let kept = held_by_place(target, t.mid_index(loc));
                    let carried = held_by_place(source, t.start_index(loc));
                    let ids: BTreeSet<_> = kept.intersection(&carried).filter(|id| published.contains(id)).copied().collect();
                    if !ids.is_empty() {
                        argument_stores.entry((term.source_info.span.with_parent(None), src)).or_default().push((holder, ids));
                    }
                }
            }
            let Some((receiver, inputs)) = args.split_first() else { continue };
            let Some(p) = receiver.node.place() else { continue };
            let TyKind::Ref(_, inner, m) = decl_place_ty(tcx, body, p).kind() else { continue };
            if !m.is_mut() || inputs.is_empty() { continue; }
            let regions = regions_of(*inner);
            if regions.is_empty() { continue; }
            let dest = resolve_temp_deref(tcx, body, tcx.mk_place_deref(p));
            let Some(name) = cx.name(dest) else { continue };
            // The referent's origins at the call include its instantiated
            // lifetime constraints, even if the receiver is dead on return.
            let retained = held_in(&regions, t.mid_index(loc));
            let before = held_by_place(dest, t.start_index(loc));
            let after = held_by_place(dest, t.start_index(Location { block: *return_to, statement_index: 0 }));
            let mut stored = Vec::new();
            for (index, arg) in inputs.iter().enumerate() {
                let Some(place) = arg.node.place() else { continue };
                let ty = decl_place_ty(tcx, body, place);
                let carried = held_by_place(place, t.start_index(loc));
                let ids: Vec<_> = carried.intersection(&retained).copied()
                    .filter(|id| loans.iter().any(|l| l.loan == *id && matches!(l.source, "borrow" | "caller" | "untracked")))
                    .collect();
                // A direct borrow operand has a known target, even if that
                // target itself holds other loans (e.g. push(&x), x: &T).
                // A call result need not identify a single referent.
                let direct = |id: usize| {
                    if id < base {
                        bwf.borrow_set.location_map().get_index(id).is_some_and(|(_, bd)| bd.assigned_place() == place)
                    } else {
                        untracked.get(id - base).is_some_and(|u| u.assigned == place && u.field.is_none())
                    }
                };
                #[expect(clippy::disallowed_methods, reason = "only tests membership among MIR destinations")]
                let literal = cx.vec_results.values().any(|(_, result)| *result == place);
                let direct_operand = ty.is_ref() && ids.iter().any(|&id| direct(id));
                let shared = matches!(ty.kind(), TyKind::Ref(_, _, m) if !m.is_mut());
                for &id in &ids {
                    let exclusive = loans.iter().any(|l| l.loan == id && l.kind == "mut");
                    if direct_operand && !direct(id) && !exclusive { continue; }
                    let operand = if shared && (!direct_operand || exclusive) {
                        StoredOperand::SharedReference
                    } else if ty.is_mutable_ptr() && !inner.walk().filter_map(|a| a.as_type()).any(|t|
                        tcx.erase_and_anonymize_regions(t) == tcx.erase_and_anonymize_regions(ty)) {
                        StoredOperand::Carrier
                    } else if !ty.is_ref() && !ty.is_closure() && !literal &&
                        (!matches!(ty.kind(), TyKind::Array(..) | TyKind::Tuple(..)) || ids.len() > 1) {
                        // E.g. extend with a filtered iterator: the receiver
                        // keeps the loan even if no element was yielded.
                        StoredOperand::Carrier
                    } else {
                        StoredOperand::Direct
                    };
                    // A shared reborrow through a shared reference can outlive
                    // that binding. Follow the loan it actually points through.
                    let mut stack = vec![id];
                    let mut visited = BTreeSet::new();
                    while let Some(source) = stack.pop() {
                        if !visited.insert(source) { continue; }
                        let Some(l) = loans.iter().find(|l| l.loan == source) else { continue };
                        if l.source == "untracked" && l.through.is_some() && !l.made_through.is_empty() {
                            stack.extend(l.made_through.iter().copied());
                        } else if matches!(l.source, "borrow" | "caller" | "untracked") {
                            let operand = if shared && l.kind == "mut" { StoredOperand::SharedReference } else { operand };
                            stored.push(ReceiverLoan { loan: source, argument: index + 1, operand, newly_held: !before.contains(&id) && after.contains(&id) });
                        }
                    }
                }
            }
            if !stored.is_empty() {
                receiver_loans.insert((term.source_info.span.with_parent(None), name), stored);
            }
        }
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            if !runs.contains(&bb) { continue; }
            let term = data.terminator();
            let TerminatorKind::Call { args, destination, target: Some(return_to), .. } = &term.kind else { continue };
            let dest_ty = decl_place_ty(tcx, body, *destination);
            if dest_ty.is_ref() || dest_ty.is_closure() { continue; }
            let loc = Location { block: bb, statement_index: data.statements.len() };
            let successor = Location { block: *return_to, statement_index: 0 };
            let before = t.start_index(loc);
            let after = t.start_index(successor);
            let returned = held_by_place(*destination, after);
            if returned.is_empty() { continue; }
            let mut captured = BTreeSet::new();
            let mut ordinary = BTreeSet::new();
            for arg in args {
                let Some(operand) = arg.node.place() else { continue };
                let TyKind::Closure(def, args) = *decl_place_ty(tcx, body, operand).kind() else {
                    ordinary.extend(held_by_place(operand, before));
                    continue;
                };
                let output = args.as_closure().sig().skip_binder().output();
                let TyKind::Ref(region, ..) = *output.kind() else { continue };
                let Some(origin) = region_vid(region) else { continue };
                if dest_ty.walk().any(|a| a.as_type().is_some_and(|ty|
                    matches!(ty.kind(), TyKind::Closure(d, _) if *d == def))) { continue; }
                // The outer origin names the reference the closure can
                // return. For `&mut slot` it carries the loan of the slot,
                // while the inner origin carries the slot's own reference.
                captured.extend(held_in(&[Origin::from(origin)], before).intersection(&held_by_place(operand, before)).copied());
            }
            if captured.is_disjoint(&returned) { continue; }
            let (holder, arrival) = if cx.name(*destination).is_some() {
                (*destination, loc)
            } else {
                // An assignment can stage the result in a temporary while
                // dropping the old value. Follow the actual MIR store.
                let Some(store) = call_result_store(&cx, *destination, successor) else { continue };
                store
            };
            let (Some(root), Some(&row)) = (cx.name(holder), cx.rows.row.get(&arrival)) else { continue };
            let mut holders = BTreeMap::new();
            for (_, place, name) in columns.iter().chain(&synth) {
                if place.local != holder.local { continue; }
                let Some(fields) = place.projection.strip_prefix(&holder.projection[..]) else { continue };
                let Some(ty) = fields.iter().try_fold(dest_ty, |ty, field| {
                    let ProjectionElem::Field(index, _) = field else { return None };
                    match ty.kind() {
                        TyKind::Tuple(tys) => tys.get(index.index()).copied(),
                        TyKind::Adt(def, args) if def.is_struct() =>
                            def.non_enum_variant().fields.get(*index).map(|fd| fd.ty(tcx, args)),
                        _ => None,
                    }
                }) else { continue };
                // Consult this field's own origins. A field without origins
                // cannot carry the loans of a sibling field.
                let returned = held_in(&regions_of(ty), after);
                let loans: BTreeSet<_> = returned.intersection(&captured).filter(|id| published.contains(id)).copied().collect();
                if !loans.is_empty() {
                    let argument_loans = returned.intersection(&ordinary).filter(|id| published.contains(id)).copied().collect();
                    holders.insert(name.clone(), ClosureResultHolder { loans, argument_loans });
                }
            }
            if !holders.is_empty() {
                closure_result_loans.insert((term.source_info.span.with_parent(None), root), ClosureResultLoans { arrival, row, holders });
            }
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

    let source_slots = super::holder_path::HolderPath { tcx, body, runs: &runs };
    let iterators: Vec<_> = body.local_decls.indices().filter(|l| for_iter(*l) && !cx.names.locals.contains_key(l)).collect();
    let mut end_provenance = |loc: Location, bi: BorrowIndex, li: &LoanInfo, only: Option<Place<'tcx>>| {
        let caller_slot = bi.index().checked_sub(base).and_then(|id| untracked[id].param).and_then(|param| {
            // A mutable binding may already contain a different reference.
            if body.local_decls[param].mutability.is_mut() { return None; }
            let mpi = move_data.rev_lookup.find_local(param)?;
            (flow.state(&[loc], false, mpi).word() == "own")
                .then(|| EndPlace { local: param.index(), name: cx.names.locals[&param].clone() })
        });
        let mut places = BTreeSet::new();
        let mut transfers = BTreeSet::new();
        if let Some(t) = table {
            let issued = if bi.index() < base {
                bwf.borrow_set.location_map().get_index(bi.index()).map(|(at, _)| *at)
            } else {
                untracked[bi.index() - base].at
            };
            let tested = only.is_none().then(|| source_slots.discriminant_input(loc)).flatten();
            // Stores through references can leave a loan in a container
            // that was never a direct assignment target. Its own origin
            // must still carry this loan at the discriminant or its input.
            let can_name = |n: &String| {
                li.direct_holders.contains(n) || li.via_call.contains(n) || tested.is_some() && li.holders.contains(n)
            };
            let named = |p: Place<'tcx>| {
                if iterators.contains(&p.local) && p.projection.is_empty() { return Some(EndPlace { local: p.local.index(), name: "<iterator>".into() }); }
                let name = cx.name(p)?;
                can_name(&name)
                    .then_some(EndPlace { local: p.local.index(), name })
            };
            let carries = |p, at| held_by_place(p, t.start_index(at)).contains(&bi.index());
            let mut record = |mut path: Vec<HolderStep>| {
                if let Some(last) = path.first() { places.insert(last.place.clone()); }
                if path.len() > 1 {
                    path.reverse();
                    transfers.insert(path);
                }
            };
            let candidates = columns.iter().chain(&synth)
                .filter(|(_,_,name)| can_name(name)).map(|(_,p,_)|*p)
                .chain(iterators.iter().map(|l| Place::from(*l)));
            for place in candidates {
                if only.is_some_and(|p| p != place) { continue; }
                let start = carries(place, loc);
                let mid = held_by_place(place, t.mid_index(loc)).contains(&bi.index());
                if !start && (!mid || loc.statement_index == body[loc.block].statements.len()) { continue; }
                let before = if start { loc } else { loc.successor_within_block() };
                record(source_slots.trace(before, place, issued, &named, &carries));
            }
            if let Some((read, input)) = tested {
                if carries(input, read) { record(source_slots.trace(read, input, issued, &named, &carries)); }
            }
            // Calls consume argument temporaries. Follow the value actually
            // passed there, even if its last named holder is no longer live.
            for arg in source_slots.call_args(loc).filter(|p| only.is_none() && carries(*p, loc)) {
                record(source_slots.trace(loc, arg, issued, &named, &carries));
            }
            // The loop binding represents its hidden iterator in the drawing.
            // It can stand for that iterator on an exit only if its value
            // still exists and its assignment came from this same iterator.
            if only.is_none() && places.iter().any(|p| p.name == "<iterator>") {
                for (mpi, p, n) in &columns {
                    if !li.via_call.contains(n) || flow.state(&[loc], false, *mpi).word() != "own" { continue; }
                    let mut chain = source_slots.trace(loc, *p, issued, &named, &carries);
                    if chain.iter().skip(1).any(|s| s.place.name == "<iterator>" && places.contains(&s.place)) {
                        chain.reverse();
                        transfers.insert(chain);
                    }
                }
            }
        }
        (places.into_iter().collect::<Vec<_>>(), transfers.into_iter().collect::<Vec<_>>(), caller_slot)
    };
    // Direct holders and projected origins are now known. Resolve each
    // edge at its MIR location, including call argument temporaries.
    let mut drop_holders = BTreeMap::<usize, BTreeSet<String>>::new();
    for ev in evs.values_mut().flatten() {
        let Fact::BorrowEnd { loan, holder, why, .. } = &mut ev.fact else { continue };
        let Some(li) = loans.iter().find(|li| li.loan == *loan) else { continue };
        let (mut places, paths, caller_slot) = end_provenance(ev.location, BorrowIndex::from_usize(*loan), li, None);
        if places.is_empty() && unused.contains(loan) {
            if let Some(param) = untracked[*loan - base].param {
                places.push(EndPlace { local: param.index(), name: cx.names.locals[&param].clone() });
            }
        }
        if why == "non-returning call" {
            *holder = if places.is_empty() { "<temp>".into() }
                else { places.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join("|") };
        }
        let at = ev.location;
        if let TerminatorKind::Drop { place, .. } = body[at.block].terminator().kind {
            let exit = ev.drop_route == DropRoute::Unwind || cx.rows.early_exit.contains_key(&at)
                || cx.rows.shared_exits.contains_key(&at);
            if exit && at == body.terminator_loc(at.block) && place.projection.is_empty()
                && place.ty(&body.local_decls, tcx).ty.ty_adt_def().is_some_and(|adt| adt.has_dtor(tcx))
            {
                drop_holders.entry(*loan).or_default().extend(
                    places.iter().filter(|p| p.local == place.local.index()).map(|p| p.name.clone())
                );
            }
        }
        ev.caller_slot = caller_slot;
        ev.end_places = places;
        ev.transfer_paths = paths;
    }
    // ---- per-holder ends ---------------------------------------------------------
    // Where a direct or via-call holder stops holding a loan that goes on.
    if let Some(t) = table {
        // The place a holder's name is (`h.0`, `h.inner.r`): its own field,
        // so `h.0` and `h.1` each have their own end.
        let place_of = |local: Local, fields: &str| -> Option<Place<'tcx>> {
            let mut place = Place::from(local);
            for seg in fields.split_terminator('.') {
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
                    if terminal_ends.contains(&(x, loan)) { continue; }
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
            let holders = l.direct_holders.iter().chain(l.via_call.iter()).filter(|h| seen.insert(*h));
            for (h, hp) in holders.flat_map(|h| {
                let (root, fields) = h.split_once('.').unwrap_or((h, ""));
                cx.names.columns.get(root).into_iter().flatten().filter_map(move |local| Some((h, place_of(*local, fields)?)))
            }) {
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
                        f.1.end_successor = Some(s);
                        let (_, paths, _) = end_provenance(loc, bi, l, Some(hp));
                        f.1.end_places.push(EndPlace { local: hp.local.index(), name: h.clone() });
                        f.1.transfer_paths = paths;
                        f.1.order.2 = 1;
                        if ends_on_row(s, row, l.loan) {
                            inline_holder_ends.push(f);
                        } else {
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
    let issued: BTreeMap<_, _> = loans.iter().filter_map(|l| Some((taken_at(l.loan)?, l.loan))).collect();
    let doms = body.basic_blocks.dominators();
    let mut issued_operand_loans = BTreeSet::new();
    let mut returned_operand_loans = BTreeSet::new();
    let mut aggregate_loans = std::collections::HashMap::new();
    // A literal stores what its fields' operands hold (`(a, a)` puts a's
    // loan in `h.0` and `h.1`; `R { r: s }` s's).
    if let Some(t) = table {
        for (&loc, _) in &cx.rows.row {
            let Some(st) = body.basic_blocks[loc.block].statements.get(loc.statement_index) else { continue };
            let StatementKind::Assign(asg) = &st.kind else { continue };
            let (operands, array) = match &asg.1 {
                Rvalue::Aggregate(k, ops) if matches!(&**k, AggregateKind::Adt(..) | AggregateKind::Tuple | AggregateKind::Array(_)) =>
                    (ops.raw.as_slice(), matches!(&**k, AggregateKind::Array(_))),
                Rvalue::Repeat(o, _) => (std::slice::from_ref(o), true),
                _ => continue,
            };
            if runs.contains(&loc.block) {
                let mut ids = Vec::new();
                let carried = |p, at| {
                    let held = held_by_place(p, t.start_index(at));
                    let name = cx.name(p);
                    // A whole operand can carry its fields' loans. Compare MIR
                    // projections before resolving a holder to its published column.
                    let in_operand = |h: &String| name.as_ref() == Some(h) || columns.iter().chain(&synth).any(|(_, field, label)|
                        label == h && field.local == p.local && field.projection.starts_with(p.projection));
                    loans.iter().filter(|l| l.runs && held.contains(&l.loan) && (name.is_none() || l.direct_holders.iter().chain(&l.via_call).any(in_operand))
                        && !loans.iter().any(|child| held.contains(&child.loan) && child.made_through.contains(&l.loan)))
                        .map(|l| l.loan).collect()
                };
                for operand in operands {
                    cx.operand_loans(operand, loc, &issued, &runs, &carried, &mut ids);
                }
                let span = st.source_info.span;
                let key = if array && cx.vec_results.contains_key(&span.ctxt().outer_expn()) {
                    span.source_callsite()
                } else { span }.with_parent(None);
                let fresh = ids.iter().filter(|b| b.issued).map(|b| b.loan);
                issued_operand_loans.extend(fresh.clone());
                if asg.0.as_local() == Some(RETURN_PLACE) {
                    returned_operand_loans.extend(fresh);
                }
                for operand in &mut ids {
                    operand.conditional |= taken_at(operand.loan).is_some_and(|at| !doms.dominates(at.block, loc.block));
                }
                aggregate_loans.entry(key).or_insert_with(Vec::new).extend(ids);
            }
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
    // What a `Drop` of a place does where it runs (`drop_how`), except that
    // a place some of whose parts were moved out drops nothing when nothing
    // left in it needs drop: drop elaboration drops what is left field by
    // field (nested fields too), and a field that needs no drop, moved on
    // some paths or not, adds none (`(bool, String, String)` with both
    // Strings moved, `((String, bool), bool)` after `t.0.0` moved: no drop
    // of `t`).
    let rest_how = |rt: &mut Flow<'_, '_, 'tcx>, at: &[Location], after: bool, top: MovePathIndex| -> &'static str {
        let how = drop_how(&rt.state(at, after, top));
        if how != "drops_rest" {
            return how;
        }
        let mut work = vec![top];
        while let Some(m) = work.pop() {
            let mty = rt.md.move_paths[m].place.ty(&body.local_decls, tcx).ty;
            if !ty_info(mty).1 {
                continue;
            }
            let st = rt.state(at, after, m);
            match (st.may_init, st.may_uninit) {
                // Moved out on every path: nothing in it left.
                (false, _) => continue,
                // Moved on some paths only: dropped behind a flag.
                (true, true) => return how,
                (true, false) => {}
            }
            if st.sub_moved.is_empty() && st.sub_maybe.is_empty() {
                return how;
            }
            // (A Box frees its allocation; an enum's or an array's parts
            // aren't followed here: something is dropped.)
            let fields: Vec<rustc_middle::ty::Ty<'tcx>> = match mty.kind() {
                TyKind::Tuple(tys) => tys.iter().collect(),
                TyKind::Adt(def, args) if def.is_struct() && !mty.is_box() => def.non_enum_variant().fields.iter().map(|fd| fd.ty(tcx, args)).collect(),
                _ => return how,
            };
            let mut child: BTreeMap<usize, MovePathIndex> = BTreeMap::new();
            let mut c = rt.md.move_paths[m].first_child;
            while let Some(ch) = c {
                match rt.md.move_paths[ch].place.projection.last() {
                    Some(ProjectionElem::Field(f, _)) => {
                        child.insert(f.index(), ch);
                    }
                    _ => return how,
                }
                c = rt.md.move_paths[ch].next_sibling;
            }
            for (i, fty) in fields.iter().enumerate() {
                match child.get(&i) {
                    Some(ch) => work.push(*ch),
                    // (No move path: it holds a value wherever its parent does.)
                    None if ty_info(*fty).1 => return how,
                    None => {}
                }
            }
        }
        "nothing_to_drop"
    };
    // ---- moves, copies, inits, re-points, drops, scope ends ---------------------
    // A shared destructor's source span is still the closing brace. In
    // particular two arms of a break value can reach the same destructor
    // without the loop's continuing edge ever reaching it (#1794).
    let drop_route = |loc: Location, exits: &[(usize, Location)]| {
        if exits.is_empty() { return DropRoute::Normal; }
        let mut seen: BTreeSet<_> = exits.iter().map(|(_, at)| *at).collect();
        let mut work = vec![loc];
        while let Some(at) = work.pop() {
            if !seen.insert(at) { continue; }
            if at == Location::START { return DropRoute::Shared; }
            if at.statement_index > 0 {
                work.push(Location { statement_index: at.statement_index - 1, ..at });
            } else {
                work.extend(rt_body.basic_blocks.predecessors()[at.block].iter()
                    .filter(|p| runs.contains(p) && !body.basic_blocks[**p].is_cleanup)
                    .map(|p| body.terminator_loc(*p)));
            }
        }
        DropRoute::ExitOnly
    };
    let mut binding_sources = Vec::new();
    // Resolve fields of a temporary tuple through the operands that built
    // it. Follow only single definitions, including MIR's staging locals.
    let mut definitions = BTreeMap::new();
    for data in body.basic_blocks.iter() {
        for st in &data.statements {
            if let StatementKind::Assign(asg) = &st.kind {
                if !cx.names.locals.contains_key(&asg.0.local) {
                    definitions.entry(asg.0.local).and_modify(|def| *def = None)
                        .or_insert(asg.0.projection.is_empty().then_some(&asg.1));
                }
            }
        }
        if let TerminatorKind::Call { destination, .. } = data.terminator().kind {
            definitions.insert(destination.local, None);
        }
    }
    let source_place = |mut p: Place<'tcx>| {
        let mut seen = BTreeSet::new();
        while !cx.is_user(p) && seen.insert(p.local) {
            let (op, suffix) = match definitions.get(&p.local).copied().flatten()? {
                Rvalue::Use(op) => (op, &p.projection[..]),
                Rvalue::Aggregate(kind, ops) if matches!(**kind, AggregateKind::Tuple) => {
                    let (ProjectionElem::Field(f, _), rest) = p.projection.split_first()? else { return None };
                    (ops.get(*f)?, rest)
                }
                _ => return None,
            };
            p = op.place()?.project_deeper(suffix, tcx);
        }
        cx.is_user(p).then_some(p)
    };
    let mut temps: BTreeMap<Local, TempInfo> = BTreeMap::new();
    let vec_buffers: std::collections::HashSet<_> = body.basic_blocks.iter().filter_map(|b| cx.r.vec_buffer(b.terminator())).collect();
    for (&loc, _) in &cx.rows.row {
        let data = &body.basic_blocks[loc.block];
        let statement = data.statements.get(loc.statement_index);
        if let Some((st, asg)) = statement.and_then(|st| match &st.kind {
            StatementKind::Assign(asg) => Some((st, asg)), _ => None,
        }) {
            let (lhs, rv) = &**asg;
            if let Some(into) = cx.names.locals.get(&lhs.local).filter(|_| lhs.projection.is_empty()) {
                let source = match rv {
                    Rvalue::Use(op) => op.place().and_then(source_place),
                    Rvalue::Ref(_, _, p) => source_place(*p),
                    _ => None,
                };
                let from = source.filter(|p| column_projection(body, *p, 0)).and_then(|p| cx.name(p));
                let mut parents = Vec::new();
                let mut covers = Vec::new();
                // An enum payload may have no column of its own. Its move
                // still takes part of the enum and leaves unrelated places alone.
                if let Some(p) = source.filter(|p| from.is_some() || p.projection.iter()
                    .all(|e| matches!(e, ProjectionElem::Field(..) | ProjectionElem::Downcast(..)))) {
                    covers.extend(cx.name(p));
                    for (_, col, name) in columns.iter().chain(&synth).filter(|(_, col, _)| col.local == p.local) {
                        if p.projection.len() > col.projection.len() && p.projection.starts_with(col.projection) {
                            parents.push(name.clone());
                        } else if col.projection.len() > p.projection.len() && col.projection.starts_with(p.projection) {
                            covers.push(name.clone());
                        }
                    }
                }
                let array_element = source.and_then(|p| {
                    let root = cx.names.locals.get(&p.local)?;
                    let label = crate::mir_data::array_operand_name(tcx, body, p, root.clone())?;
                    Some((p.local, label))
                });
                binding_sources.push(BindingSource {
                    array_element,
                    parents, covers, moves: matches!(rv, Rvalue::Use(Operand::Move(p)) if !ty_info(p.ty(&body.local_decls, tcx).ty).0),
                    location: loc, runs: runs.contains(&loc.block),
                    declaration: lhs.local,
                    site: st.source_info.span, into: into.clone(), from,
                });
            }
        }
        if !runs.contains(&loc.block) { continue; }
        if let Some(st) = statement {
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
                                    push(&mut evs, cx.place_ev(loc, p.local, Fact::Move { path, to, in_macro: in_macro.clone() }));
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
                                    push(&mut evs, cx.place_ev(loc, p.local, Fact::Move { path, to, in_macro: in_macro.clone() }));
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
                                let f = Fact::Repoint { reaching: None, holder: path.clone(), old: held(t.start_index(loc)), new: held(t.mid_index(loc)) };
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
                            push(&mut evs, cx.ev(loc, Fact::Repoint { reaching: None, holder: path.clone(), old, new: this.1.clone() }));
                        }
                        if !reassign && lhs.projection.is_empty() && body.local_decls[lhs.local].ty.is_ref() {
                            if let Some(t) = table {
                                let ids = held_by(lhs.local, t.mid_index(loc)).into_iter()
                                    .filter(|l| published.contains(l)).collect();
                                binding_loans.insert(loc, ids);
                            }
                        }
                        push(&mut evs, cx.place_ev(loc, lhs.local, Fact::Init { path, from, reassign, maybe_had }));
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
                        push(&mut evs, cx.place_ev(loc, lhs.local, Fact::Init { path, from, reassign: true, maybe_had: false }));
                    }
                }
                StatementKind::StorageDead(l) if cx.names.locals.contains_key(l) => {
                    let (copy, needs_drop, _) = ty_info(body.local_decls[*l].ty);
                    if !needs_drop {
                        let note = if copy { "copy: nothing to drop" } else { "no drop glue" };
                        push(&mut evs, cx.place_ev(loc, *l, Fact::ScopeEnd { path: cx.names.locals[l].clone(), note }));
                    }
                }
                _ => {}
            }
            continue;
        }
        match &data.terminator().kind {
            TerminatorKind::Call { args, destination, fn_span, func, unwind, .. } => {
                let f = cx.call_name(*fn_span, func, loc);
                let in_macro = cx.macro_of(*fn_span);
                for a in args.iter() {
                    match &a.node {
                        Operand::Move(p) if cx.is_user(*p) => {
                            let path = cx.name(*p).unwrap_or_default();
                            push(&mut evs, cx.place_ev(loc, p.local, Fact::Move { path, to: format!("{}()", f), in_macro: in_macro.clone() }));
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
                    push(&mut evs, cx.place_ev(loc, destination.local, Fact::Init { path, from: format!("{}()", f), reassign, maybe_had }));
                }
                // The caller still has a normal return edge for a closure
                // with a written result type. Keep its borrowck state and
                // record the actual cleanup on the call's own row (Q2).
                let UnwindAction::Cleanup(start) = *unwind else { continue };
                if !tcx.sess.panic_strategy().unwinds()
                    || skipped_closure_returns.contains_key(&loc.block) { continue; }
                let loan_cleanup = loan_cleanups.contains(&loc);
                if !loan_cleanup && vec_buffers.is_empty() { continue; }
                let drops = cleanup_drops(body, start);
                if !loan_cleanup {
                    let Some(closure) = called_closure(tcx, func) else { continue };
                    if closure_can_return(tcx, closure) || !closure_has_panic_route(tcx, closure) { continue; }
                    if !drops.iter().any(|(_, place)| vec_buffers.contains(place)
                        && matches!(rt_md.rev_lookup.find(place.as_ref()), LookupResult::Exact(mpi)
                            if rt.state(&[loc], false, mpi).may_init)) { continue; }
                }
                // Read init at this call, before other unwind edges
                // merge into the cleanup block. Moves at the call and
                // earlier drops of the same local would change it.
                let mut touched: BTreeSet<_> = rt_md.loc_map[loc].iter()
                    .map(|m| rt_md.move_paths[rt_md.moves[*m].path].place.local).collect();
                for (at, place) in drops {
                    if !touched.insert(place.local) || !place.projection.is_empty() { continue; }
                    let Some(path) = cx.name(place) else { continue };
                    if !ty_info(place.ty(&body.local_decls, tcx).ty).1 { continue; }
                    let LookupResult::Exact(mpi) = rt_md.rev_lookup.find(place.as_ref()) else { continue };
                    let how = rest_how(&mut rt, &[loc], false, mpi);
                    if how == "nothing_to_drop" { continue; }
                    let mut ev = cx.ev(loc, Fact::Drop { path, how, at_reassign: false, early_exit: None, also_on_exits: Vec::new() });
                    ev.1.drop_route = DropRoute::Unwind;
                    ev.1.site = Some(ev.1.order);
                    ev.1.order = cx.rows.order(at);
                    push(&mut evs, ev);
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
                        LookupResult::Exact(mpi) => rest_how(&mut rt, &[loc], false, mpi),
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
                            let how_there = rest_how(&mut rt, &[*at], true, mpi);
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
                                let mut ev = cx.place_ev(loc, place.local, Fact::Drop { path: n, how: h, at_reassign: false, early_exit: Some(*r), also_on_exits: Vec::new() });
                                ev.0 = *r;
                                ev.1.col = cx.rows.col.get(at).copied().unwrap_or(0);
                                ev.1.arm = cx.rows.arm_of(*at);
                                // In the order MIR runs it on that path: after
                                // the exit's own drops of inner scopes (`t`
                                // before `s` at `return`), which come before it
                                // in the CFG; one copy per exit site.
                                ev.1.order = cx.rows.order(loc);
                                ev.1.display_location = *at;
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
                        let route = drop_route(loc, &shared);
                        for (n, h, also) in items {
                            let mut ev = cx.place_ev(loc, place.local, Fact::Drop { path: n, how: h, at_reassign, early_exit, also_on_exits: also });
                            ev.1.drop_route = route;
                            push(&mut evs, ev);
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
                            if rest_how(&mut rt, &[*at], true, mpi) == "nothing_to_drop" {
                                continue;
                            }
                            let mut ev = cx.ev(loc, Fact::TempDrop { what: what.clone(), created_line, extended, early_exit: Some(*r) });
                            ev.0 = *r;
                            ev.1.col = cx.rows.col.get(at).copied().unwrap_or(0);
                            ev.1.arm = cx.rows.arm_of(*at);
                            ev.1.display_location = *at;
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
    // Follow incoming paths backwards to the reservation sites. If a
    // path gets to the body entry without crossing one of this set of
    // live loans, the column is only possibly borrowed there. Checking
    // the set together handles arms that each borrow the same column.
    let issued: BTreeMap<_, _> = bwf.borrow_set.location_map().iter().enumerate().map(|(i, (loc, _))| (*loc, i)).collect();
    let option_places: Vec<_> = columns.iter().chain(&synth).filter_map(|(_, place, name)| {
        let ty = place.ty(&body.local_decls, tcx).ty;
        let region = option_ref_region(ty)?;
        Some((*place, name, ty_info(ty).0, region_vid(region).map(Origin::from)))
    }).collect();
    let option_columns: BTreeSet<&str> = option_places.iter().map(|(_, name, _, _)| name.as_str()).collect();
    let option_copy = |place| option_places.iter().find(|(p, _, _, _)| *p == place).map(|(_, _, copy, _)| *copy);
    let mut uniform_option_refs = BTreeMap::new();
    for (_, place, name) in columns.iter().chain(&synth).filter(|(_, _, name)| option_columns.contains(name.as_str())) {
        let copy = option_copy(*place);
        uniform_option_refs.entry(name.clone()).and_modify(|previous| {
            if *previous != copy { *previous = None; }
        }).or_insert(copy);
    }
    let uniform_option_refs = uniform_option_refs.into_iter().filter_map(|(name, copy)| Some((name, copy?))).collect();
    let option_borrows: Vec<_> = bwf.borrow_set.location_map().iter().enumerate().filter_map(|(id, (loc, borrow))| {
        let (place, name, _, origin) = option_places.iter().find(|(p, _, _, _)| *p == borrow.borrowed_place())?;
        Some((id, *loc, place.local, *name, (*origin)?))
    }).collect();
    let option_refs: Vec<_> = cx.names.locals.keys().filter_map(|&local| {
        let TyKind::Ref(outer, inner, _) = body.local_decls[local].ty.kind() else { return None };
        let inner = option_ref_region(*inner)?;
        Some((local, Origin::from(region_vid(*outer)?), Origin::from(region_vid(inner)?)))
    }).collect();
    let option_fields: Vec<_> = option_places.iter().filter_map(|(p, name, _, origin)| {
        (!p.projection.is_empty()).then_some((p.local, *name, (*origin)?))
    })
        .collect();
    // (Only a path that never takes them, from the fn's entry: giving
    // their holder another value proves neither that the loan ended, `r =
    // r` keeps it, nor that a reborrow made from it did.)
    // A dead-else path can run backwards through borrowck-only blocks.
    // Once it reaches the executable CFG, use its ordinary predecessors.
    let can_walk_back = |loc: Location, imaginary: bool| {
        !body.basic_blocks[loc.block].is_cleanup && (imaginary || runs.contains(&loc.block))
    };
    let predecessors = |loc: Location| {
        if runs.contains(&loc.block) {
            &rt_body.basic_blocks.predecessors()[loc.block]
        } else {
            &body.basic_blocks.predecessors()[loc.block]
        }
    };
    let may_skip = |locations: &[Location], after: bool, ids: &BTreeSet<usize>, imaginary: bool| {
        let mut work: Vec<_> = locations.iter().map(|l| (*l, after)).collect();
        let mut seen = BTreeSet::new();
        while let Some((loc, inclusive)) = work.pop() {
            if !can_walk_back(loc, imaginary) || !seen.insert((loc, inclusive)) {
                continue;
            }
            if inclusive && issued.get(&loc).is_some_and(|id| ids.contains(id)) {
                continue;
            }
            if loc.statement_index > 0 {
                work.push((Location { statement_index: loc.statement_index - 1, ..loc }, true));
            } else if loc.block == rustc_middle::mir::START_BLOCK {
                return true;
            } else {
                work.extend(predecessors(loc).iter().map(|&bb| (body.terminator_loc(bb), true)));
            }
        }
        false
    };
    // Stop at the first write or the end of this incarnation of the
    // local. A later loop iteration can declare the same MIR local again.
    let next_assignment = |place: Place<'tcx>, exits: &[Location], after: bool| {
        let mut work = if after {
            exits.iter().flat_map(|l| cx.r.real_succs(*l)).collect()
        } else { exits.to_vec() };
        let mut seen = BTreeSet::new();
        let writes = |dest: Place<'tcx>| dest.local == place.local && place.projection.starts_with(dest.projection);
        let (mut assigned, mut ended) = (false, false);
        while let Some(l) = work.pop() {
            if !seen.insert(l) { continue; }
            let data = &body.basic_blocks[l.block];
            if let Some(st) = data.statements.get(l.statement_index) {
                match &st.kind {
                    StatementKind::Assign(a) if writes(a.0) => { assigned = true; continue; }
                    StatementKind::StorageDead(v) if *v == place.local => { ended = true; continue; }
                    _ => {}
                }
            } else if matches!(data.terminator().kind, TerminatorKind::Call { destination, target: Some(_), .. } if writes(destination)) {
                assigned = true;
                continue;
            }
            let succs = cx.r.real_succs(l);
            // A diverging call (including panic) has no normal successor.
            if succs.is_empty() {
                ended = true;
            }
            work.extend(succs);
        }
        assigned.then_some(!ended)
    };
    let take_calls: BTreeSet<_> = runs.iter().copied().filter(|&bb| {
        let data = &body.basic_blocks[bb];
        !data.is_cleanup && matches!(&data.terminator().kind, TerminatorKind::Call { func, target: Some(_), .. }
            if manually_drop_take(tcx, body, func))
    }).map(|bb| body.terminator_loc(bb)).collect();
    let manually_taken = manually_taken(&cx, &take_calls);
    let mut state_at = |exits: &[Location], after: bool, imaginary: bool| -> State {
        let mut st = State::default();
        let mut active = BTreeMap::new();
        let mut declared = |local| *active.entry(local).or_insert_with(|| {
            exits.iter().any(|e| {
                if after { storage.seek_after_primary_effect(*e); } else { storage.seek_before_primary_effect(*e); }
                storage.get().contains(local)
            })
        });
        let mut column_locals = BTreeMap::new();
        let mut holder_types = BTreeMap::new();
        let mut record_type = |name: &str, place: Place<'tcx>| {
            column_locals.entry(name.to_owned()).and_modify(|prev| {
                if *prev != Some(place.local) { *prev = None; }
            }).or_insert(Some(place.local));
            if !option_columns.contains(name) { return; }
            let holder_type = option_copy(place)
                .map(|copy| HolderType::OptionRef { local: place.local.index(), copy })
                .or_else(|| place.ty(&body.local_decls, tcx).ty.is_primitive().then_some(HolderType::Primitive));
            // Sibling bindings can share a column. Only describe a type
            // when the active places agree on the binding.
            holder_types.entry(name.to_owned()).and_modify(|prev| {
                if *prev != holder_type { *prev = None; }
            }).or_insert(holder_type);
        };
        // The row's exits on code that runs (an arm an earlier `_` arm
        // shadows is reached only by borrowck's imaginary edges): what runs
        // is read there only.
        let rt_exits: Vec<Location> = exits.iter().copied().filter(|e| runs.contains(&e.block)).collect();
        for (mpi, place, name) in &columns {
            let s = flow.state(exits, after, *mpi);
            let rt_mpi = match rt_md.rev_lookup.find(place.as_ref()) {
                LookupResult::Exact(rm) if !rt_exits.is_empty() => Some(rm),
                _ => None,
            };
            let rt_here = rt_mpi.map(|rm| rt.state(&rt_exits, after, rm));
            // Moved out (or, for `unassigned`, never given a value) on some
            // path that runs, by the runtime CFG's own move / assignment
            // dataflows.
            let rt_may = |entry: &BTreeMap<rustc_middle::mir::BasicBlock, BTreeSet<MovePathIndex>>, unassigned: bool| {
                rt_mpi.is_some_and(|rm| {
                    let (anc, desc) = (ancestors(&rt_md, rm), descendants(&rt_md, rm));
                    rt_exits.iter().any(|e| {
                        let set = if unassigned {
                            may_after(entry, &|st, l| unassigned_transfer(&rt_md, &rt_body, st, l), *e, after)
                        } else {
                            may_after(entry, &|st, l| moved_transfer(&rt_md, &rt_body, st, l), *e, after)
                        };
                        if unassigned { set.contains(&rm) } else { set.iter().any(|m| anc.contains(m) || desc.contains(m)) }
                    })
                })
            };
            // What runs, for the words (Q5 refined): moved on every real
            // execution, though borrowck's imaginary edges skip the move (an
            // or-pattern's second alternative after an irrefutable first,
            // `(x, _) | (_, x)`), reads "moved". (Uninitialized on every path
            // that runs is not enough: on one of them it may never have been
            // given a value, and then nothing was moved.)
            // Uninitialized on every path, given a value on some of them
            // (borrowck's own "moved"), but never given one on another: not
            // moved on every path (`let x; if c { x = ..; drop(x); }` after
            // the `if`): maybe (moved).
            let never_assigned_somewhere = || {
                exits.iter().any(|e| {
                    may_after(&unassigned_entry, &|st, l| unassigned_transfer(&move_data, body, st, l), *e, after).contains(mpi)
                })
            };
            let w = match s.word() {
                "moved" if never_assigned_somewhere() => "maybe",
                "maybe"
                    if rt_here.as_ref().is_some_and(|r| r.may_uninit && !r.may_init && r.ever_init)
                        && !rt_may(&rt_unassigned_entry, true) =>
                {
                    "moved"
                }
                w => w,
            };
            // A variable not assigned yet (and its fields): no column yet.
            let mut root_unassigned = || {
                columns.iter().any(|(m, rp, _)| {
                    rp.local == place.local && rp.projection.len() < place.projection.len() && flow.state(exits, after, *m).word() == "unassigned"
                })
            };
            // A variable is a column from its declaration (`let r;`: storage
            // live, no value yet) to its scope's end.
            // Out of its scope (its storage dead): no column, whatever its
            // fields' history says.
            if !declared(place.local) {
                continue;
            }
            if !rt_exits.is_empty() && place.ty(&body.local_decls, tcx).ty.needs_drop(tcx, typing_env) {
                if let Some(all) = next_assignment(*place, &rt_exits, after) {
                    st.reassigned.insert(name.clone(), all);
                }
            }
            if w == "unassigned" && !place.projection.is_empty() && root_unassigned() {
                continue;
            }
            st.state.insert(name.clone(), w);
            record_type(name, *place);
            if w == "maybe" {
                // Moved on some way here, or only never assigned?
                let moved = exits.iter().any(|e| {
                    let mut set = moved_entry.get(&e.block).cloned().unwrap_or_default();
                    for i in 0..e.statement_index + usize::from(after) {
                        moved_transfer(&move_data, body, &mut set, Location { block: e.block, statement_index: i });
                    }
                    let (anc, desc) = (ancestors(&move_data, *mpi), descendants(&move_data, *mpi));
                    set.iter().any(|m| anc.contains(m) || desc.contains(m))
                });
                // Moved only on borrowck's imaginary edges (the unreachable
                // alternative), never on a path that runs: it still can't be
                // used, but nothing was moved. (A maybe that is only "never
                // assigned" on an imaginary path is `unassigned`: E0381.)
                let imaginary = moved && rt_mpi.is_some() && !rt_may(&rt_moved_entry, false);
                // Never moved at all (borrowck's maybe is "never assigned") but
                // assigned on every path that runs: the unreachable path skips it.
                if !moved && rt_here.as_ref().is_some_and(|r| r.may_init && !r.may_uninit) {
                    st.always_assigned.insert(name.clone());
                }
                st.cause.insert(name.clone(), if imaginary { "imaginary" } else if moved { "moved" } else { "unassigned" });
            }
            if (w == "partly" || w == "own") && (!s.sub_moved.is_empty() || !s.sub_maybe.is_empty()) {
                // A part borrowck counts as moved on some paths only is moved
                // for sure when only its imaginary edges skip the move (an
                // irrefutable `if let (p, q) = pair && ..`'s `else`): which
                // parts are gone is what runs.
                let mut surely_gone = |m: MovePathIndex| match rt_md.rev_lookup.find(move_data.move_paths[m].place.as_ref()) {
                    LookupResult::Exact(rm) if !rt_exits.is_empty() => {
                        let r = rt.state(&rt_exits, after, rm);
                        r.may_uninit && !r.may_init
                    }
                    _ => false,
                };
                let (rt_moved, still_maybe): (Vec<MovePathIndex>, Vec<MovePathIndex>) =
                    s.sub_maybe.iter().copied().partition(|m| surely_gone(*m));
                // Constant-index move paths from array patterns (including
                // a rest pattern) have no field column. Name a direct
                // element from its MIR projection and the array length.
                let part_names = |m: &MovePathIndex| -> Vec<String> {
                    if let Some(n) = column_name(*m) { return vec![n]; }
                    let mp = move_data.move_paths[*m].place;
                    let Some(n) = array_len(*place).filter(|&n| n <= super::schema::MAX_NAMED_ARRAY_LEN) else { return vec![] };
                    if mp.local != place.local || !mp.projection.starts_with(place.projection) { return vec![]; }
                    let range = match mp.projection[place.projection.len()..] {
                        [ProjectionElem::ConstantIndex { offset, from_end, .. }] => {
                            let i = if from_end { n - offset as usize } else { offset as usize };
                            i..i+1
                        }
                        [ProjectionElem::Subslice { from, to, from_end }] => {
                            from as usize..if from_end { n - to as usize } else { to as usize }
                        }
                        _ => return vec![],
                    };
                    range.map(|i| format!("{name}[{i}]")).collect()
                };
                let mut parts = Parts {
                    moved: s.sub_moved.iter().chain(rt_moved.iter()).flat_map(part_names).collect(),
                    maybe: still_maybe.iter().flat_map(part_names).collect(),
                };
                parts.moved.sort();
                parts.maybe.sort();
                // A part with no name (an enum payload, `(q.a[_] as
                // Some).0`): borrowck's dataflow counts a variant's payload
                // uninitialized wherever the value is another variant, so
                // its moved / maybe can't tell "moved on every path" from
                // "not that variant" there. Whether some path that runs
                // reaches here without moving it (walking back from here to
                // an assignment of it or the fn's entry before any move of
                // it) decides: then "part of it may have been moved out".
                let move_skippable = |m: MovePathIndex| -> bool {
                    let anc = ancestors(&move_data, m);
                    let moves_it = |l: Location| move_data.loc_map[l].iter().any(|mo| anc.contains(&move_data.moves[*mo].path));
                    let inits_it = |l: Location| move_data.init_loc_map[l].iter().any(|ii| anc.contains(&move_data.inits[*ii].path));
                    let mut work: Vec<(Location, bool)> = exits.iter().map(|l| (*l, after)).collect();
                    let mut seen = BTreeSet::new();
                    while let Some((loc, inclusive)) = work.pop() {
                        if !can_walk_back(loc, imaginary) || !seen.insert((loc, inclusive)) {
                            continue;
                        }
                        if inclusive && moves_it(loc) {
                            continue;
                        }
                        if inclusive && inits_it(loc) {
                            return true;
                        }
                        if loc.statement_index > 0 {
                            work.push((Location { statement_index: loc.statement_index - 1, ..loc }, true));
                        } else if loc.block == rustc_middle::mir::START_BLOCK {
                            return true;
                        } else {
                            work.extend(predecessors(loc).iter().map(|&bb| (body.terminator_loc(bb), true)));
                        }
                    }
                    false
                };
                if s.sub_moved.iter().chain(rt_moved.iter()).chain(still_maybe.iter())
                    .any(|m| part_names(m).is_empty() && move_skippable(*m))
                {
                    st.unnamed_maybe.insert(name.clone());
                }
                st.parts.insert(name.clone(), parts);
            }
        }
        for (parent, fp, name) in &synth {
            if !declared(fp.local) { continue; }
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
            if !through_ref && fp.ty(&body.local_decls, tcx).ty.needs_drop(tcx, typing_env) {
                if let Some(all) = next_assignment(*fp, &rt_exits, after) {
                    st.reassigned.insert(name.clone(), all);
                }
            }
            st.state.insert(name.clone(), if pw == "partly" { "own" } else { pw });
            record_type(name, *fp);
            if let Some(c) = st.cause.get(&pn).copied() {
                // An untracked field can inherit a possible move of its
                // parent even when the runtime moves only a sibling.
                let imaginary = c == "moved" && !through_ref && !rt_exits.is_empty()
                    && match rt_md.rev_lookup.find(fp.as_ref()) {
                        LookupResult::Parent(Some(rm)) => {
                            let ancestors = ancestors(&rt_md, rm);
                            rt_exits.iter().all(|&loc| {
                                let moved = may_after(&rt_moved_entry, &|st, l| moved_transfer(&rt_md, &rt_body, st, l), loc, after);
                                let unassigned = may_after(&rt_unassigned_entry, &|st, l| unassigned_transfer(&rt_md, &rt_body, st, l), loc, after);
                                ancestors.iter().all(|p| !moved.contains(p) && !unassigned.contains(p))
                            })
                        }
                        _ => false,
                    };
                st.cause.insert(name.clone(), if imaginary { "imaginary" } else { c });
            }
        }
        // When different locals are active on different paths the column has no single state.
        st.locals = column_locals;
        st.state.retain(|name, _| st.locals.get(name) != Some(&None));
        st.holder_types = holder_types.into_iter().filter_map(|(name, ty)| Some((name, ty?))).collect();
        // Loans in scope where the next row starts on the same path: in
        // scope after the exit and still at the successor (a loan whose last
        // use is on this row is out of scope there; another path's loan
        // that joins at the successor is not on this one).
        let mut live: BTreeSet<usize> = BTreeSet::new();
        for e in exits {
            if !after {
                inscope.seek_before_primary_effect(*e);
                live.extend(inscope.get().iter().map(|b| b.index()));
                continue;
            }
            let line = cx.row_of(*e);
            let succs = cx.r.real_succs(*e);
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
                    for h in holders_at(if after { t.mid_index(*e) } else { t.start_index(*e) }, BorrowIndex::from_usize(l.loan)) {
                        if !now.contains(&h) {
                            now.push(h);
                        }
                    }
                }
            }
            let first = loan_holder(l);
            let holder = if now.is_empty() || now.contains(&first) { first } else { now[0].clone() };
            let mut current = BTreeSet::new();
            let mut iterator = false;
            if let (Some(pol), Some(t)) = (&pol, table) {
                let id = BorrowIndex::from_usize(l.loan);
                for e in exits {
                    let point = if after { t.mid_index(*e) } else { t.start_index(*e) };
                    let Some(origins) = pol.origin_contains_loan_at.get(&point) else { continue };
                    let carries = |o: &Origin| origins.get(o).is_some_and(|ls| ls.contains(&id));
                    iterator |= iterator_origins.iter().any(carries);
                    for n in holders_at(point, id) {
                        if st.state.get(&n) != Some(&"own") { continue; }
                        if l.runs { st.holder_sources.entry(n.clone()).or_default().insert(l.lender.clone()); }
                        let mut fields = Vec::new();
                        for (_, fp, field) in columns.iter().chain(&synth) {
                            if fp.projection.is_empty() || cx.names.locals.get(&fp.local) != Some(&n) || st.state.get(field) != Some(&"own") {
                                continue;
                            }
                            if let TyKind::Ref(region, ..) = fp.ty(&body.local_decls, tcx).ty.kind() {
                                if let rustc_middle::ty::ReVar(origin) = region.kind() {
                                    if carries(&origin.into()) { fields.push(field.clone()); }
                                }
                            }
                        }
                        if fields.is_empty() { current.insert(n); } else { current.extend(fields); }
                    }
                }
            }
            if let Some(direct) = &l.direct_holder {
                if st.state.get(direct) == Some(&"own") && reborrows.get(&l.loan).is_some_and(|children|
                    children.iter().any(|(child, through)| through == direct && live.contains(child))) {
                    current.insert(direct.clone());
                }
            }
            // A two-phase `&mut` before its activation only reserves the
            // place: it can still be read. Activated on some path to here
            // (its activation reaches the row's exit) is activated.
            // (Strictly before the row's first effect for the state where
            // the next row starts; and not across the reservation itself:
            // a loop's next call reserves anew, after the last activation.)
            let reservation = bwf.borrow_set.location_map().get_index(l.loan).map(|(loc, _)| *loc);
            let reached = |a: Location| {
                exits.iter().any(|e| {
                    let res_between = |from: usize, to: usize, bb| reservation.is_some_and(|r| r.block == bb && from < r.statement_index && r.statement_index <= to);
                    if a.block == e.block && (a.statement_index < e.statement_index || (after && a.statement_index == e.statement_index)) {
                        return !res_between(a.statement_index, e.statement_index, a.block);
                    }
                    let mut seen = BTreeSet::new();
                    let mut work: Vec<_> = body.basic_blocks[a.block].terminator().successors().collect();
                    while let Some(bb) = work.pop() {
                        let res_here = reservation.filter(|r| r.block == bb);
                        if bb == e.block && res_here.is_none_or(|r| e.statement_index < r.statement_index || (!after && e.statement_index == r.statement_index)) {
                            return true;
                        }
                        if res_here.is_some() {
                            continue;
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
                    st.lent.entry(c.clone()).or_default().push(Lent { loan: l.loan, kind, holder: holder.clone(), iterator, current: current.clone() });
                }
            }
            // A lender that is no column (`*r` reborrowed through `r`,
            // `v[_]`): listed under its own name too (not a temporary).
            if !l.columns.contains(&l.lender) && l.source == "borrow" {
                st.lent.entry(l.lender.clone()).or_default().push(Lent { loan: l.loan, kind, holder: holder.clone(), iterator, current: current.clone() });
            }
        }
        // What each variable's references hold where the next row of the
        // same path starts, as the borrow checker counts it: the loans its
        // origins contain both at the last point of the row's path and at
        // the first point of the next row. The first keeps out loans a
        // join below brings in from another arm (Polonius merges them at
        // the join's first point); the second, loans of a reference no
        // later row uses (its origin is dead there). Where the path leaves
        // the function on the row, its last point alone. Only a variable
        // that holds a value: a moved-out or unassigned one holds nothing.
        // (A superset of what it points to when it runs: a field
        // overwrite, `h.r = &b`, doesn't take `a`'s loan out of `h`'s
        // origin, and a copy made before it, `let old = h.r`, gets `b`'s
        // too; rustc rejects a write to either while they are live.)
        if let Some(t) = table {
            let known: BTreeSet<usize> = loans.iter().map(|l| l.loan).collect();
            let mut holds: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
            let mut option_holds: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
            // A block more than one way of code that runs comes into (a
            // join).
            let preds = body.basic_blocks.predecessors();
            let is_join = |bb: rustc_middle::mir::BasicBlock| {
                preds[bb]
                    .iter()
                    .filter(|p| {
                        let end = Location { block: **p, statement_index: body.basic_blocks[**p].statements.len() };
                        !body.basic_blocks[**p].is_cleanup && cx.r.real_succs(end).iter().any(|s| s.block == bb)
                    })
                    .count()
                    > 1
            };
            let lmap = bwf.borrow_set.location_map();
            for e in exits {
                let line = cx.row_of(*e);
                // Each way the path leaves the row: its last point on the
                // row and, unless it ends there (a return), the next row's
                // first point and that row. MIR's jumps that carry the
                // branch's own span stay on the row (a skipped `if`'s
                // goto). A join on the row (after one-line arms, `if c { r
                // = &x; } else { r = &y; }`) merges the arms' loans: the
                // path's own are read just before it (`frozen`).
                let mut legs: Vec<(Point, bool, Option<(Point, usize)>)> = Vec::new();
                // Loans taken on the row after such a join (on every way
                // through it).
                let mut after_join: BTreeSet<usize> = BTreeSet::new();
                let mut seen: BTreeSet<Location> = BTreeSet::new();
                let mut work: Vec<(Location, Option<Point>)> = Vec::new();
                if after {
                    work.push((*e, None));
                } else {
                    // The snapshot is at entry, before this row can issue
                    // any loans or assign a new holder.
                    legs.push((t.start_index(*e), false, None));
                }
                while let Some((l, frozen)) = work.pop() {
                    if !seen.insert(l) {
                        continue;
                    }
                    if frozen.is_some() {
                        if let Some(i) = lmap.get_index_of(&l) {
                            after_join.insert(i);
                        }
                    }
                    let last = frozen.unwrap_or_else(|| t.mid_index(l));
                    let next = cx.r.real_succs(l);
                    if next.is_empty() {
                        legs.push((last, frozen.is_some(), None));
                    }
                    for s in next {
                        match cx.rows.row.get(&s) {
                            Some(r) if *r != line => legs.push((last, frozen.is_some(), Some((t.start_index(s), *r)))),
                            _ => {
                                let f = frozen.or_else(|| (s.statement_index == 0 && is_join(s.block)).then(|| t.mid_index(l)));
                                work.push((s, f));
                            }
                        }
                    }
                }
                // Only the way on to the next row of the same path (rule
                // 3): not a branch's skip, a `?`'s or `break`'s jump, or a
                // loop head's way out, further down.
                if let Some(n) = legs.iter().filter_map(|(_, _, nx)| nx.map(|(_, r)| r)).filter(|r| *r > line).min() {
                    legs.retain(|(_, _, nx)| nx.is_some_and(|(_, r)| r == n));
                }
                for (last, frozen, nx) in legs {
                    for (o, untracked) in [(pol.as_ref(), false), (spol.as_ref(), true)] {
                        let Some(o) = o else { continue };
                        // (Each run's own loans: the untracked run's
                        // placeholders share ids with tracked ones.)
                        let ours = |l: &usize| (*l >= base) == untracked && known.contains(l);
                        let at = |p: Point, os: &[Origin]| -> BTreeSet<usize> {
                            o.origin_contains_loan_at.get(&p).map_or_else(BTreeSet::new, |m| {
                                os.iter().filter_map(|o| m.get(o)).flatten().map(|l| l.index()).filter(ours).collect()
                            })
                        };
                        // Past a join on the row, what the variable gets
                        // there comes from any value the path had before
                        // it (`let r = if c { &x } else { &y };` assigns a
                        // temporary in each arm), or a loan taken after it.
                        let before_join: BTreeSet<usize> = if frozen {
                            o.origin_contains_loan_at
                                .get(&last)
                                .map(|m| m.values().flatten().map(|l| l.index()).filter(ours).collect::<BTreeSet<usize>>())
                                .unwrap_or_default()
                                .into_iter()
                                .chain(after_join.iter().copied().filter(|l| !untracked && ours(l)))
                                .collect()
                        } else {
                            BTreeSet::new()
                        };
                        let held_between = |os: &[Origin]| {
                            let mut held = if frozen { before_join.clone() } else { at(last, os) };
                            if let Some((first, _)) = nx {
                                let there = at(first, os);
                                held.retain(|l| there.contains(l));
                            } else if frozen {
                                held.retain(|l| at(last, os).contains(l));
                            }
                            held
                        };
                        for (local, os) in &origins_of {
                            let name = &cx.names.locals[local];
                            if declared(*local) && matches!(st.state.get(name), Some(&"own" | &"partly")) {
                                let held = held_between(os);
                                if !held.is_empty() {
                                    holds.entry(name.clone()).or_default().extend(held);
                                }
                            }
                        }
                        let active_option = |local: Local, name: &str| {
                            matches!(st.holder_types.get(name), Some(HolderType::OptionRef { local: i, .. }) if *i == local.index())
                                && st.state.get(name) == Some(&"own")
                        };
                        for &(local, name, origin) in &option_fields {
                            if active_option(local, name) {
                                option_holds.entry(name.clone()).or_default().extend(held_between(&[origin]));
                            }
                        }
                        // A reference to the whole Option can keep its
                        // contents alive after the Option's own origin dies.
                        // The outer origin identifies that borrow. The inner
                        // origin tracks the contents, limited to what this
                        // exact borrow could see when it was taken.
                        for &(id, loc, local, name, origin) in &option_borrows {
                            if !active_option(local, name) || !live.contains(&id) { continue; }
                            let initial = at(t.start_index(loc), &[origin]);
                            if initial.is_empty() { continue; }
                            let Some(tracked) = &pol else { continue };
                            let via = |p, outer| tracked.origin_contains_loan_at.get(&p)
                                .and_then(|m| m.get(&outer))
                                .is_some_and(|ls| ls.contains(&BorrowIndex::from_usize(id)));
                            for &(borrower, outer, inner) in &option_refs {
                                if declared(borrower) && via(last, outer) && nx.is_none_or(|(p, _)| via(p, outer)) {
                                    option_holds.entry(name.clone()).or_default()
                                        .extend(held_between(&[inner]).intersection(&initial).copied());
                                }
                            }
                        }
                    }
                }
            }
            st.holds = holds.into_iter().map(|(k, v)| (k, v.into_iter().collect())).collect();
            st.option_holds = option_holds.into_iter().map(|(k, v)| (k, v.into_iter().collect())).collect();
        }
        for take in &manually_taken {
            if st.state.get(&take.column) != Some(&"own") { continue; }
            let mut paths = ExtractionPaths::default();
            for &loc in &rt_exits {
                if let Some(status) = take.boundaries.get(&(loc, after)) {
                    paths.join(*status);
                }
            }
            if paths.taken {
                st.manually_taken.insert(take.column.clone(), Extraction { inner: take.inner.clone(), maybe: paths.intact });
            }
        }
        for (name, loans) in &st.lent {
            let exclusive = loans.iter().any(|l| matches!(l.kind, "mut" | "unique"));
            let ids = loans.iter().filter(|l| matches!(l.kind, "mut" | "unique") == exclusive).map(|l| l.loan).collect();
            if may_skip(exits, after, &ids, imaginary) { st.may_lend.insert(name.clone()); }
        }
        st
    };
    let mut checked: BTreeSet<rustc_middle::mir::BasicBlock> = BTreeSet::new();
    {
        let mut work = vec![rustc_middle::mir::START_BLOCK];
        while let Some(bb) = work.pop() {
            if !is_unreachable(body, bb) && !body.basic_blocks[bb].is_cleanup && checked.insert(bb) {
                work.extend(body.basic_blocks[bb].terminator().successors());
            }
        }
    }
    // One arm may have several imaginary incoming edges (a let chain).
    // Follow them through scrutinee cleanup to the first MIR locations
    // belonging to the else expression, and read the state before them.
    use rustc_hir::{intravisit, Expr, ExprKind};
    #[derive(Default)]
    struct Conditionals<'hir>(Vec<(&'hir Expr<'hir>, &'hir Expr<'hir>, &'hir Expr<'hir>, Span)>);
    impl<'hir> intravisit::Visitor<'hir> for Conditionals<'hir> {
        fn visit_expr(&mut self, expr: &'hir Expr<'hir>) {
            match expr.kind {
                ExprKind::If(guard, then, Some(els)) if !expr.span.from_expansion() =>
                    self.0.push((guard, then, els, expr.span)),
                _ => {}
            }
            intravisit::walk_expr(self, expr);
        }
    }
    let edge_targets: Vec<_> = body.basic_blocks.iter_enumerated().filter_map(|(bb, data)| {
        let term = data.terminator();
        let TerminatorKind::FalseEdge { imaginary_target, .. } = term.kind else { return None };
        (checked.contains(&bb) && checked.contains(&imaginary_target) && !runs.contains(&imaginary_target))
            .then_some((term.source_info.span.with_parent(None), bb, imaginary_target))
    }).collect();
    let span_of = |loc| body.source_info(loc).span.source_callsite().with_parent(None);
    let mut conditionals = Conditionals::default();
    if let Some(hir) = def_id.as_local().filter(|_| !edge_targets.is_empty())
        .and_then(|id| tcx.hir_maybe_body_owned_by(id))
    {
        intravisit::Visitor::visit_expr(&mut conditionals, hir.value);
    }
    let mut imaginary_edges = Vec::new();
    for (guard, then, els, conditional) in conditionals.0 {
        let arm = els.span.with_parent(None);
        let guard_span = guard.span.with_parent(None);
        let then_span = then.span.with_parent(None);
        let conditional = conditional.with_parent(None);
        let edges: Vec<_> = edge_targets.iter().filter(|(span, _, _)| guard_span.contains(*span)).collect();
        let mut work: Vec<_> = edges.iter()
            .map(|&&(_, _, block)| Location { block, statement_index: 0 }).collect();
        if work.is_empty() { continue; }

        let contains = |span: Span, loc| {
            let source = span_of(loc);
            !source.is_dummy() && span.contains(source)
        };
        let mut seen = BTreeSet::new();
        let mut entry = BTreeSet::new();
        while let Some(loc) = work.pop() {
            if !runs.contains(&loc.block) && seen.insert(loc) {
                if contains(arm, loc) {
                    entry.insert(loc);
                } else {
                    work.extend(cx.r.real_succs(loc));
                }
            }
        }
        if entry.is_empty() { continue; }

        let mut arm_locations = BTreeSet::new();
        let mut rejoin = BTreeSet::new();
        work.extend(entry.iter().copied());
        while let Some(loc) = work.pop() {
            if runs.contains(&loc.block) || !arm_locations.insert(loc) { continue; }
            let next = cx.r.real_succs(loc);
            if span_of(loc) == conditional {
                rejoin.extend(next.into_iter().filter(|n| runs.contains(&n.block)));
            } else {
                work.extend(next);
                if let TerminatorKind::FalseEdge { imaginary_target, .. } = body.basic_blocks[loc.block].terminator().kind {
                    if loc == body.terminator_loc(loc.block) && checked.contains(&imaginary_target) {
                        work.push(Location { block: imaginary_target, statement_index: 0 });
                    }
                }
            }
        }
        let entry: Vec<_> = entry.into_iter().collect();
        let mut entry_state = state_at(&entry, false, true);
        // Storage can be live during a let initializer even though the
        // binding's lexical scope starts after it.
        let scopes: Vec<_> = entry.iter().flat_map(|&loc| {
            std::iter::successors(Some(body.source_info(loc).scope), |&scope| body.source_scopes[scope].parent_scope)
        }).collect();
        let mut hidden = BTreeSet::new();
        let mut visible = BTreeSet::new();
        for debug in &body.var_debug_info {
            let rustc_middle::mir::VarDebugInfoContents::Place(place) = debug.value else { continue };
            if scopes.contains(&debug.source_info.scope) {
                visible.insert(place.local);
            } else {
                hidden.insert(place.local);
            }
        }
        let hidden = &hidden - &visible;
        let mut initializer_bindings = BTreeMap::new();
        for (_, place, name) in columns.iter().chain(&synth) {
            if hidden.contains(&place.local) && entry_state.state.remove(name) == Some("unassigned") {
                if let Some(binding) = cx.names.locals.get(&place.local) {
                    initializer_bindings.insert(name.clone(), binding.clone());
                }
            }
        }

        // Assignments can replace a value without making the binding unusable.
        let moves_at = |loc: Location| {
            let data = &body.basic_blocks[loc.block];
            let storage_dead = data.statements.get(loc.statement_index)
                .is_some_and(|s| matches!(s.kind, StatementKind::StorageDead(_)));
            move_data.loc_map[loc].iter().filter_map(move |&m| (!storage_dead).then_some(m))
        };
        let mut arm_changes = BTreeMap::new();
        for &loc in &arm_locations {
            if !contains(arm, loc) { continue; }
            for i in &move_data.init_loc_map[loc] {
                arm_changes.insert(move_data.inits[*i].path, ImaginaryChange::Assign);
            }
            for m in moves_at(loc) {
                arm_changes.entry(move_data.moves[m].path).or_insert(ImaginaryChange::Move);
            }
        }
        // Locate the scrutinees through HIR and read what moved from MIR.
        fn scrutinees<'hir>(guard: &'hir Expr<'hir>, out: &mut Vec<Span>) {
            match guard.kind {
                ExprKind::Let(expr) => out.push(expr.init.span.with_parent(None)),
                ExprKind::Binary(op, a, b) if op.node == rustc_hir::BinOpKind::And => {
                    scrutinees(a, out);
                    scrutinees(b, out);
                }
                _ => {}
            }
        }
        let mut scrutinee_spans = Vec::new();
        scrutinees(guard, &mut scrutinee_spans);
        let mut scrutinee_moves = BTreeSet::new();
        let mut runtime_changes = BTreeSet::new();
        for &block in &runs {
            for statement_index in 0..=body.basic_blocks[block].statements.len() {
                let loc = Location { block, statement_index };
                if !contains(guard_span, loc) && !contains(then_span, loc) { continue; }
                runtime_changes.extend(move_data.init_loc_map[loc].iter().map(|i| move_data.inits[*i].path));
                for m in moves_at(loc) {
                    let path = move_data.moves[m].path;
                    runtime_changes.insert(path);
                    let Some(name) = column_name(path) else { continue };
                    if entry_state.state.get(&name) == Some(&"moved")
                        && scrutinee_spans.iter().any(|&span| contains(span, loc))
                        && edges.iter().all(|&&(_, source, _)| runs.contains(&source)
                            && rt_body.basic_blocks.dominators().dominates(block, source))
                    {
                        scrutinee_moves.insert(name);
                    }
                }
            }
        }
        // A write or move of a whole aggregate also affects its fields.
        // A runtime change to any part prevents saying the whole value
        // was preserved, but does not affect a sibling field.
        let includes = |whole: Place<'tcx>, part: Place<'tcx>| {
            whole.local == part.local && part.projection.starts_with(whole.projection)
        };
        let changes = columns.iter().chain(&synth).filter_map(|(_, place, name)| {
            let change = arm_changes.iter().filter_map(|(&path, &change)| {
                includes(move_data.move_paths[path].place, *place).then_some(change)
            }).max()?;
            let changed_at_runtime = runtime_changes.iter().any(|&path| {
                let other = move_data.move_paths[path].place;
                includes(other, *place) || includes(*place, other)
            });
            Some((name.clone(), if changed_at_runtime { ImaginaryChange::AlsoChangesAtRuntime } else { change }))
        }).collect();
        let after = cx.rows.exits.range(cx.r.line_lo(arm)..=cx.r.line_hi(arm)).filter_map(|(&row, exits)| {
            let exits: Vec<_> = exits.iter().copied()
                .filter(|loc| arm_locations.contains(loc) && contains(arm, *loc)).collect();
            (!exits.is_empty()).then(|| (row, state_at(&exits, true, true)))
        }).collect();
        imaginary_edges.push(ImaginaryEdge {
            id: ImaginaryEdgeId(arm),
            entry: entry_state,
            after,
            join: if rejoin.is_empty() { State::default() } else { state_at(&rejoin.into_iter().collect::<Vec<_>>(), false, false) },
            changes,
            scrutinee_moves,
            initializer_bindings,
        });
    }
    // A straight line meets the next row on its incoming CFG edges. At
    // a join those can come from several rows, even if the closing row
    // itself only contains the last arm's StorageDead.
    // The MIR source scopes retain the lexical blocks of the code they
    // contain. After a block ends, the straight segment belongs to the
    // continuing path, not to the last operation of the arm just drawn.
    let mut block_ends = BTreeSet::new();
    let mut visited = std::collections::HashSet::new();
    for &loc in cx.rows.row.keys() {
        let id = body.location_to_hir_id(loc);
        for (id, node) in std::iter::once((id, tcx.hir_node(id))).chain(tcx.hir_parent_iter(id)) {
            if !visited.insert(id) { break; }
            if let rustc_hir::Node::Block(block) = node {
                // (A block written in the source, ending in `}`: not one
                // lowering makes up, as for a destructuring assignment
                // `Pair { a: x, b: y } = r;`, which ends no scope there.)
                let braced = tcx.sess.source_map().span_to_snippet(block.span).is_ok_and(|t| t.trim_end().ends_with('}'));
                if !block.span.from_expansion() && braced { block_ends.insert(cx.r.line_hi(block.span)); }
            }
        }
    }
    let mut entries: BTreeMap<usize, BTreeSet<Location>> = BTreeMap::new();
    let start = Location::START;
    entries.entry(cx.row_of(start)).or_default().insert(start);
    for (&from, &row) in &cx.rows.row {
        if !runs.contains(&from.block) { continue; }
        for to in cx.r.real_succs(from) {
            if let Some(&next) = cx.rows.row.get(&to) {
                if next > row && !cx.rows.exit_only.contains(&to) && !cx.rows.early_exit.contains_key(&to)
                    && !cx.rows.shared_exits.get(&to).is_some_and(|sites| drop_route(to, sites) == DropRoute::ExitOnly) {
                    entries.entry(next).or_default().insert(to);
                }
            }
        }
    }
    let mut out_rows: Vec<Row> = Vec::new();
    let mut lines: BTreeSet<usize> = cx.rows.row.values().copied().collect();
    lines.extend(evs.keys().copied());
    lines.extend(block_ends.iter().copied());
    for line in lines.into_iter().filter(|l| *l >= first_line && *l <= last_line) {
        let mut row = Row { line, ..Default::default() };
        if let Some(exits) = cx.rows.exits.get(&line) {
            row.after = state_at(exits, true, false);
            if let [exit] = exits.as_slice() {
                if let [next] = cx.r.real_succs(*exit).as_slice() {
                    let next_line = cx.row_of(*next);
                    if next_line > line && (next.block == exit.block || body.basic_blocks.predecessors()[next.block].len() == 1) {
                        row.continues_to = Some(next_line);
                    }
                }
            }
        }
        row.block_end = block_ends.contains(&line);
        // Code that runs on the row only leaves it by a jump (`break k;`,
        // `return i;`, `continue;`): every way out goes past the next row
        // the paths come into, so the line drawn below the row is on the
        // other paths, which skip it (Q3).
        if let Some(exits) = cx.rows.exits.get(&line) {
            let next = entries.range(line + 1..).next().map(|(r, _)| *r);
            let succs: Vec<Location> = exits.iter().filter(|e| runs.contains(&e.block)).flat_map(|e| cx.r.real_succs(*e)).collect();
            row.jumps = next.is_some() && !succs.is_empty() && succs.iter().all(|s| {
                cx.rows.exit_only.contains(s) || cx.rows.early_exit.contains_key(s)
                    || cx.rows.row.get(s).is_some_and(|r| next.is_some_and(|n| *r > n))
            });
        }
        row.before = entries.get(&line).map(|locs| state_at(&locs.iter().copied().collect::<Vec<_>>(), false, false));
        if let Some(specs) = cx.rows.arms.get(&line) {
            let nested: BTreeSet<_> = specs.iter().filter_map(|s| s.parent).collect();
            for (i, s) in specs.iter().enumerate() {
                row.arms.push(Arm {
                    runs: s.exits.iter().any(|e| runs.contains(&e.block)),
                    branch: s.branch,
                    arm: s.arm,
                    parent: s.parent,
                    col_lo: s.col_lo,
                    col_hi: s.col_hi,
                    has_exit: s.exits.iter().any(|exit| runs.contains(&exit.block)),
                    switch_col: s.switch_col,
                    code_spans: s.code_spans.clone(),
                    continuing: (nested.contains(&i) && s.continuing.iter().any(|exit| runs.contains(&exit.block)))
                        .then(|| state_at(&s.continuing, true, false)),
                    after: state_at(&s.exits, true, false),
                });
            }
        }
        let mut fs = evs.remove(&line).unwrap_or_default();
        fs.sort_by_key(|e| e.order);
        // Only the same fact from the same MIR location is a duplicate (two
        // evaluations of one argument are two events), on the same exit site
        // (a drop two early exits share is one event per exit).
        // and edge. Distinct successors can have the same display coordinates.
        fs.dedup_by(|a, b| a.same_occurrence(b));
        row.facts = fs;
        out_rows.push(row);
    }
    for (line, fs) in evs {
        notes.push(format!("{} facts on row {} outside the body's rows", fs.len(), line));
    }
    // Lines whose code only borrowck's imaginary edges reach: reachable in
    // the CFG borrowck checks, but not in what runs (`runs`).
    let run_lines: BTreeSet<usize> = cx.rows.row.iter().filter(|(l, _)| runs.contains(&l.block)).map(|(_, r)| *r).collect();
    let dead_lines: BTreeSet<usize> = cx.rows.row.iter()
        .filter(|(l, r)| checked.contains(&l.block) && !runs.contains(&l.block) && !run_lines.contains(*r))
        .map(|(_, r)| *r)
        .filter(|r| *r >= first_line && *r <= last_line)
        .collect();
    let constructed_vecs = runs.iter().filter_map(|&bb| {
        let expansion = body.basic_blocks[bb].terminator().source_info.span.ctxt().outer_expn();
        let &(call, _) = cx.vec_results.get(&expansion)?;
        (bb == call.block).then(|| (expansion, cx.row_of(call)))
    }).collect();
    let evaluated_spans = cx.rows.row.keys().filter(|loc| runs.contains(&loc.block) && !body.basic_blocks[loc.block].is_cleanup)
        .map(|loc| body.source_info(*loc).span.with_parent(None)).collect();
    // MIR gives the incoming gotos of a labeled block or an if the span
    // of the expression they complete. Compare each incoming edge before
    // they meet, including the break value's effects. Imaginary edges
    // affect usability but do not execute an assignment.
    // Search only this expression on real incoming paths. A sibling
    // that returned, or an operation in a previous iteration, supplies
    // no evidence about the value carried by this edge.
    let reaches = |span: Span, mut work: Vec<Location>, effect: &dyn Fn(Location) -> bool| {
        let mut seen = BTreeSet::new();
        while let Some(loc) = work.pop() {
            if !seen.insert(loc) || !span.contains(body.source_info(loc).span.source_callsite()) { continue; }
            if effect(loc) { return true; }
            if loc.statement_index > 0 {
                work.push(Location { statement_index: loc.statement_index - 1, ..loc });
            } else {
                work.extend(rt_body.basic_blocks.predecessors()[loc.block].iter()
                    .filter(|p| runs.contains(p)).map(|p| body.terminator_loc(*p)));
            }
        }
        false
    };
    let mut continuations = BTreeMap::<Span, BTreeSet<_>>::new();
    for &pred in &runs {
        let term = body.basic_blocks[pred].terminator();
        if let TerminatorKind::Goto { target } = term.kind {
            if !term.source_info.span.from_expansion() {
                continuations.entry(term.source_info.span).or_default().insert(target);
            }
        }
    }
    let mut joins = BTreeMap::new();
    for (span, targets) in continuations {
        if targets.len() != 1 { continue; }
        let bb = *targets.first().expect("one continuation");
        let state = state_at(&[Location { block: bb, statement_index: 0 }], false, false);
        let join = joins.entry(span).or_insert(Join { state, assignments: BTreeMap::new(), dead_implicit_else: false });
        let preds: Vec<_> = body.basic_blocks.predecessors()[bb].iter()
            .filter(|p| checked.contains(p)).map(|p| body.terminator_loc(*p)).collect();
        if preds.len() < 2 { continue; }
        if !preds.iter().all(|p| body.source_info(*p).span == span
            && matches!(body.basic_blocks[p.block].terminator().kind, TerminatorKind::Goto { target } if target == bb)) {
            continue;
        }
        // The implicit else of an if with no else assigns unit at the
        // zero-width end of the if. Its predecessor cannot execute.
        let mut implicit_else = preds.iter().filter(|p| body.basic_blocks[p.block].statements.iter().any(|s|
            s.source_info.span == span.shrink_to_hi() && matches!(&s.kind, StatementKind::Assign(a)
                if a.0.ty(&body.local_decls, tcx).ty.is_unit()))).peekable();
        join.dead_implicit_else = implicit_else.peek().is_some() && implicit_else.all(|p| !runs.contains(&p.block));
        let incoming: Vec<_> = preds.iter().map(|p| (runs.contains(&p.block), state_at(&[*p], true, false))).collect();
        for (mpi, place, name) in &columns {
            if !place.projection.is_empty() || join.state.state.get(name) != Some(&"maybe") { continue; }
            let edges = |real, word| preds.iter().zip(&incoming)
                .filter(|(_, (r, s))| *r == real && s.state.get(name) == Some(&word))
                .map(|(loc, _)| *loc).collect::<Vec<_>>();
            let own = edges(true, "own");
            if own.is_empty() { continue; }
            let moved = edges(true, "moved");
            let runtime_moved = !moved.is_empty();
            if runtime_moved && reaches(span, moved, &|l| move_data.loc_map[l].iter()
                .any(|m| move_data.moves[*m].path == *mpi)) {
                join.state.moved_in_arm.insert(name.clone());
            }
            let runtime_empty = runtime_moved || !edges(true, "unassigned").is_empty();
            if !runtime_empty && edges(false, "unassigned").is_empty() { continue; }
            let written = reaches(span, own, &|l| move_data.init_loc_map[l].iter().any(|i| move_data.inits[*i].path == *mpi));
            if written {
                let always = join.state.always_assigned.contains(name);
                if runtime_empty || always {
                    join.assignments.insert(name.clone(), always);
                }
            }
        }
    }
    // NLL may have already released the old value at a later reassignment.
    // Find the stores that actually reach the write, stopping on each path
    // at its first definition. Record the loans at that definition, when
    // the value was produced, rather than their liveness at the overwrite.
    let has_repoints = out_rows.iter().flat_map(|r| &r.facts).any(|e| matches!(e.fact, Fact::Repoint { .. }));
    if let Some(t) = table.filter(|_| has_repoints) {
        let mut stores: BTreeMap<String, BTreeMap<Location, Vec<usize>>> = field_stores.iter()
            .map(|(path, v)| (path.clone(), v.iter().cloned().collect())).collect();
        let mut preds: BTreeMap<Location, Vec<Location>> = BTreeMap::new();
        for &loc in cx.rows.row.keys().filter(|l| runs.contains(&l.block)) {
            for next in cx.r.real_succs(loc) { preds.entry(next).or_default().push(loc); }
            let data = &body.basic_blocks[loc.block];
            let Some(st) = data.statements.get(loc.statement_index) else {
                // Only the normal return edge writes the destination. The
                // reaching-store graph excludes unwinding and cleanup.
                if let TerminatorKind::Call { destination, target: Some(target), .. } = data.terminator().kind {
                    if destination.projection.is_empty() && body.local_decls[destination.local].ty.is_ref() {
                        if let Some(name) = cx.name(destination) {
                            let returned = Location { block: target, statement_index: 0 };
                            let ids = held_by_place(destination, t.start_index(returned)).into_iter()
                                .filter(|l| published.contains(l)).collect();
                            stores.entry(name).or_default().insert(loc, ids);
                        }
                    }
                }
                continue;
            };
            let StatementKind::Assign(asg) = &st.kind else { continue };
            let (dst, rv) = &**asg;
            if !dst.projection.is_empty() || !body.local_decls[dst.local].ty.is_ref() { continue; }
            let Some(name) = cx.name(*dst) else { continue };
            let issued = loans.iter().find(|l| taken_at(l.loan) == Some(loc));
            let ids = match rv {
                Rvalue::Ref(..) => issued.map(|l| vec![l.loan]).unwrap_or_default(),
                Rvalue::Use(Operand::Copy(src) | Operand::Move(src)) =>
                    held_by_place(*src, t.start_index(loc)).into_iter().filter(|l| published.contains(l)).collect(),
                _ => held_by(dst.local, t.mid_index(loc)).into_iter().filter(|l| published.contains(l)).collect(),
            };
            stores.entry(name).or_default().insert(loc, ids);
        }
        for e in out_rows.iter_mut().flat_map(|r| &mut r.facts) {
            let Fact::Repoint { holder, reaching, .. } = &mut e.fact else { continue };
            let loc = e.location;
            let Some(defs) = stores.get(holder) else { continue };
            let mut work = preds.get(&loc).cloned().unwrap_or_default();
            let mut seen = BTreeSet::new();
            let mut ids = BTreeSet::new();
            while let Some(at) = work.pop() {
                if !seen.insert(at) { continue; }
                if let Some(ls) = defs.get(&at) { ids.extend(ls); }
                else if let Some(prev) = preds.get(&at) { work.extend(prev); }
            }
            *reaching = Some(ids.into_iter().copied().collect());
        }
    }
    let function_body_start = (!tcx.is_closure_like(def_id)).then(|| {
        let hir = tcx.hir_body_owned_by(def_id.expect_local());
        let pos = tcx.sess.source_map().lookup_char_pos(hir.value.span.lo());
        (pos.line, pos.col.0)
    });
    let conditional_ends = out_rows.iter().flat_map(|r| &r.facts).filter_map(|e| match e.fact {
        Fact::BorrowEnd { loan, .. } if taken_at(loan).is_some_and(|issue| !ldom(issue, e.location)) => Some(loan),
        _ => None,
    }).collect();
    BodyFacts {
        drop_holders,
        bindings: cx.names.bindings.into(),
        closure_result_loans,
        conditional_ends,
        aggregate_loans,
        issued_operand_loans,
        returned_operand_loans,
        inline_holder_ends,
        evaluated_spans,
        argument_stores, receiver_loans, restored_reborrows,
        imaginary_edges,
        constructed_vecs,
        take_calls: take_calls.into_iter().map(|loc| body.source_info(loc).span.with_parent(None)).collect(),
        skipped_closure_returns,
        binding_sources,
        joins,
        def_id,
        func,
        kind: if tcx.is_closure_like(def_id) { "closure" } else { "fn" },
        parent: tcx.is_closure_like(def_id).then(|| tcx.def_path_str(tcx.parent(def_id))),
        first_line,
        last_line,
        function_body_start,
        vars,
        columns: column_info,
        uniform_option_refs,
        loans,
        binding_loans,
        rows: out_rows,
        temps,
        notes,
        dead_lines,
    }
}

/// The region variable a body's region is (after borrowck's renumbering).
fn region_vid(r: rustc_middle::ty::Region<'_>) -> Option<rustc_middle::ty::RegionVid> {
    match r.kind() {
        rustc_middle::ty::RegionKind::ReVar(v) => Some(v),
        _ => None,
    }
}

/// Follow the normal edge and direct transfers of a call's temporary
/// result until it reaches a user place.
fn call_result_store<'tcx>(cx: &Cx<'_, 'tcx>, mut value: Place<'tcx>, mut at: Location) -> Option<(Place<'tcx>, Location)> {
    value.as_local()?;
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(at) { return None; }
        let block = &cx.body.basic_blocks[at.block];
        for (idx, stmt) in block.statements.iter().enumerate().skip(at.statement_index) {
            match &stmt.kind {
                StatementKind::Assign(asg) => match &asg.1 {
                    Rvalue::Use(Operand::Move(src) | Operand::Copy(src)) if *src == value => {
                        if cx.name(asg.0).is_some() {
                            return Some((asg.0, Location { block: at.block, statement_index: idx }));
                        }
                        asg.0.as_local()?;
                        value = asg.0;
                    }
                    _ if asg.0.local == value.local => return None,
                    _ => {}
                },
                StatementKind::StorageDead(local) if *local == value.local => return None,
                _ => {}
            }
        }
        match &block.terminator().kind {
            TerminatorKind::Goto { .. } => {}
            TerminatorKind::Drop { place, .. } if place.local != value.local => {}
            _ => return None,
        }
        let [next] = cx.r.real_succs(Location { block: at.block, statement_index: block.statements.len() })[..] else { return None };
        at = next;
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
    resolve_deref(tcx, body, place, false)
}

/// Follow single-assignment reference or raw-pointer locals when checking
/// writes. A mutable borrow or raw address of the pointer itself can
/// retarget it, so a unique direct assignment is insufficient in that case.
pub(super) fn resolve_write_deref<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, place: Place<'tcx>) -> Place<'tcx> {
    resolve_deref(tcx, body, place, true)
}

fn resolve_deref<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, place: Place<'tcx>, for_write: bool) -> Place<'tcx> {
    let mut place = place;
    for _ in 0..4 {
        if place.projection.first() != Some(&ProjectionElem::Deref) || (!for_write && body.local_decls[place.local].is_user_variable()) || place.local == RETURN_PLACE || place.local.index() <= body.arg_count {
            return place;
        }
        let mut src: Option<Place<'tcx>> = None;
        let mut n = 0;
        for data in body.basic_blocks.iter() {
            for st in &data.statements {
                if let StatementKind::Assign(asg) = &st.kind {
                    if for_write && matches!(asg.1, Rvalue::Ref(_, BorrowKind::Mut { .. }, p) | Rvalue::RawPtr(_, p)
                        if p.local == place.local && p.projection.is_empty())
                    {
                        return place;
                    }
                    if asg.0.local == place.local {
                        n += 1;
                        if !asg.0.projection.is_empty() {
                            return place;
                        }
                        src = match &asg.1 {
                            // `*_t` of `_t = &P` is P.
                            Rvalue::Ref(_, _, p) => Some(*p),
                            Rvalue::RawPtr(_, p) if for_write => Some(*p),
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
