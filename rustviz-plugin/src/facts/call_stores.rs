//! An instantiated lifetime can carry a loan past a call even when the
//! callee never stores the supplied reference. Trace MIR writes separately.
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{Body, Local, Operand, Place, ProjectionElem, Rvalue, StatementKind, TerminatorKind, START_BLOCK};
use rustc_middle::ty::{TyCtxt, TyKind};
use rustc_span::Symbol;
use std::collections::BTreeSet;

use super::rows::real_successor_blocks;

/// A source argument whose value may be written behind a destination argument.
pub(super) fn argument_stores(tcx: TyCtxt<'_>, def: DefId) -> BTreeSet<(usize, usize)> {
    stores(tcx, def, &mut std::collections::HashSet::new())
}

fn stores<'tcx>(tcx: TyCtxt<'tcx>, def: DefId, active: &mut std::collections::HashSet<DefId>) -> BTreeSet<(usize, usize)> {
    if tcx.is_diagnostic_item(Symbol::intern("mem_replace"), def) {
        return BTreeSet::from([(0, 1)]);
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
        (m.is_mut() && (rest.is_empty() || !pointee.is_ref())
            && (1..=body.arg_count).contains(&dest.local.index()) && !written.contains(&dest.local))
            .then(|| dest.local.index() - 1)
    };
    let mut result = BTreeSet::new();
    for &bb in &seen {
        let data = &body.basic_blocks[bb];
        for stmt in &data.statements {
            if let StatementKind::Assign(a) = &stmt.kind {
                if let Some(dest) = target(a.0) {
                    result.extend(value_places(tcx, body, &a.1).into_iter().filter_map(source).map(|src| (dest, src)));
                }
            }
        }
        if let TerminatorKind::Call { func, args, target: Some(_), .. } = &data.terminator().kind {
            let TyKind::FnDef(callee, _) = *func.ty(&body.local_decls, tcx).kind() else { continue };
            let pairs = if callee.is_local() || tcx.is_diagnostic_item(Symbol::intern("mem_replace"), callee) {
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
                                pairs.insert((dst, src));
                            }
                        }
                    }
                }
                pairs
            };
            for (dst, src) in pairs {
                let operand = |index: usize| args.get(index).and_then(|a| a.node.place());
                if let (Some(dest), Some(src)) = (operand(dst).and_then(|p| target(tcx.mk_place_deref(p))), operand(src).and_then(source)) {
                    result.insert((dest, src));
                }
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
