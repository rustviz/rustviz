//! Carry compiler occurrences from the emitting visitor to the drawn row.
use super::{E, Order, RowLayout, end, endpoints, fact_point};
use crate::facts::repoints::{Boundary, ReferenceEffects, Store, occurrence_precedes, precedes};
use crate::facts::{EndPlace, Row};
use crate::svg_generator::data::ResourceAccessPoint;
use rustc_middle::mir::Body;
use rustc_span::Span;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Default)]
pub(crate) struct DrawnReferenceEffects<'a, 'tcx> {
    /// The actual destination write. Several possible lenders of a single
    /// call result may be drawn separately, but are still one store.
    pub installs: BTreeMap<usize, Store<'a, 'tcx>>,
    pub replaced_by: BTreeMap<usize, Store<'a, 'tcx>>,
    final_store: BTreeMap<usize, Store<'a, 'tcx>>,
    /// End events selected while placing the corresponding MIR boundaries.
    /// A single event on a join can represent more than one execution.
    pub ends: BTreeMap<usize, Vec<Boundary<'a>>>,
    /// One drawn column may be shared by disjoint binding declarations.
    /// Use it to select a projected slot only when all its stores agree.
    slots: BTreeMap<u64, Option<EndPlace>>,
    /// The store of an explicit RHS borrow. RefData retains the RHS span
    /// even if another branch or a later assignment reuses its visual slot.
    rhs: HashMap<Span, Option<Store<'a, 'tcx>>>,
}

impl<'a, 'tcx> DrawnReferenceEffects<'a, 'tcx> {
    pub fn install(
        &mut self,
        event: usize,
        rhs: Span,
        column: u64,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
        store: Store<'a, 'tcx>,
    ) {
        self.installs.insert(event, store);
        let slot = EndPlace::new(tcx, store.destination, String::new());
        self.slots
            .entry(column)
            .and_modify(|previous| {
                if previous.as_ref() != Some(&slot) {
                    *previous = None;
                }
            })
            .or_insert(Some(slot));
        self.rhs
            .entry(rhs.with_parent(None))
            .and_modify(|previous| {
                if previous.is_some_and(|s| s.fact.occurrence != store.fact.occurrence) {
                    *previous = None;
                }
            })
            .or_insert(Some(store));
    }

    /// Keep independently live loans of the slot (especially those retained
    /// by a reference field). Only discard a past generation when all its
    /// normal releases precede this store, including the outgoing edge.
    pub fn ended_before_install(
        &self,
        rhs: Option<Span>,
        body: &Body<'tcx>,
        effects: &ReferenceEffects<'a, 'tcx>,
    ) -> BTreeSet<usize> {
        let Some(store) = rhs.and_then(|s| self.rhs.get(&s.with_parent(None))).copied().flatten() else {
            return BTreeSet::new();
        };
        self.installs
            .values()
            .filter(|s| s.destination == store.destination && precedes(body, s.fact, store.fact))
            .flat_map(|s| s.installed)
            .copied()
            .filter(|id| {
                if store.installed.contains(id) {
                    return false;
                }
                let mut ends = effects.ends(*id).filter(|end| end.release);
                ends.next().is_some_and(|end| precedes(body, end.fact, store.fact))
                    && ends.all(|end| precedes(body, end.fact, store.fact))
            })
            .collect()
    }

    /// Attach a synthesized end at emission time. If several normal edges
    /// can end the same loan, keep them all. A layout consumer may use a
    /// single point only when the edges agree.
    pub fn bind_end(
        &mut self,
        id: usize,
        loans: &[usize],
        rhs: Option<Span>,
        column: u64,
        local: Option<rustc_middle::mir::Local>,
        release: bool,
        effects: &ReferenceEffects<'a, 'tcx>,
    ) {
        if let Some(store) = rhs.and_then(|s| self.rhs.get(&s.with_parent(None))).copied().flatten() {
            if loans.iter().any(|id| store.installed.contains(id)) {
                self.final_store.insert(id, store);
            }
        }
        let slot = match self.slots.get(&column) {
            Some(None) => return,
            known => known.and_then(Option::as_ref),
        };
        if slot.is_none() && local.is_none() {
            return;
        }
        let boundaries: Vec<_> = loans
            .iter()
            .flat_map(|loan| effects.ends(*loan))
            .filter(|end| {
                end.release == release
                    && end.fact.end_places.iter().any(|p| {
                        slot.map_or_else(
                            || local.is_some_and(|l| p.local == l.index()) && p.projection.is_empty(),
                            |slot| p.local == slot.local && p.projection == slot.projection,
                        )
                    })
            })
            .collect();
        if !boundaries.is_empty() {
            self.ends.insert(id, boundaries);
        }
    }
}

