//! The two calls in a lowered `for` loop communicate through a storage
//! local. Only the Some payload of the second call initializes an item.
//! In particular, an IntoIterator implementation can consume one type and
//! manufacture entirely unrelated values.

use super::schema::{EndPlace, IteratorBinding, IteratorHead, IteratorInput, IteratorStorage};
use rustc_hir::LangItem;
use rustc_middle::mir::{
    Body, BorrowKind, Location, Operand, Place, ProjectionElem, Rvalue, StatementKind,
    TerminatorKind,
};
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::{DesugaringKind, Symbol};
use std::collections::{BTreeMap, BTreeSet};

/// Read the lowering's construction and next calls. Join them on the
/// receiver's storage, even if several loops share their source row.
pub(super) fn collect<'tcx>(
    tcx: TyCtxt<'tcx>, body: &Body<'tcx>,
    name: impl Fn(Place<'tcx>) -> Option<String>,
) -> Vec<IteratorStorage> {
    let mut constructors = BTreeMap::new();
    let mut next_calls = Vec::new();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        let term = data.terminator();
        if term.source_info.span.desugaring_kind() != Some(DesugaringKind::ForLoop) { continue; }
        let TerminatorKind::Call { func, args, destination, target: Some(target), .. } = &term.kind else { continue };
        let Some((def, types)) = func.const_fn_def() else { continue };
        let [arg] = args.as_ref() else { continue };
        let Some(result) = destination.as_local() else { continue };
        let at = body.terminator_loc(bb);
        if tcx.is_lang_item(def, LangItem::IntoIterIntoIter) {
            // The match binding takes the constructor's result before the
            // loop header. It has its own storage and drop scope.
            for st in &body[*target].statements {
                let StatementKind::Assign(a) = &st.kind else { continue };
                if matches!(a.1, Rvalue::Use(Operand::Move(p) | Operand::Copy(p)) if p.as_local() == Some(result)) {
                    if let Some(storage) = a.0.as_local() {
                        constructors.insert(storage, (at, &arg.node, types.type_at(0)));
                    }
                    break;
                }
            }
        } else if tcx.is_lang_item(def, LangItem::IteratorNext) {
            let Some(receiver) = arg.node.place() else { continue };
            let borrowed = super::body::resolve_temp_deref(tcx, body, tcx.mk_place_deref(receiver));
            if let Some(storage) = borrowed.as_local() {
                next_calls.push((storage, result, at, types.type_at(0)));
            }
        }
    }

    let mut loops = Vec::new();
    for (storage, result, next, iter_ty) in next_calls {
        let Some(&(constructor, arg, input_ty)) = constructors.get(&storage) else { continue };
        let mut producer = None;
        let head = head_input(tcx, body, constructor, arg, &name, &mut producer);
        // The blanket impl for every Iterator returns self. A Vec, array
        // or slice has a library-defined element mapping. An arbitrary
        // IntoIterator impl guarantees neither relationship.
        let identity = tcx.erase_and_anonymize_regions(input_ty) == tcx.erase_and_anonymize_regions(iter_ty);
        let slice_traversal = producer.is_some_and(|bb| {
            let TerminatorKind::Call { func, .. } = &body[bb].terminator().kind else { return false };
            let Some((def, _)) = func.const_fn_def() else { return false };
            tcx.is_diagnostic_item(Symbol::intern("slice_iter"), def)
                || (tcx.item_name(def).as_str() == "iter_mut" && tcx.impl_of_assoc(def).is_some_and(|imp|
                    tcx.impl_opt_trait_ref(imp).is_none() && matches!(tcx.type_of(imp).skip_binder().kind(), ty::Slice(_))))
        });
        let collection_input = producer.and_then(|bb| {
            let TerminatorKind::Call { func, .. } = &body[bb].terminator().kind else { return None };
            let (def, types) = func.const_fn_def()?;
            if !tcx.is_lang_item(def, LangItem::IntoIterIntoIter) { return None; }
            let ty = types.type_at(0);
            known_collection(tcx, ty).then_some(ty)
        });
        let direct_items = slice_traversal || collection_input.is_some();
        let consumed_collection = producer.filter(|_| collection_input.is_some()).and_then(|bb| {
            let TerminatorKind::Call { args, .. } = &body[bb].terminator().kind else { return None };
            let input = head_input(tcx, body, body.terminator_loc(bb), &args.first()?.node, &name, &mut None)?;
            (input.access == IteratorInput::Move).then_some(input.place)
        });
        let collection = known_collection(tcx, input_ty);
        let slice_iter = iter_ty.ty_adt_def().is_some_and(|def| tcx.is_diagnostic_item(Symbol::intern("SliceIter"), def.did()));
        let mut bindings = Vec::new();
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            for (statement_index, st) in data.statements.iter().enumerate() {
                let StatementKind::Assign(a) = &st.kind else { continue };
                let Some(local) = a.0.as_local() else { continue };
                let (src, access) = match a.1 {
                    Rvalue::Use(Operand::Move(p)) => (p, IteratorInput::Move),
                    Rvalue::Use(Operand::Copy(p)) => (p, IteratorInput::Copy),
                    Rvalue::Ref(_, BorrowKind::Shared, p) => (p, IteratorInput::Shared),
                    Rvalue::Ref(_, BorrowKind::Mut { .. }, p) => (p, IteratorInput::Mutable),
                    _ => continue,
                };
                let [ProjectionElem::Downcast(_, variant), ProjectionElem::Field(field, item_ty), rest @ ..] = src.projection.as_slice() else { continue };
                if src.local != result || field.index() != 0 || name(a.0).is_none() { continue; }
                let Some(option) = body.local_decls[result].ty.ty_adt_def() else { continue };
                if !tcx.is_lang_item(option.variant(*variant).def_id, LangItem::OptionSome) { continue; }
                let derefs = rest.iter().filter(|p| matches!(p, ProjectionElem::Deref)).count();
                let part = rest.iter().all(|p| matches!(p, ProjectionElem::Deref | ProjectionElem::Field(..)
                    | ProjectionElem::ConstantIndex { from_end: false, .. })).then(|| {
                    let mut ty = *item_ty;
                    let mut path = String::new();
                    for p in rest {
                        if let ProjectionElem::ConstantIndex { offset, from_end: false, .. } = p {
                            path.push_str(&format!("[{offset}]"));
                        } else if !matches!(p, ProjectionElem::Deref) {
                            path = super::places::name_projections(path, &mut ty, std::slice::from_ref(p));
                            continue;
                        }
                        ty = super::places::project_ty(ty, p);
                    }
                    path
                });
                bindings.push(IteratorBinding {
                    local, at: Location { block: bb, statement_index }, access,
                    part, derefs, loans: BTreeSet::new(), consumed: None,
                    handoffs: Vec::new(),
                });
            }
        }
        loops.push(IteratorStorage {
            constructor, site: body.source_info(constructor).span, storage, next, result, head,
            head_loan_retained: false,
            borrows_collection: (collection && input_ty.is_ref()) || slice_traversal || collection_input.is_some_and(|ty| ty.is_ref()),
            moves_references: collection && !input_ty.is_ref() && bindings.iter().any(|b|
                b.access == IteratorInput::Move && body.local_decls[b.local].ty.ref_mutability().is_some_and(|m| m.is_mut())),
            identity, collection, slice_iter, direct_items, consumed_collection, arguments: Vec::new(), opaque_item_loans: BTreeSet::new(), bindings, drops: Vec::new(), loans: BTreeSet::new(),
        });
    }
    loops
}

