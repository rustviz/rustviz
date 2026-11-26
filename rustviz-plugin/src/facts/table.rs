//! The state table (RustViz 3, P1): what the renderer will read for each
//! column's state on each row, instead of replaying events.
//!
//! Built from the fact layer's [`CrateFacts`] in the plugin (no JSON round
//! trip). A body is found by its first line (the fn signature's line, the
//! same `fn_start_line` the 2.x RAP keys use), a column by its 2.x name
//! (`s`, `s₂`, `p.a`). A row with no state of its own takes the nearest
//! row above it on the same path (design Q3): the caller bounds the search
//! by the first row of the leg it draws, since rows above a leg's start
//! may belong to another arm.

use super::schema::{LoanInfo, BodyFacts, CrateFacts, Fact, Lent, Parts, State};
use std::collections::{BTreeMap, BTreeSet};

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
    Dropped { how: &'static str, early_exit: Option<usize>, at_reassign: bool },
}

/// One column on one row (or one arm of a row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
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
    pub always_assigned: bool,
    pub why: Vec<Why>,
    /// The row this state was read on (the row asked for, or the row
    /// above it that the state carries over from).
    pub from_row: usize,
}

type Cells = BTreeMap<String, Cell>;

/// An arm of a one-line branch on a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmInfo {
    pub branch: usize,
    pub arm: usize,
    pub parent: Option<usize>,
    pub col_lo: usize,
    pub col_hi: usize,
}

#[derive(Debug, Default)]
struct RowCells {
    /// The row has a state of its own (MIR code runs on it on a way that
    /// goes on): a column missing from it is out of scope there.
    snapshot: bool,
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
    rows: BTreeMap<usize, RowCells>,
    /// Column names, in the facts' order.
    columns: Vec<String>,
    array_lengths: BTreeMap<String, usize>,
    boxes: BTreeSet<String>,
    /// Each loan's borrowed place (`s`, `p.a`, `*b`, `v[_]`).
    loans: BTreeMap<usize, LoanInfo>,
    /// Columns with fields this body can't name (no columns of their own).
    hidden_fields: BTreeSet<String>,
}

/// The state table of a crate: per fn body, per row (and arm), per
/// column. Rows are source lines: a renderer that hides or inserts lines
/// maps its visual rows back (`VisualizationData::row_lines`) before it
/// asks.
#[derive(Debug, Default)]
pub struct StateTable {
    /// fn bodies by first source line (several fns may start on one line);
    /// closure bodies are not drawn as columns of their own and are left
    /// out.
    bodies: BTreeMap<usize, Vec<BodyTable>>,
}

impl StateTable {
    pub fn from_facts(facts: &CrateFacts) -> StateTable {
        let mut bodies: BTreeMap<usize, Vec<BodyTable>> = BTreeMap::new();
        for b in facts.bodies.iter().filter(|b| b.kind == "fn") {
            bodies.entry(b.first_line).or_default().push(body_table(b));
        }
        StateTable { bodies }
    }

    /// The body starting on `fn_line` that has `column`.
    fn body(&self, fn_line: usize, column: &str) -> Option<&BodyTable> {
        self.bodies.get(&fn_line)?.iter().find(|b| b.columns.iter().any(|c| c == column))
    }

    /// The state of `column` of the fn starting on `fn_line` after `row`,
    /// carried from the nearest row at or above it that has a state of
    /// its own, but not above `floor` (the first row of the leg being
    /// drawn; `0` for no bound). None where the column is out of scope
    /// (the nearest state doesn't have it) or nothing is known.
    pub fn cell(&self, fn_line: usize, column: &str, row: usize, floor: usize) -> Option<Cell> {
        self.cell_on(fn_line, column, row, floor, None)
    }

    /// `cell` on one arm of a row with several (`arms`): its own state and
    /// events.
    pub fn cell_in_arm(&self, fn_line: usize, column: &str, row: usize, arm: usize) -> Option<Cell> {
        self.cell_on(fn_line, column, row, row, Some(arm))
    }

