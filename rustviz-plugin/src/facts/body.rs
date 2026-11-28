//! One pass per MIR body: NLL `Borrows`, Polonius (naive, once per
//! body), storage liveness, and the init analyses (MaybeInit /
//! MaybeUninit / EverInit) twice: on borrowck's CFG for states, and on
//! the CFG without its imaginary edges for what runs (drop kinds, which
//! events happen). Then read at the row model's locations.

use super::direct_holders::{self, DirectHolders, Installation};
mod temporaries;

use super::places::{borrows_part_of, column_projection, place_under, replacement_store, same_place, Capture, Names};
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
use rustc_middle::ty::{Ty, TyCtxt, TyKind, TypingEnv};
use rustc_middle::mir::visit::{MutatingUseContext, PlaceContext, Visitor};
use rustc_mir_dataflow::impls::{
    always_storage_live_locals, EverInitializedPlaces, MaybeBorrowedLocals, MaybeInitializedPlaces, MaybeStorageLive,
    MaybeUninitializedPlaces,
};
use rustc_mir_dataflow::move_paths::{LookupResult, MoveData, MovePathIndex};
use rustc_mir_dataflow::{Analysis, ResultsCursor};
use rustc_span::Span;
use rustc_utils::BodyExt;
use std::collections::{BTreeMap, BTreeSet};

type Origin = <RustcFacts as FactTypes>::Origin;
type Point = <RustcFacts as FactTypes>::Point;

#[derive(Clone, Copy)]
enum TempDefinition<'a, 'tcx> {
    Value(&'a Rvalue<'tcx>),
    Box(Place<'tcx>),
    Vec(Place<'tcx>),
    VecBuffer,
}

/// A borrow or raw pointer can let other MIR change a temporary's contents
/// without writing to its move path. A single definition alone then proves
/// nothing about which value its destructor will see.
#[derive(Default)]
struct ExposedTemps(BTreeSet<Local>);

impl<'tcx> Visitor<'tcx> for ExposedTemps {
    fn visit_place(&mut self, place: &Place<'tcx>, context: PlaceContext, _: Location) {
        if context.is_borrow() || context.is_address_of()
            || matches!(context, PlaceContext::MutatingUse(MutatingUseContext::SetDiscriminant
                | MutatingUseContext::AsmOutput | MutatingUseContext::Yield))
        {
            self.0.insert(place.local);
        }
    }
}

fn scope_legs(
    dom: &rustc_data_structures::graph::dominators::Dominators<BasicBlock>,
    entries: &BTreeMap<Span, BTreeSet<BasicBlock>>,
    sites: &[Location],
) -> Vec<Span> {
    entries.iter()
        .filter(|(_, blocks)| sites.iter().all(|site| dom.is_reachable(site.block)
            && blocks.iter().any(|&entry| dom.is_reachable(entry) && dom.dominates(entry, site.block))))
        .map(|(&span, _)| span)
        .collect()
}

#[cfg(test)]
#[test]
fn scope_leg_uses_the_reachable_copy_of_the_boundary() {
    use rustc_middle::mir::{BasicBlockData, BasicBlocks, SourceInfo, Terminator};
    use rustc_span::DUMMY_SP;

    let block = |kind| BasicBlockData::new(Some(Terminator {
        source_info: SourceInfo::outermost(DUMMY_SP),
        kind,
    }), false);
    let [original, arm, copy] = [1, 2, 3].map(BasicBlock::from_usize);
    let blocks = BasicBlocks::new(vec![
        block(TerminatorKind::Goto { target: arm }),
        block(TerminatorKind::Return),
        block(TerminatorKind::Goto { target: copy }),
        block(TerminatorKind::Return),
    ].into_iter().collect());
    assert!(!blocks.dominators().is_reachable(original));
    let entries = BTreeMap::from([(DUMMY_SP, BTreeSet::from([arm]))]);
    assert_eq!(scope_legs(blocks.dominators(), &entries, &[copy.start_location()]), [DUMMY_SP]);
}

struct Cx<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    body: &'a Body<'tcx>,
    names: Names<'tcx>,
    rows: RowModel,
    borrow_spelling: super::borrow_spelling::BorrowSpelling<'tcx>,
    r: Rows<'a, 'tcx>,
    /// The call producing each expanded vec! (from a boxed array or a
    /// repeated element). Nested expansions have distinct compiler IDs.
    vec_results: std::collections::HashMap<rustc_span::hygiene::ExpnId, (Location, Place<'tcx>)>,
}

/// Follow a conditional result through ordinary stores and constructors.
/// A replacement drop can precede the store on this same normal path.
fn conditional_result_stores<'tcx>(
    tcx: TyCtxt<'tcx>, body: &Body<'tcx>, at: Location,
    is_source_place: impl Fn(Place<'tcx>) -> bool,
    produced_at_join: impl Fn(Place<'tcx>) -> bool,
) -> Vec<Place<'tcx>> {
    let mut bb = at.block;
    let mut seen = BTreeSet::new();
    let mut staged: Vec<Place<'tcx>> = Vec::new();
    let mut overwritten = BTreeSet::new();
    let mut consumed = BTreeSet::new();
    let mut destinations = Vec::new();
    while seen.insert(bb) {
        let start = if bb == at.block { at.statement_index } else { 0 };
        for st in &body[bb].statements[start..] {
            let StatementKind::Assign(a) = &st.kind else {
                if matches!(st.kind, StatementKind::StorageLive(_) | StatementKind::StorageDead(_)
                    | StatementKind::FakeRead(_) | StatementKind::PlaceMention(_) | StatementKind::Nop) { continue; }
                return destinations;
            };
            let (dest, rhs) = &**a;
            let result_parts = |place: Place<'tcx>| -> Vec<Place<'tcx>> {
                if is_source_place(place) { return vec![]; }
                if staged.contains(&place)
                    || (!overwritten.contains(&place.local) && produced_at_join(place))
                {
                    return vec![place];
                }
                staged.iter().copied().filter(|&part| place_under(part, place)).collect()
            };
            let mut writes = Vec::new();
            let mut moves = Vec::new();
            match rhs {
                Rvalue::Use(Operand::Move(source) | Operand::Copy(source)) => {
                    if matches!(rhs, Rvalue::Use(Operand::Move(_))) {
                        moves.push(*source);
                        consumed.insert(source.local);
                    }
                    for part in result_parts(*source) {
                        writes.push(dest.project_deeper(&part.projection[source.projection.len()..], tcx));
                    }
                }
                Rvalue::Aggregate(kind, operands) => {
                    for (field, operand) in operands.iter_enumerated() {
                        if let Operand::Move(source) = operand {
                            moves.push(*source);
                            consumed.insert(source.local);
                        }
                        let Some(source) = operand.place() else { continue };
                        let parts = result_parts(source);
                        if parts.is_empty() { continue; }
                        let ty = operand.ty(&body.local_decls, tcx);
                        let field = match **kind {
                            AggregateKind::Tuple => tcx.mk_place_field(*dest, field, ty),
                            AggregateKind::Adt(did, ..) if tcx.adt_def(did).is_struct() =>
                                tcx.mk_place_field(*dest, field, ty),
                            AggregateKind::Adt(did, ..) if tcx.adt_def(did).is_enum() => {
                                writes.push(*dest);
                                continue;
                            }
                            _ => continue,
                        };
                        writes.extend(parts.iter().map(|part|
                            field.project_deeper(&part.projection[source.projection.len()..], tcx)));
                    }
                }
                Rvalue::Ref(_, _, place) if place.projection.as_slice() == [ProjectionElem::Deref] => {
                    if result_parts(Place::from(place.local)).contains(&Place::from(place.local)) {
                        writes.push(*dest);
                    }
                }
                Rvalue::Use(Operand::Constant(_)) => {}
                _ => return destinations,
            }
            overwritten.insert(dest.local);
            consumed.remove(&dest.local);
            staged.retain(|place| !place_under(*place, *dest)
                && !place_under(*dest, *place)
                && !moves.iter().any(|&source| place_under(*place, source) || place_under(source, *place)));
            if is_source_place(*dest) {
                destinations.extend(writes);
            } else {
                staged.extend(writes);
            }
        }
        bb = match body[bb].terminator().kind {
            TerminatorKind::Goto { target } if body.basic_blocks.predecessors()[target].as_slice() == [bb] =>
                target,
            TerminatorKind::Drop { place, target, .. } if place.as_local().is_some_and(|local| consumed.contains(&local)) =>
                target,
            TerminatorKind::Drop { place, target, .. } if matches!(
                body[target].statements.first().map(|s| &s.kind),
                Some(StatementKind::Assign(a)) if a.0 == place
            ) => target,
            _ => break,
        };
    }
    destinations
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

/// Walk a shared destructor's suffix back to the continuing inputs. Read
/// after the incoming effect (including a call initializing its destination).
fn continuing_drop_inputs(body: &Body<'_>, local: Local, drop: Location, exits: &BTreeSet<Location>, folded_edges: &BTreeSet<(BasicBlock, BasicBlock)>, runs: &BTreeSet<BasicBlock>,
                         original_blocks: &BTreeMap<BasicBlock, BasicBlock>) -> Vec<Location> {
    let original = |bb: BasicBlock| original_blocks.get(&bb).copied().unwrap_or(bb);
    debug_assert!(!exits.is_empty() || !folded_edges.is_empty());
    let mut work = vec![drop];
    let mut seen = BTreeSet::new();
    let mut inputs = Vec::new();
    while let Some(loc) = work.pop() {
        if !seen.insert(loc) { continue; }
        if loc.statement_index == 0 && loc.block == rustc_middle::mir::START_BLOCK {
            inputs.push(loc);
            continue;
        }
        let preds = if loc.statement_index > 0 {
            vec![Location { statement_index: loc.statement_index - 1, ..loc }]
        } else {
            body.basic_blocks.predecessors()[loc.block].iter()
                .filter(|bb| runs.contains(bb) && !body.basic_blocks[**bb].is_cleanup)
                .map(|&bb| body.terminator_loc(bb)).collect()
        };
        for p in preds {
            if exits.contains(&p) || loc.statement_index == 0 && folded_edges.contains(&(original(p.block), original(loc.block))) { continue; }
            let pass = match body.basic_blocks[p.block].statements.get(p.statement_index) {
                Some(s) => matches!(s.kind, StatementKind::Nop)
                    || matches!(s.kind, StatementKind::StorageDead(l) if l != local),
                None => matches!(body.basic_blocks[p.block].terminator().kind, TerminatorKind::Goto { .. })
                    || matches!(body.basic_blocks[p.block].terminator().kind, TerminatorKind::Drop { place, .. } if place.local != local),
            };
            if pass { work.push(p); } else { inputs.push(p); }
        }
    }
    inputs.sort();
    inputs.dedup();
    inputs
}

/// SimplifyCfg can erase a continue or a return carrying a loop. Follow
/// the resolved arm's real successors until its backedge or return.
fn folded_scope_exits<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, switches: &[SwitchExit],
                          runs: &BTreeSet<BasicBlock>, original: &Body<'tcx>) -> BTreeMap<Location, Vec<(Span, Location, bool, (BasicBlock, BasicBlock))>> {
    use rustc_hir::{Block, Expr, ExprKind, Node, PatKind, StmtKind};
    fn tail<'a>(block: &'a Block<'a>) -> Option<&'a Expr<'a>> {
        block.expr.or_else(|| match block.stmts.last()?.kind {
            StmtKind::Expr(e) | StmtKind::Semi(e) => Some(e),
            _ => None,
        })
    }
    let mut out: BTreeMap<Location, Vec<(Span, Location, bool, (BasicBlock, BasicBlock))>> = BTreeMap::new();
    let dom = body.basic_blocks.dominators();
    // A loop over a range known to be nonempty enters its body from cloned
    // blocks, so its head no longer dominates the body in the runtime CFG.
    // The original MIR still has the loop's shape for finding the back edge.
    let original_dom = original.basic_blocks.dominators();
    let back_edge = |to: BasicBlock, from: BasicBlock| dom.dominates(to, from)
        || to.index() < original.basic_blocks.len() && from.index() < original.basic_blocks.len()
            && original_dom.dominates(to, from);
    for exit in switches {
        let Some(owner) = exit.branch else { continue };
        if !runs.contains(&exit.edge.switch.block) { continue; }
        let arm = match tcx.hir_node(owner) {
            Node::Expr(expr) => match expr.kind {
                ExprKind::If(_, yes, no) => match exit.arm {
                    Some(0) => Some(yes), Some(1) => no, _ => None,
                },
                ExprKind::Match(scrutinee, arms, _) => {
                    // If simplification removed the wildcard arm's only
                    // instruction, the otherwise target of this discriminant
                    // test still selects it when all other arms are identified.
                    let other = || {
                        let (last, rest) = arms.split_last()?;
                        let TerminatorKind::SwitchInt { targets, .. } = &body[exit.edge.switch.block].terminator().kind else { return None };
                        (targets.otherwise() == exit.edge.successor && last.guard.is_none()
                            && matches!(last.pat.kind, PatKind::Wild)
                            && rest.iter().enumerate().all(|(i, a)| a.guard.is_none() && switches.iter().any(|s|
                                s.edge.switch == exit.edge.switch && s.arm == Some(i)))).then_some(rest.len())
                    };
                    let variant_arm = || {
                        let block = &body[exit.edge.switch.block];
                        let TerminatorKind::SwitchInt { discr, targets } = &block.terminator().kind else { return None };
                        let test = discr.place()?;
                        let read = block.statements.iter().rev().find_map(|s| match &s.kind {
                            StatementKind::Assign(a) if a.0 == test => Some(&a.1), _ => None,
                        })?;
                        let Rvalue::Discriminant(place) = read else { return None };
                        let adt = place.ty(&body.local_decls, tcx).ty.ty_adt_def()?;
                        let typeck = tcx.typeck(scrutinee.hir_id.owner);
                        if !adt.is_enum() || typeck.expr_ty(scrutinee).ty_adt_def() != Some(adt) { return None; }
                        let mut selected = None;
                        for (i, arm) in arms.iter().enumerate() {
                            let PatKind::TupleStruct(ref qpath, fields, _) = arm.pat.kind else { return None };
                            if arm.guard.is_some() || !fields.iter().all(|p| matches!(p.kind, PatKind::Wild | PatKind::Binding(_, _, _, None))) { return None; }
                            let variant = adt.variant_of_res(typeck.qpath_res(qpath, arm.pat.hir_id));
                            let index = adt.variant_index_with_id(variant.def_id);
                            if targets.target_for_value(adt.discriminant_for_variant(tcx, index).val) == exit.edge.successor
                                && selected.replace(i).is_some() { return None; }
                        }
                        selected
                    };
                    exit.arm.or_else(other).or_else(variant_arm).and_then(|i| arms.get(i)).map(|a| a.body)
                }
                _ => None,
            },
            Node::LetStmt(stmt) if exit.arm == Some(0) || exit.arm.is_none() &&
                switches.iter().any(|s| s.edge.switch == exit.edge.switch && s.arm == Some(1)) =>
                stmt.els.and_then(tail),
            _ => None,
        };
        let Some(mut expr) = arm else { continue };
        while let ExprKind::Block(b, _) = expr.kind {
            let Some(e) = tail(b) else { break };
            expr = e;
        }
        let (target, returning) = match expr.kind {
            ExprKind::Continue(dest) => match dest.target_id {
                Ok(id) => (tcx.hir_span(id), false), _ => continue,
            },
            ExprKind::Ret(Some(v)) if matches!(v.kind, ExprKind::Loop(..)) => (body.span, true),
            _ => continue,
        };
        let only_entry = body.basic_blocks.predecessors()[exit.edge.successor].iter()
            .all(|p| *p == exit.edge.switch.block || !runs.contains(p));
        let mut sites = Vec::new();
        let mut bb = exit.edge.successor;
        let mut edge = (exit.edge.switch.block, bb);
        let mut input = exit.edge.switch;
        let mut seen = BTreeSet::new();
        while seen.insert(bb) && !body[bb].is_cleanup {
            let data = &body[bb];
            let loc = body.terminator_loc(bb);
            let on_arm = only_entry && dom.dominates(exit.edge.successor, bb);
            for (i, statement) in data.statements.iter().enumerate() {
                if matches!(statement.kind, StatementKind::StorageDead(_)) && target.contains(statement.source_info.span) {
                    let at = Location { block: bb, statement_index: i };
                    sites.push((at, if on_arm { at } else { input }, on_arm, edge));
                }
            }
            if on_arm { input = loc; }
            if matches!(data.terminator().kind, TerminatorKind::Drop { replace: false, .. })
                && target.contains(body.source_info(loc).span) {
                sites.push((loc, input, on_arm, edge));
            }
            let next = real_successor_blocks(body, bb);
            let reached = if returning { matches!(data.terminator().kind, TerminatorKind::Return) }
                else { matches!(next.as_slice(), [to] if back_edge(*to, bb)) };
            if reached {
                for (at, input, exclusive, edge) in sites {
                    out.entry(at).or_default().push((expr.span.source_callsite(), input, exclusive, edge));
                }
                break;
            }
            if let [to] = next.as_slice() {
                if on_arm { edge = (bb, *to); }
                bb = *to;
                continue;
            }
            // A decision before the continue (e.g. reinitializing in
            // both arms) can converge on the same drop. Advance only
            // when all real successors have a common straight-line suffix.
            let arms: Vec<Vec<_>> = next.iter().map(|&start| {
                let mut path = Vec::new();
                let mut at = start;
                while !path.contains(&at) && !back_edge(at, bb) {
                    path.push(at);
                    let edges = real_successor_blocks(body, at);
                    let [to] = edges.as_slice() else { break };
                    at = *to;
                }
                path
            }).collect();
            let Some(join) = arms.first().and_then(|arm| arm.iter().find(|bb| arms.iter().all(|a| a.contains(bb)))) else { break };
            sites.clear();
            bb = *join;
        }
    }
    out
}

/// Tag each original Drop before rustc elaborates its ladder. Original
/// SourceInfo can be shared; child scopes preserve its lint root and
/// distinguish those drops across CFG simplification. Only switches on
/// elaboration-created Booleans count as flag tests.
fn tested_drop_flags<'tcx>(tcx: TyCtxt<'tcx>, original: &Body<'tcx>) -> BTreeMap<Location, Vec<Place<'tcx>>> {
    let mut body = original.clone();
    let mut owners = BTreeMap::new();
    for (bb, block) in body.basic_blocks.as_mut_preserves_cfg().iter_enumerated_mut() {
        let end = block.terminator_mut();
        if !matches!(end.kind, TerminatorKind::Drop { .. }) { continue; }
        let parent = end.source_info.scope;
        let mut scope = body.source_scopes[parent].clone();
        scope.parent_scope = Some(parent);
        let scope = body.source_scopes.push(scope);
        end.source_info.scope = scope;
        owners.insert(scope.index(), original.terminator_loc(bb));
    }
    rustc_mir_transform::run_analysis_to_runtime_passes(tcx, &mut body);
    // Open Box drops lower their dereference through a fresh pointer
    // temporary. Resolve it back to the Box projection that supplied it.
    let mut pointers = BTreeMap::new();
    for block in body.basic_blocks.iter() {
        for stmt in &block.statements {
            let StatementKind::Assign(a) = &stmt.kind else { continue };
            let Rvalue::Cast(rustc_middle::mir::CastKind::Transmute, Operand::Copy(src), _) = a.1 else { continue };
            let [prefix @ .., ProjectionElem::Field(..), ProjectionElem::Field(..)] = &src.projection[..] else { continue };
            if a.0.local.index() >= original.local_decls.len() && a.0.projection.is_empty() {
                let owner = Place::from(src.local).project_deeper(prefix, tcx);
                if owner.ty(&body.local_decls, tcx).ty.is_box() { pointers.insert(a.0.local, owner); }
            }
        }
    }
    let mut tested = BTreeMap::<_, Vec<_>>::new();
    for block in body.basic_blocks.iter() {
        let end = block.terminator();
        let TerminatorKind::SwitchInt { discr: Operand::Copy(flag), ref targets } = end.kind else { continue };
        if !flag.projection.is_empty() || flag.local.index() < original.local_decls.len()
            || !body.local_decls[flag.local].ty.is_bool() { continue; }
        let Some(&loc) = owners.get(&end.source_info.scope.index()) else { continue };
        if original[loc.block].is_cleanup != block.is_cleanup { continue; }
        let places = tested.entry(loc).or_default();
        // Only operations on the initialized edge before it rejoins the
        // absent edge are guarded. Later drops of siblings share the tag.
        let mut seen = BTreeSet::new();
        let mut work = vec![targets.target_for_value(0)];
        while let Some(bb) = work.pop() {
            let term = body[bb].terminator();
            if seen.insert(bb) && term.source_info.scope == end.source_info.scope {
                work.extend(term.successors().filter(|b| body[*b].is_cleanup == block.is_cleanup));
            }
        }
        work.push(targets.target_for_value(1));
        while let Some(bb) = work.pop() {
            if !seen.insert(bb) { continue; }
            let data = &body[bb];
            let term = data.terminator();
            if term.source_info.scope != end.source_info.scope || data.is_cleanup != block.is_cleanup { continue; }
            let mut record = |p: Place<'tcx>| {
                let p = match pointers.get(&p.local) {
                    Some(owner) if p.projection.first() == Some(&ProjectionElem::Deref) =>
                        owner.project_deeper(p.projection, tcx),
                    _ => p,
                };
                if !places.contains(&p) { places.push(p); }
            };
            if let TerminatorKind::Drop { place, .. } = term.kind { record(place); }
            for stmt in &data.statements {
                if let StatementKind::Assign(a) = &stmt.kind {
                    match a.1 {
                        Rvalue::Ref(_, _, p) if p.ty(&body.local_decls, tcx).ty.is_box() => record(p),
                        Rvalue::Discriminant(p) => record(p),
                        _ => {}
                    }
                }
            }
            work.extend(term.successors().filter(|b| body[*b].is_cleanup == block.is_cleanup));
        }
    }
    tested
}

/// Enumerate rustc's per-index move paths, including expanded subslices.
/// A missing index path inherits its parent's initialization.
fn scope_array<'tcx>(tcx: TyCtxt<'tcx>, rt: &mut Flow<'_, '_, 'tcx>, sites: &[Location],
                     after: bool, root: MovePathIndex, name: &str,
                     moved_at: &BTreeMap<BasicBlock, BTreeSet<MovePathIndex>>) -> Option<ScopeArray> {
    let mut parts = ScopeArray::default();
    let mut work = vec![(Some(root), name.to_owned())];
    while let Some((path, label)) = work.pop() {
        let Some(path) = path else { parts.left.push(label); continue };
        let ty = rt.md.move_paths[path].place.ty(&rt.body.local_decls, tcx).ty;
        let state = rt.state(sites, after, path);
        if !state.may_init {
            parts.moved.push(label);
        } else if state.may_uninit {
            // An initialization covering this slot can replace a moved value.
            // Only use the historical "no branch moved it" wording if none can.
            let parents = ancestors(rt.md, path);
            parts.may_be_refilled |= rt.md.inits.iter().any(|init| {
                let rustc_mir_dataflow::move_paths::InitLocation::Statement(at) = init.location else { return false };
                parents.contains(&init.path) && may_after(moved_at, &|s, l| moved_transfer(rt.md, rt.body, s, l), at, false).contains(&path)
            });
            parts.conditional.push(label);
        } else if state.sub_maybe.is_empty() && state.sub_moved.is_empty() {
            parts.left.push(label);
        } else if let TyKind::Array(_, len) = ty.kind() {
            let len = len.try_to_target_usize(tcx)?;
            let mut indexed = BTreeMap::new();
            let mut child = rt.md.move_paths[path].first_child;
            while let Some(ch) = child {
                let p = &rt.md.move_paths[ch];
                let Some(ProjectionElem::ConstantIndex { offset, from_end: false, .. }) = p.place.projection.last()
                    else { return None };
                indexed.insert(*offset, ch);
                child = p.next_sibling;
            }
            // Gaps have no move path and inherit the initialized parent.
            // Keep short arrays explicit and represent large gaps in constant space.
            let mut gap_end = len;
            let gap = |work: &mut Vec<_>, start: u64, end: u64| {
                if len > 64 && end.saturating_sub(start) > 1 {
                    work.push((None, format!("{label}[{start}..{end}]")));
                } else {
                    work.extend((start..end).rev().map(|i| (None, format!("{label}[{i}]"))));
                }
            };
            for (index, ch) in indexed.into_iter().rev() {
                gap(&mut work, index + 1, gap_end);
                work.push((Some(ch), format!("{label}[{index}]")));
                gap_end = index;
            }
            gap(&mut work, 0, gap_end);
        } else if payloads_gone(tcx, rt, sites, after, path,
                                    rustc_middle::ty::TypingEnv::post_analysis(tcx, rt.body.source.def_id())) {
            if let TyKind::Adt(adt, args) = ty.kind() {
                let children = descendants(rt.md, path);
                let transferred = |at| {
                    let moved = may_after(moved_at, &|s, l| moved_transfer(rt.md, rt.body, s, l), at, after);
                    children.iter().skip(1).any(|ch| moved.contains(ch))
                };
                let previously_moved = |child| rt.md.inits.iter().any(|init| {
                    let rustc_mir_dataflow::move_paths::InitLocation::Statement(at) = init.location else { return false };
                    ancestors(rt.md, child).contains(&init.path) &&
                        may_after(moved_at, &|s, l| moved_transfer(rt.md, rt.body, s, l), at, false).contains(&child)
                });
                if sites.iter().all(|at| children.iter().skip(1).any(|ch| !previously_moved(*ch) && rt.md.moves.iter().any(|mv|
                    mv.path == *ch && (mv.source != *at || after) && mv.source.dominates(*at, rt.body.basic_blocks.dominators()))))
                    && sites.iter().copied().all(transferred) {
                    parts.moved_payload.push(label.clone());
                } else if !sites.iter().copied().any(transferred) && adt.is_enum() {
                    // No payload moved on this edge. If just one variant can
                    // be empty without a move, the discriminant identifies it.
                    let env = rustc_middle::ty::TypingEnv::post_analysis(tcx, rt.body.source.def_id());
                    let variants: Vec<_> = adt.variants().iter().filter(|v|
                        v.fields.iter().all(|f| !f.ty(tcx, args).needs_drop(tcx, env))).collect();
                    if let [v] = &variants[..] { parts.absent_variant.push((label.clone(), v.name.to_string())); }
                }
            }
            parts.empty_payload.push(label);
        } else {
            // A move inside an element is not a move of the entire element.
            return None;
        }
    }
    if parts.moved.is_empty() && parts.conditional.is_empty() && parts.empty_payload.is_empty() { return None; }
    Some(parts)
}

/// An enum discriminant can survive while every droppable payload has
/// gone. An inactive downcast's maybe-uninit bit alone says nothing about
/// a destructor: inspect each variant's fields that actually need drop.
fn payloads_gone<'tcx>(tcx: TyCtxt<'tcx>, rt: &mut Flow<'_, '_, 'tcx>, sites: &[Location],
                            after: bool, path: MovePathIndex, env: rustc_middle::ty::TypingEnv<'tcx>) -> bool {
    let place = rt.md.move_paths[path].place;
    let ty = place.ty(&rt.body.local_decls, tcx).ty;
    let mut field_empty = |base, field, ty: rustc_middle::ty::Ty<'tcx>| {
        if !ty.needs_drop(tcx, env) { return true; }
        let p = tcx.mk_place_field(base, field, ty);
        let LookupResult::Exact(ch) = rt.md.rev_lookup.find(p.as_ref()) else { return false };
        !rt.state(sites, after, ch).may_init || payloads_gone(tcx, rt, sites, after, ch, env)
    };
    match *ty.kind() {
        TyKind::Adt(def, args) if !def.has_dtor(tcx) && !def.is_union() && !def.is_box() =>
            def.variants().iter_enumerated().all(|(v, variant)| {
                let base = if def.is_enum() { tcx.mk_place_downcast(place, def, v) } else { place };
                variant.fields.iter_enumerated().all(|(f, field)| field_empty(base, f, field.ty(tcx, args)))
            }),
        TyKind::Tuple(fields) => fields.iter().enumerate().all(|(i, ty)| field_empty(place, rustc_abi::FieldIdx::from_usize(i), ty)),
        _ => false,
    }
}

fn preceding_call<'tcx>(body: &Body<'tcx>, loc: Location, predicate: impl Fn(&Operand<'tcx>) -> bool) -> Option<Location> {
    let dom = body.basic_blocks.dominators();
    let mut block = loc.block;
    while let Some(pred) = dom.immediate_dominator(block) {
        if let TerminatorKind::Call { func, target: Some(target), .. } = &body.basic_blocks[pred].terminator().kind {
            if dom.dominates(*target, loc.block) && predicate(func) {
                return Some(body.terminator_loc(pred));
            }
        }
        block = pred;
    }
    None
}

fn is_residual_conversion<'tcx>(tcx: TyCtxt<'tcx>, func: &Operand<'tcx>) -> bool {
    func.const_fn_def().is_some_and(|(def, _)| {
        tcx.is_lang_item(def, rustc_hir::LangItem::TryTraitFromResidual)
            || tcx.is_lang_item(def, rustc_hir::LangItem::ResidualIntoTryType)
    })
}

#[cfg(test)]
#[test]
fn skipped_macro_residual_keeps_its_expansion_identity() {
    struct Check;
    impl rustc_driver::Callbacks for Check {
        fn config(&mut self, config: &mut rustc_interface::Config) {
            config.override_queries = Some(rustc_utils::mir::borrowck_facts::override_queries);
        }

        fn after_analysis<'tcx>(&mut self, _: &rustc_interface::interface::Compiler, tcx: TyCtxt<'tcx>) -> rustc_driver::Compilation {
            let id = tcx.hir_body_owners().find(|id| tcx.def_path_str(*id) == "expanded_residuals").unwrap();
            let bwf = rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, id);
            let facts = body_facts(tcx, bwf, "expanded_residuals".into());
            let mut sites = bwf.body.basic_blocks.iter().filter_map(|data| {
                let term = data.terminator();
                let TerminatorKind::Call { func, .. } = &term.kind else { return None };
                is_residual_conversion(tcx, func).then_some(term.source_info.span.with_parent(None))
            }).collect::<Vec<_>>();
            sites.sort_by_key(|site| site.lo());
            let [skipped, live] = sites.as_slice() else { panic!("expected two residual conversions: {sites:?}") };
            // Pre-elaboration MIR contains both calls; only the second is
            // reachable after the unconditional break in the first block.
            assert_eq!(skipped.source_callsite(), live.source_callsite());
            assert_eq!(facts.skipped_residuals, BTreeSet::from([*skipped]));
            assert!(facts.residual_drops.iter().any(|drop| drop.site == *live && drop.path.as_deref() == Some("owner")));
            assert!(facts.residual_drops.iter().all(|drop| drop.site != *skipped));
            rustc_driver::Compilation::Stop
        }
    }

    let sysroot = std::process::Command::new("rustc").args(["--print", "sysroot"]).output().unwrap();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/checked_only_macro_residual_1929.rs");
    let args = ["rustc", path, "--sysroot", std::str::from_utf8(&sysroot.stdout).unwrap().trim(),
        "--edition=2024", "--crate-type=lib", "-Awarnings", "-Zmir-opt-level=0", "-Zmaximal-hir-to-mir-coverage"].map(str::to_owned);
    rustc_driver::run_compiler(&args, &mut Check);
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
pub(crate) fn staged_residual_drop<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, facts: &BodyFacts, assignment: Location) -> Option<StagedDrop> {
    let st = body.basic_blocks[assignment.block].statements.get(assignment.statement_index)?;
    let StatementKind::Assign(a) = &st.kind else { return None };
    let drops = facts.rows.iter().flat_map(|row| &row.facts).any(|event| {
        event.drop_route != DropRoute::Unwind
            && matches!(&event.fact, Fact::TempDrop { place, .. } if *place == TransientPlace::new(tcx, a.0)) &&
            preceding_call(body, event.occurrence.at, |f| is_residual_conversion(tcx, f)).is_some_and(|call|
                !facts.skipped_residuals.contains(&body.source_info(call).span.with_parent(None))
                    && !facts.skipped_closure_returns.contains_key(&call.block))
    });
    let env = TypingEnv::post_analysis(tcx, body.source.def_id());
    drops.then(|| staged_drop_kind(tcx, a.0.ty(&body.local_decls, tcx).ty, env))
}

/// A MIR Drop of the whole value can dispatch to a variant with no drop
/// glue. Use the instantiated place type to describe what it can drop.
fn staged_drop_kind<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>, env: TypingEnv<'tcx>) -> StagedDrop {
    use super::value_flow::always_needs_drop;
    if always_needs_drop(tcx, ty, env) { return StagedDrop::Resource; }
    let TyKind::Adt(def, args) = *ty.kind() else { return StagedDrop::Value };
    if !def.is_enum() { return StagedDrop::Value; }
    let mut fields = def.variants().iter().flat_map(|variant|
        variant.fields.iter().map(move |field| (variant.name, field.ty(tcx, args))))
        .filter(|(_, ty)| ty.needs_drop(tcx, env));
    let Some((variant, payload_ty)) = fields.next() else { return StagedDrop::Value };
    if fields.next().is_some() || !always_needs_drop(tcx, payload_ty, env) { return StagedDrop::Value; }
    let TyKind::Adt(payload, args) = *payload_ty.kind() else { return StagedDrop::Value };
    // Keep the full type in the column's drop note for generic payloads.
    if !args.is_empty() { return StagedDrop::Value; }
    StagedDrop::VariantPayload { variant: variant.to_string(), payload: tcx.item_name(payload.did()).to_string() }
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

/// A reference argument can be a reborrow of the immediately preceding
/// call's result. Require the actual MIR return edge and the same place.
/// A join or another source of the operand leaves the origin unnamed.
pub(crate) fn reference_argument_source<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, call: Span, index: usize) -> Option<(String, Option<&'static str>, bool)> {
    let mut reborrow = None;
    let mut calls = body.basic_blocks.iter_enumerated().filter_map(|(bb, data)| {
        let term = data.terminator();
        let TerminatorKind::Call { args, .. } = &term.kind else { return None };
        term.source_info.span.source_equal(call).then_some((bb, args))
    });
    let (bb, args) = calls.next()?;
    if calls.next().is_some() { return None; }
    let mut local = args.get(index)?.node.place()?.as_local()?;
    if !body.local_decls[local].ty.is_ref() { return None; }
    let [pred] = body.basic_blocks.predecessors()[bb].as_slice() else { return None };
    let TerminatorKind::Call { func, destination, target: Some(target), .. } =
        &body.basic_blocks[*pred].terminator().kind else { return None };

    // Trace only copies and reborrows that reach this operand on the
    // return edge. An indirect write could change a reference via an alias.
    for statement in body.basic_blocks[bb].statements.iter().rev() {
        match &statement.kind {
            StatementKind::Assign(assign) => {
                let (dst, rhs) = &**assign;
                if dst.is_indirect() { return None; }
                if dst.local != local { continue; }
                dst.as_local()?;
                local = match rhs {
                    Rvalue::Use(Operand::Copy(p) | Operand::Move(p)) => p.as_local()?,
                    Rvalue::Ref(_, kind, p) if p.projection.as_slice() == [ProjectionElem::Deref] => {
                        reborrow.get_or_insert(match kind {
                            BorrowKind::Shared => "shared", BorrowKind::Mut { .. } => "mutable", _ => return None,
                        });
                        p.local
                    }
                    _ => return None,
                };
            }
            StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if *l != local => {}
            StatementKind::PlaceMention(_) | StatementKind::FakeRead(_) |
            StatementKind::AscribeUserType(..) | StatementKind::Nop => {}
            _ => return None,
        }
    }
    if *target != bb || destination.as_local()? != local || !body.local_decls[local].ty.is_ref() {
        return None;
    }
    let (callee, _) = func.const_fn_def()?;
    let either_input = callee.as_local().filter(|id| tcx.hir_maybe_body_owned_by(*id).is_some()).is_some_and(|id| {
        let inner = &rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, id).body;
        inner.arg_count == 2 && inner.args_iter().all(|l| inner.local_decls[l].ty.is_ref())
            && super::call_relationships::return_arguments(tcx, inner, None)
                .is_some_and(|origins| origins == inner.args_iter().map(Some).collect())
    });
    Some((tcx.item_name(callee).to_string(), reborrow, either_input))
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

