//! Calls, closure environments, and returned values, identified by MIR
//! occurrence. Signature-based restrictions are separate from value flow.
use super::BodyBindings;
use super::call_wording::operand_borrow;
use super::rows::real_successor_blocks;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::visit::{MutatingUseContext, NonMutatingUseContext, PlaceContext, Visitor};
use rustc_middle::mir::{
    AggregateKind, BasicBlock, Body, BorrowKind, Location, Operand, Place, ProjectionElem, RETURN_PLACE, Rvalue,
    START_BLOCK, StatementKind, TerminatorKind,
};
use rustc_middle::ty::{TyCtxt, TyKind};
use rustc_span::sym;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CallKind {
    Direct,
    Indirect,
    Fn,
    FnMut,
    FnOnce,
}

impl CallKind {
    pub fn is_callable(self) -> bool {
        matches!(self, Self::Fn | Self::FnMut | Self::FnOnce)
    }
}

pub(crate) fn kind(tcx: TyCtxt<'_>, method: Option<DefId>) -> CallKind {
    let Some(method) = method else { return CallKind::Indirect };
    let lang = tcx.lang_items();
    match tcx.trait_of_assoc(method) {
        Some(id) if Some(id) == lang.fn_trait() => CallKind::Fn,
        Some(id) if Some(id) == lang.fn_mut_trait() => CallKind::FnMut,
        Some(id) if Some(id) == lang.fn_once_trait() => CallKind::FnOnce,
        _ => CallKind::Direct,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CaptureTransfer {
    Copy,
    Move,
    Shared,
    Mutable,
    Unique,
    Unknown,
}

#[derive(PartialEq, Eq, Serialize)]
pub(crate) struct CapturePlace {
    local: usize,
    projection: Vec<Projection>,
}

#[derive(Serialize)]
pub(crate) struct Capture {
    #[serde(serialize_with = "super::schema::location")]
    pub aggregate: Location,
    pub row: usize,
    #[serde(skip)]
    pub(crate) closure: DefId,
    closure_body: String,
    pub field: usize,
    pub transfer: CaptureTransfer,
    pub source: Option<String>,
    pub source_place: Option<CapturePlace>,
    #[serde(serialize_with = "super::schema::optional_location")]
    pub issue: Option<Location>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureFate {
    Dropped,
    Returned,
    Transferred,
    ScopeEnd,
    NoExit,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Projection {
    Deref,
    Field(usize),
    Variant(usize),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ReturnSource {
    Input {
        index: usize,
        projection: Vec<Projection>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        string_contents: bool,
    },
    Capture {
        field: usize,
        projection: Vec<Projection>,
    },
    Constant,
    /// This return reaches a different variant, with no value at this path.
    Absent,
    Unknown,
}

#[derive(Clone, Serialize)]
pub(crate) struct ReturnPayload {
    pub projection: Vec<Projection>,
    /// Presentation label from the variant's definition, when this path
    /// names a reference in an enum's payload.
    pub variant: String,
    pub sources: BTreeSet<ReturnSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lender: Option<super::EndPlace>,
}

#[derive(Serialize)]
pub(crate) struct CallRelationship {
    #[serde(serialize_with = "super::schema::location")]
    pub location: Location,
    pub row: usize,
    pub kind: CallKind,
    pub can_return: bool,
    callee: Option<String>,
    pub return_sources: Option<BTreeSet<ReturnSource>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub return_payloads: Vec<ReturnPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_lender: Option<super::EndPlace>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub holders: Option<super::CallHolders>,
    pub capture_fates: Vec<BTreeSet<CaptureFate>>,
    #[serde(skip)]
    pub borrowed_captures: BTreeSet<usize>,
    #[serde(skip)]
    pub retained_captures: std::collections::HashSet<DefId>,
    #[serde(skip)]
    pub receiver_loans: BTreeSet<usize>,
    #[serde(skip)]
    pub receiver_borrow_ends: bool,
}

#[derive(Default, Serialize)]
pub(crate) struct CallRelationships {
    pub calls: Vec<CallRelationship>,
    pub captures: Vec<Capture>,
}

impl CallRelationships {
    pub fn call(&self, at: Location) -> Option<&CallRelationship> {
        self.calls.binary_search_by_key(&at, |call| call.location).ok().map(|i| &self.calls[i])
    }

    pub fn captures(&self, closure: DefId, field: usize) -> impl Iterator<Item = &Capture> {
        self.captures.iter().filter(move |c| c.closure == closure && c.field == field)
    }

    pub fn is_empty(&self) -> bool {
        self.calls.is_empty() && self.captures.is_empty()
    }
}

fn normal_successors(body: &Body<'_>, bb: BasicBlock) -> Vec<BasicBlock> {
    let mut successors = real_successor_blocks(body, bb);
    let block = &body[bb];
    if let TerminatorKind::SwitchInt { discr, targets } = &block.terminator().kind {
        // Lowering often stages a literal directly before its switch.
        let operand = match block.statements.last().map(|st| &st.kind) {
            Some(StatementKind::Assign(a)) if discr.place() == Some(a.0) => match &a.1 {
                Rvalue::Use(op) => op,
                _ => discr,
            },
            _ => discr,
        };
        if let Operand::Constant(c) = operand {
            if let Some(value) = c.const_.try_to_scalar_int() {
                successors.retain(|&bb| bb == targets.target_for_value(value.to_bits(value.size())));
            }
        }
    }
    successors
}

fn normal_blocks(body: &Body<'_>) -> BTreeSet<BasicBlock> {
    let mut blocks = BTreeSet::new();
    let mut pending = vec![START_BLOCK];
    while let Some(bb) = pending.pop() {
        if blocks.insert(bb) {
            pending.extend(normal_successors(body, bb));
        }
    }
    blocks
}

pub(super) fn collect<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    bindings: &BodyBindings,
    runs: &BTreeSet<BasicBlock>,
    checked: &BTreeSet<BasicBlock>,
    rows: &BTreeMap<Location, usize>,
) -> CallRelationships {
    let mut facts = CallRelationships::default();
    let mut summaries = rustc_data_structures::fx::FxHashMap::default();
    let row = |at| {
        rows.get(&at).copied().unwrap_or_else(|| {
            tcx.sess.source_map().lookup_char_pos(body.source_info(at).span.source_callsite().lo()).line
        })
    };
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        if !checked.contains(&bb) {
            continue;
        }
        for (statement_index, st) in data.statements.iter().enumerate() {
            let StatementKind::Assign(a) = &st.kind else { continue };
            let Rvalue::Aggregate(kind, fields) = &a.1 else { continue };
            let AggregateKind::Closure(closure, _) = **kind else { continue };
            let Some(local) = closure.as_local() else { continue };
            let captures = tcx.typeck(local).closure_min_captures_flattened(local);
            let at = Location { block: bb, statement_index };
            // The compiler span starts where the environment is created
            // in the drawing, even when the closure body spans lines.
            let row = tcx.sess.source_map().lookup_char_pos(st.source_info.span.source_callsite().lo()).line;
            for (field, (operand, capture)) in fields.iter().zip(captures).enumerate() {
                let (transfer, issue, source) =
                    if let rustc_middle::ty::UpvarCapture::ByRef(kind) = capture.info.capture_kind {
                        use rustc_middle::ty::BorrowKind;
                        let transfer = match kind {
                            BorrowKind::Immutable => CaptureTransfer::Shared,
                            BorrowKind::UniqueImmutable => CaptureTransfer::Unique,
                            BorrowKind::Mutable => CaptureTransfer::Mutable,
                        };
                        let borrow = operand.place().and_then(|place| operand_borrow(body, at, place));
                        (transfer, borrow.map(|(issue, _)| issue), borrow.map(|(_, source)| source))
                    } else {
                        (
                            match operand {
                                Operand::Copy(_) => CaptureTransfer::Copy,
                                Operand::Move(_) => CaptureTransfer::Move,
                                _ => CaptureTransfer::Unknown,
                            },
                            None,
                            operand.place(),
                        )
                    };
                let source_place = source.and_then(|place| {
                    Some(CapturePlace { local: place.local.index(), projection: projection(&place.projection)? })
                });
                let source = source.and_then(|place| super::call_wording::binding_place(body, bindings, place));
                facts.captures.push(Capture {
                    aggregate: at,
                    row,
                    closure,
                    closure_body: tcx.def_path_str(closure),
                    field,
                    transfer,
                    source,
                    source_place,
                    issue,
                });
            }
        }
        if !runs.contains(&bb) {
            continue;
        }
        let TerminatorKind::Call { func, args, destination, target, .. } = &data.terminator().kind else { continue };
        let direct = func.const_fn_def();
        let kind = kind(tcx, direct.map(|(def, _)| def));
        let closure = direct.filter(|_| kind.is_callable()).and_then(|(_, args)| match args.types().next()?.kind() {
            TyKind::Closure(id, _) => Some(*id),
            _ => None,
        });
        let callee = closure.or_else(|| direct.map(|(def, _)| def));
        // A trait declaration can have a body while this invocation selects
        // a different implementation. Only direct bodies and concrete
        // closure environments are summarized.
        let local = closure
            .or_else(|| callee.filter(|def| tcx.trait_of_assoc(*def).is_none()))
            .and_then(DefId::as_local)
            .filter(|id| tcx.hir_maybe_body_owned_by(*id).is_some());
        let (return_sources, mut return_payloads, (capture_fates, borrowed_captures)) = local
            .map(|id| {
                summaries
                    .entry((id, kind == CallKind::FnOnce))
                    .or_insert_with(|| {
                        let callee = &rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, id).body;
                        let blocks = normal_blocks(callee);
                        let sources = return_sources(tcx, callee, &blocks, None, Place::from(RETURN_PLACE));
                        let payloads = return_payloads(tcx, callee, &blocks);
                        let fates = if closure.is_some() && kind == CallKind::FnOnce {
                            capture_fates(tcx, callee, &blocks)
                        } else {
                            Default::default()
                        };
                        (sources, payloads, fates)
                    })
                    .clone()
            })
            .unwrap_or_default();
        let location = body.terminator_loc(bb);
        let lender = |sources: &BTreeSet<ReturnSource>, result: Place<'tcx>| {
            let mut present = sources.iter().filter(|source| **source != ReturnSource::Absent);
            let ReturnSource::Input { index, projection, string_contents } = present.next()? else { return None };
            if present.next().is_some()
                || !matches!(projection.as_slice(), [] | [Projection::Deref])
                || kind != CallKind::Direct
            {
                return None;
            }
            let mut arg = args.get(*index)?.node.place()?;
            if !*string_contents && !projection.is_empty() {
                let referent = |place: Place<'tcx>| place.ty(&body.local_decls, tcx).ty.builtin_deref(true)
                    .map(|ty| tcx.erase_and_anonymize_regions(ty));
                // Deref can reborrow the referent or load a reference
                // stored in it. Only the first points into the argument.
                if referent(arg) != referent(result) { return None; }
            }
            let mut at = location;
            let place = loop {
                let (issue, place) = operand_borrow(body, at, arg)?;
                if !place.is_indirect() { break place; }
                if place.projection.as_slice() != [ProjectionElem::Deref] { return None; }
                arg = Place::from(place.local);
                at = issue;
            };
            let name = super::call_wording::binding_place(body, bindings, place)?;
            Some(super::EndPlace::new(tcx, place, name))
        };
        let return_lender = if destination.ty(&body.local_decls, tcx).ty.is_ref() {
            return_sources.as_ref().and_then(|sources| lender(sources, *destination))
        } else {
            None
        };
        for payload in &mut return_payloads {
            payload.lender = project(tcx, body, *destination, &payload.projection)
                .and_then(|place| lender(&payload.sources, place));
        }
        facts.calls.push(CallRelationship {
            location,
            row: row(location),
            kind,
            can_return: target.is_some() && (local.is_none() || return_sources.is_some()),
            callee: callee.map(|id| tcx.def_path_str(id)),
            return_sources: target.and(return_sources),
            return_payloads: if target.is_some() { return_payloads } else { Vec::new() },
            return_lender: target.and(return_lender),
            holders: None,
            capture_fates,
            borrowed_captures,
            retained_captures: Default::default(),
            receiver_loans: BTreeSet::new(),
            receiver_borrow_ends: false,
        });
    }
    facts
}

pub(super) fn projection(steps: &[rustc_middle::mir::PlaceElem<'_>]) -> Option<Vec<Projection>> {
    steps.iter().map(|e| match e {
            ProjectionElem::Deref => Some(Projection::Deref),
            ProjectionElem::Field(field, _) => Some(Projection::Field(field.index())),
            ProjectionElem::Downcast(_, variant) => Some(Projection::Variant(variant.index())),
            _ => None,
        })
        .collect()
}

pub(super) fn project<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    mut place: Place<'tcx>,
    steps: &[Projection],
) -> Option<Place<'tcx>> {
    for step in steps {
        let ty = place.ty(&body.local_decls, tcx);
        let elem = match *step {
            Projection::Deref if ty.ty.builtin_deref(true).is_some() => ProjectionElem::Deref,
            Projection::Variant(i) => {
                let variant = ty.ty.ty_adt_def()?.variants().get(rustc_abi::VariantIdx::from_usize(i))?;
                ProjectionElem::Downcast(Some(variant.name), i.into())
            }
            Projection::Field(i) => {
                let field = match ty.ty.kind() {
                    TyKind::Adt(def, args) => def.variant(ty.variant_index.unwrap_or(rustc_abi::FIRST_VARIANT))
                        .fields.get(rustc_abi::FieldIdx::from_usize(i))?.ty(tcx, args),
                    TyKind::Tuple(fields) => fields.get(i).copied()?,
                    _ => return None,
                };
                ProjectionElem::Field(i.into(), field)
            }
            _ => return None,
        };
        place = place.project_deeper(&[elem], tcx);
    }
    Some(place)
}

