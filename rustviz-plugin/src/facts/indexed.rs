//! Indexed reference copies have two different sources of evidence.
//! The MIR value reaching the selected slot describes the referent. The
//! region of the copied reference describes the borrow checker's restrictions.

use super::schema::{EndPlace, IndexedRead};
use rustc_hir::LangItem;
use rustc_middle::mir::{
    AggregateKind, Body, Local, Location, Operand, Place, PlaceElem, ProjectionElem, Rvalue,
    StatementKind, TerminatorKind, START_BLOCK,
};
use rustc_middle::ty::{TyCtxt, TyKind};
use rustc_span::Span;
use std::collections::{BTreeMap, BTreeSet};

const WALK_LIMIT: usize = 128;
const CANDIDATE_LIMIT: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Index {
    Constant(u64),
    Incoming(Local),
    Unknown,
}

#[derive(Clone, Copy)]
enum Step<'tcx> {
    Element(Index),
    Projection(PlaceElem<'tcx>),
}

pub(super) fn reads<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    name: impl Fn(Place<'tcx>) -> Option<String>,
    loans_at: impl Fn(Location, Place<'tcx>) -> BTreeSet<usize>,
    issued_at: impl Fn(Location) -> Option<usize>,
    drop_uses: impl Fn(Location, Place<'tcx>) -> BTreeSet<usize>,
) -> BTreeMap<Span, IndexedRead> {
    let walk = Walk {
        tcx,
        body,
        address_taken: rustc_mir_dataflow::impls::borrowed_locals(body)
            .iter()
            .collect(),
    };
    let mut reads = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for (bb, block) in body.basic_blocks.iter_enumerated() {
        if block.is_cleanup {
            continue;
        }
        for (statement_index, statement) in block.statements.iter().enumerate() {
            let StatementKind::Assign(a) = &statement.kind else {
                continue;
            };
            let (input, staged_reborrow) = match a.1 {
                Rvalue::Use(Operand::Copy(input)) => (input, false),
                // rustc can stage an indexed &T into an aggregate by
                // reborrowing the same referent, as in Watch { r: slots[0] }.
                Rvalue::Ref(_, rustc_middle::mir::BorrowKind::Shared, borrowed)
                    if borrowed.projection.last() == Some(&ProjectionElem::Deref) => {
                    (Place { local: borrowed.local, projection: tcx.mk_place_elems(
                        &borrowed.projection[..borrowed.projection.len() - 1]) }, true)
                }
                _ => continue,
            };
            if !input.ty(&body.local_decls, tcx).ty.ref_mutability().is_some_and(|m| !m.is_mut()) {
                continue;
            }
            let at = Location {
                block: bb,
                statement_index,
            };
            let (stored_at, destination) = walk.destination(at, a.0);
            if staged_reborrow && stored_at == at {
                continue;
            }
            let place = super::body::resolve_temp_deref(tcx, body, input);
            let element = place.projection.iter().position(is_element);
            let span = statement.source_info.span;
            let (base, candidates, through_element) = if let Some(i) = element {
                let indirect = place.projection[i + 1..].contains(&ProjectionElem::Deref);
                let base = (!indirect).then(|| Place {
                    local: place.local,
                    projection: tcx.mk_place_elems(&place.projection[..i]),
                });
                let candidates = base.and_then(|_| {
                    let path = walk.path(place, at);
                    walk.origins(at, place.local, &path, &issued_at, &mut { WALK_LIMIT })
                });
                (base, candidates, indirect)
            } else {
                let Some(base) = walk.overloaded_input(at, place) else {
                    continue;
                };
                (base, None, false)
            };
            let stored_referents = base.filter(|_| element.is_some()).and_then(|base| {
                let TyKind::Array(_, size) = base.ty(&body.local_decls, tcx).ty.kind() else { return None };
                let size = size.try_to_target_usize(tcx)?;
                if size > CANDIDATE_LIMIT as u64 || place.projection.len() != base.projection.len() + 1 {
                    return None;
                }
                let mut path = walk.path(base, at);
                let index = path.len();
                path.push(Step::Element(Index::Unknown));
                let mut stored = BTreeSet::new();
                for i in 0..size {
                    path[index] = Step::Element(Index::Constant(i));
                    stored.extend(walk.origins(at, base.local, &path, &issued_at, &mut { WALK_LIMIT })?);
                }
                Some(stored)
            });
            let restrictions = loans_at(at, input);
            let source = base.and_then(|base| {
                let p = walk.named_prefix(base);
                name(p)
            });
            let container = base.and_then(|base| {
                if !base.projection.iter().all(|step| matches!(step, ProjectionElem::Field(..))) {
                    return None;
                }
                Some((EndPlace::new(tcx, base, name(base)?), drop_uses(at, base)))
            });
            let read = IndexedRead {
                at,
                container,
                source,
                through_element,
                trait_index: element.is_none(),
                destination: name(destination).map(|n| EndPlace::new(tcx, destination, n)),
                destination_drop: drop_uses(stored_at, destination),
                stored_referents,
                candidates,
                restrictions,
            };
            if reads.insert(span, read).is_some() {
                ambiguous.insert(span);
            }
        }
    }
    for span in ambiguous {
        reads.remove(&span);
    }
    reads
}

fn is_element(step: PlaceElem<'_>) -> bool {
    matches!(
        step,
        ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. }
    )
}

struct Walk<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    body: &'a Body<'tcx>,
    address_taken: BTreeSet<Local>,
}

