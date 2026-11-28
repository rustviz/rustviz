//! An instantiated lifetime can carry a loan past a call even when the
//! callee never stores the supplied reference. Trace MIR writes separately.
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{BasicBlock, Body, Local, Location, Operand, Place, ProjectionElem, Rvalue, StatementKind, TerminatorKind, START_BLOCK};
use rustc_middle::ty::{TyCtxt, TyKind};
use rustc_span::Symbol;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::rows::real_successor_blocks;
use super::call_relationships::{Projection, project, projection};

/// An available, straight-line body can establish that the receiver
/// is untouched even when its lifetimes accept the argument's loan.
pub(super) fn no_indirect_writes(tcx: TyCtxt<'_>, def: DefId) -> bool {
    let Some(local) = def.as_local() else { return false };
    if !tcx.is_mir_available(def) || tcx.trait_of_assoc(def).is_some() { return false; }
    let body = &rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, local).body;
    let block = &body[START_BLOCK];
    matches!(block.terminator().kind, TerminatorKind::Return) && block.statements.iter().all(|st| match &st.kind {
        StatementKind::Assign(a) => !a.0.is_indirect(),
        StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::FakeRead(_) | StatementKind::Nop => true,
        _ => false,
    })
}

/// The formal receiver type shares a lifetime or reference-carrying
/// parameter with these inputs. A receiver which already holds a loan
/// needs this independent evidence before the call can be said to keep it.
pub(super) fn receiver_ties<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    callee: Option<(DefId, rustc_middle::ty::GenericArgsRef<'tcx>)>,
) -> BTreeSet<usize> {
    use rustc_middle::ty::GenericArgKind;
    let Some((def, actual)) = callee else { return BTreeSet::new() };
    let (def, actual) = rustc_middle::ty::Instance::try_resolve(tcx, body.typing_env(tcx), def, actual)
        .ok().flatten().filter(|instance| matches!(instance.def, rustc_middle::ty::InstanceKind::Item(_))
            && matches!(tcx.def_kind(instance.def_id()), rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn))
        .map_or((def, actual), |instance| (instance.def_id(), instance.args));
    let sig = tcx.fn_sig(def).instantiate_identity().skip_norm_wip();
    let sig = tcx.liberate_late_bound_regions(def, sig);
    let Some(TyKind::Ref(_, receiver, m)) = sig.inputs().first().map(|ty| ty.kind()) else { return BTreeSet::new() };
    if !m.is_mut() { return BTreeSet::new(); }
    let mut regions = HashSet::new();
    let mut types = BTreeSet::new();
    for arg in receiver.walk() {
        match arg.kind() {
            GenericArgKind::Lifetime(region) if !region.is_static() => { regions.insert(region); }
            GenericArgKind::Type(ty) => if let TyKind::Param(index) = ty.kind() {
                if actual.get(index.index as usize).is_some_and(|arg| arg.walk().any(|arg| matches!(arg.kind(), GenericArgKind::Lifetime(_)))) {
                    types.insert(index.index);
                }
            },
            _ => {}
        }
    }
    loop {
        let before = regions.len();
        for (clause, _) in tcx.predicates_of(def).predicates.iter() {
            if let Some(bound) = clause.as_region_outlives_clause() {
                let rustc_middle::ty::OutlivesPredicate(from, to) = bound.skip_binder();
                if !from.is_static() && regions.contains(&to) { regions.insert(from); }
            }
        }
        if regions.len() == before { break; }
    }
    sig.inputs().iter().enumerate().skip(1).filter(|(_, input)| input.walk().any(|arg| match arg.kind() {
        GenericArgKind::Lifetime(region) => regions.contains(&region),
        GenericArgKind::Type(ty) => matches!(ty.kind(), TyKind::Param(index) if types.contains(&index.index)),
        _ => false,
    })).map(|(index, _)| index).collect()
}

