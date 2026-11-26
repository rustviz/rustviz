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

use super::schema::{BodyFacts, CrateFacts, Fact, Lent, Parts};
use std::collections::BTreeMap;

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
    /// A loan restricting the column went out of scope ("last use",
    /// "holder overwritten", "<x> out of scope", "<x> dropped").
    LoanEnded { loan: usize, cause: String },
    /// Borrowed here.
    Lent { loan: usize },
    /// Given a value (again: `reassign`; perhaps with none before:
    /// `maybe_had`).
    Assigned { reassign: bool, maybe_had: bool },
    /// Its old value dropped ("drops", "drops_if_init", "drops_rest",
    /// "nothing_to_drop"), on this row's path or an early exit's.
    Dropped { how: &'static str, early_exit: Option<usize> },
}

/// One column on one row.
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
    pub why: Vec<Why>,
    /// The row this state was read on (the row asked for, or the row
    /// above it that the state carries over from).
    pub from_row: usize,
}

#[derive(Debug, Default)]
struct RowCells {
    cells: BTreeMap<String, (Init, Vec<Lent>, Option<Parts>)>,
    why: BTreeMap<String, Vec<Why>>,
}

#[derive(Debug)]
struct BodyTable {
    first_line: usize,
    last_line: usize,
    rows: BTreeMap<usize, RowCells>,
}

#[derive(Debug, Default)]
pub struct StateTable {
    /// fn bodies by first line; closure bodies are not drawn as columns
    /// of their own and are left out.
    bodies: BTreeMap<usize, BodyTable>,
}

impl StateTable {
    pub fn from_facts(facts: &CrateFacts) -> StateTable {
        let mut bodies = BTreeMap::new();
        for b in facts.bodies.iter().filter(|b| b.kind == "fn") {
            bodies.insert(b.first_line, body_table(b));
        }
        StateTable { bodies }
    }

    /// The state of `column` of the fn starting on `fn_line` after `row`,
    /// carried from the nearest row at or above it but not above `floor`
    /// (the first row of the leg being drawn; `0` for no bound).
    pub fn cell(&self, fn_line: usize, column: &str, row: usize, floor: usize) -> Option<Cell> {
        let body = self.bodies.get(&fn_line)?;
        if row < body.first_line || row > body.last_line {
            return None;
        }
        let why = body.rows.get(&row).and_then(|r| r.why.get(column)).cloned().unwrap_or_default();
        let (from_row, (init, lent, parts)) = body
            .rows
            .range(floor.max(body.first_line)..=row)
            .rev()
            .find_map(|(r, cells)| cells.cells.get(column).map(|c| (*r, c.clone())))?;
        let may = init == Init::Maybe || parts.as_ref().is_some_and(|p| !p.maybe.is_empty());
        Some(Cell { init, lent, parts, may, why, from_row })
    }

    /// Whether the table has the fn starting on `fn_line`.
    pub fn has_fn(&self, fn_line: usize) -> bool {
        self.bodies.contains_key(&fn_line)
    }
}

