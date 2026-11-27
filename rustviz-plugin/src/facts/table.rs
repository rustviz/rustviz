//! The state table (RustViz 3, P1): what the renderer will read for each
//! column's state on each row, instead of replaying events.
//!
//! Built from the fact layer's [`CrateFacts`] in the plugin (no JSON round
//! trip). A RAP belongs to its compiler body. Its presentation column can
//! represent a different local at each boundary. A row with no state of its own takes the nearest
//! row above it on the same path (design Q3): the caller bounds the search
//! by the first row of the leg it draws, since rows above a leg's start
//! may belong to another arm.

use super::{SourceArm, SwitchEdge};
use rustc_hir::def_id::DefId;
use rustc_data_structures::fx::FxIndexMap;
use super::schema::{ProjectedPlace, DropRoute, LoanInfo, BodyFacts, CrateFacts, Extraction, Fact, HolderType, Lent, PartialDrop, Parts, State};
pub use super::schema::MixedInit;
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
    pub mixed_init: Option<MixedInit>,
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
    pub unwind_before_write: bool,
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

/// Source bounds of a drawn leg for layout, in 1-based character positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmSpan {
    pub start: (usize, usize),
    pub end: (usize, usize),
}

/// An arm of a one-line branch on a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmInfo {
    pub runs: bool,
    pub parent: Option<usize>,
    edge: SwitchEdge,
    source: Option<SourceArm>,
    branch: Option<rustc_hir::HirId>,
    has_exit: bool,
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

/// Only expose a property of a drawn arm when all its MIR exits agree.
fn agreed<T: PartialEq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

impl RowCells {
    /// An instruction in a common target executes on every incoming edge
    /// to that block. Its event is stored only once on the row.
    fn same_target(&self, a: usize, b: usize) -> bool {
        self.arms.get(a).zip(self.arms.get(b)).is_some_and(|((a, _), (b, _))| a.edge.successor == b.edge.successor)
    }

    fn indices(&self, source: SourceArm) -> impl Iterator<Item = usize> + '_ {
        self.arms.iter().enumerate().filter_map(move |(i, (a, _))| (a.source == Some(source)).then_some(i))
    }

    fn siblings(&self, sources: &[Option<SourceArm>]) -> Option<Vec<Vec<usize>>> {
        let branch = sources.first().copied().flatten()?.branch;
        sources.iter().map(|&s| {
            let source = s?;
            let indices: Vec<_> = self.indices(source).collect();
            (source.branch == branch && !indices.is_empty() && !self.arms.iter().any(|(a, _)|
                a.branch != Some(branch) && a.parent.is_some_and(|p| indices.contains(&p)))).then_some(indices)
        }).collect()
    }
}

