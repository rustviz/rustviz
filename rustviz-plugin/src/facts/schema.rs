//! The fact layer's output: what is true about each place on each row,
//! from MIR and borrowck only. Serialized as JSON (`RV3_FACTS`), version
//! [`VERSION`]. Every map is a `BTreeMap` and every list is in MIR
//! evaluation order or sorted, so the dump is deterministic.

use rustc_middle::mir::{BasicBlock, Local, Location};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Bumped whenever a field changes meaning or goes away. 2: `Lent.kind`
/// tells `unique` and `reserved` from `mut`; `Lent.holder` is who holds
/// the loan on that row. 3: `loans` also lists the loans borrowck doesn't
/// track (`LoanInfo.source`: a reference parameter's, a constant's, an
/// untracked shared borrow, a kept temporary's), with their `BorrowEnd`s;
/// `LoanInfo.made_through`, `direct_holders`, `via_call`, `implicit`; the
/// `HoldEnd` fact. 4: `State.holds` (the loans each variable's value
/// keeps borrowed after the row, as the borrow checker counts them).
pub const VERSION: u32 = 4;

/// Bound the space used to enumerate individual elements of an array
/// pattern. Larger arrays still have their partial-move state.
pub const MAX_NAMED_ARRAY_LEN: usize = 64;

#[derive(Serialize)]
pub struct CrateFacts {
    pub version: u32,
    pub bodies: Vec<BodyFacts>,
    /// Bodies the fact layer failed on (a panic caught per body), as
    /// "<def path>: <message>". Empty in a healthy run.
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extraction {
    pub inner: String,
    /// A real path also reaches this boundary without extracted contents.
    pub maybe: bool,
}

#[derive(Clone, Copy)]
pub struct AggregateLoan {
    pub loan: usize,
    /// This operand makes a reference, rather than carrying an older one.
    pub issued: bool,
    /// The aggregate can be constructed on a path that skips the issue.
    pub conditional: bool,
}

/// The source-level name and the typed steps that have no column of their own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectedPlace {
    pub base: String,
    pub steps: Vec<PlaceStep>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlaceStep {
    Deref,
    Field(String),
    Element,
    Subslice,
    Payload,
}

/// How the MIR operand carries this loan into a receiver.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StoredOperand {
    /// A reference with an identified referent, or an aggregate of direct references.
    Direct,
    /// A shared reference whose target or access rights differ from what
    /// the individual loan would suggest.
    SharedReference,
    /// The loan flows into the receiver's lifetimes. Its contents at run time are unknown.
    Carrier,
}

