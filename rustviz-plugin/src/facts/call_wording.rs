//! Evidence for call operands and returned references used in tooltip text.
use super::BodyBindings;
use rustc_middle::mir::{
    Body, Local, Location, Operand, Place, ProjectionElem, Rvalue, StatementKind, TerminatorKind,
    RETURN_PLACE, START_BLOCK,
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

/// Follow the return place backwards over normal MIR edges. A mutable
/// reference result can be reborrowed through several temporaries *after*
/// the arms join. Select an arm by an instruction contributing to the
/// returned value, even when both arms occupy the same source row.
pub(crate) fn return_origins<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    span: Option<Span>,
) -> Option<BTreeSet<Option<Local>>> {
    let mut work: Vec<_> = body
        .basic_blocks
        .iter_enumerated()
        .filter(|(_, bb)| matches!(bb.terminator().kind, TerminatorKind::Return))
        .map(|(bb, _)| (body.terminator_loc(bb), RETURN_PLACE, span.is_none()))
        .collect();
    // An indirect write could replace the reference in an address-taken slot.
    let exposed: BTreeSet<_> = body
        .basic_blocks
        .iter()
        .flat_map(|b| &b.statements)
        .filter_map(|st| {
            let StatementKind::Assign(a) = &st.kind else {
                return None;
            };
            match a.1 {
                Rvalue::Ref(_, rustc_middle::mir::BorrowKind::Mut { .. }, p)
                | Rvalue::RawPtr(_, p)
                    if !p.is_indirect() =>
                {
                    Some(p.local)
                }
                _ => None,
            }
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut origins = BTreeSet::new();
    'trace: while let Some((at, mut local, mut selected)) = work.pop() {
        if !seen.insert((at, local, selected)) {
            continue;
        }
        for st in body[at.block].statements[..at.statement_index].iter().rev() {
            let StatementKind::Assign(a) = &st.kind else {
                continue;
            };
            if a.0.local != local || a.0.is_indirect() {
                continue;
            }
            a.0.as_local()?;
            selected |= span.is_some_and(|s| st.source_info.span.source_equal(s));
            local = match a.1 {
                Rvalue::Use(Operand::Constant(_)) => {
                    if selected {
                        origins.insert(None);
                    }
                    continue 'trace;
                }
                Rvalue::Use(Operand::Copy(p) | Operand::Move(p)) => p.as_local()?,
                Rvalue::Ref(_, _, p)
                    if p.projection.first() == Some(&ProjectionElem::Deref)
                        && p.projection[1..].iter().all(|e| {
                            matches!(e, ProjectionElem::Field(..) | ProjectionElem::Downcast(..))
                        }) =>
                {
                    p.local
                }
                _ => return None,
            };
            if exposed.contains(&local) {
                return None;
            }
        }
        if at.block == START_BLOCK {
            if selected {
                origins.insert(Some(body.args_iter().find(|p| *p == local)?));
            }
            continue;
        }
        for &pred in body.basic_blocks.predecessors()[at.block].iter() {
            if !super::real_successor_blocks(body, pred).contains(&at.block) {
                continue;
            }
            let term = body[pred].terminator();
            let mut src = local;
            let mut through = selected;
            if let TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } = &term.kind
            {
                if destination.local == local {
                    // The standard String-to-str deref borrows the String's contents.
                    let (method, generics) = func.const_fn_def()?;
                    let self_ty = generics.types().next()?;
                    if !tcx
                        .trait_of_assoc(method)
                        .is_some_and(|d| tcx.is_lang_item(d, rustc_hir::LangItem::Deref))
                        || !self_ty
                            .ty_adt_def()
                            .is_some_and(|d| tcx.is_lang_item(d.did(), rustc_hir::LangItem::String))
                    {
                        return None;
                    }
                    let [arg] = &args[..] else { return None };
                    src = arg.node.place()?.as_local()?;
                    through |= span.is_some_and(|s| term.source_info.span.source_equal(s));
                }
            }
            work.push((body.terminator_loc(pred), src, through));
        }
    }
    (!origins.is_empty()).then_some(origins)
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

/// A guard can construct the same closure on several MIR paths. Keep the
/// borrow of this environment field at *each* construction site.
pub(crate) fn capture_borrows<'tcx>(
    body: &Body<'tcx>,
    closure: rustc_hir::def_id::LocalDefId,
    index: usize,
) -> Option<Vec<(Location, Place<'tcx>)>> {
    let mut sites = Vec::new();
    for (block, bb) in body.basic_blocks.iter_enumerated() {
        for (statement_index, statement) in bb.statements.iter().enumerate() {
            let StatementKind::Assign(a) = &statement.kind else {
                continue;
            };
            let Rvalue::Aggregate(kind, fields) = &a.1 else {
                continue;
            };
            if !matches!(**kind, rustc_middle::mir::AggregateKind::Closure(id, _) if id == closure.to_def_id())
            {
                continue;
            }
            let field = fields
                .get(rustc_abi::FieldIdx::from_usize(index))?
                .place()?;
            sites.push(operand_borrow(
                body,
                Location {
                    block,
                    statement_index,
                },
                field,
            )?);
        }
    }
    Some(sites)
}
