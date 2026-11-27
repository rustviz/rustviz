//! Direct reference values in MIR storage. A loan in an origin is a
//! borrow-checking restriction. It does not identify the reference in a
//! field when the field and its siblings share that origin.

use super::body::decl_place_ty;
use super::holder_path::HolderPath;
use super::places::place_under;
use super::repoints::IncomingStores;
use super::schema::Occurrence;
use rustc_abi::FieldIdx;
use rustc_middle::mir::{AggregateKind, Body, Location, Place, ProjectionElem, Rvalue, StatementKind};
use rustc_middle::ty::{Ty, TyCtxt, TyKind};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// The installed value and the occurrence that installed it. Two writes
/// of the same reference have different occurrences.
pub(super) struct Installation<'tcx> {
    pub place: Place<'tcx>,
    pub occurrence: Occurrence,
    pub complete: bool,
    pub retained: BTreeMap<usize, bool>,
    pub loans: BTreeSet<usize>,
}

#[derive(Default)]
pub(super) struct Origins<'tcx> {
    pub loans: BTreeSet<usize>,
    pub retained: BTreeMap<usize, bool>,
    pub unknown: bool,
    /// An actual reference was issued with no pictured lender, such as
    /// a reference made through a raw pointer.
    pub unpictured: bool,
    /// A call can carry loans through its result as far as borrowck is
    /// concerned. Its signature alone does not prove its returned value.
    pub calls: Vec<(Location, Place<'tcx>)>,
}

/// An inserted reborrow can preserve a restriction. When it is lowered
/// through a temporary, the drawn slot can also represent the staged source.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum Path {
    Value,
    NamedReborrow,
    StagedReborrow,
}

impl Origins<'_> {
    fn record(&mut self, id: usize, path: Path) {
        match path {
            Path::Value => {
                self.loans.insert(id);
            }
            Path::NamedReborrow | Path::StagedReborrow => {
                *self.retained.entry(id).or_default() |= path == Path::StagedReborrow;
            }
        }
    }
}

pub(super) struct DirectHolders<'a, 'tcx> {
    pub stores: &'a IncomingStores<'a, 'tcx>,
    pub paths: &'a HolderPath<'a, 'tcx>,
    pub tracked: usize,
    /// Tracked borrows, untracked borrows/constants, and caller references.
    /// An aggregate can issue several constant loans at one instruction.
    pub issues: BTreeMap<Option<Location>, Vec<(Place<'tcx>, usize)>>,
}

pub(super) fn element<'tcx>(tcx: TyCtxt<'tcx>, place: Place<'tcx>, offset: u64, len: u64) -> Place<'tcx> {
    tcx.mk_place_elem(place, ProjectionElem::ConstantIndex { offset, min_length: len, from_end: false })
}

/// The place written by an aggregate operand. A union's active field
/// cannot be treated as one member of a tree of disjoint storage slots.
pub(super) fn operand_place<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    place: Place<'tcx>,
    kind: &AggregateKind<'tcx>,
    f: FieldIdx,
) -> Option<Place<'tcx>> {
    let ty = decl_place_ty(tcx, body, place);
    match (kind, ty.kind()) {
        (AggregateKind::Tuple, TyKind::Tuple(tys)) => Some(tcx.mk_place_field(place, f, *tys.get(f.index())?)),
        (AggregateKind::Adt(_, variant, ..), TyKind::Adt(def, args)) if !def.is_union() => {
            let v = def.variant(*variant);
            let base = if def.is_enum() { tcx.mk_place_downcast(place, *def, *variant) } else { place };
            Some(tcx.mk_place_field(base, f, v.fields.get(f)?.ty(tcx, args)))
        }
        (AggregateKind::Array(_), TyKind::Array(_, len)) => {
            Some(element(tcx, place, f.index() as u64, len.try_to_target_usize(tcx)?))
        }
        _ => None,
    }
}