fn known_collection<'tcx>(tcx: TyCtxt<'tcx>, ty: ty::Ty<'tcx>) -> bool {
    match ty.peel_refs().kind() {
        ty::Array(..) | ty::Slice(..) => true,
        ty::Adt(def, _) => tcx.is_diagnostic_item(Symbol::intern("Vec"), def.did())
            || tcx.is_lang_item(def.did(), LangItem::Option)
            || (ty.is_ref() && tcx.is_diagnostic_item(Symbol::intern("Result"), def.did())),
        _ => false,
    }
}

/// A signature can tie a returned reference to an input even when the
/// implementation yields a different referent. Limit write attribution
/// through adapters to resolved standard operations that preserve items.
pub(super) fn preserves_items<'tcx>(
    tcx: TyCtxt<'tcx>, env: ty::TypingEnv<'tcx>, callee: rustc_hir::def_id::DefId, args: ty::GenericArgsRef<'tcx>,
) -> bool {
    let name = tcx.item_name(callee);
    // The standard vec! list constructor transfers the boxed array elements
    // into the Vec. Unlike vec![value; n], it does not call Clone.
    if tcx.is_diagnostic_item(Symbol::intern("box_assume_init_into_vec_unsafe"), callee)
        || tcx.is_diagnostic_item(Symbol::intern("slice_iter"), callee) { return true; }
    if let Some(imp) = tcx.impl_of_assoc(callee) {
        if tcx.impl_opt_trait_ref(imp).is_none() && matches!(tcx.type_of(imp).skip_binder().kind(), ty::Slice(_))
            && name.as_str() == "iter_mut" { return true; }
    }
    let Some(tr) = tcx.trait_of_assoc(callee) else {
        // The free function has the same item mapping as Iterator::zip.
        return tcx.crate_name(callee.krate) == rustc_span::sym::core
            && tcx.def_path(callee).to_string_no_crate_verbose() == "::iter::adapters::zip::zip";
    };
    if tcx.is_lang_item(tr, LangItem::Index) || tcx.is_lang_item(tr, LangItem::IndexMut) {
        // A range of a standard slice or Vec borrows its elements. An
        // overloaded Index implementation can return unrelated storage.
        return known_collection(tcx, args.type_at(0)) && ty::Instance::try_resolve(tcx, env, callee, args)
            .ok().flatten().is_some_and(|imp| matches!(tcx.crate_name(imp.def_id().krate),
                rustc_span::sym::core | rustc_span::sym::alloc));
    }
    if tcx.is_lang_item(tr, LangItem::Deref) || tcx.is_lang_item(tr, LangItem::DerefMut) {
        // A user-defined Deref can return unrelated storage. Vec's
        // implementation exposes the slice of its own elements.
        return args.type_at(0).peel_refs().ty_adt_def().is_some_and(|def|
            tcx.is_diagnostic_item(Symbol::intern("Vec"), def.did()));
    }
    if tcx.is_lang_item(callee, LangItem::IntoIterIntoIter) {
        return known_collection(tcx, args.type_at(0));
    }
    tcx.is_diagnostic_item(Symbol::intern("Iterator"), tr) && matches!(name.as_str(),
            "chain" | "zip" | "enumerate" | "rev" | "skip" | "take" | "step_by" | "skip_while"
            | "take_while" | "filter" | "peekable" | "fuse" | "inspect")
}

