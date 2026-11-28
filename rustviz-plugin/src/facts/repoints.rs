//! Reference stores and releases in one borrowck body. A source expression
//! selects a store once, when the visitor emits its arrow. After that, even
//! identical arrows use the MIR destination and occurrence of that store.
use super::{BodyFacts, DropRoute, Ev, Fact, Occurrence};
use rustc_middle::mir::{Body, Local, Location, Place, ProjectionElem, Rvalue, StatementKind, TerminatorKind};
use rustc_middle::ty::{TyCtxt, TyKind};
use rustc_span::Span;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(crate) struct Store<'a, 'tcx> {
    pub row: usize,
    pub fact: &'a Ev,
    pub destination: Place<'tcx>,
    pub incoming: Option<&'a [usize]>,
    pub installed: &'a [usize],
    pub prior: Option<Occurrence>,
}

#[derive(Clone, Copy)]
pub(crate) struct Boundary<'a> {
    pub loan: usize,
    pub fact: &'a Ev,
    pub release: bool,
    pub row: usize,
}

impl Boundary<'_> {
    pub fn key(&self) -> (usize, Occurrence) {
        (self.loan, self.fact.occurrence)
    }
}

/// Span lookup is only the bridge from an emitting HIR assignment. It must
/// denote one MIR store. In particular a duplicated lowering of an expression
/// cannot select one of its executions by taking the first match.
#[derive(Default)]
pub(crate) struct ReferenceEffects<'a, 'tcx> {
    pub(crate) stores: BTreeMap<Occurrence, Store<'a, 'tcx>>,
    pub(crate) consecutive: BTreeSet<Occurrence>,
    assignments: HashMap<Span, Vec<Store<'a, 'tcx>>>,
    calls: HashMap<Span, BTreeSet<Location>>,
    boundaries: BTreeMap<usize, Vec<Boundary<'a>>>,
    hold_ends: BTreeMap<Occurrence, Vec<Boundary<'a>>>,
}

#[derive(Default)]
pub(crate) struct ContinuingLoans {
    pub possible: BTreeSet<usize>,
    pub definite: BTreeSet<usize>,
}

impl<'a, 'tcx> ReferenceEffects<'a, 'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, body: &'a Body<'tcx>, facts: Option<&'a BodyFacts>) -> Self {
        let mut effects = Self::default();
        let Some(facts) = facts else { return effects };
        for (row, fact) in facts
            .rows
            .iter()
            .flat_map(|row| row.facts.iter().map(move |e| (row.line, e)))
            .chain(facts.inline_holder_ends.iter().map(|(line, e)| (*line, e)))
        {
            if fact.drop_route == DropRoute::Unwind {
                continue;
            }
            let at = fact.occurrence.at;
            match &fact.fact {
                Fact::Repoint { reaching, definitions, maybe_uninitialized, new, .. } => {
                    let Some(stmt) = body[at.block].statements.get(at.statement_index) else { continue };
                    let StatementKind::Assign(pair) = &stmt.kind else { continue };
                    let store = Store {
                        row,
                        fact,
                        destination: tcx.erase_and_anonymize_regions(pair.0),
                        incoming: reaching.as_deref(),
                        installed: new,
                        prior: match definitions.as_slice() {
                            [site] if reaching.is_some() && !maybe_uninitialized => Some(*site),
                            _ => None,
                        },
                    };
                    effects.stores.insert(fact.occurrence, store);
                    effects.assignments.entry(stmt.source_info.span.with_parent(None)).or_default().push(store);
                }
                Fact::BorrowEnd { loan, .. } | Fact::HoldEnd { loan, .. } => {
                    let boundary = Boundary {
                        row,
                        loan: *loan,
                        fact,
                        release: matches!(fact.fact, Fact::BorrowEnd { .. }),
                    };
                    effects.boundaries.entry(*loan).or_default().push(boundary);
                    if !boundary.release {
                        effects.hold_ends.entry(fact.occurrence).or_default().push(boundary);
                    }
                }
                _ => {}
            }
        }
        for (&occurrence, store) in &effects.stores {
            let Some(prior) = store.prior else { continue };
            if effects.stores.get(&prior).is_some_and(|p| {
                p.row == store.row && p.destination == store.destination && precedes(body, p.fact, store.fact)
            }) {
                effects.consecutive.extend([prior, occurrence]);
            }
        }
        // Argument events carry the operand span while implicit receiver
        // events carry the call span. Both come from this one terminator.
        if !effects.assignments.is_empty() {
            for (bb, data) in body.basic_blocks.iter_enumerated() {
                let term = data.terminator();
                let TerminatorKind::Call { args, .. } = &term.kind else { continue };
                if data.is_cleanup {
                    continue;
                }
                let at = body.terminator_loc(bb);
                for span in std::iter::once(term.source_info.span).chain(args.iter().map(|arg| arg.span)) {
                    effects.calls.entry(span.with_parent(None)).or_default().insert(at);
                }
            }
        }
        effects
    }

    pub fn assignment(&self, lhs: Span, assignment: Span) -> Option<Store<'a, 'tcx>> {
        let mut sites = [lhs, assignment]
            .into_iter()
            .flat_map(|span| self.assignments.get(&span.with_parent(None)).into_iter().flatten().copied());
        let first = sites.next()?;
        sites.all(|s| s.fact.occurrence == first.fact.occurrence).then_some(first)
    }

    pub fn call(&self, span: Span) -> Option<Location> {
        let sites = self.calls.get(&span.with_parent(None))?;
        if sites.len() == 1 { sites.first().copied() } else { None }
    }

    pub fn ends(&self, loan: usize) -> impl Iterator<Item = Boundary<'a>> + '_ {
        self.boundaries.get(&loan).into_iter().flatten().copied()
    }

    /// A HoldEnd proves the restriction continues. The value origins of
    /// the ending slots separately establish which references reached it.
    pub fn loans_outliving_place(
        &self, facts: &BodyFacts, edge: Occurrence, local: usize, projection: &[super::EndProjection],
    ) -> ContinuingLoans {
        let mut loans = ContinuingLoans::default();
        for end in self.hold_ends.get(&edge).into_iter().flatten() {
            let definite = facts.definite_hold_origins.get(&(edge, end.loan)).is_some_and(|slots| {
                slots.iter().any(|p| p.local.index() == local && p.projection.starts_with(projection))
            });
            if definite {
                loans.definite.insert(end.loan);
            }
            if definite || end.fact.end_places.iter().any(|p| p.local == local && p.projection.starts_with(projection)) {
                loans.possible.insert(end.loan);
            }
        }
        loans
    }
}