/// An ordinary call may return or unwind. For a call that cannot return,
/// distinguish a propagating panic from process termination or a closed loop.
fn scope_call_can_unwind<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, loc: Location,
                              visiting: &mut Vec<rustc_hir::def_id::DefId>) -> bool {
    let TerminatorKind::Call { func, target, .. } = &body[loc.block].terminator().kind else { return false };
    let ty = func.ty(&body.local_decls, tcx);
    let callee = match *ty.kind() { TyKind::FnDef(id, _) => Some(id), _ => None };
    if !rustc_middle::ty::layout::fn_can_unwind(tcx, callee, ty.fn_sig(tcx).abi()) { return false; }
    let Some(id) = callee else { return true };
    if ["process_exit", "process_abort"].iter().any(|s| tcx.is_diagnostic_item(rustc_span::Symbol::intern(s), id)) { return false; }
    let closure = called_closure(tcx, func);
    // An opaque returning call can unwind. When looking for an unwind
    // route through a diverging body, follow local helpers as well: a
    // call to an empty helper before process::exit cannot start cleanup.
    if target.is_some() && closure.is_none_or(|b| closure_can_return(tcx, b))
        && (visiting.is_empty() || closure.is_none() && !id.is_local()) { return true; }
    use rustc_hir::LangItem::{Panic, PanicFmt, PanicDisplay, BeginPanic};
    if [Panic, PanicFmt, PanicDisplay, BeginPanic].iter().any(|i| tcx.is_lang_item(id, *i)) { return true; }
    let Some(inner) = closure.or_else(|| {
        let local = id.as_local()?;
        tcx.hir_maybe_body_owned_by(local)?;
        Some(&rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, local).body)
    }) else { return false };
    let id = inner.source.def_id();
    if visiting.contains(&id) { return false; }
    visiting.push(id);
    let mut work = vec![rustc_middle::mir::START_BLOCK];
    let mut seen = BTreeSet::new();
    let mut propagates = false;
    while let Some(bb) = work.pop() {
        if !seen.insert(bb) { continue; }
        let term = inner[bb].terminator();
        let escapes = match term.unwind() {
            Some(UnwindAction::Continue) => true,
            Some(UnwindAction::Cleanup(start)) => cleanup_path(inner, *start).last()
                .is_some_and(|end| matches!(inner[end].terminator().kind, TerminatorKind::UnwindResume)),
            _ => false,
        };
        if escapes && match term.kind {
            TerminatorKind::Call { .. } => scope_call_can_unwind(tcx, inner, inner.terminator_loc(bb), visiting),
            TerminatorKind::Assert { .. } => true,
            TerminatorKind::Drop { place, .. } => {
                let env = rustc_middle::ty::TypingEnv::post_analysis(tcx, inner.source.def_id());
                if !place.ty(inner, tcx).ty.needs_drop(tcx, env) { false } else {
                    let md = MoveData::gather_moves(inner, tcx, |_| true);
                    let mut init = MaybeInitializedPlaces::new(tcx, inner, &md)
                        .iterate_to_fixpoint(tcx, inner, None).into_results_cursor(inner);
                    init.seek_before_primary_effect(inner.terminator_loc(bb));
                    match md.rev_lookup.find(place.as_ref()) {
                        LookupResult::Exact(p) | LookupResult::Parent(Some(p)) => init.get().contains(p),
                        LookupResult::Parent(None) => true,
                    }
                }
            }
            _ => false,
        } { propagates = true; break; }
        work.extend(real_successor_blocks(inner, bb));
    }
    visiting.pop();
    propagates
}

/// If dropping the old referent unwinds, rustc still installs the
/// replacement before continuing cleanup. Its staging local is empty
/// from that assignment onward.
fn cleanup_replacement<'tcx>(body: &Body<'tcx>, stmt: &rustc_middle::mir::Statement<'tcx>) -> Option<Local> {
    let StatementKind::Assign(a) = &stmt.kind else { return None };
    let Rvalue::Use(Operand::Move(source)) = a.1 else { return None };
    (a.0.projection.as_slice() == [ProjectionElem::Deref]
        && body.local_decls[a.0.local].ty.ref_mutability() == Some(rustc_hir::Mutability::Mut))
        .then(|| source.as_local()).flatten()
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
            matches!(s.kind, StatementKind::StorageDead(_) | StatementKind::Nop) || cleanup_replacement(body, s).is_some()) {
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
    let mut empty = BTreeSet::new();
    cleanup_path(body, start).filter_map(|bb| {
        let data = &body.basic_blocks[bb];
        empty.extend(data.statements.iter().filter_map(|s| match s.kind {
            StatementKind::StorageDead(local) => Some(local),
            _ => cleanup_replacement(body, s),
        }));
        match data.terminator().kind {
            TerminatorKind::Drop { place, .. } if !empty.contains(&place.local) => Some((body.terminator_loc(bb), place)),
            _ => None,
        }
    }).collect()
}

/// Trace the moves supplying a break result through MIR stores and
/// aggregates. A call's destination is a new value: its moved inputs
/// do not establish what it returns.
fn break_result_moves<'tcx>(
    body: &Body<'tcx>, result: Location, operand_span: Span, region: &BTreeMap<Location, Vec<Location>>,
    mut may_alias: impl FnMut(Place<'tcx>, Location) -> bool,
) -> Vec<(Location, Place<'tcx>)> {
    let mut moves = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut work = vec![(result, None)];
    while let Some((loc, need)) = work.pop() {
        if !region.contains_key(&loc) || !operand_span.contains(body.source_info(loc).span)
            || !seen.insert((loc, need)) { continue; }
        let data = &body[loc.block];
        let stmt = data.statements.get(loc.statement_index);
        if let Some(StatementKind::Assign(assign)) = stmt.map(|s| &s.kind) {
            if need.is_none_or(|place| place_under(place, assign.0) || place_under(assign.0, place)) {
                // Follow whole stores. A projected use of a whole
                // aggregate needs field-sensitive provenance.
                if need.is_some_and(|place| place != assign.0) { continue; }
                let operands: &[Operand<'tcx>] = match &assign.1 {
                    Rvalue::Use(operand) => std::slice::from_ref(operand),
                    Rvalue::Aggregate(_, operands) => operands.raw.as_slice(),
                    _ => &[],
                };
                for operand in operands {
                    let Operand::Move(place) = *operand else { continue; };
                    moves.push((loc, place));
                    work.extend(region[&loc].iter().map(|&prev| (prev, Some(place))));
                }
                continue;
            }
        }
        let Some(place) = need else { continue; };
        let overlaps = |other| place_under(place, other) || place_under(other, place);
        let invalidates = match stmt.map(|s| &s.kind) {
            Some(StatementKind::StorageLive(local) | StatementKind::StorageDead(local)) => *local == place.local,
            Some(StatementKind::Assign(assign)) => assign.0.is_indirect(),
            // A call or destructor can replace an intermediate value
            // through a reference to its slot.
            None => match data.terminator().kind {
                TerminatorKind::Call { destination, .. } => overlaps(destination) || may_alias(place, loc),
                TerminatorKind::Drop { place: dropped, .. } => overlaps(dropped) || may_alias(place, loc),
                TerminatorKind::InlineAsm { .. } => true,
                _ => false,
            },
            _ => false,
        };
        if invalidates {
            continue;
        }
        work.extend(region[&loc].iter().map(|&prev| (prev, Some(place))));
    }
    moves
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

/// A repeated test of the same unchanged Boolean argument cannot take
/// the opposite edge inside a branch that already tested it.
fn simplify_repeated_argument_tests(body: &mut Body<'_>) -> bool {
    use rustc_middle::mir::visit::{PlaceContext, Visitor};

    #[derive(Default)]
    struct Changed(BTreeSet<Local>);
    impl<'tcx> Visitor<'tcx> for Changed {
        fn visit_local(&mut self, local: Local, context: PlaceContext, _: Location) {
            if matches!(context, PlaceContext::MutatingUse(_)) || context.may_observe_address() {
                self.0.insert(local);
            }
        }
    }

    let tests: Vec<_> = body.basic_blocks.iter_enumerated().filter_map(|(bb, data)| {
        let TerminatorKind::SwitchInt { discr, targets } = &data.terminator().kind else { return None };
        let local = discr.place()?.as_local()?;
        let argument = if body.args_iter().any(|arg| arg == local) {
            local
        } else {
            // The temporary is assigned immediately before its test.
            let StatementKind::Assign(assign) = &data.statements.last()?.kind else { return None };
            let Rvalue::Use(Operand::Copy(source)) = assign.1 else { return None };
            if assign.0.as_local() != Some(local) { return None; }
            source.as_local()?
        };
        if !body.args_iter().any(|arg| arg == argument) || !body.local_decls[argument].ty.is_bool() {
            return None;
        }
        Some((bb, argument, targets.target_for_value(0), targets.target_for_value(1)))
    }).collect();
    if tests.len() < 2 { return false; }
    let mut changed = Changed::default();
    changed.visit_body(body);
    let preds = body.basic_blocks.predecessors();
    let doms = body.basic_blocks.dominators();
    let mut replacements = Vec::new();
    for &(bb, argument, no, yes) in &tests {
        if changed.0.contains(&argument) || !doms.is_reachable(bb) { continue; }
        let edge_enters = |from: BasicBlock, target: BasicBlock| {
            preds[target].as_slice() == [from] && doms.dominates(target, bb)
        };
        for &(prior, arg, if_no, if_yes) in &tests {
            if arg != argument || prior == bb || if_no == if_yes || !doms.is_reachable(prior) { continue; }
            if edge_enters(prior, if_no) {
                replacements.push((bb, no));
                break;
            }
            if edge_enters(prior, if_yes) {
                replacements.push((bb, yes));
                break;
            }
        }
    }
    let changed = !replacements.is_empty();
    for (bb, target) in replacements {
        body.basic_blocks_mut()[bb].terminator_mut().kind = TerminatorKind::Goto { target };
    }
    changed
}

/// A comparison on the only edge into a repeated test determines that
/// test when neither input changes between the two evaluations.
fn simplify_adjacent_comparisons(body: &mut Body<'_>) -> bool {
    use rustc_middle::mir::{BasicBlockData, BinOp};
    use rustc_middle::mir::visit::{PlaceContext, Visitor};

    #[derive(Default)]
    struct Inputs {
        addressed: BTreeSet<Local>,
        writes: BTreeSet<(BasicBlock, Local)>,
    }
    impl<'tcx> Visitor<'tcx> for Inputs {
        fn visit_local(&mut self, local: Local, context: PlaceContext, loc: Location) {
            if context.may_observe_address() { self.addressed.insert(local); }
            if matches!(context, PlaceContext::MutatingUse(_)) {
                self.writes.insert((loc.block, local));
            }
        }
    }

    let comparison = |data: &BasicBlockData<'_>| {
        let TerminatorKind::SwitchInt { discr, targets } = &data.terminator().kind else { return None; };
        let StatementKind::Assign(assign) = &data.statements.last()?.kind else { return None; };
        if discr.place()?.as_local()? != assign.0.as_local()? { return None; }
        let Rvalue::BinaryOp(op @ (BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge), operands) = &assign.1 else {
            return None;
        };
        let source = |operand: &Operand<'_>| {
            let local = operand.place()?.as_local()?;
            for st in data.statements[..data.statements.len() - 1].iter().rev() {
                match &st.kind {
                    StatementKind::Assign(assign) if assign.0.as_local() == Some(local) => {
                        let Rvalue::Use(Operand::Copy(place)) = &assign.1 else { return None; };
                        return place.as_local();
                    }
                    StatementKind::Assign(assign) if assign.0.as_local().is_some() => {}
                    StatementKind::StorageLive(other) | StatementKind::StorageDead(other) if *other != local => {}
                    _ => return None,
                }
            }
            None
        };
        Some(((*op, [source(&operands.0)?, source(&operands.1)?]), targets.target_for_value(0), targets.target_for_value(1)))
    };
    let mut inputs = None;
    let mut replacements = Vec::new();
    let preds = body.basic_blocks.predecessors();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        let &[prior] = preds[bb].as_slice() else { continue; };
        if bb == prior { continue; }
        let Some((key, no, yes)) = comparison(data) else { continue; };
        let Some((prior_key, prior_no, prior_yes)) = comparison(&body[prior]) else { continue; };
        if key != prior_key || prior_no == prior_yes { continue; }
        let inputs = inputs.get_or_insert_with(|| {
            let mut inputs = Inputs::default();
            inputs.visit_body(body);
            inputs
        });
        if key.1.iter().any(|&local| inputs.addressed.contains(&local) || inputs.writes.contains(&(bb, local)) || inputs.writes.contains(&(prior, local))) {
            continue;
        }
        replacements.push((bb, if bb == prior_no { no } else { yes }));
    }
    for &(bb, target) in &replacements {
        body.basic_blocks_mut()[bb].terminator_mut().kind = TerminatorKind::Goto { target };
    }
    !replacements.is_empty()
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

    /// Named places moved into a call argument. At a join, follow each real
    /// predecessor to the assignment that supplies the argument on that path.
    fn drop_sources(
        &self, call: Location, operand: Place<'tcx>, runs: &BTreeSet<rustc_middle::mir::BasicBlock>,
    ) -> Vec<(Location, Place<'tcx>)> {
        let mut inputs = Vec::new();
        let mut work = vec![(call, operand)];
        let mut seen = std::collections::HashSet::new();
        while let Some((at, place)) = work.pop() {
            if !seen.insert((at, place)) { continue; }
            if self.is_user(place) {
                inputs.push((at, place));
                continue;
            }
            let Some(local) = place.as_local() else { continue };
            let data = &self.body[at.block];
            let definition = data.statements[..at.statement_index].iter().enumerate().rev().find(|(_, st)| {
                match &st.kind {
                    StatementKind::Assign(a) => a.0.local == local,
                    StatementKind::StorageLive(l) | StatementKind::StorageDead(l) => *l == local,
                    _ => false,
                }
            });
            if let Some((i, st)) = definition {
                if let StatementKind::Assign(a) = &st.kind {
                    if let (true, Rvalue::Use(Operand::Move(source))) = (a.0 == place, &a.1) {
                        work.push((Location { block: at.block, statement_index: i }, *source));
                    }
                }
                continue;
            }
            for pred in super::rows::real_predecessors(self.body, at.block) {
                let data = &self.body[pred];
                if !runs.contains(&pred) || data.is_cleanup
                    || matches!(data.terminator().kind, TerminatorKind::Call { destination, .. } if destination.local == local)
                {
                    continue;
                }
                work.push((self.body.terminator_loc(pred), place));
            }
        }
        inputs
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
            (name, true, _) if name == "<temp>" => "<temporary Vec>".into(),
            (name, _, _) => name,
        }
    }

    /// The flag means the value is inside a vector element. Its fields
    /// aren't fields of the column that owns the vector.
    fn destination(&self, place: Place<'tcx>, loc: Location, depth: usize) -> (String, bool, Option<Place<'tcx>>) {
        if let Some(n) = self.name(place) {
            return (n, false, Some(place));
        }
        if place.local == RETURN_PLACE {
            return ("<return>".into(), false, None);
        }
        if depth > 6 || !place.projection.is_empty() {
            return ("<temp>".into(), false, None);
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
                    best = Some(("<operand>".into(), false, None));
                    continue;
                }
                let field = match rv {
                    Rvalue::Use(o) => in_tmp(o).then_some(None),
                    Rvalue::Cast(_, o, _) => is_tmp(o).then_some(None),
                    Rvalue::Aggregate(k, ops) => ops.iter_enumerated().find(|(_, o)| is_tmp(o))
                        .map(|(f, _)| Some((&**k, f))),
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
                            let (name, _, dest) = self.destination(result, call, depth + 1);
                            (name, true, dest)
                        } else {
                            ("<temp>".into(), false, None)
                        };
                    }
                }
                let (d, element, dest) = self.destination(*lhs, here, depth + 1);
                let closure = matches!(rv, Rvalue::Aggregate(k, _) if matches!(**k, AggregateKind::Closure(..)));
                let member = field.filter(|(k, _)| !element && matches!(k, AggregateKind::Tuple | AggregateKind::Adt(..)));
                let dest = if let Some((kind, field)) = member {
                    dest.and_then(|p| direct_holders::operand_place(self.tcx, self.body, p, kind, field))
                } else {
                    dest
                };
                let name = match field {
                    _ if closure => format!("closure {d}"),
                    _ if member.is_some() && !d.starts_with('<') && !d.ends_with("()") => {
                        let (kind, field) = member.unwrap();
                        let name = match kind {
                            AggregateKind::Adt(id, variant, ..) =>
                                self.tcx.adt_def(*id).variant(*variant).fields[field].name.to_string(),
                            _ => field.index().to_string(),
                        };
                        format!("{d}.{name}")
                    }
                    _ => d,
                };
                best = Some((name, element, dest));
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
                            ("<temp>".into(), false, None)
                        } else {
                            let (name, _, dest) = self.destination(*destination, at, depth + 1);
                            (name, true, dest)
                        }
                    } else {
                        (format!("{}()", self.call_name(*fn_span, func, at)), false, None)
                    });
                }
            }
        }
        best.unwrap_or_else(|| ("<temp>".into(), false, None))
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
        let ev = Ev { location: l, display_location: at, place_local: None, drop_route: DropRoute::Normal, occurrence: Occurrence { at: l, exit: (at != l).then_some(at), successor: None }, end_places: vec![], exact_end_places: vec![], lender_writable: false, continuing_places: vec![], fieldless_operand: None, caller_slot: None, transfer_paths: vec![], col: self.rows.col.get(&at).copied().unwrap_or(0), arm: self.rows.arm_of(at), fact, order };
        (row, ev)
    }
}

/// A None constructor stored directly, or staged immediately before the
/// old value's Drop terminator and installed on its normal return edge.
fn assigned_none<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, at: Location, lhs: Place<'tcx>, rv: &Rvalue<'tcx>) -> bool {
    let none = |rv: &Rvalue<'tcx>| matches!(rv, Rvalue::Aggregate(k, _) if matches!(**k, AggregateKind::Adt(def, variant, ..)
        if tcx.is_lang_item(tcx.adt_def(def).variant(variant).def_id, rustc_hir::LangItem::OptionNone)));
    if none(rv) { return true; }
    let Rvalue::Use(Operand::Move(tmp)) = rv else { return false };
    if at.statement_index != 0 || !tmp.as_local().is_some_and(|l| body.local_kind(l) == rustc_middle::mir::LocalKind::Temp) {
        return false;
    }
    let preds = &body.basic_blocks.predecessors()[at.block];
    !preds.is_empty() && preds.iter().all(|&bb| {
        matches!(body[bb].terminator().kind, TerminatorKind::Drop { place, target, .. } if place == lhs && target == at.block)
            && body[bb].statements.last().is_some_and(|stmt| matches!(&stmt.kind, StatementKind::Assign(a) if a.0 == *tmp && none(&a.1)))
    })
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
        .filter(|p| matches!(&body.basic_blocks[**p].terminator().kind, TerminatorKind::Drop { place: dp, .. } if same_place(*dp, place)))
        .map(|p| Location { block: *p, statement_index: body.basic_blocks[*p].statements.len() })
        .collect();
    if drops.is_empty() { vec![loc] } else { drops }
}

/// In a conditional assignment each arm may initialize the same RHS
/// temporary. Follow its reaching definitions to find how much of the
/// old place survives each arm. Discard a path if the suffix from its
/// definition can change the old place.
fn replacement_inputs<'tcx>(
    rt: &mut Flow<'_, '_, 'tcx>,
    at: Location,
    old: Place<'tcx>,
    rhs: &Rvalue<'tcx>,
    runs: &BTreeSet<BasicBlock>,
) -> Vec<(Span, &'static str)> {
    if !old.projection.is_empty() { return Vec::new(); }
    let Rvalue::Use(Operand::Move(value) | Operand::Copy(value)) = rhs else { return Vec::new() };
    let Some(value) = value.as_local() else { return Vec::new() };
    let LookupResult::Exact(path) = rt.md.rev_lookup.find(old.as_ref()) else { return Vec::new() };
    let body = rt.body;
    let mut seen = BTreeSet::new();
    let mut work = vec![(at, value)];
    let mut inputs = Vec::new();
    while let Some((mut loc, value)) = work.pop() {
        if !seen.insert((loc, value)) { continue; }
        // Loops or large joined expressions need only the shared result.
        if seen.len() > 128 { return Vec::new(); }
        let data = &body[loc.block];
        let mut found = false;
        let mut operand = value;
        for index in (0..loc.statement_index).rev() {
            loc.statement_index = index;
            let stmt = &data.statements[index];
            if let StatementKind::Assign(a) = &stmt.kind {
                if a.0.as_local() == Some(operand) {
                    if let Rvalue::Use(Operand::Move(p) | Operand::Copy(p)) = a.1 {
                        if let Some(l) = p.as_local().filter(|l|
                            *l != old.local && !body.local_decls[*l].is_user_variable()) {
                            operand = l;
                            continue;
                        }
                    }
                    inputs.push((stmt.source_info.span, drop_how(&rt.state(&[loc], true, path))));
                    found = true;
                    break;
                }
            }
            if rt.md.loc_map[loc].iter().any(|m| rt.md.move_paths[rt.md.moves[*m].path].place.local == old.local)
                || rt.md.init_loc_map[loc].iter().any(|i| rt.md.move_paths[rt.md.inits[*i].path].place.local == old.local) {
                return Vec::new();
            }
        }
        if found { continue; }
        let mut has_pred = false;
        for &pred in &body.basic_blocks.predecessors()[loc.block] {
            if !runs.contains(&pred) || body[pred].is_cleanup { continue; }
            let term = body[pred].terminator();
            let boundary = body.terminator_loc(pred);
            match term.kind {
                TerminatorKind::Call { destination, target: Some(target), .. } if target == loc.block => {
                    if destination.as_local() == Some(operand) {
                        inputs.push((term.source_info.span, drop_how(&rt.state(&[boundary], true, path))));
                        has_pred = true;
                        continue;
                    }
                    if destination.local == old.local
                        || rt.md.loc_map[boundary].iter().any(|m| rt.md.move_paths[rt.md.moves[*m].path].place.local == old.local)
                    {
                        return Vec::new();
                    }
                }
                TerminatorKind::Drop { place, .. } if place.local == old.local => return Vec::new(),
                _ => {}
            }
            has_pred = true;
            work.push((boundary, operand));
        }
        if !has_pred { return Vec::new(); }
    }
    inputs
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
}

struct Flow<'a, 'mir, 'tcx> {
    /// Sampling an unwind edge includes operand moves and excludes the call result.
    unwind_from: Option<Location>,
    body: &'mir Body<'tcx>,
    md: &'a MoveData<'tcx>,
    init: ResultsCursor<'mir, 'tcx, MaybeInitializedPlaces<'a, 'tcx>>,
    uninit: ResultsCursor<'mir, 'tcx, MaybeUninitializedPlaces<'a, 'tcx>>,
    ever: ResultsCursor<'mir, 'tcx, EverInitializedPlaces<'a, 'tcx>>,
}

/// Raw init state of one move path over a set of locations.
struct InitState {
    may_init: bool,
    part_may_init: bool,
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

/// A variable's new storage has no assignment yet. When its storage
/// ends, that path no longer describes an in-scope unassigned variable.
/// Whether control can get from `a` to `b` along some path of the body.
fn location_reaches(body: &Body<'_>, a: Location, b: Location) -> bool {
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
}

fn unassigned_transfer(md: &MoveData<'_>, body: &Body<'_>, set: &mut BTreeSet<MovePathIndex>, loc: Location) {
    match body.basic_blocks[loc.block].statements.get(loc.statement_index).map(|s| &s.kind) {
        Some(StatementKind::StorageLive(l)) => {
            if let LookupResult::Exact(root) = md.rev_lookup.find(Place::from(*l).as_ref()) {
                set.extend(descendants(md, root));
            }
        }
        Some(StatementKind::StorageDead(l)) => set.retain(|m| md.move_paths[*m].place.local != *l),
        _ => {}
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
        // MoveData also marks StorageDead as a deinitialization. This
        // transfer tracks moves of values within the local's lifetime.
        return;
    }
    for mo in md.loc_map[loc].iter() {
        set.extend(descendants(md, md.moves[*mo].path));
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
            part_may_init: false,
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
                TerminatorKind::Call { destination, target: Some(_), .. } if after && self.unwind_from.is_none() && l.statement_index == self.body.basic_blocks[l.block].statements.len() => {
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
        s.part_may_init = !sub_init.is_empty();
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
    let mut names = Names::new(tcx, body);
    let switches = super::control::switch_exits(tcx, body, &names.bindings);
    let rows = r.build(&switches);
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
    let borrow_spelling = super::borrow_spelling::BorrowSpelling::collect(tcx, def_id.expect_local());
    let cx = Cx { tcx, body, names, rows, r, vec_results, borrow_spelling };
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
        unwind_from: None,
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
    // Borrow boundaries and display paths still refer to the original MIR
    // locations. The runtime dataflow below can clone a loop entry.
    let real_predecessors = rt_body.basic_blocks.predecessors().clone();
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
    let mut checked = BTreeSet::new();
    {
        let mut work = vec![rustc_middle::mir::START_BLOCK];
        while let Some(bb) = work.pop() {
            if !is_unreachable(body, bb) && !body[bb].is_cleanup && checked.insert(bb) {
                work.extend(body[bb].terminator().successors());
            }
        }
    }
    // Drop elaboration runs before the constant and range refinements.
    // Record a runtime flag even if the executable paths all set it.
    let drop_body = rt_body.clone();
    let drop_md = MoveData::gather_moves(&drop_body, tcx, |_| true);
    let mut drop_init = MaybeInitializedPlaces::new(tcx, &drop_body, &drop_md)
        .iterate_to_fixpoint(tcx, &drop_body, None).into_results_cursor(&drop_body);
    let mut drop_uninit = MaybeUninitializedPlaces::new(tcx, &drop_body, &drop_md)
        .iterate_to_fixpoint(tcx, &drop_body, None).into_results_cursor(&drop_body);
    let mut drop_flags = BTreeSet::new();
    let mut drop_part_flags = BTreeSet::new();
    // Keep the checked CFG for usability and for describing the effects
    // in source arms. Runtime initialization also excludes a switch edge
    // when MIR supplies a constant condition.
    for data in rt_body.basic_blocks_mut().iter_mut() {
        let TerminatorKind::SwitchInt { discr, targets } = &data.terminator().kind else { continue };
        let constant = match discr {
            Operand::Constant(c) => Some(c),
            Operand::Copy(place) | Operand::Move(place) => data.statements.last().and_then(|s| {
                let StatementKind::Assign(a) = &s.kind else { return None };
                let Rvalue::Use(Operand::Constant(c)) = &a.1 else { return None };
                (place == &a.0 && place.as_local().is_some()).then_some(c)
            }),
            _ => None,
        };
        if let Some(value) = constant.filter(|c| c.const_.ty().is_bool())
            .and_then(|c| c.const_.try_eval_bool(tcx, typing_env)) {
            data.terminator_mut().kind = TerminatorKind::Goto { target: targets.target_for_value(u128::from(value)) };
        }
    }
    let runtime_copies = super::runtime::enter_nonempty_ranges(tcx, typing_env, &mut rt_body);
    let mut executes = BTreeSet::new();
    let mut work = vec![rustc_middle::mir::START_BLOCK];
    while let Some(bb) = work.pop() {
        if (bb.index() >= body.basic_blocks.len() || runs.contains(&bb)) && executes.insert(bb) {
            work.extend(rt_body.basic_blocks[bb].terminator().successors());
        }
    }
    // The original MIR locations remain the identities used by source rows.
    // Runtime dataflow must read every reachable execution of each location.
    let runtime_blocks = |bb| std::iter::once(bb)
        .chain(runtime_copies.get(&bb).into_iter().flatten().copied())
        .filter(|b| executes.contains(b));
    let runtime_locations = |loc: Location| runtime_blocks(loc.block)
        .map(move |block| Location { block, ..loc });
    let executes_at = |block| runtime_blocks(block).next().is_some();
    let original_blocks: BTreeMap<_, _> = runtime_copies.iter()
        .flat_map(|(&original, copies)| copies.iter().map(move |&copy| (copy, original))).collect();
    let original_location = |loc: Location| Location {
        block: original_blocks.get(&loc.block).copied().unwrap_or(loc.block),
        ..loc
    };
    let runtime_rows = Rows { body: &rt_body, tcx };
    let executed_successors = |loc: Location| -> BTreeSet<Location> {
        runtime_locations(loc).flat_map(|at| runtime_rows.real_succs(at))
            .filter(|s| executes.contains(&s.block)).map(original_location).collect()
    };
    let rt_md = MoveData::gather_moves(&rt_body, tcx, |_| true);
    let rt_moved_entry = maybe_moved_entry(&rt_body, &rt_md);
    // Keep the row and loan model's executable edges. Repeated Boolean
    // tests refine only the move and next-write explanations.
    let mut wording_body = rt_body.clone();
    let repeated_arguments = simplify_repeated_argument_tests(&mut wording_body);
    let repeated_comparisons = simplify_adjacent_comparisons(&mut wording_body);
    let wording_moves = (repeated_arguments || repeated_comparisons).then(|| maybe_moved_entry(&wording_body, &rt_md));
    let wording_body = if wording_moves.is_some() { &wording_body } else { &rt_body };
    let mut wording_init = repeated_comparisons.then(|| {
        MaybeInitializedPlaces::new(tcx, wording_body, &rt_md).iterate_to_fixpoint(tcx, wording_body, None)
            .into_results_cursor(wording_body)
    });
    let rt_unassigned_entry = maybe_unassigned_entry(&rt_body, &rt_md);
    let mut rt = Flow {
        unwind_from: None,
        body: &rt_body,
        md: &rt_md,
        init: MaybeInitializedPlaces::new(tcx, &rt_body, &rt_md).exclude_inactive_in_otherwise().iterate_to_fixpoint(tcx, &rt_body, None).into_results_cursor(&rt_body),
        uninit: MaybeUninitializedPlaces::new(tcx, &rt_body, &rt_md).mark_inactive_variants_as_uninit().include_inactive_in_otherwise().iterate_to_fixpoint(tcx, &rt_body, None).into_results_cursor(&rt_body),
        ever: EverInitializedPlaces::new(&rt_body, &rt_md).iterate_to_fixpoint(tcx, &rt_body, None).into_results_cursor(&rt_body),
    };
    // A target block can also be reached through another arm. Require
    // this edge to dominate the target before using block dominance to
    // establish which enclosing decisions an issuance depends on.
    let dom = rt_body.basic_blocks.dominators();
    let exclusive_edges: Vec<_> = switches.iter().filter_map(|s| {
        let edge = s.edge;
        let targets: BTreeSet<_> = runtime_locations(edge.switch).flat_map(|at| runtime_rows.real_succs(at))
            .filter(|target| executes.contains(&target.block) && original_location(*target).block == edge.successor)
            .map(|target| target.block).filter(|&target| {
                let mut enters_from_switch = false;
                let exclusive = rt_body.basic_blocks.predecessors()[target].iter().copied()
                    .filter(|p| executes.contains(p)).all(|p| {
                        if dom.dominates(target, p) { return true; }
                        let from_switch = original_location(rt_body.terminator_loc(p)) == edge.switch;
                        enters_from_switch |= from_switch;
                        from_switch
                    });
                exclusive && enters_from_switch
            }).collect();
        (!targets.is_empty()).then_some((edge, targets))
    }).collect();
    let issuance_path = |at: Location| {
        let blocks: Vec<_> = runtime_blocks(at.block).collect();
        exclusive_edges.iter().filter_map(|(edge, targets)| {
            (!blocks.is_empty() && blocks.iter().all(|&block| targets.iter().any(|&target| dom.dominates(target, block))))
                .then_some(*edge)
        }).collect()
    };
    let mut storage = MaybeStorageLive::new(std::borrow::Cow::Owned(always_storage_live_locals(body)))
        .iterate_to_fixpoint(tcx, body, None)
        .into_results_cursor(body);
    let borrows = Borrows::new(tcx, body, &bwf.region_inference_context, &bwf.borrow_set);
    let mut inscope = borrows.iterate_to_fixpoint(tcx, body, None).into_results_cursor(body);
    let mut short_call_borrows = BTreeSet::new();
    let mut other_sites = BTreeSet::new();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        let term = data.terminator();
        let TerminatorKind::Call { args, target: Some(target), .. } = &term.kind else { continue };
        inscope.seek_before_primary_effect(Location { block: *target, statement_index: 0 });
        for (i, arg) in args.iter().enumerate() {
            if arg.node.ty(&body.local_decls, tcx).ref_mutability() != Some(rustc_hir::Mutability::Mut) { continue; }
            let issue = arg.node.place().and_then(|p| super::call_wording::operand_borrow(body, body.terminator_loc(bb), p));
            let short = issue.and_then(|(loc, _)| bwf.borrow_set.location_map().get_index_of(&loc))
                .is_some_and(|idx| !inscope.get().contains(BorrowIndex::from_usize(idx)));
            let site = (term.source_info.span.with_parent(None), i);
            if short {
                short_call_borrows.insert(site);
            } else {
                other_sites.insert(site);
            }
        }
    }
    short_call_borrows.retain(|site| !other_sites.contains(site));


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
        column_info.push(ColumnInfo { local: mp.place.local, name: n.clone(), parent: None, root, kind, copy, needs_drop, is_box: mp.place.ty(&body.local_decls, tcx).ty.is_box(), array_len: array_len(mp.place), hidden_fields: false });
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
    let mut private_scope_fields = Vec::new();
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
            if kids.contains(&i) {
                continue;
            }
            let fp = tcx.mk_place_field(place, rustc_abi::FieldIdx::from_usize(i), fty);
            let Some(n) = cx.name(fp) else { continue };
            // A legacy field column may picture a private field. Describe
            // its scope from its typed projection without publishing a
            // name the source cannot access in the state table.
            if !visible {
                private_scope_fields.push((mpi, fp, n));
                continue;
            }
            let Some((root, _, _)) = cx.names.root(fp) else { continue };
            // (Two variables of one name in sibling scopes share a column
            // name; each keeps its own places.)
            if !column_info.iter().any(|c| c.name == n) {
                let (copy, needs_drop, _) = ty_info(fty);
                column_info.push(ColumnInfo { local: fp.local, name: n.clone(), parent: None, root, kind: "field", copy, needs_drop, is_box: fty.is_box(), array_len: array_len(fp), hidden_fields: false });
            }
            synth.push((mpi, fp, n));
            // (Nested structs and tuples; never through a reference or a
            // Box stored in a field, so the type nesting bounds this.)
            work.push((mpi, fp, false));
        }
    }
    let displayed: std::collections::HashMap<_, _> = columns.iter().chain(&synth)
        .map(|(_, place, name)| ((place.local, &place.projection[..]), name.as_str())).collect();
    let parents: BTreeMap<_, _> = columns.iter().chain(&synth).filter_map(|(_, place, name)| {
        let parent = place.iter_projections().filter_map(|(prefix, _)| {
            displayed.get(&(prefix.local, prefix.projection))
                .copied().filter(|parent| *parent != name)
        }).last()?;
        Some(((place.local, name.as_str()), parent))
    }).collect();
    for column in &mut column_info {
        column.parent = parents.get(&(column.local, column.name.as_str())).map(|p| (*p).to_owned());
    }
    let column_name = |mpi: MovePathIndex| columns.iter().find(|(m, _, _)| *m == mpi).map(|(_, _, n)| n.clone());
    let body_root = tcx.hir_body_owned_by(def_id.expect_local()).value.hir_id;
    let mut drop_moves = BTreeMap::new();
    let mut short_circuit_branches = rustc_data_structures::fx::FxHashSet::default();
    let mut drop_inputs = BTreeMap::<_, Vec<_>>::new();
    for &block in &runs {
        let data = &body[block];
        let call = body.terminator_loc(block);
        let TerminatorKind::Call { func, args, .. } = &data.terminator().kind else { continue };
        if data.is_cleanup || !func.const_fn_def().is_some_and(|(def, _)| tcx.is_diagnostic_item(rustc_span::sym::mem_drop, def)) {
            continue;
        }
        let [arg] = args.as_ref() else { continue };
        let Operand::Move(source) = arg.node else { continue };
        // The compiler's source scope identifies the operand that contains
        // the call. The places and their move sites come from the MIR CFG.
        let mut child = body.location_to_hir_id(call);
        let mut short_circuit = None;
        for (parent, node) in tcx.hir_parent_iter(child) {
            use rustc_hir::{BinOpKind, ExprKind, Node};
            if child == body_root { break; }
            if matches!(node, Node::Expr(e)
                if matches!(e.kind, ExprKind::Binary(op, _, rhs)
                    if matches!(op.node, BinOpKind::And | BinOpKind::Or) && rhs.hir_id == child))
            {
                short_circuit = Some(parent);
                break;
            }
            child = parent;
        }
        for (at, source) in cx.drop_sources(call, source, &runs) {
            // Fields without their own move path are taken by the same
            // move, including reference fields of Copy type.
            if !move_data.loc_map[at].iter().any(|&m| move_data.move_paths[move_data.moves[m].path].place == source) {
                continue;
            }
            let fields: Vec<_> = columns.iter().chain(&synth).filter(|(_, place, _)| {
                place.local == source.local && place.projection.len() > source.projection.len()
                    && place.projection.starts_with(source.projection) && place.ty(&body.local_decls, tcx).ty.is_ref()
            }).collect();
            if fields.is_empty() { continue; }
            drop_inputs.entry(call).or_default().extend(fields.iter().map(|(_, p, _)| (at, *p)));
            if let Some(branch) = short_circuit {
                short_circuit_branches.insert(branch);
                drop_moves.insert(at, DropMove {
                    local: source.local, fields: fields.iter().map(|(_, _, name)| name.clone()).collect(),
                });
            }
        }
    }