/// The argument index includes the receiver at index zero. A loan already
/// in the receiver needs an independent signature tie to this argument.
pub struct ReceiverLoan {
    pub loan: usize,
    pub argument: usize,
    pub operand: StoredOperand,
    pub newly_held: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalArgument {
    Borrowed,
    Formatted,
}

/// A HIR call can lower to multiple MIR call sites. Preserve each occurrence.
pub(crate) type ClosureResults = rustc_data_structures::fx::FxIndexMap<
    rustc_hir::HirId, BTreeMap<rustc_middle::mir::Local, Vec<Option<ClosureResultLoans>>>,
>;

fn closure_calls<S: serde::Serializer>(calls: &ClosureResults, s: S) -> Result<S::Ok, S::Error> {
    s.collect_seq(calls.values().flat_map(|sites| sites.values().flatten().flatten()))
}

#[derive(Serialize)]
pub struct ClosureResultLoans {
    #[serde(serialize_with = "location")]
    pub call_location: Location,
    /// The call writes here before a possible drop of the eventual holder.
    #[serde(serialize_with = "local")]
    pub destination: rustc_middle::mir::Local,
    #[serde(serialize_with = "local")]
    pub holder: rustc_middle::mir::Local,
    #[serde(serialize_with = "location")]
    pub arrival: rustc_middle::mir::Location,
    pub row: usize,
    pub holders: BTreeMap<String, ClosureResultHolder>,
}

#[derive(Serialize)]
pub struct ClosureResultHolder {
    pub loans: BTreeSet<usize>,
    /// Loans also carried by an ordinary argument into this place.
    pub argument_loans: BTreeSet<usize>,
}

/// Within one body, a declaration identifies a user MIR local. Several locals
/// can intentionally share a visual column in disjoint scopes.
#[derive(Debug, Default)]
pub struct BodyBindings {
    pub locals: BTreeMap<rustc_middle::mir::Local, String>,
    pub arrays: BTreeMap<rustc_middle::mir::Local, crate::svg_generator::data::ElemsLeft>,
    pub emptied_boxes: BTreeSet<rustc_middle::mir::Local>,
    pub declarations: std::collections::HashMap<rustc_hir::HirId, rustc_middle::mir::Local>,
    pub columns: BTreeMap<String, Vec<rustc_middle::mir::Local>>,
}

#[derive(Serialize)]
pub struct BodyFacts {
    /// Mutable call operands whose own loan ends on the normal return edge.
    #[serde(skip)]
    pub short_call_borrows: BTreeSet<(rustc_span::Span, usize)>,
    /// Executable ends of named locals and their owned field columns, including
    /// storage with no drop glue. Each binding keeps its own MIR identity.
    #[serde(skip)]
    pub(crate) scope_ends: ScopeEnds,
    #[serde(skip)]
    pub(crate) value_flow: super::value_flow::ValueFlow,
    /// Number of owned resource fields in the binding's closure type.
    #[serde(skip)]
    pub closure_captures: BTreeMap<String, usize>,
    /// Places a callee can write through a mutable operand, paired with
    /// the loans carried by the source operand at that exact MIR call.
    #[serde(skip)]
    pub(crate) argument_stores: std::collections::HashMap<(rustc_span::Span, usize), Vec<(String, BTreeSet<usize>)>>,
    /// Named holders still carrying each loan when their destructor runs.
    #[serde(skip)]
    pub drop_holders: BTreeMap<usize, BTreeSet<String>>,
    /// Old loans kept alive by drop-use at an assignment's destructor.
    #[serde(skip)]
    pub(crate) reassign_drop_loans: BTreeMap<Location, BTreeSet<usize>>,
    /// Reference fields taken by moves into `std::mem::drop` in a
    /// short-circuit operand, keyed by the instruction moving the place.
    #[serde(skip)]
    pub(crate) drop_moves: BTreeMap<Location, DropMove>,
    #[serde(skip)]
    pub(crate) short_circuit_branches: rustc_data_structures::fx::FxHashSet<rustc_hir::HirId>,
    /// Named source fields proven to carry a loan into an explicit drop.
    #[serde(skip)]
    pub(crate) explicit_drop_ends: BTreeMap<(Location, usize), BTreeSet<EndPlace>>,
    #[serde(skip)]
    pub bindings: std::rc::Rc<BodyBindings>,
    /// Loans carried by the operands of this MIR aggregate, in operand order.
    #[serde(skip)]
    pub aggregate_loans: std::collections::HashMap<rustc_span::Span, Vec<AggregateLoan>>,
    /// Loans in the outer reference origin on this call's normal return edge.
    #[serde(skip)]
    pub(crate) returned_reference_loans: BTreeMap<Location, BTreeSet<usize>>,
    /// Loans from a closure operand that reach the named call destination.
    #[serde(serialize_with = "closure_calls", skip_serializing_if = "ClosureResults::is_empty")]
    pub(crate) closure_result_loans: ClosureResults,
    /// Loans carried by a non-receiver MIR operand into the receiver, indexed
    /// by the call and the receiver place.
    #[serde(skip)]
    pub receiver_loans: std::collections::HashMap<(rustc_span::Span, String), Vec<ReceiverLoan>>,
    /// A single proven source for this holder at release. True when its whole owner is also usable.
    #[serde(skip)]
    pub(crate) direct_releases: BTreeMap<(usize, Occurrence), bool>,
    /// A reborrow end after which no loan still restricts its parent on this CFG edge.
    #[serde(skip)]
    pub restored_reborrows: BTreeSet<(rustc_middle::mir::Location, rustc_middle::mir::Location, usize)>,
    /// Holder ends absorbed into a give-back on the same row. The edge still
    /// identifies where this individual copy stopped carrying its loan.
    #[serde(skip)]
    pub inline_holder_ends: Vec<(usize, Ev)>,
    #[serde(skip)]
    pub evaluated_spans: Vec<rustc_span::Span>,
    /// Mutable reborrows of a whole referent at the operand's MIR source span.
    #[serde(skip)]
    pub implicit_mut_reborrows: BTreeSet<(rustc_span::Span, rustc_middle::mir::Local)>,
    /// Assignment and call sites, with their reachability on the runtime CFG.
    #[serde(skip)]
    pub effect_sites: BTreeMap<rustc_span::Span, Vec<(rustc_middle::mir::BasicBlock, bool)>>,
    /// MIR replacement drops keyed by the LHS span and its local. A site
    /// can be lowered more than once, so this records whether any copy runs.
    #[serde(skip)]
    pub reassignment_sites: BTreeMap<(rustc_span::Span, rustc_middle::mir::Local), bool>,
    /// Drop sites with a flag before the additional runtime CFG refinements.
    #[serde(skip)]
    pub drop_flags: BTreeSet<rustc_middle::mir::Location>,
    /// Drop sites where one part of the place has its own flag.
    #[serde(skip)]
    pub drop_part_flags: BTreeSet<(rustc_middle::mir::Location, String)>,
    /// Borrow issues whose references are staged directly in an aggregate.
    #[serde(skip)]
    pub issued_operand_loans: BTreeSet<usize>,
    /// Loans whose references themselves reach an aggregate assigned to the return place.
    #[serde(skip)]
    pub returned_operand_loans: BTreeSet<usize>,
    /// A path reaching an end of this loan can bypass the borrow altogether.
    #[serde(skip)]
    pub conditional_ends: BTreeSet<usize>,
    #[serde(skip)]
    pub imaginary_edges: Vec<ImaginaryEdge>,
    /// Standard vec! constructions that can complete on the body's normal CFG.
    #[serde(skip)]
    pub constructed_vecs: std::collections::HashMap<rustc_span::hygiene::ExpnId, usize>,
    #[serde(skip)]
    pub def_id: rustc_hir::def_id::DefId,
    /// Source spans of real MIR calls to the standard ManuallyDrop::take.
    #[serde(skip)]
    pub take_calls: BTreeSet<rustc_span::Span>,
    /// Blocks only reached by assuming a local non-returning closure can
    /// return, with the initiating call when there is only one.
    #[serde(skip)]
    pub skipped_closure_returns: BTreeMap<BasicBlock, Option<Location>>,
    /// `main`, `Foo::bar`, `main::{closure#0}`.
    pub func: String,
    /// "fn" or "closure".
    pub kind: &'static str,
    /// For a closure: the body it is defined in.
    pub parent: Option<String>,
    pub first_line: usize,
    pub last_line: usize,
    /// Source position of a function body's opening brace (line, zero-based column).
    #[serde(skip)]
    pub function_body_start: Option<(usize, usize)>,
    pub vars: Vec<VarInfo>,
    /// The places that get a state of their own: variables, their field
    /// paths (`p.a`, `t.0`) and Box payloads (`*b`, `b.a`).
    pub columns: Vec<ColumnInfo>,
    /// Columns whose MIR places all have the same Option reference type
    /// classification, including where a boundary has no state.
    #[serde(skip)]
    pub uniform_option_refs: BTreeMap<String, bool>,
    pub switches: Vec<SwitchExit>,
    pub loans: Vec<LoanInfo>,
    /// Immutable bindings initialized from the Some payload of a slice iterator in a for loop.
    #[serde(skip)]
    pub slice_items: BTreeSet<rustc_middle::mir::Local>,
    /// Borrowck loans at MIR assignments to pattern bindings.
    #[serde(skip)]
    pub binding_loans: BTreeMap<rustc_middle::mir::Local, BTreeMap<rustc_middle::mir::Location, Vec<usize>>>,
    pub rows: Vec<Row>,
    /// Drop elaboration at individual residual edges, including named
    /// fields covered by a whole-place drop and unnamed temporaries.
    #[serde(skip)]
    pub residual_drops: Vec<ResidualDrop>,
    /// Temporaries that hold a value needing drop.
    pub temps: Vec<TempInfo>,
    /// Source anchors of typed operands rustc lifted into promoted MIR.
    #[serde(skip)]
    pub(crate) promoted_operands: rustc_data_structures::fx::FxHashSet<rustc_hir::HirId>,
    pub notes: Vec<String>,
    /// Source lines whose code borrowck checks but that never runs: reached
    /// only by borrowck's imaginary edges (the dead `else` of an irrefutable
    /// `if let`). Their rows' states are that imaginary path's.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub dead_lines: BTreeSet<usize>,
    /// Binding assignments on both checked and executable MIR paths.
    #[serde(skip)]
    pub binding_sources: Vec<BindingSource>,
    /// The state at a conditional's normal MIR continuation, before its first instruction.
    #[serde(serialize_with = "conditional_joins", skip_serializing_if = "rustc_data_structures::fx::FxIndexMap::is_empty")]
    pub joins: rustc_data_structures::fx::FxIndexMap<rustc_hir::HirId, Join>,
    /// Loop boundaries keyed by the compiler's loop expression.
    #[serde(serialize_with = "loop_states", skip_serializing_if = "rustc_data_structures::fx::FxIndexMap::is_empty")]
    pub loops: rustc_data_structures::fx::FxIndexMap<rustc_hir::HirId, LoopState>,
    /// The state at the first MIR decision of a drawn conditional.
    #[serde(skip)]
    pub branch_entries: rustc_data_structures::fx::FxIndexMap<rustc_hir::HirId, State>,
    /// States at the normal entry and continuing edges of drawn arms.
    #[serde(skip)]
    pub arm_boundaries: rustc_data_structures::fx::FxIndexMap<rustc_hir::HirId, BTreeMap<usize, ArmBoundaries>>,
}

pub(crate) struct DropMove {
    pub local: Local,
    pub fields: Vec<String>,
}

fn loop_states<S: serde::Serializer>(
    loops: &rustc_data_structures::fx::FxIndexMap<rustc_hir::HirId, LoopState>, s: S,
) -> Result<S::Ok, S::Error> {
    s.collect_seq(loops.values())
}

#[derive(Serialize)]
pub struct LoopState {
    #[serde(serialize_with = "location")]
    pub at: Location,
    pub row: usize,
    pub entry: State,
    pub head: State,
    pub next_iteration: Option<State>,
    pub continuation: Option<State>,
    pub edges: Vec<LoopEdge>,
    #[serde(skip)]
    pub constant_pruned_backedge: bool,
    #[serde(skip)]
    pub changes: BTreeMap<String, LoopChange>,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LoopEdgeKind {
    Entry,
    Backedge,
    Break,
    Exhaustion,
}

#[derive(Serialize)]
pub struct LoopEdge {
    #[serde(serialize_with = "location")]
    pub from: Location,
    #[serde(serialize_with = "location")]
    pub to: Location,
    pub kind: LoopEdgeKind,
    pub runs: bool,
    #[serde(skip)]
    pub jump: Option<rustc_hir::HirId>,
    pub state: State,
}

#[derive(Default)]
pub struct LoopChange {
    pub moved: bool,
    pub moved_part: bool,
    pub entry_empty_at_runtime: bool,
    pub wrote: bool,
    pub writes_on_all_exits: bool,
    pub carried: bool,
    pub moved_away: bool,
    pub wrote_away: bool,
    pub changes_next_iteration: bool,
    pub moved_at_break: bool,
    pub branch_move: bool,
    pub moved_at_exhaustion: bool,
}

#[derive(Default)]
pub struct ArmBoundaries {
    pub entry: Option<State>,
    pub join: Option<State>,
}

fn conditional_joins<S: serde::Serializer>(
    joins: &rustc_data_structures::fx::FxIndexMap<rustc_hir::HirId, Join>, s: S,
) -> Result<S::Ok, S::Error> {
    s.collect_seq(joins.values())
}

#[derive(Serialize)]
pub struct Join {
    #[serde(serialize_with = "location")]
    pub at: Location,
    #[serde(flatten)]
    pub state: State,
    /// Checked incoming edges, including any edges that cannot execute.
    pub incoming: Vec<JoinEdge>,
    /// Effects within the conditional on paths that reach its join.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub changes: BTreeMap<String, JoinChange>,
    /// A real write reaches this join on one edge. The other leaves the
    /// place empty (`Sometimes`), skips initialization only in borrowck
    /// (`Always`), or runs no write at all (`Skipped`).
    #[serde(skip)]
    pub assignments: BTreeMap<String, JoinAssignment>,
    /// Shared reference bindings that keep their existing value.
    #[serde(skip)]
    pub unchanged_shared_refs: BTreeMap<rustc_middle::mir::Local, SharedReferenceJoin>,
    #[serde(skip)]
    pub dead_implicit_else: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedReferenceJoin {
    Unchanged,
    /// The binding no longer keeps at least one of its incoming loans live.
    LoanEnded,
}

#[derive(Serialize)]
pub struct JoinEdge {
    #[serde(serialize_with = "location")]
    pub from: Location,
    pub runs: bool,
    pub state: BTreeMap<String, &'static str>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct JoinChange {
    pub moved: bool,
    pub moves_on_all_paths: bool,
    pub wrote: bool,
    pub writes_on_all_paths: bool,
    pub entry_maybe: bool,
    /// Some continuing path can write a replacement after moving the old value.
    pub replaces_moved: bool,
    /// The join produced a value in a staging place which this destination receives.
    pub staged_result: bool,
    /// The condition or match guard itself moved this place.
    pub test_moved: bool,
    pub test_wrote: bool,
    pub test_writes_on_all_paths: bool,
    pub writes_away: bool,
    pub entry_own: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JoinAssignment {
    Sometimes,
    Always,
    Skipped,
}

pub struct BindingSource {
    /// Source local, pictured column, and the part taken by the binding.
    pub projected_move: Option<(rustc_middle::mir::Local, String, String)>,
    /// A fixed array slot used as the operand of a shared reference copy.
    pub(crate) fixed_array_copy: Option<(rustc_middle::mir::Local, String, String)>,
    pub location: rustc_middle::mir::Location,
    pub(crate) row: usize,
    pub runs: bool,
    pub declaration: rustc_middle::mir::Local,
    pub site: rustc_span::Span,
    pub into: String,
    pub from: Option<String>,
    /// The source's named enclosing fields and its named subplaces.
    pub parents: Vec<String>,
    pub covers: Vec<String>,
    pub moves: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OtherBindings {
    pub moves: BTreeSet<(String, String)>,
    pub retained: bool,
    /// Some sibling takes an unpictured projection, such as an enum payload.
    pub unpictured_move: bool,
}

impl OtherBindings {
    /// Compare complete sets of simultaneous MIR pattern bindings. A move
    /// of an enclosing place carries its fields; a field move takes only
    /// that field out of the enclosing place.
    pub fn for_place(sites: &[&BindingSource], declaration: rustc_middle::mir::Local, place: &str) -> Self {
        let mut result = Self::default();
        let declarations: BTreeSet<_> = sites.iter().map(|s| s.declaration).collect();
        let mut groups = BTreeMap::<_, Vec<_>>::new();
        for &s in sites { groups.entry(s.location.block).or_default().push(s); }
        for group in groups.values() {
            let mut touched = false;
            for s in group.iter().filter(|s| s.moves) {
                if s.covers.iter().chain(&s.parents).any(|p| p == place) {
                    touched = true;
                    if s.declaration != declaration {
                        if let Some(from) = &s.from {
                            result.moves.insert((from.clone(), s.into.clone()));
                        } else {
                            result.unpictured_move = true;
                        }
                    }
                }
            }
            if !touched && group.iter().all(|s| !s.moves || !s.covers.is_empty())
                && group.iter().map(|s| s.declaration).collect::<BTreeSet<_>>() == declarations {
                result.retained = true;
            }
        }
        result
    }
}

/// The source span of an arm that only the borrow checker reaches.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
pub struct ImaginaryEdgeId(pub rustc_span::Span);

pub struct ImaginaryEdge {
    pub id: ImaginaryEdgeId,
    pub entry: State,
    /// Borrow-checker states after this arm's written code on each row.
    pub after: BTreeMap<usize, State>,
    /// Borrow-checker state where this arm would rejoin executable code.
    pub join: State,
    /// Changes written on the dead arm, with runtime overlap marked.
    pub changes: BTreeMap<String, ImaginaryChange>,
    /// Places emptied while evaluating the scrutinee before matching.
    pub scrutinee_moves: BTreeSet<String>,
    /// Columns with live storage whose binding enters scope after its initializer.
    pub initializer_bindings: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImaginaryChange {
    Move,
    Assign,
    AlsoChangesAtRuntime,
}

#[derive(Serialize)]
pub struct VarInfo {
    /// The column name the 2.x visitor gives it (`s`, `s₂`).
    pub name: String,
    pub local: usize,
    pub decl_line: usize,
    /// "owner" | "shared_ref" | "mut_ref"
    pub kind: &'static str,
    pub copy: bool,
    pub needs_drop: bool,
    pub ty: String,
    /// In a closure body: a captured variable of the enclosing body
    /// ("by_value" | "by_ref" | "by_mut_ref"); None for its own locals.
    pub capture: Option<&'static str>,
}

#[derive(Serialize)]
pub struct ColumnInfo {
    #[serde(skip)]
    pub local: rustc_middle::mir::Local,
    pub name: String,
    /// Nearest displayed ancestor, obtained from the MIR place projection.
    #[serde(skip)]
    pub parent: Option<String>,
    /// The variable it belongs to.
    pub root: String,
    /// "var" | "field" | "box_payload"
    pub kind: &'static str,
    pub copy: bool,
    pub needs_drop: bool,
    /// It has fields this body can't name (private to another module):
    /// they have no columns, so its field columns aren't all of it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden_fields: bool,
    #[serde(skip)]
    pub array_len: Option<usize>,
    #[serde(skip)]
    pub is_box: bool,
}

fn local<S: serde::Serializer>(local: &rustc_middle::mir::Local, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64(local.index() as u64)
}

fn optional_local<S: serde::Serializer>(local: &Option<rustc_middle::mir::Local>, s: S) -> Result<S::Ok, S::Error> {
    local.map(|l| l.index()).serialize(s)
}

#[derive(Serialize, Clone, Debug)]
pub struct LoanInfo {
    #[serde(skip)]
    pub issue: Option<rustc_middle::mir::Location>,
    /// Base local of the actual borrowed place, including projected places.
    #[serde(serialize_with = "optional_local", skip_serializing_if = "Option::is_none")]
    pub lender_local: Option<rustc_middle::mir::Local>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_on: Option<SwitchEdge>,
    /// Switch edges required to reach the MIR issuance, including enclosing
    /// decisions on other source rows. The row-local edge above is for layout.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issuance_path: Vec<SwitchEdge>,
    pub loan: usize,
    /// "borrow": a borrow borrowck tracks (its `BorrowIndex`) of a user
    /// place. The others restrict no column (`columns` is empty, no `lent`
    /// entry); all but "temporary" have ids after borrowck's:
    /// - "caller": what a reference parameter holds (`self` of `&mut
    ///   self`): the caller's loan, `lender` `*p`, on the signature's row.
    ///   It is in use while the parameter is used or anything made from it
    ///   (a copy, a reborrow, a call's result) holds it; it ends where
    ///   nothing does, and doesn't end on a path that returns it.
    /// - "static": a reference to a constant (`"hi"`, a promoted `&5`) that
    ///   a variable keeps; `lender` is the constant as written.
    /// - "temporary": a borrow (tracked) of a MIR temporary that a variable
    ///   keeps (`let t = &String::from("x");`, `&make().0`): `lender` is the
    ///   temporary's code in parentheses, then the borrowed field.
    /// - "untracked": a borrow borrowck ignores because nothing can
    ///   conflict with it: a shared reborrow through a shared reference
    ///   (`&*r`, `&r.f`, the auto-ref of `r.len()`), or a borrow of an
    ///   immutable parameter that is never moved.
    pub source: &'static str,
    pub line: usize,
    pub col: usize,
    /// The borrowed place as the source writes it (`s`, `p.a`, `v[_]`,
    /// `*b`, `(o as Some).0`).
    pub lender: String,
    /// Field suffix of a temporary lender, resolved on its MIR type.
    /// The root is identified by lender_local even with identical source text.
    #[serde(skip)]
    pub(crate) temp_fields: Option<String>,
    /// Direct binding of a borrow of the whole temporary and whether
    /// that borrowed type has drop glue.
    #[serde(skip)]
    pub(crate) temp_borrow: Option<(rustc_middle::mir::Local, bool)>,
    #[serde(skip)]
    pub(crate) projected: Option<ProjectedPlace>,
    #[serde(skip)]
    pub(crate) payload: Option<ProjectedPlace>,
    /// The columns this loan restricts (every column whose place
    /// overlaps the borrowed place: `v` for `v[_]`, `p` and `p.a` for
    /// `p.a`).
    pub columns: Vec<String>,
    /// Overlapping columns covered by the entire borrow, including a
    /// Box that lends its whole payload.
    #[serde(skip)]
    pub whole: BTreeSet<String>,
    #[serde(skip)]
    pub direct_holder: Option<String>,
    /// The direct destination keeps a reference to the borrowed reference slot.
    #[serde(skip)]
    pub holds_ref_slot: bool,
    #[serde(skip)]
    pub runs: bool,
    /// Conflicting columns for which the borrowed MIR place selects a field
    /// or element inside them. Dereferencing a whole Box alone is not a
    /// field or element borrow.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub part_of: Vec<String>,
    /// "shared" | "mut" | "unique" (a closure's unique immutable
    /// capture: the closure may not mutate it, but nobody else may use it)
    pub kind: &'static str,
    /// A two-phase borrow (`v.push(v.len())`: reserved first, activated
    /// at the call).
    pub two_phase: bool,
    /// The reference the place is reached through (`r` for `&mut *r`,
    /// i.e. a reborrow): the presentation says "through r".
    pub through: Option<String>,
    /// Everyone who holds the loan at some point: the place it is stored
    /// in (`r`, a struct field `h.r`), then other variables in the order
    /// they first hold it (`s` after `let s = r;`). Who holds it on a
    /// given row: `Lent.holder`.
    pub holders: Vec<String>,
    /// The call it is passed to, when no variable keeps it
    /// (`push_str`, `len`, `println!`).
    pub to_call: Option<String>,
    /// Taken in a macro's expansion, or on an argument the expansion
    /// wraps (`println!("{s}")` borrows `s`): the macro, as written. A
    /// `macro_rules!` that pastes its argument unchanged leaves it
    /// ordinary code (rustc's MIR has no trace of the macro there).
    pub in_macro: Option<String>,
    /// Rows where the loan goes out of scope on some CFG edge, sorted and
    /// deduplicated (the edges themselves: the `BorrowEnd` events).
    pub end_lines: Vec<usize>,
    /// For a borrow through a reference (`through`: `&mut *a`, `&a.f` of a
    /// reference `a`, a caller's `&*self`): the loans that reference held
    /// where this one was taken, sorted. Giving this loan back gives it to
    /// `through`, which still holds those (the reborrow chain).
    pub made_through: Vec<usize>,
    /// The places whose own reference value is this loan: where it was
    /// stored (`r`, a field `h.r`, the parameter of a "caller" loan) and
    /// every copy or move of that reference (`let z = y;`, `S { ..s1 }`,
    /// `Holder { r: y }`), in the order the body makes them. `holders`
    /// also has those that hold it only through a reborrow of it, a call's
    /// result or a reference to a holder.
    pub direct_holders: Vec<String>,
    /// Exact projected stores that may install this loan's reference.
    /// The kind describes the stored reference, including a shared
    /// reference reached through an originally mutable loan.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub holder_stores: Vec<HolderStore>,
    /// Named locals carrying the reference directly, including MIR copies and moves.
    #[serde(skip)]
    pub direct_locals: BTreeSet<rustc_middle::mir::Local>,
    /// Variables that hold it through a call's result: the call was given
    /// the loan, or a reference to something holding it, and its result
    /// carries it (`let b = s.as_bytes();`, `let m = v.iter().max();`, `for
    /// item in list`), and copies of those. Named by the fields whose types
    /// carry the loan as rustc's lifetimes say: `pair<'a>(x: &'a mut A, y:
    /// &'a mut A) -> (&'a mut A, &'a mut A)` gives both loans to `t.0` and
    /// `t.1`, as borrowck counts them (which referent each field points to
    /// is in the callee's body, not this one's).
    pub via_call: Vec<String>,
    /// The borrow isn't written in the source: a method call's auto-ref
    /// (`v.len()`), the reborrow of a reference that is passed on (`G(r)`,
    /// `Holder { r }`, `f(m)` with `m: &mut T`), a macro's. (Not for a
    /// parameter's, a constant's or a temporary's loan.)
    pub implicit: bool,
}

impl LoanInfo {
    pub(crate) fn alternative_to(&self, other: &Self) -> bool {
        self.issuance_path.iter().any(|a| other.issuance_path.iter()
            .any(|b| a.switch == b.switch && a.successor != b.successor))
    }
}

/// One place's state after a row (or an arm of a row).
#[derive(Serialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Lent {
    pub loan: usize,
    /// "shared" | "mut" | "unique" (a closure's unique immutable capture)
    /// | "reserved" (a two-phase `&mut` before its activation: the place
    /// can still be read, as in `v.push(v.len())`).
    pub kind: &'static str,
    /// The variable whose value holds the loan on this row (`r`, or `h`
    /// for a field `h.r`; `s` once `let s = r;` ran), or the call / macro
    /// it was passed to.
    pub holder: String,
    /// The hidden iterator carries this loan at this boundary.
    #[serde(skip)]
    pub iterator: bool,
    /// Named places proven to carry this loan here. Multiple independent
    /// holders need not denote the same borrow on every incoming path.
    #[serde(skip)]
    pub current: BTreeSet<String>,
    /// The drop-use origins of this holder carry the loan to a reassignment drop.
    #[serde(skip)]
    pub drop_holder: Option<DropLoanHolder>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropLoanHolder {
    pub name: String,
    pub some_paths: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HolderType {
    /// The local matters when different bindings share a display column.
    OptionRef { local: usize, copy: bool },
    /// A primitive sharing a column with an Option cannot hold borrows.
    Primitive,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Parts {
    /// Field paths moved out on every path.
    pub moved: Vec<String>,
    /// Field paths moved out on some paths.
    pub maybe: Vec<String>,
}

pub struct ResidualDrop {
    pub local: rustc_middle::mir::Local,
    pub site: rustc_span::Span,
    pub path: Option<String>,
    pub how: &'static str,
    /// The initialized paths come from assignments and no move can
    /// explain why the drop is conditional.
    pub only_assigned: bool,
    pub staged: bool,
}

/// What happens to the named fields of a partially moved aggregate at a
/// drop site. These lists come from the executable CFG at that site.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartialDrop {
    /// The elaborated drop ladder for this place tests a Boolean.
    pub flag: bool,
    pub moved: Vec<String>,
    pub conditional: Vec<String>,
    /// Only a replacement can reach the drop of these fields.
    pub refilled: Vec<String>,
    /// The two conditional fields have complementary initialized paths.
    pub one_of_two: bool,
    pub dropped: Vec<String>,
    /// A value was taken from inside this field, which still has a column.
    pub payloads_moved: Vec<String>,
    pub payloads_conditional: Vec<String>,
    /// No value inside this enum field reaches its destructor.
    pub payloads_empty: Vec<String>,
    pub variant_drops: Vec<VariantDrop>,
    pub drops_resource: bool,
    /// Part of the drop has no named field.
    pub incomplete: bool,
}

/// Drop information for a variant of a partially moved enum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariantDrop {
    pub column: String,
    pub variant: String,
    pub payload: VariantPayload,
    /// The drop ladder tests a Boolean guarding the downcast move path.
    pub tracks_variant_init: bool,
    /// A tested Boolean guards the variant payload.
    pub payload_flag: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VariantPayload {
    Empty,
    Copy(String),
    Moved(String),
    Conditional(String),
}

/// The borrow checker considers both a skipped assignment and a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixedInit {
    pub may_hold_value: bool,
    pub may_hold_value_at_runtime: bool,
    pub whole_move: bool,
    pub imaginary_move: bool,
    /// At a join, a write reaches it from one arm and another skips it.
    pub assigned_in_some_arms: bool,
}

/// States of the columns after a row, as the next row sees them.
#[derive(Serialize, Clone, Default)]
pub struct State {
    /// The MIR local for the active place on a presentation column. A boundary
    /// reached with different sibling bindings has no single local for it.
    #[serde(skip)]
    pub locals: BTreeMap<String, Option<rustc_middle::mir::Local>>,
    /// column -> "own" | "moved" | "maybe" | "partly" | "unassigned"
    pub state: BTreeMap<String, &'static str>,
    /// For "partly" columns (and "own" columns some of whose fields may
    /// have been moved): which fields.
    pub parts: BTreeMap<String, Parts>,
    /// column -> loans in scope that restrict it.
    pub lent: BTreeMap<String, Vec<Lent>>,
    /// For a "maybe" column: "moved" (moved out on some way here),
    /// "unassigned" (never given a value on some way here) or "imaginary"
    /// (moved only on a way borrowck checks but that never runs: an
    /// or-pattern's unreachable alternative; still not usable).
    pub cause: BTreeMap<String, &'static str>,
    /// For wording where both a move and a skipped assignment reach the
    /// boundary. Runtime drop wording uses the executable paths (Q5).
    #[serde(skip)]
    pub mixed_init: BTreeMap<String, MixedInit>,
    /// variable column (a whole local: `r`, `h`; not a field column such
    /// as `h.r` or `p.0`) -> the loans its value keeps borrowed where the
    /// next row of the same path starts, as the borrow checker counts
    /// them: the loans its origins contain at the last point of the row's
    /// path and at the next row's first point (Polonius
    /// `origin_contains_loan_at`), by loan id, sorted. A dead reference
    /// (no later use), a moved-out or unassigned variable, holds none;
    /// columns that hold none are left out. This can be more than what it
    /// points to when it runs: a field overwrite (`h.r = &b`) leaves `a`'s
    /// loan in `h`, and a copy made before it (`let old = h.r`) gets `b`'s
    /// too, as rustc counts them.
    pub holds: BTreeMap<String, Vec<usize>>,
    /// Active MIR types for Option reference holders and primitives that
    /// share their display columns.
    #[serde(skip)]
    pub holder_types: BTreeMap<String, HolderType>,
    /// Initialized ManuallyDrop wrappers whose contents a resolved unsafe
    /// call took out on at least one real path to this boundary.
    #[serde(skip)]
    pub manually_taken: BTreeMap<String, Extraction>,
    /// Loans live through an Option field or a reference to the whole Option.
    #[serde(skip)]
    pub option_holds: BTreeMap<String, Vec<usize>>,
    /// Borrow checking permits an empty place here, but every executable
    /// path reaches this boundary with the place initialized.
    #[serde(skip)]
    pub always_assigned: BTreeSet<String>,
    /// The runtime paths all moved the place, but borrow checking still
    /// counts a path that holds a value (a write only a dead path reaches).
    #[serde(skip)]
    pub borrowck_maybe: BTreeSet<String>,
    /// A partial value whose remaining fields have no destructor to run.
    #[serde(skip)]
    pub partial_without_drop: BTreeSet<String>,
    /// For a possibly empty place, a later assignment before scope exit on
    /// every (true) or only some (false) continuing runtime paths.
    #[serde(skip)]
    pub reassigned: BTreeMap<String, bool>,
    /// A cleanup edge before the next write can drop the previous value.
    #[serde(skip)]
    pub unwind_before_write: BTreeSet<String>,
    /// Only at an expression join: a move inside an arm reaches one
    /// executable incoming edge and another still owns the value.
    #[serde(skip)]
    pub moved_in_arm: BTreeSet<String>,
    /// A real CFG path to this boundary can bypass all the live loans
    /// of the strongest kind restricting the column.
    #[serde(skip)]
    pub may_lend: BTreeSet<String>,
    /// Columns a part of which, with no name of its own (an enum payload),
    /// was moved out on some paths only (`parts` can't list it).
    #[serde(skip)]
    pub unnamed_maybe: BTreeSet<String>,
    /// A Box whose payload is missing on some incoming paths.
    #[serde(skip)]
    pub maybe_empty_box: BTreeSet<String>,
    /// Named values whose origins contain a live loan at this boundary.
    #[serde(skip)]
    pub holder_sources: BTreeMap<String, BTreeSet<usize>>,
}

#[derive(Serialize, Default)]
pub struct Row {
    pub line: usize,
    /// A single forward MIR edge with no join or fork. Blank display rows
    /// up to this successor lie on the same path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continues_to: Option<usize>,
    /// The columns' states after the row, as the next row of the same
    /// path sees them. A row with no MIR location on a path that goes on
    /// (blank, `} else {`, the `};` of a `let ... else { return }`) is
    /// absent or has empty maps: the previous row's state holds.
    #[serde(flatten)]
    pub after: State,
    /// State on forward edges entering this row, before its first effect.
    /// Used for the straight segment that meets it, including at CFG joins.
    #[serde(skip)]
    pub before: Option<State>,
    /// Whether any of this row's forward incoming edges can execute.
    #[serde(skip)]
    pub runtime_entry: Option<bool>,
    /// Bindings initialized by a labeled-block result on this row,
    /// at the end of the row on paths through MIR's completed-binding read.
    #[serde(skip)]
    pub completed_bindings: Vec<State>,
    #[serde(skip)]
    pub block_end: bool,
    /// Snapshot at the MIR read of a completed statement binding.
    #[serde(skip)]
    pub completed_let: bool,
    /// The row's code that runs leaves it only by jumps past the next row
    /// the paths come into (`break k;`, `return i;`).
    #[serde(skip)]
    pub jumps: bool,
    /// Events on the row, in MIR evaluation order.
    pub facts: Vec<Ev>,
    #[serde(skip)]
    pub partial_drops: Vec<(Option<rustc_span::Span>, rustc_middle::mir::Local, String, PartialDrop)>,
    /// Several arms of one branch on this row (`if c { a } else { b }`
    /// on one line): each arm's own state at its exit. Recorded for the
    /// presentation layer; how to draw them is a design decision.
    pub arms: Vec<Arm>,
}

/// Coordinates in borrowck MIR, serialized independently of rustc's debug format.
fn location<S: serde::Serializer>(loc: &Location, s: S) -> Result<S::Ok, S::Error> {
    (loc.block.index(), loc.statement_index).serialize(s)
}

fn optional_location<S: serde::Serializer>(loc: &Option<Location>, s: S) -> Result<S::Ok, S::Error> {
    loc.map(|l| (l.block.index(), l.statement_index)).serialize(s)
}

fn block<S: serde::Serializer>(bb: &BasicBlock, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64(bb.index() as u64)
}

/// A normal successor of one SwitchInt in this body. Several switch values
/// leading to the same block take the same CFG edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct SwitchEdge {
    #[serde(serialize_with = "location")]
    pub switch: Location,
    #[serde(serialize_with = "block")]
    pub successor: BasicBlock,
}

#[derive(Serialize)]
pub struct SwitchExit {
    #[serde(flatten)]
    pub edge: SwitchEdge,
    #[serde(skip)]
    pub branch: Option<rustc_hir::HirId>,
    /// Pattern whose MatchCmp result is being tested, before any guard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tested_pattern: Option<usize>,
    /// Drawn arm entered, when this test commits to one.
    pub arm: Option<usize>,
}

/// The effect of drop elaboration on one displayed place. Ending storage
/// is a separate event from running drop glue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeDrop {
    Storage,
    ExitStorage,
    ByPath,
    Empty,
    Whole,
    Conditional,
    Remaining,
}

impl ScopeDrop {
    pub(crate) fn runs(self) -> bool {
        matches!(self, Self::Whole | Self::Conditional | Self::Remaining | Self::ByPath)
    }

