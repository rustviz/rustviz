//! The state table (RustViz 3, P1): what the renderer will read for each
//! column's state on each row, instead of replaying events.
//!
//! Built from the fact layer's [`CrateFacts`] in the plugin (no JSON round
//! trip). A RAP belongs to its compiler body. Its presentation column can
//! represent a different local at each boundary. A row with no state of its own takes the nearest
//! row above it on the same path (design Q3): the caller bounds the search
//! by the first row of the leg it draws, since rows above a leg's start
//! may belong to another arm.

use rustc_hir::def_id::DefId;
use rustc_data_structures::fx::FxIndexMap;
use super::schema::{ProjectedPlace, DropRoute, LoanInfo, BodyFacts, CrateFacts, Extraction, Fact, HolderType, Lent, Parts, State};
use std::collections::{BTreeMap, BTreeSet};

/// A function declaration as drawn in the source panel. Two declarations
/// can start on the same line. Only the compiler body selects semantic facts.
#[derive(Debug, Clone, Copy)]
pub struct FunctionStart {
    pub body: DefId,
    pub line: usize,
}

/// A column's value on a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Init {
    Own,
    Moved,
    /// Moved or unassigned on some paths only.
    Maybe,
    /// Some fields moved out (`parts` says which).
    Partly,
    Unassigned,
}

impl Init {
    fn parse(s: &str) -> Option<Init> {
        Some(match s {
            "own" => Init::Own,
            "moved" => Init::Moved,
            "maybe" => Init::Maybe,
            "partly" => Init::Partly,
            "unassigned" => Init::Unassigned,
            _ => return None,
        })
    }
}

/// Why a cell differs from the row above (the row's own events about the
/// column).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// Moved out, to a variable, `f()`, `<return>`.
    Moved { to: String },
    /// A field of it (any depth) moved out (`let x = p.a;` for `p`).
    FieldMoved { field: String, to: String },
    /// A loan restricting the column went out of scope on the CFG edge to
    /// `to_line` (at source column `to_col`): "no further use", "holder
    /// overwritten", "<x> out of scope", "<x> dropped".
    LoanEnded { loan: usize, cause: String, to_line: usize, to_col: usize },
    /// Borrowed here.
    Lent { loan: usize },
    /// Given a value (again: `reassign`; perhaps with none before:
    /// `maybe_had`).
    Assigned { reassign: bool, maybe_had: bool },
    /// Its old value dropped ("drops", "drops_if_init", "drops_rest",
    /// "nothing_to_drop"), on this row's path or an early exit's.
    Dropped { local: Option<rustc_middle::mir::Local>, how: &'static str, early_exit: Option<usize>, at_reassign: bool, route: DropRoute },
}

/// One column on one row (or one arm of a row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    local: Option<rustc_middle::mir::Local>,
    pub init: Init,
    /// The loans in scope that restrict it here.
    pub lent: Vec<Lent>,
    /// Which fields are moved out, for `Partly` (and an `Own` struct some
    /// of whose fields may have been moved).
    pub parts: Option<Parts>,
    /// True on some paths only: the state is `Maybe`, or a field may have
    /// been moved.
    pub may: bool,
    /// At least one path can skip or overwrite the active borrow.
    pub may_lend: bool,
    /// A part of it with no name (an enum payload) was moved out on some
    /// paths only (`State.unnamed_maybe`).
    pub unnamed_maybe: bool,
    /// Lenders tracked through this column's own value.
    pub borrowed_from: BTreeSet<String>,
    /// Lenders tracked through each current holder, on this same boundary.
    pub holder_sources: BTreeMap<String, BTreeSet<String>>,
    /// For `Maybe`: "moved", "unassigned" or "imaginary".
    pub cause: Option<&'static str>,
    /// The loans its value keeps borrowed here, as the borrow checker
    /// counts them (by loan id; `State.holds`): none for a reference no
    /// later row uses, a moved-out or unassigned variable, or a field
    /// column (only variable columns have them).
    pub holds: Vec<usize>,
    pub holder_type: Option<HolderType>,
    pub manually_taken: Option<Extraction>,
    pub option_holds: Vec<usize>,
    pub always_assigned: bool,
    pub reassigned: Option<bool>,
    pub moved_in_arm: bool,
    pub why: Vec<Why>,
    /// The row this state was read on (the row asked for, or the row
    /// above it that the state carries over from).
    pub from_row: usize,
}

impl Cell {
    /// The active binding is an Option of a reference, and whether it is Copy.
    pub fn option_ref_copy(&self) -> Option<bool> {
        match self.holder_type {
            Some(HolderType::OptionRef { copy, .. }) => Some(copy),
            _ => None,
        }
    }
}

type Cells = BTreeMap<String, Cell>;

/// Source coordinates of the arm represented by a drawn leg. Its HIR
/// span and the MIR arm spans use the same 1-based character positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmSpan {
    pub start: (usize, usize),
    pub end: (usize, usize),
}

/// An arm of a one-line branch on a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmInfo {
    pub runs: bool,
    pub branch: usize,
    pub arm: usize,
    pub parent: Option<usize>,
    pub col_lo: usize,
    pub col_hi: usize,
    has_exit: bool,
    switch_col: usize,
    code_spans: BTreeSet<(usize, usize)>,
    continuing: Option<Cells>,
}

#[derive(Debug, Default)]
struct RowCells {
    /// The row has a state of its own (MIR code runs on it on a way that
    /// goes on): a column missing from it is out of scope there.
    snapshot: bool,
    continues_to: Option<usize>,
    cells: Cells,
    before: Option<Cells>,
    block_end: bool,
    jumps: bool,
    /// The row's events by column, each with the arm it happens in.
    why: BTreeMap<String, Vec<(Option<usize>, Why)>>,
    arms: Vec<(ArmInfo, Cells)>,
}

#[derive(Debug)]
struct BodyTable {
    first_line: usize,
    last_line: usize,
    function_body_start: Option<(usize, usize)>,
    rows: BTreeMap<usize, RowCells>,
    joins: BTreeMap<rustc_span::Span, Cells>,
    /// Column names, in the facts' order.
    columns: Vec<String>,
    moved_places: BTreeMap<rustc_middle::mir::Local, BTreeSet<String>>,
    array_lengths: BTreeMap<(rustc_middle::mir::Local, String), usize>,
    boxes: BTreeSet<String>,
    uniform_option_refs: BTreeMap<String, bool>,
    /// Each loan's borrowed place (`s`, `p.a`, `*b`, `v[_]`).
    loans: BTreeMap<usize, LoanInfo>,
    drop_holders: BTreeMap<usize, BTreeSet<String>>,
    part_lenders: BTreeSet<String>,
    /// Columns with fields this body can't name (no columns of their own).
    hidden_fields: BTreeSet<String>,
    /// Rows whose code never runs (`BodyFacts::dead_lines`).
    dead_lines: BTreeSet<usize>,
}

