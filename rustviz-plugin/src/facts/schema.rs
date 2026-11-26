//! The fact layer's output: what is true about each place on each row,
//! from MIR and borrowck only. Serialized as JSON (`RV3_FACTS`), version
//! [`VERSION`]. Every map is a `BTreeMap` and every list is in MIR
//! evaluation order or sorted, so the dump is deterministic.

use serde::Serialize;
use std::collections::BTreeMap;

/// Bumped whenever a field changes meaning or goes away. 2: `Lent.kind`
/// tells `unique` and `reserved` from `mut`; `Lent.holder` is who holds
/// the loan on that row.
pub const VERSION: u32 = 2;

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
    pub loans: Vec<LoanInfo>,
    pub rows: Vec<Row>,
    /// Temporaries that hold a value needing drop.
    pub temps: Vec<TempInfo>,
    pub notes: Vec<String>,
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
}

#[derive(Serialize, Clone)]
pub struct LoanInfo {
    pub loan: usize,
    pub line: usize,
    pub col: usize,
    /// The borrowed place as the source writes it (`s`, `p.a`, `v[_]`,
    /// `*b`, `(o as Some).0`).
    pub lender: String,
    /// The columns this loan restricts (every column whose place
    /// overlaps the borrowed place: `v` for `v[_]`, `p` and `p.a` for
    /// `p.a`).
    pub columns: Vec<String>,
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
    /// For a "maybe" column: "moved" (moved out on some way here) or
    /// "unassigned" (never given a value on some way here).
    pub cause: BTreeMap<String, &'static str>,
}

#[derive(Serialize, Default)]
pub struct Row {
    pub line: usize,
    /// The columns' states after the row, as the next row of the same
    /// path sees them. A row with no MIR location on a path that goes on
    /// (blank, `} else {`, the `};` of a `let ... else { return }`) is
    /// absent or has empty maps: the previous row's state holds.
    #[serde(flatten)]
    pub after: State,
    /// Events on the row, in MIR evaluation order.
    pub facts: Vec<Ev>,
    /// Several arms of one branch on this row (`if c { a } else { b }`
    /// on one line): each arm's own state at its exit. Recorded for the
    /// presentation layer; how to draw them is a design decision.
    pub arms: Vec<Arm>,
}

#[derive(Serialize)]
pub struct Arm {
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
    #[serde(flatten)]
    pub after: State,
}

/// A fact with its position on the row.
#[derive(Serialize, Clone, PartialEq, Eq)]
pub struct Ev {
    /// Source column (1-based) where the MIR location's span starts.
    pub col: usize,
    /// The arm of `Row::arms` it happens in (innermost), if any.
    pub arm: Option<usize>,
    #[serde(flatten)]
    pub fact: Fact,
    /// (reverse postorder of the block, statement index, phase): a
    /// `BorrowEnd` comes after the location's other events.
    #[serde(skip)]
    pub order: (usize, usize, u8),
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
    /// two arms on one row are two). `holder`: who holds it as it ends.
    /// `why`: "no further use" (nothing uses the loan on this path any
    /// more), "holder overwritten", "<x> out of scope", "<x> dropped".
    BorrowEnd { loan: usize, lender: String, holder: String, why: String, to_line: usize, to_col: usize },
    /// `holder` (a reference) gets a new loan while it held `old`.
    Repoint { holder: String, old: Vec<usize>, new: Vec<usize> },
    /// A `Drop` of `path`. `how` (what runs, as drop elaboration decides:
    /// borrowck's imaginary `else` of an irrefutable `if let` doesn't
    /// count): "drops" (it holds a value on every path), "drops_if_init"
    /// (a drop flag decides), "drops_rest" (some fields were moved out),
    /// "nothing_to_drop". `early_exit`: the row of
    /// the `return` / `break` / `continue` / `?` whose path alone runs
    /// this drop (its MIR span is the scope's `}`); `also_on_exits`: rows
    /// of early exits that share this drop with the normal path.
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
