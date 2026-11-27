//! Values read, borrowed, constructed, and produced by calls in borrowck MIR.
//! Source spans anchor occurrences to the source visitor. They do not identify
//! an operand: the MIR location and operand index do.

use super::places::Names;
use crate::svg_generator::data::TryMove;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{
    AggregateKind, BasicBlock, Body, BorrowKind, Local, LocalInfo, Location, Operand, Place, ProjectionElem, Rvalue,
    StatementKind, TerminatorKind, RETURN_PLACE,
};
use rustc_middle::ty::TyCtxt;
use rustc_span::{sym, Span};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Source {
    pub local: Local,
    pub column: String,
    pub label: String,
    pub through: Option<String>,
    pub through_element: Option<Span>,
    pub deref: bool,
    pub container: Option<String>,
    pub temporary: Option<Span>,
    pub element: bool,
    pub is_ref: bool,
    pub needs_drop: bool,
    pub guard: bool,
    pub downcast: bool,
    pub static_item: bool,
    pub string_data: bool,
    pub raw_pointer: bool,
    stages: BTreeSet<Local>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OperandEffect {
    Copy(Source),
    Move(Source),
    Borrow { source: Source, mutable: bool, of_ref_slot: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Site {
    pub at: Location,
    pub operand: usize,
    pub row: usize,
    result: bool,
}

impl Site {
    pub fn order(self, rank: usize) -> (usize, usize, i8, usize) {
        (rank, self.at.statement_index, i8::from(self.result), self.operand)
    }
}

/// A returned operand's position, ordered from the enclosing value inward.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ReturnPart {
    Tuple(usize),
    Array(usize),
    Variant(String, usize, usize),
    Field(String, String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Destination {
    Constructor(Location, usize),
    Argument(Location, usize, Option<usize>),
    Return(Vec<ReturnPart>),
}

#[derive(Clone, Debug)]
pub(crate) struct Read {
    pub site: Site,
    pub exact: bool,
    pub effect: OperandEffect,
    runs: bool,
    destinations: BTreeSet<Destination>,
    staging: Option<Local>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct CallSlots {
    pub span: Option<Span>,
    pub callee: Option<DefId>,
    pub index: usize,
    pub constructor: bool,
    pub projected_literal: Option<rustc_hir::HirId>,
    pub callable: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Call {
    pub site: Option<Site>,
    pub callee: Option<DefId>,
    pub has_result: bool,
    pub returns_reference: bool,
    pub returns_unit: bool,
    pub copyable: bool,
    pub drops_arg: bool,
    pub try_branch: bool,
    reads_referent: [bool; 2],
    pub std_ref_adapter: bool,
}

#[derive(Default)]
struct Operands {
    reads: HashMap<Span, Vec<Read>>,
    calls: HashMap<Span, Vec<Call>>,
    constructors: HashMap<Span, Vec<Site>>,
}

impl Operands {
    fn slot_matches(&self, slots: CallSlots, dest: &Destination) -> bool {
        let Some(span) = slots.span.map(|span| span.with_parent(None)) else { return false };
        match *dest {
            Destination::Argument(at, i, part) if !slots.constructor
                && if slots.callable { i == 1 && part == Some(slots.index) } else { i == slots.index } =>
                self.calls.get(&span).into_iter().flatten().any(|call|
                    call.callee == slots.callee && call.site.is_some_and(|site| site.at == at)),
            Destination::Constructor(at, i) if slots.constructor && i == slots.index =>
                self.constructors.get(&span).into_iter().flatten().any(|site| site.at == at),
            _ => false,
        }
    }

    fn argument_reads(&self, span: Span, slots: CallSlots) -> impl Iterator<Item = &Read> {
        self.reads.get(&span.with_parent(None)).into_iter().flatten()
            .filter(move |read| read.destinations.iter().any(|dest| self.slot_matches(slots, dest)))
    }

    fn common_read<'a>(mut reads: impl Iterator<Item = &'a Read>) -> Option<Read> {
        let mut first = reads.next()?.clone();
        for other in reads {
            // Separate MIR blocks can evaluate the same guard on different
            // alternatives. Show their common effect without a single site.
            if first.effect != other.effect || first.site.row != other.site.row
                || first.site.at.block == other.site.at.block
            {
                return None;
            }
            first.exact = false;
        }
        Some(first)
    }

    fn call(&self, span: Span, callee: Option<DefId>) -> Option<Call> {
        let mut calls = self.calls.get(&span.with_parent(None))?.iter().filter(|call| call.callee == callee);
        let mut first = *calls.next()?;
        for other in calls {
            // A guard with alternative patterns can have more than one
            // occurrence. Its result type is shared, its order is not.
            if (first.has_result, first.copyable) != (other.has_result, other.copyable) {
                return None;
            }
            first.site = None;
            first.reads_referent = std::array::from_fn(|i| first.reads_referent[i] && other.reads_referent[i]);
            first.std_ref_adapter &= other.std_ref_adapter;
        }
        Some(first)
    }
}

#[derive(Default)]
pub(crate) struct ValueFlow {
    operands: Operands,
    // Only argument drawing can use operands on borrowck-only paths.
    imaginary: Operands,
    try_inputs: HashMap<Span, Vec<(Read, TryMove)>>,
    owned_parameter_borrows: BTreeMap<Local, (Local, bool)>,
    results: BTreeMap<Location, BTreeSet<Destination>>,
    try_returns: HashMap<Span, Vec<Option<Vec<ReturnPart>>>>,
}

impl ValueFlow {
    pub fn read(&self, span: Span) -> Option<Read> {
        Operands::common_read(self.operands.reads.get(&span.with_parent(None))?.iter())
    }

    pub fn returned_read(&self, span: Span) -> Option<Read> {
        let reads = self.operands.reads.get(&span.with_parent(None))?.iter();
        Operands::common_read(reads.filter(|read| returned_part(&read.destinations).is_some()))
    }

    pub fn argument_read(&self, span: Span, slots: CallSlots) -> Option<Read> {
        let mut executable = self.operands.argument_reads(span, slots).peekable();
        if executable.peek().is_some() {
            Operands::common_read(executable)
        } else {
            Operands::common_read(self.imaginary.argument_reads(span, slots))
        }
    }

    /// Follow the implicit receiver's own operand to slot zero of this
    /// invocation. Ordinary closure arguments are fields of slot one.
    pub fn call_receiver(&self, span: Span, callee: DefId, receiver: Span) -> Option<(Read, Vec<Location>, bool)> {
        let slots = CallSlots { span: Some(span), callee: Some(callee), ..CallSlots::default() };
        let operands = &self.operands;
        let calls: BTreeSet<_> = operands.calls.get(&span.with_parent(None)).into_iter().flatten()
            .filter(|call| call.callee == Some(callee)).filter_map(|call| call.site.map(|site| site.at)).collect();
        if !calls.is_empty() {
            let reads = || operands.argument_reads(receiver, slots);
            let read = Operands::common_read(reads())?;
            let mut matched = BTreeSet::new();
            let mut staged = false;
            for operand in reads() {
                let (at, _) = self.unique_argument(operand, slots)?;
                staged |= at.block != operand.site.at.block;
                matched.insert(at);
            }
            return (calls == matched).then(|| (read, matched.into_iter().collect(), staged));
        }
        // A call whose arguments diverge can still stage its receiver.
        // The compiler provided a resolved FnOnce method but no call
        // terminator. Keep only an unforwarded transfer into a temporary.
        let read = self.read(receiver)?;
        (read.staging.is_some() && read.destinations.is_empty() && read.exact).then_some((read, Vec::new(), true))
    }

    pub fn drops_argument(&self, read: &Read, slots: CallSlots) -> bool {
        let operands = if read.runs { &self.operands } else { &self.imaginary };
        slots.span.and_then(|span| operands.call(span, slots.callee)).is_some_and(|call| call.drops_arg)
    }

    pub fn reads_argument_referent(&self, read: &Read, slots: CallSlots) -> bool {
        read.runs && slots.span.and_then(|span| self.call(span, slots.callee))
            .is_some_and(|call| call.reads_referent.get(slots.index) == Some(&true))
    }

    pub fn result_passes_to(&self, call: Call, slots: CallSlots) -> bool {
        call.site.and_then(|site| self.results.get(&site.at))
            .is_some_and(|destinations| destinations.iter().any(|d| self.operands.slot_matches(slots, d)))
    }

    pub fn returned_result(&self, call: Call) -> Option<&[ReturnPart]> {
        returned_part(self.results.get(&call.site?.at)?)
    }

    pub fn returned_try(&self, span: Span) -> Option<&[ReturnPart]> {
        let parts = self.try_returns.get(&span.source_callsite().with_parent(None))?;
        let first = parts.first()?;
        parts.iter().all(|path| path == first).then_some(first.as_deref()).flatten()
    }

    pub fn unique_argument(&self, read: &Read, slots: CallSlots) -> Option<(Location, usize)> {
        if !read.runs {
            return None;
        }
        unique(read.destinations.iter().filter(|dest| self.operands.slot_matches(slots, dest)).filter_map(|dest| {
            match dest {
                Destination::Argument(at, index, _) => Some((*at, *index)),
                _ => None,
            }
        }))
    }

    pub fn call(&self, span: Span, callee: Option<DefId>) -> Option<Call> {
        self.operands.call(span, callee)
    }

    pub fn constructor(&self, span: Span) -> bool {
        self.operands.constructors.contains_key(&span.with_parent(None))
    }

    pub fn argument_constructor(&self, span: Span) -> bool {
        self.constructor(span) || self.imaginary.constructors.contains_key(&span.with_parent(None))
    }

    pub fn try_input(&self, span: Span) -> Option<&(Read, TryMove)> {
        unique(self.try_inputs.get(&span.source_callsite().with_parent(None))?.iter())
    }
    pub fn own_parameter_borrow(&self, binding: Local, parameter: Local) -> Option<bool> {
        let &(argument, whole) = self.owned_parameter_borrows.get(&binding)?;
        (argument == parameter).then_some(whole)
    }
}

fn unique<T>(mut items: impl Iterator<Item = T>) -> Option<T> {
    let first = items.next()?;
    items.next().is_none().then_some(first)
}

fn returned_part(destinations: &BTreeSet<Destination>) -> Option<&[ReturnPart]> {
    unique(destinations.iter().filter_map(|destination| match destination {
        Destination::Return(path) => Some(path.as_slice()),
        _ => None,
    }))
}

#[derive(Clone)]
enum Target<'tcx> {
    Store(Place<'tcx>),
    BorrowResult(Place<'tcx>, BasicBlock),
    End(Destination),
}

struct Use<'tcx> {
    from: Place<'tcx>,
    at: Location,
    target: Target<'tcx>,
}

fn aggregate_field<'tcx>(
    tcx: TyCtxt<'tcx>, kind: &AggregateKind<'tcx>, place: Place<'tcx>,
    index: usize, ty: rustc_middle::ty::Ty<'tcx>, count: usize,
) -> Option<Place<'tcx>> {
    match kind {
        AggregateKind::Array(_) => Some(place.project_deeper(&[ProjectionElem::ConstantIndex {
            offset: index as u64, min_length: count as u64, from_end: false,
        }], tcx)),
        AggregateKind::Tuple => Some(place.project_deeper(&[ProjectionElem::Field(index.into(), ty)], tcx)),
        AggregateKind::Adt(def, variant, _, _, active) => {
            let adt = tcx.adt_def(*def);
            let field = ProjectionElem::Field(active.unwrap_or_else(|| index.into()), ty);
            let place = if adt.is_enum() {
                place.project_deeper(&[ProjectionElem::Downcast(Some(adt.variant(*variant).name), *variant)], tcx)
            } else {
                place
            };
            Some(place.project_deeper(&[field], tcx))
        }
        _ => None,
    }
}

fn projection_prefix<'a, 'tcx>(
    projection: &'a [rustc_middle::mir::PlaceElem<'tcx>], prefix: &[rustc_middle::mir::PlaceElem<'tcx>],
) -> Option<&'a [rustc_middle::mir::PlaceElem<'tcx>]> {
    if projection.len() < prefix.len() || !projection.iter().zip(prefix).all(|(a, b)| match (a, b) {
        (ProjectionElem::Field(a, _), ProjectionElem::Field(b, _)) => a == b,
        (ProjectionElem::Downcast(_, a), ProjectionElem::Downcast(_, b)) => a == b,
        _ => a == b,
    }) {
        return None;
    }
    Some(&projection[prefix.len()..])
}

fn try_output_place<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, destination: Place<'tcx>) -> Option<Place<'tcx>> {
    let ty = destination.ty(&body.local_decls, tcx).ty;
    let rustc_middle::ty::Adt(adt, args) = ty.kind() else { return None };
    let (index, variant) = adt.variants().iter_enumerated()
        .find(|(_, variant)| Some(variant.def_id) == tcx.lang_items().cf_continue_variant())?;
    let field = rustc_abi::FieldIdx::from_usize(0);
    Some(destination.project_deeper(&[
        ProjectionElem::Downcast(Some(variant.name), index),
        ProjectionElem::Field(field, variant.fields[field].ty(tcx, args)),
    ], tcx))
}

fn return_path<'tcx>(body: &Body<'tcx>, place: Place<'tcx>) -> Option<Vec<ReturnPart>> {
    let mut ty = body.return_ty();
    let mut variant = None;
    let mut path = Vec::new();
    for elem in place.projection {
        match elem {
            ProjectionElem::Downcast(_, v) => variant = Some(v),
            ProjectionElem::Field(index, field_ty) => {
                let index = index.index();
                let part = if matches!(ty.kind(), rustc_middle::ty::Tuple(_)) {
                    ReturnPart::Tuple(index)
                } else {
                    let adt = ty.ty_adt_def()?;
                    if let Some(v) = variant.take() {
                        let v = adt.variant(v);
                        ReturnPart::Variant(v.name.to_string(), index, v.fields.len())
                    } else {
                        let v = adt.non_enum_variant();
                        ReturnPart::Field(v.name.to_string(), v.fields[rustc_abi::FieldIdx::from_usize(index)].name.to_string())
                    }
                };
                path.push(part);
                ty = field_ty;
            }
            ProjectionElem::ConstantIndex { offset, from_end: false, .. } => {
                path.push(ReturnPart::Array(offset as usize));
                ty = ty.builtin_index()?;
            }
            _ => return None,
        }
    }
    Some(path)
}

pub(super) fn collect<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    names: &Names<'tcx>,
    runs: &BTreeSet<BasicBlock>,
    checked: &BTreeSet<BasicBlock>,
    rows: &BTreeMap<Location, usize>,
) -> ValueFlow {
    let mut flow = collect_blocks(tcx, body, names, runs, rows, true);
    // Preserve the older drawing of calls reached only by borrowck. A
    // separate view of the same MIR keeps their writes and staged reads
    // out of the executable graph. No imaginary sites reach event ordering.
    let imaginary: BTreeSet<_> = checked.difference(runs).copied().collect();
    if !imaginary.is_empty() {
        flow.imaginary = collect_blocks(tcx, body, names, &imaginary, rows, false).operands;
    }
    flow
}