/// The state table of a crate: per fn body, per row (and arm), per
/// column. Rows are source lines: a renderer that hides or inserts lines
/// maps its visual rows back (`VisualizationData::row_lines`) before it
/// asks.
#[derive(Debug, Default)]
pub struct StateTable {
    /// Borrow-end event IDs are unique across the drawing. Loan indices
    /// are local to the event's body.
    pub end_loans: BTreeMap<usize, Vec<usize>>,
    /// For consecutive stores on one source row, event ID to group and
    /// execution rank. Alternative lenders of one store share its rank.
    pub store_order: BTreeMap<usize, (usize, usize)>,
    bodies: FxIndexMap<DefId, BodyTable>,
    imaginary_arms: BTreeMap<super::ImaginaryEdgeId, ImaginaryArm>,
}

#[derive(Debug)]
pub struct ImaginaryArm {
    pub entry: Cells,
    pub after: BTreeMap<usize, Cells>,
    pub join: Cells,
    pub changes: BTreeMap<String, super::ImaginaryChange>,
    pub scrutinee_moves: BTreeSet<String>,
    pub initializer_bindings: BTreeMap<String, String>,
}

impl StateTable {
    pub fn from_facts(facts: &CrateFacts) -> StateTable {
        let bodies = facts.bodies.iter().filter(|b| b.kind == "fn")
            .map(|b| (b.def_id, body_table(b))).collect();
        let imaginary_arms = facts.bodies.iter().flat_map(|b| &b.imaginary_edges)
            .map(|e| (e.id, ImaginaryArm {
                entry: cells_of(&e.entry),
                after: e.after.iter().map(|(&row, state)| (row, cells_of(state))).collect(),
                join: cells_of(&e.join),
                changes: e.changes.clone(),
                scrutinee_moves: e.scrutinee_moves.clone(),
                initializer_bindings: e.initializer_bindings.clone(),
            })).collect();
        StateTable { bodies, imaginary_arms, ..Default::default() }
    }

    /// The state of `column` of the MIR body `function` after `row`,
    /// carried from the nearest row at or above it that has a state of
    /// its own, but not above `floor` (the first row of the leg being
    /// drawn; `0` for no bound). None where the column is out of scope
    /// (the nearest state doesn't have it) or nothing is known.
    pub fn cell(&self, function: FunctionStart, column: &str, row: usize, floor: usize) -> Option<Cell> {
        self.cell_on(function, column, row, floor, None)
    }

    pub fn has_part_loans(&self, function: FunctionStart, column: &str) -> bool {
        self.bodies.get(&function.body).is_some_and(|b| b.part_lenders.contains(column))
    }

    /// The source row reached by the snapshot's single forward MIR edge.
    pub fn continues_to_row(&self, function: FunctionStart, row: usize) -> Option<usize> {
        self.bodies.get(&function.body)?.rows.get(&row)?.continues_to
    }

    /// A loan boundary on the drawn path. Carry across an empty row only
    /// along the MIR edge that actually connects the surrounding rows.
    pub fn loan_cell(&self, function: FunctionStart, column: &str, row: usize, floor: usize) -> Option<Cell> {
        let c = self.cell(function, column, row, floor)?;
        let body = self.bodies.get(&function.body)?;
        (c.from_row == row || body.rows[&c.from_row].continues_to.is_some_and(|n| row < n)).then_some(c)
    }

    /// Associate a rendered arm with its own MIR exit. SwitchInt successor
    /// order is not source order. On a shared row, require exactly one
    /// MIR arm inside the supplied span. Never use the joined snapshot
    /// for a leg that cannot be identified.
    pub fn arm_loan_cell(&self, function: FunctionStart, column: &str, row: usize, span: ArmSpan) -> Option<(Cell, Option<usize>)> {
        if row < span.start.0 || row > span.end.0 {
            return None;
        }
        let body = self.bodies.get(&function.body)?;
        if let Some(rc) = body.rows.get(&row).filter(|rc| !rc.arms.is_empty()) {
            let mut matches = rc.arms.iter().enumerate().filter(|(_, (a, _))|
                a.col_lo != 0 && span.start <= (row, a.col_lo) && (row, a.col_hi) <= span.end);
            let (arm, _) = matches.next()?;
            if matches.next().is_some() { return None; }
            return Some((self.cell_in_arm(function, column, row, arm)?, Some(arm)));
        }
        // An interior row belongs to this arm alone. A boundary row can
        // include its condition, a sibling arm, or code after the join.
        if span.start.0 < row && row < span.end.0 {
            return Some((self.loan_cell(function, column, row, span.start.0 + 1)?, None));
        }
        None
    }

    /// `cell` on one arm of a row with several (`arms`): its own state and
    /// events.
    pub fn cell_in_arm(&self, function: FunctionStart, column: &str, row: usize, arm: usize) -> Option<Cell> {
        self.cell_on(function, column, row, row, Some(arm))
    }

    fn cell_on(&self, function: FunctionStart, column: &str, row: usize, floor: usize, arm: Option<usize>) -> Option<Cell> {
        let body = self.bodies.get(&function.body)?;
        if row < body.first_line || row > body.last_line || floor > row {
            return None;
        }
        let here = body.rows.get(&row);
        let why: Vec<Why> = here
            .and_then(|r| r.why.get(column))
            .map(|v| v.iter().filter(|(a, _)| arm.is_none() || *a == arm || a.is_none()).map(|(_, w)| w.clone()).collect())
            .unwrap_or_default();
        let found = if let Some(i) = arm {
            let r = here?;
            r.arms.get(i).and_then(|(_, cells)| cells.get(column)).map(|c| (row, c.clone()))
        } else {
            let mut out = None;
            for (r, cells) in body.rows.range(floor.max(body.first_line)..=row).rev() {
                if cells.snapshot {
                    out = cells.cells.get(column).map(|c| (*r, c.clone()));
                    break;
                }
            }
            out
        };
        let (from_row, mut cell) = found?;
        cell.from_row = from_row;
        cell.why = why;
        Some(cell)
    }

