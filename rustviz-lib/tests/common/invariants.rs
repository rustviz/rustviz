//! Properties every rendered program must have, checked on every snippet
//! we have: `rustviz-plugin/tests/*.rs`, `edition2024/`, the Rust Book
//! corpus (`book/`) and the repros of open bugs (`known_bugs/`).
//!
//! `corpus.rs` and `book.rs` are example-based: each snippet asserts a few
//! tooltips, so a wrong picture that nobody thought to forbid passes. The
//! checks here don't know what any program should look like; they look for
//! pictures that contradict themselves, the way a student reading the
//! diagram would: a column used after it ended, a value dropped where
//! nothing holds one, a borrow returned that was never lent, a mutable
//! borrow ended as an immutable one.
//!
//! They read the structured `data-rv-*` attributes
//! (`rustviz-plugin/docs/element-ids.md`) and the drawn geometry: a dot is
//! *reached* by its column when a live segment of that column ends at the
//! dot or runs through it, and it *continues* when one starts at the dot.
//! Live means drawn solid, hollow or dotted (lent), including the dashed
//! segments of a conditional's other arms; not the dashed "no value yet"
//! segments. A few checks also read a fixed phrase of a tooltip where the
//! SVG has no structure for it; those phrases are in `MAYBE_PHRASES` and
//! `contents_moved`.
//!
//! `KNOWN_INVARIANT_FAILURES` lists the violations main has today, each
//! with the issue that tracks it. The test fails on a violation that isn't
//! listed, and on a listed one that no longer happens: a fix PR deletes its
//! entries (and may move the repro from `known_bugs/` into the corpus with
//! `EXPECTED_TOOLTIPS` entries in `corpus.rs`).
//!
//! `geometry.rs` adds checks of the drawing itself (a column that starts
//! or ends in mid-air, a segment drawn past a fork dot, two arrows along
//! one stretch, ...); they share this file's list and test.
//!
//! The test is `corpus_invariants_hold` in `corpus.rs` (it reuses that
//! binary's renders). `RV_INVARIANTS_ONLY=a,b` checks only the snippets
//! whose name starts with one of the prefixes (`book/c4`, `known_bugs/`);
//! `RV_INVARIANTS_EXTRA=<dir>` also checks every `.rs` file in `<dir>`
//! (a program generator's output, a batch of bug repros), so
//! `RV_INVARIANTS_EXTRA=<dir> RV_INVARIANTS_ONLY=<dir>` checks just those.
//!

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Violations main has today: (snippet, invariant, issue). Snippet names
/// are file stems, with `edition2024/`, `book/` or `known_bugs/` before
/// the ones in those directories. One entry covers every violation of
/// that invariant in that snippet.
pub const KNOWN_INVARIANT_FAILURES: &[(&str, &str, u32)] = &[
    // A struct moved as a whole while a field column goes on.
    ("ctor_literals_768", "field-outlives-moved-struct", 1127),
    // Geometric (`geometry.rs`), from here on.
    // Tuple column goes on after its fields were moved out.
    ("edition2024/if_let_chain_irrefutable_move", "line-ends-nowhere", 1157),
];