/// Reference leaves in enum payloads. The compiler's variant and field
/// indices identify the slot even when different variants use the same
/// field name and lifetime.
fn return_payloads<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    blocks: &BTreeSet<BasicBlock>,
) -> Vec<ReturnPayload> {
    let TyKind::Adt(def, args) = *body.return_ty().kind() else { return Vec::new() };
    if !def.is_enum() { return Vec::new(); }
    let mut work = Vec::new();
    for (index, variant) in def.variants().iter_enumerated().rev() {
        let base = Place::from(RETURN_PLACE).project_deeper(&[ProjectionElem::Downcast(Some(variant.name), index)], tcx);
        for (field, ty) in variant.fields.iter_enumerated().rev() {
            work.push((base.project_deeper(&[ProjectionElem::Field(field, ty.ty(tcx, args))], tcx), variant.name.to_string()));
        }
    }
    let mut results = Vec::new();
    while let Some((place, variant)) = work.pop() {
        match place.ty(&body.local_decls, tcx).ty.kind() {
            TyKind::Ref(..) => {
                if let Some(sources) = return_sources(tcx, body, blocks, None, place) {
                    results.push(ReturnPayload { projection: projection(&place.projection).unwrap(), variant, sources, lender: None });
                }
            }
            TyKind::Tuple(fields) => {
                for (i, ty) in fields.iter().enumerate().rev() {
                    work.push((place.project_deeper(&[ProjectionElem::Field(i.into(), ty)], tcx), variant.clone()));
                }
            }
            TyKind::Adt(def, args) if def.is_struct() => {
                for (i, field) in def.non_enum_variant().fields.iter_enumerated().rev() {
                    work.push((place.project_deeper(&[ProjectionElem::Field(i, field.ty(tcx, args))], tcx), variant.clone()));
                }
            }
            _ => {}
        }
    }
    results
}