    /// The state between source rows. Read the incoming edges of the
    /// next MIR row, including all predecessors at a join. Empty source
    /// rows between those edges carry the same state.
    pub fn segment_cell(&self, function: FunctionStart, column: &str, row: usize) -> Option<Cell> {
        let b = self.bodies.get(&function.body)?;
        if !(b.first_line..b.last_line).contains(&row) { return None; }
        if let Some(rc) = b.rows.get(&row) {
            if rc.snapshot && !rc.block_end && !rc.jumps {
                let mut own = self.cell(function, column, row, row)?;
                // The row's state joins all its exits; the line below it is
                // on the ones that come into the next row (`if let Some(x)
                // = b && x.len() > 1 {`: x is bound in the block below,
                // unassigned only on the way past it). Where those all hold
                // a value, so does the line.
                if own.init == Init::Maybe {
                    let next = b.rows.range(row + 1..).find(|(_, c)| c.before.is_some()).and_then(|(_, c)| c.before.as_ref()?.get(column));
                    // (Its init, as at a block's end below; loans are this
                    // row's, read where its own path leaves it.)
                    if let Some(n) = next.filter(|n| n.local == own.local && n.init == Init::Own) {
                        own.init = n.init;
                        own.parts = n.parts.clone();
                        own.unnamed_maybe = n.unnamed_maybe;
                        own.may = n.may;
                        own.cause = n.cause;
                        own.always_assigned = n.always_assigned;
                        own.reassigned = n.reassigned;
                    }
                }
                return Some(own);
            }
        }
        // A row left only by a jump: the line below it is on the paths
        // that skip it (Q3), which come into the next row.
        let jumps = b.rows.get(&row).is_some_and(|rc| rc.jumps);
        let (&next, boundary) = b.rows.range(row + 1..).find(|(_, c)| c.before.is_some())?;
        let mut cell = boundary.before.as_ref()?.get(column)?.clone();
        if let Some(here) = self.cell(function, column, row, row).filter(|here| !jumps && here.local == cell.local) {
            // Paths on which a block-local was never in scope can meet at
            // its StorageDead. Inside its scope the binding has its own state.
            let definitely_dropped = boundary.why.get(column).is_some_and(|events| events.iter().any(|(_, why)|
                matches!(why, Why::Dropped { how: "drops", early_exit: None, at_reassign: false, .. })));
            if (definitely_dropped && here.init == Init::Own) || here.why.iter().any(|w| matches!(w, Why::Assigned { .. }))
                || (cell.cause == Some("unassigned") && !boundary.cells.contains_key(column)) {
                return Some(here);
            }
            // Loans are read where the row's own path leaves it: the next
            // row's incoming edges can include a jump's target (`break 'blk
            // &h` lands on the block's value, past the next row), whose
            // loans aren't on this path. (Init still comes from the edges
            // into the next row: a block-local's scope end is there.)
            cell.lent = here.lent;
            cell.may_lend = here.may_lend;
            cell.borrowed_from = here.borrowed_from;
            cell.holder_sources = here.holder_sources;
            let same_type = cell.holder_type == here.holder_type;
            cell.holds = if same_type { here.holds } else { Vec::new() };
            cell.option_holds = if same_type { here.option_holds } else { Vec::new() };
        }
        cell.from_row = next;
        Some(cell)
    }

    pub fn join_cell(&self, function: FunctionStart, column: &str, span: rustc_span::Span) -> Option<&Cell> {
        self.bodies.get(&function.body)?.joins.get(&span)?.get(column)
    }

    /// On a shared row, the only write is in one arm and a real sibling
    /// can leave the place moved. This also covers a short-circuit RHS.
    pub fn skips_reassignment(&self, function: FunctionStart, column: &str, row: usize) -> bool {
        let Some(rc) = self.bodies.get(&function.body).and_then(|b| b.rows.get(&row)) else { return false };
        let Some(why) = rc.why.get(column) else { return false };
        let mut writes = why.iter().filter(|(_, w)| matches!(w, Why::Assigned { .. }));
        let Some((Some(arm), Why::Assigned { reassign: true, .. })) = writes.next() else { return false };
        if writes.next().is_some() || !why.iter().any(|(a, w)| a.as_ref() == Some(arm) && matches!(w,
            Why::Dropped { how: "drops_if_init", at_reassign: true, .. })) { return false; }
        let Some((written, _)) = rc.arms.get(*arm) else { return false };
        rc.arms.iter().any(|(a, cells)| a.runs && a.branch == written.branch && a.arm != written.arm
            && cells.get(column).is_some_and(|c| c.init == Init::Maybe && c.cause == Some("moved")))
    }