/// The invariants, by the name the failure list and messages use.
pub const INVARIANTS: &[(&str, &str)] = &[
    ("invalid-state",
     "the plugin's state machine reached State::Invalid (no rule for an event in the column's state), \
      so the column draws nothing from there on (data-rv-invalid on the timeline's <svg>)"),
    ("event-on-dead-column",
     "a column is lent, moved, copied or has a borrow end on a row its line doesn't reach \
      (after it was moved or before it has a value), with no event on that dot giving it a value"),
    ("drop-without-value",
     "a drop mark on a column whose line doesn't reach it: nothing is there to drop \
      (not counting a drop flag after a conditional move, or a Box whose contents were moved out)"),
    ("line-after-column-ends",
     "a column's line goes on after a dot that moves its value away or ends its scope, \
      with nothing on that dot giving it a new value"),
    ("owner-after-maybe-moved-join",
     "a join dot says the value may have been moved, but the column's line goes on after it \
      (Rust doesn't let it be used until it's assigned again)"),
    ("borrow-kind-mismatch",
     "a borrow ends as the other kind than it started: a mutable loan given back as an immutable one \
      or the reverse (an arrow back, or a column's borrow end)"),
    ("return-without-lend",
     "an arrow gives a mutable borrow back to a column that never lent it (no borrow arrow to the \
      reference or what it was copied/moved from, no mutable lend on the lender)"),
    ("self-borrow",
     "a borrow arrow from a column to itself"),
    ("field-outlives-moved-struct",
     "a struct column is moved away as a whole, but a field's column goes on as if it still \
      held its value"),
    ("drop-of-moved-value-at-reassign",
     "one dot moves a column's value out and then says its previous value is dropped when it's \
      assigned: the right-hand side runs first, so there's nothing left to drop"),
    ("column-ends-without-scope-end",
     "an owner's column ends on a dot that neither moves it nor ends its scope, so its drop \
      (or that nothing is dropped) is never stated"),
    // Geometric: where things are drawn (`geometry.rs`).
    ("line-starts-nowhere",
     "a column's segment starts where none of its dots, other segments or (for a struct) its fields' \
      dots are: the column appears out of nowhere"),
    ("line-ends-nowhere",
     "a column's segment ends where none of its dots, other segments, stop bars or (for a struct) its \
      fields' dots are: it stops in mid-air"),
    ("line-through-fork",
     "a column's segment runs through a fork or join dot and on past it, so it reads as one more arm"),
    ("arrows-overlap",
     "two arrows with different tooltips run along the same stretch (on one track, or one slanting across \
      the other's at a shallow angle), so only the one on top can be hovered"),
    ("marks-on-one-spot",
     "two dots of one column, or two function logos, drawn on the same spot with different tooltips: \
      only one can be hovered"),
    ("arrow-misses-target",
     "an arrow to a column ends at none of that column's dots"),
    ("outside-figure",
     "an element is drawn outside the timeline's width or height"),
    ("fork-without-dot",
     "two legs of a column leave a point in different directions (a fork) or meet there (a join) \
      with no dot of that column there: the conditional's split or join is drawn in mid-line"),
    ("heads-or-logos-overlap",
     "an arrow's head lies on another arrow's shaft or head, or a function logo lies on an arrow not its \
      own or on another logo: the marks under it can't be told apart or hovered"),
    ("shaft-through-mark",
     "an arrow's shaft runs through the centre of a dot, or along a column's stop bar, with no hop: the \
      mark reads as part of the arrow and can't be hovered"),
    ("arrows-end-to-end",
     "one arrow leaves the dot another points at straight on in the same direction: the two read as \
      one arrow through the dot"),
];

// ─── Reading a timeline ─────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Dot {
    id: String,
    var: String,
    hash: String,
    line: u32,
    x: f64,
    y: f64,
    events: Vec<String>,
    drop: bool,
    tip: String,
}

impl Dot {
    fn has(&self, e: &str) -> bool {
        self.events.iter().any(|x| x == e)
    }
    fn has_any(&self, es: &[&str]) -> bool {
        es.iter().any(|e| self.has(e))
    }
    fn pos(&self, e: &str) -> Option<usize> {
        self.events.iter().position(|x| x == e)
    }
    fn describe(&self) -> String {
        format!("{}: {}", self.id, short(&self.tip))
    }
}

#[derive(Debug, Clone)]
struct Seg {
    id: String,
    var: String,
    hash: String,
    /// `line` (a column outside conditionals), not `branch_line`.
    main: bool,
    state: String,
    lines: Vec<(f64, f64, f64, f64)>,
}

impl Seg {
    /// Drawn as a live column: not "no value yet", not moved out.
    fn live(&self) -> bool {
        ["full_privilege", "partial_privilege", "revoked_privilege"]
            .iter()
            .any(|s| self.state == *s || self.state == format!("{}_gray", s))
    }
    fn gray(&self) -> bool {
        self.state.ends_with("_gray")
    }
}

#[derive(Debug, Clone)]
struct Arrow {
    id: String,
    var: String,
    to: String,
    event: String,
    line: u32,
    tip: String,
}

#[derive(Default)]
pub struct Fig {
    dots: Vec<Dot>,
    segs: Vec<Seg>,
    arrows: Vec<Arrow>,
    /// Column name -> x of its header, by hash.
    header_x: HashMap<String, f64>,
    header_vars: HashSet<String>,
    /// Hashes of reference columns (they have a bracket).
    ref_hashes: HashSet<String>,
    invalid: String,
    /// Where things are drawn, for the geometric invariants.
    geo: super::geometry::Geo,
}