fn starts_with<'tcx>(place: Place<'tcx>, prefix: Place<'tcx>) -> bool {
    place.local == prefix.local && place.projection.starts_with(prefix.projection.as_slice())
}

/// Writable pointer operands can expose a slot even if this body never
/// forms its address. Include their reaching copies and moves.
fn writable_arguments<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    blocks: &BTreeSet<BasicBlock>,
    at: Location,
    args: impl Iterator<Item = Place<'tcx>>,
) -> Vec<Place<'tcx>> {
    let mut arguments: Vec<_> = args.collect();
    let mut work = Vec::new();
    while let Some(place) = arguments.pop() {
        match place.ty(&body.local_decls, tcx).ty.kind() {
            TyKind::RawPtr(..) | TyKind::Ref(_, _, rustc_hir::Mutability::Mut) => work.push((at, place)),
            // Fn, FnMut and FnOnce bundle the explicit operands in a tuple.
            TyKind::Tuple(fields) => arguments.extend(fields.iter().enumerate().map(|(i, ty)| {
                place.project_deeper(&[ProjectionElem::Field(rustc_abi::FieldIdx::from_usize(i), ty)], tcx)
            })),
            _ => {}
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut places = Vec::new();
    while let Some((at, place)) = work.pop() {
        if !seen.insert((at, place)) || place.projection.len() > 32 {
            continue;
        }
        places.push(place.project_deeper(&[ProjectionElem::Deref], tcx));
        if let Some((i, assign)) = body[at.block].statements[..at.statement_index].iter().enumerate().rev().find_map(|(i, st)| {
            let StatementKind::Assign(a) = &st.kind else { return None };
            (starts_with(place, a.0) || starts_with(a.0, place)).then_some((i, a))
        }) {
            if starts_with(place, assign.0) {
                let suffix = &place.projection[assign.0.projection.len()..];
                let source = match &assign.1 {
                    Rvalue::Use(operand) => operand.place().map(|p| p.project_deeper(suffix, tcx)),
                    Rvalue::Aggregate(kind, fields) if matches!(**kind, AggregateKind::Tuple) => {
                        match suffix {
                            [ProjectionElem::Field(field, _), rest @ ..] =>
                                fields[*field].place().map(|p| p.project_deeper(rest, tcx)),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(source) = source {
                    work.push((Location { statement_index: i, ..at }, source));
                }
            }
            continue;
        }
        work.extend(body.basic_blocks.predecessors()[at.block].iter().copied()
            .filter(|pred| blocks.contains(pred) && normal_successors(body, *pred).contains(&at.block))
            .map(|pred| (body.terminator_loc(pred), place)));
    }
    places
}

fn may_write<'tcx>(place: Place<'tcx>, exposed: Place<'tcx>, address: bool) -> bool {
    if starts_with(place, exposed) {
        !address || place.projection[exposed.projection.len()..].contains(&ProjectionElem::Deref)
    } else {
        !address && starts_with(exposed, place)
            && !exposed.projection[place.projection.len()..].contains(&ProjectionElem::Deref)
    }
}

/// Trace the value at each normal return back to a reaching definition.
/// Keep the place suffix while following temporaries and reborrows. An
/// unknown path remains in the result alongside any known alternatives.
fn return_sources<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    blocks: &BTreeSet<BasicBlock>,
    span: Option<rustc_span::Span>,
    result: Place<'tcx>,
) -> Option<BTreeSet<ReturnSource>> {
    let mut work: Vec<_> = blocks
        .iter()
        .filter(|&&bb| matches!(body[bb].terminator().kind, TerminatorKind::Return))
        .map(|&bb| (body.terminator_loc(bb), result, span.is_none(), false, false))
        .collect();
    let exposed: Vec<_> = blocks
        .iter()
        .flat_map(|bb| &body[*bb].statements)
        .filter_map(|st| {
            let StatementKind::Assign(a) = &st.kind else { return None };
            match a.1 {
                Rvalue::Ref(_, BorrowKind::Mut { .. }, p) | Rvalue::RawPtr(_, p) => Some(p),
                _ => None,
            }
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut sources = BTreeSet::new();
    if blocks.iter().any(|&bb| matches!(body[bb].terminator().kind, TerminatorKind::TailCall { .. })) {
        sources.insert(ReturnSource::Unknown);
    }
    let mut call_writes = BTreeMap::new();
    'trace: while let Some((at, mut place, mut selected, string_contents, mut address)) = work.pop() {
        if !seen.insert((at, place, selected, string_contents, address)) {
            continue;
        }
        // A loop that follows a recursive projection need not reach a
        // fixed place. Stop when the path exceeds the supported depth.
        if place.projection.len() > 32 {
            sources.insert(ReturnSource::Unknown);
            continue;
        }
        for st in body[at.block].statements[..at.statement_index].iter().rev() {
            let StatementKind::Assign(a) = &st.kind else { continue };
            if !starts_with(place, a.0) {
                if !address && starts_with(a.0, place) {
                    sources.insert(ReturnSource::Unknown);
                    continue 'trace;
                }
                continue;
            }
            let suffix = &place.projection[a.0.projection.len()..];
            // A store can change the value at this address. It changes
            // its provenance only if the address loads a pointer there.
            if address && !suffix.contains(&ProjectionElem::Deref) {
                continue;
            }
            selected |= span.is_some_and(|s| st.source_info.span.source_equal(s));
            place = match a.1 {
                Rvalue::Use(Operand::Copy(p) | Operand::Move(p)) => p.project_deeper(suffix, tcx),
                Rvalue::Aggregate(ref kind, ref operands) if !suffix.is_empty() => {
                    let fields = match (&**kind, suffix) {
                        (AggregateKind::Adt(def, actual, ..), [ProjectionElem::Downcast(_, variant), rest @ ..])
                            if tcx.adt_def(*def).is_enum() =>
                        {
                            if actual != variant {
                                sources.insert(ReturnSource::Absent);
                                continue 'trace;
                            }
                            rest
                        }
                        (AggregateKind::Adt(def, ..), rest) if tcx.adt_def(*def).is_struct() => rest,
                        (AggregateKind::Tuple, rest) => rest,
                        _ => &[],
                    };
                    let operand = match fields {
                        [ProjectionElem::Field(field, _), rest @ ..] => operands.get(*field).map(|op| (op, rest)),
                        _ => None,
                    };
                    match operand {
                        Some((op, rest)) if op.place().is_some() => op.place().unwrap().project_deeper(rest, tcx),
                        Some((Operand::Constant(_), [])) => {
                            sources.insert(ReturnSource::Constant);
                            continue 'trace;
                        }
                        _ => {
                            sources.insert(ReturnSource::Unknown);
                            continue 'trace;
                        }
                    }
                }
                Rvalue::Ref(_, _, p) if suffix.is_empty() => {
                    address = true;
                    p
                }
                Rvalue::Ref(_, _, p) if suffix.first() == Some(&ProjectionElem::Deref) => {
                    p.project_deeper(&suffix[1..], tcx)
                }
                Rvalue::Use(Operand::Constant(ref c))
                    if suffix.is_empty() || address && suffix == [ProjectionElem::Deref] && c.ty().is_ref() =>
                {
                    if selected {
                        sources.insert(ReturnSource::Constant);
                    }
                    continue 'trace;
                }
                _ => {
                    sources.insert(ReturnSource::Unknown);
                    continue 'trace;
                }
            };
            if exposed.iter().any(|&p| may_write(place, p, address)) {
                sources.insert(ReturnSource::Unknown);
                continue 'trace;
            }
        }
        if at.block == START_BLOCK && selected {
            let source = body
                .args_iter()
                .position(|l| l == place.local)
                .zip(projection(&place.projection))
                .map(|(index, projection)| {
                    if tcx.is_closure_like(body.source.def_id()) && index == 0 {
                        let fields = if body.local_decls[place.local].ty.is_ref() {
                            projection.strip_prefix(&[Projection::Deref]).unwrap_or_default()
                        } else {
                            &projection
                        };
                        match fields {
                            [Projection::Field(field), rest @ ..] if !string_contents => {
                                ReturnSource::Capture { field: *field, projection: rest.to_vec() }
                            }
                            _ => ReturnSource::Unknown,
                        }
                    } else {
                        ReturnSource::Input { index, projection, string_contents }
                    }
                })
                .unwrap_or(ReturnSource::Unknown);
            sources.insert(source);
        }
        for &pred in &body.basic_blocks.predecessors()[at.block] {
            if !blocks.contains(&pred) || !normal_successors(body, pred).contains(&at.block) {
                continue;
            }
            let term = body[pred].terminator();
            let mut source = place;
            let mut selected = selected;
            let mut string_contents = string_contents;
            if let TerminatorKind::Call { func, args, destination, target: Some(target), .. } = &term.kind {
                if *target == at.block
                    && starts_with(place, *destination)
                    && (!address || place.projection[destination.projection.len()..].contains(&ProjectionElem::Deref))
                {
                    // The standard String deref returns its own contents.
                    let string_deref = func.const_fn_def().is_some_and(|(method, generics)| {
                        tcx.trait_of_assoc(method) == tcx.lang_items().deref_trait()
                            && generics
                                .types()
                                .next()
                                .and_then(|t| t.ty_adt_def())
                                .is_some_and(|d| tcx.is_lang_item(d.did(), rustc_hir::LangItem::String))
                    });
                    let Some(arg) = args.first().and_then(|arg| arg.node.place()).filter(|_| string_deref) else {
                        sources.insert(ReturnSource::Unknown);
                        continue;
                    };
                    source = arg.project_deeper(&place.projection[destination.projection.len()..], tcx);
                    selected |= span.is_some_and(|s| term.source_info.span.source_equal(s));
                    string_contents = true;
                } else if call_writes.entry(pred).or_insert_with(|| writable_arguments(
                    tcx, body, blocks, body.terminator_loc(pred), args.iter().filter_map(|arg| arg.node.place()),
                )).iter().any(|&exposed| may_write(place, exposed, address)) {
                    sources.insert(ReturnSource::Unknown);
                    continue;
                }
            }
            work.push((body.terminator_loc(pred), source, selected, string_contents, address));
        }
    }
    (!sources.is_empty()).then_some(sources)
}

/// Parameter identities for the return on this source edge. The caller
/// uses this to explain lifetime constraints on the *other* parameters.
pub(crate) fn return_arguments<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    span: Option<rustc_span::Span>,
) -> Option<BTreeSet<Option<rustc_middle::mir::Local>>> {
    return_sources(tcx, body, &normal_blocks(body), span, Place::from(RETURN_PLACE))?
        .into_iter()
        .map(|source| match source {
            ReturnSource::Input { index, .. } => body.args_iter().nth(index).map(Some),
            ReturnSource::Constant => Some(None),
            _ => None,
        })
        .collect()
}

#[derive(Default)]
struct PlaceEffects<'tcx> {
    at: BTreeMap<Location, Vec<(Place<'tcx>, PlaceContext)>>,
}

impl<'tcx> Visitor<'tcx> for PlaceEffects<'tcx> {
    fn visit_place(&mut self, place: &Place<'tcx>, context: PlaceContext, at: Location) {
        if matches!(
            context,
            PlaceContext::NonMutatingUse(NonMutatingUseContext::Move)
                | PlaceContext::MutatingUse(
                    MutatingUseContext::Store
                        | MutatingUseContext::Call
                        | MutatingUseContext::Borrow
                        | MutatingUseContext::RawBorrow
                        | MutatingUseContext::Drop
                )
        ) {
            self.at.entry(at).or_default().push((*place, context));
        }
        self.super_place(place, context, at);
    }
}

/// Follow each owned environment field through direct moves, separately
/// along each normal edge. Stop at the actual drop, return, or escape of
/// that value. Cleanup outcomes are outside this summary.
fn capture_fates<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    blocks: &BTreeSet<BasicBlock>,
) -> (Vec<BTreeSet<CaptureFate>>, BTreeSet<usize>) {
    use CaptureFate::*;
    let Some(environment) = body.args_iter().next() else { return Default::default() };
    let TyKind::Closure(_, args) = *body.local_decls[environment].ty.kind() else { return Default::default() };
    let mut effects = PlaceEffects::default();
    effects.visit_body(body);
    let env = body.typing_env(tcx);
    let mut borrowed = BTreeSet::new();
    let fates = args.as_closure()
        .upvar_tys()
        .iter()
        .enumerate()
        .map(|(field, ty)| {
            let needs_drop = tcx.erase_and_anonymize_regions(ty).needs_drop(tcx, env);
            let place = Place::from(environment).project_deeper(
                &[ProjectionElem::Field(rustc_abi::FieldIdx::from_usize(field), ty)], tcx);
            let mut work = vec![((START_BLOCK, place), false)];
            let mut seen = std::collections::HashSet::new();
            let mut active = std::collections::HashSet::new();
            let mut outcomes = BTreeSet::new();
            'path: while let Some((site @ (bb, mut carrier), leaving)) = work.pop() {
                if leaving {
                    active.remove(&site);
                    continue;
                }
                if active.contains(&site) {
                    outcomes.insert(NoExit);
                    continue;
                }
                if !seen.insert(site) || !blocks.contains(&bb) {
                    continue;
                }
                active.insert(site);
                work.push((site, true));
                let data = &body[bb];
                for i in 0..=data.statements.len() {
                    let at = Location { block: bb, statement_index: i };
                    for &(place, effect) in effects.at.get(&at).into_iter().flatten() {
                        if !starts_with(carrier, place)
                            && (!starts_with(place, carrier)
                                || place.projection[carrier.projection.len()..].contains(&ProjectionElem::Deref))
                        {
                            continue;
                        }
                        let fate = match effect {
                            PlaceContext::MutatingUse(MutatingUseContext::Borrow) if starts_with(place, carrier) => {
                                // The slot stays owned. A mutable borrow can change its contents.
                                borrowed.insert(field);
                                continue;
                            }
                            PlaceContext::MutatingUse(MutatingUseContext::Drop) if starts_with(carrier, place) => {
                                if needs_drop { Dropped } else { ScopeEnd }
                            }
                            PlaceContext::NonMutatingUse(NonMutatingUseContext::Move) if place == carrier => {
                                if let Some(st) = data.statements.get(i) {
                                    if let StatementKind::Assign(a) = &st.kind {
                                        if matches!(a.1, Rvalue::Use(Operand::Move(p)) if p == carrier)
                                            && !a.0.is_indirect()
                                        {
                                            carrier = a.0;
                                            break;
                                        }
                                    }
                                }
                                if matches!(data.statements.get(i).map(|st| &st.kind),
                                    Some(StatementKind::Assign(a)) if matches!(a.1, Rvalue::Aggregate(..)))
                                {
                                    outcomes.insert(Transferred);
                                    continue 'path;
                                }
                                match &data.terminator().kind {
                                    TerminatorKind::Call { func, args, .. }
                                        if i == data.statements.len()
                                            && args.iter().any(|a| a.node.place() == Some(place)) =>
                                    {
                                        if func
                                            .const_fn_def()
                                            .is_some_and(|(d, _)| tcx.is_diagnostic_item(sym::mem_drop, d))
                                        {
                                            Dropped
                                        } else {
                                            Transferred
                                        }
                                    }
                                    _ => Unknown,
                                }
                            }
                            _ => Unknown,
                        };
                        outcomes.insert(fate);
                        continue 'path;
                    }
                    if data.statements.get(i).is_some_and(|st| {
                        matches!(st.kind,
                    StatementKind::StorageDead(l) if l == carrier.local)
                    }) {
                        outcomes.insert(if needs_drop { Unknown } else { ScopeEnd });
                        continue 'path;
                    }
                }
                let successors = normal_successors(body, bb);
                if successors.is_empty() {
                    outcomes.insert(match data.terminator().kind {
                        TerminatorKind::Return if carrier.local == RETURN_PLACE => Returned,
                        TerminatorKind::Return if !needs_drop => ScopeEnd,
                        _ => Unknown,
                    });
                }
                work.extend(successors.into_iter().map(|next| ((next, carrier), false)));
            }
            outcomes
        })
        .collect();
    (fates, borrowed)
}