    /// A normal scope-end drop (the column can already be out of storage
    /// in the row's after-cell). Several sites must agree.
    pub fn scope_drop(&self, function: FunctionStart, column: &str, row: usize) -> Option<&'static str> {
        self.local_scope_drop(function, column, row, None)
    }

    pub fn local_scope_drop(&self, function: FunctionStart, column: &str, row: usize, local: Option<rustc_middle::mir::Local>) -> Option<&'static str> {
        let events = self.bodies.get(&function.body)?.rows.get(&row)?.why.get(column)?.iter()
            .filter(|(_, why)| local.is_none() || matches!(why, Why::Dropped { local: l, .. } if *l == local));
        if events.clone().any(|(_, w)| matches!(w, Why::Dropped { how: "drops_if_init", early_exit: None, at_reassign: false, route: DropRoute::Shared, .. })) {
            return None;
        }
        let mut drops = events.filter_map(|(_, w)| match w {
            Why::Dropped { how, early_exit: None, at_reassign: false, route: DropRoute::Normal | DropRoute::Shared, .. } => Some(*how),
            _ => None,
        });
        let how = drops.next()?;
        drops.all(|h| h == how).then_some(how)
    }

    /// The next MIR boundary is the function's scope exit and actually
    /// drops the remainder of this place (drop elaboration).
    pub fn drops_rest_at_exit(&self, function: FunctionStart, column: &str, row: usize) -> bool {
        let Some(body) = self.bodies.get(&function.body) else { return false };
        let Some((&next, next_row)) = body.rows.range(row + 1..).next() else { return false };
        next == body.last_line && next_row.why.get(column).is_some_and(|events| events.iter().any(|(_, why)|
            matches!(why, Why::Dropped { how: "drops_rest", early_exit: None, at_reassign: false, .. })))
    }

    pub fn loan(&self, function: FunctionStart, loan: usize) -> Option<&LoanInfo> {
        self.bodies.get(&function.body)?.loans.get(&loan)
    }

    pub fn kept_through_drop(&self, function: FunctionStart, loan: usize, holder: &str) -> bool {
        self.bodies.get(&function.body).and_then(|b| b.drop_holders.get(&loan)).is_some_and(|places| places.contains(holder))
    }

    /// The source lines of the loans of `lender` (a temporary's `(code)`
    /// name) in the body that has `column`.
    pub fn lender_lines(&self, function: FunctionStart, lender: &str) -> Vec<usize> {
        self.bodies.get(&function.body).map_or_else(Vec::new, |b| b.loans.values().filter(|l| l.lender == lender).map(|l| l.line).collect())
    }

    /// Whether `column` has no value on some row above `row` while in
    /// scope (`let x;`): a binding a pattern gives its value when it comes
    /// into scope (`if let Some(x) = o`) never has.
    pub fn unassigned_above(&self, function: FunctionStart, column: &str, row: usize, cell: &Cell) -> bool {
        let Some(b) = self.bodies.get(&function.body) else { return false };
        let unassigned = |cells: &Cells| cells.get(column).is_some_and(|c| c.local == cell.local && c.init == Init::Unassigned);
        b.rows.range(..row).any(|(_, rc)| unassigned(&rc.cells) || rc.arms.iter().any(|(_, cells)| unassigned(cells)))
    }

    /// Whether the body moves `column`, a place it is in or a place in it
    /// out on some row (a `Move` fact): a column the table says moved (or
    /// maybe moved) with none is out of its scope there, not moved.
    pub fn has_move(&self, function: FunctionStart, column: &str, cell: &Cell) -> bool {
        let Some(b) = self.bodies.get(&function.body) else { return false };
        let me = column.trim_start_matches('*');
        let within = |a: &str, b: &str| a.starts_with(&format!("{b}.")) || a.starts_with(&format!("{b}["));
        let related = |c: &str| {
            let c = c.trim_start_matches('*');
            c == me || within(me, c) || within(c, me)
        };
        cell.local.and_then(|l| b.moved_places.get(&l)).is_some_and(|places| places.iter().any(|p| related(p)))
    }

    /// The arms of a one-line branch on `row` of the fn starting on
    /// `function` (the body that has `column`).
    pub fn arms(&self, function: FunctionStart, row: usize) -> Vec<ArmInfo> {
        self.bodies.get(&function.body)
            .and_then(|b| b.rows.get(&row))
            .map_or(Vec::new(), |r| r.arms.iter().map(|(a, _)| a.clone()).collect())
    }

    /// The facts' arms (indices into row `row`'s arms) of a renderer's
    /// conditional on that row, one per renderer arm in its order, given
    /// those arms' source spans (None: an arm with no code of its own, an
    /// implicit `else`): the one branch of the row whose every arm lies in
    /// exactly one of the spans, one per span; spanless arms take the
    /// branch's arms no span holds, in source order. That tells a
    /// conditional from a sibling one on the same line. None where no
    /// single branch fits, or where an arm of it has a branch of its own
    /// (`?`, an `else if`): its state joins that branch's exits, an early
    /// return's too. `whole_row`: also None where any branch on the row
    /// nests in another.
    fn match_arms(r: &RowCells, row: usize, spans: &[Option<ArmSpan>], whole_row: bool) -> Option<Vec<usize>> {
        if r.arms.is_empty() || spans.is_empty() || (whole_row && r.arms.iter().any(|(a, _)| a.parent.is_some())) {
            return None;
        }
        // (An arm in a span: all its code, or where its code starts or ends
        // when no arm is all inside: an `if let` arm's code starts at its
        // pattern's binding, before its block's span; a `return` arm's runs
        // on to the end of the line, its early exit's drops.)
        let at = |s: &ArmSpan, c: usize| s.start <= (row, c) && (row, c) <= s.end;
        let inside = |s: &ArmSpan, a: &ArmInfo| a.col_lo != 0 && (at(s, a.col_lo) || at(s, a.col_hi));
        let whole = |s: &ArmSpan, a: &ArmInfo| a.col_lo != 0 && at(s, a.col_lo) && at(s, a.col_hi);
        let branches: BTreeSet<usize> = r.arms.iter().map(|(a, _)| a.branch).collect();
        let mut found = None;
        for br in branches {
            let idx: Vec<usize> = (0..r.arms.len()).filter(|&i| r.arms[i].0.branch == br).collect();
            if idx.len() != spans.len() || r.arms.iter().any(|(a, _)| a.parent.is_some_and(|p| idx.contains(&p))) {
                continue;
            }
            // A branch nested in an arm (`else if`, a `?`) is the
            // conditional only if the conditional lies in that arm's code.
            let within = |s: &ArmSpan, p: &ArmInfo| s.start.0 == row && s.end.0 == row && p.col_lo <= s.start.1 && s.end.1 <= p.col_hi + 1;
            if let Some(p) = r.arms[idx[0]].0.parent.and_then(|p| r.arms.get(p)) {
                if !spans.iter().flatten().all(|s| within(s, &p.0)) {
                    continue;
                }
            }
            let mut out: Vec<Option<usize>> = vec![None; spans.len()];
            let mut ok = true;
            for (j, s) in spans.iter().enumerate() {
                let Some(s) = s else { continue };
                let mut m: Vec<usize> = idx.iter().copied().filter(|&i| inside(s, &r.arms[i].0)).collect();
                // The empty successor can coincide with a non-empty arm's
                // closing brace. It belongs to the implicit else.
                if spans.iter().any(Option::is_none) && m.iter().any(|&i| r.arms[i].0.col_lo < r.arms[i].0.col_hi) {
                    m.retain(|&i| r.arms[i].0.col_lo < r.arms[i].0.col_hi);
                }
                // (Best fit first: all the arm's code in the span, else where
                // it ends, else where it starts.)
                if m.len() > 1 && m.iter().any(|&i| whole(s, &r.arms[i].0)) {
                    m.retain(|&i| whole(s, &r.arms[i].0));
                }
                if m.len() > 1 && m.iter().any(|&i| at(s, r.arms[i].0.col_hi)) {
                    m.retain(|&i| at(s, r.arms[i].0.col_hi));
                }
                // (An implicit `else` has no code: its arm is the point at
                // the `if` arm's closing brace.)
                if m.len() > 1 {
                    m.retain(|&i| r.arms[i].0.col_lo < r.arms[i].0.col_hi);
                }
                match m.as_slice() {
                    [one] if !out.contains(&Some(*one)) => out[j] = Some(*one),
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            let mut rest: Vec<usize> = idx.iter().copied().filter(|i| !out.contains(&Some(*i))).collect();
            rest.sort_by_key(|&i| (r.arms[i].0.col_lo == 0, r.arms[i].0.col_lo, r.arms[i].0.col_hi));
            let mut rest = rest.into_iter();
            for o in out.iter_mut().filter(|o| o.is_none()) {
                *o = rest.next();
            }
            if found.is_some() {
                return None;
            }
            found = out.into_iter().collect::<Option<Vec<usize>>>();
        }
        found
    }

    /// Whether the arms of the one-line conditional on source row `row`
    /// whose arms span `spans` (`match_arms`) leave one of `columns` in
    /// different states (its value; the kind, holder and every variable
    /// holding each loan restricting it; the parts moved out of it; the
    /// loans it holds): design Q1, such a conditional gets a row per arm.
    /// Loans that die on an arm's way out don't count (after its code they
    /// are over, `arm_loan_ends`; an arm `match_arms` accepts has no
    /// branch of its own, so its code has one way out).
    pub fn arms_disagree(&self, function: DefId, row: usize, columns: &BTreeSet<String>, spans: &[Option<ArmSpan>]) -> bool {
        type Key<'a> = (Init, Vec<(&'static str, String, BTreeSet<String>)>, Option<Parts>, BTreeSet<&'a str>);
        self.bodies.get(&function).is_some_and(|b| {
            // (What it holds: the variables its loans borrow, not a constant
            // or a temporary, which each arm lends anew: `if c { "a" } else
            // { "b" }` holds a string literal either way.)
            let key = |c: &Cell, ends: &BTreeSet<usize>| -> Key<'_> {
                let mut lent: Vec<(&'static str, String, BTreeSet<String>)> = c.lent.iter().filter(|l| !ends.contains(&l.loan))
                    .map(|l| (l.kind, l.holder.clone(), l.current.clone())).collect();
                lent.sort_unstable();
                lent.dedup();
                let holds = c.holds.iter().filter(|l| !ends.contains(l))
                    .filter_map(|l| b.loans.get(l).map(|i| i.lender.as_str()))
                    .filter(|lender| b.columns.iter().any(|col| col == lender || lender.starts_with(&format!("{col}."))))
                    .collect();
                (c.init, lent, c.parts.clone(), holds)
            };
            let Some(r) = b.rows.get(&row) else { return false };
            let Some(arms) = Self::match_arms(r, row, spans, false) else { return false };
            // The loans each arm's own events end, whatever column has them.
            let ends: Vec<BTreeSet<usize>> = arms.iter().map(|&i| {
                r.why.values().flatten().filter_map(|(a, w)| match w {
                    Why::LoanEnded { loan, .. } if *a == Some(i) => Some(*loan),
                    _ => None,
                }).collect()
            }).collect();
            columns.iter().any(|col| {
                let mut seen = arms.iter().zip(&ends).map(|(&i, e)| r.arms[i].1.get(col).map(|c| key(c, e)));
                let first = seen.next();
                first.is_some_and(|f| seen.any(|o| o != f))
            })
        })
    }

    /// The loans of `column` whose end is one of arm `arm`'s own events on
    /// source row `row` (a loan its code last uses, dying on the arm's way
    /// out to the join): the arm's state, read at its exits, still has
    /// them. (An arm `arm_order` matches has no branch of its own, so its
    /// code has one way out.)
    pub fn arm_loan_ends(&self, function: FunctionStart, column: &str, row: usize, arm: usize) -> Vec<usize> {
        self.bodies.get(&function.body)
            .and_then(|b| b.rows.get(&row))
            .and_then(|r| r.why.get(column))
            .map_or(Vec::new(), |ws| ws.iter().filter_map(|(a, w)| match w {
                Why::LoanEnded { loan, .. } if *a == Some(arm) => Some(*loan),
                _ => None,
            }).collect())
    }

    /// The arms of the one-line conditional on source row `row` (in the
    /// body of the MIR body `function` that has `column`) whose arms
    /// span `spans` (a renderer's, in its order), as indices into the
    /// row's arms (`match_arms`): its arm `i` is the renderer's `i`th.
    /// None on a row where branches nest.
    pub fn arm_order(&self, function: FunctionStart, row: usize, spans: &[Option<ArmSpan>]) -> Option<Vec<usize>> {
        let r = self.bodies.get(&function.body)?.rows.get(&row)?;
        Self::match_arms(r, row, spans, true)
    }

    /// Match one drawn arm by the compiler spans of its own MIR code.
    /// A switch inside the span belongs to a nested conditional, while
    /// the arm's own switch precedes its code. The siblings' shapes and
    /// the names on the diagram play no part in this lookup.
    fn arm_in_span(&self, function: DefId, row: usize, span: ArmSpan) -> Option<(&RowCells, usize)> {
        let r = self.bodies.get(&function)?.rows.get(&row)?;
        let mut arms = r.arms.iter().enumerate().filter_map(|(i, (a, _))| {
                ((row, a.switch_col) < span.start && a.code_spans.iter().any(|&(lo, hi)|
                    span.start <= (row, lo) && (row, hi) <= span.end)).then_some((r, i))
            });
        let first = arms.next()?;
        arms.next().is_none().then_some(first)
    }

    /// Whether the matched MIR arm has a reachable exit on this row.
    pub fn arm_has_exit(&self, function: DefId, row: usize, span: ArmSpan) -> bool {
        self.arm_in_span(function, row, span).is_some_and(|(r, i)| r.arms[i].0.has_exit)
    }

    pub fn continuing_arm_cell(&self, function: FunctionStart, column: &str, row: usize, span: ArmSpan) -> Option<Cell> {
        let (r, i) = self.arm_in_span(function.body, row, span)?;
        let mut cell = r.arms[i].0.continuing.as_ref()?.get(column)?.clone();
        cell.from_row = row;
        Some(cell)
    }

    /// An empty arm can have no MIR span of its own. When the other arm
    /// of a two-arm conditional is matched and has a nested join, the
    /// remaining successor of that switch supplies the empty arm's cell.
    pub fn empty_sibling_cell(&self, function: FunctionStart, column: &str, row: usize, other: ArmSpan) -> Option<Cell> {
        let (r, i) = self.arm_in_span(function.body, row, other)?;
        let (matched, _) = &r.arms[i];
        matched.continuing.as_ref()?;
        let mut siblings = r.arms.iter().enumerate().filter(|(j, (a, _))| *j != i && a.branch == matched.branch);
        let (_, (arm, cells)) = siblings.next()?;
        if siblings.next().is_some() || !arm.has_exit { return None; }
        let mut cell = cells.get(column)?.clone();
        cell.from_row = row;
        Some(cell)
    }

    /// `column` where the arms of the one branch on the fn's last row join,
    /// before its scope end drops what is left: a fn written on one line
    /// (`fn f(p: P, c: bool) -> String { if c { p.a } else { p.b } }`),
    /// whose rows have no state between them (`segment_cell`). Partly
    /// moved: the fields moved on every arm, and those moved on some arms
    /// only (p.a or p.b). None where no arm moves a part of it, or an arm
    /// moves all of it, or branches nest.
    pub fn arms_joined_cell(&self, function: FunctionStart, column: &str, row: usize) -> Option<Cell> {
        let b = self.bodies.get(&function.body)?;
        let r = b.rows.get(&row).filter(|_| row == b.last_line)?;
        let first = r.arms.first()?;
        if r.arms.iter().any(|(a, _)| a.parent.is_some() || a.branch != first.0.branch) {
            return None;
        }
        let cells: Vec<&Cell> = r.arms.iter().map(|(_, c)| c.get(column)).collect::<Option<_>>()?;
        if !cells.iter().all(|c| c.local == cells[0].local && matches!(c.init, Init::Own | Init::Partly)) {
            return None;
        }
        let moved = |c: &Cell| -> BTreeSet<String> { c.parts.as_ref().map_or(BTreeSet::new(), |p| p.moved.iter().chain(&p.maybe).cloned().collect()) };
        let sure: BTreeSet<String> = cells.iter().map(|c| moved(c)).reduce(|a, b| a.intersection(&b).cloned().collect())?;
        let any: BTreeSet<String> = cells.iter().flat_map(|c| moved(c)).collect();
        if any.is_empty() {
            return None;
        }
        let mut cell = cells[0].clone();
        cell.init = Init::Partly;
        cell.may = any != sure;
        cell.parts = Some(Parts { moved: sure.iter().cloned().collect(), maybe: any.difference(&sure).cloned().collect() });
        cell.lent.retain(|l| cells.iter().all(|c| c.lent.iter().any(|m| m.loan == l.loan)));
        cell.from_row = row;
        cell.why = Vec::new();
        Some(cell)
    }

    /// The borrow checker's state on entering an arm that never runs.
    pub fn imaginary_arm(&self, edge: super::ImaginaryEdgeId) -> Option<&ImaginaryArm> {
        self.imaginary_arms.get(&edge)
    }

    /// Source rows the borrow checker sees but the function body cannot execute.
    pub fn unreachable_rows(&self) -> impl Iterator<Item = ((usize, usize), usize)> + '_ {
        self.bodies.values().filter_map(|b| Some((b.function_body_start?, &b.dead_lines)))
            .flat_map(|(body, rows)| rows.iter().map(move |&row| (body, row)))
    }

    /// The direct field columns of `column` (`p.a`, `p.b` of `p`).
    pub fn fields_of(&self, function: FunctionStart, column: &str) -> Vec<String> {
        let prefix = format!("{}.", column);
        self.bodies.get(&function.body).map_or(Vec::new(), |b| {
            b.columns
                .iter()
                .filter(|n| n.strip_prefix(&prefix).is_some_and(|rest| !rest.is_empty() && !rest.contains('.')))
                .cloned()
                .collect()
        })
    }

    pub fn is_box(&self, function: FunctionStart, column: &str) -> bool {
        self.bodies.get(&function.body).is_some_and(|b| b.boxes.contains(column))
    }

    /// The event's column can be absent from either side (a new binding,
    /// or one whose scope ends on the same row). Require agreement across
    /// available snapshots if both sides have a binding.
    pub fn option_ref_copy(&self, function: FunctionStart, column: &str, row: Option<usize>) -> Option<bool> {
        let body = self.bodies.get(&function.body)?;
        if let Some(&copy) = body.uniform_option_refs.get(column) {
            // A branch leg may have no cell of its own. All bindings
            // represented by this column have the same type classification.
            return Some(copy);
        }
        let r = body.rows.get(&row?)?;
        let mut cells = [&r.cells].into_iter()
            .chain(r.before.iter())
            .chain(r.arms.iter().map(|(_, cells)| cells))
            .filter_map(|cells| cells.get(column));
        let holder_type = cells.next()?.holder_type?;
        let HolderType::OptionRef { copy, .. } = holder_type else { return None };
        cells.all(|c| c.holder_type == Some(holder_type)).then_some(copy)
    }

    pub fn array_len(&self, function: FunctionStart, column: &str, cell: &Cell) -> Option<usize> {
        self.bodies.get(&function.body)?.array_lengths.get(&(cell.local?, column.to_owned())).copied()
    }

    /// Whether `column`'s field columns are all of its fields (none is
    /// private to another module).
    pub fn fields_complete(&self, function: FunctionStart, column: &str) -> bool {
        // (A Box's payload's fields are its: `*b`'s hidden ones too.)
        self.bodies.get(&function.body)
            .is_some_and(|b| !b.hidden_fields.contains(column) && !b.hidden_fields.contains(&format!("*{column}")))
    }

    /// The first row below `row` with a state of its own, in the body of
    /// this function body (P1-4b's reference
    /// columns).
    pub fn next_state_row(&self, function: FunctionStart, row: usize) -> Option<usize> {
        let body = self.bodies.get(&function.body)?;
        if row >= body.last_line {
            return None;
        }
        body.rows.range(row + 1..=body.last_line).find(|(_, r)| r.snapshot).map(|(l, _)| *l)
    }

    /// Whether `row` has no event about any place of `column`'s variable
    /// (`p`, `p.a`, `*b`): its state before the row is then its state
    /// after it. `values_only`: only events that change what it holds
    /// count (a move, an assignment, a drop), not its loans.
    pub fn quiet(&self, function: FunctionStart, column: &str, row: usize, values_only: bool) -> bool {
        let Some(body) = self.bodies.get(&function.body) else { return false };
        let Some(r) = body.rows.get(&row) else { return true };
        let root = |c: &str| c.trim_start_matches('*').split(['.', '[']).next().unwrap_or_default().to_owned();
        // (Not a place reached through a reference it holds, `*h.r = ..`:
        // that writes what h.r points to, not h. Such places have no
        // column; a Box's payload `*b` has one.)
        let its = |c: &str| root(c) == root(column) && (!c.starts_with('*') || body.columns.iter().any(|k| k == c));
        r.why.iter().filter(|(c, _)| its(c)).all(|(_, ws)| {
            ws.iter().all(|(_, w)| values_only && matches!(w, Why::Lent { .. } | Why::LoanEnded { .. }))
        })
    }

    /// The place `loan` borrows, as the facts name it (`x`, `*r`).
    pub fn loan_lender(&self, function: FunctionStart, loan: usize) -> Option<&str> {
        self.loan(function, loan).map(|l| l.lender.as_str())
    }

    pub(crate) fn projected_lender(&self, fn_line: FunctionStart, name: &str) -> Option<&ProjectedPlace> {
        self.bodies.get(&fn_line.body)?.loans.values().find_map(|loan|
            (loan.lender == name).then_some(loan.projected.as_ref()).flatten())
    }

    /// The source line `loan` is taken on.
    pub fn loan_line(&self, function: FunctionStart, loan: usize) -> Option<usize> {
        self.loan(function, loan).map(|l| l.line)
    }

    /// Whether `loan` (restricting `column`) borrows the column itself, a
    /// place it is in (`p` for `p.a`) or, for a Box, its contents (`*b`),
    /// rather than a part of it (`p.a` for `p`, an element `v[_]`).
    pub fn lends_whole(&self, function: FunctionStart, column: &str, loan: usize) -> bool {
        let Some(lender) = self.loan_lender(function, loan) else { return false };
        // The facts name a Box payload's fields without the deref (`b.0` for
        // `(*b).0`): a loan of `*b` (or `**b`) covers `b` and them.
        let place = lender.trim_start_matches('*');
        place == column || column.starts_with(&format!("{place}.")) || column.starts_with(&format!("{place}["))
    }

    /// The arm of a one-line branch `loan` was taken in (`if c { &mut x }
    /// else { &mut y }`; held on that arm's path only), as (row, branch,
    /// arm), with the number of arms that branch has.
    pub fn loan_arm(&self, function: FunctionStart, loan: usize) -> Option<((usize, usize, usize), usize)> {
        let b = self.bodies.get(&function.body)?;
        let info = b.loans.get(&loan)?;
        let r = b.rows.get(&info.line)?;
        // The innermost arm with the loan's column in it.
        let (a, _) = r.arms.iter().filter(|(a, _)| a.col_lo <= info.col && info.col <= a.col_hi).min_by_key(|(a, _)| a.col_hi - a.col_lo)?;
        let n = r.arms.iter().filter(|(o, _)| o.branch == a.branch).count();
        Some(((info.line, a.branch, a.arm), n))
    }

    /// Whether the table contains this MIR body.
    pub fn has_fn(&self, function: FunctionStart) -> bool {
        self.bodies.contains_key(&function.body)
    }
}