    pub(super) fn from_how(how: &str) -> Self {
        match how {
            "drops" => Self::Whole,
            "drops_if_init" => Self::Conditional,
            "drops_rest" => Self::Remaining,
            "nothing_to_drop" => Self::Empty,
            _ => unreachable!("unexpected drop kind {how}"),
        }
    }
}

/// Initialized and missing array slots at one MIR drop. Names label typed
/// ConstantIndex projections, including indices expanded from a Subslice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScopeArray {
    pub empty_payload: Vec<String>,
    pub moved_payload: Vec<String>,
    pub absent_variant: Vec<(String, String)>,
    pub may_be_refilled: bool,
    pub moved: Vec<String>,
    pub conditional: Vec<String>,
    pub left: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScopeEnd {
    /// This edge is an alternative to the normal return of a call.
    pub optional_unwind: bool,
    pub legs: Vec<rustc_span::Span>,
    pub array: Option<ScopeArray>,
    pub partial: Option<PartialDrop>,
    pub span: rustc_span::Span,
    /// Ordinary storage boundary, also when an inline exit stands in for it.
    pub boundary: usize,
    pub has_value: bool,
    pub payload_moved: bool,
    pub payload_maybe: bool,
    pub box_contents_need_drop: Option<bool>,
    pub row: usize,
    pub site: Option<rustc_span::Span>,
    pub effect: ScopeDrop,
    /// At a shared destructor this can be true even when this particular
    /// exit always (or never) supplies the value.
    pub flag: bool,
    pub flag_on_some_paths: bool,
    /// Field indices in declaration order within this MIR local.
    pub field_order: Vec<usize>,
    pub moved: bool,
    pub unassigned: bool,
}

impl ScopeEnd {
    /// A collapsed column can summarize several incoming edges of one
    /// destructor. Keep only the properties shared by those occurrences.
    pub(crate) fn across(ends: &[&Self]) -> Option<Self> {
        let mut end = (*ends.iter().min_by_key(|e| (e.span, e.boundary, e.row))?).clone();
        let same_site = ends.iter().all(|e| e.site == end.site);
        if !ends.iter().all(|e| e.span == end.span) && !(same_site && end.site.is_some()) { return None; }
        end.legs.retain(|leg| ends.iter().all(|e| e.legs.contains(leg)));
        if !ends.iter().all(|e| e.array == end.array) { end.array = None; }
        if !ends.iter().all(|e| e.partial == end.partial) { end.partial = None; }
        end.flag = ends.iter().all(|e| e.flag);
        end.flag_on_some_paths = !end.flag && ends.iter().any(|e| e.flag || e.flag_on_some_paths);
        if !ends.iter().all(|e| e.effect == end.effect) {
            end.effect = if end.flag && !ends.iter().any(|e| e.effect == ScopeDrop::ByPath) {
                ScopeDrop::Conditional
            } else if ends.iter().all(|e| e.site.is_none()) {
                ScopeDrop::ByPath
            } else { return None };
        }
        end.moved = ends.iter().any(|e| e.moved);
        end.unassigned = ends.iter().any(|e| e.unassigned);
        end.payload_moved = ends.iter().all(|e| e.payload_moved);
        end.payload_maybe = ends.iter().any(|e| e.payload_maybe || e.payload_moved != end.payload_moved);
        end.has_value = ends.iter().any(|e| e.has_value);
        end.optional_unwind = ends.iter().all(|e| e.optional_unwind);
        if !same_site { end.site = None; }
        Some(end)
    }
}

#[derive(Default, Debug, Clone)]
pub(crate) struct ScopeEnds {
    /// Calls with no normal successor, observed while the local's storage is live.
    /// These mark the end of a timeline even when no destructor executes.
    pub(crate) terminal_calls: BTreeMap<rustc_middle::mir::Local, Vec<(rustc_span::Span, usize)>>,
    pub(crate) boundaries: BTreeMap<rustc_middle::mir::Local, rustc_span::Span>,
    places: BTreeMap<rustc_middle::mir::Local, BTreeMap<String, Vec<ScopeEnd>>>,
}

impl ScopeEnds {
    pub(super) fn insert(&mut self, local: rustc_middle::mir::Local, column: &str, end: ScopeEnd) {
        let ends = self.places.entry(local).or_default().entry(column.to_owned()).or_default();
        if !ends.contains(&end) { ends.push(end); }
    }