#[derive(Debug)]
struct BodyTable {
    first_line: usize,
    last_line: usize,
    function_body_start: Option<(usize, usize)>,
    rows: BTreeMap<usize, RowCells>,
    successor_counts: BTreeMap<rustc_middle::mir::Location, usize>,
    joins: BTreeMap<rustc_span::Span, Cells>,
    branch_entries: FxIndexMap<rustc_hir::HirId, Cells>,
    arm_boundaries: FxIndexMap<rustc_hir::HirId, BTreeMap<usize, ArmBoundaryCells>>,
    /// Column names, in the facts' order.
    columns: Vec<String>,
    parents: BTreeMap<String, String>,
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

#[derive(Debug)]
struct ArmBoundaryCells {
    entry: Option<Cells>,
    join: Option<Cells>,
}

type ScopePartials = BTreeMap<(Option<(rustc_span::BytePos, rustc_span::BytePos)>, rustc_middle::mir::Local, String), Option<PartialDrop>>;
type TryDrops = BTreeMap<(rustc_middle::mir::Local, rustc_span::BytePos, rustc_span::BytePos, Option<String>), Option<(&'static str, bool)>>;

#[derive(Debug, Default)]
struct DropWording {
    scope_partials: BTreeMap<usize, ScopePartials>,
    try_drops: TryDrops,
    staged_try_drops: TryDrops,
}

impl DropWording {
    fn from_body(body: &BodyFacts) -> Self {
        fn record<K: Ord, T: PartialEq>(map: &mut BTreeMap<K, Option<T>>, key: K, value: T) {
            use std::collections::btree_map::Entry;
            match map.entry(key) {
                Entry::Vacant(entry) => { entry.insert(Some(value)); }
                Entry::Occupied(mut entry) if entry.get().as_ref() != Some(&value) => { entry.insert(None); }
                Entry::Occupied(_) => {}
            }
        }

        let mut wording = Self::default();
        for row in &body.rows {
            for (site, local, name, partial) in &row.partial_drops {
                let parts = wording.scope_partials.entry(row.line).or_default();
                record(parts, (site.map(|s| (s.lo(), s.hi())), *local, name.clone()), partial.clone());
            }
        }
        for drop in &body.residual_drops {
            let map = if drop.staged { &mut wording.staged_try_drops } else { &mut wording.try_drops };
            record(map, (drop.local, drop.site.lo(), drop.site.hi(), drop.path.clone()), (drop.how, drop.only_assigned));
        }
        wording
    }
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
    drop_wording: FxIndexMap<DefId, DropWording>,
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
        let drop_wording = facts.bodies.iter().filter(|b| b.kind == "fn")
            .map(|b| (b.def_id, DropWording::from_body(b))).collect();
        StateTable { bodies, drop_wording, imaginary_arms, ..Default::default() }
    }

    /// Hidden code still has MIR scope ends and residual exits. Keep the
    /// drop wording and move its row keys with the code panel.
    pub fn hide_rows(&mut self, keep: impl Fn(usize) -> bool, shift: impl Fn(usize) -> usize) {
        let mut drop_wording = std::mem::take(&mut self.drop_wording);
        for wording in drop_wording.values_mut() {
            wording.scope_partials = std::mem::take(&mut wording.scope_partials).into_iter()
                .filter(|(row, _)| keep(*row)).map(|(row, parts)| (shift(row), parts)).collect();
        }
        *self = Self { drop_wording, ..Self::default() };
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

    /// Read the named arm's exit on a shared row. On an interior row the
    /// displayed arm supplies the bounds for carrying its last state forward.
    pub fn arm_loan_cell(&self, function: FunctionStart, column: &str, row: usize, span: ArmSpan, source: Option<SourceArm>) -> Option<(Cell, Option<SourceArm>)> {
        if row < span.start.0 || row > span.end.0 { return None; }
        let body = self.bodies.get(&function.body)?;
        if body.rows.get(&row).is_some_and(|rc| !rc.arms.is_empty()) {
            let arm = source?;
            return Some((self.source_arm_cell(function, column, row, arm)?, Some(arm)));
        }
        if span.start.0 < row && row < span.end.0 {
            return Some((self.loan_cell(function, column, row, span.start.0 + 1)?, None));
        }
        None
    }

    pub fn source_arm_cell(&self, function: FunctionStart, column: &str, row: usize, arm: SourceArm) -> Option<Cell> {
        let rc = self.bodies.get(&function.body)?.rows.get(&row)?;
        agreed(rc.indices(arm).map(|i| self.cell_in_arm(function, column, row, i)))?
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
        let why = here.map(|r| r.why.get(column).into_iter().flatten()
            .filter(|(a, _)| arm.zip(*a).is_none_or(|(i, j)| r.same_target(i, j)))
            .map(|(_, w)| w.clone()).collect()).unwrap_or_default();
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
                        own.mixed_init = n.mixed_init;
                        own.always_assigned = n.always_assigned;
                        own.reassigned = n.reassigned;
                        own.unwind_before_write = n.unwind_before_write;
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

    pub fn incoming_cell(&self, function: FunctionStart, column: &str, row: usize) -> Option<&Cell> {
        self.bodies.get(&function.body)?.rows.get(&row)?.before.as_ref()?.get(column)
    }

    pub fn branch_entry_cell(&self, function: FunctionStart, column: &str, branch: rustc_hir::HirId) -> Option<&Cell> {
        self.bodies.get(&function.body)?.branch_entries.get(&branch)?.get(column)
    }

    pub fn arm_boundary_cell(&self, function: FunctionStart, column: &str, arm: SourceArm, joining: bool) -> Option<&Cell> {
        let bounds = self.bodies.get(&function.body)?.arm_boundaries.get(&arm.branch)?.get(&arm.index)?;
        let cells = if joining { &bounds.join } else { &bounds.entry };
        cells.as_ref()?.get(column)
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
        rc.arms.iter().any(|(a, cells)| a.runs && a.edge.switch == written.edge.switch && a.edge.successor != written.edge.successor
            // A shared target's snapshot includes paths from other tests.
            // It cannot establish what skipping this particular write did.
            && !rc.arms.iter().any(|(other, _)| other.edge.switch != a.edge.switch && other.edge.successor == a.edge.successor)
            && cells.get(column).is_some_and(|c| c.init == Init::Maybe && c.cause == Some("moved")))
    }

    /// A normal scope-end drop (the column can already be out of storage
    /// in the row's after-cell). Several sites must agree.
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

    /// A drop on the residual edge of this compiler span, scoped to the
    /// exact local even when different bindings share a column.
    pub fn try_drop(&self, function: FunctionStart, column: &str, local: rustc_middle::mir::Local, span: rustc_span::Span) -> Option<(&'static str, bool)> {
        let wording = self.drop_wording.get(&function.body)?;
        let key = (local, span.lo(), span.hi(), Some(column.to_owned()));
        wording.staged_try_drops.get(&key).or_else(|| wording.try_drops.get(&key))
            .or_else(|| wording.try_drops.get(&(local, span.lo(), span.hi(), None))).copied().flatten()
    }

    pub fn partial_drop(&self, function: FunctionStart, column: &str, row: usize, local: rustc_middle::mir::Local, site: Option<rustc_span::Span>) -> Option<&PartialDrop> {
        self.drop_wording.get(&function.body)?.scope_partials.get(&row)?
            .get(&(site.map(|s| (s.lo(), s.hi())), local, column.to_owned()))?.as_ref()
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

    pub fn shared_drop_holder<'a>(&self, function: FunctionStart, column: &str, cell: &'a Cell) -> Option<&'a super::schema::DropLoanHolder> {
        let [loan] = cell.lent.as_slice() else { return None };
        if loan.kind != "shared" || !self.loan(function, loan.loan)?.whole.contains(column) {
            return None;
        }
        loan.drop_holder.as_ref()
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

    /// A one-line conditional gets separate rows when its own exits disagree.
    /// Compare sibling exits of a single decision. A guard can commit to the
    /// same body on several edges, and a nested branch has a separate join.
    pub fn arms_disagree(&self, function: DefId, row: usize, columns: &BTreeSet<String>, sources: &[Option<SourceArm>]) -> bool {
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
            let Some(arms) = r.siblings(sources) else { return false };
            let ends = |i| r.why.values().flatten().filter_map(|(a, w)| match w {
                Why::LoanEnded { loan, .. } if a.is_some_and(|j| r.same_target(i, j)) => Some(*loan),
                _ => None,
            }).collect();
            columns.iter().any(|col| {
                let states = arms.iter().map(|group| agreed(group.iter().map(|&i|
                    r.arms[i].1.get(col).map(|c| key(c, &ends(i)))))).collect::<Option<Vec<_>>>();
                states.is_some_and(|s| s.windows(2).any(|w| w[0] != w[1]))
            })
        })
    }

    /// The loans of `column` whose end is one of arm `arm`'s own events on
    /// source row `row` (a loan its code last uses, dying on the arm's way
    /// out to the join): the arm's state, read at its exits, still has
    /// them until the ending edge is applied.
    fn arm_loan_ends(&self, function: FunctionStart, column: &str, row: usize, arm: usize) -> Vec<usize> {
        let Some(r) = self.bodies.get(&function.body).and_then(|b| b.rows.get(&row)) else { return Vec::new() };
        r.why.get(column).into_iter().flatten().filter_map(|(a, w)| match w {
            Why::LoanEnded { loan, .. } if a.is_some_and(|i| r.same_target(i, arm)) => Some(*loan),
            _ => None,
        }).collect()
    }

    /// State after the code on a one-line leg, including loans that end
    /// as that leg leaves the row. Tests within a guard or && are part of
    /// one source decision. A nested source branch has its own join.
    pub fn one_line_arm_cell(&self, function: FunctionStart, column: &str, row: usize, source: SourceArm, sources: &[Option<SourceArm>]) -> Option<Cell> {
        let r = self.bodies.get(&function.body)?.rows.get(&row)?;
        if r.arms.iter().any(|(a, _)| a.parent.is_some_and(|p| r.arms[p].0.branch != a.branch)) { return None; }
        r.siblings(sources)?;
        agreed(r.indices(source).map(|i| {
            let mut cell = self.cell_in_arm(function, column, row, i)?;
            let ends = self.arm_loan_ends(function, column, row, i);
            cell.lent.retain(|l| !ends.contains(&l.loan));
            Some(cell)
        }))?
    }

    pub fn arm_has_exit(&self, function: DefId, row: usize, source: SourceArm) -> bool {
        self.bodies.get(&function).and_then(|b| b.rows.get(&row))
            .and_then(|r| agreed(r.indices(source).map(|i| r.arms[i].0.has_exit))) == Some(true)
    }

    pub fn continuing_arm_cell(&self, function: FunctionStart, column: &str, row: usize, source: SourceArm) -> Option<Cell> {
        let r = self.bodies.get(&function.body)?.rows.get(&row)?;
        let mut cell = agreed(r.indices(source).map(|i| r.arms[i].0.continuing.as_ref()?.get(column)))??.clone();
        cell.from_row = row;
        Some(cell)
    }

    pub fn empty_sibling_cell(&self, function: FunctionStart, column: &str, row: usize, source: SourceArm, other: SourceArm) -> Option<Cell> {
        let r = self.bodies.get(&function.body)?.rows.get(&row)?;
        if source.branch != other.branch || !self.arm_has_exit(function.body, row, source)
            || agreed(r.indices(other).map(|i| r.arms[i].0.continuing.is_some())) != Some(true) { return None; }
        let mut cell = agreed(r.indices(source).map(|i| r.arms[i].1.get(column)))??.clone();
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
        if r.arms.iter().any(|(a, _)| a.parent.is_some() || a.edge.switch != first.0.edge.switch) {
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

    pub fn is_within(&self, function: FunctionStart, column: &str, ancestor: &str) -> bool {
        let Some(body) = self.bodies.get(&function.body) else { return false };
        let mut column = column;
        loop {
            if column == ancestor {
                return true;
            }
            let Some(parent) = body.parents.get(column) else { return false };
            column = parent;
        }
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

    /// Exact edge on which the loan was issued on this row, and the
    /// number of real successors of that switch, including empty ones.
    pub fn loan_arm(&self, function: FunctionStart, loan: usize) -> Option<(SwitchEdge, usize)> {
        let body = self.bodies.get(&function.body)?;
        let edge = body.loans.get(&loan)?.issued_on?;
        Some((edge, *body.successor_counts.get(&edge.switch)?))
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
                mixed_init: st.mixed_init.get(col).copied(),
                always_assigned: st.always_assigned.contains(col),
                reassigned: st.reassigned.get(col).copied(),
                unwind_before_write: st.unwind_before_write.contains(col),
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
            let exit = b.switches.binary_search_by_key(&a.edge, |e| e.edge).ok().map(|i| &b.switches[i]);
            let branch = exit.and_then(|e| e.branch);
            let source = exit.and_then(|e| Some(SourceArm { branch: branch?, index: e.arm? }));
            let info = ArmInfo { edge: a.edge, branch, source, runs: a.runs,
                parent: a.parent, has_exit: a.has_exit, continuing: a.continuing.as_ref().map(cells_of) };
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
    let mut successor_counts = BTreeMap::new();
    for exit in &b.switches {
        *successor_counts.entry(exit.edge.switch).or_default() += 1;
    }
    let parents = b.columns.iter().filter_map(|c| Some((c.name.clone(), c.parent.clone()?))).collect();
    let branch_entries = b.branch_entries.iter().map(|(&branch, state)| (branch, cells_of(state))).collect();
    let arm_boundaries = b.arm_boundaries.iter().map(|(&branch, arms)| {
        (branch, arms.iter().map(|(&index, bounds)| (index, ArmBoundaryCells {
            entry: bounds.entry.as_ref().map(cells_of),
            join: bounds.join.as_ref().map(cells_of),
        })).collect())
    }).collect();
    BodyTable { successor_counts, drop_holders: b.drop_holders.clone(), joins: b.joins.iter().map(|(s, j)| (*s, cells_of(&j.state))).collect(), branch_entries, arm_boundaries, first_line: b.first_line, last_line: b.last_line, function_body_start: b.function_body_start, rows, columns, parents, moved_places, array_lengths, boxes, uniform_option_refs: b.uniform_option_refs.clone(), loans, part_lenders, hidden_fields, dead_lines: b.dead_lines.clone() }
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
        let facts = facts.into_iter().map(|(arm, fact)| Ev { location: rustc_middle::mir::Location::START, display_location: rustc_middle::mir::Location::START, place_local: None, drop_route: DropRoute::Normal, occurrence: super::super::Occurrence::at(rustc_middle::mir::Location::START), end_places: vec![], caller_slot: None, transfer_paths: vec![], col: 1, arm, fact, order: (0, 0, 0) }).collect();
        Row { line, continues_to: None, after: state(cols, lent), before: None, block_end: false, jumps: false, facts, arms: Vec::new(), partial_drops: Vec::new() }
    }

    fn col(name: &str) -> ColumnInfo {
        ColumnInfo { local: rustc_middle::mir::Local::from_usize(1), name: name.into(), parent: None, root: name.split('.').next().unwrap().into(), kind: "var", copy: false, needs_drop: true, is_box: false, array_len: None, hidden_fields: false }
    }

    fn body(func: &str, first: usize, cols: &[&str], rows: Vec<Row>) -> BodyFacts {
        BodyFacts {
            residual_drops: Vec::new(),
            drop_holders: Default::default(),
            switches: vec![],
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
            slice_items: BTreeSet::new(),
            binding_loans: BTreeMap::new(),
            loans: vec![LoanInfo {
                projected: None,
                issue: None,
                lender_local: None,
                issued_on: None,
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
                direct_locals: BTreeSet::new(),
                direct_holders: vec!["r".into()],
                via_call: Vec::new(),
                implicit: false,
            }],
            rows,
            temps: Vec::new(),
            notes: Vec::new(),
            binding_sources: Vec::new(),
            joins: BTreeMap::new(),
            branch_entries: Default::default(),
            arm_boundaries: Default::default(),
            dead_lines: BTreeSet::new(),
            imaginary_edges: Vec::new(),
        }
    }

    fn main_body() -> BodyFacts {
        let mut one_line = row(8, &[("s", "maybe")], &[], vec![(Some(0), Fact::Move { path: "s".into(), to: "f()".into(), in_macro: None })]);
        one_line.after.cause.insert("s".into(), "moved");
        one_line.arms = vec![
            Arm { runs: true, branch: 0, arm: 0, parent: None, col_lo: 10, col_hi: 20, has_exit: true,
                edge: SwitchEdge { switch: rustc_middle::mir::Location::START, successor: rustc_middle::mir::BasicBlock::from_usize(1) }, continuing: None, after: state(&[("s", "moved")], &[]) },
            Arm { runs: true, branch: 0, arm: 1, parent: None, col_lo: 30, col_hi: 40, has_exit: true,
                edge: SwitchEdge { switch: rustc_middle::mir::Location::START, successor: rustc_middle::mir::BasicBlock::from_usize(2) }, continuing: None, after: state(&[("s", "own")], &[]) },
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
    fn a_nested_place_contains_its_fields_and_excludes_siblings() {
        let mut main = body("main", 1, &["p", "p.inner", "p.inner.a", "p.other"], Vec::new());
        main.columns[1].parent = Some("p".into());
        main.columns[2].parent = Some("p.inner".into());
        main.columns[3].parent = Some("p".into());
        let table = StateTable::from_facts(&CrateFacts { version: 2, bodies: vec![main], errors: Vec::new() });
        assert!(table.is_within(start(1), "p.inner.a", "p.inner"));
        assert!(table.is_within(start(1), "p.inner.a", "p"));
        assert!(!table.is_within(start(1), "p.other", "p.inner"));
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