/// Tooltip text without its markup: `<br>` becomes " / ".
fn plain(t: &str) -> String {
    let t = t.replace("<br>", " / ");
    let mut out = String::with_capacity(t.len());
    let mut in_tag = false;
    for c in t.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn short(t: &str) -> String {
    let mut s: String = t.chars().take(180).collect();
    if t.chars().count() > 180 {
        s.push('…');
    }
    s
}

fn num(n: roxmltree::Node, a: &str) -> f64 {
    n.attribute(a).and_then(|v| v.parse().ok()).unwrap_or(f64::NAN)
}

pub fn parse(svg: &str) -> Fig {
    let doc = roxmltree::Document::parse(svg).expect("well-formed timeline SVG");
    let mut fig = Fig {
        invalid: doc.root_element().attribute("data-rv-invalid").unwrap_or("").to_owned(),
        geo: super::geometry::parse(&doc),
        ..Default::default()
    };
    for n in doc.descendants().filter(|n| n.attribute("data-rv-id").is_some()) {
        let a = |k: &str| n.attribute(k).unwrap_or("").to_owned();
        let hash = n.descendants().find_map(|c| c.attribute("data-hash")).unwrap_or("").to_owned();
        let id = a("data-rv-id");
        let var = a("data-rv-var");
        let part = n.attribute("data-rv-part");
        match n.attribute("data-rv-kind").unwrap_or("") {
            "dot" => {
                let Some(c) = n.descendants().find(|c| c.has_tag_name("circle")) else { continue };
                fig.dots.push(Dot {
                    id, var, hash,
                    line: a("data-rv-line").parse().unwrap_or(0),
                    x: num(c, "cx"),
                    y: num(c, "cy"),
                    events: a("data-rv-event").split(' ').filter(|e| !e.is_empty()).map(str::to_owned).collect(),
                    drop: n.attribute("data-rv-drop") == Some("true"),
                    tip: plain(&a("data-tooltip-text")),
                });
            }
            kind @ ("line" | "branch_line") => {
                // A conditional's arm segment is drawn by its parts; its
                // hover polygon (no part) and bevels carry no line.
                if kind == "branch_line" && !matches!(part, Some("line" | "side1" | "side2")) {
                    continue;
                }
                let lines = n.descendants().filter(|c| c.has_tag_name("line"))
                    .map(|l| (num(l, "x1"), num(l, "y1"), num(l, "x2"), num(l, "y2")))
                    .collect();
                fig.segs.push(Seg { id, var, hash, main: kind == "line", state: a("data-rv-state"), lines });
            }
            "arrow" => fig.arrows.push(Arrow {
                id, var,
                to: a("data-rv-to"),
                event: a("data-rv-event"),
                line: a("data-rv-line").parse().unwrap_or(0),
                tip: plain(&a("data-tooltip-text")),
            }),
            "header" => {
                fig.header_x.entry(hash).or_insert(num(n, "x"));
                fig.header_vars.insert(var);
            }
            "bracket" => {
                fig.ref_hashes.insert(hash);
            }
            _ => {}
        }
    }
    fig
}

/// How a column's segment meets a dot at (x, y): (reaches it, continues
/// from it). A dot's circle has radius 5 and a hollow segment's two sides
/// sit 1.75 px either side of the centre, so ends within 4.6 px count.
fn meets(seg: &Seg, x: f64, y: f64) -> (bool, bool) {
    const TOL: f64 = 4.6;
    let (mut reach, mut cont) = (false, false);
    for &(x1, y1, x2, y2) in &seg.lines {
        let (x1, y1, x2, y2) = if y1 > y2 { (x2, y2, x1, y1) } else { (x1, y1, x2, y2) };
        if (x1 - x2).abs() < 0.01 && (x1 - x).abs() <= TOL && y1 + 2.0 < y && y < y2 - 2.0 {
            reach = true;
            cont = true;
        }
        if (x2 - x).powi(2) + (y2 - y).powi(2) <= TOL * TOL && y1 < y - 2.0 {
            reach = true;
        }
        if (x1 - x).powi(2) + (y1 - y).powi(2) <= TOL * TOL && y2 > y + 2.0 {
            cont = true;
        }
    }
    (reach, cont)
}

// ─── The invariants ─────────────────────────────────────────────────

/// Events that give a column a value (or a borrow).
const INIT: &[&str] = &[
    "acquire", "copy", "static_borrow", "mutable_borrow", "init_ref_param",
    "loop_maybe_init", "loop_exit_kept", "or_alt_borrow",
];
/// Events after which a column holds nothing.
const END: &[&str] = &[
    "move", "owner_go_out_of_scope", "ref_go_out_of_scope", "manually_drop_by_hand",
    "loop_exit_move", "or_alt_move", "closure_call", "loop_iterator_drop", "owner_drop_unless_returned",
];
/// Uses of a column's value.
const USE: &[&str] = &["static_lend", "mutable_lend", "duplicate", "move", "or_alt_lend"];
/// Ends of a borrow (the column's own, or a loan of it).
const BORROW_END: &[&str] = &["static_die", "mutable_die", "static_reacquire", "mutable_reacquire"];
/// Drops of something that came out of the column (a temporary, the
/// tuple a scrutinee built, a `for` loop's iterator): they're drawn on
/// the column after its value left it.
const TEMP_DROPS: &[&str] = &["tuple_temp_drop", "loop_iterator_drop", "temporary_drop"];
/// Events after which a column may or may not hold its value (a drop flag
/// decides): a join or a loop exit that moved it on some paths.
const MAYBE_EVENTS: &[&str] = &["loop_exit_move", "loop_maybe_init", "or_alt_move", "owner_drop_if_try_returns"];
/// What a dot says when the column may have been moved (a join after a
/// conditional move, `break a` out of a loop). The join dot carries no
/// structured mark for it.
const MAYBE_PHRASES: &[&str] = &[
    "moved in at least one branch", "may have been moved", "may have no value after",
    "moved out in at least one branch",
];

/// A move of what a Box points to (`let s = *b;`): the Box itself stays,
/// and is freed (only its heap allocation) at scope end. Its dot names the
/// place `*b`.
fn contents_moved(d: &Dot) -> bool {
    d.tip.starts_with('*')
}

/// `b = Box::new(*b);` (#1423): the right-hand side moves what the Box
/// points to out first, and the assignment then frees only the old
/// Box's heap allocation, as the dot says. Its move names `*b`, and its
/// drop says only the allocation is freed.
fn reboxes_contents(d: &Dot) -> bool {
    d.tip.split(" / ").any(|part| part.trim_start_matches("• ").starts_with(&format!("*{}'s ", d.var)))
        && d.tip.contains("contents were moved out earlier, so only its previous Box's heap allocation is freed")
}

/// A slice pattern's move of some elements out of an array that keeps
/// the rest (`[c, _] => …`, #1285), or the join after branches that did:
/// the array's line goes on, holding the elements left. Its dot names
/// the elements (`pair[0]'s resource is moved`).
fn elems_moved(d: &Dot) -> bool {
    d.tip.trim_start_matches("• ").starts_with(&format!("{}[", d.var))
}

fn says_maybe_moved(d: &Dot) -> bool {
    MAYBE_PHRASES.iter().any(|p| d.tip.contains(p))
}

/// What a scope end says when only part of the value is left to drop,
/// or a drop flag decides: a struct whose fields were moved out (one
/// in a `match` arm, an or-pattern binding from either field, one a
/// `loop` moves on some rounds), a value a branch or a `break` moved
/// on some paths only, a Box re-boxed on some paths. The column rightly
/// stops where the (partial or maybe) move made the whole unusable, and
/// the drop of what's left is stated on its own dot below, which no
/// line reaches. (A Box whose contents were moved out says "Its
/// contents were moved out earlier, so only the Box's heap allocation";
/// `contents_moved` covers it.)
const PARTIAL_DROP_PHRASES: &[&str] = &["drop flag", "moved out earlier, so only"];
/// A move drawn on the column's own line that says it happens on some
/// paths only (`break f` in a loop whose value is returned, #1171).
const CONDITIONAL_MOVE_PHRASES: &[&str] = &["if the loop ended with"];

fn states_partial_drop(d: &Dot) -> bool {
    PARTIAL_DROP_PHRASES.iter().any(|p| d.tip.contains(p)) && !d.tip.contains("Its contents were moved out earlier")
}

/// `break k` out of a `loop` is drawn where the loop ends, on the dot
/// where `k` goes out of scope on the iterations that don't break
/// (#431, #1156). When the body moves `k` on those iterations
/// (`take(k)` after the `if … { break k; }`), its line has ended above,
/// but on the iteration that breaks it still held its value.
/// The same for a local of a labeled block carried out by one of its ways
/// out, drawn where the block ends (#1288: "k's resource is moved to r if
/// the block ended with break 'blk k").
fn break_out_move(d: &Dot) -> bool {
    d.has("move") && (d.tip.contains("break that ends the loop") || block_local_break_move(d))
}

/// A move of a labeled block's local by one of the block's ways out
/// (#1288): on the paths that leave the block another way, the local is
/// still there, and dropped where the block ends, below.
fn block_local_break_move(d: &Dot) -> bool {
    d.has("move") && d.tip.contains("if the block ended with")
        && !d.tip.contains("may have been moved")
}

/// Line `line` of `src` is a `for` loop's head (`for x in r {`,
/// `'outer: for x in r {`).
fn is_for_head(src: &str, line: u32) -> bool {
    let Some(l) = src.lines().nth((line as usize).wrapping_sub(1)) else { return false };
    let l = l.trim_start();
    let l = match l.strip_prefix('\'') {
        Some(rest) => rest.split_once(':').map_or(l, |(_, r)| r.trim_start()),
        None => l,
    };
    l.starts_with("for ") && l.contains(" in ")
}

/// Every violation in one timeline: (invariant, what and where).
pub fn check(fig: &Fig, src: &str) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = super::geometry::check(&fig.geo);
    if !fig.invalid.is_empty() {
        out.push(("invalid-state", fig.invalid.clone()));
    }
    // `// rustviz: skip` hides a line's events (the fixture asks for an
    // incomplete picture), so a column can look used before it has a value.
    let skip = src.contains("rustviz: skip");

    let mut live: HashMap<&str, Vec<&Seg>> = HashMap::new();
    for s in fig.segs.iter().filter(|s| s.live()) {
        live.entry(s.hash.as_str()).or_default().push(s);
    }
    let mut dots_of: HashMap<&str, Vec<&Dot>> = HashMap::new();
    for d in &fig.dots {
        dots_of.entry(d.hash.as_str()).or_default().push(d);
    }
    for ds in dots_of.values_mut() {
        ds.sort_by(|a, b| a.y.total_cmp(&b.y));
    }
    let meets_col = |d: &Dot, solid_only: bool| {
        let (mut r, mut c) = (false, false);
        for s in live.get(d.hash.as_str()).into_iter().flatten() {
            if solid_only && s.gray() {
                continue;
            }
            let (a, b) = meets(s, d.x, d.y);
            r |= a;
            c |= b;
        }
        (r, c)
    };

    let on_main = |d: &Dot| fig.header_x.get(&d.hash).is_some_and(|x| (x - d.x).abs() < 1.0);
    // A move of the whole value on the column's own line, on every path
    // as drawn: after it, a scope end that says part of the value is
    // left, or that a drop flag decides, contradicts the picture (the
    // reason the value may still be there isn't drawn).
    let whole_move = |p: &Dot| p.has("move") && on_main(p) && !contents_moved(p)
        && !says_maybe_moved(p) && !CONDITIONAL_MOVE_PHRASES.iter().any(|c| p.tip.contains(c));
    for d in &fig.dots {
        let (reached, _) = meets_col(d, false);
        // A conditional's other arms (dashed) don't carry this path's value on.
        let (_, continues) = meets_col(d, true);
        let inits = d.has_any(INIT);
        let split_or_join = d.has_any(&["branch", "branch_join"]);
        let earlier: Vec<&&Dot> = dots_of[d.hash.as_str()].iter().filter(|p| p.y < d.y).collect();
        // A join that some arm reaches holding the value but no line leaves:
        // other arms (that reach it) don't hold it, so it may hold one or not.
        let maybe = earlier.iter().any(|p| p.has_any(MAYBE_EVENTS) || says_maybe_moved(p)
            || (p.has("branch_join") && meets_col(p, false).0 && !meets_col(p, true).1));

        if !reached && !inits && !split_or_join && !skip {
            // `for x in r` (r a `&mut`) moves r into the loop's iterator,
            // which holds r's loan until the loop ends; that end is drawn
            // on r's column (the `for` loop has none, #1129).
            let into_for = fig.ref_hashes.contains(&d.hash)
                && earlier.iter().rev().find(|p| p.has_any(END))
                    .is_some_and(|p| p.has("move") && is_for_head(src, p.line));
            let uses = d.events.iter().any(|e| USE.contains(&e.as_str()) && !(e == "move" && break_out_move(d)));
            // A maybe-moved value can't be used, but a loan of it can still end.
            if uses || (d.has_any(BORROW_END) && !maybe && !into_for) {
                out.push(("event-on-dead-column", d.describe()));
            }
            if d.drop && !d.has_any(TEMP_DROPS) {
                let box_contents = earlier.iter().rev().find(|p| p.has("move")).is_some_and(|p| contents_moved(p));
                let partial = states_partial_drop(d)
                    && !earlier.iter().rev().find(|p| p.has_any(END)).is_some_and(|p| whole_move(p));
                if !maybe && !box_contents && !partial {
                    out.push(("drop-without-value", d.describe()));
                }
            }
        }
        if continues && !inits && d.has_any(END) && !split_or_join && !contents_moved(d) && !elems_moved(d)
            && !block_local_break_move(d) {
            out.push(("line-after-column-ends", d.describe()));
        }
        if d.has("branch_join") && says_maybe_moved(d) && continues && !inits && !elems_moved(d) {
            out.push(("owner-after-maybe-moved-join", d.describe()));
        }
        if let (Some(m), Some(r)) = (d.pos("move"), d.pos("owner_drop_at_reassign")) {
            if m < r && !reboxes_contents(d) {
                out.push(("drop-of-moved-value-at-reassign", d.describe()));
            }
        }
        // A whole struct moved away from its own column (not in an arm).
        if d.has("move") && !inits && !continues && on_main(d) && !contents_moved(d) && !d.var.contains('.') {
            let prefix = format!("{}.", d.var);
            for s in fig.segs.iter().filter(|s| s.main && s.live() && s.var.starts_with(&prefix)) {
                if s.lines.iter().any(|&(_, y1, _, y2)| y1.min(y2) <= d.y + 0.5 && y1.max(y2) > d.y + 2.0) {
                    out.push(("field-outlives-moved-struct", format!("{} after {}", s.id, d.describe())));
                }
            }
        }
    }

    // A column's borrows end as the kind they started. Except a reborrow
    // through a reference that's re-pointed while the reborrow is live
    // (`let first = &*r; r = &mut b;`, #1182): r's old loan of `a` lasts
    // as long as `first` is used, so `first`'s last use gives `a`'s
    // *mutable* loan back (arrows a -> r -> first, then first -> a).
    let via_reborrow = |d: &Dot, e: &str| {
        let kind = e.split('_').next().unwrap_or("");
        let borrow = format!("{kind}_borrow");
        fig.arrows.iter().any(|ret| ret.var == d.var && ret.event == e && ret.line == d.line
            && fig.arrows.iter().any(|lend| lend.var == ret.to && lend.event == borrow && lend.line <= d.line
                && fig.arrows.iter().any(|re| re.var == lend.to && re.to == d.var && re.line <= d.line
                    && matches!(re.event.as_str(), "static_borrow" | "mutable_borrow"))))
    };
    for ds in dots_of.values() {
        let (mut borrows, mut lends) = (BTreeSet::new(), BTreeSet::new());
        for d in ds {
            for e in &d.events {
                let kind = e.split('_').next().unwrap_or("");
                match e.as_str() {
                    "static_borrow" | "mutable_borrow" => { borrows.insert(kind); }
                    "static_lend" | "mutable_lend" => { lends.insert(kind); }
                    "static_die" | "mutable_die" if !borrows.is_empty() && !borrows.contains(kind) && !via_reborrow(d, e) =>
                        out.push(("borrow-kind-mismatch", d.describe())),
                    "static_reacquire" | "mutable_reacquire" if !lends.is_empty() && !lends.contains(kind) =>
                        out.push(("borrow-kind-mismatch", d.describe())),
                    _ => {}
                }
            }
        }
    }

    // Owners end by moving away or going out of scope, saying what's dropped.
    if !skip {
        for (h, ds) in &dots_of {
            if fig.ref_hashes.contains(*h) {
                continue;
            }
            let last = ds.last().unwrap();
            // A scrutinee temporary a binding can empty is one dot that
            // owns nothing; its tooltip states the drop on the paths where
            // no binding takes the value (#1367).
            let taken_temp = ds.len() == 1 && last.has("acquire")
                && last.tip.contains("what the temporary holds is dropped with it");
            if !last.has_any(END) && !last.has_any(&["tuple_temp_drop", "temporary_drop", "dead_else_note"]) && !taken_temp {
                out.push(("column-ends-without-scope-end", last.describe()));
            }
        }
    }

    // Arrows.
    let mut arrows: Vec<&Arrow> = fig.arrows.iter().collect();
    arrows.sort_by_key(|a| a.line);
    let is_borrow = |e: &str| e == "static_borrow" || e == "mutable_borrow";
    for a in &arrows {
        if is_borrow(&a.event) && a.var == a.to {
            out.push(("self-borrow", format!("{}: {}", a.id, a.tip)));
        }
        if a.event != "static_die" && a.event != "mutable_die" {
            continue;
        }
        // `a` gives back what `holder` borrowed from `lender`.
        let (holder, lender) = (&a.var, &a.to);
        let lent: Vec<&&Arrow> = arrows.iter()
            .filter(|b| is_borrow(&b.event) && &b.var == lender && &b.to == holder && b.line <= a.line)
            .collect();
        let shared = a.event == "static_die";
        if !lent.is_empty() && !lent.iter().any(|b| (b.event == "static_borrow") == shared) {
            out.push(("borrow-kind-mismatch", format!("{}: {}", a.id, a.tip)));
        }
        if !shared && lent.is_empty() && holder != lender && fig.header_vars.contains(lender) {
            // The loan may have reached `holder` by copies, moves and reborrows.
            let mut from: HashMap<&str, Vec<&str>> = HashMap::new();
            for b in arrows.iter().filter(|b| b.line <= a.line
                && matches!(b.event.as_str(), "static_borrow" | "mutable_borrow" | "copy" | "move")) {
                from.entry(b.to.as_str()).or_default().push(b.var.as_str());
            }
            let mut seen = HashSet::new();
            let mut stack = vec![holder.as_str()];
            let mut traced = false;
            while let Some(n) = stack.pop() {
                if !seen.insert(n) {
                    continue;
                }
                for &s in from.get(n).into_iter().flatten() {
                    traced |= s == lender.as_str();
                    stack.push(s);
                }
            }
            let lender_lent = fig.dots.iter().any(|d| &d.var == lender && d.has("mutable_lend") && d.line <= a.line);
            if !traced && !lender_lent {
                out.push(("return-without-lend", format!("{}: {}", a.id, a.tip)));
            }
        }
    }
    out
}