/// Visit the reference slots of a value stored as an aggregate. Arrays
/// only need slots constructed or selected in this MIR body. In particular a
/// repeat expression can have a very large length and only one operand.
pub(super) fn reference_slots<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    place: Place<'tcx>,
    offsets: &BTreeMap<u64, BTreeSet<u64>>,
    out: &mut Vec<Place<'tcx>>,
) {
    fn visit<'tcx>(
        tcx: TyCtxt<'tcx>,
        place: Place<'tcx>,
        ty: Ty<'tcx>,
        offsets: &BTreeMap<u64, BTreeSet<u64>>,
        parents: &mut Vec<Ty<'tcx>>,
        out: &mut Vec<Place<'tcx>>,
    ) {
        if ty.is_ref() {
            out.push(place);
            return;
        }
        if parents.contains(&ty) {
            return;
        }
        parents.push(ty);
        match ty.kind() {
            TyKind::Tuple(fields) => {
                for (f, ty) in fields.iter().enumerate() {
                    visit(tcx, tcx.mk_place_field(place, FieldIdx::from_usize(f), ty), ty, offsets, parents, out);
                }
            }
            TyKind::Adt(def, args) if !def.is_union() && !def.is_box() => {
                for (v, variant) in def.variants().iter_enumerated() {
                    let base = if def.is_enum() { tcx.mk_place_downcast(place, *def, v) } else { place };
                    for (f, field) in variant.fields.iter_enumerated() {
                        let ty = field.ty(tcx, args);
                        visit(tcx, tcx.mk_place_field(base, f, ty), ty, offsets, parents, out);
                    }
                }
            }
            TyKind::Array(ty, len) => {
                if let Some(len) = len.try_to_target_usize(tcx) {
                    for &offset in offsets.get(&len).into_iter().flatten() {
                        visit(tcx, element(tcx, place, offset, len), *ty, offsets, parents, out);
                    }
                }
            }
            _ => {}
        }
        parents.pop();
    }
    visit(tcx, place, decl_place_ty(tcx, body, place), offsets, &mut Vec::new(), out);
}

