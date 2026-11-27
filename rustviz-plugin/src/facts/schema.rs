//! The fact layer's output: what is true about each place on each row,
//! from MIR and borrowck only. Serialized as JSON (`RV3_FACTS`), version
//! [`VERSION`]. Every map is a `BTreeMap` and every list is in MIR
//! evaluation order or sorted, so the dump is deterministic.

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

#[derive(Serialize)]
pub struct BodyFacts {
    #[serde(skip)]
    pub evaluated_spans: Vec<rustc_span::Span>,
    /// Standard vec! constructions that can complete on the body's normal CFG.
    #[serde(skip)]
    pub constructed_vecs: std::collections::HashMap<rustc_span::hygiene::ExpnId, usize>,
    #[serde(skip)]
    pub def_id: rustc_hir::def_id::DefId,
    /// `main`, `Foo::bar`, `main::{closure#0}`.
    pub func: String,
    /// "fn" or "closure".
    pub kind: &'static str,
    /// For a closure: the body it is defined in.
    pub parent: Option<String>,
    pub first_line: usize,
    pub last_line: usize,
    pub vars: Vec<VarInfo>,
    /// The places that get a state of their own: variables, their field
    /// paths (`p.a`, `t.0`) and Box payloads (`*b`, `b.a`).
    pub columns: Vec<ColumnInfo>,
    /// Columns whose MIR places all have the same Option reference type
    /// classification, including where a boundary has no state.
    #[serde(skip)]
    pub uniform_option_refs: BTreeMap<String, bool>,
    pub loans: Vec<LoanInfo>,
    /// Borrowck loans at MIR assignments to pattern bindings.
    #[serde(skip)]
    pub binding_loans: BTreeMap<rustc_middle::mir::Location, Vec<usize>>,
    #[serde(skip)]
    pub inline_holder_ends: Vec<(usize, Ev)>,
    pub rows: Vec<Row>,
    /// Temporaries that hold a value needing drop.
    pub temps: Vec<TempInfo>,
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
    #[serde(skip)]
    pub joins: BTreeMap<rustc_span::Span, Join>,
}

pub struct Join {
    pub state: State,
    /// A real write reaches this join on one edge. The other leaves the
    /// place empty (false), or skips initialization only in borrowck (true).
    pub assignments: BTreeMap<String, bool>,
    pub dead_implicit_else: bool,
}

pub struct BindingSource {
    pub location: rustc_middle::mir::Location,
    pub runs: bool,
    pub declaration: rustc_span::Span,
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
    pub fn for_place(sites: &[&BindingSource], declaration: rustc_span::Span, place: &str) -> Self {
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
    pub name: String,
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

#[derive(Serialize, Clone, Debug)]
pub struct LoanInfo {
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

/// States of the columns after a row, as the next row sees them.
#[derive(Serialize, Clone, Default)]
pub struct State {
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
    /// Loans live through an Option field or a reference to the whole Option.
    #[serde(skip)]
    pub option_holds: BTreeMap<String, Vec<usize>>,
    /// Borrow checking includes an unreachable path skipping initialization.
    /// The place is initialized on every executable path to this boundary.
    #[serde(skip)]
    pub always_assigned: BTreeSet<String>,
    /// For a possibly empty place, a later assignment before scope exit on
    /// every (true) or only some (false) continuing runtime paths.
    #[serde(skip)]
    pub reassigned: BTreeMap<String, bool>,
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
    /// Named values whose origins contain a live loan at this boundary.
    #[serde(skip)]
    pub holder_sources: BTreeMap<String, BTreeSet<String>>,
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
    #[serde(skip)]
    pub block_end: bool,
    /// The row's code that runs leaves it only by jumps past the next row
    /// the paths come into (`break k;`, `return i;`).
    #[serde(skip)]
    pub jumps: bool,
    /// Events on the row, in MIR evaluation order.
    pub facts: Vec<Ev>,
    /// Several arms of one branch on this row (`if c { a } else { b }`
    /// on one line): each arm's own state at its exit. Recorded for the
    /// presentation layer; how to draw them is a design decision.
    pub arms: Vec<Arm>,
}

#[derive(Serialize)]
pub struct Arm {
    #[serde(skip)]
    pub runs: bool,
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
    #[serde(skip)]
    pub switch_col: usize,
    #[serde(skip)]
    pub code_spans: BTreeSet<(usize, usize)>,
    /// State on the continuing exits of an arm with a nested switch.
    #[serde(skip)]
    pub continuing: Option<State>,
    #[serde(flatten)]
    pub after: State,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DropRoute {
    #[default]
    Normal,
    Shared,
    ExitOnly,
}

/// A fact with its position on the row.
#[derive(Serialize, Clone, PartialEq, Eq)]
pub struct Ev {
    /// MIR site used when placing this event in the drawing. On an early
    /// exit this is the exit itself, even if its drop runs in a shared block.
    #[serde(skip)]
    pub location: rustc_middle::mir::Location,
    /// Reachability of a scope drop without traversing its early exits.
    #[serde(skip)]
    pub drop_route: DropRoute,
    /// Successor on which this borrow or hold ends. Two edges can lead to the same display row.
    #[serde(skip)]
    pub end_successor: Option<rustc_middle::mir::Location>,
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
    /// For a copy of a shared drop on one early exit's row: that exit's own
    /// location (its order), so copies on two exits stay two events.
    #[serde(skip)]
    pub site: Option<(usize, usize, u8)>,
}

impl Ev {
    /// Display coordinates can coincide for two different MIR edges.
    pub(super) fn same_occurrence(&self, other: &Self) -> bool {
        self.fact == other.fact && self.order == other.order && self.site == other.site
            && self.end_successor == other.end_successor
    }
}

#[cfg(test)]
mod tests {
    use super::{DropRoute, Ev, Fact};
    use rustc_middle::mir::{BasicBlock, Location};

    #[test]
    fn dedup_keeps_ends_with_distinct_mir_successors() {
        let first = Ev {
            location: Location::START, drop_route: DropRoute::Normal, end_successor: Some(BasicBlock::from_usize(1).start_location()),
            col: 8, arm: None, order: (0, 0, 1), site: None,
            fact: Fact::HoldEnd { loan: 0, holder: "other.r".into(), to_line: 4, to_col: 1 },
        };
        let mut second = first.clone();
        second.end_successor = Some(BasicBlock::from_usize(2).start_location());
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
    /// function here). `holder`: who holds it as it ends.
    /// `why`: "no further use" (nothing uses the loan on this path any
    /// more), "holder overwritten", "<x> out of scope", "<x> dropped",
    /// "unused, the function returns" (a reference parameter the body
    /// never uses: the caller's loan ends with the call).
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
    TempDrop { what: String, created_line: usize, extended: bool, early_exit: Option<usize> },
}

#[derive(Serialize)]
pub struct TempInfo {
    pub what: String,
    pub ty: String,
    pub created_line: usize,
    pub drop_lines: Vec<usize>,
    /// Lives past the row it is made on (lifetime extension by `let`).
    pub extended: bool,
    /// User variables that hold a loan of it.
    pub held_by: Vec<String>,
}