/// A source argument whose value may be written behind a destination argument.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Store {
    pub destination: usize,
    pub projection: Vec<Projection>,
    pub source: usize,
    pub through_source: Option<Vec<Projection>>,
    /// This effect replaces the complete destination on every normal return.
    pub replaces: bool,
}

pub(super) fn argument_stores(tcx: TyCtxt<'_>, def: DefId) -> BTreeSet<Store> {
    stores(tcx, def, &mut std::collections::HashSet::new())
}

fn stores<'tcx>(tcx: TyCtxt<'tcx>, def: DefId, active: &mut std::collections::HashSet<DefId>) -> BTreeSet<Store> {
    if tcx.is_diagnostic_item(Symbol::intern("mem_replace"), def) {
        return BTreeSet::from([Store { destination: 0, projection: Vec::new(), source: 1, through_source: None, replaces: true }]);
    }
    if tcx.is_diagnostic_item(Symbol::intern("mem_swap"), def) {
        return BTreeSet::from([
            Store { destination: 0, projection: Vec::new(), source: 1, through_source: Some(Vec::new()), replaces: true },
            Store { destination: 1, projection: Vec::new(), source: 0, through_source: Some(Vec::new()), replaces: true },
        ]);
    }
    let Some(local) = def.as_local() else { return BTreeSet::new() };
    if !tcx.is_mir_available(def) || !active.insert(def) {
        return BTreeSet::new();
    }
    let body = &rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, local).body;
    let mut seen = BTreeSet::new();
    let mut work = vec![START_BLOCK];
    while let Some(bb) = work.pop() {
        if seen.insert(bb) {
            work.extend(real_successor_blocks(body, bb));
        }
    }

    // Only follow direct locals with one definition. Indirect writes and
    // projected reads carry more information than this summary tracks.
    let mut defs = vec![None; body.local_decls.len()];
    let mut written = BTreeSet::new();
    let mut record = |dest: Place<'tcx>, rhs: Option<&'tcx Rvalue<'tcx>>| {
        if !dest.is_indirect() {
            let first = written.insert(dest.local);
            defs[dest.local.index()] = rhs.filter(|_| first && dest.projection.is_empty());
        }
    };
    for &bb in &seen {
        let data = &body.basic_blocks[bb];
        for stmt in &data.statements {
            if let StatementKind::Assign(a) = &stmt.kind {
                record(a.0, Some(&a.1));
            }
        }
        if let TerminatorKind::Call { destination, .. } = data.terminator().kind {
            record(destination, None);
        }
    }

    let source = |p| source_parameter(tcx, body, &defs, &written, p);
    let target = |p| {
        let dest = super::body::resolve_write_deref(tcx, body, p);
        let [ProjectionElem::Deref, rest @ ..] = dest.projection.as_slice() else { return None };
        let ty = body.local_decls[dest.local].ty;
        let TyKind::Ref(_, pointee, m) = ty.kind() else { return None };
        if !m.is_mut() || !rest.is_empty() && pointee.is_ref() || rest.contains(&ProjectionElem::Deref)
            || !(1..=body.arg_count).contains(&dest.local.index()) || written.contains(&dest.local) {
            return None;
        }
        Some((dest.local.index() - 1, projection(rest)?))
    };
    // A single straight-line write of a parameter's value is a strong
    // update. Otherwise this summary only establishes possible writes.
    let one_write = seen.len() == 1 && matches!(body[START_BLOCK].terminator().kind, TerminatorKind::Return)
        && body[START_BLOCK].statements.iter().filter(|st| matches!(&st.kind,
            StatementKind::Assign(a) if a.0.is_indirect())).count() == 1
        && body[START_BLOCK].statements.iter().all(|st| !matches!(&st.kind,
            StatementKind::Assign(a) if matches!(a.1, Rvalue::RawPtr(..))));
    let mut result = BTreeSet::new();
    for &bb in &seen {
        let data = &body.basic_blocks[bb];
        for stmt in &data.statements {
            if let StatementKind::Assign(a) = &stmt.kind {
                if let Some((destination, projection)) = target(a.0) {
                    let dest_ty = tcx.erase_and_anonymize_regions(a.0.ty(&body.local_decls, tcx).ty);
                    result.extend(value_places(tcx, body, &a.1).into_iter().filter_map(source)
                        .map(|src| {
                            let param_ty = tcx.erase_and_anonymize_regions(body.local_decls[Local::from_usize(src + 1)].ty);
                            let replaces = one_write && matches!(&a.1, Rvalue::Use(_)) && param_ty == dest_ty;
                            Store { destination, projection: projection.clone(), source: src, through_source: None, replaces }
                        }));
                }
            }
        }
        if let TerminatorKind::Call { func, args, target: Some(_), .. } = &data.terminator().kind {
            let TyKind::FnDef(callee, _) = *func.ty(&body.local_decls, tcx).kind() else { continue };
            let pairs = if callee.is_local() || ["mem_replace", "mem_swap"].into_iter()
                .any(|item| tcx.is_diagnostic_item(Symbol::intern(item), callee)) {
                stores(tcx, callee, active)
            } else {
                // For an opaque sink into a container, MIR must pass both
                // values to it on this path. This supports the borrow-checker
                // wording for e.g. Vec::push in a wrapper. A slot arrow always
                // requires a write or a summarized store through that slot.
                let mut pairs = BTreeSet::new();
                let sig = func.ty(&body.local_decls, tcx).fn_sig(tcx).skip_binder();
                if sig.output().is_unit() {
                    for (dst, ty) in sig.inputs().iter().enumerate() {
                        let TyKind::Ref(_, inner, m) = ty.kind() else { continue };
                        if !m.is_mut() || !inner.is_adt() { continue; }
                        for (src, input) in sig.inputs().iter().enumerate() {
                            if dst != src && inner.walk().filter_map(|arg| arg.as_type()).any(|ty|
                                tcx.erase_and_anonymize_regions(ty) == tcx.erase_and_anonymize_regions(*input))
                            {
                                pairs.insert(Store { destination: dst, projection: Vec::new(), source: src, through_source: None, replaces: false });
                            }
                        }
                    }
                }
                pairs
            };
            for Store { destination, projection, source: argument, through_source, .. } in pairs {
                let operand = |index: usize| args.get(index).and_then(|a| a.node.place());
                let Some((destination, projection)) = operand(destination)
                    .and_then(|p| project(tcx, body, tcx.mk_place_deref(p), &projection)).and_then(target) else { continue };
                let Some(arg) = operand(argument) else { continue };
                let src = match through_source {
                    Some(path) => project(tcx, body, tcx.mk_place_deref(arg), &path)
                        .and_then(target).map(|(index, path)| (index, Some(path))),
                    None => source(arg).map(|index| (index, None)),
                };
                let Some((source, through_source)) = src else { continue };
                result.insert(Store { destination, projection, source, through_source, replaces: false });
            }
        }
    }
    active.remove(&def);
    result
}