/// An indirect write can replace a slot even when an explicit store
/// intervenes between taking its address and using the pointer.
pub(super) fn addressed_slots<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> Vec<Place<'tcx>> {
    use rustc_middle::mir::{BorrowKind, Rvalue};
    body.basic_blocks
        .iter()
        .filter(|data| !data.is_cleanup)
        .flat_map(|data| &data.statements)
        .filter_map(|statement| {
            let StatementKind::Assign(write) = &statement.kind else { return None };
            match write.1 {
                Rvalue::Ref(_, BorrowKind::Mut { .. }, place) | Rvalue::RawPtr(_, place) if !place.is_indirect() => {
                    Some(tcx.erase_and_anonymize_regions(place))
                }
                _ => None,
            }
        })
        .collect()
}

#[derive(Default)]
pub(super) struct Incoming {
    pub sites: BTreeSet<Occurrence>,
    pub uninitialized: bool,
    pub argument: bool,
    pub unknown: bool,
}

pub(super) struct IncomingStores<'a, 'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub body: &'a Body<'tcx>,
    pub preds: BTreeMap<Location, Vec<Location>>,
    pub addressed: Vec<Place<'tcx>>,
    pub swaps: BTreeMap<Location, [Place<'tcx>; 2]>,
    pub moves: &'a rustc_mir_dataflow::move_paths::MoveData<'tcx>,
}

impl<'a, 'tcx> IncomingStores<'a, 'tcx> {
    #[cfg(test)]
    pub fn new(
        tcx: TyCtxt<'tcx>, body: &'a Body<'tcx>, runs: &BTreeSet<rustc_middle::mir::BasicBlock>,
        moves: &'a rustc_mir_dataflow::move_paths::MoveData<'tcx>,
    ) -> Self {
        let mut preds: BTreeMap<Location, Vec<Location>> = BTreeMap::new();
        for &block in runs {
            let terminator = body.terminator_loc(block);
            for statement_index in 0..terminator.statement_index {
                let at = Location { block, statement_index };
                preds.entry(at.successor_within_block()).or_default().push(at);
            }
            for next in super::real_successor_blocks(body, block).into_iter().filter(|b| runs.contains(b)) {
                preds.entry(next.start_location()).or_default().push(terminator);
            }
        }
        Self { tcx, body, preds, addressed: addressed_slots(tcx, body), swaps: BTreeMap::new(), moves }
    }