fn cells_of(st: &State) -> Cells {
    let mut cells = Cells::new();
    for (col, s) in &st.state {
        let Some(init) = Init::parse(s) else { continue };
        let lent = st.lent.get(col).cloned().unwrap_or_default();
        let parts = st.parts.get(col).cloned();
        let may = init == Init::Maybe || parts.as_ref().is_some_and(|p| !p.maybe.is_empty());
        cells.insert(
            col.clone(),
            Cell {
                local: st.locals.get(col).copied().flatten(),
                init,
                lent,
                parts,
                cause: st.cause.get(col).copied(),
                always_assigned: st.always_assigned.contains(col),
                reassigned: st.reassigned.get(col).copied(),
                moved_in_arm: st.moved_in_arm.contains(col),
                may,
                may_lend: st.may_lend.contains(col),
                unnamed_maybe: st.unnamed_maybe.contains(col),
                borrowed_from: st.holder_sources.get(col).cloned().unwrap_or_default(),
                holder_sources: st
                    .lent
                    .get(col)
                    .into_iter()
                    .flatten()
                    .flat_map(|l| &l.current)
                    .filter_map(|h| Some((h.clone(), st.holder_sources.get(h)?.clone())))
                    .collect(),
                holds: st.holds.get(col).cloned().unwrap_or_default(),
                holder_type: st.holder_types.get(col).copied(),
                manually_taken: st.manually_taken.get(col).cloned(),
                option_holds: st.option_holds.get(col).cloned().unwrap_or_default(),
                from_row: 0,
                why: Vec::new(),
            },
        );
    }
    cells
}

