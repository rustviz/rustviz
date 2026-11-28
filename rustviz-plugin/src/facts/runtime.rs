//! Refinements to the runtime CFG used for initialization and drops.

use std::collections::BTreeMap;

use rustc_middle::mir::{AggregateKind, BasicBlock, Body, Location, Operand, Place, ProjectionElem, Rvalue, StatementKind, TerminatorKind};
use rustc_middle::ty::{TyCtxt, TyKind, TypingEnv};

/// Prove that all incoming paths supply the None variant. A joined
/// payload move path can be maybe-initialized even though every reaching
/// discriminant is None.
pub(super) fn option_is_none<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    at: Location,
    place: Place<'tcx>,
    reachable: &std::collections::BTreeSet<BasicBlock>,
) -> bool {
    let Some(local) = place.as_local() else { return false };
    let TyKind::Adt(def, _) = *body.local_decls[local].ty.kind() else { return false };
    if !tcx.is_lang_item(def.did(), rustc_hir::LangItem::Option) {
        return false;
    }
    let Some(none) = def.variants().iter_enumerated().find_map(|(index, variant)|
        tcx.is_lang_item(variant.def_id, rustc_hir::LangItem::OptionNone).then_some(index))
    else { return false };
    if body.basic_blocks.iter().flat_map(|block| &block.statements).any(|stmt|
        matches!(&stmt.kind, StatementKind::Assign(a) if matches!(a.1,
            Rvalue::RawPtr(_, p) | Rvalue::Ref(_, rustc_middle::mir::BorrowKind::Mut { .. }, p)
                if p.local == local && !p.is_indirect())))
    {
        return false;
    }
    let tag = def.discriminant_for_variant(tcx, none).val;
    let mut work = vec![(at, None)];
    let mut fuel = 128;
    while let Some((at, successor)) = work.pop() {
        if fuel == 0 { return false; }
        fuel -= 1;
        let block = &body[at.block];
        let decision = successor.and_then(|successor| match &block.terminator().kind {
            TerminatorKind::Call { destination, .. } if *destination == place => Some(Err(())),
            TerminatorKind::Drop { place: dropped, .. } if *dropped == place => Some(Err(())),
            TerminatorKind::SwitchInt { discr, targets } => {
                let discr = discr.place()?;
                let is_tag = block.statements.iter().rev().find_map(|stmt| match &stmt.kind {
                    StatementKind::Assign(a) if a.0 == discr =>
                        Some(matches!(a.1, Rvalue::Discriminant(p) if p == place)),
                    _ => None,
                }) == Some(true);
                if !is_tag { return None; }
                let selected = def.discriminants(tcx).filter(|(_, d)| targets.target_for_value(d.val) == successor)
                    .map(|(_, d)| d.val).collect::<Vec<_>>();
                (selected.as_slice() == [tag]).then_some(Ok(()))
            }
            _ => None,
        });
        match decision {
            Some(Ok(())) => continue,
            Some(Err(())) => return false,
            None => {}
        }
        let initialized = block.statements[..at.statement_index].iter().enumerate().rev().find_map(|(statement_index, st)| match &st.kind {
            StatementKind::Assign(a) if a.0 == place =>
                Some(super::body::assigned_none(tcx, body, Location { statement_index, ..at }, place, &a.1)),
            StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if *l == local => Some(false),
            StatementKind::SetDiscriminant { place: p, variant_index } if **p == place =>
                Some(*variant_index == none),
            _ => None,
        });
        match initialized {
            Some(true) => continue,
            Some(false) => return false,
            None => {}
        }
        let size = work.len();
        work.extend(body.basic_blocks.predecessors()[at.block].iter()
            .filter(|pred| reachable.contains(pred) && !body[**pred].is_cleanup)
            .map(|&pred| (body.terminator_loc(pred), Some(at.block))));
        if work.len() == size { return false; }
    }
    true
}

/// The first call to `next` on a freshly constructed nonempty integer
/// range returns Some. Give that call its own path. Its original block and
/// switch still serve subsequent iterations, which can return None.
pub(super) fn enter_nonempty_ranges<'tcx>(tcx: TyCtxt<'tcx>, env: TypingEnv<'tcx>, body: &mut Body<'tcx>) -> BTreeMap<BasicBlock, Vec<BasicBlock>> {
    let entries: Vec<_> = body.basic_blocks.iter_enumerated()
        .filter_map(|(bb, _)| range_entry(tcx, env, body, bb))
        .collect();
    let mut copies = BTreeMap::<_, Vec<_>>::new();
    for (entry, head, next, test, some) in entries {
        let blocks = body.basic_blocks_mut();
        let Some(head) = head else {
            blocks[test].terminator_mut().kind = TerminatorKind::Goto { target: some };
            continue;
        };
        let mut first_test = blocks[test].clone();
        first_test.terminator_mut().kind = TerminatorKind::Goto { target: some };
        let first_test = blocks.push(first_test);
        let mut first_next = blocks[next].clone();
        let TerminatorKind::Call { target, .. } = &mut first_next.terminator_mut().kind else { continue };
        *target = Some(first_test);
        let first_next = blocks.push(first_next);
        let mut first_head = blocks[head].clone();
        first_head.terminator_mut().kind = TerminatorKind::Goto { target: first_next };
        let first_head = blocks.push(first_head);
        blocks[entry].terminator_mut().kind = TerminatorKind::Goto { target: first_head };
        for (original, copy) in [(head, first_head), (next, first_next), (test, first_test)] {
            copies.entry(original).or_default().push(copy);
        }
    }
    copies
}

