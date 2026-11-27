//! Reference stores and releases in one borrowck body. A source expression
//! selects a store once, when the visitor emits its arrow. After that, even
//! identical arrows use the MIR destination and occurrence of that store.
use super::{BodyFacts, DropRoute, Ev, Fact, Occurrence};
use rustc_middle::mir::{Body, Location, Place, StatementKind, TerminatorKind};
use rustc_middle::ty::TyCtxt;
use rustc_span::Span;
use std::collections::{BTreeMap, BTreeSet, HashMap};

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
                    effects.boundaries.entry(*loan).or_default().push(Boundary {
                        row,
                        loan: *loan,
                        fact,
                        release: matches!(fact.fact, Fact::BorrowEnd { .. }),
                    });
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
    pub unknown: bool,
}

pub(super) struct IncomingStores<'a, 'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub body: &'a Body<'tcx>,
    pub preds: BTreeMap<Location, Vec<Location>>,
    pub addressed: Vec<Place<'tcx>>,
    pub moves: &'a rustc_mir_dataflow::move_paths::MoveData<'tcx>,
}

impl<'tcx> IncomingStores<'_, 'tcx> {
    /// Follow only value-preserving transfers back to the issuing borrow.
    /// The signature of a call cannot identify which reference it returned.
    pub fn direct_borrow(&self, mut before: Location, mut slot: Place<'tcx>) -> Option<Location> {
        use rustc_middle::mir::{Operand, ProjectionElem, Rvalue};
        let mut seen = std::collections::HashSet::new();
        while seen.insert((before, slot)) {
            let defs = self.definitions(before, slot);
            if defs.unknown || defs.uninitialized || defs.sites.len() != 1 { return None; }
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

    /// Stop each normal incoming path independently. An uninitialized arm
    /// contributes no loan, but its sibling's definition still reaches the join.
    pub fn definitions(&self, point: Location, slot: Place<'tcx>) -> Incoming {
        use rustc_middle::mir::ProjectionElem;
        let Self { tcx, body, preds, addressed, moves } = self;
        let tcx = *tcx;
        let slot = tcx.erase_and_anonymize_regions(slot);
        let defines = |dst: Place<'tcx>| {
            dst.local == slot.local && slot.projection.starts_with(tcx.erase_and_anonymize_regions(dst).projection)
        };
        let mut result = Incoming::default();
        if !slot.iter_projections().all(|(base, step)|
            matches!(step, ProjectionElem::Field(..)) && !base.ty(&body.local_decls, tcx).ty.is_union())
        {
            result.unknown = true;
            return result;
        }
        let exposed = addressed.iter().any(|&p| defines(p));
        let mut work = vec![point];
        let mut seen = BTreeSet::new();
        while let Some(next) = work.pop() {
            if !seen.insert(next) { continue; }
            let Some(incoming) = preds.get(&next) else {
                if body.args_iter().any(|l| l == slot.local) { result.unknown = true; }
                else { result.uninitialized = true; }
                continue;
            };
            for &at in incoming {
                let data = &body[at.block];
                let def = if let Some(st) = data.statements.get(at.statement_index) {
                    match &st.kind {
                        StatementKind::Assign(write) if defines(write.0) => Some(None),
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
                            if target.start_location() == next && defines(destination) => Some(Some(next)),
                        TerminatorKind::Goto { .. } | TerminatorKind::SwitchInt { .. } => None,
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
                } else if moves.loc_map[at].iter().any(|m| defines(moves.move_paths[moves.moves[*m].path].place)) {
                    result.uninitialized = true;
                } else {
                    work.push(at);
                }
            }
        }
        result
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