    pub(crate) fn for_place(&self, local: rustc_middle::mir::Local, column: &str) -> &[ScopeEnd] {
        self.places.get(&local).and_then(|paths| paths.get(column)).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn at_exit(&self, site: rustc_span::Span) -> impl Iterator<Item = (rustc_middle::mir::Local, &str, ScopeEnd)> {
        self.places.iter().flat_map(move |(&local, paths)| paths.iter().filter_map(move |(name, ends)| {
            let incoming: Vec<_> = ends.iter().filter(|end| end.site == Some(site)).collect();
            Some((local, name.as_str(), ScopeEnd::across(&incoming)?))
        }))
    }
}

/// One execution site in this body. Drops on two early exits can execute the
/// same MIR instruction. A borrow can end on just one successor of that site.
#[derive(Hash, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct Occurrence {
    #[serde(serialize_with = "location")]
    pub at: Location,
    #[serde(serialize_with = "optional_location", skip_serializing_if = "Option::is_none")]
    pub exit: Option<Location>,
    #[serde(serialize_with = "optional_location", skip_serializing_if = "Option::is_none")]
    pub successor: Option<Location>,
}

#[cfg(test)]
impl Occurrence {
    pub(crate) fn at(at: Location) -> Self { Self { at, exit: None, successor: None } }
}

#[derive(Serialize)]
pub struct Arm {
    pub edge: SwitchEdge,
    #[serde(skip)]
    pub runs: bool,
    #[serde(skip)]
    pub runs_at_runtime: bool,
    /// Index of the branch on this row (one per `SwitchInt` with arms
    /// here), and of the arm within it (MIR successor order).
    pub branch: usize,
    pub arm: usize,
    /// The arm this branch sits in, when branches nest on one row
    /// (`else if`): an index into the row's `arms`.
    pub parent: Option<usize>,
    /// Source columns (1-based, chars) the arm's code spans on the row.
    pub col_lo: usize,
    pub col_hi: usize,
    /// A reachable MIR edge leaves the arm's locations on this row, or
    /// one of those locations terminates execution in this body.
    #[serde(skip)]
    pub has_exit: bool,
    /// State on the continuing exits of an arm with a nested switch.
    #[serde(skip)]
    pub continuing: Option<State>,
    #[serde(flatten)]
    pub after: State,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DropRoute {
    #[default]
    Normal,
    Shared,
    ExitOnly,
    Unwind,
}

impl DropRoute {
    fn is_normal(&self) -> bool {
        *self == Self::Normal
    }
}

/// A slot carrying the loan on an ending edge. The local index distinguishes
/// bindings sharing a display column. Named slots include their field path;
/// a hidden for-loop iterator has the name `<iterator>`.
#[derive(Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EndPlace {
    pub local: usize,
    /// The complete MIR projection, independent of the presentation name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<EndProjection>,
    pub name: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EndProjection {
    Deref,
    Field(usize),
    Index(usize),
    ConstantIndex { offset: u64, min_length: u64, from_end: bool },
    Subslice { from: u64, to: u64, from_end: bool },
    Downcast(usize),
    OpaqueCast(String),
    UnwrapUnsafeBinder(String),
}

/// A place within this MIR body. No display name participates in equality.
#[derive(Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TransientPlace {
    #[serde(serialize_with = "local")]
    pub local: rustc_middle::mir::Local,
    pub projection: Vec<EndProjection>,
}

impl TransientPlace {
    pub(super) fn new<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, p: rustc_middle::mir::Place<'tcx>) -> Self {
        use rustc_middle::mir::ProjectionElem::*;
        let projection = p.projection.iter().map(|step| match step {
            Deref => EndProjection::Deref,
            Field(f, _) => EndProjection::Field(f.index()),
            Index(i) => EndProjection::Index(i.index()),
            ConstantIndex { offset, min_length, from_end } => EndProjection::ConstantIndex { offset, min_length, from_end },
            Subslice { from, to, from_end } => EndProjection::Subslice { from, to, from_end },
            Downcast(_, v) => EndProjection::Downcast(v.index()),
            OpaqueCast(ty) => EndProjection::OpaqueCast(format!("{:032x}", tcx.type_id_hash(ty))),
            UnwrapUnsafeBinder(ty) => EndProjection::UnwrapUnsafeBinder(format!("{:032x}", tcx.type_id_hash(ty))),
        }).collect();
        Self { local: p.local, projection }
    }
}

impl EndPlace {
    pub(crate) fn new<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, p: rustc_middle::mir::Place<'tcx>, name: String) -> Self {
        let place = TransientPlace::new(tcx, p);
        Self { local: place.local.index(), projection: place.projection, name }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct HolderStore {
    #[serde(flatten)]
    pub place: EndPlace,
    pub occurrence: Occurrence,
    pub kind: &'static str,
    /// Every reaching source of the stored reference was identified.
    pub complete: bool,
}

/// A holder along a reaching MIR value transfer. `assigned_at` is the
/// defining basic block and statement index when the definition is unique.
#[derive(Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HolderStep {
    #[serde(flatten)]
    pub place: EndPlace,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_at: Option<(usize, usize)>,
}

/// A fact with its position on the row.
#[derive(Serialize, Clone, PartialEq, Eq)]
pub struct Ev {
    /// MIR site used when placing this event in the drawing. On an early
    /// exit this is the exit itself, even if its drop runs in a shared block;
    /// for an unwind drop it is the initiating call.
    #[serde(skip)]
    pub location: rustc_middle::mir::Location,
    /// Root MIR local for initialization, move, drop, and scope-end events.
    #[serde(skip)]
    pub place_local: Option<rustc_middle::mir::Local>,
    /// An unwind drop is displayed at the call that can enter its cleanup.
    #[serde(skip_serializing_if = "DropRoute::is_normal")]
    pub drop_route: DropRoute,
    /// Actual instruction, including the exit or successor that distinguishes this occurrence.
    pub occurrence: Occurrence,
    /// MIR location used by the row model to display an early exit.
    #[serde(skip)]
    pub display_location: rustc_middle::mir::Location,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub end_places: Vec<EndPlace>,
    /// Ending reference values traced to the issuing borrow without crossing
    /// an opaque call or an ambiguous reaching definition.
    #[serde(skip)]
    pub exact_end_places: Vec<EndPlace>,
    /// A mutable owned local is initialized and unrestricted on this successor.
    #[serde(skip)]
    pub lender_writable: bool,
    /// Initialized, named reference slots whose MIR origins contain this loan
    /// on the HoldEnd successor. Unpictured temporaries can carry it too.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub continuing_places: Vec<EndPlace>,
    /// On this discriminator edge the tested place has a variant with no
    /// fields. It cannot carry a loan in its payload here, even if a
    /// BorrowEnd names it among the possible holders at the switch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fieldless_operand: Option<EndPlace>,
    /// Original caller slot still initialized when this loan ends. A reborrow
    /// can keep it restricted after its own origin stops carrying the loan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_slot: Option<EndPlace>,
    /// Slots along each proven transfer chain, from earlier to later. For an
    /// iterator end, includes transfers to still-initialized loop bindings.
    /// Locals are MIR local indices in this body. A path may stop at a merge.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub transfer_paths: Vec<Vec<HolderStep>>,
    /// Source column (1-based) where the MIR location's span starts.
    pub col: usize,
    /// The innermost arm of this event's row. For an early-exit drop,
    /// this indexes the exit row's arms, including when several exits on
    /// that row share a destructor in MIR.
    pub arm: Option<usize>,
    #[serde(flatten)]
    pub fact: Fact,
    /// (reverse postorder of the block, statement index, phase): a
    /// `BorrowEnd` comes after the location's other events.
    #[serde(skip)]
    pub order: (usize, usize, u8),
}

impl Ev {
    /// Display coordinates can coincide for two different MIR edges.
    pub(super) fn same_occurrence(&self, other: &Self) -> bool {
        self.fact == other.fact && self.occurrence == other.occurrence && self.drop_route == other.drop_route
            && self.place_local == other.place_local
    }
}

#[cfg(test)]
mod tests {
    use super::{DropRoute, Ev, Fact};
    use rustc_middle::mir::{BasicBlock, Location};

