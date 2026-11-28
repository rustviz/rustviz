//! Evidence for call operands and returned references used in tooltip text.
use super::BodyBindings;
use rustc_middle::mir::{
    Body, Local, Location, Operand, Place, ProjectionElem, Rvalue, StatementKind, TerminatorKind,
};
use rustc_middle::ty::{Ty, TyCtxt};
use rustc_span::Span;
use std::collections::BTreeSet;

/// The call's operand, before any renderer column mapping. A nested call
/// on the same row has a different instruction and argument list.
pub(crate) fn call_operand<'tcx>(
    body: &Body<'tcx>,
    span: Span,
    index: usize,
) -> Option<(Location, Place<'tcx>)> {
    let mut calls = body
        .basic_blocks
        .iter_enumerated()
        .filter_map(|(bb, data)| {
            let term = data.terminator();
            let TerminatorKind::Call { args, .. } = &term.kind else {
                return None;
            };
            if !term.source_info.span.source_equal(span) {
                return None;
            }
            Some((body.terminator_loc(bb), args.get(index)?.node.place()?))
        });
    let operand = calls.next()?;
    calls.next().is_none().then_some(operand)
}

/// The reaching borrow for an operand. Stop at joins and instructions
/// that could replace the reference. In particular, do not follow the
/// result of a call back into its arguments.
pub(crate) fn operand_borrow<'tcx>(
    body: &Body<'tcx>,
    loc: Location,
    arg: Place<'tcx>,
) -> Option<(Location, Place<'tcx>)> {
    let (at, Rvalue::Ref(_, _, place)) = operand_assignment(body, loc, arg, false)? else {
        return None;
    };
    Some((at, *place))
}

/// Follow copies and raw-pointer casts to the reaching pointer creation.
pub(crate) fn raw_pointer_origin<'tcx>(
    body: &Body<'tcx>,
    loc: Location,
    arg: Place<'tcx>,
) -> Option<(Location, Place<'tcx>)> {
    let (at, Rvalue::RawPtr(_, place)) = operand_assignment(body, loc, arg, true)? else {
        return None;
    };
    Some((at, *place))
}

fn operand_assignment<'a, 'tcx>(
    body: &'a Body<'tcx>,
    mut loc: Location,
    arg: Place<'tcx>,
    follow_pointer_casts: bool,
) -> Option<(Location, &'a Rvalue<'tcx>)> {
    let mut local = arg.as_local()?;
    let mut seen = BTreeSet::new();
    while seen.insert(loc.block) {
        let block = &body[loc.block];
        for (i, st) in block.statements[..loc.statement_index]
            .iter()
            .enumerate()
            .rev()
        {
            match &st.kind {
                StatementKind::Assign(a) if a.0.is_indirect() => return None,
                StatementKind::Assign(a) if a.0.local == local => {
                    a.0.as_local()?;
                    match a.1 {
                        Rvalue::Use(Operand::Move(p) | Operand::Copy(p)) => local = p.as_local()?,
                        Rvalue::Cast(rustc_middle::mir::CastKind::PtrToPtr, Operand::Move(p) | Operand::Copy(p), _)
                            if follow_pointer_casts => local = p.as_local()?,
                        _ => return Some((Location { block: loc.block, statement_index: i }, &a.1)),
                    }
                }
                StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if *l == local => {
                    return None
                }
                _ => {}
            }
        }
        let &[pred] = body.basic_blocks.predecessors()[loc.block].as_slice() else {
            return None;
        };
        match body[pred].terminator().kind {
            TerminatorKind::Goto { .. } | TerminatorKind::SwitchInt { .. } | TerminatorKind::Assert { .. } => {}
            TerminatorKind::Call { destination, target: Some(target), .. } if target == loc.block
                && destination.local != local && !body.basic_blocks.iter().flat_map(|b| &b.statements).any(|st|
                    matches!(&st.kind, StatementKind::Assign(a) if matches!(a.1,
                        Rvalue::Ref(_, rustc_middle::mir::BorrowKind::Mut { .. }, p) | Rvalue::RawPtr(_, p)
                        if p.local == local && !p.is_indirect()))) => {}
            _ => return None,
        }
        loc = body.terminator_loc(pred);
    }
    None
}

/// Follow a field back through direct stores and aggregate operands.
/// A control-flow join or an indirect write leaves the source unproved.
pub(super) fn projected_operand_borrow<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    mut loc: Location,
    mut place: Place<'tcx>,
    address_taken: &[Place<'tcx>],
) -> Option<Location> {
    use rustc_middle::mir::AggregateKind;

    let mut seen = BTreeSet::new();
    while seen.insert(loc.block) {
        let block = &body[loc.block];
        for (index, stmt) in block.statements[..loc.statement_index].iter().enumerate().rev() {
            match &stmt.kind {
                StatementKind::Assign(assign) if assign.0.is_indirect() => return None,
                StatementKind::Assign(assign) if assign.0.local == place.local
                    && place.projection.starts_with(assign.0.projection) =>
                {
                    let suffix = &place.projection[assign.0.projection.len()..];
                    let (source, rest) = match &assign.1 {
                        Rvalue::Ref(..) if suffix.is_empty() =>
                            return Some(Location { block: loc.block, statement_index: index }),
                        Rvalue::Use(Operand::Copy(source) | Operand::Move(source)) => (*source, suffix),
                        Rvalue::Aggregate(kind, operands) => {
                            let [ProjectionElem::Field(field, _), rest @ ..] = suffix else { return None };
                            match &**kind {
                                AggregateKind::Tuple => {}
                                AggregateKind::Adt(def, _, _, _, None) if tcx.adt_def(*def).is_struct() => {}
                                _ => return None,
                            }
                            (operands.get(*field)?.place()?, rest)
                        }
                        _ => return None,
                    };
                    if source.is_indirect() { return None; }
                    place = source.project_deeper(rest, tcx);
                    if address_taken.iter().any(|&address| super::places::place_under(place, address)) {
                        return None;
                    }
                }
                StatementKind::StorageLive(local) | StatementKind::StorageDead(local) if *local == place.local =>
                    return None,
                _ => {}
            }
        }
        let preds = super::rows::real_predecessors(body, loc.block);
        let pred = *preds.first().filter(|_| preds.len() == 1)?;
        match body[pred].terminator().kind {
            TerminatorKind::Call { destination, .. } if destination.local == place.local => return None,
            TerminatorKind::InlineAsm { .. } => return None,
            _ => {}
        }
        loc = body.terminator_loc(pred);
    }
    None
}