impl<'tcx> DirectHolders<'_, 'tcx> {
    fn issue(&self, at: Option<Location>, slot: Place<'tcx>) -> Option<usize> {
        self.issues
            .get(&at)?
            .iter()
            .find_map(|&(p, id)| (p.projection.len() == slot.projection.len() && place_under(slot, p)).then_some(id))
    }

    /// Read the reference immediately before an instruction. Follow
    /// each reaching value through copies/moves, including operands of
    /// whole-aggregate stores. Stop at calls and indirect writes.
    pub fn before(&self, at: Location, place: Place<'tcx>) -> Origins<'tcx> {
        self.resolve(vec![(at, place, None, Path::Value)])
    }

    pub fn installed(&self, at: Location, place: Place<'tcx>) -> Origins<'tcx> {
        self.resolve(vec![(at, place, Some(Occurrence { at, exit: None, successor: None }), Path::Value)])
    }

    pub fn reaching(&self, sites: &[Occurrence], place: Place<'tcx>) -> Origins<'tcx> {
        self.resolve(sites.iter().map(|&site| (site.at, place, Some(site), Path::Value)).collect())
    }

    fn resolve(&self, mut work: Vec<(Location, Place<'tcx>, Option<Occurrence>, Path)>) -> Origins<'tcx> {
        let IncomingStores { tcx, body, .. } = *self.stores;
        let mut out = Origins::default();
        let mut seen = HashSet::new();
        while let Some((at, slot, site, path)) = work.pop() {
            let value_path = path == Path::Value;
            let Some(slot) = self.stores.slot_before(at, slot) else {
                out.unknown |= value_path;
                continue;
            };
            if !seen.insert((at, slot, site, path)) {
                continue;
            }
            let Some(site) = site else {
                let incoming = self.stores.definitions(at, slot);
                out.unknown |= incoming.unknown && value_path;
                if incoming.argument {
                    if let Some(id) = self.issue(None, slot) {
                        out.record(id, path);
                    } else {
                        out.unknown |= value_path;
                    }
                }
                work.extend(incoming.sites.into_iter().map(|site| (site.at, slot, Some(site), path)));
                continue;
            };
            if let Some(next) = site.successor {
                if !value_path {
                    continue;
                }
                // For variant results, keep the borrowck candidates used
                // by the call/variant renderer. They do not prove a lender.
                if slot.projection.is_empty()
                    || slot.projection.iter().any(|p| matches!(p, ProjectionElem::Downcast(..)))
                {
                    out.calls.push((next, slot));
                } else {
                    out.unknown = true;
                }
                continue;
            }
            let StatementKind::Assign(pair) = &body[at.block].statements[at.statement_index].kind else { continue };
            let (dst, value) = &**pair;
            let suffix = &slot.projection[dst.projection.len()..];
            let reborrow = match value {
                Rvalue::Ref(_, _, borrowed) if suffix.is_empty() => {
                    borrowed.projection.iter().position(|p| p == ProjectionElem::Deref).and_then(|n| {
                        let source =
                            Place { local: borrowed.local, projection: tcx.mk_place_elems(&borrowed.projection[..n]) };
                        decl_place_ty(tcx, body, source)
                            .is_ref()
                            .then_some((source, n + 1 == borrowed.projection.len()))
                    })
                }
                _ => None,
            };
            let retained = |source: Place<'tcx>| {
                if path == Path::StagedReborrow || !body.local_decls[source.local].is_user_variable() {
                    Path::StagedReborrow
                } else {
                    Path::NamedReborrow
                }
            };
            if let Some(id) = self.issue(Some(at), slot) {
                out.record(id, path);
                if id >= self.tracked {
                    work.extend(reborrow.map(|(p, _)| (at, p, None, retained(p))));
                }
            } else if let Some((p, whole)) = reborrow {
                // Borrowck can omit a reborrow of the whole pointee of a
                // staged reference. A projection can only retain its loan.
                out.unpictured |= value_path && !whole;
                work.push((at, p, None, if whole { path } else { retained(p) }));
            } else if let Some(src) = self.paths.direct_source(value, suffix) {
                work.push((at, src, None, path));
            } else {
                if matches!(value, Rvalue::Ref(..)) && suffix.is_empty() {
                    out.unpictured |= value_path;
                    continue;
                }
                let different_variant = matches!((value, suffix.first()),
                    (Rvalue::Aggregate(kind, _), Some(ProjectionElem::Downcast(_, selected)))
                        if matches!(&**kind, AggregateKind::Adt(_, variant, ..) if selected != variant));
                out.unknown |= value_path && !different_variant;
            }
        }
        out
    }
}

/// Positions exposed by constructors and by the MIR's own uses of places.
pub(super) fn array_offsets<'tcx>(stores: &IncomingStores<'_, 'tcx>) -> BTreeMap<u64, BTreeSet<u64>> {
    use rustc_middle::mir::visit::{PlaceContext, Visitor};

    struct Arrays<'a, 'tcx> {
        stores: &'a IncomingStores<'a, 'tcx>,
        offsets: BTreeMap<u64, BTreeSet<u64>>,
    }
    impl<'tcx> Visitor<'tcx> for Arrays<'_, 'tcx> {
        fn visit_place(&mut self, place: &Place<'tcx>, _: PlaceContext, at: Location) {
            if !place
                .projection
                .iter()
                .any(|p| matches!(p, ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. }))
            {
                return;
            }
            let Some(place) = self.stores.slot_before(at, *place) else { return };
            for (_, step) in place.iter_projections() {
                if let ProjectionElem::ConstantIndex { offset, min_length, .. } = step {
                    self.offsets.entry(min_length).or_default().insert(offset);
                }
            }
        }

        fn visit_assign(&mut self, place: &Place<'tcx>, value: &Rvalue<'tcx>, at: Location) {
            let count = match value {
                Rvalue::Aggregate(kind, ops) if matches!(**kind, AggregateKind::Array(_)) => ops.len() as u64,
                Rvalue::Repeat(..) => 1,
                _ => 0,
            };
            if count != 0 {
                let ty = decl_place_ty(self.stores.tcx, self.stores.body, *place);
                if let TyKind::Array(_, len) = ty.kind() {
                    if let Some(len) = len.try_to_target_usize(self.stores.tcx) {
                        self.offsets.entry(len).or_default().extend(0..len.min(count));
                    }
                }
            }
            self.super_assign(place, value, at);
        }
    }
    let mut arrays = Arrays { stores, offsets: BTreeMap::new() };
    arrays.visit_body(stores.body);
    arrays.offsets
}