    #[test]
    fn dedup_keeps_ends_with_distinct_mir_successors() {
        let first = Ev {
            location: Location::START, display_location: Location::START, place_local: None, drop_route: DropRoute::Normal, occurrence: super::Occurrence { at: Location::START, exit: None, successor: Some(BasicBlock::from_usize(1).start_location()) },
            end_places: vec![], exact_end_places: vec![], lender_writable: false, continuing_places: vec![], fieldless_operand: None, caller_slot: None, transfer_paths: vec![],
            col: 8, arm: None, order: (0, 0, 1),
            fact: Fact::HoldEnd { loan: 0, holder: "other.r".into(), to_line: 4, to_col: 1 },
        };
        let mut second = first.clone();
        second.occurrence.successor = Some(BasicBlock::from_usize(2).start_location());
        let mut facts = vec![first.clone(), first, second];
        facts.dedup_by(|a, b| a.same_occurrence(b));
        assert_eq!(facts.len(), 2);
    }
}

#[derive(Serialize, Clone, PartialEq, Eq)]
#[serde(tag = "ev")]
pub enum Fact {
    /// `path` gets a value: the first time on every path (`reassign`
    /// false), or again (`reassign`, with `maybe_had` when it may have
    /// had no value just before).
    Init { path: String, from: String, reassign: bool, maybe_had: bool },
    /// A move out of a user place. `to` is a variable, `f()` (a call's
    /// argument), `<return>`, or `<temp>`.
    Move { path: String, to: String, in_macro: Option<String> },
    Copy { path: String, to: String, in_macro: Option<String> },
    Borrow { loan: usize, lender: String, kind: &'static str, holder: String },
    /// The loan is no longer in scope for rustc's borrowck after this
    /// row, on the CFG edge to `to_line` (the next row of that path, past
    /// temporaries' drops) at source column `to_col` (one event per edge:
    /// two arms on one row are two; `to_line` 0: the path returns from the
    /// function here, or the call has no normal successor). `holder`: who holds it as it ends.
    /// `why`: "no further use" (nothing uses the loan on this path any
    /// more), "holder overwritten", "<x> out of scope", "<x> dropped",
    /// "unused, the function returns" (a reference parameter the body
    /// never uses: the caller's loan ends with the call), "non-returning call".
    BorrowEnd { loan: usize, lender: String, holder: String, why: String, to_line: usize, to_col: usize },
    /// `holder`, which holds the loan as its own reference (one of the
    /// loan's `direct_holders` or `via_call`), no longer holds it after
    /// this row, on the CFG edge to `to_line`, while someone else still
    /// does (else it is the loan's `BorrowEnd`): a loop binding at its last
    /// use in each iteration while the iterator keeps the loan, a copied
    /// reference at its last use while the copy goes on.
    HoldEnd { loan: usize, holder: String, to_line: usize, to_col: usize },
    /// `holder` (a reference) gets a new loan while it held `old`.
    Repoint {
        holder: String,
        old: Vec<usize>,
        new: Vec<usize>,
        /// Loans carried by the definitions that reach this overwrite,
        /// including a definition whose loan has already gone out of scope.
        #[serde(skip_serializing_if = "Option::is_none")]
        reaching: Option<Vec<usize>>,
        /// Definition sites of the slot on the incoming normal CFG paths.
        /// The same loan can be installed again by copying a reference.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        definitions: Vec<Occurrence>,
        /// At least one incoming normal path has no value in the slot.
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        maybe_uninitialized: bool,
    },
    /// A `Drop` of `path`. `how` (what runs, as drop elaboration decides:
    /// borrowck's imaginary `else` of an irrefutable `if let` doesn't
    /// count): "drops" (it holds a value on every path), "drops_if_init"
    /// (a drop flag decides), "drops_rest" (some fields were moved out),
    /// "nothing_to_drop". `early_exit`: the row of
    /// the `return` / `break` / `continue` / `?` whose path alone runs
    /// this drop (its MIR span is the scope's `}`); `also_on_exits`: rows
    /// of exits sharing this MIR drop. Each exit that drops a value has
    /// its own event with the arm and drop kind for that path.
    Drop { path: String, how: &'static str, at_reassign: bool, early_exit: Option<usize>, also_on_exits: Vec<usize> },
    /// `StorageDead` of a variable with nothing to drop.
    ScopeEnd { path: String, note: &'static str },
    /// A temporary holding a value that needs drop is dropped. `extended`:
    /// its lifetime was extended past its own row (`let r = &make();`).
    TempDrop {
        place: TransientPlace,
        what: String,
        created_line: usize,
        extended: bool,
        early_exit: Option<usize>,
        remainder: TempRemainder,
        /// The elaborated destructor actually tests a drop flag. An exit
        /// supplying one known value can still pass through that test.
        flag: bool,
    },
}

/// What is still owned at this occurrence. `Remaining` covers a
/// partially initialized aggregate, including a path-dependent remainder.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TempValue {
    Whole,
    Conditional,
    Remaining,
    Empty,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct TempRemainder {
    pub value: TempValue,
    /// Move paths known to be absent or possibly absent here. Keep their
    /// variant / field / array identity even for parts with no column.
    pub absent: Vec<TransientPlace>,
    pub maybe_absent: Vec<TransientPlace>,
}

#[derive(Serialize)]
pub struct TempMove {
    /// Source field suffix for a projection consisting entirely of fields.
    #[serde(skip)]
    pub(crate) field: Option<String>,
    #[serde(serialize_with = "location")]
    pub at: Location,
    pub from: TransientPlace,
    /// Direct value transfers only. A call argument needn't produce the result.
    pub into: Option<TransientPlace>,
    pub after: TempValue,
    /// The remainder of each enclosing field when the displayed value was staged there.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub enclosing: Vec<(TransientPlace, TempValue)>,
}

#[derive(Serialize)]
pub struct TempInfo {
    #[serde(serialize_with = "local")]
    pub local: rustc_middle::mir::Local,
    /// Unique typed expression for this local, if the compiler gives one.
    #[serde(skip)]
    pub(crate) expression: Option<rustc_hir::HirId>,
    #[serde(skip)]
    pub(crate) extension: Option<TempExtension>,
    #[serde(skip)]
    pub(crate) scope: Option<rustc_span::Span>,
    #[serde(skip)]
    pub(crate) iterator_binding: Option<rustc_span::Span>,
    /// Some normal paths through the extending initializer bypass creation.
    pub conditional: bool,
    #[serde(serialize_with = "locations")]
    pub created_at: Vec<Location>,
    pub moves: Vec<TempMove>,
    /// For each exit (or normal continuation), do all executable root drops check a flag?
    #[serde(skip)]
    pub(crate) root_flags: BTreeMap<Option<rustc_span::Span>, bool>,
    #[serde(skip)]
    pub(crate) has_root_drop: bool,
    pub what: String,
    pub ty: String,
    pub created_line: usize,
    pub drop_lines: Vec<usize>,
    /// Lives past the row it is made on (lifetime extension by `let`).
    pub extended: bool,
    /// User variables that hold a loan of it.
    pub held_by: Vec<String>,
}

/// The compiler's remainder scope identifies the very let that extends
/// the temporary. Its borrow expression is a presentation anchor.
pub(crate) struct TempExtension {
    pub initializer: rustc_hir::HirId,
    pub borrow: Option<rustc_hir::HirId>,
    pub fields: String,
    pub end: usize,
}

impl BodyFacts {
    pub(crate) fn temporary(&self, local: rustc_middle::mir::Local) -> Option<&TempInfo> {
        self.temps.binary_search_by_key(&local, |t| t.local).ok().map(|i| &self.temps[i])
    }

    pub(crate) fn expression_temp(&self, id: rustc_hir::HirId) -> Option<&TempInfo> {
        self.temps.iter().find(|t| t.expression == Some(id))
    }
}

fn locations<S: serde::Serializer>(sites: &[Location], s: S) -> Result<S::Ok, S::Error> {
    s.collect_seq(sites.iter().map(|l| (l.block.index(), l.statement_index)))
}