impl<'a, 'tcx> Order<'a, 'tcx> {
    /// Distinguish the old and new values on a field's coincident arrows.
    /// The preceding definition can hold the very same loans as the new
    /// one. Its store identity, rather than loan-set inequality, tells us
    /// which reference was installed by which assignment.
    pub(super) fn field_generation(&self, event: &E, row: &Row) -> Option<(Store<'a, 'tcx>, bool)> {
        let id = event.get_id();
        let slot = if end(event) { endpoints(event).0 } else { endpoints(event).1 };
        if !slot.extract_rap().is_some_and(|field| field.is_ref() && field.is_member()) {
            return None;
        }
        let (store, replaced) = if let Some(store) = self.references.replaced_by.get(&id) {
            match store.prior {
                Some(prior) if self.reference_facts.stores.get(&prior).is_some_and(|s| s.row == row.line) => {
                    (&self.reference_facts.stores[&prior], false)
                }
                _ => (store, true),
            }
        } else {
            (self.references.installs.get(&id).or_else(|| self.references.final_store.get(&id))?, false)
        };
        (store.row == row.line && self.reference_facts.consecutive.contains(&store.fact.occurrence))
            .then_some((*store, replaced))
    }

    /// Does this end represent copying a shared reference which keeps a
    /// mutable loan alive? The same holder can also lose an unrelated
    /// shared loan at an overwrite on this very row.
    pub(super) fn copied_at_repoint(&self, id: usize, holder: rustc_middle::mir::Local) -> bool {
        use rustc_middle::mir::{Operand, Rvalue, StatementKind};
        let Some(ends) = self.references.ends.get(&id) else { return false };
        !ends.is_empty()
            && ends.iter().all(|end| {
                if end.release {
                    return false;
                }
                let at = end.fact.occurrence.at;
                let Some(statement) = self.body[at.block].statements.get(at.statement_index) else { return false };
                let StatementKind::Assign(copy) = &statement.kind else { return false };
                let Rvalue::Use(Operand::Copy(src)) = copy.1 else { return false };
                if src.as_local() != Some(holder)
                    || !copy.0.as_local().is_some_and(|dest| {
                        end.fact.continuing_places.iter().any(|p| p.local == dest.index() && p.projection.is_empty())
                    })
                {
                    return false;
                }
                let Some(loan) = self.loan(end.loan) else { return false };
                let mutable_origin = loan.kind == "mut"
                    || loan.made_through.iter().any(|parent| {
                        self.loan(*parent).is_some_and(|l| l.kind == "mut")
                            && ends.iter().any(|end| end.loan == *parent)
                    });
                mutable_origin
                    && self.references.installs.values().any(|s| {
                        s.destination.as_local() == Some(holder)
                            && (s.incoming.unwrap_or_default().contains(&end.loan) || s.installed.contains(&end.loan))
                    })
            })
    }

    /// The holder can lose the value earlier than NLL releases its
    /// region (e.g. a copy lives on in q). Its actual overwrite is an
    /// upper bound on how long that generation stays in this field.
    pub(super) fn holder_end_point(&self, id: usize) -> Option<(super::Point, crate::facts::Occurrence)> {
        let ends = self.references.ends.get(&id)?;
        let first = ends.first()?;
        if !ends.iter().all(|b| b.fact.occurrence == first.fact.occurrence && b.release == first.release) { return None; }
        if ends.iter().all(|b| !b.release) {
            if let Some(store) = self.references.replaced_by.get(&id) {
                if store.fact.occurrence.at == first.fact.occurrence.at || precedes(self.body, store.fact, first.fact) {
                    let (bb, i, _) = fact_point(store.fact);
                    return Some(((bb, i, super::BEFORE_STORE), store.fact.occurrence));
                }
            }
        }
        Some((fact_point(first.fact), first.fact.occurrence))
    }