/// The source of the operand of IntoIterator, through staging temporaries
/// only. A call result or a merge has no direct, named input.
pub(super) fn head_input<'tcx>(
    tcx: TyCtxt<'tcx>, body: &Body<'tcx>, mut at: Location, arg: &Operand<'tcx>,
    name: impl Fn(Place<'tcx>) -> Option<String>, producer: &mut Option<rustc_middle::mir::BasicBlock>,
) -> Option<IteratorHead> {
    let mut arg = arg;
    let mut seen = BTreeSet::new();
    loop {
        let place = arg.place()?;
        if let Some(n) = name(place) {
            let access = if matches!(arg, Operand::Copy(_)) { IteratorInput::Copy } else { IteratorInput::Move };
            return Some(IteratorHead { place: EndPlace::new(tcx, place, n), access, borrow_at: None });
        }
        let local = place.as_local()?;
        if !seen.insert((at, local)) { return None; }
        let block = &body[at.block];
        let assignment = block.statements[..at.statement_index].iter().enumerate().rev().find_map(|(i, st)| match &st.kind {
            StatementKind::Assign(a) if a.0.local == local => Some((i, Some(&**a))),
            StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if *l == local => Some((i, None)),
            _ => None,
        });
        match assignment {
            Some((i, Some((dst, rv)))) if *dst == place => match rv {
                Rvalue::Use(op) => { at.statement_index = i; arg = op; }
                Rvalue::Ref(_, kind, borrowed) => {
                    let place = super::body::resolve_temp_deref(tcx, body, *borrowed);
                    let access = match kind {
                        BorrowKind::Shared => IteratorInput::Shared,
                        BorrowKind::Mut { .. } => IteratorInput::Mutable,
                        BorrowKind::Fake(_) => return None,
                    };
                    return Some(IteratorHead { place: EndPlace::new(tcx, place, name(place)?), access, borrow_at: Some(Location { statement_index: i, ..at }) });
                }
                _ => return None,
            },
            None => {
                let &[bb] = body.basic_blocks.predecessors()[at.block].as_slice() else { return None };
                match body[bb].terminator().kind {
                    TerminatorKind::Call { destination, target: Some(target), .. }
                        if destination == place && target == at.block => {
                        *producer = Some(bb);
                        return None;
                    }
                    TerminatorKind::Goto { .. } => {}
                    _ => return None,
                }
                at = body.terminator_loc(bb);
            }
            _ => return None,
        }
    }
}