fn value_places<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, value: &Rvalue<'tcx>) -> Vec<Place<'tcx>> {
    match value {
        Rvalue::Use(o) | Rvalue::Cast(_, o, _) => o.place().into_iter().collect(),
        Rvalue::Ref(_, _, place) => {
            let place = super::body::resolve_write_deref(tcx, body, *place);
            if place.projection.as_slice() == [ProjectionElem::Deref] {
                vec![Place::from(place.local)]
            } else {
                Vec::new()
            }
        }
        Rvalue::Aggregate(_, ops) => ops.iter().filter_map(Operand::place).collect(),
        _ => Vec::new(),
    }
}

fn source_parameter<'tcx>(
    tcx: TyCtxt<'tcx>, body: &Body<'tcx>, defs: &[Option<&Rvalue<'tcx>>], written: &BTreeSet<Local>, place: Place<'tcx>,
) -> Option<usize> {
    let mut current = place;
    let mut seen = BTreeSet::new();
    loop {
        if current.is_indirect() {
            current = super::body::resolve_write_deref(tcx, body, current);
        }
        if !current.projection.is_empty() || !seen.insert(current.local) { return None; }
        let index = current.local.index();
        if index > 0 && index <= body.arg_count && !written.contains(&current.local) {
            return Some(index - 1);
        }
        let sources = value_places(tcx, body, defs[index]?);
        let [source] = sources.as_slice() else { return None };
        current = *source;
    }
}