fn collect_blocks<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    names: &Names<'tcx>,
    blocks: &BTreeSet<BasicBlock>,
    rows: &BTreeMap<Location, usize>,
    runs: bool,
) -> ValueFlow {
    let mut out = ValueFlow::default();
    let env = body.typing_env(tcx);
    let copyable = |ty| tcx.type_is_copy_modulo_regions(env, tcx.erase_and_anonymize_regions(ty));
    let site = |at, operand| Site {
        at,
        operand,
        result: false,
        row: rows.get(&at).copied().unwrap_or_else(|| {
            tcx.sess.source_map().lookup_char_pos(body.source_info(at).span.source_callsite().lo()).line
        }),
    };
    let span_at = |at| body.source_info(at).span.with_parent(None);
    let mut writes = BTreeMap::<Local, Vec<(Location, Place<'tcx>)>>::new();
    let mut definitions = BTreeMap::<Local, Vec<_>>::new();
    let mut temporary_results = BTreeMap::<Local, Vec<Span>>::new();
    let mut projected_results = BTreeMap::<Local, Vec<_>>::new();
    let mut try_results = Vec::new();
    let doms = body.basic_blocks.dominators();
    let dominates = |source: Location, use_at: Location| {
        if source.block == use_at.block {
            source.statement_index < use_at.statement_index
        } else {
            doms.dominates(source.block, use_at.block)
        }
    };
    for &bb in blocks {
        let data = &body[bb];
        if data.is_cleanup {
            continue;
        }
        for (statement_index, statement) in data.statements.iter().enumerate() {
            if let StatementKind::Assign(a) = &statement.kind {
                let at = Location { block: bb, statement_index };
                writes.entry(a.0.local).or_default().push((at, a.0));
                if let Some(local) = a.0.as_local() {
                    definitions.entry(local).or_default().push((at, &a.1));
                }
            }
        }
        if let TerminatorKind::Call { func, args, destination, target: Some(next), .. } = &data.terminator().kind {
            let at = body.terminator_loc(bb);
            writes.entry(destination.local).or_default().push((at, *destination));
            if func.const_fn_def().is_some_and(|(def, _)| Some(def) == tcx.lang_items().branch_fn()) {
                if let Some(payload) = try_output_place(tcx, body, *destination) {
                    try_results.push((span_at(at).source_callsite().with_parent(None), at, *next, payload));
                }
            }
            if let Some(local) = destination.as_local().filter(|l| !body.local_decls[*l].is_user_variable()) {
                temporary_results.entry(local).or_default().push(span_at(body.terminator_loc(bb)));
            }
            if let Some((local, receiver)) = destination.as_local().zip(args.first().and_then(|a| a.node.place())) {
                let traits = tcx.lang_items();
                let trait_id = func.const_fn_def().and_then(|(id, _)| tcx.trait_of_assoc(id));
                let indexed =
                    trait_id.is_some() && [traits.index_trait(), traits.index_mut_trait()].contains(&trait_id);
                // String's Deref implementation exposes the borrow of its
                // string data, including when rustc inserts the coercion
                // while evaluating a returned reference.
                let string_deref = trait_id.is_some()
                    && trait_id == traits.deref_trait()
                    && receiver
                        .ty(&body.local_decls, tcx)
                        .ty
                        .builtin_deref(true)
                        .and_then(|ty| ty.ty_adt_def())
                        .is_some_and(|adt| tcx.lang_items().string() == Some(adt.did()));
                if indexed || string_deref {
                    projected_results.entry(local).or_default().push((*next, receiver, indexed));
                }
            }
        }
    }
    // The deref of a borrowed temporary still reaches the original place.
    // Only peel single-def staging locals. At a merge the source is unknown.
    let normalize = |mut place: Place<'tcx>, at: Location| {
        let mut seen = BTreeSet::new();
        let mut indexed = false;
        let mut through_element = None;
        let mut projection_at = at;
        let mut string_data = false;
        while names.root(place).is_none() && seen.insert(place.local) {
            if let Some([(next, receiver, selects_index)]) = projected_results.get(&place.local).map(Vec::as_slice) {
                if doms.dominates(*next, at.block) && place.projection.first() == Some(&ProjectionElem::Deref) {
                    if *selects_index && place.projection.get(1) == Some(&ProjectionElem::Deref) {
                        through_element = Some(span_at(projection_at));
                    }
                    place = receiver.project_deeper(&[ProjectionElem::Deref], tcx);
                    indexed |= selects_index;
                    string_data |= !selects_index;
                    continue;
                }
            }
            let Some([(defined, value)]) = definitions.get(&place.local).map(Vec::as_slice) else {
                break;
            };
            if !dominates(*defined, at) {
                break;
            }
            let (base, rest) = match (value, place.projection.as_slice()) {
                (Rvalue::Ref(_, _, p), [ProjectionElem::Deref, rest @ ..]) => (*p, rest),
                (Rvalue::Use(op), [ProjectionElem::Deref, ..]) => {
                    let Some(p) = op.place().filter(|p| p.ty(&body.local_decls, tcx).ty.is_ref()) else {
                        break;
                    };
                    (p, place.projection.as_slice())
                }
                _ => break,
            };
            place = base.project_deeper(rest, tcx);
            projection_at = *defined;
        }
        (place, indexed, through_element, seen, string_data)
    };
    let source = |place: Place<'tcx>, at| {
        let ty = place.ty(&body.local_decls, tcx).ty;
        let guard = body.local_decls[place.local].is_ref_for_guard();
        let (place, indexed, through_element, stages, string_data) = normalize(place, at);
        let static_item = matches!(body.local_decls[place.local].local_info(), LocalInfo::StaticRef { .. });
        let raw_pointer = place.projection.iter().enumerate().any(|(i, elem)| {
            elem == ProjectionElem::Deref && !(static_item && i == 0)
                && rustc_middle::mir::PlaceRef { local: place.local, projection: &place.projection[..i] }
                    .ty(&body.local_decls, tcx).ty.is_raw_ptr()
        });
        let (column, label, through, skip) = if let Some((column, skip, _)) = names.root(place) {
            let label = names.name(tcx, body, place)?;
            (column, label, names.through(tcx, body, place), skip)
        } else if let (LocalInfo::StaticRef { def_id, .. }, [ProjectionElem::Deref, rest @ ..]) =
            (body.local_decls[place.local].local_info(), place.projection.as_slice())
        {
            let column = tcx.def_path_str(*def_id);
            let mut static_ty = tcx.type_of(*def_id).instantiate_identity().skip_normalization();
            let label = super::places::name_projections(column.clone(), &mut static_ty, rest);
            (column, label, None, 1)
        } else {
            return None;
        };
        let projection = &place.projection[skip..];
        Some(Source {
            local: place.local,
            label,
            through,
            through_element,
            deref: !indexed && projection.last() == Some(&ProjectionElem::Deref),
            container: projection.last().filter(|p| **p == ProjectionElem::Deref).and_then(|_| {
                let base = Place {
                    local: place.local,
                    projection: tcx.mk_place_elems(&place.projection[..place.projection.len() - 1]),
                };
                names.name(tcx, body, base)
            }),
            temporary: None,
            element: indexed
                || projection.iter().any(|p| {
                    matches!(
                        p,
                        ProjectionElem::Index(_)
                            | ProjectionElem::ConstantIndex { .. }
                            | ProjectionElem::Subslice { .. }
                    )
                }),
            column,
            is_ref: ty.is_ref(),
            needs_drop: tcx.erase_and_anonymize_regions(ty).needs_drop(tcx, env),
            stages, guard, static_item, string_data, raw_pointer,
            downcast: projection.iter().any(|p| matches!(p, ProjectionElem::Downcast(..))),
        })
    };
    let continuation_bindings: BTreeSet<_> = definitions.iter().filter_map(|(&local, defs)| {
        let [(_, Rvalue::Use(op))] = defs.as_slice() else { return None };
        let from = op.place()?;
        try_results.iter().any(|&(_, _, _, payload)| from.local == payload.local
            && from.projection.len() == payload.projection.len()
            && projection_prefix(from.projection.as_slice(), payload.projection.as_slice()).is_some())
            .then_some(local)
    }).collect();
    let mut uses: BTreeMap<Local, Vec<Use<'tcx>>> = BTreeMap::new();
    let mut staged = Vec::new();
    let mut results = Vec::new();
    let temporary = |place: Place<'tcx>| place.local != RETURN_PLACE
        && (names.root(place).is_none() || continuation_bindings.contains(&place.local));
    let mut record_use = |from: Place<'tcx>, at, target| {
        if temporary(from) {
            uses.entry(from.local).or_default().push(Use { from, at, target });
        }
    };
    let read = |op: &Operand<'tcx>, at, operand| {
        let place = op.place()?;
        let src = source(place, at).or_else(|| {
            if !matches!(op, Operand::Move(_)) || !matches!(place.projection.first(), Some(ProjectionElem::Field(..))) {
                return None;
            }
            let [span] = temporary_results.get(&place.local)?.as_slice() else {
                return None;
            };
            Some(Source {
                local: place.local,
                column: String::new(),
                label: String::new(),
                through: None,
                through_element: None,
                deref: false,
                container: None,
                element: false,
                is_ref: false,
                needs_drop: false,
                stages: BTreeSet::new(),
                guard: false, downcast: false, static_item: false, string_data: false, raw_pointer: false,
                temporary: Some(*span),
            })
        })?;
        let effect = match op {
            Operand::Copy(_) => OperandEffect::Copy(src),
            Operand::Move(_) => OperandEffect::Move(src),
            _ => return None,
        };
        Some(Read { site: site(at, operand), exact: runs, effect, runs, destinations: BTreeSet::new(), staging: None })
    };

    for &bb in blocks {
        let data = &body[bb];
        if data.is_cleanup {
            continue;
        }
        for (statement_index, st) in data.statements.iter().enumerate() {
            let StatementKind::Assign(a) = &st.kind else {
                continue;
            };
            let at = Location { block: bb, statement_index };
            let span = span_at(at);
            let target = Target::Store(a.0);
            match &a.1 {
                Rvalue::Use(op) | Rvalue::Cast(_, op, _) => {
                    if let Some(place) = op.place() {
                        record_use(place, at, target.clone());
                    }
                    if let Some(mut read) = read(op, at, 0) {
                        if matches!(a.1, Rvalue::Use(_)) && temporary(a.0) {
                            read.staging = a.0.as_local();
                        }
                        staged.push((span, read, vec![target]));
                    }
                }
                Rvalue::Ref(_, kind, p) => {
                    if p.projection.is_empty() || p.projection.first() == Some(&ProjectionElem::Deref) {
                        record_use(Place::from(p.local), at, target.clone());
                    }
                    if body.args_iter().any(|local| local == p.local)
                        && !p.projection.iter().enumerate().any(|(i, projection)| {
                            projection == ProjectionElem::Deref
                                && rustc_middle::mir::PlaceRef { local: p.local, projection: &p.projection[..i] }
                                    .ty(&body.local_decls, tcx)
                                    .ty
                                    .is_ref()
                        })
                    {
                        if let Some(binding) = a.0.as_local().filter(|l| names.locals.contains_key(l)) {
                            out.owned_parameter_borrows.insert(binding, (p.local, p.projection.is_empty()));
                        }
                    }
                    if let Some(source) = source(*p, at) {
                        let of_ref_slot = source.is_ref && source.through.is_none();
                        let mutable = matches!(kind, BorrowKind::Mut { .. });
                        let effect = OperandEffect::Borrow { source, mutable, of_ref_slot };
                        staged.push((span, Read { site: site(at, 0), exact: runs, effect, runs, destinations: BTreeSet::new(), staging: a.0.as_local() }, vec![target]));
                    }
                }
                Rvalue::Aggregate(kind, ops) => {
                    let ctor = matches!(&**kind, AggregateKind::Adt(..));
                    if ctor {
                        out.operands.constructors.entry(span).or_default().push(site(at, 0));
                    }
                    for (index, op) in ops.iter().enumerate() {
                        let mut targets = Vec::new();
                        if let Some(field) = aggregate_field(tcx, kind, a.0, index, op.ty(&body.local_decls, tcx), ops.len()) {
                            targets.push(Target::Store(field));
                        }
                        if ctor {
                            targets.push(Target::End(Destination::Constructor(at, index)));
                        }
                        if let Some(place) = op.place() {
                            for target in &targets {
                                record_use(place, at, target.clone());
                            }
                        }
                        staged.extend(read(op, at, index).map(|read| (span, read, targets)));
                    }
                }
                _ => {}
            }
        }
        let term = data.terminator();
        let TerminatorKind::Call { func, args, destination, target, .. } = &term.kind else {
            continue;
        };
        let at = body.terminator_loc(bb);
        let ty = destination.ty(&body.local_decls, tcx).ty;
        let func = match *func.ty(&body.local_decls, tcx).kind() {
            rustc_middle::ty::FnDef(id, args) => Some((id, args)),
            _ => None,
        };
        let def = func.map(|(id, _)| id);
        let try_branch = def.is_some_and(|id| tcx.lang_items().branch_fn() == Some(id));
        let trait_id = def.and_then(|id| tcx.trait_of_assoc(id));
        let lang = tcx.lang_items();
        let arithmetic = trait_id.is_some() && [
            lang.add_trait(), lang.sub_trait(), lang.mul_trait(), lang.div_trait(), lang.rem_trait(),
            lang.bitand_trait(), lang.bitor_trait(), lang.bitxor_trait(), lang.shl_trait(), lang.shr_trait(),
        ].contains(&trait_id);
        let adapter = def.is_some_and(|id| trait_id == lang.iterator_trait()
            && matches!(tcx.item_name(id).as_str(), "zip" | "chain"))
            && args.get(1).is_some_and(|arg| {
                let rustc_middle::ty::Ref(_, pointee, mutability) = *arg.node.ty(&body.local_decls, tcx).kind()
                    else { return false };
                !mutability.is_mut() && (matches!(pointee.kind(), rustc_middle::ty::Slice(_) | rustc_middle::ty::Array(..))
                    || pointee.ty_adt_def().is_some_and(|adt| tcx.is_diagnostic_item(sym::Vec, adt.did())))
            });
        let instance = (arithmetic || adapter).then(|| func.and_then(|(def, args)| {
            rustc_middle::ty::Instance::try_resolve(tcx, env, def, tcx.erase_and_anonymize_regions(args)).ok().flatten()
        })).flatten();
        let mut reads_referent = [false; 2];
        if let Some(instance) = instance.filter(|i| arithmetic && !i.def_id().is_local() && tcx.is_mir_available(i.def_id())) {
            // Standard forwarding operators load primitive operands through
            // their reference parameters before performing the arithmetic.
            let implementation = tcx.optimized_mir(instance.def_id());
            let mut record = |operand: &Operand<'tcx>| {
                if let Some(place) = operand.place().filter(|p| p.projection.as_slice() == [ProjectionElem::Deref]) {
                    if let Some(slot) = place.local.index().checked_sub(1).and_then(|i| reads_referent.get_mut(i)) {
                        *slot = true;
                    }
                }
            };
            for statement in &implementation[Location::START.block].statements {
                if let StatementKind::Assign(a) = &statement.kind {
                    match &a.1 {
                        Rvalue::Use(op) => record(op),
                        Rvalue::BinaryOp(_, ops) => {
                            record(&ops.0);
                            record(&ops.1);
                        }
                        _ => {}
                    }
                }
            }
        }
        let call = Call {
            site: Some(Site { result: true, ..site(at, 0) }),
            callee: def,
            has_result: target.is_some(),
            returns_reference: ty.is_ref(),
            returns_unit: ty.is_unit(),
            copyable: copyable(ty),
            drops_arg: def.is_some_and(|id| tcx.is_diagnostic_item(sym::mem_drop, id)),
            try_branch,
            reads_referent,
            std_ref_adapter: adapter && instance.is_some_and(|i| Some(i.def_id()) == def),
        };
        out.operands.calls.entry(span_at(at)).or_default().push(call);
        if let Some(next) = target {
            results.push((at, *next, *destination));
            // Rust's deref/index coercions can introduce a new temporary
            // reference. The borrow checker ties its lifetime to the
            // receiver's loan through these trait signatures.
            if trait_id.is_some() && [
                lang.deref_trait(), lang.deref_mut_trait(), lang.index_trait(), lang.index_mut_trait(),
            ].contains(&trait_id) {
                if let Some(receiver) = args.first().and_then(|arg| arg.node.place()) {
                    record_use(receiver, at, Target::BorrowResult(*destination, *next));
                }
            }

        }
        for (i, arg) in args.iter().enumerate() {
            let span = arg.span.with_parent(None);
            let receives = Target::End(Destination::Argument(at, i, None));
            if let Some(place) = arg.node.place() {
                record_use(place, at, receives.clone());
            }
            let direct = read(&arg.node, at, i);
            if try_branch {
                let staged = arg.node.place().and_then(|p| p.as_local()).and_then(|l| {
                    let [(from, Rvalue::Use(op))] = definitions.get(&l)?.as_slice() else {
                        return None;
                    };
                    dominates(*from, at).then(|| read(op, *from, 0)).flatten()
                });
                if let Some(input) = direct.clone().or(staged) {
                    let operand_ty = arg.node.ty(&body.local_decls, tcx);
                    let kind = match operand_ty.ty_adt_def() {
                        Some(adt) if tcx.is_diagnostic_item(sym::Result, adt.did()) => TryMove::Result,
                        Some(adt) if tcx.is_diagnostic_item(sym::Option, adt.did()) => TryMove::Option,
                        _ => TryMove::Other,
                    };
                    out.try_inputs
                        .entry(span_at(at).source_callsite().with_parent(None))
                        .or_default()
                        .push((input, kind));
                }
            }
            staged.extend(direct.map(|read| (span, read, vec![receives])));
        }
    }
    let reaches_use = |at: Location, next: Option<BasicBlock>, place: Place<'tcx>, use_at: Location| {
        let overlaps = |other: Place<'tcx>| {
            projection_prefix(place.projection.as_slice(), other.projection.as_slice()).is_some()
                || projection_prefix(other.projection.as_slice(), place.projection.as_slice()).is_some()
        };
        let writes = writes.get(&place.local).map_or(&[][..], Vec::as_slice);
        if writes.len() == 1 && next.map_or_else(
            || dominates(at, use_at), |bb| doms.dominates(bb, use_at.block),
        ) {
            return true;
        }
        // An arm may initialize the staged place even though it does
        // not dominate the call after the join. Stop on another write
        // to that place on the particular path being followed.
        let (block, first) = next.map_or((at.block, at.statement_index + 1), |bb| (bb, 0));
        let mut work = vec![(block, first)];
        let mut seen = BTreeSet::new();
        while let Some((block, first)) = work.pop() {
            if !seen.insert((block, first)) {
                continue;
            }
            let kill = writes.iter().filter(|(loc, dest)|
                loc.block == block && loc.statement_index >= first && overlaps(*dest))
                .map(|(loc, _)| loc.statement_index).min();
            if block == use_at.block && use_at.statement_index >= first
                && kill.is_none_or(|kill| use_at.statement_index <= kill)
            {
                return true;
            }
            if kill.is_none() {
                work.extend(super::rows::real_successor_blocks(body, block).into_iter()
                    .filter(|bb| runs || blocks.contains(bb)).map(|bb| (bb, 0)));
            }
        }
        false
    };
    // Follow only uses of this subplace, or an operand containing it. Keep
    // the suffix when the operand is staged in another aggregate.
    let trace = |start: Location, next: Option<BasicBlock>, targets: Vec<Target<'tcx>>, borrow: bool| {
        let mut found = BTreeSet::new();
        let mut seen = std::collections::HashSet::new();
        let mut pending: Vec<_> = targets.into_iter().map(|target| (start, next, target)).collect();
        while let Some((at, next, target)) = pending.pop() {
            let place = match target {
                Target::End(dest) => {
                    found.insert(dest);
                    continue;
                }
                Target::Store(place) => place,
                Target::BorrowResult(..) => continue,
            };
            if place.local == RETURN_PLACE {
                if let Some(path) = return_path(body, place) {
                    found.insert(Destination::Return(path));
                }
                continue;
            }
            if !seen.insert((at, place)) || !temporary(place) {
                continue;
            }
            for usage in uses.get(&place.local).into_iter().flatten() {
                let Some(rest) = projection_prefix(place.projection.as_slice(), usage.from.projection.as_slice()) else { continue };
                if !reaches_use(at, next, place, usage.at) {
                    continue;
                }
                let (target, next) = match &usage.target {
                    Target::End(Destination::Argument(at, i, _)) => {
                        let field = match rest.first() {
                            Some(ProjectionElem::Field(field, _)) => Some(field.index()),
                            _ => None,
                        };
                        (Target::End(Destination::Argument(*at, *i, field)), None)
                    }
                    Target::End(dest) => (Target::End(dest.clone()), None),
                    Target::Store(to) => (Target::Store(to.project_deeper(rest, tcx)), None),
                    Target::BorrowResult(place, next) if borrow => (Target::Store(*place), Some(*next)),
                    Target::BorrowResult(..) => continue,
                };
                pending.push((usage.at, next, target));
            }
        }
        found
    };
    for (span, mut read, targets) in staged {
        read.destinations = trace(read.site.at, None, targets, matches!(read.effect, OperandEffect::Borrow { .. }));
        out.operands.reads.entry(span).or_default().push(read);
    }
    for (at, next, place) in results {
        out.results.insert(at, trace(at, Some(next), vec![Target::Store(place)], false));
    }
    for (span, at, next, place) in try_results {
        let destinations = trace(at, Some(next), vec![Target::Store(place)], false);
        out.try_returns.entry(span).or_default().push(returned_part(&destinations).map(<[_]>::to_vec));
    }

    // When the compiler stages a reborrow through a reference just
    // created at the same source expression, display the reference
    // actually passed on. Retain unrelated occurrences at that span.
    #[expect(
        clippy::disallowed_methods,
        clippy::iter_over_hash_type,
        reason = "each span's operand list is independent"
    )]
    for reads in out.operands.reads.values_mut() {
        let intermediate: BTreeSet<_> = reads
            .iter()
            .filter_map(|read| match &read.effect {
                OperandEffect::Borrow { source, .. } => Some(&source.stages),
                _ => None,
            })
            .flatten()
            .copied()
            .collect();
        reads.retain(|read| {
            !matches!(
                (&read.effect, read.staging),
                (OperandEffect::Borrow { .. }, Some(local)) if intermediate.contains(&local)
            )
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_instruction_operands_precede_call_result_in_slot_order() {
        let at = Location::START;
        let first = Site { at, operand: 0, row: 1, result: false };
        let second = Site { operand: 1, ..first };
        let arrival = Site { result: true, ..first };
        assert!(first.order(0) < second.order(0));
        assert!(second.order(0) < arrival.order(0));
    }
}
