//! Identify the slot behind a MIR operand, including a field inside a whole
//! aggregate move. Stop when the defining instruction no longer carries the
//! same Polonius loan. In particular, do not search for a previously live
//! binding once an unrelated call, overwrite or storage boundary is reached.

use super::schema::{EndPlace, HolderStep};
use rustc_middle::mir::{
    AggregateKind, Body, Location, Operand, Place, PlaceElem, ProjectionElem, Rvalue, StatementKind, TerminatorKind,
};
use rustc_middle::ty::TyCtxt;
use std::collections::{BTreeSet, HashSet};

pub(super) struct HolderPath<'a, 'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub runs: &'a BTreeSet<rustc_middle::mir::BasicBlock>,
    pub body: &'a Body<'tcx>,
}

/// The displayed slots on the way to the operand, newest first. Each MIR
/// assignment has at most one source for the projected field being traced.
impl<'tcx> HolderPath<'_, 'tcx> {
    fn source(
        &self,
        value: &Rvalue<'tcx>,
        suffix: &[PlaceElem<'tcx>],
        carries: impl Fn(Place<'tcx>) -> bool,
    ) -> Option<Place<'tcx>> {
        let (source, remaining) = match value {
            Rvalue::Use(op) => (op.place()?, suffix),
            Rvalue::Cast(_, op, _) if suffix.is_empty() => (op.place()?, suffix),
            Rvalue::Aggregate(kind, _) if matches!(&**kind, AggregateKind::Adt(id, ..) if self.tcx.adt_def(*id).is_union()) => {
                return None
            }
            Rvalue::Aggregate(kind, ops) => {
                // The projected field is relative to its selected variant.
                let suffix = match suffix {
                    [ProjectionElem::Downcast(_, selected), rest @ ..] => {
                        if !matches!(&**kind, AggregateKind::Adt(_, variant, ..) if selected == variant) { return None; }
                        rest
                    }
                    other => other,
                };
                match suffix.split_first() {
                    Some((ProjectionElem::Field(index, _), rest)) => (ops.get(*index)?.place()?, rest),
                    None => return unique(ops.iter().filter_map(Operand::place).filter(|p| carries(*p))),
                    _ => return None,
                }
            }
            Rvalue::Ref(_, _, borrowed) => {
                // The loan is carried through the reference in `&*m` or
                // `&*h.r`. A non-reborrow such as `&s` starts a new loan.
                let n = borrowed.projection.iter().position(|e| e == ProjectionElem::Deref).unwrap_or(borrowed.projection.len());
                return Some(Place { local: borrowed.local, projection: self.tcx.mk_place_elems(&borrowed.projection[..n]) });
            }
            _ => return None,
        };
        Some(source.project_deeper(remaining, self.tcx))
    }

    /// Trace a single reaching definition. A merge stops the walk.
    /// Cyclic or indirect definitions can leave a prefix unknown.
    pub fn trace(
        &self,
        point: Location,
        place: Place<'tcx>,
        issued: Option<Location>,
        named: &impl Fn(Place<'tcx>) -> Option<EndPlace>,
        carries: &impl Fn(Place<'tcx>, Location) -> bool,
    ) -> Vec<HolderStep> {
        let mut path = Vec::new();
        let mut cursor = (point, place);
        let mut seen = HashSet::new();
        while seen.insert(cursor) {
            let (before, slot) = cursor;
            let name = named(slot);
            let mut definition = None;

            // Only collapse an unambiguous chain of blocks. A branch can
            // assign a different value to this very same local on each leg.
            let mut at = before;
            let source = loop {
                if Some(at) == issued {
                    break None;
                }
                if at.statement_index == 0 {
                    let predecessors = self.body.basic_blocks.predecessors()[at.block].iter().copied().filter(|bb| {
                        self.runs.contains(bb)
                            && !self.body[*bb].is_cleanup
                            && match self.body[*bb].terminator().kind {
                                TerminatorKind::FalseEdge { real_target, .. }
                                | TerminatorKind::FalseUnwind { real_target, .. } => real_target == at.block,
                                _ => true,
                            }
                    });
                    let Some(bb) = unique(predecessors) else { break None };
                    let term = self.body.terminator_loc(bb);
                    if !seen.insert((term, slot)) {
                        break None;
                    }
                    match &self.body[bb].terminator().kind {
                        TerminatorKind::Call { args, destination, target, .. } if destination.local == slot.local => {
                            if *target != Some(at.block) {
                                break None;
                            }
                            definition = Some(term);
                            // A result may keep the arguments' loans. Attribute
                            // it only when a single argument carried this loan.
                            break unique(args.iter().filter_map(|a| a.node.place()).filter(|p| carries(*p, term)))
                                .map(|p| (term, p));
                        }
                        TerminatorKind::InlineAsm { .. } => break None,
                        _ => {}
                    }
                    at = term;
                    continue;
                }
                at.statement_index -= 1;
                match &self.body[at.block].statements[at.statement_index].kind {
                    StatementKind::StorageLive(local) | StatementKind::StorageDead(local) if *local == slot.local => break None,
                    StatementKind::Assign(pair) if pair.0.local == slot.local => {
                        let Some(rest) = slot.projection.as_slice().strip_prefix(pair.0.projection.as_slice()) else {
                            // A partial write can change a whole aggregate.
                            if pair.0.projection.starts_with(slot.projection) {
                                break None;
                            }
                            continue;
                        };
                        definition = Some(at);
                        if Some(at) == issued {
                            break None;
                        }
                        break self.source(&pair.1, rest, |p| carries(p, at)).filter(|p| carries(*p, at)).map(|p| (at, p));
                    }
                    _ => {}
                }
            };
            if let Some(place) = name {
                let assigned_at = definition.map(|at| (at.block.index(), at.statement_index));
                let step = HolderStep { place, assigned_at };
                if path.last() != Some(&step) {
                    path.push(step);
                }
            }
            let Some(source) = source else { break };
            cursor = source;
        }
        path
    }

    /// The value tested by an enum switch. Its payload can still carry a
    /// loan when the discriminant itself (an integer) carries none.
    pub fn discriminant_input(&self, at: Location) -> Option<(Location, Place<'tcx>)> {
        let bb = &self.body[at.block];
        if at.statement_index != bb.statements.len() {
            return None;
        }
        let TerminatorKind::SwitchInt { discr, .. } = &bb.terminator().kind else { return None };
        let discr = discr.place()?;
        for (i, st) in bb.statements.iter().enumerate().rev() {
            match &st.kind {
                StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if *l == discr.local => return None,
                StatementKind::Assign(pair) if pair.0.local == discr.local => {
                    let Rvalue::Discriminant(input) = pair.1 else { return None };
                    return (pair.0 == discr).then_some((Location { block: at.block, statement_index: i }, input));
                }
                _ => {}
            }
        }
        None
    }

    /// A switch target can be shared by several variants (including the
    /// otherwise target). Exclude the payload only when *every* variant
    /// selecting this actual successor is fieldless.
    pub fn fieldless_operand(&self, at: Location, to: Location) -> Option<Place<'tcx>> {
        if to.statement_index != 0 { return None; }
        let (read, input) = self.discriminant_input(at)?;
        // The switch tests the saved integer. A later write could have
        // replaced the operand whose discriminant was read.
        if self.body[at.block].statements[read.statement_index + 1..].iter().any(|st| match st.kind {
            StatementKind::Nop => false,
            StatementKind::StorageLive(l) | StatementKind::StorageDead(l) => l == input.local,
            _ => true,
        }) { return None; }
        let ty = input.ty(&self.body.local_decls, self.tcx).ty;
        let def = ty.ty_adt_def().filter(|def| def.is_enum())?;
        let TerminatorKind::SwitchInt { targets, .. } = &self.body[at.block].terminator().kind else { return None };
        let mut selected = def.discriminants(self.tcx).filter(|(_, d)| targets.target_for_value(d.val) == to.block).peekable();
        selected.peek()?;
        selected.all(|(variant, _)| def.variant(variant).fields.is_empty()).then_some(input)
    }

    pub fn call_args(&self, at: Location) -> impl Iterator<Item = Place<'tcx>> + '_ {
        let bb = &self.body[at.block];
        let args = match &bb.terminator().kind {
            TerminatorKind::Call { args, .. } if at.statement_index == bb.statements.len() => &args[..],
            _ => &[],
        };
        args.iter().filter_map(|arg| Operand::place(&arg.node))
    }
}

fn unique<T>(mut candidates: impl Iterator<Item = T>) -> Option<T> {
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first)
}