/// A move into a call consumes the binding at the argument evaluation.
/// Follow only whole-local staging moves, and require a single consuming
/// site on every path back to next. A conditional move has no single
/// unconditional endpoint in the one-pass drawing.
pub(super) fn consumed_item<'tcx>(
    body: &Body<'tcx>, binding: &IteratorBinding, next: Location,
) -> Option<Location> {
    let dom = body.basic_blocks.dominators();
    let mut sites = BTreeSet::new();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        if data.is_cleanup || !dom.dominates(binding.at.block, bb) { continue; }
        let TerminatorKind::Call { args, .. } = &data.terminator().kind else { continue };
        for arg in args {
            let Operand::Move(p) = arg.node else { continue };
            let Some(mut src) = p.as_local() else { continue };
            let mut at = body.terminator_loc(bb);
            for (i, st) in data.statements.iter().enumerate().rev() {
                if src == binding.local { break; }
                match &st.kind {
                    StatementKind::Assign(a) if a.0.local == src => {
                        let Rvalue::Use(Operand::Move(p)) = a.1 else { break };
                        let Some(from) = p.as_local() else { break };
                        if a.0.as_local().is_none() { break; }
                        src = from;
                        at.statement_index = i;
                    }
                    StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if *l == src => break,
                    _ => {}
                }
            }
            if src == binding.local && still_yielded(body, binding, at) {
                sites.insert(at);
            }
        }
    }
    let mut sites = sites.into_iter();
    let at = sites.next()?;
    if sites.next().is_some() { return None; }
    // Remove the consuming block and check whether the item path can
    // still reach next (including through a continue).
    let mut work = vec![binding.at.block];
    let mut seen = BTreeSet::new();
    while let Some(bb) = work.pop() {
        if bb == at.block || !seen.insert(bb) { continue; }
        if bb == next.block { return None; }
        work.extend(super::rows::real_successor_blocks(body, bb));
    }
    Some(at)
}

/// All normal continuations of the move must reach this iterator's next
/// call. A break, return, or cycle that avoids next cannot justify this.
pub(super) fn proceeds_to_next(body: &Body<'_>, from: Location, next: Location) -> bool {
    let mut guaranteed = BTreeSet::from([next.block]);
    loop {
        let mut changed = false;
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            if data.is_cleanup || guaranteed.contains(&bb) {
                continue;
            }
            let successors = super::rows::real_successor_blocks(body, bb);
            if !successors.is_empty() && successors.iter().all(|succ| guaranteed.contains(succ)) {
                changed |= guaranteed.insert(bb);
            }
        }
        if !changed || guaranteed.contains(&from.block) {
            return guaranteed.contains(&from.block);
        }
    }
}

/// Walk back from the move to the payload assignment. A write through
/// the reference preserves its identity, a write to its slot replaces it.
fn still_yielded(body: &Body<'_>, binding: &IteratorBinding, at: Location) -> bool {
    let mut work = vec![at];
    let mut seen = BTreeSet::new();
    while let Some(at) = work.pop() {
        if !seen.insert(at) { continue; }
        let mut reached = false;
        for (i, st) in body[at.block].statements[..at.statement_index].iter().enumerate().rev() {
            if (Location { block: at.block, statement_index: i }) == binding.at {
                reached = true;
                break;
            }
            match &st.kind {
                StatementKind::Assign(a) if a.0.as_local() == Some(binding.local) => return false,
                StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if *l == binding.local => return false,
                _ => {}
            }
        }
        if reached { continue; }
        if at.block == rustc_middle::mir::START_BLOCK { return false; }
        for pred in super::rows::real_predecessors(body, at.block) {
            if matches!(body[pred].terminator().kind, TerminatorKind::Call { destination, .. }
                if destination.as_local() == Some(binding.local)) { return false; }
            work.push(body.terminator_loc(pred));
        }
    }
    true
}

/// A return can enter a chain shared by other exits. Follow the normal
/// successors from the item's destructor to this iterator's destructor.
pub(super) fn drops_before(body: &Body<'_>, mut from: rustc_middle::mir::BasicBlock, to: rustc_middle::mir::BasicBlock) -> bool {
    let mut seen = BTreeSet::new();
    while seen.insert(from) {
        if from == to { return true; }
        let successors = super::rows::real_successor_blocks(body, from);
        let [next] = successors.as_slice() else { break };
        from = *next;
    }
    false
}