    fn cell_on(&self, fn_line: usize, column: &str, row: usize, floor: usize, arm: Option<usize>) -> Option<Cell> {
        let body = self.body(fn_line, column)?;
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
    pub fn segment_cell(&self, fn_line: usize, column: &str, row: usize) -> Option<Cell> {
        let b = self.body(fn_line, column)?;
        if !(b.first_line..b.last_line).contains(&row) { return None; }
        if let Some(rc) = b.rows.get(&row) {
            if rc.snapshot && !rc.block_end && !rc.jumps {
                let mut own = self.cell(fn_line, column, row, row)?;
                // The row's state joins all its exits; the line below it is
                // on the ones that come into the next row (`if let Some(x)
                // = b && x.len() > 1 {`: x is bound in the block below,
                // unassigned only on the way past it). Where those all hold
                // a value, so does the line.
                if own.init == Init::Maybe {
                    let next = b.rows.range(row + 1..).find(|(_, c)| c.before.is_some()).and_then(|(_, c)| c.before.as_ref()?.get(column));
                    // (All of that boundary's state: its loans and holders go
                    // with its init.)
                    if let Some(n) = next.filter(|n| n.init == Init::Own) {
                        let mut n = n.clone();
                        n.from_row = own.from_row;
                        n.why = own.why;
                        return Some(n);
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
        if let Some(here) = self.cell(fn_line, column, row, row).filter(|_| !jumps) {
            // Paths on which a block-local was never in scope can meet at
            // its StorageDead. Inside its scope the binding has its own state.
            let definitely_dropped = boundary.why.get(column).is_some_and(|events| events.iter().any(|(_, why)|
                matches!(why, Why::Dropped { how: "drops", early_exit: None, at_reassign: false })));
            if (definitely_dropped && here.init == Init::Own) || here.why.iter().any(|w| matches!(w, Why::Assigned { .. }))
                || (cell.cause == Some("unassigned") && !boundary.cells.contains_key(column)) {
                return Some(here);
            }
            // (The boundary's loans and holders go with its init: they
            // describe the same incoming edges.)
        }
        cell.from_row = next;
        Some(cell)
    }

    /// The next MIR boundary is the function's scope exit and actually
    /// drops the remainder of this place (drop elaboration).
    pub fn drops_rest_at_exit(&self, function: usize, column: &str, row: usize) -> bool {
        let Some(body) = self.body(function, column) else { return false };
        let Some((&next, next_row)) = body.rows.range(row + 1..).next() else { return false };
        next == body.last_line && next_row.why.get(column).is_some_and(|events| events.iter().any(|(_, why)|
            matches!(why, Why::Dropped { how: "drops_rest", early_exit: None, at_reassign: false })))
    }

    pub fn loan(&self, fn_line: usize, column: &str, loan: usize) -> Option<&LoanInfo> {
        self.body(fn_line, column)?.loans.get(&loan)
    }

    /// The source lines of the loans of `lender` (a temporary's `(code)`
    /// name) in the body that has `column`.
    pub fn lender_lines(&self, fn_line: usize, column: &str, lender: &str) -> Vec<usize> {
        self.body(fn_line, column).map_or_else(Vec::new, |b| b.loans.values().filter(|l| l.lender == lender).map(|l| l.line).collect())
    }

    /// Whether `column` has no value on some row above `row` while in
    /// scope (`let x;`): a binding a pattern gives its value when it comes
    /// into scope (`if let Some(x) = o`) never has.
    pub fn unassigned_above(&self, fn_line: usize, column: &str, row: usize) -> bool {
        let Some(b) = self.body(fn_line, column) else { return false };
        b.rows.range(..row).any(|(_, rc)| rc.cells.get(column).is_some_and(|c| c.init == Init::Unassigned)
            || rc.arms.iter().any(|(_, cells)| cells.get(column).is_some_and(|c| c.init == Init::Unassigned)))
    }

    /// Whether the body moves `column`, a place it is in or a place in it
    /// out on some row (a `Move` fact): a column the table says moved (or
    /// maybe moved) with none is out of its scope there, not moved.
    pub fn has_move(&self, fn_line: usize, column: &str) -> bool {
        let Some(b) = self.body(fn_line, column) else { return false };
        let me = column.trim_start_matches('*');
        let within = |a: &str, b: &str| a.starts_with(&format!("{b}.")) || a.starts_with(&format!("{b}["));
        let related = |c: &str| {
            let c = c.trim_start_matches('*');
            c == me || within(me, c) || within(c, me)
        };
        // (A field moved out of a struct, `drop(p.a)`, is recorded on `p`:
        // it moves `p.b` only if it is `p.b` or contains it.)
        b.rows.values().any(|r| r.why.iter().any(|(c, ws)| {
            related(c) && ws.iter().any(|(_, w)| match w {
                Why::Moved { .. } => true,
                Why::FieldMoved { field, .. } => related(field),
                _ => false,
            })
        }))
    }

    /// The arms of a one-line branch on `row` of the fn starting on
    /// `fn_line` (the body that has `column`).
    pub fn arms(&self, fn_line: usize, column: &str, row: usize) -> Vec<ArmInfo> {
        self.body(fn_line, column)
            .and_then(|b| b.rows.get(&row))
            .map_or(Vec::new(), |r| r.arms.iter().map(|(a, _)| a.clone()).collect())
    }

    /// The direct field columns of `column` (`p.a`, `p.b` of `p`).
    pub fn fields_of(&self, fn_line: usize, column: &str) -> Vec<String> {
        let prefix = format!("{}.", column);
        self.body(fn_line, column).map_or(Vec::new(), |b| {
            b.columns
                .iter()
                .filter(|n| n.strip_prefix(&prefix).is_some_and(|rest| !rest.is_empty() && !rest.contains('.')))
                .cloned()
                .collect()
        })
    }

    pub fn is_box(&self, fn_line: usize, column: &str) -> bool {
        self.body(fn_line, column).is_some_and(|b| b.boxes.contains(column))
    }

    pub fn array_len(&self, fn_line: usize, column: &str) -> Option<usize> {
        self.body(fn_line, column)?.array_lengths.get(column).copied()
    }

    /// Whether `column`'s field columns are all of its fields (none is
    /// private to another module).
    pub fn fields_complete(&self, fn_line: usize, column: &str) -> bool {
        // (A Box's payload's fields are its: `*b`'s hidden ones too.)
        self.body(fn_line, column)
            .is_some_and(|b| !b.hidden_fields.contains(column) && !b.hidden_fields.contains(&format!("*{column}")))
    }

    /// The first row below `row` with a state of its own, in the body of
    /// the fn starting on `fn_line` that has `column` (P1-4b's reference
    /// columns).
    pub fn next_state_row(&self, fn_line: usize, column: &str, row: usize) -> Option<usize> {
        let body = self.body(fn_line, column)?;
        if row >= body.last_line {
            return None;
        }
        body.rows.range(row + 1..=body.last_line).find(|(_, r)| r.snapshot).map(|(l, _)| *l)
    }

    /// Whether `row` has no event about any place of `column`'s variable
    /// (`p`, `p.a`, `*b`): its state before the row is then its state
    /// after it. `values_only`: only events that change what it holds
    /// count (a move, an assignment, a drop), not its loans.
    pub fn quiet(&self, fn_line: usize, column: &str, row: usize, values_only: bool) -> bool {
        let Some(body) = self.body(fn_line, column) else { return false };
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
    pub fn loan_lender(&self, fn_line: usize, column: &str, loan: usize) -> Option<&str> {
        self.loan(fn_line, column, loan).map(|l| l.lender.as_str())
    }

    /// The source line `loan` is taken on.
    pub fn loan_line(&self, fn_line: usize, column: &str, loan: usize) -> Option<usize> {
        self.loan(fn_line, column, loan).map(|l| l.line)
    }

    /// Whether `loan` (restricting `column`) borrows the column itself, a
    /// place it is in (`p` for `p.a`) or, for a Box, its contents (`*b`),
    /// rather than a part of it (`p.a` for `p`, an element `v[_]`).
    pub fn lends_whole(&self, fn_line: usize, column: &str, loan: usize) -> bool {
        let Some(lender) = self.loan_lender(fn_line, column, loan) else { return false };
        // The facts name a Box payload's fields without the deref (`b.0` for
        // `(*b).0`): a loan of `*b` (or `**b`) covers `b` and them.
        let place = lender.trim_start_matches('*');
        place == column || column.starts_with(&format!("{place}.")) || column.starts_with(&format!("{place}["))
    }

    /// The arm of a one-line branch `loan` was taken in (`if c { &mut x }
    /// else { &mut y }`; held on that arm's path only), as (row, branch,
    /// arm), with the number of arms that branch has.
    pub fn loan_arm(&self, fn_line: usize, column: &str, loan: usize) -> Option<((usize, usize, usize), usize)> {
        let b = self.body(fn_line, column)?;
        let info = b.loans.get(&loan)?;
        let r = b.rows.get(&info.line)?;
        // The innermost arm with the loan's column in it.
        let (a, _) = r.arms.iter().filter(|(a, _)| a.col_lo <= info.col && info.col <= a.col_hi).min_by_key(|(a, _)| a.col_hi - a.col_lo)?;
        let n = r.arms.iter().filter(|(o, _)| o.branch == a.branch).count();
        Some(((info.line, a.branch, a.arm), n))
    }

    /// Whether the table has a fn starting on `fn_line`.
    pub fn has_fn(&self, fn_line: usize) -> bool {
        self.bodies.contains_key(&fn_line)
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
                init,
                lent,
                parts,
                cause: st.cause.get(col).copied(),
                always_assigned: st.always_assigned.contains(col),
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
    let mut rows = BTreeMap::new();
    for r in &b.rows {
        let mut rc = RowCells { snapshot: !r.after.state.is_empty(), cells: cells_of(&r.after), before: r.before.as_ref().map(cells_of), block_end: r.block_end, jumps: r.jumps, ..Default::default() };
        for a in &r.arms {
            let info = ArmInfo { branch: a.branch, arm: a.arm, parent: a.parent, col_lo: a.col_lo, col_hi: a.col_hi };
            rc.arms.push((info, cells_of(&a.after)));
        }
        for ev in &r.facts {
            let arm = ev.arm;
            let mut add = |col: &str, w: Why| rc.why.entry(col.to_owned()).or_default().push((arm, w));
            match &ev.fact {
                Fact::Move { path, to, .. } => {
                    add(path, Why::Moved { to: to.clone() });
                    for c in containing(path) {
                        add(&c, Why::FieldMoved { field: path.clone(), to: to.clone() });
                    }
                }
                Fact::Init { path, reassign, maybe_had, .. } => add(path, Why::Assigned { reassign: *reassign, maybe_had: *maybe_had }),
                Fact::Drop { path, how, early_exit, at_reassign, .. } => add(path, Why::Dropped { how, early_exit: *early_exit, at_reassign: *at_reassign }),
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
    let array_lengths = b.columns.iter().filter_map(|c| Some((c.name.clone(), c.array_len?))).collect();
    let boxes = b.columns.iter().filter(|c| c.is_box).map(|c| c.name.clone()).collect();
    let loans = b.loans.iter().map(|l| (l.loan, l.clone())).collect();
    let hidden_fields = b.columns.iter().filter(|c| c.hidden_fields).map(|c| c.name.clone()).collect();
    BodyTable { first_line: b.first_line, last_line: b.last_line, rows, columns, array_lengths, boxes, loans, hidden_fields }
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
        let facts = facts.into_iter().map(|(arm, fact)| Ev { col: 1, arm, fact, order: (0, 0, 0), site: None }).collect();
        Row { line, after: state(cols, lent), before: None, block_end: false, jumps: false, facts, arms: Vec::new() }
    }

    fn col(name: &str) -> ColumnInfo {
        ColumnInfo { name: name.into(), root: name.split('.').next().unwrap().into(), kind: "var", copy: false, needs_drop: true, is_box: false, array_len: None, hidden_fields: false }
    }

    fn body(func: &str, first: usize, cols: &[&str], rows: Vec<Row>) -> BodyFacts {
        BodyFacts {
            func: func.into(),
            kind: "fn",
            parent: None,
            first_line: first,
            last_line: first + 9,
            vars: Vec::new(),
            columns: cols.iter().map(|c| col(c)).collect(),
            loans: vec![LoanInfo {
                loan: 0,
                line: 3,
                col: 5,
                lender: "s".into(),
                columns: vec!["s".into()],
                whole: BTreeSet::from(["s".into()]),
                direct_holder: Some("r".into()),
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
        }
    }

    fn main_body() -> BodyFacts {
        let mut one_line = row(8, &[("s", "maybe")], &[], vec![(Some(0), Fact::Move { path: "s".into(), to: "f()".into(), in_macro: None })]);
        one_line.after.cause.insert("s".into(), "moved");
        one_line.arms = vec![
            Arm { branch: 0, arm: 0, parent: None, col_lo: 10, col_hi: 20, after: state(&[("s", "moved")], &[]) },
            Arm { branch: 0, arm: 1, parent: None, col_lo: 30, col_hi: 40, after: state(&[("s", "own")], &[]) },
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

    fn table() -> StateTable {
        let other = body("g", 1, &["y"], vec![row(2, &[("y", "own")], &[], Vec::new())]);
        StateTable::from_facts(&CrateFacts { version: 2, bodies: vec![main_body(), other], errors: Vec::new() })
    }

    #[test]
    fn a_row_with_state_reads_its_own() {
        let c = table().cell(1, "s", 3, 0).unwrap();
        assert_eq!((c.init, c.lent.len(), c.from_row), (Init::Own, 1, 3));
        assert_eq!(c.why, vec![Why::Lent { loan: 0 }]);
    }

    #[test]
    fn a_row_without_state_carries_the_row_above() {
        let c = table().cell(1, "s", 4, 0).unwrap();
        assert_eq!((c.init, c.from_row, c.lent.len()), (Init::Own, 3, 1));
        assert!(c.why.is_empty(), "the carried row's events are not this row's");
    }

    #[test]
    fn the_floor_bounds_the_carry_and_an_inverted_floor_is_none() {
        assert_eq!(table().cell(1, "s", 4, 4), None);
        assert_eq!(table().cell(1, "s", 4, 5), None);
    }

    #[test]
    fn a_dropped_column_is_not_revived() {
        // Row 10 has a state without s: s is out of scope, not row 8's.
        assert_eq!(table().cell(1, "s", 10, 0), None);
        assert_eq!(table().cell(1, "s", 11, 0), None);
    }

    #[test]
    fn arms_have_their_own_state_and_events() {
        let t = table();
        assert_eq!(t.arms(1, "s", 8).len(), 2);
        let a0 = t.cell_in_arm(1, "s", 8, 0).unwrap();
        let a1 = t.cell_in_arm(1, "s", 8, 1).unwrap();
        assert_eq!((a0.init, a1.init), (Init::Moved, Init::Own));
        assert_eq!(a0.why, vec![Why::Moved { to: "f()".into() }]);
        assert!(a1.why.is_empty(), "the other arm's move is not this arm's");
        let row = t.cell(1, "s", 8, 0).unwrap();
        assert_eq!((row.init, row.cause), (Init::Maybe, Some("moved")));
    }

    #[test]
    fn a_field_move_explains_the_struct() {
        let c = table().cell(1, "p", 6, 0).unwrap();
        assert_eq!(c.init, Init::Partly);
        assert_eq!(c.why, vec![Why::FieldMoved { field: "p.a".into(), to: "x".into() }]);
    }

    #[test]
    fn loan_ends_keep_their_edge() {
        let c = table().cell(1, "s", 5, 0).unwrap();
        assert_eq!(c.why, vec![Why::LoanEnded { loan: 0, cause: "no further use".into(), to_line: 6, to_col: 5 }]);
    }

    #[test]
    fn two_fns_on_one_line_keep_their_own_columns() {
        let t = table();
        assert_eq!(t.cell(1, "y", 2, 0).map(|c| c.init), Some(Init::Own));
        assert_eq!(t.cell(1, "s", 2, 0).map(|c| c.init), Some(Init::Own));
        assert!(t.has_fn(1) && !t.has_fn(2));
    }

    #[test]
    fn outside_the_body_or_unknown() {
        let t = table();
        assert_eq!(t.cell(1, "s", 12, 0), None);
        assert_eq!(t.cell(1, "t", 3, 0), None);
        assert_eq!(t.cell(2, "s", 3, 0), None);
    }
}