fn body_table(b: &BodyFacts) -> BodyTable {
    // Which columns a loan restricts, for its events.
    let loan_cols: BTreeMap<usize, &Vec<String>> = b.loans.iter().map(|l| (l.loan, &l.columns)).collect();
    let mut rows = BTreeMap::new();
    for r in &b.rows {
        let mut rc = RowCells::default();
        for (col, s) in &r.after.state {
            let Some(init) = Init::parse(s) else { continue };
            let lent = r.after.lent.get(col).cloned().unwrap_or_default();
            let parts = r.after.parts.get(col).cloned();
            rc.cells.insert(col.clone(), (init, lent, parts));
        }
        for ev in &r.facts {
            let mut add = |col: &str, w: Why| rc.why.entry(col.to_owned()).or_default().push(w);
            match &ev.fact {
                Fact::Move { path, to, .. } => add(path, Why::Moved { to: to.clone() }),
                Fact::Init { path, reassign, maybe_had, .. } => add(path, Why::Assigned { reassign: *reassign, maybe_had: *maybe_had }),
                Fact::Drop { path, how, early_exit, .. } => add(path, Why::Dropped { how, early_exit: *early_exit }),
                Fact::Borrow { loan, .. } => {
                    for c in loan_cols.get(loan).into_iter().flat_map(|v| v.iter()) {
                        add(c, Why::Lent { loan: *loan });
                    }
                }
                Fact::BorrowEnd { loan, why, .. } => {
                    for c in loan_cols.get(loan).into_iter().flat_map(|v| v.iter()) {
                        add(c, Why::LoanEnded { loan: *loan, cause: why.clone() });
                    }
                }
                _ => {}
            }
        }
        rows.insert(r.line, rc);
    }
    BodyTable { first_line: b.first_line, last_line: b.last_line, rows }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::schema::{BodyFacts, Ev, LoanInfo, Row, State};

    fn row(line: usize, state: &[(&str, &'static str)], lent: &[(&str, usize, &'static str)], facts: Vec<Fact>) -> Row {
        let mut after = State::default();
        for (c, s) in state {
            after.state.insert(c.to_string(), s);
        }
        for (c, loan, kind) in lent {
            after.lent.entry(c.to_string()).or_default().push(Lent { loan: *loan, kind, holder: "r".into() });
        }
        let facts = facts.into_iter().map(|fact| Ev { col: 1, arm: None, fact, order: (0, 0, 0) }).collect();
        Row { line, after, facts, arms: Vec::new() }
    }

    fn body() -> BodyFacts {
        BodyFacts {
            func: "main".into(),
            kind: "fn",
            parent: None,
            first_line: 1,
            last_line: 9,
            vars: Vec::new(),
            columns: Vec::new(),
            loans: vec![LoanInfo {
                loan: 0,
                line: 3,
                col: 5,
                lender: "s".into(),
                columns: vec!["s".into()],
                kind: "shared",
                two_phase: false,
                through: None,
                holders: vec!["r".into()],
                to_call: None,
                in_macro: None,
                end_lines: vec![5],
            }],
            rows: vec![
                row(2, &[("s", "own")], &[], vec![Fact::Init { path: "s".into(), from: "<value>".into(), reassign: false, maybe_had: false }]),
                row(3, &[("s", "own")], &[("s", 0, "shared")], vec![Fact::Borrow { loan: 0, lender: "s".into(), kind: "shared", holder: "r".into() }]),
                row(5, &[("s", "own")], &[], vec![Fact::BorrowEnd { loan: 0, lender: "s".into(), holder: "r".into(), why: "last use".into(), to_line: 6 }]),
                row(7, &[("s", "maybe")], &[], vec![Fact::Move { path: "s".into(), to: "f()".into(), in_macro: None }]),
            ],
            temps: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn table() -> StateTable {
        StateTable::from_facts(&CrateFacts { version: 2, bodies: vec![body()], errors: Vec::new() })
    }

    #[test]
    fn a_row_with_state_reads_its_own() {
        let c = table().cell(1, "s", 3, 0).unwrap();
        assert_eq!(c.init, Init::Own);
        assert_eq!(c.lent.len(), 1);
        assert_eq!(c.why, vec![Why::Lent { loan: 0 }]);
        assert_eq!(c.from_row, 3);
    }

    #[test]
    fn a_row_without_state_carries_the_row_above() {
        let c = table().cell(1, "s", 4, 0).unwrap();
        assert_eq!((c.init, c.from_row, c.lent.len()), (Init::Own, 3, 1));
        assert!(c.why.is_empty(), "the carried row's events are not this row's");
    }

    #[test]
    fn the_floor_bounds_the_carry() {
        // A leg starting on row 4 doesn't take row 3's state.
        assert_eq!(table().cell(1, "s", 4, 4), None);
    }

    #[test]
    fn maybe_and_why() {
        let c = table().cell(1, "s", 7, 0).unwrap();
        assert_eq!(c.init, Init::Maybe);
        assert!(c.may);
        assert_eq!(c.why, vec![Why::Moved { to: "f()".into() }]);
        let e = table().cell(1, "s", 5, 0).unwrap();
        assert_eq!(e.why, vec![Why::LoanEnded { loan: 0, cause: "last use".into() }]);
    }

    #[test]
    fn outside_the_body_or_unknown() {
        let t = table();
        assert_eq!(t.cell(1, "s", 12, 0), None);
        assert_eq!(t.cell(1, "t", 3, 0), None);
        assert_eq!(t.cell(2, "s", 3, 0), None);
        assert!(t.has_fn(1) && !t.has_fn(2));
    }
}