pub(crate) fn binding_place<'tcx>(
    body: &Body<'tcx>,
    bindings: &BodyBindings,
    place: Place<'tcx>,
) -> Option<String> {
    if place
        .projection
        .iter()
        .any(|p| matches!(p, ProjectionElem::Downcast(..)))
    {
        return None;
    }
    let root = bindings.locals.get(&place.local)?.clone();
    let mut ty = body.local_decls[place.local].ty;
    Some(super::places::name_projections(
        root,
        &mut ty,
        place.projection,
    ))
}

/// Prefer the compiler's place name. An indexed receiver can be a field
/// of an unnamed IndexMut result. The MIR borrow's own source expression
/// names it only if its unadjusted type also matches the place.
pub(crate) fn operand_label<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    bindings: &BodyBindings,
    call: Span,
    index: usize,
    operand: Span,
    expr_ty: Ty<'tcx>,
) -> Option<String> {
    let (call, arg) = call_operand(body, call, index)?;
    let (issue, place) = operand_borrow(body, call, arg)?;
    let site = body.source_info(issue).span;
    let resolved = super::body::resolve_temp_deref(tcx, body, place);
    let ty = |t| tcx.erase_and_anonymize_regions(t);
    let pointee = ty(place.ty(&body.local_decls, tcx).ty);
    if site.source_equal(operand) {
        if let Ok(source) = tcx.sess.source_map().span_to_snippet(operand) {
            if pointee == ty(expr_ty) {
                return Some(source);
            }
            if resolved.projection.iter().any(|e| {
                matches!(
                    e,
                    ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. }
                )
            }) && resolved.projection.last() == Some(&ProjectionElem::Deref)
                && matches!(expr_ty.kind(), rustc_middle::ty::Ref(_, inner, _) if ty(*inner) == pointee)
            {
                return Some(format!("*{source}"));
            }
        }
    }
    binding_place(body, bindings, resolved)
}

/// The expansion can borrow an operand twice, once for the formatting
/// tuple and once through the tuple. Both denote the user's expression.
/// Only a direct borrow of the resolved HIR binding borrows its slot.
pub(crate) fn macro_operand<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    borrow: Span,
    operand: Span,
    ty: Ty<'tcx>,
    binding: Option<Local>,
) -> Option<bool> {
    let ty = tcx.erase_and_anonymize_regions(ty);
    let mut slot = None;
    for st in body.basic_blocks.iter().flat_map(|bb| &bb.statements) {
        if st.source_info.span != borrow && !st.source_info.span.source_equal(operand) {
            continue;
        }
        let StatementKind::Assign(a) = &st.kind else {
            continue;
        };
        let Rvalue::Ref(_, rustc_middle::mir::BorrowKind::Shared, p) = a.1 else {
            continue;
        };
        if tcx.erase_and_anonymize_regions(p.ty(&body.local_decls, tcx).ty) == ty {
            *slot.get_or_insert(false) |=
                ty.is_ref() && binding.is_some() && p.as_local() == binding;
        }
    }
    slot
}

/// A pattern reference bound directly to the payload of a single-field
/// enum variant, including through the shared temporary in `match &opt`.
pub(crate) fn payload_borrow<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    local: Local,
    bindings: &BodyBindings,
) -> Option<(String, &'static str)> {
    let mut defs = body
        .basic_blocks
        .iter()
        .flat_map(|bb| &bb.statements)
        .filter_map(|st| {
            let StatementKind::Assign(a) = &st.kind else {
                return None;
            };
            (a.0.as_local() == Some(local)).then_some(&a.1)
        });
    let Rvalue::Ref(_, kind, place) = *defs.next()? else {
        return None;
    };
    if defs.next().is_some() {
        return None;
    }
    let p = super::body::resolve_temp_deref(tcx, body, place);
    let [prefix @ .., ProjectionElem::Downcast(_, variant), ProjectionElem::Field(_, _)] =
        p.projection.as_slice()
    else {
        return None;
    };
    let container = Place {
        local: p.local,
        projection: tcx.mk_place_elems(prefix),
    };
    let adt = container.ty(&body.local_decls, tcx).ty.ty_adt_def()?;
    if !adt.is_enum() || adt.variant(*variant).fields.len() != 1 {
        return None;
    }
    let mut ty = body.local_decls[p.local].ty;
    let parent =
        super::places::name_projections(bindings.locals.get(&p.local)?.clone(), &mut ty, prefix);
    let kind = match kind {
        rustc_middle::mir::BorrowKind::Shared => "shared",
        rustc_middle::mir::BorrowKind::Mut { .. } => "mutable",
        _ => return None,
    };
    Some((parent, kind))
}
