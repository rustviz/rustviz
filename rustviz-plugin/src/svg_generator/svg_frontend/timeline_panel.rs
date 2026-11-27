extern crate handlebars;
use crate::facts::table::{FunctionStart, Init, JoinCell};
use crate::svg_generator::data::{state_after_loop_exits, AltRuntime};
use crate::svg_generator::data::{AltEnd, FieldLenders, ApproxNote, place_elem_names, breaks_out_only, is_param_value_line, FIRST_ROW_Y, EarlyReturns, returned_closure, branch_column_rows, branch_is_collapsed, branch_state_converter, convert_back, init_join, InitBy, InitJoin, BranchData, BreakAlt, BranchExit, CopyOf, BranchType, ChainFailHow, ChainRef, Event, ExternalEvent, LineState, LoopHead, MaybeMovedAt, PlaceWrite, ResourceAccessPoint, ResourceAccessPoint_extract, ResourceTy, State, Struct, StructsInfo, Timeline, Visualizable, VisualizationData, LINE_SPACE, StaticLender, ViaCarrier, leaves_maybe_held};
use crate::svg_generator::hover_messages::{self, styled_name_html};
use crate::svg_generator::svg_frontend::line_styles::OwnerLine;
use crate::svg_generator::svg_frontend::rv_attrs::{self, Rv};
use handlebars::Handlebars;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use serde::Serialize;
use std::cmp::{self, max};


/// Centres `(x, y)` of the event dots drawn so far. Ordered, so the
/// SVG doesn't depend on hash iteration order.
type DotPositions = BTreeSet<(i64, i64)>;

/// One dot on a variable's column: every event (and call) that lands on
/// the same line of that column. Two events on one line used to get two
/// dots at the same point, and only the top one could be hovered
/// (#309). They now share one dot whose tooltip lists each event.
struct PendingDot {
    hash: u64,
    x: i64,
    y: i64,
    /// `output` key (`-1` outside struct groups) and whether the column
    /// is a struct member, the same routing `append_dot` used to do.
    owner_key: i64,
    is_member: bool,
    /// Tooltip lines in the order the events were recorded. The id is
    /// set for a lend to a function (`f(&s)`), so the call's own line
    /// ("f reads from s") can go right after it.
    lines: Vec<(Option<usize>, String)>,
    /// Some event here drops a resource: draw the drop triangle.
    drop: bool,
    /// An event dot is drawn here (not just a function call).
    event: bool,
    /// A function-call icon (`f`) is drawn here.
    call: bool,
    never_initialized: bool,
    /// The column's name, the row the dot is on and the events drawn on
    /// it, for its `data-rv-*` attributes (`rv_attrs`).
    var: String,
    row: usize,
    events: Vec<&'static str>,
    /// Approximation notes (`ApproxNote`), after the events' text.
    notes: Vec<DotNote>,
}

/// An approximation note on a dot, and whether it is a loop's.
#[derive(Clone, PartialEq)]
enum DotNote {
    Loop(String),
    Other(String),
    UnreachedScopeExit,
}

impl DotNote {
    fn text(&self) -> String {
        match self {
            DotNote::Loop(t) => t.clone(),
            DotNote::Other(t) => t.clone(),
            DotNote::UnreachedScopeExit => "This dot describes normal scope exit. A preceding closure call never returns, so this exit never runs. If the call panics, values still owned at the call are dropped if unwinding reaches their cleanup.".to_owned(),
        }
    }
}

/// Collects the dots of every column, merging events that share a
/// point, then writes them out once the arrows (which add the call
/// icons and need the dot positions) are done.
#[derive(Default)]
struct DotLayer {
    dots: Vec<PendingDot>,
    at: BTreeMap<(u64, i64, i64), usize>,
    positions: DotPositions,
    /// Where a call icon (`f`) will be drawn on a dot, known before the
    /// arrows are: an arrow ending on such a dot stops short of the icon,
    /// which is wider than the dot (#578).
    call_spots: DotPositions,
    /// Text of a split or join dot that isn't drawn because no line of
    /// its column reaches it (#702), per column: it goes on the next
    /// event dot drawn on that column, before that dot's own text.
    carry: HashMap<u64, Vec<(String, &'static str)>>,
    /// Where each arrow of a row goes so that none lies on another
    /// (`ArrowLanes`): filled by a first, recording pass of the arrows.
    lanes: ArrowLanes,
    /// The centres of the columns' stop bars (`render_branches`): an arrow
    /// drawn along one hides it (#1344).
    bars: Vec<(f64, f64)>,
    /// The columns' slanted legs into and out of a conditional's arms
    /// (`render_branches`): an arrow's head lying along one hides the end
    /// of the leg (#1476).
    legs: Vec<Piece>,
    /// Vertical portions of the branch timelines, also kept in `cols`.
    branch_cols: Vec<Piece>,
    /// The columns' vertical stretches (`render_timeline`,
    /// `render_branches`): an arrow's head coming steeply up (or down)
    /// into a dot lies along the column under (or over) it (#1488).
    cols: Vec<Piece>,
    /// The slanted ends of the references' brackets (`render_ref_line`):
    /// a head lengthened off a column (#1488) mustn't lie along one.
    brackets: Vec<Piece>,
}

/// What the columns drew that the arrows must keep clear of: the
/// centres of the stop bars, the slanted legs and the vertical
/// stretches (`DotLayer::bars`, `DotLayer::legs`, `DotLayer::cols`).
#[derive(Default)]
struct ColumnMarks {
    bars: Vec<(f64, f64)>,
    legs: Vec<Piece>,
    branch_cols: Vec<Piece>,
    cols: Vec<Piece>,
}

impl DotLayer {
    fn branch_stretches(&self) -> impl Iterator<Item = Piece> + '_ {
        self.legs.iter().chain(&self.branch_cols).copied()
    }

    fn trunk_restart_y(&self, row: usize, hash: u64, owner_key: i64, is_descendant: impl Fn(&str) -> bool) -> Option<i64> {
        self.dots.iter()
            .filter(|dot| dot.event && dot.row == row
                && (dot.hash == hash || (dot.owner_key == owner_key && is_descendant(&dot.var))))
            .min_by_key(|dot| (dot.hash != hash, !dot.events.contains(&rv_attrs::BRANCH_JOIN), dot.y))
            .map(|dot| dot.y)
    }

    fn entry(&mut self, hash: u64, x: i64, y: i64, row: usize, column: &TimelineColumnData) -> &mut PendingDot {
        let (owner_key, is_member) = if column.is_struct_group {
            (column.owner as i64, column.is_member)
        } else {
            (-1, false)
        };
        let dots = &mut self.dots;
        let i = *self.at.entry((hash, x, y)).or_insert_with(|| {
            dots.push(PendingDot {
                hash, x, y, owner_key, is_member,
                lines: Vec::new(), drop: false, event: false, call: false, never_initialized: false,
                var: column.name.clone(), row, events: Vec::new(), notes: Vec::new(),
            });
            dots.len() - 1
        });
        &mut self.dots[i]
    }

    /// An event dot (`drop`: one with the drop triangle) for the event
    /// `ev` (`rv_attrs::event_name`) on row `row`.
    fn add_event(&mut self, hash: u64, x: i64, y: i64, column: &TimelineColumnData,
                 title: String, drop: bool, lend_id: Option<usize>, ev: &'static str, row: usize) {
        self.positions.insert((x, y));
        let carried = self.carry.remove(&hash).unwrap_or_default();
        let dot = self.entry(hash, x, y, row, column);
        dot.event = true;
        dot.drop |= drop;
        // Carried text happened before anything recorded on this dot.
        dot.events.splice(0..0, carried.iter().map(|(_, e)| *e));
        dot.lines.splice(0..0, carried.into_iter().map(|(t, _)| (None, t)));
        dot.lines.push((lend_id, title));
        dot.events.push(ev);
    }

    /// Rewrites the text the event dot of column `hash` at (`x`, `y`)
    /// got last.
    fn edit_last_line(&mut self, hash: u64, x: i64, y: i64, f: impl FnOnce(&str) -> String) {
        if let Some(&i) = self.at.get(&(hash, x, y)) {
            if let Some((_, text)) = self.dots[i].lines.last_mut() {
                *text = f(text);
            }
        }
    }

    /// Text waiting for the next event dot of column `hash` (see
    /// `carry`), taken off it.
    fn take_carry(&mut self, hash: u64) -> Vec<(String, &'static str)> {
        self.carry.remove(&hash).unwrap_or_default()
    }

    /// Makes `texts` the text waiting for the next event dot of column
    /// `hash`, replacing what was waiting.
    fn set_carry(&mut self, hash: u64, texts: Vec<(String, &'static str)>) {
        if texts.is_empty() {
            self.carry.remove(&hash);
        } else {
            self.carry.insert(hash, texts);
        }
    }

    /// The `f` icon of a call that borrows the variable (`f(&s)`), from
    /// the external event `id`. Its line goes right after the lend that
    /// event recorded, if that is drawn here: borrowed by f, f reads
    /// from s, no longer borrowed.
    fn add_call(&mut self, hash: u64, x: i64, y: i64, column: &TimelineColumnData,
                title: String, id: usize, ev: &'static str, row: usize) {
        let dot = self.entry(hash, x, y, row, column);
        dot.call = true;
        dot.events.push(ev);
        let at = dot.lines.iter()
            .position(|(lend, _)| *lend == Some(id))
            .map_or(dot.lines.len(), |i| i + 1);
        dot.lines.insert(at, (None, title));
    }

    /// Puts each approximation note on the event dots it belongs to
    /// (see `ApproxNote`). `row_lines` maps a dot's row to its source
    /// line.
    fn add_approx_notes(&mut self, notes: &[ApproxNote], row_lines: &[Option<usize>]) {
        // Event dots by column, with their source lines, in dot order.
        let mut by_col: BTreeMap<u64, Vec<(usize, usize)>> = BTreeMap::new();
        for (i, dot) in self.dots.iter().enumerate() {
            if !dot.event {
                continue;
            }
            if let Some(line) = rv_attrs::line_of(dot.row, row_lines) {
                by_col.entry(dot.hash).or_default().push((line, i));
            }
        }
        // The dots of `col` on the first of its lines in `lo..=hi`.
        let first_in = |col: &[(usize, usize)], lo: usize, hi: usize| -> Vec<usize> {
            let Some(first) = col.iter().map(|(l, _)| *l).filter(|l| (lo..=hi).contains(l)).min() else { return Vec::new() };
            col.iter().filter(|(l, _)| *l == first).map(|(_, i)| *i).collect()
        };
        let add = |dots: &mut Vec<PendingDot>, at: Vec<usize>, note: DotNote| {
            for i in at {
                if !dots[i].notes.contains(&note) {
                    dots[i].notes.push(note.clone());
                }
            }
        };
        // Loops, one note each, on a dot outside the loops nested in it.
        // A loop whose dots are all in nested loops gets none: its note
        // would sit inside a nested loop, where "this loop" reads as the
        // nested one, whose own note already says it.
        for note in notes {
            let ApproxNote::Loop { head, pass: (lo, hi), head_cols, locals, nested, text } = note else { continue };
            let mut best: Option<(usize, i64, usize)> = None;
            for (hash, col) in &by_col {
                if !locals.contains(hash) && !col.iter().any(|(l, _)| l < head) {
                    continue;
                }
                for &(line, i) in col {
                    let in_pass = (*lo..=*hi).contains(&line) || (line == *head && head_cols.contains(hash));
                    if !in_pass || nested.iter().any(|(h, e)| (*h..=*e).contains(&line)) {
                        continue;
                    }
                    let key = (self.dots[i].row, self.dots[i].x, i);
                    if best.is_none_or(|b| key < b) {
                        best = Some(key);
                    }
                }
            }
            if let Some((.., i)) = best {
                add(&mut self.dots, vec![i], DotNote::Loop(text.clone()));
            }
        }
        for note in notes {
            if let ApproxNote::UnreachedValue { hash } = note {
                for dot in &mut self.dots {
                    if dot.hash == *hash { dot.never_initialized = true; }
                }
            }
            if let ApproxNote::UnreachedTransfer { scope_ends, .. } = note {
                for (hash, end) in scope_ends {
                    if let Some(col) = by_col.get(hash) {
                        add(&mut self.dots, first_in(col, *end, *end), DotNote::UnreachedScopeExit);
                    }
                }
            }
            if let ApproxNote::At { hash, lines: (lo, hi), text } = note {
                if let Some(col) = by_col.get(hash) {
                    add(&mut self.dots, first_in(col, *lo, *hi), DotNote::Other(text.clone()));
                }
            }
        }
    }

    fn flush(self, output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>, registry: &Handlebars) {
        let mut rendered: BTreeMap<(i64, bool), String> = BTreeMap::new();
        for dot in self.dots {
            let svg = render_pending_dot(&dot, registry);
            rendered.entry((dot.owner_key, dot.is_member)).or_default().push_str(&svg);
        }
        // Dots go before anything the arrow pass already put in the
        // layer (function logos), as they did when drawn straight away.
        for ((owner_key, is_member), svg) in rendered {
            let entry = ensure_owner_entry(output, owner_key);
            let panel = if is_member { &mut entry.1 } else { &mut entry.0 };
            panel.dots.insert_str(0, &svg);
        }
    }
}

/// Tooltip for a dot: the one event's text, or one bulleted line per
/// event, in order. Empty texts (the structural merge dot of an
/// unchanged variable) are left out, and so is a line that repeats the
/// one before it (two borrows of `s` ending on one line both say "s's
/// resource is no longer immutably borrowed").
fn merged_dot_title(lines: &[(Option<usize>, String)]) -> String {
    let mut texts: Vec<&str> = lines.iter()
        .map(|(_, t)| t.as_str())
        .filter(|t| !t.is_empty())
        .collect();
    texts.dedup();
    // Two `?` on a line: the drop at the first `?` is not about the
    // second `?`'s move of the same variable (#668).
    let moves_into_try = texts.iter().any(|t| t.contains("is moved into the `?`."));
    let rewritten: Vec<Option<String>> = texts.iter()
        .map(|t| if moves_into_try { hover_messages::try_drop_before_own_try(t) } else { None })
        .collect();
    let texts_owned: Vec<String> = texts.iter().zip(&rewritten)
        .map(|(t, r)| r.clone().unwrap_or_else(|| (*t).to_owned()))
        .collect();
    let mut texts: Vec<&str> = texts_owned.iter().map(String::as_str).collect();
    // `s = String::from(..)` says "acquires ownership of a new resource;
    // its previous resource is dropped": the plain "acquires ownership of
    // a resource" next to it adds nothing (#506, #519).
    let all = texts.clone();
    texts.retain(|t| !all.iter().any(|r| hover_messages::reassign_subsumes_acquire(r, t)));
    // The same for a value one of several `break`s carried (#586).
    let mut texts: Vec<String> = texts.iter().map(|t| t.to_string()).collect();
    if let Some((i, j, merged)) = (0..texts.len()).find_map(|i| (0..texts.len())
        .find_map(|j| hover_messages::reassign_with_break_alt(&texts[i], &texts[j]).map(|m| (i, j, m)))) {
        texts[i] = merged;
        texts.remove(j);
    }
    // An end that says the lender "is still borrowed by x" and the lend
    // to x on the same dot say one thing (#1450).
    while let Some((i, j, merged)) = (0..texts.len()).find_map(|i| (0..texts.len())
        .filter(|&j| j != i)
        .find_map(|j| hover_messages::still_borrowed_with_lend(&texts[i], &texts[j]).map(|m| (i, j, m)))) {
        texts[i] = merged;
        texts.remove(j);
    }
    // `s = f()?;` (#1151): the assignment runs only if the `?` succeeds.
    if let Some((i, j, merged)) = (0..texts.len()).find_map(|i| (0..texts.len())
        .filter(|&j| j != i)
        .find_map(|j| hover_messages::try_drop_then_reassign(&texts[i], &texts[j]).map(|m| (i, j, m)))) {
        texts[i] = merged;
        texts.remove(j);
    } else if texts.len() > 1 {
        for t in texts.iter_mut() {
            if let Some(r) = hover_messages::try_drop_with_other_events(t) {
                *t = r;
            }
        }
    }
    // The note on a write drawn inside a borrow that is over on its
    // path (#796) goes last: after the write's own text as a second
    // sentence, or as the last bullet.
    if let Some(i) = texts.iter().position(|t| t.starts_with(hover_messages::DEAD_LOAN_NOTE_START)) {
        let note = texts.remove(i);
        match texts.as_mut_slice() {
            [only] => *only = format!("{}. {}", only, note),
            _ => texts.push(note),
        }
    }
    if texts.len() <= 1 {
        return texts.first().cloned().unwrap_or_default();
    }
    texts.iter()
        .map(|t| format!("• {}", t))
        .collect::<Vec<_>>()
        .join("&lt;br&gt;")
}

/// A dot's tooltip with its approximation notes, each on a line of its
/// own after the events' text (which stays as it was).
fn with_approx_notes(title: String, notes: &[DotNote]) -> String {
    let qualifies = |note: &DotNote| matches!(note, DotNote::UnreachedScopeExit);
    let mut lines: Vec<_> = notes.iter().filter(|n| qualifies(n)).map(DotNote::text).collect();
    if !title.is_empty() {
        lines.push(title);
    }
    lines.extend(notes.iter().filter(|n| !qualifies(n)).map(DotNote::text));
    lines.join("&lt;br&gt;")
}

/// Template data plus the `data-rv-*` attributes (`{{rv}}`).
#[derive(Serialize)]
struct WithRv<'a, T: Serialize> {
    #[serde(flatten)]
    data: &'a T,
    rv: String,
}

fn render_pending_dot(dot: &PendingDot, registry: &Handlebars) -> String {
    let title = if dot.never_initialized {
        let name = styled_name_html(&dot.var);
        if dot.call {
            format!("The call shown here never runs because {name} is never initialized")
        } else {
            format!("A preceding closure call never returns, so {name} is never initialized and has no value to drop")
        }
    } else {
        with_approx_notes(merged_dot_title(&dot.lines), &dot.notes)
    };
    let (cx, cy) = (dot.x, dot.y);
    let kind = if dot.event { rv_attrs::DOT } else { rv_attrs::CALL };
    let rv = Rv::new(kind).events(&dot.events).var(&dot.var).row(dot.row).drop(dot.drop).attrs();
    if !dot.event {
        let data = FunctionDotData { hash: dot.hash, x: cx, y: cy, title };
        return registry.render("function_dot_template", &WithRv { data: &data, rv }).unwrap();
    }
    if !dot.call {
        if dot.drop {
            let data = DropDotData {
                hash: dot.hash, dot_x: cx, dot_y: cy, title,
                p1x: cx - 3, p1y: cy - 1,
                p2x: cx + 3, p2y: cy - 1,
                p3x: cx,     p3y: cy + 3,
            };
            return registry.render("drop_dot_template", &WithRv { data: &data, rv }).unwrap();
        }
        let data = EventDotData { hash: dot.hash, dot_x: cx, dot_y: cy, title };
        return registry.render("dot_template", &WithRv { data: &data, rv }).unwrap();
    }
    // An event dot with a call icon on it: one hover target for both.
    // The `f` glyph covers the dot, so a drop on this dot can't use the
    // usual white triangle inside it (it would be hidden). Draw the drop
    // mark just below the dot instead, in the column's colour: a
    // downward triangle where the column ends.
    let triangle = if dot.drop {
        format!(
            "            <polygon class=\"drop-mark\" data-hash=\"{}\" points=\"{},{} {},{} {},{}\"/>\n",
            dot.hash, cx - 4, cy + 12, cx + 4, cy + 12, cx, cy + 17,
        )
    } else {
        String::new()
    };
    format!(
        "        <g class=\"tooltip-trigger\" data-tooltip-text=\"{title}\"{rv}>\n\
         \x20           <circle cx=\"{cx}\" cy=\"{cy}\" r=\"5\" data-hash=\"{hash}\"/>\n\
         {triangle}\
         \x20           <use xlink:href=\"#functionDot\" data-hash=\"{hash}\" x=\"{cx}\" y=\"{cy}\"/>\n\
         \x20       </g>\n",
        title = title, cx = cx, cy = cy, hash = dot.hash, triangle = triangle, rv = rv,
    )
}

static BRANCH_WEIGHT: i64 = 25;
#[derive(Debug, Clone)]
pub struct TimelineColumnData {
    pub name: String,
    pub x_val: i64,
    pub title: String,
    pub is_ref: bool,
    pub is_struct_group: bool,
    pub is_member: bool,
    pub owner: u64
}

#[derive(Serialize)]
struct TimelinePanelData {
    labels: String,
    dots: String,
    timelines: String,
    ref_line: String,
    arrows: String
}

impl TimelinePanelData {
    fn empty() -> Self {
        TimelinePanelData { labels: String::new(), dots: String::new(), timelines: String::new(),
            ref_line: String::new(), arrows: String::new() }
    }
}

#[derive(Serialize)]
struct ResourceAccessPointLabelData {
    x_val: i64,
    y_val: i64,
    hash: String,
    name: String,
    title: String,
    rv: String,
}

#[derive(Serialize)]
struct EventDotData {
    hash: u64,
    dot_x: i64,
    dot_y: i64,
    title: String,
}

// Variant of EventDotData carrying the inner-triangle vertices for the
// drop-dot visual (rendered when an owner goes out of scope while still
// holding its resource). Computed at render time from the dot's center
// so the triangle fits cleanly inside the 5px-radius circle.
#[derive(Serialize)]
struct DropDotData {
    hash: u64,
    dot_x: i64,
    dot_y: i64,
    title: String,
    p1x: i64,
    p1y: i64,
    p2x: i64,
    p2y: i64,
    p3x: i64,
    p3y: i64,
}

#[derive(Serialize)]
struct FunctionDotData {
    hash: u64,
    x: i64,
    y: i64,
    title: String
}

#[derive(Serialize)]
struct ArrowData {
    coordinates: Vec<(f64, f64)>,
    coordinates_hbs: String,
    // Pre-rendered "x1,y1 x2,y2 x3,y3" for the arrow head triangle,
    // drawn inline as a sibling polygon of the polyline so that the
    // head shares the same hover region as the shaft. Replaces the
    // marker-end approach (markers live in a separate <defs> scope
    // and don't inherit hover events from the referencing element).
    head_points: String,
    title: String,
    rv: String,
    /// The arrow's (event id, line) and the names of its two ends, as
    /// `data-rv-var` / `data-rv-to` give them (`Drawn`).
    #[serde(skip)]
    key: (usize, usize),
    #[serde(skip)]
    names: (String, String),
    #[serde(skip)]
    columns: (Option<u64>, Option<u64>),
    /// An arrow from a column to itself (a literal's or static's borrow
    /// ending, drawn beside the column) drawn on the dot's right instead
    /// of its left (`ArrowLanes::self_right`).
    #[serde(skip)]
    self_right: bool,
}

#[derive(Serialize)]
struct FunctionLogoData {
    hash: u64,
    x: i64,
    y: i64,
    title: String,
    rv: String,
}

#[derive(Serialize)]
struct BoxData {
    name: u64,
    hash: u64,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    title: String
}


#[derive(Serialize, Clone)]
struct VerticalLineData {
    line_class: String,
    hash: u64,
    x1: f64,
    x2: f64,
    y1: i64,
    y2: i64,
    title: String,
    opacity: f64,
    /// SVG `stroke-dasharray`. "none" for a solid line; "4 3" or
    /// similar to dash an inactive (Gray-state) branch column so
    /// readers can tell which lines a given branch is "passive" on.
    dasharray: String,
    rv: String,
}

#[derive(Serialize, Clone, Debug)]
struct HollowLineData {
    line_class: String,
    hash: u64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x3: f64,
    y3: f64,
    x4: f64,
    y4: f64,
    title: String,
    opacity: f64,
    /// Mirrors VerticalLineData::dasharray; propagated through
    /// `compute_hollow_line_data` so a Gray-state hollow segment
    /// renders dashed.
    dasharray: String,
    rv: String,
}

#[derive(Serialize, Clone)]
struct RefLineData {
    line_class: String,
    hash: u64,
    x1: i64,
    x2: i64,
    y1: i64,
    y2: i64,
    dx: i64,
    dy: i64,
    v: i64,
    title: String,
    rv: String,
}

#[derive(Serialize)]
struct OutputStringData {
    struct_name: String,
    struct_instance: String,
    struct_members: String
}

/// Look up the (parent_panel, members_panel) entry for a struct
/// owner key, lazy-inserting an empty pair if missing.
///
/// Why: `output[parent_hash]` is created on-demand by `append_line`
/// when a Timeline's `states` produces at least one drawable
/// segment. A parent struct whose history is "exists but never had
/// a real ownership / borrow event" produces no such segment, so
/// `append_line` skips it and the entry never appears. Subsequent
/// passes (labels, dots, arrows, refs) still iterate the column
/// layout and would unwrap None for that owner key — so route every
/// per-owner write through here instead of `.get_mut(...).unwrap()`.
fn ensure_owner_entry<'a>(
    output: &'a mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    owner_key: i64,
) -> &'a mut (TimelinePanelData, TimelinePanelData) {
    output.entry(owner_key).or_insert_with(|| {
        let blank = || TimelinePanelData {
            labels: String::new(),
            dots: String::new(),
            timelines: String::new(),
            ref_line: String::new(),
            arrows: String::new(),
        };
        (blank(), blank())
    })
}

pub fn render_timeline_panel(visualization_data : & mut VisualizationData) -> (String, i32, i32) {
    /* Template creation */
    let mut registry = Handlebars::new();
    prepare_registry(&mut registry);

    let mut structs_info = StructsInfo { structs: Vec::new() };

    // hash -> TimelineColumnData
    let (resource_owners_layout, width) = compute_column_layout(visualization_data, &mut structs_info);

    let mut output : BTreeMap<i64, (TimelinePanelData, TimelinePanelData)> = BTreeMap::new();
    output.insert(-1, (TimelinePanelData{ labels: String::new(), dots: String::new(), timelines: String::new(), 
        ref_line: String::new(), arrows: String::new() }, TimelinePanelData{ labels: String::new(), dots: String::new(), 
            timelines: String::new(), ref_line: String::new(), arrows: String::new() })); 
    // Note: key {-1} = non-struct timelines
    
    // Place event dots before the branch bars and arrows, which need
    // their positions.
    let mut dots = render_dots_string(visualization_data, &resource_owners_layout); // dot events
    let mut marks = ColumnMarks::default();
    render_timelines(&mut output, visualization_data, &resource_owners_layout, &registry, &dots, &mut marks); // vertical bars
    render_labels_string(&mut output, &resource_owners_layout, &visualization_data.fn_start_lines, &visualization_data.raised_headers, &registry); // headers
    dots.bars = marks.bars;
    dots.legs = marks.legs;
    dots.branch_cols = marks.branch_cols;
    dots.cols = marks.cols;
    render_ref_line(&mut output, visualization_data, &resource_owners_layout, &registry, &mut dots.brackets); // reference lines
    render_arrows_string_external_events_version(&mut output, visualization_data, &resource_owners_layout, &mut dots, &registry); // arrows
    let width = render_argument_results(&mut output, visualization_data, &mut dots, &registry, width);
    // A join or scope-end mark can sit below the last code baseline.
    // Include the whole glyph, including the drop marker on call dots.
    let dot_bottom = dots.dots.iter().map(|d| d.y + if d.call && d.drop { 17 } else { 5 }).max().unwrap_or(0);
    let segment_bottom = dots.legs.iter().chain(&dots.cols).chain(&dots.brackets)
        .map(|&(a, b)| a.1.max(b.1) + BRANCH_HALF_W)
        .chain(dots.bars.iter().map(|&(_, y)| y + STOP_BAR_HOVER_HALF_H))
        .fold(0.0_f64, f64::max);
    let min_height = dot_bottom.max(segment_bottom.max(dots.lanes.painted_bottom).ceil() as i64) as i32;
    // Written last: a call icon (`f`) added by the arrow pass shares
    // the dot of the borrow it makes.
    dots.add_approx_notes(&visualization_data.approx_notes, &visualization_data.row_lines);
    dots.flush(&mut output, &registry);
    render_struct_box(&mut output, &structs_info, &visualization_data.fn_start_lines, &visualization_data.raised_headers, &registry); // struct box

    let mut output_string : String = String::new();
    for (hash, (timelinepanel, member_timelinepanel)) in output{
        let struct_name;
        if hash == -1 {
            struct_name = String::from("non-struct");
        } else {
            struct_name = match visualization_data.get_name_from_hash(&(hash as u64)) {
                Some(_name) => _name,
                None => panic!("internal state: no variable has hash {}", hash),
            };
        }
        let timelinepanel_string = registry.render("timeline_panel_template", &timelinepanel).unwrap();
        let member_timelinepanel_string = registry.render("timeline_panel_template", &member_timelinepanel).unwrap();
        let output_data = OutputStringData{
            struct_name: struct_name,
            struct_instance: timelinepanel_string,
            struct_members: member_timelinepanel_string,
        };
        output_string.push_str(&registry.render("struct_template", &output_data).unwrap());
    }

    (output_string, width, min_height)

}

fn prepare_registry(registry: &mut Handlebars) {
    // We want to preserve the inputs `as is`, and want to make no changes based on html escape.
    registry.register_escape_fn(handlebars::no_escape);

    // Arrows come before events so dots and function icons paint (and
    // hit-test) above the arrow shafts that start or end at their centres.
    let timeline_panel_template =
        "    <g id=\"labels\">\n{{ labels }}    </g>\n\n    \
        <g id=\"timelines\">\n{{ timelines }}    </g>\n\n    \
        <g id=\"ref_line\">\n{{ ref_line }}    </g>\n\n    \
        <g id=\"arrows\">\n{{ arrows }}    </g>\n\n    \
        <g id=\"events\">\n{{ dots }}    </g>";

    let struct_template  = 
        "    <g id=\"{{struct_name}}\">\n\
             \t<g class=\"struct_instance\">\n{{ struct_instance }}</g>\n\
             \t<g class=\"struct_members\">\n{{ struct_members }}</g>\n\
             \t</g>\n    ";
    
    let label_template =
        "        <text x=\"{{x_val}}\" y=\"{{y_val}}\" style=\"text-anchor:middle\" data-hash=\"{{hash}}\" class=\"label tooltip-trigger\" data-tooltip-text=\"{{title}}\"{{rv}}>{{name}}</text>\n";
    let dot_template =
        "        <circle cx=\"{{dot_x}}\" cy=\"{{dot_y}}\" r=\"5\" data-hash=\"{{hash}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"{{rv}}/>\n";
    // Used for Owner end-of-scope dots when the owner is still
    // holding the resource at that point — i.e. the resource is
    // dropped here. The visual is a normal dot with a small white
    // down-pointing triangle inside, so users can see that a drop
    // happened without having to hover for the tooltip. The
    // surrounding <g> carries the same hash + tooltip metadata as a
    // regular dot so existing UI behavior (highlighting on hash
    // hover, tooltip text) is unchanged.
    let drop_dot_template =
        "        <g data-hash=\"{{hash}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"{{rv}}>\n            <circle cx=\"{{dot_x}}\" cy=\"{{dot_y}}\" r=\"5\"/>\n            <polygon points=\"{{p1x}},{{p1y}} {{p2x}},{{p2y}} {{p3x}},{{p3y}}\" style=\"fill: var(--rv-drop-notch); stroke: none; pointer-events: none;\"/>\n        </g>\n";
    let function_dot_template =    
        "        <use xlink:href=\"#functionDot\" data-hash=\"{{hash}}\" x=\"{{x}}\" y=\"{{y}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"{{rv}}/>\n";
    let function_logo_template =
        "        <text x=\"{{x}}\" y=\"{{y}}\" data-hash=\"{{hash}}\" class=\"functionLogo tooltip-trigger fn-trigger\" data-tooltip-text=\"{{title}}\"{{rv}}>f</text>\n";
    // Arrow = shaft (polyline) + head (polygon) wrapped in a single
    // <g class="tooltip-trigger"> so the pair is treated as one
    // hover target: hovering either child triggers `:hover` on the
    // group, which runs the glow filter over the whole arrow and
    // bubbles the mousemove event up to the listener on the group.
    // Previously each child carried its own tooltip-trigger and
    // glowed independently, which made the shaft and head feel like
    // two separate things.
    //
    // Replaces the older marker-end="url(#arrowHead)" approach —
    // markers live in <defs> scope and don't receive hover events
    // from the polylines that reference them.
    let arrow_template =
        "        <g class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"{{rv}}>\n            <polyline class=\"arrow-shaft\" stroke-width=\"5px\" points=\"{{coordinates_hbs}}\" style=\"fill: none;\"/>\n            <polygon points=\"{{head_points}}\" class=\"arrow-head\"/>\n        </g>\n";
    let vertical_line_template =
        "        <line data-hash=\"{{hash}}\" class=\"{{line_class}} tooltip-trigger\" x1=\"{{x1}}\" x2=\"{{x2}}\" y1=\"{{y1}}\" y2=\"{{y2}}\" data-tooltip-text=\"{{title}}\"{{rv}} style=\"opacity: {{opacity}}; stroke-dasharray: {{dasharray}};\"/>\n";
    let hollow_line_template =
        "        <path data-hash=\"{{hash}}\" class=\"hollow tooltip-trigger\" style=\"fill:transparent;\" d=\"M {{x1}},{{y1}} V {{y2}} h 3.5 V {{y1}} h -3.5\" data-tooltip-text=\"{{title}}\"/>\n";
    // Hollow column rendering: two parallel `<line>` elements,
    // one per side of the strip. No closing horizontal at top or
    // bottom — those were the source of the "stray dash tick" at
    // segment boundaries (closed/open paths drew a dashed
    // horizontal connector that appeared as an extra mark where a
    // dashed segment met a solid one). Two independent lines have
    // `stroke-linecap: butt` by default, so adjacent state
    // segments — dashed leading + solid body + dashed trailing —
    // meet at exact y-coordinates with nothing between them, and
    // the column reads as one continuous strip with two textures.
    // Hollow column = two thin parallel lines plus a transparent
    // fill polygon over the gap between them. The polygon catches
    // mouse events in the strip's interior so hovering anywhere
    // inside the column (not just precisely on a line) fires the
    // wrapping `<g>`'s tooltip + glow. Without it, the cursor had
    // to land on one of the 1.5px line strokes to highlight.
    // (Same hover-capture pattern as branch strips in render_branch_run.)
    let new_hollow_line_template = "<g class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"{{rv}}>\n            <line data-hash=\"{{hash}}\" class=\"hollow\" x1=\"{{x1}}\" y1=\"{{y1}}\" x2=\"{{x2}}\" y2=\"{{y2}}\" style=\"stroke-opacity: {{opacity}}; stroke-dasharray: {{dasharray}};\"/>\n            <line data-hash=\"{{hash}}\" class=\"hollow\" x1=\"{{x4}}\" y1=\"{{y4}}\" x2=\"{{x3}}\" y2=\"{{y3}}\" style=\"stroke-opacity: {{opacity}}; stroke-dasharray: {{dasharray}};\"/>\n            <polygon data-hash=\"{{hash}}\" points=\"{{x1}},{{y1}} {{x2}},{{y2}} {{x3}},{{y3}} {{x4}},{{y4}}\" style=\"fill:transparent; stroke:none; pointer-events:fill;\"/>\n        </g>\n";
    // Borrow-region trapezoids — visible as outlines (fill is
    // transparent), but `pointer-events:all` makes the fill area
    // hit-test too so hovering anywhere inside the trapezoid fires
    // the tooltip + glow, not just on the 2px outline strokes.
    let solid_ref_line_template =
        "        <path data-hash=\"{{hash}}\" class=\"mutref {{line_class}} tooltip-trigger\" style=\"fill:transparent; stroke-width: 2px !important; pointer-events: all;\" d=\"M {{x1}} {{y1}} l {{dx}} {{dy}} v {{v}} l -{{dx}} {{dy}}\" data-tooltip-text=\"{{title}}\"{{rv}}/>\n";
    let hollow_ref_line_template =
        "        <path data-hash=\"{{hash}}\" class=\"staticref tooltip-trigger\" style=\"fill: transparent; pointer-events: all;\" stroke-width=\"2px\" stroke-dasharray=\"3\" d=\"M {{x1}} {{y1}} l {{dx}} {{dy}} v {{v}} l -{{dx}} {{dy}}\" data-tooltip-text=\"{{title}}\"{{rv}}/>\n";
    let box_template =
        "        <rect id=\"{{name}}\" x=\"{{x}}\" y=\"{{y}}\" rx=\"20\" ry=\"20\" width=\"{{w}}\" height=\"{{h}}\" style=\"fill:var(--rv-struct-fill);stroke:var(--rv-struct-stroke);stroke-width:3;opacity:0.1\" pointer-events=\"none\" />\n";

    assert!(
        registry.register_template_string("new_hollow_line_template", new_hollow_line_template).is_ok()
    );
    assert!(
        registry.register_template_string("struct_template", struct_template).is_ok()
    );
    assert!(
        registry.register_template_string("timeline_panel_template", timeline_panel_template).is_ok()
    );
    assert!(
        registry.register_template_string("label_template", label_template).is_ok()
    );
    assert!(
        registry.register_template_string("dot_template", dot_template).is_ok()
    );
    assert!(
        registry.register_template_string("drop_dot_template", drop_dot_template).is_ok()
    );
    assert!(
        registry.register_template_string("arrow_template", arrow_template).is_ok()
    );
    assert!(
        registry.register_template_string("vertical_line_template", vertical_line_template).is_ok()
    );
    assert!(
        registry.register_template_string("function_dot_template", function_dot_template).is_ok()
    );
    assert!(
        registry.register_template_string("function_logo_template", function_logo_template).is_ok()
    );
    assert!(
        registry.register_template_string("hollow_line_template", hollow_line_template).is_ok()
    );
    assert!(
        registry.register_template_string("solid_ref_line_template", solid_ref_line_template).is_ok()
    );
    assert!(
        registry.register_template_string("hollow_ref_line_template", hollow_ref_line_template).is_ok()
    );
    assert!(
        registry.register_template_string("box_template", box_template).is_ok()
    );
}

 
// computes a width coefficient for a resource, considering branches
fn compute_width(events: & mut Vec<(usize, Event)>) -> usize {
    let mut max_width = 0;
    for (_, ev) in events {
        match ev {
            Event::Branch { branch_history, ..} => {
                let mut b_width = 0;
                // DFS to calculate width of each branch
                for branch in branch_history.iter_mut() {
                    let branch_w = compute_width(& mut branch.e_data);
                    branch.width = branch_w; // store branch width for later DOES NOT INCLUDE PADDING BETWEEN BRANCHES AT SAME LEVEL
                    b_width += branch_w;
                }
                let padding = (branch_history.len() - 1) * 2;
                b_width += padding;
                max_width = max(b_width, max_width);
            }
            _ => {}
        }
    }
    max_width
}

// Centre x of each arm of a branch, in BRANCH_WEIGHT units relative to
// the parent column. Shared by `compute_extent` (column reservation) and
// `update_timeline_data` (actual placement) so the two can't drift.
//
// Even N: arms `[0, N/2)` fan out to the left of the parent and
// `[N/2, N)` to the right, each pair of neighbours 2 units apart plus
// the outer arm's width (2 arms: -1, +1; 4 arms: -3, -1, +1, +3).
// Odd N >= 3: the middle arm sits on the parent's centreline and the
// others fan out from its left/right extent the same way (3 arms: -2, 0,
// +2), so the layout is symmetric (#119). A lone arm (`if` without
// `else`) stays offset to the right (+1) so it doesn't sit on top of the
// parent. Requires `branch.width` to already be populated by
// `compute_width`.
fn branch_centers(branch_history: &[BranchData]) -> Vec<i64> {
    let n = branch_history.len();
    let halfway = n / 2;
    let mut centers: Vec<i64> = vec![0; n];
    // Where each side starts, which arm is first on the right, and the
    // extra unit of clearance the arms next to the parent get when no
    // middle arm sits between them.
    let (mut left_run, mut right_run, right_start, inner_pad) = if n % 2 == 1 && n > 1 {
        let (mid_left, mid_right) = compute_extent(&branch_history[halfway].e_data);
        (-mid_left - 2, mid_right + 2, halfway + 1, 0)
    } else {
        (0, 0, halfway, 1)
    };
    // `width` reserves an arm's room on the side towards the parent, but
    // a nested branch's arms fan out on both sides of the arm, and one
    // that cascades towards the parent (an `else if` chain in a left
    // arm) can reach past that room onto the other side's legs (#1539).
    // So an arm also keeps its whole reach (`compute_extent`) clear of
    // what is already placed nearer the parent: of the parent's centre
    // line for the two arms next to it, of a middle arm's reach, and of
    // the neighbouring arm's reach, by at least one unit. Where `width`
    // already leaves that room (every layout without such a cascade)
    // nothing moves.
    let reach = |i: usize| compute_extent(&branch_history[i].e_data);
    // How far left of the parent's centre the arms placed so far reach
    // (left side), and how far right (right side).
    let (mut left_edge, mut right_edge) = if n % 2 == 1 && n > 1 {
        let (mid_left, mid_right) = compute_extent(&branch_history[halfway].e_data);
        (-mid_left, mid_right)
    } else {
        (0, 0)
    };
    for i in (0..halfway).rev() {
        let padding = if i + 1 == halfway { inner_pad } else { 0 };
        left_run -= branch_history[i].width as i64 + padding;
        let (arm_left, arm_right) = reach(i);
        left_run = left_run.min(left_edge - 1 - arm_right);
        centers[i] = left_run;
        left_edge = left_run - arm_left;
        left_run -= 2;
    }
    for i in right_start..n {
        let padding = if i == right_start { inner_pad } else { 0 };
        right_run += branch_history[i].width as i64 + padding;
        let (arm_left, arm_right) = reach(i);
        right_run = right_run.max(right_edge + 1 + arm_left);
        centers[i] = right_run;
        right_edge = right_run + arm_right;
        right_run += 2;
    }
    centers
}

// Returns (max_left, max_right) extent in BRANCH_WEIGHT units that any
// nested branch leaf reaches from the timeline's center x. Uses the
// placement in `branch_centers`, then walks each branch's recursive
// extent and maxes both sides separately.
//
// `compute_width` returns a single sum-of-children value that's adequate
// when the layout is symmetric, but nesting (e.g. an inner if/else inside
// the outer if's if-arm) cascades placement leftward without contributing
// proportionally to the rightward sum, leaving the column-layout reservation
// short on one side and the leftmost leaf intruding into the code panel.
// Tracking left/right separately fixes that. Requires `branch.width` to
// already be populated by `compute_width`.
fn compute_extent(events: &Vec<(usize, Event)>) -> (i64, i64) {
    let mut max_left: i64 = 0;
    let mut max_right: i64 = 0;
    for (_, ev) in events {
        if let Event::Branch { branch_history, .. } = ev {
            let centers = branch_centers(branch_history);
            for (i, center) in centers.iter().enumerate() {
                let (cl, cr) = compute_extent(&branch_history[i].e_data);
                let left_leaf = -center + cl;
                let right_leaf = center + cr;
                if left_leaf > max_left { max_left = left_leaf; }
                if right_leaf > max_right { max_right = right_leaf; }
            }
        }
    }
    (max_left, max_right)
}

fn update_timeline_data(events: & mut Vec<(usize, Event)>, parent_data: &TimelineColumnData) {
    for (_, ev) in events {
        match ev {
            Event::Branch { branch_history, ty, ..} => {
                for branch in & mut *branch_history {
                    // copy the parent data
                    branch.t_data = parent_data.clone();
                }
                // Update the x-value of each branch based on its width.
                // The N-branch fanned-out layout used to be Match-only;
                // the dedicated 2-branch If/Loop arm hardcoded
                // `branch_history[1]`, which panicked when an `if`
                // without an `else` produced a single-branch event.
                // Math reduces to the same x-values as the old If
                // layout for N=2, so use the generic path for every
                // BranchType.
                let _ = ty; // BranchType doesn't affect x-positioning
                let centers = branch_centers(branch_history);
                let mut parent_branch_data: Vec<TimelineColumnData> = Vec::new();
                for (b_data, center) in branch_history.iter_mut().zip(&centers) {
                    b_data.t_data.x_val = parent_data.x_val + center * BRANCH_WEIGHT;
                    parent_branch_data.push(b_data.t_data.clone());
                }

                // recurse
                for (i, branch) in branch_history.iter_mut().enumerate() {
                    update_timeline_data(&mut branch.e_data, &parent_branch_data[i])
                }

            }
            _ => {}
        }
    }
}

// Header labels are monospace at 0.875em (14px); a glyph is about
// 0.6em wide. Round up so the estimate errs towards more room.
const LABEL_CHAR_PX: i64 = 9;
// Minimum blank space between two neighbouring header labels.
const LABEL_GAP: i64 = 10;
// Blank space between a struct box's edge and the label inside it that
// reaches furthest (#477). Neighbouring labels keep this much extra
// room so a widened box doesn't touch them.
const BOX_PAD: i64 = 6;

fn label_half_width(chars: i64) -> i64 {
    (chars * LABEL_CHAR_PX + 1) / 2
}

// Returns: a binary tree map from the hash of the ResourceOwner to its Column information
fn compute_column_layout<'a>(
    visualization_data: &'a mut VisualizationData,
    structs_info: &'a mut StructsInfo,
) -> (BTreeMap< u64, TimelineColumnData>, i32) {
    let mut resource_owners_layout = BTreeMap::new();
    let mut max_x: i64 = 0;
    let mut extent_map: HashMap<u64, (i64, i64)> = HashMap::new();

    // get the left/right extent of each timeline (in BRANCH_WEIGHT units).
    // compute_width populates branch.width as a side-effect; compute_extent
    // then walks the same tree to derive each side's actual leaf reach.
    for (h, timeline) in visualization_data.timelines.iter_mut() {
        let _ = compute_width(&mut timeline.history);
        let extent = compute_extent(&timeline.history);
        extent_map.insert(*h, extent);
    }

    // Group RAPs by their owning fn so each fn's columns get their
    // own x-axis (restarting from 0). Different fns occupy different
    // y ranges, so columns sharing x positions across fns don't
    // collide visually. Within a group, hashes are sorted to keep
    // declaration order.
    let mut by_fn: BTreeMap<usize, Vec<u64>> = BTreeMap::new();
    for (hash, timeline) in visualization_data.timelines.iter() {
        if matches!(timeline.resource_access_point, ResourceAccessPoint::Function(_)) {
            continue;
        }
        let fn_line = visualization_data.fn_start_lines.get(hash).copied().unwrap_or(0);
        by_fn.entry(fn_line).or_default().push(*hash);
    }
    // A pattern binding drawn side by side with the variable it binds
    // from (`if let Some(a) = &x && let Some(b) = y {`) gets its column
    // right after that variable's, so that their arrows don't overlap
    // (#510): x, a, y, b.
    for hashes in by_fn.values_mut() {
        for (from, to) in &visualization_data.binding_columns {
            let (Some(_), Some(j)) = (hashes.iter().position(|h| h == from), hashes.iter().position(|h| h == to)) else { continue };
            let to = hashes.remove(j);
            let i = hashes.iter().position(|h| h == from).unwrap_or(j);
            hashes.insert(i + 1, to);
        }
    }

    for (_fn_line, hashes) in by_fn.iter() {
        let mut x: i64 = 0; // reset per fn
        let mut owner: i64 = -1;
        let mut owner_x: i64 = 0;
        let mut last_x: i64 = 0;
        // Half the header-label widths of the open struct's first and
        // last columns (#477): the box must reach past both.
        let mut owner_half: i64 = 0;
        let mut last_half: i64 = 0;
        // Half the header-label width of the previous column in this
        // fn, so the next column can keep clear of it (#361).
        let mut prev_label: Option<(i64, i64)> = None; // (centre x, half width)

        for hash in hashes {
            let timeline = &visualization_data.timelines[hash];
            let name = match visualization_data.get_name_from_hash(hash) {
                Some(_name) => _name,
                None => panic!("internal state: no variable has hash {}", hash),
            };
            // Sized by what's shown (`SCOPE_MARK`s aren't).
            let shown = crate::expr_visitor_utils::display_text(&name);
            let mut x_space = cmp::max(70, (&(shown.len() as i64) - 1) * 13);
            let (extent_left, extent_right) = *extent_map.get(hash).unwrap();
            let left_px = extent_left * BRANCH_WEIGHT;
            let right_px = extent_right * BRANCH_WEIGHT;
            x = x + x_space + left_px;
            let title = match visualization_data.is_mut(hash) {
                true => String::from("mutable"),
                false => String::from("immutable"),
            };
            let mut ref_bool = false;

            // render reference label
            let label_len = if timeline.resource_access_point.is_ref() {
                let temp_name = shown.clone() + "|*" + &shown;
                x = x - x_space;
                x_space = cmp::max(90, (&(temp_name.len() as i64) - 1) * 7);
                x = x + x_space;
                ref_bool = true;
                temp_name.chars().count() as i64
            } else {
                shown.chars().count() as i64
            };

            // Labels are centred on their column, so the previous
            // label's right half and this label's left half share the
            // gap between the two columns. The spacing above only
            // looks at this column's own name, so a long label on the
            // left (`h.e.part|*h.e.part`) ran into a short one on its
            // right (#361). Widen the gap just enough when that would
            // happen; columns that already fit keep their position.
            let label_half = label_half_width(label_len);
            let rap = &timeline.resource_access_point;
            let starts_struct = rap.is_struct_group() && !rap.is_member();
            let extends_open_struct =
                owner != -1 && rap.is_member() && rap.get_owner() as i64 == owner;
            if let Some((prev_x, prev_half)) = prev_label {
                // A struct box ends after the previous column or starts
                // at this one: leave room for the box padding too.
                let mut needed = prev_half + label_half + LABEL_GAP;
                if owner != -1 && !extends_open_struct {
                    needed += BOX_PAD;
                }
                if starts_struct {
                    needed += BOX_PAD;
                }
                if x - prev_x < needed {
                    x = prev_x + needed;
                }
            }
            prev_label = Some((x, label_half));
            let label_x = x;

            let styled_name = styled_name_html(&name);

            // Struct boxes: a box spans a struct's own column and the
            // columns of its members that directly follow it (members
            // are registered right after their struct, so their hashes
            // are contiguous). A member extends the open box only if it
            // belongs to that struct (`get_owner`), and a new struct
            // closes the open box and opens its own, so two structs
            // declared back to back get one box each. `last_x` starts
            // at the struct's own column: a struct none of whose fields
            // got a column (none was ever used) gets a box around its
            // own label rather than a negative width (#192).
            if owner != -1 && !extends_open_struct {
                structs_info.structs.push((owner, owner_x, last_x, owner_half, last_half));
                owner = -1;
            }
            if starts_struct {
                owner = *rap.hash() as i64;
                owner_x = x;
                last_x = x;
                owner_half = label_half;
                last_half = label_half;
            } else if extends_open_struct {
                last_x = x;
                last_half = label_half;
            }

            resource_owners_layout.insert(*hash, TimelineColumnData
                {
                    name: name.clone(),
                    x_val: x,
                    title: styled_name.clone() + ", " + &title + &hover_messages::shadowed_name_note(&name, visualization_data.shadow_notes.get(hash)),
                    is_ref: ref_bool,
                    is_struct_group: timeline.resource_access_point.is_struct_group(),
                    is_member: timeline.resource_access_point.is_member(),
                    owner: timeline.resource_access_point.get_owner(),
                });
            x += right_px;
            // Keep the rightmost label inside the panel (its width is
            // max_x + 100).
            max_x = cmp::max(max_x, label_x + label_half + LABEL_GAP - 100);
        }
        // Finalize any open struct group at the end of this fn
        // (same trailing-struct fix as before, scoped per-fn).
        if owner != -1 {
            structs_info.structs.push((owner, owner_x, last_x, owner_half, last_half));
        }
        max_x = cmp::max(max_x, x);
    }

    // After per-fn x assignment, update each Timeline's history with
    // its TimelineColumnData (used downstream for arrow rendering).
    for (h, timeline) in visualization_data.timelines.iter_mut() {

        match timeline.resource_access_point {
            ResourceAccessPoint::Function(_) => {},
            _ => {
                let root_data = resource_owners_layout.get(h).unwrap();
                update_timeline_data(& mut timeline.history, root_data);
            }
        }
    }

    (resource_owners_layout, (max_x as i32) + 100)
}

/// How many rows above its fn's signature the column header of `hash` is
/// drawn: one, or two over a one-line conditional's fork on the signature
/// row (`VisualizationData::raised_headers`, #1342).
fn header_rows_up(raised_headers: &HashSet<u64>, hash: u64) -> i64 {
    if raised_headers.contains(&hash) { 2 } else { 1 }
}

fn render_labels_string(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    fn_start_lines: &HashMap<u64, usize>,
    raised_headers: &HashSet<u64>,
    registry: &Handlebars
) {
    // Default label-y matches the legacy "all labels at the top of
    // the SVG" behavior; per-fn RAPs override below.
    const DEFAULT_LABEL_Y: i64 = FIRST_ROW_Y - 20;
    for (hash, column_data) in resource_owners_layout.iter() {
        // Position the label on the row directly above the fn's
        // first source line so each fn gets its own label header
        // adjacent to its body. `get_y_axis_pos(line)` is the
        // baseline for source line `line`; subtracting LINE_SPACE
        // puts us on the row above. Falls back to the legacy
        // top-of-svg position for RAPs without an fn association
        // (e.g. globals — none today, but defensive).
        let y_val = match fn_start_lines.get(hash) {
            // A row higher over a one-line conditional's fork (#1342).
            Some(&line) => get_y_axis_pos(line) - LINE_SPACE * header_rows_up(raised_headers, *hash),
            None => DEFAULT_LABEL_Y,
        };
        let mut data = ResourceAccessPointLabelData {
            x_val: column_data.x_val,
            y_val,
            hash: hash.to_string(),
            name: column_data.name.clone(),
            title: column_data.title.clone(),
            rv: Rv::new(rv_attrs::HEADER).var(&column_data.name).attrs(),
        };

        if column_data.is_ref {
            let new_name = column_data.name.to_owned() + "<tspan stroke=\"none\">|</tspan>*" + &column_data.name;
            data.name = new_name;
        }

        // push to individual timelines
        if column_data.is_struct_group {
            let owner_entry = ensure_owner_entry(output, column_data.owner as i64);
            if column_data.is_member {
                owner_entry.1.labels.push_str(&registry.render("label_template", &data).unwrap());
            } else {
                owner_entry.0.labels.push_str(&registry.render("label_template", &data).unwrap());
            }
        }
        else {
            output.get_mut(&-1).unwrap().0.labels.push_str(&registry.render("label_template", &data).unwrap());
        }
    }
}

fn append_dot(
    dot_data: EventDotData,
    timeline_data: &TimelineColumnData,
    dots: &mut DotLayer,
    lend_id: Option<usize>,
    ev: &'static str,
    row: usize,
) {
    dots.add_event(dot_data.hash, dot_data.dot_x, dot_data.dot_y, timeline_data, dot_data.title, false, lend_id, ev, row);
}

fn caller_end(event: &Event) -> bool {
    matches!(event, Event::StaticDie { to: ResourceTy::Caller, .. } | Event::MutableDie { to: ResourceTy::Caller, .. })
}

fn branch_entry_state(state: &State) -> State {
    match state {
        State::OutOfScope => State::FullPrivilege { s: LineState::Uninit },
        state => convert_back(state),
    }
}

fn arm_entry_state(parent: &State, active: bool) -> State {
    if active { parent.clone() } else { branch_state_converter(parent) }
}

/// The split dot, the arm lines and the join dot a conditional draws on one
/// variable's column (an `Event::Branch` in its history).
fn render_branch_dots(
    hash: &u64,
    history: &Vec<(usize, Event)>,
    event: &Event,
    line_number: &usize,
    is: &ResourceTy,
    branch_history: &Vec<BranchData>,
    ty: &BranchType,
    split_point: &usize,
    merge_point: &usize,
    moved_at_event: bool,
    held_at_event: bool,
    timeline_data: &TimelineColumnData,
    visualization_data: &VisualizationData,
    dots: &mut DotLayer,
) {
        // Top-of-branch dot on the parent column. Tooltip
        // explains the variable's role ("X is live in a
        // conditional expression"). Anchors the parent
        // column visually at the line where the conditional
        // begins. When the branches share lines it sits on
        // the top corner of their diamond (#117).
        let collapsed = branch_is_collapsed(*split_point, *merge_point);
        let rap = is.extract_rap().unwrap_or(&visualization_data.timelines[hash].resource_access_point);
        // A dot sits on a line of its column. Where none reaches
        // it, it isn't drawn, and its text goes on the column's
        // next dot (`DotLayer::carry`, #702).
        // The split: the arms leave it in the state the column has
        // there (as `render_branches` draws them), dotted while the
        // column is mutably borrowed (`r = &mut s` live across the
        // conditional, #722). The legs start at the split dot then too
        // (#1371): left out, the dotted diamond forked in mid-line and
        // each arm's first dot said s is live in the conditional.
        let split_has_line = branch_history.first()
            .and_then(|b| b.entry_state.as_ref())
            .is_none_or(|entry| {
                let parent = branch_entry_state(entry);
                branch_history.iter().enumerate().any(|(i, arm)| {
                    seg_drawn(rap, &arm_entry_state(&parent, i == 0 && !arm.e_data.is_empty()))
                })
            });
        // A conditional on a column whose value was already moved
        // away (a nested `if` in an arm after `match (x, y)` moved
        // x into the temporary tuple): x isn't live in it, and no
        // arm line leaves the split, so a dot there would float
        // (#694), and its text isn't true. Its arms still draw
        // their events (a drop stated at a `return`).
        let imaginary_edge = branch_history.iter().find_map(|b| b.imaginary);
        let imaginary_arm = imaginary_edge.and_then(|edge| visualization_data.state_table.imaginary_arm(edge));
        let imaginary_has_line = imaginary_edge
            .is_some_and(|edge| imaginary_leg(rap, visualization_data, edge, *split_point).is_some());
        let split_drawn = imaginary_has_line || split_has_line;
        // Text waiting from before the conditional goes on the
        // split dot, or else on each arm's first dot.
        let initializer = imaginary_arm.and_then(|arm| arm.initializer_bindings.get(rap.name()));
        let entry = visualization_data.fn_source_lines.get(hash).and_then(|&function| {
            let branch = branch_history.iter().find_map(|arm| arm.source)?.branch;
            visualization_data.state_table.branch_entry_cell(function, rap.name(), branch)
        });
        let entry_maybe_init = entry.map_or_else(|| visualization_data.fn_source_lines.get(hash)
            .zip(rv_attrs::line_of(*line_number, &visualization_data.row_lines))
            .and_then(|(&function, row)| visualization_data.state_table.incoming_cell(function, rap.name(), row))
            .is_some_and(|cell| cell.init == crate::facts::table::Init::Maybe)
            // A source row can include another arm's boundary (e.g. an
            // else-if on its own line after the preceding arm's brace).
            // Check the history of the arm that actually reaches this fork.
            && history.iter().position(|(_, e)| std::ptr::eq(e, event))
                .and_then(|i| history_path(&visualization_data.timelines[hash].history, history, i))
                .is_some_and(|path| maybe_moved_at_end(&path)
                    || matches!(init_at_end(&path), Some(InitJoin::Maybe { .. } | InitJoin::Loop))),
            // The runtime view can say "moved" where the borrow checker
            // still counts a path that holds a value (a write only a dead
            // path reaches): it still can't prove the place holds one.
            |cell| cell.init == crate::facts::table::Init::Maybe || cell.borrowck_maybe);
        let fork_title = || if let Some(binding) = initializer {
            hover_messages::event_dot_imaginary_initializer(binding)
        } else if visualization_data.row_layout.ended_at_fork.contains(&(event.get_id(), *hash)) {
            hover_messages::state_ref_borrow_ended(&is.real_name())
        } else if entry_maybe_init {
            format!("The borrow checker can't prove that {} holds a value at this branch decision.", is.real_name())
        } else if entry.is_some_and(|cell| cell.init == crate::facts::table::Init::Unassigned) {
            hover_messages::state_uninitialized(&is.real_name())
        } else if moved_at_event && split_has_line {
            // The entry state still draws a dashed fork, for example
            // after a block that may have left the binding unassigned.
            "The conditional begins here.".to_owned()
        } else {
            event.print_message_with_name(&mut is.real_name())
        };
        let mut arm_carry = Vec::new();
        if !split_drawn {
            arm_carry = dots.take_carry(*hash);
            if !moved_at_event {
                arm_carry.push((fork_title().trim_end().to_owned(), rv_attrs::event_name(event)));
            }
        }
        if split_drawn {
            let b_data = EventDotData {
                hash: *hash as u64,
                dot_x: timeline_data.x_val,
                dot_y: if collapsed {
                    get_y_axis_pos(*split_point) - COLLAPSED_APEX
                } else if let Some(rise) = CollapsedApexes::of(history).raised_splits.get(split_point) {
                    get_y_axis_pos(*split_point) - rise
                } else {
                    get_y_axis_pos(*line_number)
                },
                title: fork_title()
            };
            append_dot(b_data, timeline_data, dots, None, rv_attrs::event_name(event), *line_number);
        }

        // Each branch column gets its real events rendered.
        // No "If"/"Else" bookend dots: the per-branch
        // start-dot (at split_point + 1) was a teaching
        // distraction — it landed on whichever line the
        // *other* branch happened to acquire on, with the
        // wrong arm's label. The per-branch end-dot at
        // merge_point added another redundant pair on the
        // closing-brace line. Branch identity is already
        // communicated by column position; the join dot
        // below speaks for what happened.
        // Text an arm that reaches the join leaves waiting goes on
        // the join dot or past it; one an arm that returns or
        // panics leaves can't (that arm never gets there).
        let mut leftover: Vec<(String, &'static str)> = Vec::new();
        for (arm, branch) in branch_history.iter().enumerate() {
            dots.set_carry(*hash, arm_carry.clone());
            // Whether the value is still owned when it goes out
            // of scope inside the branch: only at a `return`
            // (#363), where it's dropped if so.
            // The last state it goes out of scope from, not the one
            // before the first gap: a conditional nested in the branch
            // leaves an out-of-scope gap of its own, between its split
            // and its join, and a move before it would read as "nothing
            // to drop" although the nested branches all gave the
            // variable a new value (#1135, #1194).
            let hold = (1..branch.states.len()).rev()
                .find(|&i| matches!(branch.states[i].2, State::OutOfScope)
                    && !matches!(branch.states[i - 1].2, State::OutOfScope))
                // (`Uninit`: may hold no value, #244.)
                .map_or(false, |i| matches!(
                    branch.states[i - 1].2,
                    State::FullPrivilege { s: LineState::Full | LineState::Gray } | State::PartialPrivilege { .. }
                ));
            render_dot(hash, &branch.e_data, &branch.t_data, visualization_data, hold, dots, held_at_event);
            if branch.diverges && branch_history.iter().all(|b| b.diverges) {
                // The end dot occupies the stop marker on this column.
                let stop = ty.get_start_end(arm).1;
                if let Some((row, _)) = branch.e_data.last().filter(|(row, e)| *row == stop && caller_end(e)) {
                    let at = (*hash, branch.t_data.x_val, get_y_axis_pos(*row));
                    if let Some(&i) = dots.at.get(&at) {
                        dots.dots[i].lines.push((None, hover_messages::branch_never_reaches_merge(branch.exits.contains(&BranchExit::Other))));
                    }
                }
            }
            for text in dots.take_carry(*hash) {
                if !branch.diverges && !leftover.iter().any(|(t, _)| *t == text.0) {
                    leftover.push(text);
                }
            }
        }
        dots.set_carry(*hash, leftover);

        // A conditional whose arms all leave the function has no normal
        // continuation. A loop whose arms break can still have an exit.
        if !branch_history.is_empty() && branch_history.iter().all(|b| b.diverges) {
            // Unless some of them `break` out of a loop (#759): the
            // column goes on after the loop, in the state those branches
            // left it (`compute_branch_states`), from a dot here, so it
            // doesn't start in mid-air below the arms' stop bars (#1296).
            let broke: Vec<State> = branch_history.iter()
                .filter(|b| breaks_out_only(&b.exits))
                .filter_map(|b| b.states.last().map(|s| s.2.clone()))
                .collect();
            let (state, exhausted) = state_after_loop_exits(&broke, matches!(event, Event::Branch { on_exhaustion: true, .. }), branch_history);
            if seg_drawn(rap, &state) {
                // The branches it goes on from: those that break still
                // holding something (not one that dropped it, #410).
                let live: Vec<_> = branch_history.iter().enumerate()
                    .filter(|(_, b)| breaks_out_only(&b.exits)
                        && b.states.last().is_some_and(|s| seg_drawn(rap, &convert_back(&s.2))))
                    .map(|(i, _)| i).collect();
                let carried = match live.as_slice() {
                    [arm] if branch_history.len() == 2 && branch_history.iter().all(|b| b.exits.as_slice() == [BranchExit::Break]) => {
                        let b = &branch_history[*arm];
                        b.source_span.and_then(|span| {
                            let f = *visualization_data.fn_source_lines.get(hash)?;
                            let (cell, _) = visualization_data.state_table.arm_loan_cell(f, rap.name(), span.end.0, span, b.source)?;
                            // MIR reborrows the reference on this edge. The
                            // transfer to the loop result has that same holder.
                            let supplied = cell.lent.iter().any(|l| history.iter().any(|(_, e)|
                                matches!(e, Event::MutableLend { id, to, .. } if to.name() == l.holder
                                    && borrow_break_alt(*id, visualization_data).is_some_and(|a| a.end == AltEnd::Loop && a.reborrow == Some(l.loan)))));
                            Some(hover_messages::both_branches_break(rap.name(), &ty.string_of_branch(*arm), supplied))
                        })
                    }
                    _ => None,
                };
                let cy = get_y_axis_pos(*merge_point) + if collapsed { COLLAPSED_APEX } else { 0 };
                let m_data = EventDotData {
                    hash: *hash as u64,
                    dot_x: timeline_data.x_val,
                    dot_y: cy,
                    title: if exhausted {
                        "The iterator's next() returned None, so the loop exits instead of starting another iteration.".to_owned()
                    } else {
                        carried.unwrap_or_else(|| hover_messages::event_dot_branch_merge_after_break(&is.real_name(), live.len()))
                    },
                };
                append_dot(m_data, timeline_data, dots, None, rv_attrs::BRANCH_JOIN, *merge_point);
            }
            return;
        }
        // An `if let`'s scrutinee temporary forked because an exit in the
        // matched block drops it (#1360): every branch that gets here
        // dropped it too (at the end of the matched block, or before the
        // `else` block), so its column ends in those drops, with no join.
        if rap.is_unbound_temporary() && branch_history.iter().filter(|b| !b.diverges)
            .all(|b| matches!(b.states.last().map(|s| &s.2), Some(State::OutOfScope))) {
            return;
        }
        let function = visualization_data.fn_source_lines.get(hash).copied();
        let Event::Branch { join, .. } = event else { unreachable!() };
        let join = join.as_deref();
        let short_circuit = visualization_data.row_layout.short_circuit_forks.contains(&event.get_id());
        let segment = if join.is_none() && (matches!(ty, BranchType::Loop(..)) || is_branch_temp_col(rap) || short_circuit) {
            (|| {
                let row = rv_attrs::line_of(*merge_point, &visualization_data.row_lines)?;
                visualization_data.state_table.segment_cell(function?, rap.name(), row)
            })()
        } else {
            None
        };
        let join_cell = join.map(|j| &j.cell).or(segment.as_ref());
        let field_initialized = short_circuit && rap.is_ref()
            && join_cell.is_some_and(|cell| cell.init == crate::facts::table::Init::Own);
        let reassigned = join_cell.and_then(|cell| cell.reassigned);
        let staged = join.is_some_and(|j| j.change.staged_result);
        let (mixed_moved, all_moved) = if staged { (false, false) } else { branch_moves(event) };
        let init = (!staged).then(|| branch_initialization(event)).flatten();
        let branch_keyword = if matches!(ty, BranchType::Match(..)) { "match" } else { "if" };
        let cx = timeline_data.x_val;
        let cy = get_y_axis_pos(*merge_point) + if collapsed {
            COLLAPSED_APEX
        } else {
            -join_rise(visualization_data, hash, history, *split_point, *merge_point)
        };
        // A reference declared before the conditional whose
        // borrow ended in every arm (`end_outer_borrows_in_arms`).
        let borrow_ended_in_every_arm = is.extract_rap().is_some_and(|r| r.is_ref())
            && all_paths_present(ty, branch_history.len())
            && branch_history.iter().all(|b| {
                matches!(
                    b.states.last().map(|s| &s.2),
                    Some(State::OutOfScope) | Some(State::FullPrivilege { s: LineState::Uninit })
                ) && ends_borrow(&b.e_data)
            });
        // One whose borrow ended before the conditional (never read
        // again) that an arm gave a new loan, ended in that arm (`r =
        // &t;` in one arm of `if c { r = &t; } else { }`, #1524): its legs
        // reach the join with no borrow on any path.
        // (Its last event before the conditional ends its borrow, or one
        // on the split line after it does: a last use in the condition.)
        let is_die = |e: &Event| matches!(e, Event::StaticDie { .. } | Event::MutableDie { .. } | Event::RefDie { .. });
        let is_start = |e: &Event| matches!(e, Event::StaticBorrow { .. } | Event::MutableBorrow { .. } | Event::Copy { .. } | Event::Acquire { .. });
        let ended_before = || is.extract_rap().is_some_and(|r| r.is_ref())
            && history.iter().position(|(_, e)| std::ptr::eq(e, event)).is_some_and(|k| {
                let before = &history[..k];
                let died = before.iter().rposition(|(_, e)| is_die(e));
                let started = before.iter().rposition(|(_, e)| is_start(e));
                died.is_some_and(|d| started.is_none_or(|s| d > s))
                    || history[k + 1..].iter().take_while(|(l, _)| *l == *split_point).any(|(_, e)| is_die(e))
            })
            && all_paths_present(ty, branch_history.len())
            && branch_history.iter().any(|b| !b.diverges && ends_borrow(&b.e_data))
            && branch_history.iter().filter(|b| !b.diverges).all(|b| {
                matches!(
                    b.states.last().map(|s| &s.2),
                    Some(State::OutOfScope) | Some(State::FullPrivilege { s: LineState::Uninit })
                ) && (b.e_data.is_empty() || ends_borrow(&b.e_data))
            });
        // The join is reached by the line of each arm that gets
        // there still holding something (`render_branches`). Where
        // no arm does (the arms of a nested `if` that kept s end at
        // its own join, a reference's borrow ended in every arm),
        // the dot would float: its text goes on the column's next
        // dot instead, the scope end or the next event (#702).
        // An arm still mutably lent out at the join (`r = &mut s` in it)
        // reaches it with a dotted line (#722), and the dot is drawn
        // where that line ends (another arm moved s, #1297) and where
        // the column goes on after the join, dotted: the legs end at the
        // join dot, not in mid-line (#1371).
        let join_has_line = branch_history.iter().any(|b| {
            let end = arm_end_state(rap, b, branch_history, ty);
            !b.diverges && seg_drawn(rap, &end)
        });
        // `None`: no join dot at all (the all-moved case below).
        let imaginary_join = imaginary_arm.and_then(|arm| {
            use crate::facts::{table::Init, ImaginaryChange};
            let entry = arm.entry.get(rap.name())?;
            if !matches!(entry.init, Init::Own | Init::Partly) { return None; }
            let join = arm.join.get(rap.name());
            if let Some(parts) = join.and_then(|c| c.parts.as_ref()) {
                let imaginary: Vec<_> = parts.maybe.iter().filter(|p|
                    matches!(arm.changes.get(*p), Some(ImaginaryChange::Move | ImaginaryChange::Assign))
                        && arm.join.get(*p).is_some_and(|c| c.cause == Some("imaginary"))
                ).cloned().collect();
                if !imaginary.is_empty() {
                    let mut parts = parts.clone();
                    parts.maybe.retain(|p| !imaginary.contains(p));
                    return Some(hover_messages::state_parts_imaginary(rap.name(), &parts, &imaginary));
                }
            }
            let change = arm.changes.get(rap.name())?;
            if matches!(change, ImaginaryChange::AlsoChangesAtRuntime) {
                return join.filter(|c| c.init == Init::Maybe && c.cause == Some("moved"))
                    .map(|_| hover_messages::event_dot_borrowck_maybe_moved(rap.name()));
            }
            let assigned = matches!(change, ImaginaryChange::Assign);
            if join.is_some_and(|c| c.cause == Some("imaginary")
                || (!assigned && entry.init == Init::Own && c.init == Init::Maybe && c.cause == Some("moved")))
            {
                // An untracked field can inherit its parent's cause. If
                // this field was initialized at entry and has no runtime
                // changes, the dead arm accounts for the possible move.
                Some(hover_messages::state_maybe(rap.name(), Some("imaginary")))
            } else {
                let usable = join.is_some_and(|cell| cell.init == Init::Own && cell.lent.is_empty()
                    && !cell.unnamed_maybe && cell.parts.as_ref().is_none_or(|p| p.moved.is_empty() && p.maybe.is_empty()));
                Some(hover_messages::event_dot_imaginary_change(rap.name(), assigned, usable))
            }
        });
        let mixed_init = join.filter(|j| !j.change.replaces_moved && !j.change.entry_own).map(|j| &j.cell).filter(|c| c.mixed_init.is_some_and(|mixed|
            mixed.may_hold_value && !is_branch_temp_col(rap)));
        // The irrefutable-pattern join already explains which arm ran.
        let runtime_assigned = join.map(|j| &j.cell).filter(|c| c.always_assigned && !branch_history.iter().any(|b| b.imaginary.is_some()));
        let title = if field_initialized {
            let kind = if rap.is_mutref() { "a mutable" } else { "an immutable" };
            Some(format!("{} holds {kind} reference after the short-circuit expression.", styled_name_html(rap.name())))
        } else if imaginary_join.is_some() {
            imaginary_join
        } else if let Some(cell) = runtime_assigned {
            Some(hover_messages::event_dot_runtime_assigned(rap.name(), !rap.drops_nothing(), rap.is_mut(), cell.reassigned, &rap.drop_note()))
        } else if join.is_some_and(|j| j.cell.partial_without_drop) {
            Some(format!("{}. No resource remains in {} on the paths that run.",
                hover_messages::event_dot_borrowck_maybe_moved(rap.name()), styled_name_html(rap.name())))
        } else if join.is_some_and(|j| j.cell.borrowck_maybe && j.cell.init == Init::Moved && !staged) {
            // Every path that runs moved it; the checker also counts a
            // path (a dead branch) that kept it. The dot is drawn
            // because that branch's line still reaches the join.
            Some(format!("{}. No resource remains in {} on the paths that run.",
                hover_messages::event_dot_borrowck_maybe_moved(rap.name()), styled_name_html(rap.name())))
        } else if visualization_data.row_layout.ended_at_fork.contains(&(event.get_id(), *hash)) {
            Some(hover_messages::state_ref_borrow_ended(&is.real_name()))
        } else if borrow_ended_in_every_arm {
            let mut title = hover_messages::event_dot_branch_merge_borrow_ended(&is.real_name());
            if rap.is_ref() && !rap.is_mutref() && join.is_some_and(|j| j.cell.init == crate::facts::table::Init::Own)
                && branch_history.iter().all(|b| b.e_data.iter().any(|(_, e)|
                    visualization_data.row_layout.fork_arrow_titles.contains_key(&e.get_id()))) {
                title.push_str(". ");
                title.push_str(&hover_messages::event_dot_branch_merge_unchanged(&is.real_name()));
            }
            Some(title)
        } else if ended_before() {
            Some(hover_messages::event_dot_branch_merge_borrow_ended_before(&is.real_name()))
        } else if rap.is_ref() && init.is_some() && !mixed_moved {
            join_has_line.then(hover_messages::event_dot_branch_merge_conditional_ends)
        } else if !staged && join.is_some_and(|j| j.cell.init == Init::Maybe
            && j.change.entry_maybe && j.change.test_wrote && !j.change.test_moved) {
            Some(hover_messages::event_dot_branch_merge_chain_new_value(
                rap.name(), branch_keyword, rap.drops_nothing(), rap.is_mut(), reassigned, &rap.drop_note()))
        } else if let Some(cell) = mixed_init {
            Some(hover_messages::event_dot_branch_merge_mixed_init(
                rap.name(), cell, !rap.drops_nothing(), rap.is_mut(), &rap.drop_note(),
            ))
        } else if let Some(InitJoin::AllMoved) = init {
            Some(hover_messages::event_dot_branch_merge_init_all_moved(&is.real_name(), rap.is_mut()))
        } else if let (Some(InitJoin::Maybe { moved }), false) = (init,
            branch_maybe_held(event) || join.is_some_and(|j| j.change.replaces_moved)) {
            // As for a maybe-moved variable below, the old value
            // is dropped by a drop flag at the next assignment
            // if there is one, or else at the scope end. That
            // assignment may be in a later conditional's
            // branch, on some paths only.
            let full = visualization_data.timelines.get(hash).map_or(history, |t| &t.history);
            Some(if is_branch_temp_col(rap) {
                // A temporary a `let` extends through a branch (#1397).
                hover_messages::event_dot_branch_merge_temp_made(&is.real_name(), made_on_line(full, &visualization_data.row_lines))
            } else if reassigned == Some(true) {
                hover_messages::event_dot_branch_merge_maybe_init_then_reassigned(&is.real_name(), false, moved)
            } else if reassigned == Some(false) {
                hover_messages::event_dot_branch_merge_maybe_init_then_reassigned(&is.real_name(), true, moved)
            } else {
                hover_messages::event_dot_branch_merge_maybe_init(
                    &is.real_name(),
                    is.extract_rap().map_or(false, |r| r.drops_nothing()),
                    rap.is_mut(),
                    moved,
                    &is.extract_rap().map_or(String::new(), |r| r.drop_note()),
                )
            })
        } else if !staged && branch_maybe_held(event) && !join.is_some_and(|j| j.change.replaces_moved) {
            // It may have held nothing as the conditional started,
            // and only some branches assign it (#1034): it may hold
            // the new value, the old one, or nothing.
            Some(hover_messages::event_dot_branch_merge_maybe_held(
                &is.real_name(),
                is.extract_rap().map_or(false, |r| r.drops_nothing()),
                rap.is_mut(),
                reassigned,
            ))
        } else if !staged && join.is_some_and(|j| j.cell.always_assigned) {
            Some(hover_messages::state_imaginary_unassigned(rap.name()))
        } else if !staged && join.is_some_and(|j| j.cell.init == Init::Unassigned) {
            let mut leavers = Vec::new();
            assigned_by_leavers(branch_history, &mut leavers);
            Some(hover_messages::event_dot_branch_merge_still_unassigned(&is.real_name(), &leavers))
        } else if !staged && join.is_some_and(|j| j.cell.init == Init::Maybe && j.cell.cause == Some("unassigned")) {
            let mut leavers = Vec::new();
            assigned_by_leavers(branch_history, &mut leavers);
            Some(if !leavers.is_empty() {
                hover_messages::event_dot_branch_merge_maybe_still_unassigned(&is.real_name(), &leavers)
            } else {
                hover_messages::event_dot_branch_merge_maybe_init_unchanged(&is.real_name(), is_branch_temp_col(rap))
            })
        } else if join.is_some_and(|j| j.cell.init == Init::Maybe && j.cell.cause == Some("moved")
            && !j.change.moved && !j.change.wrote)
        {
            Some(hover_messages::event_dot_branch_merge_maybe_init_unchanged(rap.name(), is_branch_temp_col(rap)))
        } else if mixed_moved {
            // Mixed: at least one branch moved, at least
            // one didn't (counting the implicit untouched
            // else of an `if` without an else as a
            // didn't-move branch). The variable is maybe-moved
            // from here on. Rust doesn't drop it at the
            // branch's end: a runtime drop flag decides at
            // scope end whether it still owns its resource
            // (#179), so this is a plain dot and the drop is
            // shown, conditionally, on the scope-end dot.
            // If the variable is assigned again before that,
            // the drop flag is checked there instead (#335).
            // Looked up in what runs after the join (`events_after`):
            // the rest of each outer branch this merge is nested in
            // comes first (`if c { if d { drop(s); } s = …; }` assigns
            // s again in that branch, #1413; so can a later conditional
            // in it, #1460), then the column's whole history.
            // This `if`'s let-chain condition, not only a branch body,
            // may have moved it (#940, #953), or a `match` arm's
            // guard that failed (#247): the keyword of the conditional,
            // for "can't be used after the if / match".
            let by_cond = if short_circuit {
                Some("short-circuit expression")
            } else {
                (join.is_some_and(|j| j.change.test_moved)
                    || chain_condition_moved_here(branch_history)).then_some(branch_keyword)
            };
            Some(if rap.is_split_scrutinee_temporary() {
                // A scrutinee temporary an arm's pattern moved (part of)
                // its value out of (#1386): its drop, on this dot or
                // below, says what is dropped.
                hover_messages::event_dot_branch_merge_temp_moved_out(&is.real_name())
            } else if join.is_some_and(|j| j.cell.init == Init::Partly && j.maybe_empty_box
                && !j.change.moved && rap.is_struct_group())
            {
                // The Box allocation stays in this column. Its named
                // fields show which contents leave in each arm.
                hover_messages::event_dot_branch_merge_unchanged(rap.name())
            } else if join.is_some_and(|j| j.maybe_empty_box && !j.change.moved) {
                hover_messages::event_dot_branch_merge_box_contents_moved(&is.real_name(), rap.is_mut())
            } else if join.is_some_and(|j| j.maybe_empty_box) {
                hover_messages::event_dot_branch_merge_moved_box_paths(&is.real_name(), rap.is_mut(), by_cond)
            } else if rap.elems_left().is_none()
                && join.is_some_and(|j| j.cell.init == Init::Partly && j.cell.unnamed_maybe)
            {
                hover_messages::event_dot_branch_merge_move(
                    &is.real_name(), rap.drops_nothing(), rap.is_mut(), &rap.drop_note(), by_cond, join_cell,
                )
            } else if let Some(kw) = by_cond.filter(|_| chain_condition_assigned_here(branch_history)) {
                // It may have been given a new value when it held none
                // (#603): no "if nothing above moved it".
                hover_messages::event_dot_branch_merge_chain_new_value(
                    &is.real_name(), kw,
                    is.extract_rap().map_or(false, |r| r.drops_nothing()),
                    rap.is_mut(), reassigned,
                    &is.extract_rap().map_or(String::new(), |r| r.drop_note()))
            } else if join.is_some_and(|j| j.change.replaces_moved) {
                hover_messages::event_dot_branch_merge_replaced_or_moved(
                    rap.name(), rap.drops_nothing(), rap.is_mut(), reassigned, by_cond, &rap.drop_note(),
                )
            } else if mixed_box_paths(rap, &history.iter()
                .take_while(|(_, e)| !std::ptr::eq(e, event)).cloned()
                .chain(std::iter::once((*line_number, event.clone()))).collect::<Vec<_>>()) {
                // A Box whose contents were moved out before (#1220):
                // what the paths that didn't move it still hold isn't
                // the whole Box on all of them.
                hover_messages::event_dot_branch_merge_moved_box_paths(&is.real_name(), rap.is_mut(), by_cond)
            } else if let Some(el) = rap.elems_left() {
                // A slice pattern in a branch took the last elements
                // left in the array (#1302): the others were moved
                // before it, so only those may still be in it.
                let mut moved: Vec<String> = Vec::new();
                for b in branch_history.iter().filter(|b| !b.diverges) {
                    for p in elems_moved_before(&b.e_data, usize::MAX, true).0.iter().flat_map(|p| place_elem_names(p)) {
                        if !moved.contains(&p) {
                            moved.push(p);
                        }
                    }
                }
                let before: Vec<String> = history.iter().take_while(|(_, e)| !std::ptr::eq(e, event))
                    .filter_map(|(_, e)| match e {
                        Event::Move { is: ResourceTy::Index(_, place, _), .. } if !place.starts_with('*') => Some(place_elem_names(place)),
                        _ => None,
                    }).flatten().collect();
                let still: Vec<String> = el.all().into_iter().filter(|n| !moved.contains(n) && !before.contains(n)).collect();
                hover_messages::event_dot_branch_merge_elems_moved(&is.real_name(), &moved, &still, &el.payloads)
            } else {
                // A `&mut` reference moved on some paths (`if b {
                // x } else { y }` returned, #674) owns nothing to
                // drop on the others, nor does a ManuallyDrop (#1147).
                let move_cell = join_cell.filter(|cell| cell.mixed_init.is_none());
                hover_messages::event_dot_branch_merge_move(
                    &is.real_name(), rap.drops_nothing(), rap.is_mut(), &rap.drop_note(), by_cond, move_cell,
                )
            })
        } else if rap.is_ref() && !all_moved && branch_history.iter().filter(|b| !b.diverges).any(|b| {
            matches!(b.states.last().map(|s| &s.2),
                Some(State::OutOfScope | State::FullPrivilege { s: LineState::Uninit }))
        }) {
            join_has_line.then(hover_messages::event_dot_branch_merge_conditional_ends)
        } else if all_moved {
            // Every branch ends without the resource, by a move
            // or a drop (or an implicit drop at a nested merge):
            // no line comes out of any arm, so a join dot here
            // would float, unconnected, between the arms (#694).
            // Like a column after a plain move, it just ends with
            // the arms; the scope-end dot says why nothing is
            // dropped there (`all_moved_at_end`).
            None
        } else if let Some(timing) = rap.is_closure().then(|| branch_history.iter()
            .filter(|b| b.diverges && b.imaginary.is_none())
            .flat_map(|b| &b.e_data)
            .find_map(|(_, e)| match e {
                Event::ClosureCall { timing, .. } => Some(*timing),
                _ => None,
            })).flatten()
        {
            use crate::svg_generator::data::CallTiming;
            let name = styled_name_html(rap.name());
            Some(match timing {
                CallTiming::Direct => format!("Calling {name} consumes it on the branch that exits here"),
                CallTiming::Skipped => format!("The branch that would consume {name} is skipped at runtime"),
                CallTiming::AfterArguments | CallTiming::Never => format!(
                    "{name} is moved into a temporary before evaluating the call arguments on the branch that exits here"),
            })
        } else {
            Some(branch_join_message(branch_history, ty, is, imaginary_arm, join))
        };
        // Unless the column's next event is a later conditional, which
        // draws no split dot either (none of its lines reach it): the
        // text would go on the first dot of each of its arms, and on its
        // join for an arm with none, as if about that conditional
        // (`if let Some(x) = o && x.len() > 1 {} if c { o = …; }`,
        // #1460). The join dot stays on its own row, bare, as the
        // later dots of a maybe-moved column are.
        let next_is_conditional = !join_has_line && matches!(
            events_after(visualization_data, hash, history, event, *merge_point).first(),
            Some((_, Event::Branch { .. })));
        match title {
            Some(title) if join_has_line || next_is_conditional => {
                let m_data = EventDotData {
                    hash: *hash as u64,
                    dot_x: cx,
                    dot_y: cy,
                    title,
                };
                append_dot(m_data, timeline_data, dots, None, rv_attrs::BRANCH_JOIN, *merge_point);
            }
            Some(title) if !title.is_empty() => {
                let mut waiting = dots.take_carry(*hash);
                waiting.push((title, rv_attrs::BRANCH_JOIN));
                dots.set_carry(*hash, waiting);
            }
            _ => {}
        }
        return;
}

/// The dot for `Event::OwnerGoOutOfScope`: a variable's scope end, with the
/// drop triangle when its resource is dropped there. Return the tooltip
/// and whether that occurrence runs drop glue.
fn scope_end_message(
    hash: &u64,
    history: &Vec<(usize, Event)>,
    index: usize,
    event: &Event,
    line_number: &usize,
    resource_hold: bool,
    mut name: String,
    visualization_data: &VisualizationData,
) -> (String, bool) {
        use crate::facts::ScopeDrop;
        let mut title;
        let Event::OwnerGoOutOfScope { ro, id, .. } = event else { unreachable!() };
        // A static outlives the function (#690).
        if ro.is_static() {
            title = hover_messages::event_dot_static(&name);
            return (title, false);
        }
        let in_branch = !std::ptr::eq(&visualization_data.timelines[hash].history, history);
        // The events on the path to an exit's drop in a branch, from the
        // column's start.
        let exit_path = in_branch.then(|| history_path(&visualization_data.timelines[hash].history, history, history.len())).flatten();
        let never_drops = ro.never_drops();
        // Not Copy but nothing to drop (#484): a value, so
        // no drop triangle and no "resource" wording.
        // (A Copy field of a struct moved on some paths only
        // is a value to the drop flag too: nothing to drop.)
        let is_plain = ro.is_plain_value()
            || never_drops;
        let end = visualization_data.row_layout.scope_ends.get(id);
        let effect = end.map(|end| end.effect);
        // For named places the destructor's own state decides the mark.
        // Replay is still used for synthetic columns and special exit
        // explanations that have no ordinary MIR storage boundary.
        let resource_hold = end.map_or(resource_hold, |end| end.has_value && !end.payload_moved && !end.payload_maybe);
        let can_be_conditional = effect.is_none_or(|h| h == ScopeDrop::Conditional);
        if effect == Some(ScopeDrop::ExitStorage) {
            return (event.print_message_with_name(&mut name) + " on each path that leaves this scope.", false);
        }
        if effect == Some(ScopeDrop::ByPath) {
            return (event.print_message_with_name(&mut name) + ". On paths that still hold a resource, it is dropped.", true);
        }
        let is_closure = ro.is_closure();
        let is_box = matches!(ro, ResourceAccessPoint::Struct(Struct { is_box: true, .. }));
        let partial = end.and_then(|end| end.partial.as_ref());
        if end.is_some_and(|end| end.effect == ScopeDrop::Storage && end.has_value) && !ro.drops_nothing() && !ro.is_closure() {
            return (hover_messages::event_dot_owner_go_out_of_scope(&name) + ". This value needs no drop glue.", false);
        }
        if let Some(parts) = end.and_then(|end| end.array.as_ref()) {
            let drops = !parts.conditional.is_empty() || !parts.left.is_empty();
            if !parts.empty_payload.is_empty() {
                let mut title = event.print_message_with_name(&mut name) + ".";
                for element in &parts.moved_payload {
                    title.push_str(&format!(" The payload of {} was moved out.", hover_messages::styled_name_html(element)));
                }
                for (element, variant) in &parts.absent_variant {
                    title.push_str(&format!(" {} holds {}.", hover_messages::styled_name_html(element), hover_messages::styled_name_html(variant)));
                }
                for element in &parts.empty_payload {
                    title.push_str(&format!(" No payload remains to drop in {}.", hover_messages::styled_name_html(element)));
                }
                for element in &parts.left {
                    title.push_str(&format!(" {} is dropped.", hover_messages::styled_name_html(element)));
                }
                for element in &parts.moved {
                    title.push_str(&format!(" {} was moved out earlier.", hover_messages::styled_name_html(element)));
                }
                for element in &parts.conditional {
                    title.push_str(&format!(" {} is dropped only on paths that still hold it.", hover_messages::styled_name_html(element)));
                }
                return (title, drops);
            }
            if drops || ro.elems_left().is_some() || ro.is_member() {
                return (hover_messages::event_dot_struct_partial_go_out_of_scope(
                    &name, &parts.moved, &parts.conditional, &parts.left, false,
                    moved_by_chain_condition(history) || maybe_moved_by_let(history), parts.may_be_refilled, false), drops);
            }
            return (event.print_message_with_name(&mut name) + &hover_messages::event_dot_no_resource_dropped_suffix(), false);
        }
        if end.is_some_and(|e| e.effect == ScopeDrop::Empty && e.has_value)
            && partial.is_some_and(|p| p.drops_resource) {
            let mut title = event.print_message_with_name(&mut name);
            title.push_str(". No payload remains to drop.");
            if partial.is_some_and(|p| p.variant_drops.iter().any(|v| v.variant == "None" &&
                matches!(v.payload, crate::facts::VariantPayload::Empty))) {
                title.push_str(" None carries no payload.");
            }
            return (title, false);
        }
        if is_closure {
            // Closure value going out of scope: only
            // surface "Its captured resources are
            // dropped" when at least one upvar was
            // move-captured. Borrow-only and capture-
            // less closures have no owned resources to
            // drop, just the closure struct itself —
            // render a plain OOS dot like a Copy type
            // with the standard "f goes out of scope"
            // message.
            // ...and only if the closure still holds them: not
            // after an `FnOnce` call used it up or it was moved
            // away (`let g = f;`); then there's nothing left to
            // drop (#271).
            let move_count = closure_move_capture_count(visualization_data, *hash);
            if move_count > 0 && end.is_some_and(|e| e.effect == ScopeDrop::Storage && e.has_value) {
                return (event.print_message_with_name(&mut name) + ". Its captures need no drop glue.", false);
            }
            let maybe_moved = move_count > 0 && effect.map_or_else(|| !resource_hold && maybe_moved_at_end(history), |h| h == ScopeDrop::Conditional);
            if move_count > 0 && (resource_hold || maybe_moved || effect == Some(ScopeDrop::Remaining)) {
                // Called (so moved) on some paths only: the drop
                // flag decides whether its captures are dropped.
                let title = if maybe_moved {
                    let (called, moved) = closure_used_up_in_branches(history);
                    hover_messages::event_dot_closure_go_out_of_scope_maybe_moved(&name, moved_by_chain_condition(history), move_count, called, moved)
                } else if effect == Some(ScopeDrop::Remaining) {
                    format!("{} goes out of scope. Any captured resources it still holds are dropped.",
                        styled_name_html(&name))
                } else {
                    hover_messages::event_dot_closure_go_out_of_scope(&name, move_count)
                };
                return (title, true);
            }
            title = event.print_message_with_name(&mut name);
            if move_count > 0 {
                title.push_str(&hover_messages::event_dot_no_resource_dropped_suffix());
            }
        } else if ro.is_manually_drop() && effect == Some(ScopeDrop::Storage) && !resource_hold {
            return (event.print_message_with_name(&mut name) + ". No resource is dropped. A ManuallyDrop never drops the value inside it automatically.", false);
        } else if (ro.is_manually_drop() || ro.is_maybe_uninit() || (ro.is_union() && !ro.union_drop())) && !never_drops
            && (visualization_data.timelines[hash].history.iter()
                .any(|(_, e)| matches!(e, Event::ManuallyDropByHand))
                || resource_hold
                || maybe_moved_at_end(history)
                || matches!(init_at_end(history), Some(InitJoin::Loop | InitJoin::Maybe { .. }))) {
            // Dropped by hand earlier: still nothing is
            // dropped here (#283). Nor when it was moved, or
            // given its value, on some paths only (#1147): a
            // ManuallyDrop has no drop glue, so no drop flag.
            // The same for a MaybeUninit and a union without a
            // Drop impl (#191, #1146).
            title = if ro.is_union() {
                hover_messages::event_dot_union_go_out_of_scope(&name)
            } else if ro.is_maybe_uninit() {
                hover_messages::event_dot_maybe_uninit_go_out_of_scope(&name)
            } else {
                hover_messages::event_dot_manually_drop_go_out_of_scope(&name)
            };
        } else if is_plain {
            // Never a resource: going out of scope drops
            // nothing, whether or not it was moved earlier.
            title = match partial {
                Some(split) =>
                    hover_messages::event_dot_partial_scope_drop(&name, split, true, false),
                _ => event.print_message_with_name(&mut name),
            };
        } else if history.iter().any(|(l, e)| l == line_number && matches!(e, Event::Move { break_out: true, .. }))
            // Or only a field of it (`break l.name`, #1280), when nothing
            // else moved part of it: all of it is dropped here on the
            // iterations that don't break.
            || (!in_branch && field_broken_out_at(visualization_data, *hash, *line_number)
                && partial.is_none())
            // Or out of a labeled block it is declared in, by one of the
            // ways the block can end (#1288).
            || (!in_branch && broken_out_of_block(visualization_data, *hash).is_some()
                && partial.is_none()) {
            // A `break` carried it out of the loop on this
            // line (#431), but only on the iteration that
            // ended the loop: every other one drops it here,
            // unless the body moved it on the way (#1156).
            // Out of a labeled block (#1288): the block can end another
            // way, which drops it. (The move's arrow may have a row of
            // its own, next to this one.)
            let block = broken_out_of_block(visualization_data, *hash);
            let other: Vec<(usize, Event)> = history.iter()
                .filter(|(l, e)| match (&block, e) {
                    (Some(_), Event::Move { break_out: true, .. } | Event::OwnerGoOutOfScope { .. }) => false,
                    (Some(_), _) => true,
                    _ => l < line_number || (l == line_number
                        && !matches!(e, Event::Move { break_out: true, .. } | Event::OwnerGoOutOfScope { .. })),
                })
                .cloned().collect();
            let held = other.iter().fold(false, |h, (_, e)| holds_value_after(h, e));
            if maybe_moved_at_end(&other) {
                return (match &block {
                        Some((alt, _)) => hover_messages::event_dot_block_break_out_go_out_of_scope_maybe_moved(&name, &ro.drop_note(), alt),
                        None => hover_messages::event_dot_break_out_go_out_of_scope_maybe_moved(&name, &ro.drop_note()),
                    }, true);
            }
            if !held {
                title = match &block {
                    Some(_) => hover_messages::event_dot_block_break_out_go_out_of_scope_moved(&name),
                    None => hover_messages::event_dot_break_out_go_out_of_scope_moved(&name),
                };
                return (title, false);
            }
            return (match &block {
                    Some((alt, Some(part))) => hover_messages::event_dot_block_break_out_part_go_out_of_scope(&name, alt, part),
                    Some((alt, None)) => hover_messages::event_dot_block_break_out_go_out_of_scope(&name, alt),
                    None => hover_messages::event_dot_break_out_go_out_of_scope(&name),
                }, true);
        } else if effect.is_none_or(|h| h == ScopeDrop::Empty) && !ro.holds_value() && init_at_end(history) == Some(InitJoin::AllMoved) {
            // Every branch that assigned it moved it (#244).
            title = hover_messages::event_dot_go_out_of_scope_init_all_moved(&name);
        } else if !ro.holds_value() && init_at_end(history) == Some(InitJoin::Loop)
            && (can_be_conditional || partial.is_some_and(|p| !p.payloads_conditional.is_empty())) {
            // A loop above may have ended before assigning
            // it (#786): the drop flag decides here.
            let title = if loop_maybe_init_prior_assignment(history) {
                hover_messages::event_dot_go_out_of_scope_maybe_kept(&name, &ro.drop_note())
            } else {
                hover_messages::event_dot_go_out_of_scope_after_loop(&name, &ro.drop_note(), loop_maybe_init_before(history))
            };
            return (title, true);
        } else if let (false, Some(InitJoin::Maybe { moved })) = (ro.holds_value() || !can_be_conditional, (match &exit_path {
            // An exit's drop in a branch, after a conditional above the
            // branch's that assigned it in some branches only (#1436).
            Some(path) => init_at_end(path),
            None if in_branch => None,
            None => init_at_end(history),
        // Mixed Box paths have their own conditional drop text below.
        }).filter(|_| !mixed_box_paths(ro, history))) {
            // Assigned in some branches of a conditional but
            // not others (#244): the drop flag decides here.
            // Whether it holds a value isn't only up to the branch
            // that ran when it may have held one as the conditional
            // started (#1036).
            return (if is_branch_temp_col(ro) {
                    // A temporary a `let` extends through a branch (#1397).
                    hover_messages::event_dot_go_out_of_scope_temp_made(&name, made_on_line(history, &visualization_data.row_lines))
                } else if end.is_some_and(|e| e.moved && e.unassigned) || init_by_maybe_held(exit_path.as_deref().unwrap_or(history)) {
                    hover_messages::event_dot_go_out_of_scope_maybe_kept(&name, &ro.drop_note())
                } else {
                    hover_messages::event_dot_go_out_of_scope_maybe_init(&name, &ro.drop_note(), moved)
                }, true);
        } else if let Some(needs_drop) = end.filter(|e| e.payload_maybe && ro.box_contents_moved()).and_then(|e| e.box_contents_need_drop) {
            let title = if effect == Some(ScopeDrop::Conditional) {
                hover_messages::event_dot_box_maybe_moved_go_out_of_scope(&name, needs_drop)
            } else if !needs_drop {
                hover_messages::event_dot_go_out_of_scope_box_plain_contents_maybe_moved(&name)
            } else {
                hover_messages::event_dot_box_payload_conditional(&name, end.is_some_and(|e| e.flag))
            };
            return (title, true);
        } else if effect.is_none_or(|e| matches!(e, ScopeDrop::Remaining | ScopeDrop::Conditional)) && mixed_box_paths(ro, history) {
            // A Box whose contents were moved out, given a new Box
            // or moved away whole on some paths only (#1220): what is
            // dropped here depends on the path.
            return (hover_messages::event_dot_box_paths_go_out_of_scope(&name), true);
        } else if partial.is_none_or(|p| p.payloads_empty.is_empty())
            && effect.map_or_else(|| !resource_hold && (maybe_moved_at_end(history)
            // An exit's drop in a branch, after a conditional above the
            // branch's that moved it on some paths (#1194).
            || in_branch && history_path(&visualization_data.timelines[hash].history, history, history.len())
                .is_some_and(|path| maybe_moved_at_end(&path))), |h| h == ScopeDrop::Conditional) {
            // Moved in some branches of a conditional but
            // not others (#179): the drop flag decides here.
            let by_cond = moved_by_chain_condition(history) || maybe_moved_by_let(history);
            let last_branch = history.iter().rev().find_map(|(_, e)| match e {
                Event::Branch { branch_history, .. } => Some(branch_history),
                _ => None,
            });
            // A Box whose contents need no drop (no drop note), moved
            // out in some branches only (#929).
            let title = if let Some(split) = partial.filter(|p| p.drops_resource
                && p.variant_drops.iter().any(|drop| matches!(drop.payload, crate::facts::schema::VariantPayload::Conditional(_))))
            {
                hover_messages::event_dot_partial_scope_drop(&name, split, false, is_box)
            } else if end.is_some_and(|end| end.unassigned && end.moved) {
                hover_messages::event_dot_go_out_of_scope_maybe_kept(&name, &ro.drop_note())
            } else if chain_new_value_lasts(history) {
                // Given a new value by a condition (#603).
                hover_messages::event_dot_go_out_of_scope_chain_new_value(&name, &ro.drop_note())
            } else if end.is_none() && ro.box_contents_moved() && ro.drop_note().is_empty()
                && last_branch.is_some_and(|b| moves_only_box_contents(b)) {
                hover_messages::event_dot_go_out_of_scope_box_plain_contents_maybe_moved(&name)
            } else if ro.box_contents_moved() {
                hover_messages::event_dot_box_paths_go_out_of_scope(&name)
            } else if end.is_some_and(|end| end.unassigned || !maybe_moved_at_end(exit_path.as_deref().unwrap_or(history))) || maybe_moved_by_kept_round(history.iter()) {
                // After a loop that moves it on some time rounds only
                // (#1019).
                hover_messages::event_dot_go_out_of_scope_maybe_kept(&name, &ro.drop_note())
            } else {
                hover_messages::event_dot_go_out_of_scope_maybe_moved(&name, &ro.drop_note(), by_cond)
            };
            return (title, true);
        } else if end.is_some_and(|e| e.payload_moved && e.effect.runs()) || end.is_none() && !resource_hold && ro.box_contents_moved() && box_contents_moved_last(&history[..index]) != Some(false) {
            // `take(*b)`: the contents are gone but the Box's
            // allocation is still freed here (#609). Not when the
            // Box was given a new value and moved away whole since
            // (#1145).
            return (hover_messages::event_dot_box_contents_moved_go_out_of_scope(&name), true);
        } else if let Some(split) = partial {
            // Partially moved struct (#207): only the
            // fields still holding a value are dropped,
            // and those moved on some paths only maybe
            // (#395).
            let title = hover_messages::event_dot_partial_scope_drop(&name, split, false, is_box);
            return (title, split.drops_resource);
        } else if effect == Some(ScopeDrop::Remaining) {
            return (event.print_message_with_name(&mut name)
                + ". Any remaining initialized parts that need dropping are dropped.", true);
        } else if end.is_some_and(|e| e.effect == ScopeDrop::Empty) || !resource_hold {
            // Resource was already moved out: note there's
            // nothing to drop here, and why, when a
            // conditional moved or dropped it in every branch
            // (#694: that join has no dot of its own).
            // A Copy value never had a resource to move or
            // drop, so its scope end reads the same whatever
            // its borrow history was (#793).
            title = event.print_message_with_name(& mut name);
            if !never_drops {
                match all_moved_at_end(history, &visualization_data.state_table, &name) {
                    Some(ScopeMove::Scrutinee) => title.push_str(
                        &hover_messages::event_dot_scrutinee_move_scope_end(&name)),
                    Some(ScopeMove::Conditional) => title.push_str(
                        &hover_messages::event_dot_go_out_of_scope_moved_through_conditional(&name)),
                    Some(ScopeMove::Branches { some_diverge }) => title.push_str(
                        &hover_messages::event_dot_go_out_of_scope_all_moved_suffix(&name, some_diverge)),
                    None => title.push_str(&hover_messages::event_dot_no_resource_dropped_suffix()),
                }
            }
        } else if ro.is_union() && ro.union_drop() && !never_drops {
            // Its own Drop impl runs; its fields are still never
            // dropped (#191, #1146).
            return (hover_messages::event_dot_union_drop_go_out_of_scope(&name), true);
        } else if ro.is_union() && !never_drops {
            // A union never drops its fields (#191).
            title = hover_messages::event_dot_union_go_out_of_scope(&name);
        } else if ro.is_manually_drop() && !never_drops {
            // Nor does a ManuallyDrop drop its contents (#283).
            title = hover_messages::event_dot_manually_drop_go_out_of_scope(&name);
        } else if never_drops {
            // Copy types have no Drop glue — going out of
            // scope just reclaims storage. Render a plain
            // dot (no drop triangle) and skip the
            // "resource is dropped" suffix.
            title = event.print_message_with_name(&mut name);
        } else {
            // Render with a down-arrow triangle inside the
            // dot to make the drop visible at a glance.
            // Triangle is inscribed in the 5px-radius
            // circle: base 1px above center (~6px wide),
            // apex 3px below center.
            let title = if ro.is_temporary() {
                // A `let`-extended temporary (#400): say why
                // it lives, and so drops, only here.
                hover_messages::event_dot_temporary_go_out_of_scope(&name)
            } else {
                let mut title = event.print_message_with_name(&mut name);
                if ro.holds_borrow() && !holder_never_borrows(visualization_data, ro) {
                    title.push_str(&hover_messages::event_dot_holder_drop_suffix());
                } else {
                    title.push_str(&hover_messages::event_dot_resource_dropped_suffix(&ro.drop_note()));
                }
                title
            };
            return (title, true);
        }
    (title, false)
}

/// At the `}` of a `loop` nothing breaks out of (#1057): whether the
/// column holds a resource that is dropped where it leaves the loop
/// (`Some(true)`), or is a reference or a value with nothing to drop
/// (`Some(false)`). `None` when an owner may not hold its resource there
/// (moved before the loop, on some paths, or in part): nothing is drawn,
/// as after a conditional whose every branch leaves (#515, #1043).
fn loop_runs_on_owns(line_number: &usize, hash: &u64, history: &[(usize, Event)], resource_hold: bool, visualization_data: &VisualizationData) -> Option<bool> {
    let ro = &visualization_data.timelines[hash].resource_access_point;
    if ro.is_ref() || ro.never_drops() || ro.is_plain_value() || ro.is_union() || ro.is_manually_drop()
        || (ro.is_closure() && closure_move_capture_count(visualization_data, *hash) == 0)
    {
        return Some(false);
    }
    let partial = matches!(ro, ResourceAccessPoint::Struct(_)) && (|| {
        let function = *visualization_data.fn_source_lines.get(hash)?;
        let row = rv_attrs::line_of(*line_number, &visualization_data.row_lines)?;
        visualization_data.state_table.incoming_cell(function, ro.name(), row)
    })().is_none_or(|cell| cell.init != crate::facts::table::Init::Own
        || cell.parts.as_ref().is_some_and(|parts| !parts.moved.is_empty() || !parts.maybe.is_empty()));
    let whole = resource_hold && !maybe_moved_at_end(history) && !partial;
    whole.then_some(true)
}

/// A readable interval that fits entirely on the borrow's end dot.
fn one_dot_holder_state(history: &[(usize, Event)], i: usize, rap: &ResourceAccessPoint,
    layout: &crate::event_order::RowLayout) -> Option<State> {
    let (row, event) = &history[i];
    let (Event::StaticDie { readable, .. } | Event::MutableDie { readable, .. }) = event else { return None };
    if !rap.is_ref() { return None; }
    let here = event_dot_y(history, i);
    let mut on_dot = history[..i].iter().enumerate().rev()
        .take_while(|(j, (r, _))| r == row && event_dot_y(history, *j) == here)
        .map(|(_, (_, e))| e);
    if on_dot.clone().next().is_some_and(|e| layout.restored.contains(&(e.get_id(), event.get_id()))) {
        return Some(State::FullPrivilege { s: LineState::Full });
    }
    let acquired = on_dot.find(|e| matches!(e, Event::StaticDie { .. } | Event::MutableDie { .. } | Event::Move { .. }
        | Event::StaticBorrow { .. } | Event::MutableBorrow { .. } | Event::Copy { is: ResourceTy::Value(_), .. }
        | Event::Acquire { is: ResourceTy::Value(_), .. } | Event::StaticLend { is: ResourceTy::DerefField(..), .. }
        | Event::MutableLend { is: ResourceTy::DerefField(..), .. }));
    let same_generation = acquired.is_some_and(|start|
        layout.field_generations.get(&start.get_id()) == layout.field_generations.get(&event.get_id()));
    (*readable && same_generation && matches!(acquired, Some(Event::StaticBorrow { .. } | Event::MutableBorrow { .. }
        | Event::Copy { .. } | Event::Acquire { .. }))).then(|| {
        if rap.is_mutref() { State::FullPrivilege { s: LineState::Full } }
        else { State::PartialPrivilege { s: LineState::Full } }
    })
}

/// Introduce the closure capture first, then explain that the previous
/// binding's borrow had already ended. Both effects must share a dot and
/// MIR must put the release before the capture.
fn borrow_before_capture<'a>(history: &'a [(usize, Event)], i: usize, layout: &crate::event_order::RowLayout) -> Option<&'a String> {
    let Event::StaticLend { to, id, .. } = &history[i].1 else { return None };
    let previous = i.checked_sub(1)?;
    let Event::StaticReacquire { from: ResourceTy::Value(holder), id: end, still_borrowed_by, .. } = &history[previous].1 else { return None };
    (to.is_closure() && holder.is_ref() && still_borrowed_by.is_empty()
        && layout.closure_captures.values().any(|ids| ids.contains(id))
        && layout.precedes(*end, *id) && event_dot_y(history, previous) == event_dot_y(history, i)).then_some(holder.name())
}

fn render_dot(
    hash: &u64,
    history: &Vec<(usize, Event)>,
    timeline_data: &TimelineColumnData,
    visualization_data: &VisualizationData,
    resource_hold: bool,
    dots: &mut DotLayer,
    held_before: bool,
) {
    // Whether this column already holds a value before the current
    // event: an Acquire / Copy into a Copy-typed column that already
    // holds one is an assignment (`n += 1`, `p.x = 5`), not an
    // initialization (#178).
    let mut held = held_before;
    // Same-named variables of sibling blocks share a column and each
    // goes out of scope at its own `}` (#589): whether one still holds
    // its resource there is whether it still holds a value just before
    // that event (a `drop(s)` in the first block is not undone by the
    // second block's own `s`), not the column's penultimate state.
    let scope_ends = history.iter().filter(|(_, e)| matches!(e, Event::OwnerGoOutOfScope { .. })).count();
    let resource_hold_of_column = resource_hold;
    // Whether the column's value was moved away earlier in `history`
    // (and nothing was put back): a conditional after that finds it
    // empty, so no line runs into or out of its split (#694).
    let mut moved_out = false;
    // Several `break` values reaching a loop's receiver on one line
    // (#561): only one of them did, so a later one isn't an assignment
    // over an earlier one. (line, whether it held a value before them)
    let mut before_breaks: Option<(usize, bool)> = None;
    for (i, (line_number, event)) in history.iter().enumerate() {
        let resource_hold = if scope_ends > 1 && matches!(event, Event::OwnerGoOutOfScope { .. }) {
            held
        } else {
            resource_hold_of_column
        };
        let moved_at_event = moved_out;
        let mut held_at_event = held;
        if let Event::Acquire { break_alt: Some(_), .. } | Event::Copy { break_alt: Some(_), .. } = event {
            // (On the source line, not the row: a `break [&s]`'s borrow
            // arrow may push the next `break`'s value a row down, #1435.)
            let src = |r: usize| rv_attrs::line_of(r, &visualization_data.row_lines).unwrap_or(r);
            match before_breaks {
                Some((line, before)) if src(line) == src(*line_number) => held_at_event = before,
                _ => before_breaks = Some((*line_number, held)),
            }
        }
        let event_y = event_dot_y(history, i);
        held = holds_value_after(held, event);
        moved_out = match event {
            e if e.is_box_payload_move() => moved_out && !held,
            // A slice pattern that takes some of an array's elements
            // leaves the rest in it: a conditional after it splits the
            // column at a dot as for any live value (#1302).
            Event::Move { is: ResourceTy::Index(source, place, _), .. }
                if !place.starts_with('*') && source.elems_left().is_some_and(|el| !el.empties(place)) =>
                moved_out && !held,
            Event::Move { .. } | Event::LoopExitMove { definite: true, .. } => true,
            _ => moved_out && !held,
        };
        // Closure-capture per-upvar dots on the closure's own
        // timeline are suppressed here: the closure's Bind-Acquire
        // dot at the same line carries a combined tooltip listing
        // every capture (see the Anonymous-from arm below). The
        // per-capture events stay in `history` because the arrow
        // renderer traverses them to find the closure column for
        // each capture arrow's endpoint.
        let is_closure_capture_event = match event {
            Event::Acquire { from: ResourceTy::Anonymous, .. } => false,
            // `let g = keep(|| s.len());`: the closure arrives from the
            // call that returned it (#686); its dot lists the captures.
            Event::Acquire { from: ResourceTy::Value(f), .. }
            | Event::Copy { from: ResourceTy::Value(f), .. } if f.is_fn() => false,
            // `let h = g;` moves or copies the closure g; h's dot
            // says so (#704).
            Event::Acquire { id, .. } | Event::Copy { id, .. }
                if closure_value_event(visualization_data, *id).is_some() => false,
            Event::Acquire { is, .. }
            | Event::Copy { is, .. }
            | Event::StaticBorrow { is, .. }
            | Event::MutableBorrow { is, .. } => is.is_closure(),
            _ => false,
        };
        if is_closure_capture_event {
            continue;
        }
        //matching the event
        match event {
            Event::RefDie { .. } => {
                continue;
            }
            Event::Branch { branch_history, .. } if only_loop_iterator_drops(branch_history) => {
                // The column was moved into a `for` loop as its iterator
                // before the conditional, and only that iterator's drop
                // at a `return` is in it (#482): the variable itself
                // takes no part, so no split or join dot.
                for branch in branch_history.iter() {
                    render_dot(hash, &branch.e_data, &branch.t_data, visualization_data, false, dots, false);
                }
                continue;
            }
            Event::Branch { is, branch_history, ty, split_point, merge_point, .. } => {
                render_branch_dots(hash, history, event, line_number, is, branch_history, ty, split_point, merge_point, moved_at_event, held_at_event, timeline_data, visualization_data, dots);
                continue;
            }
            _ => {} //do nothing
        }
        
        let mut data = EventDotData {
            hash: *hash as u64,
            dot_x: timeline_data.x_val,
            dot_y: event_y,
            // default value if print_message_with_name() fails
            title: hover_messages::event_dot_unknown_owner()
        };
        if let Some(mut name) = visualization_data.get_name_from_hash(hash) {
            if matches!(event, Event::OwnerGoOutOfScope { .. } | Event::RefGoOutOfScope { .. })
                && visualization_data.row_layout.terminal_calls.contains(&event.get_id()) {
                data.title = format!("The timeline for {} ends here because this call never returns.", hover_messages::styled_name_html(&name));
                append_dot(data, timeline_data, dots, None, rv_attrs::event_name(event), *line_number);
                continue;
            }
            match event {
                Event::OwnerGoOutOfScope { early: Some(EarlyReturns { process_exit: Some(exit), .. }), .. } => {
                    // At a `process::exit`: the process ends, no
                    // destructor runs (#1172). A plain dot.
                    let owns = loop_runs_on_owns(line_number, hash, history, resource_hold, visualization_data);
                    data.title = hover_messages::event_dot_process_exit(&name, owns, exit);
                    append_dot(data, timeline_data, dots, None, rv_attrs::event_name(event), *line_number);
                    continue;
                }
                Event::OwnerGoOutOfScope { early: Some(EarlyReturns { runs_on: Some(on), .. }), .. }
                | Event::RefGoOutOfScope { early: Some(EarlyReturns { runs_on: Some(on), .. }), .. } => {
                    // At the `}` of a `loop` nothing breaks out of: still
                    // alive while the loop keeps running (#1057). A plain
                    // dot: nothing is dropped here.
                    if let Some(owns) = loop_runs_on_owns(line_number, hash, history, resource_hold, visualization_data) {
                        data.title = hover_messages::event_dot_loop_runs_on(&name, owns, on.exits, on.moved);
                        append_dot(data, timeline_data, dots, None, rv_attrs::event_name(event), *line_number);
                    }
                    continue;
                }
                Event::OwnerGoOutOfScope { early, .. } => {
                let (mut title, dropped) = scope_end_message(hash, history, i, event, line_number, resource_hold, name, visualization_data);
                let scope = visualization_data.row_layout.scope_ends.get(&event.get_id());
                if scope.is_some_and(|end| end.flag) && !title.contains("drop flag") {
                    title.push_str(" Rust tracks this with a runtime drop flag.");
                } else if scope.is_some_and(|end| end.flag_on_some_paths) {
                    title.push_str(" On some paths Rust tracks this with a runtime drop flag.");
                }
                if dropped {
                    dots.add_event(*hash, timeline_data.x_val, event_y, timeline_data, title, true, None,
                        rv_attrs::event_name(event), *line_number);
                    // A drop: `return`s in a loop body may drop it
                    // first (#1020).
                    if let Some(e) = early {
                        dots.edit_last_line(*hash, timeline_data.x_val, event_y,
                            |t| hover_messages::event_dot_scope_end_after_loop_return(t, e, true));
                    }
                    continue;
                }
                data.title = title;
                },
                // Reassignment-drop renders the same down-arrow dot as
                // OwnerGoOutOfScope (resource is dropped at this line),
                // but the owner stays in scope — there's always a
                // resource to drop here, so no `if !resource_hold` branch.
                Event::ManuallyDropByHand => {
                    let cx = timeline_data.x_val;
                    let cy = event_y;
                    let title = event.print_message_with_name(&mut name);
                    let drop_data = DropDotData {
                        hash: *hash as u64,
                        dot_x: cx,
                        dot_y: cy,
                        title,
                        p1x: cx - 3, p1y: cy - 1,
                        p2x: cx + 3, p2y: cy - 1,
                        p3x: cx,     p3y: cy + 3,
                    };
                    append_drop_dot(drop_data, timeline_data, dots, rv_attrs::event_name(event), *line_number);
                    continue;
                },
                Event::OwnerDropAtReassign { skipped: true, .. } => {
                    data.title = hover_messages::event_dot_skipped_reassignment(&name);
                }
                Event::OwnerDropAtReassign { .. } | Event::OwnerDropIfTryReturns { .. } | Event::OwnerDropUnlessReturned { .. } | Event::TemporaryDrop { .. } | Event::TupleTempDrop { .. } | Event::LoopIteratorDrop { .. } => {
                    let cx = timeline_data.x_val;
                    let cy = event_y;
                    let try_drop = match event {
                        Event::OwnerDropIfTryReturns { local: Some(local), span } =>
                            visualization_data.fn_source_lines.get(hash).and_then(|&function|
                                visualization_data.state_table.try_drop(function, &name, *local, *span)),
                        _ => None,
                    };
                    if matches!(try_drop, Some(("nothing_to_drop", _))) {
                        continue;
                    }
                    let try_variant_drop = (|| {
                        let Event::OwnerDropIfTryReturns { local: Some(local), span } = event else { return None; };
                        let function = *visualization_data.fn_source_lines.get(hash)?;
                        let row = rv_attrs::line_of(*line_number, &visualization_data.row_lines)?;
                        visualization_data.state_table.partial_drop(function, &name, row, *local, Some(*span))
                            .filter(|split| !split.variant_drops.is_empty() && split.drops_resource)
                    })();
                    // A reassignment, or the drop at a `?`'s early return,
                    // says what else goes with it (#373, #542).
                    let note = if matches!(event, Event::OwnerDropAtReassign { parts: None, payload: false, .. } | Event::OwnerDropIfTryReturns { .. } | Event::OwnerDropUnlessReturned { .. }) {
                        visualization_data.timelines[hash].resource_access_point.drop_note()
                    } else { String::new() };
                    // `*b = …` drops the Box's old payload, not the Box
                    // (#893): the dot names `*b`, with no note (the Box's
                    // "including the (String, i32) it holds" is about
                    // dropping the Box).
                    let mut name = match event {
                        Event::OwnerDropAtReassign { payload: true, .. } => format!("*{}", name),
                        _ => name.clone(),
                    };
                    // The same assignment's Acquire can precede its drop dot.
                    // It describes the new Box, not the value being overwritten.
                    let box_paths = matches!(event, Event::OwnerDropAtReassign { parts: None, payload: false, .. })
                        && mixed_box_paths(&visualization_data.timelines[hash].resource_access_point,
                            &history[..i].iter().filter(|(l, _)| l < line_number).cloned().collect::<Vec<_>>());
                    let conditional_drop = matches!(event, Event::OwnerDropAtReassign { .. }).then(||
                        visualization_data.fn_source_lines.get(hash).zip(rv_attrs::line_of(*line_number, &visualization_data.row_lines))
                            .and_then(|(&f, src)| visualization_data.state_table.conditional_reassignment_drop(f, &name, src))).flatten();
                    let mut title = if matches!(event, Event::OwnerDropAtReassign { payload: true, parts: None, maybe_moved: false, maybe_init: None, contents_gone: false, .. }) {
                        hover_messages::event_dot_replace_at_place(&name)
                    } else if box_paths {
                        // A Box whose contents were moved out, given a
                        // new Box or moved away on some paths only
                        // (#1220): what the old value drops depends on
                        // the path.
                        hover_messages::event_dot_owner_drop_at_reassign_box_paths(&name)
                    } else if conditional_drop == Some("drops_if_init") && !matches!(event, Event::OwnerDropAtReassign { maybe_init: Some(InitBy::Loop), .. }) {
                        hover_messages::event_dot_owner_drop_at_reassign_maybe_kept(&name, &note)
                    } else if matches!(event, Event::OwnerDropAtReassign { maybe_init: Some(InitBy::Branch | InitBy::Loop), .. })
                        // (Before the new value's Acquire on this line.)
                        && init_by_maybe_held(history[..i].iter().filter(|(l, _)| l < line_number)) {
                        // After a conditional that assigns a `let s;`
                        // local in some branches only, where it may
                        // have held a value as it started (#1036).
                        hover_messages::event_dot_owner_drop_at_reassign_maybe_kept(&name, &note)
                    } else if let Event::OwnerDropAtReassign { maybe_init: Some(InitBy::Branch), maybe_moved, .. } = event {
                        // A `let s;` local only some branches above
                        // assigned (#244), and maybe moved after that.
                        hover_messages::event_dot_owner_drop_at_reassign_maybe_init(&name, &note, *maybe_moved)
                    } else if let Event::OwnerDropAtReassign { maybe_init: Some(InitBy::Loop), .. } = event {
                        // A loop above may have ended before assigning
                        // it (#786), maybe after a branch moved it
                        // before the loop (#1012).
                        let ro = &visualization_data.timelines[hash].resource_access_point;
                        let drop = visualization_data.fn_source_lines.get(hash).and_then(|&function| {
                            let row = rv_attrs::line_of(*line_number, &visualization_data.row_lines)?;
                            visualization_data.state_table.reassignment_drop(function, ro.name(), row, ro.owner_local())
                        });
                        if drop == Some("drops") {
                            let mut title = hover_messages::event_dot_owner_drop_at_reassign(&name, &note);
                            if visualization_data.fn_source_lines.get(hash).zip(rv_attrs::line_of(*line_number, &visualization_data.row_lines))
                                .is_some_and(|(&function, row)| visualization_data.state_table
                                    .uses_drop_flag(function, ro.name(), row, ro.owner_local(), true)) {
                                title.push('.');
                                title.push_str(hover_messages::runtime_drop_flag());
                            }
                            title
                        } else {
                            if loop_maybe_init_prior_assignment(&history[..i]) {
                                hover_messages::event_dot_owner_drop_at_reassign_maybe_kept(&name, &note)
                            } else {
                                hover_messages::event_dot_owner_drop_at_reassign_after_loop(&name, &note, loop_maybe_init_before(&history[..i]))
                            }
                        }
                    } else if let Event::OwnerDropAtReassign { maybe_init: Some(InitBy::EarlierIteration), maybe_moved: true, by_cond, .. } = event {
                        // The first thing a loop's body does to it,
                        // which only may hold a value as the loop
                        // starts (#1012).
                        hover_messages::event_dot_owner_drop_at_reassign_maybe_entry(&name, &note, *by_cond
                            || maybe_moved_by_let(history[..i].iter().filter(|(l, _)| l < line_number)))
                    } else if let Event::OwnerDropAtReassign { maybe_init: Some(InitBy::OuterLoop { moves }), by_cond, .. } = event {
                        // The same in a loop inside another (#1019): a
                        // run of it may follow an earlier one.
                        hover_messages::event_dot_owner_drop_at_reassign_outer_loop(&name, &note, *by_cond
                            || maybe_moved_by_let(history[..i].iter().filter(|(l, _)| l < line_number)), *moves)
                    } else if let Event::OwnerDropAtReassign { maybe_init: Some(InitBy::DropFlag), .. } = event {
                        // rustc says it only may hold a value here,
                        // though nothing drawn above says so (#1027).
                        hover_messages::event_dot_owner_drop_at_reassign_maybe_kept(&name, &note)
                    } else if matches!(event, Event::OwnerDropAtReassign { maybe_moved: false, maybe_init: None, parts: None, payload: false, contents_gone: true, .. }) {
                        // The old Box no longer owns the moved value (#1145).
                        hover_messages::event_dot_owner_drop_at_reassign_box_contents_moved(&name)
                    } else if let Event::OwnerDropAtReassign { maybe_init: Some(InitBy::EarlierIteration), .. } = event {
                        // Its first assignment, in a loop body (#786).
                        let ro = &visualization_data.timelines[hash].resource_access_point;
                        let flag = visualization_data.fn_source_lines.get(hash)
                            .zip(rv_attrs::line_of(*line_number, &visualization_data.row_lines))
                            .is_some_and(|(&function, row)| {
                                let table = &visualization_data.state_table;
                                table.uses_drop_flag(function, ro.name(), row, ro.owner_local(), true)
                                    || (ro.is_member() && ro.owner_local().is_some_and(|local|
                                        visualization_data.timelines.get(&ro.get_owner()).is_some_and(|owner|
                                            table.uses_whole_reassignment_drop_flag(
                                                function, owner.resource_access_point.name(), row, local))))
                            });
                        hover_messages::event_dot_init_drops_earlier_iteration(&name, flag)
                    } else if matches!(event, Event::OwnerDropAtReassign { maybe_moved: true, loop_head: true, parts: None, payload: false, .. }) {
                        // Maybe-moved only because the loop can come
                        // back round after a move further down (#992).
                        hover_messages::event_dot_owner_drop_at_reassign_loop_head(&name, &note)
                    } else if matches!(event, Event::OwnerDropAtReassign { maybe_moved: true, parts: None, payload: false, .. })
                        // (Before the new value's Acquire on this line.)
                        && maybe_moved_by_kept_round(history[..i].iter().filter(|(l, _)| l < line_number)) {
                        // After a loop that moves it on some time
                        // rounds only (#1019).
                        hover_messages::event_dot_owner_drop_at_reassign_maybe_kept(&name, &note)
                    } else if matches!(event, Event::OwnerDropAtReassign { maybe_moved: true, by_cond: false, .. })
                        // (Before the new value's Acquire on this line.)
                        && maybe_moved_by_let(history[..i].iter().filter(|(l, _)| l < line_number)) {
                        // A `let` or-pattern, not a branch, may have moved it (#597).
                        hover_messages::event_dot_owner_drop_at_reassign_maybe_moved(&name, &note, true)
                    } else if matches!(event, Event::OwnerDropAtReassign { maybe_moved: true, parts: None, payload: false, .. })
                        && chain_new_value_lasts(history[..i].iter().filter(|(l, _)| l < line_number)) {
                        // A condition may have given it a new value (#603).
                        hover_messages::event_dot_owner_drop_at_reassign_maybe_kept(&name, &note)
                    } else if let Event::OwnerDropAtReassign { maybe_moved: true, by_cond, .. } = event {
                        // A Box whose contents only were moved out in
                        // some branches (#921).
                        let rap = &visualization_data.timelines[hash].resource_access_point;
                        let last_branch = history[..i].iter().rev().find_map(|(_, e)| match e {
                            Event::Branch { branch_history, .. } => Some(branch_history),
                            _ => None,
                        });
                        if rap.box_contents_moved() && last_branch.is_some_and(|b| moves_only_box_contents(b)) {
                            hover_messages::event_dot_owner_drop_at_reassign_box_paths(&name)
                        } else {
                            hover_messages::event_dot_owner_drop_at_reassign_maybe_moved(&name, &note, *by_cond)
                        }
                    } else if let (Event::TemporaryDrop { ty: None, when, .. }, Some(made_at)) = (event, branch_temp_maybe_made(visualization_data, hash, history, i)) {
                        // A temporary a `let` extends through a branch,
                        // dropped by an exit after the join (#1436): only
                        // if the branch that made it ran.
                        hover_messages::event_dot_extended_temporary_drop_made(&name, when, made_at)
                    } else if let (Event::OwnerDropIfTryReturns { .. }, Some(made_at)) = (event, branch_temp_maybe_made(visualization_data, hash, history, i)) {
                        hover_messages::event_dot_temp_drop_if_try_returns_made(&name, made_at)
                    } else if let Some(split) = try_variant_drop {
                        let boxed = matches!(visualization_data.timelines[hash].resource_access_point, ResourceAccessPoint::Struct(Struct { is_box: true, .. }));
                        let scope_end = hover_messages::event_dot_partial_scope_drop(&name, split, false, boxed);
                        hover_messages::event_dot_owner_drop_if_try_returns_partial(&name, &scope_end)
                    } else if let Some(split) = matches!(event, Event::OwnerDropIfTryReturns { .. })
                        .then(|| try_partial_move_split(visualization_data, *hash as u64, *line_number)).flatten() {
                        // A struct some fields of which were moved out
                        // before the `?` (#1184): only what is left of it
                        // is dropped if it returns early, and nothing
                        // when every field that needs dropping is gone
                        // (no dot).
                        if split.dropped.is_empty() && split.maybe_moved.is_empty() {
                            continue;
                        }
                        let boxed = matches!(visualization_data.timelines[hash].resource_access_point, ResourceAccessPoint::Struct(Struct { is_box: true, .. }));
                        let scope_end = hover_messages::event_dot_struct_partial_go_out_of_scope(&name, &split.moved, &split.maybe_moved, &split.dropped, false, split.by_cond, false, boxed);
                        hover_messages::event_dot_owner_drop_if_try_returns_partial(&name, &scope_end)
                    } else if matches!(event, Event::OwnerDropIfTryReturns { .. })
                        && visualization_data.timelines[hash].resource_access_point.box_contents_moved() {
                        hover_messages::event_dot_box_drop_if_try_returns(&name)
                    } else if let Some(("drops_if_init", only_assigned)) = try_drop {
                        hover_messages::event_dot_owner_drop_if_try_returns_maybe(&name, &note, Some(only_assigned))
                    } else if matches!(event, Event::OwnerDropIfTryReturns { .. }) && try_drop.is_none() {
                        hover_messages::event_dot_owner_drop_if_try_returns_maybe(&name, &note, None)
                    } else if let Event::OwnerDropUnlessReturned { is_break } = event {
                        // A ManuallyDrop has nothing to drop either (#1137).
                        let value = visualization_data.timelines[hash].resource_access_point.drops_nothing();
                        hover_messages::event_dot_owner_drop_unless_returned(&name, value, &note, *is_break)
                    } else if let Some(part) = matches!(event, Event::OwnerDropIfTryReturns { .. }).then(|| try_assigned_part(visualization_data, *hash as u64, *line_number)).flatten() {
                        // `p.f = f()?;` (#1151): on success the assignment
                        // gives a part of p, on its own column, a new value.
                        hover_messages::event_dot_owner_drop_if_try_returns_part(&name, &note, &part)
                    } else if note.is_empty() {
                        event.print_message_with_name(&mut name)
                    } else if matches!(event, Event::OwnerDropIfTryReturns { .. }) {
                        hover_messages::event_dot_owner_drop_if_try_returns(&name, &note)
                    } else {
                        hover_messages::event_dot_owner_drop_at_reassign(&name, &note)
                    };
                    if let Event::OwnerDropIfTryReturns { span, .. } = event {
                        let sites: BTreeSet<_> = history.iter().filter_map(|(r, e)| match e {
                            Event::OwnerDropIfTryReturns { span, .. } if r == line_number => Some(span.lo()),
                            _ => None,
                        }).collect();
                        if sites.len() > 1 {
                            if let Some(rank) = sites.iter().position(|&site| site == span.lo()) {
                                let label = match (rank, sites.len()) {
                                    (0, 2) => "the first `?`".to_owned(),
                                    (1, 2) => "the second `?`".to_owned(),
                                    _ => format!("question mark {} on this line", rank + 1),
                                };
                                title = title.replace("the `?` here", &label).replace("the `?` succeeds", &format!("{label} succeeds"));
                                // An assignment and a second residual can share
                                // the dot, so limit the success claim to this `?`.
                                title = title.replace(&format!("nothing happens to {} here.", hover_messages::styled_name_html(&name)),
                                    &format!("it does not drop {}'s resource.", hover_messages::styled_name_html(&name)));
                            }
                        }
                    }
                    if conditional_drop.is_some() {
                        title = format!("{}. This assignment runs on only some paths through this expression.", title.trim_end_matches('.'));
                    }
                    // A struct whose parts were all moved out before it
                    // is overwritten drops nothing: a plain dot that says
                    // so, no drop triangle (#1138).
                    let drops_nothing = matches!(event, Event::OwnerDropAtReassign { parts: Some(p), .. }
                        if p.dropped.is_empty() && p.maybe_moved.is_empty());
                    if drops_nothing {
                        data.title = title;
                    } else {
                        let drop_data = DropDotData {
                            hash: *hash as u64,
                            dot_x: cx,
                            dot_y: cy,
                            title,
                            p1x: cx - 3, p1y: cy - 1,
                            p2x: cx + 3, p2y: cy - 1,
                            p3x: cx,     p3y: cy + 3,
                        };
                        append_drop_dot(drop_data, timeline_data, dots, rv_attrs::event_name(event), *line_number);
                        continue;
                    }
                },
                Event::LoopMaybeInit { nested: true, .. } => {
                    // A loop inside another, every run of which may
                    // start with it holding nothing (#1032).
                    let rap = &visualization_data.timelines[hash].resource_access_point;
                    data.title = hover_messages::event_dot_loop_maybe_init_nested(
                        &name, rap.drops_nothing(), rap.is_mut());
                }
                Event::LoopMaybeInit { before_loop, prior_assignment, always_assigned, .. } => {
                    // A loop above may have ended before assigning it
                    // (#786): like a conditional's maybe-initialized
                    // join, say when its value, if any, is dropped.
                    let rap = &visualization_data.timelines[hash].resource_access_point;
                    let later = &history[i + 1..];
                    let reassigned = if later.iter().any(|(_, e)| matches!(e, Event::OwnerDropAtReassign { maybe_init: Some(InitBy::Branch | InitBy::Loop), .. })) {
                        Some(true)
                    } else if later.iter().any(|(_, e)| reassigns_maybe_init(e)) {
                        // Only in a branch of a later conditional.
                        Some(false)
                    } else {
                        None
                    };
                    let assigned = visualization_data.fn_source_lines.get(hash).and_then(|&function| {
                        let row = rv_attrs::line_of(*line_number, &visualization_data.row_lines)?;
                        visualization_data.state_table.segment_cell(function, rap.name(), row)
                    }).is_some_and(|cell| cell.always_assigned) || *always_assigned;
                    data.title = if assigned {
                        hover_messages::event_dot_runtime_assigned(rap.name(), !rap.drops_nothing(), rap.is_mut(), reassigned, &rap.drop_note())
                    } else {
                        hover_messages::event_dot_loop_maybe_init(
                            &name, rap.drops_nothing(), rap.is_mut(), reassigned, *before_loop, *prior_assignment)
                    };
                }
                Event::LoopExitKept { reassigned } => {
                    let rap = &visualization_data.timelines[hash].resource_access_point;
                    data.title = hover_messages::event_dot_loop_exit_kept(&name, *reassigned, rap.holds_value());
                }
                _ => {
                    // Closure Bind-Acquire: aggregate every capture
                    // event landing at the same line into a single
                    // tooltip listing all captured upvars, since the
                    // per-capture target-side dots are suppressed
                    // (they'd stack on this one and the topmost
                    // tooltip would mask the rest).
                    let rap = &visualization_data.timelines[hash].resource_access_point;
                    // (A `let` literal's temporary takes each element's
                    // value in turn: not a reassignment, #1363.)
                    let copyable_assign = held_at_event && rap.holds_value() && !rap.is_closure() && !rap.is_let_literal_temporary();
                    // A reference variable that already holds a
                    // reference is reassigned (`r = s;`), not
                    // initialized: the reference is copied (`&T`) or
                    // moved (`&mut T`) into it (#414). So is one whose
                    // reference was moved out of it before (`(m1, m2) =
                    // (m2, m1)` moves m1's out first, #1343).
                    let ref_assign = (held_at_event || moved_at_event) && rap.is_ref();
                    let ref_assign_title = match (ref_assign, event) {
                        (true, Event::Copy { from: ResourceTy::Value(f), is: ResourceTy::Value(_), .. }) =>
                            hover_messages::event_dot_ref_assign(&name, f, true, rap.is_mutref()),
                        (true, Event::Acquire { from: ResourceTy::Value(f), is: ResourceTy::Value(_), .. }) =>
                            hover_messages::event_dot_ref_assign(&name, f, false, rap.is_mutref()),
                        _ => None,
                    };
                    let replacement = match (history[..i].last(), event) {
                        (Some((_, Event::ClosureCall { fates: None, .. })), Event::Acquire { from: ResourceTy::Value(source), .. })
                            if rap.holds_borrow() && !source.is_fn() && !source.is_ref() =>
                            Some(hover_messages::event_dot_replaced_callable(&name, source.name())),
                        _ => None,
                    };
                    if let Some(title) = replacement {
                        data.title = title;
                    } else if let Some(title) = union_write_title(event, rap, &name, visualization_data, *hash) {
                        data.title = title;
                    } else if let Some(title) = ref_assign_title {
                        data.title = title;
                    } else if let (true, Event::Acquire { is: ResourceTy::Value(_), .. }) = (copyable_assign, event) {
                        data.title = hover_messages::event_dot_copyable_assign(&name);
                    } else if let (true, Event::Copy { from, is: ResourceTy::Value(_), of, .. }) = (copyable_assign, event) {
                        // `y = Fruit::Apple(z);` / `opt = Some(x + 1);`:
                        // y gets a new value, not a copy of z or of the
                        // constructor, like `y = make();` (#866).
                        data.title = match of {
                            // (A `&` pattern can't be assigned to; a new
                            // value, if it ever is, #874.)
                            CopyOf::IntoCtor(_) | CopyOf::Pointee { .. } => hover_messages::event_dot_copyable_assign(&name),
                            _ if from.extract_rap().is_some_and(|f| f.is_fn()) => hover_messages::event_dot_copyable_assign(&name),
                            CopyOf::Inside => hover_messages::event_dot_copyable_assign_copy_inside(&name, &from.name()),
                            CopyOf::Const(c) => hover_messages::event_dot_assign_copy_of_const(&name, c),
                            CopyOf::Whole => hover_messages::event_dot_copyable_assign_copy(&name, &from.name()),
                        };
                    } else if let Some((func, true)) = match event {
                        Event::Acquire { from: ResourceTy::Value(f), is, .. }
                        | Event::Copy { from: ResourceTy::Value(f), is, .. } if f.is_fn() => Some((f.name().to_owned(), is.is_closure())),
                        _ => None,
                    } {
                        let captures = collect_closure_captures(visualization_data, *hash, *line_number, event.get_id());
                        data.title = hover_messages::event_dot_closure_returned(&name, &func, &captures);
                    } else if let Some(title) = closure_value_title(event, &name, visualization_data) {
                        data.title = title;
                    } else if let Event::Acquire { from: ResourceTy::Anonymous, is, .. } = event {
                        if is.is_closure() {
                            let captures = collect_closure_captures(visualization_data, *hash, *line_number, event.get_id());
                            data.title = hover_messages::event_dot_closure_bind_with_captures(&name, &captures);
                        } else {
                            data.title = event.print_message_with_name(& mut name);
                        }
                    } else if let Some(title) = reborrow_kept_mut_loan_title(event, &history[..i], &name, rap) {
                        data.title = title;
                    } else if let Some(title) = field_lenders_dot_title(event, &name, visualization_data) {
                        data.title = title;
                    } else if let Some(title) = ends_several_loans_title(event, history, *line_number, &mut name, visualization_data, rap) {
                        data.title = title;
                    } else if let Some(title) = part_lend_title(event, visualization_data) {
                        data.title = title;
                    } else if let Some(title) = loop_head_lend_title(event, &name, rap, visualization_data) {
                        data.title = title;
                    } else if let Some(title) = loop_iterator_ref_title(event, *line_number, rap, visualization_data) {
                        data.title = title;
                    } else if let Some(title) = kept_closure_lend_title(event, &name, visualization_data) {
                        data.title = title;
                    } else if let Some(title) = closure_reborrow_lend_title(event, visualization_data) {
                        data.title = title;
                    } else if let Some(title) = deep_lend_title(event, visualization_data) {
                        data.title = title;
                    } else {
                        let may_be_none = matches!(event, Event::StaticBorrow { .. } | Event::MutableBorrow { .. })
                            && option_ref_copy(visualization_data, hash, &name, Some(*line_number)).is_some()
                            && borrow_result_may_be_none(&visualization_data.external_events, event.get_id());
                        data.title = event.print_message_with_option_type(&mut name, may_be_none);
                    }
                    if let (Event::Acquire { .. }, Some((slot, target))) =
                        (event, visualization_data.row_layout.retained_references.get(&event.get_id())) {
                        data.title.push_str(&format!(". {} keeps its reference to {}", styled_name_html(slot), styled_name_html(target)));
                    }
                    // One of several `break` values reaching the loop's
                    // receiver: it happened only if that `break` ended
                    // the loop (#561).
                    match event {
                        Event::Move { to, is, break_alt: Some(alt), .. } => {
                            let what = match is.extract_rap() {
                                Some(r) if r.is_ref() => "reference",
                                Some(r) if r.holds_value() => "value",
                                _ => "resource",
                            };
                            data.title = if matches!(to, ResourceTy::Caller) {
                                // The loop is what the function returns (#1171).
                                hover_messages::event_dot_break_alt_move_to_caller(&name, alt, what)
                            } else {
                                hover_messages::event_dot_break_alt_move(&name, &to.name(), alt, what)
                            };
                        }
                        Event::Duplicate { break_alt: Some(alt), .. } => {
                            if let Some(skipped) = hover_messages::skipped_break_alt(alt) {
                                data.title = skipped;
                            } else {
                                data.title.push_str(&hover_messages::break_alt_if_suffix(alt));
                            }
                        }
                        // The lender of a borrow one of the `break`s
                        // made (`break rb` reborrowing a `&mut`, #657):
                        // it was lent only if that `break` ended the
                        // loop.
                        Event::StaticLend { id, .. } | Event::MutableLend { id, .. } => {
                            if let Some(alt) = borrow_break_alt(*id, visualization_data) {
                                data.title = hover_messages::with_break_alt_lend(&data.title, &alt);
                            }
                        }
                        // A value that holds a borrow one of the `break`s
                        // gave it (`break Some(&s)`, #1435): only if that
                        // `break` ended the loop.
                        Event::StaticBorrow { id, .. } | Event::MutableBorrow { id, .. }
                            if visualization_data.timelines[hash].resource_access_point.holds_borrow()
                                && !visualization_data.timelines[hash].resource_access_point.is_ref() => {
                            if let Some(alt) = borrow_break_alt(*id, visualization_data) {
                                data.title.push_str(&hover_messages::break_alt_if_suffix(alt));
                            }
                        }
                        Event::Acquire { break_alt: Some(BreakAlt { runtime: AltRuntime::Skipped, .. }), .. }
                        | Event::Copy { break_alt: Some(BreakAlt { runtime: AltRuntime::Skipped, .. }), .. } => {
                            data.title.clear();
                        }
                        Event::Acquire { is, break_alt: Some(alt), .. } | Event::Copy { is, break_alt: Some(alt), .. } => {
                            // Which `break` ran isn't known here, so
                            // name no source: what a literal would say.
                            data.title = if copyable_assign {
                                hover_messages::event_dot_copyable_assign(&name)
                            } else {
                                let generic = Event::Acquire { from: ResourceTy::Anonymous, is: is.clone(), via_try: None, id: 0, break_alt: None, loop_head: LoopHead::No };
                                generic.print_message_with_name(&mut name.clone())
                            };
                            data.title.push_str(&hover_messages::break_alt_receiver_suffix_for(alt));
                        }
                        _ => {}
                    }
                }
            }
        }
        let layout = &visualization_data.row_layout;
        let projected = layout.projected_loan_places.get(&event.get_id())
            .map(|(_, place)| hover_messages::projected_place_html(place));
        let stored = layout.stored_loans.get(&event.get_id()).copied();
        if let Some(place) = projected.as_deref() {
            let mutable = matches!(event, Event::MutableLend { .. } | Event::MutableBorrow { .. } |
                Event::MutableReacquire { .. } | Event::MutableDie { .. });
            let kind = if mutable { "mutable" } else { "shared" };
            match event {
                Event::StaticLend { to, is, .. } | Event::MutableLend { to, is, .. } => {
                    let receiver = styled_name_html(&to.name());
                    let owner = styled_name_html(&is.name());
                    let restriction = if mutable {
                        format!("{owner} can't be used as a whole while the borrow is live")
                    } else {
                        format!("While the borrow is live, you can't overwrite or move {place}")
                    };
                    data.title = if layout.shared_projected_calls.contains(&event.get_id()) && mutable {
                        format!("A mutable borrow of {place} is live while {receiver} uses a shared reference. {restriction}")
                    } else if stored.is_some() {
                        format!("The borrow checker keeps the {kind} borrow of {place} live while {receiver} is in use. {restriction}")
                    } else {
                        let adverb = if mutable { "mutably" } else { "immutably" };
                        format!("{receiver} {adverb} borrows {place}. {restriction}")
                    };
                }
                Event::StaticBorrow { is, .. } | Event::MutableBorrow { is, .. } if stored.is_none() => {
                    let holder = styled_name_html(&is.name());
                    let adverb = if mutable { "mutably" } else { "immutably" };
                    data.title = format!("{holder} holds a value that {adverb} borrows {place} ({holder} isn't a reference itself)");
                }
                Event::StaticReacquire { from, .. } | Event::MutableReacquire { from, .. }
                    if from.extract_rap().is_some_and(|r| r.is_fn()) => {
                    data.title = if layout.shared_projected_calls.contains(&event.get_id()) && mutable {
                        format!("The stored shared reference keeps the original mutable borrow of {place} live")
                    } else {
                        format!("The borrow of {place} continues in the stored reference")
                    };
                }
                Event::StaticReacquire { from, .. } | Event::MutableReacquire { from, .. }
                | Event::StaticDie { is: from, .. } | Event::MutableDie { is: from, .. } => {
                    data.title = format!("{}'s {kind} borrow of {place} ends", styled_name_html(&from.name()));
                }
                _ => {}
            }
        }
        if let Some(detail) = stored {
            let parties = match event {
                Event::StaticBorrow { from, is, .. } | Event::MutableBorrow { from, is, .. } => Some((is, from, false)),
                Event::StaticLend { to, is, .. } | Event::MutableLend { to, is, .. }
                    if matches!(detail, crate::event_order::StoredLoan::UniqueCapture) => Some((to, is, false)),
                Event::MutableDie { to, is, .. } => Some((is, to, true)),
                Event::MutableReacquire { from, is, .. } => Some((from, is, true)),
                _ => None,
            };
            if let Some((holder, lender, ended)) = parties {
                data.title = hover_messages::stored_loan_message(detail, holder, lender, None, ended, projected.as_deref());
            }
        }
        if let Some(end) = visualization_data.row_layout.dropped_field_loans.get(&event.get_id()) {
            let field = styled_name_html(&end.field);
            let holder = styled_name_html(&end.holder);
            let lender = styled_name_html(visualization_data.timelines[hash].resource_access_point.name());
            let kind = if matches!(event, Event::MutableReacquire { .. }) { "mutable" } else { "shared" };
            data.title = format!("The old {field}'s {kind} borrow of {lender} ends when the old {holder} is dropped. This borrow no longer restricts access to {lender}.");
            if let Some(conditional) = end.replacement {
                data.title.push_str(" Its destructor can use this reference before the replacement is installed.");
                if conditional {
                    data.title.push_str(&format!(" Rust tracks whether {holder} still holds the old value with a runtime drop flag."));
                }
            }
        }
        if matches!(event, Event::StaticDie { .. } | Event::MutableDie { .. }) {
            if let Some(owner) = visualization_data.row_layout.after_destructor.get(&event.get_id()) {
                data.title.push_str(&format!(". The old {} can use this reference in its destructor before the replacement is installed.", styled_name_html(owner)));
            }
            if visualization_data.row_layout.copied_ends.contains(&event.get_id()) {
                let name = styled_name_html(visualization_data.timelines[hash].resource_access_point.name());
                data.title = format!("{name} stops using its shared reference made from a mutable borrow. A copy keeps the original mutable borrow live.");
            }
        }
        if matches!(event, Event::RefDie { .. } | Event::MutableDie { .. } | Event::StaticDie { .. }) {
            if let Some(text) = layout.holder_handoff_titles.get(&event.get_id()) {
                data.title.clone_from(text);
            }
        }
        if matches!(event, Event::MutableLend { .. } | Event::StaticLend { .. }) {
            if let Some(text) = layout.result_lend_titles.get(&event.get_id()) {
                data.title.clone_from(text);
            }
        }
        if let Some(cases) = visualization_data.row_layout.fork_end_cases.get(&event.get_id()) {
            data.title.clone_from(cases);
        }
        if visualization_data.row_layout.reassigned_receivers.contains(&event.get_id()) {
            let name = styled_name_html(visualization_data.timelines[hash].resource_access_point.name());
            match event {
                Event::MutableLend { to, .. } => data.title = format!("{} activates the mutable borrow of the receiver reserved through {name} before the reassignment", styled_name_html(&to.name())),
                Event::MutableReacquire { from, .. } => data.title = format!("{} returns and releases its borrow of that receiver", styled_name_html(&from.name())),
                _ => {}
            }
        }
        // A scope end that drops nothing, which `return`s in a loop body
        // may come before (#1020).
        if let Event::OwnerGoOutOfScope { early: Some(e), .. } | Event::RefGoOutOfScope { early: Some(e), .. } = event {
            data.title = hover_messages::event_dot_scope_end_after_loop_return(&data.title, e, false);
        }
        for note in &visualization_data.approx_notes {
            match note {
                ApproxNote::VecElement { id, staged, drops_on_exit, repeat, .. } if event.get_id() == *id => {
                    if let Event::Move { is, .. } = event {
                        if *staged {
                            data.title.push_str(". The value is held temporarily while the vector is being built");
                            if *drops_on_exit {
                                data.title.push_str(". If the `?` returns early, it drops the staged value");
                                if !repeat { data.title.push_str(". If construction finishes, the value becomes a vector element"); }
                            }
                            if *repeat { data.title.push_str(". A zero count drops the element and leaves the vector empty"); }
                        }
                        if !is.is_ref() && (*staged || matches!(event, Event::Move { to: ResourceTy::Value(r), .. } if !r.is_fn())) {
                            data.title.push_str(&format!("&lt;br&gt;{} can't be used until it is assigned again", styled_name_html(&is.name())));
                        }
                    }
                }
                ApproxNote::PatternReference { holder } if holder == hash => {
                    if let Event::MutableBorrow { of_ref_var: true, from, is, .. } | Event::StaticBorrow { of_ref_var: true, from, is, .. } = event {
                        let kind = if is.is_mutref() { "a mutable" } else { "an immutable" };
                        data.title.push_str(&format!(". {} is bound to {kind} reference to {}", styled_name_html(&is.name()), styled_name_html(&from.real_name())));
                    }
                }
                ApproxNote::IteratorElement { id } if event.get_id() == *id => {
                    data.title = match event {
                        Event::Move { is, .. } | Event::Acquire { from: is, .. } =>
                            format!("{} moves into the vector consumed by the loop's iterator. The iterator yields its elements by value and drops any it still holds when the loop ends", styled_name_html(&is.name())),
                        _ => data.title,
                    };
                }
                _ => {}
            }
        }
        if let Some(ApproxNote::UnreachedTransfer { unwind_drop, .. }) = visualization_data.approx_notes.iter()
            .find(|n| matches!(n, ApproxNote::UnreachedTransfer { id, .. } if *id == event.get_id())) {
            data.title = format!("{} {}", hover_messages::UNREACHED_TRANSFER, data.title);
            if let Event::Move { is, .. } = event {
                let name = styled_name_html(&is.name());
                let drop = match unwind_drop {
                    Some("drops") => format!("{name}'s resource is dropped"),
                    Some("drops_if_init") => format!("{name}'s resource is dropped if it still holds one"),
                    Some("drops_rest") => format!("whatever {name} still holds is dropped"),
                    _ => String::new(),
                };
                if !drop.is_empty() {
                    data.title.push_str(&format!("&lt;br&gt;If the closure call panics and unwinding reaches {name}, {drop}."));
                }
            }
        }
        if let Some(title) = visualization_data.row_layout.aggregate_titles.get(&event.get_id()) {
            data.title.clone_from(title);
        }
        let mut event_kind = rv_attrs::event_name(event);
        if matches!(event, Event::StaticLend { .. } | Event::MutableLend { .. }
            | Event::StaticBorrow { .. } | Event::MutableBorrow { .. }) {
            if let Some(skipped) = borrow_break_alt(event.get_id(), visualization_data)
                .and_then(hover_messages::skipped_break_alt) {
                data.title = skipped;
                event_kind = rv_attrs::SKIPPED_EXIT;
            }
        }
        let rap = &visualization_data.timelines[hash].resource_access_point;
        let stacked = visualization_data.row_layout.stack_points.contains_key(&event.get_id());
        if matches!(event, Event::Acquire { from, .. }
            if matches!(from, ResourceTy::Anonymous) || from.extract_rap().is_some_and(|r| r.is_fn())) {
            for input in visualization_data.row_layout.temporary_staged_moves.get(hash).into_iter().flatten() {
                data.title.push_str(&format!(". {} is moved into this temporary and then moved out as an argument",
                    styled_name_html(input)));
            }
        }
        if let Some(state) = one_dot_holder_state(history, i, rap, &visualization_data.row_layout) {
            let tip = timeline_segment_title(&state, rap, visualization_data, None);
            let dot = dots.entry(*hash, timeline_data.x_val, event_y, *line_number, timeline_data);
            if !stacked || !dot.lines.iter().any(|(_, line)| *line == tip) {
                dot.lines.push((None, tip));
            }
        }
        // push to individual timelines
        let lend_id = match event {
            Event::StaticLend { id, .. } | Event::MutableLend { id, .. } => Some(*id),
            _ => None,
        };
        let borrower_title = visualization_data.row_layout.shadowed_borrow_lenders.get(&event.get_id()).and_then(|lender| {
            let mutable = match event {
                Event::StaticBorrow { .. } => false,
                Event::MutableBorrow { .. } => true,
                _ => return None,
            };
            let name = rap.name();
            let generic = if mutable { hover_messages::event_dot_mut_borrow } else { hover_messages::event_dot_static_borrow };
            (data.title == generic(name, false) || data.title == generic(name, true))
                .then(|| format!("{} from {}", data.title, styled_name_html(lender)))
        });
        if let Some(title) = borrower_title {
            data.title = title;
        }
        append_dot(data, timeline_data, dots, lend_id, event_kind, *line_number);
        if (stacked || layout.field_generations.contains_key(&event.get_id()))
            && matches!(event, Event::StaticBorrow { .. }) && rap.is_ref() && !rap.is_mutref() {
            let tip = hover_messages::state_static_ref_full_privilege(rap.name(), rap.points_to_copy());
            let dot = dots.entry(*hash, timeline_data.x_val, event_y, *line_number, timeline_data);
            if !dot.lines.iter().any(|(_, line)| *line == tip) {
                dot.lines.push((None, tip));
            }
        }
        if let Some(holder) = borrow_before_capture(history, i, &visualization_data.row_layout) {
            let dot = dots.entry(*hash, timeline_data.x_val, event_y, *line_number, timeline_data);
            if let [.., release, capture] = dot.lines.as_mut_slice() {
                if release.1 != hover_messages::event_dot_static_reacquire(rap.name(), rap.holds_value()) { continue; }
                release.1 = format!("Before this capture, the immutable borrow by {} had already ended. {}'s {} was no longer immutably borrowed", styled_name_html(holder), styled_name_html(rap.name()), if rap.holds_value() { "value" } else { "resource" });
                std::mem::swap(release, capture);
            }
        }
    }
}

// ── Conditional join-state computation ─────────────────────────────
//
// Computes the joined ownership state of one variable across the
// branches of an `if` / `match` / `if let`, used to label the per-
// variable merge dot at the bottom of the conditional.
//
// We classify each branch's *end state* for the variable as
// (ends_moved, has_acquire) by walking the per-variable Event list
// the conversion in data.rs already filtered for us. Move events on
// that list mean the variable was moved out; Acquire events mean it
// received a new resource (re-bind or first bind). Borrows / dies /
// duplicates don't change ownership.
//
// Nested conditionals are handled recursively: if any nested path
// moves the variable, we propagate that up as a possible move on the
// containing path. If every nested path acquires the variable, we
// propagate that up as an acquire.
//
// Joining across the outer branches:
//   - Any branch ends moved             → MovedAfter
//   - Every branch acquires AND every    → BoundHere
//     conceptual path is represented
//     (no missing else)
//   - Otherwise                          → Unchanged (no tooltip)
//
// "Every conceptual path is represented" — for `if cond { body }`
// without an else, the implicit-untouched else means the var can't
// be considered freshly-bound regardless of what the if branch does.
// Match arms are exhaustive in Rust, so all paths are present.

enum BranchJoin {
    Unchanged,
    MovedAfter,
    BoundHere,
}

fn analyze_branch_for_join(events: &[(usize, Event)], holder: bool) -> (bool, bool) {
    let mut moved = false;
    let mut acquired = false;
    for (_, e) in events {
        match e {
            // After a loop that may have been left through a `break`
            // following a move, it may have been moved too.
            e if e.is_box_payload_move() => {}
            Event::Move { .. } | Event::LoopExitMove { .. } | Event::OrAltMove { tuple: None, .. } | Event::ClosureCall { .. } => { moved = true; acquired = false; }
            // (A copy in, `Ok(num) => num` with num an i32, gives it
            // its value too, #994.)
            Event::Acquire { .. } | Event::Copy { .. } | Event::LoopExitKept { .. } => { moved = false; acquired = true; }
            // A value that holds a borrow gets its value by the borrow
            // (`let t = if c { Some(&s) } else { None };`, #1435).
            Event::StaticBorrow { .. } | Event::MutableBorrow { .. } if holder => { moved = false; acquired = true; }
            Event::Branch { branch_history, ty, .. } => {
                let mut nested_any_moved = false;
                let mut nested_all_acquired = branch_history.iter().any(|nb| !nb.diverges);
                for nb in branch_history.iter().filter(|nb| !nb.diverges) {
                    let (m, a) = analyze_branch_for_join(&nb.e_data, holder);
                    if m { nested_any_moved = true; }
                    if !a { nested_all_acquired = false; }
                }
                if nested_any_moved {
                    moved = true; acquired = false;
                } else if nested_all_acquired && all_paths_present(ty, branch_history.len()) {
                    moved = false; acquired = true;
                }
            }
            // Borrows, returns, copies-from-here, duplicates: don't
            // change whether the variable still owns its resource.
            _ => {}
        }
    }
    (moved, acquired)
}

/// A reference field's own dot where its borrows from several lenders
/// start or end (#1354, `field_lender_groups`): it names them all, as the
/// one arrow drawn for them does.
fn field_lenders_dot_title(event: &Event, name: &String, vd: &VisualizationData) -> Option<String> {
    let (id, mutable, end) = match event {
        Event::StaticBorrow { id, .. } => (id, false, false),
        Event::MutableBorrow { id, .. } => (id, true, false),
        Event::StaticDie { id, .. } => (id, false, true),
        Event::MutableDie { id, .. } => (id, true, true),
        _ => return None,
    };
    let f = vd.field_lenders.get(id).filter(|f| f.end == end)?;
    if !end {
        if let Some(via) = vd.external_events.iter().find_map(|(_, event)| match event {
            ExternalEvent::StaticBorrow { id: event_id, via, .. }
            | ExternalEvent::MutableBorrow { id: event_id, via, .. } if event_id == id => via.as_ref(),
            _ => None,
        }).filter(|via| hover_messages::is_forwarded_borrow(&via.tie, f.lenders.len())) {
            return Some(hover_messages::event_dot_forwarded_borrow(name, via, &f.lenders));
        }
    }
    Some(if end {
        hover_messages::event_dot_field_lenders_end(name, &f.lenders)
    } else {
        hover_messages::event_dot_field_lenders_borrow(name, &f.lenders, mutable)
    })
}

// A borrow's end on a line where the same reference's borrow of another
// lender ends too, with the reference's new borrow in between (`c = &y;`
// ends c's loan of `x` and its unused new loan of `y`, #803): the dot
// would say "c's immutable borrow ends" twice, with the new borrow's
// text between, so it names each lender. Ends listed together (a
// reference that borrows from two lenders, `longest(&a, &b)`) already
// read as one sentence, and a lone end keeps its plain wording.
fn ends_several_loans_title(event: &Event, history: &[(usize, Event)], line: usize, name: &mut String, vd: &VisualizationData, rap: &ResourceAccessPoint) -> Option<String> {
    if vd.row_layout.uncertain_store_ends.contains(&event.get_id()) { return None; }
    let mine = event.borrow_end_lender_name()?;
    let is_mut = matches!(event, Event::MutableDie { .. });
    let table = &vd.state_table;
    let identified = |e: &Event| {
        let lender = e.borrow_end_lender_name()?;
        let ids = table.end_loans.get(&e.get_id())?;
        let fn_line = *vd.fn_source_lines.get(rap.hash())?;
        let mut matching = ids.iter().filter(|id| table.loan(fn_line, **id).is_some_and(|l| l.lender == lender));
        let id = *matching.next()?;
        matching.next().is_none().then_some(id)
    };
    if let Some(id) = identified(event) {
        // One overwritten reference can have alternative reaching loans.
        // Separate lender names would say it held both. Use its ordinary
        // end clause when the two definitions came from sibling CFG edges.
        if rap.is_ref() {
            let function = vd.fn_source_lines[rap.hash()];
            if let Some(loan) = table.loan(function, id) {
                if table.end_loans[&event.get_id()].iter().filter_map(|&other| table.loan(function, other))
                    .any(|other| loan.alternative_to(other)) {
                    return None;
                }
            }
        }
        let src = rv_attrs::line_of(line, &vd.row_lines);
        let on_row = |l| src.is_some() && rv_attrs::line_of(l, &vd.row_lines) == src;
        // A field returned by a call may represent alternative lenders.
        // Only separate its names when the stores themselves are ordered.
        let alternatives = history.iter().any(|(l, e)| on_row(*l) && vd.field_lenders.contains_key(&e.get_id()));
        if (!alternatives || vd.row_layout.stack_points.contains_key(&event.get_id()))
            && history.iter().any(|(l,e)| on_row(*l) && matches!(e, Event::StaticBorrow { .. } | Event::MutableBorrow { .. }))
            && history.iter().any(|(l,e)| on_row(*l) && identified(e).is_some_and(|other| other != id)
                && e.borrow_end_lender_name().as_ref() != Some(&mine))
        {
            return event.borrow_end_naming_lender(name);
        }
    }
    let row: Vec<&Event> = history.iter().filter(|(l, _)| *l == line).map(|(_, e)| e).collect();
    let mut ends = row.iter().enumerate().filter(|(_, e)| e.borrow_end_lender_name().is_some());
    let me = row.iter().position(|e| std::ptr::eq(*e, event))?;
    let repeated = ends.any(|(j, e)| {
        j != me
            && matches!(e, Event::MutableDie { .. }) == is_mut
            && e.borrow_end_lender_name().is_some_and(|other| other != mine)
            // Something else on the dot between the two ends.
            && (vd.row_layout.name_lender(event.get_id()) || row[j.min(me)..=j.max(me)].iter().any(|x| x.borrow_end_lender_name().is_none()))
    });
    if repeated { event.borrow_end_naming_lender(name) } else { None }
}

// The first line where a reference's borrow ends in `history` (one
// arm's events), looking into conditionals nested in it.
fn first_borrow_end(history: &[(usize, Event)]) -> Option<usize> {
    history.iter().filter_map(|(l, e)| match e {
        // (`OrAltBorrow { end }`: the use after a failed guard's
        // assignment that ends the borrow, #603.)
        Event::StaticDie { .. } | Event::MutableDie { .. } | Event::OrAltBorrow { end: true, .. } => Some(*l),
        Event::Branch { branch_history, .. } => branch_history.iter().filter_map(|b| first_borrow_end(&b.e_data)).min(),
        _ => None,
    }).min()
}

// Whether a reference's borrow ends in `history` (one arm's events),
// possibly in every arm of a conditional nested in it.
fn ends_borrow(history: &[(usize, Event)]) -> bool {
    history.iter().any(|(_, e)| match e {
        // (`OrAltBorrow`'s end: a let-chain's arm, where the reference
        // may point at either place, #603.)
        Event::StaticDie { .. } | Event::MutableDie { .. } | Event::OrAltBorrow { end: true, .. } => true,
        Event::Branch { branch_history, .. } => branch_history.iter().all(|b| ends_borrow(&b.e_data)),
        _ => false,
    })
}

/// The state an arm reaches its conditional's join in, for drawing: the
/// arm's last state, except for a reference whose borrow ended in the arm
/// (`end_outer_borrows_in_arms`), which reaches the join with the dashed
/// "r's borrow has ended on this path" rest of the arm (#495). Every arm
/// that gets to the join has that rest, down to the join dot, which says
/// "r's borrow ended in each branch above". An arm whose borrow ends on its
/// last row has no rows left after it, and its last state (the borrow
/// over, nothing held) drew no leg on to the join: the last arm of an
/// `if` / `else` or `match`, and an `else if use(r)` whose condition is
/// r's last use (#1370, #1338). Only where the borrow ended on every
/// path (that join dot is drawn): an arm beside one that moved r has no
/// join to go on to. Likewise an arm that ends with a conditional of its
/// own whose every arm ended the borrow (#1326).
fn arm_end_state(rap: &ResourceAccessPoint, branch: &BranchData, branch_history: &[BranchData], ty: &BranchType) -> State {
    let end = branch.states.last().map_or(State::OutOfScope, |s| s.2.clone());
    if !rap.is_ref() || branch.diverges {
        return end;
    }
    let ended = |s: Option<&State>| matches!(s, Some(State::OutOfScope) | Some(State::FullPrivilege { s: LineState::Uninit }));
    // The join's test for its "r's borrow ended in each branch above"
    // dot (`borrow_ended_in_every_arm`).
    let every_arm_ended = all_paths_present(ty, branch_history.len())
        && branch_history.iter().all(|b| ends_borrow(&b.e_data) && ended(b.states.last().map(|s| &s.2)));
    // Or its borrow ended before the conditional, and an arm that gave it a
    // new one ended that (#1524): the join's "r's borrow ended before the
    // branches above" dot. The arms that do nothing hold no borrow.
    let every_arm_ended = every_arm_ended || (all_paths_present(ty, branch_history.len())
        && branch_history.iter().any(|b| !b.diverges && ends_borrow(&b.e_data))
        && branch_history.iter().filter(|b| !b.diverges).all(|b| ended(b.states.last().map(|s| &s.2))
            && (ends_borrow(&b.e_data) || b.e_data.is_empty())));
    // An arm beside one that moved r (`if c { take(m); }`, `Some(_) =>
    // drop(m)`), where every other arm ended its borrow: the join has no
    // dot (a reference whose borrow ended in only some arms gets none),
    // so the arm's leg stops at its borrow end instead of running on,
    // dashed, to nothing (#1416).
    let reaching = || branch_history.iter().filter(|b| !b.diverges).map(|b| b.states.last().map(|s| &s.2));
    if !every_arm_ended && !matches!(ty, BranchType::Loop(..)) && ended(Some(&end))
        && reaching().any(|s| matches!(s, Some(State::ResourceMoved { .. })))
        && reaching().all(|s| ended(s) || matches!(s, Some(State::ResourceMoved { .. }))) {
        return State::OutOfScope;
    }
    if !matches!(end, State::OutOfScope) {
        return end;
    }
    let ended_here = !matches!(ty, BranchType::Loop(..))
        && every_arm_ended
        && branch.e_data.iter().any(|(_, e)| matches!(e,
            Event::StaticDie { .. } | Event::MutableDie { .. } | Event::OrAltBorrow { end: true, .. }));
    let ended_in_nested = matches!(branch.e_data.last(), Some((_, Event::Branch { branch_history: inner, ty: inner_ty, .. }))
        if all_paths_present(inner_ty, inner.len()) && inner.iter().all(|b| ends_borrow(&b.e_data)));
    if ended_here || ended_in_nested { State::FullPrivilege { s: LineState::Uninit } } else { end }
}

fn branch_moves(event: &Event) -> (bool, bool) {
    match event {
        Event::Branch { join: Some(join), .. } => (
            join.cell.init == Init::Maybe && join.cell.cause == Some("moved")
                || join.cell.init == Init::Partly && (join.cell.unnamed_maybe || join.maybe_empty_box),
            join.cell.init == Init::Moved,
        ),
        _ => (false, false),
    }
}

fn branch_maybe_held(event: &Event) -> bool {
    match event {
        Event::Branch { join: Some(join), .. } => join.cell.init == Init::Maybe
            && join.change.wrote && !join.change.writes_on_all_paths && join.change.entry_maybe,
        Event::Branch { is, .. } if is.extract_rap().is_some_and(is_branch_temp_col) => leaves_maybe_held(event),
        _ => false,
    }
}

fn branch_initialization(event: &Event) -> Option<InitJoin> {
    let Event::Branch { join, is, branch_history, maybe_init, .. } = event else { return None };
    if let Some(j) = join {
        if j.cell.init != Init::Maybe || j.cell.always_assigned { return None; }
        if let Some(mixed) = j.cell.mixed_init.filter(|_| !j.change.entry_own) {
            return Some(if mixed.may_hold_value {
                InitJoin::Maybe { moved: mixed.whole_move }
            } else {
                InitJoin::AllMoved
            });
        }
        return (j.cell.cause == Some("unassigned") && j.change.wrote)
            .then_some(InitJoin::Maybe { moved: false });
    }
    // Transient values still use their own display lifecycle.
    if is.extract_rap().is_some_and(is_branch_temp_col) {
        return branches_init_join(*maybe_init, branch_history);
    }
    None
}

/// Whether the branches in `history` call the closure (an `FnOnce` call
/// uses it up, #271) and whether they move it (`v.push(f)`, `let g =
/// f;`, #1158), at any depth.
fn closure_used_up_in_branches(history: &[(usize, Event)]) -> (bool, bool) {
    fn walk(events: &[(usize, Event)], in_branch: bool, out: &mut (bool, bool)) {
        for (_, e) in events {
            match e {
                Event::Branch { branch_history, .. } => {
                    for b in branch_history {
                        walk(&b.e_data, true, out);
                    }
                }
                Event::ClosureCall { .. } if in_branch => out.0 = true,
                Event::Move { .. } if in_branch => out.1 = true,
                _ => {}
            }
        }
    }
    let mut out = (false, false);
    walk(history, false, &mut out);
    if !out.0 && !out.1 {
        out.1 = true;
    }
    out
}

fn maybe_moved_at_end(history: &[(usize, Event)]) -> bool {
    let mut maybe = false;
    for (_, e) in history {
        match e {
            // A conditional none of whose branches gives it a value or
            // takes it (an exit's drop in one arm, `_ => {}` in the
            // other, #1194) leaves it as it was.
            Event::Branch { branch_history, maybe_held: false, .. } if leaves_value_as_it_was(branch_history) => {}
            // Maybe held nothing as it started, assigned in some
            // branches only (#1034), also in a branch of this one
            // (#1413).
            e if branch_maybe_held(e) => maybe = true,
            // Assigned in some branches only, not moved (#244).
            Event::Branch { .. } if branch_initialization(e).is_some() => maybe = false,
            // Or by a loop that may have ended before assigning it (#786).
            Event::LoopMaybeInit { .. } => maybe = false,
            Event::Branch { .. } => {
                let (mixed, all_moved) = branch_moves(e);
                if mixed {
                    maybe = true;
                } else if all_moved {
                    maybe = false;
                }
            }
            // A loop left through a `break` after a move (or another
            // way): maybe-moved; through such `break`s only: moved.
            Event::LoopExitMove { at: MaybeMovedAt::LoopPartial, .. } => {}
            Event::LoopExitMove { definite, .. } => maybe = !*definite,
            // Moved into a name an or-pattern binds, on the paths where
            // the alternative that binds from here matched (#533).
            // (Not a variable whose value was already moved into a
            // scrutinee tuple: it owns nothing either way.)
            Event::OrAltMove { tuple: None, .. } => maybe = true,
            // Moved by one of several `break`s of a loop: only if that
            // `break` ended the loop (#561).
            e if e.is_box_payload_move() => {}
            Event::Move { break_alt: Some(alt), .. } => maybe = !alt.in_loop,
            Event::Acquire { is: ResourceTy::Value(_), .. }
            | Event::Copy { is: ResourceTy::Value(_), .. }
            // Owns a value at every `break` of the loop above (#1000).
            | Event::LoopExitKept { .. }
            | Event::Move { .. } | Event::ClosureCall { .. } => maybe = false,
            _ => {}
        }
    }
    maybe
}

// Whether what last made the column maybe-moved (as
// `maybe_moved_at_end` reads `history`) is the end of a loop that
// moves it on some time rounds and not others (`MaybeMovedAt::LoopRound
// { kept: true }`, #1019), or a conditional that assigns it in some
// branches only where it may have held nothing as it started (a
// `maybe_held` join, #1034): either way, whether something above moved
// it doesn't settle what's dropped, so "only if it still holds it".
fn maybe_moved_by_kept_round<'a>(history: impl Iterator<Item = &'a (usize, Event)>) -> bool {
    let mut kept = false;
    for (_, e) in history {
        match e {
            Event::LoopExitMove { definite: false, at: MaybeMovedAt::LoopRound { kept: k } | MaybeMovedAt::Loop { kept: k, .. } } => kept = *k,
            // A conditional that assigns it in some branches only,
            // where it may have held nothing as it started (#1034):
            // the old value, the new one or nothing. (Also in a branch
            // of this one, #1413.)
            e if branch_maybe_held(e) => kept = true,
            Event::Branch { join: Some(j), .. }
                if j.cell.init == Init::Maybe && (j.change.replaces_moved || j.change.writes_away) => kept = true,
            Event::Branch { join: Some(j), .. } if j.cell.init == Init::Maybe
                && !j.change.moved && !j.change.wrote => {}
            Event::Branch { .. } if branch_initialization(e).is_some() => kept = false,
            Event::Branch { .. } => {
                let (mixed, all_moved) = branch_moves(e);
                if mixed || all_moved {
                    kept = false;
                }
            }
            e if e.is_box_payload_move() => {}
            Event::LoopMaybeInit { .. } | Event::LoopExitMove { .. } | Event::OrAltMove { tuple: None, .. }
            | Event::Acquire { is: ResourceTy::Value(_), .. }
            | Event::Copy { is: ResourceTy::Value(_), .. }
            | Event::LoopExitKept { .. }
            | Event::Move { .. } | Event::ClosureCall { .. } => kept = false,
            _ => {}
        }
    }
    kept
}

// The events on column `hash` after the conditional `event` (found in
// `history`) joins at `merge_point`, in the order they run: the rest of
// each arm it is nested in, innermost first, then what follows on the
// column's whole history (#1413, #1460). Not past an arm that returns or
// panics: that code doesn't run after it. A nested conditional only has
// its arm's events in `history`.
fn events_after<'a>(vd: &'a VisualizationData, hash: &u64, history: &'a [(usize, Event)], event: &Event, merge_point: usize) -> Vec<&'a (usize, Event)> {
    fn find<'a>(h: &'a [(usize, Event)], event: &Event, out: &mut Vec<&'a (usize, Event)>) -> bool {
        for (i, item) in h.iter().enumerate() {
            if std::ptr::eq(&item.1, event) {
                out.extend(&h[i + 1..]);
                return true;
            }
            if let Event::Branch { branch_history, .. } = &item.1 {
                for b in branch_history {
                    if find(&b.e_data, event, out) {
                        if !b.diverges {
                            out.extend(&h[i + 1..]);
                        }
                        return true;
                    }
                }
            }
        }
        false
    }
    let mut out = Vec::new();
    if let Some(t) = vd.timelines.get(hash) {
        if find(&t.history, event, &mut out) {
            return out;
        }
    }
    // (Not on the column's history: the rest of `history`.)
    out.clear();
    out.extend(history.iter().filter(|(l, _)| *l > merge_point));
    out
}

// The `before_loop` of the last `LoopMaybeInit` in `history` (#1012).
fn loop_maybe_init_before(history: &[(usize, Event)]) -> Option<bool> {
    history.iter().rev().find_map(|(_, e)| match e {
        Event::LoopMaybeInit { before_loop, .. } => Some(*before_loop),
        _ => None,
    }).flatten()
}

fn loop_maybe_init_prior_assignment(history: &[(usize, Event)]) -> bool {
    history.iter().rev().find_map(|(_, e)| match e {
        Event::LoopMaybeInit { prior_assignment, .. } => Some(*prior_assignment),
        _ => None,
    }).unwrap_or(false)
}

// A drop-flag assignment to a maybe-initialized `let x;` local (#244),
// also one in a branch of a later conditional.
fn reassigns_maybe_init(e: &Event) -> bool {
    match e {
        Event::OwnerDropAtReassign { maybe_init, .. } => matches!(maybe_init, Some(InitBy::Branch | InitBy::Loop)),
        Event::Branch { branch_history, .. } =>
            branch_history.iter().any(|b| b.e_data.iter().any(|(_, e)| reassigns_maybe_init(e))),
        _ => false,
    }
}


// The end state of each branch of a conditional that reaches its join
// (not one that returns or panics, #296).
fn reaching_end_states(branch_history: &[BranchData]) -> impl Iterator<Item = &State> + Clone {
    branch_history.iter().filter(|b| !b.diverges).filter_map(|b| b.states.last().map(|s| &s.2))
}

// A maybe-initialized column (`InitJoin::Maybe`) that is a temporary,
// or a field column of one: only a temporary that a `let` extends
// through a branch can be one (#1397).
fn is_branch_temp_col(rap: &ResourceAccessPoint) -> bool {
    rap.is_temporary() || hover_messages::is_temporary_name(rap.name().split('.').next().unwrap_or(""))
}

// For a temporary a `let` extends through a branch (#1397) that only may
// exist at `history[i]` (after the join of the conditional whose branch
// made it): the line that branch made it on (#1436).
fn branch_temp_maybe_made(vd: &VisualizationData, hash: &u64, history: &[(usize, Event)], i: usize) -> Option<Option<usize>> {
    let timeline = &vd.timelines[hash];
    if !is_branch_temp_col(&timeline.resource_access_point) {
        return None;
    }
    let path = history_path(&timeline.history, history, i)?;
    matches!(init_at_end(&path), Some(InitJoin::Maybe { .. }))
        .then(|| made_on_line(&timeline.history, &vd.row_lines))
}

// The source line a column first gets a value on, looking into the
// branches of its conditionals: where a branch makes a temporary that a
// `let` extends (#1397). (`history` is by row; `row_lines` maps rows to
// source lines.)
fn made_on_line(history: &[(usize, Event)], row_lines: &[Option<usize>]) -> Option<usize> {
    fn made_on_row(history: &[(usize, Event)]) -> Option<usize> {
        for (row, e) in history {
            match e {
                Event::Branch { branch_history, .. } => {
                    if let Some(r) = branch_history.iter().find_map(|b| made_on_row(&b.e_data)) {
                        return Some(r);
                    }
                }
                e if holds_value_after(false, e) => return Some(*row),
                _ => {}
            }
        }
        None
    }
    made_on_row(history).and_then(|r| rv_attrs::line_of(r, row_lines))
}

// `init_join` for a conditional's branches: a branch whose own nested
// conditional left the column maybe-initialized (`let x;` assigned, or
// a temporary made, in a branch of an `if` inside one branch, #1397)
// may hold a value at the join too.
fn branches_init_join(maybe_init: bool, branch_history: &[BranchData]) -> Option<InitJoin> {
    let join = init_join(maybe_init, reaching_end_states(branch_history));
    if join.is_some() || !maybe_init {
        return join;
    }
    branch_history.iter().filter(|b| !b.diverges)
        .any(|b| matches!(init_at_end(&b.e_data), Some(InitJoin::Maybe { .. })))
        .then_some(InitJoin::Maybe { moved: false })
}

// `Some` when the last thing that decided what this column holds was a
// conditional that assigned it, a `let s;` local, in only some of the
// branches that reach its join (#244): at scope end, Rust drops the
// value only if there is one (a runtime drop flag).
fn init_at_end(history: &[(usize, Event)]) -> Option<InitJoin> {
    init_after(None, history)
}

// Whether what last decided what a `let s;` local holds (as
// `init_at_end` reads `history`) is a conditional that assigns it in
// some branches only, where it may have held a value as it started
// (a `maybe_held` join, #1036): the drop flag's "if it still holds it",
// not "if the branch that ran assigned it".
fn init_by_maybe_held<'a>(history: impl IntoIterator<Item = &'a (usize, Event)>) -> bool {
    let mut held = false;
    for (_, e) in history {
        match e {
            // (Not a later one that leaves it as it was, #1533.)
            Event::Branch { branch_history, maybe_held: false, .. } if leaves_value_as_it_was(branch_history) => {}
            Event::Branch { .. } => held = branch_maybe_held(e),
            Event::LoopMaybeInit { .. } => held = false,
            e if holds_value_after(false, e) => held = false,
            _ => {}
        }
    }
    held
}

// Whether none of the branches of a conditional (`branch_history`) gives
// the column a value or takes it: an exit's drop in one arm, `_ => {}`
// in the other (#1194). It leaves what the column holds as it was.
fn leaves_value_as_it_was(branch_history: &[BranchData]) -> bool {
    branch_history.iter().all(|b| b.e_data.iter().all(|(_, e)| !matches!(e,
        Event::Move { .. } | Event::Acquire { .. } | Event::Copy { .. } | Event::Branch { .. }
        | Event::LoopExitMove { .. } | Event::LoopExitKept { .. } | Event::LoopMaybeInit { .. }
        | Event::OrAltMove { .. } | Event::ClosureCall { .. } | Event::InitRefParam { .. })
        && !(matches!(e, Event::OwnerGoOutOfScope { .. }) && !b.diverges)))
}

// `init_at_end` for events that follow a point where the column was in
// state `start`.
fn init_after(start: Option<InitJoin>, history: &[(usize, Event)]) -> Option<InitJoin> {
    let mut init = start;
    for (_, e) in history {
        match e {
            // A `let s;` local only some branches above assigned, which a
            // later `if d { return; }` drops if it holds a value (#1436):
            // still assigned in only some of them after it.
            Event::Branch { branch_history, maybe_held: false, .. }
                if init.is_some() && leaves_value_as_it_was(branch_history) => {}
            // One that may leave it holding a value or not (#1034, #1413,
            // #1495): so does a later conditional that leaves it as it
            // was (#1533).
            e if branch_maybe_held(e) => init = Some(InitJoin::Maybe { moved: false }),
            Event::Branch { .. } => {
                init = branch_initialization(e);
            }
            Event::LoopMaybeInit { .. } => init = Some(InitJoin::Loop),
            e if holds_value_after(false, e) => init = None,
            _ => {}
        }
    }
    init
}

// `Some(some_diverge)` when the last thing that decided this column's
// ownership was a conditional that consumed it on every path to its join.
enum ScopeMove {
    Scrutinee,
    Conditional,
    Branches { some_diverge: bool },
}

fn all_moved_at_end(
    history: &[(usize, Event)],
    table: &crate::facts::table::StateTable,
    name: &str,
) -> Option<ScopeMove> {
    let mut all = None;
    for (_, e) in history {
        match e {
            Event::Branch { branch_history, join, .. } => {
                let (mixed, all_moved) = branch_moves(e);
                if all_moved && join.as_ref().is_some_and(|j| j.change.moves_on_all_paths) {
                    let scrutinee = branch_history.iter().filter_map(|b| table.imaginary_arm(b.imaginary?))
                        .any(|arm| arm.scrutinee_moves.contains(name));
                    all = Some(if scrutinee {
                        ScopeMove::Scrutinee
                    } else if join.as_ref().is_some_and(|j| j.change.test_moved) {
                        ScopeMove::Conditional
                    } else {
                        ScopeMove::Branches { some_diverge: any_diverges(branch_history) }
                    });
                } else if mixed || join.is_some() {
                    all = None;
                }
            }
            e if e.is_box_payload_move() => {}
            Event::LoopExitMove { .. } | Event::LoopExitKept { .. }
            | Event::Acquire { .. } | Event::Copy { .. }
            | Event::Move { .. } | Event::ClosureCall { .. }
            | Event::OwnerDropAtReassign { .. } => all = None,
            _ => {}
        }
    }
    all
}

// True when a let-chain's condition (or a match guard, #247) may have
// moved this column's value before failing (#281), or a `while` loop's
// (#968), or a loop body before a `break` outside any branch that
// breaks (#980), or before the loop comes back round (#992), or on the
// way to a labeled block's end that a `break` skips (#1286), so a
// drop-flag message says "nothing above" moved it rather than "no
// branch above". Not when the column got a new value after that on the
// same path (`s = …` in the else branch): from there it holds one
// whatever the condition did, and the drop flag is down to what comes
// after.
fn moved_by_chain_condition(history: &[(usize, Event)]) -> bool {
    chain_move_lasts(false, history)
}

// `moved_by_chain_condition` along `events`, starting from `lasts`.
fn chain_move_lasts(mut lasts: bool, events: &[(usize, Event)]) -> bool {
    for (_, e) in events {
        match e {
            Event::LoopExitMove { at: MaybeMovedAt::ChainFail { .. } | MaybeMovedAt::WhileCondFail { .. }
                | MaybeMovedAt::Loop { nothing_above: true, .. } | MaybeMovedAt::LoopHead | MaybeMovedAt::LoopRound { .. }
                | MaybeMovedAt::LabeledBlock { skipped: true, .. }, .. } => lasts = true,
            e if gives_new_value(e) => lasts = false,
            Event::Branch { branch_history, .. } => {
                lasts = branch_history.iter().any(|b| chain_move_lasts(lasts, &b.e_data));
            }
            _ => {}
        }
    }
    lasts
}

// True when a let-chain's condition (or a match guard) may have given
// this column a new value when it held none, or after moving it (#603,
// `ChainFailHow::may_hold_new`), and nothing gave it one since on that
// path: its drop flag decides "only if it still holds it".
fn chain_new_value_lasts<'a>(history: impl IntoIterator<Item = &'a (usize, Event)>) -> bool {
    fn along<'a>(mut lasts: bool, events: impl IntoIterator<Item = &'a (usize, Event)>) -> bool {
        for (_, e) in events {
            match e {
                Event::LoopExitMove { at: MaybeMovedAt::ChainFail { how, .. }, .. } => lasts = how.may_hold_new(),
                e if gives_new_value(e) => lasts = false,
                Event::Branch { branch_history, .. } => {
                    lasts = branch_history.iter().any(|b| along(lasts, &b.e_data));
                }
                _ => {}
            }
        }
        lasts
    }
    along(false, history)
}

// `chain_condition_moved_here` for a `chain_new_value_lasts` effect.
fn chain_condition_assigned_here(branch_history: &[BranchData]) -> bool {
    branch_history.iter().any(|b| {
        let at = b.e_data.iter().rposition(|(_, e)|
            matches!(e, Event::LoopExitMove { at: MaybeMovedAt::ChainFail { .. }, .. }));
        at.is_some_and(|i| matches!(&b.e_data[i].1, Event::LoopExitMove { at: MaybeMovedAt::ChainFail { how, .. }, .. } if how.may_hold_new())
            && !b.e_data[i + 1..].iter().any(|(_, e)| gives_new_value(e)))
    })
}

// An assignment that gives the column a new value (`s = …`, `*b = …`).
fn gives_new_value(e: &Event) -> bool {
    matches!(e, Event::OwnerDropAtReassign { .. } | Event::Acquire { .. })
}

// Whether this conditional's own let-chain condition may have moved the
// column's value before a later part failed: the else branch (or the
// implicit empty one) starts with the `MaybeMovedAt::ChainFail` event
// (#940, #953). Not one nested in a branch: for an outer conditional's
// join, a condition inside one of its branches is part of that branch,
// so "if the branch that ran didn't move it" stays true. Nor one that
// assigns it again after that (`s = …` in the else branch): that branch
// then ends holding a value, like one that didn't move it.
fn chain_condition_moved_here(branch_history: &[BranchData]) -> bool {
    branch_history.iter().any(|b| {
        let at = b.e_data.iter().rposition(|(_, e)|
            matches!(e, Event::LoopExitMove { at: MaybeMovedAt::ChainFail { .. }, .. }));
        at.is_some_and(|i| !b.e_data[i + 1..].iter().any(|(_, e)| gives_new_value(e)))
    })
}

// Whether what makes this column maybe-moved at its end (as
// `maybe_moved_at_end` reads it) is a `let` whose or-pattern binds a
// name from it or from another place (`let ((x, _) | (_, x)) = t;`,
// #597): no branch, so its drop wording says "nothing above".
fn maybe_moved_by_let<'a>(history: impl IntoIterator<Item = &'a (usize, Event)>) -> bool {
    let mut by_let = false;
    for (_, e) in history {
        match e {
            Event::Branch { .. } => {
                let (mixed, all_moved) = branch_moves(e);
                if mixed || all_moved {
                    by_let = false;
                }
            }
            Event::OrAltMove { in_let, .. } => by_let = *in_let,
            Event::LoopExitMove { .. } | Event::LoopExitKept { .. }
            | Event::Acquire { is: ResourceTy::Value(_), .. }
            | Event::Copy { is: ResourceTy::Value(_), .. }
            | Event::Move { .. } | Event::ClosureCall { .. } => by_let = false,
            _ => {}
        }
    }
    by_let
}

fn all_paths_present(ty: &BranchType, n_branches: usize) -> bool {
    match ty {
        // Plain `if cond { … }` without an else has only one branch
        // recorded; the implicit-untouched else means not every
        // conceptual path acquires, so BoundHere can't fire.
        BranchType::If(labels, _) => labels.len() >= 2 && n_branches >= 2,
        // Rust match is exhaustive — every path is one of the arms.
        BranchType::Match(labels, _) => labels.len() == n_branches,
        // Loop bodies are conditionally entered (zero or more times),
        // so a "BoundHere" claim isn't appropriate for their merge.
        BranchType::Loop(_, _) => false,
    }
}

fn compute_branch_join(branch_history: &[BranchData], ty: &BranchType, holder: bool) -> BranchJoin {
    let mut any_moved = false;
    // Only branches that reach the merge count; a diverging one
    // (`_ => unreachable!()`) produces no value and moves nothing
    // that's still around afterwards (#296). `all_paths_present`
    // still counts every branch: a diverging arm is a path, just not
    // one that gets here.
    let mut all_acquired = branch_history.iter().any(|b| !b.diverges);
    for b in branch_history.iter().filter(|b| !b.diverges) {
        let (m, a) = analyze_branch_for_join(&b.e_data, holder);
        if m { any_moved = true; }
        if !a { all_acquired = false; }
    }
    if any_moved {
        BranchJoin::MovedAfter
    } else if all_acquired && all_paths_present(ty, branch_history.len()) {
        BranchJoin::BoundHere
    } else {
        BranchJoin::Unchanged
    }
}

// Some branch of the conditional has type `!` and never reaches the
// merge (#296); the merge messages then speak of the branches that do.
fn any_diverges(branch_history: &[BranchData]) -> bool {
    branch_history.iter().any(|b| b.diverges)
}

fn branch_join_message(
    branch_history: &[BranchData], ty: &BranchType, is: &ResourceTy,
    imaginary_arm: Option<&crate::facts::table::ImaginaryArm>, join: Option<&JoinCell>,
) -> String {
    let var_name = &is.real_name();
    let imaginary_else = branch_history.iter().any(|b| b.imaginary.is_some());
    // The branches that never get here, by name, with how they left.
    let diverged: Vec<(String, Vec<BranchExit>)> = branch_history.iter().enumerate()
        .filter(|(_, b)| b.diverges && b.imaginary.is_none())
        .map(|(i, b)| (ty.string_of_branch(i), b.exits.clone()))
        .collect();
    // An array a slice pattern took elements out of in some branches
    // (#1285): the rest stay in it.
    if let Some(left) = is.extract_rap().and_then(|r| r.elems_left()) {
        let mut moved: Vec<String> = Vec::new();
        for b in branch_history.iter().filter(|b| !b.diverges) {
            for p in elems_moved_before(&b.e_data, usize::MAX, true).0 {
                if !moved.contains(&p) {
                    moved.push(p);
                }
            }
        }
        if !moved.is_empty() {
            // What is surely still in it: not what is moved at the end
            // (`left`), nor, when every element is moved, the rest
            // (#1302: they are moved above or below the branch).
            return hover_messages::event_dot_branch_merge_elems_moved(var_name, &moved, &left.left, &left.payloads);
        }
    }
    let holder = is.extract_rap().is_some_and(|r| !r.is_ref() && r.holds_borrow());
    let kind = match join {
        Some(j) if j.cell.init == Init::Own && j.change.writes_on_all_paths && j.change.test_wrote
            && !j.change.staged_result =>
            return hover_messages::event_dot_branch_merge_assigned_in_test(var_name, j.change.test_writes_on_all_paths),
        Some(j) if j.change.staged_result || j.cell.init == Init::Own && j.change.writes_on_all_paths =>
            BranchJoin::BoundHere,
        Some(j) if j.cell.init == Init::Own => BranchJoin::Unchanged,
        Some(j) if j.cell.init == Init::Maybe && j.cell.cause == Some("imaginary") =>
            return match is.extract_rap() {
                // Says whether a later assignment lets it be used again.
                Some(rap) => hover_messages::event_dot_branch_merge_move(
                    var_name, rap.drops_nothing(), rap.is_mut(), &rap.drop_note(), None, Some(&j.cell)),
                None => hover_messages::state_maybe(var_name, j.cell.cause),
            },
        // The value of a conditional expression is sometimes staged in an
        // unnamed MIR slot. Its displayed arrival belongs to the result
        // arrows, before the later store to the named destination.
        _ => compute_branch_join(branch_history, ty, holder),
    };
    match kind {
        BranchJoin::MovedAfter if is.extract_rap().is_some_and(|r| r.is_split_scrutinee_temporary()) =>
            hover_messages::event_dot_branch_merge_temp_moved_out(var_name),
        BranchJoin::MovedAfter if join.is_none() => match is.extract_rap() {
            Some(rap) => hover_messages::event_dot_branch_merge_maybe_moved_no_snapshot(var_name, rap.drops_nothing(), &rap.drop_note()),
            None => hover_messages::state_maybe(var_name, Some("moved")),
        },
        BranchJoin::MovedAfter => hover_messages::event_dot_branch_merge_conditional_ends(),
        // A reference bound in every arm (`let msg = match n { .. => "a", .. }`)
        // holds a borrow, not ownership.
        BranchJoin::BoundHere if let Some(binding) = imaginary_arm.and_then(|arm| arm.initializer_bindings.get(var_name)) =>
            hover_messages::event_dot_imaginary_initializer(binding),
        BranchJoin::BoundHere => match is.extract_rap() {
            Some(rap) if rap.is_ref() =>
                hover_messages::event_dot_branch_merge_bound_ref(var_name, rap.is_mutref(), &diverged, imaginary_else),
            // A Copy value (`let n = match x { 0 => 1, _ => 2 };`) isn't
            // a resource anyone owns (#316).
            Some(rap) if rap.never_drops() || rap.is_plain_value() =>
                hover_messages::event_dot_branch_merge_bound_copy(var_name, &diverged, imaginary_else),
            _ => hover_messages::event_dot_branch_merge_bound(var_name, &diverged, imaginary_else),
        },
        // No branch that gets here moved it. One that never gets here
        // (`return s;`, #318) may have: then it still holds its value
        // on every path that does.
        BranchJoin::Unchanged => {
            let mut leavers = Vec::new();
            moved_by_leavers(branch_history, &mut leavers);
            if join.is_some_and(|j| j.change.moved && j.change.wrote) {
                // A branch that moved it gave it a new value after
                // (`if c { drop(s); s = …; }`, #1413): it holds one on
                // every path, but it was moved.
                hover_messages::event_dot_branch_merge_reassigned_after_move(var_name, &leavers)
            } else if leavers.is_empty() {
                // #179: this used to be an empty tooltip.
                hover_messages::event_dot_branch_merge_unchanged(var_name)
            } else {
                let is_ref = is.extract_rap().filter(|r| r.is_ref()).map(|r| r.is_mutref());
                hover_messages::event_dot_branch_merge_kept(
                    var_name, is_ref, is.extract_rap().map_or(false, |r| r.holds_value()), &leavers)
            }
        }
    }
}

// How each branch of type `!` that moved the variable left (`if c {
// take(s); return; }`), also ones nested in branches that get here.
fn moved_by_leavers(branch_history: &[BranchData], out: &mut Vec<Vec<BranchExit>>) {
    for b in branch_history {
        if b.diverges {
            if moves_somewhere(&b.e_data) {
                out.push(b.exits.clone());
            }
        } else {
            for (_, e) in &b.e_data {
                if let Event::Branch { branch_history, .. } = e {
                    moved_by_leavers(branch_history, out);
                }
            }
        }
    }
}

// Whether the branches that reach a conditional's join moved only a
// Box's contents (`let w = *e;`, `take(*e)`, #921), and not the Box
// itself, on some path.
fn moves_only_box_contents(branch_history: &[BranchData]) -> bool {
    moves_only_box_contents_impl(branch_history)
}

// Whether what last emptied a Box's column (in `history`, the events
// before the point in question) was a move of its whole contents (`let s
// = *b;`), not of the Box itself, nor a new value since (#1145): `Owner::
// box_contents_moved` says only that the body moves them out somewhere,
// and the Box may since have been given a new one and moved away whole
// (`b = Box::new(…); drop(b);`). `None`: neither found.
fn box_contents_moved_last(history: &[(usize, Event)]) -> Option<bool> {
    for (_, e) in history.iter().rev() {
        match e {
            Event::Move { is: ResourceTy::Deref(_), .. } => return Some(true),
            Event::Move { .. } | Event::ClosureCall { .. } | Event::OrAltMove { .. } => return Some(false),
            Event::Acquire { is: ResourceTy::Value(_), .. } | Event::Copy { is: ResourceTy::Value(_), .. } => return Some(false),
            Event::Branch { branch_history, .. } => {
                let ends: Vec<Option<bool>> = branch_history.iter().filter(|b| !b.diverges)
                    .map(|b| box_contents_moved_last(&b.e_data)).collect();
                // Every path that reaches here moved the Box away or
                // gave it a new value: settled. Some did and some
                // didn't: a drop flag's business, which this can't say
                // (`None`, as before #1145).
                if !ends.is_empty() && ends.iter().all(|e| *e == Some(false)) { return Some(false); }
                if ends.iter().any(|e| *e == Some(false)) { return None; }
                if ends.iter().any(|e| *e == Some(true)) { return Some(true); }
            }
            _ => {}
        }
    }
    None
}

// An empty Box on one path and a replaced or moved Box on another
// require a conditional drop description (#1220).
fn mixed_box_paths(ro: &ResourceAccessPoint, history: &[(usize, Event)]) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Holds { Kept, Reboxed, Emptied, Nothing }
    fn walk(history: &[(usize, Event)], mut cur: BTreeSet<Holds>) -> Option<BTreeSet<Holds>> {
        for (_, e) in history {
            cur = match e {
                Event::Move { is: ResourceTy::Deref(_), .. } => BTreeSet::from([Holds::Emptied]),
                Event::Move { .. } | Event::ClosureCall { .. } => BTreeSet::from([Holds::Nothing]),
                Event::Acquire { is: ResourceTy::Value(_), .. } | Event::Copy { is: ResourceTy::Value(_), .. }
                | Event::InitRefParam { .. } => BTreeSet::from([if cur.is_empty() { Holds::Kept } else { Holds::Reboxed }]),
                Event::Branch { branch_history, ty, .. } => {
                    let mut out = BTreeSet::new();
                    for b in branch_history.iter().filter(|b| !b.diverges) {
                        out.extend(walk(&b.e_data, cur.clone())?);
                    }
                    // The path past an `if` without `else` leaves it as it was.
                    if !all_paths_present(ty, branch_history.len()) || out.is_empty() {
                        out.extend(cur.iter().copied());
                    }
                    out
                }
                // Anything else this doesn't follow: no answer.
                Event::OrAltMove { .. } | Event::LoopExitMove { .. } | Event::LoopMaybeInit { .. } | Event::LoopExitKept { .. } => return None,
                _ => cur,
            };
        }
        Some(cur)
    }
    ro.box_contents_moved() && walk(history, BTreeSet::new()).is_some_and(|ends| {
        ends.contains(&Holds::Emptied) && (ends.contains(&Holds::Reboxed) || ends.contains(&Holds::Nothing))
    })
}

fn moves_only_box_contents_impl(branch_history: &[BranchData]) -> bool {
    // `None`: the Box itself is moved somewhere in `events`.
    fn contents_moved(events: &[(usize, Event)]) -> Option<bool> {
        let mut any = false;
        for (_, e) in events {
            match e {
                Event::Move { is: ResourceTy::Deref(_), .. } => any = true,
                // A let-chain's condition that moved `*e` before it
                // failed (`&& { let w = *e; … }`, #940): the path into
                // the else branch.
                Event::LoopExitMove { at: MaybeMovedAt::ChainFail { how: ChainFailHow::Out(_), .. }
                    | MaybeMovedAt::WhileCondFail { how: ChainFailHow::Out(_), .. }, .. } => any = true,
                Event::Move { .. } | Event::LoopExitMove { .. } | Event::OrAltMove { .. } | Event::ClosureCall { .. } => return None,
                Event::Branch { branch_history, .. } => {
                    for b in branch_history.iter().filter(|b| !b.diverges) {
                        any |= contents_moved(&b.e_data)?;
                    }
                }
                _ => {}
            }
        }
        Some(any)
    }
    let mut any = false;
    for b in branch_history.iter().filter(|b| !b.diverges) {
        match contents_moved(&b.e_data) {
            Some(moved) => any |= moved,
            None => return false,
        }
    }
    any
}

// How each branch of type `!` that gave the variable a value left (`if
// c { s = …; return; }`), also ones nested in branches that get here.
fn assigned_by_leavers(branch_history: &[BranchData], out: &mut Vec<Vec<BranchExit>>) {
    for b in branch_history {
        if b.diverges {
            let assigned = b.states.iter().any(|s| !matches!(
                s.2,
                State::OutOfScope | State::FullPrivilege { s: LineState::Uninit }
            )) || b.e_data.iter().any(|(_, e)| holds_value_after(false, e));
            if assigned {
                out.push(b.exits.clone());
            }
        } else {
            for (_, e) in &b.e_data {
                if let Event::Branch { branch_history, .. } = e {
                    assigned_by_leavers(branch_history, out);
                }
            }
        }
    }
}

// Whether the variable is moved anywhere in `events`, nested branches
// included.
fn moves_somewhere(events: &[(usize, Event)]) -> bool {
    events.iter().any(|(_, e)| match e {
        e if e.is_box_payload_move() => false,
        Event::Move { .. } | Event::LoopExitMove { .. } | Event::OrAltMove { tuple: None, .. } => true,
        Event::Branch { branch_history, .. } => branch_history.iter().any(|b| moves_somewhere(&b.e_data)),
        _ => false,
    })
}

// Walk the original ExternalEvents to find every capture (Move /
// StaticBorrow / MutableBorrow) at `line` whose target is the
// closure identified by `closure_hash`. Returns (upvar_name,
// kind_label) pairs in source order so the rendered list matches
// what the user wrote in the closure literal.
fn collect_closure_captures(
    visualization_data: &VisualizationData,
    closure_hash: u64,
    line: usize,
    bind: usize,
) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    // Also a closure made in a branch of a conditional (#1148).
    let ids = visualization_data.row_layout.closure_captures.get(&bind);
    let mut captures = all_external_events(visualization_data);
    captures.retain(|(l, ev)| ids.map_or(*l == line, |ids| ids.contains(&ev.get_id())));
    if let Some(ids) = ids {
        captures.sort_by_key(|(_, ev)| ids.iter().position(|id| *id == ev.get_id()));
    }
    for (_, ev) in captures {
        match ev {
            // `let h = g;` isn't a capture (#704).
            ExternalEvent::Move { closure_value: true, .. } | ExternalEvent::Copy { closure_value: true, .. } => {}
            ExternalEvent::Move { from, to, .. } if matches_closure(to, closure_hash) => {
                if let Some(name) = upvar_name(from) {
                    out.push((name, hover_messages::capture_verb_moved()));
                }
            }
            // A capture of `*r` through a reference r is a reborrow
            // of what r points to, not a borrow of r (#277).
            ExternalEvent::StaticBorrow { from, to, is_reborrow: true, .. } if matches_closure(to, closure_hash) => {
                if let Some(name) = upvar_name(from) {
                    out.push((format!("*{}", name), hover_messages::capture_verb_immutably_reborrowed()));
                }
            }
            ExternalEvent::MutableBorrow { from, to, is_reborrow: true, .. } if matches_closure(to, closure_hash) => {
                if let Some(name) = upvar_name(from) {
                    out.push((format!("*{}", name), hover_messages::capture_verb_mutably_reborrowed()));
                }
            }
            ExternalEvent::Copy { from, to, .. } if matches_closure(to, closure_hash) => {
                if let Some(name) = upvar_name(from) {
                    out.push((name, hover_messages::capture_verb_copied()));
                }
            }
            ExternalEvent::StaticBorrow { from, to, .. } if matches_closure(to, closure_hash) => {
                if let Some(name) = upvar_name(from) {
                    out.push((name, hover_messages::capture_verb_immutably_borrowed()));
                }
            }
            ExternalEvent::MutableBorrow { from, to, .. } if matches_closure(to, closure_hash) => {
                if let Some(name) = upvar_name(from) {
                    out.push((name, hover_messages::capture_verb_mutably_borrowed()));
                }
            }
            _ => {}
        }
    }
    out
}

// The `break` a borrow came from, when it is one of several `break`
// values reaching a loop's receiver (#657), or a call that loop is
// the argument of (#662). The lend event doesn't carry it, so look up
// the borrow it came from.
fn borrow_break_alt<'v>(id: usize, visualization_data: &'v VisualizationData) -> Option<&'v BreakAlt> {
    visualization_data.external_events.iter().find_map(|(_, ev)| match ev {
        ExternalEvent::StaticBorrow { id: i, break_alt: Some(alt), .. }
        | ExternalEvent::MutableBorrow { id: i, break_alt: Some(alt), .. }
        | ExternalEvent::PassByStaticReference { id: i, break_alt: Some(alt), .. }
        | ExternalEvent::PassByMutableReference { id: i, break_alt: Some(alt), .. } if *i == id => Some(alt),
        _ => None,
    })
}

// Source-side dot of a closure capture that reborrows through a
// reference (`|| r.push('a')` captures `*r`; #277). The lend event
// itself doesn't carry `is_reborrow`, so look up the capture event it
// came from by id.
fn closure_reborrow_lend_title(event: &Event, visualization_data: &VisualizationData) -> Option<String> {
    let (to, is, id, mutable) = match event {
        Event::StaticLend { to, is, id, .. } => (to, is, *id, false),
        Event::MutableLend { to, is, id, .. } => (to, is, *id, true),
        _ => return None,
    };
    if !to.is_closure() {
        return None;
    }
    let reborrow = visualization_data.external_events.iter().any(|(_, ev)| match ev {
        ExternalEvent::StaticBorrow { id: i, is_reborrow, .. }
        | ExternalEvent::MutableBorrow { id: i, is_reborrow, .. } => *i == id && *is_reborrow,
        _ => false,
    });
    if !reborrow {
        return None;
    }
    let r = is.extract_rap()?;
    Some(hover_messages::event_dot_capture_reborrow_through_ref(r.name(), &to.name(), mutable, r.points_to_copy()))
}

// Lender-side dots of a method call whose receiver r is a reference to
// a reference (`r.len()` with `r: &&String`, #952): the call borrows
// what `*r` points to. The lend / reacquire events don't carry
// `deep`, so look up the call event they came from by id.
fn deep_lend_title(event: &Event, visualization_data: &VisualizationData) -> Option<String> {
    let (other, is, id, mutable, end) = match event {
        Event::StaticLend { to, is, id, .. } => (to, is, *id, false, false),
        Event::MutableLend { to, is, id, .. } => (to, is, *id, true, false),
        Event::StaticReacquire { from, is, id, .. } => (from, is, *id, false, true),
        Event::MutableReacquire { from, is, id, .. } => (from, is, *id, true, true),
        _ => return None,
    };
    // (Also a call in a conditional's arm.)
    let deep = all_external_events(visualization_data).into_iter().find_map(|(_, ev)| match ev {
        ExternalEvent::PassByStaticReference { id: i, deep: Some(d), .. }
        | ExternalEvent::PassByMutableReference { id: i, deep: Some(d), .. } if *i == id => Some(d.clone()),
        _ => None,
    })?;
    // (Or a Box holding a reference, `k.push('!')` with `k: Box<&mut
    // String>`, #1282: the reference `*k` it holds says what the call
    // can do through k. Or an array element holding one, `k[0]`, #1319.)
    let r = match is {
        ResourceTy::Value(r) if r.is_ref() || deep.held_mut.is_some() => r,
        _ => return None,
    };
    let borrower = other.name();
    let pointer = deep.pointer(r.name());
    Some(if end {
        hover_messages::event_dot_reacquire_deep(&pointer, &borrower, deep.holds_value)
    } else {
        hover_messages::event_dot_lend_deep(r.name(), &pointer, &deep.via(r.name()), &borrower, mutable, deep.held_mut.unwrap_or(r.is_mutref()), deep.holds_value)
    })
}

// Source-side dot of a borrow kept by a closure a call returned (`let
// g = keep(|| s.len());`, #686): g didn't capture s itself (the closure
// literal did, or it borrows s through a reference it captured), so
// this reads like any other lend to g rather than "captured by closure
// g".
fn kept_closure_lend_title(event: &Event, name: &String, visualization_data: &VisualizationData) -> Option<String> {
    let (to, is, id, mutable) = match event {
        Event::StaticLend { to, is, id, .. } => (to, is, *id, false),
        Event::MutableLend { to, is, id, .. } => (to, is, *id, true),
        _ => return None,
    };
    if !to.is_closure() {
        return None;
    }
    let reborrow = visualization_data.external_events.iter().find_map(|(_, ev)| match ev {
        ExternalEvent::StaticBorrow { id: i, is_reborrow, via, .. }
        | ExternalEvent::MutableBorrow { id: i, is_reborrow, via, .. } if *i == id && returned_closure(via) => Some(*is_reborrow),
        _ => None,
    })?;
    let r = is.extract_rap()?;
    let holds_value = crate::svg_generator::data::lender_holds_value(is);
    if reborrow {
        return Some(hover_messages::event_dot_lend_through_ref(r.name(), &to.name(), mutable, r.is_mutref(), r.points_to_copy()));
    }
    if mutable {
        return Some(hover_messages::event_dot_mut_lend(name, &to.name(), holds_value));
    }
    // A `&T` lender was read-only already; a RefCell / Cell lender isn't
    // read-only at all (as in Event::StaticLend's own message).
    let omit_read_only = (r.is_ref() && !r.is_mutref()) || r.has_interior_mut();
    Some(hover_messages::event_dot_static_lend(name, &to.name(), omit_read_only, holds_value))
}

// Count of upvars move-captured by the closure identified by
// `closure_hash`. Zero ⇒ borrow-only or capture-less closure;
// distinguishes the scope-end "captured resources are dropped"
// message and the timeline state line ("owns N resources via
// capture") from their borrow-only fallbacks.
//
// A closure moved into this one as a whole (`let h = g;`, #704) brings
// what it owns along: h owns what g captured by move.
fn closure_move_capture_count(
    visualization_data: &VisualizationData,
    closure_hash: u64,
) -> usize {
    let events = all_external_events(visualization_data);
    // A capture by move brings a resource along only if what is moved
    // needs drop: not a reference (`move || r.push('!')` with `r: &mut
    // String` moves the reference; references never own, #1148), nor a
    // value with nothing to drop (#484).
    let owns_resource = |from: &ResourceTy| from.extract_rap()
        .is_none_or(|r| !r.is_ref() && !r.holds_value());
    fn count(events: &[&(usize, ExternalEvent)], owns_resource: &dyn Fn(&ResourceTy) -> bool, closure_hash: u64, depth: usize) -> usize {
        if depth > 8 {
            return 0;
        }
        events
            .iter()
            .map(|(line, ev)| match ev {
                // `let h = keep_mut(|| t.push('!'));` (#686): the closure a
                // call returned is moved out of the call, which captures
                // nothing. It owns what the closure literal passed to that
                // call on the same line moved in (none here: `&mut t`).
                ExternalEvent::Move { from: ResourceTy::Value(f), to, .. } if f.is_fn() && matches_closure(to, closure_hash) =>
                    events.iter()
                        .filter(|(l, e)| l == line && matches!(e,
                            ExternalEvent::Move { from, to: ResourceTy::Value(g), into_closure: true, .. }
                                if g.hash() == f.hash() && owns_resource(from)))
                        .count(),
                ExternalEvent::Move { from, to, closure_value: false, .. } if matches_closure(to, closure_hash) =>
                    usize::from(owns_resource(from)),
                ExternalEvent::Move { from, to, closure_value: true, .. } if matches_closure(to, closure_hash) =>
                    from.extract_rap().map_or(0, |g| count(events, owns_resource, *g.hash(), depth + 1)),
                _ => 0,
            })
            .sum()
    }
    count(&events, &owns_resource, closure_hash, 0)
}

/// Every external event, also those inside the branches of a
/// conditional (and of conditionals nested in those), in order. A
/// closure made or moved in an `if` / `match` arm records its captures
/// there (#1148, #1159).
fn all_external_events(visualization_data: &VisualizationData) -> Vec<&(usize, ExternalEvent)> {
    fn walk<'a>(events: &'a [(usize, ExternalEvent)], out: &mut Vec<&'a (usize, ExternalEvent)>) {
        for e in events {
            out.push(e);
            if let ExternalEvent::Branch { branches, .. } = &e.1 {
                for b in branches {
                    walk(&b.e_data, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&visualization_data.external_events, &mut out);
    out
}

/// The `let h = g;` event (#704) with id `id`, if that's what it is:
/// (g, h, whether it's a copy).
fn closure_value_event(visualization_data: &VisualizationData, id: usize) -> Option<(&ResourceTy, &ResourceTy, bool)> {
    all_external_events(visualization_data).into_iter().find_map(|(_, ev)| match ev {
        ExternalEvent::Copy { from, to, closure_value: true, id: i, .. } if *i == id => Some((from, to, true)),
        ExternalEvent::Move { from, to, closure_value: true, id: i, .. } if *i == id => Some((from, to, false)),
        _ => None,
    })
}

/// The number of borrows closure `h` gives back (its captures by
/// reference, or those of the closure it was copied or moved from).
fn closure_borrow_count(visualization_data: &VisualizationData, closure_hash: u64) -> usize {
    let mut lenders: Vec<String> = Vec::new();
    for (_, ev) in all_external_events(visualization_data) {
        if let ExternalEvent::StaticDie { from, to, .. } | ExternalEvent::MutableDie { from, to, .. } = ev {
            if matches_closure(from, closure_hash) && !lenders.contains(&to.name()) {
                lenders.push(to.name());
            }
        }
    }
    lenders.len()
}

/// The dots of `let h = g;` with g a closure (#704): a plain move or
/// copy of g's value, not a capture. h holds the borrows g's closure
/// made, so its dot says so.
fn closure_value_title(event: &Event, my_name: &String, visualization_data: &VisualizationData) -> Option<String> {
    let id = match event {
        Event::Duplicate { id, .. } | Event::Move { id, .. } | Event::Acquire { id, .. } | Event::Copy { id, .. } => *id,
        _ => return None,
    };
    let (g, h, is_copy) = closure_value_event(visualization_data, id)?;
    let g_rap = g.extract_rap()?;
    let h_rap = h.extract_rap()?;
    let owns = closure_move_capture_count(visualization_data, *g_rap.hash()) > 0;
    Some(match event {
        Event::Duplicate { .. } => hover_messages::event_dot_copy_to(my_name),
        Event::Move { .. } => hover_messages::event_dot_move_to(my_name, !owns),
        _ => {
            let borrows = closure_borrow_count(visualization_data, *h_rap.hash());
            // `h = g;` after h had a value (a Copy closure: the old
            // value has nothing to drop).
            let assigned = all_external_events(visualization_data).into_iter().any(|(_, ev)| match ev {
                ExternalEvent::Bind { to, id: i, .. } | ExternalEvent::Copy { to, id: i, .. } | ExternalEvent::Move { to, id: i, .. } =>
                    *i < id && matches_closure(to, *h_rap.hash()),
                _ => false,
            });
            hover_messages::event_dot_closure_value_acquire(my_name, g_rap.name(), is_copy, borrows, assigned)
        }
    })
}

// A holder (#336) whose value was created without a borrow (`let mut
// refs = Vec::new();`): on the line it gets its value, it neither
// receives a borrow arrow nor takes over another holder's value. Its
// borrows arrive later, through `refs.push(&s)` (#392).
fn holder_starts_empty(visualization_data: &VisualizationData, rap: &ResourceAccessPoint) -> bool {
    let Some(timeline) = visualization_data.timelines.get(rap.hash()) else { return false };
    let Some((first, _)) = timeline.history.first() else { return false };
    let mut created = false;
    for (line, ev) in timeline.history.iter().take_while(|(l, _)| l == first) {
        let _ = line;
        match ev {
            Event::StaticBorrow { .. } | Event::MutableBorrow { .. } => return false,
            Event::Acquire { from: ResourceTy::Value(r), .. } if !r.is_fn() => return false,
            Event::Acquire { .. } => created = true,
            _ => {}
        }
    }
    created
}

// A holder that never receives a borrow we can draw (`map.entry(word)`
// where `word`'s lender has no column; #392): drawn as a plain owner.
fn holder_never_borrows(visualization_data: &VisualizationData, rap: &ResourceAccessPoint) -> bool {
    // (`let k2 = k;` takes over the borrow k's value holds, #1282.)
    // (Or that took the reference a `&mut` moved into it held:
    // `vec![r]` with r a parameter, #1289.)
    // (Or borrowed in a branch of the conditional it is bound to:
    // `let v = if c { vec![&a] } else { vec![&b] };`, #1404.)
    fn borrows(history: &[(usize, Event)]) -> bool {
        history.iter().any(|(_, e)| match e {
            Event::StaticBorrow { .. } | Event::MutableBorrow { .. } => true,
            Event::Acquire { from: ResourceTy::Value(f), .. } => {
                (!f.is_ref() && f.holds_borrow()) || f.is_mutref()
            }
            Event::Branch { branch_history, .. } => branch_history.iter().any(|b| borrows(&b.e_data)),
            _ => false,
        })
    }
    visualization_data.timelines.get(rap.hash()).is_none_or(|t| !borrows(&t.history))
}

// Title for a vertical timeline segment. Routes around the generic
// `state.print_message_with_name` for closure bindings so the
// FullPrivilege segment reads as "f owns a closure (which owns a
// resource)?" instead of the misleading generic "f is the owner
// of the resource" — `f` doesn't own the upvar's resource directly,
// it owns the closure value, and that closure may or may not in
// turn own a captured resource.
fn timeline_segment_title(
    state: &State,
    rap: &ResourceAccessPoint,
    visualization_data: &VisualizationData,
    line: Option<usize>,
) -> String {
    if let State::FullPrivilege { s: LineState::Uninit } = state {
        return state.print_message_with_name(rap.name());
    }
    if matches!(state, State::PartialPrivilege { .. } | State::RevokedPrivilege { .. }) && !rap.is_ref() {
        if let Some((into, mutable, sources, chain)) = or_alt_lend_at(visualization_data, rap, line) {
            if let Some(chain) = chain {
                return hover_messages::state_chain_ref_lent(rap.name(), into.name(), sources, chain, mutable, rap.holds_value());
            }
            if let [source] = sources {
                return hover_messages::event_dot_single_pattern_lend(rap.name(), into.name(), source, mutable, false);
            }
            return hover_messages::state_or_alt_lent(rap.name(), into.name(), mutable, rap.holds_value());
        }
    }
    if let (State::RevokedPrivilege { .. }, Some(line)) = (state, line) {
        return lent_out_title(rap, visualization_data, line, &[]);
    }
    if let Some(title) = line.and_then(|l| loop_iterator_title(state, rap, visualization_data, l)) {
        return title;
    }
    if rap.is_closure() {
        if let State::FullPrivilege { .. } = state {
            let name = rap.name().to_owned();
            let count = closure_move_capture_count(visualization_data, *rap.hash());
            if count > 0 {
                return hover_messages::state_closure_full_privilege_with_resource(&name, count);
            }
            return hover_messages::state_closure_full_privilege_no_resource(&name);
        }
    }
    // A value that holds a borrow (`let it = v.iter();`, #336) owns
    // its value, not the data it borrows.
    if rap.holds_borrow() {
        if let State::FullPrivilege { .. } = state {
            if let Some(copy) = option_ref_copy(visualization_data, rap.hash(), rap.name(), line) {
                return hover_messages::state_value_holds_loans(rap.name(), copy, &[], false);
            }
            if visualization_data.row_layout.closure_result_holders.contains(rap.hash()) {
                return hover_messages::state_value_holds_loans(rap.name(), rap.is_copy_owner(), &[], false);
            }
            // `let mut refs = Vec::new();` starts out holding no
            // reference; `refs.push(&s)` stores one later (#392).
            if holder_starts_empty(visualization_data, rap) {
                if !holder_never_borrows(visualization_data, rap) {
                    return hover_messages::state_holder_starts_empty(rap.name());
                }
            } else {
                return hover_messages::state_holder_full_privilege(rap.name());
            }
        }
    }
    // Copy-typed owners (i32 etc.) — "the owner of the resource"
    // language doesn't fit primitives; route to the value-flavoured
    // wording so the lifeline tooltip stays consistent with the
    // event-dot tooltip on the same column.
    if rap.is_copy_owner() {
        match state {
            State::FullPrivilege { .. } =>
                return hover_messages::state_full_privilege_copyable(rap.name()),
            State::PartialPrivilege { .. } =>
                return hover_messages::state_partial_privilege(rap.name(), true),
            _ => {}
        }
    }
    // A non-Copy owner with nothing to drop holds a value too (#484).
    if rap.is_plain_value() {
        match state {
            State::FullPrivilege { .. } => return hover_messages::state_full_privilege_value(rap.name()),
            State::PartialPrivilege { .. } => return hover_messages::state_partial_privilege(rap.name(), true),
            State::ResourceMoved { dropped_by_hand: false, .. } => return hover_messages::state_value_moved(rap.name(), rap.is_mut()),
            State::RevokedPrivilege { .. } => return hover_messages::state_value_revoked(rap.name()),
            _ => {}
        }
    }
    // References (`&mut T` / `&T` bindings, `&mut self`, ref params,
    // `for i in &mut v`) never own their referent, so the generic
    // "is the owner of the resource" FullPrivilege text is false.
    if let State::FullPrivilege { .. } = state {
        match rap {
            ResourceAccessPoint::MutRef(_) =>
                return hover_messages::state_mut_ref_full_privilege(rap.name(), rap.points_to_copy()),
            ResourceAccessPoint::StaticRef(_) =>
                return hover_messages::state_static_ref_full_privilege(rap.name(), rap.points_to_copy()),
            _ => {}
        }
    }
    // A reference that was moved out never had ownership to lose (#292).
    if let (State::ResourceMoved { .. }, true) = (state, rap.is_ref()) {
        return hover_messages::state_ref_moved(rap.name(), rap.is_mutref());
    }
    // A `&T` is in PartialPrivilege from the start (it shares its
    // referent) and stays there when it's copied; it never had a
    // resource of its own to share (#274).
    if let (State::PartialPrivilege { .. }, ResourceAccessPoint::StaticRef(_)) = (state, rap) {
        return hover_messages::state_static_ref_full_privilege(rap.name(), rap.points_to_copy());
    }
    // A `&mut` whose referent is shared through it (`let r3 = &*r;`)
    // is read-only meanwhile; it has no resource of its own (#313).
    if let (State::PartialPrivilege { .. }, ResourceAccessPoint::MutRef(_)) = (state, rap) {
        return hover_messages::state_mut_ref_partial_privilege(rap.name(), rap.points_to_copy());
    }
    state.print_message_with_name(rap.name())
}

fn option_ref_copy(vd: &VisualizationData, hash: &u64, name: &str, row: Option<usize>) -> Option<bool> {
    let fn_line = *vd.fn_source_lines.get(hash)?;
    vd.state_table.option_ref_copy(fn_line, name, row.and_then(|r| rv_attrs::line_of(r, &vd.row_lines)))
}

fn borrow_result_may_be_none(events: &[(usize, ExternalEvent)], id: usize) -> bool {
    events.iter().any(|(_, event)| match event {
        ExternalEvent::StaticBorrow { id: i, via: Some(via), .. }
        | ExternalEvent::MutableBorrow { id: i, via: Some(via), .. } if *i == id => {
            !via.tie.constructs_variant || !via.passed_on.is_empty()
        }
        ExternalEvent::Branch { branches, .. } => branches.iter().any(|b| borrow_result_may_be_none(&b.e_data, id)),
        _ => false,
    })
}

/// Every event of a column, those in its conditionals' arms included,
/// with its line. Of a conditional on `path` (its split line, and an
/// arm), only that arm's.
fn flat_history<'a>(history: &'a [(usize, Event)], path: &[(usize, usize)], out: &mut Vec<(usize, &'a Event)>) {
    for (l, e) in history {
        out.push((*l, e));
        if let Event::Branch { branch_history, split_point, .. } = e {
            let on_path = path.iter().find(|(split, _)| split == split_point).map(|(_, arm)| *arm);
            for (i, b) in branch_history.iter().enumerate() {
                if on_path.is_none_or(|arm| arm == i) {
                    flat_history(&b.e_data, path, out);
                }
            }
        }
    }
}

/// A conditional's arm's segment of a column while it is mutably lent
/// out: as `timeline_segment_title`, on the arm `path`.
fn revoked_title(rap: &ResourceAccessPoint, visualization_data: &VisualizationData, row: usize, path: &[(usize, usize)]) -> String {
    if !rap.is_ref() {
        if let Some((into, mutable, sources, chain)) = or_alt_lend_at(visualization_data, rap, Some(row)) {
            if let Some(chain) = chain {
                return hover_messages::state_chain_ref_lent(rap.name(), into.name(), sources, chain, mutable, rap.holds_value());
            }
            if let [source] = sources {
                return hover_messages::event_dot_single_pattern_lend(rap.name(), into.name(), source, mutable, false);
            }
            return hover_messages::state_or_alt_lent(rap.name(), into.name(), mutable, rap.holds_value());
        }
    }
    lent_out_title(rap, visualization_data, row, path)
}

/// The `&mut` reference that holds what `r` held on line `from`, after
/// the moves of r's value up to `line` (#1237): `r2` for `let r2 = r;`,
/// `w.f` for `let w = G { f: r };`, through several moves. `None` once it
/// is moved into anything but a reference (a call, a holder value: that
/// one's own lend names it, #1128).
fn holder_after_moves(visualization_data: &VisualizationData, r: &ResourceAccessPoint, from: usize, line: usize) -> Option<ResourceAccessPoint> {
    let mut cur = r.clone();
    // The first hop may happen on the lend's own line; each later one
    // strictly after the previous (`(m1, m2) = (m2, m1)` moves each into
    // the other on one line: one hop each).
    let mut after = from.checked_sub(1);
    for _ in 0..16 {
        let next = visualization_data.timelines.get(cur.hash()).and_then(|t| t.history.iter()
            .find_map(|(l, e)| match e {
                Event::Move { to, .. } if after.is_none_or(|a| *l > a) && *l <= line => Some((*l, to.clone())),
                _ => None,
            }));
        match next {
            None => return Some(cur),
            Some((l, ResourceTy::Value(t))) if t.is_mutref() && t.hash() != cur.hash() => { cur = t; after = Some(l); }
            Some(_) => return None,
        }
    }
    None
}

/// Rows strictly inside `start..end` where a `&mut` reference's value is
/// moved into another reference (`let r2 = r;`, `G { f: r }`): a column
/// mutably lent out over them is cut there, so its text can name the new
/// holder (#1237). (Pieces with the same text are joined again.)
fn ref_move_rows(visualization_data: &VisualizationData, start: usize, end: usize) -> Vec<usize> {
    let mut rows: Vec<usize> = visualization_data.timelines.values()
        .flat_map(|t| t.history.iter())
        .filter_map(|(l, e)| match e {
            Event::Move { is: ResourceTy::Value(f), to: ResourceTy::Value(t), .. }
                if f.is_mutref() && t.is_mutref() && *l > start && *l < end => Some(*l),
            _ => None,
        })
        .collect();
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// Whether the reference `r`'s value is moved away (at the top level of
/// its column) on a line in `from..=line`.
fn moved_between(visualization_data: &VisualizationData, r: &ResourceAccessPoint, from: usize, line: usize) -> bool {
    visualization_data.timelines.get(r.hash()).is_some_and(|t| t.history.iter()
        .any(|(l, e)| (from..=line).contains(l) && matches!(e, Event::Move { .. })))
}

/// The text of a column's dotted segment from row `line` while it is
/// mutably lent out (#722), naming who holds the borrow there: a `for`
/// loop's iterator, or the one variable or closure whose `&mut` borrow
/// of it is live on that row. A call that hands its borrow on to its
/// result (`let w = first_mut(&mut v);`) isn't named; with several
/// holders (`it` and the `y` taken out of it) none is.
fn lent_out_title(rap: &ResourceAccessPoint, visualization_data: &VisualizationData, line: usize, path: &[(usize, usize)]) -> String {
    let iterator = loop_iterator_holds(visualization_data, rap, line).is_some();
    let replay = if iterator { ReplayedLend::default() } else { replayed_lent_holder(rap, visualization_data, line, path) };
    let to = match replay.holder.as_ref() {
        _ if iterator => hover_messages::LentTo::LoopIterator,
        Some((name, true)) => hover_messages::LentTo::Closure(name),
        Some((_, false)) if replay.qualified_result => hover_messages::LentTo::Unknown,
        Some((name, false)) => hover_messages::LentTo::Var(name),
        None => hover_messages::LentTo::Unknown,
    };
    // Deref coercion can leave a loan of the binding live while the
    // result accesses its referent. Only claim this at a known boundary.
    let table = &visualization_data.state_table;
    let binding_borrowed = visualization_data.fn_source_lines.get(rap.hash())
        .zip(rv_attrs::line_of(line, &visualization_data.row_lines))
        .and_then(|(&f, r)| Some((f, table.cell(f, rap.name(), r, r)?)))
        .is_some_and(|(f, c)| !c.may_lend && c.lent.iter().any(|l|
            table.loan(f, l.loan).is_some_and(|i| i.whole.contains(rap.name()))));
    hover_messages::state_lent_out(rap.name(), to, lent_place(rap, replay.of_ref_var, binding_borrowed), replay.alt)
}

fn lent_place(rap: &ResourceAccessPoint, of_ref_var: bool, binding_borrowed: bool) -> hover_messages::LentPlace {
    if rap.is_ref() && (of_ref_var || !rap.is_mutref()) {
        hover_messages::LentPlace::RefBinding
    } else if rap.is_mutref() {
        hover_messages::LentPlace::ThroughRef { pointee_copy: rap.points_to_copy(), binding_borrowed }
    } else {
        hover_messages::LentPlace::Owned { holds_value: rap.holds_value() }
    }
}

/// The holder and source qualification the legacy title would use.
#[derive(Default)]
struct ReplayedLend<'a> {
    holder: Option<(String, bool)>,
    qualified_result: bool,
    of_ref_var: bool,
    alt: Option<&'a BreakAlt>,
}

fn replayed_lent_holder<'a>(
    rap: &ResourceAccessPoint,
    visualization_data: &'a VisualizationData,
    line: usize,
    path: &[(usize, usize)],
) -> ReplayedLend<'a> {
    let Some(timeline) = visualization_data.timelines.get(rap.hash()) else {
        return ReplayedLend::default();
    };
    let mut history = Vec::new();
    flat_history(&timeline.history, path, &mut history);
    history.sort_by_key(|(l, _)| *l);
    // Who holds a `&mut` borrow of it after the events up to `line`.
    let key = |to: &ResourceTy| match to {
        ResourceTy::Value(r) => Some((r.name().to_owned(), r.is_closure(), r.is_fn())),
        ResourceTy::Deref(r) => Some((format!("*{}", r.name()), false, false)),
        _ => None,
    };
    // Each live lend's holder, and whether it borrows the reference
    // column itself (`&mut r`, #914).
    let mut holders: Vec<(Option<(String, bool, bool)>, bool, usize)> = Vec::new();
    // Holders reached by following a reference's moves (#1237).
    let mut moved_to: Vec<Option<(String, bool, bool)>> = Vec::new();
    for (l, e) in history.iter().filter(|(l, _)| *l <= line) {
        match e {
            // A `&mut` reference holder whose value was moved on since
            // holds nothing any more: into another reference (`let r2 =
            // r;`, `G { f: r }`), that one holds the borrow now (#1237);
            // into a call's result (`let k = id(r);`, #1128), the result's
            // holder has a lend of its own.
            Event::MutableLend { to: ResourceTy::Value(r), of_ref_var, id, .. } if r.is_mutref() && moved_between(visualization_data, r, *l, line) => {
                if let Some(h) = holder_after_moves(visualization_data, r, *l, line) {
                    let k = key(&ResourceTy::Value(h));
                    moved_to.push(k.clone());
                    holders.push((k, *of_ref_var, *id));
                }
            }
            Event::MutableLend { to, of_ref_var, id, .. } => holders.push((key(to), *of_ref_var, *id)),
            Event::MutableReacquire { from, .. } => {
                let k = key(from);
                if let Some(i) = holders.iter().position(|h| h.0 == k) {
                    holders.remove(i);
                }
            }
            _ => {}
        }
    }
    // A holder reached through moves gives way to a later lend still
    // live (`let r3 = pass(r2);` reborrows through r2, which r's value
    // moved into: r3 is named, as before #1237).
    if holders.len() > 1 && holders.iter().any(|h| !moved_to.contains(&h.0)) {
        holders.retain(|h| !moved_to.contains(&h.0));
    }
    let of_ref_var = !holders.is_empty() && holders.iter().all(|h| h.1);
    // The same event ID that qualifies the lender dot also qualifies
    // its segment. Only one of the value breaks actually supplied it.
    let alt = holders.first().and_then(|h| borrow_break_alt(h.2, visualization_data))
        .filter(|alt| holders.iter().all(|h| borrow_break_alt(h.2, visualization_data) == Some(*alt)));
    // The call's result may be shared or have multiple possible lenders.
    // Preserve its binding identity for other consumers of the replay.
    let qualified_result = holders.iter().any(|h| visualization_data.row_layout.result_lend_titles.contains_key(&h.2));
    // A call that handed its borrow on to its result isn't the holder.
    let mut holders: Vec<(String, bool)> = holders.into_iter()
        .map(|h| h.0)
        .filter(|h| !h.as_ref().is_some_and(|h| h.2))
        .map(|h| h.map_or((String::new(), false), |h| (h.0, h.1)))
        .collect();
    holders.sort();
    holders.dedup();
    let holder = match holders.as_slice() {
        [one] if !one.0.is_empty() => Some(one.clone()),
        _ => None,
    };
    ReplayedLend { holder, qualified_result, of_ref_var, alt }
}

/// The lender's dot of a borrow of a whole struct or tuple by a pattern
/// binding that points to one part of it (`let (a, b) = &mut t;`, #1130):
/// "t.0 is mutably borrowed by a, as part of a mutable borrow of all of t".
fn part_lend_title(event: &Event, visualization_data: &VisualizationData) -> Option<String> {
    let (id, mutable) = match event {
        Event::StaticLend { id, .. } => (*id, false),
        Event::MutableLend { id, .. } => (*id, true),
        _ => return None,
    };
    fn find(events: &[(usize, ExternalEvent)], id: usize) -> Option<(String, String, String)> {
        events.iter().find_map(|(_, ev)| match ev {
            ExternalEvent::StaticBorrow { id: i, from, to, via: Some(v), .. }
            | ExternalEvent::MutableBorrow { id: i, from, to, via: Some(v), .. }
                if *i == id && v.carrier == ViaCarrier::Part => Some((from.name(), v.func.clone(), to.name())),
            ExternalEvent::Branch { branches, .. } => branches.iter().find_map(|b| find(&b.e_data, id)),
            _ => None,
        })
    }
    let (whole, part, to) = find(&visualization_data.external_events, id)?;
    Some(hover_messages::event_dot_lend_part(&whole, &part, &to, mutable))
}

/// The end of a shared reborrow made through a `&mut` reference that was
/// then given another reference (`let first = &*r; r = &mut b;`, #1182):
/// it gives back the lender's mutable loan (its use kept that loan
/// alive), but its own borrow was an immutable one.
fn reborrow_kept_mut_loan_title(event: &Event, before: &[(usize, Event)], name: &String, rap: &ResourceAccessPoint) -> Option<String> {
    let Event::MutableDie { to: ResourceTy::Value(lender), .. } = event else { return None };
    if !rap.is_ref() || rap.is_mutref() {
        return None;
    }
    let through = before.iter().rev().find_map(|(_, e)| match e {
        Event::StaticBorrow { from, .. } => Some(Some(from)),
        Event::MutableBorrow { .. } => Some(None),
        _ => None,
    })??;
    let r = match through {
        ResourceTy::Deref(r) | ResourceTy::Value(r) | ResourceTy::DerefField(r, ..) | ResourceTy::Index(r, ..) if r.is_mutref() => r,
        _ => return None,
    };
    Some(hover_messages::event_dot_reborrow_kept_mut_loan(name, r.name(), lender.name()))
}

/// The dot of the borrow a `for` loop's head `&v` / `&mut v` makes when
/// only the loop's iterator holds it (`for &y in &v`; #885): the
/// iterator has no column, so say it is the borrower.
fn loop_head_lend_title(event: &Event, name: &String, rap: &ResourceAccessPoint, visualization_data: &VisualizationData) -> Option<String> {
    fn is_loop_head_lend(events: &[(usize, ExternalEvent)], lend: usize) -> bool {
        events.iter().any(|(_, ev)| match ev {
            ExternalEvent::StaticDie { static_lender: Some(StaticLender::LoopHeadBorrow { lend: l }), .. }
            | ExternalEvent::MutableDie { static_lender: Some(StaticLender::LoopHeadBorrow { lend: l }), .. } => *l == lend,
            ExternalEvent::Branch { branches, .. } => branches.iter().any(|b| is_loop_head_lend(&b.e_data, lend)),
            _ => false,
        })
    }
    let (id, is_mut) = match event {
        Event::StaticLend { to: ResourceTy::Anonymous, id, .. } => (*id, false),
        Event::MutableLend { to: ResourceTy::Anonymous, id, .. } => (*id, true),
        _ => return None,
    };
    is_loop_head_lend(&visualization_data.external_events, id).then(|| {
        hover_messages::event_dot_lend_to_loop_iterator(name, is_mut, rap.is_copy_owner() || rap.is_plain_value())
    })
}

/// The dots of a `&mut` reference whose pointee a `for` loop's
/// iterator reborrows (`for e in v.iter_mut()` with `v: &mut Vec<_>`;
/// #1046, or `v.iter()`, shared; #1059), or a field of whose pointee it
/// borrows (`self.items.iter_mut()`; #1060): the head's call lends it
/// there, but the loop's iterator holds the reborrow until the loop
/// ends (`StaticLender::LoopIterator`). Also a `&T` passed as it is to
/// the head's adapter (`a.iter_mut().zip(b)`; #1076), or whose pointee
/// the head's call reads (`v.iter()` with `v: &Vec<_>`; #1081): the
/// iterator holds a copy of it.
fn loop_iterator_ref_title(event: &Event, line: usize, rap: &ResourceAccessPoint, visualization_data: &VisualizationData) -> Option<String> {
    if !rap.is_ref() {
        return None;
    }
    // What the loop's iterator borrows: all of what rap points to
    // (None), or the field `place` of it.
    fn lent_place(is: &ResourceTy) -> Option<Option<&String>> {
        match is {
            ResourceTy::Value(_) => Some(None),
            ResourceTy::DerefField(_, place, _, _) => Some(Some(place)),
            _ => None,
        }
    }
    // The end, where the loop ends, of a borrow its iterator holds.
    fn loop_end(ev: &ExternalEvent) -> Option<(usize, &ResourceTy, bool, &String)> {
        match ev {
            ExternalEvent::MutableDie { to, static_lender: Some(StaticLender::LoopIterator { call }), id, .. } => Some((*id, to, true, call)),
            ExternalEvent::StaticDie { to, static_lender: Some(StaticLender::LoopIterator { call }), id, .. } => Some((*id, to, false, call)),
            _ => None,
        }
    }
    fn loop_ends<'a>(events: &'a [(usize, ExternalEvent)], out: &mut Vec<(usize, &'a ResourceTy, bool, &'a String)>) {
        for (_, ev) in events {
            if let Some(end) = loop_end(ev) {
                out.push(end);
            } else if let ExternalEvent::Branch { branches, .. } = ev {
                for b in branches {
                    loop_ends(&b.e_data, out);
                }
            }
        }
    }
    let mut all_ends = Vec::new();
    loop_ends(&visualization_data.external_events, &mut all_ends);
    let same = |a: &ResourceTy, b: &ResourceTy| a.extract_rap().is_some_and(|r| r.hash() == rap.hash())
        && b.extract_rap().is_some_and(|r| r.hash() == rap.hash())
        && lent_place(a).is_some() && lent_place(a) == lent_place(b);
    match event {
        // The head's call (`iter_mut`), when the loop's end gives back
        // what it lent: no other lend to that call comes between.
        Event::MutableLend { to: ResourceTy::Value(ResourceAccessPoint::Function(f)), is, .. }
        | Event::StaticLend { to: ResourceTy::Value(ResourceAccessPoint::Function(f)), is, .. } => {
            let is_mut = matches!(event, Event::MutableLend { .. });
            let ids: Vec<usize> = all_ends.iter()
                .filter(|(_, to, m, call)| *m == is_mut && **call == f.name && same(to, is))
                .map(|(id, ..)| *id).collect();
            let history = &visualization_data.timelines.get(rap.hash())?.history;
            let ends: Vec<usize> = history.iter().filter_map(|(l, e)| match e {
                Event::MutableReacquire { id, .. } | Event::StaticReacquire { id, .. } if ids.contains(id) => Some(*l),
                _ => None,
            }).collect();
            let lends_to_call = |lo: usize, hi: usize| history.iter().any(|(l, e)| *l > lo && *l <= hi && matches!(e,
                Event::MutableLend { to: ResourceTy::Value(ResourceAccessPoint::Function(g)), is: i, .. }
                | Event::StaticLend { to: ResourceTy::Value(ResourceAccessPoint::Function(g)), is: i, .. } if g.name == f.name && same(i, is)));
            if !ends.iter().any(|&end| end >= line && !lends_to_call(line, end)) {
                return None;
            }
            Some(match lent_place(is)? {
                None => hover_messages::event_dot_lend_through_ref_to_loop_iterator(rap.name(), is_mut, rap.is_mutref(), rap.points_to_copy()),
                Some(place) => hover_messages::event_dot_lend_field_through_ref_to_loop_iterator(place, is_mut, rap.is_mutref()),
            })
        }
        Event::MutableReacquire { from: ResourceTy::Anonymous, id, still_borrowed_by, is, .. }
        | Event::StaticReacquire { from: ResourceTy::Anonymous, id, still_borrowed_by, is, .. } if still_borrowed_by.is_empty() => {
            if !all_ends.iter().any(|(i, ..)| i == id) {
                return None;
            }
            Some(match lent_place(is)? {
                None => hover_messages::event_dot_loop_iterator_reacquire_through_ref(rap.name(), rap.points_to_copy()),
                Some(place) => hover_messages::event_dot_loop_iterator_reacquire_field_through_ref(place),
            })
        }
        _ => None,
    }
}

// The or-pattern reference (and whether its borrow is mutable) whose
// possible borrow puts `rap` in a lent state on the segment starting
// after `line` (#565): the last borrow of `rap` that starts at or
// before `line` is an `OrAltLend`. Without a line, `rap` must be lent
// only that way.
/// A segment of `rap` from row `line` that only a `for` loop's iterator
/// borrows (#871): the iterator has no column, so "being shared by one
/// or more variables" sent the reader looking for a variable that isn't
/// there. Not for a `&mut` reference whose pointee the iterator
/// reborrows (`v.iter()` with `v: &mut Vec<_>`; #1059): its own state
/// text ("…currently shared through it, so v can only read it") says it.
/// (When the iterator borrows only a field of what a reference points
/// to, `self.items.iter()`, see `field_lent_through_ref_title`.)
fn loop_iterator_title(state: &State, rap: &ResourceAccessPoint, visualization_data: &VisualizationData, line: usize) -> Option<String> {
    if let Some(title) = field_lent_through_ref_title(state, rap, visualization_data, line) {
        return Some(title);
    }
    let State::PartialPrivilege { s } = state else { return None };
    if matches!(s, LineState::Gray) {
        return None;
    }
    match (loop_iterator_holds(visualization_data, rap, line)?, rap.is_ref()) {
        (None, false) => Some(hover_messages::state_borrowed_by_loop_iterator(rap.name(), rap.is_copy_owner() || rap.is_plain_value())),
        _ => None,
    }
}

/// A segment of the `&mut` reference `rap` from row `line` while only
/// a field of what it points to is borrowed (#1073): by a `for` loop's
/// iterator (`for e in self.items.iter_mut()`; #1060), or by a
/// reference variable (`let r = &mut self.items;`, `let first =
/// self.items.first_mut().unwrap();`). rustc restricts only that field
/// (`self.n += 1` compiles, `self.items.len()` doesn't), so rap keeps
/// its state and line style (see `calc_state`); the text names the
/// field. None when no field borrow is live there.
fn field_lent_through_ref_title(state: &State, rap: &ResourceAccessPoint, visualization_data: &VisualizationData, line: usize) -> Option<String> {
    if !rap.is_mutref() || !matches!(state, State::FullPrivilege { s: LineState::Full } | State::PartialPrivilege { s: LineState::Full }) {
        return None;
    }
    fn flat<'a>(history: &'a [(usize, Event)], out: &mut Vec<(usize, &'a Event)>) {
        for (l, e) in history {
            out.push((*l, e));
            if let Event::Branch { branch_history, .. } = e {
                for b in branch_history {
                    flat(&b.e_data, out);
                }
            }
        }
    }
    let timeline = visualization_data.timelines.get(rap.hash())?;
    let mut history = Vec::new();
    flat(&timeline.history, &mut history);
    let field = |is: &ResourceTy| match is {
        ResourceTy::DerefField(r, place, _, _) if r.hash() == rap.hash() => Some(place.clone()),
        _ => None,
    };
    // The field borrows live at `line`.
    let mut live: Vec<(usize, String, bool, Option<String>)> = Vec::new();
    for &(l, e) in &history {
        if l > line {
            continue;
        }
        let (to, is, is_mut) = match e {
            Event::MutableLend { to, is, .. } => (to, is, true),
            Event::StaticLend { to, is, .. } => (to, is, false),
            _ => continue,
        };
        let Some(place) = field(is) else { continue };
        let borrower = match to {
            // A reference variable: live until it gives the field back.
            ResourceTy::Value(b) if !b.is_fn() => {
                let back = history.iter().any(|&(l2, e2)| l2 >= l && l2 <= line && match e2 {
                    Event::MutableReacquire { from, is: i, .. } | Event::StaticReacquire { from, is: i, .. } =>
                        field(i).as_ref() == Some(&place) && match from {
                            ResourceTy::Value(f) => f.hash() == b.hash(),
                            // Given back by the `for` loop that took b
                            // over (#1080).
                            ResourceTy::Anonymous => true,
                            _ => false,
                        },
                    _ => false,
                });
                if back {
                    continue;
                }
                // A value holding the borrow that a `for` loop took
                // (`for (o, i) in pairs`; #1080): the loop's iterator
                // holds it from there on.
                if moved_into_loop(visualization_data, rap, b, l).is_some_and(|lm| lm <= line) {
                    None
                } else {
                    Some(b.name().to_owned())
                }
            }
            // A call: a `for` loop's iterator keeps it until the loop
            // ends; any other call gives it back on its own line, or
            // its result's borrow is the variable's above.
            ResourceTy::Value(_) => {
                if !loop_iterator_holds_all(visualization_data, rap, line).contains(&Some(place.clone())) {
                    continue;
                }
                None
            }
            _ => continue,
        };
        live.push((l, place, is_mut, borrower));
    }
    // The latest borrow, with every field the same borrower took on
    // that row (`self.items` and `self.other` in `let it =
    // self.items.iter().chain(self.other.iter());`; #1080).
    let (l0, _, _, borrower) = live.iter().max_by_key(|(l, ..)| *l)?.clone();
    let mut places: Vec<(String, bool)> = Vec::new();
    for (l, p, m, bw) in &live {
        if *l != l0 || *bw != borrower {
            continue;
        }
        match places.iter_mut().find(|(x, _)| x == p) {
            Some(entry) => entry.1 |= *m,
            None => places.push((p.clone(), *m)),
        }
    }
    Some(match borrower {
        None => hover_messages::state_mut_ref_fields_borrowed_by_loop_iterator(rap.name(), &places),
        Some(b) => hover_messages::state_mut_ref_fields_borrowed_by(rap.name(), &places, &b),
    })
}

/// The row where the value `b`, holding a borrow of a field of what
/// `rap` points to made on row `from`, was moved into a `for` loop (`for
/// (o, i) in pairs`; #1080): from then on the loop's iterator holds that
/// borrow. That move has no column to go to, and the loop gives the field
/// back where it ends, on rap's column alone (`from` Anonymous; see
/// `emit_borrow_ends`).
fn moved_into_loop(visualization_data: &VisualizationData, rap: &ResourceAccessPoint, b: &ResourceAccessPoint, from: usize) -> Option<usize> {
    let moved = visualization_data.timelines.get(b.hash())?.history.iter().find_map(|(l, e)| match e {
        Event::Move { to: ResourceTy::Anonymous, .. } if *l > from => Some(*l),
        _ => None,
    })?;
    let given_back_by_loop = visualization_data.timelines.get(rap.hash())?.history.iter().any(|(l, e)| *l > moved && matches!(e,
        Event::MutableReacquire { from: ResourceTy::Anonymous, is: ResourceTy::DerefField(r, ..), .. }
        | Event::StaticReacquire { from: ResourceTy::Anonymous, is: ResourceTy::DerefField(r, ..), .. } if r.hash() == rap.hash()));
    given_back_by_loop.then_some(moved)
}

/// Whether `rap`'s segment from `line` lies where a `for` loop's
/// iterator borrows it (#871): from the call that lent it (`chars` in
/// `for c in s.chars()`) to the end the loop draws for that borrow
/// (`StaticLender::LoopIterator`). The plugin draws that end only when
/// no loop variable gives the borrow back itself. Some(place) when what
/// it borrows is the field `place` reached through the reference rap
/// (`self.items.iter()`; #1060), Some(None) when it is rap's resource
/// (or all of what rap points to), None when it doesn't borrow it there.
fn loop_iterator_holds(visualization_data: &VisualizationData, rap: &ResourceAccessPoint, line: usize) -> Option<Option<String>> {
    loop_iterator_holds_all(visualization_data, rap, line).into_iter().next()
}

/// `loop_iterator_holds`, for every borrow of rap (or of a field of
/// what it points to) a `for` loop's iterator holds at `line`: several
/// fields with `for (a, b) in self.items.iter().zip(self.other.iter_mut())`
/// (#1080).
fn loop_iterator_holds_all(visualization_data: &VisualizationData, rap: &ResourceAccessPoint, line: usize) -> Vec<Option<String>> {
    fn loop_ends(events: &[(usize, ExternalEvent)], hash: u64, out: &mut Vec<(usize, StaticLender, Option<String>)>) {
        for (_, ev) in events {
            match ev {
                ExternalEvent::StaticDie { to: ResourceTy::Value(r), static_lender: Some(l @ (StaticLender::LoopIterator { .. } | StaticLender::LoopHeadBorrow { .. })), id, .. }
                | ExternalEvent::MutableDie { to: ResourceTy::Value(r), static_lender: Some(l @ (StaticLender::LoopIterator { .. } | StaticLender::LoopHeadBorrow { .. })), id, .. }
                    if *r.hash() == hash => out.push((*id, l.clone(), None)),
                ExternalEvent::StaticDie { to: ResourceTy::DerefField(r, place, _, _), static_lender: Some(l @ StaticLender::LoopIterator { .. }), id, .. }
                | ExternalEvent::MutableDie { to: ResourceTy::DerefField(r, place, _, _), static_lender: Some(l @ StaticLender::LoopIterator { .. }), id, .. }
                    if *r.hash() == hash => out.push((*id, l.clone(), Some(place.clone()))),
                ExternalEvent::Branch { branches, .. } => {
                    for b in branches {
                        loop_ends(&b.e_data, hash, out);
                    }
                }
                _ => {}
            }
        }
    }
    fn flat<'a>(history: &'a [(usize, Event)], out: &mut Vec<(usize, &'a Event)>) {
        for (l, e) in history {
            out.push((*l, e));
            if let Event::Branch { branch_history, .. } = e {
                for b in branch_history {
                    flat(&b.e_data, out);
                }
            }
        }
    }
    let mut ends = Vec::new();
    loop_ends(&visualization_data.external_events, *rap.hash(), &mut ends);
    if ends.is_empty() {
        return Vec::new();
    }
    let Some(timeline) = visualization_data.timelines.get(rap.hash()) else { return Vec::new() };
    let mut history = Vec::new();
    flat(&timeline.history, &mut history);
    ends.into_iter().filter(|(id, lender, _)| {
        let Some(end) = history.iter().find_map(|(l, e)| match e {
            Event::StaticReacquire { id: i, .. } | Event::MutableReacquire { id: i, .. } if i == id => Some(*l),
            _ => None,
        }) else { return false };
        let lent = history.iter().filter_map(|(l, e)| match (e, lender) {
            (Event::StaticLend { to: ResourceTy::Value(ResourceAccessPoint::Function(f)), .. }
            | Event::MutableLend { to: ResourceTy::Value(ResourceAccessPoint::Function(f)), .. },
             StaticLender::LoopIterator { call })
                if f.name == *call && *l <= end => Some(*l),
            // `for &y in &v`: the borrow the `for` line drew (#885).
            (Event::StaticLend { id: i, .. } | Event::MutableLend { id: i, .. }, StaticLender::LoopHeadBorrow { lend })
                if i == lend => Some(*l),
            _ => None,
        }).max();
        lent.is_some_and(|from| from <= line && line < end)
    }).map(|(_, _, place)| place).collect()
}

fn or_alt_lend_at<'a>(
    visualization_data: &'a VisualizationData,
    rap: &ResourceAccessPoint,
    line: Option<usize>,
) -> Option<(&'a ResourceAccessPoint, bool, &'a [String], Option<&'a ChainRef>)> {
    fn lends<'a>(history: &'a [(usize, Event)], out: &mut Vec<(usize, &'a Event)>) {
        for (l, e) in history {
            match e {
                Event::StaticLend { .. } | Event::MutableLend { .. }
                | Event::OrAltLend { end: false, .. } => out.push((*l, e)),
                Event::Branch { branch_history, .. } => {
                    for b in branch_history {
                        lends(&b.e_data, out);
                    }
                }
                _ => {}
            }
        }
    }
    let timeline = visualization_data.timelines.get(rap.hash())?;
    let mut all = Vec::new();
    lends(&timeline.history, &mut all);
    let found = match line {
        Some(line) => {
            all.sort_by_key(|(l, _)| *l);
            all.iter().rev().find(|(l, _)| *l <= line).map(|(_, e)| *e)
        }
        None if all.iter().all(|(_, e)| matches!(e, Event::OrAltLend { .. })) => all.first().map(|(_, e)| *e),
        None => None,
    };
    match found {
        Some(Event::OrAltLend { into, mutable, chain, sources, .. }) => Some((into, *mutable, sources, chain.as_ref())),
        _ => None,
    }
}

fn matches_closure(rty: &ResourceTy, closure_hash: u64) -> bool {
    rty.extract_rap().map_or(false, |r| *r.hash() == closure_hash)
}

fn upvar_name(rty: &ResourceTy) -> Option<String> {
    // A function isn't captured: it returned the closure (#686).
    rty.extract_rap().filter(|r| !r.is_fn()).map(|r| r.name().to_owned())
}

/// Whether every event in these branches (and in the conditionals
/// nested in them) is the drop of a `for` loop's iterator at a
/// `return` (`Event::LoopIteratorDrop`, #482), and there is one.
fn only_loop_iterator_drops(branches: &[BranchData]) -> bool {
    fn all_drops(branches: &[BranchData]) -> bool {
        branches.iter().all(|b| b.e_data.iter().all(|(_, e)| match e {
            Event::LoopIteratorDrop { .. } => true,
            Event::Branch { branch_history, .. } => all_drops(branch_history),
            _ => false,
        }))
    }
    fn any_drop(branches: &[BranchData]) -> bool {
        branches.iter().any(|b| b.e_data.iter().any(|(_, e)| match e {
            Event::LoopIteratorDrop { .. } => true,
            Event::Branch { branch_history, .. } => any_drop(branch_history),
            _ => false,
        }))
    }
    all_drops(branches) && any_drop(branches)
}

// Like `append_dot`, but the dot is drawn as a drop dot (circle +
// inner triangle) instead of the plain circle.
fn append_drop_dot(
    drop_data: DropDotData,
    timeline_data: &TimelineColumnData,
    dots: &mut DotLayer,
    ev: &'static str,
    row: usize,
) {
    dots.add_event(drop_data.hash, drop_data.dot_x, drop_data.dot_y, timeline_data, drop_data.title, true, None, ev, row);
}

/// Collects every event dot. The arrow pass routes arrows around
/// their positions (see `render_hopping_arrow`) and adds call icons;
/// `DotLayer::flush` then draws them.
fn render_dots_string(
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
) -> DotLayer {
    let mut dots = DotLayer::default();
    let timelines = &visualization_data.timelines;
    for (hash, timeline) in timelines {
        // render just the name of Owners and References
        match timeline.resource_access_point {
            ResourceAccessPoint::Function(_) => {
                // nothing to be done
            },
            ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_) | ResourceAccessPoint::MutRef(_) | ResourceAccessPoint::StaticRef(_) =>
            {
                let resource_hold = if matches!(
                    timeline.resource_access_point,
                    ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_)
                ) {
                    // Each owner / struct has at least two states (init →
                    // gos); the penultimate one tells us what state the
                    // RAP was in *just before* going out of scope. If it
                    // still held the resource (FullPrivilege /
                    // PartialPrivilege) the destructor runs and we want
                    // the down-arrow drop indicator. Includes Struct so
                    // a `let r = Rect{..}` shows the drop on r at end of
                    // scope (and on r.w / r.h, which are also Struct
                    // RAPs in the data model).
                    holds_resource_at_scope_end(timeline)
                } else { false };
                render_dot(hash, &timeline.history, &resource_owners_layout[hash], visualization_data, resource_hold, &mut dots, false);
            },
        }
    }
    dots
}

// Whether an owner / struct column still holds its resource just
// before going out of scope: the penultimate state (the last one is the
// out-of-scope state) is FullPrivilege / PartialPrivilege.
// Whether a `break` carries a field of the struct column `hash` (at any
// depth) out of its loop on `line` (#1280): a Move marked `break_out`.
fn field_broken_out_at(visualization_data: &VisualizationData, hash: u64, line: usize) -> bool {
    let Some(rap @ ResourceAccessPoint::Struct(Struct { owner, .. })) =
        visualization_data.timelines.get(&hash).map(|t| &t.resource_access_point) else { return false };
    let root = if rap.is_member() { *owner } else { hash };
    let prefix = format!("{}.", rap.name());
    visualization_data.timelines.values().any(|t| match &t.resource_access_point {
        ResourceAccessPoint::Struct(Struct { is_member: true, owner, name, .. }) if *owner == root && name.starts_with(&prefix) =>
            t.history.iter().any(|(l, e)| *l == line && matches!(e, Event::Move { break_out: true, .. })),
        _ => false,
    })
}

/// The way out of a labeled block (as a BreakAlt: a `break 'blk`, or
/// the block's last expression) that carries the column `hash`, or a
/// field of it, out of the block it is declared in, as the last thing
/// that happens to it (#1288); None for a loop's `break`.
/// With the field carried out, when it is one (`g.name` for `g`).
fn broken_out_of_block(visualization_data: &VisualizationData, hash: u64) -> Option<(BreakAlt, Option<String>)> {
    let alt_in = |t: &Timeline| t.history.iter().rev()
        .find(|(_, e)| !matches!(e, Event::OwnerGoOutOfScope { .. }))
        .and_then(|(_, e)| match e {
            Event::Move { break_out: true, break_alt: Some(alt), .. } if alt.end != AltEnd::Loop =>
                Some((alt.clone(), Some(t.resource_access_point.name().clone()))),
            _ => None,
        });
    let t = visualization_data.timelines.get(&hash)?;
    if let Some((alt, _)) = alt_in(t) {
        return Some((alt, None));
    }
    let rap @ ResourceAccessPoint::Struct(Struct { owner, .. }) = &t.resource_access_point else { return None };
    let root = if rap.is_member() { *owner } else { hash };
    let prefix = format!("{}.", rap.name());
    let mut fields: Vec<&Timeline> = visualization_data.timelines.values().filter(|f| matches!(&f.resource_access_point,
        ResourceAccessPoint::Struct(Struct { is_member: true, owner, name, .. }) if *owner == root && name.starts_with(&prefix))).collect();
    fields.sort_by_key(|f| f.resource_access_point.name().clone());
    fields.into_iter().find_map(alt_in)
}

fn holds_resource_at_scope_end(timeline: &Timeline) -> bool {
    let penultimate_state = timeline
        .states
        .get(timeline.states.len().saturating_sub(2))
        .unwrap_or(&(0, 0, State::Invalid))
        .2
        .clone();
    matches!(
        penultimate_state,
        State::FullPrivilege { .. } | State::PartialPrivilege { .. }
    )
}

// The events that lead to `history[..upto]`, where `history` is the
// column's own history `top` or the events of a branch nested in it: the
// events of `top` before the conditional it is in, and so on down (#1194).
// What the column holds where a branch starts (moved on some paths above)
// is decided by those.
fn history_path(top: &[(usize, Event)], history: &[(usize, Event)], upto: usize) -> Option<Vec<(usize, Event)>> {
    if std::ptr::eq(top, history) {
        return Some(top[..upto.min(top.len())].to_vec());
    }
    for (k, (_, e)) in top.iter().enumerate() {
        if let Event::Branch { branch_history, .. } = e {
            if let Some(mut inner) = branch_history.iter().find_map(|b| history_path(&b.e_data, history, upto)) {
                let mut out = top[..k].to_vec();
                out.append(&mut inner);
                return Some(out);
            }
        }
    }
    None
}

// A partial drop on a `?`'s early return (#1184).
struct TryPartialDrop {
    moved: Vec<String>,
    maybe_moved: Vec<String>,
    dropped: Vec<String>,
    by_cond: bool,
}

fn try_partial_move_split(visualization_data: &VisualizationData, hash: u64, line: usize) -> Option<TryPartialDrop> {
    // Whether `history` has a `?` dot on `line`, and if so, whether the
    // column holds its value there on some paths only.
    fn try_drop_at(top: &[(usize, Event)], history: &[(usize, Event)], line: usize) -> Option<bool> {
        for (i, (l, e)) in history.iter().enumerate() {
            match e {
                Event::OwnerDropIfTryReturns { .. } if *l == line =>
                    return Some(history_path(top, history, i).is_some_and(|path| maybe_moved_at_end(&path))),
                Event::Branch { branch_history, .. } => {
                    if let Some(m) = branch_history.iter().find_map(|b| try_drop_at(top, &b.e_data, line)) {
                        return Some(m);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let rap = &visualization_data.timelines.get(&hash)?.resource_access_point;
    let ResourceAccessPoint::Struct(Struct { owner, .. }) = rap else { return None };
    let root = if rap.is_member() { *owner } else { hash };
    let prefix = format!("{}.", rap.name());
    let fields: Vec<(&Timeline, &String)> = visualization_data
        .timelines
        .values()
        .filter_map(|t| match &t.resource_access_point {
            ResourceAccessPoint::Struct(Struct { is_member: true, owner, name, is_copy: false, is_plain_value: false, .. })
                // A field with nothing to drop (a ManuallyDrop, #1137)
                // gets no `?` dot, but wasn't moved out.
                if *owner == root && name.starts_with(&prefix) && !t.resource_access_point.drops_nothing() => Some((t, name)),
            _ => None,
        })
        .collect();
    let mut moved: Vec<String> = Vec::new();
    let mut maybe_moved: Vec<String> = Vec::new();
    let mut dropped = Vec::new();
    let mut by_cond = false;
    for (timeline, name) in &fields {
        if moved.iter().any(|m| name.starts_with(&format!("{}.", m))) {
            continue;
        }
        let child_prefix = format!("{}.", name);
        let is_leaf = !fields.iter().any(|(_, n)| n.starts_with(&child_prefix));
        match try_drop_at(&timeline.history, &timeline.history, line) {
            None => moved.push((*name).clone()),
            Some(true) if is_leaf => {
                by_cond |= moved_by_chain_condition(&timeline.history) || maybe_moved_by_let(&timeline.history);
                maybe_moved.push((*name).clone());
            }
            Some(false) if is_leaf => dropped.push((*name).clone()),
            Some(_) => {}
        }
    }
    if moved.is_empty() && maybe_moved.is_empty() {
        None
    } else {
        Some(TryPartialDrop { moved, maybe_moved, dropped, by_cond })
    }
}

// Smallest id among the events that give a column its value (Acquire /
// Copy into it), searching branches too. Event ids follow source order,
// so a later one is a write to a column that already holds a value.
fn first_value_event_id(history: &[(usize, Event)]) -> Option<usize> {
    history.iter().filter_map(|(_, e)| match e {
        Event::Acquire { is: ResourceTy::Value(_), id, .. }
        | Event::Copy { is: ResourceTy::Value(_), id, .. } => Some(*id),
        Event::Branch { branch_history, .. } => branch_history.iter()
            .filter_map(|b| first_value_event_id(&b.e_data)).min(),
        _ => None,
    }).min()
}

// Tooltip for a write to a union column that already holds a value
// (`u.a = 5;`, `u.a = y;`): the union isn't (re)acquired or
// initialized, and its old contents aren't dropped (#191).
fn union_write_title(
    event: &Event,
    rap: &ResourceAccessPoint,
    name: &String,
    visualization_data: &VisualizationData,
    hash: u64,
) -> Option<String> {
    if !rap.is_union() {
        return None;
    }
    // The copy of the union RAP the write event carries says whether the
    // value written is Copy (`PlaceWrite`); a literal's event is an
    // Acquire either way, so the event's kind can't tell.
    let (from, id, written) = match event {
        Event::Acquire { from, is: ResourceTy::Value(w), id, .. } => (from, *id, w),
        Event::Copy { from, is: ResourceTy::Value(w), id, .. } => (from, *id, w),
        _ => return None,
    };
    let is_copy = matches!(written.place_write(), PlaceWrite::AssignCopy);
    let history = &visualization_data.timelines.get(&hash)?.history;
    let first = first_value_event_id(history)?;
    if id <= first {
        return None;
    }
    // Assigning a whole union with its own `Drop` impl runs that impl
    // on the old value (#1146): the drop dot on this line says so, and
    // "never drops its old contents" would contradict it.
    if rap.union_drop() && reassign_drop_on_line(history, id) {
        return None;
    }
    let source = match from {
        ResourceTy::Value(r) | ResourceTy::Deref(r) if !matches!(r, ResourceAccessPoint::Function(_)) =>
            Some(r.name()),
        _ => None,
    };
    Some(hover_messages::event_dot_union_write(name, is_copy, source))
}

// Whether `history` (searched into branches) has a drop at reassignment
// on the line of the event with id `id`.
fn reassign_drop_on_line(history: &[(usize, Event)], id: usize) -> bool {
    fn line_of(h: &[(usize, Event)], id: usize) -> Option<usize> {
        h.iter().find_map(|(l, e)| match e {
            Event::Branch { branch_history, .. } => branch_history.iter().find_map(|b| line_of(&b.e_data, id)),
            Event::Acquire { id: x, .. } | Event::Copy { id: x, .. } if *x == id => Some(*l),
            _ => None,
        })
    }
    fn has_drop(h: &[(usize, Event)], line: usize) -> bool {
        h.iter().any(|(l, e)| match e {
            Event::Branch { branch_history, .. } => branch_history.iter().any(|b| has_drop(&b.e_data, line)),
            Event::OwnerDropAtReassign { .. } => *l == line,
            _ => false,
        })
    }
    line_of(history, id).is_some_and(|l| has_drop(history, l))
}

// Whether a column holds a value after `event`, given whether it held
// one before. Only used to tell a Copy-typed column's assignment from
// its initialization, so moves out (which Copy columns don't have) are
// tracked only roughly.
fn holds_value_after(held: bool, event: &Event) -> bool {
    match event {
        Event::Acquire { is: ResourceTy::Value(_), .. }
        | Event::Copy { is: ResourceTy::Value(_), .. }
        // A reference variable bound by a borrow (`let mut r = &a;`)
        // holds a reference from then on, also after that borrow
        // ends: the end is drawn where r is reassigned (#414).
        | Event::StaticBorrow { is: ResourceTy::Value(_), .. }
        | Event::MutableBorrow { is: ResourceTy::Value(_), .. }
        | Event::OrAltBorrow { end: false, .. }
        // Owns a value at every `break` of the loop above (#1000).
        | Event::LoopExitKept { .. }
        | Event::InitRefParam { .. } => true,
        e if e.is_box_payload_move() => held,
        Event::Move { .. } | Event::LoopExitMove { definite: true, .. } | Event::ClosureCall { .. } => false,
        // A variable that has gone out of scope holds nothing. When
        // same-named variables of disjoint branches share a column
        // (`if let Some(z) = a {…} else if let Some(z) = b {…}`), the
        // next one's first event initializes a new variable; it isn't an
        // assignment to the one that ended.
        Event::OwnerGoOutOfScope { .. } | Event::RefGoOutOfScope { .. } => false,
        // May have no value after a loop (#786).
        Event::LoopMaybeInit { .. } => false,
        // (Not one that returns or panics after assigning a `let x;`
        // local: on the paths that get past the conditional, x still
        // has no value, #930. A `break` out of a loop takes the value
        // to the code after the loop.)
        Event::Branch { branch_history, .. } => {
            held || branch_history.iter()
                .filter(|b| !(b.diverges && b.exits.iter().all(|x| matches!(x, BranchExit::Return | BranchExit::Panic | BranchExit::Other))))
                .any(|b| b.e_data.iter().any(|(_, e)| holds_value_after(false, e)))
        }
        _ => held,
    }
}

fn traverse_timeline2<'a> (t: &'a TimelineColumnData, history: & 'a Vec<(usize, Event)>, id: usize) -> Option<& 'a TimelineColumnData> {
    for (_, e) in history {
        match e {
            Event::Branch { branch_history, .. } => {
                for branch in branch_history {
                    let res = traverse_timeline2(&branch.t_data, &branch.e_data, id);
                    if res.is_some() {
                        return res;
                    }
                }
            }
            _ => {
                if e.get_id() == id {
                    return Some(t);
                }
            }
        }
    }
    None
}


/// The x of the column the arm holding event `id` forks from: the
/// variable's own column, or the arm of an enclosing conditional. `None`
/// when the event is on the variable's own column, in no arm.
fn arm_parent_x(hash: &u64, vd: &VisualizationData, ro_layout: &BTreeMap<u64, TimelineColumnData>, id: usize) -> Option<i64> {
    arm_parent(hash, vd, ro_layout, id).map(|(x, _)| x)
}

/// `arm_parent_x`, and whether the event's dot is on the arm's diagonal
/// leg or at its bend: in a one-row diamond (`branch_is_collapsed`), or
/// on the fork's line or the next (`if c { take(s); } else { take(t);`
/// on two lines, an arm on #1187's rows). The other arm's leg then runs
/// close by on the dot's inner side.
fn arm_parent(hash: &u64, vd: &VisualizationData, ro_layout: &BTreeMap<u64, TimelineColumnData>, id: usize) -> Option<(i64, bool)> {
    // `Err(line)`: the event is in `history` itself, on `line`.
    fn walk(parent: &TimelineColumnData, history: &[(usize, Event)], id: usize) -> Option<Result<(i64, bool), usize>> {
        history.iter().find_map(|(line, e)| match e {
            Event::Branch { branch_history, split_point, merge_point, .. } => branch_history.iter().find_map(|b|
                walk(&b.t_data, &b.e_data, id).map(|inner| inner.or_else(|l|
                    Ok((parent.x_val, branch_is_collapsed(*split_point, *merge_point) || l <= *split_point + 1))))),
            _ if e.get_id() == id => Some(Err(*line)),
            _ => None,
        })
    }
    walk(&ro_layout[hash], &vd.timelines.get(hash)?.history, id)?.ok()
}

/// Which side of its dot a call's logo goes on by default, for an event
/// on an arm's diagonal leg or its bend (`arm_parent`): the arm's outer
/// side (left of a left arm, right of a right arm). The inner side lies
/// on, or against, the other arm's dashed leg (#1309). `None` elsewhere.
fn diamond_arm_outer_left(hash: &u64, vd: &VisualizationData, ro_layout: &BTreeMap<u64, TimelineColumnData>, id: usize, x: i64) -> Option<bool> {
    match arm_parent(hash, vd, ro_layout, id)? {
        (parent_x, true) if x != parent_x => Some(x < parent_x),
        _ => None,
    }
}

fn fetch_timeline<'a>(hash: &u64, vd: &'a VisualizationData, ro_layout: & 'a BTreeMap<u64, TimelineColumnData>, id: usize) -> & 'a TimelineColumnData {
    match traverse_timeline2(&ro_layout[hash], &vd.timelines[hash].history, id) {
        Some(t) => t,
        None => panic!("internal state: no layout column for the timeline of event {}", id)
    }
}

fn traverse_events2<'a> (
    history: & 'a Vec<(usize, ExternalEvent)>, 
    line_map: & 'a BTreeMap<usize, Vec<ExternalEvent>>,
    id: usize
) ->  Option<& 'a BTreeMap<usize, Vec<ExternalEvent>>> {
    for (_, e) in history {
        match e {
            ExternalEvent::Branch { branches, .. } => {
                for branch in branches {
                    let res = traverse_events2(&branch.e_data, &branch.line_map, id);
                    if res.is_some() {
                        return res;
                    }
                }
            }
            _ => {
                if e.get_id() == id {
                    return Some(line_map);
                }
            }
        }
    }
    None
}

fn fetch_line_map<'a>(
    vd: &'a VisualizationData,
    id: usize 
) -> & 'a BTreeMap<usize, Vec<ExternalEvent>> {
    match traverse_events2(&vd.external_events, &vd.event_line_map, id) {
        Some(t) => t,
        None => panic!("internal state: no line map for event {}", id)
    }
}

/// How much farther than a plain dot an arrow's head stops from a dot that
/// also carries a call icon: the `f` glyph pokes out of the dot's 5px radius
/// to the right (its italic hook), so the tip of a head coming in from the
/// right would touch it (#578). Heads coming in from the left clear the
/// glyph's left edge already.
const CALL_ICON_CLEARANCE: f64 = 3.0;

/// Does an arrow head aimed at `target` land on a dot with a call icon?
fn head_lands_on_call_icon(target: &(f64, f64), dots: &DotLayer) -> bool {
    target.0.fract() == 0.0
        && target.1.fract() == 0.0
        && dots.call_spots.contains(&(target.0 as i64, target.1 as i64))
}

/// How far an arrow's tail starts from the centre of its source dot:
/// the dot's 5px radius plus the hairline the caller-return L already
/// leaves (`source_x = cx + 5.25`).
const TAIL_CLEARANCE: f64 = 5.25;

/// Where an arrow whose tail is at `tail`, heading towards `next`,
/// should start. Arrows are drawn after dots, so a tail on a dot's
/// centre covers the middle of that dot and hovering there shows the
/// arrow's tooltip instead of the dot's (#269). When a dot is drawn at
/// `tail`, start at its edge instead, the way the head is pulled back
/// by `head_offset`. A tail with no dot under it stays put.
fn tail_off_dot(tail: (f64, f64), next: (f64, f64), dots: &DotPositions) -> (f64, f64) {
    let on_dot = tail.0.fract() == 0.0
        && tail.1.fract() == 0.0
        && dots.contains(&(tail.0 as i64, tail.1 as i64));
    let (dx, dy) = (next.0 - tail.0, next.1 - tail.1);
    let len = (dx * dx + dy * dy).sqrt();
    if !on_dot || len <= TAIL_CLEARANCE {
        return tail;
    }
    (tail.0 + dx / len * TAIL_CLEARANCE, tail.1 + dy / len * TAIL_CLEARANCE)
}

/// Build the SVG markup for a straight horizontal arrow that hops
/// over any event dot it would otherwise pass through.
///
/// Arrows are drawn after dots, so an arrow running straight through
/// a dot covers it, and the arrow's tooltip wins the hover: the dot's
/// own tooltip can't be reached. Each dot strictly between the two
/// ends gets a half-circle hop around it instead.
///
/// `x1` is where the head lands, `x2` the source column (the caller's
/// convention). Returns `None` when there are no crossings; the caller
/// then draws the usual polyline, so those arrows are unchanged.
/// Also returns what it draws (`Drawn`: its straight pieces, the arcs
/// left out, and its head).
fn render_hopping_arrow(
    x1: i64, y: i64, x2: i64,
    title: &str,
    rv: &str,
    dots: &DotLayer,
    head_offset: f64,
    legs: &[Piece],
    turn: Option<i64>,
    branch_hops: bool,
) -> Option<(String, Vec<Piece>, Vec<(f64, f64)>, Vec<Hop>)> {
    let (lo_x, hi_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
    let dot_positions = &dots.positions;

    // Pull every (x, y) dot position that sits at this arrow's y
    // and strictly between source/target x. Source and target dots
    // (at exactly lo_x or hi_x) aren't intervening — they're the
    // endpoints we're connecting. Ascending and duplicate-free,
    // since `DotPositions` is a set ordered by (x, y).
    let mut overlap_xs: BTreeSet<i64> = dot_positions
        .iter()
        .filter_map(|(dx, dy)| {
            if *dy == y && *dx > lo_x && *dx < hi_x { Some(*dx) } else { None }
        })
        .collect();
    if branch_hops {
        let y = y as f64;
        let direction = (x1 - x2).signum() as f64;
        let tail = x2 as f64 + TAIL_CLEARANCE * direction;
        let base = x1 as f64 - head_offset * direction;
        let shaft = ((tail, y), (base, y));
        let crossings: BTreeSet<_> = dots.branch_stretches()
            .filter_map(|leg| horizontal_crossing_x(shaft, leg))
            .map(|x| x.round() as i64).collect();
        for x in crossings {
            // A hop needs its full diameter. Where crossings are close,
            // keep the arc already protecting the event dot or leg.
            let room = HOP_RADIUS as i64;
            if direction * (x as f64 - tail) > HOP_RADIUS && direction * (base - x as f64) > HOP_RADIUS
                && overlap_xs.range(x - 2 * room..=x + 2 * room).next().is_none() {
                overlap_xs.insert(x);
            }
        }
    }
    if overlap_xs.is_empty() { return None; }

    // The arrowhead is 12.75 units long and pulled back 18 from the
    // dot center (matching the polyline path) so the tip lands on
    // the dot's near edge. Sign of `head_dir` tells the head
    // polygon which way to point.
    let r = HOP_RADIUS;
    let y_f = y as f64;
    let source_x = x2 as f64;

    // Always emit the path source → endpoint. Endpoint is x1 with
    // pullback applied along the arrow's direction.
    let goes_right = x2 < x1;
    let head_dir = if goes_right { 1.0 } else { -1.0 };
    let end_x_pulled = if goes_right {
        x1 as f64 - head_offset
    } else {
        x1 as f64 + head_offset
    };
    // Order the hops by path direction so we draw L/A segments in
    // travel order. `overlap_xs` is ascending; reverse when going
    // right-to-left.
    let hops: Vec<i64> = if goes_right {
        overlap_xs.into_iter().collect()
    } else {
        overlap_xs.into_iter().rev().collect()
    };

    let first_x = match hops.first() {
        Some(&ox) => ox as f64,
        None => end_x_pulled,
    };
    let (source_x, _) = tail_off_dot((source_x, y_f), (first_x, y_f), dot_positions);
    let mut d = format!("M {} {}", source_x, y_f);
    let mut pieces = Vec::new();
    let mut arcs = Vec::new();
    let mut at = source_x;
    for ox in &hops {
        let oxf = *ox as f64;
        let near = if goes_right { oxf - r } else { oxf + r };
        let far  = if goes_right { oxf + r } else { oxf - r };
        let up = hop_opens_up((*ox, y), true, dot_positions);
        let sweep = u8::from(goes_right == up);
        // Straight on to just before the dot, then a half-circle
        // arc over it, then continue.
        append_arrow_shaft(&mut d, &mut pieces, ((at, y_f), (near, y_f)), legs, turn);
        d.push_str(&format!(" A {} {} 0 0 {} {} {}", r, r, sweep, far, y_f));
        arcs.push(Hop { center: (oxf, y_f), up, right: goes_right });
        at = far;
    }
    let mut base = (end_x_pulled, y_f);
    let mut triangle = vec![(base.0, y_f - 6.0), (base.0 + 12.75 * head_dir, y_f), (base.0, y_f + 6.0)];
    let on_leg = |triangle: &[(f64, f64)]| legs.iter().any(|&leg| {
        let (a, b) = leg;
        if triangle.iter().all(|p| p.0 < a.0.min(b.0) - 1.0)
            || triangle.iter().all(|p| p.0 > a.0.max(b.0) + 1.0)
            || triangle.iter().all(|p| p.1 < a.1.min(b.1) - 1.0)
            || triangle.iter().all(|p| p.1 > a.1.max(b.1) + 1.0) { return false; }
        samples(&[leg]).into_iter().filter(|&point| inside_tri(triangle, point) > -1.0).nth(6).is_some()
    });
    // When a borrow ends on a nested fork, a level head can hide the
    // incoming leg. Keep the hops on their row and turn just the final
    // part of the approach, past the last intervening dot.
    let mut tail = (at, y_f);
    if head_offset == 18.0 && (at - x1 as f64).abs() > 5.0 * head_offset && on_leg(&triangle) {
        let corner = (x1 as f64 - 4.0 * head_dir * head_offset, y_f);
        for dy in [-15.0, 15.0] {
            let bend = (x1 as f64 - head_dir * head_offset, y_f + dy);
            let (head, mid) = head_triangle(bend, (x1 as f64, y_f));
            if on_leg(&head) || legs.iter().any(|&leg| shaft_on_leg((corner, bend), leg) || shaft_on_leg((bend, mid), leg)) { continue; }
            d.push_str(&format!(" L {} {} L {} {}", corner.0, corner.1, bend.0, bend.1));
            pieces.extend([(tail, corner), (corner, bend)]);
            tail = bend;
            triangle = head;
            base = mid;
            break;
        }
    }
    append_arrow_shaft(&mut d, &mut pieces, (tail, base), legs, turn);
    let (v1, v2, v3) = (triangle[0], triangle[1], triangle[2]);

    Some((format!(
        "        <g class=\"tooltip-trigger\" data-tooltip-text=\"{title}\"{rv}>\n\
            \x20           <path class=\"arrow-shaft\" stroke-width=\"5px\" d=\"{d}\" style=\"fill: none;\"/>\n\
            \x20           <polygon points=\"{},{} {},{} {},{}\" class=\"arrow-head\"/>\n\
            \x20       </g>\n",
        v1.0, v1.1, v2.0, v2.1, v3.0, v3.1,
        title = title, d = d, rv = rv,
    ), pieces, vec![v1, v2, v3], arcs))
}

fn append_arrow_shaft(
    path: &mut String, pieces: &mut Vec<Piece>, (from, to): Piece, legs: &[Piece], turn: Option<i64>,
) {
    let route = [from, to];
    let jog;
    let route = if let Some(dy) = turn {
        jog = turn_across_legs(&route, legs, dy as f64);
        jog.as_slice()
    } else {
        route.as_slice()
    };
    for points in route.windows(2) {
        let (p, q) = (points[0], points[1]);
        path.push_str(&format!(" L {} {}", q.0, q.1));
        pieces.push((p, q));
    }
}

/// What one arrow (or function logo with its short arrow) covers on a
/// horizontal track, recorded by the first pass of the arrows
/// (`ArrowLanes::recording`).
#[derive(Clone, Debug)]
struct LaneItem {
    /// (event id, line): an event can be drawn on more than one line.
    key: (usize, usize),
    /// The track's y, and the stretch of it covered (head and logo
    /// included).
    y: i64,
    lo: f64,
    hi: f64,
    kind: LaneKind,
    /// Render order, for ties.
    order: usize,
    /// An arrow between two columns' dots: the dots' y and the x of its
    /// head's and tail's dots.
    ends: Option<(i64, f64, f64)>,
}

#[derive(Clone, Debug)]
enum LaneKind {
    /// An arrow between two columns: `base` is the y of its two dots, and
    /// it may move to another track of that row (`free_lane`).
    Movable { base: i64, die: bool },
    /// An arrow dipping whole rows below its line (its place among the
    /// line's arrows, `arrow_order_of`); `line_y` is the line's row.
    WholeRows { line_y: i64, dip: i64 },
    /// A call's logo and its short arrow on a column's dot; `alt` is the
    /// stretch it covers drawn on the dot's other side.
    Logo { alt: (f64, f64) },
    /// A call's result handed to the caller (the logo in the gutter).
    FnResult,
    /// A column's L to the caller (`render_return_to_caller_arrow`).
    CallerL,
    /// Anything else (an L to or from the caller): stays where it is.
    Fixed,
}

/// Where the arrows of a row go so that no two lie along one stretch
/// (#1300), no logo sits on another arrow (#1307), no two results to the
/// caller share the gutter's spot (#1299), and arrows dipping below a
/// line don't cross (#1312). The arrows are drawn twice: a first pass
/// records what each covers (`items`) without drawing; `resolve` then
/// picks the tracks, and the second pass draws with them.
#[derive(Default, Debug)]
struct ArrowLanes {
    recording: bool,
    items: Vec<LaneItem>,
    /// A movable arrow's track's y, where it isn't its own row's.
    lane_y: HashMap<(usize, usize), i64>,
    /// A whole-row dip, where the arrows of a line swap tracks.
    dip: HashMap<(usize, usize), i64>,
    /// A logo drawn on its dot's other side.
    flipped: std::collections::HashSet<(usize, usize)>,
    /// A result to the caller's place in the gutter, after the row's first.
    slot: HashMap<(usize, usize), i64>,
    /// A result to the caller in the gutter and a left arm's L to the
    /// caller on the same row, both drawn narrower (`GUTTER_SQUEEZED_*`).
    squeezed: std::collections::HashSet<(usize, usize)>,
    /// How far along the row a dipping arrow's leg runs from its head's
    /// (`true`) or tail's dot, where it isn't the usual 20 px.
    leg_dx: HashMap<((usize, usize), bool), i64>,
    /// Arrows whose track runs on past the target's dot, so the head
    /// comes back to it from the far side (#1316).
    far: std::collections::HashSet<(usize, usize)>,
    /// A column's arrow to itself drawn right of its dot (#1316).
    self_right: std::collections::HashSet<(usize, usize)>,
    /// Arrows on a track whose head comes into its dot level along the
    /// dot's row, or at a few degrees off it (the value), from the side
    /// `far` says (#1499): the leg from the track turns `LEVEL_IN_DX` px
    /// before the dot.
    level_in: HashMap<(usize, usize), i64>,
    /// Every arrow between two columns' dots, by the first pass: its
    /// head's dot, its tail's dot and its line's row.
    ends: HashMap<(usize, usize), ((i64, i64), (i64, i64), i64)>,
    /// A probing pass (`untangle`): what each arrow and
    /// logo is drawn as goes into `drawn`, and nothing is added to the
    /// dots.
    probing: bool,
    drawn: Vec<Drawn>,
    /// Bottom edge of the arrows and standalone logos painted in the final pass.
    painted_bottom: f64,
    /// A probing pass draws a track's hops over dots (`track_hops`), as
    /// the final drawing always does (`untangle` searches both ways).
    probe_hops: bool,
    /// A call's logo's dot's y, by the first pass.
    logo_rows: HashMap<(usize, usize), i64>,
    /// Vertical clearance for a local turn across a shallow fork leg.
    leg_turn: HashMap<ArrowKey, i64>,
    /// Event-row routes that mark crossings of branch stretches.
    branch_hops: BTreeSet<ArrowKey>,
}

/// A straight piece of an arrow's shaft.
type Piece = ((f64, f64), (f64, f64));
type ArrowKey = (usize, usize);

/// An arrow or a function logo as a probing pass draws it: the shaft's
/// straight pieces (a hop's arcs left out), the head's triangle, or the
/// logo's anchor; the same reading of the drawing as the geometric checks
/// (`rustviz-lib/tests/common/geometry.rs`).
#[derive(Clone, Debug, Default)]
struct Drawn {
    key: ArrowKey,
    tip: String,
    /// `data-rv-var` / `data-rv-to`.
    var: String,
    to: String,
    columns: (Option<u64>, Option<u64>),
    pieces: Vec<Piece>,
    tri: Vec<(f64, f64)>,
    logo: Option<(f64, f64)>,
    arcs: Vec<Hop>,
    /// How many dots its track (off its dots' row) hops (`track_hops`):
    /// `untangle` tries another track for it first.
    track_hops: usize,
}

/// How far below or above its dot a call's logo moved off it goes
/// (`ArrowLanes::alternatives`): more than a logo's height.
const LOGO_DIP: i64 = 20;
/// How far apart a row's tracks are: half a row, between two rows.
const LANE_STEP: i64 = LINE_SPACE / 2;
/// Two stretches on one track share more than this: they overlap.
const LANE_OVERLAP: f64 = 2.0;
/// How wide an arrowhead is, in degrees, seen from the centre of the dot
/// it points at (6 px either side, 18 px out).
const HEAD_WIDTH_DEG: f64 = 36.0;
/// Where a result to the caller's logo goes in the gutter, and the x of
/// its L's bend (`render_function_result_arrow`); each later result on
/// the row goes half a row lower, with its bend this much farther right.
const FN_RESULT_LOGO_X: f64 = 14.0;
const FN_RESULT_BEND_X: f64 = 44.0;
const FN_RESULT_SLOT_DX: f64 = 14.0;
const FN_RESULT_SLOT_LOGO_DX: f64 = 12.0;
/// A result to the caller whose gutter spot lies on a left arm's L to
/// the caller (`if c { *b } else { String::new() }`, b the first column,
/// #1309): its L bends here, its logo `FN_RESULT_SLOT_LOGO_DX` farther
/// left, and the arm's L bends no farther left than
/// `GUTTER_SQUEEZED_ARM_BEND_X`, so the two heads are apart.
const GUTTER_SQUEEZED_BEND_X: f64 = 28.0;
const GUTTER_SQUEEZED_ARM_BEND_X: f64 = 48.0;

fn stretches_overlap(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0.max(b.0) < a.1.min(b.1) - LANE_OVERLAP
}

impl ArrowLanes {
    fn record(&mut self, key: (usize, usize), y: i64, lo: f64, hi: f64, kind: LaneKind) {
        if self.recording {
            let order = self.items.len();
            self.items.push(LaneItem { key, y, lo: lo.min(hi), hi: lo.max(hi), kind, order, ends: None });
        }
    }

    /// `record` for an arrow from the dot at (`from_x`, `dot_y`) to the
    /// one at (`to_x`, `dot_y`).
    fn record_arrow(&mut self, key: (usize, usize), y: i64, dot_y: i64, to_x: f64, from_x: f64, kind: LaneKind) {
        self.record(key, y, to_x, from_x, kind);
        if let Some(it) = self.items.last_mut().filter(|_| self.recording) {
            it.ends = Some((dot_y, to_x, from_x));
        }
    }

    /// Picks every arrow's track from what the first pass recorded.
    fn resolve(&mut self, dots: &DotPositions) {
        let mut items = std::mem::take(&mut self.items);

        // #1312: the arrows dipping whole rows below one line take their
        // tracks narrowest first, so one whose span lies inside
        // another's runs above it and they don't cross.
        let mut lines: BTreeMap<(i64, usize), Vec<usize>> = BTreeMap::new();
        for (i, it) in items.iter().enumerate() {
            if let LaneKind::WholeRows { line_y, .. } = it.kind {
                lines.entry((line_y, it.key.1)).or_default().push(i);
            }
        }
        let dip_of = |it: &LaneItem| match it.kind { LaneKind::WholeRows { dip, .. } => dip, _ => 0 };
        for group in lines.values().filter(|g| g.len() > 1) {
            // Only where one arrow's span lies inside another's that runs
            // above it (they cross); otherwise the line's order stands.
            let inside = |a: &LaneItem, b: &LaneItem| a.lo >= b.lo && a.hi <= b.hi && a.hi - a.lo < b.hi - b.lo;
            if !group.iter().any(|&a| group.iter().any(|&b| inside(&items[a], &items[b]) && dip_of(&items[a]) > dip_of(&items[b]))) {
                continue;
            }
            let mut dips: Vec<i64> = group.iter().map(|&i| dip_of(&items[i])).collect();
            dips.sort();
            let mut by_width = group.clone();
            by_width.sort_by(|&a, &b| (items[a].hi - items[a].lo).total_cmp(&(items[b].hi - items[b].lo))
                .then(dip_of(&items[a]).cmp(&dip_of(&items[b]))));
            for (&i, &dip) in by_width.iter().zip(&dips) {
                if let LaneKind::WholeRows { line_y, dip: old } = items[i].kind {
                    if dip != old {
                        self.dip.insert(items[i].key, dip);
                        items[i].y = line_y + dip;
                        items[i].kind = LaneKind::WholeRows { line_y, dip };
                    }
                }
            }
        }

        // #1299: results to the caller on one row, one below the other.
        let mut seen: HashMap<i64, i64> = HashMap::new();
        for it in items.iter_mut().filter(|it| matches!(it.kind, LaneKind::FnResult)) {
            let k = seen.entry(it.y).or_insert(0);
            if *k > 0 {
                self.slot.insert(it.key, *k);
                it.y += *k * LANE_STEP;
                it.hi += *k as f64 * FN_RESULT_SLOT_DX;
                if *k % 2 == 1 {
                    it.lo -= FN_RESULT_SLOT_LOGO_DX;
                }
            }
            *k += 1;
        }
        // #1309: the row's first result to the caller and a left arm's L
        // to the caller reaching into the gutter: both narrower.
        for i in 0..items.len() {
            if !matches!(items[i].kind, LaneKind::FnResult) || self.slot.contains_key(&items[i].key) {
                continue;
            }
            let ls: Vec<usize> = (0..items.len()).filter(|&j| matches!(items[j].kind, LaneKind::CallerL)
                && items[j].y == items[i].y && stretches_overlap((items[i].lo, items[i].hi), (items[j].lo, items[j].hi))).collect();
            if ls.is_empty() {
                continue;
            }
            self.squeezed.insert(items[i].key);
            items[i].lo = FN_RESULT_LOGO_X - FN_RESULT_SLOT_LOGO_DX - 2.0;
            items[i].hi = GUTTER_SQUEEZED_BEND_X + 6.0;
            for j in ls {
                self.squeezed.insert(items[j].key);
                items[j].lo = items[j].lo.max(GUTTER_SQUEEZED_ARM_BEND_X - 6.0);
            }
        }

        // #1307: a logo on another arrow or logo goes to its dot's other
        // side, if that is free.
        let clash = |items: &[LaneItem], i: usize, y: i64, span: (f64, f64)| {
            items.iter().enumerate().any(|(j, o)| j != i && o.y == y && stretches_overlap(span, (o.lo, o.hi)))
        };
        for i in 0..items.len() {
            let LaneKind::Logo { alt } = items[i].kind else { continue };
            let y = items[i].y;
            if clash(&items, i, y, (items[i].lo, items[i].hi)) && !clash(&items, i, y, alt) {
                self.flipped.insert(items[i].key);
                let old = (items[i].lo, items[i].hi);
                items[i].lo = alt.0;
                items[i].hi = alt.1;
                items[i].kind = LaneKind::Logo { alt: old };
            }
        }

        // #1300: movable arrows, narrowest first, each on its own track
        // where that lies on nothing placed; then each of the others on
        // the first track of its row (half a row apart) where it lies on
        // nothing placed and passes no dot.
        let (mut movable, mut placed): (Vec<LaneItem>, Vec<LaneItem>) =
            items.into_iter().partition(|it| matches!(it.kind, LaneKind::Movable { .. }));
        movable.sort_by(|a, b| (a.hi - a.lo).total_cmp(&(b.hi - b.lo))
            .then((matches!(a.kind, LaneKind::Movable { die: true, .. }))
                .cmp(&matches!(b.kind, LaneKind::Movable { die: true, .. })))
            .then(a.order.cmp(&b.order)));
        let lies_on = |it: &LaneItem, y: i64, placed: &[LaneItem]| {
            placed.iter().any(|o| o.y == y && stretches_overlap((it.lo, it.hi), (o.lo, o.hi)))
        };
        let mut left = Vec::new();
        for it in movable {
            if lies_on(&it, it.y, &placed) {
                left.push(it);
            } else {
                placed.push(it);
            }
        }
        for mut it in left {
            if let Some(y) = free_lane(&it, &placed, dots) {
                self.lane_y.insert(it.key, y);
                it.y = y;
                placed.push(it);
                continue;
            }
            // No track of its own: it stays, and the one arrow it lies on
            // moves instead, if that one has a track to go to.
            let blockers: Vec<usize> = placed.iter().enumerate()
                .filter(|(_, o)| matches!(o.kind, LaneKind::Movable { .. }) && o.y == it.y
                    && stretches_overlap((it.lo, it.hi), (o.lo, o.hi)))
                .map(|(j, _)| j).collect();
            if let [j] = blockers[..] {
                let mut b = placed.remove(j);
                placed.push(it.clone());
                if let Some(y) = free_lane(&b, &placed, dots) {
                    self.lane_y.insert(b.key, y);
                    b.y = y;
                    placed.push(b);
                    continue;
                }
                placed.pop();
                placed.insert(j, b);
            }
            placed.push(it);
        }
        self.spread_legs(&placed);
        self.recording = false;
    }

    /// Where arrows meet at a dot from the same side, one arriving (its
    /// head) and another arriving or leaving, the legs of those that dip
    /// leave the dot at angles far enough apart that no head lies on the
    /// other arrow (#1312: v's borrow returned and v lent again, on one
    /// dot). A head is about 36 degrees wide seen from the dot, so the
    /// legs go 42 and 84 degrees below (or above) a level arrow there,
    /// else 28 and 72 (20, 55 and 85 for three).
    fn spread_legs(&mut self, placed: &[LaneItem]) {
        // (dot's y, its x, goes right) -> (item, is its head, depth).
        let mut at: BTreeMap<(i64, i64, bool), Vec<(usize, bool, i64)>> = BTreeMap::new();
        for (i, it) in placed.iter().enumerate() {
            let Some((dot_y, to, from)) = it.ends else { continue };
            if !matches!(it.kind, LaneKind::Movable { .. } | LaneKind::WholeRows { .. }) {
                continue;
            }
            let depth = it.y - dot_y;
            at.entry((dot_y, to.round() as i64, from > to)).or_default().push((i, true, depth));
            at.entry((dot_y, from.round() as i64, to > from)).or_default().push((i, false, depth));
        }
        for legs in at.values() {
            if legs.len() < 2 || !legs.iter().any(|l| l.1) {
                continue;
            }
            let level: Vec<&(usize, bool, i64)> = legs.iter().filter(|l| l.2 == 0).collect();
            for down in [true, false] {
                let mut side: Vec<&(usize, bool, i64)> = legs.iter().filter(|l| l.2 != 0 && (l.2 > 0) == down).collect();
                if side.is_empty() || (side.len() == 1 && level.is_empty()) {
                    continue;
                }
                side.sort_by_key(|l| (l.2.abs(), placed[l.0].order));
                // Only where, drawn as usual, a head comes within a head's
                // width of another leg.
                let usual = |&&(i, _, depth): &&(usize, bool, i64)| {
                    let it = &placed[i];
                    let d = if self.lane_y.contains_key(&it.key) { ((it.hi - it.lo) / 3.0).min(20.0) } else { 20.0 };
                    (depth.abs() as f64).atan2(d).to_degrees()
                };
                let mut fan: Vec<(f64, bool)> = level.iter().map(|l| (0.0, l.1)).chain(side.iter().map(|l| (usual(l), l.1))).collect();
                fan.sort_by(|a, b| a.0.total_cmp(&b.0));
                if !fan.windows(2).any(|w| (w[0].1 || w[1].1) && w[1].0 - w[0].0 < HEAD_WIDTH_DEG) {
                    continue;
                }
                let level = !level.is_empty();
                let angles: &[f64] = match (level, side.len()) {
                    (true, _) => &[42.0, 84.0],
                    (false, 2) => &[28.0, 72.0],
                    _ => &[20.0, 55.0, 85.0],
                };
                for (k, &&(i, head, depth)) in side.iter().enumerate() {
                    let it = &placed[i];
                    let angle = angles[k.min(angles.len() - 1)].to_radians();
                    let d = (depth.abs() as f64 / angle.tan()).round() as i64;
                    let d = d.clamp(2, (((it.hi - it.lo) / 2.0) as i64).max(2));
                    self.leg_dx.insert((it.key, head), d);
                }
            }
        }
    }
}

/// A track of `it`'s row other than its own (half a row apart, or below
/// the rows inserted under it) where it lies on no placed arrow or logo,
/// passes no dot, its legs down (or up) to the track don't run along
/// another arrow's legs out of the same dot, and it doesn't cross the leg
/// into another arrow's head.
fn free_lane(it: &LaneItem, placed: &[LaneItem], dots: &DotPositions) -> Option<i64> {
    let LaneKind::Movable { base, .. } = it.kind else { return None };
    let lies_on = |y: i64| placed.iter().any(|o| o.y == y && stretches_overlap((it.lo, it.hi), (o.lo, o.hi)));
    let passes_dot = |y: i64| y != base
        && dots.iter().any(|&(dx, dy)| dy == y && (dx as f64) > it.lo && (dx as f64) < it.hi);
    // A leg: its dot's x, the way it goes along the row (towards the
    // arrow's other end), up or down, and whether it ends in the head.
    // (Two tails leaving one dot fork; a head beside another leg doesn't
    // read.)
    let legs = |o: &LaneItem, y: i64| -> Vec<(f64, bool, bool, bool)> {
        match o.ends {
            Some((dot_y, to, from)) if y != dot_y => vec![(to, from > to, y > dot_y, true), (from, to > from, y > dot_y, false)],
            _ => Vec::new(),
        }
    };
    let legs_meet = |y: i64| legs(it, y).iter().any(|&(x, right, down, head)| placed.iter().any(|o| {
        o.ends.is_some_and(|(dy, _, _)| Some(dy) == it.ends.map(|e| e.0))
            && legs(o, o.y).iter().any(|&(ox, oright, odown, ohead)|
                (ox - x).abs() < 1.0 && oright == right && odown == down && (head || ohead))
    }));
    // The track would cross the leg into another dipping arrow's head.
    let crosses_head = |y: i64| placed.iter().any(|o| match o.ends {
        Some((dot_y, to, from)) if o.y != dot_y && (y - dot_y).signum() == (o.y - dot_y).signum()
            && (y - dot_y).abs() < (o.y - dot_y).abs() => {
            let d = ((to - from).abs() / 3.0).min(20.0) * (y - dot_y) as f64 / (o.y - dot_y) as f64;
            let x = if from > to { to + d } else { to - d };
            x > it.lo && x < it.hi
        }
        _ => false,
    });
    // Below the arrows dipping whole rows under the row, if there are any:
    // the rows inserted for them leave room.
    let deepest = placed.iter()
        .filter(|o| o.ends.is_some_and(|(dy, _, _)| dy == base) && o.y - base >= LINE_SPACE)
        .map(|o| o.y).max();
    [base, base + LANE_STEP, base - LANE_STEP, it.y + LANE_STEP, it.y - LANE_STEP].into_iter()
        .chain(deepest.map(|y| y + LANE_STEP))
        .find(|&y| y != it.y && !lies_on(y) && !passes_dot(y) && !legs_meet(y) && !crosses_head(y))
}

impl ArrowLanes {
    /// Keeps what a probing pass draws (`untangle`).
    fn capture(&mut self, d: Drawn) {
        if self.probing {
            self.drawn.push(d);
        } else if !self.recording {
            self.painted_bottom = d.pieces.iter()
                .map(|&(a, b)| a.1.max(b.1) + 2.5)
                .chain(d.arcs.iter().map(|arc| arc.center.1 + if arc.up { 2.5 } else { HOP_RADIUS + 2.5 }))
                .chain(d.tri.iter().map(|p| p.1))
                .chain(d.logo.map(|(_, y)| y + 3.0))
                .fold(self.painted_bottom, f64::max);
        }
    }

    fn settings(&self) -> LaneSettings {
        LaneSettings {
            lane_y: self.lane_y.clone(), flipped: self.flipped.clone(), slot: self.slot.clone(),
            leg_dx: self.leg_dx.clone(), far: self.far.clone(), self_right: self.self_right.clone(),
            level_in: self.level_in.clone(),
            leg_turn: self.leg_turn.clone(),
            branch_hops: self.branch_hops.clone(),
        }
    }

    fn set(&mut self, s: LaneSettings) {
        self.lane_y = s.lane_y;
        self.flipped = s.flipped;
        self.slot = s.slot;
        self.leg_dx = s.leg_dx;
        self.far = s.far;
        self.self_right = s.self_right;
        self.level_in = s.level_in;
        self.leg_turn = s.leg_turn;
        self.branch_hops = s.branch_hops;
    }

    /// The other ways `untangle` may draw the arrow or logo `key`, given
    /// how it is drawn now (`drawn`): a column's arrow to itself on its
    /// dot's other side; a logo on its dot's other side; an arrow between
    /// two columns on another track (level with either dot, or half or a
    /// whole row off either, at most half a row above them, within `ys`),
    /// and, where it runs on a track, with its legs at other angles. With
    /// `far` (a last resort) also: its track running on past its head's
    /// dot, a logo off its dot's row, a result to the caller half a row
    /// lower, and, for an arrow whose head lies on another's shaft, its
    /// head coming in level or nearly, from either side (`level_in_ways`).
    fn alternatives(&self, key: (usize, usize), drawn: &[Drawn], ys: (i64, i64), far: bool, on_leg: bool) -> Vec<LaneSettings> {
        let now = self.settings();
        let mut out = Vec::new();
        let mine: Vec<&Drawn> = drawn.iter().filter(|d| d.key == key).collect();
        if let Some(&((tx, ty), (fx, fy), line_y)) = self.ends.get(&key) {
            if (tx, ty) == (fx, fy) {
                let mut s = now.clone();
                if !s.self_right.remove(&key) {
                    s.self_right.insert(key);
                }
                out.push(s);
                return out;
            }
            let width = (tx - fx).abs();
            // (A head coming in level, or nearly, is a last resort for a
            // head on another arrow's shaft, #1499: elsewhere it loops
            // round its dot for less.)
            let boxed = far && mine.iter().filter(|a| a.logo.is_none()).any(|a| drawn.iter()
                .any(|b| b.logo.is_none() && !same_arrow(a, b, &self.ends) && b.var != "caller" && b.to != "caller" && head_on_shaft(a, b)));
            // The track it runs on now, if any: a trapezoid's middle piece.
            let track = mine.iter().find(|d| d.logo.is_none() && d.pieces.len() == 3)
                .map(|d| d.pieces[1].0 .1.round() as i64);
            if let Some(track) = track {
                let depth = (track - ty).abs() as f64;
                let tail_depth = (track - fy).abs() as f64;
                let lane = self.lane_y.contains_key(&key);
                let default = if lane { (width / 3).min(20) } else { 20 };
                let tail = self.leg_dx.get(&(key, false)).copied().unwrap_or(default);
                // No leg steeper than 75 degrees: it would run along the
                // column it leaves or reaches.
                let steepest = |depth: f64| (depth * 15f64.to_radians().tan()).ceil() as i64;
                let mut legs = Vec::new();
                for angle in [25.0_f64, 35.0, 45.0, 55.0, 65.0, 75.0] {
                    let d = (depth / angle.to_radians().tan()).round() as i64;
                    if d < 2 || d > width - 6 {
                        continue;
                    }
                    // The tail's leg at the same angle (nested inside, or
                    // around, another arrow between the same two dots), or
                    // as it is, if there is room.
                    for t in [(tail_depth / angle.to_radians().tan()).round() as i64, tail.min(width - d - 4)] {
                        if t >= 2 && t >= steepest(tail_depth) && d + t <= width - 4 && !legs.contains(&(d, t)) {
                            legs.push((d, t));
                        }
                    }
                }
                for (d, t) in legs {
                    let mut s = now.clone();
                    s.leg_dx.insert((key, true), d);
                    s.leg_dx.insert((key, false), t);
                    out.push(s);
                }
                if far {
                    let mut s = now.clone();
                    if !s.far.remove(&key) {
                        s.far.insert(key);
                    }
                    out.push(s);
                }
            }
            let mut cands = vec![ty, fy];
            for base in [ty, fy, line_y] {
                for k in [1, -1, 2, -2] {
                    cands.push(base + k * LANE_STEP);
                }
            }
            let mut seen = std::collections::HashSet::new();
            for y in cands {
                // (Never more than half a row above its higher dot: it
                // would run among the line above's marks.)
                if y < ys.0 || y > ys.1 || y < ty.min(fy) - LANE_STEP || Some(y) == track || !seen.insert(y) {
                    continue;
                }
                // Level, on its own row: no track at all.
                if y == ty && ty == fy && track.is_none() {
                    continue;
                }
                let mut s = now.clone();
                s.lane_y.insert(key, y);
                s.leg_dx.remove(&(key, true));
                s.leg_dx.remove(&(key, false));
                s.far.remove(&key);
                s.level_in.remove(&key);
                out.push(s.clone());
                // (A last resort: the head coming in level along its dot's
                // row, or nearly, from either side, #1499.)
                if boxed && y != ty {
                    for (past, angle) in level_in_ways() {
                        let mut s = s.clone();
                        s.level_in.insert(key, angle);
                        if past { s.far.insert(key); }
                        out.push(s);
                    }
                }
            }
            // (Its current track, the head coming in level.)
            if boxed && track.is_some() {
                for (past, angle) in level_in_ways() {
                    let mut s = now.clone();
                    s.leg_dx.remove(&(key, true));
                    s.leg_dx.remove(&(key, false));
                    s.level_in.insert(key, angle);
                    if past { s.far.insert(key); } else { s.far.remove(&key); }
                    if s.level_in != now.level_in || s.far != now.far {
                        out.push(s);
                    }
                }
            }
            if on_leg {
                // Wide forks leave shallow diagonals across several columns.
                // The usual half-row tracks and 20px bends can all follow
                // one of those diagonals. Try intermediate tracks, varying
                // both bends together so neither end has to follow a leg.
                let mut leg_tracks = BTreeSet::new();
                for base in [fy, ty] {
                    for offset in [0, 8, -8, 15, -15, 23, -23, 30, -30, 38, -38, 45, -45, 53, -53, 60, -60,
                        5, -5, 11, -11, 19, -19, 26, -26, 34, -34] {
                        let y = base + offset;
                        if y < ys.0 || y > ys.1 || y < ty.min(fy) - 2 * LINE_SPACE || (y == ty && y == fy)
                            || !leg_tracks.insert(y) {
                            continue;
                        }
                        for (head, tail) in [(28, 8), (8, 28), (32, 32), (12, 12), (40, 20), (20, 40),
                            (48, 48), (30, 54), (54, 30)] {
                            if head + tail + 4 > width {
                                continue;
                            }
                            let mut s = now.clone();
                            s.lane_y.insert(key, y);
                            s.leg_dx.insert((key, true), head);
                            s.leg_dx.insert((key, false), tail);
                            s.far.remove(&key);
                            s.level_in.remove(&key);
                            out.push(s);
                        }
                    }
                }
            }
        }
        for d in &mine {
            if d.logo.is_none() {
                continue;
            }
            if d.to == "caller" {
                // (A last resort too: its L may cross the row's arrows.)
                if !far {
                    continue;
                }
                let mut s = now.clone();
                let k = s.slot.get(&key).copied().unwrap_or(0);
                s.slot.insert(key, k + 1);
                out.push(s);
            } else {
                let mut s = now.clone();
                if !s.flipped.remove(&key) {
                    s.flipped.insert(key);
                }
                out.push(s.clone());
                // Below or above its dot, on either side of it, clear of
                // a logo left on the dot (a last resort, with `far`).
                for dy in [LOGO_DIP, -LOGO_DIP].into_iter().filter(|_| far) {
                    for flip in [now.flipped.contains(&key), !now.flipped.contains(&key)] {
                        let mut s = now.clone();
                        let Some(&base) = self.logo_rows.get(&key) else { continue };
                        if base + dy < ys.0 || base + dy > ys.1 {
                            continue;
                        }
                        s.lane_y.insert(key, base + dy);
                        if flip { s.flipped.insert(key); } else { s.flipped.remove(&key); }
                        out.push(s);
                    }
                }
            }
        }
        out
    }

    /// Event-row crossings and local bends before moving a whole track.
    fn local_routes(&self, key: ArrowKey) -> Vec<LaneSettings> {
        let Some(&((tx, ty), (fx, fy), _)) = self.ends.get(&key) else { return Vec::new() };
        if tx == fx || ty != fy {
            return Vec::new();
        }
        let mut row = self.settings();
        row.lane_y.insert(key, ty);
        row.far.remove(&key);
        row.level_in.remove(&key);
        row.leg_turn.remove(&key);
        let mut routes = vec![row.clone()];
        routes.extend(row.with_local_turns(key));
        routes
    }

    fn repair_alternatives(&self, key: ArrowKey, drawn: &[Drawn], ys: (i64, i64), far: bool, local: bool) -> Vec<LaneSettings> {
        let mut routes = if local { self.local_routes(key) } else { Vec::new() };
        if local && !routes.first().is_some_and(|s| s.lane_y.get(&key) == self.lane_y.get(&key)
            && !self.far.contains(&key) && !self.level_in.contains_key(&key)) {
            routes.extend(self.settings().with_local_turns(key));
        }
        routes.extend(self.alternatives(key, drawn, ys, far, false));
        routes
    }
}

/// The ways `ArrowLanes::alternatives` brings a head in level (#1499):
/// on the tail's side or past its dot (`far`), level or at
/// `LEVEL_IN_SLANT` degrees. (Coming in level from past its dot, it can
/// read as one arrow with another leaving the dot straight on, #1349; at
/// that slant it doesn't, and its head still keeps clear of a track half
/// a row off the dot.)
fn level_in_ways() -> [(bool, i64); 4] {
    [(false, 0), (true, 0), (false, LEVEL_IN_SLANT), (true, LEVEL_IN_SLANT)]
}

/// The settings of `ArrowLanes` that `untangle` changes.
#[derive(Clone)]
struct LaneSettings {
    lane_y: HashMap<(usize, usize), i64>,
    flipped: std::collections::HashSet<(usize, usize)>,
    slot: HashMap<(usize, usize), i64>,
    leg_dx: HashMap<((usize, usize), bool), i64>,
    far: std::collections::HashSet<(usize, usize)>,
    self_right: std::collections::HashSet<(usize, usize)>,
    level_in: HashMap<(usize, usize), i64>,
    leg_turn: HashMap<ArrowKey, i64>,
    branch_hops: BTreeSet<ArrowKey>,
}

impl LaneSettings {
    fn with_local_turns(&self, key: ArrowKey) -> [Self; 2] {
        [12, -12].map(|dy| {
            let mut route = self.clone();
            route.leg_turn.insert(key, dy);
            route
        })
    }
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

fn off_piece(c: (f64, f64), (p, q): Piece) -> f64 {
    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
    let l = dist(p, q);
    let t = if l < 1e-9 { 0.0 } else { (((c.0 - p.0) * dx + (c.1 - p.1) * dy) / (l * l)).clamp(0.0, 1.0) };
    dist(c, (p.0 + t * dx, p.1 + t * dy))
}

/// Points 1 px apart along `pieces`.
fn samples(pieces: &[Piece]) -> Vec<(f64, f64)> {
    let mut v = Vec::new();
    for &(p, q) in pieces {
        let n = (dist(p, q).ceil() as usize).max(1);
        for k in 0..=n {
            let t = k as f64 / n as f64;
            v.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
        }
    }
    v
}

fn tri_edges(t: &[(f64, f64)]) -> Vec<Piece> {
    if t.len() < 3 {
        return Vec::new();
    }
    vec![(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
}

/// How far `p` lies inside the triangle `t` (negative outside).
fn inside_tri(t: &[(f64, f64)], p: (f64, f64)) -> f64 {
    if t.len() < 3 {
        return f64::NEG_INFINITY;
    }
    let area = (t[1].0 - t[0].0) * (t[2].1 - t[0].1) - (t[2].0 - t[0].0) * (t[1].1 - t[0].1);
    let s = area.signum();
    tri_edges(t).iter().map(|&(a, b)| {
        let l = dist(a, b).max(1e-9);
        s * ((b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)) / l
    }).fold(f64::INFINITY, f64::min)
}

/// How far `p` lies from the triangle `t` (0 inside it).
fn off_tri(t: &[(f64, f64)], p: (f64, f64)) -> f64 {
    if inside_tri(t, p) >= 0.0 {
        return 0.0;
    }
    tri_edges(t).into_iter().map(|edge| off_piece(p, edge)).fold(f64::INFINITY, f64::min)
}

/// Where `p` lies along the segment a–b, in px from a, if it's within
/// `tol` of the segment's line.
fn along_piece(p: (f64, f64), a: (f64, f64), b: (f64, f64), tol: f64) -> Option<f64> {
    let l = dist(a, b);
    if l < 1e-9 {
        return None;
    }
    let (dx, dy) = ((b.0 - a.0) / l, (b.1 - a.1) / l);
    let off = ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs();
    (off <= tol).then(|| (p.0 - a.0) * dx + (p.1 - a.1) * dy)
}

/// How long a stretch of the piece p–q has the piece r–s's centre line
/// within 2.5 px (half a shaft's stroke) of its own, alongside r–s: the
/// two strokes lie at least half on each other there. Two pieces on one
/// track share their common stretch; two crossing at an angle share 5 px
/// / sin(angle) or less (12 px at 25 degrees), so a slanted shaft rising
/// across a level track at 5 degrees shares 40 px or more of it (#1468).
/// (`shared_stretch` in `rustviz-lib/tests/common/geometry.rs`.)
fn shared_stretch(a: Piece, b: Piece) -> f64 {
    stretch_within(a, b, 2.5)
}

/// `shared_stretch` with the centre lines within `half` px.
fn stretch_within((p, q): Piece, (r, s): Piece, half: f64) -> f64 {
    let (lp, lr) = (dist(p, q), dist(r, s));
    if lp < 1.0 || lr < 1.0 {
        return 0.0;
    }
    // A point p + t (q - p) / lp: its signed distance off r–s's line and
    // its place along r–s are both linear in t.
    let (dx, dy) = ((q.0 - p.0) / lp, (q.1 - p.1) / lp);
    let (ex, ey) = ((s.0 - r.0) / lr, (s.1 - r.1) / lr);
    let (wx, wy) = (p.0 - r.0, p.1 - r.1);
    let off = (wx * ey - wy * ex, dx * ey - dy * ex);
    let at = (wx * ex + wy * ey, dx * ex + dy * ey);
    // The t where a + b t lies in lo..=hi.
    let between = |(a, b): (f64, f64), lo: f64, hi: f64| -> (f64, f64) {
        if b.abs() < 1e-12 {
            return if a >= lo && a <= hi { (f64::NEG_INFINITY, f64::INFINITY) } else { (1.0, 0.0) };
        }
        let (t1, t2) = ((lo - a) / b, (hi - a) / b);
        (t1.min(t2), t1.max(t2))
    };
    let (o1, o2) = between(off, -half, half);
    let (a1, a2) = between(at, 0.0, lr);
    (o2.min(a2).min(lp) - o1.max(a1).max(0.0)).max(0.0)
}

/// A shaft's piece lying on a column's slanted leg (`on_legs`): the
/// centre lines within 2.5 px for more than 12 px, as two arrows' shafts
/// (`shared_stretch`); or, leaving (or reaching) the dot the leg ends at
/// less than 30 degrees off it, the 5 px stroke on the leg's (~5 px) for
/// more than 8 px (a borrow leaving a join dot steeply beside the leg
/// that comes into it, #1476).
fn shaft_on_leg(piece: Piece, leg: Piece) -> bool {
    let (lp, ll) = (dist(piece.0, piece.1), dist(leg.0, leg.1));
    if lp < 1.0 || ll < 1.0 {
        return false;
    }
    let sin = (((piece.1 .0 - piece.0 .0) * (leg.1 .1 - leg.0 .1) - (piece.1 .1 - piece.0 .1) * (leg.1 .0 - leg.0 .0)) / (lp * ll)).abs();
    let at_its_dot = [piece.0, piece.1].iter().any(|&e| [leg.0, leg.1].iter().any(|&f| dist(e, f) < 8.0));
    shared_stretch(piece, leg) > 12.0 || (at_its_dot && sin < 0.5 && stretch_within(piece, leg, 4.5) > 8.0)
}

/// The arrows between two columns' dots, by key (`ArrowLanes::ends`).
type Ends = HashMap<(usize, usize), ((i64, i64), (i64, i64), i64)>;

/// Two drawn marks that are one arrow (an arrow and its call's logo, or a
/// call's results to the caller): one tooltip. Not two arrows between
/// columns on different lines, the same return drawn on two rows (an end
/// copied to an arm's lines): along one track they read as one arrow with
/// two heads (#1559).
fn same_arrow(a: &Drawn, b: &Drawn, ends: &Ends) -> bool {
    let between = |d: &Drawn| d.logo.is_none() && ends.get(&d.key).is_some_and(|(t, f, _)| t != f);
    a.tip == b.tip && (a.key.1 == b.key.1 || !between(a) || !between(b))
}

/// Whether `a`'s head lies on `b`'s shaft. A shaft passing by a head may
/// not touch it (its 5 px stroke within 1 px of the triangle); one that
/// starts or ends at the head's column, or hops over the dot it points
/// at, may.
fn head_on_shaft(a: &Drawn, b: &Drawn) -> bool {
    if a.tri.len() < 3 {
        return false;
    }
    let related = b.var == a.to || b.to == a.to
        || b.arcs.iter().any(|arc| dist(arc.center, a.tri[1]) < 8.0);
    samples(&b.pieces).into_iter()
        .any(|p| inside_tri(&a.tri, p) > -1.5 || (!related && off_tri(&a.tri, p) < 3.5))
}

/// The marks that lie on others, the way the geometric checks
/// (`rustviz-lib/tests/common/geometry.rs`, `arrows-overlap` and
/// `heads-or-logos-overlap`) find them: pairs of indices into `drawn`,
/// the first one's head or logo on the second, or the two along one
/// stretch.
fn marks_on_others(drawn: &[Drawn], ends: &Ends) -> Vec<(usize, usize)> {
    arrow_mark_hits(drawn, ends).into_iter().map(|(i, j, _)| (i, j)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ArrowMarkHit { SharedShaft, Head, Logo }

fn arrow_mark_hits(drawn: &[Drawn], ends: &Ends) -> Vec<(usize, usize, ArrowMarkHit)> {
    let mut out = Vec::new();
    let arrows: Vec<usize> = (0..drawn.len()).filter(|&i| drawn[i].logo.is_none()).collect();
    // What each covers, a little wider: two whose boxes don't meet can't
    // lie on each other.
    let boxes: Vec<(f64, f64, f64, f64)> = drawn.iter().map(|d| {
        let pts = d.pieces.iter().flat_map(|&(p, q)| [p, q]).chain(d.tri.iter().copied())
            .chain(d.logo.into_iter().flat_map(|(x, y)| [(x - 1.0, y - 15.0), (x + 11.0, y + 3.0)]));
        pts.fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            |b, p| (b.0.min(p.0 - 3.0), b.1.min(p.1 - 3.0), b.2.max(p.0 + 3.0), b.3.max(p.1 + 3.0)))
    }).collect();
    let meet = |i: usize, j: usize| {
        let (a, b) = (boxes[i], boxes[j]);
        a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
    };
    // Two arrows along one stretch (more than 12 px): on one track, or
    // one slanting across the other's track at a shallow angle (#1468).
    for (n, &i) in arrows.iter().enumerate() {
        for &j in &arrows[n + 1..] {
            let (a, b) = (&drawn[i], &drawn[j]);
            if same_arrow(a, b, ends) || !meet(i, j) {
                continue;
            }
            let most = a.pieces.iter()
                .flat_map(|&p| b.pieces.iter().map(move |&q| shared_stretch(p, q)))
                .fold(0.0, f64::max);
            if most > 12.0 {
                out.push((i, j, ArrowMarkHit::SharedShaft));
            }
        }
    }
    // A head on another arrow's shaft or head.
    for &i in &arrows {
        for &j in &arrows {
            let (a, b) = (&drawn[i], &drawn[j]);
            let elbow = (a.var == "caller" && b.to == "caller" && a.to == b.var)
                || (b.var == "caller" && a.to == "caller" && b.to == a.var);
            if i == j || same_arrow(a, b, ends) || a.tri.len() < 3 || (elbow && a.key.1 == b.key.1) || !meet(i, j) {
                continue;
            }
            let shaft = head_on_shaft(a, b);
            let head = b.tri.len() == 3 && i < j
                && samples(&tri_edges(&b.tri)).into_iter().any(|p| inside_tri(&a.tri, p) > 1.0);
            if shaft || head {
                out.push((i, j, ArrowMarkHit::Head));
            }
        }
    }
    // A logo on an arrow not its own, or on another logo.
    let logo_box = |(x, y): (f64, f64)| (x - 1.0, y - 15.0, x + 11.0, y + 3.0);
    let in_box = |b: (f64, f64, f64, f64), p: (f64, f64), m: f64| p.0 > b.0 - m && p.0 < b.2 + m && p.1 > b.1 - m && p.1 < b.3 + m;
    for (i, l) in drawn.iter().enumerate() {
        let Some(at) = l.logo else { continue };
        let bx = logo_box(at);
        for &j in &arrows {
            let b = &drawn[j];
            if (b.var == l.var && b.to == l.to && b.key.1 == l.key.1) || !meet(i, j) {
                continue;
            }
            if samples(&b.pieces).into_iter().any(|p| in_box(bx, p, 1.5))
                || samples(&tri_edges(&b.tri)).into_iter().any(|p| in_box(bx, p, -1.0)) {
                out.push((i, j, ArrowMarkHit::Logo));
            }
        }
        for (j, m) in drawn.iter().enumerate().skip(i + 1) {
            let Some(mat) = m.logo else { continue };
            let c = logo_box(mat);
            if l.tip != m.tip && bx.0.max(c.0) < bx.2.min(c.2) - 1.0 && bx.1.max(c.1) < bx.3.min(c.3) - 1.0 {
                out.push((i, j, ArrowMarkHit::Logo));
            }
        }
    }
    out
}

/// The marks an arrow's shaft runs through or touches, one entry (the
/// arrow's index into `drawn`) per mark: a dot not its own end (#1316's
/// `y -> a` past z's dot), or a column's stop bar (#1344). A hop's arc
/// isn't a shaft.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkClearance {
    Shaft,
    Head,
    DotGap,
}

const SHAFT_HALF: f64 = 2.5;
// The dot's 5 px radius plus half its stroke and 1 px of visible space.
const DOT_CLEARANCE: f64 = 6.5;
const DOT_SHAFT_CLEARANCE: f64 = DOT_CLEARANCE + SHAFT_HALF;
const HOP_RADIUS: f64 = 10.0;

/// The horizontal semicircle emitted by both hop renderers.
#[derive(Clone, Copy, Debug)]
struct Hop {
    center: (f64, f64),
    up: bool,
    right: bool,
}

impl Hop {
    fn ends(self) -> Piece {
        let (x, y) = self.center;
        let dx = if self.right { HOP_RADIUS } else { -HOP_RADIUS };
        ((x - dx, y), (x + dx, y))
    }

    fn head_overlaps(self, tri: &[(f64, f64)]) -> bool {
        const TOLERANCE: f64 = 1.5;
        let (x, y) = self.center;
        if inside_tri(tri, self.center) < -HOP_RADIUS - TOLERANCE {
            return false;
        }
        let (start, end) = self.ends();
        let in_head = |p| inside_tri(tri, p) >= -TOLERANCE - 1e-9;
        if in_head(start) || in_head(end) {
            return true;
        }
        // If neither endpoint is inside the expanded triangle, the arc
        // must cross one of its three offset edges to enter it.
        let area = (tri[1].0 - tri[0].0) * (tri[2].1 - tri[0].1)
            - (tri[2].0 - tri[0].0) * (tri[1].1 - tri[0].1);
        tri_edges(tri).into_iter().any(|(a, b)| {
            let length = dist(a, b);
            if length < 1e-9 {
                return false;
            }
            let tangent = ((b.0 - a.0) / length, (b.1 - a.1) / length);
            let normal = (-tangent.1 * area.signum(), tangent.0 * area.signum());
            let offset = (a.0 - x) * normal.0 + (a.1 - y) * normal.1 - TOLERANCE;
            if offset.abs() > HOP_RADIUS {
                return false;
            }
            let along = (HOP_RADIUS * HOP_RADIUS - offset * offset).sqrt();
            [-along, along].into_iter().any(|s| {
                let p = (x + offset * normal.0 + s * tangent.0, y + offset * normal.1 + s * tangent.1);
                (if self.up { p.1 <= y } else { p.1 >= y }) && in_head(p)
            })
        })
    }
}

/// Each sharp join's two outer stroke edges. The shaft distance already
/// covers the rest of the join, including a bevel when the miter limit
/// (four stroke halves from the vertex) is exceeded.
fn shaft_miters(pieces: &[Piece], arcs: &[Hop]) -> Vec<Piece> {
    let mut edges = Vec::new();
    let mut join = |at: (f64, f64), incoming: (f64, f64), outgoing: (f64, f64)| {
        let (dx, dy) = incoming;
        let (ex, ey) = outgoing;
        let cross = dx * ey - dy * ex;
        if cross.abs() < 1e-9 {
            return;
        }
        let reach = SHAFT_HALF * cross.signum();
        let scale = reach / (1.0 + dx * ex + dy * ey);
        let offset = ((dy + ey) * scale, -(dx + ex) * scale);
        if offset.0.hypot(offset.1) <= 4.0 * SHAFT_HALF {
            let tip = (at.0 + offset.0, at.1 + offset.1);
            edges.extend([((at.0 + dy * reach, at.1 - dx * reach), tip),
                (tip, (at.0 + ey * reach, at.1 - ex * reach))]);
        }
    };
    let unit = |(p, q): Piece| {
        let length = dist(p, q);
        (length > 1e-9).then(|| ((q.0 - p.0) / length, (q.1 - p.1) / length))
    };
    let mut lines = pieces.iter().copied().filter_map(|p| unit(p).map(|u| (p, u))).peekable();
    while let Some(((p, q), u)) = lines.next() {
        if let Some(&(_, v)) = lines.peek().filter(|((start, _), _)| *start == q) {
            join(q, u, v);
        }
        for arc in arcs {
            let (start, end) = arc.ends();
            let side = if arc.up { -1.0 } else { 1.0 };
            if q == start {
                join(q, u, (0.0, side));
            }
            if p == end {
                join(p, (0.0, -side), u);
            }
        }
    }
    edges
}

fn off_hop(point: (f64, f64), center: (f64, f64), up: bool) -> f64 {
    if (point.1 <= center.1) == up || point.1 == center.1 {
        (dist(point, center) - HOP_RADIUS).abs()
    } else {
        let (x, y) = center;
        dist(point, (x - HOP_RADIUS, y)).min(dist(point, (x + HOP_RADIUS, y)))
    }
}

fn hop_opens_up((x, y): (i64, i64), preferred: bool, dots: &DotPositions) -> bool {
    let extent = (HOP_RADIUS + DOT_SHAFT_CLEARANCE).ceil() as i64;
    let center = (x as f64, y as f64);
    let clear = |up| dots.range((x - extent, i64::MIN)..=(x + extent, i64::MAX))
        .all(|&(dx, dy)| off_hop((dx as f64, dy as f64), center, up) >= DOT_SHAFT_CLEARANCE);
    if !clear(preferred) && clear(!preferred) { !preferred } else { preferred }
}

fn through_marks(drawn: &[Drawn], dots: &DotLayer, clearance: MarkClearance) -> Vec<usize> {
    dot_and_bar_hits(drawn, dots, clearance).into_iter().map(|(i, _, _)| i).collect()
}

fn dot_and_bar_hits(drawn: &[Drawn], dots: &DotLayer, clearance: MarkClearance) -> Vec<(usize, bool, usize)> {
    let mut out = Vec::new();
    for (i, d) in drawn.iter().enumerate().filter(|(_, d)| d.logo.is_none()) {
        let head = d.tri.get(1).copied();
        let ends: Vec<_> = if clearance == MarkClearance::DotGap {
            Vec::new()
        } else {
            d.pieces.iter().flat_map(|&(p, q)| [p, q]).chain(head).collect()
        };
        let miters = if clearance == MarkClearance::DotGap { shaft_miters(&d.pieces, &d.arcs) } else { Vec::new() };
        let head_extent = if clearance != MarkClearance::Shaft {
            head.map_or(0.0, |h| d.tri.iter().map(|&corner| dist(h, corner)).fold(0.0, f64::max))
        } else {
            0.0
        };
        for (mark, &(x, y)) in dots.positions.iter().enumerate() {
            let c = (x as f64, y as f64);
            if clearance != MarkClearance::DotGap {
                let reach = if ends.iter().any(|&e| dist(e, c) < 6.0) { 4.5 } else { 7.5 };
                if d.pieces.iter().any(|&p| off_piece(c, p) < reach)
                    || (clearance == MarkClearance::Head
                        && head.is_some_and(|h| dist(h, c) < head_extent + 5.0 && off_tri(&d.tri, c) < 5.0)) {
                    out.push((i, false, mark));
                }
                continue;
            }
            let belongs_to = |column: Option<u64>| column.and_then(|hash| dots.at.get(&(hash, x, y)))
                .is_some_and(|&dot| dots.dots[dot].event);
            let shaft = d.pieces.iter().enumerate().any(|(n, &(p, q))| {
                let gap = off_piece(c, (p, q));
                gap < DOT_SHAFT_CLEARANCE
                    && !(n == 0 && gap >= 4.99 && gap >= dist(p, c) - 0.01 && belongs_to(d.columns.0))
            });
            let arc = d.arcs.iter().any(|arc| off_hop(c, arc.center, arc.up) < DOT_SHAFT_CLEARANCE);
            let head = head.is_some_and(|h| dist(h, c) < head_extent + DOT_CLEARANCE
                && off_tri(&d.tri, c) < DOT_CLEARANCE && !(dist(h, c) <= 10.0 && belongs_to(d.columns.1)));
            if shaft || arc || head || miters.iter().any(|&edge| off_piece(c, edge) < DOT_CLEARANCE) {
                out.push((i, false, mark));
            }
        }
        // Along a stop bar (12 px wide, level): its middle under the shaft.
        for (mark, &c) in dots.bars.iter().enumerate() {
            let bar = ((c.0 - 6.0, c.1), (c.0 + 6.0, c.1));
            if d.pieces.iter().any(|&p| off_piece(c, p) < 4.5)
                || (clearance != MarkClearance::Shaft && d.tri.len() == 3
                    && samples(&[bar]).into_iter().any(|p| off_tri(&d.tri, p) < 2.0)) {
                out.push((i, true, mark));
            }
        }
    }
    out
}

/// The arrows whose head or shaft lies along a column's slanted leg, one
/// entry per leg (#1476: a return coming down into a lender's `else`
/// drop dot at about the angle of that lender's own `else` leg hid the
/// leg's last 10 px under its head). A head: the leg's centre line within
/// 1 px of its triangle for more than 6 px (a leg that merely ends at the
/// dot the head points at, or crosses the head, passes under it for a
/// few px). A shaft: `shaft_on_leg`. The
/// `arrow-along-leg` check in `rustviz-lib/tests/common/geometry.rs`.
fn on_legs(drawn: &[Drawn], legs: &[Piece]) -> Vec<usize> {
    leg_hits(drawn, legs).into_iter().map(|(i, _)| i).collect()
}

fn leg_hits(drawn: &[Drawn], legs: &[Piece]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (i, d) in drawn.iter().enumerate().filter(|(_, d)| d.logo.is_none()) {
        let pts = d.pieces.iter().flat_map(|&(p, q)| [p, q]).chain(d.tri.iter().copied());
        let (lo_x, lo_y, hi_x, hi_y) = pts.fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            |b, p| (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1)));
        for (mark, &(p, q)) in legs.iter().enumerate() {
            if p.0.max(q.0) < lo_x - 4.5 || p.0.min(q.0) > hi_x + 4.5 || p.1.max(q.1) < lo_y - 4.5 || p.1.min(q.1) > hi_y + 4.5 {
                continue;
            }
            let head = d.tri.len() == 3
                && samples(&[(p, q)]).into_iter().filter(|&c| inside_tri(&d.tri, c) > -1.0).count() > 6;
            let shaft = d.pieces.iter().any(|&piece| shaft_on_leg(piece, (p, q)));
            if head || shaft {
                out.push((i, mark));
            }
        }
    }
    out
}

/// The arrows whose head lies along a column's vertical stretch, one
/// entry per stretch (#1488: a copy dipping two rows below its line came
/// back up into its target's first dot at about 70 degrees, its head
/// over the first 10 px of the target's column). The same test as a
/// head on a slanted leg (`on_legs`): the stretch's centre line within
/// 1 px of the head's triangle for more than 6 px; a level head, or one
/// coming in at the usual 56 degrees, covers a few px at most. The
/// `arrow-along-leg` check in `rustviz-lib/tests/common/geometry.rs`.
fn on_cols(drawn: &[Drawn], cols: &[Piece]) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, d) in drawn.iter().enumerate().filter(|(_, d)| d.logo.is_none() && d.tri.len() == 3) {
        let (lo_x, hi_x) = d.tri.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |b, p| (b.0.min(p.0), b.1.max(p.0)));
        let (lo_y, hi_y) = d.tri.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |b, p| (b.0.min(p.1), b.1.max(p.1)));
        for &(p, q) in cols {
            if p.0 < lo_x - 2.0 || p.0 > hi_x + 2.0 || p.1.max(q.1) < lo_y - 2.0 || p.1.min(q.1) > hi_y + 2.0 {
                continue;
            }
            if samples(&[(p, q)]).into_iter().filter(|&c| inside_tri(&d.tri, c) > -1.0).count() > 6 {
                out.push(i);
            }
        }
    }
    out
}

/// The head an arrow from `from` into the dot at `dot` is drawn with
/// (its tip on the dot's edge, 12.75 px long, 12 px wide), and the
/// middle of its base, where its shaft stops.
fn head_triangle(from: (f64, f64), dot: (f64, f64)) -> (Vec<(f64, f64)>, (f64, f64)) {
    let l = dist(from, dot).max(1e-9);
    let (ux, uy) = ((dot.0 - from.0) / l, (dot.1 - from.1) / l);
    let tip = (dot.0 - 5.25 * ux, dot.1 - 5.25 * uy);
    let base = (tip.0 - 12.75 * ux, tip.1 - 12.75 * uy);
    (vec![(base.0 - 6.0 * uy, base.1 + 6.0 * ux), tip, (base.0 + 6.0 * uy, base.1 - 6.0 * ux)], base)
}

/// Whether an arrow (its pieces, its head's triangle) lies along one of
/// the references' brackets' slanted ends: its head over more than 6 px
/// of it, or a piece within 4.5 px of it for more than 8 px at less than
/// 30 degrees (`shaft_on_leg`'s second rule, wherever it is).
fn on_bracket(pieces: &[Piece], tri: &[(f64, f64)], brackets: &[Piece]) -> bool {
    brackets.iter().any(|&b| {
        let lb = dist(b.0, b.1);
        if lb < 1.0 {
            return false;
        }
        let head = tri.len() == 3 && samples(&[b]).into_iter().filter(|&c| inside_tri(tri, c) > -1.0).count() > 6;
        head || pieces.iter().any(|&p| {
            let lp = dist(p.0, p.1);
            lp >= 1.0 && (((p.1 .0 - p.0 .0) * (b.1 .1 - b.0 .1) - (p.1 .1 - p.0 .1) * (b.1 .0 - b.0 .0)) / (lp * lb)).abs() < 0.5
                && stretch_within(p, b, 4.5) > 8.0
        })
    })
}

/// Two arrows between columns end to end on one line: the first's head
/// points at a dot and the second leaves that dot straight on in the same
/// direction, so they read as one arrow through the dot (#1349, as #577's
/// two returns did). Pairs of indices into `drawn`. (Not a call's short
/// arrow to or from its logo, nor a column's arrow to itself: `ends`
/// holds the arrows between two columns' dots.)
fn end_to_end(drawn: &[Drawn], ends: &HashMap<(usize, usize), ((i64, i64), (i64, i64), i64)>) -> Vec<(usize, usize)> {
    let between = |d: &Drawn| d.logo.is_none() && d.tri.len() == 3
        && ends.get(&d.key).is_some_and(|(t, f, _)| t != f);
    // Its piece into the head, and the one leaving its tail (the pieces
    // aren't in one order: a hop arrow's run from the tail, a polyline's
    // from the head).
    let head_piece = |d: &Drawn| d.pieces.iter().copied()
        .min_by(|a, b| dist(a.1, d.tri[1]).total_cmp(&dist(b.1, d.tri[1])));
    let tail_piece = |d: &Drawn| d.pieces.iter().copied()
        .find(|&(p, _)| !d.pieces.iter().any(|&(_, q)| dist(p, q) < 0.5));
    let mut out = Vec::new();
    for (i, a) in drawn.iter().enumerate().filter(|(_, d)| between(d)) {
        let Some((p, q)) = head_piece(a) else { continue };
        for (j, b) in drawn.iter().enumerate().filter(|(_, d)| between(d)) {
            let Some((r, t)) = tail_piece(b) else { continue };
            let (la, lb) = (dist(p, q), dist(r, t));
            if i == j || a.tip == b.tip || la < 1.0 || lb < 1.0 {
                continue;
            }
            let cos = ((q.0 - p.0) * (t.0 - r.0) + (q.1 - p.1) * (t.1 - r.1)) / (la * lb);
            if cos > 0.99 && along_piece(r, p, q, 1.0).is_some_and(|x| x > la) && dist(a.tri[1], r) < 15.0 {
                out.push((i, j));
            }
        }
    }
    out
}

/// What `untangle` tries to draw another way: the marks on others
/// (`marks_on_others`), the shafts through marks (`through_marks`, as
/// `(i, i)`), the tracks hopping dots (`track_hops`; another track that
/// passes none is better) and the arrows end to end (`end_to_end`).
fn defects(drawn: &[Drawn], dots: &DotLayer, clearance: MarkClearance) -> Vec<(usize, usize)> {
    let mut out = marks_on_others(drawn, &dots.lanes.ends);
    out.extend(head_arc_hits(drawn).map(|(head, arc, _)| (head, arc)));
    out.extend(through_marks(drawn, dots, clearance).into_iter().map(|i| (i, i)));
    out.extend(on_legs(drawn, &dots.legs).into_iter().map(|i| (i, i)));
    out.extend((0..drawn.len()).filter(|&i| drawn[i].track_hops > 0).map(|i| (i, i)));
    out.extend(end_to_end(drawn, &dots.lanes.ends));
    out
}

fn head_arc_hits(drawn: &[Drawn]) -> impl Iterator<Item = (usize, usize, &Hop)> {
    drawn.iter().enumerate().filter(|(_, a)| a.tri.len() == 3).flat_map(move |(i, a)| {
        drawn.iter().enumerate().filter(move |(j, b)| i != *j && a.tip != b.tip)
            .flat_map(move |(j, b)| b.arcs.iter().filter(move |arc| arc.head_overlaps(&a.tri)).map(move |arc| (i, j, arc)))
    })
}

/// How badly the arrows are drawn: the marks on others, the dots a track
/// hops, the marks a shaft runs through (`through_marks`) and the arrows
/// end to end.
/// Second, to choose between drawings with as few of those: how many
/// times two arrows cross.
fn untangle_cost(drawn: &[Drawn], dots: &DotLayer, clearance: MarkClearance) -> (usize, usize) {
    let through = through_marks(drawn, dots, clearance).len() + end_to_end(drawn, &dots.lanes.ends).len();
    let arrows: Vec<&Drawn> = drawn.iter().filter(|d| d.logo.is_none()).collect();
    let mut crossings = 0;
    for (n, a) in arrows.iter().enumerate() {
        for b in &arrows[n + 1..] {
            crossings += shaft_crossings(a, b);
        }
    }
    // Two arrows' pieces in line and touching (one running on where the
    // other stops reads as one arrow), however short the shared stretch.
    let in_line = |a: Piece, b: Piece| {
        let (la, lb) = (dist(a.0, a.1), dist(b.0, b.1));
        if la < 1.0 || lb < 1.0 {
            return false;
        }
        let sin = (((a.1 .0 - a.0 .0) * (b.1 .1 - b.0 .1) - (a.1 .1 - a.0 .1) * (b.1 .0 - b.0 .0)) / (la * lb)).abs();
        sin < 0.17 && [off_piece(a.0, b), off_piece(a.1, b), off_piece(b.0, a), off_piece(b.1, a)].iter().any(|&d| d < 4.0)
    };
    let mut touching = 0;
    for (n, a) in arrows.iter().enumerate() {
        for b in &arrows[n + 1..] {
            if !same_arrow(a, b, &dots.lanes.ends) && a.pieces.iter().any(|&p| b.pieces.iter().any(|&q| in_line(p, q))) {
                touching += 1;
            }
        }
    }
    // (A shaft through a dot, or two arrows reading as one, is worse
    // than a head on another arrow: never traded for one.)
    // Two logos on one spot, even of one function.
    let logos: Vec<(f64, f64)> = drawn.iter().filter_map(|d| d.logo).collect();
    let stacked = logos.iter().enumerate().filter(|&(i, a)| logos[i + 1..].iter()
        .any(|b| (a.0 - b.0).abs() < 12.0 && (a.1 - b.1).abs() < 18.0)).count();
    // A head on another arrow's hop.
    let on_arcs = head_arc_hits(drawn).map(|(i, _, _)| i).collect::<BTreeSet<_>>().len();
    let hops: usize = drawn.iter().map(|d| d.track_hops).sum();
    (marks_on_others(drawn, &dots.lanes.ends).len() + on_legs(drawn, &dots.legs).len() + on_arcs + hops + 10 * (through + touching + stacked), crossings)
}

fn shaft_crossings(a: &Drawn, b: &Drawn) -> usize {
    let cross = |(p, q): Piece, (r, t): Piece| {
        let d = (q.0 - p.0) * (t.1 - r.1) - (q.1 - p.1) * (t.0 - r.0);
        if d.abs() < 1e-9 {
            return false;
        }
        let u = ((r.0 - p.0) * (t.1 - r.1) - (r.1 - p.1) * (t.0 - r.0)) / d;
        let v = ((r.0 - p.0) * (q.1 - p.1) - (r.1 - p.1) * (q.0 - p.0)) / d;
        u > 0.0 && u < 1.0 && v > 0.0 && v < 1.0
    };
    a.pieces.iter().map(|&p| b.pieces.iter().filter(|&&q| cross(p, q)).count()).sum()
}

/// One of a field's arrows from or to several lenders other than the one
/// drawn for them all (#1354, `field_lender_groups`).
fn arrow_left_out(vd: &VisualizationData, ev: &ExternalEvent) -> bool {
    vd.field_lenders.get(&ev.get_id()).is_some_and(|f| !f.drawn)
}

/// How many rows below its line an arrow is stacked: its place among the
/// arrows its line map lists for `row`. An arrow the map doesn't list (a
/// borrow's end placed in its arm) is drawn
/// first on its line, and so is a return from a reference to itself.
fn arrow_order_of(vd: &VisualizationData, row: usize, ev: &ExternalEvent) -> i64 {
    let line_map = fetch_line_map(vd, ev.get_id());
    // (Not counting the arrows left out, `arrow_left_out`.)
    let order = line_map.get(&row)
        .and_then(|evs| evs.iter().filter(|x| !arrow_left_out(vd, x)).position(|x| x == ev))
        .unwrap_or(0) as i64;
    match ev {
        ExternalEvent::StaticDie { from, to, .. } | ExternalEvent::MutableDie { from, to, .. }
            if from.is_same_underlying(to) => 0,
        _ => order,
    }
}

/// How far below its line's row an arrow runs: a whole row per arrow
/// before it on the line (`arrow_order_of`), or, on a struct write whose
/// arrows share tracks (`VisualizationData::arrow_tracks`), half a row
/// per track before its own (#624). 0 means drawn level on the row.
fn arrow_dip_of(vd: &VisualizationData, row: usize, ev: &ExternalEvent) -> i64 {
    // The arrows between `(parameter)` columns and their `ref` bindings
    // (#747): all on one level, the borrows half a row below the row, so
    // the caller's value arrow into the column stays clear of them.
    if let Some(evs) = fetch_line_map(vd, ev.get_id()).get(&row).filter(|evs| is_param_value_line(evs)) {
        if evs.iter().any(|x| x == ev) {
            return if matches!(ev, ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. }) { LINE_SPACE / 2 } else { 0 };
        }
    }
    let arrow_order = arrow_order_of(vd, row, ev);
    match vd.arrow_tracks.get(&ev.get_id()) {
        Some(&track) if arrow_order > 0 => {
            let mut firsts: Vec<usize> = Vec::new();
            for x in fetch_line_map(vd, ev.get_id()).get(&row).into_iter().flatten() {
                let first = vd.arrow_tracks.get(&x.get_id()).copied().unwrap_or(x.get_id());
                if !firsts.contains(&first) {
                    firsts.push(first);
                }
            }
            firsts.iter().position(|&f| f == track).unwrap_or(0) as i64 * LINE_SPACE / 2
        }
        _ => LINE_SPACE * arrow_order,
    }
}

/// Is `ev` a return (a borrow's end) whose source column is where another
/// return, drawn level on the same row, ends: a reborrow's end followed by
/// its lender's end (`s` to `r`, then `r` to `x`)?
fn returns_chain_on_row(vd: &VisualizationData, row: usize, ev: &ExternalEvent) -> bool {
    let (ExternalEvent::StaticDie { from, .. } | ExternalEvent::MutableDie { from, .. }) = ev else { return false };
    let Some(src) = from.extract_rap() else { return false };
    let mut all = Vec::new();
    arrows_with_rows(&vd.external_events, &mut all);
    all.into_iter().any(|(l, other)| {
        let (ExternalEvent::StaticDie { from: of, to: ot, .. } | ExternalEvent::MutableDie { from: of, to: ot, .. }) = other else { return false };
        let (Some(of_rap), Some(ot_rap)) = (of.extract_rap(), ot.extract_rap()) else { return false };
        let id = other.get_id();
        l == row
            && !arrow_left_out(vd, other)
            && id != ev.get_id()
            && !of.is_same_underlying(ot)
            && ot_rap.hash() == src.hash()
            && of_rap.hash() != src.hash()
            && arrow_dip_of(vd, l, other) == 0
            && column_event_y(vd, of_rap.hash(), id, l) == column_event_y(vd, ot_rap.hash(), id, l)
    })
}

/// Is `ev` a return (a borrow's end) that runs back along the shaft of a
/// borrow on the same row, drawn level with it: `x`'s borrow from `v` and
/// its return to `v` on one line (`Some(x) => println!("{}", x)`; #710)?
/// Both would share one straight shaft, and only one could be hovered.
fn return_overlaps_borrow_on_row(vd: &VisualizationData, row: usize, ev: &ExternalEvent) -> bool {
    let (ExternalEvent::StaticDie { from, to, .. } | ExternalEvent::MutableDie { from, to, .. }) = ev else { return false };
    let (Some(src), Some(dst)) = (from.extract_rap(), to.extract_rap()) else { return false };
    let mut all = Vec::new();
    arrows_with_rows(&vd.external_events, &mut all);
    all.into_iter().any(|(l, other)| {
        let (ExternalEvent::StaticBorrow { from: of, to: ot, .. } | ExternalEvent::MutableBorrow { from: of, to: ot, .. }) = other else { return false };
        let (Some(of_rap), Some(ot_rap)) = (of.extract_rap(), ot.extract_rap()) else { return false };
        l == row
            && !arrow_left_out(vd, other)
            && of_rap.hash() == dst.hash()
            && ot_rap.hash() == src.hash()
            && arrow_dip_of(vd, l, other) == 0
    })
}

// render arrow
fn render_arrow (
    line_number : &usize,
    external_event: &ExternalEvent,
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    dots: &mut DotLayer,
    registry: &Handlebars
) {
    match external_event {
        ExternalEvent::Branch { branches, .. } => {
            // render all the events in the branch
            for branch in branches.iter() {
                for (l, e) in branch.e_data.iter() {
                    // somewhat redundant but have to filter out external events here
                    match e {
                        ExternalEvent::Bind {..} | ExternalEvent::GoOutOfScope {..} | ExternalEvent::InitRefParam { .. }
                        | ExternalEvent::RefDie {..} | ExternalEvent::ClosureCall {..} => {}
                        _ => {
                            render_arrow(l, e,  output, visualization_data, resource_owners_layout, dots, registry);
                        }
                    }
                }
            }
        }
        // Owned function-parameter init: draw an L-shaped arrow
        // on the right side of the param dot. The vertical leg
        // descends from above, bends at the dot's row, and a
        // horizontal stub lands on the dot's right edge with the
        // arrow head pointing left into the dot.
        //
        //              │   ← top of vertical (no arrowhead — the
        //              │     stroke ending in mid-air reads as
        //              │     "from outside this scope" by itself)
        //              │
        //   ●──────────┘   ← bend; horizontal stub ends at the
        //   param dot       dot's right edge (head pointing left).
        //
        // Skipped for ref-typed params (they're borrows, not
        // ownership transfers); falls through to the generic
        // logic below for those, which sees an (Anonymous,
        // Anonymous) extraction and early-returns.
        ExternalEvent::InitRefParam { param, id, .. } => render_init_param_arrow(line_number, param, id, output, visualization_data, resource_owners_layout, dots),
        // A call's result handed to the caller (`Err(String::from(..))`
        // as the returned value, #783). A function has no column to
        // return from, so its logo is drawn in the gutter left of the
        // fn's first column (columns start at x = 70), with the same L
        // up to the caller as a returned variable's.
        ExternalEvent::Move { from: ResourceTy::Value(ResourceAccessPoint::Function(f)), to: ResourceTy::Caller, .. }
        | ExternalEvent::Copy { from: ResourceTy::Value(ResourceAccessPoint::Function(f)), to: ResourceTy::Caller, .. } => render_function_result_arrow(line_number, external_event, f, output, visualization_data, registry, dots),
        // Move/Copy out to the caller (function tail expression):
        // L on the dot's *right* side, matching the InitRefParam
        // L's positioning so callers and returns occupy the same
        // visual lane. The polyline starts just past the dot's
        // right edge, runs right to a bend, then up, with an
        // arrowhead at the top pointing UP into mid-air.
        //
        //                    ┌   ← arrowhead pointing UP
        //                    │
        //                    │   ← vertical ascent (line `leg`)
        //                    │
        //   ●────────────────┘   ← horizontal stub (line `leg`)
        //   return value dot       touches the dot's right edge.
        //
        // Each leg's stroke is `leg`-px long; the arrowhead on
        // the vertical sits beyond that as an extra cap so neither
        // leg is visually amputated by the head's body.
        ExternalEvent::Move { from, to: ResourceTy::Caller, id, .. }
        | ExternalEvent::Copy { from, to: ResourceTy::Caller, id, .. } => render_return_to_caller_arrow(line_number, external_event, from, id, output, visualization_data, resource_owners_layout, dots),
        _ => render_generic_arrow(line_number, external_event, output, visualization_data, resource_owners_layout, dots, registry),
    }
}

/// `render_arrow` for every other event: an arrow from one column to
/// another, or a function logo with its short arrow.
fn render_generic_arrow(
    line_number : &usize,
    external_event: &ExternalEvent,
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    dots: &mut DotLayer,
    registry: &Handlebars
) {
    // get the resource owners involved in the event
    let (from, to) = ResourceAccessPoint_extract(external_event);
    if arrow_is_undrawn(external_event, from, to) || arrow_left_out(visualization_data, external_event) {
        return;
    }
    let option_ref = matches!(external_event, ExternalEvent::StaticBorrow { via: Some(_), .. } | ExternalEvent::MutableBorrow { via: Some(_), .. })
        && to.extract_rap().is_some_and(|rap| option_ref_copy(visualization_data, rap.hash(), rap.name(), Some(*line_number)).is_some());
    let id = external_event.get_id();
    let layout = &visualization_data.row_layout;
    let mut title = match (visualization_data.field_lenders.get(&id), external_event) {
        (Some(f), _) => {
            let previous = layout.field_generations.get(&id).is_some_and(|g| g.replaced);
            hover_messages::arrow_field_lenders_title(external_event, from, to, f, previous, option_ref)
        }
        (None, ExternalEvent::StaticDie { static_lender, of_ref_var, .. } | ExternalEvent::MutableDie { static_lender, of_ref_var, .. }) => {
            let table = &visualization_data.state_table;
            let lookup = |id| table.loan(*visualization_data.fn_source_lines.get(from.hash())?, id);
            let ids = table.end_loans.get(&id).map(Vec::as_slice).unwrap_or_default();
            let loan = match ids { [loan] if !layout.uncertain_store_ends.contains(&id) => lookup(*loan), _ => None };
            // Several loans can share one end arrow. Their common permission
            // is known even when no single loan describes the holder's path.
            let kind_of = |id| lookup(id).and_then(hover_messages::loan_kind_word);
            let kind = ids.first().and_then(|id| kind_of(*id)).filter(|k| ids.iter().all(|id| kind_of(*id) == Some(*k)));
            if let Some(kind) = layout.available_after_store_end.get(&id).and_then(|id| kind_of(*id)) {
                hover_messages::arrow_repoint_release(from, to, kind)
            } else {
                hover_messages::arrow_end_title(from, to, static_lender.as_ref(), *of_ref_var, loan, kind, layout.pattern_payloads.get(&id))
            }
        }
        _ => hover_messages::arrow_full_title(external_event, from, to, option_ref,
            visualization_data.row_layout.result_lend_titles.get(&external_event.get_id())),
    };

    let projected = layout.projected_loan_places.get(&id)
        .map(|(_, place)| hover_messages::projected_place_html(place));
    if let Some(end) = layout.fork_arrow_titles.get(&id) {
        title.clone_from(end);
    } else if let Some(operand) = layout.aggregate_titles.get(&id) {
        title.clone_from(operand);
    } else if let Some(&detail) = layout.stored_loans.get(&id) {
        match external_event {
            ExternalEvent::StaticBorrow { via: Some(v), .. } | ExternalEvent::MutableBorrow { via: Some(v), .. } =>
                title = hover_messages::stored_loan_message(detail, to, from, Some(&v.func), false, projected.as_deref()),
            ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. } =>
                title = hover_messages::stored_loan_message(detail, from, to, None, true, projected.as_deref()),
            _ => {}
        }
    } else if let Some(place) = projected {
        match external_event {
            ExternalEvent::StaticBorrow { via: Some(v), .. } | ExternalEvent::MutableBorrow { via: Some(v), .. } => {
                let kind = if matches!(external_event, ExternalEvent::MutableBorrow { .. }) { "Mutable" } else { "Immutable" };
                let holder = styled_name_html(&to.name());
                let method = styled_name_html(&v.func);
                title = format!("{kind} borrow of {place} stored in {holder} by {method}. {holder} isn't a reference itself. Its value holds the reference");
            }
            ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. } => {
                let loan = layout.projected_loan_places.get(&id).and_then(|(loan, _)|
                    visualization_data.state_table.loan(*visualization_data.fn_source_lines.get(from.hash())?, *loan));
                let kind = loan.and_then(hover_messages::loan_kind_word).map_or(String::new(), |k| format!(" {k}"));
                title = format!("{}'s{kind} borrow of {place} ends", styled_name_html(&from.name()));
            }
            _ => {}
        }
    }
    if visualization_data.approx_notes.iter().any(|n| matches!(n, ApproxNote::VecElement { id, nested: true, .. } if *id == external_event.get_id())) {
        title.push_str(". The element goes into an inner vector held by the destination");
    }
    if visualization_data.approx_notes.iter().any(|n| matches!(n, ApproxNote::UnreachedTransfer { id, .. } if *id == external_event.get_id())) {
        title = format!("{} {}", hover_messages::UNREACHED_TRANSFER, title);
    }

    // order of points is to -> from
    let mut data = ArrowData {
        coordinates: Vec::new(),
        coordinates_hbs: String::new(),
        head_points: String::new(),
        title: title,
        rv: Rv::new(rv_attrs::ARROW).event(rv_attrs::external_event_name(external_event))
            .var(&rv_attrs::party_name(from)).to(&rv_attrs::party_name(to)).row(*line_number).attrs(),
        key: (external_event.get_id(), *line_number),
        names: (rv_attrs::party_name(from).to_string(), rv_attrs::party_name(to).to_string()),
        columns: (from.extract_rap().map(|r| *r.hash()), to.extract_rap().map(|r| *r.hash())),
        self_right: dots.lanes.self_right.contains(&(external_event.get_id(), *line_number)),
    };

    let arrow_length = 20;

    // How far to pull the polyline endpoint back along the
    // arrow's direction before drawing, so the marker-end
    // arrow head's tip lands on the destination's near edge
    // rather than over its center.
    //
    // arrowHead marker geometry: viewBox 0 0 10 10, refX=0,
    // markerWidth=3 × strokeWidth=5 = 15 user units, tip at
    // viewBox x=8.5 → ~12.75px past the polyline endpoint.
    //
    //   - For arrows ending on a 5px-radius event dot we want
    //     12.75 + 5 = ~17.75 of pullback. 18 leaves a hairline
    //     gap so the head doesn't touch the dot.
    //   - For arrows ending at a function logo (no dot to
    //     overlap, the head should sit close to the `f`)
    //     the long-standing 10px works visually.
    //
    // Each arm of the match below sets this based on what its
    // marker-end actually points at.
    let mut head_offset: f64 = 18.0;

    match (from, to, external_event) {
        (ResourceTy::Value(ResourceAccessPoint::Function(from_function)), to_variable, _)  => {  // (Some(function), Some(variable), _)
            // ro1 (to_variable) <- ro2 (from_function)
            // arrow go from (x2, y2) -> (x1, y1)
            // get position of to_variable
            let to_timeline = fetch_timeline(to_variable.hash(), visualization_data, resource_owners_layout, external_event.get_id());
            // Anchor the polyline endpoint on the dot center;
            // the post-match `head_offset` pullback (18px)
            // moves it the right distance to land the tip on
            // the dot's near edge. The historical `+ 3` was
            // a hand-tuned partial fix for the same problem.
            let x1 = to_timeline.x_val;
            let y1 = column_event_y(visualization_data, to_variable.hash(), external_event.get_id(), *line_number);
            let y2 = y1;
            // The arrow is mostly its head; when the dot also
            // carries a call icon the head stops farther out (see
            // `head_lands_on_call_icon`), so the shaft and the
            // function's logo move out with it.
            let clearance = if head_lands_on_call_icon(&(x1 as f64, y1 as f64), dots) {
                CALL_ICON_CLEARANCE as i64
            } else {
                0
            };
            // The logo goes right of the dot, or left of it where the
            // right lies on another arrow of the row (#1307,
            // `ArrowLanes`). On a left arm's diagonal it goes left
            // first: on the right it sat inside the diamond, on the other
            // arm's leg (#1309, `diamond_arm_outer_left`).
            let key = (external_event.get_id(), *line_number);
            // (Or half a row off its dot, the arrow from it slanting:
            // `untangle`.)
            let y2 = dots.lanes.lane_y.get(&key).copied().unwrap_or(y2);
            let right = (x1 as f64 + 5.0, (x1 + arrow_length + clearance + 15) as f64);
            let left = (x1 as f64 - (arrow_length + 15) as f64, x1 as f64 - 5.0);
            let prefer_left = diamond_arm_outer_left(to_variable.hash(), visualization_data, resource_owners_layout, external_event.get_id(), x1) == Some(true);
            let (preferred, other) = if prefer_left { (left, right) } else { (right, left) };
            dots.lanes.record(key, y1, preferred.0, preferred.1, LaneKind::Logo { alt: other });
            if dots.lanes.recording {
                dots.lanes.logo_rows.insert(key, y1);
            }
            let flip = dots.lanes.flipped.contains(&key) != prefer_left;
            let (x2, logo_x) = if flip {
                (x1 - arrow_length, x1 - arrow_length - 12)
            } else {
                (x1 + arrow_length + clearance, x1 + arrow_length + clearance + 3)
            };
            data.coordinates.push((x1 as f64, y1 as f64));
            data.coordinates.push((x2 as f64, y2 as f64));
    
            let function_data = FunctionLogoData {
                x: logo_x,
                y: y2 + 5,
                hash: from_function.hash.to_owned() as u64,
                title: hover_messages::function_logo_title(from_function),
                rv: Rv::new(rv_attrs::FN).event(rv_attrs::external_event_name(external_event))
                    .var(&from_function.name).to(&rv_attrs::party_name(to_variable)).row(*line_number).attrs(),
            };
            dots.lanes.capture(Drawn {
                key, tip: function_data.title.clone(), var: from_function.name.clone(),
                to: rv_attrs::party_name(to_variable).to_string(),
                logo: Some((logo_x as f64, (y2 + 5) as f64)), ..Default::default()
            });
    
            if to_timeline.is_struct_group {
                let owner_entry = ensure_owner_entry(output, to_timeline.owner as i64);
                if to_timeline.is_member {
                    owner_entry.1.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
                } else {
                    owner_entry.0.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
                }
            }
            else {
                output.get_mut(&-1).unwrap().0.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
            }
        }
        (from_variable, ResourceTy::Value(ResourceAccessPoint::Function(function)), ExternalEvent::PassByStaticReference { .. })
        | (from_variable, ResourceTy::Value(ResourceAccessPoint::Function(function)), ExternalEvent::PassByMutableReference { .. }) => { 
            // (Some(variable), Some(function), PassByRef)
            // get position of to_variable
            let from_timeline = fetch_timeline(from_variable.hash(), visualization_data, resource_owners_layout, external_event.get_id());

            let title = if visualization_data.row_layout.reassigned_receivers.contains(&external_event.get_id()) {
                format!("{} may read or write through the receiver reserved before {} was reassigned", styled_name_html(&function.name), styled_name_html(&from_variable.name()))
            } else if let Some((_, place)) = visualization_data.row_layout.projected_loan_places.get(&external_event.get_id()) {
                let shared = visualization_data.row_layout.shared_projected_calls.contains(&external_event.get_id());
                let access = if !shared && matches!(external_event, ExternalEvent::PassByMutableReference { .. }) {
                    "may read or write"
                } else {
                    "may read"
                };
                format!("{} {access} {} through this borrow", styled_name_html(&function.name), hover_messages::projected_place_html(place))
            } else {
                hover_messages::arrow_call_title(external_event, function, from_variable)
            };
            if dots.lanes.recording || dots.lanes.probing {
                return;
            }

            // The call icon shares the dot of the borrow it makes
            // (see `DotLayer::add_call`).
            dots.add_call(
                from_variable.hash().to_owned() as u64,
                from_timeline.x_val,
                column_event_y(visualization_data, from_variable.hash(), external_event.get_id(), *line_number),
                from_timeline,
                title,
                external_event.get_id(),
                rv_attrs::external_event_name(external_event),
                *line_number,
            );
        }
        (from_variable, ResourceTy::Value(ResourceAccessPoint::Function(to_function)), _e) => { // (Some(variable), Some(function), _)
            let styled_fn_name = hover_messages::function_logo_title(to_function);
            //  ro1 (to_function) <- ro2 (from_variable)
            let from_timeline = fetch_timeline(from_variable.hash(), visualization_data, resource_owners_layout, external_event.get_id());
            // Marker-end terminates near the function logo
            // (no dot to clear) so the gentler 10px pullback
            // looks better than the dot-clearing 18px.
            head_offset = 10.0;
            let y1 = column_event_y(visualization_data, from_variable.hash(), external_event.get_id(), *line_number);
            let y2 = y1;
            // The logo goes left of the dot, or right of it where the
            // left lies on another arrow or logo of the row (#1307,
            // `ArrowLanes`). On a right arm's diagonal it goes right
            // first: on the left it sat inside the diamond, on the other
            // arm's leg (#1309, `diamond_arm_outer_left`).
            let cx = from_timeline.x_val;
            let key = (external_event.get_id(), *line_number);
            let left = ((cx - 5 - arrow_length - 12) as f64, cx as f64 - 5.0);
            let right = (cx as f64 + 5.0, (cx + 5 + arrow_length + 15) as f64);
            let prefer_right = diamond_arm_outer_left(from_variable.hash(), visualization_data, resource_owners_layout, external_event.get_id(), cx) == Some(false);
            let (preferred, other) = if prefer_right { (right, left) } else { (left, right) };
            dots.lanes.record(key, y1, preferred.0, preferred.1, LaneKind::Logo { alt: other });
            if dots.lanes.recording {
                dots.lanes.logo_rows.insert(key, y1);
            }
            let flip = dots.lanes.flipped.contains(&key) != prefer_right;
            let (x1, x2, logo_x) = if flip {
                (cx + 5 + arrow_length, cx + 5, cx + 5 + arrow_length + 5)
            } else {
                (cx - 5 - arrow_length, cx - 5, cx - 5 - arrow_length - 10)
            };
            // (Or half a row off its dot, the arrow to it slanting:
            // `untangle`.)
            let y1 = dots.lanes.lane_y.get(&key).copied().unwrap_or(y1);
            data.coordinates.push((x1 as f64, y1 as f64));
            data.coordinates.push((x2 as f64, y2 as f64));
    
            let function_data = FunctionLogoData {
                // adjust Function logo pos
                x: logo_x,  
                y: y1 + 5,
                hash: to_function.hash.to_owned() as u64,
                title: styled_fn_name,
                rv: Rv::new(rv_attrs::FN).event(rv_attrs::external_event_name(external_event))
                    .var(&rv_attrs::party_name(from_variable)).to(&to_function.name).row(*line_number).attrs(),
            };
            dots.lanes.capture(Drawn {
                key, tip: function_data.title.clone(), var: rv_attrs::party_name(from_variable).to_string(),
                to: to_function.name.clone(),
                logo: Some((logo_x as f64, (y1 + 5) as f64)), ..Default::default()
            });
    
            if from_timeline.is_struct_group {
                let owner_entry = ensure_owner_entry(output, from_timeline.owner as i64);
                if from_timeline.is_member {
                    owner_entry.1.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
                } else {
                    owner_entry.0.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
                }
            }
            else {
                output.get_mut(&-1).unwrap().0.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
            }
        },
        (from_variable, to_variable, _) => {
            if plan_variable_arrow(line_number, external_event, from_variable, to_variable, &mut data, head_offset, output, visualization_data, resource_owners_layout, dots) {
                return;
            }
        }
    }

    finish_arrow(data, head_offset, from, output, resource_owners_layout, dots, registry);
}

/// `render_arrow` for an owned function parameter's init (see the comment
/// in `render_arrow`): the L-shaped arrow from outside the scope.
fn render_init_param_arrow(line_number: &usize, param: &ResourceAccessPoint, id: &usize, output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>, visualization_data: &VisualizationData, resource_owners_layout: &BTreeMap<u64, TimelineColumnData>, dots: &mut DotLayer) {
        let is_owned = matches!(
            param,
            ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_)
        );
        // A static doesn't come from the caller (#690).
        if !is_owned || param.is_static() {
            return;
        }
        let timeline = fetch_timeline(param.hash(), visualization_data, resource_owners_layout, *id);
        let cx = timeline.x_val as f64;
        let cy = column_event_y(visualization_data, param.hash(), *id, *line_number) as f64;

        // L sized so the visible horizontal span equals
        // `target_visible` from the bend out to the arrow
        // tip. The vertical leg is shortened to a third of
        // that so it doesn't poke up into the row above —
        // the param dot sits at the same y as the function-
        // header line, and the variable's bold name label is
        // drawn just above the dot. A leg the full
        // `target_visible` tall would cut across the label.
        let leg: f64 = 10.0;
        let head_offset: f64 = 18.0;
        let arrow_tip_protrusion: f64 = 12.75;
        let target_visible: f64 = leg + arrow_tip_protrusion;
        let vertical_line: f64 = target_visible / 3.0;
        let bend_x = cx + head_offset + leg;
        let top_y = cy - vertical_line;
        // Horizontal head end after pullback: the leg of the
        // polyline runs from (bend_x, cy) leftward; pulling
        // back by 18 puts the polyline endpoint at cx + 18.
        // The arrow head's tip then extends 12.75 further
        // leftward, landing 0.25px past the dot's right edge.
        let head_x = cx + head_offset;
        let head_y = cy;
        dots.lanes.record((*id, *line_number), cy as i64, cx + 5.0, bend_x + 3.0, LaneKind::Fixed);

        // Polyline string in render order (source → bend → head).
        let polyline_pts = format!(
            "{} {} {} {} {} {}",
            bend_x, top_y,   // source = top of vertical leg
            bend_x, cy,      // bend
            head_x, head_y,  // head end (pulled-back)
        );

        // Arrowhead at the polyline endpoint, tip extends
        // 12.75 leftward into the dot's right edge; base is 6
        // above and below. Same geometry the other arrow arms
        // produce.
        let bot_v1 = (head_x, head_y + 6.0);
        let bot_v2 = (head_x - 12.75, head_y);
        let bot_v3 = (head_x, head_y - 6.0);

        let title = if param.is_copy_owner() {
            hover_messages::event_dot_copy_init_from_caller(&param.name().to_string())
        } else if param.is_plain_value() {
            hover_messages::event_dot_owner_init_from_caller_value(&param.name().to_string())
        } else {
            hover_messages::event_dot_owner_init_from_caller(&param.name().to_string())
        };

        // Reuses the regular arrow_template shape (one polygon
        // for the head) but emitted inline because we already
        // have ArrowData populated and assembling the template
        // is overkill for one site.
        let rv = Rv::new(rv_attrs::ARROW).event("init_ref_param")
            .var("caller").to(param.name()).row(*line_number).attrs();
        dots.lanes.capture(Drawn {
            key: (*id, *line_number), tip: title.clone(), var: "caller".into(), to: param.name().to_string(),
            columns: (None, Some(*param.hash())),
            pieces: vec![((bend_x, top_y), (bend_x, cy)), ((bend_x, cy), (head_x, head_y))],
            tri: vec![bot_v1, bot_v2, bot_v3], logo: None, arcs: Vec::new(), track_hops: 0,
        });
        let rendered = format!(
            "        <g class=\"tooltip-trigger\" data-tooltip-text=\"{title}\"{rv}>\n\
                \x20           <polyline class=\"arrow-shaft\" stroke-width=\"5px\" points=\"{polyline_pts}\" style=\"fill: none;\"/>\n\
                \x20           <polygon points=\"{},{} {},{} {},{}\" class=\"arrow-head\"/>\n\
                \x20       </g>\n",
            bot_v1.0, bot_v1.1, bot_v2.0, bot_v2.1, bot_v3.0, bot_v3.1,
            title = title,
            rv = rv,
            polyline_pts = polyline_pts,
        );

        if timeline.is_struct_group {
            let owner_entry = ensure_owner_entry(output, timeline.owner as i64);
            if timeline.is_member {
                owner_entry.1.arrows.push_str(&rendered);
            } else {
                owner_entry.0.arrows.push_str(&rendered);
            }
        } else {
            output.get_mut(&-1).unwrap().0.arrows.push_str(&rendered);
        }
}

/// A transfer between calls has no column endpoint. Give these transfers
/// room alongside the timelines so their marks do not cover branch legs,
/// borrow arrows or the gutter's arrows to the caller.
fn render_argument_results(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    dots: &mut DotLayer,
    registry: &Handlebars,
    width: i32,
) -> i32 {
    let results = &visualization_data.row_layout.argument_results;
    if results.is_empty() {
        return width;
    }
    let mut arrows = Vec::new();
    arrows_with_rows(&visualization_data.external_events, &mut arrows);
    let mut per_row = BTreeMap::<usize, i64>::new();
    let out = &mut output.get_mut(&-1).unwrap().0;
    const SPACING: i64 = 82;
    for (row, event) in arrows {
        if !results.contains(&event.get_id()) {
            continue;
        }
        let (from, to) = ResourceAccessPoint_extract(event);
        let (ResourceTy::Value(ResourceAccessPoint::Function(from)), ResourceTy::Value(ResourceAccessPoint::Function(to))) =
            (from, to) else { continue };
        let slot = per_row.entry(row).or_default();
        let x = i64::from(width) + 10 + *slot * SPACING;
        *slot += 1;
        let y = get_y_axis_pos(row) as i64;
        let destination = styled_name_html(&to.name);
        let title = if from.result_try.is_some() {
            hover_messages::callable_result(&destination, from, None)
        } else {
            let value = hover_messages::function_result_value(from);
            format!("The {} is passed to {destination}", value.trim_start_matches("the "))
        };
        let kind = rv_attrs::external_event_name(event);
        let rv = |mark| Rv::new(mark).event(kind).var(&from.name).to(&to.name).row(row).attrs();
        let shaft = format!("{} {y} {} {y}", x + 16, x + 38);
        let head = format!("{},{} {},{y} {},{}", x + 38, y - 5, x + 48, x + 38, y + 5);
        out.arrows.push_str(&format!(
            "<g class=\"tooltip-trigger\" data-tooltip-text=\"{title}\"{}><polyline class=\"arrow-shaft\" stroke-width=\"5px\" points=\"{shaft}\" style=\"fill: none;\"/><polygon points=\"{head}\" class=\"arrow-head\"/></g>\n",
            rv(rv_attrs::ARROW),
        ));
        let producer = FunctionLogoData {
            x, y: y + 5, hash: from.hash, title: hover_messages::function_logo_title(from), rv: rv(rv_attrs::FN),
        };
        let consumer = FunctionLogoData {
            x: x + 58, y: y + 5, hash: to.hash, title: hover_messages::function_logo_title(to), rv: rv(rv_attrs::FN),
        };
        for logo in [producer, consumer] {
            out.dots.push_str(&registry.render("function_logo_template", &logo).unwrap());
        }
        dots.lanes.painted_bottom = dots.lanes.painted_bottom.max((y + 7) as f64);
    }
    width + (per_row.values().copied().max().unwrap_or(0) * SPACING) as i32
}

/// `render_arrow` for a call's result handed to the caller (#783): the
/// function's logo in the gutter and the L up to the caller.
fn render_function_result_arrow(line_number: &usize, external_event: &ExternalEvent, f: &crate::svg_generator::data::Function, output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>, visualization_data: &VisualizationData, registry: &Handlebars, dots: &mut DotLayer) {
        let row_y = get_y_axis_pos(*line_number) as f64;
        let key = (external_event.get_id(), *line_number);
        dots.lanes.record(key, row_y as i64, FN_RESULT_LOGO_X - 2.0, FN_RESULT_BEND_X + 6.0, LaneKind::FnResult);
        // A later result on the row (`(make(), String::from("b"))`, #1299)
        // goes half a row lower, its L bending farther right, round the
        // first's.
        let slot = dots.lanes.slot.get(&key).copied().unwrap_or(0);
        let y = row_y + (slot * LANE_STEP) as f64;
        // Its logo also a little left of the one above, clear of it.
        let squeezed = dots.lanes.squeezed.contains(&key);
        let logo_x = FN_RESULT_LOGO_X - if slot % 2 == 1 || squeezed { FN_RESULT_SLOT_LOGO_DX } else { 0.0 };
        let source_x = logo_x + 16.0;
        let bend_x = if squeezed { GUTTER_SQUEEZED_BEND_X } else { FN_RESULT_BEND_X + slot as f64 * FN_RESULT_SLOT_DX };
        let head_end_y = row_y - 5.0;
        let tip: f64 = 12.75;
        let styled_fn = styled_name_html(&f.name);
        let last = f.name.rsplit("::").next().unwrap_or(&f.name);
        let named_call = !last.starts_with(|c: char| c.is_uppercase()) && !f.name.ends_with('!');
        let mut title = if let Some(title) = visualization_data.row_layout.returned_call_titles.get(&key.0) {
            title.clone()
        } else if f.mir_result && matches!(external_event, ExternalEvent::Copy { .. }) {
            let inside = match external_event {
                ExternalEvent::Copy { into_ctor: Some(name), .. } => Some(hover_messages::ctor_value_phrase(name)),
                _ => None,
            };
            let result = hover_messages::function_result_value(f);
            match inside {
                Some(value) => format!("The caller receives {value} holding {result}"),
                None => format!("The caller receives {result}"),
            }
        } else if f.callable_result && matches!(external_event, ExternalEvent::Move { .. }) {
            format!("Move of the result of {styled_fn}() to the caller")
        } else { hover_messages::arrow_fn_return_title(matches!(external_event, ExternalEvent::Copy { .. }), named_call, &styled_fn) };
        if f.callable_result {
            let alt = match external_event {
                ExternalEvent::Move { break_alt, .. } | ExternalEvent::Copy { break_alt, .. } => break_alt.as_ref(),
                _ => None,
            };
            if let Some(alt) = alt {
                if let Some(skipped) = hover_messages::skipped_break_alt(alt) {
                    title = skipped;
                } else {
                    title.push_str(&hover_messages::break_alt_if_suffix(alt));
                }
            }
        }
        let ev = rv_attrs::external_event_name(external_event);
        let rv = Rv::new(rv_attrs::ARROW).event(ev).var(&f.name).to("caller").row(*line_number).attrs();
        let rendered = format!(
            "        <g class=\"tooltip-trigger\" data-tooltip-text=\"{title}\"{rv}>\n\
                \x20           <polyline class=\"arrow-shaft\" stroke-width=\"5px\" points=\"{source_x} {y} {bend_x} {y} {bend_x} {head_end_y}\" style=\"fill: none;\"/>\n\
                \x20           <polygon points=\"{},{} {},{} {},{}\" class=\"arrow-head\"/>\n\
                \x20       </g>\n",
            bend_x - 6.0, head_end_y, bend_x, head_end_y - tip, bend_x + 6.0, head_end_y,
        );
        let logo = FunctionLogoData {
            x: logo_x as i64,
            y: y as i64 + 5,
            hash: f.hash as u64,
            title: hover_messages::function_logo_title(f),
            rv: Rv::new(rv_attrs::FN).event(ev).var(&f.name).to("caller").row(*line_number).attrs(),
        };
        dots.lanes.capture(Drawn {
            key, tip: title.clone(), var: f.name.clone(), to: "caller".into(),
            columns: (None, None),
            pieces: vec![((source_x, y), (bend_x, y)), ((bend_x, y), (bend_x, head_end_y))],
            tri: vec![(bend_x - 6.0, head_end_y), (bend_x, head_end_y - tip), (bend_x + 6.0, head_end_y)], logo: None, arcs: Vec::new(), track_hops: 0,
        });
        dots.lanes.capture(Drawn {
            key, tip: logo.title.clone(), var: f.name.clone(), to: "caller".into(),
            logo: Some((logo.x as f64, logo.y as f64)), ..Default::default()
        });
        let out = &mut output.get_mut(&-1).unwrap().0;
        out.arrows.push_str(&rendered);
        out.dots.push_str(&registry.render("function_logo_template", &logo).unwrap());
}

/// `render_arrow` for a Move/Copy out to the caller (a function's tail
/// expression): the L on the dot's right side.
fn render_return_to_caller_arrow(line_number: &usize, external_event: &ExternalEvent, from: &ResourceTy, id: &usize, output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>, visualization_data: &VisualizationData, resource_owners_layout: &BTreeMap<u64, TimelineColumnData>, dots: &mut DotLayer) {
        let rap = match from.extract_rap() {
            Some(r) => r,
            None => return,
        };
        let timeline = fetch_timeline(rap.hash(), visualization_data, resource_owners_layout, *id);
        let cx = timeline.x_val as f64;
        let cy = column_event_y(visualization_data, rap.hash(), *id, *line_number) as f64;

        let leg: f64 = 10.0;
        let head_offset: f64 = 18.0;
        let arrow_tip_protrusion: f64 = 12.75;
        // Bend at the same x as the InitRefParam L's bend
        // (cx + head_offset + leg), so caller-in and caller-
        // out arrows have their vertical legs at the same
        // column. Vertical stroke is half the input L's leg
        // length to keep the L compact (matches the spirit
        // of the input L's shortened vertical, which clears
        // the label above the param dot).
        // On an arm left of the column it forks from (the `if` arm of a
        // one-line `if c { x } else { y }`), the L goes out to the left:
        // drawn to the right, it ran into the diamond, across the other
        // arm's leg, and ended under the fork dot (#1309).
        let side = match arm_parent_x(rap.hash(), visualization_data, resource_owners_layout, *id) {
            Some(parent_x) if (timeline.x_val) < parent_x => -1.0,
            _ => 1.0,
        };
        let source_x = cx + side * 5.25;
        let mut bend_x = cx + side * (head_offset + leg);
        // Going left into the gutter where a call's result goes to the
        // caller on the same row, it bends nearer its dot, and the
        // result's L farther left (`ArrowLanes::squeezed`).
        if dots.lanes.squeezed.contains(&(*id, *line_number)) {
            bend_x = bend_x.max(GUTTER_SQUEEZED_ARM_BEND_X);
        }
        let vertical_line: f64 = leg / 2.0;
        let head_end_y = cy - vertical_line;
        dots.lanes.record((*id, *line_number), cy as i64, source_x, bend_x + side * 6.0, LaneKind::CallerL);

        let polyline_pts = format!(
            "{} {} {} {} {} {}",
            source_x, cy,        // source = dot's right edge (open end)
            bend_x, cy,          // bend
            bend_x, head_end_y,  // head end (top of vertical line)
        );

        // Arrowhead at the top of the vertical, pointing UP.
        // Direction at endpoint = (0, -1), tip extends 12.75
        // upward; base half-width 6 to either side.
        let head_v1 = (bend_x - 6.0, head_end_y);
        let head_v2 = (bend_x, head_end_y - arrow_tip_protrusion);
        let head_v3 = (bend_x + 6.0, head_end_y);

        // Returning a Copy value (i32, bool, ..) copies it out;
        // only non-Copy values are moved. Same wording as the
        // dot tooltip for this event (data.rs).
        // A returned `&T` is a copy of the reference, not of the data.
        // A returned `&mut T` moves the reference, not the data.
        let ptc = rap.points_to_copy();
        let message: Box<dyn Fn(&String) -> String> = match external_event {
            ExternalEvent::Copy { from: ResourceTy::Value(ResourceAccessPoint::StaticRef(_)), .. } =>
                Box::new(hover_messages::event_dot_copy_ref_to_caller),
            ExternalEvent::Copy { .. } => Box::new(hover_messages::event_dot_copy_to_caller),
            ExternalEvent::Move { from: ResourceTy::Value(ResourceAccessPoint::MutRef(_)), .. } =>
                Box::new(move |name: &String| hover_messages::event_dot_ref_moved(name, true, ptc, true)),
            _ => { let v = rap.holds_value(); Box::new(move |name: &String| hover_messages::event_dot_move_to_caller(name, v)) }
        };
        let title = match external_event {
            ExternalEvent::Move { carried_with: Some((parent, _)), .. } =>
                hover_messages::event_dot_copy_field_returned(rap.name(), parent),
            ExternalEvent::Move { via_try: Some(kind), .. } =>
                hover_messages::event_dot_move_into_try(rap.name(), Some(&"the caller".into()), *kind, rap.holds_value()),
            // `&self.s` returned: a reborrow through `self` (#784).
            ExternalEvent::Copy { from: ResourceTy::DerefField(r, place, _, _), .. } if place.starts_with('&') =>
                hover_messages::event_dot_reborrow_to_caller(r.name(), place),
            // `*r` returned with r a reference: a copy of the value
            // r points to (#670).
            ExternalEvent::Copy { from: ResourceTy::DerefField(r, place, _, _), .. } if place.starts_with('*') =>
                hover_messages::event_dot_copy_deref_to_caller(r.name(), place),
            // `p.x` returned from behind a reference p: the field
            // is copied, not p's resource (#266).
            ExternalEvent::Copy { from: ResourceTy::DerefField(r, place, _, _), .. } =>
                hover_messages::event_dot_copy_through_deref_field(
                    r.name(), place, &"the caller".to_string(), r.points_to_copy()),
            // `&PRIMES` / `v[0]` returned (#1053, #1052).
            ExternalEvent::Copy { from: ResourceTy::Index(r, place, _), .. } if place.starts_with('&') =>
                hover_messages::event_dot_static_ref_to_caller(r.name(), place),
            ExternalEvent::Copy { from: ResourceTy::Index(r, place, _), .. } =>
                hover_messages::event_dot_copy_element_to_caller(
                    r.name(), place, r.is_ref(), if r.is_ref() { r.points_to_copy() } else { r.holds_value() }),
            // `*b` returned with b a Box: its contents go to the caller,
            // not b (#1191).
            ExternalEvent::Move { from: ResourceTy::Deref(r), .. } | ExternalEvent::Copy { from: ResourceTy::Deref(r), .. } =>
                message(&format!("*{}", r.name())),
            _ => message(&rap.name().to_string()),
        };

        let alt = match external_event {
            ExternalEvent::Move { break_alt, .. } | ExternalEvent::Copy { break_alt, .. } => break_alt.as_ref(),
            _ => None,
        };
        let title = if let Some(skipped) = alt.and_then(hover_messages::skipped_break_alt) {
            skipped
        } else {
            let mut title = visualization_data.row_layout.aggregate_titles.get(id).cloned().unwrap_or(title);
            if let Some(alt) = alt { title.push_str(&hover_messages::break_alt_if_suffix(alt)); }
            title
        };
        let rv = Rv::new(rv_attrs::ARROW).event(rv_attrs::external_event_name(external_event))
            .var(rap.name()).to("caller").row(*line_number).attrs();
        dots.lanes.capture(Drawn {
            key: (*id, *line_number), tip: title.clone(), var: rap.name().to_string(), to: "caller".into(),
            columns: (Some(*rap.hash()), None),
            pieces: vec![((source_x, cy), (bend_x, cy)), ((bend_x, cy), (bend_x, head_end_y))],
            tri: vec![head_v1, head_v2, head_v3], logo: None, arcs: Vec::new(), track_hops: 0,
        });
        let rendered = format!(
            "        <g class=\"tooltip-trigger\" data-tooltip-text=\"{title}\"{rv}>\n\
                \x20           <polyline class=\"arrow-shaft\" stroke-width=\"5px\" points=\"{polyline_pts}\" style=\"fill: none;\"/>\n\
                \x20           <polygon points=\"{},{} {},{} {},{}\" class=\"arrow-head\"/>\n\
                \x20       </g>\n",
            head_v1.0, head_v1.1, head_v2.0, head_v2.1, head_v3.0, head_v3.1,
            title = title,
            rv = rv,
            polyline_pts = polyline_pts,
        );

        if timeline.is_struct_group {
            let owner_entry = ensure_owner_entry(output, timeline.owner as i64);
            if timeline.is_member {
                owner_entry.1.arrows.push_str(&rendered);
            } else {
                owner_entry.0.arrows.push_str(&rendered);
            }
        } else {
            output.get_mut(&-1).unwrap().0.arrows.push_str(&rendered);
        }
}

/// Whether an arrow event is not drawn as an arrow at all (to the caller,
/// to or from an anonymous value, fn to fn, an element's borrow ...).
fn arrow_is_undrawn(external_event: &ExternalEvent, from: &ResourceTy, to: &ResourceTy) -> bool {
    match (from, to) { // don't render arrow for anything to caller or anonymous or fn -> fn
        (ResourceTy::Anonymous, _) | (_, ResourceTy::Caller) | (_, ResourceTy::Anonymous) | (_, ResourceTy::Temporary { .. })
        | (ResourceTy::Value(ResourceAccessPoint::Function(_)), ResourceTy::Value(ResourceAccessPoint::Function(_))) => return true,
        // A borrow stored in an element (`v[0] = &u`) and its
        // end are drawn on the lender's column only (#282).
        // (A read through an element, `let n = *slots[0];`, is an
        // arrow out of the collection's column: #438.)
        (ResourceTy::Index(..), _)
            if matches!(external_event, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. }) => return true,
        // A holder of a `&mut` parameter moved into it (`Box::new(r)`,
        // #1289) borrows the caller's data, which has no column.
        (ResourceTy::Caller, _)
            if matches!(external_event, ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. }) => return true,
        (_, ResourceTy::Index(..))
            if matches!(external_event, ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. }) => return true,
        _ => {}
    }
    // The end of a borrow of a temporary tuple's element: the
    // tuple has no column, so there's nothing to draw an arrow
    // to; only the end dot on the reference's column (#658).
    if let ExternalEvent::StaticDie { static_lender: Some(StaticLender::TupleTemporary), .. }
         | ExternalEvent::MutableDie { static_lender: Some(StaticLender::TupleTemporary), .. } = external_event {
        return true;
    }
    false
}

/// The arrow between two variables' columns, as the polyline points in
/// `data` (a straight line, or a trapezoid when it dips; see below). Returns
/// true when the arrow was already drawn whole (hopping over dots).
#[allow(clippy::too_many_arguments)]
fn plan_variable_arrow(line_number: &usize, external_event: &ExternalEvent, from_variable: &ResourceTy, to_variable: &ResourceTy, data: &mut ArrowData, head_offset: f64, output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>, visualization_data: &VisualizationData, resource_owners_layout: &BTreeMap<u64, TimelineColumnData>, dots: &mut DotLayer) -> bool {
        let from_timeline = fetch_timeline(from_variable.hash(), visualization_data, resource_owners_layout, external_event.get_id());
        let to_timeline = fetch_timeline(to_variable.hash(), visualization_data, resource_owners_layout, external_event.get_id());

        let x1 = to_timeline.x_val;
        let x2 = from_timeline.x_val;
        let y1 = column_event_y(visualization_data, to_variable.hash(), external_event.get_id(), *line_number);
        let y2 = column_event_y(visualization_data, from_variable.hash(), external_event.get_id(), *line_number);
        // A return whose source is where another return on this
        // row ends (`s` gives back to `r`, `r` to `x`) would be
        // drawn level with it and read as one long arrow
        // through `r`'s dot (#577). It dips half a row instead.
        let key = (external_event.get_id(), *line_number);
        let natural_dip = arrow_dip_of(visualization_data, *line_number, external_event);
        // The arrows dipping whole rows below a line swap tracks so they
        // don't cross (#1312, `ArrowLanes`).
        let dip = dots.lanes.dip.get(&key).copied().unwrap_or(natural_dip);
        // The same goes for a return that retraces a borrow on its
        // row (#710).
        let chained = dip == 0 && y1 == y2
            && (returns_chain_on_row(visualization_data, *line_number, external_event)
                || return_overlaps_borrow_on_row(visualization_data, *line_number, external_event));
        let row_drop = if chained { LINE_SPACE / 2 } else { dip };
        let line_y = get_y_axis_pos(*line_number);
        let mut y_row = if chained { y1 + row_drop } else { line_y + row_drop };
        let (lo, hi) = (x1.min(x2) as f64, x1.max(x2) as f64);
        if dots.lanes.recording {
            dots.lanes.ends.insert(key, ((x1, y1), (x2, y2), line_y));
        }
        let whole_rows = !chained && dip > 0 && dip % LINE_SPACE == 0
            && !visualization_data.arrow_tracks.contains_key(&external_event.get_id())
            && !fetch_line_map(visualization_data, external_event.get_id()).get(line_number).is_some_and(|evs| is_param_value_line(evs));
        if whole_rows {
            dots.lanes.record_arrow(key, y_row, y1, x1 as f64, x2 as f64, LaneKind::WholeRows { line_y, dip });
        } else if y1 == y2 {
            // On its row or a track half a row off it: it may move to
            // another track of the row, where it lies on no other arrow
            // (#1300).
            let y = if row_drop > 0 { y_row } else { y1 };
            let die = matches!(external_event, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. });
            dots.lanes.record_arrow(key, y, y1, x1 as f64, x2 as f64, LaneKind::Movable { base: y1, die });
        }
        // Moved to another track of its row: a trapezoid down (or up) to
        // it and back, the way a dipping arrow is drawn.
        // (Back on its own row: straight, hopping any dot between.)
        let lane = dots.lanes.lane_y.get(&key).copied();
        let row_drop = match lane {
            Some(y) => {
                y_row = y;
                y - y1
            }
            None => row_drop,
        };
        // (A track level with the head's dot is one for an arrow whose
        // tail's dot is higher or lower.)
        let lane = lane.filter(|&y| y != y1 || y1 != y2);
        if lane.is_some() || row_drop > 0 {
            // A trapezoid: down (or up) from the source's dot to the track,
            // along it, and back to the target's dot. Its legs are 20 px
            // wide, or as `ArrowLanes::spread_legs` sets them where they
            // meet another arrow at a dot.
            let default = if lane.is_some() { ((hi - lo) / 3.0).min(20.0) as i64 } else { 20 };
            let d_from = dots.lanes.leg_dx.get(&(key, false)).copied().unwrap_or(default);
            // (Its track running on past the head's dot, the head comes
            // back to it from the far side: `ArrowLanes::far`.)
            let far = if dots.lanes.far.contains(&key) { -1 } else { 1 };
            // A track more than a row off the head's dot, its target's
            // column running on towards it: the 20 px leg would bring the
            // head in at 70 degrees or more, along the column under (or
            // over) the dot (#1488). The head's leg comes in at a one-row
            // dip's angle instead (56 degrees), where the arrow is wide
            // enough.
            let d_to = dots.lanes.leg_dx.get(&(key, true)).copied().unwrap_or_else(|| {
                let depth = (y_row - y1).abs();
                let toward = (y_row - y1).signum() as f64;
                let (hx, hy) = (x1 as f64, y1 as f64);
                let column_on = dots.cols.iter().any(|&(p, q)| (p.0 - hx).abs() < 0.5 && (q.0 - hx).abs() < 0.5
                    && p.1.min(q.1) <= hy.min(hy + toward * 10.0) + 0.5 && p.1.max(q.1) >= hy.max(hy + toward * 10.0) - 0.5);
                if column_on && depth as f64 > default as f64 * 60f64.to_radians().tan() {
                    // (Coming back from past the dot, `far`, the leg has
                    // the room.)
                    let room = if far == 1 { (hi - lo) as i64 - d_from - 4 } else { i64::MAX };
                    // (At 65 degrees where the 56-degree head would lie on
                    // a reference's bracket leaving the dot on that side;
                    // if that does too, as it was, for `untangle`.)
                    let toward_x = if x2 <= x1 { -far } else { far };
                    [depth * 2 / 3, depth * 7 / 15].into_iter().map(|d| d.min(room).max(default)).find(|&d| {
                        let from = ((x1 + toward_x * d) as f64, y_row as f64);
                        let (tri, base) = head_triangle(from, (hx, hy));
                        !on_bracket(&[(from, base)], &tri, &dots.brackets)
                    }).unwrap_or(default)
                } else {
                    default
                }
            });
            let (x3, x4) = if x2 <= x1 { (x2 + d_from, x1 - far * d_to) } else { (x2 - d_from, x1 + far * d_to) };
            // The head coming in level along its dot's row, or nearly
            // (#1499): from the track, the leg turns `LEVEL_IN_DX` px
            // before the dot, on the tail's side or (`far`) the other one.
            if let Some(bend) = level_in_bend(dots.lanes.level_in.get(&key).copied(), far, (x1, y1), (x2, y2), y_row, d_from) {
                data.coordinates.push((x1 as f64, y1 as f64));
                data.coordinates.extend(bend.iter().map(|&(x, y)| (x as f64, y as f64)));
                data.coordinates.push((x3 as f64, y_row as f64));
                data.coordinates.push((x2 as f64, y2 as f64));
                return false;
            }
            data.coordinates.push((x1 as f64, y1 as f64));
            data.coordinates.push((x4 as f64, y_row as f64));
            data.coordinates.push((x3 as f64, y_row as f64));
            data.coordinates.push((x2 as f64, y2 as f64));
        } else { // straight line
            // Hop over any event dot that sits between the
            // arrow's source and target columns at this line.
            // Without the hop the arrow's polyline runs
            // straight through those middle dots, visually
            // overlapping them and (more importantly) eating
            // their hover events because the arrow's <g>
            // tooltip-trigger sits on top. (Not when one end
            // is on a split dot and the other isn't, see
            // `event_dot_y`: that arrow isn't level.)
            if let Some((rendered, pieces, tri, arcs)) = (y1 == y2).then(|| render_hopping_arrow(
                x1, y1, x2,
                &data.title,
                &data.rv,
                dots,
                if x2 > x1 && head_lands_on_call_icon(&(x1 as f64, y1 as f64), dots) { head_offset + CALL_ICON_CLEARANCE } else { head_offset },
                if dots.lanes.leg_turn.contains_key(&key) || matches!(external_event, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. }) {
                    &dots.legs
                } else {
                    &[]
                },
                dots.lanes.leg_turn.get(&key).copied(),
                dots.lanes.branch_hops.contains(&key),
            )).flatten() {
                dots.lanes.capture(Drawn {
                    key, tip: data.title.clone(), var: data.names.0.clone(), to: data.names.1.clone(),
                    columns: data.columns,
                    pieces, tri, logo: None, arcs, track_hops: 0,
                });
                // Inline render done; push to the right
                // output bucket and bypass the post-match
                // polyline pipeline by leaving coordinates
                // empty.
                if resource_owners_layout.contains_key(from_variable.hash()) && resource_owners_layout[from_variable.hash()].is_struct_group {
                    let is_member = resource_owners_layout[from_variable.hash()].is_member;
                    let owner_key = resource_owners_layout[from_variable.hash()].owner as i64;
                    let owner_entry = ensure_owner_entry(output, owner_key);
                    if is_member {
                        owner_entry.1.arrows.push_str(&rendered);
                    } else {
                        owner_entry.0.arrows.push_str(&rendered);
                    }
                } else {
                    output.get_mut(&-1).unwrap().0.arrows.push_str(&rendered);
                }
                return true;
            }
            data.coordinates.push((x1 as f64, y1 as f64));
            data.coordinates.push((x2 as f64, y2 as f64));
        }
    false
}

/// How far before its dot an arrow whose head comes in level
/// (`ArrowLanes::level_in`) turns level: the head (18 px) and room before
/// it. This also bounds the horizontal extent of a far-side approach.
const LEVEL_IN_DX: i64 = 26;
/// The slant, in degrees, of a head coming in nearly level
/// (`level_in_ways`).
const LEVEL_IN_SLANT: i64 = 14;

/// The two bends of an arrow whose head comes into its dot (`head`) level,
/// or `angle` degrees off level towards its track at `y_row`
/// (`ArrowLanes::level_in`, #1499), head first: where its last leg starts,
/// `LEVEL_IN_DX` px before the dot on the tail's side (`far` 1) or the
/// other (`far` -1), and where the leg before it leaves the track. A
/// far-side bend keeps that leg close to the target column. `None` where
/// it isn't asked for, the track is the dot's row, the legs wouldn't fit
/// between the dot and the tail's leg,
/// or they would run out of the figure on the left.
fn level_in_bend(angle: Option<i64>, far: i64, head: (i64, i64), tail: (i64, i64), y_row: i64, d_from: i64) -> Option<[(i64, i64); 2]> {
    let ((x1, y1), (x2, _)) = (head, tail);
    let angle = angle?;
    if y_row == y1 || x1 == x2 {
        return None;
    }
    let dir = (x2 - x1).signum() * far;
    let a = (angle as f64).to_radians();
    let (dx, dy) = ((LEVEL_IN_DX as f64 * a.cos()).round() as i64, (LEVEL_IN_DX as f64 * a.sin()).round() as i64);
    let by = y1 + (y_row - y1).signum() * dy;
    if (y_row - by).abs() < 4 {
        return None;
    }
    let track_dx = if far == -1 { LEVEL_IN_DX } else { dx + ((y_row - by).abs() * 2 / 3).max(8) };
    let (bx, lx) = (x1 + dir * dx, x1 + dir * track_dx);
    let fits = far == -1 || (lx - x1).abs() + d_from + 4 <= (x2 - x1).abs();
    (fits && bx.min(lx) >= 12).then_some([(bx, by), (lx, y_row)])
}

/// The end of `render_arrow`'s generic case: pull the head back, add the
/// arrowhead and file the finished arrow under its column.
/// Room left between an arrowhead's base and the bend before it (px).
const HEAD_BEND_CLEARANCE: f64 = 3.0;

/// Moves the bend before an arrow's endpoint (`route[1]`, the route
/// running head first) back along the leg before it (towards `route[2]`)
/// until the last leg is `need` px long, if it is shorter. Not past
/// `route[2]`.
fn ensure_head_room(route: &mut [(f64, f64)], need: f64) {
    if route.len() < 3 {
        return;
    }
    let (c0, c1, c2) = (route[0], route[1], route[2]);
    let last = ((c1.0 - c0.0).powi(2) + (c1.1 - c0.1).powi(2)).sqrt();
    let prev = ((c2.0 - c1.0).powi(2) + (c2.1 - c1.1).powi(2)).sqrt();
    if last >= need || prev < 0.5 {
        return;
    }
    let u = ((c2.0 - c1.0) / prev, (c2.1 - c1.1) / prev);
    let w = (c1.0 - c0.0, c1.1 - c0.1);
    let wu = w.0 * u.0 + w.1 * u.1;
    let disc = wu * wu - (w.0 * w.0 + w.1 * w.1) + need * need;
    if disc < 0.0 {
        return;
    }
    let t = (-wu + disc.sqrt()).min(prev);
    if t > 0.0 {
        route[1] = (c1.0 + t * u.0, c1.1 + t * u.1);
    }
}

fn finish_arrow(mut data: ArrowData, mut head_offset: f64, from: &ResourceTy, output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>, resource_owners_layout: &BTreeMap<u64, TimelineColumnData>, dots: &mut DotLayer, registry: &Handlebars) {
    // draw arrow only if data.x1 is not default value
    if !data.coordinates.is_empty() {
        let last_index = data.coordinates.len() - 1;
        if head_offset == 18.0 && data.coordinates[last_index].0 > data.coordinates[0].0
            && head_lands_on_call_icon(&data.coordinates[0], dots) {
            head_offset += CALL_ICON_CLEARANCE;
        }

        if data.coordinates.len() == 2 && data.coordinates[0].1 == data.coordinates[1].1 {
            // [0]     [last index]
            // <-------------------
            // (An arrow from a column to itself points left, or right
            // where `self_right` says.)
            if data.coordinates[0].0 < data.coordinates[last_index].0
                || (data.self_right && data.coordinates[0].0 == data.coordinates[last_index].0) {

                data.coordinates[0].0 += head_offset;
            }
            // [last index]     [0]
            // ------------------->
            else {
                data.coordinates[0].0 -= head_offset;
            }
        } else {
            // The head is pulled back along the last leg by `head_offset`.
            // A last leg shorter than that (a track a few px above its
            // target dot, turning down into it) put the endpoint past the
            // bend, and the head, drawn from the bend to the endpoint,
            // pointed away from the dot (#1540). The bend moves back along
            // the leg before it, far enough that the last leg comes into
            // the dot at an angle and keeps the head clear of the bend.
            ensure_head_room(&mut data.coordinates, head_offset + HEAD_BEND_CLEARANCE);

            if data.coordinates[0].0 < data.coordinates[last_index].0 {
                let hypotenuse = (((data.coordinates[1].0 - data.coordinates[0].0) as f64).powi(2) + ((data.coordinates[1].1 - data.coordinates[0].1) as f64).powi(2)).sqrt();
                let cos_ratio = ((data.coordinates[1].0 - data.coordinates[0].0) as f64) / hypotenuse;
                let sin_ratio = ((data.coordinates[1].1 - data.coordinates[0].1) as f64) / hypotenuse;
                data.coordinates[0].0 += cos_ratio * head_offset;
                data.coordinates[0].1 += sin_ratio * head_offset;
            }
            else {
                let hypotenuse = (((data.coordinates[0].0 - data.coordinates[1].0) as f64).powi(2) + ((data.coordinates[1].1 - data.coordinates[0].1) as f64).powi(2)).sqrt();
                let cos_ratio = ((data.coordinates[0].0 - data.coordinates[1].0) as f64) / hypotenuse;
                let sin_ratio = ((data.coordinates[1].1 - data.coordinates[0].1) as f64) / hypotenuse;
                data.coordinates[0].0 -= cos_ratio * head_offset;
                data.coordinates[0].1 += sin_ratio * head_offset;
            }
        }

        // The last point is the tail. Start it at the edge of
        // the source dot rather than its centre. Done after the
        // head pullback so an arrow between the two halves of
        // one column (`s` to `*s`, where both ends start on the
        // same dot) leaves the dot in the direction it points.
        let n = data.coordinates.len();
        data.coordinates[n - 1] =
            tail_off_dot(data.coordinates[n - 1], data.coordinates[n - 2], &dots.positions);
        if let Some(&clearance) = dots.lanes.leg_turn.get(&data.key) {
            data.coordinates = turn_across_legs(&data.coordinates, &dots.legs, clearance as f64);
        }

        // Compute the inline arrow-head triangle, sized to
        // match the previous SVG marker (viewBox 0 0 10 10,
        // markerWidth=3 × strokeWidth=5, path M 0 0 L 8.5 4
        // L 0 8 z). In user coordinates the marker is 15px
        // square; the path's tip at viewBox (8.5, 4) maps
        // to 12.75 user units forward of the reference, and
        // the base half-height is 6 user units. Reference
        // point sits at the polyline endpoint coord[0].
        //
        // Direction at the endpoint = vector from coord[1]
        // (the segment immediately preceding the endpoint
        // in the polyline traversal, both for 2-point lines
        // and for the kinked trapezoid form) to coord[0].
        {
            let ex = data.coordinates[0].0;
            let ey = data.coordinates[0].1;
            let dx = ex - data.coordinates[1].0;
            let dy = ey - data.coordinates[1].1;
            let len = (dx * dx + dy * dy).sqrt();
            let (cos, sin) = if len > 0.0 { (dx / len, dy / len) } else { (1.0, 0.0) };
            // V1 / V3 are the two base corners (perpendicular
            // ±6 from the endpoint), V2 is the tip 12.75 along
            // the line direction beyond the endpoint.
            let v1 = (ex + 6.0 * sin, ey - 6.0 * cos);
            let v2 = (ex + 12.75 * cos, ey + 12.75 * sin);
            let v3 = (ex - 6.0 * sin, ey + 6.0 * cos);
            data.head_points = format!(
                "{},{} {},{} {},{}",
                v1.0, v1.1, v2.0, v2.1, v3.0, v3.1
            );
            // A track off its dots' row hops the dots on it (#1344), the
            // way an arrow level on its row does (`render_hopping_arrow`).
            let route: Vec<(f64, f64)> = data.coordinates.iter().rev().copied().collect();
            let hops = if dots.lanes.probing && !dots.lanes.probe_hops { None } else { track_hops(&route, &dots.positions) };
            if let Some((d, pieces, arcs, n)) = hops {
                dots.lanes.capture(Drawn {
                    key: data.key, tip: data.title.clone(), var: data.names.0.clone(), to: data.names.1.clone(),
                    columns: data.columns,
                    pieces, tri: vec![v1, v2, v3], logo: None, arcs, track_hops: n,
                });
                let rendered = format!(
                    "        <g class=\"tooltip-trigger\" data-tooltip-text=\"{title}\"{rv}>\n\
                        \x20           <path class=\"arrow-shaft\" stroke-width=\"5px\" d=\"{d}\" style=\"fill: none;\"/>\n\
                        \x20           <polygon points=\"{head}\" class=\"arrow-head\"/>\n\
                        \x20       </g>\n",
                    title = data.title, rv = data.rv, d = d, head = data.head_points,
                );
                file_arrow(rendered, from, output, resource_owners_layout);
                return;
            }
            dots.lanes.capture(Drawn {
                key: data.key, tip: data.title.clone(), var: data.names.0.clone(), to: data.names.1.clone(),
                columns: data.columns,
                pieces: route.windows(2).map(|w| (w[0], w[1])).collect(),
                tri: vec![v1, v2, v3], logo: None, arcs: Vec::new(), track_hops: 0,
            });
        }

        while !data.coordinates.is_empty() {
            let recent = data.coordinates.pop();
            data.coordinates_hbs.push_str(&recent.unwrap().0.to_string());
            data.coordinates_hbs.push_str(&String::from(" "));
            data.coordinates_hbs.push_str(&recent.unwrap().1.to_string());
            data.coordinates_hbs.push_str(&String::from(" "));
        }

        file_arrow(registry.render("arrow_template", &data).unwrap(), from, output, resource_owners_layout);
    }
}

/// Files a drawn arrow under its source's column (a struct's group, or
/// the plain columns).
fn file_arrow(rendered: String, from: &ResourceTy, output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>, resource_owners_layout: &BTreeMap<u64, TimelineColumnData>) {
    // will need to change this later for structs in conditionals
    if resource_owners_layout.contains_key(from.hash()) && resource_owners_layout[from.hash()].is_struct_group {
        let is_member = resource_owners_layout[from.hash()].is_member;
        let owner_key = resource_owners_layout[from.hash()].owner as i64;
        let owner_entry = ensure_owner_entry(output, owner_key);
        if is_member {
            owner_entry.1.arrows.push_str(&rendered);
        } else {
            owner_entry.0.arrows.push_str(&rendered);
        }
    } else {
        output.get_mut(&-1).unwrap().0.arrows.push_str(&rendered);
    }
}

/// An arrow's route (tail first) with more than one piece, as a path
/// whose level pieces hop over the dots on them (#1344: `match (a, (b,
/// c))`'s `b -> x` dipping through c's dot): its `d`, its straight pieces,
/// its arcs, and how many dots it hops. `None` when no such
/// dot is in the way (a level arrow of one piece is
/// `render_hopping_arrow`'s). A hop bulges up, or down where a dot
/// half a row above would be under it.
fn track_hops(route: &[(f64, f64)], dots: &DotPositions) -> Option<(String, Vec<Piece>, Vec<Hop>, usize)> {
    const R: f64 = HOP_RADIUS;
    if route.len() < 3 {
        return None;
    }
    let on = |(p, q): Piece| -> Vec<i64> {
        if (p.1 - q.1).abs() > 1e-9 || p.1.fract() != 0.0 {
            return Vec::new();
        }
        let (lo, hi) = (p.0.min(q.0), p.0.max(q.0));
        let mut xs: Vec<i64> = dots.iter().filter(|&&(x, y)| y == p.1 as i64 && x as f64 > lo + R && (x as f64) < hi - R)
            .map(|&(x, _)| x).collect();
        if q.0 < p.0 {
            xs.reverse();
        }
        xs
    };
    if route.windows(2).all(|w| on((w[0], w[1])).is_empty()) {
        return None;
    }
    let mut d = format!("M {} {}", route[0].0, route[0].1);
    let (mut pieces, mut arcs, mut n) = (Vec::new(), Vec::new(), 0);
    for w in route.windows(2) {
        let (p, q) = (w[0], w[1]);
        let mut at = p;
        for x in on((p, q)) {
            let (xf, y) = (x as f64, p.1);
            let right = q.0 > p.0;
            let (near, far) = if right { (xf - R, xf + R) } else { (xf + R, xf - R) };
            let prefer_up = !dots.contains(&(x, y as i64 - LANE_STEP)) || dots.contains(&(x, y as i64 + LANE_STEP));
            let up = hop_opens_up((x, y as i64), prefer_up, dots);
            // Sweep 1 turns clockwise on screen: going right, over the top.
            let sweep = if right == up { 1 } else { 0 };
            d.push_str(&format!(" L {} {} A {} {} 0 0 {} {} {}", near, y, R, R, sweep, far, y));
            pieces.push((at, (near, y)));
            arcs.push(Hop { center: (xf, y), up, right });
            at = (far, y);
            n += 1;
        }
        d.push_str(&format!(" L {} {}", q.0, q.1));
        pieces.push((at, q));
    }
    Some((d, pieces, arcs, n))
}

/// Turn a level shaft across a shallow leg at a steeper angle, leaving
/// the tracks near its dots alone. The route is checked again after drawing.
fn turn_across_legs(route: &[(f64, f64)], legs: &[Piece], clearance: f64) -> Vec<(f64, f64)> {
    let mut out = vec![route[0]];
    for points in route.windows(2) {
        let (p, q) = (points[0], points[1]);
        if (p.1 - q.1).abs() < 1e-9 {
            let dir = (q.0 - p.0).signum();
            let mut crossings: Vec<_> = legs.iter().filter_map(|&(a, b)| {
                if (b.1 - a.1).abs() < 1e-9 || !shaft_on_leg((p, q), (a, b)) {
                    return None;
                }
                let fraction = (p.1 - a.1) / (b.1 - a.1);
                (0.0..=1.0).contains(&fraction).then(|| a.0 + fraction * (b.0 - a.0))
            }).collect();
            crossings.sort_by(|a, b| (dir * a).total_cmp(&(dir * b)));
            for x in crossings {
                if dir * (x - out.last().unwrap().0) > 18.0 && dir * (q.0 - x) > 18.0 {
                    out.extend([(x - dir * 18.0, p.1), (x - dir * 8.0, p.1 + clearance),
                        (x + dir * 8.0, p.1 + clearance), (x + dir * 18.0, p.1)]);
                }
            }
        }
        out.push(q);
    }
    out
}

/// Where a column crosses a horizontal shaft, including a leg that
/// meets the row at its endpoint.
fn horizontal_crossing_x((p, q): Piece, (a, b): Piece) -> Option<f64> {
    if p.1 != q.1 || a.1 == b.1 {
        return None;
    }
    let t = (p.1 - a.1) / (b.1 - a.1);
    let x = a.0 + t * (b.0 - a.0);
    ((0.0..=1.0).contains(&t) && p.0.min(q.0) < x && x < p.0.max(q.0)).then_some(x)
}

/// Every arrow event with the row it is drawn on, including those nested
/// in branch arms.
fn arrows_with_rows<'a>(events: &'a [(usize, ExternalEvent)], out: &mut Vec<(usize, &'a ExternalEvent)>) {
    for (l, e) in events {
        match e {
            ExternalEvent::Branch { branches, .. } => {
                for b in branches {
                    arrows_with_rows(&b.e_data, out);
                }
            }
            _ => out.push((*l, e)),
        }
    }
}

/// A reference field's column (`d.a` with `a: &'p str`): it holds one
/// reference, so it points into one place at a time.
fn ref_field(ty: &ResourceTy) -> Option<&ResourceAccessPoint> {
    match ty {
        ResourceTy::Value(r) if r.is_member() && r.is_ref() => Some(r),
        _ => None,
    }
}

/// A lender with a column of its own.
fn column_lender(ty: &ResourceTy) -> Option<&ResourceAccessPoint> {
    match ty {
        ResourceTy::Value(r) if !r.is_fn() => Some(r),
        _ => None,
    }
}

/// The borrows into, and ends out of, a reference field that may point
/// into any of several lenders, one arrow drawn per field (#1354; see
/// `FieldLenders`):
/// * borrows of several lenders into one field on one row of one arm
///   (`two(&s1, &s2)` returning `Two<'a>`): the first lender's arrow is
///   drawn (in the call's argument order, else the events'), naming all;
/// * the field's borrow ends on one row of one arm: the lenders are
///   those it ends and the ones it may point into there, that is, the
///   ones its last assignment on each path to the row borrowed (the
///   arms of a conditional, `if c { Two { a: &s1, .. } } else { Two {
///   a: &s2, .. } }`). A later assignment (`hold.r = &t;`) replaces the
///   lenders before it; a borrow on the end's own row is a new loan the
///   end doesn't give back. The end to the first of them is drawn, or
///   the only one there is.
/// Only groups of two or more lenders are listed.
pub fn field_lender_groups(vd: &VisualizationData) -> HashMap<usize, FieldLenders> {
    let mut pending = FieldPending::new();
    let mut out = HashMap::new();
    field_lender_walk(&vd.external_events, &mut pending, &mut out, &vd.row_layout);
    out
}

/// Per field column: the lenders it may point into here, with the row
/// each lent on, in source order.
type FieldPending = BTreeMap<u64, Vec<(String, usize, Option<crate::event_order::FieldGeneration>)>>;
type FieldGroup = (u64, usize, bool, Option<crate::event_order::FieldGeneration>);

fn field_lender_walk(
    events: &[(usize, ExternalEvent)],
    pending: &mut FieldPending,
    out: &mut HashMap<usize, FieldLenders>,
    layout: &crate::event_order::RowLayout,
) {
    // (field, row, mutable) -> (id, lender, place in the call's arguments)
    let mut borrows: Vec<(FieldGroup, Vec<(usize, String, usize)>)> = Vec::new();
    // (field, row, mutable) -> (lenders before the row, [(id, lender)])
    let mut ends: Vec<(FieldGroup, Vec<String>, Vec<(usize, String)>)> = Vec::new();
    for (row, ev) in events {
        match ev {
            ExternalEvent::Branch { branches, .. } => {
                // After the conditional: what any arm that reaches the
                // join left (an arm that doesn't assign the field leaves
                // what it had before).
                let before = pending.clone();
                let mut merged = FieldPending::new();
                let mut any = false;
                for b in branches {
                    let mut p = before.clone();
                    field_lender_walk(&b.e_data, &mut p, out, layout);
                    if b.diverges {
                        continue;
                    }
                    any = true;
                    for (field, list) in p {
                        let m = merged.entry(field).or_default();
                        for (n, r, store) in list {
                            if !m.iter().any(|(x, ..)| *x == n) {
                                m.push((n, r, store));
                            }
                        }
                    }
                }
                if any {
                    for list in merged.values_mut() {
                        list.sort_by_key(|(_, r, _)| *r);
                    }
                    *pending = merged;
                }
            }
            ExternalEvent::StaticBorrow { from, to, id, via, is_partial: false, .. }
            | ExternalEvent::MutableBorrow { from, to, id, via, is_partial: false, .. } => {
                let (Some(field), Some(lender)) = (ref_field(to), column_lender(from)) else { continue };
                let name = from.name();
                let list = pending.entry(*field.hash()).or_default();
                // A new assignment replaces what it pointed into (the
                // other lenders of its own row are alternatives).
                let generation = layout.field_generations.get(id).copied();
                list.retain(|(_, r, prior)| r == row && *prior == generation);
                // A reborrow through a reference (`M(r)`, `r: &mut String`)
                // points into what that reference borrows: the field's end
                // names those, not the reference (#1482).
                if !lender.is_ref() && !list.iter().any(|(n, ..)| *n == name) {
                    list.push((name.clone(), *row, generation));
                }
                let key = (*field.hash(), *row, matches!(ev, ExternalEvent::MutableBorrow { .. }), generation);
                let order = via.as_ref().and_then(|v| v.lenders.iter().position(|l| l == lender.name())).unwrap_or(usize::MAX);
                match borrows.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, g)) => g.push((*id, name, order)),
                    None => borrows.push((key, vec![(*id, name, order)])),
                }
            }
            // A reference moved or copied into it (`o.inner = other;`,
            // `hold.r = tr;`) replaces what it pointed into too.
            ExternalEvent::Move { to, .. } | ExternalEvent::Copy { to, .. } => {
                if let Some(list) = ref_field(to).and_then(|f| pending.get_mut(f.hash())) {
                    list.retain(|(_, r, _)| r == row);
                }
            }
            ExternalEvent::StaticDie { from, to, id, .. } | ExternalEvent::MutableDie { from, to, id, .. } => {
                let (Some(field), Some(_)) = (ref_field(from), column_lender(to)) else { continue };

                if from.is_same_underlying(to) {
                    continue;
                }
                if layout.stored_field_ends.contains(id) { continue; }
                let key = (*field.hash(), *row, matches!(ev, ExternalEvent::MutableDie { .. }), layout.field_generations.get(id).copied());
                let before: Vec<String> = pending.get(field.hash()).into_iter().flatten()
                    .filter(|(_, r, _)| r != row && key.3.is_none()).map(|(n, ..)| n.clone()).collect();
                match ends.iter_mut().find(|(k, ..)| *k == key) {
                    Some((_, _, g)) => g.push((*id, to.name())),
                    None => ends.push((key, before, vec![(*id, to.name())])),
                }
                // What it borrowed before this row is given back here;
                // what it borrows on this row is a new loan, still held.
                if let Some(list) = pending.get_mut(field.hash()) {
                    list.retain(|(_, r, _)| r == row);
                }
            }
            _ => {}
        }
    }
    for (_, mut g) in borrows {
        // (A stable sort: lenders the call doesn't list stay in the
        // events' order.)
        g.sort_by_key(|(_, _, order)| *order);
        let mut lenders: Vec<String> = Vec::new();
        for (_, n, _) in &g {
            if !lenders.contains(n) {
                lenders.push(n.clone());
            }
        }
        if lenders.len() < 2 {
            continue;
        }
        for (k, (id, _, _)) in g.iter().enumerate() {
            out.insert(*id, FieldLenders { lenders: lenders.clone(), drawn: k == 0, end: false });
        }
    }
    for (_, before, g) in ends {
        let mut lenders = before;
        for (_, n) in &g {
            if !lenders.contains(n) {
                lenders.push(n.clone());
            }
        }
        if lenders.len() < 2 {
            continue;
        }
        let first = g.iter().enumerate()
            .min_by_key(|(_, (_, n))| lenders.iter().position(|l| l == n).unwrap_or(usize::MAX))
            .map(|(k, _)| k).unwrap_or(0);
        for (k, (id, _)) in g.iter().enumerate() {
            out.insert(*id, FieldLenders { lenders: lenders.clone(), drawn: k == first, end: true });
        }
    }
}

// render arrows that support function
fn render_arrows_string_external_events_version(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    dots: &mut DotLayer,
    registry: &Handlebars
){
    let mut all = Vec::new();
    arrows_with_rows(&visualization_data.external_events, &mut all);
    for (row, ev) in all {
        // The arrows that draw a call icon (see `render_arrow`).
        if let ExternalEvent::PassByStaticReference { from, to, .. } | ExternalEvent::PassByMutableReference { from, to, .. } = ev {
            if let (Some(rap), ResourceTy::Value(ResourceAccessPoint::Function(_))) = (from.extract_rap(), to) {
                if rap.is_fn() || !resource_owners_layout.contains_key(rap.hash()) {
                    continue;
                }
                let x = fetch_timeline(rap.hash(), visualization_data, resource_owners_layout, ev.get_id()).x_val;
                dots.call_spots.insert((x, column_event_y(visualization_data, rap.hash(), ev.get_id(), row)));
            }
        }
    }
    // First only recording what each arrow covers, into a scratch
    // output, so that the tracks can be picked (`ArrowLanes`); then
    // probing passes (`untangle`); then for real.
    let mut scratch: BTreeMap<i64, (TimelinePanelData, TimelinePanelData)> = BTreeMap::new();
    scratch.insert(-1, (TimelinePanelData::empty(), TimelinePanelData::empty()));
    dots.lanes = ArrowLanes { recording: true, ..Default::default() };
    render_all_arrows(&mut scratch, visualization_data, resource_owners_layout, dots, registry);
    let positions = dots.positions.clone();
    dots.lanes.resolve(&positions);
    untangle(visualization_data, resource_owners_layout, dots, registry);
    render_all_arrows(output, visualization_data, resource_owners_layout, dots, registry);
}

fn render_all_arrows(
    out: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    dots: &mut DotLayer,
    registry: &Handlebars
) {
    for (line_number, external_event) in &visualization_data.external_events {
        match external_event {
            // events that should be skipped (we don't render arrows for them)
            ExternalEvent::Bind {..} | ExternalEvent::GoOutOfScope {..}
            | ExternalEvent::RefDie {..} | ExternalEvent::ClosureCall {..} => {}
            // A place moved out and back into the same place (`p.b` in
            // `p = Pair { a: .., b: p.b };`, #1411): its dot says both;
            // an arrow from the column to itself would read as a stub.
            ExternalEvent::Move { from: ResourceTy::Value(f), to: ResourceTy::Value(t), .. } if f.hash() == t.hash() => {}
            // InitRefParam falls into render_arrow because owned-param
            // initializations get an L-shaped "ownership from caller"
            // arrow there. Ref params are filtered out inside
            // render_arrow itself rather than here, since the
            // event-vs-arrow split is already a render_arrow concern.
            _ => render_arrow(line_number, external_event, out, visualization_data, resource_owners_layout, dots, registry),
        }
    }
}

/// What the arrows are drawn as with `dots.lanes`' current settings,
/// drawn into a scratch output (`ArrowLanes::probing`).
fn probe_arrows(
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    dots: &mut DotLayer,
    registry: &Handlebars
) -> Vec<Drawn> {
    let mut scratch: BTreeMap<i64, (TimelinePanelData, TimelinePanelData)> = BTreeMap::new();
    scratch.insert(-1, (TimelinePanelData::empty(), TimelinePanelData::empty()));
    dots.lanes.probing = true;
    dots.lanes.drawn.clear();
    render_all_arrows(&mut scratch, visualization_data, resource_owners_layout, dots, registry);
    dots.lanes.probing = false;
    std::mem::take(&mut dots.lanes.drawn)
}

/// The heads and logos that still lie on another arrow once `resolve`
/// has picked the tracks (#1316): several arrows into one dot from one
/// side, an arrow whose two dots are on different heights, a column's
/// arrow to itself beside an arrow into its dot, a call's logo on a
/// result to the caller, an arrow dipping into a row that has dots
/// (#1323). Probing passes draw the arrows into a scratch output; for
/// each mark on another, the two arrows' (or logos') other ways of being
/// drawn (`ArrowLanes::alternatives`) are tried, and the one that leaves
/// the fewest marks on others and shafts through dots (`untangle_cost`)
/// is kept, if it leaves fewer than now. Rows are never added: the
/// tracks stay between the figure's first and last dots or tracks.
///
/// Also a shaft through a dot or along a stop bar, and two arrows end to
/// end (#1344, #1349, `defects`). A track still on a dot hops it
/// (`track_hops`); the search is run with the probes drawing those hops
/// and without (where a hop costs little, the greedy search can settle
/// on one beside a head on a shaft, where without it would have found a
/// clean drawing), and the better drawing (with its hops) is kept.
fn untangle(
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    dots: &mut DotLayer,
    registry: &Handlebars
) {
    let start = dots.lanes.settings();
    let mut results = Vec::new();
    for hops in [false, true] {
        dots.lanes.set(start.clone());
        dots.lanes.probe_hops = hops;
        untangle_search(visualization_data, resource_owners_layout, dots, registry, MarkClearance::Shaft);
        dots.lanes.probe_hops = true;
        let drawn = probe_arrows(visualization_data, resource_owners_layout, dots, registry);
        // (On a tie, the one with fewer heads along a column, #1488.)
        let c = untangle_cost(&drawn, dots, MarkClearance::Shaft);
        let geometry = visible_arrow_defects(&drawn, dots) + on_legs(&drawn, &dots.legs).len();
        results.push(((geometry, c.0, c.1, on_cols(&drawn, &dots.cols).len()), dots.lanes.settings()));
    }
    // (The first on a tie: no hops weighed.)
    let best = results.into_iter().min_by_key(|(c, _)| *c).map(|(_, s)| s).unwrap_or_else(|| start.clone());
    dots.lanes.set(best);
    let drawn = probe_arrows(visualization_data, resource_owners_layout, dots, registry);
    let drawn = if needs_route_repair(&drawn, dots) {
        repair_dot_routes(visualization_data, resource_owners_layout, dots, registry, &start, drawn);
        probe_arrows(visualization_data, resource_owners_layout, dots, registry)
    } else {
        drawn
    };
    let drawn = if drawn.iter().any(|a| crosses_branches_off_row(a, dots)) {
        repair_arrow_routes(visualization_data, resource_owners_layout, dots, registry, drawn, RouteRepair::Crossings);
        probe_arrows(visualization_data, resource_owners_layout, dots, registry)
    } else {
        drawn
    };
    repair_arrow_routes(visualization_data, resource_owners_layout, dots, registry, drawn, RouteRepair::Heads);
}

fn repair_dot_routes(
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    dots: &mut DotLayer,
    registry: &Handlebars,
    start: &LaneSettings,
    drawn: Vec<Drawn>,
) {
    repair_arrow_routes(visualization_data, resource_owners_layout, dots, registry, drawn, RouteRepair::Dots);
    let drawn = probe_arrows(visualization_data, resource_owners_layout, dots, registry);
    let score = |d: &[Drawn], dots: &DotLayer| {
        let gaps = dot_and_bar_hits(d, dots, MarkClearance::DotGap).into_iter().filter(|&(_, bar, _)| !bar).count();
        let loops = d.iter().filter(|a| runs_past_ends(a, &dots.lanes.ends)).count();
        let detours = d.iter().map(|a| high_track_length(a, &dots.lanes.ends)).sum::<f64>();
        (gaps, loops, detours)
    };
    let mut quality = score(&drawn, dots);
    if quality == (0, 0, 0.0) {
        return;
    }
    // The affected arrow may need a lane already taken by another
    // arrow. Revisit the lanes together with dot clearance in the cost.
    let hard_hits = |d: &[Drawn], dots: &DotLayer| {
        route_hits(d, dots, MarkClearance::DotGap).into_iter()
            .filter(|h| !matches!(h, RouteHit::Crossing(..))).collect::<BTreeSet<_>>()
    };
    let mut hits = hard_hits(&drawn, dots);
    let mut best = dots.lanes.settings();
    for hops in [true, false] {
        dots.lanes.set(start.clone());
        dots.lanes.probe_hops = hops;
        untangle_search(visualization_data, resource_owners_layout, dots, registry, MarkClearance::DotGap);
        dots.lanes.probe_hops = true;
        let d = probe_arrows(visualization_data, resource_owners_layout, dots, registry);
        let candidate = score(&d, dots);
        if candidate < quality {
            let next = hard_hits(&d, dots);
            if next.is_subset(&hits) {
                best = dots.lanes.settings();
                hits = next;
                quality = candidate;
            }
        }
        if quality == (0, 0, 0.0) {
            break;
        }
    }
    dots.lanes.set(best);
}

/// One of `untangle`'s searches, from the current settings.
fn untangle_search(
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    dots: &mut DotLayer,
    registry: &Handlebars,
    clearance: MarkClearance,
) {
    let mut drawn = probe_arrows(visualization_data, resource_owners_layout, dots, registry);
    let tangled = !defects(&drawn, dots, clearance).is_empty();
    if !tangled && on_cols(&drawn, &dots.cols).is_empty() {
        return;
    }
    let dot_ys = dots.positions.iter().map(|&(_, y)| y);
    let drawn_ys = drawn.iter().flat_map(|d| d.pieces.iter().flat_map(|&(p, q)| [p.1, q.1])).map(|y| y.round() as i64);
    let ys = (dots.positions.iter().map(|&(_, y)| y).min().unwrap_or(0), dot_ys.chain(drawn_ys).max().unwrap_or(0));
    let mut cost = untangle_cost(&drawn, dots, clearance);
    // Every drawing tried costs a pass over the arrows.
    let mut budget: usize = 4000;
    let eval = |dots: &mut DotLayer, s: &LaneSettings, budget: &mut usize| {
        *budget = budget.saturating_sub(1);
        dots.lanes.set(s.clone());
        let d = probe_arrows(visualization_data, resource_owners_layout, dots, registry);
        (untangle_cost(&d, dots, clearance), d)
    };
    let keys_of = |d: &[Drawn], (i, j): (usize, usize)| {
        let mut keys = vec![d[i].key, d[j].key];
        keys.dedup();
        keys
    };
    if tangled {
        for _ in 0..24 {
            let now = dots.lanes.settings();
            let marks = defects(&drawn, dots, clearance);
            let local_keys: BTreeSet<_> = if clearance == MarkClearance::DotGap {
                on_legs(&drawn, &dots.legs).into_iter().chain(through_marks(&drawn, dots, clearance))
                    .map(|i| drawn[i].key).collect()
            } else {
                BTreeSet::new()
            };
            // One arrow or logo drawn another way, for the first mark where
            // that helps; else two, the second for a mark the first leaves;
            // a track running on past its head's dot only where nothing else
            // helps. Of the drawings with the fewest marks, the one with the
            // fewest crossings.
            let mut best: Option<((usize, usize), LaneSettings, Vec<Drawn>)> = None;
            let better = |c: (usize, usize), best: &Option<((usize, usize), LaneSettings, Vec<Drawn>)>| {
                c.0 < cost.0 && best.as_ref().is_none_or(|b| c < b.0)
            };
            for far in [false, true] {
                'one: for &m in &marks {
                    for key in keys_of(&drawn, m) {
                        for s in dots.lanes.repair_alternatives(key, &drawn, ys, far, local_keys.contains(&key)) {
                            let (c, d) = eval(dots, &s, &mut budget);
                            if better(c, &best) {
                                best = Some((c, s, d));
                            }
                        }
                        dots.lanes.set(now.clone());
                    }
                    if best.is_some() {
                        break 'one;
                    }
                }
                if best.is_some() {
                    break;
                }
                let local = clearance == MarkClearance::DotGap;
                'two: for &m in &marks {
                    for key in keys_of(&drawn, m) {
                        for s1 in dots.lanes.repair_alternatives(key, &drawn, ys, far, local) {
                            if budget == 0 {
                                break 'two;
                            }
                            let (c1, d1) = eval(dots, &s1, &mut budget);
                            if c1.0 > cost.0 + 12 {
                                dots.lanes.set(now.clone());
                                continue;
                            }
                            for m2 in defects(&d1, dots, clearance) {
                                for key2 in keys_of(&d1, m2) {
                                    dots.lanes.set(s1.clone());
                                    for s2 in dots.lanes.repair_alternatives(key2, &d1, ys, far, local) {
                                        let (c, d) = eval(dots, &s2, &mut budget);
                                        if better(c, &best) {
                                            best = Some((c, s2, d));
                                        }
                                    }
                                }
                            }
                            dots.lanes.set(now.clone());
                        }
                    }
                    if best.is_some() {
                        break 'two;
                    }
                }
                if best.is_some() {
                    break;
                }
            }
            dots.lanes.set(now);
            let Some((c, s, d)) = best else { break };
            dots.lanes.set(s);
            cost = c;
            drawn = d;
            if budget == 0 || defects(&drawn, dots, clearance).is_empty() {
                break;
            }
        }
    }
    // Then a head still along a column (#1488): the same track with its
    // legs at other angles, or running on past the head's dot and coming
    // back to it from the far side (#1316), so that the rows a dipping
    // arrow was given aren't left empty by moving it to another track.
    // (After the search above, which runs as it did without the heads.)
    // (Never bringing its head nearer another's than it was: several
    // arrows into one dot from below fan out at set angles, #1312.)
    let heads_touching = |d: &[Drawn]| {
        let heads: Vec<&Drawn> = d.iter().filter(|a| a.logo.is_none() && a.tri.len() == 3).collect();
        heads.iter().enumerate().map(|(n, a)| heads[n + 1..].iter()
            .filter(|b| samples(&tri_edges(&b.tri)).into_iter().any(|p| inside_tri(&a.tri, p) > -1.0)).count()).sum::<usize>()
    };
    // (Nor running out of the figure on the left, where it would be cut
    // off, or more than a leg's width past its right side.)
    let hi_x = dots.positions.iter().map(|&(x, _)| x as f64)
        .chain(drawn.iter().flat_map(|d| d.pieces.iter().flat_map(|&(p, q)| [p.0, q.0])))
        .fold(f64::NEG_INFINITY, f64::max);
    let inside = |d: &[Drawn]| d.iter().flat_map(|d| d.pieces.iter().flat_map(|&(p, q)| [p.0, q.0])).all(|x| x >= 10.0 && x <= hi_x + 45.0);
    // (Nor along a reference's bracket, #1544's review.)
    let brackets = dots.brackets.clone();
    let brackets_hit = |d: &[Drawn]| d.iter().filter(|a| a.logo.is_none() && on_bracket(&a.pieces, &a.tri, &brackets)).count();
    let mut keys: Vec<(usize, usize)> = on_cols(&drawn, &dots.cols).into_iter().map(|i| drawn[i].key).collect();
    keys.dedup();
    let col_cost = |c: (usize, usize), d: &[Drawn], dots: &DotLayer| (c.0 + on_cols(d, &dots.cols).len(), c.1);
    let mut now_cost = col_cost(cost, &drawn, dots);
    for key in keys {
        let now = dots.lanes.settings();
        let mut best: Option<((usize, usize), LaneSettings, Vec<Drawn>)> = None;
        // (Its head's leg as `plan_variable_arrow` lengthens it, on this
        // side of the dot or the far one, where `spread_legs` set it.)
        let mut cands = Vec::new();
        if now.leg_dx.contains_key(&(key, true)) {
            for far in [false, true] {
                let mut s = now.clone();
                s.leg_dx.remove(&(key, true));
                if far != s.far.contains(&key) {
                    if far { s.far.insert(key); } else { s.far.remove(&key); }
                }
                cands.push(s);
            }
        }
        cands.extend(dots.lanes.alternatives(key, &drawn, ys, true, false));
        for s in cands {
            if s.lane_y.get(&key) != now.lane_y.get(&key) || s.slot != now.slot || s.flipped != now.flipped || s.self_right != now.self_right
                || s.level_in != now.level_in {
                continue;
            }
            let (c, d) = eval(dots, &s, &mut budget);
            let cc = col_cost(c, &d, dots);
            // (No new mark of any other kind, and no new crossing of two
            // arrows.)
            if c.0 <= cost.0 && c.1 <= cost.1 && cc.0 < now_cost.0 && best.as_ref().is_none_or(|b| cc < b.0) && heads_touching(&d) <= heads_touching(&drawn) && inside(&d)
                && brackets_hit(&d) <= brackets_hit(&drawn) {
                best = Some((cc, s, d));
            }
        }
        dots.lanes.set(now);
        if let Some((cc, s, d)) = best {
            dots.lanes.set(s);
            now_cost = cc;
            cost = untangle_cost(&d, dots, clearance);
            drawn = d;
        }
    }
    let repair = if clearance == MarkClearance::Shaft { RouteRepair::Legs } else { RouteRepair::Dots };
    repair_arrow_routes(visualization_data, resource_owners_layout, dots, registry, drawn, repair);
}

fn needs_route_repair(drawn: &[Drawn], dots: &DotLayer) -> bool {
    // A track may deliberately dip under its event row to fan out
    // arrows to the same column. A long track above it warrants a try.
    drawn.iter().any(|a| runs_past_ends(a, &dots.lanes.ends) || high_track_length(a, &dots.lanes.ends) > 0.0)
        || dot_and_bar_hits(drawn, dots, MarkClearance::DotGap).into_iter().any(|(_, bar, _)| !bar)
}

/// The length of a full track more than half a row above its event dots.
/// Crossing arcs can split that track into several straight pieces.
fn high_track_length(arrow: &Drawn, ends: &Ends) -> f64 {
    let Some(&((tx, ty), (fx, fy), _)) = ends.get(&arrow.key) else { return 0.0 };
    if ty != fy {
        return 0.0;
    }
    let above = (ty - LANE_STEP) as f64;
    let length = arrow.pieces.iter().copied().chain(arrow.arcs.iter().map(|arc| arc.ends()))
        .filter(|(p, q)| p.1 == q.1 && p.1 < above)
        .map(|(p, q)| (p.0 - q.0).abs()).sum::<f64>();
    if length > (tx - fx).abs() as f64 / 2.0 { length } else { 0.0 }
}

/// A brief approach from the far side is sometimes needed to fan out
/// two heads. Extra coverage beyond that approach reads as a loop.
fn runs_past_ends(arrow: &Drawn, ends: &Ends) -> bool {
    let Some(&((tx, _), (fx, _), _)) = ends.get(&arrow.key) else { return false };
    if tx == fx {
        return false;
    }
    let (lo, hi) = ((tx.min(fx) - LEVEL_IN_DX) as f64, (tx.max(fx) + LEVEL_IN_DX) as f64);
    arrow.pieces.iter().any(|&(p, q)| p.0.min(q.0) < lo || p.0.max(q.0) > hi)
}

/// A displaced track crossing a branch can read as part of the branch.
/// Try putting the main track back at the height of its two event dots.
fn crosses_branches_off_row(arrow: &Drawn, dots: &DotLayer) -> bool {
    let Some(&((tx, ty), (fx, fy), _)) = dots.lanes.ends.get(&arrow.key) else { return false };
    if ty != fy || tx == fx {
        return false;
    }
    let row = ((fx as f64, fy as f64), (tx as f64, ty as f64));
    let crosses = |piece, leg| horizontal_crossing_x(piece, leg).is_some();
    let fork_on_row = dots.legs.iter().copied().any(|leg| crosses(row, leg));
    arrow.pieces.iter().copied().filter(|piece| piece.0.1 != ty as f64 && piece.0.1 == piece.1.1)
        .any(|piece| {
            if fork_on_row {
                dots.branch_stretches().any(|leg| crosses(piece, leg))
            } else {
                dots.legs.iter().copied().any(|leg| crosses(piece, leg))
            }
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RouteRepair { Legs, Dots, Crossings, Heads }

fn repair_arrow_routes(
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    dots: &mut DotLayer,
    registry: &Handlebars,
    mut drawn: Vec<Drawn>,
    repair: RouteRepair,
) {
    let painted = repair != RouteRepair::Legs;
    let clearance = if painted { MarkClearance::DotGap } else { MarkClearance::Head };
    let mut keys = BTreeSet::new();
    if matches!(repair, RouteRepair::Legs | RouteRepair::Dots) {
        keys.extend(on_legs(&drawn, &dots.legs).into_iter().map(|i| drawn[i].key));
    }
    if repair == RouteRepair::Dots {
        keys.extend(dot_and_bar_hits(&drawn, dots, clearance).into_iter()
            .filter(|&(_, bar, _)| !bar).map(|(i, _, _)| drawn[i].key));
        keys.extend(drawn.iter().filter(|a| high_track_length(a, &dots.lanes.ends) > 0.0
            || runs_past_ends(a, &dots.lanes.ends)).map(|a| a.key));
    }
    if repair == RouteRepair::Crossings {
        keys.extend(drawn.iter().filter(|a| crosses_branches_off_row(a, dots)).map(|a| a.key));
    }
    if repair == RouteRepair::Heads {
        keys.extend(arrow_mark_hits(&drawn, &dots.lanes.ends).into_iter()
            .filter(|&(_, _, kind)| kind == ArrowMarkHit::Head)
            .flat_map(|(i, j, _)| [drawn[i].key, drawn[j].key]));
    }
    if keys.is_empty() {
        return;
    }
    let mut keys: Vec<_> = keys.into_iter().collect();
    if repair == RouteRepair::Crossings {
        // Overlapping tracks cannot both take the event row. Start with
        // the wider arrow, where the detour hides more of the crossing.
        keys.sort_by_key(|key| {
            let ((tx, _), (fx, _), _) = dots.lanes.ends[key];
            std::cmp::Reverse((tx - fx).abs())
        });
    }
    let dot_ys = dots.positions.iter().map(|&(_, y)| y);
    let ys = (dot_ys.clone().min().unwrap_or(0), dot_ys.chain(drawn.iter().flat_map(|d| d.pieces.iter())
        .flat_map(|&(p, q)| [p.1, q.1]).map(|y| y.round() as i64)).max().unwrap_or(0));
    let hi_x = dots.positions.iter().map(|&(x, _)| x as f64)
        .chain(drawn.iter().flat_map(|d| d.pieces.iter().flat_map(|&(p, q)| [p.0, q.0])))
        .fold(f64::NEG_INFINITY, f64::max);
    // A later head repair can adjust a bend or move its neighbor
    // while keeping the crossings already placed by the dot repair.
    let arc_crossings = |drawing: &[Drawn]| {
        drawing.iter().flat_map(|a| {
            a.arcs.iter().map(move |arc| (a.key, arc.center.0 as i64, arc.center.1 as i64))
        }).collect::<BTreeSet<_>>()
    };
    let crossings = if repair == RouteRepair::Heads { arc_crossings(&drawn) } else { BTreeSet::new() };
    let allowed = |d: &[Drawn], ends: &Ends, loops: &BTreeSet<ArrowKey>| {
        d.iter().flat_map(|d| d.pieces.iter().flat_map(|&(p, q)| [p.0, q.0])).all(|x| x >= 10.0 && x <= hi_x + 45.0)
            && (crossings.is_empty() || crossings.is_subset(&arc_crossings(d)))
            && d.iter().all(|a| !runs_past_ends(a, ends) || loops.contains(&a.key))
    };
    let redraw = |dots: &mut DotLayer, settings: LaneSettings| {
        dots.lanes.set(settings);
        let drawn = probe_arrows(visualization_data, resource_owners_layout, dots, registry);
        let hits = route_hits(&drawn, dots, clearance);
        (drawn, hits)
    };
    let mut hits = route_hits(&drawn, dots, clearance);
    // One search proposes routes without track hops. Before accepting
    // any of its proposals, check the arcs the final drawing will paint.
    let painted_arc_hits = |dots: &mut DotLayer| {
        dots.lanes.probe_hops = true;
        let actual = probe_arrows(visualization_data, resource_owners_layout, dots, registry);
        dots.lanes.probe_hops = false;
        arc_head_route_hits(&actual).collect::<BTreeSet<_>>()
    };
    let mut actual_arc_hits = (!dots.lanes.probe_hops).then(|| painted_arc_hits(dots));
    let mut arcs_are_safe = |dots: &mut DotLayer| {
        let Some(before) = actual_arc_hits.as_mut() else { return true };
        let after = painted_arc_hits(dots);
        if !after.is_subset(before) {
            return false;
        }
        *before = after;
        true
    };
    for key in keys {
        let loops: BTreeSet<_> = drawn.iter().filter(|a| runs_past_ends(a, &dots.lanes.ends)).map(|a| a.key).collect();
        let obstructed = |hit: &RouteHit| {
            matches!(hit, RouteHit::Leg(k, _) if *k == key)
                || (repair == RouteRepair::Dots && matches!(hit, RouteHit::Dot(k, _) if *k == key))
                || (repair == RouteRepair::Heads
                    && matches!(hit, RouteHit::Arrow((a, _), (b, _), ArrowMarkHit::Head) if *a == key || *b == key))
        };
        let blocked = hits.iter().any(obstructed);
        let crossing = repair == RouteRepair::Crossings;
        if repair != RouteRepair::Dots && !crossing && !blocked {
            continue;
        }
        let now = dots.lanes.settings();
        let shape = |d: &[Drawn], ends: &Ends| d.iter().find(|a| a.key == key && a.logo.is_none())
            .map_or((false, 0.0), |a| (runs_past_ends(a, ends), high_track_length(a, ends)));
        let original_shape = shape(&drawn, &dots.lanes.ends);
        let improves_route = |d: &[Drawn], dots: &DotLayer| {
            if crossing {
                d.iter().any(|a| a.key == key && !a.arcs.is_empty() && !crosses_branches_off_row(a, dots))
            } else {
                blocked || shape(d, &dots.lanes.ends) < original_shape
            }
        };
        let mut pairs = Vec::new();
        let mut routes = Vec::new();
        if crossing {
            routes.extend(dots.lanes.local_routes(key).into_iter().map(|mut s| {
                s.branch_hops.insert(key);
                s
            }));
        } else if painted {
            routes.extend(dots.lanes.local_routes(key));
            if let Some(&y) = now.lane_y.get(&key).filter(|_| blocked) {
                // A close pass can need only a pixel or two of extra
                // space. Keep the same bends and move the track first.
                for offset in [-1, 1, -2, 2, -3, 3] {
                    if (ys.0..=ys.1).contains(&(y + offset)) {
                        let mut s = now.clone();
                        s.lane_y.insert(key, y + offset);
                        routes.push(s);
                    }
                }
            }
            for s in dots.lanes.alternatives(key, &drawn, ys, false, false).into_iter().filter(|s| !s.far.contains(&key)) {
                routes.push(s.clone());
                routes.extend(s.with_local_turns(key));
            }
            if blocked {
                routes.extend(now.with_local_turns(key));
            }
        }
        if blocked && !crossing {
            routes.extend(dots.lanes.alternatives(key, &drawn, ys, true, true));
        }
        if !painted {
            routes.extend(now.with_local_turns(key));
        }
        let mut accepted = false;
        for s in routes {
            let (d, candidate) = redraw(dots, s.clone());
            if candidate.iter().any(obstructed) || !allowed(&d, &dots.lanes.ends, &loops)
                || !improves_route(&d, dots) {
                continue;
            }
            if candidate.is_subset(&hits) && arcs_are_safe(dots) {
                hits = candidate;
                drawn = d;
                accepted = true;
                break;
            }
            let can_move = |other: &ArrowKey| {
                if now.branch_hops.contains(other) {
                    return false;
                }
                if crossing {
                    // A return dips off the row when its borrow traces
                    // the same span in reverse (#710). Keep that order.
                    let (target, source, _) = dots.lanes.ends[&key];
                    if dots.lanes.ends.get(other).is_some_and(|&(t, s, _)| t == source && s == target) {
                        return false;
                    }
                }
                true
            };
            if let Some(other) = single_route_blocker(&hits, &candidate, key).filter(can_move) {
                let arc_blocker = candidate.difference(&hits).any(|h| matches!(h, RouteHit::ArcHead(_, _, _)));
                pairs.push((s, d, other, arc_blocker));
            }
        }
        if accepted {
            continue;
        }
        // A borrow and its return can share both dots. Try moving the
        // blocking arrow too when it is the only mark in the way.
        let mut remaining = 400;
        'pair: for (s, d, other, arc_blocker) in pairs {
            dots.lanes.set(s);
            let mut alternatives = dots.lanes.repair_alternatives(other, &d, ys, true, true);
            if arc_blocker {
                // The half-row tracks can already be occupied. The
                // blocker may need an intermediate track to avoid
                // hopping through the other arrow's new head.
                alternatives.extend(dots.lanes.alternatives(other, &d, ys, false, true));
            }
            for s in alternatives {
                if remaining == 0 {
                    break 'pair;
                }
                remaining -= 1;
                let (d, candidate) = redraw(dots, s);
                if !candidate.iter().any(obstructed) && candidate.is_subset(&hits)
                    && allowed(&d, &dots.lanes.ends, &loops) && improves_route(&d, dots) && arcs_are_safe(dots) {
                    hits = candidate;
                    drawn = d;
                    accepted = true;
                    break 'pair;
                }
            }
        }
        if !accepted {
            dots.lanes.set(now);
        }
    }
}

type ArrowMark = (ArrowKey, bool);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RouteHit {
    Arrow(ArrowMark, ArrowMark, ArrowMarkHit),
    ArcHead(ArrowKey, ArrowKey, (i64, i64)),
    EndToEnd(ArrowKey, ArrowKey),
    Crossing(ArrowKey, ArrowKey, usize),
    Dot(ArrowKey, usize),
    StopBar(ArrowKey, usize),
    Leg(ArrowKey, usize),
    Column(ArrowKey, usize),
    Bracket(ArrowKey, usize),
}

fn single_route_blocker(before: &BTreeSet<RouteHit>, after: &BTreeSet<RouteHit>, key: ArrowKey) -> Option<ArrowKey> {
    let mut partners = after.difference(before).map(|hit| {
        let (a, b) = match *hit {
            RouteHit::Arrow((a, _), (b, _), _) | RouteHit::ArcHead(a, b, _)
            | RouteHit::Crossing(a, b, _) | RouteHit::EndToEnd(a, b) => (a, b),
            _ => return None,
        };
        if a == key { Some(b) } else if b == key { Some(a) } else { None }
    });
    let other = partners.next()??;
    (other != key && partners.all(|p| p == Some(other))).then_some(other)
}

fn arc_head_route_hits(drawn: &[Drawn]) -> impl Iterator<Item = RouteHit> + '_ {
    head_arc_hits(drawn).map(move |(i, j, arc)| {
        RouteHit::ArcHead(drawn[i].key, drawn[j].key, (arc.center.0 as i64, arc.center.1 as i64))
    })
}

/// Each obstruction keeps the identities of the marks that meet. Clearing
/// one of them cannot pay for introducing another, even of the same kind.
fn route_hits(drawn: &[Drawn], dots: &DotLayer, clearance: MarkClearance) -> BTreeSet<RouteHit> {
    let mark = |i: usize| (drawn[i].key, drawn[i].logo.is_some());
    let mut hits = BTreeSet::new();
    hits.extend(arrow_mark_hits(drawn, &dots.lanes.ends).into_iter()
        .map(|(i, j, kind)| RouteHit::Arrow(mark(i), mark(j), kind)));
    hits.extend(arc_head_route_hits(drawn));
    hits.extend(end_to_end(drawn, &dots.lanes.ends).into_iter()
        .map(|(i, j)| RouteHit::EndToEnd(drawn[i].key, drawn[j].key)));
    hits.extend(dot_and_bar_hits(drawn, dots, clearance).into_iter()
        .map(|(i, bar, m)| if bar { RouteHit::StopBar(drawn[i].key, m) } else { RouteHit::Dot(drawn[i].key, m) }));
    hits.extend(leg_hits(drawn, &dots.legs).into_iter().map(|(i, m)| RouteHit::Leg(drawn[i].key, m)));
    hits.extend(leg_hits(drawn, &dots.cols).into_iter().map(|(i, m)| RouteHit::Column(drawn[i].key, m)));
    for (i, a) in drawn.iter().enumerate().filter(|(_, a)| a.logo.is_none()) {
        for b in drawn[i + 1..].iter().filter(|b| b.logo.is_none()) {
            let (lo, hi) = (a.key.min(b.key), a.key.max(b.key));
            hits.extend((0..shaft_crossings(a, b)).map(|n| RouteHit::Crossing(lo, hi, n)));
        }
        hits.extend(dots.brackets.iter().enumerate()
            .filter(|&(_, b)| on_bracket(&a.pieces, &a.tri, std::slice::from_ref(b)))
            .map(|(m, _)| RouteHit::Bracket(a.key, m)));
    }
    hits
}

fn visible_arrow_defects(drawn: &[Drawn], dots: &DotLayer) -> usize {
    marks_on_others(drawn, &dots.lanes.ends).len()
        + through_marks(drawn, dots, MarkClearance::Shaft).len()
        + end_to_end(drawn, &dots.lanes.ends).len()
        + on_cols(drawn, &dots.cols).len()
}

fn determine_owner_line_styles(
    rap: &ResourceAccessPoint,
    state: &State
) -> OwnerLine {
    // Hollow tracks "read-only" — either because the binding is
    // immutable (`let x`, can't write) or because an immutable
    // borrow is currently alive on the owner (PartialPrivilege).
    // Solid is reserved for the mutable-binding-no-borrow case
    // where the owner can both read and write right now. The loan
    // itself is communicated by the borrow-region trapezoid drawn
    // on the borrower's column, not by varying the lender's
    // stroke style.
    let state = match state {
        State::ManuallyTaken { wrapper } => wrapper.as_ref(),
        state => state,
    };
    match (state, rap.is_mut()) {
        (State::FullPrivilege{..}, true) => OwnerLine::Solid,
        (State::FullPrivilege{..}, false) => OwnerLine::Hollow,
        (State::PartialPrivilege{..}, _) => OwnerLine::Hollow,
        // Mutably lent out (#722): a faint dotted line, so the events
        // during and at the end of the lend sit on the column.
        (State::RevokedPrivilege{..}, _) => OwnerLine::Dotted,
        _ => OwnerLine::Empty,
    }
}

/// A column's faint dotted segment while it is mutably lent out
/// (RevokedPrivilege, #722): it can't be used, but it's still there,
/// and the events of the lend (another `&mut` taken through it, a
/// sub-borrow ending, the borrow's end) sit on it. One thin line of
/// round dots (`.lent`) under a transparent strip as wide as a hollow
/// column, which takes the hover: the dots' gaps alone would be hard
/// to hit. Hovering it underlines the rows from the segment's first up
/// to the one it ends on, as a solid line's does (`data-underline-y`
/// names rows; helpers.js).
fn lent_line_string(data: &VerticalLineData) -> String {
    let w = BRANCH_HALF_W;
    format!(
        "<g class=\"tooltip-trigger\" data-tooltip-text=\"{title}\"{rv} data-underline-y=\"{y1} {last}\">\n            <line data-hash=\"{hash}\" class=\"lent\" x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\"/>\n            <polygon data-hash=\"{hash}\" points=\"{l1},{y1} {r1},{y1} {r2},{y2} {l2},{y2}\" style=\"fill:transparent; stroke:none; pointer-events:fill;\"/>\n        </g>\n",
        title = data.title, hash = data.hash, rv = data.rv,
        x1 = data.x1, y1 = data.y1, x2 = data.x2, y2 = data.y2, last = (data.y2 - 3).max(data.y1),
        l1 = data.x1 - w, r1 = data.x1 + w, l2 = data.x2 - w, r2 = data.x2 + w,
    )
}

fn compute_hollow_line_data(v_data: VerticalLineData, w: f64) -> HollowLineData{
    // Direction vector components
    let x1 = v_data.x1 as f64;
    let x2 = v_data.x2 as f64;
    let y1 = v_data.y1 as f64;
    let y2 = v_data.y2 as f64;
    let dx = x1 - x2;
    let dy = y1 - y2;

    // Length of the direction vector. Zero-length inputs (a state
    // segment whose top and bottom landed on the same line) would
    // produce NaN coordinates downstream when we divide by it; the
    // rendering filters such segments out today, but a defensive
    // fallback to a vertical orientation keeps the output finite
    // for any future caller that hasn't done that filtering.
    let d_length = (dx.powi(2) + dy.powi(2)).sqrt();
    let (ux, uy) = if d_length < 1e-9 {
        (0.0_f64, 1.0_f64)
    } else {
        (dx / d_length, dy / d_length)
    };

    let p_x = -uy * (-w / 2.0);
    let p_y =  ux * (-w / 2.0);

    // create new x and y coordinates
    let x1 = x1 + p_x;
    let x2 = x2 + p_x;
    let y1 = y1 + p_y;
    let y2 = y2 + p_y;
    
    // Perpendicular vector components, normalized and scaled by the width
    let px = -uy * w;
    let py =  ux * w;

    // Compute the remaining points
    let x3 = x1 + px;
    let y3 = y1 + py;
    let x4 = x2 + px;
    let y4 = y2 + py;

    HollowLineData {
        line_class: v_data.line_class,
        hash: v_data.hash,
        x1: x1, x2: x2, x3: x4, x4: x3,
        y1: y1, y2: y2, y3: y4, y4: y3,
        title: v_data.title,
        opacity: v_data.opacity,
        dasharray: v_data.dasharray,
        rv: v_data.rv,
    }
}

// generate a Owner Line string from the RAP and its State
fn create_owner_line_string(
    rap: &ResourceAccessPoint,
    state: &State,
    data: &mut VerticalLineData,
    registry: &Handlebars,
) -> String {
    // TODO: prevent creating line when function dot already exists
    let style = determine_owner_line_styles(rap, state);

    // Gray-state segments are conditional-branch portions where
    // *this* branch isn't actively doing anything on the line. Dash
    // them so readers can scan vertically and see at-a-glance which
    // arm is producing the events at each row. Faded opacity alone
    // wasn't visually distinct enough — both arms read as solid
    // columns to first glance.
    // Gray-state segments: render at full opacity so they read as
    // the same line as the active solid segments, just textured
    // differently. Pre-fix the dashed legs sat at 0.5 opacity which
    // made the dashed-then-solid transitions look like two
    // independent lines instead of one continuous timeline. The
    // dash pattern is the only inactive cue now.
    match state {
        State::FullPrivilege { s: LineState::Gray | LineState::Uninit } | State::PartialPrivilege { s: LineState::Gray } => {
            data.dasharray = "4 4".to_owned();
        }
        _ => {}
    }
    match style {
        OwnerLine::Solid => {
            data.line_class = String::from("solid");
            // (The historical "binding can be reassigned" suffix
            // mixed a binding property into a state message; the
            // state text alone is enough.)
            registry.render("vertical_line_template", &data).unwrap()
        },
        OwnerLine::Hollow => {
            let hollow_line_data = data.clone();
            let new_hollow_line_data = compute_hollow_line_data(hollow_line_data, 3.5);
            registry.render("new_hollow_line_template", &new_hollow_line_data).unwrap()
        },
        OwnerLine::Dotted => lent_line_string(data),
        OwnerLine::Empty => String::new(),
    }
}

// generate Reference Line(s) string from the RAP and its State
fn create_reference_line_string(
    rap: &ResourceAccessPoint,
    state: &State,
    data: &mut VerticalLineData,
    registry: &Handlebars,
) -> String {
    // Gray-state segments: render at full opacity so they read as
    // the same line as the active solid segments, just textured
    // differently. Pre-fix the dashed legs sat at 0.5 opacity which
    // made the dashed-then-solid transitions look like two
    // independent lines instead of one continuous timeline. The
    // dash pattern is the only inactive cue now.
    match state {
        State::FullPrivilege { s: LineState::Gray | LineState::Uninit } | State::PartialPrivilege { s: LineState::Gray } => {
            data.dasharray = "4 4".to_owned();
        }
        _ => {}
    }
    match (state, rap.is_mut()) {
        (State::FullPrivilege{..}, true) => {
            data.line_class = String::from("solid");
            registry.render("vertical_line_template", &data).unwrap()
        },
        (State::FullPrivilege{..}, false) => {
            let hollow_line_data = data.clone();
            registry.render("new_hollow_line_template", &compute_hollow_line_data(hollow_line_data, 3.5)).unwrap()
        },
        (State::PartialPrivilege{ .. }, _muta) => {
            data.line_class = String::from("solid");
            let hollow_line_data = data.clone();
            registry.render("new_hollow_line_template", &compute_hollow_line_data(hollow_line_data, 3.5)).unwrap()
        },
        (State::ResourceMoved{ .. }, true) => {
            data.line_class = String::from("extend");
            registry.render("vertical_line_template", &data).unwrap()
        }
        // Mutably reborrowed (`let r2 = &mut *r;`) or, for a `&T`
        // binding, itself borrowed `&mut` (#722).
        (State::RevokedPrivilege{ .. }, _) => lent_line_string(data),
        // do nothing when the case is (OutofScope, _), (ResourceMoved, false)
        _ => "".to_owned(),
    }
}

/// Whether anything was drawn (an out-of-scope or moved-out stretch
/// draws nothing).
fn append_line(
    state: &State,
    data: & mut VerticalLineData,
    rap: &ResourceAccessPoint,
    timeline_data: &TimelineColumnData,
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    registry: &Handlebars
) -> bool {
    let line = match rap {
        ResourceAccessPoint::Function(_) => return false, // Don't do anything
        ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_) => create_owner_line_string(rap, state, data, registry),
        ResourceAccessPoint::StaticRef(_) | ResourceAccessPoint::MutRef(_) => create_reference_line_string(rap, state, data, registry),
    };
    match rap {
        ResourceAccessPoint::Function(_) => {},
        ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_) => {
            if timeline_data.is_struct_group { //TODO: not sure if this is correct
                if !output.contains_key(&(timeline_data.owner.to_owned() as i64)) {
                    output.insert(timeline_data.owner.to_owned() as i64, (TimelinePanelData{ labels: String::new(), dots: String::new(), timelines: String::new(), 
                        ref_line: String::new(), arrows: String::new() }, TimelinePanelData{ labels: String::new(), dots: String::new(), 
                            timelines: String::new(), ref_line: String::new(), arrows: String::new() })); 
                }
                if timeline_data.is_member {
                    output.get_mut(&(timeline_data.owner.to_owned() as i64)).unwrap().1.timelines.push_str(&line);
                } else {
                    output.get_mut(&(timeline_data.owner.to_owned() as i64)).unwrap().0.timelines.push_str(&line);
                }
            }
            else {
                output.get_mut(&-1).unwrap().0.timelines.push_str(&line);
            }
        },
        ResourceAccessPoint::StaticRef(_) | ResourceAccessPoint::MutRef(_) => {
            // Ref RAPs that participate in a struct group (member_of
            // is set) get routed under the parent's output entry,
            // same as Struct members above. Otherwise the struct
            // bounding box doesn't pick them up and the ref-line /
            // timeline lands outside the box.
            if timeline_data.is_struct_group && timeline_data.is_member {
                let owner = timeline_data.owner.to_owned() as i64;
                if !output.contains_key(&owner) {
                    output.insert(owner, (
                        TimelinePanelData{ labels: String::new(), dots: String::new(), timelines: String::new(), ref_line: String::new(), arrows: String::new() },
                        TimelinePanelData{ labels: String::new(), dots: String::new(), timelines: String::new(), ref_line: String::new(), arrows: String::new() },
                    ));
                }
                output.get_mut(&owner).unwrap().1.timelines.push_str(&line);
            } else {
                output.get_mut(&-1).unwrap().0.timelines.push_str(&line);
            }
        },
    }
    !line.is_empty()
}

// ── Branch column rendering ────────────────────────────────────────
//
// A branch event renders as one strip per recorded arm. Each arm's
// strip is a sequence of segments:
//
//   leading converge:  (parent split-dot) → (top of branch column)
//   body:              one per `branch.states` segment
//   trailing converge: (bottom of branch column) → (parent merge-dot)
//
// Every segment carries the State that drives its rendering. The
// per-segment style decision goes through `determine_owner_line_styles`
// — the same classifier the regular column uses — so the branch
// strip honors mut-vs-immut and PartialPrivilege identically to a
// non-branch column. Gray states (the convention for "this row
// belongs to a different arm") get a dashed stroke regardless of
// solid/hollow.
//
// Structural invariants:
//
//   * The leading converge is dropped when the parent's pre-branch
//     state classifies as Empty (variable not alive entering the
//     branch). Synthesized to FullPrivilege::Full for let-as-rhs,
//     where the parent state is OOS but the branch is about to
//     bring the variable into scope.
//   * The trailing converge is dropped when the branch's last
//     state classifies as Empty. A Move terminates the column at
//     the move event; nothing diagonal trails toward the merge.
//   * Body segments with Empty classification (OOS / Moved) drop
//     out, so a Move mid-body ends the visible column right there.
//   * Segments are grouped into centerline-contiguous runs before
//     rendering. A run is a maximal subsequence whose adjacent
//     segments meet at the same centerline point. When a parent
//     branch's body span is taken over by a nested Branch event
//     (leading ends at line N, trailing starts many rows later,
//     no body in between), each becomes its own run with its own
//     hover polygons — the gap stays empty rather than getting
//     bridged.

/// Whether the arm draws a solid, hollow or dotted segment for the column.
fn seg_drawn(rap: &ResourceAccessPoint, state: &State) -> bool {
    !matches!(determine_owner_line_styles(rap, state), OwnerLine::Empty)
}

/// Half-width of a hollow strip — matches `compute_hollow_line_data`'s
/// `w = 3.5 / 2`, so branch strips share endpoint coordinates with
/// regular columns at the seams.
const BRANCH_HALF_W: f64 = 1.75;

/// One centerline segment of a branch's strip. The state drives
/// rendering style via `determine_owner_line_styles`.
#[derive(Clone)]
struct BranchSeg {
    p1: (f64, f64),
    p2: (f64, f64),
    state: State,
    title: String,
    /// The arm's label when this is a dashed (`Gray`) vertical: its
    /// `title` names the underlined code, so it's reworded if hovering
    /// it underlines nothing (see `reword_unlit_inactive`).
    inactive_arm: Option<String>,
    /// The solid stretch around a one-row arm's own row (#845); hovering
    /// it underlines that row.
    own_row: bool,
    /// The first and last row the segment stands for, for its
    /// `data-rv-*` attributes (a diagonal: the split's or join's row).
    rows: (usize, usize),
}

/// Which part of an arm's strip a segment is.
#[derive(Clone, Copy, PartialEq)]
enum Leg {
    /// The diagonal from the parent's column into the arm's.
    Split,
    /// A vertical stretch of the arm's own column.
    Column,
    /// The diagonal from the arm's column to where the branches join.
    Join,
}

/// A column never draws two segments over the same rows: a dashed
/// vertical segment gives way to a solid one on the rows they share.
/// A dashed state can reach down over rows where the arm's own first
/// event already starts it (an `if let` binding's dot sits above the
/// arm's body rows, so the leading dashes cover rows the arm owns),
/// and drawing both made two hover regions with different tooltips on
/// the same rows (#626).
fn trim_dashes_under_solid(segs: Vec<BranchSeg>) -> Vec<BranchSeg> {
    let vertical = |s: &BranchSeg| (s.p1.0 - s.p2.0).abs() < 1e-6 && (s.p1.1 - s.p2.1).abs() > 1e-6;
    // (x, top, bottom) of each solid vertical segment.
    let solid: Vec<(f64, f64, f64)> = segs.iter()
        .filter(|s| vertical(s) && !state_is_dashed(&s.state))
        .map(|s| (s.p1.0, s.p1.1.min(s.p2.1), s.p1.1.max(s.p2.1)))
        .collect();
    let mut out = Vec::with_capacity(segs.len());
    for seg in segs {
        if !vertical(&seg) || !state_is_dashed(&seg.state) { out.push(seg); continue; }
        let down = seg.p1.1 < seg.p2.1;
        let (top, bot) = (seg.p1.1.min(seg.p2.1), seg.p1.1.max(seg.p2.1));
        // Pieces of [top, bot] not covered by a solid segment.
        let mut pieces = vec![(top, bot)];
        for &(x, st, sb) in &solid {
            if (x - seg.p1.0).abs() > 1e-6 { continue; }
            pieces = pieces.into_iter().flat_map(|(a, b)| {
                if sb <= a || st >= b { return vec![(a, b)]; }
                let mut v = Vec::new();
                if st > a { v.push((a, st)); }
                if sb < b { v.push((sb, b)); }
                v
            }).collect();
        }
        if !down { pieces.reverse(); }
        for (a, b) in pieces {
            let (p1y, p2y) = if down { (a, b) } else { (b, a) };
            out.push(BranchSeg { p1: (seg.p1.0, p1y), p2: (seg.p2.0, p2y), state: seg.state.clone(), title: seg.title.clone(), inactive_arm: seg.inactive_arm.clone(), own_row: seg.own_row, rows: seg.rows });
        }
    }
    out
}

/// Unit-length perpendicular to `p1 → p2`. Used to offset the two
/// sides of a hollow strip by `±BRANCH_HALF_W` perpendicular to the
/// segment direction so the visual gap between sides is constant
/// regardless of angle (a vertical body and a 45° diagonal both
/// read at the same width).
fn perp_unit(p1: (f64, f64), p2: (f64, f64)) -> (f64, f64) {
    let dx = p2.0 - p1.0;
    let dy = p2.1 - p1.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 { return (1.0, 0.0); }
    (-dy / len, dx / len)
}

/// Render every Branch event reachable through `history`. Recurses
/// into nested branches.
///
/// `maybe_init`: the column is a `let s;` local that only some branches
/// of an earlier conditional assigned (#244) when `history` starts.
fn render_branches(
    hash: &u64,
    rap: &ResourceAccessPoint,
    history: &Vec<(usize, Event)>,
    parent_states: &Vec<(usize, usize, State)>,
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    timeline_data: &TimelineColumnData,
    maybe_init: bool,
    path: &[(usize, usize)],
    dots: &DotLayer,
    bars: &mut ColumnMarks,
) {
    if rap.is_fn() { return; }
    for (k, (_, ev)) in history.iter().enumerate() {
        let Event::Branch { branch_history, split_point, merge_point, ty, fork_above, id, .. } = ev else { continue };
        // Maybe-initialized as the conditional starts (#244): where an
        // arm hasn't assigned s yet, s may or may not hold a value.
        let entry_init = init_after(maybe_init.then_some(InitJoin::Maybe { moved: false }), &history[..k]);
        let entry_maybe_init = matches!(entry_init, Some(InitJoin::Maybe { .. } | InitJoin::Loop));

        let parent_x = timeline_data.x_val as f64;
        // Branches that share lines (`let s = if c { a } else { b };`)
        // have no rows between the split and the join. Their columns
        // then run on the split..join rows, and the split and join
        // move a row out (onto blank rows `render_svg` adds), so the
        // arms form a diamond around those rows instead of crossing
        // into an X (#117).
        let (split_y, merge_y, column_top_y, column_bot_y) =
            if branch_is_collapsed(*split_point, *merge_point) {
                (
                    (get_y_axis_pos(*split_point) - COLLAPSED_APEX) as f64,
                    (get_y_axis_pos(*merge_point) + COLLAPSED_APEX) as f64,
                    get_y_axis_pos(*split_point) as f64,
                    get_y_axis_pos(*merge_point) as f64,
                )
            } else {
                (
                    get_y_axis_pos(*split_point) as f64,
                    get_y_axis_pos(*merge_point) as f64,
                    get_y_axis_pos(*split_point + 1) as f64,
                    get_y_axis_pos(merge_point.saturating_sub(1)) as f64,
                )
            };
        // An `if let` whose pattern binds from this column (#382): the
        // arms fork from half a row above the split row and start on
        // it, the `if` arm with the binding's dot.
        let raised_rise = CollapsedApexes::of(history).raised_splits.get(split_point).copied();
        let raised = raised_rise.is_some();
        let (split_y, column_top_y) = if let Some(rise) = raised_rise {
            ((get_y_axis_pos(*split_point) - rise) as f64, get_y_axis_pos(*split_point) as f64)
        } else {
            (split_y, column_top_y)
        };
        // An `else if` (#1294): this join may sit above the merge row,
        // and an arm's leg starts at the join of the one nested in it.
        let merge_y = merge_y - join_rise(visualization_data, hash, history, *split_point, *merge_point) as f64;
        // The first arm starts with a conditional on the split line whose
        // fork is raised above it (a one-line one, #1287, or one binding
        // from this column, #1306): its leg ends at that fork.
        let first_arm_fork_y = arm_first_fork_rise(branch_history, *split_point)
            .filter(|_| raised)
            .map(|rise| (get_y_axis_pos(*split_point) - rise) as f64);

        // Parent's state at the split. The active arm uses this
        // directly; passive arms Gray-convert it so their leading
        // reads as inactive. A variable with no value yet (let-as-rhs,
        // or `let s;` assigned inside the branches, #234) gets the
        // dashed "has no value yet" leading on every arm: it owns
        // nothing until the branch's acquire.
        // That's the state the arms were computed from: looking it up
        // in `parent_states` by line found the state *before* an event
        // on the split line, so an arm's binding on the line its inner
        // `match` splits on (`Some(x) => match b {`) read as "x has no
        // value yet" on every inner arm (#502).
        let parent_pre = branch_history.first()
            .and_then(|b| b.entry_state.clone())
            .unwrap_or_else(|| parent_states.iter()
                .find(|(s, e, _)| *s <= *split_point && *split_point <= *e)
                .map(|item| item.2.clone())
                .unwrap_or(State::OutOfScope));
        let parent_active = branch_entry_state(&parent_pre);

        // Each arm's source span: which of the facts' arms on a one-line
        // conditional's row are this conditional's (matched by source arm).
        let arm_sources: Vec<_> = branch_history.iter().map(|b| b.source).collect();
        for (i, branch) in branch_history.iter().enumerate() {
            let branch_x = branch.t_data.x_val as f64;
            // The arms this one is nested in, and this one, by their
            // conditional's split line (`lent_out_title`).
            let arm_path = [path, &[(*split_point, i)]].concat();
            // Active arm: i == 0 with non-empty body. Other arms,
            // and any empty body, render passive (Gray-converted
            // leading so it's dashed).
            let is_active = i == 0 && !branch.e_data.is_empty();
            let arm_label = ty.string_of_branch(i);
            // Each leg can have a different entry state when the discriminant
            // proves that a variant cannot hold its sibling's reference.
            let parent_active = branch.entry_state.as_ref().map(branch_entry_state).unwrap_or_else(|| parent_active.clone());
            let leading_state = arm_entry_state(&parent_active, is_active);
            let explain_drop_loan = |seg: &mut BranchSeg, joining| {
                if !rap.is_ref() && matches!(seg.state, State::PartialPrivilege { s: LineState::Full }
                    | State::FullPrivilege { s: LineState::Full })
                {
                    if let Some((&function, arm)) = visualization_data.fn_source_lines.get(rap.hash()).zip(branch.source) {
                        let table = &visualization_data.state_table;
                        if let Some((cell, holder)) = table.arm_boundary_cell(function, rap.name(), arm, joining)
                            .and_then(|cell| Some((cell, table.shared_drop_holder(function, rap.name(), cell)?)))
                        {
                            if holder.some_paths || matches!(seg.state, State::FullPrivilege { .. }) {
                                let (state, title, _) = owner_loan_title(rap, visualization_data, function, cell, &seg.state, seg.rows.0, false);
                                seg.state = state;
                                seg.title = title;
                            }
                            append_drop_holder(&mut seg.title, rap.name(), holder);
                        }
                    }
                }
            };
            // A passive arm's column is drawn dashed beside the other
            // arms' rows unless the state has no inactive look (a lent
            // owner's dotted line, #1333).
            let passive_dashed = state_is_dashed(&branch_state_converter(&parent_active));
            let end_state = arm_end_state(rap, branch, branch_history, ty);

            // A diverging arm stops on its exit row instead of joining.
            // For separate arm rows, a matched MIR exit can land on the
            // first arm's own row, where the raised fork's diagonal ends.
            let own = ty.get_start_end(i);
            let exit_at_split = branch.diverges && *fork_above && own.1 == *split_point
                && visualization_data.exits_at_split.contains(&(*id, i));
            let stop_line = (branch.diverges && branch.imaginary.is_none()).then(|| {
                let (lo, hi) = branch_column_rows(*split_point, *merge_point);
                let lo = if exit_at_split { *split_point } else { lo };
                own.1.clamp(lo, hi.max(lo))
            });

            // Build segments. Empty-classified states drop out so
            // a Move (anywhere) ends the visible run there.
            let mut segs: Vec<BranchSeg> = Vec::new();
            // The state and title of a one-line arm's column at the bottom
            // of its own rows, from its own cell (design Q1): its diagonal
            // into the join goes on in it.
            let mut arm_bottom: Option<(usize, State, Option<String>)> = None;
            let apexes = CollapsedApexes::of(&branch.e_data);
            // The first arm starts with a one-line conditional on the
            // split line (#1287): its leg ends at that conditional's fork.
            // Otherwise the arm's own first row may start a conditional
            // whose fork is raised above it (`else if d { g } else { h }`
            // on one line with its arms on rows of their own, #1264): the
            // leg into the arm ends at that fork's dot. Run on to the
            // row, it ended in mid-air below the fork.
            let inner_rise = (!raised && !branch_is_collapsed(*split_point, *merge_point))
                .then(|| apexes.raised_splits.get(&(*split_point + 1)).copied())
                .flatten();
            let leg_end_y = match (first_arm_fork_y.filter(|_| i == 0), inner_rise) {
                (Some(y), _) => y,
                (None, Some(rise)) => column_top_y - rise as f64,
                (None, None) => column_top_y,
            };
            let (own_first, own_last) = ty.get_start_end(i);
            if let Some(edge) = branch.imaginary {
                if let Some((state, title)) = imaginary_leg(rap, visualization_data, edge, *split_point) {
                    let top = (get_y_axis_pos(own_first) as f64).max(leg_end_y);
                    let bot = get_y_axis_pos(own_last) as f64
                        + if own_first == own_last { LINE_SPACE as f64 / 2.0 } else { 0.0 };
                    let mut add = |p1, p2, state, rows, title| segs.push(BranchSeg {
                        p1, p2, state, rows, title, inactive_arm: None, own_row: false,
                    });
                    let passive = branch_state_converter(&state);
                    add((parent_x, split_y), (branch_x, leg_end_y), passive.clone(), (*split_point, *split_point), title.clone());
                    if top > leg_end_y {
                        add((branch_x, leg_end_y), (branch_x, top), passive, (*split_point, own_first), title.clone());
                    }
                    let mut start = (own_first, top);
                    let mut note = None;
                    for row in own_first..=own_last {
                        let y = (get_y_axis_pos(row) as f64).max(top);
                        if y >= bot { break; }
                        let Some(src) = rv_attrs::line_of(row, &visualization_data.row_lines) else { continue };
                        let next = imaginary_move_note(rap, visualization_data, edge, src);
                        if next == note { continue; }
                        if start.1 < y {
                            add((branch_x, start.1), (branch_x, y), state.clone(), (start.0, row),
                                title.clone() + note.as_deref().unwrap_or(""));
                        }
                        start = (row, y);
                        note = next;
                    }
                    if start.1 < bot {
                        add((branch_x, start.1), (branch_x, bot), state, (start.0, own_last),
                            title + note.as_deref().unwrap_or(""));
                    }
                }
            } else {
                let split_segment = segs.len();
                push_seg_if_renderable(
                    &mut segs, rap,
                    (parent_x, split_y), (branch_x, leg_end_y),
                    leading_state.clone(), visualization_data, &arm_label, Leg::Split, *split_point, *split_point, &arm_path,
                );
                if let Some(seg) = segs.get_mut(split_segment) {
                    explain_drop_loan(seg, false);
                }
                if is_active && branch.diverges && !branch.e_data.is_empty() && branch.e_data.iter().all(|(_, e)| caller_end(e)) {
                    if let Some(seg) = segs.get_mut(split_segment) {
                        seg.title.push_str(". ");
                        seg.title.push_str(&hover_messages::state_arm_split(&arm_label));
                    }
                }
            }
            // An arm that starts below the raised fork's row (the `else`)
            // runs down to its first row in the state it forked with. When
            // that row starts a conditional of the arm's own whose fork is
            // raised above it too (`else if d { g } else { h }` on the
            // line, its arms on rows of their own, #1264), the leg ends at
            // that fork's dot, half a row up: run on to the row, it went
            // past the dot between the inner arms' diagonals, like a third
            // arm. Likewise a one-row diamond's fork, a row up (`else { if
            // s.is_empty() { return; } }` on the line, #1534): run on to
            // the row, it hung below the fork in mid-air.
            if raised && !exit_at_split && branch.imaginary.is_none() {
                let first_row = branch.states.iter().find(|(top, bot, _)| top <= bot).map(|s| s.0);
                if let Some(first_row) = first_row.filter(|r| r > split_point) {
                    let rise = apexes.raised_splits.get(&first_row).copied()
                        .or_else(|| apexes.splits.contains(&first_row).then_some(COLLAPSED_APEX))
                        .unwrap_or(0);
                    push_seg_if_renderable(
                        &mut segs, rap,
                        (branch_x, column_top_y), (branch_x, (get_y_axis_pos(first_row) - rise) as f64),
                        leading_state, visualization_data, &arm_label, Leg::Column, *split_point, first_row, &arm_path,
                    );
                }
            }
            // A reference declared before the conditional whose borrow
            // ends in this arm (`end_outer_borrows_in_arms`): the
            // dashed rest of the arm is "no value" only in the sense
            // that its borrow is over, not that r was never assigned.
            let borrow_end = if rap.is_ref() { first_borrow_end(&branch.e_data) } else { None };
            let ended_title = |segs: &mut Vec<BranchSeg>, to: usize| {
                if let (Some(end), Some(seg)) = (borrow_end, segs.last_mut()) {
                    if to > end && matches!(seg.state, State::FullPrivilege { s: LineState::Uninit }) {
                        seg.title = hover_messages::state_ref_borrow_ended(rap.name());
                    }
                }
            };
            let mut stop_y: Option<f64> = None;
            // The dashed rest after the borrow ended, where the join has
            // no dot for it to reach (another arm moved r, `arm_end_state`):
            // the column stops at the dot where the borrow ended (#1416).
            let no_rest = |k: usize| borrow_end.is_some_and(|e| branch.states[k].0 >= e)
                && matches!(end_state, State::OutOfScope)
                && branch.states[k..].iter().all(|s| matches!(s.2, State::OutOfScope | State::FullPrivilege { s: LineState::Uninit }));
            for (k, (top, bot, state)) in branch.states.iter().enumerate().filter(|_| branch.imaginary.is_none()) {
                // The binding's dot is on the split row, above the row
                // the arm's column would start on: the state up to it
                // ran backwards, from that row up to the dot. A raised
                // fork's arm starts on the split row itself.
                if raised && top > bot { continue; }
                if no_rest(k) { continue; }
                let bot = match stop_line {
                    None => *bot,
                    Some(stop) if *top >= stop => continue,
                    Some(stop) => (*bot).min(stop),
                };
                // After the borrow ends in an arm whose dashed "borrow has
                // ended" rest runs on to the join (the arm's last rows are
                // dashed), the rows between the dot where it ended and those
                // have no state to draw (the reference holds nothing), so the
                // dashes started a row below the dot, in mid-air (#1292). They
                // start at the dot. From the join of a conditional nested in
                // the arm, if the stretch starts with one (where it ended):
                // over the nested arms the column is theirs. (An arm whose
                // column has no dashed rest just ends at the dot, as a
                // reference's column does outside a conditional.)
                let (top, state) = match state {
                    State::OutOfScope if stop_line.is_none() && borrow_end.is_some() && seg_drawn(rap, &end_state) => {
                        let from = branch.e_data.iter()
                            .filter_map(|(_, e)| match e {
                                Event::Branch { merge_point, .. } if *merge_point > *top && *merge_point <= bot => Some(*merge_point),
                                _ => None,
                            })
                            .max()
                            .unwrap_or(*top);
                        if from < bot && borrow_end.is_some_and(|end| from >= end) {
                            (from, State::FullPrivilege { s: LineState::Uninit })
                        } else {
                            (*top, state.clone())
                        }
                    }
                    s => (*top, s.clone()),
                };
                let top = &top;
                let Some((y1, y2)) = apexes.segment_ys(*top, bot, None) else { continue };
                if stop_line == Some(bot) { stop_y = Some(y2 as f64); }
                let n = segs.len();
                // RustViz 3: on the arm's own rows, the state table's.
                // (Not a one-line conditional's: its row's state is every arm's.)
                // (A one-line arm's own cell is its state after its code: from its
                // last dot on.)
                let last_event = branch.e_data.iter().map(|(l, _)| *l).max().unwrap_or(0);
                let pieces = leg_table_pieces(rap, visualization_data, &state, *top, bot, own, *split_point, (i, &arm_sources, *merge_point, last_event), &arm_path);
                if pieces.is_empty() {
                    push_seg_if_renderable(
                        &mut segs, rap,
                        (branch_x, y1 as f64),
                        (branch_x, y2 as f64),
                        state, visualization_data, &arm_label, Leg::Column, *top, bot, &arm_path,
                    );
                    if *top == own.0 {
                        if let Some(seg) = segs.get_mut(n) {
                            explain_drop_loan(seg, false);
                        }
                    }
                }
                for (p_top, p_bot, p_state, p_title, from_arm) in pieces {
                    arm_bottom = from_arm.then(|| (p_bot, p_state.clone(), p_title.clone()));
                    // (Inside the stretch 2.x draws for the segment.)
                    let Some((py1, py2)) = apexes.segment_ys(p_top, p_bot, None) else { continue };
                    let (py1, py2) = (if p_top == *top { y1 } else { py1.max(y1) }, if p_bot == bot { y2 } else { py2.min(y2) });
                    if py2 <= py1 {
                        continue;
                    }
                    match p_title {
                        Some(title) if seg_drawn(rap, &p_state) => segs.push(BranchSeg {
                            p1: (branch_x, py1 as f64), p2: (branch_x, py2 as f64), state: p_state, title,
                            inactive_arm: None, own_row: false, rows: (p_top, p_bot),
                        }),
                        Some(_) => {}
                        None => {
                            let next = segs.len();
                            push_seg_if_renderable(
                                &mut segs, rap, (branch_x, py1 as f64), (branch_x, py2 as f64),
                                p_state, visualization_data, &arm_label, Leg::Column, p_top, p_bot, &arm_path,
                            );
                            if p_top == own.0 {
                                if let Some(seg) = segs.get_mut(next) {
                                    explain_drop_loan(seg, false);
                                }
                            }
                        }
                    }
                }
                if segs.len() > n { ended_title(&mut segs, bot); }
            }
            // The split diagonal alone can reach the exit row of a
            // diverging arm (#1295, #1876). A dot ending the value will
            // cap it below. On a separate arm row, a live dot still
            // needs a bar just past it.
            if let (None, Some(stop)) = (stop_y, stop_line) {
                if exit_at_split || (seg_drawn(rap, &end_state) && !branch.e_data.iter().any(|(l, _)| *l == stop)) {
                    let stop_row_y = get_y_axis_pos(stop) as f64;
                    stop_y = segs.last()
                        .filter(|seg| seg.p2.0 == branch_x && if exit_at_split {
                            seg.p2.1 == stop_row_y
                        } else {
                            seg.p2.1 <= stop_row_y + 0.5
                        })
                        .map(|seg| seg.p2.1);
                }
            }
            let mut stop = None;
            match (stop_line, branch.imaginary) {
                (_, Some(_)) => {
                    stop = segs.last().map(|seg| (own_last, seg.p2.1));
                }
                (None, None) => {
                    // From the join of an `else if` nested in this arm,
                    // which ends on the same row (#1294).
                    let leg_top_y = branch.e_data.iter().rev().find_map(|(_, e)| match e {
                        Event::Branch { split_point: s, merge_point: m, .. }
                            if *m == *merge_point && !branch_is_collapsed(*s, *m) && !branch_is_collapsed(*split_point, *merge_point) =>
                            Some((get_y_axis_pos(*m) - join_rise(visualization_data, hash, history, *s, *m)) as f64),
                        _ => None,
                    }).unwrap_or(column_bot_y);
                    // An arm whose own column runs on below that row, to
                    // the merge row (a one-line arm with rows of its own
                    // whose borrow ends on the conditional's line, #1321):
                    // the leg starts where the column stops, not in
                    // mid-air beside it.
                    let leg_top_y = segs.iter().rev()
                        .find(|seg| seg.p2.0 == branch_x && seg.p1.0 == branch_x)
                        .map(|seg| seg.p2.1)
                        .filter(|&y| y > leg_top_y && y <= merge_y)
                        .unwrap_or(leg_top_y);
                    let n = segs.len();
                    // A one-line arm's column read from its own cell down to
                    // the diagonal: the diagonal says the same (the arm's
                    // state on its way to the join).
                    let lined = |st: &State| matches!(st, State::FullPrivilege { s: LineState::Full } | State::PartialPrivilege { .. } | State::RevokedPrivilege { .. });
                    let column_bot = branch.states.iter().map(|(_, b, _)| *b).max();
                    // (An arm whose last dot ends its column's own rows, `else
                    // { None }`: its cell from that dot on.)
                    let last_dot = branch.e_data.iter().map(|(l, _)| *l).max();
                    let diagonal = match arm_bottom.take() {
                        Some((b, st, t)) => (Some(b) == column_bot).then_some((st, t)),
                        None => last_dot.filter(|d| Some(*d) == column_bot).and_then(|d| {
                            let cell = leg_one_line_arm_cell(rap, visualization_data, d, ty.get_start_end(i), *split_point, (i, &arm_sources, *merge_point, d))?;
                            one_line_arm_piece(&end_state, rap, visualization_data, d, cell, &arm_path)
                        }),
                    };
                    // The padded outer leg can follow a nested join even
                    // when its replayed end state is the inactive stretch
                    // beside a sibling. Read ownership on the MIR exits
                    // that can actually carry a value to this join.
                    let nested_join = (matches!(end_state, State::FullPrivilege { s: LineState::Gray }) && !rap.is_ref())
                        .then(|| {
                            let table = &visualization_data.state_table;
                            let span = branch.source_span.filter(|s| s.start.0 == s.end.0)?;
                            let row = span.start.0;
                            if *fork_above && i == 1 && branch_history.len() == 2 && branch.e_data.is_empty() {
                                let other = branch_history[0].source?;
                                table.empty_sibling_cell(*visualization_data.fn_source_lines.get(rap.hash())?, rap.name(), row, branch.source?, other)
                            } else {
                                table.continuing_arm_cell(*visualization_data.fn_source_lines.get(rap.hash())?, rap.name(), row, branch.source?)
                            }
                        }).flatten()
                        .filter(|c| c.init == crate::facts::table::Init::Own && c.parts.is_none() && c.lent.is_empty())
                        .map(|_| (State::FullPrivilege { s: LineState::Full }, None));
                    match diagonal.filter(|(st, _)| lined(st) && lined(&end_state)).or(nested_join) {
                        Some((st, Some(title))) => segs.push(BranchSeg {
                            p1: (branch_x, leg_top_y), p2: (parent_x, merge_y), state: st, title,
                            inactive_arm: None, own_row: false, rows: (*merge_point, *merge_point),
                        }),
                        Some((st, None)) => push_seg_if_renderable(
                            &mut segs, rap,
                            (branch_x, leg_top_y), (parent_x, merge_y),
                            st, visualization_data, &arm_label, Leg::Join, *merge_point, *merge_point, &arm_path,
                        ),
                        None => push_seg_if_renderable(
                            &mut segs, rap,
                            (branch_x, leg_top_y), (parent_x, merge_y),
                            end_state, visualization_data, &arm_label, Leg::Join, *merge_point, *merge_point, &arm_path,
                        ),
                    }
                    // A scrutinee temporary can give its borrow back on the
                    // failed edge of a let-chain. Keep the dashed connector's
                    // explanation and the owner's state once the loan ends.
                    if let Some(seg) = segs.get_mut(n) {
                        explain_drop_loan(seg, true);
                        if matches!(seg.state, State::FullPrivilege { s: LineState::Gray }) {
                            if let Some(title) = fieldless_join_title(visualization_data, rap, ev, i, branch) {
                                seg.title.push(' ');
                                seg.title.push_str(&title);
                            }
                        }
                        if matches!(seg.state, State::FullPrivilege { s: LineState::Gray })
                            && !rap.is_ref() && !rap.holds_borrow()
                            && branch.e_data.iter().any(|(l, e)| Some(*l) == last_dot
                                && matches!(e, Event::StaticReacquire { from: ResourceTy::Anonymous, .. }
                                    | Event::MutableReacquire { from: ResourceTy::Anonymous, .. })
                                && visualization_data.state_table.end_loans.contains_key(&e.get_id()))
                            && rv_attrs::line_of(*merge_point, &visualization_data.row_lines)
                                .zip(visualization_data.fn_source_lines.get(rap.hash()))
                                .and_then(|(row, f)| visualization_data.state_table.cell(*f, rap.name(), row, row))
                                .is_some_and(|c| c.init == crate::facts::table::Init::Own && !c.may && c.lent.is_empty())
                        {
                            let full = State::FullPrivilege { s: LineState::Full };
                            seg.title.push(' ');
                            seg.title.push_str(&timeline_segment_title(&full, rap, visualization_data, None));
                        }
                    }
                    if segs.len() > n { ended_title(&mut segs, *merge_point); }
                }
                (Some(stop_row), None) => {
                    let has_dot = !exit_at_split || dots.at.contains_key(&(*hash, branch.t_data.x_val, get_y_axis_pos(stop_row)));
                    // Only where the column is still drawn: a branch
                    // that moved the value away ended its column at
                    // the move already.
                    // Nor where it ends in a drop on that line (a
                    // one-line `0 => panic!()` arm, #410): the drop
                    // dot ends it.
                    let dropped_at_stop = has_dot && branch.e_data.iter().any(|(l, e)| *l == stop_row
                        && matches!(e, Event::OwnerGoOutOfScope { .. } | Event::RefGoOutOfScope { .. } | Event::TemporaryDrop { .. }));
                    // Nor where the arm moved the value away on that line
                    // (`return take(q);`), as in a one-row arm (#1295) and
                    // an arm whose move comes before its last line, or a
                    // reference's borrow ends there (`1 => continue,` after
                    // its last use): the column ends at that dot, which
                    // covered the bar drawn under it but for a 1px nub
                    // (#1375).
                    let at_stop = |e: &Event| !matches!(e, Event::Branch { .. });
                    let ended_at_stop = has_dot && branch.e_data.iter().any(|(l, e)| *l == stop_row && at_stop(e)
                        && (!seg_drawn(rap, &end_state)
                            || matches!(e, Event::StaticDie { .. } | Event::MutableDie { .. } | Event::RefDie { .. })));
                    // A dot of the still-live column on that line (`s.push('a');
                    // break;`) covered it too: the column runs on a little
                    // past the dot and the bar goes there.
                    if has_dot && !dropped_at_stop && !ended_at_stop && branch.e_data.iter().any(|(l, e)| *l == stop_row && at_stop(e)) {
                        if let (Some(y), Some(seg)) = (stop_y, segs.last_mut()) {
                            if seg.p2 == (branch_x, y) {
                                if seg.p1.0 == branch_x {
                                    seg.p2.1 += STOP_BAR_BELOW_DOT;
                                } else {
                                    push_seg_if_renderable(
                                        &mut segs, rap,
                                        (branch_x, y), (branch_x, y + STOP_BAR_BELOW_DOT),
                                        end_state.clone(), visualization_data, &arm_label, Leg::Column, stop_row, stop_row, &arm_path,
                                    );
                                }
                                stop_y = Some(y + STOP_BAR_BELOW_DOT);
                            }
                        }
                    }
                    let dropped_at_stop = dropped_at_stop || ended_at_stop;
                    if let Some(y) = stop_y.filter(|y| !dropped_at_stop && segs.last().is_some_and(|seg| seg.p2 == (branch_x, *y))) {
                        stop = Some((stop_row, y));
                    }
                }
            }

            // A stop bar marks where the leg ends. Its hover underlines
            // no code, and an imaginary leg has no event at its end.
            let stop_at = stop.map(|(_, y)| (branch_x, y));
            let stop_bar = stop.map_or_else(String::new, |(row, y)| {
                bars.bars.push((branch_x, y));
                let title = if branch.imaginary.is_some() {
                    hover_messages::event_dot_dead_else_note(rap.name())
                } else {
                    hover_messages::branch_never_reaches_merge(branch.exits.contains(&BranchExit::Other))
                };
                render_stop_bar(rap, *hash, row, branch_x, y, &title)
            });

            // An arm of one row (`None => println!(..),`, `1 => show(&s),`)
            // has no rows of its own in its column to be solid over. It
            // still gets the half row around its own row, so it reads as
            // the arm that runs here (#845): above the row in the state the
            // arm starts in, below it in the state its events there leave
            // (none after a move; a reference whose borrow ended there goes
            // on dashed). Left out for an arm with events, its leg was
            // dashed straight through its own dot (#1373). The join
            // diagonal starts below it; the join point doesn't move.
            let on_own_row = branch.e_data.iter()
                .all(|(l, e)| *l == own_first && !matches!(e, Event::Branch { .. }));
            if on_own_row && !branch.diverges && !is_active
                && own_first == own_last && own_first > *split_point && own_first < *merge_point
                && !branch_is_collapsed(*split_point, *merge_point)
                && !state_is_dashed(&parent_active)
                // Lent (dotted) beside the other arms too: the column is
                // drawn the same over this row already. A second segment
                // over it ended and started in mid-air (#1333).
                && passive_dashed
            {
                let row_y = get_y_axis_pos(own_first) as f64;
                let top = (row_y - LINE_SPACE as f64 / 2.0).max(column_top_y);
                let bot = (row_y + LINE_SPACE as f64 / 2.0).min(merge_y);
                // What the row's events leave: the last state from the row
                // on (a move's), or, where the arm's column goes on dashed
                // beside the arms below, the state before those dashes.
                let after = if branch.e_data.is_empty() {
                    Some(parent_active.clone())
                } else {
                    let from_row: Vec<&State> = branch.states.iter()
                        .filter(|(t, _, _)| *t == own_first).map(|s| &s.2).collect();
                    match from_row.as_slice() {
                        [.., before, last] if state_is_dashed(last) => Some((*before).clone()),
                        [.., last] => Some((*last).clone()),
                        [] => None,
                    }.filter(|st| !state_is_dashed(st))
                };
                let halves: Vec<((f64, f64), State)> = match after {
                    Some(st) if st == parent_active => vec![((top, bot), st)],
                    Some(st) => vec![((top, row_y), parent_active.clone()), ((row_y, bot), st)],
                    None => vec![((top, row_y), parent_active.clone())],
                };
                // After the dashes above it, before any below it and the join.
                let mut at = segs.iter().position(|s| s.p1.1 >= row_y - 1e-6)
                    .unwrap_or(segs.len());
                let mut solid_bot = None;
                // Not over a stretch the arm already draws solid there.
                let solid_at = |segs: &[BranchSeg], y1: f64, y2: f64| segs.iter().any(|s| {
                    s.p1.0 == branch_x && s.p2.0 == branch_x && !state_is_dashed(&s.state)
                        && s.p1.1.min(s.p2.1) < y2 - 1e-6 && s.p1.1.max(s.p2.1) > y1 + 1e-6
                });
                for ((y1, y2), st) in halves {
                    if solid_at(&segs, y1, y2) { continue; }
                    let n = segs.len();
                    push_seg_if_renderable(
                        &mut segs, rap, (branch_x, y1), (branch_x, y2),
                        st, visualization_data, &arm_label, Leg::Column, own_first, own_first, &arm_path,
                    );
                    if segs.len() > n {
                        let mut solid = segs.pop().unwrap();
                        solid.own_row = true;
                        segs.insert(at, solid);
                        at += 1;
                        solid_bot = Some(y2);
                    }
                }
                if let Some(bot) = solid_bot.filter(|y| *y > row_y) {
                    if let Some(join) = segs.iter_mut().find(|s| (s.p1.0 - s.p2.0).abs() > 1e-6 && s.p1.0 == branch_x) {
                        if join.p1.1 < bot { join.p1.1 = bot; }
                    }
                }
            }

            // A reference whose borrow ended before the conditional (`r = if
            // c { let y = &a; y } else { &b };` after r's last use, #1393:
            // the legs before each branch gives it a new one), or in an
            // arm: it isn't a variable with no value yet.
            // (r was given a value before the outermost conditional: not
            // `let r = if ..`, whose legs before a value say it has none.)
            let had_value = visualization_data.timelines.get(hash).and_then(|t| t.history.first())
                .is_some_and(|(l, e)| !matches!(e, Event::Branch { .. }) && *l < *split_point);
            if rap.is_ref() && had_value && !entry_maybe_init {
                let no_value = hover_messages::state_uninitialized(rap.name());
                for seg in segs.iter_mut().filter(|seg| seg.title == no_value) {
                    seg.title = hover_messages::state_ref_borrow_ended(rap.name());
                }
            }
            // A let chain's scrutinee temporary that a later part of the
            // chain makes (`if k && let Ok(s) = make()`, #1386) isn't one a
            // branch borrows (#1397): it exists only if the parts before
            // its `let` matched.
            if rap.is_split_scrutinee_temporary() {
                let no_value = hover_messages::state_uninitialized(rap.name());
                for seg in segs.iter_mut().filter(|seg| seg.title == no_value) {
                    seg.title = hover_messages::state_chain_temp_unmade(rap.name());
                }
            }
            // After a conditional inside this arm that leaves the column
            // maybe-initialized (a nested `if` that assigns a `let x;`
            // local or makes a branch temporary in one of its branches,
            // #1437, #1397), the arm's rows below its join may hold a
            // value too.
            let inner_maybe_from = branch.e_data.iter().filter_map(|(_, e)| match e {
                Event::Branch { branch_history, maybe_init, merge_point, .. }
                    if matches!(branches_init_join(*maybe_init, branch_history), Some(InitJoin::Maybe { .. })) => Some(*merge_point),
                _ => None,
            }).min();
            if let Some(from) = inner_maybe_from.filter(|_| !entry_maybe_init) {
                let no_value = hover_messages::state_uninitialized(rap.name());
                for seg in segs.iter_mut().filter(|seg| seg.title == no_value && seg.rows.0 >= from) {
                    seg.title = hover_messages::state_maybe_uninitialized(rap.name());
                }
            }
            if entry_maybe_init {
                let no_value = hover_messages::state_uninitialized(rap.name());
                for seg in segs.iter_mut().filter(|seg| seg.title == no_value) {
                    // Not branches above: a loop above that may have
                    // ended before assigning it (#1034).
                    seg.title = if entry_init == Some(InitJoin::Loop) {
                        hover_messages::state_maybe_uninitialized_after_loop(rap.name())
                    } else {
                        hover_messages::state_maybe_uninitialized(rap.name())
                    };
                }
            }
            let mut segs = trim_dashes_under_solid(segs);
            if let Some(span) = branch.source_span.filter(|_| branch.imaginary.is_none()) {
                segs = part_loans_in_arm(segs, rap, visualization_data, span, branch.source, branch_x);
                let released_on_this_path = || {
                    use crate::facts::table::{Init, Why};
                    let table = &visualization_data.state_table;
                    let &function = visualization_data.fn_source_lines.get(rap.hash())?;
                    let source = branch.source?;
                    let row = span.start.0;
                    if span.end.0 != row || rap.is_ref() { return None; }
                    let cell = table.one_line_arm_cell(function, rap.name(), row, source, &arm_sources)?;
                    if cell.init != Init::Own || cell.may || cell.parts.is_some() || !cell.lent.is_empty() { return None; }
                    arm_sources.iter().flatten().filter(|&&other| other != source)
                        .filter_map(|&other| table.one_line_arm_cell(function, rap.name(), row, other, &arm_sources))
                        .any(|other| other.lent.iter().any(|l| cell.why.iter().any(|why|
                            matches!(why, Why::LoanEnded { loan, .. } if *loan == l.loan))))
                        .then_some(())
                };
                if let Some(join) = segs.iter_mut().find(|s| s.p1.0 == branch_x && s.p2.0 == parent_x
                    && matches!(s.state, State::FullPrivilege { s: LineState::Full })).filter(|_| released_on_this_path().is_some()) {
                    let path = hover_messages::state_arm_join(&arm_label);
                    let owner = styled_name_html(rap.name());
                    join.title = format!("{path} {owner} can be used on this path. {}", join.title);
                }
            }

            // This leg combines several feasible incoming edges. Its
            // approach to the qualified end has the same path dependence.
            if let Some((row, cases)) = branch.e_data.iter()
                .filter(|(_, e)| matches!(e, Event::MutableReacquire { .. } | Event::StaticReacquire { .. })).find_map(|(row, e)|
                visualization_data.row_layout.fork_end_cases.get(&e.get_id()).map(|cases| (row, cases))) {
                for seg in segs.iter_mut().filter(|seg| seg.rows.1 <= *row
                    && matches!(seg.state, State::RevokedPrivilege { .. } | State::PartialPrivilege { s: LineState::Full })) {
                    seg.title.clone_from(cases);
                }
            }

            // The arm's own source lines, as the y-range that hovering
            // its column underlines (#369). The column is also drawn
            // dashed beside the other arms' code, and the diagonals
            // pass it; neither underlines those lines. An arm with no
            // lines of its own (the implicit `else`) underlines none.
            let own_y = (own_first <= own_last).then(|| (
                get_y_axis_pos(own_first) as f64,
                get_y_axis_pos(own_last) as f64,
            ));

            // A dashed stretch that underlines nothing mustn't say
            // "the underlined code" (#843).
            let split_row = get_y_axis_pos(*split_point) as f64;
            // Nested forks with separate arm rows can leave a dashed
            // stretch entirely between the code rows it would underline.
            let row_lines = (*fork_above && !path.is_empty()).then_some(visualization_data.row_lines.as_slice());
            for seg in &mut segs {
                if let Some(arm) = &seg.inactive_arm {
                    if seg_underline_y(seg, own_y, split_row, row_lines).is_none() {
                        seg.title = hover_messages::state_inactive_arm_unlit(arm);
                    }
                }
            }
            if let Some(binding) = branch_history.iter().filter_map(|b| visualization_data.state_table.imaginary_arm(b.imaginary?))
                .find_map(|arm| arm.initializer_bindings.get(rap.name()))
            {
                let title = hover_messages::event_dot_imaginary_initializer(binding);
                for seg in &mut segs {
                    seg.title = title.clone();
                }
            }
            let runs = split_centerline_runs(&segs);
            // Its slanted legs (into a split's arm, out to a join), for
            // `untangle`: an arrow's head along one hides it (#1476).
            bars.legs.extend(segs.iter().filter(|seg| (seg.p1.0 - seg.p2.0).abs() > 0.5).map(|seg| (seg.p1, seg.p2)));
            // And its vertical stretches: a head coming steeply into a dot
            // along one hides it (#1488).
            let cols = segs.iter().filter(|seg| (seg.p1.0 - seg.p2.0).abs() <= 0.5).map(|seg| (seg.p1, seg.p2));
            bars.cols.extend(cols.clone());
            bars.branch_cols.extend(cols);

            // `data-arm-rows`: the first and last row of those lines (the
            // corpus tests check hover against it).
            let arm_rows_attr = if own_y.is_some() {
                format!(" data-arm-rows=\"{} {}\"", own_first, own_last)
            } else {
                String::new()
            };

            let mut imaginary_attr = String::new();
            if let Some(span) = branch.source_span.filter(|_| branch.imaginary.is_some()) {
                imaginary_attr = format!(" data-rv-cause=\"imaginary\" data-rv-arm-start=\"{} {}\"", span.start.0, span.start.1);
                // With no leg out of the conditional, the matching arm's
                // events describe only that arm, even on the fork's row.
                if let Some(src) = rv_attrs::line_of(*split_point, &visualization_data.row_lines)
                    .filter(|_| branch_history.iter().all(|b| b.diverges))
                {
                    imaginary_attr.push_str(&format!(" data-rv-no-continuation=\"{src} {}\"", span.end.0));
                }
            }

            // The arm's column is split into stretches (see
            // `render_branch_run`), each its own `branch-group`.
            let mut stretches: Vec<String> = Vec::new();
            for (run_index, run) in runs.iter().enumerate() {
                let (joints, run_stretches) = render_branch_run(rap, *hash, run, (*id, i, run_index), own_y, split_row, row_lines, stop_at);
                // Joint discs are decorative; they ride with the first stretch.
                let mut run_stretches = run_stretches.into_iter();
                if let Some(first) = run_stretches.next() { stretches.push(joints + &first); }
                stretches.extend(run_stretches);
            }
            if let Some(last) = stretches.last_mut() { last.push_str(&stop_bar); }
            let branch_svg: String = stretches.iter().map(|st| format!(
                "        <g class=\"branch-group\"{}{}>\n{}        </g>\n", arm_rows_attr, imaginary_attr, st,
            )).collect();

            output.entry(-1).or_insert_with(empty_panel_pair).0.timelines.push_str(&branch_svg);

            render_branches(
                hash, rap, &branch.e_data, &branch.states,
                output, visualization_data, &branch.t_data, entry_maybe_init, &arm_path, dots, bars,
            );
        }
    }
}

fn render_stop_bar(rap: &ResourceAccessPoint, hash: u64, row: usize, x: f64, y: f64, title: &str) -> String {
    let rv = Rv::new(rv_attrs::STOP_BAR).var(rap.name()).row(row).at(format!("stop {x} {y}"));
    format!(
        "            <line data-hash=\"{hash}\" class=\"tooltip-trigger\" x1=\"{}\" y1=\"{y}\" x2=\"{}\" y2=\"{y}\" data-tooltip-text=\"{title}\"{} data-underline=\"none\" style=\"stroke-width: 3px; stroke-linecap: round;\"/>\n            <polygon data-hash=\"{hash}\" class=\"tooltip-trigger\" points=\"{},{} {},{} {},{} {},{}\" data-tooltip-text=\"{title}\"{} data-underline=\"none\" style=\"fill:transparent; stroke:none; pointer-events:fill;\"/>\n",
        x - 6.0, x + 6.0, rv.attrs(),
        x - 6.0, y - STOP_BAR_HOVER_HALF_H, x + 6.0, y - STOP_BAR_HOVER_HALF_H,
        x + 6.0, y + STOP_BAR_HOVER_HALF_H, x - 6.0, y + STOP_BAR_HOVER_HALF_H,
        rv.part("hit").attrs(),
    )
}

/// The imaginary leg holds one snapshot for its entire length (Q9).
fn imaginary_leg(
    rap: &ResourceAccessPoint,
    vd: &VisualizationData,
    edge: crate::facts::ImaginaryEdgeId,
    row: usize,
) -> Option<(State, String)> {
    use crate::facts::table::Init;
    let cell = vd.state_table.imaginary_arm(edge)?.entry.get(rap.name())?.clone();
    let full = State::FullPrivilege { s: LineState::Full };
    let uninit = State::FullPrivilege { s: LineState::Uninit };
    let (state, title) = match cell.init {
        Init::Moved => return None,
        Init::Unassigned => (uninit, hover_messages::state_uninitialized(rap.name())),
        Init::Maybe => (uninit, hover_messages::state_maybe_init(rap.name(), &cell)),
        Init::Own | Init::Partly if rap.is_ref() => {
            if cell.lent.is_empty() || (!rap.is_mutref() && cell.lent.iter().all(|l| l.kind == "shared")) {
                let title = timeline_segment_title(&full, rap, vd, None);
                (full, title)
            } else {
                let fn_line = *vd.fn_source_lines.get(rap.hash())?;
                let exclusive = cell.lent.iter().any(|l| matches!(l.kind, "mut" | "unique")
                    && vd.state_table.lends_whole(fn_line, rap.name(), l.loan));
                let state = if exclusive {
                    State::RevokedPrivilege {
                        to: ResourceTy::Anonymous, borrow_to: ResourceTy::Anonymous,
                        prev_state: Box::new(full),
                    }
                } else {
                    State::PartialPrivilege { s: LineState::Full }
                };
                let shared = cell.lent.iter().all(|l| l.kind == "shared"
                    && vd.state_table.lends_whole(fn_line, rap.name(), l.loan));
                (state, hover_messages::state_imaginary_ref(rap.name(), rap.is_mutref(), shared, cell.may_lend))
            }
        }
        Init::Own | Init::Partly => owner_segment_of(rap, vd, row, &full, Some(cell), Some(edge))?,
    };
    let note = hover_messages::event_dot_dead_else_note(rap.name());
    Some((state, format!("{note}. At the entry to the borrow checker's imaginary path, {title}")))
}

/// Keep the entry snapshot on the leg. Its hover can also distinguish
/// what borrow checking would allow after the written code on this row.
fn imaginary_move_note(
    rap: &ResourceAccessPoint,
    vd: &VisualizationData,
    edge: crate::facts::ImaginaryEdgeId,
    row: usize,
) -> Option<String> {
    use crate::facts::table::Init;
    let arm = vd.state_table.imaginary_arm(edge)?;
    let entry = arm.entry.get(rap.name())?;
    if !matches!(entry.init, Init::Own | Init::Partly) { return None; }
    let (_, cells) = arm.after.range(..=row).next_back()?;
    let cell = cells.get(rap.name())?;
    let title = match cell.init {
        Init::Moved => hover_messages::state_moved_out(rap.name(), rap.is_ref() || rap.holds_value()),
        Init::Maybe if cell.cause == Some("moved") || cell.mixed_init.is_some() => hover_messages::state_maybe_init(rap.name(), cell),
        _ => {
            let parts = cell.parts.as_ref().filter(|parts| Some(*parts) != entry.parts.as_ref())?;
            if parts.moved.is_empty() && parts.maybe.is_empty() { return None; }
            let fn_line = *vd.fn_source_lines.get(rap.hash())?;
            let fields = vd.state_table.fields_of(fn_line, rap.name());
            let others: Vec<_> = fields.iter().filter(|f| !parts.moved.contains(f) && !parts.maybe.contains(f)).collect();
            let others_usable = vd.state_table.fields_complete(fn_line, rap.name()) && !others.is_empty()
                && others.iter().all(|&f| cells.get(f).is_some_and(|c| c.init == Init::Own && c.lent.is_empty()));
            hover_messages::state_table_partly_moved(rap.name(), &parts.moved, &parts.maybe, &[], others_usable)
        }
    };
    Some(format!(". After this code on that path, {title}"))
}

/// RustViz 3 (P1-6): an arm's column on its own rows reads each row's
/// state from the state table, as a straight column does
/// (`table_lent_state`, `table_partly_moved_title`): a row inside a
/// multi-line arm has that arm's path's state. The pieces (first row,
/// last row, state, title; `None`: the leg's own title for that state)
/// cover `top..bot`. None where the table changes nothing or doesn't
/// apply (a stretch drawn dashed or not at all, rows beside another
/// arm's code), and 2.x's segment stands.
fn leg_table_pieces(
    rap: &ResourceAccessPoint,
    visualization_data: &VisualizationData,
    state: &State,
    top: usize,
    bot: usize,
    own: (usize, usize),
    split: usize,
    arm: (usize, &[Option<crate::facts::SourceArm>], usize, usize),
    path: &[(usize, usize)],
) -> Vec<(usize, usize, State, Option<String>, bool)> {
    let lined = match state {
        State::FullPrivilege { s } | State::PartialPrivilege { s } => !matches!(s, LineState::Gray | LineState::Uninit),
        State::RevokedPrivilege { .. } => true,
        _ => false,
    };
    if !lined || rap.is_fn() || rap.is_closure() || top >= bot {
        return Vec::new();
    }
    if !visualization_data.fn_source_lines.get(rap.hash()).is_some_and(|l| visualization_data.state_table.has_fn(*l)) {
        return Vec::new();
    }
    let rl = &visualization_data.row_lines;
    let mut bounds = vec![top];
    // (A reference column's: after a row's last stacked sub-row of dots,
    // as on a straight column.)
    bounds.extend((top + 1..bot).filter(|r| !rap.is_ref() || rl.get(r + 1).is_none_or(Option::is_some)));
    bounds.push(bot);
    // (Each piece: whether the table decided it.)
    let mut out: Vec<(usize, usize, State, Option<String>, bool, bool)> = Vec::new();
    for w in bounds.windows(2) {
        // A blank or comment-only row of the arm carries the row above
        // (design Q3), and so does a row that leaves it (`return;`): the
        // state is the one the exit leaves with.
        let blank = w[0] > own.0 && w[0] < own.1 && rl.get(w[0]).copied().flatten()
            .and_then(|l| visualization_data.source_lines.get(l.wrapping_sub(1)))
            .is_some_and(|t| {
                let code = code_text(t);
                code.is_empty() || code.split(|c: char| !c.is_alphanumeric() && c != '_').next().is_some_and(|w| matches!(w, "return" | "break" | "continue"))
            });
        if let (true, Some(last)) = (blank, out.last_mut()) {
            if last.4 && last.1 == w[0] {
                last.1 = w[1];
                continue;
            }
        }
        let one_line = leg_one_line_arm_cell(rap, visualization_data, w[0], own, split, arm);
        let from_arm = one_line.is_some();
        let (st, title, table) = match (one_line, leg_table_row(rap, visualization_data, w[0], own, split)) {
            // A one-line conditional's arm: its own cell (design Q1).
            (Some(cell), _) => match one_line_arm_piece(state, rap, visualization_data, w[0], cell, path) {
                Some((st, t)) => (st, t, true),
                None => (state.clone(), None, true),
            },
            (None, None) => (state.clone(), None, false),
            (None, Some(at)) => match leg_lent_state(state, rap, visualization_data, at, path) {
                Some((st, t)) => (st, t, true),
                None => (state.clone(), None, true),
            },
        };
        let same = |last: &(usize, usize, State, Option<String>, bool, bool)| {
            last.4 == table && last.5 == from_arm && last.3 == title
                && (!table || std::mem::discriminant(&last.2) == std::mem::discriminant(&st))
                && (!table || title.is_some() || last.2 == st)
        };
        match out.last_mut() {
            Some(last) if same(last) => last.1 = w[1],
            _ => out.push((w[0], w[1], st, title, table, from_arm)),
        }
    }
    // Nothing differs from 2.x's.
    let unchanged = |st: &State| st == state || matches!((st, state), (State::RevokedPrivilege { .. }, State::RevokedPrivilege { .. }));
    if out.iter().all(|p| !p.4 || (p.3.is_none() && unchanged(&p.2))) {
        return Vec::new();
    }
    out.into_iter().map(|(a, b, st, t, _, arm)| (a, b, st, t, arm)).collect()
}

/// The visual row whose state-table state an arm's column shows below
/// visual row `row` (`leg_table_pieces`), when the table has it on the
/// arm's path: a row of the arm's own (`own`: its first and last row)
/// that the next one is too, with a state of its own. Not the
/// conditional's own row (`split`), nor a row with a branch of its own:
/// their state is one arm's or every arm's. The arm's first row, when it
/// only closes the arm above (`} else {`) or is a match arm's head (`Some(x)
/// => {`, whose state the facts read before the arm's way in, #1865), reads the
/// next row's state, if that row does nothing to the variable.
fn leg_table_row(rap: &ResourceAccessPoint, visualization_data: &VisualizationData, row: usize, own: (usize, usize), split: usize) -> Option<usize> {
    let (first, last) = own;
    // (Not a one-row arm's row, `0 => r = &t,`: the facts read its state
    // where it would go on to the next row, another arm's, #1865.)
    if row < first || row >= last || row == split {
        return None;
    }
    let rl = &visualization_data.row_lines;
    // (Not between dots stacked under one source line: the table's state
    // is the one after the whole line.)
    if rl.get(row + 1).is_some_and(Option::is_none) {
        return None;
    }
    let table = &visualization_data.state_table;
    let fn_line = *visualization_data.fn_source_lines.get(rap.hash())?;
    let col = rap.name();
    let src = rv_attrs::line_of(row, rl)?;
    // (The conditional's own line: the first source line from its split
    // row on; a let chain's split row is one added above it.)
    if rl.get(split..).and_then(|v| v.iter().find_map(|l| *l)).is_none_or(|split_src| src <= split_src) {
        return None;
    }
    let text = code_text(visualization_data.source_lines.get(src.wrapping_sub(1))?);
    // A row the column's state can be read after on this path: it has a
    // state of its own and, if it branches (`if d {` inside the arm, the
    // column not forked there), every way on agrees: its arms' states are
    // the same, or it does nothing to the column (a loan dying on one way
    // on only is an event of the row).
    let own_state = |l: usize| {
        let Some(cell) = table.cell(fn_line, col, l, l) else { return false };
        let arms = table.arms(fn_line, l);
        let cells: Vec<_> = (0..arms.len()).map(|i| table.cell_in_arm(fn_line, col, l, i).map(|c| (c.init, c.lent.iter().map(|x| x.loan).collect::<Vec<_>>(), c.parts))).collect();
        let heads = code_text(visualization_data.source_lines.get(l.wrapping_sub(1)).map_or("", |t| t.as_str()));
        if cells.is_empty() {
            !opens_conditional(&heads) || table.quiet(fn_line, col, l, false)
        } else {
            // On a lowered `for` head, the iterator's body entry and
            // exhausted exit can disagree. Use the snapshot if its MIR
            // edge leads straight into the next row of this leg and
            // the hidden iterator holds the loan there.
            let continues_here = table.continues_to_row(fn_line, l)
                .is_some_and(|next| rl.get(row + 1) == Some(&Some(next)));
            cells.windows(2).all(|w| w[0] == w[1])
                // A short-circuit write inside this arm rejoins before its
                // next row. The RHS alone cannot initialize the skipped path.
                || (continues_here && (table.skips_reassignment(fn_line, col, l)
                    || (!rap.is_ref() && cell.lent.iter().any(|loan| loan.iterator))))
        }
    };
    // (A match arm on one row, `0 => c = &y,`: as a one-row arm, #1865.)
    let arm_head = text.contains("=>");
    if arm_head && !text.ends_with('{') {
        return None;
    }
    let entry = row == first && (text.starts_with('}') || arm_head);
    if !entry {
        // A row that closes a block inside the arm (a nested conditional's
        // join) is read where the next row starts (`segment_cell`): only
        // when that row is the arm's too.
        let next_inside = || table.next_state_row(fn_line, src)
            .and_then(|n| rl.iter().position(|l| *l == Some(n)))
            .is_some_and(|at| at < last);
        return (own_state(src) && (!text.starts_with('}') || next_inside())).then_some(row);
    }
    // The row closes the arm above: its own state (if any) is that arm's.
    let code = text.trim_start_matches('}').trim().trim_start_matches("else").trim().trim_end_matches('{').trim();
    if text.starts_with('}') && !code.is_empty() {
        return None;
    }
    let next = table.next_state_row(fn_line, src)?;
    let next_text = code_text(visualization_data.source_lines.get(next.wrapping_sub(1)).map_or("", |t| t.as_str()));
    if next_text.starts_with('}') || !own_state(next) || !table.quiet(fn_line, col, next, false) {
        return None;
    }
    let at = rl.iter().position(|l| *l == Some(next))?;
    (at > row && at < last && rl.get(at + 1).is_none_or(Option::is_some)).then_some(at)
}

/// The state and title of an arm's column after visual row `at` (one
/// `leg_table_row` chose), as a straight column's: `owner_segment` for an
/// owner or a field, `table_lent_state` for a reference.
fn leg_lent_state(state: &State, rap: &ResourceAccessPoint, visualization_data: &VisualizationData, at: usize, path: &[(usize, usize)]) -> Option<(State, Option<String>)> {
    // (The row's own state: `segment_cell` reads the line below a row that
    // jumps or ends a block from the next row, which is another path's
    // here, Q3.)
    let fn_line = *visualization_data.fn_source_lines.get(rap.hash())?;
    let src = rv_attrs::line_of(at, &visualization_data.row_lines)?;
    let cell = visualization_data.state_table.cell(fn_line, rap.name(), src, src);
    leg_lent_state_of(state, rap, visualization_data, at, cell, path)
}

/// A one-line conditional's arm's column below visual row `at`, from the
/// arm's own cell `cell` (design Q1). The cell is read at the arm's exit,
/// where the arm's value may not be in the variable it initializes yet
/// (`let r = match n { 0 => &f, _ => &g };`, `let (r, s) = if c { (&mut x,
/// &mut y) } else { .. };`): a loan's holder there is a temporary, and a
/// title read for the row is the join's ("a branch above borrowed it on
/// some paths only"). So:
/// - where the cell has the column lent the way 2.x's leg draws it and
///   names no holder, 2.x's leg title stands: it names the variable each
///   arm's code lends it to;
/// - where the cell corrects 2.x's leg (`if choose { v.first() }`: v
///   borrowed, drawn as owned) but the facts can't say the holder points
///   to it (`maybe` may be `None`), 2.x's clause stays, with that the
///   place stays borrowed while the holder is in use;
/// - a holder whose arm gives it a value holding no reference (`None` of
///   an `Option<&T>`) can hold references; it doesn't contain one.
fn one_line_arm_piece(state: &State, rap: &ResourceAccessPoint, visualization_data: &VisualizationData, at: usize, cell: crate::facts::table::Cell, path: &[(usize, usize)]) -> Option<(State, Option<String>)> {
    use crate::facts::table::{Init, Why};
    let name = rap.name();
    let lent = !cell.lent.is_empty();
    let holds_nothing = rap.holds_borrow() && !lent && cell.init == Init::Own
        && cell.holds.is_empty() && cell.borrowed_from.is_empty()
        && cell.why.iter().any(|w| matches!(w, Why::Assigned { .. }));
    let has_drop_holder = !rap.is_ref() && visualization_data.fn_source_lines.get(rap.hash())
        .and_then(|&function| visualization_data.state_table.shared_drop_holder(function, name, &cell)).is_some();
    let (st, title) = leg_lent_state_of(state, rap, visualization_data, at, Some(cell), path)?;
    let same_kind = match (&st, state) {
        (State::FullPrivilege { s: a }, State::FullPrivilege { s: b }) => a == b,
        (State::PartialPrivilege { .. }, State::PartialPrivilege { .. })
        | (State::RevokedPrivilege { .. }, State::RevokedPrivilege { .. }) => true,
        _ => false,
    };
    let full = State::FullPrivilege { s: LineState::Full };
    if holds_nothing && same_kind && st == full {
        return Some((st, Some(hover_messages::state_holder_starts_empty(name))));
    }
    // (Unless the cell names who holds it: "r holds a shared borrow", a
    // part's loan "p.a is borrowed by r here".)
    let names_holder = title.as_deref().is_some_and(|t| t.contains(" holds a ") || t.replace("shared by one or more", "").contains(" by "));
    if same_kind && lent && !names_holder && !has_drop_holder {
        return Some((state.clone(), None));
    }
    // (`state_kept_borrowed_by`'s clause: the facts can't say the holder
    // points to it.)
    let kept = format!(". {} stays borrowed while ", styled_name_html(name));
    let title = match title {
        Some(t) if !same_kind && *state == full && !has_drop_holder && t.contains(&kept) => {
            let at_kept = t.find(&kept).unwrap_or(0);
            Some(format!("{}{}", timeline_segment_title(&full, rap, visualization_data, None), &t[at_kept..]))
        }
        t => t,
    };
    Some((st, title))
}

/// `leg_lent_state` from the cell `cell` (a row's own, or one arm's of a
/// one-line conditional).
fn leg_lent_state_of(state: &State, rap: &ResourceAccessPoint, visualization_data: &VisualizationData, at: usize, cell: Option<crate::facts::table::Cell>, path: &[(usize, usize)]) -> Option<(State, Option<String>)> {
    if rap.is_ref() {
        let fn_line = *visualization_data.fn_source_lines.get(rap.hash())?;
        let src = rv_attrs::line_of(at, &visualization_data.row_lines)?;
        let cell = cell.filter(|c| matches!(c.init, crate::facts::table::Init::Own | crate::facts::table::Init::Partly))?;
        return table_lent_state_of(state, rap, visualization_data, fn_line, src, &cell, at, path);
    }
    // No value yet on the arm's path (`let y = Some(if c { .. } else { ..
    // });` is assigned after the arms): an arm's leg is dashed then, as
    // 2.x draws a `let s;` local in an arm.
    let unassigned = cell.as_ref().is_some_and(|c| c.init == crate::facts::table::Init::Unassigned);
    owner_segment_of(rap, visualization_data, at, state, cell, None).map(|(st, title)| {
        (if unassigned { State::FullPrivilege { s: LineState::Uninit } } else { st }, Some(title))
    })
}

/// The state-table cell an arm (`arm`: its index and the number of arms)
/// of a conditional written on one line shows below visual row `row`:
/// its own arm's, from the facts' arms of that row
/// (matched by source arm), on the arm's own rows (`own`; one row, or
/// the conditional's row when the arms share it). None for another row,
/// or where the row's arms can't be matched with the conditional's (a
/// nested one on the line).
fn leg_one_line_arm_cell(
    rap: &ResourceAccessPoint,
    visualization_data: &VisualizationData,
    row: usize,
    own: (usize, usize),
    split: usize,
    (i, spans, merge, last_event): (usize, &[Option<crate::facts::SourceArm>], usize, usize),
) -> Option<crate::facts::table::Cell> {
    // (Its cell is the state after the arm's code: not above its last dot.)
    if row < last_event {
        return None;
    }
    if rap.is_fn() || rap.is_closure() {
        return None;
    }
    let rl = &visualization_data.row_lines;
    let table = &visualization_data.state_table;
    let fn_line = *visualization_data.fn_source_lines.get(rap.hash())?;
    let split_src = rl.get(split..).and_then(|v| v.iter().find_map(|l| *l))?;
    let src = rv_attrs::line_of(row, rl)?;
    let (first, last) = own;
    // (A conditional written on one line: it joins on that line too, or
    // on a row added below it.)
    if src != split_src || rv_attrs::line_of(merge, rl) != Some(split_src)
        || (first <= last && (row < first || row > last) && row != split)
    {
        return None;
    }
    table.one_line_arm_cell(fn_line, rap.name(), src, (*spans.get(i)?)?, spans)
}

/// A source line's code: without comments and string or char literals'
/// contents (`println!("=>")` reads `println!("")`), trimmed.
pub(crate) fn code_text(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    let mut in_str = false;
    let mut in_block = false;
    while let Some(c) = chars.next() {
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
        } else if in_str {
            if c == '\\' {
                chars.next();
            } else if c == '"' {
                in_str = false;
                out.push(c);
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '/' && chars.peek() == Some(&'/') {
            break;
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            in_block = true;
        } else {
            out.push(c);
        }
    }
    out.trim().to_owned()
}

/// Whether a line of code opens a conditional or loop whose block starts
/// on it (`if d {`, `let x = match y {`, `while let Some(v) = it.next() {`).
fn opens_conditional(code: &str) -> bool {
    code.ends_with('{') && code.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| matches!(w, "if" | "match" | "while" | "for" | "loop"))
}

/// Restrict active stretches of this leg only. The split precedes the
/// arm, the join leaves it. A one-line arm has a MIR exit snapshot, but
/// no snapshots between the events stacked inside it.
fn part_loans_in_arm(
    segments: Vec<BranchSeg>, rap: &ResourceAccessPoint, data: &VisualizationData,
    span: crate::facts::table::ArmSpan, source: Option<crate::facts::SourceArm>, branch_x: f64,
) -> Vec<BranchSeg> {
    let Some(&fn_line) = data.fn_source_lines.get(rap.hash()) else { return segments };
    if rap.is_ref() || rap.is_closure() || !data.state_table.has_part_loans(fn_line, rap.name()) { return segments; }
    // (A part named as the owner itself keeps the leg's own title.)
    let on_row = |row, state: &State, title: &String| {
        let (cell, arm) = data.state_table.arm_loan_cell(fn_line, rap.name(), row, span, source)?;
        part_loan_state(state, rap, data, fn_line, &cell, arm).map(|(st, t)| (st, t.unwrap_or_else(|| title.clone())))
    };
    let mut out = Vec::new();
    for mut seg in segments {
        if seg.p1.0 != branch_x || state_is_dashed(&seg.state) {
            out.push(seg);
            continue;
        }
        if seg.p2.0 != branch_x {
            // The join diagonal: the state the arm leaves with. A one-line
            // arm's is its row's own (a MIR arm snapshot); a longer arm's
            // `}` row has none, so it's its last row with a state of its
            // own (a loan taken there, `&mut p.b` as the arm's value, is
            // still live past the `}`).
            let exit = (span.start.0 + 1..span.end.0).rev()
                .find(|r| data.state_table.arm_loan_cell(fn_line, rap.name(), *r, span, source).is_some());
            let at = if span.start.0 == span.end.0 || data.state_table.arm_loan_cell(fn_line, rap.name(), span.end.0, span, source).is_some() {
                Some(span.end.0)
            } else {
                exit
            };
            if let Some((state, title)) = at.and_then(|r| on_row(r, &seg.state, &seg.title)) {
                (seg.state, seg.title) = (state, title);
            }
            out.push(seg);
            continue;
        }
        let (first, last) = seg.rows;
        let mut piece = seg.clone();
        for row in first..last {
            let y = get_y_axis_pos(row) as f64;
            if y >= seg.p2.1 { break; }
            // (Between dots stacked under one source line the state after
            // the whole line doesn't hold yet: `let q = &p; ..; let r = &mut
            // p.a;` lends p.a after q's loan; it applies from the last.)
            if data.row_lines.get(row + 1).is_some_and(Option::is_none) {
                continue;
            }
            let change = rv_attrs::line_of(row, &data.row_lines)
                .filter(|_| span.start.0 != span.end.0)
                .and_then(|src| on_row(src, &seg.state, &seg.title));
            let (state, title) = change.as_ref().map_or((&seg.state, &seg.title), |(s, t)| (s, t));
            if piece.title == *title && std::mem::discriminant(&piece.state) == std::mem::discriminant(state) {
                continue;
            }
            if piece.p1.1 < y {
                let mut before = piece.clone();
                before.p2.1 = y;
                before.rows.1 = row;
                out.push(before);
                piece.p1.1 = y;
            }
            piece.rows.0 = row;
            piece.state = state.clone();
            piece.title = title.clone();
        }
        out.push(piece);
    }
    out
}

/// Push a `BranchSeg` for the given centerline endpoints + state
/// only when the state classifies as renderable (not Empty).
fn push_seg_if_renderable(
    segs: &mut Vec<BranchSeg>,
    rap: &ResourceAccessPoint,
    p1: (f64, f64),
    p2: (f64, f64),
    state: State,
    visualization_data: &VisualizationData,
    arm_label: &str,
    leg: Leg,
    row: usize,
    end_row: usize,
    path: &[(usize, usize)],
) {
    if !seg_drawn(rap, &state) {
        return;
    }
    // A dashed arm column (`Gray`): another arm's code runs on these
    // rows, so say what the dashes mean instead of repeating the
    // solid segment's text (#626).
    // The diagonals underline nothing, so they say where they lead
    // instead (#843).
    let inactive = matches!(
        state,
        State::FullPrivilege { s: LineState::Gray } | State::PartialPrivilege { s: LineState::Gray }
    );
    let title = match (inactive, leg) {
        (true, Leg::Split) => hover_messages::state_arm_split(arm_label),
        (true, Leg::Join) => hover_messages::state_arm_join(arm_label),
        (true, Leg::Column) => hover_messages::state_inactive_arm(arm_label),
        // Mutably lent out (#722): who holds the borrow on the row the
        // segment starts on (a join's, the row above the join).
        (false, _) if matches!(state, State::RevokedPrivilege { .. }) => {
            let row = if leg == Leg::Join { row.saturating_sub(1) } else { row };
            revoked_title(rap, visualization_data, row, path)
        }
        // `row`: the first source row the segment covers, for a `for`
        // loop around the conditional (#871).
        (false, _) => loop_iterator_title(&state, rap, visualization_data, row)
            .unwrap_or_else(|| timeline_segment_title(&state, rap, visualization_data, None)),
    };
    let inactive_arm = (inactive && leg == Leg::Column).then(|| arm_label.to_owned());
    segs.push(BranchSeg { p1, p2, state, title, inactive_arm, own_row: false, rows: (row, end_row) });
}

/// The y-range of code to underline while hovering a segment. A solid
/// vertical segment underlines the part that lies on the arm's own lines
/// (`own_y`: the timeline y of its first and last line). A code line's
/// text sits 5px below its timeline row, so the last line extends by 5.
/// A dashed segment runs beside the other arms' code, which this path
/// skips: it underlines those rows and not the arm's own, so hovering
/// the two stretches highlights separate code (#626). A diagonal (the
/// split's or the join's connector) and a solid segment of an arm with
/// no lines of its own give `None` (#369).
fn seg_underline_y(seg: &BranchSeg, own_y: Option<(f64, f64)>, split_row: f64, row_lines: Option<&[Option<usize>]>) -> Option<(f64, f64)> {
    if (seg.p1.0 - seg.p2.0).abs() > 1e-6 { return None; }
    let (top, bot) = (seg.p1.1.min(seg.p2.1), seg.p1.1.max(seg.p2.1));
    if state_is_dashed(&seg.state) {
        // Stay off the arm's own rows (rows are 30px apart; the hover
        // script matches a row within 2px of the range).
        // Nor the conditional's own line (an `if let`'s arms fork from it).
        let (mut top, mut bot) = (top.max(split_row + 3.0), bot);
        if let Some((own_top, own_bot)) = own_y {
            // Dashes on the arm's own rows (a value not acquired yet)
            // aren't beside another arm's code.
            if top >= own_top - 3.0 && bot <= own_bot + 3.0 { return None; }
            if top < own_top - 3.0 {
                bot = bot.min(own_top - 3.0);
            } else {
                top = top.max(own_bot + 3.0);
            }
        }
        let has_code = row_lines.is_none_or(|row_lines| {
            let row_zero = get_y_axis_pos(0) as f64;
            let first = ((top - 2.0 - row_zero) / LINE_SPACE as f64).ceil() as usize;
            let last = ((bot + 2.0 - row_zero) / LINE_SPACE as f64).floor() as usize;
            row_lines.get(first..=last).is_some_and(|rows| rows.iter().any(Option::is_some))
        });
        return (top < bot && has_code).then_some((top, bot));
    }
    let (own_top, own_bot) = own_y?;
    // A one-row arm's own stretch spans half a row either side of its row.
    if seg.own_row { return Some((own_top, own_bot + 5.0)); }
    let (top, bot) = (top.max(own_top), bot.min(own_bot));
    // Touching an arm's first or last row only at its end isn't beside it.
    if top >= bot { return None; }
    Some((top, if bot >= own_bot { own_bot + 5.0 } else { bot }))
}

/// Whether a segment is drawn dashed (another arm's code runs here, or
/// the variable has no value yet on this path).
fn state_is_dashed(state: &State) -> bool {
    matches!(
        state,
        State::FullPrivilege { s: LineState::Gray | LineState::Uninit }
        | State::PartialPrivilege { s: LineState::Gray }
    )
}

/// ` data-underline-y` / ` data-underline="none"` for one drawn line of
/// a segment (read by `insertUnderline` in helpers.js).
fn underline_attr(seg: &BranchSeg, own_y: Option<(f64, f64)>, split_row: f64, row_lines: Option<&[Option<usize>]>) -> String {
    match seg_underline_y(seg, own_y, split_row, row_lines) {
        Some((t, b)) => format!(" data-underline-y=\"{} {}\"", t, b),
        None => " data-underline=\"none\"".to_owned(),
    }
}

/// Group segments into maximal centerline-contiguous runs.
fn split_centerline_runs<'a>(segs: &'a [BranchSeg]) -> Vec<Vec<&'a BranchSeg>> {
    let mut runs: Vec<Vec<&BranchSeg>> = Vec::new();
    let mut current: Vec<&BranchSeg> = Vec::new();
    let mut last_p2: Option<(f64, f64)> = None;
    for seg in segs {
        if let Some(p) = last_p2 {
            let gap = (p.0 - seg.p1.0).abs() > 1e-6
                || (p.1 - seg.p1.1).abs() > 1e-6;
            if gap && !current.is_empty() {
                runs.push(std::mem::take(&mut current));
            }
        }
        current.push(seg);
        last_p2 = Some(seg.p2);
    }
    if !current.is_empty() { runs.push(current); }
    runs
}

/// Render one centerline-contiguous run: per-segment visible lines
/// (solid or hollow per state), plus bevel lines at any corners
/// between adjacent hollow segments where the perpendicular rotates
/// (e.g. diagonal leading meeting vertical body), plus a transparent
/// hover polygon per segment.
fn render_branch_run(
    rap: &ResourceAccessPoint,
    hash: u64,
    run: &[&BranchSeg],
    (branch, arm, run_index): (usize, usize, usize),
    own_y: Option<(f64, f64)>,
    split_row: f64,
    row_lines: Option<&[Option<usize>]>,
    stop_at: Option<(f64, f64)>,
) -> (String, Vec<String>) {
    let mut svg = String::new();

    // Classify each segment up front so the bevel pass can
    // consult both neighbors without re-evaluating the
    // classifier.
    let classified: Vec<(OwnerLine, bool, &BranchSeg)> = run.iter().map(|seg| {
        let style = determine_owner_line_styles(rap, &seg.state);
        let dashed = state_is_dashed(&seg.state);
        (style, dashed, *seg)
    }).collect();

    // A stretch is a maximal run of segments that are all dashed or all
    // not. The caller wraps each stretch in its own `branch-group`, so
    // hovering the dashed part of an arm's column glows only that part,
    // not the solid part below it (#626).
    let mut chunk_of: Vec<usize> = Vec::with_capacity(classified.len());
    for (i, (_, dashed, _)) in classified.iter().enumerate() {
        let c = if i == 0 { 0 } else if classified[i - 1].1 == *dashed { chunk_of[i - 1] } else { chunk_of[i - 1] + 1 };
        chunk_of.push(c);
    }
    let n_chunks = chunk_of.last().map_or(0, |c| c + 1);
    let mut lines: Vec<String> = vec![String::new(); n_chunks];
    let mut polys: Vec<String> = vec![String::new(); n_chunks];

    // The `data-rv-*` attributes of a segment's nodes: its hover
    // polygon and its lines belong to one segment of this branch event.
    // Different forks on one row can have identical endpoints.
    let seg_rv = |seg: &BranchSeg, index: usize| Rv::new(rv_attrs::BRANCH_LINE).state(&seg.state).var(rap.name())
        .rows(seg.rows.0, seg.rows.1)
        .at(format!("{branch} {arm} {run_index} {index}"));

    // Visible lines per segment.
    for (i, (style, dashed, seg)) in classified.iter().enumerate() {
        let svg = &mut lines[chunk_of[i]];
        let dasharray = if *dashed { "4 4" } else { "none" };
        match style {
            OwnerLine::Solid => svg.push_str(&format!(
                "            <line data-hash=\"{}\" class=\"solid tooltip-trigger\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" data-tooltip-text=\"{}\"{}{} style=\"stroke-dasharray: {};\"/>\n",
                hash, seg.p1.0, seg.p1.1, seg.p2.0, seg.p2.1, seg.title, seg_rv(seg, i).part("line").attrs(), underline_attr(seg, own_y, split_row, row_lines), dasharray,
            )),
            OwnerLine::Hollow => {
                let perp = perp_unit(seg.p1, seg.p2);
                for (sign, part) in [(1.0_f64, "side1"), (-1.0, "side2")] {
                    let ox = perp.0 * sign * BRANCH_HALF_W;
                    let oy = perp.1 * sign * BRANCH_HALF_W;
                    svg.push_str(&format!(
                        "            <line data-hash=\"{}\" class=\"hollow tooltip-trigger\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" data-tooltip-text=\"{}\"{}{} style=\"stroke-dasharray: {};\"/>\n",
                        hash, seg.p1.0 + ox, seg.p1.1 + oy, seg.p2.0 + ox, seg.p2.1 + oy, seg.title, seg_rv(seg, i).part(part).attrs(), underline_attr(seg, own_y, split_row, row_lines), dasharray,
                    ));
                }
            }
            // Mutably lent out (#722); the segment's hover polygon below
            // takes the hover.
            OwnerLine::Dotted => svg.push_str(&format!(
                "            <line data-hash=\"{}\" class=\"lent tooltip-trigger\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" data-tooltip-text=\"{}\"{}{}/>\n",
                hash, seg.p1.0, seg.p1.1, seg.p2.0, seg.p2.1, seg.title, seg_rv(seg, i).part("line").attrs(), underline_attr(seg, own_y, split_row, row_lines),
            )),
            OwnerLine::Empty => {} // unreachable: filtered before push
        }
    }

    // Round joint. A thick solid column is drawn as one butt-capped
    // `<line>` per segment, so where the direction changes (a split
    // diagonal meeting the arm's vertical, or the reverse at a join)
    // the outer corner of the bend is left uncovered: a wedge-shaped
    // notch. A zero-length line with a round cap at the shared point
    // is a disc of the stroke's own width, which closes the notch
    // without lengthening either segment (so run ends and dash
    // phases are untouched). It is purely decorative: not a tooltip
    // trigger (each segment already has its own hover area), and it
    // sits on the bend point, which can lie on another arm's row. Thin hollow columns have their own
    // bevel pass below.
    for window in classified.windows(2) {
        let (prev_style, _, prev_seg) = &window[0];
        let (curr_style, _, curr_seg) = &window[1];
        if !matches!(prev_style, OwnerLine::Solid) { continue; }
        if !matches!(curr_style, OwnerLine::Solid) { continue; }
        let a = (prev_seg.p2.0 - prev_seg.p1.0, prev_seg.p2.1 - prev_seg.p1.1);
        let b = (curr_seg.p2.0 - curr_seg.p1.0, curr_seg.p2.1 - curr_seg.p1.1);
        if (a.0 * b.1 - a.1 * b.0).abs() < 1e-6 { continue; }
        svg.push_str(&format!(
            "            <line data-hash=\"{}\" class=\"solid\" data-underline=\"none\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" style=\"stroke-linecap: round; pointer-events: none;\"/>\n",
            hash, prev_seg.p2.0, prev_seg.p2.1, prev_seg.p2.0, prev_seg.p2.1,
        ));
    }

    // Bevel pass. Adjacent hollow segments with different
    // perpendicular directions (a diagonal converge meeting a
    // vertical body, or vice versa) leave a small gap on each side
    // because the offset endpoints don't quite touch — the
    // diagonal's perp rotates relative to the body's. Emit a small
    // connector line on each side bridging them. The connector
    // inherits the previous segment's dasharray so a dashed
    // leading-into-body transition reads as "the leading ends
    // here". Solid segments share a centerline with their
    // neighbors and need no bevel.
    for (wi, window) in classified.windows(2).enumerate() {
        let svg = &mut lines[chunk_of[wi]];
        let (prev_style, prev_dashed, prev_seg) = &window[0];
        let (curr_style, _, curr_seg) = &window[1];
        if !matches!(prev_style, OwnerLine::Hollow) { continue; }
        if !matches!(curr_style, OwnerLine::Hollow) { continue; }
        let prev_perp = perp_unit(prev_seg.p1, prev_seg.p2);
        let curr_perp = perp_unit(curr_seg.p1, curr_seg.p2);
        let dasharray = if *prev_dashed { "4 4" } else { "none" };
        for (sign, part) in [(1.0_f64, "bevel1"), (-1.0, "bevel2")] {
            let prev_off = (
                prev_seg.p2.0 + prev_perp.0 * sign * BRANCH_HALF_W,
                prev_seg.p2.1 + prev_perp.1 * sign * BRANCH_HALF_W,
            );
            let curr_off = (
                curr_seg.p1.0 + curr_perp.0 * sign * BRANCH_HALF_W,
                curr_seg.p1.1 + curr_perp.1 * sign * BRANCH_HALF_W,
            );
            if (prev_off.0 - curr_off.0).abs() > 1e-6
                || (prev_off.1 - curr_off.1).abs() > 1e-6
            {
                // The connector underlines nothing (#843).
                let bevel_title = prev_seg.inactive_arm.as_deref()
                    .map_or_else(|| prev_seg.title.clone(), hover_messages::state_inactive_arm_unlit);
                svg.push_str(&format!(
                    "            <line data-hash=\"{}\" class=\"hollow tooltip-trigger\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" data-tooltip-text=\"{}\"{} data-underline=\"none\" style=\"stroke-dasharray: {};\"/>\n",
                    hash, prev_off.0, prev_off.1, curr_off.0, curr_off.1, bevel_title, seg_rv(prev_seg, wi).part(part).attrs(), dasharray,
                ));
            }
        }
    }

    // Hover polygons, one per segment, on top of the lines. They're
    // what the mouse hovers: transparent with `pointer-events:fill`,
    // and as wide as a hollow strip (±BRANCH_HALF_W) even where the
    // visible line is a solid centreline. Each shows its own
    // segment's tooltip (#388: a single polygon over the whole run
    // showed the run's first segment's text, e.g. "s is the owner"
    // over the rows where `s` is borrowed) and underlines its own
    // lines: the part of a vertical segment on the arm's own lines
    // (`seg_underline_y`); a diagonal, a segment beside another arm's
    // code, or an arm with no lines of its own, nothing (#369).
    for (si, seg) in run.iter().enumerate() {
        let svg = &mut polys[chunk_of[si]];
        let (dx, dy) = (seg.p2.0 - seg.p1.0, seg.p2.1 - seg.p1.1);
        if dx.abs() < 1e-6 && dy.abs() < 1e-6 { continue; }
        let mut quad = seg_quad(seg);
        // The segment that ends at the stop bar leaves the bar's rows to
        // the bar's own polygon, so no polygon shows the other's tooltip.
        // A split diagonal that ends at the bar (a one-row arm, #1295)
        // is pulled back along itself, far enough that its slanted end
        // clears the bar's rows too.
        if stop_at == Some(seg.p2) {
            let len = (dx * dx + dy * dy).sqrt();
            let (ux, uy) = (dx / len, dy / len);
            let slant = BRANCH_HALF_W * ux.abs();
            let back = (STOP_BAR_HOVER_HALF_H - 1.0 + slant) / uy.abs().max(1e-6);
            if dy.abs() > 2.0 * (STOP_BAR_HOVER_HALF_H + slant) && back < len {
                quad[1].0 -= ux * back;
                quad[1].1 -= uy * back;
                quad[2].0 -= ux * back;
                quad[2].1 -= uy * back;
            }
        }
        let pts_str = quad.iter()
            .map(|p| format!("{},{}", p.0, p.1))
            .collect::<Vec<_>>()
            .join(" ");
        let underline = underline_attr(seg, own_y, split_row, row_lines);
        svg.push_str(&format!(
            "            <polygon data-hash=\"{}\" class=\"tooltip-trigger\" points=\"{}\" data-tooltip-text=\"{}\"{}{} style=\"fill:transparent; stroke:none; pointer-events:fill;\"/>\n",
            hash, pts_str, seg.title, seg_rv(seg, si).attrs(), underline,
        ));
    }

    let stretches = lines.iter().zip(&polys).map(|(l, p)| format!("{}{}", l, p)).collect();
    (svg, stretches)
}

/// How far below a dot on its row a diverging arm's stop bar is drawn:
/// half a row, clear of the dot (radius 5) and of a call's `f` logo
/// drawn on it, which hangs some 12px below its centre (#1375).
const STOP_BAR_BELOW_DOT: f64 = 15.0;

/// Half the height of the hover polygon over a stop bar (the bar's 3px
/// stroke, with room for its round caps).
const STOP_BAR_HOVER_HALF_H: f64 = 3.5;

/// The strip of half-width `BRANCH_HALF_W` around one segment's
/// centreline, as a polygon's four corners.
fn seg_quad(seg: &BranchSeg) -> [(f64, f64); 4] {
    let (px, py) = perp_unit(seg.p1, seg.p2);
    let (ox, oy) = (px * BRANCH_HALF_W, py * BRANCH_HALF_W);
    [
        (seg.p1.0 + ox, seg.p1.1 + oy),
        (seg.p2.0 + ox, seg.p2.1 + oy),
        (seg.p2.0 - ox, seg.p2.1 - oy),
        (seg.p1.0 - ox, seg.p1.1 - oy),
    ]
}

fn empty_panel_pair() -> (TimelinePanelData, TimelinePanelData) {
    let blank = || TimelinePanelData {
        labels: String::new(), dots: String::new(),
        timelines: String::new(), ref_line: String::new(),
        arrows: String::new(),
    };
    (blank(), blank())
}

// render timeline given a hash
fn render_timeline(
    hash: &u64,
    rap: &ResourceAccessPoint,
    history: &Vec<(usize, Event)>,
    states: &Vec<(usize, usize, State)>,
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    timeline_data: &TimelineColumnData,
    registry: &Handlebars,
    dots: &DotLayer,
    bars: &mut ColumnMarks,
) {
    if rap.is_fn() { return; } // functions have no timelines
    // Branch columns + convergences render as polylines via
    // `render_branches`; the per-state loop below only emits
    // column lines for the parent timeline outside the branch
    // ranges (clean_states leaves an OOS placeholder over the
    // branch's split..merge span, so those rows naturally render
    // as nothing).
    render_branches(hash, rap, history, states, output, visualization_data, timeline_data, false, &[], dots, bars);

    let apexes = CollapsedApexes::of(history);
    let owner = !rap.is_ref() && !rap.is_closure();
    let cuts = if owner { Vec::new() } else { field_borrow_rows(rap, history, visualization_data) };
    // A reference column's line reads the table for what is borrowed
    // through it (P1-4b; where its line runs and stops stays 2.x's until
    // its ends come from the facts).
    let table_ref_rows = rap.is_ref()
        && visualization_data.fn_source_lines.get(rap.hash()).is_some_and(|l| visualization_data.state_table.has_fn(*l));
    // The rows of the column's own dots that change its state (not its
    // scope end, which comes after the state a line leaves).
    let own_dots: Vec<usize> = history.iter()
        .filter(|(_, e)| !matches!(e, Event::Branch { .. } | Event::OwnerGoOutOfScope { .. } | Event::RefGoOutOfScope { .. }))
        .map(|(l, _)| *l)
        .collect();
    // The rows of one-line conditionals (split to merge; below the split,
    // which may be the blank row above, all one source line), any column's.
    let line = |r: usize| rv_attrs::line_of(r, &visualization_data.row_lines);
    let one_line_conds: Vec<(usize, usize)> = visualization_data.timelines.values()
        .flat_map(|t| t.history.iter())
        .filter_map(|(_, e)| match e {
            Event::Branch { split_point, merge_point, .. } if line(split_point + 1).is_some() && line(split_point + 1) == line(*merge_point) => Some((*split_point, *merge_point)),
            _ => None,
        })
        .collect();
    // A row of a one-line conditional's that its arms' rows put above this
    // column's own dot on that line, at or below the join.
    let above_own_dot = |row: usize| {
        one_line_conds.iter().any(|&(s, m)| s <= row && row < m && line(row) == line(m)
            && own_dots.iter().any(|&d| d >= m && line(d) == line(row)))
    };
    // A source row's table cell describes the state after all of its
    // stores. Between two stores, retain the lender state up to the
    // release dot placed at the later MIR effect.
    let field_releases: Vec<usize> = history.iter().filter_map(|(at, ev)| {
        (owner && matches!(ev, Event::StaticReacquire { .. } | Event::MutableReacquire { .. })
            && visualization_data.row_layout.later_field_effects.contains(&ev.get_id())).then_some(*at)
    }).collect();
    let mut trunk_ended = false;
    for (seg_start, seg_end, state) in states {
        let short_circuit_join = table_ref_rows && matches!(state, State::ResourceMoved { .. })
            && history.iter().any(|(_, e)| {
                matches!(e, Event::Branch { merge_point, .. } if merge_point == seg_start)
                    && visualization_data.row_layout.short_circuit_forks.contains(&e.get_id())
            });
        if rap.is_closure() && history.iter().any(|(at, e)| at == seg_end
            && matches!(e, Event::LoopExitMove { definite: true, at: MaybeMovedAt::LabeledBlock { runtime_only: true, .. } }))
            && visualization_data.fn_source_lines.get(hash).zip(line(*seg_start))
                .and_then(|(&function, src)| visualization_data.state_table.next_row_runs(function, src)) == Some(false)
        {
            // This connector follows the arm that would skip consuming
            // the closure. That arm's continuation is dead at runtime.
            continue;
        }
        let mut bounds = vec![*seg_start];
        if owner {
            // OOS includes the placeholder occupied by the arm legs.
            if matches!(state, State::OutOfScope | State::ResourceMoved { dropped_by_hand: true, .. }) {
                continue;
            }
            // An owner line normally ends at its move dot. Moving just a
            // Box's payload leaves the Box itself until its scope-end dot.
            if matches!(state, State::ResourceMoved { .. }) && !(
                !history.iter().any(|(_, e)| matches!(e, Event::Branch { .. }))
                && history.iter().filter(|(l, _)| l == seg_start).all(|(_, e)| matches!(e, Event::Move { .. }))
                && history.iter().any(|(l, e)| l == seg_end && matches!(e, Event::OwnerGoOutOfScope { early: None, .. }))
            ) { continue; }
            bounds.extend(seg_start + 1..*seg_end);
        } else {
            bounds.extend(cuts.iter().copied().filter(|l| l > seg_start && l < seg_end));
            if matches!(state, State::RevokedPrivilege { .. }) {
                bounds.extend(ref_move_rows(visualization_data, *seg_start, *seg_end));
            }
            // (A reference column's: after a row's last stacked sub-row of
            // dots, not inside them; the table's state is after the row's
            // events.)
            let lined = matches!(state, State::FullPrivilege { .. } | State::PartialPrivilege { .. } | State::RevokedPrivilege { .. });
            if table_ref_rows && lined {
                let rl = &visualization_data.row_lines;
                bounds.extend((seg_start + 1..*seg_end).filter(|r| rl.get(r + 1).is_none_or(Option::is_some)));
            }
        }
        bounds.sort_unstable();
        bounds.dedup();
        bounds.push(*seg_end);
        // (Pieces with the same state and text are drawn as one.)
        let mut pieces: Vec<(usize, usize, State, String, Option<i64>)> = Vec::new();
        let between_own_dots = owner && line(*seg_start).is_some_and(|src| line(*seg_end) == Some(src))
            && own_dots.contains(seg_start) && own_dots.contains(seg_end);
        for w in bounds.windows(2) {
            let mut restart_y = None;
            let before_field_release = field_releases.iter().any(|&at| w[0] < at && line(w[0]) == line(at));
            let (st, title) = if before_field_release || above_own_dot(w[0]) {
                // A row of a one-line conditional's arms above this column's
                // own dot on that line (`w.push((if f { r } else { &b }, &c))`
                // borrows c after the conditional): the table's state is the
                // one after the line, not yet in effect. The layout's stands.
                if owner && matches!(state, State::ResourceMoved { .. }) {
                    continue;
                }
                (state.clone(), timeline_segment_title(state, rap, visualization_data, Some(w[0])))
            } else if owner {
                let piece = owner_segment(rap, visualization_data, w[0], state, between_own_dots);
                if piece.is_none() && matches!(rap, ResourceAccessPoint::Struct(Struct { is_box: false, .. })) {
                    trunk_ended |= visualization_data.fn_source_lines.get(hash).zip(line(w[0]))
                        .is_some_and(|(f, src)| trunk_ends_at_fields(visualization_data, *f, rap.name(), w[0], src));
                }
                if trunk_ended && piece.is_some() {
                    // Resume a trunk from the event on it or on one of
                    // its fields, e.g. the join of a labeled block that
                    // can break before moving the last field.
                    let function = visualization_data.fn_source_lines[hash];
                    restart_y = dots.trunk_restart_y(w[0], *hash, timeline_data.owner as i64,
                        |column| visualization_data.state_table.is_within(function, column, rap.name()));
                    trunk_ended = restart_y.is_none();
                    if trunk_ended { continue; }
                }
                let Some(piece) = piece else { continue };
                piece
            } else {
                let maybe_moved = short_circuit_join && visualization_data.fn_source_lines.get(hash).zip(line(w[0]))
                    .and_then(|(&function, row)| visualization_data.state_table.segment_cell(function, rap.name(), row))
                    .is_some_and(|c| c.init == crate::facts::table::Init::Maybe && c.cause == Some("moved"));
                match table_ref_rows.then(|| table_lent_state(state, rap, visualization_data, w[0], &[])).flatten() {
                    Some((st, Some(title))) => (st, title),
                    Some((st, None)) => {
                        let t = timeline_segment_title(&st, rap, visualization_data, Some(w[0]));
                        (st, t)
                    }
                    None if maybe_moved => (state.clone(), hover_messages::state_maybe(rap.name(), Some("moved"))),
                    None => (state.clone(), timeline_segment_title(state, rap, visualization_data, Some(w[0]))),
                }
            };
            let title = if visualization_data.approx_notes.iter().any(|n| matches!(n, ApproxNote::UnreachedValue { hash: h } if h == hash)) {
                format!("{} is never initialized because a preceding closure call never returns", styled_name_html(rap.name()))
            } else { title };
            match pieces.last_mut() {
                Some(last) if restart_y.is_none() && last.1 == w[0] && last.3 == title && std::mem::discriminant(&last.2) == std::mem::discriminant(&st) => last.1 = w[1],
                _ => pieces.push((w[0], w[1], st, title, restart_y)),
            }
        }
        for (line_start, line_end, piece_state, title, restart_y) in pieces {
            let state = &piece_state;
            // Every part was moved out: nothing is left to draw (#1157).
            if title.is_empty() {
                continue;
            }
            let Some((mut y1, mut y2)) = apexes.segment_ys(line_start, line_end, restart_y) else { continue };
            // A reference column's pieces stay inside the stretch 2.x draws
            // for the segment (a row's stacked sub-rows of dots below it
            // aren't part of it). A segment drawn whole is drawn as 2.x did.
            if table_ref_rows && (line_start, line_end) != (*seg_start, *seg_end) {
                let Some((s1, s2)) = apexes.segment_ys(*seg_start, *seg_end, None) else { continue };
                (y1, y2) = (y1.max(s1), y2.min(s2));
                if y2 <= y1 {
                    continue;
                }
            }
            let mut data = VerticalLineData {
                    line_class: String::new(),
                    hash: *hash,
                    x1: timeline_data.x_val as f64,
                    y1,
                    x2: timeline_data.x_val as f64,
                    y2,
                    title,
                    opacity: 1.0,
                    dasharray: "none".to_owned(),
                    rv: Rv::new(rv_attrs::LINE).state(state).var(&timeline_data.name)
                        .rows(line_start, line_end).attrs(),
            };
            if append_line(state, & mut data, rap, timeline_data, output, registry) {
                // For `untangle`: a head coming steeply into a dot along
                // the column hides it (#1488).
                bars.cols.push(((data.x1, y1 as f64), (data.x2, y2 as f64)));
            }
        }
    }
}

/// The straight segment for an owner or a field. The replay still lays
/// out its column and the space occupied by branch legs; the segment's
/// text comes from the state table's cell where the table has one. That
/// text keeps every clause 2.x's title has (owner, copyable value, holds a
/// reference, borrowed by ...) and adds or corrects only what the cell
/// knows (who holds a borrow, on some paths only, moved parts).
fn owner_segment(rap: &ResourceAccessPoint, vd: &VisualizationData, row: usize, layout: &State, between_own_dots: bool) -> Option<(State, String)> {
    // The row-end snapshot can include events after this interval. Replay
    // has already applied the current dot and has yet to apply the next one.
    if between_own_dots {
        let (state, mut title) = owner_segment_of(rap, vd, row, layout, None, None)?;
        if matches!(state, State::PartialPrivilege { s: LineState::Full }) && !rap.holds_value() && !rap.holds_borrow() {
            title = format!("{}. {title}", hover_messages::state_full_privilege(rap.name()));
        }
        return Some((state, title));
    }
    let fn_line = *vd.fn_source_lines.get(rap.hash())?;
    let src = rv_attrs::line_of(row, &vd.row_lines)?;
    // (A fn written on one line has no state between its rows: below a
    // one-line conditional's arms, the arms joined.)
    let cell = vd.state_table.segment_cell(fn_line, rap.name(), src)
        .or_else(|| vd.state_table.arms_joined_cell(fn_line, rap.name(), src));
    owner_segment_of(rap, vd, row, layout, cell, None)
}

/// `owner_segment` from the cell `cell` (the one `segment_cell` reads, or
/// a branch leg's: its row's own on the arm's path, P1-6).
fn owner_segment_of(rap: &ResourceAccessPoint, vd: &VisualizationData, row: usize, layout: &State, cell: Option<crate::facts::table::Cell>, imaginary: Option<crate::facts::ImaginaryEdgeId>) -> Option<(State, String)> {
    use crate::facts::table::Init;
    let name = rap.name();
    let table = &vd.state_table;
    let fn_line = *vd.fn_source_lines.get(rap.hash())?;
    let src = rv_attrs::line_of(row, &vd.row_lines)?;
    let legacy = || Some((layout.clone(), timeline_segment_title(layout, rap, vd, Some(row))));
    // No MIR place for it (a temporary, a static, an async fn's
    // parameter, a `(parameter)` column) or no state here: 2.x's title.
    let Some(mut cell) = cell else {
        if matches!(layout, State::ResourceMoved { .. }) {
            return None;
        }
        return legacy();
    };
    // A field the layout draws moved out on this row, by a statement that
    // MIR finishes on a row below (`..user1` in a struct literal that ends
    // at `};`): the struct's line says so from the row of the field's move
    // dot, as its arrow does (the state where that statement ends).
    if imaginary.is_none() && matches!(rap, ResourceAccessPoint::Struct(_)) && cell.init == Init::Own
        && cell.parts.as_ref().is_none_or(|p| p.moved.is_empty() && p.maybe.is_empty())
        && field_drawn_moved(vd, fn_line, name, row)
    {
        if let Some(c) = (src + 1..).take_while(|l| *l <= src + 8)
            .find_map(|l| table.cell(fn_line, name, l, l).filter(|c| c.from_row == l))
            .filter(|c| c.init == Init::Partly || c.parts.as_ref().is_some_and(|p| !p.moved.is_empty() || !p.maybe.is_empty()))
        {
            cell = c;
        }
    }
    let full = State::FullPrivilege { s: LineState::Full };
    if matches!(layout, State::ResourceMoved { .. }) && !(cell.init == Init::Partly && table.is_box(fn_line, name)) {
        return None;
    }
    if imaginary.is_none() && cell.init == Init::Partly
        && matches!(rap, ResourceAccessPoint::Struct(Struct { is_box: false, .. }))
        && trunk_ends_at_fields(vd, fn_line, name, row, src)
    {
        return None;
    }
    // The line holds a value where the cell says it may have none. A cell
    // that says "maybe moved" where no row moves the place is where its
    // scope ends
    // (a `StorageDead` counts as a move in MIR's move data); "maybe
    // unassigned" for a variable never without a value in its scope is
    // read where the paths that never bound it join. 2.x's title
    // stands for these lines: a solid line saying "moved" would contradict
    // its own drawing. (A place moved or unassigned on every path, `let p
    // = P {` .. `};` before its `};`, or moved on some paths only, or by
    // the borrow checker's reckoning only, is the table's correction, Q5.)
    let held = matches!(layout, State::FullPrivilege { s: LineState::Full } | State::PartialPrivilege { .. } | State::RevokedPrivilege { .. });
    let artifact = match cell.init {
        Init::Unassigned | Init::Moved => false,
        Init::Maybe => match cell.cause {
            Some("moved") => !table.has_move(fn_line, name, &cell),
            // (A pattern's binding read where its block ends, joined with
            // the way past the pattern, on which it never came into scope.)
            Some("unassigned") => !cell.always_assigned && !table.unassigned_above(fn_line, name, src, &cell),
            _ => false,
        },
        Init::Own | Init::Partly => false,
    };
    if imaginary.is_none() && held && artifact {
        return legacy();
    }
    match cell.init {
        // The segment still connects the layout's dots (a pattern move
        // can be marked on the binding, below its MIR move). Its title
        // describes the cell at the source boundary.
        Init::Moved => return Some((layout.clone(), hover_messages::state_moved_out(name, rap.is_ref() || rap.holds_value()))),
        Init::Unassigned => return Some((layout.clone(), hover_messages::state_uninitialized(name))),
        Init::Maybe => {
            let title = if cell.mixed_init.is_some() {
                hover_messages::state_maybe_init(name, &cell)
            } else if cell.cause == Some("imaginary") && !rap.holds_value() && !rap.is_ref() {
                format!("{}. {}", timeline_segment_title(&full, rap, vd, Some(row)), hover_messages::state_maybe(name, cell.cause))
            } else if cell.always_assigned && cell.cause == Some("unassigned") {
                hover_messages::state_imaginary_unassigned(name)
            } else {
                hover_messages::state_maybe(name, cell.cause)
            };
            // May have no value (the borrow checker's view, Q5: `let value;`
            // assigned only in an irrefutable `if let`'s arm, #1867): it
            // can't be used, so its line is dashed, as 2.x draws a local
            // some branches leave unassigned, not solid.
            if matches!(cell.cause, Some("unassigned" | "moved" | "imaginary")) && held {
                return Some((State::FullPrivilege { s: LineState::Uninit }, title));
            }
            return Some((layout.clone(), title));
        }
        Init::Own | Init::Partly => {}
    }
    if let Some(taken) = &cell.manually_taken {
        let mut title = hover_messages::state_manually_taken(name, &taken.inner, taken.maybe, rap.is_copy_owner());
        let wrapper = if cell.lent.is_empty() {
            full
        } else {
            let (state, borrow, _) = owner_loan_title(rap, vd, fn_line, &cell, layout, row, false);
            title.push(' ');
            title.push_str(&borrow);
            state
        };
        return Some((State::ManuallyTaken { wrapper: Box::new(wrapper) }, title));
    }
    // A live loan of a part (a field, an element, a capture of one) restricts
    // the whole owner column, worded by the part (P1-3b).
    let part_loan = imaginary.is_none().then(|| owner_part_loan_state(
        if matches!(layout, State::ResourceMoved { .. }) { &full } else { layout }, rap, vd, row,
    )).flatten();
    let part_loan_style = match part_loan {
        Some((st, PartLoanTitle::Text(title))) => return Some((st, title)),
        Some((st, PartLoanTitle::Legacy)) => {
            let title = timeline_segment_title(&st, rap, vd, Some(row));
            return Some((st, title));
        }
        Some((st, PartLoanTitle::OwnerLoan)) => Some(st),
        None => None,
    };
    let parts = if let Some(entry) = imaginary.and_then(|edge| table.imaginary_arm(edge)).map(|arm| &arm.entry) {
        partly_moved_cell_title(rap, table, fn_line, src, &cell, &|name| entry.get(name).cloned())
    } else {
        table_partly_moved_title(rap, table, fn_line, src, &cell)
    };
    let whole_title = parts.is_none() && cell.init == Init::Own;
    let mut title = parts.unwrap_or_else(|| {
        if cell.init == Init::Partly {
            hover_messages::state_unnamed_part(name, cell.unnamed_maybe || cell.may)
        } else {
            owner_value_title(rap, vd, fn_line, &cell, Some(row))
        }
    });
    if cell.lent.is_empty() {
        return Some((part_loan_style.unwrap_or(full), title));
    }
    let (state, mut loan_title, whole) = owner_loan_title(rap, vd, fn_line, &cell, layout, row, imaginary.is_some());
    if imaginary.is_none() {
        if let Some(holder) = table.shared_drop_holder(fn_line, name, &cell) {
            append_drop_holder(&mut loan_title, name, holder);
        }
    }
    if !whole_title || !whole || cell.option_ref_copy().is_some() {
        // The value's own loans and a borrow of the value can coexist.
        title.push_str(". ");
        title.push_str(&loan_title);
    } else {
        title = loan_title;
    }
    // (A loan of an enum's payload restricts the owner's line, P1-3b.)
    Some((part_loan_style.unwrap_or(state), title))
}

/// Keep the sharing explanation and explain the drop obligation.
fn append_drop_holder(title: &mut String, name: &str, holder: &crate::facts::schema::DropLoanHolder) {
    let start = if holder.some_paths { ". On some paths, the" } else { ". The" };
    let clause = format!("{start} borrow checker keeps {} borrowed until the old value of {} is dropped",
        styled_name_html(name), styled_name_html(&holder.name));
    if !title.contains(&clause) {
        title.push_str(&clause);
    }
}

/// Keep the ordinary owner explanation on the connector from a proven
/// fieldless edge. The two columns are the lender and its former holder.
fn fieldless_join_title(vd: &VisualizationData, rap: &ResourceAccessPoint, event: &Event,
    arm: usize, branch: &BranchData) -> Option<String> {
    let Event::Branch { id, join, .. } = event else { return None };
    if !vd.row_layout.fieldless_lenders.iter().any(|(&(b, a, lender), holder)|
        b == *id && a == arm && (*rap.hash() == lender || rap.hash() == holder)) { return None; }
    let function = *vd.fn_source_lines.get(rap.hash())?;
    let source = branch.source?;
    let table = &vd.state_table;
    let cell = table.arm_boundary_cell(function, rap.name(), source, true).cloned()
        .or_else(|| join.as_ref().map(|j| j.cell.clone()))
        .or_else(|| {
            let end = branch.source_span?.end.0;
            table.source_arm_cell(function, rap.name(), end, source)
                .or_else(|| table.cell(function, rap.name(), end, end))
        })?;
    (cell.init == crate::facts::table::Init::Own && cell.parts.is_none() && !cell.unnamed_maybe
        && !cell.may_lend && cell.lent.is_empty()).then(|| owner_value_title(rap, vd, function, &cell, None))
}

/// The ownership clause for a cell with its whole value.
fn owner_value_title(rap: &ResourceAccessPoint, vd: &VisualizationData, fn_line: FunctionStart, cell: &crate::facts::table::Cell, row: Option<usize>) -> String {
    let name = rap.name();
    if let Some(copy) = cell.option_ref_copy()
        .or_else(|| vd.row_layout.closure_result_holders.contains(rap.hash()).then_some(rap.is_copy_owner()))
    {
        let mut lenders = BTreeSet::new();
        let mut temporary = false;
        for loan in cell.holds.iter().chain(&cell.option_holds)
            .filter_map(|&id| vd.state_table.loan(fn_line, id))
        {
            match loan.source {
                "borrow" => { lenders.insert(Some(loan.projected.as_ref().map_or_else(|| styled_name_html(&loan.lender), hover_messages::projected_place_html))); }
                "temporary" => temporary = true,
                _ => {}
            }
        }
        return hover_messages::state_value_holds_loans(name, copy, &lenders.into_iter().collect::<Vec<_>>(), temporary);
    } else if cell.holder_type == Some(crate::facts::schema::HolderType::Primitive) {
        return hover_messages::state_full_privilege_copyable(name);
    } else if rap.holds_borrow() && !cell.borrowed_from.is_empty() {
        // An iterator can borrow data through raw pointers.
        let from = display_places(vd, fn_line, name, &cell.borrowed_from);
        if cell.holds.iter().any(|&id| vd.row_layout.uncertain_projected_stores.contains(&(*rap.hash(), id))) {
            let mut title = hover_messages::state_holder_starts_empty(name);
            title.push_str(&hover_messages::state_holder_tracks(name, &from));
            return title;
        }
        return hover_messages::state_holder_borrows_from(name, &from);
    }
    timeline_segment_title(&State::FullPrivilege { s: LineState::Full }, rap, vd, row)
}

/// Whether the layout draws a direct field of the struct column `name`
/// moved out on the visual row `row` (its segment from there on is a
/// moved one).
fn field_drawn_moved(vd: &VisualizationData, fn_line: FunctionStart, name: &str, row: usize) -> bool {
    let prefix = format!("{name}.");
    vd.timelines.values().any(|t| {
        let r = &t.resource_access_point;
        r.name().strip_prefix(&prefix).is_some_and(|f| !f.contains('.'))
            && vd.fn_source_lines.get(r.hash()).is_some_and(|start| start.body == fn_line.body)
            && t.states.iter().any(|(a, b, st)| *a <= row && row < *b && matches!(st, State::ResourceMoved { dropped_by_hand: false, .. }))
    })
}

/// The names 2.x's columns use for places the facts name (lenders and
/// holders of loans): the place itself where it is a column or a Rust
/// place (`s`, `h.r`, `*b`); for a part of an enum variant, `(o as
/// Some).0`, the place it is in (`o`); for a temporary the facts name by
/// its code, `(String::from("x"))`, its `(temporary N)` column where 2.x
/// draws one (None where it doesn't: "a temporary value").
fn display_place(vd: &VisualizationData, fn_line: FunctionStart, column: &str, place: &str) -> Option<String> {
    let in_fn = |n: &str| vd.timelines.values().any(|t| {
        t.resource_access_point.name() == n && vd.fn_source_lines.get(t.resource_access_point.hash()).is_some_and(|start| start.body == fn_line.body)
    });
    if !place.starts_with('(') {
        return Some(place.to_owned());
    }
    if in_fn(place) {
        return Some(place.to_owned());
    }
    // The parenthesized head and what follows it.
    let mut depth = 0usize;
    let close = place.char_indices().find_map(|(i, c)| {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        None
    })?;
    let (inner, rest) = (&place[1..close], &place[close + 1..]);
    if let Some((base, _variant)) = inner.rsplit_once(" as ") {
        return display_place(vd, fn_line, column, base);
    }
    let head = &place[..=close];
    let lines = vd.state_table.lender_lines(fn_line, head);
    let temps: BTreeSet<&str> = vd.timelines.values()
        .filter(|t| {
            let r = &t.resource_access_point;
            r.name().starts_with("(temporary") && !r.name().contains('.')
                && vd.fn_source_lines.get(r.hash()).is_some_and(|start| start.body == fn_line.body)
                && {
                    // (Its events in a branch's arms too: a temporary made
                    // in one arm.)
                    let mut events = Vec::new();
                    flat_history(&t.history, &[], &mut events);
                    events.iter().any(|(row, _)| rv_attrs::line_of(*row, &vd.row_lines).is_some_and(|l| lines.contains(&l)))
                }
        })
        .map(|t| t.resource_access_point.name().as_str())
        .collect();
    match temps.into_iter().collect::<Vec<_>>().as_slice() {
        [one] => Some(format!("{one}{rest}")),
        _ => None,
    }
}

/// `display_place` of each, sorted, without repeats.
fn display_places<'a, T: AsRef<str> + ?Sized + 'a>(vd: &VisualizationData, fn_line: FunctionStart, column: &str, places: impl IntoIterator<Item = &'a T>) -> Vec<Option<String>> {
    let set: BTreeSet<Option<String>> = places.into_iter().map(|p| {
        let p = p.as_ref();
        vd.state_table.projected_lender(fn_line, p).map(hover_messages::projected_place_html)
            .or_else(|| display_place(vd, fn_line, column, p).map(|p| styled_name_html(&p)))
    }).collect();
    set.into_iter().collect()
}

/// The style and title of an owner column's line while loans restrict it
/// (the cell's `lent`), and whether those loans borrow all of it (not a
/// field or an element only).
fn owner_loan_title(
    rap: &ResourceAccessPoint,
    vd: &VisualizationData,
    fn_line: FunctionStart,
    cell: &crate::facts::table::Cell,
    layout: &State,
    row: usize,
    frozen: bool,
) -> (State, String, bool) {
    let table = &vd.state_table;
    let name = rap.name();
    let full = State::FullPrivilege { s: LineState::Full };
    let exclusive = cell.lent.iter().any(|l| matches!(l.kind, "mut" | "unique"));
    let mut state = if exclusive {
        State::RevokedPrivilege { to: ResourceTy::Anonymous, borrow_to: ResourceTy::Anonymous, prev_state: Box::new(full) }
    } else {
        State::PartialPrivilege { s: LineState::Full }
    };
    let loans: Vec<_> = cell.lent.iter().filter(|l| matches!(l.kind, "mut" | "unique") == exclusive).collect();
    let borrowed: BTreeSet<_> =
        loans.iter().filter_map(|l| table.loan(fn_line, l.loan).map(|info| info.lender.as_str())).collect();
    let by: BTreeSet<_> = loans.iter().flat_map(|l| l.current.iter().map(String::as_str)).collect();
    let iterator = loans.iter().all(|l| l.iterator);
    let direct: BTreeSet<_> = loans
        .iter()
        .filter_map(|l| table.loan(fn_line, l.loan)?.direct_holder.as_deref())
        .filter(|h| by.contains(h))
        .collect();
    let candidate = if direct.len() == 1 { direct.first() } else { by.first().filter(|_| by.len() == 1) };
    let holder = candidate.copied().filter(|h| loans.iter().all(|l| l.current.contains(*h)));
    let shown_holder = holder.and_then(|h| display_place(vd, fn_line, name, h));
    let holder = holder.filter(|_| shown_holder.is_some());
    let holder_rap = |h: &str| vd.timelines.values().map(|t| &t.resource_access_point)
        .find(|r| r.name() == h && vd.fn_source_lines.get(r.hash()).is_some_and(|start| start.body == fn_line.body));
    // Named as the one holding the borrow only where the facts prove it:
    // what the holder borrows is this place only (`result` of `longest(a,
    // b)` may point into either), and a `&T` can't hold a mutable borrow
    // (`let r2 = id(r)` downgrades it). Otherwise the title says only that
    // the place stays borrowed while the holder is in use.
    // It holds it where the borrow was assigned to it directly (`let r =
    // &s;`, `&mut s` into a field), not through a call's result (`longest(a,
    // b)` may return either; `id(r)` downgrades a `&mut`).
    let exact = holder.is_some_and(|h| {
        // (A closure's capture is its own: it holds what it captured.)
        if holder_rap(h).is_some_and(|r| r.is_closure()) {
            return true;
        }
        let sources = cell.holder_sources.get(h);
        sources.is_some_and(|s| !s.is_empty() && s.iter().all(|x| borrowed.contains(x.as_str())))
            && loans.iter().all(|l| table.loan(fn_line, l.loan).is_some_and(|i| i.direct_holder.as_deref() == Some(h)))
            // (A `&T` downgraded from a `&mut` keeps a mutable loan live
            // but can't write: it isn't said to hold it.)
            && !(exclusive && holder_rap(h).is_some_and(|r| r.is_ref() && !r.is_mutref()))
    }) && !iterator;
    let exact_holder = shown_holder.as_deref().filter(|_| exact);
    let kept_by = shown_holder.as_deref().filter(|_| !exact && !iterator);
    let to = match exact_holder {
        _ if iterator => hover_messages::LentTo::LoopIterator,
        Some(h) if holder_rap(h).is_some_and(|r| r.is_closure()) => hover_messages::LentTo::Closure(h),
        Some(h) => hover_messages::LentTo::Var(h),
        None => hover_messages::LentTo::Unknown,
    };
    let whole = loans.iter().all(|l| table.loan(fn_line, l.loan).is_some_and(|i| i.whole.contains(name)));
    if !whole && !frozen && !matches!(layout, State::ResourceMoved { .. }) {
        state = layout.clone();
    }
    if cell.manually_taken.is_some() {
        let kind = if exclusive { "an exclusive" } else { "a shared" };
        let mut title = match exact_holder {
            Some(h) if !cell.may_lend => format!("{} holds {kind} borrow of the wrapper.", styled_name_html(h)),
            _ if cell.may_lend => format!("The wrapper may have {kind} borrow."),
            _ => format!("The wrapper has {kind} borrow."),
        };
        if let Some(h) = kept_by.filter(|_| !cell.may_lend) {
            title.push_str(&hover_messages::state_kept_borrowed_by(name, h));
        }
        return (state, title, whole);
    }
    let subject = borrowed.first().copied().filter(|_| borrowed.len() == 1);
    let possible_sources = holder.and_then(|h| cell.holder_sources.get(h));
    let read_only = whole && !rap.may_have_interior_mut();
    let drop_holder = table.shared_drop_holder(fn_line, name, cell);
    // 2.x's title where its replay drew the same kind of loan and explains
    // the paths itself: a place an or-pattern's reference may borrow
    // (#565), "borrowed by r if the alternative that binds it matched".
    let imaginary = loans.iter().all(|l| table.loan(fn_line, l.loan).is_some_and(|i| !i.runs));
    let same_kind = std::mem::discriminant(layout) == std::mem::discriminant(&state);
    let legacy = || timeline_segment_title(layout, rap, vd, Some(row));
    if same_kind && !imaginary && !frozen && !matches!(layout, State::ResourceMoved { .. }) && or_alt_lend_at(vd, rap, Some(row)).is_some() {
        return (layout.clone(), legacy(), true);
    }
    // The places lent, as 2.x names them; a part named as the column itself
    // (`(t.0 as Some).0` is shown as `t.0`) is a loan of it.
    let parts: BTreeSet<String> = borrowed.iter().filter_map(|p| display_place(vd, fn_line, name, p)).collect();
    let whole = whole || (!parts.is_empty() && parts.iter().all(|p| p == name));
    let reserved = loans.iter().any(|l| l.kind == "reserved") && !exclusive;
    let unique = loans.iter().any(|l| l.kind == "unique");
    let mut title = if imaginary {
        hover_messages::state_imaginary_borrow(name, exclusive)
    } else if !whole {
        // Only a part of it (a field, an element) is borrowed: the part is
        // named, whatever the loan's kind (`p.a` reserved by `p.a.insert(..)`
        // leaves `p.b` usable).
        let kind = if unique { "unique" } else if exclusive { "mutable" } else if reserved { "reserved" } else { "shared" };
        let parts = display_places(vd, fn_line, name, borrowed.iter().copied());
        let parts: Vec<_> = parts.into_iter().flatten().collect();
        hover_messages::state_lent_parts(name, &parts, exact_holder, kind, cell.may_lend)
    } else if reserved {
        let call =
            loans.iter().find(|l| l.kind == "reserved").and_then(|l| table.loan(fn_line, l.loan)?.to_call.as_deref());
        if cell.may_lend || !loans.iter().all(|l| l.kind == "reserved") {
            format!("The borrow checker restricts changes to {} here. A two-phase mutable borrow can still allow reads before the call starts", styled_name_html(name))
        } else {
            hover_messages::state_reserved(name, call)
        }
    } else if cell.may_lend && exclusive {
        if loans.iter().all(|l| l.kind == "mut") {
            hover_messages::state_maybe_lent_out(name, false)
        } else {
            hover_messages::state_maybe_exclusive(name, false)
        }
    } else if drop_holder.is_some_and(|holder| holder.some_paths) {
        format!("The borrow checker counts a shared borrow of {} here", styled_name_html(name))
    } else if cell.may_lend {
        hover_messages::state_maybe_shared(name, exact_holder, rap.holds_value(), read_only, false)
    } else if loans.iter().all(|l| l.kind == "unique") {
        hover_messages::state_unique_lent(name, to)
    } else if exclusive && unique {
        format!("The borrow checker requires exclusive access to {} until the borrow ends", styled_name_html(name))
    } else if let Some(parent) = subject.filter(|p| *p != name && rap.is_member()).and_then(|p| display_place(vd, fn_line, name, p)) {
        hover_messages::state_lent_with_parent(name, &parent, exact_holder, exclusive, read_only)
    } else if iterator {
        // 2.x's own loop-iterator title where it drew one; else 2.x's clause
        // and who holds the borrow (not "until the loop ends": a path that
        // leaves the loop early ends it sooner, and a `Cell` inside can
        // still be set).
        let old = legacy();
        if old.contains("loop's iterator") {
            old
        } else {
            hover_messages::state_borrowed_by_loop_iterator_holder(name, rap.holds_value(), exclusive)
        }
    } else if kept_by.is_some() && same_kind && !matches!(layout, State::ResourceMoved { .. }) {
        // A container can borrow reference slots as well as the places
        // they keep borrowed. The shared line alone doesn't name the
        // container. Keep its duration clause for those places (#1888).
        let borrows_slots = holder.and_then(holder_rap).is_some_and(|r| r.holds_borrow())
            && possible_sources.is_some_and(|sources| sources.iter().any(|p| holder_rap(p).is_some_and(|r| r.is_ref())));
        let stored = vd.row_layout.stored_lender_loans.get(rap.hash())
            .is_some_and(|ids| loans.iter().any(|l| ids.contains(&l.loan)));
        if let Some(h) = kept_by.filter(|h| exclusive && whole && possible_sources.is_some_and(|s| s.len() > 1)
            && !rap.is_ref() && replayed_lent_holder(rap, vd, row, &[]).holder.is_none()
            && holder_rap(h).is_some_and(|r| r.is_mutref())
            && loans.iter().all(|l| table.loan(fn_line, l.loan).is_some_and(|i| i.direct_holder.as_deref() == Some(*h))))
        {
            return (state, hover_messages::state_repointed_lender(name, h, rap.holds_value()), true);
        }
        let drops_holder = kept_by.is_some_and(|h| loans.iter().any(|l| table.kept_through_drop(fn_line, l.loan, h)));
        if !stored && !drops_holder && (exclusive || !borrows_slots) {
            return (layout.clone(), legacy(), true);
        }
        legacy()
    } else if exclusive {
        hover_messages::state_lent_out(name, to, lent_place(rap, false, false), None)
    } else {
        hover_messages::state_shared_by(name, exact_holder, rap.holds_value())
    };
    if let Some(h) = kept_by.filter(|_| whole && !imaginary && !cell.may_lend && drop_holder.is_none()) {
        title.push_str(&hover_messages::state_kept_borrowed_by(name, h));
    }
    if let Some((h, sources)) = shown_holder.as_deref().zip(possible_sources).filter(|(_, s)| cell.may_lend && s.len() > 1) {
        let sources = display_places(vd, fn_line, name, sources);
        if sources.len() > 1 {
            title.push_str(&hover_messages::state_loan_sources(h, &sources));
        }
    }
    let through: BTreeSet<_> = loans.iter().filter_map(|l| table.loan(fn_line, l.loan)?.through.as_deref()).collect();
    if let Some(r) = through.first().filter(|_| {
        through.len() == 1 && loans.iter().all(|l| table.loan(fn_line, l.loan).is_some_and(|i| i.through.is_some()))
    }).and_then(|r| display_place(vd, fn_line, name, r)) {
        title.push_str(&hover_messages::state_through(&r));
    }
    (state, title, whole)
}

/// RustViz 3 (P1-4b, reference columns): the row of the state table a
/// column's line after the visual row `row` reads (see P1-3's
/// `table_struct_row`, which P1-5 replaced for owners by `owner_segment`). `values_only`: the next row may borrow from the
/// column (only its value is read); otherwise it may do nothing to it.
fn table_row(rap: &ResourceAccessPoint, visualization_data: &VisualizationData, row: usize, values_only: bool) -> Option<(FunctionStart, usize)> {
    if rap.is_fn() || rap.is_closure() {
        return None;
    }
    let fn_line = *visualization_data.fn_source_lines.get(rap.hash())?;
    let src = rv_attrs::line_of(row, &visualization_data.row_lines)?;
    let table = &visualization_data.state_table;
    let brace = |l: usize| visualization_data.source_lines.get(l.wrapping_sub(1)).is_some_and(|t| t.trim_start().starts_with('}'));
    let own = |l: usize| table.cell(fn_line, rap.name(), l, l).is_some();
    // `} else if d {` opens an arm with code on its row (the condition):
    // its own state is that arm's way in. `} else {` has none (its state
    // would be the arm above's), and a row that closes more blocks than it
    // opens (`}`, `} else { drop(p.x); }`) ends where the paths join.
    let opens_with_code = |l: usize| visualization_data.source_lines.get(l.wrapping_sub(1)).is_some_and(|t| {
        let code = t.trim().trim_start_matches('}').trim_start().trim_start_matches("else").trim().trim_end_matches('{').trim();
        !code.is_empty() && t.matches('{').count() >= t.matches('}').count()
    });
    // A `}` row that does something to the variable itself, not as an
    // arm (`};` of a multi-line struct literal or call: its statement takes
    // effect there), has its own state too.
    let ends_statement = |l: usize| !table.quiet(fn_line, rap.name(), l, false)
        && visualization_data.source_lines.get(l.wrapping_sub(1)).is_some_and(|t| !t.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w == "else"));
    if own(src) && (!brace(src) || opens_with_code(src) || ends_statement(src)) {
        return Some((fn_line, src));
    }
    let mut r = table.next_state_row(fn_line, src)?;
    while brace(r) {
        if !table.quiet(fn_line, rap.name(), r, values_only) {
            return None;
        }
        r = table.next_state_row(fn_line, r)?;
    }
    table.quiet(fn_line, rap.name(), r, values_only).then_some((fn_line, r))
}

/// A field or element loan restricts access to its enclosing owner. Read
/// both the restriction and its holder at the same MIR boundary. A
/// reservation restricts like a shared loan until the call activates the
/// borrow. The title is None where every lent part is named as the owner
/// itself (an enum's payload): the owner's own loan title says it.
fn part_loan_state(
    state: &State,
    rap: &ResourceAccessPoint,
    data: &VisualizationData,
    fn_line: FunctionStart,
    cell: &crate::facts::table::Cell,
    arm: Option<crate::facts::SourceArm>,
) -> Option<(State, Option<String>)> {
    use crate::facts::table::Init;
    if rap.is_ref() || rap.is_fn() || rap.is_closure() || !matches!(cell.init, Init::Own | Init::Partly) {
        return None;
    }
    let s = match state {
        State::FullPrivilege { s: LineState::Full } | State::PartialPrivilege { s: LineState::Full } |
        State::RevokedPrivilege { .. } => LineState::Full,
        _ => return None,
    };
    let table = &data.state_table;
    let col = rap.name();
    let rank = |kind: &str| match kind {
        "mut" | "unique" => 2,
        "reserved" => 1,
        _ => 0,
    };
    let strongest = cell.lent.iter().map(|l| rank(l.kind)).max()?;
    let cell_for = |name: &str| match arm {
        Some(a) => table.source_arm_cell(fn_line, name, cell.from_row, a),
        None => table.cell(fn_line, name, cell.from_row, cell.from_row),
    };
    let strongest_loans = || cell.lent.iter().filter(|l| rank(l.kind) == strongest);
    if !strongest_loans().any(|l| table.loan(fn_line, l.loan).is_some_and(|loan| loan.part_of.contains(col))) {
        return None;
    }
    // The loans, as the title says them: every part loan of the column
    // live here (the strongest sets the line's style; a weaker one, `y`
    // keeping `e.0` shared beside `e.1` mutable, is said too), each part
    // named as 2.x's columns name it (`(t.0 as Some).0` is `t.0`), never as
    // a MIR place.
    let mut parts: Vec<(String, &str, Option<String>, bool)> = Vec::new();
    let lenders: BTreeSet<&str> = cell.lent.iter().filter_map(|l| Some(table.loan(fn_line, l.loan)?.lender.as_str())).collect();
    for lent in &cell.lent {
        let loan = table.loan(fn_line, lent.loan)?;
        // The eventual result of a call doesn't exist during reservation.
        // Name a holder only when its value keeps this loan on this path.
        let holder = (lent.kind != "reserved" && lent.holder != *col
            && cell_for(&lent.holder).is_some_and(|h| h.holds.contains(&lent.loan)))
            .then_some(lent.holder.as_str());
        // Said to hold the borrow itself ("p.a is borrowed by r") only where
        // the facts prove it, as `owner_loan_title` does: r took this loan
        // itself, it runs, and r can point to nothing else here (not after
        // `r = &other` on some path, not a `&T` keeping a `&mut` live).
        let exact = holder.is_some_and(|h| {
            loan.runs && loan.direct_holders.iter().any(|d| d == h)
                && cell.holder_sources.get(h).is_some_and(|src| !src.is_empty() && src.iter().all(|x| lenders.contains(x.as_str())))
                && !(rank(lent.kind) == 2 && data.timelines.values().map(|t| &t.resource_access_point)
                    .any(|r| r.name() == h && r.is_ref() && !r.is_mutref() && data.fn_source_lines.get(r.hash()).is_some_and(|start| start.body == fn_line.body)))
        });
        let shown = holder.and_then(|h| display_place(data, fn_line, col, h));
        let place = loan.projected.as_ref().map_or_else(
            || hover_messages::part_name_html(&part_display_name(data, fn_line, col, &loan.lender)),
            hover_messages::projected_place_html,
        );
        parts.push((place, lent.kind, shown, exact && loan.runs));
    }
    // A loan of a part 2.x's columns don't name (an enum's payload, shown
    // as the owner itself): the owner's own loan title says it.
    let whole_named = parts.iter().all(|(p, ..)| *p == styled_name_html(col));
    let behind_ref = strongest_loans().all(|l| table.loan(fn_line, l.loan).is_some_and(|i| i.through.is_some()));
    // One loan held by a variable the facts prove holds it: who holds it.
    // Otherwise (several loans, which need not all be taken on one path: an
    // or-pattern's alternatives, arms lending different fields; a holder
    // that may hold something else; a loan on some paths only or on none
    // that runs): what the borrow checker counts, and while what is in use.
    let holders: BTreeSet<Option<&str>> = parts.iter().map(|(_, _, h, _)| h.as_deref()).collect();
    let holder = holders.first().copied().flatten().filter(|_| holders.len() == 1);
    let runs = cell.lent.iter().all(|l| table.loan(fn_line, l.loan).is_some_and(|i| i.runs));
    let reason = match parts.as_slice() {
        [(place, kind, Some(h), true)] if !cell.may_lend => hover_messages::part_loan(place, kind, Some(h)),
        [(place, kind, None, _)] if !cell.may_lend && runs => hover_messages::part_loan(place, kind, None),
        [(place, kind, ..)] if cell.may_lend && runs => hover_messages::part_loan_maybe(place, kind, holder),
        _ => {
            let mut by_kind: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
            for (place, kind, ..) in &parts {
                by_kind.entry(*kind).or_default().insert(place.clone());
            }
            // (Strongest first: "e.1 as mutably borrowed and e.0 as immutably borrowed".)
            let mut groups: Vec<(&str, Vec<String>)> = by_kind.into_iter().map(|(k, v)| (k, v.into_iter().collect())).collect();
            groups.sort_by_key(|(k, _)| std::cmp::Reverse(rank(k)));
            hover_messages::part_loans_counted(&groups, holder)
        }
    };
    // The owner's own clause first, as where nothing is lent: what was
    // moved out of it, or that it owns its value (holds a copyable value,
    // borrows from ..).
    let partly = partly_moved_cell_title(rap, table, fn_line, cell.from_row, cell, &|name| match arm {
        Some(_) => cell_for(name),
        None => table.segment_cell(fn_line, name, cell.from_row),
    });
    let moved = partly.is_some() || cell.init == Init::Partly;
    let teaching = partly.unwrap_or_else(|| {
        if cell.init == Init::Partly {
            hover_messages::state_unnamed_part(col, cell.unnamed_maybe || cell.may)
        } else {
            owner_value_title(rap, data, fn_line, cell, None)
        }
    });
    let mut title = hover_messages::state_part_lent(&teaching, col, &reason, strongest == 2, behind_ref, moved);
    // Keep the teaching clause where every restricting loan is a reborrow
    // through one reference (`*h.r`, taken through `h.r`).
    let through: BTreeSet<_> = strongest_loans().filter_map(|l| table.loan(fn_line, l.loan)?.through.as_deref()).collect();
    if let Some(r) = through.first().filter(|_| through.len() == 1 && behind_ref).and_then(|r| display_place(data, fn_line, col, r)) {
        title.push_str(&hover_messages::state_through(&r));
    }
    let st = if strongest == 2 {
        State::RevokedPrivilege { to: ResourceTy::Anonymous, borrow_to: ResourceTy::Anonymous,
            prev_state: Box::new(State::FullPrivilege { s }) }
    } else {
        State::PartialPrivilege { s }
    };
    Some((st, (!whole_named).then_some(title)))
}

/// A lent part's name in an owner's title: as 2.x's columns name it
/// (`display_place`). A place behind a reference held in an enum's payload
/// (`*(s.v as Some).0`) has no such name: it's what that reference points to.
fn part_display_name(vd: &VisualizationData, fn_line: FunctionStart, column: &str, lender: &str) -> String {
    if let Some(inner) = lender.strip_prefix('*').filter(|i| i.starts_with('(')) {
        if let Some(shown) = display_place(vd, fn_line, column, inner).filter(|d| d != inner) {
            return format!("what the reference in {shown} points to");
        }
    }
    display_place(vd, fn_line, column, lender).filter(|d| !d.contains(" as ")).unwrap_or_else(|| format!("a part of {column}"))
}

/// Restrict the owner trunk at its own MIR boundary, including a Box
/// that still owns its allocation after part of the payload moved.
fn owner_part_loan_state(state: &State, rap: &ResourceAccessPoint, data: &VisualizationData, row: usize) -> Option<(State, PartLoanTitle)> {
    let table = &data.state_table;
    let fn_line = *data.fn_source_lines.get(rap.hash())?;
    if !table.has_part_loans(fn_line, rap.name()) { return None; }
    // (Not between dots stacked under one source line: the state after the
    // whole line holds from its last one, as in `part_loans_in_arm`.)
    if data.row_lines.get(row + 1).is_some_and(Option::is_none) {
        return None;
    }
    let src = rv_attrs::line_of(row, &data.row_lines)?;
    let cell = table.loan_cell(fn_line, rap.name(), src, 0)?;
    let (st, title) = part_loan_state(state, rap, data, fn_line, &cell, None)?;
    // Keep the or-pattern's teaching clause when its named holder really
    // does keep the loans live. Only the matched alternative ran.
    let alt = std::mem::discriminant(state) == std::mem::discriminant(&st)
        && or_alt_lend_at(data, rap, Some(row)).is_some_and(|(h, mutable, _, _)| {
            mutable == matches!(st, State::RevokedPrivilege { .. }) &&
                table.cell(fn_line, h.name(), cell.from_row, cell.from_row)
                    .is_some_and(|h| cell.lent.iter().all(|l| h.holds.contains(&l.loan)))
        });
    let title = match title {
        _ if alt => PartLoanTitle::Legacy,
        Some(t) => PartLoanTitle::Text(t),
        None => PartLoanTitle::OwnerLoan,
    };
    Some((st, title))
}

/// How an owner column's title says a live loan of a part of it.
enum PartLoanTitle {
    /// `part_loan_state`'s.
    Text(String),
    /// 2.x's for the drawn state (an or-pattern's alternatives).
    Legacy,
    /// The owner's own loan title (`owner_loan_title`): the part is named
    /// as the owner itself.
    OwnerLoan,
}

/// RustViz 3: the style of a column's line after the visual row `row`
/// from the state table's loans, where it differs from 2.x's `state` (a
/// loan 2.x missed, or one it keeps after the borrow checker ended it):
/// the new state, and its title when 2.x's history can't word it. For a
/// reference column, the loans of what it points to (a reborrow through
/// it, `&mut *r` / `&*r`) or of the reference itself (`&mut r`).
fn table_lent_state(state: &State, rap: &ResourceAccessPoint, visualization_data: &VisualizationData, row: usize, path: &[(usize, usize)]) -> Option<(State, Option<String>)> {
    use crate::facts::table::Init;
    let table = &visualization_data.state_table;
    if rap.is_fn() || rap.is_closure() {
        return None;
    }
    let fn_line = *visualization_data.fn_source_lines.get(rap.hash())?;
    let src = rv_attrs::line_of(row, &visualization_data.row_lines)?;
    // It holds a value: as for its text (after a `}`, what the next row
    // starts with), or, where that can't be read (the next row ends the
    // fn), the row's own state.
    let init = match table_row(rap, visualization_data, row, true) {
        Some((_, r)) => table.cell(fn_line, rap.name(), r, r)?.init,
        None => table.cell(fn_line, rap.name(), src, src)?.init,
    };
    if !matches!(init, Init::Own | Init::Partly) {
        return None;
    }
    // Its loans are read where the next row starts (rule 3), past any join
    // there: a row's own state has them (a `}` row's too). A row with none
    // takes the next row's, when that one changes no loan of it.
    // A row with none and no `}` between it and the row above that has
    // one (inside a multi-line call, a closure's body passed to it, a
    // comment) carries that row's loans (Q3): it is on the same path, and
    // a loan the statement takes is held until its last row.
    // (Only from a row whose code goes on onto the next line: a match
    // arm's `x => y,` row is the end of its arm, not on the next row's
    // path.)
    let text = |l: usize| visualization_data.source_lines.get(l.wrapping_sub(1)).map(|t| t.split("//").next().unwrap_or("").trim().to_owned());
    let brace = |l: usize| text(l).is_some_and(|t| t.starts_with('}'));
    let continues = |l: usize| text(l).is_some_and(|t| t.ends_with(['{', '(', '[', '=', '|', '.']));
    let carried = table.cell(fn_line, rap.name(), src, 0)
        .filter(|c| !brace(c.from_row) && continues(c.from_row) && (c.from_row + 1..=src).all(|l| !brace(l)));
    let cell = match (table.cell(fn_line, rap.name(), src, src), carried) {
        (Some(c), _) => c,
        (None, Some(c)) if !brace(src) => c,
        _ => {
            let (_, r) = table_row(rap, visualization_data, row, false)?;
            table.cell(fn_line, rap.name(), r, r)?
        }
    };
    table_lent_state_of(state, rap, visualization_data, fn_line, src, &cell, row, path)
}

/// `table_lent_state` from the cell `cell` the column shows after source
/// row `src` (a branch leg's: one arm's of a one-line conditional, P1-6).
fn table_lent_state_of(
    state: &State,
    rap: &ResourceAccessPoint,
    visualization_data: &VisualizationData,
    fn_line: FunctionStart,
    src: usize,
    cell: &crate::facts::table::Cell,
    row: usize,
    path: &[(usize, usize)],
) -> Option<(State, Option<String>)> {
    let table = &visualization_data.state_table;
    let s = match state {
        State::FullPrivilege { s } | State::PartialPrivilege { s } => s.clone(),
        State::RevokedPrivilege { prev_state, .. } => match prev_state.as_ref() {
            State::FullPrivilege { s } | State::PartialPrivilege { s } => s.clone(),
            _ => LineState::Full,
        },
        _ => return None,
    };
    if matches!(s, LineState::Uninit) {
        return None;
    }
    // (Through a `&T`, a shared reborrow restricts nothing: it is `Copy`
    // and only reads.)
    let own_loans: Vec<_> = cell.lent.iter()
        .filter(|l| table.lends_whole(fn_line, rap.name(), l.loan))
        .filter(|l| !(rap.is_ref() && !rap.is_mutref() && l.kind == "shared"))
        .collect();
    // A reference column: the loan is of the binding itself (`&mut r`,
    // a closure's unique capture of r), of a struct it is a field of
    // (`&mut p`, `&mut *h` for `h.r`), or of what it points to (`&mut *r`).
    let col = rap.name();
    let lender = |loan: usize| table.loan_lender(fn_line, loan).unwrap_or("");
    let of_itself = |loan: usize| lender(loan) == col;
    let of_parent = |loan: usize| col.starts_with(&format!("{}.", lender(loan).trim_start_matches('*')));
    let reserved = own_loans.iter().find(|l| l.kind == "reserved");
    let binding_shared = own_loans.iter().any(|l| l.kind == "shared"
        && table.loan(fn_line, l.loan).is_some_and(|info| info.whole.contains(col)));
    if rap.is_ref() && !own_loans.is_empty() {
        if own_loans.iter().any(|l| table.loan_arm(fn_line, l.loan).is_some()) {
            return None;
        }
        // A drawn shared lend of the reference binding itself (match &r).
        // The partial-privilege style alone can't distinguish it from &*r.
        if let [loan] = own_loans.as_slice() {
            if loan.kind == "shared" && of_itself(loan.loan)
                && matches!(state, State::PartialPrivilege { .. })
                && table.loan_arm(fn_line, loan.loan).is_none()
                && table.loan(fn_line, loan.loan).is_some_and(|l| l.direct_holders.contains(&loan.holder)
                    && visualization_data.timelines.get(rap.hash()).is_some_and(|timeline| timeline.history.iter().any(|(row, e)|
                        matches!(e, Event::StaticLend { to, of_ref_var: true, .. } if to.real_name() == loan.holder)
                        && rv_attrs::line_of(*row, &visualization_data.row_lines) == Some(l.line))))
            {
                let holds = hover_messages::ref_line_holds_reference(&styled_name_html(col), rap.is_mutref());
                let lend = hover_messages::event_dot_static_lend_ref_var(col, &loan.holder);
                let title = format!("{holds}. {lend}");
                return Some((State::PartialPrivilege { s }, Some(title)));
            }
        }
        // A shared loan of a reference slot restricts the binding itself.
        // With a direct holder, the borrowed reference survived as a
        // reference to the slot. A coercion through a temporary can
        // reborrow the pointee; keep its reborrow explanation.
        if !cell.may_lend && rap.is_mutref() && matches!(state, State::PartialPrivilege { .. })
            && own_loans.iter().all(|l| l.kind == "shared" && of_itself(l.loan)
                && table.loan(fn_line, l.loan).is_some_and(|loan| loan.holds_ref_slot))
        {
            let msg = format!("{0} holds a mutable reference. {0} is immutably borrowed and is read-only while the borrow is live", hover_messages::styled_name_html(rap.name()));
            return Some((State::PartialPrivilege { s }, Some(msg)));
        }
        // Where the table can't say it per path or 2.x's words don't fit,
        // 2.x's style stands: a loan taken in one arm of a branch on one
        // row; a shared loan of the binding or its struct (`&r`, `match &r`:
        // not "shared through it"); a unique capture of an enclosing struct;
        // or a loan through it taken before it was given another reference
        // on some path (it may point elsewhere now).
        if (binding_shared && reserved.is_none())
            || own_loans.iter().any(|l| l.kind == "unique" && of_parent(l.loan))
        {
            return None;
        }
        let through_lines: Vec<usize> = own_loans.iter()
            .filter(|l| !of_itself(l.loan) && !of_parent(l.loan))
            .filter_map(|l| table.loan_line(fn_line, l.loan))
            .collect();
        if let (Some(first), Some(t)) = (through_lines.iter().min(), visualization_data.timelines.get(rap.hash())) {
            let mut evs = Vec::new();
            flat_history(&t.history, path, &mut evs);
            let repointed = evs.iter().any(|(l, e)| {
                matches!(e, Event::StaticBorrow { .. } | Event::MutableBorrow { .. } | Event::Acquire { .. } | Event::Copy { .. })
                    && rv_attrs::line_of(*l, &visualization_data.row_lines).is_some_and(|sl| sl > *first && sl <= src)
            });
            if repointed {
                return None;
            }
        }
    }
    let exclusive: Vec<_> = own_loans.iter().filter(|l| matches!(l.kind, "mut" | "unique")).collect();
    let shared = own_loans.iter().any(|l| l.kind == "shared");
    if !exclusive.is_empty() {
        // Its holder, when every path agrees on one (a join may bring a
        // loan of it from each arm to a different holder).
        // (Not one taken in one arm of a branch on one row: on the other
        // arm's path nothing holds it.)
        let holders: BTreeSet<&str> = exclusive.iter().map(|l| l.holder.as_str()).collect();
        let arms: Vec<_> = exclusive.iter().map(|l| table.loan_arm(fn_line, l.loan)).collect();
        let in_arm = arms.iter().any(Option::is_some);
        // Borrowed on some paths only: a loan from an arm, unless some
        // loan isn't from one or every arm of that branch has one.
        let every_path = arms.iter().any(Option::is_none) || arms.iter().flatten().any(|(edge, count)| {
            let covered: BTreeSet<_> = arms.iter().flatten().filter(|(other, _)| other.switch == edge.switch)
                .map(|(other, _)| other.successor).collect();
            covered.len() == *count
        });
        let column = |name: &str| visualization_data.timelines.values().map(|t| &t.resource_access_point)
            .find(|h| h.name() == name && visualization_data.fn_source_lines.get(h.hash()).is_some_and(|start| start.body == fn_line.body));
        let carries = |h: &ResourceAccessPoint, loan: &crate::facts::schema::Lent| {
            if h.is_member() {
                // Cell::holds is for whole locals. The loan records which
                // projected places carry it at this same MIR boundary.
                loan.current.contains(h.name())
            } else {
                table.cell(fn_line, h.name(), cell.from_row, cell.from_row)
                    .is_some_and(|c| c.holds.contains(&loan.loan))
            }
        };
        let unique = exclusive.iter().all(|l| l.kind == "unique");
        let named = holders.first().filter(|_| holders.len() == 1 && !in_arm).and_then(|name| column(name));
        // A capture belongs to the closure even on the row that constructs
        // it, before the completed closure is stored in its binding.
        let capture = named.filter(|h| unique && h.is_closure() && exclusive.iter().all(|l|
            table.loan(fn_line, l.loan).is_some_and(|i| i.direct_holders.contains(h.name()))));
        let holder = capture.or_else(|| named.filter(|h| h.hash() != rap.hash() && exclusive.iter().all(|l| {
                // Lent::holder can select one of several live carriers.
                // A join with different possible holders has no single name.
                carries(h, l) && table.loan(fn_line, l.loan).is_some_and(|info| info.holders.iter()
                    .filter(|n| *n != h.name() && *n != col).all(|n| !table.cell(fn_line, n, cell.from_row, cell.from_row)
                        .is_some_and(|c| c.holds.contains(&l.loan))))
            })));
        if matches!(state, State::RevokedPrivilege { .. }) {
            let old = if loop_iterator_holds(visualization_data, rap, row).is_none() {
                let replayed = replayed_lent_holder(rap, visualization_data, row, path);
                replayed.holder.as_ref().and_then(|(name, _)| column(name))
            } else {
                None
            };
            // Validate the old claim even when there is no single named
            // replacement. A shared reference can keep a mutable loan
            // live, but cannot provide mutable access to its referent.
            let invalid = old.is_some_and(|h| (h.is_ref() && !h.is_mutref())
                || !exclusive.iter().all(|l| carries(h, l)));
            if !unique && every_path && !invalid {
                return None;
            }
        }
        let direct_mut = |h: &ResourceAccessPoint| h.is_mutref() && exclusive.iter().all(|l|
            table.loan(fn_line, l.loan).is_some_and(|i| i.direct_holders.contains(h.name())));
        let kept_by = holder.filter(|h| !h.is_closure() && !direct_mut(h));
        let lent_to = match holder.filter(|_| kept_by.is_none()) {
            Some(h) if h.is_closure() => hover_messages::LentTo::Closure(h.name()),
            Some(h) => hover_messages::LentTo::Var(h.name()),
            None => hover_messages::LentTo::Unknown,
        };
        let mut title = if !every_path && !unique {
            hover_messages::state_maybe_lent_out(rap.name(), false)
        } else if unique {
            // (For a reference: a closure's unique immutable borrow of the
            // binding itself, which writes through it.)
            hover_messages::state_unique_lent(rap.name(), lent_to)
        } else if rap.is_ref() {
            // The reference itself is borrowed (`&mut r`, or the struct it is
            // a field of: `&mut p` for `p.n`; always so for a `&T`), or what
            // it points to is, through it (`&mut *r`).
            let ref_itself = !rap.is_mutref() || exclusive.iter().all(|l| of_itself(l.loan) || of_parent(l.loan));
            hover_messages::state_lent_out(col, lent_to, lent_place(rap, ref_itself, binding_shared), None)
        } else {
            hover_messages::state_lent_out(col, lent_to, lent_place(rap, false, false), None)
        };
        let origin = exclusive.first().and_then(|l| table.loan(fn_line, l.loan)?.direct_holders.first())
            .and_then(|name| column(name));
        if let Some(h) = origin.filter(|h| h.is_mutref() && !cell.may_lend && holder.is_none_or(|cur| cur.hash() != h.hash())
            && exclusive.iter().all(|l| table.loan(fn_line, l.loan)
                .is_some_and(|i| i.runs && i.direct_holders.first() == Some(h.name()))))
        {
            // The initial borrower is meaningful even when it is no
            // longer the live carrier. A match scrutinee can also be
            // borrowed before the arm binds its reference.
            let original = styled_name_html(h.name());
            if exclusive.iter().all(|l| table.loan(fn_line, l.loan).is_some_and(|i|
                i.direct_holder.as_ref() == Some(h.name()) && i.through.is_some()))
            {
                title.push_str(&format!(". {original} took the original mutable reborrow"));
            } else {
                title.push_str(&format!(". This borrow supplies the reference for {original}"));
            }
        }
        if let Some(h) = kept_by {
            title.push_str(&format!(". The borrow checker tracks this borrow through {}", styled_name_html(h.name())));
            if h.is_ref() && !h.is_mutref() {
                title.push_str(", which holds a shared reference");
            }
        }
        let to = holder.map_or(ResourceTy::Anonymous, |h| ResourceTy::Value(h.clone()));
        return Some((State::RevokedPrivilege { to: to.clone(), borrow_to: to, prev_state: Box::new(State::FullPrivilege { s }) }, Some(title)));
    }
    if rap.is_ref() {
        // A `&mut` whose referent is shared through it is read-only
        // meanwhile; a `&T` is drawn shared all along (2.x's style).
        if let Some(reserved) = reserved {
            let loan = table.loan(fn_line, reserved.loan)?;
            // `whole` tests the column's MIR place. A reservation of **r
            // goes through r as well as *r, and leaves r replaceable.
            let title = hover_messages::state_ref_reserved(col, rap.is_mutref(), loan.to_call.as_deref(), loan.whole.contains(col), binding_shared);
            return Some((State::PartialPrivilege { s }, Some(title)));
        }
        return match (shared, state) {
            (true, State::PartialPrivilege { .. }) => None,
            (true, _) => Some((State::PartialPrivilege { s }, Some(hover_messages::state_mut_ref_partial_privilege(rap.name(), rap.points_to_copy())))),
            // Only a part of what it points to is lent (`&mut r.f`).
            (false, _) if !cell.lent.is_empty() => None,
            (false, State::RevokedPrivilege { .. } | State::PartialPrivilege { .. }) if rap.is_mutref() => Some((State::FullPrivilege { s }, None)),
            (false, State::RevokedPrivilege { .. }) => Some((State::PartialPrivilege { s }, None)),
            (false, _) => None,
        };
    }
    match (shared, reserved, state) {
        (true, _, State::PartialPrivilege { .. }) => None,
        (true, _, _) => Some((State::PartialPrivilege { s }, Some(hover_messages::state_partial_privilege(rap.name(), rap.holds_value())))),
        // Only reserved for a two-phase `&mut` (a method call's receiver
        // while its arguments run): it can still be read.
        (false, Some(l), _) => Some((State::PartialPrivilege { s }, Some(hover_messages::state_reserved(rap.name(), l.holder.strip_suffix("()"))))),
        (false, None, State::FullPrivilege { .. }) => None,
        // No confirmed part-loan boundary here (for example, a skipped arm).
        (false, None, _) if !cell.lent.is_empty() => None,
        (false, None, _) => Some((State::FullPrivilege { s }, None)),
    }
}

/// A struct's trunk stops where the remaining fields' drawn legs carry
/// them to their moves. Use the actual split/merge geometry and require
/// the MIR state to confirm every field is gone at its respective end.
fn trunk_ends_at_fields(vd: &VisualizationData, func: FunctionStart, name: &str, row: usize, src: usize) -> bool {
    use crate::facts::table::Init;
    let table = &vd.state_table;
    let fields = table.fields_of(func, name);
    if fields.is_empty() || !table.fields_complete(func, name) {
        return false;
    }
    fields.iter().all(|f| {
        let Some(t) = vd.timelines.values().find(|t| {
            t.resource_access_point.name() == f && vd.fn_source_lines.get(t.resource_access_point.hash()).is_some_and(|start| start.body == func.body)
        }) else {
            return false;
        };
        if t.states
            .iter()
            .any(|(a, b, s)| *a <= row && row < *b && matches!(s, State::ResourceMoved { dropped_by_hand: false, .. }))
            && table.segment_cell(func, f, src).is_some_and(|c| c.init == Init::Moved)
        {
            return true;
        }
        if t.history.iter().any(|(_, e)| match e {
            Event::Branch { split_point, merge_point, .. } if *split_point <= row && row < *merge_point => {
                rv_attrs::line_of(*merge_point, &vd.row_lines)
                    .and_then(|r| table.segment_cell(func, f, r))
                    .is_some_and(|c| c.init == Init::Moved)
            }
            _ => false,
        }) {
            return true;
        }
        trunk_ends_at_fields(vd, func, f, row, src)
    })
}

fn table_partly_moved_title(rap: &ResourceAccessPoint, table: &crate::facts::table::StateTable,
    fn_line: FunctionStart, r: usize, cell: &crate::facts::table::Cell) -> Option<String> {
    partly_moved_cell_title(rap, table, fn_line, r, cell, &|name| table.segment_cell(fn_line, name, r))
}

/// `table_partly_moved_title` with the other columns' cells read by
/// `cell_for` (an arm's own cells, for a leg of a one-line branch).
fn partly_moved_cell_title(rap: &ResourceAccessPoint, table: &crate::facts::table::StateTable,
    fn_line: FunctionStart, r: usize, cell: &crate::facts::table::Cell,
    cell_for: &impl Fn(&str) -> Option<crate::facts::table::Cell>) -> Option<String> {
    use crate::facts::table::Init;
    let mut parts = cell.parts.as_ref().cloned().unwrap_or_default();
    let imaginary: Vec<_> = parts.maybe.iter().filter(|p| cell_for(p)
        .is_some_and(|c| c.cause == Some("imaginary"))).cloned().collect();
    if !imaginary.is_empty() {
        parts.maybe.retain(|p| !imaginary.contains(p));
        return Some(hover_messages::state_parts_imaginary(rap.name(), &parts, &imaginary));
    }
    if let Some(n) = table.array_len(fn_line, rap.name(), cell) {
        if n <= crate::facts::schema::MAX_NAMED_ARRAY_LEN && (!parts.moved.is_empty() || !parts.maybe.is_empty()) {
            let left = (0..n).map(|i| format!("{}[{i}]", rap.name()))
                .filter(|p| !parts.moved.contains(p) && !parts.maybe.contains(p)).collect::<Vec<_>>();
            if parts.maybe.is_empty() || parts.moved.is_empty() {
                let list = if cell.may { &parts.maybe } else { &parts.moved };
                return Some(hover_messages::state_elems_moved(rap.name(), list, &left, cell.may));
            }
        }
    }
    // A Box whose contents were moved out (`let s = *b;`): the Box itself
    // wasn't, and still owns its heap allocation.
    // (Moved on some paths only: a Box struct's fields say which were for
    // sure.)
    let contents = format!("*{}", rap.name());
    let is_struct = matches!(rap, ResourceAccessPoint::Struct(_));
    if parts.moved.contains(&contents) || (!is_struct && parts.maybe.contains(&contents)) {
        return Some(hover_messages::state_box_contents_moved(rap.name(), !parts.moved.contains(&contents)));
    }
    // A move from deeper inside it (`**boxed` of a `Box<Box<String>>`: the
    // inner Box stays): name the places moved out.
    if !is_struct {
        let surely: Vec<String> = parts.moved.clone();
        let maybe: Vec<String> = parts.maybe.iter().filter(|m| !surely.contains(m)).cloned().collect();
        return (!surely.is_empty() || !maybe.is_empty())
            .then(|| hover_messages::state_table_partly_moved(rap.name(), &surely, &maybe, &[], false));
    }
    let ResourceAccessPoint::Struct(Struct { is_box, .. }) = rap else { return None };
    // Its field columns are all of its fields (none is private to another
    // module): only then can it say every field is gone, or that the
    // others can be used.
    let complete = table.fields_complete(fn_line, rap.name());
    let is_box = *is_box;
    // Each direct field: what the moved places say of it, else its own
    // column's state (a part of it moved out may have no column: an array
    // element).
    let covers = |f: &str, n: &str| {
        let n = n.trim_start_matches('*');
        n.starts_with(&format!("{f}.")) || n.starts_with(&format!("{f}["))
    };
    let field_init: Vec<(String, Init)> = table.fields_of(fn_line, rap.name()).into_iter()
        .filter_map(|f| {
            let init = if parts.moved.contains(&f) {
                Init::Moved
            } else if parts.maybe.contains(&f) {
                Init::Maybe
            } else if parts.moved.iter().chain(parts.maybe.iter()).any(|n| covers(&f, n)) {
                Init::Partly
            } else {
                cell_for(&f)?.init
            };
            Some((f, init))
        })
        .collect();
    // Every field gone, surely or on some paths: none of it can be used.
    if complete && !field_init.is_empty() && field_init.iter().all(|(_, i)| matches!(i, Init::Moved | Init::Maybe)) {
        let surely: Vec<String> = field_init.iter().filter(|(_, i)| *i == Init::Moved).map(|(f, _)| f.clone()).collect();
        let maybe: Vec<String> = field_init.iter().filter(|(_, i)| *i == Init::Maybe).map(|(f, _)| f.clone()).collect();
        // Every field of a Box's payload: its contents (surely, or some
        // fields on some paths only).
        if is_box {
            return Some(if maybe.is_empty() {
                hover_messages::state_box_contents_moved(rap.name(), false)
            } else {
                hover_messages::state_box_fields_maybe_gone(rap.name(), &surely, &maybe)
            });
        }
        if maybe.is_empty() {
            return Some(hover_messages::state_fields_all_moved(rap.name(), &surely));
        }
        let mut title = hover_messages::state_table_fields_maybe_gone(rap.name(), &surely, &maybe);
        if table.drops_rest_at_exit(fn_line, rap.name(), r) {
            title.push_str(&format!("; what is left in {} is dropped when it goes out of scope", styled_name_html(rap.name())));
        }
        return Some(title);
    }
    // The moved-out places, outermost only (`o.inner` when it moved out
    // whole, not also `o.inner.x`).
    let outermost = |list: &[String], also: &[String]| -> Vec<String> {
        list.iter()
            .filter(|m| !list.iter().chain(also.iter()).any(|o| m.starts_with(&format!("{}.", o))))
            .cloned()
            .collect()
    };
    let mut surely = outermost(&parts.moved, &[]);
    let mut maybe: Vec<String> = outermost(&parts.maybe, &surely).into_iter().filter(|m| !surely.contains(m)).collect();
    // A field that isn't whole though nothing in it is named: on every
    // path, or (its own cell says so) on some only.
    let mut partly = Vec::new();
    let mut partly_maybe = Vec::new();
    for (f, init) in &field_init {
        if surely.iter().chain(maybe.iter()).any(|n| n.trim_start_matches('*') == f || covers(f, n)) {
            continue;
        }
        match init {
            Init::Moved => surely.push(f.clone()),
            Init::Maybe => maybe.push(f.clone()),
            // (Its own cell knows whether what is gone from it is gone on
            // every path: `p.a[0]` of an array field, an enum payload.)
            Init::Partly if cell_for(f).is_none_or(|c| c.unnamed_maybe || c.may) => partly_maybe.push(f.clone()),
            Init::Partly => partly.push(f.clone()),
            _ => {}
        }
    }
    if surely.is_empty() && maybe.is_empty() && partly.is_empty() && partly_maybe.is_empty() {
        return None;
    }
    // "Its other fields still can" (be used), where the facts show it:
    // every place named was surely moved out (none may have been, no part
    // unnamed), and every other field holds its whole value on every path
    // here, lent at most shared.
    let others_usable = complete && maybe.is_empty() && partly.is_empty() && partly_maybe.is_empty()
        && {
            let others: Vec<&String> = field_init.iter().map(|(f, _)| f)
                .filter(|f| !surely.iter().any(|n| n.trim_start_matches('*') == f.as_str() || covers(f, n)))
                .collect();
            !others.is_empty() && others.iter().all(|f| cell_for(f).is_some_and(|c| {
                c.init == Init::Own
                    && c.parts.as_ref().is_none_or(|p| p.moved.is_empty() && p.maybe.is_empty())
                    && c.lent.iter().all(|l| matches!(l.kind, "shared" | "reserved"))
            }))
        };
    Some(hover_messages::state_table_partly_moved_hedged(rap.name(), &surely, &maybe, &partly, &partly_maybe, others_usable))
}

/// The elements (`pair[0]`) a slice pattern moved out of an array's
/// column by the row `line`, in its `history` (#1285), and whether
/// that happened in a branch of a conditional only (`in_branch`: the
/// history is a branch's).
fn elems_moved_before(history: &[(usize, Event)], line: usize, in_branch: bool) -> (Vec<String>, bool) {
    let (mut moved, mut maybe) = (Vec::new(), false);
    for (l, e) in history {
        if *l > line {
            break;
        }
        match e {
            Event::Move { is: ResourceTy::Index(_, place, _), .. } if !place.starts_with('*') => {
                if !moved.contains(place) {
                    moved.push(place.clone());
                }
                maybe |= in_branch;
            }
            Event::Branch { branch_history, .. } => {
                for b in branch_history {
                    let (m, _) = elems_moved_before(&b.e_data, line, true);
                    for p in m {
                        maybe = true;
                        if !moved.contains(&p) {
                            moved.push(p);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (moved, maybe)
}

/// The rows where a borrow of a field of what the `&mut` reference
/// `rap` points to starts or ends (`self.items` in `for e in
/// self.items.iter_mut()`; #1073), sorted. Empty for anything else.
fn field_borrow_rows(rap: &ResourceAccessPoint, history: &[(usize, Event)], visualization_data: &VisualizationData) -> Vec<usize> {
    if !rap.is_mutref() {
        return Vec::new();
    }
    let mut rows: Vec<usize> = history.iter().filter_map(|(l, e)| match e {
        Event::MutableLend { is: ResourceTy::DerefField(r, ..), .. }
        | Event::StaticLend { is: ResourceTy::DerefField(r, ..), .. }
        | Event::MutableReacquire { is: ResourceTy::DerefField(r, ..), .. }
        | Event::StaticReacquire { is: ResourceTy::DerefField(r, ..), .. } if r.hash() == rap.hash() => Some(*l),
        _ => None,
    }).collect();
    // Where a `for` loop takes over a value that holds such a borrow
    // (#1080): the text names the loop's iterator from there.
    for (l, e) in history {
        if let Event::MutableLend { is: ResourceTy::DerefField(r, ..), to: ResourceTy::Value(b), .. }
            | Event::StaticLend { is: ResourceTy::DerefField(r, ..), to: ResourceTy::Value(b), .. } = e
        {
            if r.hash() == rap.hash() && !b.is_fn() {
                rows.extend(moved_into_loop(visualization_data, rap, b, *l));
            }
        }
    }
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// How far above its first row / below its last row the split and the
/// join of a conditional whose branches share lines are drawn: one row,
/// onto the blank rows `render_svg` puts around it (see
/// `render_branches`).
const COLLAPSED_APEX: i64 = 30;

/// How far above its row the split dot of an `if let` is drawn when the
/// pattern binds from the column on that row (see `binds_at_split`):
/// half a row (`LINE_SPACE` is 30), into the gap above, so no row is
/// added.
const PATTERN_SPLIT_RISE: i64 = 15;

/// Whether the conditional `history[i]` (over several lines) has a
/// first arm that starts on the split line, with an event of this column:
/// an `if let` pattern's borrow, copy or move out of it, which is the
/// first thing its `if` arm does (#382), or any event of an arm written
/// on the `if` / `match` line itself, as in `if c { take(s); } else {`
/// (#508). The arm's column then starts on the split row with that
/// event's dot, and the split dot goes half a row above it
/// (`PATTERN_SPLIT_RISE`) with the arms forking from there. Drawn from
/// the split dot on the split row, the arm ran back up from its diagonal
/// to the event's dot, and an arrow from that dot hopped over the split
/// dot.
///
/// An event of the column's own in the condition or scrutinee on that
/// line (`if s.len() > 1 { take(s); }`, `if x.is_some() && let Some(a)
/// = &x {`, or a copy out of an `if let` scrutinee, #557) runs before
/// either arm: it goes on the raised split dot (`event_dot_y`), not
/// between the arms (#509).
fn binds_at_split(history: &[(usize, Event)], i: usize) -> bool {
    let Event::Branch { branch_history, split_point, merge_point, fork_above, .. } = &history[i].1 else { return false };
    // (Or a one-line conditional whose arms got rows of their own, on every
    // column: it forks a full row up, `split_rise`, design Q9.)
    if *fork_above || branch_history.iter().any(|b| b.imaginary.is_some()) {
        return true;
    }
    // (Or a later arm of a one-line conditional whose arms got rows of
    // their own, `if n > 2 { } else if let Some(x) = v.first() { let r =
    // x; }`: that arm's column starts on the split row too, #1426. Its
    // leg forked from the split dot on that row, a row above the dot
    // where it arrived, and the arm's events were drawn above its leg.)
    // (Or an arm that starts on the next row with a one-row diamond of
    // its own, `else if b.len() > 5 { &b } else { best }` on the line of
    // `if a.len() > 5 { &a }`, the outer join a row below the inner one,
    // #1534: that diamond's fork, a row above its row, is level with the
    // split row, and the arm's leg ran on past it into the diamond.)
    let starts_with_diamond = |arm: &BranchData| {
        let mut evs = arm.e_data.iter().skip_while(|(l, ev)| *l == *split_point + 1 && !matches!(ev, Event::Branch { .. }));
        matches!(evs.next(), Some((l, Event::Branch { split_point: s, merge_point: m, .. }))
            if *l == *split_point + 1 && *s == *l && branch_is_collapsed(*s, *m))
    };
    !branch_is_collapsed(*split_point, *merge_point)
        && branch_history.iter().any(|arm| arm.e_data.iter().any(|(l, _)| l == split_point) || starts_with_diamond(arm))
}

/// How far above the split line the fork of the conditional that the
/// first arm of `branch_history` (a conditional splitting on
/// `split_point`) starts with is drawn, when that fork is raised above the
/// line: one whose arms share that line, like the one-line `if` in a
/// let-chain's block (`if let Some(a) = y && { if a > 2 { take(s); } … }
/// {`, #1287), is a row up (`COLLAPSED_APEX`); one whose first arm starts
/// on the line with an event of this column (`binds_at_split`), like the
/// inner `if` of `if c { if d { take(f) } else { take(g) } } else {
/// take(h) }` with its arms on rows of their own (#1306), is half a row
/// up. The outer fork goes above that, and the first arm's leg ends at it.
/// Events of this column in the inner conditional's own condition (`if c {
/// if s.is_empty() { return; } } else { … }`) come before it in the arm,
/// on the split line: they are drawn on the inner fork (`event_dot_y`), so
/// the arm still starts with that fork (#1534; counted as no fork, the
/// outer one went half a row up, below the inner one).
fn arm_first_fork_rise(branch_history: &[BranchData], split_point: usize) -> Option<i64> {
    let arm = branch_history.first()?;
    let k = arm.e_data.iter()
        .position(|(l, ev)| *l != split_point || matches!(ev, Event::Branch { .. }))?;
    match &arm.e_data[k] {
        (l, Event::Branch { split_point: s, merge_point: m, .. }) if *l == split_point && *s == split_point =>
            if branch_is_collapsed(*s, *m) {
                Some(COLLAPSED_APEX)
            } else {
                binds_at_split(&arm.e_data, k).then(|| split_rise(&arm.e_data, k))
            },
        _ => None,
    }
}

/// How far above its row the split dot of the conditional `history[i]` is
/// raised (`binds_at_split`): `PATTERN_SPLIT_RISE`, or that above the
/// raised fork of a conditional its first arm starts with
/// (`arm_first_fork_rise`).
fn split_rise(history: &[(usize, Event)], i: usize) -> i64 {
    match &history[i].1 {
        // A one-line conditional whose arms got rows of their own: on the
        // blank row above its line, its first arm on the line's row (Q9).
        Event::Branch { fork_above: true, .. } => COLLAPSED_APEX,
        Event::Branch { branch_history, split_point, .. } =>
            arm_first_fork_rise(branch_history, *split_point).unwrap_or(0) + PATTERN_SPLIT_RISE,
        _ => PATTERN_SPLIT_RISE,
    }
}

/// How far above its merge row each conditional's join dot sits on a
/// column, by (split, merge) line (#1294). An `else if` is a conditional
/// nested in the outer one's `else` arm that ends on the same `}` row:
/// the two joins would share that row, and the outer arm's leg, drawn
/// from the row above, would start in mid-air beside the inner join. A
/// chain of n conditionals ending on one row (`if … else if … else if …
/// else`) spreads their joins over the row above it instead: the k-th
/// from the outside sits k/n of a row up, and each `else` arm's leg runs
/// from the join of the conditional nested in it. Conditionals whose arms
/// share lines (`branch_is_collapsed`) keep their own joins.
fn shared_merge_rises(history: &[(usize, Event)]) -> BTreeMap<(usize, usize), i64> {
    type Info = BTreeMap<(usize, usize), (usize, (usize, usize))>;
    fn walk(h: &[(usize, Event)], parent: Option<(usize, usize, (usize, usize))>, info: &mut Info) {
        for (_, ev) in h {
            let Event::Branch { branch_history, split_point, merge_point, .. } = ev else { continue };
            let key = (*split_point, *merge_point);
            let collapsed = branch_is_collapsed(*split_point, *merge_point);
            let (depth, root) = match parent {
                Some((m, d, root)) if !collapsed && m == *merge_point => (d + 1, root),
                _ => (0, key),
            };
            info.insert(key, (depth, root));
            for b in branch_history {
                walk(&b.e_data, (!collapsed).then_some((*merge_point, depth, root)), info);
            }
        }
    }
    let mut info = Info::new();
    walk(history, None, &mut info);
    let mut len: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (depth, root) in info.values() {
        let n = len.entry(*root).or_insert(1);
        *n = (*n).max(depth + 1);
    }
    info.iter()
        .filter(|(_, (depth, _))| *depth > 0)
        .map(|(key, (depth, root))| (*key, LINE_SPACE * *depth as i64 / len[root] as i64))
        .collect()
}

/// `shared_merge_rises` of the conditional (split, merge) on `hash`'s
/// column (`history`: where the caller found it, if the column has none).
fn join_rise(vd: &VisualizationData, hash: &u64, history: &[(usize, Event)], split: usize, merge: usize) -> i64 {
    let full = vd.timelines.get(hash).map_or(history, |t| &t.history);
    shared_merge_rises(full).get(&(split, merge)).copied().unwrap_or(0)
}

/// Lines where a column stops for, or resumes after, a conditional
/// whose branches share lines. The column has to stop at the diamond's
/// top corner (half a row above the line), not run on into it. Also
/// the split lines of `if let`s whose split dot is raised above its row
/// (`binds_at_split`), where the column stops at that dot.
struct CollapsedApexes {
    splits: BTreeSet<usize>,
    merges: BTreeSet<usize>,
    /// Split line → how far above its row the split dot is (`split_rise`).
    raised_splits: BTreeMap<usize, i64>,
}

impl CollapsedApexes {
    fn of(history: &[(usize, Event)]) -> Self {
        let mut apexes = CollapsedApexes {
            splits: BTreeSet::new(),
            merges: BTreeSet::new(),
            raised_splits: BTreeMap::new(),
        };
        for (i, (_, ev)) in history.iter().enumerate() {
            if let Event::Branch { split_point, merge_point, .. } = ev {
                if branch_is_collapsed(*split_point, *merge_point) {
                    apexes.splits.insert(*split_point);
                    apexes.merges.insert(*merge_point);
                } else if binds_at_split(history, i) {
                    apexes.raised_splits.insert(*split_point, split_rise(history, i));
                }
            }
        }
        apexes
    }

    /// The y range of a column segment from line `top` to line `bot`,
    /// or `None` when nothing is left to draw.
    fn segment_ys(&self, top: usize, bot: usize, start_y: Option<i64>) -> Option<(i64, i64)> {
        if top == bot {
            return None;
        }
        let (after_join, before_split) = (self.merges.contains(&top), self.splits.contains(&bot));
        let raised_rise = self.raised_splits.get(&bot).copied();
        let before_raised_split = raised_rise.is_some();
        let y1 = start_y.unwrap_or_else(|| get_y_axis_pos(top) + if after_join { COLLAPSED_APEX } else { 0 });
        let y2 = get_y_axis_pos(bot) - if before_split {
            COLLAPSED_APEX
        } else if let Some(rise) = raised_rise {
            rise
        } else {
            0
        };
        if (start_y.is_some() || after_join || before_split || before_raised_split) && y1 >= y2 {
            return None;
        }
        Some((y1, y2))
    }
}

/// The y of the dot for event `i` of a column's `history`. Usually its
/// line's row, but a conditional whose branches share lines draws its
/// arms on those rows, as a diamond from the split dot above its first
/// line to the join dot below its last (see `render_branches`). An
/// event recorded before the split, on its line, is part of the
/// condition or scrutinee (`if t.len() > 1 { .. } else { .. }` on one
/// line): it runs before any arm, so it goes on the split dot, not in
/// the middle of the diamond (#492). Likewise an event recorded after
/// the conditional, on its last line, runs after the arms: it goes on
/// the join dot. The same holds for a conditional whose split dot is
/// raised half a row above its line (`binds_at_split`): an event of
/// the condition on that line goes on the raised split dot (#509).
fn event_dot_y(history: &[(usize, Event)], i: usize) -> i64 {
    let line = history[i].0;
    let collapsed = |ev: &Event, on: fn(usize, usize) -> usize| matches!(ev,
        Event::Branch { split_point, merge_point, .. }
            if branch_is_collapsed(*split_point, *merge_point) && on(*split_point, *merge_point) == line);
    let mut same_line_after = history[i + 1..].iter().enumerate()
        .take_while(|(_, (l, _))| *l == line);
    let before_split = same_line_after.clone().any(|(_, (_, ev))| collapsed(ev, |split, _| split));
    let raised_rise = same_line_after
        .find(|(j, (_, ev))| matches!(ev, Event::Branch { split_point, .. } if *split_point == line)
            && binds_at_split(history, i + 1 + j))
        .map(|(j, _)| split_rise(history, i + 1 + j));
    let after_join = history[..i].iter().rev()
        .any(|(_, ev)| collapsed(ev, |_, merge| merge));
    get_y_axis_pos(line) + if before_split {
        -COLLAPSED_APEX
    } else if let Some(rise) = raised_rise {
        -rise
    } else if after_join {
        COLLAPSED_APEX
    } else {
        0
    }
}

/// `event_dot_y` of the event `id` on `hash`'s column (or in one of its
/// branches), for the arrows and call icons that meet that event's dot.
/// The line's row when the column has no such event.
fn column_event_y(vd: &VisualizationData, hash: &u64, id: usize, line: usize) -> i64 {
    fn find(history: &[(usize, Event)], id: usize) -> Option<i64> {
        history.iter().enumerate().find_map(|(i, (_, ev))| match ev {
            Event::Branch { branch_history, .. } =>
                branch_history.iter().find_map(|b| find(&b.e_data, id)),
            _ if ev.get_id() == id => Some(event_dot_y(history, i)),
            _ => None,
        })
    }
    vd.timelines.get(hash)
        .and_then(|t| find(&t.history, id))
        .unwrap_or_else(|| get_y_axis_pos(line))
}

// render timelines (states) for RAPs using vertical lines
fn render_timelines(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    registry: &Handlebars,
    dots: &DotLayer,
    bars: &mut ColumnMarks,
){
    let timelines = &visualization_data.timelines;
    for (hash, timeline) in timelines {
        let rap = &timeline.resource_access_point;
        match rap {
            ResourceAccessPoint::Function(_) => {},
            _ => {
                // println!("hash {}, timeline {:#?}", hash, timeline);
                let t_data = resource_owners_layout.get(hash).unwrap();
                render_timeline(hash, rap, &timeline.history, &timeline.states, output, visualization_data, t_data, registry, dots, bars);
            }
        }
    }
}

// vertical lines indicating whether a reference can mutate its resource(deref as many times)
// (iff it's a MutRef && it has FullPrivilege)
fn render_ref_line(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    registry: &Handlebars,
    brackets: &mut Vec<Piece>,
){
    let timelines = &visualization_data.timelines;

    for (hash, timeline) in timelines{
        match timeline.resource_access_point {
            ResourceAccessPoint::Function(_) => (), /* do nothing */
            ResourceAccessPoint::Struct(_) | ResourceAccessPoint::Owner(_) | ResourceAccessPoint::MutRef(_) | ResourceAccessPoint::StaticRef(_) =>
            {
                let ro = timeline.resource_access_point.to_owned();
                // verticle state lines
                let states = &timeline.states;
                // A reference bound by an `if`/`match` only exists once the
                // join below the diamond has produced it, so its bracket
                // starts at the join, clear of the arms (#489).
                let apexes = CollapsedApexes::of(&timeline.history);
                let start_y = |line: usize| get_y_axis_pos(line)
                    + if apexes.merges.contains(&line) { COLLAPSED_APEX } else { 0 };
                let end_y = |line: usize| get_y_axis_pos(line) - if apexes.splits.contains(&line) {
                    COLLAPSED_APEX
                } else {
                    apexes.raised_splits.get(&line).copied().unwrap_or(0)
                };
                // struct can live over events
                let mut alive = false;
                let mut data = RefLineData {
                    line_class: String::new(),
                    hash: 0,
                    x1: 0,
                    x2: 0,
                    y1: 0,
                    y2: 0,
                    v: 0,
                    dx: 15,
                    dy: 0,
                    title: String::new(),
                    rv: String::new(),
                };
                let mut bracket = Rv::new(rv_attrs::BRACKET);
                let mut bracket_start = 0;
                for (line_start, _line_end, state) in states.iter() {
                    match state { 
                        State::OutOfScope | State::ResourceMoved{ .. } => {
                            if alive {
                                // finish line template
                                data.x2 = data.x1.clone();
                                // Before a conditional whose fork is drawn
                                // above its row (one-line, or `if let`
                                // binding from x), the column stops at the
                                // fork dot, and so does the bracket: run on
                                // to the row, it ended in the middle of the
                                // diamond (#1308).
                                data.y2 = end_y(*line_start).max(data.y1);
                                let dv = data.y2 - data.y1;
                                // A reference bound and ended on one dot
                                // (`let r = x;` never used) has no span to
                                // bracket: drawn, it was a 15 px stub off
                                // the dot (#1611). The dot says it all.
                                if dv <= 0 {
                                    alive = false;
                                    continue;
                                }
                                data.v = dv - 2*dv/5;
                                data.dy = dv/5;
                                data.rv = bracket.clone().rows(bracket_start, *line_start).attrs();
                                // (`M x1 y1 l dx dy v v l -dx dy`: its two
                                // slanted ends, for the arrows.)
                                if matches!(ro, ResourceAccessPoint::MutRef(_) | ResourceAccessPoint::StaticRef(_)) {
                                    let (x, y1, y2) = (data.x1 as f64, data.y1 as f64, data.y2 as f64);
                                    let (dx, dy) = (data.dx as f64, data.dy as f64);
                                    brackets.push(((x, y1), (x + dx, y1 + dy)));
                                    brackets.push(((x + dx, y2 - dy), (x, y2)));
                                }

                                match ro {
                                    ResourceAccessPoint::MutRef(_) => {
                                        output.get_mut(&-1).unwrap().0.ref_line.push_str(&registry.render("solid_ref_line_template", &data).unwrap());
                                    },
                                    ResourceAccessPoint::StaticRef(_) => {
                                        output.get_mut(&-1).unwrap().0.ref_line.push_str(&registry.render("hollow_ref_line_template", &data).unwrap());
                                    },
                                    _ => (),
                                }

                                alive = false;
                            }
                        },
                        State::FullPrivilege{..} => {
                            if !alive {
                                // set known vals
                                data.hash = *hash;
                                data.x1 = resource_owners_layout[hash].x_val;
                                data.y1 = start_y(*line_start);
                                bracket_start = *line_start;
                                bracket = Rv::new(rv_attrs::BRACKET).state(state).var(&resource_owners_layout[hash].name);

                                let styled = styled_name_html(&visualization_data.get_name_from_hash(hash).unwrap());
                                data.title = hover_messages::ref_line_holds_reference(&styled, matches!(ro, ResourceAccessPoint::MutRef(_)));
                                data.line_class = String::from("solid");
                                alive = true;
                            }
                        },
                        State::PartialPrivilege{..} => {
                            if !alive {
                                // set known vals
                                data.hash = *hash;
                                data.x1 = resource_owners_layout[hash].x_val;
                                data.y1 = start_y(*line_start);
                                bracket_start = *line_start;
                                bracket = Rv::new(rv_attrs::BRACKET).state(state).var(&resource_owners_layout[hash].name);

                                let styled = styled_name_html(&visualization_data.get_name_from_hash(hash).unwrap());
                                // A `&mut` whose referent is shared for now
                                // (resumed after a join) is still a mutable
                                // reference (#1591).
                                data.title = match ro {
                                    ResourceAccessPoint::MutRef(_) => hover_messages::ref_line_holds_reference(&styled, true),
                                    _ => hover_messages::ref_line_holds_immutable_reference(&styled),
                                };
                                data.line_class = String::from("solid");
                                alive = true;
                            }
                        },
                        _ => (),
                    }
                }
            },  
        }
    }
}

fn render_struct_box(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    structs_info: &StructsInfo,
    fn_start_lines: &HashMap<u64, usize>,
    raised_headers: &HashSet<u64>,
    registry: &Handlebars,
) {
    // Default y matches the legacy "all labels at the top of the
    // SVG" layout (label_y=70, box=50..80). Per-fn struct boxes
    // override below to track their fn's label row.
    const DEFAULT_BOX_Y: i64 = FIRST_ROW_Y - 40;
    for (owner, owner_x, last_x, owner_half, last_half) in structs_info.structs.iter() {
        let owner_hash = *owner as u64;
        let y = match fn_start_lines.get(&owner_hash) {
            // Labels sit on the row directly above the fn signature
            // (see render_labels_string). Box centers on that label
            // row: 20px above (matches the legacy 70 - 20 = 50 offset)
            // and 30px tall.
            Some(&line) => get_y_axis_pos(line) - LINE_SPACE * header_rows_up(raised_headers, owner_hash) - 20,
            None => DEFAULT_BOX_Y,
        };
        // Fixed padding around the end columns, or the end labels'
        // extent plus BOX_PAD if that is wider (#477). Labels between
        // the two ends lie between the end columns, so they fit.
        let left = cmp::min(owner_x - 20, owner_x - owner_half - BOX_PAD);
        let right = cmp::max(last_x + 40, last_x + last_half + BOX_PAD);
        let box_data = BoxData {
            name: owner_hash,
            hash: 0,
            x: left,
            y,
            w: right - left,
            h: 30,
            title: String::new(),
        };
        output.get_mut(owner).unwrap().1.arrows.push_str(&registry.render("box_template", &box_data).unwrap());
    }
}

fn get_y_axis_pos(line_number : usize) -> i64 {
    FIRST_ROW_Y - 5 - LINE_SPACE + LINE_SPACE * line_number as i64
}

/// The part of `owner` (a member column, `p.f` of `p`) that is given a
/// new value on `line`, where a `?` may return early first (#1151).
fn try_assigned_part(visualization_data: &VisualizationData, owner: u64, line: usize) -> Option<String> {
    visualization_data.timelines.values().find(|t| {
        let rap = &t.resource_access_point;
        rap.is_member() && rap.get_owner() == owner
            && t.history.iter().any(|(l, e)| *l == line
                && matches!(e, Event::Acquire { .. } | Event::Copy { .. } | Event::OwnerDropAtReassign { .. }))
    }).map(|t| t.resource_access_point.name().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arm(e_data: Vec<(usize, Event)>) -> BranchData {
        BranchData {
            source: None,
            source_span: None,
            imaginary: None,
            t_data: TimelineColumnData {
                name: String::new(),
                x_val: 0,
                title: String::new(),
                is_ref: false,
                is_struct_group: false,
                is_member: false,
                owner: 0,
            },
            e_data,
            width: 0,
            states: Vec::new(),
            diverges: false,
            exits: Vec::new(),
            entry_state: None,
        }
    }

    /// A `Branch` event with `n` empty arms, its widths populated.
    fn branch(arms: Vec<BranchData>) -> Vec<(usize, Event)> {
        let mut events = vec![(0, Event::Branch {
            join: None,
            is: ResourceTy::Anonymous,
            branch_history: arms,
            ty: BranchType::Match(Vec::new(), Vec::new()),
            split_point: 0,
            merge_point: 0,
            maybe_init: false,
            loop_assigned: false,
            maybe_held: false,
            fork_above: false, skipped_hold: false, on_exhaustion: false,
            id: 0,
        })];
        compute_width(&mut events);
        events
    }

    fn centers_of(events: &[(usize, Event)]) -> Vec<i64> {
        match &events[0].1 {
            Event::Branch { branch_history, .. } => branch_centers(branch_history),
            _ => panic!("internal state: expected a branch event here"),
        }
    }

    fn leaves(n: usize) -> Vec<(usize, Event)> {
        branch((0..n).map(|_| arm(Vec::new())).collect())
    }

    #[test]
    fn a_nested_trunk_restarts_at_its_fields_offset_join() {
        let row = 8;
        let baseline = get_y_axis_pos(row);
        for offset in [COLLAPSED_APEX, -LINE_SPACE / 2] {
            let mut dots = DotLayer::default();
            let mut column = arm(Vec::new()).t_data;
            column.is_struct_group = true;
            column.is_member = true;
            column.owner = 1;
            column.name = "p.other".into();
            let is_descendant = |column: &str| column == "p.inner.a";
            dots.add_event(3, 100, baseline, &column, String::new(), false, None, "acquire", row);
            assert_eq!(dots.trunk_restart_y(row, 2, 1, is_descendant), None);
            column.name = "p.inner.a".into();
            let join_y = baseline + offset;
            dots.add_event(4, 130, join_y, &column, String::new(), false, None, rv_attrs::BRANCH_JOIN, row);
            let start = dots.trunk_restart_y(row, 2, 1, is_descendant);
            assert_eq!(start, Some(join_y));
            let apexes = CollapsedApexes::of(&[]);
            assert_eq!(apexes.segment_ys(row, row + 2, start), Some((join_y, get_y_axis_pos(row + 2))));
        }
    }

    #[test]
    fn clearing_a_leg_cannot_trade_one_covered_mark_for_another() {
        let key = (1, 1);
        let before = Drawn { key, pieces: vec![((0.0, 0.0), (40.0, 0.0))], ..Default::default() };
        let after = Drawn { key, pieces: vec![((0.0, 20.0), (40.0, 20.0))], ..Default::default() };
        let dots = DotLayer {
            positions: BTreeSet::from([(20, 0), (20, 20)]),
            legs: vec![((5.0, 0.0), (35.0, 0.0))],
            ..Default::default()
        };
        assert_eq!(visible_arrow_defects(std::slice::from_ref(&before), &dots),
            visible_arrow_defects(std::slice::from_ref(&after), &dots));
        let before = route_hits(&[before], &dots, MarkClearance::DotGap);
        let after = route_hits(&[after], &dots, MarkClearance::DotGap);
        assert!(before.contains(&RouteHit::Leg(key, 0)));
        assert!(!after.contains(&RouteHit::Leg(key, 0)));
        assert!(!after.is_subset(&before));
    }

    #[test]
    fn fallback_checks_heads_against_dots_and_stop_bars() {
        let key = (1, 1);
        let arrow = Drawn {
            key, pieces: vec![((0.0, 20.0), (30.0, 20.0))],
            tri: vec![(30.0, 14.0), (43.0, 20.0), (30.0, 26.0)],
            ..Default::default()
        };
        let dots = DotLayer {
            positions: BTreeSet::from([(39, 20)]), bars: vec![(40.0, 22.0)],
            ..Default::default()
        };
        let hits = route_hits(&[arrow], &dots, MarkClearance::DotGap);
        assert!(hits.contains(&RouteHit::Dot(key, 0)));
        assert!(hits.contains(&RouteHit::StopBar(key, 0)));
    }

    #[test]
    fn a_shadowed_binding_is_its_arrows_own_endpoint() {
        let mut dots = DotLayer::default();
        let mut column = arm(Vec::new()).t_data;
        column.name = format!("x{}", crate::expr_visitor_utils::SCOPE_MARK);
        dots.add_event(2, 20, 30, &column, String::new(), false, None, "static_lend", 1);
        let mut a = Drawn {
            var: "x".into(), columns: (Some(2), None),
            pieces: vec![((25.25, 30.0), (60.0, 30.0))],
            ..Default::default()
        };
        assert!(dot_and_bar_hits(std::slice::from_ref(&a), &dots, MarkClearance::DotGap).is_empty());
        a.columns.0 = Some(1);
        assert_eq!(dot_and_bar_hits(&[a], &dots, MarkClearance::DotGap), vec![(0, false, 0)]);
    }

    #[test]
    fn routing_counts_the_dots_stroke() {
        let arrow = Drawn {
            pieces: vec![((10.0, 20.25), (50.0, 20.25))],
            ..Default::default()
        };
        let dots = DotLayer { positions: BTreeSet::from([(30, 29)]), ..Default::default() };
        assert_eq!(dot_and_bar_hits(&[arrow], &dots, MarkClearance::DotGap), vec![(0, false, 0)]);
    }

    #[test]
    fn routing_counts_the_painted_corner_of_a_bend() {
        let arrow = Drawn {
            pieces: vec![((15.0, 20.0), (40.0, 20.0)), ((40.0, 20.0), (40.0, 40.0))],
            ..Default::default()
        };
        // Both centerlines are sqrt(85) away. The miter at (42.5, 17.5)
        // passes within sqrt(32.5) of the dot, leaving less than 1 px.
        let dots = DotLayer { positions: BTreeSet::from([(46, 13)]), ..Default::default() };
        assert_eq!(dot_and_bar_hits(&[arrow], &dots, MarkClearance::DotGap), vec![(0, false, 0)]);
    }

    #[test]
    fn a_head_corner_can_crowd_a_dot_over_twenty_pixels_from_the_tip() {
        let arrow = Drawn {
            tri: vec![(30.4, 14.5), (43.15, 20.5), (30.4, 26.5)],
            ..Default::default()
        };
        let dots = DotLayer { positions: BTreeSet::from([(25, 12)]), ..Default::default() };
        assert_eq!(dot_and_bar_hits(&[arrow], &dots, MarkClearance::DotGap), vec![(0, false, 0)]);
    }

    #[test]
    fn high_tracks_need_repair_even_without_dot_hits() {
        let key = (1, 1);
        let mut dots = DotLayer::default();
        dots.lanes.ends.insert(key, ((100, 80), (0, 80), 80));
        let mut arrow = Drawn { key, pieces: vec![((24.0, 40.0), (76.0, 40.0))], ..Default::default() };
        assert!(needs_route_repair(std::slice::from_ref(&arrow), &dots));

        dots.positions.insert((50, 40));
        let (_, pieces, arcs, _) = track_hops(&[(0.0, 80.0), (24.0, 40.0), (76.0, 40.0), (100.0, 80.0)], &dots.positions).unwrap();
        arrow.pieces = pieces;
        arrow.arcs = arcs;
        let drawing = [arrow];
        assert!(dot_and_bar_hits(&drawing, &dots, MarkClearance::DotGap).is_empty());
        assert_eq!(high_track_length(&drawing[0], &dots.lanes.ends), 52.0);
        assert!(needs_route_repair(&drawing, &dots));
    }

    #[test]
    fn a_repair_cannot_put_a_crossing_arc_through_another_head() {
        let dots = DotLayer::default();
        let head = Drawn {
            key: (1, 1), tip: "head".into(),
            tri: vec![(55.0, 37.0), (43.0, 40.0), (55.0, 43.0)],
            ..Default::default()
        };
        let route = [(15.0, 52.0), (25.0, 50.0), (75.0, 50.0)];
        let (_, pieces, arcs, _) = track_hops(&route, &BTreeSet::from([(50, 50)])).unwrap();
        let shaft = Drawn { key: (2, 2), tip: "hop".into(), pieces, ..Default::default() };
        let before = route_hits(&[head.clone(), shaft.clone()], &dots, MarkClearance::DotGap);
        let after = route_hits(&[head, Drawn { arcs, ..shaft }], &dots, MarkClearance::DotGap);
        assert!(!after.is_subset(&before));
        assert_eq!(single_route_blocker(&before, &after, (2, 2)), Some((1, 1)));
    }

    #[test]
    fn both_arrows_in_a_pair_can_take_a_local_bend() {
        let mut lanes = ArrowLanes::default();
        let (a, b) = ((1, 1), (2, 1));
        lanes.ends.insert(a, ((200, 80), (20, 80), 80));
        lanes.ends.insert(b, ((20, 80), (200, 80), 80));
        let first = lanes.repair_alternatives(a, &[], (20, 120), false, true).into_iter()
            .find(|s| s.leg_turn.get(&a) == Some(&12)).unwrap();
        lanes.set(first);
        for far in [false, true] {
            assert!(lanes.repair_alternatives(b, &[], (20, 120), far, true).iter()
                .any(|s| s.leg_turn.get(&a) == Some(&12) && s.leg_turn.get(&b) == Some(&-12)));
        }
    }

    #[test]
    fn fallback_checks_for_a_crossing_with_the_other_arrow() {
        let a = Drawn { key: (1, 1), pieces: vec![((0.0, 15.0), (40.0, 15.0))], ..Default::default() };
        let b = Drawn { key: (2, 2), pieces: vec![((20.0, 0.0), (20.0, 30.0))], ..Default::default() };
        assert!(route_hits(&[a, b], &DotLayer::default(), MarkClearance::DotGap).contains(&RouteHit::Crossing((1, 1), (2, 2), 0)));
    }

    /// #1468: a level return on the track half a row below its dots, and
    /// a binding arrow rising across that track at under 5 degrees: their
    /// strokes lie on each other for 47 px, a mark `untangle` redraws.
    /// Crossing it at 30 degrees, they touch for 10 px: no mark.
    #[test]
    fn shafts_on_each_other_at_a_shallow_angle_are_marks() {
        let arrow = |tip: &str, pts: &[(f64, f64)]| Drawn {
            tip: tip.into(),
            pieces: pts.windows(2).map(|w| (w[0], w[1])).collect(),
            ..Default::default()
        };
        let ret = arrow("pair>a", &[(305.8, 218.15), (290.0, 230.0), (90.0, 230.0), (84.4, 225.8)]);
        let slant = arrow("a>x", &[(100.23, 244.56), (432.06, 216.52)]);
        let steep = arrow("a>x", &[(150.0, 260.0), (300.0, 173.4)]);
        let shared = shared_stretch(ret.pieces[1], slant.pieces[0]);
        assert!((shared - 47.0).abs() < 1.0, "{shared}");
        assert_eq!(marks_on_others(&[ret.clone(), slant], &HashMap::new()), vec![(0, 1)]);
        assert!(marks_on_others(&[ret.clone(), steep], &HashMap::new()).is_empty());
        // On one track, the common stretch.
        let along = arrow("b>c", &[(150.0, 230.0), (400.0, 230.0)]);
        assert_eq!(shared_stretch(ret.pieces[1], along.pieces[0]), 140.0);
    }

    #[test]
    fn arms_are_placed_symmetrically() {
        // A lone arm (`if` without `else`) sits to the right.
        assert_eq!(centers_of(&leaves(1)), vec![1]);
        assert_eq!(centers_of(&leaves(2)), vec![-1, 1]);
        // Odd counts used to skew right: [-1, 1, 3] (#119).
        assert_eq!(centers_of(&leaves(3)), vec![-2, 0, 2]);
        assert_eq!(centers_of(&leaves(4)), vec![-3, -1, 1, 3]);
        assert_eq!(centers_of(&leaves(5)), vec![-4, -2, 0, 2, 4]);
        assert_eq!(compute_extent(&leaves(3)), (2, 2));
        assert_eq!(compute_extent(&leaves(5)), (4, 4));
    }

    #[test]
    fn middle_arm_with_nested_branch_pushes_neighbours_out() {
        // 3 arms, the middle one holding an if/else (leaves at -1, +1
        // around it): the outer arms clear its leaves by 2 units.
        let events = branch(vec![arm(Vec::new()), arm(leaves(2)), arm(Vec::new())]);
        assert_eq!(centers_of(&events), vec![-3, 0, 3]);
        assert_eq!(compute_extent(&events), (3, 3));
    }

    #[test]
    fn level_in_bends() {
        // Head at (340, 1115), tail right of it at (960, 1115), track a
        // row below: from past the dot, level, then 14 degrees off level.
        assert_eq!(level_in_bend(Some(0), -1, (340, 1115), (960, 1115), 1145, 20), Some([(314, 1115), (314, 1145)]));
        assert_eq!(level_in_bend(Some(14), -1, (340, 1115), (960, 1115), 1145, 20), Some([(315, 1121), (314, 1145)]));
        // On the tail's side; not asked for; out of the figure.
        assert_eq!(level_in_bend(Some(0), 1, (340, 1115), (960, 1115), 1145, 20), Some([(366, 1115), (386, 1145)]));
        assert_eq!(level_in_bend(None, 1, (340, 1115), (960, 1115), 1145, 20), None);
        assert_eq!(level_in_bend(Some(0), -1, (30, 1115), (960, 1115), 1145, 20), None);
    }

    // The state after `ManuallyDrop::drop(&mut md)` isn't drawn (like
    // any moved-out state, it has no line), so check its text here.
    #[test]
    fn dropped_by_hand_state_message() {
        let st = State::ResourceMoved {
            move_to: ResourceTy::Anonymous,
            move_at_line: 3,
            dropped_by_hand: true,
        };
        let msg = st.print_message_with_name(&"md".to_owned());
        assert!(msg.contains("'s value was dropped by hand; it must not be used again"), "{msg}");
        assert!(!msg.contains("moved"), "{msg}");
    }
}