fn body_table(b: &BodyFacts) -> BodyTable {
    // Which columns a loan restricts, for its events.
    let loan_cols: BTreeMap<usize, &Vec<String>> = b.loans.iter().map(|l| (l.loan, &l.columns)).collect();
    let columns: Vec<String> = b.columns.iter().map(|c| c.name.clone()).collect();
    // The columns a field path is inside (`p` for `p.a.b`, `b` for `b.a`).
    let containing = |path: &str| -> Vec<String> {
        columns.iter().filter(|c| path.starts_with(&format!("{}.", c))).cloned().collect()
    };
    let mut moved_places = BTreeMap::<_, BTreeSet<_>>::new();
    let mut rows = BTreeMap::new();
    for r in &b.rows {
        let mut rc = RowCells { snapshot: !r.after.state.is_empty() || !r.after.locals.is_empty(), continues_to: r.continues_to, cells: cells_of(&r.after), before: r.before.as_ref().map(cells_of), block_end: r.block_end, jumps: r.jumps, ..Default::default() };
        for a in &r.arms {
            let info = ArmInfo { runs: a.runs, branch: a.branch, arm: a.arm, parent: a.parent, col_lo: a.col_lo, col_hi: a.col_hi, has_exit: a.has_exit,
                switch_col: a.switch_col, code_spans: a.code_spans.clone(), continuing: a.continuing.as_ref().map(cells_of) };
            rc.arms.push((info, cells_of(&a.after)));
        }
        for ev in &r.facts {
            let arm = ev.arm;
            let mut add = |col: &str, w: Why| rc.why.entry(col.to_owned()).or_default().push((arm, w));
            match &ev.fact {
                Fact::Move { path, to, .. } => {
                    if let Some(local) = ev.place_local { moved_places.entry(local).or_default().insert(path.clone()); }
                    add(path, Why::Moved { to: to.clone() });
                    for c in containing(path) {
                        add(&c, Why::FieldMoved { field: path.clone(), to: to.clone() });
                    }
                }
                Fact::Init { path, reassign, maybe_had, .. } => add(path, Why::Assigned { reassign: *reassign, maybe_had: *maybe_had }),
                Fact::Drop { path, how, early_exit, at_reassign, .. } if ev.drop_route != DropRoute::Unwind => add(path, Why::Dropped {
                    local: ev.place_local, how, early_exit: *early_exit, at_reassign: *at_reassign, route: ev.drop_route,
                }),
                Fact::Borrow { loan, .. } => {
                    for c in loan_cols.get(loan).into_iter().flat_map(|v| v.iter()) {
                        add(c, Why::Lent { loan: *loan });
                    }
                }
                Fact::BorrowEnd { loan, why, to_line, to_col, .. } => {
                    for c in loan_cols.get(loan).into_iter().flat_map(|v| v.iter()) {
                        add(c, Why::LoanEnded { loan: *loan, cause: why.clone(), to_line: *to_line, to_col: *to_col });
                    }
                }
                _ => {}
            }
        }
        rows.insert(r.line, rc);
    }
    let array_lengths = b.columns.iter().filter_map(|c| Some(((c.local, c.name.clone()), c.array_len?))).collect();
    let boxes = b.columns.iter().filter(|c| c.is_box).map(|c| c.name.clone()).collect();
    let part_lenders = b.loans.iter().flat_map(|l| l.part_of.iter().cloned()).collect();
    let loans = b.loans.iter().map(|l| (l.loan, l.clone())).collect();
    let hidden_fields = b.columns.iter().filter(|c| c.hidden_fields).map(|c| c.name.clone()).collect();
    BodyTable { drop_holders: b.drop_holders.clone(), joins: b.joins.iter().map(|(s, j)| (*s, cells_of(&j.state))).collect(), first_line: b.first_line, last_line: b.last_line, function_body_start: b.function_body_start, rows, columns, moved_places, array_lengths, boxes, uniform_option_refs: b.uniform_option_refs.clone(), loans, part_lenders, hidden_fields, dead_lines: b.dead_lines.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::schema::{Arm, BodyFacts, ColumnInfo, Ev, LoanInfo, Row, State};

    fn state(cols: &[(&str, &'static str)], lent: &[(&str, usize, &'static str)]) -> State {
        let mut after = State::default();
        for (c, s) in cols {
            after.state.insert(c.to_string(), s);
        }
        for (c, loan, kind) in lent {
            after.lent.entry(c.to_string()).or_default().push(Lent { loan: *loan, kind, holder: "r".into(), ..Default::default() });
        }
        after
    }

    fn row(line: usize, cols: &[(&str, &'static str)], lent: &[(&str, usize, &'static str)], facts: Vec<(Option<usize>, Fact)>) -> Row {
        let facts = facts.into_iter().map(|(arm, fact)| Ev { location: rustc_middle::mir::Location::START, display_location: rustc_middle::mir::Location::START, place_local: None, drop_route: DropRoute::Normal, end_successor: None, end_places: vec![], caller_slot: None, transfer_paths: vec![], col: 1, arm, fact, order: (0, 0, 0), site: None }).collect();
        Row { line, continues_to: None, after: state(cols, lent), before: None, block_end: false, jumps: false, facts, arms: Vec::new() }
    }

    fn col(name: &str) -> ColumnInfo {
        ColumnInfo { local: rustc_middle::mir::Local::from_usize(1), name: name.into(), root: name.split('.').next().unwrap().into(), kind: "var", copy: false, needs_drop: true, is_box: false, array_len: None, hidden_fields: false }
    }

    fn body(func: &str, first: usize, cols: &[&str], rows: Vec<Row>) -> BodyFacts {
        BodyFacts {
            drop_holders: Default::default(),
            closure_result_loans: Default::default(),
            bindings: Default::default(),
            aggregate_loans: Default::default(),
            issued_operand_loans: Default::default(),
            returned_operand_loans: Default::default(),
            conditional_ends: Default::default(),
            argument_stores: Default::default(), receiver_loans: Default::default(), restored_reborrows: Default::default(),
            inline_holder_ends: vec![],
            evaluated_spans: Vec::new(),
            constructed_vecs: Default::default(),
            def_id: def(1),
            take_calls: BTreeSet::new(),
            skipped_closure_returns: Default::default(),
            func: func.into(),
            kind: "fn",
            parent: None,
            first_line: first,
            last_line: first + 9,
            function_body_start: Some((first, 0)),
            vars: Vec::new(),
            columns: cols.iter().map(|c| col(c)).collect(),
            uniform_option_refs: Default::default(),
            binding_loans: BTreeMap::new(),
            loans: vec![LoanInfo {
                projected: None,
                issue: None,
                loan: 0,
                line: 3,
                col: 5,
                lender: "s".into(),
                columns: vec!["s".into()],
                whole: BTreeSet::from(["s".into()]),
                direct_holder: Some("r".into()),
                holds_ref_slot: false,
                part_of: Vec::new(),
                runs: true,
                kind: "shared",
                two_phase: false,
                through: None,
                holders: vec!["r".into()],
                to_call: None,
                in_macro: None,
                end_lines: vec![5],
                source: "borrow",
                made_through: Vec::new(),
                direct_holders: vec!["r".into()],
                via_call: Vec::new(),
                implicit: false,
            }],
            rows,
            temps: Vec::new(),
            notes: Vec::new(),
            binding_sources: Vec::new(),
            joins: BTreeMap::new(),
            dead_lines: BTreeSet::new(),
            imaginary_edges: Vec::new(),
        }
    }

    fn main_body() -> BodyFacts {
        let mut one_line = row(8, &[("s", "maybe")], &[], vec![(Some(0), Fact::Move { path: "s".into(), to: "f()".into(), in_macro: None })]);
        one_line.after.cause.insert("s".into(), "moved");
        one_line.arms = vec![
            Arm { runs: true, branch: 0, arm: 0, parent: None, col_lo: 10, col_hi: 20, has_exit: true,
                switch_col: 5, code_spans: BTreeSet::from([(10, 20)]), continuing: None, after: state(&[("s", "moved")], &[]) },
            Arm { runs: true, branch: 0, arm: 1, parent: None, col_lo: 30, col_hi: 40, has_exit: true,
                switch_col: 5, code_spans: BTreeSet::from([(30, 40)]), continuing: None, after: state(&[("s", "own")], &[]) },
        ];
        body("main", 1, &["s", "p", "p.a"], vec![
            row(2, &[("s", "own")], &[], vec![(None, Fact::Init { path: "s".into(), from: "<value>".into(), reassign: false, maybe_had: false })]),
            row(3, &[("s", "own")], &[("s", 0, "shared")], vec![(None, Fact::Borrow { loan: 0, lender: "s".into(), kind: "shared", holder: "r".into() })]),
            row(5, &[("s", "own")], &[], vec![(None, Fact::BorrowEnd { loan: 0, lender: "s".into(), holder: "r".into(), why: "no further use".into(), to_line: 6, to_col: 5 })]),
            row(6, &[("s", "own"), ("p", "partly"), ("p.a", "moved")], &[], vec![(None, Fact::Move { path: "p.a".into(), to: "x".into(), in_macro: None })]),
            one_line,
            // `}`: s dropped, out of scope (a state without it).
            row(10, &[("q", "own")], &[], vec![(None, Fact::Drop { path: "s".into(), how: "drops", at_reassign: false, early_exit: None, also_on_exits: Vec::new() })]),
        ])
    }

    fn def(index: usize) -> DefId {
        DefId { krate: rustc_hir::def_id::LOCAL_CRATE, index: rustc_hir::def_id::DefIndex::from_usize(index) }
    }

    fn start(index: usize) -> FunctionStart { FunctionStart { body: def(index), line: 1 } }

    fn table() -> StateTable {
        let mut other = body("g", 1, &["s"], vec![row(2, &[("s", "moved")], &[], Vec::new())]);
        other.def_id = def(2);
        StateTable::from_facts(&CrateFacts { version: 2, bodies: vec![main_body(), other], errors: Vec::new() })
    }

    #[test]
    fn a_row_with_state_reads_its_own() {
        let c = table().cell(start(1), "s", 3, 0).unwrap();
        assert_eq!((c.init, c.lent.len(), c.from_row), (Init::Own, 1, 3));
        assert_eq!(c.why, vec![Why::Lent { loan: 0 }]);
    }

    #[test]
    fn a_row_without_state_carries_the_row_above() {
        let c = table().cell(start(1), "s", 4, 0).unwrap();
        assert_eq!((c.init, c.from_row, c.lent.len()), (Init::Own, 3, 1));
        assert!(c.why.is_empty(), "the carried row's events are not this row's");
    }

    #[test]
    fn the_floor_bounds_the_carry_and_an_inverted_floor_is_none() {
        assert_eq!(table().cell(start(1), "s", 4, 4), None);
        assert_eq!(table().cell(start(1), "s", 4, 5), None);
    }

    #[test]
    fn a_dropped_column_is_not_revived() {
        // Row 10 has a state without s: s is out of scope, not row 8's.
        assert_eq!(table().cell(start(1), "s", 10, 0), None);
        assert_eq!(table().cell(start(1), "s", 11, 0), None);
    }

    #[test]
    fn arms_have_their_own_state_and_events() {
        let t = table();
        assert_eq!(t.arms(start(1), 8).len(), 2);
        let a0 = t.cell_in_arm(start(1), "s", 8, 0).unwrap();
        let a1 = t.cell_in_arm(start(1), "s", 8, 1).unwrap();
        assert_eq!((a0.init, a1.init), (Init::Moved, Init::Own));
        assert_eq!(a0.why, vec![Why::Moved { to: "f()".into() }]);
        assert!(a1.why.is_empty(), "the other arm's move is not this arm's");
        let row = t.cell(start(1), "s", 8, 0).unwrap();
        assert_eq!((row.init, row.cause), (Init::Maybe, Some("moved")));
    }

    #[test]
    fn a_field_move_explains_the_struct() {
        let c = table().cell(start(1), "p", 6, 0).unwrap();
        assert_eq!(c.init, Init::Partly);
        assert_eq!(c.why, vec![Why::FieldMoved { field: "p.a".into(), to: "x".into() }]);
    }

    #[test]
    fn loan_ends_keep_their_edge() {
        let c = table().cell(start(1), "s", 5, 0).unwrap();
        assert_eq!(c.why, vec![Why::LoanEnded { loan: 0, cause: "no further use".into(), to_line: 6, to_col: 5 }]);
    }

    #[test]
    fn two_fns_on_one_line_keep_their_own_columns() {
        let t = table();
        assert_eq!(t.cell(start(2), "s", 2, 0).map(|c| c.init), Some(Init::Moved));
        assert_eq!(t.cell(start(1), "s", 2, 0).map(|c| c.init), Some(Init::Own));
        assert!(t.has_fn(start(1)) && t.has_fn(start(2)));
    }

    #[test]
    fn outside_the_body_or_unknown() {
        let t = table();
        assert_eq!(t.cell(start(1), "s", 12, 0), None);
        assert_eq!(t.cell(start(1), "t", 3, 0), None);
        assert_eq!(t.cell(start(3), "s", 3, 0), None);
    }
}