    /// A normal swap return defines the destination from the *pre-call*
    /// value in the other slot, including a subplace of an aggregate.
    pub(super) fn swap_source(&self, at: Location, slot: Place<'tcx>) -> Option<Place<'tcx>> {
        let &[left, right] = self.swaps.get(&at)?;
        [(left, right), (right, left)].into_iter().find_map(|(dst, src)| {
            super::places::place_under(slot, dst)
                .then(|| src.project_deeper(&slot.projection[dst.projection.len()..], self.tcx))
        })
    }

    /// Follow only value-preserving transfers back to the issuing borrow.
    /// The signature of a call cannot identify which reference it returned.
    pub fn direct_borrow(&self, mut before: Location, mut slot: Place<'tcx>) -> Option<Location> {
        use rustc_middle::mir::{Operand, ProjectionElem, Rvalue};
        let mut seen = std::collections::HashSet::new();
        while seen.insert((before, slot)) {
            let defs = self.definitions(before, slot);
            if defs.unknown || defs.uninitialized || defs.argument || defs.sites.len() != 1 { return None; }
            let site = defs.sites.first()?;
            let st = self.body[site.at.block].statements.get(site.at.statement_index)?;
            let StatementKind::Assign(write) = &st.kind else { return None };
            if write.0.as_local()? != slot.as_local()? { return None; }
            before = site.at;
            slot = match write.1 {
                Rvalue::Use(Operand::Move(p) | Operand::Copy(p)) => p,
                Rvalue::Ref(_, _, p) if p.projection.as_slice() == [ProjectionElem::Deref] => p.local.into(),
                Rvalue::Ref(..) => return Some(before),
                _ => return None,
            };
        }
        None
    }

    /// Normalize fixed array coordinates, including indices whose reaching
    /// definitions all evaluate to the same MIR constant. An index local is
    /// an operand, so its identity alone cannot identify array storage.
    pub(super) fn slot_before(&self, at: Location, place: Place<'tcx>) -> Option<Place<'tcx>> {
        let mut out = Place::from(place.local);
        for step in place.projection.iter() {
            out = match step {
                ProjectionElem::Field(..) | ProjectionElem::Downcast(..) => self.tcx.mk_place_elem(out, step),
                ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. } => {
                    let TyKind::Array(_, len) = *super::body::decl_place_ty(self.tcx, self.body, out).kind() else { return None };
                    let len = len.try_to_target_usize(self.tcx)?;
                    let offset = match step {
                        ProjectionElem::Index(index) => self.constant_index(at, index)?,
                        ProjectionElem::ConstantIndex { offset, from_end: true, .. } => len.checked_sub(offset)?,
                        ProjectionElem::ConstantIndex { offset, .. } => offset,
                        _ => unreachable!(),
                    };
                    if offset >= len { return None; }
                    super::direct_holders::element(self.tcx, out, offset, len)
                }
                _ => return None,
            };
        }
        Some(out)
    }

    fn constant_index(&self, at: Location, index: rustc_middle::mir::Local) -> Option<u64> {
        let defs = self.definitions(at, index.into());
        if defs.unknown || defs.uninitialized || defs.argument { return None; }
        let mut result = None;
        for site in defs.sites {
            let st = self.body[site.at.block].statements.get(site.at.statement_index)?;
            let StatementKind::Assign(pair) = &st.kind else { return None };
            let Rvalue::Use(op) = &pair.1 else { return None };
            let index = op.constant()?.const_.try_eval_target_usize(self.tcx, self.body.typing_env(self.tcx))?;
            if result.is_some_and(|old| old != index) { return None; }
            result = Some(index);
        }
        result
    }

    /// Stop each normal incoming path independently. An uninitialized arm
    /// contributes no loan, but its sibling's definition still reaches the join.
    pub fn definitions(&self, point: Location, slot: Place<'tcx>) -> Incoming {
        let Self { tcx, body, preds, addressed, moves, swaps } = self;
        let tcx = *tcx;
        let mut result = Incoming::default();
        let Some(slot) = self.slot_before(point, slot) else {
            result.unknown = true;
            return result;
        };
        let under = super::places::place_under;
        if slot.iter_projections().any(|(base, _)| base.ty(&body.local_decls, tcx).ty.is_union()) {
            result.unknown = true;
            return result;
        }
        let defines = |at, dst: Place<'tcx>| dst.local == slot.local
            && self.slot_before(at, dst).is_some_and(|dst| under(slot, dst));
        let overlaps = |place: Place<'tcx>| {
            place.local == slot.local && !place.is_indirect()
                && rustc_borrowck::consumers::places_conflict(tcx, body, place, slot, rustc_borrowck::consumers::PlaceConflictBias::Overlap)
        };
        let exposed = addressed.iter().copied().any(overlaps);
        let mut work = vec![point];
        let mut seen = BTreeSet::new();
        while let Some(next) = work.pop() {
            if !seen.insert(next) { continue; }
            let Some(incoming) = preds.get(&next) else {
                if body.args_iter().any(|l| l == slot.local) { result.argument = true; }
                else { result.uninitialized = true; }
                continue;
            };
            for &at in incoming {
                let data = &body[at.block];
                let def = if let Some(st) = data.statements.get(at.statement_index) {
                    match &st.kind {
                        StatementKind::Assign(write) if defines(at, write.0) => Some(None),
                        StatementKind::Assign(write) if overlaps(write.0)
                            && self.slot_before(at, write.0).is_none() => {
                            result.unknown = true;
                            continue;
                        }
                        StatementKind::SetDiscriminant { place, .. } if overlaps(**place) => {
                            result.unknown = true;
                            continue;
                        }
                        StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if *l == slot.local => {
                            result.uninitialized = true;
                            continue;
                        }
                        StatementKind::Assign(write) if !write.0.is_indirect() => None,
                        StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop
                        | StatementKind::FakeRead(..) | StatementKind::AscribeUserType(..) | StatementKind::PlaceMention(..) => None,
                        _ if exposed => {
                            // Taking the address does not write the slot. An
                            // indirect store or opaque effect may do so.
                            result.unknown = true;
                            continue;
                        }
                        _ => None,
                    }
                } else {
                    match data.terminator().kind {
                        TerminatorKind::Call { destination, target: Some(target), .. }
                            if target.start_location() == next && defines(at, destination) => Some(Some(next)),
                        TerminatorKind::Call { target: Some(target), .. }
                            if target.start_location() == next && swaps.contains_key(&at) => {
                            if self.swap_source(at, slot).is_some() {
                                Some(Some(next))
                            } else if swaps[&at].iter().copied().any(overlaps) {
                                result.unknown = true;
                                continue;
                            } else {
                                None
                            }
                        }
                        TerminatorKind::Goto { .. } | TerminatorKind::SwitchInt { .. }
                        | TerminatorKind::FalseEdge { .. } | TerminatorKind::FalseUnwind { .. }
                        | TerminatorKind::Assert { .. } => None,
                        _ if exposed => {
                            result.unknown = true;
                            continue;
                        }
                        TerminatorKind::InlineAsm { .. } => {
                            result.unknown = true;
                            continue;
                        }
                        _ => None,
                    }
                };
                if let Some(successor) = def {
                    result.sites.insert(Occurrence { at, exit: None, successor });
                } else if moves.loc_map[at].iter().any(|m| defines(at, moves.move_paths[moves.moves[*m].path].place)) {
                    result.uninitialized = true;
                } else {
                    work.push(at);
                }
            }
        }
        result
    }