impl<'tcx> Walk<'_, 'tcx> {
    fn operand_index(&self, at: Location, op: &Operand<'tcx>, fuel: &mut usize) -> Index {
        match op {
            Operand::Constant(c) => c
                .const_
                .try_eval_target_usize(self.tcx, self.body.typing_env(self.tcx))
                .map_or(Index::Unknown, Index::Constant),
            Operand::Move(p) | Operand::Copy(p) => p
                .as_local()
                .map_or(Index::Unknown, |local| self.index(at, local, fuel)),
            Operand::RuntimeChecks(_) => Index::Unknown,
        }
    }

    fn index(&self, mut at: Location, local: Local, fuel: &mut usize) -> Index {
        if self.address_taken.contains(&local) {
            return Index::Unknown;
        }
        while *fuel > 0 {
            *fuel -= 1;
            if at.statement_index == 0 {
                if at.block == START_BLOCK && self.body.args_iter().any(|arg| arg == local) {
                    return Index::Incoming(local);
                }
                let Some(prev) = self.predecessor(at, local) else {
                    break;
                };
                if matches!(self.body[prev.block].terminator().kind, TerminatorKind::SwitchInt { .. }) {
                    return Index::Unknown;
                }
                at = prev;
                continue;
            }
            at.statement_index -= 1;
            match &self.body[at.block].statements[at.statement_index].kind {
                StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if *l == local => {
                    break
                }
                StatementKind::Assign(a) if a.0.local == local => {
                    if let ([], Rvalue::Use(op)) = (a.0.projection.as_slice(), &a.1) {
                        return self.operand_index(at, op, fuel);
                    }
                    break;
                }
                _ => {}
            }
        }
        Index::Unknown
    }

    fn predecessor(&self, at: Location, local: Local) -> Option<Location> {
        let mut preds = super::rows::real_predecessors(self.body, at.block)
            .into_iter()
            .filter(|bb| !self.body[*bb].is_cleanup);
        let prev = preds.next()?;
        if preds.next().is_some() {
            return None;
        }
        match self.body[prev].terminator().kind {
            TerminatorKind::Call { destination, .. } if destination.local == local => None,
            TerminatorKind::InlineAsm { .. } | TerminatorKind::Yield { .. } => None,
            _ => Some(self.body.terminator_loc(prev)),
        }
    }

    fn path(&self, place: Place<'tcx>, at: Location) -> Vec<Step<'tcx>> {
        place
            .projection
            .iter()
            .enumerate()
            .map(|(i, step)| match step {
                ProjectionElem::Index(local) => {
                    Step::Element(self.index(at, local, &mut { WALK_LIMIT }))
                }
                ProjectionElem::ConstantIndex {
                    offset,
                    from_end: false,
                    ..
                } => Step::Element(Index::Constant(offset)),
                ProjectionElem::ConstantIndex {
                    offset,
                    from_end: true,
                    ..
                } => {
                    let prefix = Place {
                        local: place.local,
                        projection: self.tcx.mk_place_elems(&place.projection[..i]),
                    };
                    let index = match prefix.ty(&self.body.local_decls, self.tcx).ty.kind() {
                        TyKind::Array(_, size) => size
                            .try_to_target_usize(self.tcx)
                            .and_then(|n| n.checked_sub(offset)),
                        _ => None,
                    };
                    Step::Element(index.map_or(Index::Unknown, Index::Constant))
                }
                _ => Step::Projection(step),
            })
            .collect()
    }

    /// The displayed source is a named binding or field before any
    /// implicit dereference or dynamic projection.
    fn named_prefix(&self, place: Place<'tcx>) -> Place<'tcx> {
        let len = place.projection.iter().take_while(|step|
            matches!(step, ProjectionElem::Field(..))).count();
        Place { local: place.local, projection: self.tcx.mk_place_elems(&place.projection[..len]) }
    }

    /// Follow a staged load into its aggregate. Only follow a use of the
    /// exact temporary made by this load, before its storage is reused.
    fn destination(&self, mut at: Location, mut dest: Place<'tcx>) -> (Location, Place<'tcx>) {
        let block = &self.body[at.block];
        while dest.as_local().is_some() && !self.body.local_decls[dest.local].is_user_variable()
            && !self.address_taken.contains(&dest.local) {
            let mut transfer = None;
            for (i, statement) in block.statements.iter().enumerate().skip(at.statement_index + 1) {
                match &statement.kind {
                    StatementKind::Assign(a) => {
                        let uses = match &a.1 {
                            Rvalue::Use(op) => op.place() == Some(dest),
                            Rvalue::Aggregate(_, operands) => operands.iter().any(|op| op.place() == Some(dest)),
                            _ => false,
                        };
                        if uses {
                            transfer = Some((i, a.0));
                            break;
                        }
                        if a.0.local == dest.local {
                            break;
                        }
                    }
                    StatementKind::StorageLive(local) | StatementKind::StorageDead(local)
                        if *local == dest.local => break,
                    _ => {}
                }
            }
            let Some((index, next)) = transfer else { break };
            at.statement_index = index;
            dest = next;
        }
        (at, dest)
    }

    /// A unique predecessor and a bounded set of reaching values suffice
    /// for straight-line construction and overwrites. Merges, pointer
    /// aliasing and opaque calls leave the actual origin unknown.
    fn origins(
        &self,
        mut at: Location,
        local: Local,
        path: &[Step<'tcx>],
        issued_at: &impl Fn(Location) -> Option<usize>,
        fuel: &mut usize,
    ) -> Option<BTreeSet<usize>> {
        if self.address_taken.contains(&local)
            || path.iter().any(|s| {
                matches!(
                    s,
                    Step::Projection(ProjectionElem::Deref | ProjectionElem::Subslice { .. })
                )
            })
        {
            return None;
        }
        while *fuel > 0 {
            *fuel -= 1;
            if at.statement_index == 0 {
                at = self.predecessor(at, local)?;
                continue;
            }
            at.statement_index -= 1;
            match &self.body[at.block].statements[at.statement_index].kind {
                StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if *l == local => {
                    return None
                }
                StatementKind::Assign(a) if a.0.local == local => {
                    let written = self.path(a.0, at);
                    let mut may_overlap = false;
                    let mut disjoint = false;
                    for (i, (&read, &store)) in path.iter().zip(&written).enumerate() {
                        match (read, store) {
                            (
                                Step::Element(Index::Constant(x)),
                                Step::Element(Index::Constant(y)),
                            ) if x != y => {
                                disjoint = true;
                                break;
                            }
                            (Step::Element(x), Step::Element(y)) => {
                                may_overlap |= x != y || x == Index::Unknown
                            }
                            (
                                Step::Projection(ProjectionElem::Field(x, read_ty)),
                                Step::Projection(ProjectionElem::Field(y, store_ty)),
                            ) => {
                                if x == y { continue; }
                                let prefix = Place { local, projection: self.tcx.mk_place_elems(&a.0.projection[..i]) };
                                if let Some(union) = prefix.ty(&self.body.local_decls, self.tcx).ty.ty_adt_def().filter(|def| def.is_union()) {
                                    // C-layout unions overlay same-typed fields at offset zero.
                                    // For other layouts or types, the store stops the proof.
                                    if !union.repr().c() || self.tcx.erase_and_anonymize_regions(read_ty)
                                        != self.tcx.erase_and_anonymize_regions(store_ty) {
                                        return None;
                                    }
                                    continue;
                                }
                                disjoint = true;
                                break;
                            }
                            (Step::Projection(x), Step::Projection(y)) if x == y => {}
                            _ => return None,
                        }
                    }
                    if disjoint {
                        continue;
                    }
                    // A partial overwrite of a dynamic selection kills
                    // some old values. Until that selection is split at
                    // the write, the old aggregate is not a candidate list.
                    if may_overlap {
                        return None;
                    }
                    return self.value_origins(at, &a.1, path.get(written.len()..)?, issued_at, fuel);
                }
                _ => {}
            }
        }
        None
    }

    fn value_origins(
        &self,
        at: Location,
        value: &Rvalue<'tcx>,
        path: &[Step<'tcx>],
        issued_at: &impl Fn(Location) -> Option<usize>,
        fuel: &mut usize,
    ) -> Option<BTreeSet<usize>> {
        let trace = |op: &Operand<'tcx>, suffix: &[Step<'tcx>], fuel: &mut usize| {
            let p = op.place()?;
            let mut prefix = self.path(p, at);
            prefix.extend_from_slice(suffix);
            self.origins(at, p.local, &prefix, issued_at, fuel)
        };
        match (value, path) {
            (Rvalue::Ref(_, _, place), []) => {
                // Shared reborrows of a shared reference may be absent from
                // borrowck's tracked set. They still preserve the referent.
                if place.projection.as_slice() == [ProjectionElem::Deref]
                    && self.body.local_decls[place.local]
                        .ty
                        .ref_mutability()
                        .is_some_and(|m| !m.is_mut())
                {
                    return self.origins(at, place.local, &[], issued_at, fuel);
                }
                Some(BTreeSet::from([issued_at(at)?]))
            }
            (Rvalue::Use(op), _) => trace(op, path, fuel),
            (Rvalue::Repeat(op, _), [Step::Element(_), rest @ ..]) => trace(op, rest, fuel),
            (Rvalue::Aggregate(kind, ops), [Step::Element(index), rest @ ..])
                if matches!(**kind, AggregateKind::Array(_)) =>
            {
                if let Index::Constant(index) = index {
                    return trace(ops.iter().nth(usize::try_from(*index).ok()?)?, rest, fuel);
                }
                if *index == Index::Unknown || ops.len() > CANDIDATE_LIMIT {
                    return None;
                }
                let mut candidates = BTreeSet::new();
                for (i, op) in ops.iter().enumerate() {
                    // Repeated uses of the same unchanged argument select
                    // the same offset (e.g. matrix[i][i]).
                    let rest: Vec<_> = rest.iter().map(|step| match step {
                        Step::Element(other) if other == index => Step::Element(Index::Constant(i as u64)),
                        other => *other,
                    }).collect();
                    candidates.extend(trace(op, &rest, fuel)?);
                }
                (candidates.len() <= CANDIDATE_LIMIT).then_some(candidates)
            }
            (
                Rvalue::Aggregate(kind, ops),
                [Step::Projection(ProjectionElem::Field(field, _)), rest @ ..],
            ) if matches!(**kind, AggregateKind::Tuple)
                || matches!(**kind, AggregateKind::Adt(def, ..) if self.tcx.adt_def(def).is_struct()) =>
            {
                trace(ops.get(*field)?, rest, fuel)
            }
            _ => None,
        }
    }

    /// A trait call can return unrelated storage. Recognize the MIR call
    /// and its copied result, and label a collection only for standard
    /// slice or Vec indexing.
    fn overloaded_input(&self, mut at: Location, p: Place<'tcx>) -> Option<Option<Place<'tcx>>> {
        if p.projection.as_slice() != [ProjectionElem::Deref] || self.address_taken.contains(&p.local) {
            return None;
        }
        for _ in 0..WALK_LIMIT {
            if self.body[at.block].statements[..at.statement_index].iter().any(|s|
                matches!(&s.kind, StatementKind::Assign(a) if a.0.local == p.local)
                    || matches!(s.kind, StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if l == p.local)) {
                return None;
            }
            let mut preds = super::rows::real_predecessors(self.body, at.block)
                .into_iter()
                .filter(|bb| !self.body[*bb].is_cleanup);
            let pred = preds.next()?;
            if preds.next().is_some() {
                return None;
            }
            let term = self.body[pred].terminator();
            if let TerminatorKind::Call {
                destination,
                func,
                args,
                target: Some(target),
                ..
            } = &term.kind
            {
                if destination.as_local() != Some(p.local) {
                    at = self.predecessor(at, p.local)?;
                    continue;
                }
                if *target != at.block {
                    return None;
                }
                let (callee, types) = func.const_fn_def()?;
                let tr = self.tcx.trait_of_assoc(callee)?;
                if !self.tcx.is_lang_item(tr, LangItem::Index)
                    && !self.tcx.is_lang_item(tr, LangItem::IndexMut)
                {
                    return None;
                }
                let base = super::iterators::preserves_items(
                    self.tcx,
                    self.body.typing_env(self.tcx),
                    callee,
                    types,
                )
                .then(|| args.first()?.node.place())
                .flatten()
                .map(|p| {
                    super::body::resolve_temp_deref(self.tcx, self.body, self.tcx.mk_place_deref(p))
                });
                return Some(base);
            }
            at = self.predecessor(at, p.local)?;
        }
        None
    }
}