    // ---- Polonius, once ---------------------------------------------------------
    let pfacts = bwf.input_facts.as_ref();
    let table = bwf.location_table.as_ref();
    let pol = pfacts.map(|f| Output::compute(f, Algorithm::Naive, true));
    // The hidden iterator of a `for` loop (`let mut iter =
    // IntoIterator::into_iter(<head>)` in the desugaring).
    let mut for_loops = super::iterators::collect(tcx, body, |p| cx.name(p));
    let for_iter = |l: Local| for_loops.iter().any(|it| it.storage == l);
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
                    });
                }
                Rvalue::Aggregate(k, ops) => {
                    // A constant in a struct or tuple literal (`P { text: "x" }`).
                    for (f, o) in ops.iter_enumerated() {
                        let Operand::Constant(c) = o else { continue };
                        if !c.ty().is_ref() {
                            continue;
                        }
                        let Some(slot) = direct_holders::operand_place(tcx, body, *lhs, k, f) else { continue };
                        if !cx.is_user(*lhs) && cx.destination(*lhs, loc, 0).2.is_none() { continue; }
                        // (Into the field's own origins: `"left"` of `("left",
                        // "right")` is held by `p.0` only.)
                        let os = regions_of(decl_place_ty(tcx, body, slot));
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
                            assigned: slot,
                            param: None,
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
    let mut call_relationships = super::call_relationships::collect(tcx, body, &cx.names.bindings, &runs, &checked, &cx.rows.event_row);
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
    // Classify each capture field as we would the captured value. Following
    // moves between bindings would count a restored closure more than once.
    let owner = tcx.local_def_id_to_hir_id(def_id.expect_local()).owner;
    let closure_captures = cx.names.bindings.columns.iter().filter_map(|(name, locals)| {
        let mut counts = locals.iter().map(|&l| match body.local_decls[l].ty.kind() {
            TyKind::Closure(_, args) => Some(args.as_closure().upvar_tys().iter()
                .filter(|ty| !ty.is_ref() && !tcx.type_is_copy_modulo_regions(typing_env, *ty)
                    && !crate::expr_visitor::ty_is_plain(tcx, *ty, owner)).count()),
            _ => None,
        });
        let count = counts.next()??;
        counts.all(|n| n == Some(count)).then(|| (name.clone(), count))
    }).collect();
    let mut binding_loans = BTreeMap::new();
    let reference_slots: Vec<_> = columns.iter().chain(&synth).filter_map(|(_, slot, name)| {
        let TyKind::Ref(_, referent, mutability) = *slot.ty(body, tcx).ty.kind() else { return None };
        Some((*slot, name, ReferenceType {
            mutable: mutability.is_mut(),
            referent_needs_drop: referent.needs_drop(tcx, typing_env),
        }))
    }).collect();
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
        let assigned = bd.assigned_place();
        let coerced = body[loc.block].statements.get(loc.statement_index + 1).is_some_and(|st| {
            matches!(&st.kind, StatementKind::Assign(pair) if matches!(pair.1,
                Rvalue::Ref(_, _, p) if p.local == assigned.local
                    && assigned.projection.is_empty() && !body.local_decls[assigned.local].is_user_variable()
                    && p.projection.as_slice() == [ProjectionElem::Deref, ProjectionElem::Deref]))
        });
        loans.push(LoanInfo {
            issue: Some(*loc),
            issued_on: cx.rows.edge_at(*loc),
            issuance_path: issuance_path(*loc),
            lender_local: Some(bp.local),
            reference_label: cx.names.variant_field_label(tcx, body, bp),
            reference_drop_holders: BTreeSet::new(),
            reference_targets: reference_slots.iter().filter_map(|(slot, name, _)| {
                let target = if place_under(*slot, bp) {
                    if !place_under(bp, *slot) { ReferenceTarget::Ancestor }
                    else if coerced { ReferenceTarget::Coerced }
                    else { ReferenceTarget::Binding }
                } else if place_under(bp, *slot) {
                    match &bp.projection[slot.projection.len()..] {
                        [ProjectionElem::Deref] => ReferenceTarget::Referent,
                        [ProjectionElem::Deref, rest @ ..] if rest.iter().all(|p| *p == ProjectionElem::Deref)
                            && matches!(kind, "mut" | "unique") => ReferenceTarget::NestedReferent,
                        [ProjectionElem::Deref, ProjectionElem::Downcast(_, variant), ProjectionElem::Field(..)]
                            if slot.ty(body, tcx).ty.builtin_deref(true).and_then(|ty| ty.ty_adt_def())
                                .is_some_and(|adt| adt.variant(*variant).fields.len() == 1) =>
                            ReferenceTarget::VariantPayload,
                        [ProjectionElem::Deref, ..] => ReferenceTarget::Part,
                        _ => return None,
                    }
                } else {
                    return None;
                };
                Some(((slot.local, (*name).clone()), target))
            }).collect(),
            loan: idx,
            line: cx.row_of(*loc),
            col: cx.rows.col.get(loc).copied().unwrap_or(0),
            lender,
            projected: cx.names.projected(tcx, body, bp),
            payload: cx.names.payload(tcx, body, bp),
            temp_borrow: bp.as_local().filter(|_| source == "temporary")
                .and_then(|_| bd.assigned_place().as_local()).map(|l| (l, ty_info(bp.ty(body, tcx).ty).1)),
            temp_fields: (source == "temporary" && bp.projection.iter().all(|p| matches!(p, ProjectionElem::Field(..))))
                .then(|| cx.names.name_in_temp(body, bp, String::new())),
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
            direct_locals: BTreeSet::new(),
            direct_holders: Vec::new(),
            holder_stores: Vec::new(),
            via_call: Vec::new(),
            implicit: !cx.borrow_spelling.is_written(tcx, body, *loc),
        });
    }
    for (i, u) in untracked.iter().enumerate() {
        let id = base + i;
        let bi = BorrowIndex::from_usize(id);
        let (direct, in_macro) = match (u.param, u.at) {
            (Some(p), _) => (cx.names.locals[&p].clone(), None),
            (None, Some(loc)) => (cx.dest_name(u.assigned, loc, 0), cx.macro_at(loc)),
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
            issued_on: u.at.and_then(|at| cx.rows.edge_at(at)),
            issuance_path: u.at.map(issuance_path).unwrap_or_default(),
            lender_local: u.place.map(|p| p.local).or(u.param),
            reference_label: None,
            reference_drop_holders: BTreeSet::new(),
            reference_targets: BTreeMap::new(),
            loan: id,
            line,
            col,
            lender: u.lender.clone(),
            temp_fields: None, temp_borrow: None,
            projected: u.place.and_then(|p| cx.names.projected(tcx, body, p)),
            payload: u.place.and_then(|p| cx.names.payload(tcx, body, p)),
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
            direct_locals: BTreeSet::new(),
            direct_holders: Vec::new(),
            holder_stores: Vec::new(),
            via_call: Vec::new(),
            implicit: u.place.is_some() && u.at.is_some_and(|loc| !cx.borrow_spelling.is_written(tcx, body, loc)),
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
    let mut usable_after_release = BTreeSet::new();
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
                    event.1.occurrence = Occurrence { at: loc, exit: Some(call), successor: None };
                    event.1.location = loc;
                    event.1.order = cx.rows.order(loc);
                    event.1.order.2 = 1;
                    event.1.drop_route = DropRoute::Unwind;
                }
                event.1.occurrence.successor = Some(successor);
                // A direct loan of an entire local can give back access at
                // this particular boundary when borrowck permits access on
                // the normal successor and the value is initialized.
                if !diverges && event.1.occurrence.exit.is_none() {
                    if let Some((_, data)) = bwf.borrow_set.location_map().get_index(b.index()) {
                        if let Some(local) = data.borrowed_place().as_local() {
                            // The synthetic origins used to trace a value
                            // through calls aren't borrowck restrictions.
                            if inscope.get().iter().all(|other| {
                                let (_, bd) = bwf.borrow_set.location_map().get_index(other.index()).unwrap();
                                !places_conflict(tcx, body, bd.borrowed_place(), local.into(), PlaceConflictBias::Overlap)
                            })
                                && move_data.rev_lookup.find_local(local).is_some_and(|mpi|
                                    {
                                        let state = flow.state(&[successor], false, mpi);
                                        state.word() == "own" && !state.payload_maybe && !state.payload_moved
                                    })
                            {
                                usable_after_release.insert((b.index(), event.1.occurrence));
                            }
                        }
                    }
                }
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
                ev.1.occurrence.exit = Some(at);
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
    let mut returned_reference_loans = BTreeMap::new();
    let mut closure_result_loans = ClosureResults::default();
    let mut call_holders = BTreeMap::<Location, CallHolders>::new();
    let published: BTreeSet<usize> = loans.iter().map(|l| l.loan).collect();
    let taken_at = |loan: usize| -> Option<Location> {
        if loan < base {
            bwf.borrow_set.location_map().get_index(loan).map(|(at, _)| *at)
        } else {
            untracked.get(loan - base).and_then(|u| u.at)
        }
    };
    let source_slots = super::holder_path::HolderPath {
        tcx, body, runs: &runs,
        borrowed_slots: rustc_mir_dataflow::impls::borrowed_locals(body).iter().collect(),
    };
    let stores = {
        let addressed = super::repoints::addressed_slots(tcx, body);
        let mut preds: BTreeMap<Location, Vec<Location>> = BTreeMap::new();
        for &loc in cx.rows.row.keys().filter(|l| executes_at(l.block)) {
            for next in executed_successors(loc) { preds.entry(next).or_default().push(loc); }
        }
        super::repoints::IncomingStores { tcx, body, preds, addressed, moves: &move_data }
    };
    let mut direct = DirectHolders { stores: &stores, paths: &source_slots, tracked: base, issues: BTreeMap::new() };
    for l in &loans {
        let place = if l.loan < base {
            bwf.borrow_set.location_map().get_index(l.loan).map(|(_, bd)| bd.assigned_place())
        } else {
            untracked.get(l.loan - base).map(|u| u.assigned)
        };
        let Some(place) = place else { continue };
        direct.issues.entry(taken_at(l.loan)).or_default().push((place, l.loan));
    }
    let mut exact_reborrow_parents = BTreeMap::new();
    for loan in loans.iter().filter(|l| l.runs && !l.issuance_path.is_empty() && l.through.is_some()) {
        let Some(at) = loan.issue else { continue };
        let Some(statement) = body[at.block].statements.get(at.statement_index) else { continue };
        let StatementKind::Assign(assign) = &statement.kind else { continue };
        let Rvalue::Ref(_, _, place) = assign.1 else { continue };
        if place.projection.as_slice() != [ProjectionElem::Deref] { continue; }
        let Some(issue) = stores.direct_borrow(at, place.local.into()) else { continue };
        if let Some(id) = bwf.borrow_set.location_map().get_index_of(&issue) {
            exact_reborrow_parents.insert(loan.loan, id);
        }
    }
    let mut joined_argument_loans = BTreeMap::new();
    for &bb in body.basic_blocks.reverse_postorder().iter().filter(|bb| runs.contains(bb)) {
        let TerminatorKind::Call { args, target: Some(_), .. } = &body[bb].terminator().kind else { continue };
        let at = body.terminator_loc(bb);
        for (index, arg) in args.iter().enumerate() {
            if !arg.node.ty(&body.local_decls, tcx).is_ref() { continue; }
            let Some(place) = arg.node.place() else { continue };
            let sources = direct.before(at, place);
            if sources.loans.len() > 1 && !sources.unknown && !sources.unpictured && sources.calls.is_empty() {
                joined_argument_loans.insert((at, index), sources.loans);
            }
        }
    }
    let array_offsets = direct_holders::array_offsets(&stores);
    // Pick the drawn column after resolving the projected place. Array
    // elements and enum payloads share the containing value's column.
    let drawn_slot = |p: Place<'tcx>| {
        let (_, skip, _) = cx.names.root(p)?;
        let n = p.projection.iter().skip(skip).position(|step| !matches!(step, ProjectionElem::Field(..)))
            .map_or(p.projection.len(), |n| skip + n);
        let shown = Place { local: p.local, projection: tcx.mk_place_elems(&p.projection[..n]) };
        Some((shown, cx.name(shown)?))
    };
    let mut installations = Vec::new();
    for &bb in body.basic_blocks.reverse_postorder().iter().filter(|bb| runs.contains(bb)) {
        for (i, stmt) in body[bb].statements.iter().enumerate() {
            let StatementKind::Assign(pair) = &stmt.kind else { continue };
            let at = Location { block: bb, statement_index: i };
            let Some(dst) = stores.slot_before(at, pair.0).filter(|p| cx.is_user(*p)) else { continue };
            let mut slots = Vec::new();
            direct_holders::reference_slots(tcx, body, dst, &array_offsets, &mut slots);
            for place in slots {
                let origins = direct.installed(at, place);
                installations.push(Installation {
                    place, occurrence: Occurrence { at, exit: None, successor: None },
                    complete: !origins.unknown && !origins.unpictured && origins.calls.is_empty(),
                    retained: origins.retained, loans: origins.loans,
                });
            }
        }
    }
    let opaque = |place| {
        let ty = decl_place_ty(tcx, body, place);
        place.is_indirect() || ty.is_closure()
            || ty.ty_adt_def().is_some_and(|adt| adt.is_box() || adt.is_struct() && !adt.did().is_local())
    };
    let mut holder_places: BTreeMap<usize, Vec<(Place<'tcx>, String)>> = BTreeMap::new();
    for (&at, issues) in &direct.issues {
        for &(place, loan) in issues {
            let label = if at.is_none() || place.is_indirect() {
                cx.name(place).map(|n| (place, n))
            } else {
                let (name, _, dest) = cx.destination(place, at.unwrap(), 0);
                dest.filter(|&p| opaque(p)).zip(user_place(&name))
            };
            if let Some(holder) = label { holder_places.entry(loan).or_default().push(holder); }
        }
    }
    for install in &installations {
        let Some((shown, name)) = drawn_slot(install.place) else { continue };
        let alias = if install.place.projection.iter().any(|p| matches!(p, ProjectionElem::Downcast(..)))
            && !install.place.projection.iter().any(|p| matches!(p, ProjectionElem::ConstantIndex { .. }))
        {
            // The aggregate may have a payload label even when its only
            // column is the enclosing enum. Omit the variant in that label.
            let projection = tcx.mk_place_elems_from_iter(install.place.projection.iter()
                .filter(|p| !matches!(p, ProjectionElem::Downcast(..))));
            let mut ty = body.local_decls[install.place.local].ty;
            cx.names.locals.get(&install.place.local).map(|root|
                super::places::name_projections(root.clone(), &mut ty, projection.as_slice()))
        } else { None };
        let kind = const_kind(decl_place_ty(tcx, body, install.place));
        let at = install.occurrence.at;
        let held = table.map_or_else(BTreeSet::new, |t| held_by_place(install.place, t.mid_index(at)));
        for &id in install.loans.iter().chain(install.retained.keys().filter(|id| !install.loans.contains(id) && held.contains(id))) {
            let Some(loan) = loans.iter_mut().find(|l| l.loan == id) else { continue };
            let direct = install.loans.contains(&id);
            if name == loan.lender || !direct && loan.kind != "mut" && !install.retained[&id] { continue; }
            if direct {
                loan.holder_stores.push(HolderStore {
                    place: EndPlace::new(tcx, install.place, name.clone()), occurrence: install.occurrence, kind,
                    complete: install.complete,
                });
            }
            let name = alias.as_ref().unwrap_or(&name);
            let holders = holder_places.entry(id).or_default();
            if !holders.iter().any(|(p, n)| n == name && p.projection.len() == shown.projection.len() && place_under(*p, shown)) {
                holders.push((shown, name.clone()));
            }
        }
    }
    for loan in &mut loans {
        for (p, name) in holder_places.get(&loan.loan).into_iter().flatten() {
            loan.direct_locals.insert(p.local);
            if !loan.direct_holders.contains(name) { loan.direct_holders.push(name.clone()); }
        }
    }
    let mut definite_reborrow_sources = BTreeMap::new();
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
            if l.source == "untracked" && bp.projection.last() == Some(&ProjectionElem::Deref)
                && decl_place_ty(tcx, body, refp).is_ref()
            {
                if let Some(id) = direct.before(loc, refp).definite_loan().filter(|id| v.contains(id)) {
                    definite_reborrow_sources.insert(l.loan, id);
                }
            }
            l.made_through = v;
        }
        let mut callees = std::collections::HashMap::new();
        let mut no_indirect_writes = std::collections::HashMap::new();
        let mut write_sources = Vec::new();
        let mut strong_stores = BTreeMap::<_, Vec<_>>::new();
        let writable_addresses = super::call_stores::writable_addresses(body);
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            let term = data.terminator();
            let TerminatorKind::Call { func, args, target: Some(return_to), .. } = &term.kind else { continue };
            if !runs.contains(&bb) || args.len() < 2 { continue; }
            let loc = Location { block: bb, statement_index: data.statements.len() };
            let callee = super::call_stores::resolved_callee(tcx, body, func.const_fn_def());
            let callee = callee.filter(|_| args.iter().any(|arg| arg.node.place()
                .is_some_and(|p| decl_place_ty(tcx, body, p).is_mutable_ptr())));
            if let Some(def) = callee {
                let writes = callees.entry(def).or_insert_with(|| super::call_stores::argument_stores(tcx, def));
                for write in writes.iter() {
                    let operand = |index: usize| args.get(index).and_then(|a| a.node.place());
                    let (Some(dst), Some(source)) = (operand(write.destination), operand(write.source)) else { continue };
                    let source = if let Some(path) = &write.through_source {
                        let p = resolve_write_deref(tcx, body, tcx.mk_place_deref(source));
                        let Some(p) = super::call_relationships::project(tcx, body, p, path) else { continue };
                        p
                    } else {
                        source
                    };
                    if !decl_place_ty(tcx, body, dst).is_mutable_ptr() { continue; }
                    let argument = resolve_write_deref(tcx, body, tcx.mk_place_deref(dst));
                    let Some(target) = super::call_relationships::project(tcx, body, argument, &write.projection) else { continue };
                    let Some((root, skip, _)) = cx.names.root(target) else { continue };
                    if !column_projection(body, target, skip) { continue; }
                    let Some(holder) = cx.name(target) else { continue };
                    if write.replaces && !target.is_indirect() {
                        strong_stores.entry(loc).or_default().push((target, source));
                    }
                    let kept = held_by_place(target, t.mid_index(loc));
                    let carried = held_by_place(source, t.start_index(loc));
                    let ids: BTreeSet<_> = kept.intersection(&carried).filter(|id| published.contains(id)).copied().collect();
                    if !ids.is_empty() {
                        let sibling_use = target.projection.iter().enumerate().skip(skip).any(|(i, elem)| {
                            if !matches!(elem, ProjectionElem::Field(..)) { return false; }
                            let prefix = Place { local: target.local, projection: tcx.mk_place_elems(&target.projection[..i]) };
                            match decl_place_ty(tcx, body, prefix).kind() {
                                TyKind::Tuple(fields) => fields.len() > 1,
                                TyKind::Adt(def, _) if def.is_struct() => def.non_enum_variant().fields.len() > 1,
                                _ => false,
                            }
                        });
                        let source_name = cx.names.root(source)
                            .filter(|(_, skip, _)| column_projection(body, source, *skip))
                            .and_then(|_| cx.name(source))
                            .or_else(|| {
                                write.through_source.as_ref().filter(|path| path.is_empty())?;
                                let p = resolve_temp_deref(tcx, body, tcx.mk_place_deref(operand(write.source)?));
                                // A pattern binding can expose a slot that has no
                                // column of its own. Name the dereferenced binding.
                                (p.projection.as_slice() == [ProjectionElem::Deref]
                                    && body.local_decls[p.local].is_user_variable()
                                    && body.local_decls[p.local].ty.is_ref()).then(|| cx.name(p)).flatten()
                            });
                        let stores = &mut call_holders.entry(loc).or_default().stores;
                        write_sources.push((loc, stores.len(), source));
                        stores.push(CallStore {
                            argument: write.source,
                            destination_argument: write.destination,
                            source: EndPlace::new(tcx, source, source_name.unwrap_or_default()),
                            target_argument: EndPlace::new(tcx, argument, cx.name(argument).unwrap_or_default()),
                            destination: EndPlace::new(tcx, target, holder),
                            holder: root,
                            sibling_use,
                            loans: ids,
                            replaces: write.replaces,
                            guaranteed: write.guaranteed && (!write.iterator || nonempty_extend_input(&cx, loc, source)),
                            iterator: write.iterator,
                            through_source: write.through_source.is_some(),
                            reference_value: decl_place_ty(tcx, body, source).is_ref(),
                            effective_loan: None,
                            returned_at: return_to.start_location(),
                        });
                    }
                }
            }
            let Some((receiver, inputs)) = args.split_first() else { continue };
            let Some((method, _)) = func.const_fn_def() else { continue };
            if tcx.opt_associated_item(method).is_none_or(|item| !item.is_method()) { continue; }
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
            let tied = super::call_stores::receiver_ties(tcx, body, func.const_fn_def());
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
                        untracked.get(id - base).is_some_and(|u| u.assigned == place)
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
                    let newly_held = !before.contains(&id) && after.contains(&id);
                    let signature_tied = tied.contains(&(index + 1));
                    if !newly_held && !signature_tied { continue; }
                    let mut visited = BTreeSet::new();
                    while let Some(source) = stack.pop() {
                        if !visited.insert(source) { continue; }
                        let Some(l) = loans.iter().find(|l| l.loan == source) else { continue };
                        if l.source == "untracked" && l.through.is_some() && !l.made_through.is_empty() {
                            stack.extend(l.made_through.iter().copied());
                        } else if matches!(l.source, "borrow" | "caller" | "untracked") {
                            let operand = if shared && l.kind == "mut" { StoredOperand::SharedReference } else { operand };
                            stored.push(ReceiverLoan { loan: source, argument: index + 1, operand, newly_held, signature_tied });
                        }
                    }
                }
            }
            if !stored.is_empty() {
                let unchanged = callee.is_some_and(|def| *no_indirect_writes.entry(def).or_insert_with(||
                    super::call_stores::no_indirect_writes(tcx, def)));
                let entry = call_holders.entry(loc).or_default();
                let vec_method = callee.and_then(|def| super::call_stores::vec_method(tcx, def));
                let extends_vec = vec_method.is_some_and(|name| name.as_str() == "extend");
                let stored_arguments = stored.iter().filter_map(|loan| {
                    let arg = inputs.get(loan.argument - 1)?.node.place()?;
                    let known_vec_write = vec_method.is_some_and(|name| matches!(name.as_str(), "push" | "insert"))
                        || extends_vec && nonempty_extend_input(&cx, loc, arg);
                    let proved_write = entry.stores.iter().any(|write|
                        write.guaranteed && write.destination_argument == 0 && write.argument == loan.argument
                            && write.loans.contains(&loan.loan));
                    (known_vec_write || proved_write).then_some(loan.argument)
                }).collect();
                entry.receiver = Some(ReceiverHolds {
                    place: EndPlace::new(tcx, dest, name),
                    loans: stored,
                    unchanged,
                    stored_arguments,
                    extends_vec,
                    operands: Vec::new(),
                });
            }
        }
        for (loc, index, source) in write_sources {
            if !decl_place_ty(tcx, body, source).is_ref() { continue; }
            let Some(issue) = super::call_stores::reaching_issue(tcx, body, &runs, loc, source, &strong_stores, &writable_addresses) else { continue };
            let Some(loan) = loans.iter().find(|loan| loan.issue == Some(issue)) else { continue };
            let store = &mut call_holders.get_mut(&loc).unwrap().stores[index];
            if store.loans.contains(&loan.loan) {
                store.effective_loan = Some(loan.loan);
            }
        }
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            if !runs.contains(&bb) { continue; }
            let term = data.terminator();
            let TerminatorKind::Call { args, destination, target: Some(return_to), .. } = &term.kind else { continue };
            let loc = Location { block: bb, statement_index: data.statements.len() };
            let successor = Location { block: *return_to, statement_index: 0 };
            let before = t.start_index(loc);
            let after = t.start_index(successor);
            let dest_ty = decl_place_ty(tcx, body, *destination);
            let returned = held_by_place(*destination, after);
            let stored = if cx.name(*destination).is_some() {
                Some((*destination, loc))
            } else {
                call_result_store(&cx, *destination, successor, &writable_addresses)
            };
            if let Some((holder, arrival)) = stored {
                let slots = columns.iter().chain(&synth).filter_map(|(_, place, name)| {
                    if place.local != holder.local { return None; }
                    let fields = place.projection.strip_prefix(holder.projection.as_slice())?;
                    let source = destination.project_deeper(fields, tcx);
                    let loans = held_in(&regions_of(decl_place_ty(tcx, body, source)), after)
                        .intersection(&published).copied().collect();
                    Some(CallHolderSlot { place: EndPlace::new(tcx, *place, name.clone()), loans })
                }).collect();
                call_holders.entry(loc).or_default().result = Some(CallHolderResult {
                    destination: EndPlace::new(tcx, holder, cx.name(holder).unwrap_or_default()),
                    arrival,
                    slots,
                    argument_loans: args.iter().filter_map(|arg| arg.node.place())
                        .flat_map(|place| held_by_place(place, before))
                        .filter(|loan| published.contains(loan)).collect(),
                });
            }
            if let TyKind::Ref(region, ..) = *dest_ty.kind() {
                if destination.as_local().is_some_and(|l| cx.names.locals.contains_key(&l)) {
                    if let Some(vid) = region_vid(region) {
                        let held = held_in(&[Origin::from(vid)], t.start_index(return_to.start_location()));
                        if !held.is_empty() { returned_reference_loans.insert(loc, held); }
                    }
                }
                continue;
            }
            if dest_ty.is_closure() { continue; }
            if !args.iter().filter_map(|a| a.node.place()).any(|p| decl_place_ty(tcx, body, p).is_closure()) { continue; }
            // Even a call with no retained loan is a distinct occurrence
            // of a repeated guard. One recoverable result cannot identify
            // the HIR expression if another occurrence has no result facts.
            let sites = closure_result_loans.entry(body.location_to_hir_id(loc)).or_default()
                .entry(destination.local).or_default();
            sites.push(None);
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
            let Some((holder, arrival)) = stored else { continue };
            let (Some(_), Some(&row)) = (cx.name(holder), cx.rows.row.get(&arrival)) else { continue };
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
                *sites.last_mut().unwrap() = Some(ClosureResultLoans {
                    call_location: loc, destination: destination.local, holder: holder.local, arrival, row, holders,
                });
            }
        }
        closure_result_loans.retain(|_, sites| sites.values().flatten().any(Option::is_some));
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
                            // A local callee may return the actual reference
                            // operand unchanged. Its signature alone would
                            // only establish that the result keeps the loan.
                            let forwarded = call_relationships.call(loc)
                                .filter(|call| call.kind == super::call_relationships::CallKind::Direct)
                                .and_then(|call| call.return_sources.as_ref())
                                .and_then(|sources| {
                                    use super::call_relationships::ReturnSource;
                                    let mut sources = sources.iter();
                                    let ReturnSource::Input { index, projection, string_contents: false } = sources.next()? else {
                                        return None;
                                    };
                                    if !projection.is_empty() || sources.next().is_some() { return None; }
                                    args.get(*index)?.node.place()
                                })
                                .is_some_and(|src| {
                                    let ty = destination.ty(&body.local_decls, tcx).ty;
                                    destination.projection.is_empty() && src.projection.is_empty()
                                        && ty.is_ref() && tcx.erase_and_anonymize_regions(ty)
                                            == tcx.erase_and_anonymize_regions(src.ty(&body.local_decls, tcx).ty)
                                        && how.get(&src.local) == Some(&0)
                                        && held_by(src.local, t.start_index(loc)).contains(&l.loan)
                                });
                            set(&mut how, destination.local, u8::from(!forwarded), t.start_index(after));
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
                let root_direct = holder_places.get(&l.loan).is_some_and(|places|
                    places.iter().any(|(p, _)| p.local == *local));
                if root_direct || *n == l.lender {
                    continue;
                }
                if *k == 0 && !matches!(body.local_decls[*local].ty.kind(), TyKind::Ref(..) | TyKind::Tuple(..) | TyKind::Array(..))
                    && !body.local_decls[*local].ty.ty_adt_def().is_some_and(|def| def.is_struct() && def.did().is_local() || def.is_enum())
                {
                    l.direct_locals.insert(*local);
                    l.direct_holders.push(n.clone());
                    holder_places.entry(l.loan).or_default().push((Place::from(*local), n.clone()));
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

    let iterators: Vec<_> = body.local_decls.indices().filter(|l| for_iter(*l) && !cx.names.locals.contains_key(l)).collect();
    let end_provenance = |flow: &mut Flow<'_, '_, 'tcx>, loc: Location, bi: BorrowIndex, li: &LoanInfo, only: Option<Place<'tcx>>| {
        let caller_slot = bi.index().checked_sub(base).and_then(|id| untracked[id].param).and_then(|param| {
            // A mutable binding may already contain a different reference.
            if body.local_decls[param].mutability.is_mut() { return None; }
            let mpi = move_data.rev_lookup.find_local(param)?;
            (flow.state(&[loc], false, mpi).word() == "own")
                .then(|| EndPlace::new(tcx, Place::from(param), cx.names.locals[&param].clone()))
        });
        let mut places = BTreeSet::new();
        let mut transfers = BTreeSet::new();
        let mut exact = BTreeSet::new();
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
                li.via_call.contains(n) || tested.is_some() && li.holders.contains(n)
            };
            let direct_slots = holder_places.get(&li.loan).map(Vec::as_slice).unwrap_or(&[]);
            let belongs = |p: Place<'tcx>| direct_slots.iter().any(|(slot, _)|
                slot.projection.len() == p.projection.len() && place_under(p, *slot));
            let named = |p: Place<'tcx>| {
                if iterators.contains(&p.local) && p.projection.is_empty() { return Some(EndPlace::new(tcx, p, "<iterator>".into())); }
                let name = cx.name(p)?;
                (belongs(p) || li.via_call.contains(&name) || tested.is_some() && li.holders.contains(&name))
                    .then_some(EndPlace::new(tcx, p, name))
            };
            let carries = |p, at| held_by_place(p, t.start_index(at)).contains(&bi.index());
            let mut record = |trace: super::holder_path::Trace| {
                let mut path = trace.steps;
                if let Some(last) = path.first().filter(|_| trace.exact) { exact.insert(last.place.clone()); }
                if let Some(last) = path.first() { places.insert(last.place.clone()); }
                if path.len() > 1 {
                    path.reverse();
                    transfers.insert(path);
                }
            };
            let candidates = columns.iter().chain(&synth)
                .filter(|(_,p,name)| belongs(*p) || can_name(name)).map(|(_,p,_)|*p)
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
                    let mut chain = source_slots.trace(loc, *p, issued, &named, &carries).steps;
                    if chain.iter().skip(1).any(|s| s.place.name == "<iterator>" && places.contains(&s.place)) {
                        chain.reverse();
                        transfers.insert(chain);
                    }
                }
            }
        }
        (places.into_iter().collect::<Vec<_>>(), transfers.into_iter().collect::<Vec<_>>(), caller_slot, exact.into_iter().collect())
    };
    // Direct holders and projected origins are now known. Resolve each
    // edge at its MIR location, including call argument temporaries.
    let drops_at_reassign = |place, target| replacement_store(body, place, target).is_some();
    // Drop-use includes fields destroyed by drop glue, even when the
    // enclosing type has no Drop implementation.
    let mut drop_origins = BTreeMap::<_, Vec<_>>::new();
    if let Some(facts) = pfacts {
        for &(local, origin) in &facts.drop_of_var_derefs_origin {
            drop_origins.entry(local).or_default().push(origin);
        }
    }
    // A pending destructor can keep an origin live beyond the holder's
    // last explicit use. Require the loan in a drop-use origin at the Drop.
    if !reference_slots.is_empty() {
        let mut drop_uses = BTreeMap::<usize, BTreeSet<String>>::new();
        if let Some(t) = table {
            for &bb in &runs {
                let TerminatorKind::Drop { place, .. } = body[bb].terminator().kind else { continue };
                if !place.projection.is_empty() || !place.ty(body, tcx).ty.needs_drop(tcx, typing_env) { continue; }
                let (Some(name), Some(origins)) = (cx.names.locals.get(&place.local), drop_origins.get(&place.local)) else { continue };
                let at = t.start_index(body.terminator_loc(bb));
                for loan in held_in(origins, at) {
                    if out_for(BorrowIndex::from_usize(loan)).is_some_and(|o| o.loans_in_scope_at(at).contains(&BorrowIndex::from_usize(loan))) {
                        drop_uses.entry(loan).or_default().insert(name.clone());
                    }
                }
            }
        }
        for loan in &mut loans {
            loan.reference_drop_holders = drop_uses.remove(&loan.loan).unwrap_or_default();
        }
    }
    let mut drop_holders = BTreeMap::<usize, BTreeSet<String>>::new();
    let mut reassign_drop_holders = BTreeMap::<_, BTreeSet<_>>::new();
    let mut reassign_drop_loans = BTreeMap::<_, BTreeSet<_>>::new();
    let mut explicit_drop_ends = BTreeMap::<_, BTreeSet<_>>::new();
    for ev in evs.values_mut().flatten() {
        let Fact::BorrowEnd { loan, holder, why, .. } = &mut ev.fact else { continue };
        let Some(li) = loans.iter().find(|li| li.loan == *loan) else { continue };
        let (mut places, paths, caller_slot, exact) = end_provenance(&mut flow, ev.location, BorrowIndex::from_usize(*loan), li, None);
        if places.is_empty() && unused.contains(loan) {
            if let Some(param) = untracked[*loan - base].param {
                places.push(EndPlace::new(tcx, Place::from(param), cx.names.locals[&param].clone()));
            }
        }
        if why == "non-returning call" {
            *holder = if places.is_empty() { "<temp>".into() }
                else { places.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join("|") };
        }
        let at = ev.location;
        if let TerminatorKind::Drop { place, target, .. } = body[at.block].terminator().kind {
            let exit = ev.drop_route == DropRoute::Unwind || cx.rows.early_exit.contains_key(&at)
                || cx.rows.shared_exits.contains_key(&at);
            if at == body.terminator_loc(at.block) && place.projection.is_empty() {
                let ty = place.ty(&body.local_decls, tcx).ty;
                if exit && ty.ty_adt_def().is_some_and(|adt| adt.has_dtor(tcx)) {
                    drop_holders.entry(*loan).or_default().extend(
                        places.iter().filter(|p| p.local == place.local.index()).map(|p| p.name.clone())
                    );
                }
                let carries_at_drop = || table.zip(drop_origins.get(&place.local)).is_some_and(|(t, origins)| {
                    held_in(origins, t.start_index(at)).contains(loan)
                });
                if *loan < base && executes_at(at.block) && drops_at_reassign(place, target)
                    && ty.needs_drop(tcx, typing_env) && carries_at_drop()
                {
                    reassign_drop_holders.entry(place.local).or_default().insert(*loan);
                    reassign_drop_loans.entry(at).or_default().insert(*loan);
                }
            }
        }
        if let Some(inputs) = drop_inputs.get(&at) {
            for &(source, field) in inputs {
                let (fields, _, _, _) = end_provenance(&mut flow, source, BorrowIndex::from_usize(*loan), li, Some(field));
                explicit_drop_ends.entry((at, *loan)).or_default().extend(fields);
            }
        }
        ev.caller_slot = caller_slot;
        if let Some(to) = ev.occurrence.successor.filter(|s| executed_successors(ev.location).contains(s)) {
            ev.fieldless_operand = source_slots.fieldless_operand(ev.location, to).and_then(|place|
                Some(EndPlace::new(tcx, place, cx.name(place)?)));
        }
        ev.exact_end_places = exact;
        if let Some((local, successor)) = bwf.borrow_set.location_map().get_index(*loan)
            .and_then(|(_, data)| data.borrowed_place().as_local()).zip(ev.occurrence.successor)
        {
            if let Some(mpi) = move_data.rev_lookup.find_local(local).filter(|_| body.local_decls[local].mutability.is_mut())
            {
                inscope.seek_before_primary_effect(successor);
                ev.lender_writable = flow.state(&[successor], false, mpi).word() == "own"
                    && !inscope.get().iter().any(|id| bwf.borrow_set.location_map().get_index(id.index())
                        .is_some_and(|(_, other)| other.borrowed_place().local == local));
            }
        }
        ev.end_places = places;
        ev.transfer_paths = paths;
    }
    // ---- per-holder ends ---------------------------------------------------------
    // Where a direct or via-call holder stops holding a loan that goes on.
    let mut definite_hold_origins = BTreeMap::<_, BTreeSet<_>>::new();
    if let Some(t) = table {
        let killed: BTreeSet<_> = pfacts.into_iter().flat_map(|f| f.loan_killed_at.iter().copied()).collect();
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
        let mut definite_origins = std::collections::HashMap::new();
        for l in &loans {
            if unused.contains(&l.loan) {
                continue;
            }
            let bi = BorrowIndex::from_usize(l.loan);
            let mut holders = holder_places.get(&l.loan).cloned().unwrap_or_default();
            for (_, p, name) in columns.iter().chain(&synth) {
                if l.via_call.contains(name) && !holders.contains(&(*p, name.clone())) {
                    holders.push((*p, name.clone()));
                }
            }
            let refs: Vec<_> = holders.iter().filter(|(p, _)| decl_place_ty(tcx, body, *p).is_ref()).collect();
            for (hp, h) in &holders {
                let hp = *hp;
                let mut slots = Vec::new();
                direct_holders::reference_slots(tcx, body, hp, &array_offsets, &mut slots);
                for (&loc, &row) in &cx.rows.row {
                    if !runs.contains(&loc.block) {
                        continue;
                    }
                    let held_here = held_by_place(hp, t.start_index(loc)).contains(&l.loan)
                        || held_by_place(hp, t.mid_index(loc)).contains(&l.loan);
                    if !held_here {
                        continue;
                    }
                    let succs = succs_of(loc);
                    let executable = executed_successors(loc);
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
                        f.1.occurrence.successor = Some(s);
                        let (_, paths, _, _) = end_provenance(&mut flow, loc, bi, l, Some(hp));
                        f.1.end_places.push(EndPlace::new(tcx, hp, h.clone()));
                        if executable.contains(&s) {
                            for &slot in &slots {
                                let origin = definite_origins.entry((loc, slot))
                                    .or_insert_with(|| direct.before(loc, slot).definite_loan());
                                if *origin == Some(l.loan) {
                                    definite_hold_origins.entry((f.1.occurrence, l.loan)).or_default()
                                        .insert(super::schema::TransientPlace::new(tcx, slot));
                                }
                            }
                        }
                        // Propagation across this edge requires the same live
                        // reference origin on both ends. The successor alone
                        // can contain a loan introduced by another predecessor.
                        f.1.continuing_places = refs.iter().filter(|_| executable.contains(&s)).filter_map(|(p, n)| {
                            let TyKind::Ref(region, ..) = *decl_place_ty(tcx, body, *p).kind() else { return None };
                            let origin = Origin::from(region_vid(region)?);
                            let out = out_for(bi)?;
                            let carries = |at| out.origin_contains_loan_at.get(&at)?.get(&origin);
                            if !carries(t.mid_index(loc))?.contains(&bi) || !carries(t.start_index(s))?.contains(&bi)
                                || killed.contains(&(bi, t.mid_index(loc))) { return None; }
                            // A Copy field may have no move path of its own.
                            // Its nearest tracked ancestor's initialization
                            // bit excludes whole moves but ignores sibling moves.
                            let mpi = match move_data.rev_lookup.find(p.as_ref()) {
                                LookupResult::Exact(m) | LookupResult::Parent(Some(m)) => m,
                                LookupResult::Parent(None) => return None,
                            };
                            let state = flow.state(&[loc], true, mpi);
                            (state.may_init && !state.may_uninit).then(|| EndPlace::new(tcx, *p, n.clone()))
                        }).collect::<BTreeSet<_>>().into_iter().collect();
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

    let projected_stores: BTreeMap<_, _> = installations.iter().filter_map(|install| {
        let at = install.occurrence.at;
        let StatementKind::Assign(write) = &body[at.block].statements[at.statement_index].kind else { return None };
        (!write.0.projection.is_empty() && write.0.projection.len() == install.place.projection.len())
            .then_some((at, install))
    }).collect();
    let issued: BTreeMap<_, _> = loans.iter().filter_map(|l| Some((taken_at(l.loan)?, l.loan))).collect();
    let doms = body.basic_blocks.dominators();
    let mut issued_operand_loans = BTreeSet::new();
    let mut returned_operand_loans = BTreeSet::new();
    let mut aggregate_loans = std::collections::HashMap::new();
    let mut operand_aggregates = BTreeMap::new();
    let mut variant_constructions = Vec::new();
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
                let mut ids: Vec<AggregateLoan> = Vec::new();
                let variant = match &asg.1 {
                    Rvalue::Aggregate(kind, _) => match &**kind {
                        AggregateKind::Adt(def, index, ..) if tcx.adt_def(*def).is_enum() =>
                            Some((*index, tcx.adt_def(*def).variant(*index))),
                        _ => None,
                    },
                    _ => None,
                };
                let carried = |p, at| {
                    let held = held_by_place(p, t.start_index(at));
                    let name = cx.name(p);
                    // A whole operand can carry its fields' loans. Compare MIR
                    // projections before resolving a holder to its published column.
                    let in_operand = |h: &String| name.as_ref() == Some(h) || columns.iter().chain(&synth).any(|(_, field, label)|
                        label == h && place_under(*field, p));
                    let mut slots = Vec::new();
                    direct_holders::reference_slots(tcx, body, p, &array_offsets, &mut slots);
                    let direct_value = !slots.is_empty() && !opaque(p);
                    let exact: BTreeSet<_> = slots.into_iter().flat_map(|p| direct.before(at, p).loans).collect();
                    loans.iter().filter(|l| l.runs && held.contains(&l.loan)
                        && (exact.contains(&l.loan) || l.via_call.iter().any(in_operand)
                            || name.is_none() || (!direct_value && holder_places.get(&l.loan).is_some_and(|hs|
                                hs.iter().any(|(slot, _)| place_under(*slot, p)))))
                        && !loans.iter().any(|child| held.contains(&child.loan) && child.made_through.contains(&l.loan)))
                        .map(|l| l.loan).collect()
                };
                let selected = |loan: &mut AggregateLoan| {
                    loan.conditional |= taken_at(loan.loan).is_some_and(|at| !doms.dominates(at.block, loc.block));
                };
                let field_loans: Vec<_> = if variant.is_some() {
                    operands.iter().map(|operand| {
                        let mut loans = Vec::new();
                        cx.operand_loans(operand, loc, &issued, &runs, &carried, &mut loans);
                        for loan in &mut loans {
                            selected(loan);
                            if let Some(existing) = ids.iter_mut().find(|prior| prior.loan == loan.loan && prior.issued == loan.issued) {
                                existing.conditional &= loan.conditional;
                            } else {
                                ids.push(*loan);
                            }
                        }
                        loans
                    }).collect()
                } else {
                    for operand in operands {
                        cx.operand_loans(operand, loc, &issued, &runs, &carried, &mut ids);
                    }
                    ids.iter_mut().for_each(selected);
                    Vec::new()
                };
                let span = st.source_info.span;
                let vec_call = array.then(|| cx.vec_results.get(&span.ctxt().outer_expn())).flatten();
                let key = if vec_call.is_some() { span.source_callsite() } else { span }.with_parent(None);
                let fresh = ids.iter().filter(|b| b.issued).map(|b| b.loan);
                issued_operand_loans.extend(fresh.clone());
                if asg.0.as_local() == Some(RETURN_PLACE) {
                    returned_operand_loans.extend(fresh);
                }
                if let Some((index, variant)) = variant {
                    let base = asg.0.project_deeper(&[ProjectionElem::Downcast(Some(variant.name), index)], tcx);
                    let fields = variant.fields.iter_enumerated().zip(operands).zip(field_loans).filter_map(|(((field, _), operand), loans)| {
                        let ty = operand.ty(&body.local_decls, tcx);
                        let place = tcx.mk_place_field(base, field, ty);
                        (!loans.is_empty()).then(|| VariantField {
                            place: EndPlace::new(tcx, place, cx.name(place).unwrap_or_default()),
                            loans,
                        })
                    }).collect();
                    variant_constructions.push(VariantConstruction {
                        location: loc,
                        destination: EndPlace::new(tcx, asg.0, cx.name(asg.0).unwrap_or_default()),
                        variant: index.index(),
                        fields,
                        operands: ids.clone(),
                    });
                }
                let definition = vec_call.map_or(loc, |(call, _)| *call);
                operand_aggregates.entry(definition).or_insert_with(Vec::new).extend(ids.iter().copied());
                aggregate_loans.entry(key).or_insert_with(Vec::new).extend(ids);
            }
        }
    }
    variant_constructions.sort_by_key(|construction| construction.location);
    for (&at, holders) in &mut call_holders {
        let Some(receiver) = &mut holders.receiver else { continue };
        let TerminatorKind::Call { args, .. } = &body[at.block].terminator().kind else { continue };
        let retained: BTreeSet<_> = receiver.loans.iter().filter(|loan| loan.signature_tied).map(|loan| loan.argument).collect();
        for index in retained {
            let Some(place) = args[index].node.place() else { continue };
            if let Some(at) = operand_construction(body, &runs, at, place) {
                receiver.operands.extend(operand_aggregates.get(&at).into_iter().flatten().copied());
            }
        }
    }
    let ldom = |a: Location, b: Location| if a.block == b.block { a.statement_index <= b.statement_index } else { doms.dominates(a.block, b.block) };
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
            let enum_ty = match mty.kind() {
                TyKind::Adt(def, args) if def.is_enum() => Some((def, args)),
                _ => None,
            };
            if let Some((def, args)) = enum_ty {
                // Visit every field needing drop. A missing payload path
                // may still hold a value (e.g. the wildcard in Both(s, _)).
                let root = rt.md.move_paths[m].place;
                for (variant, v) in def.variants().iter_enumerated() {
                    for (f, field) in v.fields.iter_enumerated() {
                        let ty = field.ty(tcx, args);
                        if !ty_info(ty).1 { continue; }
                        let path = descendants(rt.md, m).into_iter().find(|&path| {
                            let p = rt.md.move_paths[path].place;
                            matches!(&p.projection[root.projection.len()..],
                                [ProjectionElem::Downcast(_, index), ProjectionElem::Field(field, _)]
                                if *index == variant && *field == f)
                        });
                        let Some(path) = path else { return how };
                        work.push(path);
                    }
                }
                continue;
            }
            if let TyKind::Array(_, len) = mty.kind() {
                let Some(len) = len.try_to_target_usize(tcx) else { return how };
                // Untracked elements may still be initialized. Prove
                // that moved move paths cover the entire array.
                let mut moved = Vec::new();
                let mut child = rt.md.move_paths[m].first_child;
                while let Some(path) = child {
                    child = rt.md.move_paths[path].next_sibling;
                    if rt.state(at, after, path).may_init { continue; }
                    let range = match *rt.md.move_paths[path].place.projection.last().unwrap() {
                        ProjectionElem::ConstantIndex { offset, from_end, .. } => {
                            let index = if from_end { len - offset } else { offset };
                            (index, index + 1)
                        }
                        ProjectionElem::Subslice { from, to, from_end } =>
                            (from, if from_end { len - to } else { to }),
                        _ => continue,
                    };
                    moved.push(range);
                }
                moved.sort_unstable();
                let mut covered = 0;
                for (start, end) in moved {
                    if start > covered { return how; }
                    covered = covered.max(end);
                }
                if covered < len { return how; }
                continue;
            }
            // A Box frees its allocation even after all payload moves.
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
    // Walk backwards to the last effect on each field. A join can
    // hide that the two fields are initialized on complementary paths,
    // or that only a field store can supply the value still present.
    let field_origins = |paths: &[MovePathIndex], at: &[Location], after: bool| {
        // A bit records fields whose most recent effect still needs
        // finding. The other masks record initialized fields and refills.
        let all = (1u8 << paths.len()) - 1;
        let mut work: Vec<_> = at.iter().map(|&loc| (loc, after, all, 0u8, 0u8)).collect();
        let mut seen = BTreeSet::new();
        let mut outcomes = BTreeSet::new();
        let covers = |a: MovePathIndex, b: MovePathIndex| {
            let a = rt_md.move_paths[a].place;
            let b = rt_md.move_paths[b].place;
            a.local == b.local && b.projection.starts_with(a.projection)
        };
        while let Some((loc, inclusive, mut pending, mut held, mut refilled)) = work.pop() {
            if !seen.insert((loc, inclusive, pending, held, refilled)) { continue; }
            if seen.len() > 4096 { return None; }
            if inclusive {
                for (i, &path) in paths.iter().enumerate() {
                    let bit = 1 << i;
                    if pending & bit == 0 { continue; }
                    if let Some(init) = rt_md.init_loc_map[loc].iter().map(|&i| rt_md.inits[i].path).find(|&p| covers(p, path)) {
                        pending &= !bit;
                        held |= bit;
                        if init == path { refilled |= bit; }
                    } else if rt_md.loc_map[loc].iter().any(|&m| covers(rt_md.moves[m].path, path)) {
                        pending &= !bit;
                    }
                }
            }
            if pending == 0 {
                outcomes.insert((held, refilled));
            } else if loc.statement_index > 0 {
                work.push((Location { statement_index: loc.statement_index - 1, ..loc }, true, pending, held, refilled));
            } else if loc == Location::START {
                // Any unresolved field of an argument was initialized by the caller.
                for (i, &path) in paths.iter().enumerate() {
                    let bit = 1 << i;
                    if pending & bit != 0 && rt_md.move_paths[path].place.local.index() <= body.arg_count {
                        held |= bit;
                    }
                }
                outcomes.insert((held, refilled));
            } else {
                work.extend(rt_body.basic_blocks.predecessors()[loc.block].iter()
                    .filter(|p| executes.contains(p) && !rt_body.basic_blocks[**p].is_cleanup)
                    .map(|&p| (rt_body.terminator_loc(p), true, pending, held, refilled)));
            }
        }
        (!outcomes.is_empty()).then_some(outcomes)
    };
    // Alongside the drop facts, retain the field explanation for each
    // enclosing aggregate, including nested columns with no Drop of
    // their own. A lost field and all its descendants count only once.
    let rt_doms = rt_body.basic_blocks.dominators();
    let tested_flags = std::cell::OnceCell::new();
    let tests_flag = |loc: Location, place: Place<'tcx>| {
        tested_flags.get_or_init(|| tested_drop_flags(tcx, body)).get(&loc).is_some_and(|guarded| {
            matches!(body[loc.block].terminator().kind, TerminatorKind::Drop { place: root, .. } if same_place(root, place))
                || guarded.iter().any(|&p| place_under(p, place) || place_under(place, p))
        })
    };
    let variant_drops = |rt: &mut Flow<'_, '_, 'tcx>, at: &[Location], after: bool, drop_at: Option<Location>, root: MovePathIndex, name: &str| {
        let place = rt_md.move_paths[root].place;
        let ty = place.ty(&body.local_decls, tcx).ty;
        let TyKind::Adt(def, args) = *ty.kind() else { return Vec::new() };
        if !def.is_enum() || !ty_info(ty).1 { return Vec::new(); }
        let moved: BTreeSet<_> = at.iter().flat_map(|&loc|
            may_after(&rt_moved_entry, &|st, l| moved_transfer(&rt_md, &rt_body, st, l), loc, after)).collect();
        let state = rt.state(at, after, root);
        if !state.may_init || state.may_uninit { return Vec::new(); }
        // Like ElaborateDrops::collect_drop_flags, use the paths which
        // are both maybe initialized and maybe uninitialized at the
        // actual Drop, including enum downcasts. An exit sharing that
        // Drop can still know more about what reaches it.
        let drops: Vec<_> = drop_at.into_iter().flat_map(runtime_locations).collect();
        let flagged = rt.state(&drops, false, root).sub_maybe;
        let downcasts: BTreeMap<_, _> = descendants(&rt_md, root).into_iter().filter_map(|path| {
            match &rt_md.move_paths[path].place.projection[place.projection.len()..] {
                [ProjectionElem::Downcast(_, variant)] => Some((*variant, path)),
                _ => None,
            }
        }).collect();
        // If a downcast is always initialized, that is the only variant
        // reaching this exit. A known inactive variant has no move to report.
        let active = downcasts.iter().find_map(|(&variant, &path)| {
            let st = rt.state(at, after, path);
            (st.may_init && !st.may_uninit).then_some(variant)
        });
        def.variants().iter_enumerated().filter_map(|(variant, data)| {
            if active.is_some_and(|active| active != variant) { return None; }
            let downcast = downcasts.get(&variant).copied();
            let payload = match data.fields.raw.as_slice() {
                [] => VariantPayload::Empty,
                [field] => {
                    let ty = field.ty(tcx, args);
                    let (copy, needs_drop, _) = ty_info(ty);
                    if copy && ty.is_primitive() {
                        VariantPayload::Copy(ty.to_string())
                    } else {
                        if !needs_drop { return None; }
                        let downcast = downcast?;
                        let path = rt_md.move_paths[downcast].first_child?;
                        if !moved.contains(&path) { return None; }
                        let st = rt.state(at, after, path);
                        let TyKind::Adt(def, _) = ty.kind() else { return None; };
                        let ty = tcx.item_name(def.did()).to_string();
                        if !st.may_init {
                            VariantPayload::Moved(ty)
                        } else if flagged.contains(&path) && st.may_uninit && st.sub_moved.is_empty() && st.sub_maybe.is_empty() {
                            VariantPayload::Conditional(ty)
                        } else {
                            return None;
                        }
                    }
                }
                _ => return None,
            };
            let tracks_variant_init = matches!(payload, VariantPayload::Moved(_)) && downcast.is_some_and(|path| {
                flagged.contains(&path) && state.sub_maybe.contains(&path) && drop_at.is_some_and(|loc| tests_flag(loc, rt_md.move_paths[path].place))
            });
            let payload_flag = downcast.is_some_and(|path| drop_at.is_some_and(|loc| tests_flag(loc, rt_md.move_paths[path].place)));
            Some(VariantDrop { column: name.to_owned(), variant: data.name.to_string(), payload, tracks_variant_init, payload_flag })
        }).collect()
    };
    let array_indices = |place: Place<'tcx>, part: Place<'tcx>, len: usize| {
        if part.local != place.local || !part.projection.starts_with(place.projection) {
            return None;
        }
        match part.projection[place.projection.len()..] {
            [ProjectionElem::ConstantIndex { offset, from_end, .. }] => {
                let i = if from_end { len - offset as usize } else { offset as usize };
                Some(i..i + 1)
            }
            [ProjectionElem::Subslice { from, to, from_end }] =>
                Some(from as usize..if from_end { len - to as usize } else { to as usize }),
            _ => None,
        }
    };
    let partial_drops = |rt: &mut Flow<'_, '_, 'tcx>, at: &[Location], after: bool, drop_at: Option<Location>, top: MovePathIndex, early_exit: bool| {
        let state = rt.state(at, after, top);
        if !state.ever_init || (state.may_init && state.sub_moved.is_empty() && state.sub_maybe.is_empty()) {
            return Vec::new();
        }
        let move_paths = &rt_md.move_paths;
        let field_key = |m: MovePathIndex| -> Vec<_> {
            move_paths[m].place.projection.iter().filter_map(|p| match p {
                ProjectionElem::Field(f, _) => Some(f.index()),
                _ => None,
            }).collect()
        };
        let mut out = Vec::new();
        for m in descendants(&rt_md, top) {
            let place = move_paths[m].place;
            let ty = place.ty(&body.local_decls, tcx).ty;
            let (copy, needs_drop, _) = ty_info(ty);
            if let Some(len) = array_len(place).filter(|&len| early_exit && needs_drop && len <= super::schema::MAX_NAMED_ARRAY_LEN) {
                let Some((_, _, name)) = columns.iter().find(|(_, p, _)| *p == place) else { continue };
                let st = rt.state(at, after, m);
                if !st.sub_moved.is_empty() && st.sub_maybe.is_empty() && st.may_init && !st.may_uninit {
                    let moved: Option<Vec<_>> = st.sub_moved.iter()
                        .map(|&part| array_indices(place, move_paths[part].place, len)).collect();
                    if let Some(moved) = moved {
                        let moved: BTreeSet<_> = moved.into_iter().flatten().collect();
                        let dropped: Vec<_> = (0..len).filter(|i| !moved.contains(i))
                            .map(|i| format!("{name}[{i}]")).collect();
                        let split = PartialDrop {
                            moved: moved.into_iter().map(|i| format!("{name}[{i}]")).collect(),
                            drops_resource: !dropped.is_empty(),
                            dropped,
                            ..PartialDrop::default()
                        };
                        out.push((place.local, name.clone(), split));
                    }
                }
                continue;
            }
            if copy || !(ty.is_box() || matches!(ty.kind(), TyKind::Tuple(_))
                || matches!(ty.kind(), TyKind::Adt(def, _) if def.is_struct() || def.is_enum()))
            {
                continue;
            }
            let Some((_, _, name)) = columns.iter().find(|(_, p, _)| *p == place) else { continue };
            let st = rt.state(at, after, m);
            // Moving the entire Box transfers its allocation as well.
            if ty.is_box() && !st.may_init {
                continue;
            }
            let moved_tuple = !st.may_init && matches!(ty.kind(), TyKind::Tuple(_));
            if !st.ever_init || st.may_init && st.may_uninit
                || !moved_tuple && st.sub_moved.is_empty() && st.sub_maybe.is_empty()
            {
                continue;
            }
            let definitely_moved: BTreeSet<_> = st.sub_moved.iter().copied().collect();
            let unavailable: BTreeSet<_> = st.sub_moved.iter().chain(&st.sub_maybe).copied().collect();
            let fields = field_drops(rt, at, after, m);
            let hidden = column_info.iter().any(|col| col.hidden_fields
                && columns.iter().any(|(_, p, n)| *n == col.name && p.local == place.local
                    && p.projection.starts_with(place.projection)));
            let drops_resource = needs_drop && rest_how(rt, at, after, m) != "nothing_to_drop";
            let mut split = PartialDrop { flag: drop_at.is_some_and(|loc| tests_flag(loc, place)), drops_resource, incomplete: hidden && drops_resource, ..PartialDrop::default() };
            let mut flagged_fields = Vec::new();
            let mut observed_enums = BTreeSet::new();
            for (paths, conditional) in [(st.sub_moved, false), (st.sub_maybe, true)] {
                let mut paths = paths;
                paths.sort_by_key(|&ch| field_key(ch));
                for ch in paths {
                    let field = move_paths[ch].place;
                    if !matches!(field.projection.last(), Some(ProjectionElem::Field(..)))
                        || ancestors(&rt_md, ch).into_iter().any(|p| p != ch && (definitely_moved.contains(&p) || conditional && unavailable.contains(&p))
                            && matches!(move_paths[p].place.projection.last(), Some(ProjectionElem::Field(..))))
                    {
                        continue;
                    }
                    let field_needs_drop = ty_info(field.ty(&body.local_decls, tcx).ty).1;
                    if !field_needs_drop && needs_drop && (conditional || ty.is_box()) {
                        continue;
                    }
                    let column = columns.iter().find(|(_, p, _)| *p == field);
                    if let Some((_, _, name)) = column {
                        let names = if conditional { &mut split.conditional } else { &mut split.moved };
                        names.push(name.clone());
                        if conditional && field_needs_drop && move_paths[ch].first_child.is_none() {
                            flagged_fields.push(ch);
                        }
                    } else if field.projection.iter().any(|p| matches!(p, ProjectionElem::Downcast(..))) {
                        // The enum field can still hold its discriminant. Use
                        // its named parent for the moved payload explanation.
                        if let Some((_, parent, name)) = columns.iter()
                            .filter(|(_, p, _)| p.local == field.local && field.projection.starts_with(p.projection)
                                && p.projection.len() >= place.projection.len())
                            .max_by_key(|(_, p, _)| p.projection.len())
                        {
                            let parent = match rt_md.rev_lookup.find(parent.as_ref()) {
                                LookupResult::Exact(path) => Some(path),
                                LookupResult::Parent(_) => None,
                            };
                            if let Some(parent) = parent {
                                observed_enums.insert((parent, name));
                            }
                            if parent.is_some_and(|p| rest_how(rt, at, after, p) == "nothing_to_drop") {
                                if !split.payloads_empty.contains(name) {
                                    split.payloads_empty.push(name.clone());
                                }
                                // Inactive variants appear uninitialized too.
                                // Retain a definite move explanation only if
                                // a real transfer precedes this scope end.
                                if rt_md.moves.iter().any(|mv| {
                                    if mv.path != ch || !executes.contains(&mv.source.block) { return false; }
                                    let precedes = at.iter().all(|&at| {
                                        if mv.source.block == at.block {
                                            mv.source.statement_index < at.statement_index || after && mv.source == at
                                        } else {
                                            rt_doms.dominates(mv.source.block, at.block)
                                        }
                                    });
                                    precedes && matches!(rt_body.basic_blocks[mv.source.block].statements.get(mv.source.statement_index).map(|st| &st.kind),
                                        Some(StatementKind::Assign(asg)) if matches!(asg.1, Rvalue::Use(Operand::Move(_))))
                                }) && !split.payloads_moved.contains(name) {
                                    split.payloads_moved.push(name.clone());
                                }
                            } else {
                                let names = if conditional { &mut split.payloads_conditional } else { &mut split.payloads_moved };
                                if !names.contains(name) {
                                    names.push(name.clone());
                                }
                                split.incomplete = true;
                            }
                        } else {
                            split.incomplete = true;
                        }
                    }
                }
            }
            for (_, _, name, how) in fields {
                if how == "drops" && columns.iter().chain(&synth).any(|(_, p, n)| p.local == place.local && *n == name) {
                    if !split.dropped.contains(&name) {
                        split.dropped.push(name);
                    }
                } else if how != "nothing_to_drop" {
                    split.incomplete |= !split.conditional.contains(&name);
                }
            }
            if drops_resource && !ty.is_box() && split.dropped.is_empty() && split.conditional.is_empty()
                && split.payloads_conditional.is_empty()
            {
                // A partially moved enum can retain a payload with no column
                // or move path. A missing field name says nothing about its drop.
                split.incomplete = true;
            }
            if moved_tuple {
                // Moving the tuple as a whole transfers every owned field.
                split.moved = ty.tuple_fields().iter().enumerate().filter(|(_, ty)| !ty_info(*ty).0)
                    .map(|(i, _)| format!("{name}.{i}")).collect();
            }
            if (1..=2).contains(&flagged_fields.len()) {
                if let Some(outcomes) = field_origins(&flagged_fields, at, after) {
                    split.one_of_two = flagged_fields.len() == 2 && split.conditional.len() == 2
                        && outcomes.iter().all(|(held, _)| held.count_ones() == 1)
                        && outcomes.iter().any(|(held, _)| *held == 1)
                        && outcomes.iter().any(|(held, _)| *held == 2);
                    for (i, &field) in flagged_fields.iter().enumerate() {
                        let bit = 1 << i;
                        if outcomes.iter().any(|(held, _)| held & bit != 0)
                            && outcomes.iter().all(|(held, refill)| held & bit == refill & bit)
                        {
                            split.refilled.extend(cx.name(move_paths[field].place));
                        }
                    }
                }
            }
            for (root, name) in observed_enums {
                split.variant_drops.extend(variant_drops(rt, at, after, drop_at, root, name));
            }
            if split.moved.is_empty() && split.conditional.is_empty()
                && split.payloads_moved.is_empty() && split.payloads_conditional.is_empty()
                && split.payloads_empty.is_empty() && !split.incomplete
            {
                continue;
            }
            for empty in split.payloads_empty.iter().filter(|empty| *empty != name) {
                out.push((place.local, empty.clone(), PartialDrop {
                    flag: columns.iter().any(|(_, p, n)| n == empty && p.local == place.local && drop_at.is_some_and(|loc| tests_flag(loc, *p))),
                    payloads_empty: vec![empty.clone()],
                    payloads_moved: split.payloads_moved.iter().filter(|name| *name == empty).cloned().collect(),
                    variant_drops: split.variant_drops.iter().filter(|drop| drop.column == *empty).cloned().collect(),
                    ..PartialDrop::default()
                }));
            }
            out.push((place.local, name.clone(), split));
        }
        out
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
                work.extend(real_predecessors[at.block].iter()
                    .filter(|p| runs.contains(p) && !body.basic_blocks[**p].is_cleanup)
                    .map(|p| body.terminator_loc(*p)));
            }
        }
        DropRoute::ExitOnly
    };
    let mut scope_partials: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut binding_sources = Vec::new();
    // Resolve staging locals through single definitions. A projected
    // write is ambiguous except for rustc's initialization of a vec!
    // buffer, which is only followed through its conversion to Vec.
    let mut definitions = BTreeMap::new();
    let mut vec_payloads = BTreeMap::new();
    let vec_buffers: std::collections::HashSet<_> = body.basic_blocks.iter().filter_map(|b| cx.r.vec_buffer(b.terminator())).collect();
    let vec_conversion = tcx.get_diagnostic_item(rustc_span::Symbol::intern("box_assume_init_into_vec_unsafe"));
    let box_new = tcx.get_diagnostic_item(rustc_span::sym::box_new);
    for data in body.basic_blocks.iter() {
        for st in &data.statements {
            if let StatementKind::Assign(asg) = &st.kind {
                if !cx.names.locals.contains_key(&asg.0.local) {
                    let dest = asg.0;
                    if vec_buffers.contains(&Place::from(dest.local)) && dest.projection.first() == Some(&ProjectionElem::Deref) {
                        let buffer = body.local_decls[dest.local].ty.boxed_ty().unwrap();
                        let payload = match &asg.1 {
                            Rvalue::Aggregate(kind, ops) if matches!(**kind, AggregateKind::Array(_))
                                && matches!(buffer.kind(), TyKind::Adt(_, args)
                                    if args.type_at(0) == dest.ty(&body.local_decls, tcx).ty) => Some(ops),
                            _ => None,
                        };
                        vec_payloads.entry(dest.local).and_modify(|def| *def = None).or_insert(payload);
                        continue;
                    }
                    definitions.entry(asg.0.local).and_modify(|def| *def = None)
                        .or_insert(asg.0.projection.is_empty().then_some(TempDefinition::Value(&asg.1)));
                }
            }
        }
        if let TerminatorKind::Call { func, args, destination, .. } = &data.terminator().kind {
            let def = match (destination.as_local(), func.const_fn_def(), args.as_ref()) {
                (Some(_), _, _) if vec_buffers.contains(destination) => Some(TempDefinition::VecBuffer),
                (Some(_), Some((id, _)), [arg]) => match arg.node {
                    Operand::Move(p) if Some(id) == box_new => Some(TempDefinition::Box(p)),
                    Operand::Move(p) if Some(id) == vec_conversion => Some(TempDefinition::Vec(p)),
                    _ => None,
                },
                _ => None,
            };
            definitions.entry(destination.local).and_modify(|prev| *prev = None).or_insert(def);
        }
    }
    let source_place = |mut p: Place<'tcx>| {
        let mut seen = BTreeSet::new();
        while !cx.is_user(p) && seen.insert(p.local) {
            let (op, suffix) = match definitions.get(&p.local).copied().flatten()? {
                TempDefinition::Value(Rvalue::Use(op)) => (op, &p.projection[..]),
                TempDefinition::Value(Rvalue::Aggregate(kind, ops)) if matches!(**kind, AggregateKind::Tuple) => {
                    let (ProjectionElem::Field(f, _), rest) = p.projection.split_first()? else { return None };
                    (ops.get(*f)?, rest)
                }
                _ => return None,
            };
            p = op.place()?.project_deeper(suffix, tcx);
        }
        cx.is_user(p).then_some(p)
    };
    let mut exposed = ExposedTemps::default();
    exposed.visit_body(body);
    let vec_elements = |mut buffer: Place<'tcx>| {
        let mut seen = BTreeSet::new();
        while let Some(local) = buffer.as_local().filter(|&l| seen.insert(l) && !exposed.0.contains(&l)) {
            match definitions.get(&local).copied().flatten()? {
                TempDefinition::VecBuffer => return vec_payloads.get(&local).copied().flatten(),
                TempDefinition::Value(Rvalue::Use(Operand::Move(source))) => buffer = *source,
                _ => return None,
            }
        }
        None
    };
    let staged_sources = |place: Place<'tcx>| {
        let mut work = vec![(place, ResidualStaging::Operand)];
        let mut seen = std::collections::HashSet::new();
        let mut sources = Vec::new();
        while let Some((p, staging)) = work.pop() {
            if !seen.insert((p, staging)) || !ty_info(p.ty(&body.local_decls, tcx).ty).1 { continue; }
            if cx.is_user(p) {
                if column_projection(body, p, 0) { sources.push((p, staging)); }
                continue;
            }
            if exposed.0.contains(&p.local) { continue; }
            let Some(def) = definitions.get(&p.local).copied().flatten() else { continue };
            let mut follow = |op: &Operand<'tcx>, suffix, staging| {
                if let Operand::Move(source) = op {
                    work.push((source.project_deeper(suffix, tcx), staging));
                }
            };
            match def {
                TempDefinition::Value(Rvalue::Use(op)) => follow(op, p.projection, staging),
                TempDefinition::Value(Rvalue::Repeat(op, len))
                    if p.projection.is_empty() && len.try_to_target_usize(tcx) == Some(1) =>
                {
                    follow(op, &[], ResidualStaging::Constructed);
                }
                TempDefinition::Value(Rvalue::Aggregate(kind, ops)) => {
                    let mut projection = &p.projection[..];
                    match **kind {
                        AggregateKind::Tuple | AggregateKind::Array(_) => {}
                        AggregateKind::Adt(did, variant, ..) => {
                            let adt = tcx.adt_def(did);
                            if adt.is_union() || adt.has_dtor(tcx) { continue; }
                            if !projection.is_empty() && adt.is_enum() {
                                let [ProjectionElem::Downcast(_, v), rest @ ..] = projection else { continue };
                                if *v != variant { continue; }
                                projection = rest;
                            }
                        }
                        _ => continue,
                    }
                    match projection {
                        [] => ops.iter().for_each(|op| follow(op, &[], ResidualStaging::Constructed)),
                        [ProjectionElem::Field(field, _), rest @ ..] => {
                            if let Some(op) = ops.get(*field) { follow(op, rest, staging); }
                        }
                        _ => {}
                    }
                }
                TempDefinition::Box(source) if p.projection.is_empty() => work.push((source, ResidualStaging::Constructed)),
                TempDefinition::Vec(buffer) if p.projection.is_empty() => {
                    if let Some(ops) = vec_elements(buffer) {
                        ops.iter().for_each(|op| follow(op, &[], ResidualStaging::Constructed));
                    }
                }
                _ => {}
            }
        }
        sources
    };
    let residual_site = |at: Location| {
        let span = body.source_info(at).span;
        if !span.is_desugaring(rustc_span::DesugaringKind::QuestionMark) { return None; }
        let is_conversion = |loc: Location| loc == body.terminator_loc(loc.block)
            && matches!(&body[loc.block].terminator().kind, TerminatorKind::Call { func, .. }
                if is_residual_conversion(tcx, func));
        let call = if is_conversion(at) { at } else {
            preceding_call(body, at, |f| is_residual_conversion(tcx, f))?
        };
        Some((call, body.source_info(call).span.with_parent(None)))
    };
    let try_span = |at| residual_site(at).map(|(_, site)| site.source_callsite());
    let residual_runs = |call: Location| executes_at(call.block) && !skipped_closure_returns.contains_key(&call.block);
    let mut residual_drops = Vec::new();
    let mut record_residual = |rt: &mut Flow<'_, '_, 'tcx>, place: Place<'tcx>, site, at, after,
        flagged: &dyn Fn(Place<'tcx>) -> bool| {
        let LookupResult::Exact(root) = rt_md.rev_lookup.find(place.as_ref()) else { return };
        let at: Vec<_> = runtime_locations(at).collect();
        if at.is_empty() { return; }
        let moved: BTreeSet<_> = at.iter().flat_map(|&loc|
            may_after(&rt_moved_entry, &|st, l| moved_transfer(&rt_md, &rt_body, st, l), loc, after)).collect();
        let only_assigned = |path| !descendants(&rt_md, path).iter().any(|p| moved.contains(p));
        let how = rest_how(rt, &at, after, root);
        residual_drops.push(ResidualDrop {
            local: place.local, site, path: cx.is_user(place).then(|| cx.name(place)).flatten(),
            how, only_assigned: only_assigned(root), flag: flagged(place), staged: None,
        });
        if !cx.is_user(place) {
            // Some arguments already own the moved operand when a later
            // argument's question mark exits. The staged value's drop
            // is described on the source column.
            if how == "drops" {
                for (source, staging) in staged_sources(place) {
                    let ty = tcx.erase_and_anonymize_regions(source.ty(&body.local_decls, tcx).ty);
                    residual_drops.push(ResidualDrop {
                        local: source.local, site, path: cx.name(source), how, only_assigned: false, flag: false,
                        staged: Some((staging, staged_drop_kind(tcx, ty, typing_env))),
                    });
                }
            }
            return;
        }
        for (_, field, path) in columns.iter().chain(&synth).filter(|(_, p, _)| {
            p.local == place.local && p.projection.len() > place.projection.len()
                && p.projection.starts_with(place.projection)
                && p.projection[place.projection.len()..].iter().all(|p| matches!(p, ProjectionElem::Field(..) | ProjectionElem::Deref))
        }) {
            if !ty_info(field.ty(&body.local_decls, tcx).ty).1 { continue; }
            let (field_how, only_assigned) = match rt_md.rev_lookup.find(field.as_ref()) {
                LookupResult::Exact(path) => (rest_how(rt, &at, after, path), only_assigned(path)),
                LookupResult::Parent(Some(parent)) => {
                    // An untracked field holds a value exactly when its
                    // nearest tracked parent is initialized.
                    let state = rt.state(&at, after, parent);
                    let field_how = match (state.may_init, state.may_uninit) {
                        (true, false) => "drops",
                        (true, true) => "drops_if_init",
                        _ => "nothing_to_drop",
                    };
                    (field_how, only_assigned(parent))
                }
                LookupResult::Parent(None) => continue,
            };
            residual_drops.push(ResidualDrop { local: place.local, site, path: Some(path.clone()), how: field_how, only_assigned, flag: flagged(*field), staged: None });
        }
    };
    let scope_site = |at| try_span(at).unwrap_or_else(|| cx.r.exit_span(at));
    // A local ends its owned fields. An untracked field inherits its
    // ancestor's shallow init bit. Dropping a reference leaves its referent.
    let mut scope_ends = ScopeEnds::default();
    let tree = tcx.region_scope_tree(def_id.expect_local());
    #[expect(clippy::iter_over_hash_type, reason = "the one-to-one declaration map is inverted into a BTreeMap")]
    for (&id, &local) in &cx.names.bindings.declarations {
        if let Some(scope) = tree.var_scope(id.local_id) {
            let span = scope.span(tcx, tree);
            scope_ends.boundaries.insert(local, span);
        }
    }
    let mut live_storage: BTreeSet<_> = body.args_iter().collect();
    let mut declared_storage = BTreeSet::new();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        for stmt in &data.statements {
            if let StatementKind::StorageLive(local) = stmt.kind {
                if checked.contains(&bb) { declared_storage.insert(local); }
                if executes_at(bb) { live_storage.insert(local); }
            }
            if let StatementKind::StorageDead(local) = stmt.kind {
                if checked.contains(&bb) && !executes_at(bb)
                    && scope_ends.boundaries.get(&local).is_some_and(|span| stmt.source_info.span.hi() == span.hi())
                {
                    scope_ends.unreached_boundaries.insert(local);
                }
            }
        }
    }
    // After an unconditional transfer, rustc can retain a debug binding
    // without emitting any storage statements for it in borrowck MIR.
    scope_ends.unreachable.extend(scope_ends.boundaries.keys().copied()
        .filter(|l| !live_storage.contains(l)).map(|l| (l, declared_storage.contains(&l))));
    // The runtime view: a cloned loop entry executes beside the original
    // block, and a constant condition removes edges. Scope ends read every
    // execution of a location, not only the original's.
    let executing: BTreeSet<_> = executes.iter().copied()
        .chain(executes.iter().filter_map(|bb| original_blocks.get(bb).copied())).collect();
    let mut folded = folded_scope_exits(tcx, &rt_body, &switches, &executing, body);
    // A creation belongs to a branch when only that switch edge can reach
    // its MIR location. Decisions outside the extending let do not count.
    let conditional_creation = |init, sites: &[Location]| {
        let Some((&first, rest)) = sites.split_first() else { return false };
        issuance_path(first).iter().any(|edge| executed_successors(edge.switch).len() > 1
            && switches.iter().find(|s| s.edge == *edge).and_then(|s| s.branch)
                .is_some_and(|branch| branch == init || tcx.hir_parent_id_iter(branch).any(|p| p == init))
            && rest.iter().all(|&at| issuance_path(at).contains(edge)))
    };
    let runtime_sites = |at| runtime_locations(at).collect::<Vec<_>>();
    let (mut temps, promoted_operands) = temporaries::collect(&cx, &mut rt, runtime_sites,
        &bwf.promoted.raw, conditional_creation, temp_holders, for_iter);
    folded.retain(|at, exits| {
        let block = &rt_body[at.block];
        let local = match block.statements.get(at.statement_index).map(|s| &s.kind) {
            Some(StatementKind::StorageDead(local)) => *local,
            None => match block.terminator().kind { TerminatorKind::Drop { place, .. } => place.local, _ => return false },
            _ => return false,
        };
        // An earlier nested scope may have its own drop on the way to
        // the jump. It is already dead when the jump executes.
        let scope = scope_ends.boundaries.get(&local).copied().or_else(|| temps.get(&local)?.scope);
        exits.retain(|(site, _, _, _)| scope.is_some_and(|s| s.contains(*site)));
        !exits.is_empty()
    });
    let mut leg_entries = BTreeMap::<Span, BTreeSet<_>>::new();
    let spans = switches.iter().filter_map(|s| {
        let (owner, index) = (s.branch?, s.arm?);
        let (_, targets) = exclusive_edges.iter().find(|(edge, _)| *edge == s.edge)?;
        let span = match tcx.hir_node(owner) {
            rustc_hir::Node::Expr(e) => match e.kind {
                rustc_hir::ExprKind::If(_, yes, no) => if index == 0 { yes.span } else { no?.span },
                rustc_hir::ExprKind::Match(_, arms, _) => arms.get(index)?.body.span,
                _ => return None,
            },
            rustc_hir::Node::LetStmt(stmt) if index == 0 => stmt.els?.span,
            _ => return None,
        };
        Some((span, targets))
    });
    for (span, targets) in spans {
        leg_entries.entry(span).or_default().extend(targets);
    }
    let mut record_scope = |rt: &mut Flow<'_, '_, 'tcx>, local: Local, loc: Location,
                            sites: &[Location], after: bool, site: Option<Span>, storage: bool| {
        if sites.is_empty() { return; }
        // Test the executable copies of both the arm entry and the end.
        let legs = scope_legs(dom, &leg_entries, sites);
        let boundary = cx.r.line_hi(cx.r.span_of(loc));
        let row = site.map_or(boundary, |span| cx.r.line_lo(span));
        let mut parts = match rt_md.rev_lookup.find(Place::from(local).as_ref()) {
            LookupResult::Exact(root) => {
                let mut inputs = sites.iter().map(|at| partial_drops(rt, std::slice::from_ref(at), after, (!storage).then_some(loc), root, false));
                let mut common = inputs.next().unwrap_or_default();
                for input in inputs { common.retain(|part| input.contains(part)); }
                common
            }
            _ => Vec::new(),
        };
        for (_, place, name) in columns.iter().chain(&synth).chain(&private_scope_fields).filter(|(_, p, _)| p.local == local) {
            if place.iter_projections().any(|(prefix, elem)| elem == ProjectionElem::Deref
                && !prefix.ty(&body.local_decls, tcx).ty.is_box()) { continue; }
            let needs_drop = ty_info(place.ty(&body.local_decls, tcx).ty).1;
            let (path, exact) = match rt_md.rev_lookup.find(place.as_ref()) {
                LookupResult::Exact(p) => (p, true),
                LookupResult::Parent(Some(p)) => (p, false),
                LookupResult::Parent(None) => continue,
            };
            let state = rt.state(sites, after, path);
            if rt.unwind_from.is_some() && !state.may_init { continue; }
            let mut end = ScopeEnd {
                optional_unwind: rt.unwind_from.is_some_and(|at| matches!(&body[at.block].terminator().kind,
                    TerminatorKind::Call { target, func, .. } if func.const_fn_def().is_none()
                        || target.is_some() && !closure_has_no_return(tcx, func))),
                partial: parts.iter().position(|(l, col, _)| *l == local && col == name).map(|i| parts.swap_remove(i).2),
                array: None, legs: legs.clone(), span: cx.r.span_of(loc), boundary, row, site, effect: ScopeDrop::Storage, flag: false, flag_on_some_paths: false,
                field_order: place.projection.iter().filter_map(|e| match e {
                    ProjectionElem::Field(f, _) => Some(f.index()), _ => None,
                }).collect(),
                moved: false, unassigned: false, has_value: state.may_init,
                box_contents_need_drop: place.ty(&body.local_decls, tcx).ty.boxed_ty().map(|ty| ty_info(ty).1),
                payload_moved: exact && state.may_init && state.payload_moved, payload_maybe: exact && state.may_init && state.payload_maybe,
            };
            if needs_drop && !storage {
                end.effect = if exact {
                    ScopeDrop::from_how(rest_how(rt, sites, after, path))
                } else {
                    match (state.may_init, state.may_uninit) {
                        (true, true) => ScopeDrop::Conditional,
                        (true, false) => ScopeDrop::Whole,
                        _ => ScopeDrop::Empty,
                    }
                };
                end.flag = tests_flag(loc, *place);
                let ty = place.ty(&body.local_decls, tcx).ty;
                if exact && state.may_init && !state.may_uninit {
                    if matches!(ty.kind(), TyKind::Array(..)) && drop_how(&state) == "drops_rest" {
                        end.array = scope_array(tcx, rt, sites, after, path, name, &rt_moved_entry);
                        if end.array.as_ref().is_some_and(|parts| parts.conditional.is_empty() && parts.left.is_empty()) {
                            end.effect = ScopeDrop::Empty;
                        }
                    } else if end.effect == ScopeDrop::Remaining && ty.ty_adt_def().is_some_and(|d| d.is_enum())
                        && payloads_gone(tcx, rt, sites, after, path, typing_env) {
                        end.effect = ScopeDrop::Empty;
                    }
                }
                if end.effect == ScopeDrop::Remaining {
                    // Ignore no-glue fields and inactive variant bits (e.g. Err on a failed let Ok).
                    let missing: BTreeSet<_> = state.sub_moved.iter().chain(&state.sub_maybe).copied()
                        .filter(|&p| ty_info(rt_md.move_paths[p].place.ty(&body.local_decls, tcx).ty).1).collect();
                    if missing.is_empty() {
                        end.effect = ScopeDrop::Whole;
                    } else if ty.ty_adt_def().is_some_and(|d| d.is_enum()) && !sites.iter().any(|&at| {
                        let moved = may_after(&rt_moved_entry, &|st, l| moved_transfer(&rt_md, &rt_body, st, l), at, after);
                        !moved.is_disjoint(&missing)
                    }) {
                        end.effect = ScopeDrop::Whole;
                        end.partial = None;
                    }
                }
                if state.may_uninit {
                    for &at in sites {
                        end.moved |= may_after(&rt_moved_entry, &|st, l| moved_transfer(&rt_md, &rt_body, st, l), at, after).contains(&path);
                        end.unassigned |= may_after(&rt_unassigned_entry, &|st, l| unassigned_transfer(&rt_md, &rt_body, st, l), at, after).contains(&path);
                    }
                }
            }
            scope_ends.insert(local, name, end);
        }
    };
    // Read the state at each incoming exit, then cut those same CFG
    // inputs from the ordinary continuation. Return ends argument storage
    // in exactly the same way that StorageDead ends a local.
    let boundary_inputs = |local, loc| {
        let mut out = Vec::new();
        let exit = cx.rows.exit_site.get(&loc).copied();
        let shared = cx.rows.shared_exits.get(&loc).map(Vec::as_slice).unwrap_or_default();
        let folded = folded.get(&loc).map(Vec::as_slice).unwrap_or_default();
        let source = |at| (scope_site(at), at, None);
        for &(site, input, exclusive, (from, to)) in folded {
            if !shared.iter().any(|(_, at)| scope_site(*at) == site) && exit.map(scope_site) != Some(site) {
                out.push((runtime_locations(input).collect(), !exclusive, Some((site, body.terminator_loc(from), Some(to.start_location())))));
            }
        }
        for &(_, at) in shared {
            out.push((runtime_locations(at).collect(), true, Some(source(at))));
        }
        if cx.rows.exit_only.contains(&loc) || folded.iter().any(|e| e.2) || drop_route(loc, shared) == DropRoute::ExitOnly { return out; }
        if shared.is_empty() && folded.is_empty() {
            out.push((runtime_locations(loc).collect(), false, exit.map(source)));
        } else {
            let exits = shared.iter().flat_map(|(_, at)| runtime_locations(*at)).collect();
            let edges = folded.iter().map(|e| e.3).collect();
            let mut incoming: Vec<_> = runtime_locations(loc)
                .flat_map(|at| continuing_drop_inputs(&rt_body, local, at, &exits, &edges, &executes, &original_blocks)).collect();
            incoming.sort();
            incoming.dedup();
            if !incoming.is_empty() { out.push((incoming, true, exit.map(source))); }
        }
        out
    };
    // Arguments have no StorageDead. Return ends their storage.
    let no_glue_args: Vec<_> = body.args_iter().filter(|arg| cx.names.locals.contains_key(arg)
        && !ty_info(body.local_decls[*arg].ty).1).collect();
    let mut terminal_calls = BTreeMap::<Local, Vec<(Span, usize)>>::new();
    let mut writes = BTreeMap::<Span, Vec<AssignmentWrite>>::new();
    let mut replacement_sites = BTreeMap::<Location, Vec<Location>>::new();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        if data.is_cleanup { continue; }
        let TerminatorKind::Drop { place, target, replace, .. } = data.terminator().kind else { continue };
        if let Some(store) = replacement_store(body, place, target).filter(|store|
            replace || body.source_info(*store).span == data.terminator().source_info.span) {
            replacement_sites.entry(store).or_default().push(body.terminator_loc(bb));
        }
    }
    let write_column = |place: Place<'tcx>| {
        cx.names.root(place).filter(|(_, skip, _)| column_projection(body, place, *skip))?;
        let payload = if let Some(ProjectionElem::Deref) = place.projection.last() {
            let prefix = &place.projection[..place.projection.len() - 1];
            let owner = Place::from(place.local).project_deeper(prefix, tcx);
            owner.ty(&body.local_decls, tcx).ty.is_box().then_some(owner)
        } else {
            None
        };
        Some(WriteColumn {
            local: place.local, name: cx.name(payload.unwrap_or(place))?, payload: payload.is_some(),
        })
    };
    let vec_buffers: std::collections::HashSet<_> = body.basic_blocks.iter().filter_map(|b| cx.r.vec_buffer(b.terminator())).collect();
    for (&loc, _) in &cx.rows.row {
        let data = &body.basic_blocks[loc.block];
        let statement = data.statements.get(loc.statement_index);
        if let Some((st, asg)) = statement.and_then(|st| match &st.kind {
            StatementKind::Assign(asg) => Some((st, asg)), _ => None,
        }) {
            let (lhs, rv) = &**asg;
            if !data.is_cleanup && (cx.is_user(*lhs) || lhs.is_indirect()) {
                let resolved = resolve_write_deref(tcx, body, *lhs);
                let dest = write_column(resolved);
                let drop_at = replacement_sites.get(&loc).and_then(|sites| match sites.as_slice() {
                    [at] => Some(*at),
                    _ => None,
                });
                let replacing = drop_at.and_then(|at| {
                    let sites: Vec<_> = runtime_locations(at).collect();
                    if sites.is_empty() { return None; }
                    let exact = |place: Place<'tcx>| match rt_md.rev_lookup.find(place.as_ref()) {
                        LookupResult::Exact(path) => Some(path),
                        LookupResult::Parent(_) => None,
                    };
                    let flag = |place| tests_flag(at, place);
                    let moved: BTreeSet<_> = sites.iter().flat_map(|&site|
                        may_after(&rt_moved_entry, &|st, l| moved_transfer(&rt_md, &rt_body, st, l), site, false)).collect();
                    let may_have_moved = |place: Place<'tcx>| {
                        let path = match rt_md.rev_lookup.find(place.as_ref()) {
                            LookupResult::Exact(path) => Some(path),
                            LookupResult::Parent(parent) => parent,
                        };
                        path.is_some_and(|path| moved.contains(&path))
                    };
                    let moved_columns = columns.iter().chain(&synth).filter_map(|(_, field, _)| {
                        if same_place(*field, *lhs) || !place_under(*field, *lhs) { return None; }
                        may_have_moved(*field).then(|| write_column(*field)).flatten()
                    }).collect();
                    let emptied_box = |rt: &mut Flow<'_, '_, 'tcx>, place: Place<'tcx>, how| {
                        how == "drops_rest" && place.ty(&body.local_decls, tcx).ty.is_box()
                            && exact(tcx.mk_place_deref(place)).is_some_and(|p| !rt.state(&sites, false, p).may_init)
                    };
                    let how = exact(*lhs).map_or("drops", |path| rest_how(&mut rt, &sites, false, path));
                    let box_parts = lhs.ty(&body.local_decls, tcx).ty.is_box() && how == "drops_rest";
                    let box_payload_maybe_moved = box_parts && exact(tcx.mk_place_deref(*lhs)).is_some_and(|p| {
                        let state = rt.state(&sites, false, p);
                        state.may_init && state.may_uninit && ty_info(tcx.mk_place_deref(*lhs).ty(&body.local_decls, tcx).ty).1
                    });
                    let contents_gone = emptied_box(&mut rt, *lhs, how);
                    let how = if contents_gone { "drops" } else { how };
                    let flag_for_whole = how != "nothing_to_drop" && flag(*lhs);
                    let parts = exact(*lhs).map_or_else(Vec::new, |path| partial_drops(&mut rt, &sites, false, Some(at), path, false));
                    let detail = |place: Place<'tcx>, how| {
                        let split = cx.name(place).and_then(|name| parts.iter().find(|(local, label, _)|
                            *local == place.local && *label == name).map(|(_, _, part)| part.clone()));
                        split.or_else(|| (how == "drops_rest").then(|| PartialDrop {
                            flag: flag(place), drops_resource: true, incomplete: true, ..PartialDrop::default()
                        }))
                    };
                    let partial = if contents_gone { None } else { detail(*lhs, how) };
                    let fields = if how == "nothing_to_drop" || contents_gone || dest.is_none() {
                        Vec::new()
                    } else {
                        columns.iter().chain(&synth).filter_map(|(_, field, _)| {
                            if same_place(*field, resolved) || !place_under(*field, resolved)
                                || !ty_info(field.ty(&body.local_decls, tcx).ty).1 {
                                return None;
                            }
                            let column = write_column(*field)?;
                            if column.payload { return None; }
                            // Through a reference the entire referent is
                            // initialized. For owned writes sample each
                            // child before the parent destructor.
                            let kind = if same_place(resolved, *lhs) {
                                match rt_md.rev_lookup.find(field.as_ref()) {
                                    LookupResult::Exact(path) => rest_how(&mut rt, &sites, false, path),
                                    LookupResult::Parent(Some(parent)) => {
                                        // An untracked field cannot have
                                        // moved independently of its parent.
                                        let st = rt.state(&sites, false, parent);
                                        match (st.may_init, st.may_uninit) {
                                            (true, false) => "drops",
                                            (true, true) => "drops_if_init",
                                            _ => "nothing_to_drop",
                                        }
                                    }
                                    LookupResult::Parent(None) => how,
                                }
                            } else { how };
                            if kind == "nothing_to_drop" { return None; }
                            let contents_gone = emptied_box(&mut rt, *field, kind);
                            Some(ReplacementField {
                                column, conditional: kind == "drops_if_init", flag: flag(*field),
                                may_have_moved: may_have_moved(*field), contents_gone,
                                partial: if kind == "drops_rest" && !contents_gone { detail(*field, kind) } else { None },
                            })
                        }).collect()
                    };
                    // If a preceding store could have refilled this place, a
                    // move on the way here does not by itself decide the drop.
                    let may_be_refilled = how == "drops_if_init" && exact(*lhs).is_some_and(|path| {
                        let parents = ancestors(&rt_md, path);
                        rt_md.inits.iter().any(|init| {
                            let rustc_mir_dataflow::move_paths::InitLocation::Statement(site) = init.location else { return false };
                            parents.contains(&init.path) && !rt_body[site.block].is_cleanup
                                && executes_at(original_location(site).block) && original_location(site) != loc
                                && location_reaches(body, original_location(site), at)
                                && may_after(&rt_moved_entry, &|st, l| moved_transfer(&rt_md, &rt_body, st, l),
                                    site, false).contains(&path)
                        })
                    });
                    let inputs = if how == "drops_if_init" && sites.as_slice() == [at] {
                        replacement_inputs(&mut rt, at, *lhs, rv, &executes)
                    } else { Vec::new() };
                    Some(ReplacementDrop {
                        how, flag: flag_for_whole, may_have_moved: may_have_moved(*lhs), may_be_refilled,
                        moved_columns, contents_gone, partial, fields, inputs,
                        box_parts: box_parts && !contents_gone, box_payload_maybe_moved,
                    })
                });
                let span = st.source_info.span.with_parent(None);
                let operand = match rv {
                    Rvalue::Use(op) => op.place().and_then(source_place),
                    _ => None,
                };
                let reborrow = match rv {
                    Rvalue::Use(op) => op.place().and_then(|p| p.as_local())
                        .and_then(|temp| match definitions.get(&temp).copied().flatten() {
                            Some(TempDefinition::Value(def)) => Some(def),
                            _ => None,
                        }).unwrap_or(rv),
                    _ => rv,
                };
                let self_assignment = operand.is_some_and(|p| same_place(p, *lhs)) || matches!(reborrow, Rvalue::Ref(_, _, place)
                    if lhs.ty(&body.local_decls, tcx).ty.is_ref() && same_place(*place, tcx.mk_place_deref(*lhs)));
                let staged_move = match rv {
                    Rvalue::Use(Operand::Move(_)) => {
                        let source = operand.or_else(|| {
                            // Destructuring first moves the mutable
                            // reference into a pattern temp. The final
                            // store reborrows through that temp.
                            let Rvalue::Ref(_, BorrowKind::Mut { .. }, place) = reborrow else { return None };
                            if place.projection.as_slice() != [ProjectionElem::Deref] { return None; }
                            let slot = Place::from(place.local);
                            if cx.is_user(slot) { return None; }
                            source_place(slot)
                        });
                        source.filter(|p| !ty_info(p.ty(&body.local_decls, tcx).ty).0).and_then(write_column)
                    }
                    _ => None,
                };
                writes.entry(span).or_default().push(AssignmentWrite {
                    at: loc, row: cx.rows.event_row.get(&loc).copied().unwrap_or_else(|| cx.row_of(loc)),
                    runs: executes_at(loc.block),
                    skipped: skipped_closure_returns.contains_key(&loc.block),
                    copy: ty_info(lhs.ty(&body.local_decls, tcx).ty).0, direct: same_place(resolved, *lhs),
                    dest, staged_move, self_assignment, drop_at, replacing,
                });
            }
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
                let projected_move = source.and_then(|p| {
                    let (_, base, column) = columns.iter().chain(&synth).filter(|(_, col, _)|
                        col.local == p.local && p.projection.len() > col.projection.len()
                            && p.projection.starts_with(col.projection)).max_by_key(|(_, col, _)| col.projection.len())?;
                    let prefix = base.projection.len();
                    let label = crate::mir_data::array_operand_name(tcx, body, p, prefix, column.clone()).map(|element| {
                        if prefix > 0 && p.projection[prefix..].iter().any(|e| matches!(e, ProjectionElem::Downcast(..))) {
                            format!("the payload of {element}")
                        } else { element }
                    }).or_else(|| {
                        let [ProjectionElem::Downcast(_, variant), ProjectionElem::Field(field, _)] = &p.projection[prefix..] else { return None };
                        if prefix == 0 { return None; }
                        let def = base.ty(&body.local_decls, tcx).ty.ty_adt_def()?.variant(*variant);
                        Some(format!("field {} of {}'s {} variant", def.fields[*field].name, column, def.name))
                    })?;
                    Some((p.local, column.clone(), label))
                });
                let fixed_array_copy = if matches!(rv, Rvalue::Use(Operand::Copy(_))) && body.local_decls[lhs.local].ty.is_ref() {
                    source.and_then(|p| stores.slot_before(loc, p)).and_then(|p| cx.names.fixed_array_slot(tcx, body, p))
                } else {
                    None
                };
                binding_sources.push(BindingSource {
                    projected_move,
                    fixed_array_copy,
                    parents, covers, moves: matches!(rv, Rvalue::Use(Operand::Move(p)) if !ty_info(p.ty(&body.local_decls, tcx).ty).0),
                    location: loc, row: cx.row_of(loc), runs: runs.contains(&loc.block),
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
                    // Repoint is about the storage slot even when it has
                    // no column of its own, as for a fixed array element.
                    if let Some(this) = projected_stores.get(&loc) {
                        let holder = if lhs_col {
                            cx.name(*lhs)
                        } else {
                            cx.names.fixed_array_slot(tcx, body, this.place).map(|(_, _, label)| label)
                        };
                        if let Some(holder) = holder {
                            let mut installed = this.loans.clone();
                            if let Some(t) = table {
                                let current = held_by_place(*lhs, t.mid_index(loc));
                                let prior = held_by_place(*lhs, t.start_index(loc));
                                installed.extend(this.retained.keys().copied().filter(|id| current.contains(id) && !prior.contains(id)));
                                if installed.is_empty() && !this.complete {
                                    installed.extend(current.difference(&prior).copied().filter(|id| published.contains(id)));
                                }
                            }
                            push(&mut evs, cx.ev(loc, Fact::Repoint {
                                definitions: Vec::new(), maybe_uninitialized: false, reaching: None, holder,
                                old: Vec::new(), new: installed.into_iter().collect(),
                            }));
                        }
                    }
                    if lhs_col {
                        let path = cx.name(*lhs).unwrap_or_default();
                        let from = match rv {
                            _ if assigned_none(tcx, body, loc, *lhs, rv) => "Option::None".into(),
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
                                let f = Fact::Repoint { definitions: Vec::new(), maybe_uninitialized: false, reaching: None, holder: path.clone(), old: held(t.start_index(loc)), new: held(t.mid_index(loc)) };
                                push(&mut evs, cx.ev(loc, f));
                            }
                        }
                        if !reassign && lhs.projection.is_empty() {
                            if let Some(t) = table {
                                let ids: Vec<_> = held_by(lhs.local, t.mid_index(loc)).into_iter()
                                    .filter(|l| published.contains(l)).collect();
                                if !ids.is_empty() { binding_loans.entry(lhs.local).or_insert_with(BTreeMap::new).insert(loc, ids); }
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
                        for (inputs, after, site) in boundary_inputs(*l, loc) {
                            record_scope(&mut rt, *l, loc, &inputs, after, site.map(|s| s.0), true);
                        }
                        let note = if copy { "copy: nothing to drop" } else { "no drop glue" };
                        let ev = cx.place_ev(loc, *l, Fact::ScopeEnd { path: cx.names.locals[l].clone(), note });
                        if let LookupResult::Exact(top) = rt_md.rev_lookup.find(Place::from(*l).as_ref()) {
                            let site = cx.rows.exit_site.get(&loc).map(|&at| cx.r.exit_span(at));
                            scope_partials.entry(ev.0).or_default().extend(
                                partial_drops(&mut rt, &runtime_locations(loc).collect::<Vec<_>>(), false, None, top, false).into_iter().map(|(local, name, parts)| (site, local, name, parts)));
                        }
                        push(&mut evs, ev);
                    }
                }
                _ => {}
            }
            continue;
        }
        // The cleanup ladder is entered on an unwind edge, before a
        // call's return destination is initialized. A Drop which owns
        // nothing cannot panic. Aborting never enters these blocks.
        let temp_cleanup = match &data.terminator().kind {
            TerminatorKind::Call { unwind: UnwindAction::Cleanup(bb), .. } if !skipped_closure_returns.contains_key(&loc.block)
                && scope_call_can_unwind(tcx, body, loc, &mut Vec::new()) => Some(*bb),
            TerminatorKind::Assert { unwind: UnwindAction::Cleanup(bb), cond, expected, .. }
                if cond.constant().and_then(|c| c.const_.try_to_bool()) != Some(*expected) => Some(*bb),
            TerminatorKind::Drop { unwind: UnwindAction::Cleanup(bb), place, .. }
                if ty_info(place.ty(body, tcx).ty).1 && match rt_md.rev_lookup.find(place.as_ref()) {
                    LookupResult::Exact(path) => temporaries::remainder(tcx, &mut rt, &runtime_locations(loc).collect::<Vec<_>>(), false, path, None).value != TempValue::Empty,
                    LookupResult::Parent(Some(path)) => rt.state(&runtime_locations(loc).collect::<Vec<_>>(), false, path).may_init,
                    LookupResult::Parent(None) => true,
                } => Some(*bb),
            _ => None,
        };
        if let Some(start) = temp_cleanup.filter(|_| executes_at(loc.block) && !temps.is_empty() && tcx.sess.panic_strategy().unwinds()) {
            rt.unwind_from = Some(loc);
            for (at, place) in cleanup_drops(body, start) {
                if let Some(temp) = temps.get_mut(&place.local) {
                    let edge = Some((body.source_info(loc).span.source_callsite(), loc, Some(start.start_location())));
                    if let Some(event) = temporaries::drop_event(&cx, &mut rt, temp, at, place, &runtime_sites(loc), true,
                        edge, DropRoute::Unwind, tests_flag(at, place), &rt_moved_entry) { push(&mut evs, event); }
                }
            }
            rt.unwind_from = None;
        }
        match &data.terminator().kind {
            TerminatorKind::Return => {
                for &arg in &no_glue_args {
                    for (inputs, after, site) in boundary_inputs(arg, loc) {
                        record_scope(&mut rt, arg, loc, &inputs, after, site.map(|s| s.0), true);
                    }
                }
            }
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
                // A nonreturning call ends the picture even if it aborts or loops.
                // Store its location independently of executable unwinding drops.
                if matches!(data.terminator().kind, TerminatorKind::Call { target: None, .. }) {
                    let site = body.source_info(loc).span.source_callsite();
                    storage.seek_before_primary_effect(loc);
                    // An extended temporary can still hold its value on only
                    // some incoming paths. Even an abort ends that column.
                    for t in temps.values().filter(|t| t.extension.is_some()) {
                        let LookupResult::Exact(path) = rt_md.rev_lookup.find(Place::from(t.local).as_ref()) else { continue };
                        if !args.iter().any(|a| matches!(a.node, Operand::Move(p) if p == Place::from(t.local)))
                            && rt.state(&runtime_locations(loc).collect::<Vec<_>>(), false, path).may_init {
                            terminal_calls.entry(t.local).or_default().push((site, cx.r.line_lo(site)));
                        }
                    }
                    for &local in cx.names.locals.keys().filter(|l| storage.get().contains(**l)) {
                        terminal_calls.entry(local).or_default().push((site, cx.r.line_lo(site)));
                    }
                }
                if !matches!(unwind, UnwindAction::Continue | UnwindAction::Cleanup(_))
                    || !tcx.sess.panic_strategy().unwinds()
                    || !executes_at(loc.block) || skipped_closure_returns.contains_key(&loc.block) { continue; }
                if scope_call_can_unwind(tcx, body, loc, &mut Vec::new()) {
                    rt.unwind_from = Some(loc);
                    let site = Some(body.source_info(loc).span.source_callsite());
                    let sites: Vec<_> = runtime_locations(loc).collect();
                    storage.seek_before_primary_effect(loc);
                    for &local in cx.names.locals.keys() {
                        if storage.get().contains(local) && !ty_info(body.local_decls[local].ty).1 {
                            record_scope(&mut rt, local, loc, &sites, true, site, true);
                        }
                    }
                    if let UnwindAction::Cleanup(start) = *unwind {
                        for (drop, place) in cleanup_drops(body, start) {
                            if place.projection.is_empty() && cx.names.locals.contains_key(&place.local) {
                                record_scope(&mut rt, place.local, drop, &sites, true, site, false);
                            }
                        }
                    }
                    rt.unwind_from = None;
                }
                let UnwindAction::Cleanup(start) = *unwind else { continue };
                let loan_cleanup = loan_cleanups.contains(&loc);
                if !loan_cleanup && vec_buffers.is_empty() { continue; }
                let runtime = runtime_locations(loc).collect::<Vec<_>>();
                let drops = cleanup_drops(body, start);
                if !loan_cleanup {
                    let Some(closure) = called_closure(tcx, func) else { continue };
                    if closure_can_return(tcx, closure) || !closure_has_panic_route(tcx, closure) { continue; }
                    if !drops.iter().any(|(_, place)| vec_buffers.contains(place)
                        && matches!(rt_md.rev_lookup.find(place.as_ref()), LookupResult::Exact(mpi)
                            if rt.state(&runtime, false, mpi).may_init)) { continue; }
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
                    let how = rest_how(&mut rt, &runtime, false, mpi);
                    if how == "nothing_to_drop" { continue; }
                    let mut ev = cx.ev(loc, Fact::Drop { path, how, at_reassign: false, early_exit: None, also_on_exits: Vec::new() });
                    ev.1.drop_route = DropRoute::Unwind;
                    ev.1.occurrence = Occurrence { at, exit: Some(loc), successor: None };
                    ev.1.order = cx.rows.order(at);
                    push(&mut evs, ev);
                }
            }
            TerminatorKind::Drop { place, target, .. } => {
                let at_reassign = drops_at_reassign(*place, *target);
                let runtime = runtime_locations(loc).collect::<Vec<_>>();
                if runtime.is_empty() { continue; }
                let early_exit = cx.rows.early_exit.get(&loc).map(|(r, _)| *r);
                let exit = cx.rows.exit_site.get(&loc).copied();
                let shared = cx.rows.shared_exits.get(&loc).map(Vec::as_slice).unwrap_or_default();
                if !at_reassign && matches!(data.terminator().kind, TerminatorKind::Drop { replace: false, .. })
                    && place.projection.is_empty() && cx.names.locals.contains_key(&place.local) {
                    for (inputs, after, site) in boundary_inputs(place.local, loc) {
                        record_scope(&mut rt, place.local, loc, &inputs, after, site.map(|s| s.0), false);
                    }
                }
                if !at_reassign {
                    drop_init.seek_before_primary_effect(loc);
                    drop_uninit.seek_before_primary_effect(loc);
                    // An untracked field is dropped with its parent and
                    // shares the parent's flag when that drop is guarded.
                    let flagged = |p: Place<'tcx>| matches!(drop_md.rev_lookup.find(p.as_ref()),
                        LookupResult::Exact(path) | LookupResult::Parent(Some(path))
                        if drop_init.get().contains(path) && drop_uninit.get().contains(path));
                    if let Some((_, site)) = exit.and_then(residual_site).filter(|(call, _)| residual_runs(*call)) {
                        record_residual(&mut rt, *place, site, loc, false, &flagged);
                    }
                    for (_, at) in shared {
                        if let Some((_, site)) = residual_site(*at).filter(|(call, _)| residual_runs(*call)) {
                            record_residual(&mut rt, *place, site, *at, true, &flagged);
                        }
                    }
                }
                if cx.is_user(*place) {
                    let path = cx.name(*place).unwrap_or_default();
                    let mut flagged_parts = Vec::new();
                    if let LookupResult::Exact(mpi) = drop_md.rev_lookup.find(place.as_ref()) {
                        drop_init.seek_before_primary_effect(loc);
                        drop_uninit.seek_before_primary_effect(loc);
                        if drop_init.get().contains(mpi) && drop_uninit.get().contains(mpi) {
                            drop_flags.insert(loc);
                        }
                        // Drop elaboration opens a partly moved aggregate into
                        // per-field drops. Each part that needs drop and may be
                        // initialized or moved out has its own flag, and the
                        // whole place's drop has one when any part does (a
                        // sibling part has none).
                        for m in descendants(&drop_md, mpi).into_iter().filter(|m| *m != mpi) {
                            let part = drop_md.move_paths[m].place;
                            if drop_init.get().contains(m) && drop_uninit.get().contains(m)
                                && part.ty(&body.local_decls, tcx).ty.needs_drop(tcx, typing_env) {
                                drop_part_flags.insert((loc, path.clone()));
                                if let Some(name) = cx.name(part) {
                                    drop_part_flags.insert((loc, name.clone()));
                                    flagged_parts.push((part, name));
                                }
                            }
                        }
                    }
                    let how = match rt_md.rev_lookup.find(place.as_ref()) {
                        LookupResult::Exact(mpi) => rest_how(&mut rt, &runtime, false, mpi),
                        LookupResult::Parent(_) => "drops",
                    };
                    let also_on_exits: Vec<usize> = shared.iter().map(|(r, _)| *r).collect();
                    // A Box's contents are dropped before its allocation is
                    // freed; a struct's summary drop comes before its fields'.
                    let is_box = place.ty(&body.local_decls, tcx).ty.is_box();
                    // The drop also runs on early exits that share it: each
                    // exit gets its own on its row, with what the place (and
                    // each part of it) holds when that path leaves (Q2: on
                    // the exit's row).
                    if let LookupResult::Exact(mpi) = rt_md.rev_lookup.find(place.as_ref()) {
                        for (r, at) in shared {
                            let edge = runtime_locations(*at).collect::<Vec<_>>();
                            if edge.is_empty() { continue; }
                            let how_there = rest_how(&mut rt, &edge, true, mpi);
                            let (row, site) = try_span(*at).map_or_else(
                                || (*r, cx.r.exit_span(*at)),
                                |site| (cx.r.line_lo(site), site),
                            );
                            scope_partials.entry(row).or_default().extend(
                                partial_drops(&mut rt, &edge, true, Some(loc), mpi, true).into_iter()
                                    .map(|(local, name, parts)| (Some(site), local, name, parts)));
                            if how_there == "nothing_to_drop" {
                                continue;
                            }
                            let parts = field_drops(&mut rt, &edge, true, mpi);
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
                                ev.1.occurrence.exit = Some(*at);
                                push(&mut evs, ev);
                            }
                        }
                    }
                    let parts = match rt_md.rev_lookup.find(place.as_ref()) {
                        LookupResult::Exact(top) => field_drops(&mut rt, &runtime, false, top),
                        LookupResult::Parent(_) => Vec::new(),
                    };
                    // Only early exits reach this drop: each has its own above.
                    if !cx.rows.exit_only.contains(&loc) {
                        let whole = (path, how, also_on_exits);
                        let mut items: Vec<(String, &'static str, Vec<usize>)> = parts.into_iter().map(|(_, _, n, h)| (n, h, Vec::new())).collect();
                        // A runtime path can hold the complete aggregate even
                        // when drop elaboration emitted a guarded field drop.
                        for (part, name) in flagged_parts {
                            if items.iter().any(|(n, _, _)| *n == name) { continue; }
                            if let LookupResult::Exact(mpi) = rt_md.rev_lookup.find(part.as_ref()) {
                                let how = rest_how(&mut rt, &runtime, false, mpi);
                                items.push((name, how, Vec::new()));
                            }
                        }
                        if is_box {
                            items.push(whole);
                        } else {
                            items.insert(0, whole);
                        }
                        let route = drop_route(loc, shared);
                        if !at_reassign {
                            if let LookupResult::Exact(top) = rt_md.rev_lookup.find(place.as_ref()) {
                                let (row, site) = exit.and_then(try_span).map_or_else(
                                    || (cx.rows.event_row[&loc], exit.map(|at| cx.r.exit_span(at))),
                                    |site| (cx.r.line_lo(site), Some(site)),
                                );
                                scope_partials.entry(row).or_default().extend(
                                    partial_drops(&mut rt, &runtime, false, Some(loc), top, exit.is_some()).into_iter()
                                        .map(|(local, name, parts)| (site, local, name, parts)));
                            }
                        }
                        for (n, h, also) in items {
                            let mut ev = cx.place_ev(loc, place.local, Fact::Drop { path: n, how: h, at_reassign, early_exit, also_on_exits: also });
                            ev.1.drop_route = route;
                            push(&mut evs, ev);
                        }
                    }
                } else if let Some(temp) = temps.get_mut(&place.local) {
                    let inputs = boundary_inputs(place.local, loc);
                    let ordinary = inputs.iter().any(|e| e.2.is_none());
                    let exits = inputs.iter().any(|e| e.2.is_some());
                    let route = match (ordinary, exits) {
                        (true, true) => DropRoute::Shared,
                        (_, true) => DropRoute::ExitOnly,
                        _ => DropRoute::Normal,
                    };
                    let flag = tests_flag(loc, *place);
                    for (sites, after, exit) in inputs {
                        if let Some(event) = temporaries::drop_event(&cx, &mut rt, temp, loc, *place,
                            &sites, after, exit, route, flag, &rt_moved_entry) {
                            push(&mut evs, event);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let temps = temps.into_values().map(|mut t| {
        t.drop_lines.sort_unstable();
        t.drop_lines.dedup();
        t
    }).collect();

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
            &real_predecessors[loc.block]
        } else {
            &body.basic_blocks.predecessors()[loc.block]
        }
    };
    let may_skip = |locations: &[Location], after: bool, ids: &BTreeSet<usize>, imaginary: bool,
                    released: &dyn Fn(Location, bool) -> bool| {
        let mut work: Vec<_> = locations.iter().map(|l| (*l, after)).collect();
        let mut seen = BTreeSet::new();
        while let Some((loc, inclusive)) = work.pop() {
            if !can_walk_back(loc, imaginary) || !seen.insert((loc, inclusive)) {
                continue;
            }
            if inclusive && issued.get(&loc).is_some_and(|id| ids.contains(id)) {
                continue;
            }
            if released(loc, inclusive) {
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
    // Stop at the first write or the end of this incarnation of the
    // local. A later loop iteration can declare the same MIR local again.
    let mut cleanup_places = BTreeMap::new();
    let rt_rows = Rows { body: wording_body, tcx };
    let runtime_successors = |loc: Location| {
        let succs = rt_rows.real_succs(loc);
        if loc.statement_index < wording_body.basic_blocks[loc.block].statements.len() { return succs; }
        succs.into_iter().filter(|s| executes.contains(&s.block)).collect::<Vec<_>>()
    };
    let mut next_assignment = |place: Place<'tcx>, exits: &[Location], after: bool| {
        let mut work = if after {
            exits.iter().flat_map(|l| runtime_successors(*l)).collect()
        } else { exits.to_vec() };
        let mut seen = BTreeSet::new();
        let writes = |dest: Place<'tcx>| dest.local == place.local && place.projection.starts_with(dest.projection);
        let (mut assigned, mut ended) = (false, false);
        let mut unwind_before_write = false;
        while let Some(l) = work.pop() {
            if !seen.insert(l) { continue; }
            let data = &wording_body.basic_blocks[l.block];
            if let Some(st) = data.statements.get(l.statement_index) {
                match &st.kind {
                    StatementKind::Assign(a) if writes(a.0) => { assigned = true; continue; }
                    StatementKind::StorageDead(v) if *v == place.local => { ended = true; continue; }
                    _ => {}
                }
            } else {
                let term = data.terminator();
                // Unwinding the old value's own destructor still means
                // it was dropped at the overwrite, if there is one.
                let can_unwind = match &term.kind {
                    TerminatorKind::Call { func, .. } => {
                        let ty = func.ty(&body.local_decls, tcx);
                        let def = func.const_fn_def().map(|(def, _)| def);
                        rustc_middle::ty::layout::fn_can_unwind(tcx, def, ty.fn_sig(tcx).abi())
                    }
                    TerminatorKind::Drop { place: dest, .. } => !writes(*dest),
                    _ => true,
                };
                if tcx.sess.panic_strategy().unwinds() && can_unwind {
                    if let Some(&UnwindAction::Cleanup(start)) = term.unwind() {
                        let places = cleanup_places.entry(start).or_insert_with(|| {
                            let mut drops = Vec::new();
                            let mut seen = BTreeSet::new();
                            let mut at = start;
                            while seen.insert(at) && rt_body.basic_blocks[at].is_cleanup {
                                match rt_body.basic_blocks[at].terminator().kind {
                                    TerminatorKind::Drop { place, target, .. } => {
                                        drops.push(place);
                                        at = target;
                                    }
                                    TerminatorKind::Goto { target } => at = target,
                                    _ => break,
                                }
                            }
                            drops
                        });
                        unwind_before_write |= places.iter().any(|dest| writes(*dest));
                    }
                }
                if matches!(term.kind, TerminatorKind::Call { destination, target: Some(_), .. } if writes(destination)) {
                    assigned = true;
                    continue;
                }
            }
            let succs = runtime_successors(l);
            // A diverging call (including panic) has no normal successor.
            if succs.is_empty() {
                ended = true;
            }
            work.extend(succs);
        }
        assigned.then_some((!ended, unwind_before_write))
    };
    let take_calls: BTreeSet<_> = runs.iter().copied().filter(|&bb| {
        let data = &body.basic_blocks[bb];
        !data.is_cleanup && matches!(&data.terminator().kind, TerminatorKind::Call { func, target: Some(_), .. }
            if manually_drop_take(tcx, body, func))
    }).map(|bb| body.terminator_loc(bb)).collect();
    let manually_taken = manually_taken(&cx, &take_calls);
    let drop_holders_at_reassign: Vec<_> = reassign_drop_holders.iter().filter_map(|(local, loans)|
        Some((*local, cx.names.locals.get(local)?, drop_origins.get(local)?, loans))
    ).collect();
    let iterator_paths: Vec<_> = iterators.iter().filter_map(|&local|
        Some((local, move_data.rev_lookup.find_local(local)?))).collect();
    let item_moves: BTreeMap<_, _> = for_loops.iter().flat_map(|it| {
        it.bindings.iter().filter(|binding| body.local_decls[binding.local].ty.ref_mutability().is_some_and(|m| m.is_mut()))
            .filter_map(|binding| Some(((it.storage, binding.at), super::iterators::consumed_item(body, binding, it.next)?)))
    }).collect();
    let iterator_move_edges: Vec<_> = item_moves.iter().flat_map(|(&(storage, _), &at)|
        cx.r.real_succs(at).into_iter().map(move |to| (storage, at, to))).collect();
    // Universal origins can relate independent parameters with the same
    // lifetime. An unchanged argument's value cannot contain a loan
    // issued by this body, even if its origin contains the loan.
    let assigned_locals: BTreeSet<_> = move_data.inits.iter().filter_map(|init|
        matches!(init.location, rustc_mir_dataflow::move_paths::InitLocation::Statement(_))
            .then(|| move_data.move_paths[init.path].place.local)).collect();
    let unchanged_arguments: BTreeSet<_> = body.args_iter().filter(|arg|
        !assigned_locals.contains(arg) && !stores.addressed.iter().any(|place| place.local == *arg)
            && matches!(body.local_decls[*arg].ty.kind(), TyKind::Ref(_, pointee, _) if regions_of(*pointee).is_empty()))
        .collect();
    let mut entries: BTreeMap<usize, BTreeSet<Location>> = BTreeMap::new();
    let mut incoming_rows = BTreeMap::<_, BTreeSet<_>>::new();
    let start = Location::START;
    entries.entry(cx.row_of(start)).or_default().insert(start);
    for (&from, &row) in &cx.rows.row {
        if !runs.contains(&from.block) { continue; }
        for to in cx.r.real_succs(from) {
            if let Some(&next) = cx.rows.row.get(&to) {
                if next > row && !cx.rows.exit_only.contains(&to) && !cx.rows.early_exit.contains_key(&to)
                    && !cx.rows.shared_exits.get(&to).is_some_and(|sites| drop_route(to, sites) == DropRoute::ExitOnly) {
                    entries.entry(next).or_default().insert(to);
                    incoming_rows.entry(next).or_default().insert(row);
                }
            }
        }
    }
    // A source row can join the end of a local's scope with a path
    // that bypassed its declaration. Only exclude entry locations where
    // MIR proves its storage dead. A location with both incoming paths
    // retains both in the initialization dataflow.
    let mut scoped_entries = BTreeMap::new();
    for (&line, entries) in &entries {
        for ev in evs.get(&line).into_iter().flatten() {
            let Fact::Drop { path, how: "drops", early_exit: None, at_reassign: false, .. } = &ev.fact else { continue };
            let Some(local) = ev.place_local.filter(|_| ev.drop_route == DropRoute::Normal) else { continue };
            let active: Vec<_> = entries.iter().copied().filter(|&at| {
                storage.seek_before_primary_effect(at);
                storage.get().contains(local)
            }).collect();
            if !active.is_empty() && active.len() < entries.len() {
                scoped_entries.entry(line).or_insert_with(BTreeMap::new).insert((local, path.clone()), active);
            }
        }
    }
    // A reborrow does not prevent writing a new reference into an
    // unborrowed slot. Stop the backward slice at the actual MIR writes
    // or at the issue of this loan, keeping the two generations distinct.
    let reference_writes: Vec<_> = body.basic_blocks.iter_enumerated()
        .filter(|(bb, _)| runs.contains(bb)).flat_map(|(bb, block)| {
            let normal_result = match block.terminator().kind {
                TerminatorKind::Call { destination, target: Some(target), .. } =>
                    Some((body.terminator_loc(bb), Some(target), destination)),
                _ => None,
            };
            block.statements.iter().enumerate().filter_map(move |(statement_index, st)| {
                let StatementKind::Assign(pair) = &st.kind else { return None };
                Some((Location { block: bb, statement_index }, None, pair.0))
            }).chain(normal_result)
        }).filter(|&(_, _, dest)| reference_slots.iter().any(|(slot, _, _)|
            place_under(*slot, dest) || (place_under(dest, *slot) && dest.ty(body, tcx).ty.is_ref())))
        .collect();
    let reborrow_slots: BTreeMap<_, _> = bwf.borrow_set.location_map().iter().filter_map(|(at, borrow)| {
        let place = borrow.borrowed_place();
        let index = place.projection.iter().rposition(|p| p == ProjectionElem::Deref)?;
        let slot = Place { local: place.local, projection: tcx.mk_place_elems(&place.projection[..index]) };
        slot.ty(body, tcx).ty.is_ref().then_some((*at, slot))
    }).collect();
    let replaced_since = |slot, issue, exits: &[Location], after, imaginary, successor| {
        let writes: BTreeSet<_> = reference_writes.iter().filter_map(|&(at, target, dest)|
            (at != issue && place_under(slot, dest)).then_some((at, target))).collect();
        if writes.is_empty() { return (false, false); }
        let mut seen = BTreeSet::new();
        let mut work: Vec<_> = exits.iter().map(|&at| (at, after, successor, false)).collect();
        let (mut original, mut replaced) = (false, false);
        while let Some((at, inclusive, target, wrote)) = work.pop() {
            if !seen.insert((at, inclusive, target, wrote)) || !can_walk_back(at, imaginary) { continue; }
            if at == issue && inclusive {
                original |= !wrote;
                replaced |= wrote;
                if original && replaced { break; }
                continue;
            }
            let wrote = wrote || (inclusive && writes.contains(&(at, target)));
            if at.statement_index != 0 {
                work.push((Location { statement_index: at.statement_index - 1, ..at }, true, None, wrote));
            } else {
                work.extend(predecessors(at).iter().map(|&bb| (body.terminator_loc(bb), true, Some(at.block), wrote)));
            }
        }
        (original, replaced)
    };
    // An escaped address can let another place overwrite a reference
    // slot. Only trace slots whose storage is reached directly.
    let address_taken: Vec<_> = body.basic_blocks.iter().flat_map(|block| &block.statements).filter_map(|stmt| {
        let StatementKind::Assign(assign) = &stmt.kind else { return None };
        match assign.1 {
            Rvalue::Ref(_, BorrowKind::Mut { .. }, place) | Rvalue::RawPtr(_, place) if !place.is_indirect() => Some(place),
            _ => None,
        }
    }).collect();
    let direct_slots: BTreeMap<_, _> = reference_slots.iter().filter_map(|(place, name, _)| {
        (!place.is_indirect()
            && !address_taken.iter().any(|&address| place_under(*place, address)))
            .then_some(((place.local, name.as_str()), *place))
    }).collect();
    let mut returned_arguments = rustc_data_structures::fx::FxHashMap::default();
    for block in body.basic_blocks.iter() {
        let TerminatorKind::Call { func, destination, .. } = &block.terminator().kind else { continue };
        if !destination.ty(&body.local_decls, tcx).ty.is_ref() { continue; }
        if let Some((def, _)) = func.const_fn_def() {
            returned_arguments.entry(def).or_insert_with(|| super::call_wording::returned_argument(tcx, def));
        }
    }
    returned_arguments.retain(|_, argument| argument.is_some());
    let mut state_at = |exits: &[Location], after: bool, imaginary: bool, runtime: Option<&[Location]>,
                        adjacent: Option<&BTreeMap<Location, Location>>, point: Option<SegmentPoint>| -> State {
        let edge_to = match point {
            Some(SegmentPoint::Edge { to, .. }) => Some(to),
            _ => None,
        };
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
        let default_runtime;
        let rt_exits = if let Some(runtime) = runtime {
            runtime
        } else {
            default_runtime = exits.iter().copied().flat_map(runtime_locations).collect::<Vec<_>>();
            &default_runtime
        };
        let moved_here = |mpi, include_parts| {
            let paths = if include_parts { descendants(&move_data, mpi) } else { vec![mpi] };
            exits.iter().any(|e| {
                let set = may_after(&moved_entry, &|st, l| moved_transfer(&move_data, body, st, l), *e, after);
                paths.iter().any(|m| set.contains(m))
            })
        };
        for (mpi, place, name) in &columns {
            let s = flow.state(exits, after, *mpi);
            let rt_mpi = match rt_md.rev_lookup.find(place.as_ref()) {
                LookupResult::Exact(rm) if !rt_exits.is_empty() => Some(rm),
                _ => None,
            };
            let rt_here = rt_mpi.map(|rm| rt.state(rt_exits, after, rm));
            // Moved out (or, for `unassigned`, never given a value) on some
            // path that runs, by the runtime CFG's own move / assignment
            // dataflows.
            let rt_may = |entry: &BTreeMap<rustc_middle::mir::BasicBlock, BTreeSet<MovePathIndex>>, unassigned: bool| {
                rt_mpi.is_some_and(|rm| {
                    let desc = descendants(&rt_md, rm);
                    rt_exits.iter().any(|e| {
                        let set = if unassigned {
                            may_after(entry, &|st, l| unassigned_transfer(&rt_md, &rt_body, st, l), *e, after)
                        } else {
                            may_after(entry, &|st, l| moved_transfer(&rt_md, &rt_body, st, l), *e, after)
                        };
                        if unassigned { set.contains(&rm) } else { desc.iter().any(|m| set.contains(m)) }
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
            if runtime.is_some() && rt_here.as_ref().is_some_and(|r| r.may_uninit && !r.may_init && !r.part_may_init) {
                st.runtime_empty.insert((place.local, name.clone()));
            }
            if !rt_exits.is_empty() && place.ty(&body.local_decls, tcx).ty.needs_drop(tcx, typing_env) {
                if let Some((all, unwind)) = next_assignment(*place, rt_exits, after) {
                    st.reassigned.insert(name.clone(), all);
                    if unwind { st.unwind_before_write.insert(name.clone()); }
                }
            }
            if w == "unassigned" && !place.projection.is_empty() && root_unassigned() {
                continue;
            }
            st.state.insert(name.clone(), w);
            if w == "moved" && place.ty(body, tcx).ty.is_ref() {
                if let Some(parent) = ancestors(&move_data, *mpi).into_iter().skip(1)
                    .find(|&parent| flow.state(exits, after, parent).word() == "moved")
                    .and_then(column_name)
                {
                    st.reference_moved_with.insert(name.clone(), parent);
                }
            }
            if s.payload_maybe || s.payload_moved {
                st.maybe_empty_box.insert(name.clone());
            }
            if s.word() == "maybe" && w == "moved" { st.borrowck_maybe.insert(name.clone()); }
            record_type(name, *place);
            if w == "maybe" {
                // Moved on some way here, or only never assigned?
                let moved = moved_here(*mpi, true);
                // Moved only on borrowck's imaginary edges (the unreachable
                // alternative), never on a path that runs: it still can't be
                // used, but nothing was moved. (A maybe that is only "never
                // assigned" on an imaginary path is `unassigned`: E0381.)
                let runtime_move = || {
                    let Some(entry) = &wording_moves else { return rt_may(&rt_moved_entry, false) };
                    rt_mpi.is_some_and(|path| {
                        let desc = descendants(&rt_md, path);
                        rt_exits.iter().filter(|e| wording_body.basic_blocks.dominators().is_reachable(e.block)).any(|&e| {
                            let set = may_after(entry, &|st, loc| moved_transfer(&rt_md, wording_body, st, loc), e, after);
                            desc.iter().any(|p| set.contains(p))
                        })
                    })
                };
                let imaginary = moved && rt_mpi.is_some() && !runtime_move();
                // Borrow checking may see an empty place, although every
                // path that actually runs holds a value here.
                if rt_here.as_ref().is_some_and(|r| r.word() == "own") {
                    st.always_assigned.insert(name.clone());
                }
                if rt_mpi.is_some_and(|rm| rt_here.as_ref().is_some_and(|r| r.word() == "partly")
                    && rest_how(&mut rt, &rt_exits, after, rm) == "nothing_to_drop") {
                    st.partial_without_drop.insert(name.clone());
                }
                let mixed = moved && never_assigned_somewhere();
                let whole_move = mixed && moved_here(*mpi, false);
                if mixed {
                    st.mixed_init.insert(name.clone(), MixedInit {
                        may_hold_value: rt_here.as_ref().map_or(s.may_init, |r| r.may_init),
                        may_hold_value_at_runtime: rt_here.as_ref().is_some_and(|r| r.may_init),
                        whole_move,
                        imaginary_move: imaginary,
                        assigned_in_some_arms: false,
                    });
                }
                // A move of a field doesn't move the aggregate. A path
                // that skipped assigning the whole place gives E0381
                // even if another path moved a field (E0382 for that field).
                let cause = if !moved || (mixed && !whole_move) {
                    "unassigned"
                } else if imaginary {
                    "imaginary"
                } else {
                    "moved"
                };
                st.cause.insert(name.clone(), cause);
            }
            if (w == "partly" || w == "own") && (!s.sub_moved.is_empty() || !s.sub_maybe.is_empty()) {
                // A part borrowck counts as moved on some paths only is moved
                // for sure when only its imaginary edges skip the move (an
                // irrefutable `if let (p, q) = pair && ..`'s `else`): which
                // parts are gone is what runs.
                let mut surely_gone = |m: MovePathIndex| match rt_md.rev_lookup.find(move_data.move_paths[m].place.as_ref()) {
                    LookupResult::Exact(rm) if !rt_exits.is_empty() => {
                        let r = rt.state(rt_exits, after, rm);
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
                    array_indices(*place, mp, n).into_iter().flatten().map(|i| format!("{name}[{i}]")).collect()
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
                if let Some((all, unwind)) = next_assignment(*fp, rt_exits, after) {
                    st.reassigned.insert(name.clone(), all);
                    if unwind { st.unwind_before_write.insert(name.clone()); }
                }
            }
            // A field without its own move path may still be initialized
            // when a sibling's move affects the tracked parent's state.
            // Ask whether the ancestor itself was ever empty.
            let live_sibling = !through_ref && pw == "maybe" && exits.iter().all(|&loc| {
                let moved = may_after(&moved_entry, &|st, l| moved_transfer(&move_data, body, st, l), loc, after);
                let unassigned = may_after(&unassigned_entry, &|st, l| unassigned_transfer(&move_data, body, st, l), loc, after);
                !moved.contains(parent) && !unassigned.contains(parent)
            });
            st.state.insert(name.clone(), if pw == "partly" || live_sibling { "own" } else { pw });
            record_type(name, *fp);
            if live_sibling { continue; }
            if pw == "moved" && fp.ty(body, tcx).ty.is_ref() {
                st.reference_moved_with.insert(name.clone(), pn.clone());
            }
            if let Some(c) = st.cause.get(&pn).copied() {
                // An untracked field can inherit a possible move of its
                // parent even when the runtime moves only a sibling.
                let imaginary = c == "moved" && !through_ref && !rt_exits.is_empty()
                    && match rt_md.rev_lookup.find(fp.as_ref()) {
                        LookupResult::Parent(Some(rm)) => {
                            rt_exits.iter().all(|&loc| {
                                let moved = may_after(&rt_moved_entry, &|st, l| moved_transfer(&rt_md, &rt_body, st, l), loc, after);
                                let unassigned = may_after(&rt_unassigned_entry, &|st, l| unassigned_transfer(&rt_md, &rt_body, st, l), loc, after);
                                !moved.contains(&rm) && !unassigned.contains(&rm)
                            })
                        }
                        _ => false,
                    };
                // A sibling move can affect the parent's usability without
                // moving this field. Only a move of its tracked ancestor
                // can give this synthesized field a mixed history.
                let mut cause = if imaginary { "imaginary" } else { c };
                if let Some(mixed) = st.mixed_init.get(&pn).copied().filter(|_| !through_ref) {
                    if mixed.whole_move {
                        st.mixed_init.insert(name.clone(), mixed);
                    } else {
                        cause = "unassigned";
                    }
                }
                st.cause.insert(name.clone(), cause);
            }
        }
        // When different locals are active on different paths the column has no single state.
        st.locals = column_locals;
        st.state.retain(|name, _| st.locals.get(name) != Some(&None));
        st.holder_types = holder_types.into_iter().filter_map(|(name, ty)| Some((name, ty?))).collect();
        st.reference_types = reference_slots.iter().filter_map(|(slot, name, kind)|
            (st.locals.get(*name) == Some(&Some(slot.local))).then_some(((*name).clone(), *kind))).collect();
        // Loans in scope where the next row starts on the same path: in
        // scope after the exit and still at the successor (a loan whose last
        // use is on this row is out of scope there; another path's loan
        // that joins at the successor is not on this one).
        let mut live = BTreeMap::<usize, BTreeSet<(Location, bool)>>::new();
        let mut boundaries = Vec::new();
        for e in exits {
            if !after {
                boundaries.push((*e, false));
                inscope.seek_before_primary_effect(*e);
                for loan in inscope.get().iter() {
                    live.entry(loan.index()).or_default().insert((*e, false));
                }
                continue;
            }
            let line = cx.row_of(*e);
            let succs = adjacent.map_or_else(|| cx.r.real_succs(*e), |next| vec![next[e]]);
            let later: Vec<Location> = succs.iter().copied().filter(|s| cx.row_of(*s) > line).collect();
            let at = match point {
                Some(SegmentPoint::Edge { to, .. }) => vec![to],
                Some(_) => Vec::new(),
                None if later.is_empty() => succs,
                None => later,
            };
            boundaries.push((*e, true));
            inscope.seek_after_primary_effect(*e);
            let after: BTreeSet<usize> = inscope.get().iter().map(|b| b.index()).collect();
            if at.is_empty() {
                for &loan in &after {
                    live.entry(loan).or_default().insert((*e, true));
                }
            }
            for s in at {
                boundaries.push((s, false));
                inscope.seek_before_primary_effect(s);
                for loan in inscope.get().iter().map(|b| b.index()).filter(|b| after.contains(b)) {
                    live.entry(loan).or_default().extend([(*e, true), (s, false)]);
                }
            }
        }
        for l in &loans {
            let Some(live_on) = live.get(&l.loan) else { continue };
            // Who holds it here (Polonius: whose origins contain it after
            // the row's exits); the call / macro it went to otherwise.
            let mut now: Vec<String> = Vec::new();
            let current_holders = |e: Location, id: BorrowIndex| {
                let Some(t) = table else { return Vec::new() };
                let mut holders = holders_at(if after { t.mid_index(e) } else { t.start_index(e) }, id);
                if let Some(SegmentPoint::Edge { to, .. }) = point {
                    let next = holders_at(t.start_index(to), id);
                    holders.retain(|holder| next.contains(holder));
                }
                holders
            };
            if table.is_some() {
                for e in exits {
                    for h in current_holders(*e, BorrowIndex::from_usize(l.loan)) {
                        if !now.contains(&h) {
                            now.push(h);
                        }
                    }
                }
            }
            let first = loan_holder(l);
            let holder = if now.is_empty() || now.contains(&first) { first } else { now[0].clone() };
            let mut current = BTreeSet::new();
            let iterator = table.and_then(|t| {
                let mut carrying = iterator_paths.iter().filter(|&&(local, path)| {
                    let holds_at = |&(at, after): &(Location, bool)| {
                        let point = if after { t.mid_index(at) } else { t.start_index(at) };
                        held_by(local, point).contains(&l.loan) && flow.state(&[at], after, path).word() == "own"
                    };
                    // An exit from the loop can end a shared loan on one
                    // edge while the iterator keeps it on the continuing edge.
                    if l.kind == "shared" {
                        live_on.iter().all(holds_at)
                    } else {
                        boundaries.iter().all(holds_at)
                    }
                });
                let &(local, _) = carrying.next()?;
                carrying.next().is_none().then_some(local)
            });
            let iterator_handoff = iterator.and_then(|local| {
                let mut moves = iterator_move_edges.iter().filter(|&&(storage, at, to)| storage == local
                    && boundaries.iter().all(|&(point, after)| (after && point == at) || ldom(to, point)));
                let &(_, at, to) = moves.next()?;
                moves.all(|&edge| edge == (local, at, to)).then_some((at, to))
            });
            if let (Some(pol), Some(t)) = (&pol, table) {
                let id = BorrowIndex::from_usize(l.loan);
                for e in exits {
                    let point = if after { t.mid_index(*e) } else { t.start_index(*e) };
                    let Some(origins) = pol.origin_contains_loan_at.get(&point) else { continue };
                    let carries = |o: &Origin| origins.get(o).is_some_and(|ls| ls.contains(&id))
                        && edge_to.is_none_or(|to| pol.origin_contains_loan_at.get(&t.start_index(to))
                            .and_then(|os| os.get(o)).is_some_and(|ls| ls.contains(&id)));
                    for (local, os) in &origins_of {
                        if !os.iter().any(carries) { continue; }
                        let n = cx.names.locals[local].clone();
                        if st.locals.get(&n) != Some(&Some(*local)) || st.state.get(&n) != Some(&"own") { continue; }
                        if l.runs { st.holder_sources.entry(n.clone()).or_default().insert(l.loan); }
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
            if let Some(direct) = l.direct_holder.as_ref().filter(|_| point.is_none()) {
                if st.state.get(direct) == Some(&"own") && reborrows.get(&l.loan).is_some_and(|children|
                    children.iter().any(|(child, through)| through == direct && live.contains_key(child))) {
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
                    st.lent.entry(c.clone()).or_default().push(Lent { loan: l.loan, kind, holder: holder.clone(), iterator, iterator_handoff, current: current.clone(), ..Default::default() });
                    if let Some(issue) = l.issue {
                        for (slot, name, _) in &reference_slots {
                            if *name == c && st.locals.get(c) == Some(&Some(slot.local))
                                && matches!(l.reference_targets.get(&(slot.local, c.clone())),
                                    Some(ReferenceTarget::Referent | ReferenceTarget::NestedReferent | ReferenceTarget::VariantPayload | ReferenceTarget::Part))
                            {
                                let source = reborrow_slots.get(&issue).copied().unwrap_or(*slot);
                                let (original, replaced) = replaced_since(source, issue, exits, after, imaginary, edge_to.map(|to| to.block));
                                if replaced {
                                    let replacements = if original {
                                        &mut st.maybe_replaced_references
                                    } else {
                                        &mut st.replaced_references
                                    };
                                    replacements.insert((c.clone(), l.loan));
                                }
                            }
                        }
                    }
                }
            }
            // A lender that is no column (`*r` reborrowed through `r`,
            // `v[_]`): listed under its own name too (not a temporary).
            if !l.columns.contains(&l.lender) && l.source == "borrow" {
                st.lent.entry(l.lender.clone()).or_default().push(Lent { loan: l.loan, kind, holder: holder.clone(), iterator, iterator_handoff, current: current.clone(), ..Default::default() });
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
            let mut reassignment_holds = BTreeMap::<_, BTreeMap<_, usize>>::new();
            let mut legs_checked = 0;
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
                if let Some(next) = adjacent {
                    legs.push((t.mid_index(*e), false, Some((t.start_index(next[e]), cx.row_of(next[e])))));
                } else if after && point.is_some() {
                    legs.push((t.mid_index(*e), false, edge_to.map(|to| (t.start_index(to), cx.row_of(to)))));
                } else if after {
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
                    legs_checked += 1;
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
                        for &(local, name, origins, loans) in &drop_holders_at_reassign {
                            if untracked || !runs.contains(&e.block) || st.locals.get(name) != Some(&Some(local))
                                || st.state.get(name) != Some(&"own") { continue; }
                            let mut held = at(last, origins);
                            if let Some((first, _)) = nx {
                                let there = at(first, origins);
                                held.retain(|loan| there.contains(loan));
                            }
                            for &loan in held.intersection(loans) {
                                *reassignment_holds.entry(loan).or_default().entry(name).or_default() += 1;
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
                            if !active_option(local, name) || !live.contains_key(&id) { continue; }
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
            let mut explanations = BTreeMap::new();
            for (loan, holders) in reassignment_holds {
                let Some((&name, &legs)) = holders.first_key_value().filter(|_| holders.len() == 1) else { continue; };
                let id = BorrowIndex::from_usize(loan);
                // A path may have issued the loan and released it before
                // joining a path that still holds it.
                let released = |loc, after| {
                    let point = if after { t.mid_index(loc) } else { t.start_index(loc) };
                    pol.as_ref().is_some_and(|pol| !pol.loans_in_scope_at(point).contains(&id))
                };
                let some_paths = legs < legs_checked || may_skip(exits, after, &BTreeSet::from([loan]), imaginary, &released);
                explanations.insert(loan, DropLoanHolder { name: name.clone(), some_paths });
            }
            for loan in st.lent.values_mut().flatten() {
                loan.drop_holder = explanations.get(&loan.loan).cloned();
            }
        }
        // Borrowck origins can retain a loan across an overwrite or a
        // call. For projected slots and call results, trace the value
        // back to its issuance before naming an exact holder.
        let mut slot_issues = BTreeMap::new();
        for loan in st.lent.values_mut().flatten() {
            let Some(info) = loans.iter().find(|info| info.loan == loan.loan) else { continue };
            for name in &loan.current {
                let Some(&Some(local)) = st.locals.get(name) else { continue };
                let Some(&slot) = direct_slots.get(&(local, name.as_str())) else { continue };
                if slot.projection.is_empty() && returned_arguments.is_empty() { continue; }
                let origin = *slot_issues.entry((local, name.as_str())).or_insert_with(|| {
                    let mut found = exits.iter().map(|at| {
                        // The index marks the first instruction outside
                        // the backward slice, including a completed store.
                        let end = at.statement_index + usize::from(after && *at != body.terminator_loc(at.block));
                        super::call_wording::reaching_borrow(tcx, body, Location { statement_index: end, ..*at }, slot, &address_taken,
                            |def| returned_arguments.get(&def).copied().flatten())
                    });
                    let first = found.next()??;
                    found.all(|other| other == Some(first)).then_some(first)
                });
                if let Some((_, forwarded)) = origin.filter(|(issue, _)| Some(*issue) == taken_at(loan.loan)) {
                    if !slot.projection.is_empty() {
                        st.holder_sources.entry(name.clone()).or_default().insert(info.loan);
                    }
                    if forwarded { loan.forwarded_to.insert(name.clone()); }
                }
            }
        }
        // Use the same point's live holder origins. In particular, an
        // initialized local with a dead origin can remain in Lent::current.
        for (name, restrictions) in &st.lent {
            if !st.reference_types.contains_key(name) { continue; }
            for loan in restrictions {
                st.reference_carriers.entry(loan.loan).or_insert_with(|| {
                    let mut carriers: BTreeSet<String> = if point.is_some() {
                        loan.current.clone()
                    } else {
                        st.holds.iter().filter(|(_, ls)|
                            ls.contains(&loan.loan)).map(|(h, _)| h.clone()).collect()
                    };
                    // For shared direct references, the origin at this
                    // boundary can be live through a later use of an inner
                    // projection. Keep that direct carrier if no other
                    // origin supplies it. A mutable origin can outlive the
                    // reference that first carried it.
                    if carriers.is_empty() && loan.kind == "shared" && loan.current.len() == 1 {
                        carriers.extend(loan.current.iter().cloned());
                    }
                    // A live projection is a more precise carrier than the
                    // corresponding aggregate local.
                    for field in &loan.current {
                        if let Some(column) = column_info.iter().find(|col| col.name == *field && col.parent.is_some()) {
                            carriers.remove(&column.root);
                            carriers.insert(field.clone());
                        }
                    }
                    if taken_at(loan.loan).is_some() {
                        carriers.retain(|name| st.locals.get(name).copied().flatten()
                            .is_none_or(|local| !unchanged_arguments.contains(&local)));
                    }
                    carriers
                });
            }
        }
        for take in &manually_taken {
            if st.state.get(&take.column) != Some(&"own") { continue; }
            let mut paths = ExtractionPaths::default();
            for &loc in rt_exits {
                if let Some(status) = take.boundaries.get(&(loc, after)) {
                    paths.join(*status);
                }
            }
            if paths.taken {
                st.manually_taken.insert(take.column.clone(), Extraction { inner: take.inner.clone(), maybe: paths.intact });
            }
        }
        let mut fieldless_carriers = BTreeMap::new();
        for (name, loans) in &st.lent {
            let exclusive = loans.iter().any(|l| matches!(l.kind, "mut" | "unique"));
            let ids = loans.iter().filter(|l| matches!(l.kind, "mut" | "unique") == exclusive).map(|l| l.loan).collect();
            // A shared loan can be taken before the branch, yet only a
            // payload on some paths can use it after the join.
            let fieldless = (st.reference_types.contains_key(name))
                .then(|| match loans.as_slice() {
                    [loan] if loan.kind == "shared" => st.reference_carriers.get(&loan.loan),
                    _ => None,
                }).flatten()
                .and_then(|names| names.first().filter(|_| names.len() == 1))
                .and_then(|name| st.locals.get(name).copied().flatten())
                .is_some_and(|local| *fieldless_carriers.entry(local).or_insert_with(|| {
                    exits.iter().any(|&at| {
                        let at = if after && at != body.terminator_loc(at.block) { at.successor_within_block() } else { at };
                        stores.has_fieldless_variant(at, local)
                    })
                }));
            if fieldless || may_skip(exits, after, &ids, imaginary, &|_, _| false) {
                st.may_lend.insert(name.clone());
            }
        }
        st
    };
    let mut branch_tests = rustc_data_structures::fx::FxIndexMap::<_, BTreeSet<_>>::default();
    for exit in &switches {
        if let Some(branch) = exit.branch.filter(|_| checked.contains(&exit.edge.switch.block)) {
            branch_tests.entry(branch).or_default().insert(exit.edge.switch);
        }
    }
    let branch_entries: rustc_data_structures::fx::FxIndexMap<_, _> = branch_tests.into_iter().filter_map(|(branch, tests)| {
        // A guard or a short circuit can lower to more than one test. Only
        // use a test every other test of this conditional passes through.
        let first = tests.iter().copied().find(|&test| tests.iter().all(|&other| ldom(test, other)))?;
        Some((branch, state_at(&[first], false, false, None, None, None)))
    }).collect();
    let mut arm_boundaries = rustc_data_structures::fx::FxIndexMap::<_, BTreeMap<_, ArmBoundaries>>::default();
    let fieldless_arms: Vec<_> = evs.values().flatten().filter_map(|e| {
        let place = e.fieldless_operand.as_ref()?;
        if !e.end_places.contains(place) { return None; }
        super::control::entered(&switches, SwitchEdge { switch: e.location, successor: e.occurrence.successor?.block })
    }).collect();
    let entry_edges = switches.iter().filter_map(|exit| {
        let arm = super::SourceArm { branch: exit.branch?, index: exit.arm? };
        let edge = exit.edge;
        executed_successors(edge.switch).contains(&edge.successor.start_location())
            .then_some((arm, edge.successor))
    });
    let arm_edges: Vec<_> = entry_edges.clone().collect();
    let mut arm_entries = rustc_data_structures::fx::FxIndexMap::<_, BTreeMap<_, BTreeSet<_>>>::default();
    for (arm, block) in entry_edges {
        arm_entries.entry(arm.branch).or_default().entry(arm.index).or_default().insert(block.start_location());
    }
    for (branch, arms) in arm_entries {
        for (arm, entries) in arms {
            let boundaries = arm_boundaries.entry(branch).or_default().entry(arm).or_default();
            // Every incoming edge must enter this arm. In a short circuit
            // the same arm can be reached directly and through cleanup.
            if entries.iter().all(|entry| {
                let incoming = super::rows::real_predecessors(body, entry.block);
                super::control::common_entry(&switches, body, entry.block, &incoming)
                    == Some(super::SourceArm { branch, index: arm })
            }) {
                boundaries.own_rows = cx.rows.exits.iter().filter_map(|(&row, exits)| {
                    (!exits.is_empty() && exits.iter().all(|exit| entries.iter()
                        .any(|entry| doms.dominates(entry.block, exit.block)))).then_some(row)
                }).collect();
                let entries: Vec<_> = entries.into_iter().collect();
                // The switch can enter through StorageDead statements for
                // the test. The first substantive instruction bounds the
                // connector where we can still show the arm's entry state.
                boundaries.entry_row = entries.iter().map(|entry| {
                    let block = &body[entry.block];
                    let statement_index = block.statements.iter().position(|st| !matches!(
                        st.kind, StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop
                    )).unwrap_or(block.statements.len());
                    cx.row_of(Location { block: entry.block, statement_index })
                }).min();
                boundaries.entry = Some(state_at(&entries, false, false, None, None, None));
            }
        }
    }
    // One arm may have several imaginary incoming edges (a let chain).
    // Follow them through scrutinee cleanup to the first MIR locations
    // belonging to the else expression, and read the state before them.
    use rustc_hir::{intravisit, Expr, ExprKind};
    #[derive(Default)]
    struct BranchSpans<'hir> {
        if_else: Vec<(&'hir Expr<'hir>, &'hir Expr<'hir>, &'hir Expr<'hir>, Span)>,
        conditionals: Vec<&'hir Expr<'hir>>,
        labeled: BTreeSet<Span>,
    }
    impl<'hir> intravisit::Visitor<'hir> for BranchSpans<'hir> {
        fn visit_expr(&mut self, expr: &'hir Expr<'hir>) {
            if matches!(expr.kind, ExprKind::If(..) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal) | ExprKind::Block(_, Some(_)))
                && !expr.span.from_expansion()
            {
                self.conditionals.push(expr);
            }
            match expr.kind {
                ExprKind::If(guard, then, Some(els)) if !expr.span.from_expansion() =>
                    self.if_else.push((guard, then, els, expr.span)),
                ExprKind::Block(_, Some(_)) if !expr.span.from_expansion() => {
                    self.labeled.insert(expr.span);
                }
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
    let mut branch_spans = BranchSpans::default();
    if let Some(hir) = def_id.as_local().and_then(|id| tcx.hir_maybe_body_owned_by(id)) {
        intravisit::Visitor::visit_expr(&mut branch_spans, hir.value);
    }
    let mut imaginary_edges = Vec::new();
    for (guard, then, els, conditional) in branch_spans.if_else {
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
        let mut entry_state = state_at(&entry, false, true, None, None, None);
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
                        && edges.iter().all(|&&(_, source, _)| executes_at(source)
                            && runtime_blocks(source).all(|source| runtime_blocks(block)
                                .any(|block| rt_body.basic_blocks.dominators().dominates(block, source))))
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
            (!exits.is_empty()).then(|| (row, state_at(&exits, true, true, None, None, None)))
        }).collect();
        imaginary_edges.push(ImaginaryEdge {
            id: ImaginaryEdgeId(arm),
            entry: entry_state,
            after,
            join: if rejoin.is_empty() { State::default() } else { state_at(&rejoin.into_iter().collect::<Vec<_>>(), false, false, None, None, None) },
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
    // A successful let has a point of its own before the enclosing arm's
    // scope ends. rustc emits a ForLet read of the initialized binding,
    // including after the successful branch of `?`. The next source row
    // may instead join a path that skipped the entire declaration.
    let mut completed = BTreeMap::<usize, Vec<Location>>::new();
    for &loc in cx.rows.row.keys() {
        let data = &body.basic_blocks[loc.block];
        let Some(StatementKind::FakeRead(read)) = data.statements.get(loc.statement_index).map(|s| &s.kind) else { continue };
        if !matches!(read.0, rustc_middle::mir::FakeReadCause::ForLet(None)) { continue; }
        let Some(local) = read.1.as_local() else { continue };
        let Some(binding) = body.local_decls[local].source_info.scope.lint_root(&body.source_scopes) else { continue };
        if cx.names.declarations.get(&binding) != Some(&local) { continue; }
        let Some((_, rustc_hir::Node::LetStmt(ls))) = tcx.hir_parent_iter(binding).next() else { continue };
        let Some(init) = ls.init else { continue };
        if ls.span.from_expansion() { continue; }
        let end = cx.r.line_hi(ls.span);
        if cx.r.line_lo(ls.span) == end && !matches!(init.kind, rustc_hir::ExprKind::Match(_, _, rustc_hir::MatchSource::TryDesugar(_))) {
            continue;
        }
        // Only the boundary between statements on separate rows has a
        // segment of its own. An enclosing block ending here is a join.
        let Some(block) = tcx.hir_parent_iter(ls.hir_id).find_map(|(_, n)| match n {
            rustc_hir::Node::Block(b) => Some(b), _ => None,
        }) else { continue };
        if cx.r.line_hi(block.span) == end || block.stmts.iter().any(|s| s.span.lo() >= ls.span.hi() && cx.r.line_lo(s.span) == end)
            || block.expr.is_some_and(|e| e.span.lo() >= ls.span.hi() && cx.r.line_lo(e.span) == end) { continue; }
        // Follow the normal completion of this statement, including its
        // temporary destructors. Stop on its side of the edge: the next
        // block may also be reached by a path that skipped the binding.
        let mut work = vec![loc];
        let mut seen = BTreeSet::new();
        while let Some(at) = work.pop() {
            if !seen.insert(at) { continue; }
            for next in cx.r.real_succs(at) {
                if ls.span.contains(body.source_info(next).span.source_callsite()) {
                    work.push(next);
                } else {
                    completed.entry(end).or_default().push(at);
                }
            }
        }
    }
    // A let-chain can execute another operand after a test whose other
    // edge enters the else arm. The drawing places that evaluation on
    // the continuing leg, even when a later test ends up taking the else.
    let guard_edges: Vec<_> = switches.iter().filter_map(|s| {
        let branch = s.branch?;
        let rustc_hir::Node::Expr(expr) = tcx.hir_node(branch) else { return None };
        let ExprKind::If(cond, ..) = expr.kind else { return None };
        (s.arm.is_none() && switches.iter().any(|other|
            other.edge.switch == s.edge.switch && other.branch == Some(branch) && other.arm == Some(1)))
            .then_some((s.edge, branch, cond.hir_id))
    }).collect();
    let arm_paths = |locations: &[Location]| locations.iter().map(|&loc| {
        let id = body.location_to_hir_id(loc);
        let mut arms = super::control::enclosing_arms_with(tcx, id, false, |branch| short_circuit_branches.contains(&branch));
        let mut record_edge = |edge: SwitchEdge, branch, index| {
            let on_edge = exclusive_edges.iter().find(|(candidate, _)| *candidate == edge)
                .is_some_and(|(_, targets)| checked.contains(&loc.block)
                    && runtime_blocks(loc.block).all(|block| targets.iter().any(|&target| dom.dominates(target, block)))
                    && runtime_blocks(loc.block).next().is_some());
            if on_edge && !arms.iter().any(|arm| arm.branch == branch) {
                arms.push(super::SourceArm { branch, index });
            }
        };
        for switch in &switches {
            let (Some(branch), Some(index)) = (switch.branch, switch.arm) else { continue };
            record_edge(switch.edge, branch, index);
        }
        for &(edge, branch, condition) in &guard_edges {
            if id == condition || tcx.hir_parent_id_iter(id).any(|parent| parent == condition) {
                record_edge(edge, branch, 0);
            }
        }
        arms
    }).collect();
    let mut out_rows: Vec<Row> = Vec::new();
    let mut lines: BTreeSet<usize> = cx.rows.row.values().copied().collect();
    lines.extend(evs.keys().copied());
    lines.extend(scope_partials.keys().copied());
    lines.extend(block_ends.iter().copied());
    lines.extend(completed.keys().copied());
    for line in lines.into_iter().filter(|l| *l >= first_line && *l <= last_line) {
        let mut row = Row { line, partial_drops: scope_partials.remove(&line).unwrap_or_default(), ..Default::default() };
        if let Some(exits) = cx.rows.exits.get(&line) {
            row.after = state_at(exits, true, false, None, None, None);
            row.arm_paths = arm_paths(exits);
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
        row.runtime_entry = entries.get(&line).map(|locs| locs.iter().any(|l| executes_at(l.block)));
        if let Some(points) = completed.get(&line) {
            row.after = state_at(points, true, false, None, None, None);
            row.completed_let = true;
        }
        if let Some(locs) = entries.get(&line) {
            let locs: Vec<_> = locs.iter().copied().collect();
            row.before = Some(state_at(&locs, false, false, None, None, None));
            row.before_arm_paths = arm_paths(&locs);
            row.single_incoming_row = incoming_rows.get(&line)
                .and_then(|rows| rows.first().copied().filter(|_| rows.len() == 1));
        }
        if let Some(before) = &row.before {
            for ((local, column), points) in scoped_entries.remove(&line).into_iter().flatten() {
                if before.locals.get(&column) != Some(&Some(local))
                    || before.cause.get(&column) != Some(&"unassigned")
                    || row.after.locals.contains_key(&column)
                {
                    continue;
                }
                let scoped = state_at(&points, false, false, None, None, None);
                if scoped.locals.get(&column) == Some(&Some(local)) {
                    row.before_scope_end.insert(column, scoped);
                }
            }
        }
        if let Some(specs) = cx.rows.arms.get(&line) {
            let nested: BTreeSet<_> = specs.iter().filter_map(|s| s.parent).collect();
            for (i, s) in specs.iter().enumerate() {
                row.arms.push(Arm {
                    runs: s.exits.iter().any(|e| runs.contains(&e.block)),
                    runs_at_runtime: s.exits.iter().any(|e| executes_at(e.block)),
                    branch: s.branch,
                    arm: s.arm,
                    parent: s.parent,
                    col_lo: s.col_lo,
                    col_hi: s.col_hi,
                    has_exit: s.exits.iter().any(|exit| runs.contains(&exit.block)),
                    edge: s.edge,
                    continuing: (nested.contains(&i) && s.continuing.iter().any(|exit| runs.contains(&exit.block)))
                        .then(|| state_at(&s.continuing, true, false, None, None, None)),
                    after: state_at(&s.exits, true, false, None, None, None),
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
    // A line can have contributions from several paths. Only mark it
    // unreachable if none of its MIR locations execute.
    let run_lines: BTreeSet<usize> = cx.rows.row.iter().filter(|(l, _)| executes_at(l.block)).map(|(_, r)| *r).collect();
    let dead_lines: BTreeSet<usize> = cx.rows.row.iter()
        .filter(|(l, r)| checked.contains(&l.block) && !run_lines.contains(*r))
        .map(|(_, r)| *r)
        .filter(|r| *r >= first_line && *r <= last_line)
        .collect();
    let mut arm_runs = rustc_data_structures::fx::FxIndexMap::<_, BTreeMap<_, bool>>::default();
    for switch in &switches {
        let (Some(branch), Some(arm)) = (switch.branch, switch.arm) else { continue };
        if !checked.contains(&switch.edge.switch.block) { continue; }
        let runs = executed_successors(switch.edge.switch).contains(&switch.edge.successor.start_location());
        *arm_runs.entry(branch).or_default().entry(arm).or_default() |= runs;
    }
    // A drawn match leg also contains the guard's effects. Its body can
    // be unreachable even though the guard evaluates and mutates a place.
    let mut dead_guarded_bodies = rustc_data_structures::fx::FxHashSet::default();
    for (&branch, arms) in &mut arm_runs {
        let rustc_hir::Node::Expr(expr) = tcx.hir_node(branch) else { continue };
        let rustc_hir::ExprKind::Match(_, source_arms, _) = expr.kind else { continue };
        for (&index, runs) in arms.iter_mut().filter(|(_, runs)| !**runs) {
            let Some(arm) = source_arms.get(index) else { continue };
            let Some(guard) = arm.guard else { continue };
            let span = guard.span.with_parent(None);
            *runs = cx.rows.row.keys().any(|&loc| executes_at(loc.block)
                && span.contains(body.source_info(loc).span.source_callsite().with_parent(None)));
            if *runs { dead_guarded_bodies.insert(arm.body.hir_id); }
        }
    }
    let dead_arms = arm_runs.into_iter().map(|(branch, arms)| {
        (branch, arms.into_iter().filter_map(|(arm, runs)| (!runs).then_some(arm)).collect())
    }).collect();
    let constructed_vecs = runs.iter().filter_map(|&bb| {
        let expansion = body.basic_blocks[bb].terminator().source_info.span.ctxt().outer_expn();
        let &(call, _) = cx.vec_results.get(&expansion)?;
        (bb == call.block).then(|| (expansion, cx.row_of(call)))
    }).collect();
    let evaluated_spans = cx.rows.row.keys().filter(|loc| runs.contains(&loc.block) && !body.basic_blocks[loc.block].is_cleanup)
        .map(|loc| body.source_info(*loc).span.with_parent(None)).collect();
    let implicit_mut_reborrows = runs.iter().flat_map(|&bb| &body[bb].statements).filter_map(|st| {
        let StatementKind::Assign(a) = &st.kind else { return None; };
        let Rvalue::Ref(_, BorrowKind::Mut { .. }, place) = a.1 else { return None; };
        (place.projection.as_slice() == [ProjectionElem::Deref]
            && body.args_iter().any(|arg| arg == place.local)
            && body.local_decls[place.local].ty.ref_mutability().is_some_and(|m| m.is_mut()))
            .then_some((st.source_info.span.with_parent(None), place.local))
    }).collect();
    let mut effect_sites = BTreeMap::<_, Vec<_>>::new();
    for &bb in &runs {
        let data = &body.basic_blocks[bb];
        if data.is_cleanup { continue; }
        let runs = executes_at(bb);
        let mut add = |span: Span| effect_sites.entry(span.with_parent(None)).or_default().push((bb, runs));
        for statement in &data.statements {
            if matches!(statement.kind, StatementKind::Assign(_)) {
                add(statement.source_info.span);
            }
        }
        let term = data.terminator();
        if matches!(term.kind, TerminatorKind::Call { .. }) {
            add(term.source_info.span);
        }
    }
    let mut loops = rustc_data_structures::fx::FxIndexMap::default();
    let boundaries = super::control::loop_boundaries(tcx, body, &checked);
    let mut borrowed_slots = None;
    let mut may_alias = |place: Place<'tcx>, loc| {
        if place.is_indirect() { return true; }
        let borrowed = borrowed_slots.get_or_insert_with(|| {
            MaybeBorrowedLocals.iterate_to_fixpoint(tcx, body, None).into_results_cursor(body)
        });
        borrowed.seek_before_primary_effect(loc);
        borrowed.get().contains(place.local)
    };
    // An edge runs when the runtime CFG executes its source and takes it.
    // The runtime CFG already follows constant switches.
    let running = |from: Location, to: Location| executes_at(from.block)
        && (from.successor_within_block() == to
            || runtime_blocks(from.block).any(|b| real_successor_blocks(&rt_body, b).into_iter().any(|t|
                executes.contains(&t) && original_blocks.get(&t).copied().unwrap_or(t) == to.block)));
    for bounds in boundaries {
        let super::control::LoopBoundary { id, head: at, entries, backedges, exits } = bounds;
        if !executes_at(at.block) { continue; }
        let mut edge = |from, to, kind, jump| {
            let runs = running(from, to);
            let state = state_at(&[from], true, !runs, (!runs).then_some(&[]), None, None);
            LoopEdge { from, to, kind, runs, jump, state }
        };
        let head_entry = at.block.start_location();
        let mut edges = Vec::new();
        edges.extend(entries.iter().map(|&from| edge(from, head_entry, LoopEdgeKind::Entry, None)));
        edges.extend(backedges.iter().map(|&from| edge(from, head_entry, LoopEdgeKind::Backedge, None)));
        edges.extend(exits.iter().map(|exit| {
            let kind = if exit.exhaustion { LoopEdgeKind::Exhaustion } else { LoopEdgeKind::Break };
            edge(exit.from, exit.to, kind, Some(exit.jump))
        }));
        edges.dedup_by_key(|e| (e.from, e.to, e.jump));
        let head = state_at(&[at], false, false, None, None, None);
        let entry = if at.statement_index > 0 || at == Location::START {
            // SimplifyCfg can absorb pre-loop effects into the head block.
            // The head is the FalseUnwind, after these effects.
            head.clone()
        } else {
            state_at(&entries, true, false, None, None, None)
        };
        let executable_backedges: Vec<_> = edges.iter().filter(|e| e.runs && e.kind == LoopEdgeKind::Backedge)
            .map(|e| e.from).collect();
        // The checked CFG has the backedge but a constant condition
        // keeps the runtime from taking it.
        let constant_pruned_backedge = edges.iter().any(|e|
            e.kind == LoopEdgeKind::Backedge && !e.runs && runs.contains(&e.from.block)
                && real_successor_blocks(&drop_body, e.from.block).contains(&e.to.block));
        let next_iteration = (!executable_backedges.is_empty())
            .then(|| state_at(&executable_backedges, true, false, None, None, None));
        let runtime_exits: Vec<_> = exits.iter().filter(|e| running(e.from, e.to)).map(|e| e.from).collect();
        let continuation = (!runtime_exits.is_empty()).then(|| {
            let first = exits[0];
            let adjacent: BTreeMap<_, _> = exits.iter().map(|e| (e.last, e.continuation)).collect();
            let exact = exits.iter().all(|exit| exit.continuation == first.continuation)
                && (first.continuation.statement_index > 0
                    || body.basic_blocks.predecessors()[first.continuation.block].iter()
                        .all(|bb| adjacent.contains_key(&body.terminator_loc(*bb))));
            if exact {
                state_at(&[first.continuation], false, false, None, None, None)
            } else {
                // The shared continuation can also be reached from
                // outside the loop. Read only the edges after their cleanup.
                let last: Vec<_> = adjacent.keys().copied().collect();
                let runtime_last: Vec<_> = exits.iter().filter(|e| running(e.from, e.to))
                    .map(|e| e.last).collect::<BTreeSet<_>>().into_iter().flat_map(runtime_locations).collect();
                state_at(&last, true, false, Some(&runtime_last), Some(&adjacent), None)
            }
        });

        // Follow the edges backwards within an iteration. A write is
        // guaranteed before an exit only when no path from the head to
        // that exit bypasses it. Earlier iterations may still change the
        // state reported at the head and on every exit.
        let mut region = BTreeMap::new();
        let mut work = runtime_exits.clone();
        work.extend(&executable_backedges);
        while let Some(loc) = work.pop() {
            if region.contains_key(&loc) { continue; }
            let previous = if loc == at {
                Vec::new()
            } else if loc.statement_index > 0 {
                vec![Location { statement_index: loc.statement_index - 1, ..loc }]
            } else {
                // A runtime copy of a block stands for its original.
                super::rows::real_predecessors(&rt_body, loc.block).into_iter()
                    .map(|p| original_blocks.get(&p).copied().unwrap_or(p))
                    .collect::<BTreeSet<_>>().into_iter()
                    .filter(|&p| body.basic_blocks.dominators().dominates(at.block, p))
                    .map(|p| body.terminator_loc(p)).filter(|&from| running(from, loc)).collect()
            };
            work.extend(&previous);
            region.insert(loc, previous);
        }
        let mut round_region = BTreeSet::new();
        let mut work = executable_backedges;
        while let Some(loc) = work.pop() {
            if round_region.insert(loc) {
                work.extend(region.get(&loc).into_iter().flatten());
            }
        }
        let loop_span = tcx.hir_span(id);
        let inside = |loc: Location| executes_at(loc.block) && !body[loc.block].is_cleanup
            && body.basic_blocks.dominators().dominates(at.block, loc.block)
            && loop_span.contains(body.source_info(loc).span);
        let is_operand = |loc: Location| match body[loc.block].statements.get(loc.statement_index) {
            Some(s) => !matches!(s.kind, StatementKind::StorageLive(_) | StatementKind::StorageDead(_)),
            None => !matches!(body[loc.block].terminator().kind, TerminatorKind::Drop { .. }),
        };
        let outside_region: Vec<_> = cx.rows.row.keys().copied()
            .filter(|loc| !region.contains_key(loc) && inside(*loc)).collect();
        let wording_entries: Vec<_> = entries.iter().copied().filter(|from|
            wording_body.basic_blocks.dominators().is_reachable(from.block)
            && real_successor_blocks(wording_body, from.block).contains(&head_entry.block)).collect();
        let mut result_moves = BTreeMap::<_, Vec<_>>::new();
        for exit in exits.iter().filter(|e| !e.exhaustion && running(e.from, e.to)) {
            let Some(result) = exit.result else { continue; };
            let rustc_hir::Node::Expr(expr) = tcx.hir_node(exit.jump) else { continue; };
            let ExprKind::Break(_, Some(value)) = expr.kind else { continue; };
            result_moves.entry((exit.jump.local_id, exit.from, exit.to)).or_default()
                .extend(break_result_moves(body, result, value.span, &region, &mut may_alias));
        }
        let mut changes = BTreeMap::<_, LoopChange>::new();
        for (_, place, name) in columns.iter().chain(&synth) {
            if head.locals.get(name) != Some(&Some(place.local)) { continue; }
            let affects = |path: MovePathIndex| place_under(*place, move_data.move_paths[path].place);
            let mut writes = BTreeSet::new();
            let mut moves = BTreeSet::new();
            let mut moved_part = false;
            for &loc in region.keys() {
                if move_data.init_loc_map[loc].iter().any(|i| affects(move_data.inits[*i].path)) {
                    writes.insert(loc);
                }
                if is_operand(loc) {
                    for &m in move_data.loc_map[loc].iter() {
                        let path = move_data.moves[m].path;
                        if affects(path) {
                            moves.insert(loc);
                        } else if place_under(move_data.move_paths[path].place, *place) {
                            moved_part = true;
                        }
                    }
                }
            }
            let moved_away = outside_region.iter().any(|&loc| is_operand(loc)
                && move_data.loc_map[loc].iter().any(|m| affects(move_data.moves[*m].path)));
            let wrote_away = outside_region.iter().any(|&loc|
                move_data.init_loc_map[loc].iter().any(|i| affects(move_data.inits[*i].path)));
            if moves.is_empty() && !moved_part && writes.is_empty() && !moved_away && !wrote_away { continue; }
            let mut work = runtime_exits.clone();
            let mut seen = BTreeSet::new();
            let mut writes_on_all_exits = !work.is_empty();
            while let Some(loc) = work.pop() {
                if writes.contains(&loc) || !seen.insert(loc) { continue; }
                if region[&loc].is_empty() {
                    writes_on_all_exits = false;
                    break;
                }
                work.extend(&region[&loc]);
            }
            let mut change = LoopChange {
                moved: !moves.is_empty(), moved_part, moved_away, wrote: !writes.is_empty(), wrote_away, writes_on_all_exits,
                changes_next_iteration: writes.iter().chain(&moves).any(|loc| round_region.contains(loc)),
                ..Default::default()
            };
            if at.statement_index == 0 && !wording_entries.is_empty()
                && entry.cause.get(name) == Some(&"moved")
            {
                // In particular, a repeated comparison can rule out a
                // re-entry that would carry a previous run's value.
                if let (Some(init), LookupResult::Exact(path)) = (&mut wording_init, rt_md.rev_lookup.find(place.as_ref())) {
                    change.entry_empty_at_runtime = wording_entries.iter().all(|&from| {
                        init.seek_after_primary_effect(from);
                        !init.get().contains(path)
                    });
                }
            }
            for edge in edges.iter().filter(|e| e.runs && e.jump.is_some()) {
                let Some(jump) = edge.jump else { continue; };
                let carrying = result_moves.get(&(jump.local_id, edge.from, edge.to)).is_some_and(|operands|
                    operands.iter().any(|&(loc, moved)| moves.contains(&loc) && place_under(*place, moved)));
                change.carried |= carrying;
                if !matches!(edge.state.state.get(name), Some(&"moved" | &"maybe")) { continue; }
                match edge.kind {
                    LoopEdgeKind::Break if !carrying => {
                        change.moved_at_break = true;
                        let mut arms = super::control::enclosing_arms(tcx, jump, false);
                        arms.retain(|arm| match tcx.hir_node(arm.branch) {
                            rustc_hir::Node::Expr(e) => match e.kind {
                                rustc_hir::ExprKind::If(..) => !tcx.hir_parent_iter(arm.branch).take(2)
                                    .any(|(_, node)| matches!(node, rustc_hir::Node::Expr(parent)
                                        if matches!(parent.kind, rustc_hir::ExprKind::Loop(_, _, rustc_hir::LoopSource::While, _)))),
                                rustc_hir::ExprKind::Match(_, _, source) => matches!(source, rustc_hir::MatchSource::Normal),
                                _ => true,
                            },
                            _ => true,
                        });
                        change.branch_move |= !arms.is_empty() && moves.iter().any(|&loc| {
                            super::control::enclosing_arms(tcx, body.location_to_hir_id(loc), false)
                                .iter().any(|arm| arms.contains(arm))
                        });
                    }
                    LoopEdgeKind::Exhaustion => change.moved_at_exhaustion = true,
                    _ => {}
                }
            }
            changes.insert(name.clone(), change);
        }
        loops.insert(id, LoopState {
            at, row: cx.r.line_lo(tcx.hir_span(id)), entry, head, next_iteration, continuation, edges,
            constant_pruned_backedge, changes,
        });
    }

    // A residual site can be lowered more than once. Suppress its drop
    // wording only when none of the checked copies can execute.
    let mut residual_sites = BTreeMap::<_, bool>::new();
    for &bb in &checked {
        let TerminatorKind::Call { func, .. } = &body[bb].terminator().kind else { continue };
        if is_residual_conversion(tcx, func) {
            if let Some((call, site)) = residual_site(body.terminator_loc(bb)) {
                *residual_sites.entry(site).or_default() |= residual_runs(call);
            }
        }
    }
    // Read the MIR continuation before its first store or drop.
    let mut joins = rustc_data_structures::fx::FxIndexMap::default();
    let boundaries = super::control::conditional_joins(tcx, body, &cx.rows, &branch_spans.conditionals,
        &checked, &cx.names.bindings, &switches);
    for (id, at) in boundaries.iter().copied() {
        if !checked.contains(&at.block) { continue; }
        let span = tcx.hir_span(id);
        let mut preds: Vec<_> = if at.statement_index == 0 {
            body.basic_blocks.predecessors()[at.block].iter()
                .filter(|p| checked.contains(p)).map(|p| body.terminator_loc(*p)).collect()
        } else {
            vec![Location { statement_index: at.statement_index - 1, ..at }]
        };
        // SimplifyCfg can fold nested joins into the same block. Rustc's
        // completion jumps retain the enclosing expression's SourceInfo.
        // An edge completing only an enclosing expression skips this join.
        let block_pred_count = preds.len();
        preds.retain(|&from| {
            let source = body.source_info(from).span;
            !boundaries.iter().any(|&(other, dest)| dest == at && other != id
                && source == tcx.hir_span(other) && source != span && source.contains(span))
        });
        let edge_runs = |from: Location| runs.contains(&from.block)
            && (at.statement_index != 0 || real_successor_blocks(body, from.block).contains(&at.block));
        let incoming: Vec<_> = preds.iter().copied().filter(|&from| edge_runs(from)).collect();
        // Runtime dataflow reads every executable copy of an edge's source;
        // a constant condition or range can leave a checked edge dead.
        let executes = &executes;
        let runtime_rows = &runtime_rows;
        let runtime_edge = |from: Location| runtime_locations(from).filter(move |&copy|
            runtime_rows.real_succs(copy).into_iter().any(|to| executes.contains(&to.block)
                && original_location(to) == at));
        let edge_executes = |from| runtime_edge(from).next().is_some();
        let runtime_incoming: Vec<_> = incoming.iter().copied().flat_map(runtime_edge).collect();
        let checked_only = runtime_incoming.is_empty();
        if preds.is_empty() { continue; }
        let completed_local = body.basic_blocks[at.block].statements.get(at.statement_index).and_then(|first| {
            let StatementKind::FakeRead(read) = &first.kind else { return None };
            let (rustc_middle::mir::FakeReadCause::ForLet(_), place) = &**read else { return None };
            place.as_local()
        });
        if let Some(local) = completed_local.filter(|_| branch_spans.labeled.contains(&span)) {
            let line = tcx.sess.source_map().lookup_char_pos(span.hi()).line;
            // Follow the completed initializer to the end of its row.
            // More MIR on that row can move or borrow the new binding.
            let mut work = vec![at];
            let mut seen = BTreeSet::new();
            let mut exits = Vec::new();
            while let Some(loc) = work.pop() {
                if !seen.insert(loc) { continue; }
                for next in cx.r.real_succs(loc) {
                    if cx.row_of(next) > line
                        || (next.block != loc.block && !body.basic_blocks.dominators().dominates(at.block, next.block)) {
                        exits.push(next);
                    } else {
                        work.push(next);
                    }
                }
            }
            let mut completed = state_at(&exits, false, false, None, None, None);
            completed.state.retain(|name, _| completed.locals.get(name) == Some(&Some(local)));
            if let Some(row) = out_rows.iter_mut().find(|r| r.line == line).filter(|_| !completed.state.is_empty()) {
                row.completed_bindings.push(completed);
            }
        }
        // The arms' join connectors precede the merged state. A loan's
        // holder can still be initialized on one incoming edge only.
        if !arm_edges.is_empty() {
            if let Some(boundaries) = arm_boundaries.get_mut(&id) {
                let mut by_arm = BTreeMap::<_, Vec<_>>::new();
                for &pred in &incoming {
                    let arms: BTreeSet<_> = arm_edges.iter().filter_map(|(arm, successor)|
                        (arm.branch == id && doms.dominates(*successor, pred.block)).then_some(arm.index)).collect();
                    if let Some(&arm) = arms.first().filter(|_| arms.len() == 1) {
                        by_arm.entry(arm).or_default().push(pred);
                    }
                }
                for (arm, entries) in by_arm {
                    if let Some(bounds) = boundaries.get_mut(&arm).filter(|bounds| bounds.entry.is_some()) {
                        let snapshot = state_at(&entries, true, false, None, None, Some(SegmentPoint::Edge { from: entries[0], to: at }));
                        bounds.join_references_only = drop_holders_at_reassign.is_empty()
                            && !fieldless_arms.contains(&super::SourceArm { branch: id, index: arm });
                        bounds.join = Some(snapshot);
                    }
                }
            }
        }
        let unshared = preds.len() == block_pred_count;
        let mut state = if unshared {
            state_at(&[at], false, false, checked_only.then_some(&[][..]), None, None)
        } else {
            state_at(&preds, true, false, Some(&runtime_incoming), None, None)
        };
        // A nested join can exist only on a skipped edge. Its enclosing
        // continuation can still run. Follow MIR jumps across bookkeeping
        // only, so state at the continuation cannot describe a later move.
        let mut empty_at_runtime = BTreeSet::new();
        if checked_only {
            let mut next = at;
            let mut seen = BTreeSet::new();
            while seen.insert(next) {
                let runtime: Vec<_> = runtime_locations(next).collect();
                if !runtime.is_empty() {
                    empty_at_runtime = state_at(&[at], false, false, Some(&runtime), None, None).runtime_empty
                        .into_iter().filter_map(|(local, name)|
                            (state.locals.get(&name) == Some(&Some(local))).then_some(name))
                        .collect();
                    break;
                }
                let data = &body[next.block];
                if !data.statements[next.statement_index..].iter().all(|st| match &st.kind {
                    StatementKind::StorageLive(l) | StatementKind::StorageDead(l) =>
                        !body.local_decls[*l].is_user_variable(),
                    StatementKind::Assign(a) => a.0.as_local().is_some_and(|l| !body.local_decls[l].is_user_variable())
                        && matches!(&a.1, Rvalue::Use(Operand::Constant(c)) if c.ty().is_unit()),
                    StatementKind::Nop => true,
                    _ => false,
                }) {
                    break;
                }
                let TerminatorKind::Goto { target } = data.terminator().kind else { break };
                next = target.start_location();
            }
        }

        let tests: Vec<_> = match tcx.hir_node(id) {
            rustc_hir::Node::Expr(e) => match e.kind {
                ExprKind::If(cond, ..) => vec![cond.span],
                ExprKind::Match(scrutinee, arms, _) => std::iter::once(scrutinee.span)
                    .chain(arms.iter().filter_map(|arm| arm.guard.map(|g| g.span))).collect(),
                _ => vec![],
            },
            _ => vec![],
        };
        let in_test = |loc| tests.iter().any(|span| span.contains(body.source_info(loc).span.source_callsite()));
        let inside = |loc| span.contains(body.source_info(loc).span.source_callsite());
        // Rustc can insert an unspanned jump between a failed let-chain
        // test and the else arm. Keep that edge in the backward slice.
        let in_region = |loc: Location| inside(loc) || loc == body.terminator_loc(loc.block)
            && body.source_info(loc).span.is_dummy() && matches!(body[loc.block].terminator().kind, TerminatorKind::Goto { .. });
        let mut region = BTreeMap::<Location, Vec<Location>>::new();
        let mut work = incoming.clone();
        while let Some(loc) = work.pop() {
            if region.contains_key(&loc) { continue; }
            let previous = if !in_region(loc) {
                vec![]
            } else if loc.statement_index > 0 {
                vec![Location { statement_index: loc.statement_index - 1, ..loc }]
            } else {
                rt_body.basic_blocks.predecessors()[loc.block].iter().copied()
                    .filter(|p| runs.contains(p))
                    .map(|p| body.terminator_loc(p)).collect()
            };
            work.extend(&previous);
            region.insert(loc, previous);
        }
        let entries: Vec<_> = region.iter().filter(|(loc, previous)| {
            in_region(**loc) && (previous.is_empty() || previous.iter().any(|&prev| !in_region(prev)))
        }).map(|(&loc, _)| loc).collect();
        let entry = state_at(&entries, false, false, None, None, None);
        let effect_on_every_path = |effects: &BTreeSet<Location>| {
            if effects.is_empty() { return false; }
            let mut work = incoming.clone();
            let mut seen = BTreeSet::new();
            while let Some(l) = work.pop() {
                if effects.contains(&l) || !seen.insert(l) { continue; }
                let previous = &region[&l];
                if previous.is_empty() { return false; }
                work.extend(previous);
            }
            true
        };
        let writes_to = |place: Place<'tcx>| -> BTreeSet<_> {
            region.keys().copied().filter(|&l| inside(l) && move_data.init_loc_map[l].iter().any(|i| {
                let dest = move_data.move_paths[move_data.inits[*i].path].place;
                dest.local == place.local && place.projection.starts_with(dest.projection)
            })).collect()
        };
        let staged = conditional_result_stores(tcx, body, at, |place| cx.names.root(place).is_some(),
            |place| effect_on_every_path(&writes_to(place)));
        let mut changes = BTreeMap::new();
        let mut assignments = BTreeMap::new();
        let mut unchanged_shared_refs = BTreeMap::new();
        let if_edges = preds.len() >= 2 && preds.iter().all(|p| body.source_info(*p).span == span);
        let edge_states: Vec<_> = preds.iter().map(|&from| {
            let executable = edge_executes(from);
            let runtime: Vec<Location> = runtime_edge(from).collect();
            // A sole checked predecessor includes its normal return effect
            // at the destination (a Call initializes its result there).
            let mut edge_state = if unshared && preds.len() == 1 {
                state.state.clone()
            } else {
                state_at(&[from], true, !edge_runs(from), Some(&runtime), None, None).state
            };
            match body[from.block].terminator().kind {
                TerminatorKind::Call { destination, target: Some(target), .. }
                    if from == body.terminator_loc(from.block) && at == target.start_location() =>
                {
                    for (_, place, name) in columns.iter().chain(&synth) {
                        if place.local == destination.local && place.projection.starts_with(destination.projection) {
                            edge_state.insert(name.clone(), "own");
                        }
                    }
                }
                _ => {}
            }
            JoinEdge { from, runs: executable, state: edge_state }
        }).collect();
        for (_, place, name) in columns.iter().chain(&synth) {
            if state.locals.get(name) != Some(&Some(place.local)) { continue; }
            let affects = |path: MovePathIndex| {
                let operand = move_data.move_paths[path].place;
                operand.local == place.local && place.projection.starts_with(operand.projection)
            };
            let writes = writes_to(*place);
            let mut moves = BTreeSet::new();
            for &l in region.keys().filter(|&&l| inside(l)) {
                if !body[l.block].statements.get(l.statement_index)
                    .is_some_and(|s| matches!(s.kind, StatementKind::StorageDead(_) | StatementKind::StorageLive(_)))
                    && move_data.loc_map[l].iter().any(|m| affects(move_data.moves[*m].path))
                {
                    moves.insert(l);
                }
            }
            let moved = !moves.is_empty();
            let wrote = !writes.is_empty();
            let writes_on_all_paths = effect_on_every_path(&writes);
            let staged_result = staged.iter().any(|&dest| place_under(*place, dest));
            let writes_away = !wrote && state.state.get(name) == Some(&"maybe") && cx.rows.row.keys().any(|&l|
                inside(l) && runs.contains(&l.block) && !body[l.block].is_cleanup && !region.contains_key(&l)
                    && move_data.init_loc_map[l].iter().any(|i| affects(move_data.inits[*i].path)));
            let before = entry.state.get(name).filter(|_| entry.locals.get(name) == Some(&Some(place.local)));
            // An initialized immutable binding cannot be assigned again,
            // including on an exiting arm. Ending its loan leaves the
            // shared reference value in the slot.
            if let Some(local) = place.as_local().filter(|_| !moved && !wrote && !staged_result
                && before == Some(&"own") && state.state.get(name) == Some(&"own")) {
                let decl = &body.local_decls[local];
                if !decl.mutability.is_mut() && decl.ty.ref_mutability().is_some_and(|m| !m.is_mut()) {
                    // Only borrowck-tracked loans add an end explanation
                    // to a join that already describes the binding's moves.
                    let lost_loan = entry.holds.get(name).is_some_and(|before| before.iter().any(|id|
                        *id < base && state.holds.get(name).is_none_or(|after| !after.contains(id))));
                    unchanged_shared_refs.insert(local, if lost_loan {
                        SharedReferenceJoin::LoanEnded
                    } else {
                        SharedReferenceJoin::Unchanged
                    });
                }
            }
            if moved || wrote || staged_result || writes_away {
                let entry_maybe = matches!(before, Some(&"maybe" | &"moved"));
                let replaces_moved = moved && wrote && {
                    let mut work: Vec<_> = writes.iter().flat_map(|l| region[l].iter().copied()).collect();
                    let mut seen = BTreeSet::new();
                    let mut replaced = false;
                    while let Some(l) = work.pop() {
                        if !seen.insert(l) { continue; }
                        if moves.contains(&l) {
                            replaced = true;
                            break;
                        }
                        work.extend(&region[&l]);
                    }
                    replaced
                };
                let test_writes = writes.iter().copied().filter(|&at| in_test(at)).collect::<BTreeSet<_>>();
                changes.insert(name.clone(), JoinChange {
                    moved, wrote, writes_on_all_paths, entry_maybe, replaces_moved, staged_result,
                    moves_on_all_paths: effect_on_every_path(&moves),
                    test_moved: moves.iter().copied().any(in_test),
                    test_wrote: !test_writes.is_empty(),
                    test_writes_on_all_paths: writes_on_all_paths && effect_on_every_path(&test_writes),
                    writes_away,
                    entry_own: before == Some(&"own"),
                });
            }
            if let Some(mixed) = state.mixed_init.get_mut(name) {
                mixed.assigned_in_some_arms = wrote && !writes_on_all_paths;
            }
            // Runtime says the value is gone, though borrowck keeps it
            // as a maybe: the write the checker considers may not run.
            let skipped = state.borrowck_maybe.contains(name);
            if skipped || state.state.get(name) == Some(&"maybe") {
                if moved && state.cause.get(name) == Some(&"moved") {
                    state.moved_in_arm.insert(name.clone());
                }
                if if_edges && place.projection.is_empty() && wrote {
                    let always = state.always_assigned.contains(name);
                    let runtime_own = |runs: bool| edge_states.iter().filter(|edge|
                        edge.runs == runs && edge.state.get(name) == Some(&"own")).map(|edge| edge.from).collect::<Vec<_>>();
                    let own = runtime_own(true);
                    let runtime_empty = edge_states.iter().any(|edge| edge.runs
                        && matches!(edge.state.get(name), Some(&"moved" | &"unassigned")));
                    let (word, mut work) = if own.is_empty() {
                        (skipped.then_some(JoinAssignment::Skipped), runtime_own(false))
                    } else {
                        ((always || runtime_empty).then_some(if always { JoinAssignment::Always } else { JoinAssignment::Sometimes }), own)
                    };
                    if let Some(word) = word {
                        let mut seen = BTreeSet::new();
                        while let Some(l) = work.pop() {
                            if !seen.insert(l) { continue; }
                            if writes.contains(&l) {
                                assignments.insert(name.clone(), word);
                                break;
                            }
                            work.extend(&region[&l]);
                        }
                    }
                }
            }
        }
        let mut implicit_else = preds.iter().filter(|p| body[p.block].statements.iter().any(|s|
            s.source_info.span == span.shrink_to_hi() && matches!(&s.kind, StatementKind::Assign(a)
                if a.0.ty(&body.local_decls, tcx).ty.is_unit()))).peekable();
        let dead_implicit_else = implicit_else.peek().is_some() && implicit_else.all(|&p| !edge_runs(p));
        joins.insert(id, Join { at, state, incoming: edge_states, checked_only, empty_at_runtime, changes, assignments, unchanged_shared_refs, dead_implicit_else });
    }
    // Keep the same compiler coordinates for owners, fields and references.
    let mut points = BTreeSet::from([SegmentPoint::Before(Location::START)]);
    for row in &out_rows {
        let effects: Vec<_> = row.facts.iter().filter(|e| e.drop_route == DropRoute::Normal).collect();
        // A transfer and a release on its outgoing edge are distinct
        // drawing boundaries even when they share one MIR instruction.
        let transfer_releases = effects.iter().any(|e| matches!(e.fact, Fact::Copy { .. } | Fact::Move { .. }))
            && effects.iter().any(|e| matches!(e.fact, Fact::BorrowEnd { .. } | Fact::HoldEnd { .. }));
        if !transfer_releases && !effects.first().is_some_and(|first| effects.iter().any(|e| e.location != first.location)) {
            continue;
        }
        for event in effects {
            let at = event.occurrence.at;
            points.extend([SegmentPoint::Before(at), SegmentPoint::After(at)]);
            if let Some(to) = event.occurrence.successor {
                points.insert(SegmentPoint::Edge { from: at, to });
            }
            if let Fact::Borrow { loan, .. } = event.fact {
                if let Some(&activation) = activation.get(&loan) {
                    points.insert(SegmentPoint::After(activation));
                }
            }
        }
    }
    // A projected operand of an aggregate can have a mark on a
    // different display row from the aggregate's MIR instruction.
    for row in &out_rows {
        for event in &row.facts {
            let at = event.occurrence.at;
            if matches!(event.fact, Fact::Move { .. })
                && body[at.block].statements.get(at.statement_index).is_some_and(|stmt|
                    matches!(&stmt.kind, StatementKind::Assign(assign) if
                        matches!(&assign.1, Rvalue::Aggregate(_, operands) if operands.iter()
                            .any(|op| matches!(op, Operand::Move(p) if !p.projection.is_empty())))))
            {
                points.insert(SegmentPoint::After(at));
            }
        }
    }
    let mut segment_points = BTreeMap::new();
    for point in points {
        let call_result = |at: Location| {
            (at == body.terminator_loc(at.block)).then(|| body[at.block].terminator())
                .and_then(|term| match term.kind {
                    TerminatorKind::Call { target: Some(target), .. }
                        if body.basic_blocks.predecessors()[target].as_slice() == [at.block] => Some(target.start_location()),
                    _ => None,
                })
        };
        let (at, after) = match point {
            SegmentPoint::Before(at) => (at, false),
            SegmentPoint::After(at) => call_result(at).map_or((at, true), |to| (to, false)),
            SegmentPoint::Edge { from, to } if call_result(from) == Some(to) => (to, false),
            SegmentPoint::Edge { from, .. } => (from, true),
        };
        let boundary = if after { point } else { SegmentPoint::Before(at) };
        segment_points.insert(point, state_at(&[at], after, false, None, None, Some(boundary)));
    }
    // NLL may have already released the old value at a later reassignment.
    // Find the stores that actually reach the write, stopping on each path
    // at its first definition. Record the loans at that definition, when
    // the value was produced, rather than their liveness at the overwrite.
    let has_repoints = out_rows.iter().flat_map(|r| &r.facts).any(|e| matches!(e.fact, Fact::Repoint { .. }));
    let mut direct_releases = BTreeMap::new();
    if let Some(t) = table.filter(|_| has_repoints) {
        let slots: BTreeSet<_> = out_rows.iter().flat_map(|r| &r.facts).filter_map(|e| {
            if !matches!(e.fact, Fact::Repoint { .. }) { return None; }
            let StatementKind::Assign(write) = &body[e.location.block].statements[e.location.statement_index].kind else { return None };
            write.0.as_local()
        }).collect();
        for end in out_rows.iter().flat_map(|r| &r.facts) {
            let Fact::BorrowEnd { loan, .. } = end.fact else { continue };
            let [p] = end.end_places.as_slice() else { continue };
            let holder = Local::from_usize(p.local);
            if !p.projection.is_empty() || !slots.contains(&holder) { continue; }
            let at = end.occurrence.at;
            // An unused reference can end at the exit of the very store
            // that installs it. At a call the holder supplied its operand.
            let before = if body[at.block].statements.get(at.statement_index).is_some_and(|stmt|
                matches!(&stmt.kind, StatementKind::Assign(write) if write.0.as_local() == Some(holder))) {
                at.successor_within_block()
            } else { at };
            if stores.direct_borrow(before, holder.into()).is_some_and(|issue| taken_at(loan) == Some(issue)) {
                let key = (loan, end.occurrence);
                direct_releases.insert(key, usable_after_release.contains(&key));
            }
        }
        let carried = |src, at| held_by_place(src, at).into_iter()
            .filter(|id| published.contains(id)).collect::<BTreeSet<_>>();
        for e in out_rows.iter_mut().flat_map(|r| &mut r.facts) {
            let Fact::Repoint { definitions, reaching, maybe_uninitialized, old, new, .. } = &mut e.fact else { continue };
            let loc = e.occurrence.at;
            let Some(stmt) = body[loc.block].statements.get(loc.statement_index) else { continue };
            let StatementKind::Assign(write) = &stmt.kind else { continue };
            let Some(slot) = stores.slot_before(loc, write.0) else { continue; };
            let incoming = stores.definitions(loc, slot);
            *maybe_uninitialized = incoming.uninitialized;
            definitions.extend(incoming.sites);
            if incoming.unknown || incoming.argument { continue; }
            if !executes_at(loc.block) { continue; }
            let origins = direct.reaching(definitions, slot);
            if origins.unknown { continue; }
            let mut ids = origins.loans;
            if slot.projection.is_empty() {
                // The local repoint layout also tracks the original
                // restriction kept by a reborrow of the entire reference.
                ids.extend(origins.retained.into_keys());
            }
            for (next, place) in origins.calls {
                ids.extend(carried(place, t.start_index(next)));
            }
            if !slot.projection.is_empty() {
                inscope.seek_before_primary_effect(loc);
                let mut live: BTreeSet<_> = inscope.get().iter().collect();
                live.extend(untracked_at(t.start_index(loc)));
                old.extend(ids.iter().copied().filter(|id| live.contains(&BorrowIndex::from_usize(*id)) && !new.contains(id)));
            }
            *reaching = Some(ids.into_iter().collect());
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
    if let Some(t) = table {
        let loan_parents: BTreeMap<_, _> = loans.iter().map(|l| (l.loan, l.made_through.as_slice())).collect();
        for it in &mut for_loops {
            let storage = Place::from(it.storage);
            let term = body[it.next.block].terminator();
            if let TerminatorKind::Call { args, .. } = &term.kind {
                if let Some(receiver) = args[0].node.place() {
                    it.loans = held_by_place(receiver, t.start_index(it.next)).intersection(&published).copied().collect();
                }
            }
            let TerminatorKind::Call { args: head, .. } = &body[it.constructor.block].terminator().kind else { continue };
            it.head_loan_retained = it.head.as_ref().and_then(|h| h.borrow_at)
                .is_some_and(|at| it.loans.iter().any(|id| taken_at(*id) == Some(at)));
            // A custom conversion can return an unrelated referent even if its
            // signature ties the item's lifetime to the input.
            if !it.identity && !it.collection {
                it.opaque_item_loans.extend(&it.loans);
            }
            let head_span = head[0].span;
            // A unique normal call-return edge can end an intermediate
            // result's loan before the loop starts (iter().copied().collect()).
            let mut ends = BTreeMap::<usize, BTreeSet<Option<Location>>>::new();
            for ev in out_rows.iter().flat_map(|r| &r.facts) {
                let Fact::BorrowEnd { loan, .. } = ev.fact else { continue };
                let at = ev.occurrence.at;
                let term = body[at.block].terminator();
                let normal = at == body.terminator_loc(at.block) && head_span.contains(term.source_info.span)
                    && matches!(term.kind, TerminatorKind::Call { target: Some(target), .. }
                        if ev.occurrence.successor == Some(Location { block: target, statement_index: 0 }));
                ends.entry(loan).or_default().insert(normal.then_some(at));
            }
            for (bb, data) in body.basic_blocks.iter_enumerated() {
                let term = data.terminator();
                let site = term.source_info.span;
                if !head_span.contains(site) || site.desugaring_kind().is_some() { continue; }
                let TerminatorKind::Call { func, args, destination, target: Some(target), .. } = &term.kind else { continue };
                let ret = held_by_place(*destination, t.start_index(Location { block: *target, statement_index: 0 }));
                let carried: BTreeSet<_> = ret.intersection(&published).copied().collect();
                if carried.is_empty() { continue; }
                let callee = func.const_fn_def();
                if !callee.is_some_and(|(def, types)| super::iterators::preserves_items(tcx, typing_env, def, types)) {
                    it.opaque_item_loans.extend(it.loans.intersection(&carried));
                }
                let Some((callee, _)) = callee else { continue };
                let at = body.terminator_loc(bb);
                for arg in args {
                    let Some(p) = arg.node.place() else { continue };
                    let held = held_by_place(p, t.start_index(at));
                    let loans: BTreeSet<_> = carried.intersection(&held).copied().collect();
                    if loans.is_empty() { continue; }
                    // Parent loans can reach the loop through a different
                    // field. Only the immediate loans describe this operand.
                    let parents: BTreeSet<_> = loans.iter().flat_map(|id|
                        loan_parents.get(id).copied().into_iter().flatten().copied()).collect();
                    let loans: BTreeSet<_> = loans.difference(&parents).copied().collect();
                    let reaches_storage = !loans.is_disjoint(&it.loans);
                    let ends_after_call = (!reaches_storage).then(|| {
                        let mut sites = loans.iter().map(|id| {
                            let choices = ends.get(id)?;
                            if choices.len() != 1 { return None; }
                            *choices.first()?
                        });
                        let end = sites.next()??;
                        if !sites.all(|other| other == Some(end)) { return None; }
                        let TerminatorKind::Call { func, .. } = &body[end.block].terminator().kind else { return None };
                        Some(tcx.item_name(func.const_fn_def()?.0).to_string())
                    }).flatten();
                    it.arguments.push(IteratorArgument {
                        reaches_storage, ends_after_call,
                        site, operand: arg.span, call: tcx.item_name(callee).to_string(), loans,
                        input: super::iterators::head_input(tcx, body, at, &arg.node, |p| cx.name(p), &mut None),
                    });
                }
            }
            for binding in &mut it.bindings {
                binding.loans = held_by(binding.local, t.mid_index(binding.at)).intersection(&published).copied().collect();
                let Some(&at) = item_moves.get(&(it.storage, binding.at)) else { continue };
                // The holder end is on the move's own edge. A loan merely
                // present in both types need not be represented by this end.
                let edges = out_rows.iter().flat_map(|row| row.facts.iter().map(move |e| (row.line, e)))
                    .chain(inline_holder_ends.iter().map(|(row, e)| (*row, e)));
                for (row, e) in edges {
                    if e.occurrence.at != at { continue; }
                    let Fact::HoldEnd { loan, .. } = e.fact else { continue };
                    if !binding.loans.contains(&loan) || !it.loans.contains(&loan)
                        || !e.end_places.iter().any(|p| p.local == binding.local.index() && p.projection.is_empty()) {
                        continue;
                    }
                    let Some(to) = e.occurrence.successor else { continue };
                    if held_by_place(storage, t.start_index(to)).contains(&loan) {
                        binding.handoffs.push(IteratorHandoff {
                            loan, occurrence: e.occurrence, row,
                            next_on_all_paths: super::iterators::proceeds_to_next(body, to, it.next),
                        });
                        binding.consumed = Some(at);
                    }
                }
            }
        }
    }
    for it in &mut for_loops {
        for row in &out_rows {
            for ev in row.facts.iter().filter(|ev| ev.place_local == Some(it.storage) && matches!(ev.fact, Fact::TempDrop { .. })) {
                let exit = ev.occurrence.exit;
                let after_item = exit.and_then(|exit| {
                    let mut items = row.facts.iter().filter_map(|item| {
                        let Fact::Drop { path, how: "drops", .. } = &item.fact else { return None };
                        let local = item.place_local?;
                        (cx.names.locals.get(&local) == Some(path) && item.occurrence.exit == Some(exit)
                            && it.bindings.iter().any(|b| b.local == local)
                            && super::iterators::drops_before(body, item.occurrence.at.block, ev.occurrence.at.block)).then_some(path)
                    });
                    let item = items.next()?;
                    items.next().is_none().then(|| item.clone())
                });
                it.drops.push(IteratorDrop { occurrence: ev.occurrence, exit_span: exit.map(|at| cx.r.exit_span(at)), after_item });
            }
        }
    }
    let indexed_reads = table.map_or_else(BTreeMap::new, |t| super::indexed::reads(tcx, body,
        |place| cx.name(place),
        |at, place| held_by_place(place, t.start_index(at)).intersection(&published).copied().collect(),
        |at| issued.get(&at).copied(),
        |at, place| {
            let Some(origins) = drop_origins.get(&place.local) else { return BTreeSet::new() };
            let projected = regions_of(decl_place_ty(tcx, body, place));
            let drop_regions: Vec<_> = origins.iter().copied().filter(|o| projected.contains(o)).collect();
            held_in(&drop_regions, t.start_index(at)).into_iter()
                .chain(held_in(&drop_regions, t.mid_index(at))).collect()
        },
    ));
    scope_ends.terminal_calls = terminal_calls;
    let value_flow = super::value_flow::collect(tcx, body, &cx.names, &runs, &checked, &cx.rows.event_row);
    let capture_loans: BTreeMap<_, _> = call_relationships.captures.iter().filter_map(|capture| {
        let issue = capture.issue?;
        let loan = bwf.borrow_set.location_map().get_index_of(&issue)?;
        Some((BorrowIndex::from_usize(loan), capture.closure))
    }).collect();
    for call in &mut call_relationships.calls {
        call.holders = call_holders.remove(&call.location);
        if !capture_loans.is_empty() {
            if let TerminatorKind::Call { target: Some(target), .. } = body[call.location.block].terminator().kind {
                inscope.seek_before_primary_effect(Location { block: target, statement_index: 0 });
                call.retained_captures.extend(inscope.get().iter().filter_map(|id| capture_loans.get(&id)));
            }
        }
    }
    if let (Some(t), Some(pol)) = (table, pol.as_ref()) {
        let capture_issues: BTreeSet<_> = call_relationships.captures.iter()
            .filter(|capture| capture.transfer == super::call_relationships::CaptureTransfer::Shared)
            .filter_map(|capture| capture.issue).collect();
        let mut call_ends = BTreeMap::<_, Vec<_>>::new();
        for event in out_rows.iter().flat_map(|row| &row.facts) {
            if let Fact::BorrowEnd { loan, .. } = &event.fact {
                call_ends.entry(event.location).or_default().push((*loan, &event.end_places));
            }
        }
        for call in call_relationships.calls.iter_mut().filter(|call| call.kind.is_callable()) {
            let TerminatorKind::Call { args, .. } = &body[call.location.block].terminator().kind else { continue };
            let Some(receiver) = args.first().and_then(|arg| arg.node.place()) else { continue };
            let borrow = super::call_wording::operand_borrow(body, call.location, receiver);
            let source = borrow.map_or(receiver, |(_, place)| place);
            let point = t.start_index(call.location);
            let live = pol.loans_in_scope_at(point);
            let capture_reaches_call = |issue| capture_issues.contains(&issue) && ldom(issue, call.location);
            call.receiver_loans = held_by_place(source, point).into_iter().filter(|&id| {
                live.contains(&BorrowIndex::from_usize(id)) && taken_at(id).is_some_and(capture_reaches_call)
            }).collect();
            for &(id, places) in call_ends.get(&call.location).into_iter().flatten() {
                let Some(issue) = taken_at(id) else { continue };
                if capture_reaches_call(issue) && places.iter().any(|place| place.local == source.local.index()) {
                    call.receiver_loans.insert(id);
                }
                call.receiver_borrow_ends |= borrow.is_some_and(|(at, _)| at == issue);
            }
        }
    }
    BodyFacts {
        writes,
        scope_ends,
        call_relationships,
        value_flow,
        joined_argument_loans,
        exact_reborrow_parents,
        usable_after_release,
        segment_points,
        residual_drops,
        skipped_residuals: residual_sites.into_iter().filter_map(|(site, runs)| (!runs).then_some(site)).collect(),
        short_call_borrows,
        drop_holders,
        reassign_drop_loans,
        drop_moves,
        short_circuit_branches,
        explicit_drop_ends,
        switches,
        bindings: cx.names.bindings.into(),
        returned_reference_loans,
        closure_result_loans,
        conditional_ends,
        aggregate_loans,
        indexed_reads,
        variant_constructions,
        issued_operand_loans,
        returned_operand_loans,
        inline_holder_ends,
        definite_hold_origins,
        definite_reborrow_sources,
        evaluated_spans,
        implicit_mut_reborrows,
        effect_sites,
        drop_flags,
        drop_part_flags,
        restored_reborrows, direct_releases,
        imaginary_edges,
        constructed_vecs,
        take_calls: take_calls.into_iter().map(|loc| body.source_info(loc).span.with_parent(None)).collect(),
        skipped_closure_returns,
        binding_sources,
        joins,
        loops,
        branch_entries,
        arm_boundaries,
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
        closure_captures,
        iterators: for_loops,
        binding_loans,
        rows: out_rows,
        promoted_operands,
        temps,
        notes,
        dead_lines,
        dead_arms,
        dead_guarded_bodies,
    }
}

/// The region variable a body's region is (after borrowck's renumbering).
fn region_vid(r: rustc_middle::ty::Region<'_>) -> Option<rustc_middle::ty::RegionVid> {
    match r.kind() {
        rustc_middle::ty::RegionKind::ReVar(v) => Some(v),
        _ => None,
    }
}

/// A call operand comes from an aggregate built while evaluating the
/// argument. Named sources already carried their references before the call.
fn operand_construction<'tcx>(
    body: &Body<'tcx>,
    runs: &BTreeSet<BasicBlock>,
    mut at: Location,
    mut place: Place<'tcx>,
) -> Option<Location> {
    let mut seen = BTreeSet::new();
    while place.as_local().is_some() && !body.local_decls[place.local].is_user_variable()
        && seen.insert((at, place.local))
    {
        let block = &body[at.block];
        for (i, stmt) in block.statements[..at.statement_index].iter().enumerate().rev() {
            match &stmt.kind {
                StatementKind::Assign(a) if a.0.local == place.local => {
                    if a.0 != place { return None; }
                    match &a.1 {
                        Rvalue::Use(op) => {
                            place = op.place()?;
                            if place.as_local().is_none() || body.local_decls[place.local].is_user_variable() { return None; }
                        }
                        Rvalue::Aggregate(..) | Rvalue::Repeat(..) => return Some(Location { statement_index: i, ..at }),
                        _ => return None,
                    }
                }
                StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if *l == place.local => return None,
                _ => {}
            }
        }
        let mut predecessors = body.basic_blocks.predecessors()[at.block].iter().copied()
            .filter(|&bb| runs.contains(&bb) && super::rows::real_successor_blocks(body, bb).contains(&at.block));
        let prev = predecessors.next()?;
        if predecessors.next().is_some() { return None; }
        at = body.terminator_loc(prev);
        if matches!(body[prev].terminator().kind, TerminatorKind::Call { destination, .. } if destination == place) {
            return Some(at);
        }
    }
    None
}

/// Only standard, nonempty inputs establish that Vec::extend appends.
fn nonempty_extend_input<'tcx>(cx: &Cx<'_, 'tcx>, at: Location, input: Place<'tcx>) -> bool {
    let (tcx, body) = (cx.tcx, cx.body);
    let ty = decl_place_ty(tcx, body, input);
    if let TyKind::Array(_, len) = ty.builtin_deref(false).unwrap_or(ty).kind() {
        return len.try_to_target_usize(tcx).is_some_and(|n| n > 0);
    }
    if ty.ty_adt_def().is_some_and(|adt| tcx.is_diagnostic_item(rustc_span::sym::Option, adt.did()))
        && input.as_local().is_some_and(|local| !body.local_decls[local].is_user_variable()) {
        let assign = body[at.block].statements[..at.statement_index].iter().rev().find_map(|stmt| {
            let StatementKind::Assign(assign) = &stmt.kind else { return None };
            (assign.0.local == input.local).then_some(assign)
        });
        return assign.is_some_and(|assign| assign.0 == input && matches!(&assign.1,
            Rvalue::Aggregate(kind, operands) if operands.len() == 1
                && matches!(&**kind, AggregateKind::Adt(def, ..) if tcx.is_diagnostic_item(rustc_span::sym::Option, *def))));
    }
    #[expect(clippy::disallowed_methods, reason = "only tests membership among MIR destinations")]
    cx.vec_results.values().any(|(site, result)| {
        if *result != input { return false; }
        let TerminatorKind::Call { func, args, .. } = &body[site.block].terminator().kind else { return false };
        let Some((def, _)) = func.const_fn_def() else { return false };
        if tcx.is_diagnostic_item(rustc_span::Symbol::intern("vec_from_elem"), def) {
            args.get(1).and_then(|a| a.node.constant())
                .and_then(|c| c.const_.try_eval_target_usize(tcx, body.typing_env(tcx))).is_some_and(|n| n > 0)
        } else {
            // The other construction here is vec![element, ...].
            true
        }
    })
}

/// Follow the normal edge and direct transfers of a call's temporary
/// result until it reaches a user place.
fn call_result_store<'tcx>(cx: &Cx<'_, 'tcx>, mut value: Place<'tcx>, mut at: Location,
    writable_addresses: &[(Location, Place<'tcx>)],
) -> Option<(Place<'tcx>, Location)> {
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(at) { return None; }
        let block = &cx.body.basic_blocks[at.block];
        for (idx, stmt) in block.statements.iter().enumerate().skip(at.statement_index) {
            match &stmt.kind {
                StatementKind::Assign(asg) => {
                    if matches!(asg.1, Rvalue::Ref(_, _, p) | Rvalue::RawPtr(_, p) if p.local == value.local) {
                        return None;
                    }
                    let loc = Location { block: at.block, statement_index: idx };
                    let suffix = |src: Place<'tcx>| {
                        (src.local == value.local).then(|| value.projection.strip_prefix(src.projection.as_slice())).flatten()
                    };
                    let next = match &asg.1 {
                        Rvalue::Use(op) => op.place().and_then(suffix).map(|rest| asg.0.project_deeper(rest, cx.tcx)),
                        Rvalue::Aggregate(kind, ops) => {
                            let ty = decl_place_ty(cx.tcx, cx.body, asg.0);
                            let mut targets = ops.iter_enumerated().filter_map(|(field, op)| {
                                let rest = suffix(op.place()?)?;
                                let fty = aggregate_field_ty(cx.tcx, kind, ty, field)?;
                                let mut dest = asg.0;
                                if let AggregateKind::Adt(def, variant, ..) = **kind {
                                    let def = cx.tcx.adt_def(def);
                                    if def.is_union() { return None; }
                                    if def.is_enum() {
                                        dest = dest.project_deeper(&[ProjectionElem::Downcast(Some(def.variant(variant).name), variant)], cx.tcx);
                                    }
                                }
                                Some(dest.project_deeper(&[ProjectionElem::Field(field, fty)], cx.tcx).project_deeper(rest, cx.tcx))
                            });
                            let first = targets.next();
                            if targets.next().is_some() { return None; }
                            first
                        }
                        _ => None,
                    };
                    if let Some(next) = next {
                        if cx.name(next).is_some() { return Some((next, loc)); }
                        value = next;
                    } else if asg.0.local == value.local {
                        return None;
                    }
                }
                StatementKind::StorageDead(local) if *local == value.local => return None,
                _ => {}
            }
        }
        match &block.terminator().kind {
            TerminatorKind::Goto { .. } => {}
            // An intermediate operand can itself be a call. On the normal
            // edge the saved result survives if that call neither uses it
            // nor can write to it through an address.
            TerminatorKind::Call { func, args, destination, target: Some(target), .. }
                if !destination.is_indirect() && destination.local != value.local
                    && func.place().is_none_or(|p| p.local != value.local)
                    && args.iter().all(|arg| arg.node.place().is_none_or(|p| p.local != value.local))
                    && writable_addresses.iter().all(|(_, p)| p.local != value.local) =>
            {
                at = Location { block: *target, statement_index: 0 };
                continue;
            }
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
pub(super) fn decl_place_ty<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, place: Place<'tcx>) -> rustc_middle::ty::Ty<'tcx> {
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
pub(super) fn resolve_temp_deref<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, place: Place<'tcx>) -> Place<'tcx> {
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
        if for_write && body.local_decls[place.local].ty.is_box() {
            return place;
        }
        let mut src: Option<Place<'tcx>> = None;
        let mut n = 0;
        for (block, data) in body.basic_blocks.iter_enumerated() {
            for (statement_index, st) in data.statements.iter().enumerate() {
                if let StatementKind::Assign(asg) = &st.kind {
                    if for_write && matches!(asg.1, Rvalue::Ref(_, BorrowKind::Mut { .. }, p) | Rvalue::RawPtr(_, p)
                        if p.local == place.local && p.projection.is_empty())
                    {
                        let init = Location { block, statement_index };
                        if !matches!(asg.1, Rvalue::Ref(..)) || asg.0.as_local().is_none_or(|alias|
                            reference_slot_can_change(body, alias, init)) {
                            return place;
                        }
                    }
                    if asg.0.local == place.local && (!for_write || !asg.0.is_indirect()) {
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
                if destination.local == place.local && (!for_write || !destination.is_indirect()) {
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

/// A borrow of a reference slot can only change its stored pointer by
/// writing through the first dereference or by passing that address on.
/// Uses through two dereferences reach the referent instead.
fn reference_slot_can_change(body: &Body<'_>, alias: Local, init: Location) -> bool {
    use rustc_middle::mir::visit::{MutatingUseContext, NonMutatingUseContext, PlaceContext, Visitor};
    struct Uses {
        alias: Local,
        init: Location,
        can_change: bool,
    }
    impl<'tcx> Visitor<'tcx> for Uses {
        fn visit_place(&mut self, place: &Place<'tcx>, context: PlaceContext, loc: Location) {
            if place.local != self.alias || matches!(context, PlaceContext::NonUse(_)) {
                return;
            }
            let reads_only = matches!(context, PlaceContext::NonMutatingUse(
                NonMutatingUseContext::Inspect | NonMutatingUseContext::PlaceMention | NonMutatingUseContext::FakeBorrow
            ));
            let defines_alias = loc == self.init && matches!(context, PlaceContext::MutatingUse(MutatingUseContext::Store));
            if !reads_only && !defines_alias
                && !place.projection.starts_with(&[ProjectionElem::Deref, ProjectionElem::Deref])
            {
                self.can_change = true;
            }
        }
    }
    let mut uses = Uses { alias, init, can_change: false };
    uses.visit_body(body);
    uses.can_change
}


#[cfg(test)]
mod owner_segment_tests {
    use super::*;

    #[test]
    fn compiler_edge_snapshots_preserve_the_selected_path() {
        struct Check;

        impl rustc_driver::Callbacks for Check {
            fn config(&mut self, config: &mut rustc_interface::Config) {
                config.override_queries = Some(rustc_utils::mir::borrowck_facts::override_queries);
            }

            fn after_analysis<'tcx>(
                &mut self, _: &rustc_interface::interface::Compiler, tcx: TyCtxt<'tcx>,
            ) -> rustc_driver::Compilation {
                let facts = crate::facts::collect_crate(tcx, &|_| true);
                assert!(facts.errors.is_empty(), "{:?}", facts.errors);
                let body = |name| facts.bodies.iter().find(|body| body.func == name).unwrap();
                let split = body("same_row_successor");
                let (left, right) = split.rows.iter().flat_map(|row| &row.facts).filter_map(|event| {
                    let Fact::BorrowEnd { lender, .. } = &event.fact else { return None };
                    let from = event.occurrence.at;
                    let to = event.occurrence.successor?;
                    let mir = &rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(
                        tcx, split.def_id.expect_local()).body;
                    (from == mir.terminator_loc(from.block)
                        && matches!(mir[from.block].terminator().kind, TerminatorKind::SwitchInt { .. }))
                        .then_some((lender.as_str(), SegmentPoint::Edge { from, to }))
                }).fold((None, None), |(left, right), (ended, point)| match ended {
                    "left" => (left, Some(point)),
                    "right" => (Some(point), right),
                    _ => (left, right),
                });
                // The two successors use different references. Putting
                // left's only use on the same row as the switch does not
                // free left on that successor.
                for (point, lent, free, holder) in [
                    (left.unwrap(), "left", "right", "l"),
                    (right.unwrap(), "right", "left", "r"),
                ] {
                    let state = &split.segment_points[&point];
                    let [loan] = state.lent[lent].as_slice() else { panic!("expected one loan of {lent}") };
                    assert_eq!(loan.kind, "mut");
                    assert!(!state.lent.get(free).is_some_and(|loans| !loans.is_empty()));
                    assert_eq!(state.holds[holder], [loan.loan]);
                }

                // The pair's a field was moved on one path. The other
                // path still owns it. Only the merged continuation is mixed.
                let moved = body("partial_move_edge");
                let arm_join = moved.arm_boundaries.values().find(|arms| {
                    arms.get(&0).and_then(|arm| arm.join.as_ref())
                        .is_some_and(|join| join.state.get("pair.a") == Some(&"moved"))
                    && arms.get(&1).is_some_and(|arm| arm.join.is_some())
                }).expect("the outer conditional has joins for both arms");
                let moved_arm = arm_join[&0].join.as_ref().unwrap();
                assert_eq!(moved_arm.parts["pair"].moved, ["pair.a"]);
                assert_eq!(arm_join[&1].join.as_ref().unwrap().state["pair.a"], "own");
                assert!(moved.joins.values().any(|join| join.state.state.get("pair.a") == Some(&"maybe")));
                rustc_driver::Compilation::Stop
            }
        }

        let sysroot = std::process::Command::new("rustc").args(["--print", "sysroot"])
            .output().expect("locate the pinned compiler sysroot");
        assert!(sysroot.status.success());
        let sysroot = std::str::from_utf8(&sysroot.stdout).unwrap().trim();
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/p4_owner_segments_review.rs");
        let args = ["rustc", "--sysroot", sysroot, "--edition=2024", "--crate-type=lib", "--emit=metadata",
            "-Zmir-opt-level=0", "-Zmaximal-hir-to-mir-coverage", fixture];
        rustc_driver::run_compiler(&args.map(str::to_owned), &mut Check);
    }
}