// ─── Comparing with the known list ──────────────────────────────────

/// What differs between `found` ((snippet, invariant) -> what violates
/// it, for the snippets in `checked`) and `KNOWN_INVARIANT_FAILURES`:
/// one paragraph per new violation, and per listed one that's gone.
pub fn report(found: &BTreeMap<(String, &'static str), Vec<String>>, checked: &HashSet<String>) -> String {
    let known: BTreeMap<(&str, &str), u32> =
        KNOWN_INVARIANT_FAILURES.iter().map(|(s, i, n)| ((*s, *i), *n)).collect();
    let mut out = String::new();
    for ((snippet, inv), whats) in found {
        if known.contains_key(&(snippet.as_str(), *inv)) {
            continue;
        }
        let why = INVARIANTS.iter().find(|(n, _)| n == inv).map_or("", |(_, w)| *w);
        out.push_str(&format!("\nNEW  {snippet}: {inv} ({why})\n"));
        for w in whats {
            out.push_str(&format!("       {w}\n"));
        }
    }
    for ((snippet, inv), issue) in &known {
        if checked.contains(*snippet) && !found.contains_key(&(snippet.to_string(), *inv)) {
            out.push_str(&format!(
                "\nGONE {snippet}: {inv} (#{issue}) no longer happens. Fixed? Delete its entry from \
                 KNOWN_INVARIANT_FAILURES (and check #{issue}).\n"));
        }
    }
    out
}

/// Every entry names an invariant, and no (snippet, invariant) twice.
pub fn known_list_problems() -> Vec<String> {
    let names: HashSet<&str> = INVARIANTS.iter().map(|(n, _)| *n).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (s, inv, issue) in KNOWN_INVARIANT_FAILURES {
        if !names.contains(inv) {
            out.push(format!("({s}, {inv}, #{issue}) names no invariant"));
        }
        if !seen.insert((*s, *inv)) {
            out.push(format!("({s}, {inv}) is listed twice"));
        }
    }
    out
}

/// The checks on hand-written timelines, so a change to them is tested
/// without the plugin.
#[cfg(test)]
mod unit {
    use super::*;

    fn svg(body: &str, root: &str) -> String {
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\"{root}>{body}</svg>")
    }
    fn dot(var: &str, hash: u32, y: u32, ev: &str, drop: bool, tip: &str) -> String {
        format!("<circle cx=\"70\" cy=\"{y}\" r=\"5\" data-hash=\"{hash}\" data-tooltip-text=\"{tip}\" \
                 data-rv-id=\"{var}@{y}:dot:{ev}\" data-rv-kind=\"dot\" data-rv-event=\"{ev}\" \
                 data-rv-var=\"{var}\"{}/>", if drop { " data-rv-drop=\"true\"" } else { "" })
    }
    fn line(var: &str, hash: u32, y1: u32, y2: u32, state: &str) -> String {
        format!("<line data-hash=\"{hash}\" x1=\"70\" y1=\"{y1}\" x2=\"70\" y2=\"{y2}\" \
                 data-rv-id=\"{var}@{y1}:line:{state}\" data-rv-kind=\"line\" data-rv-state=\"{state}\" \
                 data-rv-var=\"{var}\"/>")
    }
    fn header(var: &str, hash: u32) -> String {
        format!("<text x=\"70\" y=\"35\" data-hash=\"{hash}\" data-rv-id=\"hdr:{var}\" data-rv-kind=\"header\" data-rv-var=\"{var}\">{var}</text>")
    }
    /// The ownership invariants a hand-written timeline violates (these
    /// pictures leave out what doesn't matter to them, so their geometry,
    /// tested in `geometry.rs`, is incomplete).
    fn invs(body: &str) -> Vec<&'static str> {
        check(&parse(&svg(body, "")), "").into_iter().map(|(i, _)| i)
            .filter(|i| !super::super::geometry::INVARIANTS.contains(i)).collect()
    }

    #[test]
    fn a_moved_column_used_again_is_caught() {
        let ok = [header("s", 1), line("s", 1, 95, 155, "full_privilege"), dot("s", 1, 95, "acquire", false, "a"),
                  dot("s", 1, 155, "move", false, "m"), dot("s", 1, 215, "owner_go_out_of_scope", false, "o")].concat();
        assert_eq!(invs(&ok), Vec::<&str>::new());
        let used = [ok.clone(), dot("s", 1, 185, "static_lend static_reacquire", false, "b")].concat();
        assert_eq!(invs(&used), vec!["event-on-dead-column"]);
        let dropped = ok.replace("owner_go_out_of_scope\" data-rv-var=\"s\"/>", "owner_go_out_of_scope\" data-rv-var=\"s\" data-rv-drop=\"true\"/>");
        assert_eq!(invs(&dropped), vec!["drop-without-value"]);
        let on = ok.replace("y2=\"155\"", "y2=\"215\"");
        assert_eq!(invs(&on), vec!["line-after-column-ends"]);
    }

    #[test]
    fn invalid_states_and_kinds_are_caught() {
        assert_eq!(check(&parse(&svg("", " data-rv-invalid=\"s@L4-6\"")), "")[0].0, "invalid-state");
        let r = [header("r", 2), line("r", 2, 95, 155, "full_privilege"),
                 dot("r", 2, 95, "mutable_borrow", false, "b"), dot("r", 2, 155, "static_die ref_go_out_of_scope", false, "e")].concat();
        assert_eq!(invs(&r), vec!["borrow-kind-mismatch"]);
    }

    #[test]
    fn a_stated_partial_drop_needs_a_reason_on_the_column() {
        // A struct whose fields were moved out in arms: its line stops
        // with no move of its own, and the scope end says a drop flag
        // decides.
        let flag = "s goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).";
        let arms = [header("s", 1), line("s", 1, 95, 155, "full_privilege"), dot("s", 1, 95, "acquire", false, "a"),
                    dot("s", 1, 215, "owner_go_out_of_scope", true, flag)].concat();
        assert_eq!(invs(&arms), Vec::<&str>::new());
        // The same after a move of the whole value on every path as drawn:
        // nothing says why it may still be there.
        let moved = [arms.clone(), dot("s", 1, 155, "move", false, "s's resource is moved into drop, which drops it")].concat();
        assert_eq!(invs(&moved), vec!["drop-without-value"]);
        // A plain drop after a move is caught as before.
        let plain = moved.replace(flag, "s goes out of scope. Its resource is dropped.");
        assert_eq!(invs(&plain), vec!["drop-without-value"]);
    }

    #[test]
    fn a_break_value_moved_at_the_loops_end_is_allowed() {
        // `break k` drawn where the loop ends, after the body moved k on
        // the iterations that don't break (#1156).
        let k = [header("k", 1), line("k", 1, 95, 155, "full_privilege"), dot("k", 1, 95, "acquire", false, "a"),
                 dot("k", 1, 155, "move", false, "k's resource is moved"),
                 dot("k", 1, 185, "owner_go_out_of_scope move", false,
                     "k goes out of scope. No resource is dropped. / k's resource is moved to found by the break that ends the loop")].concat();
        assert_eq!(invs(&k), Vec::<&str>::new());
        let other = k.replace("by the break that ends the loop", "by take");
        assert_eq!(invs(&other), vec!["event-on-dead-column"]);
    }

    #[test]
    fn a_box_reboxing_its_own_contents_is_allowed() {
        // `b = Box::new(*b);` (#1423): `*b` moves out, then only the old
        // Box's allocation is freed.
        let tip = "• *b's resource is moved / • b acquires ownership of a new resource. Its previous contents were moved out earlier, so only its previous Box's heap allocation is freed here.";
        let b = [header("b", 1), line("b", 1, 95, 215, "full_privilege"), dot("b", 1, 95, "acquire", false, "a"),
                 dot("b", 1, 155, "move acquire owner_drop_at_reassign", false, tip),
                 dot("b", 1, 215, "owner_go_out_of_scope", true, "o")].concat();
        assert_eq!(invs(&b), Vec::<&str>::new());
        // A move of the whole Box, then a drop of its previous value, is
        // still caught: nothing is left to drop.
        let whole = b.replace(tip, "• b's resource is moved / • b acquires ownership of a new resource; its previous resource is dropped");
        assert_eq!(invs(&whole), vec!["drop-of-moved-value-at-reassign"]);
        // And so is a contents move whose drop text says the old value is dropped.
        let wrong = b.replace("Its previous contents were moved out earlier, so only its previous Box's heap allocation is freed here.", "Its previous resource is dropped.");
        assert_eq!(invs(&wrong), vec!["drop-of-moved-value-at-reassign"]);
    }

    #[test]
    fn an_owner_ending_silently_is_caught() {
        let s = [header("s", 1), line("s", 1, 95, 155, "full_privilege"), dot("s", 1, 95, "acquire", false, "a"),
                 dot("s", 1, 155, "static_lend static_reacquire", false, "r")].concat();
        assert_eq!(invs(&s), vec!["column-ends-without-scope-end"]);
    }
}