/// Trace a concrete reference back through the caller's assignments and
/// strong call writes. Borrowck origins can include a loan from a value
/// that an earlier call replaced. Keep that loan as a constraint, but
/// never use it to identify the reference stored by this call.
pub(super) fn reaching_issue<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    runs: &BTreeSet<BasicBlock>,
    mut at: Location,
    mut place: Place<'tcx>,
    stores: &BTreeMap<Location, Vec<(Place<'tcx>, Place<'tcx>)>>,
) -> Option<Location> {
    let mut seen = HashSet::new();
    let overlaps = |a: Place<'tcx>, b: Place<'tcx>| {
        a.local == b.local && !a.projection.iter().zip(b.projection.iter()).any(|(a, b)| match (a, b) {
            (ProjectionElem::Field(a, _), ProjectionElem::Field(b, _)) => a != b,
            (ProjectionElem::Downcast(_, a), ProjectionElem::Downcast(_, b)) => a != b,
            _ => false,
        })
    };
    while seen.insert((at, place)) && !place.is_indirect() {
        let block = &body[at.block];
        for (index, stmt) in block.statements[..at.statement_index].iter().enumerate().rev() {
            let StatementKind::Assign(a) = &stmt.kind else {
                if matches!(stmt.kind, StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if l == place.local) {
                    return None;
                }
                continue;
            };
            let destination = super::body::resolve_write_deref(tcx, body, a.0);
            if destination.is_indirect() { return None; }
            if !overlaps(place, destination) { continue; }
            if destination.local != place.local || destination.projection.len() > place.projection.len() {
                return None;
            }
            let suffix = &place.projection[destination.projection.len()..];
            match &a.1 {
                Rvalue::Use(operand) => place = operand.place()?.project_deeper(suffix, tcx),
                Rvalue::Ref(_, _, borrowed) if suffix.is_empty() => {
                    if borrowed.projection.as_slice() == [ProjectionElem::Deref] && body.local_decls[borrowed.local].ty.is_ref() {
                        place = Place::from(borrowed.local);
                    } else {
                        return Some(Location { statement_index: index, ..at });
                    }
                }
                _ => return None,
            }
        }
        let mut predecessors = body.basic_blocks.predecessors()[at.block].iter().copied()
            .filter(|&bb| runs.contains(&bb) && real_successor_blocks(body, bb).contains(&at.block));
        let prev = predecessors.next()?;
        if predecessors.next().is_some() { return None; }
        at = body.terminator_loc(prev);
        if let TerminatorKind::Call { destination, args, .. } = &body[prev].terminator().kind {
            if overlaps(place, *destination) { return None; }
            let path = projection(&place.projection)?;
            if let Some((_, source)) = stores.get(&at).into_iter().flatten().find(|(dest, _)| {
                dest.local == place.local && projection(&dest.projection).as_ref() == Some(&path)
            }) {
                place = *source;
            } else if args.iter().any(|arg| arg.node.place().is_some_and(|arg| {
                if !arg.ty(&body.local_decls, tcx).ty.is_mutable_ptr() { return false; }
                let written = super::body::resolve_write_deref(tcx, body, tcx.mk_place_deref(arg));
                written.is_indirect() || overlaps(place, written)
            })) {
                return None;
            }
        }
    }
    None
}