    /// A visual row is a request to lay out a set of events. Which write
    /// each arrow depicts, and which release it returns, were recorded by
    /// their producers. Sibling arms have separate lists even on one row.
    pub(super) fn order_stores(&self, indices: &[usize], events: &[(usize, E)], layout: &mut RowLayout) {
        let installs: Vec<_> = indices
            .iter()
            .filter_map(|&i| {
                let id = events[i].1.get_id();
                Some((&events[i].1, self.references.installs.get(&id)?))
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        for &(_, store) in &installs {
            if !seen.insert(store.destination) {
                continue;
            }
            let writes: Vec<_> = installs.iter().copied().filter(|(_, s)| s.destination == store.destination).collect();
            let mut ordered_writes: Vec<_> = writes.iter().map(|(_, s)| *s).collect();
            ordered_writes.sort_by_key(|s| s.fact.order);
            ordered_writes.dedup_by_key(|s| s.fact.occurrence);
            if ordered_writes.len() < 2
                || !ordered_writes.windows(2).all(|w| w[1].prior == Some(w[0].fact.occurrence))
            {
                continue;
            }
            let column = endpoints(writes[0].0).1;
            if !writes.iter().all(|(event, _)| endpoints(event).1 == column) {
                continue;
            }
            let group = writes[0].0.get_id();
            let loans: BTreeSet<_> =
                writes.iter().flat_map(|(_, s)| s.incoming.unwrap_or_default().iter().chain(s.installed)).copied().collect();
            let mut effects: BTreeMap<_, _> = writes.iter()
                .map(|(event, s)| (event.get_id(), (fact_point(s.fact), s.fact.occurrence))).collect();
            for &i in indices {
                let event = &events[i].1;
                let id = event.get_id();
                if end(event) && endpoints(event).0 == column && self.references.ends.get(&id)
                    .is_some_and(|ends| ends.iter().all(|b| loans.contains(&b.loan)))
                {
                    if let Some(point) = self.holder_end_point(id) { effects.insert(id, point); }
                }
            }

            // A definite release on a named local has a holder even when
            // two successive borrows have identical printed endpoints.
            for &i in indices {
                let event = &events[i].1;
                let id = event.get_id();
                if !effects.contains_key(&id) {
                    continue;
                }
                let Some(ends) = self.references.ends.get(&id) else { continue };
                let Some(holder) = store.destination.as_local() else { continue };
                if ends.iter().any(|b| b.release && !self.facts.direct_releases.contains_key(&b.key())) {
                    layout.uncertain_store_ends.insert(id);
                }
                let [boundary] = ends.as_slice() else { continue };
                let Some(info) = self.loan(boundary.loan) else { continue };
                let (from, to) = endpoints(event);
                if boundary.release
                    && from.extract_rap().and_then(ResourceAccessPoint::ref_local) == Some(holder)
                    && matches!(boundary.fact.end_places.as_slice(), [p] if p.local == holder.index() && p.projection.is_empty())
                    && info.lender_local.is_some()
                    && info.lender_local == to.extract_rap().and_then(ResourceAccessPoint::owner_local)
                    && !self.facts.conditional_ends.contains(&boundary.loan)
                    && info.issuance_path.is_empty()
                    && info.issued_on.is_none()
                    && self.facts.direct_releases.get(&boundary.key()) == Some(&true)
                {
                    layout.available_after_store_end.insert(id, boundary.loan);
                }
            }
            // Every event that shares the stack needs a compiler point.
            // Refuse the split when a sibling path or an unknown operation
            // would make the vertical order speculative.
            let blocks = self.body.basic_blocks.reverse_postorder();
            let mut points = Vec::new();
            for &i in indices {
                let event = &events[i].1;
                let id = event.get_id();
                let point = if let Some(&(point, _)) = effects.get(&id) {
                    Some(point)
                } else if matches!(event, E::PassByMutableReference { .. } | E::PassByStaticReference { .. }) {
                    self.call_input_point(event)
                } else {
                    self.stack_transfer(event)
                };
                if let Some(p) = point { points.push((id, p)); }
            }
            if points.len() != indices.len() { continue; }
            points.sort_by_key(|(_, p)| *p);
            if !points.windows(2).all(|w| w[0].1 == w[1].1 || self.earlier(w[0].1, w[1].1)) { continue; }
            if !points.windows(2).all(|w| {
                let (id, p) = w[0];
                let (next_id, q) = w[1];
                let Some(&(_, occurrence)) = effects.get(&id) else { return true };
                if p == q {
                    return effects.get(&next_id).is_none_or(|(_, other)| occurrence == *other);
                }
                let target = rustc_middle::mir::Location { block: blocks[q.0], statement_index: q.1 };
                occurrence_precedes(self.body, occurrence, target)
            }) { continue; }
            for (id, point) in points {
                layout.stack_points.insert(id, (group, point));
            }
        }
    }

    fn call_input_point(&self, event: &E) -> Option<super::Point> {
        use rustc_middle::mir::{Rvalue, StatementKind, TerminatorKind};
        let span = *self.spans.get(&event.get_id())?;
        let at = self.reference_facts.call(span)?;
        let block = &self.body[at.block];
        let rank = self.body.basic_blocks.reverse_postorder().iter().position(|bb| *bb == at.block)?;
        // A call may take its reference through an unnamed reborrow. The
        // holder supplies the argument before its HoldEnd on that assignment.
        // Only fold adjacent instructions with the very same MIR operand.
        if let Some(stmt) = block.statements.last() {
            if let StatementKind::Assign(assign) = &stmt.kind {
                if let Rvalue::Ref(_, _, borrowed) = assign.1 {
                    let local = endpoints(event).0.extract_rap().and_then(|r| r.ref_local().or_else(|| r.owner_local()));
                    if local == Some(borrowed.local)
                        && matches!(&block.terminator().kind, TerminatorKind::Call { args, .. }
                            if args.iter().any(|arg| arg.span.source_equal(span) && arg.node.place() == Some(assign.0)))
                    {
                        return Some((rank, at.statement_index - 1, 0));
                    }
                }
            }
        }
        Some((rank, at.statement_index, 0))
    }

    /// A displayed transfer can combine several MIR operations (for
    /// example the read and write of a compound assignment). Place it
    /// when its destination is actually assigned.
    fn stack_transfer(&self, event: &E) -> Option<super::Point> {
        use rustc_middle::mir::{Operand, Rvalue, StatementKind, TerminatorKind};
        if let E::GoOutOfScope { ro, .. } = event {
            use crate::facts::Fact;
            let local = ro.owner_local().or_else(|| ro.ref_local()).map(|l| l.index())
                .or_else(|| Some(self.references.slots.get(ro.hash())?.as_ref()?.local))?;
            let ends = self.facts.rows.iter().flat_map(super::normal_facts)
                .filter(|f| f.place_local.is_some_and(|l| l.index() == local));
            let mut drops = ends.clone().filter(|f| matches!(f.fact, Fact::Drop { at_reassign: false, .. }));
            if let Some(drop) = drops.next() {
                return drops.next().is_none().then(|| fact_point(drop));
            }
            let mut dead = ends.filter(|f| matches!(f.fact, Fact::ScopeEnd { .. }));
            let point = fact_point(dead.next()?);
            return dead.all(|f| fact_point(f) == point).then_some(point);
        }
        if !matches!(event, E::Bind { .. } | E::Copy { .. } | E::Move { .. }) { return None; }
        let (from, to) = endpoints(event);
        let local = |ty: &crate::svg_generator::data::ResourceTy| {
            let crate::svg_generator::data::ResourceTy::Value(r) = ty else { return None };
            if r.is_member() { return None; }
            r.ref_local().or_else(|| r.owner_local())
        };
        let dest = local(to);
        let src = local(from);
        let span = self.spans.get(&event.get_id())?;
        let reads = |op: &Operand<'tcx>| src.is_some() && op.place().and_then(|p| p.as_local()) == src;
        let blocks = self.body.basic_blocks.reverse_postorder();
        if let Some(at) = self.reference_facts.call(*span) {
            if let TerminatorKind::Call { args, .. } = &self.body[at.block].terminator().kind {
                if to.extract_rap().is_some_and(ResourceAccessPoint::is_fn)
                    && args.iter().any(|arg| arg.span.source_equal(*span)
                        && (reads(&arg.node) || matches!(arg.node, Operand::Constant(_))))
                {
                    return Some((blocks.iter().position(|bb| *bb == at.block)?, at.statement_index, 0));
                }
            }
        }
        let mut sites = blocks.iter().enumerate().flat_map(|(rank, &bb)| {
            let data = &self.body[bb];
            (0..=data.statements.len()).filter_map(move |i| {
                if data.is_cleanup { return None; }
                let at = rustc_middle::mir::Location { block: bb, statement_index: i };
                let mir = self.body.source_info(at).span;
                if !(span.contains(mir) || mir.contains(*span)) { return None; }
                let matches = if let Some(stmt) = data.statements.get(i) {
                    let StatementKind::Assign(a) = &stmt.kind else { return None };
                    if dest.is_some() {
                        a.0.as_local() == dest
                    } else {
                        match &a.1 {
                            Rvalue::Use(o) | Rvalue::Cast(_, o, _) | Rvalue::UnaryOp(_, o) => reads(o),
                            Rvalue::BinaryOp(_, ops) => reads(&ops.0) || reads(&ops.1),
                            Rvalue::Aggregate(_, ops) => ops.iter().any(reads),
                            _ => false,
                        }
                    }
                } else {
                    matches!(&data.terminator().kind, TerminatorKind::Call { destination, .. }
                        if dest.is_some() && destination.as_local() == dest)
                };
                matches.then_some((rank, i, 0))
            })
        });
        let point = sites.next()?;
        sites.next().is_none().then_some(point)
    }
}