fn range_entry<'tcx>(
    tcx: TyCtxt<'tcx>, env: TypingEnv<'tcx>, body: &Body<'tcx>, construct: BasicBlock,
) -> Option<(BasicBlock, Option<BasicBlock>, BasicBlock, BasicBlock, BasicBlock)> {
    let lang = tcx.lang_items();
    let data = &body.basic_blocks[construct];
    let TerminatorKind::Call { func, args, destination, target: Some(entry), .. } = &data.terminator().kind else { return None };
    if Some(func.const_fn_def()?.0) != lang.into_iter_fn() { return None; }
    if body.basic_blocks.predecessors()[*entry].as_slice() != [construct] { return None; }
    let [arg] = &**args else { return None };
    let StatementKind::Assign(range) = &data.statements.last()?.kind else { return None };
    let Rvalue::Aggregate(kind, bounds) = &range.1 else { return None };
    let AggregateKind::Adt(def, _, _, None, None) = **kind else { return None };
    if arg.node.place()? != range.0 || Some(def) != lang.range_struct() { return None; }
    let [Operand::Constant(start), Operand::Constant(end)] = bounds.raw.as_slice() else { return None };
    if start.const_.ty() != end.const_.ty() { return None; }
    let lo = start.const_.try_eval_bits(tcx, env)?;
    let hi = end.const_.try_eval_bits(tcx, env)?;
    let nonempty = match start.const_.ty().kind() {
        TyKind::Int(_) => {
            let size = tcx.layout_of(env.as_query_input(start.const_.ty())).ok()?.size;
            size.sign_extend(lo) < size.sign_extend(hi)
        }
        TyKind::Uint(_) => lo < hi,
        _ => false,
    };
    if !nonempty { return None; }

    let data = &body.basic_blocks[*entry];
    let mut iterators = data.statements.iter().filter_map(|statement| match &statement.kind {
        StatementKind::Assign(iterator) if matches!(&iterator.1, Rvalue::Use(Operand::Move(from)) if from == destination) => Some(iterator.0),
        _ => None,
    });
    let iterator = iterators.next()?;
    if iterators.next().is_some() { return None; }
    let (head, next) = match data.terminator().kind {
        TerminatorKind::Goto { target: head } => {
            let data = &body.basic_blocks[head];
            match data.terminator().kind {
                TerminatorKind::Goto { target: next } if data.statements.is_empty() => (Some(head), next),
                TerminatorKind::Call { .. } if body.basic_blocks.predecessors()[head].as_slice() == [*entry] => (None, head),
                _ => return None,
            }
        }
        // If every iteration exits, MIR may merge the entry and first
        // next call. This block has only the IntoIterator predecessor.
        TerminatorKind::Call { .. } => (None, *entry),
        _ => return None,
    };
    let data = &body.basic_blocks[next];
    let TerminatorKind::Call { func, args, destination, target: Some(test), .. } = &data.terminator().kind else { return None };
    if Some(func.const_fn_def()?.0) != lang.next_fn()
        || body.basic_blocks.predecessors()[*test].as_slice() != [next] { return None; }
    let [arg] = &**args else { return None };
    let [.., directly, reborrow] = data.statements.as_slice() else { return None };
    let (StatementKind::Assign(directly), StatementKind::Assign(reborrow)) = (&directly.kind, &reborrow.kind) else { return None };
    if !matches!(&directly.1, Rvalue::Ref(_, _, from) if *from == iterator)
        || !matches!(&reborrow.1, Rvalue::Ref(_, _, from)
            if from.local == directly.0.as_local()? && from.projection.as_slice() == [ProjectionElem::Deref])
        || arg.node.place()? != reborrow.0
    {
        return None;
    }

    let data = &body.basic_blocks[*test];
    let TerminatorKind::SwitchInt { discr, targets } = &data.terminator().kind else { return None };
    let StatementKind::Assign(tag) = &data.statements.last()?.kind else { return None };
    if discr.place()? != tag.0 || !matches!(&tag.1, Rvalue::Discriminant(from) if from == destination) { return None; }
    let ty = destination.ty(&body.local_decls, tcx).ty;
    let TyKind::Adt(adt, _) = ty.kind() else { return None };
    let some = lang.option_some_variant()?;
    let (variant, _) = adt.variants().iter_enumerated().find(|(_, v)| v.def_id == some)?;
    let some = targets.target_for_value(adt.discriminant_for_variant(tcx, variant).val);
    Some((*entry, head, next, *test, some))
}