    /// A value reaching this boundary can be an enum variant with no payload.
    pub(super) fn has_fieldless_variant(&self, at: Location, local: Local) -> bool {
        use rustc_middle::mir::{AggregateKind, Operand};
        let TyKind::Adt(def, _) = self.body.local_decls[local].ty.kind() else { return false; };
        if !def.is_enum() || !def.variants().iter().any(|v| v.fields.is_empty()) {
            return false;
        }
        let mut work = vec![(at, local)];
        let mut seen = BTreeSet::new();
        while let Some((at, local)) = work.pop() {
            if !seen.insert((at, local)) { continue; }
            for site in self.definitions(at, local.into()).sites {
                let Some(statement) = self.body[site.at.block].statements.get(site.at.statement_index) else { continue; };
                let StatementKind::Assign(write) = &statement.kind else { continue; };
                match &write.1 {
                    Rvalue::Aggregate(kind, operands)
                        if operands.is_empty() && matches!(**kind, AggregateKind::Adt(id, ..) if id == def.did()) =>
                        return true,
                    Rvalue::Use(Operand::Copy(src) | Operand::Move(src)) => {
                        if let Some(src) = src.as_local() {
                            work.push((site.at, src));
                        }
                    }
                    _ => {}
                }
            }
        }
        false
    }
}

/// A loan can end at the exit from an instruction, on just one successor.
/// Plain RPO is a display order and gives no ordering between sibling arms.
pub(crate) fn precedes(body: &Body<'_>, a: &Ev, b: &Ev) -> bool {
    if a.order >= b.order {
        return false;
    }
    occurrence_precedes(body, a.occurrence, b.occurrence.at)
}

pub(crate) fn occurrence_precedes(body: &Body<'_>, occurrence: Occurrence, target: Location) -> bool {
    let dom = body.basic_blocks.dominators();
    let dominates = |x: Location| x == target || x.dominates(target, dom);
    if !dominates(occurrence.at) || occurrence.exit.is_some_and(|exit| !dominates(exit)) {
        return false;
    }
    let Some(next) = occurrence.successor else { return true };
    if next == occurrence.at.successor_within_block() {
        return dominates(next);
    }
    // A back edge enters another iteration. It cannot order the first
    // execution of the target, including an exit from a self-loop.
    if dom.dominates(next.block, occurrence.at.block) {
        return false;
    }
    // At a join the target block can dominate a later use even if this
    // particular incoming edge never ran. Prove the edge was required.
    dominates(next)
        && (super::real_successor_blocks(body, occurrence.at.block).iter().all(|&bb| bb == next.block)
            || super::rows::real_predecessors(body, next.block).iter().all(|&bb| bb == occurrence.at.block))
}
