extern crate handlebars;

use crate::svg_generator::data::{branch_is_collapsed, is_param_value_line, returned_closure, Event, ExtBranchData, ExternalEvent, ResourceAccessPoint, ResourceAccessPoint_extract, ResourceTy, StaticLender, Visualizable, VisualizationData, FIRST_ROW_Y, OLD_FIRST_ROW_Y, LINE_SPACE};
use crate::svg_generator::svg_frontend::{code_panel, rv_attrs, timeline_panel};
use handlebars::Handlebars;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use crate::svg_generator::svg_frontend::templates::*;
use crate::svg_generator::svg_frontend::themes::theme_css;
use log::info;
use crate::svg_generator::hover_messages::escape_tooltip_attrs;

#[derive(Serialize)]
struct SvgData {
    visualization_name: String,
    css: String,
    code: String,
    diagram: String,
    tl_id: String,
    tl_width: i32,
    height: i32,
    /// Attributes for the timeline's root `<svg>` that draw nothing
    /// (`rv_attrs::invalid_states_attr`).
    tl_rv: String,
}

/// Empty space kept below the last code line's text baseline, in px.
const BOTTOM_MARGIN: i32 = 25;

fn sort_branch_external_events(b_data: & mut ExtBranchData) {
    // sort first,
    for (_, event_vec) in b_data.line_map.iter_mut() {
        event_vec.sort_by(by_rap_hashes);
    }
    
    // then recurse
    for (_, ev) in b_data.e_data.iter_mut() {
        match ev {
            ExternalEvent::Branch { branches, ..} => {
                for branch in branches {
                    sort_branch_external_events(branch);
                }
            }
            _ => {}
        }
    }
}

/// The order `sort_line_map_events` / `sort_branch_external_events` draw
/// one line's stacked arrows in: by the hashes of their target, then their
/// source.
fn by_rap_hashes(a: &ExternalEvent, b: &ExternalEvent) -> std::cmp::Ordering {
    let (af, at) = ResourceAccessPoint_extract(a);
    let (bf, bt) = ResourceAccessPoint_extract(b);
    at.hash().cmp(&bt.hash()).then(af.hash().cmp(&bf.hash()))
}

/// A move or copy into a call: drawn first among the top-level line's
/// arrows (#200).
fn moves_into_fn(e: &ExternalEvent) -> bool {
    matches!(e, ExternalEvent::Move { .. } | ExternalEvent::Copy { .. })
        && ResourceAccessPoint_extract(e).1.extract_rap().map_or(false, |rap| rap.is_fn())
}

/// The rows a line's stacked arrows (`evs`, one line map entry) reach
/// below the line's own row. Each arrow dips a row per arrow drawn before
/// it (`timeline_panel::arrow_order_of`), except the end of a borrow whose
/// lender has no column (`let r = &compute();`: an arrow from r to r
/// pointing into empty space), which is always drawn level on the row. So
/// the rows needed run down to the last arrow in drawing order that isn't
/// such an end: n of those ends on one line need none, where they used to
/// get n - 1 empty rows (#1364). `top_level`: the line map is the
/// function's own, which `sort_line_map_events` sorts after placement;
/// an arm's is drawn in the order `mutate_branch_lines` leaves it (the
/// timeline draws the copies of the arms made during placement, so
/// `sort_branch_external_events` doesn't reach them).
fn stacked_arrow_rows(evs: &[ExternalEvent], top_level: bool) -> usize {
    let mut drawn = evs.to_vec();
    if top_level {
        drawn.sort_by(by_rap_hashes);
        drawn.sort_by_key(|e| !moves_into_fn(e));
    }
    drawn.iter()
        .rposition(|e| !matches!(e,
            ExternalEvent::StaticDie { from, to, .. } | ExternalEvent::MutableDie { from, to, .. }
                if from.is_same_underlying(to)))
        .unwrap_or(0)
}

/// Splits the arrows that share one source line (and so get stacked
/// rows: the first straight on the line's row, the others as
/// trapezoids dipping into extra rows below) into the ones that stay
/// stacked from the line's first row and the ends of borrows that
/// *start* on this same line. Those ends move to a row of their own
/// below the stack, together with the line's events that come after
/// the borrow (#371). Without that, a match arm `Some(s) =>
/// println!("{}", s),` on `&opt` drew `s`'s borrow ending on the
/// first row, above the row where `println!` reads through `s`.
fn split_same_line_borrow_ends(group: &[ExternalEvent]) -> (Vec<ExternalEvent>, Vec<ExternalEvent>) {
    let rap_hash = |r: &ResourceTy| r.extract_rap().map(|r| *r.hash());
    let ends = |e: &ExternalEvent| -> Option<(u64, u64)> {
        let (from, to) = ResourceAccessPoint_extract(e);
        Some((rap_hash(from)?, rap_hash(to)?))
    };
    let borrows: Vec<(u64, u64)> = group.iter()
        .filter(|e| matches!(e, ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. }))
        .filter_map(|e| ends(e))
        .collect();
    group.iter().cloned().partition(|e| {
        let is_end = matches!(e, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. });
        let ends_a_borrow_here = is_end && ends(e).map_or(false, |(f, t)| {
            f != t && borrows.iter().any(|&(bf, bt)| (bf, bt) == (f, t) || (bf, bt) == (t, f))
        });
        !ends_a_borrow_here
    })
}

/// Whether `e`, one of a line's stacked arrows (`group`), ends the
/// borrow of a reference that the line also uses otherwise: in
/// another of its arrows, or in an event of the line not yet appended
/// (`rest`). `*x += 1;` as x's last use copies out of `*x`, writes
/// through it, and returns x's borrow, in that order. Appended first,
/// the end came before the write in x's history, and the write then
/// showed x holding a live `&mut` again down to its scope end (#587).
///
/// A borrow into the same holder recorded after the end (`s2 = Shelf {
/// items: vec![w.as_str()] };` ends s2.items's borrow of v, then gives
/// it w's) is its new borrow, not a use of the one that ends: the end
/// stays first (#1097).
fn ends_after_line_uses(e: &ExternalEvent, group: &[ExternalEvent], rest: &[ExternalEvent]) -> bool {
    let Some(r) = ended_ref(e) else { return false };
    group.iter().chain(rest)
        .filter(|o| o.get_id() != e.get_id() && !new_borrow_into(o, r, e.get_id()))
        .any(|o| uses_rap(o, r))
}

/// Whether `o` is a borrow into the column `r` recorded after the event
/// with id `after`.
fn new_borrow_into(o: &ExternalEvent, r: u64, after: usize) -> bool {
    matches!(o, ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. })
        && o.get_id() > after
        && ResourceAccessPoint_extract(o).1.extract_rap().is_some_and(|t| *t.hash() == r)
}

/// Whether `drop` is the drop of `holder`'s old value at a reassignment
/// (`o = vec![&w];`), recorded before `borrow`, the borrow the new value
/// gives `holder`: the drop comes first on the dot, as it does in the
/// source of the events (#1097).
fn drop_before_new_borrow(drop: &ExternalEvent, borrow: &ExternalEvent) -> bool {
    let ExternalEvent::OwnerDropAtReassign { ro, id, .. } = drop else { return false };
    new_borrow_into(borrow, *ro.hash(), *id)
}

/// Whether `drop` is the drop of the old value of `lender` (or of one of
/// its fields) at a write through the reference whose borrow of `lender`
/// `die` ends on the same line (`*r = t;` with `r = &mut p`): the drop
/// goes on the end's row, so `p` gets one dot for both. Left in the
/// line's sequence, it was appended on the row the line's later arrow
/// (the move of `t`) added, as a second dot (#1192).
fn drop_with_lender_return(drop: &ExternalEvent, die: &ExternalEvent) -> bool {
    let ExternalEvent::OwnerDropAtReassign { ro, .. } = drop else { return false };
    if !matches!(die, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. }) {
        return false;
    }
    let (from, to) = ResourceAccessPoint_extract(die);
    let (Some(r), Some(lender)) = (from.extract_rap(), to.extract_rap()) else { return false };
    r.is_ref() && (ro.hash() == lender.hash() || ro.name().starts_with(&format!("{}.", lender.name())))
}

/// Whether `die` is the end (`RefDie`) of the borrow of the reference
/// that the arrow `arrow` copies or moves a new value into.
/// `first_lines`: the first line each column has an event on. A
/// reference given its first value by the arrow (`j.p` in `let j =
/// Excerpt { p: i.p };`) has no old value: an end of its borrow on that
/// line is of the new one, after the arrow (#1310).
fn ref_die_before_arrow(die: &ExternalEvent, arrow: &ExternalEvent, line: usize, first_lines: &HashMap<u64, usize>) -> bool {
    let ExternalEvent::RefDie { from, .. } = die else { return false };
    if !matches!(arrow, ExternalEvent::Copy { .. } | ExternalEvent::Move { .. }) {
        return false;
    }
    let (Some(r), Some(dest)) = (from.extract_rap(), ResourceAccessPoint_extract(arrow).1.extract_rap()) else { return false };
    r.hash() == dest.hash() && first_lines.get(dest.hash()) != Some(&line)
}

/// The first line each column (by RAP hash) has an event on, in
/// `events` and the branches in them.
fn first_event_lines(events: &[(usize, ExternalEvent)], out: &mut HashMap<u64, usize>) {
    for (line, e) in events {
        if let ExternalEvent::Branch { branches, .. } = e {
            for b in branches {
                first_event_lines(&b.e_data, out);
            }
            continue;
        }
        let (from, to) = ResourceAccessPoint_extract(e);
        for r in [from, to].into_iter().filter_map(|r| r.extract_rap()) {
            let first = out.entry(*r.hash()).or_insert(*line);
            *first = (*first).min(*line);
        }
    }
}

/// The hash of the reference whose borrow `e` ends, if it is a
/// StaticDie / MutableDie.
fn ended_ref(e: &ExternalEvent) -> Option<u64> {
    if !matches!(e, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. }) {
        return None;
    }
    ResourceAccessPoint_extract(e).0.extract_rap().map(|r| *r.hash())
}

/// Whether `e` ends a borrow of the reference `r` (its hash): a
/// reborrow through r giving its borrow back.
fn returns_to(e: &ExternalEvent, r: u64) -> bool {
    ended_ref(e).is_some() && ResourceAccessPoint_extract(e).1.extract_rap().is_some_and(|t| *t.hash() == r)
}

fn uses_rap(e: &ExternalEvent, r: u64) -> bool {
    let (from, to) = ResourceAccessPoint_extract(e);
    from.extract_rap().is_some_and(|f| *f.hash() == r) || to.extract_rap().is_some_and(|t| *t.hash() == r)
}

/// Whether `e`, an event of a line that draws no arrow (the write in
/// `*x += 1;`), uses a reference whose borrow one of the line's
/// stacked arrows (`group`) ends. Its dot belongs on the line's own
/// row with the end's, not on a row inserted below for the stack
/// (#587).
fn uses_ref_ended_on_line(e: &ExternalEvent, group: &[ExternalEvent]) -> bool {
    group.iter().filter_map(ended_ref).any(|r| uses_rap(e, r))
        || writes_part_of_lender_ended_on_line(e, group)
}

/// Whether `e` is the write `*x += 1;` drawn on the column of x's lender
/// (`v`, x from `v.get_mut(0)`, #861) on the line where one of the line's
/// stacked arrows (`group`) ends a borrow of v: like the write on x's own
/// column, it happens before x's borrow ends, on the line's own row.
fn writes_part_of_lender_ended_on_line(e: &ExternalEvent, group: &[ExternalEvent]) -> bool {
    let ExternalEvent::Bind { to: ResourceTy::Index(owner, ..), .. } = e else { return false };
    group.iter()
        .filter(|g| ended_ref(g).is_some())
        .any(|g| ResourceAccessPoint_extract(g).1.extract_rap().is_some_and(|l| l.hash() == owner.hash()))
}

/// Whether `d`, a move (or copy) of a struct's field that names no
/// destination column (`propagate_struct_transfer`), goes with the move
/// of the whole struct that the line's arrow `e` draws: `*r = t;` moves
/// `t.name` with `t`. It belongs on the row of that arrow. Appended in
/// the line's order, after its stacked arrows, it landed on the row
/// below (where `r`'s borrow ends), so `t.name` was still drawn owned
/// after `t` was moved as a whole (#1283).
fn part_moves_with_whole(d: &ExternalEvent, e: &ExternalEvent) -> bool {
    let transfer = |x: &ExternalEvent| matches!(x, ExternalEvent::Move { is_partial: false, .. } | ExternalEvent::Copy { is_partial: false, .. });
    if !transfer(d) || !transfer(e) || matches!(d, ExternalEvent::Move { .. }) != matches!(e, ExternalEvent::Move { .. }) {
        return false;
    }
    let (ResourceTy::Value(part), to) = ResourceAccessPoint_extract(d) else { return false };
    if !matches!(to, ResourceTy::Anonymous | ResourceTy::Caller) {
        return false;
    }
    let ResourceTy::Value(whole @ ResourceAccessPoint::Struct(_)) = ResourceAccessPoint_extract(e).0 else { return false };
    part.is_member()
        && part.name().starts_with(&format!("{}.", whole.name()))
        && (part.get_owner() == *whole.hash() || (whole.is_member() && part.get_owner() == whole.get_owner()))
}

/// Whether `e`, an event of a line that draws no arrow, binds a variable
/// that one of the line's stacked arrows (`group`) copies or moves out
/// of: `let k = { let t = m * 2; t };` binds `t` and copies it into `k`
/// on one line. The binding comes first, so its dot belongs on the line's
/// own row, before the arrow's dot, not on a row inserted below for the
/// stack (#731).
fn binds_source_of_line(e: &ExternalEvent, group: &[ExternalEvent]) -> bool {
    if !matches!(e, ExternalEvent::Bind { .. }) {
        return false;
    }
    let ResourceTy::Value(t) = ResourceAccessPoint_extract(e).1 else { return false };
    // A field of a struct or tuple that one of the line's arrows moves
    // or copies into gets its part with the whole, on the row the whole
    // is bound on: `(r, z)` on `(g, (h, k))` moves `h` and `k` into `z`,
    // and `z.0`'s dot sat on the row below them, after all of its
    // events, so its column was a single dot (#781).
    // (Not when the line also moves the whole or the field away, as in
    // `(p, q) = (q, p)`: the new value arrives after the old one left.)
    let owner = t.get_owner() as u64;
    let arrow_to_owner = |o: &ExternalEvent| matches!(o, ExternalEvent::Copy { .. } | ExternalEvent::Move { .. })
        && matches!(ResourceAccessPoint_extract(o).1, ResourceTy::Value(p) if *p.hash() == owner);
    let arrow_from_whole = |o: &ExternalEvent| matches!(o, ExternalEvent::Copy { .. } | ExternalEvent::Move { .. })
        && matches!(ResourceAccessPoint_extract(o).0, ResourceTy::Value(p) if *p.hash() == owner || p.hash() == t.hash());
    if t.is_member() && group.iter().any(arrow_to_owner) && !group.iter().any(arrow_from_whole) {
        return true;
    }
    let t = *t.hash();
    // Only an arrow that comes after the binding: `n = n + 1;` copies `n`
    // first and is assigned after.
    group.iter().any(|o| o.get_id() > e.get_id()
        && matches!(o, ExternalEvent::Copy { .. } | ExternalEvent::Move { .. })
        && matches!(ResourceAccessPoint_extract(o).0, ResourceTy::Value(f) if *f.hash() == t))
}

/// The column a use (`println!("{}", a)` lending `a`) or a scope end
/// is on.
fn use_or_end_column(e: &ExternalEvent) -> Option<u64> {
    match e {
        ExternalEvent::GoOutOfScope { ro, .. } => Some(*ro.hash()),
        // A reference's borrow end with no arrow (#1277).
        ExternalEvent::RefDie { from: ResourceTy::Value(r), .. } => Some(*r.hash()),
        ExternalEvent::PassByStaticReference { from: ResourceTy::Value(r), .. }
        | ExternalEvent::PassByMutableReference { from: ResourceTy::Value(r), .. } => Some(*r.hash()),
        _ => None,
    }
}

/// Whether `e`, an event later on a line whose arrows (`group`) are
/// stacked, uses or ends a variable that one of those arrows moves or
/// copies into, and that nothing else on the line does anything else to
/// (`rest`: the line's other events): `E::V(a, b) => println!("{} {}",
/// a, b),` moves e into a and into b, then lends both to println! and
/// drops them (#1223). It goes on the row of the arrival's dot, the
/// line's own. On the row inserted below for the stack, a's column had a
/// second dot, on a row with no code, which the dipped `e -> b` ran
/// through.
fn follows_arrival_on_line(e: &ExternalEvent, group: &[ExternalEvent], rest: &[ExternalEvent]) -> bool {
    let Some(col) = use_or_end_column(e) else { return false };
    let arrival = |g: &ExternalEvent| matches!(g, ExternalEvent::Move { .. } | ExternalEvent::Copy { .. })
        && matches!(ResourceAccessPoint_extract(g).1, ResourceTy::Value(t) if *t.hash() == col);
    // (Nor when an arrow moves a part of it: `Some(pr) => pr,` moves
    // o into pr, then pr.0 into m below, and pr ends after that.)
    let part_of_col = |t: &ResourceTy| t.extract_rap().is_some_and(|r| r.is_member() && r.get_owner() as u64 == col);
    group.iter().filter(|g| arrival(g)).count() == 1
        && !group.iter().any(|g| !arrival(g) && uses_rap(g, col))
        && !group.iter().any(|g| { let (f, t) = ResourceAccessPoint_extract(g); part_of_col(f) || part_of_col(t) })
        && rest.iter().all(|o| use_or_end_column(o).is_some() || !uses_rap(o, col))
}

/// The (source, binding) column pairs of the arrows `evs` of one line of
/// an arm, when they are all bindings of the arm's pattern (`to` is
/// declared by the arm) from distinct variables that aren't bound there
/// themselves: `if let Some(a) = &x && let Some(b) = y {` (#510). Such
/// arrows are drawn side by side on their row, each binding's column
/// right after its source's (`binding_columns`), rather than one below
/// the other with a row added for each extra one: by default the
/// bindings' columns come after both sources, so `x -> a` ran over `y`'s
/// dot, and `y -> b` was a trapezoid through a blank row that pushed the
/// arm's body a row away from its code. A struct (or a field) has
/// columns of its fields next to it, so a line with one isn't changed.
fn side_by_side_bindings(evs: &[ExternalEvent], b_data: &ExtBranchData) -> Option<Vec<(u64, u64)>> {
    if evs.len() < 2 {
        return None;
    }
    let mut pairs = Vec::new();
    for e in evs {
        if !matches!(e, ExternalEvent::Move { .. } | ExternalEvent::Copy { .. }
            | ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. }) {
            return None;
        }
        let (ResourceTy::Value(from), ResourceTy::Value(to)) = ResourceAccessPoint_extract(e) else { return None };
        let plain = |r: &ResourceAccessPoint| !r.is_fn() && !r.is_struct_group() && !r.is_member();
        if !plain(from) || !plain(to) || !b_data.decl_vars.contains(to) || b_data.decl_vars.contains(from) {
            return None;
        }
        pairs.push((*from.hash(), *to.hash()));
    }
    let distinct = |hs: Vec<u64>| hs.iter().collect::<HashSet<_>>().len() == hs.len();
    (distinct(pairs.iter().map(|p| p.0).collect()) && distinct(pairs.iter().map(|p| p.1).collect()))
        .then_some(pairs)
}

/// True when the arrows `evs` of one line are two or more of the
/// let-chain bindings the plugin asked to draw side by side
/// (`VisualizationData::binding_columns`): `while let Some(b) = y &&
/// let Some(c) = z {` (#584), or `if let … && let …` without a branch
/// to put them in (#583). Like `side_by_side_bindings`'s, they go on the
/// line's own row, each binding's column right after its source's.
fn chain_bindings_side_by_side(evs: &[ExternalEvent], pairs: &[(u64, u64)]) -> bool {
    evs.len() >= 2 && evs.iter().all(|e| {
        matches!(e, ExternalEvent::Move { .. } | ExternalEvent::Copy { .. }
            | ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. })
            && matches!(ResourceAccessPoint_extract(e),
                (ResourceTy::Value(from), ResourceTy::Value(to)) if pairs.contains(&(*from.hash(), *to.hash())))
    })
}

/// Whether two arrows of one line can be drawn on one track: arrows of
/// the same kind going the same way out of one variable (a fan-out, `z`
/// lending to `p.a` and `p.b`) or into one (a fan-in, `p.a` and `p.b`
/// giving `z` its resource back). Their shared stretch then reads as one
/// arrow that forks. Columns are laid out in declaration (hash) order,
/// with a struct's fields right after it, so the hashes give the
/// direction.
fn arrows_share_track(a: &ExternalEvent, b: &ExternalEvent) -> bool {
    let ends = |e: &ExternalEvent| -> Option<(u64, u64)> {
        let (from, to) = ResourceAccessPoint_extract(e);
        Some((*from.extract_rap()?.hash(), *to.extract_rap()?.hash()))
    };
    let (Some((af, at)), Some((bf, bt))) = (ends(a), ends(b)) else { return false };
    std::mem::discriminant(a) == std::mem::discriminant(b)
        && (af < at) == (bf < bt)
        && (af == bf || at == bt)
}

/// The tracks of the arrows on one line of a struct write with reference
/// fields (#624). `p = P { a: &z, b: &z };` over `P { a: &x, b: &y }` has
/// four arrows (x's and y's borrows returned from `p.a` and `p.b`, `z`
/// lent to both), and each arrow after the first used to dip a whole row
/// lower, with a row inserted for it: the line grew four rows tall, and
/// the code below it moved away from its rows. Here arrows that fan out
/// of or into one variable share a track (`arrows_share_track`), and
/// tracks are half a row apart, so that line takes two rows.
///
/// Applies when every arrow on the line is between two variables and has
/// a struct field at one end. Returns each arrow's id with the id of its
/// track's first arrow, and the number of tracks.
fn struct_write_tracks(evs: &[ExternalEvent]) -> Option<(Vec<(usize, usize)>, usize)> {
    if evs.len() < 2 {
        return None;
    }
    let mut tracks: Vec<Vec<&ExternalEvent>> = Vec::new();
    let mut out = Vec::new();
    for e in evs {
        let (ResourceTy::Value(from), ResourceTy::Value(to)) = ResourceAccessPoint_extract(e) else { return None };
        if from.is_fn() || to.is_fn() || !(from.is_member() || to.is_member()) || from.hash() == to.hash() {
            return None;
        }
        match tracks.iter_mut().find(|t| t.iter().all(|o| arrows_share_track(o, e))) {
            Some(t) => {
                out.push((e.get_id(), t[0].get_id()));
                t.push(e);
            }
            None => {
                out.push((e.get_id(), e.get_id()));
                tracks.push(vec![e]);
            }
        }
    }
    Some((out, tracks.len()))
}

/// Rows a line's arrows on `n` tracks half a row apart reach below the
/// line's own row: the last track is `(n - 1) / 2` rows down, rounded
/// down, since one half a row down still sits above the next row.
fn half_row_tracks_extra_rows(n: usize) -> usize {
    n.saturating_sub(1) / 2
}

/// The (from, to) columns of the arrows an arm draws on `line`, its
/// nested conditionals' included (`else if`): moves, copies, borrows,
/// their ends and pattern bindings between two variables' columns. An
/// arrow to or from a function (`take(f)`) is a short one beside its dot.
fn arm_arrows_on_line(e_data: &[(usize, ExternalEvent)], line: usize, out: &mut Vec<(u64, u64)>) {
    for (l, e) in e_data {
        if let ExternalEvent::Branch { branches, .. } = e {
            for b in branches {
                arm_arrows_on_line(&b.e_data, line, out);
            }
            continue;
        }
        if *l != line {
            continue;
        }
        let (from, to) = ResourceAccessPoint_extract(e);
        if let (Some(f), Some(t)) = (from.extract_rap(), to.extract_rap()) {
            if !f.is_fn() && !t.is_fn() && f.hash() != t.hash() {
                out.push((*f.hash(), *t.hash()));
            }
        }
    }
}

/// Whether an arrow arm `j` draws on `line` would run along a stretch of
/// one an earlier arm draws there, both on the one row of a one-line
/// conditional (#1264). Columns are in declaration (hash) order, with
/// the moves `binding_columns` makes (as `timeline_panel` lays them
/// out), and in each column the arms' dots sit left to right in arm
/// order, all within the column's own slot. So an arrow spans the
/// positions (column, arm) from its left end to its right end, and two
/// arrows of different arms share a stretch when those spans overlap:
/// `f -> a` in the `if` arm and `h -> a` in the `else` arm, with f, h,
/// a left to right, share the stretch from h's `else` dot to a's `if`
/// dot. Arrows into a from opposite sides (`f -> a`, `a`, `h`) don't.
fn arms_arrows_overlap(branches: &[ExtBranchData], j: usize, line: usize, binding_columns: &[(u64, u64)]) -> bool {
    let arrows: Vec<Vec<(u64, u64)>> = branches[..=j].iter().map(|b| {
        let mut v = Vec::new();
        arm_arrows_on_line(&b.e_data, line, &mut v);
        v
    }).collect();
    if arrows[j].is_empty() {
        return false;
    }
    let mut order: Vec<u64> = arrows.iter().flatten().flat_map(|&(f, t)| [f, t])
        .chain(binding_columns.iter().flat_map(|&(f, t)| [f, t]))
        .collect();
    order.sort_unstable();
    order.dedup();
    for (from, to) in binding_columns {
        let (Some(_), Some(k)) = (order.iter().position(|h| h == from), order.iter().position(|h| h == to)) else { continue };
        let to = order.remove(k);
        let i = order.iter().position(|h| h == from).unwrap_or(k);
        order.insert(i + 1, to);
    }
    let rank = |h: u64| order.iter().position(|&o| o == h).unwrap_or(0);
    let span = |(f, t): (u64, u64), arm: usize| {
        let (a, b) = ((rank(f), arm), (rank(t), arm));
        (a.min(b), a.max(b))
    };
    arrows[j].iter().any(|&aj| {
        let (lo_j, hi_j) = span(aj, j);
        arrows[..j].iter().enumerate().any(|(i, ai)| ai.iter().any(|&a| {
            let (lo_i, hi_i) = span(a, i);
            lo_i.max(lo_j) < hi_i.min(hi_j)
        }))
    })
}

/// Whether an arm of `branches`, or of a conditional nested in one,
/// declares `r`.
fn arm_declares(branches: &[ExtBranchData], r: &ResourceAccessPoint) -> bool {
    branches.iter().any(|arm| arm.decl_vars.contains(r) || arm.e_data.iter().any(|(_, e)|
        matches!(e, ExternalEvent::Branch { branches, .. } if arm_declares(branches, r))))
}

/// Puts the borrow end `end` of `r` into the arm declaring `r` (see
/// `arm_declares`), after that arm's events on its line and before r's
/// scope end there, and into the arm's line map with the line's other
/// arrows, so it is stacked with them as it would be at the top level.
fn insert_in_declaring_arm(branches: &mut [ExtBranchData], r: &ResourceAccessPoint, end: (usize, ExternalEvent)) {
    let Some(arm) = branches.iter_mut().find(|arm| arm_declares(std::slice::from_ref(&**arm), r)) else { return };
    if !arm.decl_vars.contains(r) {
        for (_, e) in arm.e_data.iter_mut() {
            if let ExternalEvent::Branch { branches, .. } = e {
                if arm_declares(branches, r) {
                    return insert_in_declaring_arm(branches, r, end);
                }
            }
        }
        return;
    }
    let line = end.0;
    let mut at = arm.e_data.iter().rposition(|(l, _)| *l <= line).map_or(0, |i| i + 1);
    if let Some(k) = arm.e_data[..at].iter().rposition(|(l, e)| *l == line && e.is_gos_ev() == Some(r)) {
        at = k;
    }
    if end.1.is_var_arrow_ev() {
        arm.line_map.entry(line).or_default().push(end.1.clone());
    }
    arm.e_data.insert(at, end);
}

/// A borrow end recorded after the conditional `events[i]`, on its
/// line, of a reference declared in one of its arms (`let x = ra;` in
/// `if c { .. } else { let x = ra; .. }` on one line). The plugin nests
/// such an end in the arm only when the lender has a column in the
/// conditional (`nest_borrow_ends_in_arms`); otherwise it came after
/// the conditional, and once the arms got rows of their own (#1187,
/// #1264) it was drawn on the join row, below the reference's scope end
/// in the arm, where the reference no longer exists (#1277). Each such
/// end moves into its arm, before the arm's rows are laid out (it is
/// one of the arm's stacked arrows there), out of `line_map`'s group
/// for the line. The lender's column still gets it (`data.rs`, the
/// Branch's ends of lenders it doesn't draw), and its dot there names
/// the references whose ends stay after the conditional as still
/// borrowing it (`ra`, on the join row). Returns how many events left
/// `events`.
fn nest_arm_ref_ends(events: &mut Vec<(usize, ExternalEvent)>, i: usize, skip: &HashSet<usize>, line_map: &mut BTreeMap<usize, Vec<ExternalEvent>>) -> usize {
    let line = events[i].0;
    let ExternalEvent::Branch { branches, .. } = &events[i].1 else { return 0 };
    let ended = |e: &ExternalEvent| match e {
        ExternalEvent::StaticDie { from: ResourceTy::Value(r), .. }
        | ExternalEvent::MutableDie { from: ResourceTy::Value(r), .. }
        | ExternalEvent::RefDie { from: ResourceTy::Value(r), .. } => Some(r.clone()),
        _ => None,
    };
    let on_line: Vec<usize> = (i + 1..events.len())
        .filter(|&j| events[j].0 == line && !skip.contains(&events[j].1.get_id()))
        .collect();
    let moved: Vec<usize> = on_line.iter().copied()
        .filter(|&j| ended(&events[j].1).is_some_and(|r| arm_declares(branches, &r)))
        .collect();
    if moved.is_empty() {
        return 0;
    }
    let lender = |e: &ExternalEvent| match e {
        ExternalEvent::StaticDie { to, .. } | ExternalEvent::MutableDie { to, .. } => to.extract_rap().map(|r| *r.hash()),
        _ => None,
    };
    let mut taken: Vec<(ResourceAccessPoint, (usize, ExternalEvent))> = Vec::new();
    for &j in &moved {
        let mut end = events[j].clone();
        let holders: Vec<String> = on_line.iter()
            .filter(|k| !moved.contains(k))
            .map(|&k| &events[k].1)
            .filter(|o| lender(o).is_some() && lender(o) == lender(&end.1))
            .filter_map(|o| ended(o).map(|r| r.name().to_owned()))
            .collect();
        if let ExternalEvent::StaticDie { still_borrowed_by, .. } | ExternalEvent::MutableDie { still_borrowed_by, .. } = &mut end.1 {
            for name in holders {
                if !still_borrowed_by.contains(&name) {
                    still_borrowed_by.push(name);
                }
            }
        }
        taken.push((ended(&end.1).unwrap(), end));
    }
    for &j in moved.iter().rev() {
        let id = events[j].1.get_id();
        events.remove(j);
        if let Some(group) = line_map.get_mut(&line) {
            group.retain(|g| g.get_id() != id);
            if group.is_empty() {
                line_map.remove(&line);
            }
        }
    }
    let ExternalEvent::Branch { branches, .. } = &mut events[i].1 else { unreachable!() };
    for (r, end) in taken {
        insert_in_declaring_arm(branches, &r, end);
    }
    moved.len()
}

/// A reference's own borrow end listed after its scope end on one row
/// (`x goes out of scope / x's immutable borrow ends`) reads as the
/// borrow ending once x is gone (#1277): it goes first.
fn ends_before_scope_ends(history: &mut Vec<(usize, Event)>) {
    for (_, e) in history.iter_mut() {
        if let Event::Branch { branch_history, .. } = e {
            for b in branch_history.iter_mut() {
                ends_before_scope_ends(&mut b.e_data);
            }
        }
    }
    let mut i = 0;
    while i < history.len() {
        if matches!(history[i].1, Event::RefGoOutOfScope { .. } | Event::OwnerGoOutOfScope { .. }) {
            let row = history[i].0;
            let end = history[i + 1..].iter().position(|(l, e)| *l == row
                && matches!(e, Event::StaticDie { .. } | Event::MutableDie { .. } | Event::RefDie { .. }));
            if let Some(k) = end {
                let ev = history.remove(i + 1 + k);
                history.insert(i, ev);
            }
        }
        i += 1;
    }
}

/// Rows to add around the arms of a conditional written on one line
/// whose arms stack arrows (`let (a, b) = if c { (f, g) } else { (h,
/// k) };`, #1187). Each arm's stacked arrows run on rows inserted below
/// its own, and the next arm, on the same source line, started on the
/// first of those rows, with the join on the last arm's: an arrow of one
/// arm ran along the other arm's row or the join's, through their dots
/// and along their arrows. Instead each arm with events after the first
/// gets a row of its own below the previous arms' rows (an arm with no
/// events, like an implicit `else`, needs none), and the join one below
/// the last arm's (`before_merge`). Arms with one arrow each get rows of
/// their own too when those arrows would run along one stretch of the
/// row (`let a = if c { f } else { h };`: f -> a passes h's column on its
/// way to a, alongside h -> a, so only one of the two could be hovered
/// there, #1264). So do the arms of a one-line conditional with another
/// one in an arm (`if c { take(f) } else if d { take(g) } else { take(h)
/// }`), and the arms of that inner one (`forced`): drawn as a one-row
/// diamond inside the outer one's, the inner fork sat level with or above
/// the outer fork, and the outer leg into the arm ended in the middle of
/// the inner diamond (#1306). A one-line conditional whose arrows don't
/// stack and don't overlap, with no conditional of its own on the line,
/// keeps its arms side by side on one row (#117).
struct OneLineArmGaps {
    /// Arm j starts on the source line arm j - 1 ends on, has events,
    /// comes after an arm with events, and one of them stacks arrows or
    /// an arrow of arm j runs alongside one of an earlier arm's.
    gap_before: Vec<bool>,
    /// The last arm with events ends on the join's line.
    ends_on_merge: bool,
    /// A conditional over several lines whose arm has events on the
    /// join's line (`} else { take(s); }`): the join goes on a row of its
    /// own below that line. On the line's row it sat beside the arm's
    /// dot, and the arm's arrow to a call's logo ran through it, the logo
    /// on top of it (#1374).
    join_below: bool,
    /// One of those events is a one-line conditional nested in the arm
    /// (`} else if d { take(s); }`), which joins on the row below the
    /// line (`branch_is_collapsed`): the join goes a row below that one,
    /// onto the blank row below the line (`insert_blank_rows`). Level with
    /// it, or above it on the line's row, the two joins looked like one
    /// conditional's.
    join_past_inner: bool,
}

impl OneLineArmGaps {
    fn of(
        branches: &[ExtBranchData],
        branch_type: &crate::svg_generator::data::BranchType,
        merge_point: usize,
        line_num: usize,
        live_vars: &HashSet<ResourceAccessPoint>,
        l_map: &HashMap<usize, usize>,
        binding_columns: &[(u64, u64)],
        arrow_tracks: &HashMap<usize, usize>,
        forced: bool,
    ) -> Self {
        let spans: Vec<(usize, usize)> = (0..branches.len()).map(|j| branch_type.get_start_end(j)).collect();
        // Arm j and an earlier arm both have events on the line arm j
        // starts on (so they would be drawn on one row), and one of them
        // stacks arrows there. A `} else {` line, where the arm before
        // has nothing, needs no gap.
        let on_line = |b: &ExtBranchData, line: usize| b.e_data.iter().any(|(l, _)| *l == line);
        let stacks_on = |b: &ExtBranchData, line: usize| b.line_map.get(&line).is_some_and(|v| v.len() > 1);
        let shares_row = |j: usize| {
            let line = spans[j].0;
            j > 0
                && spans[j].0 == spans[j - 1].1
                && on_line(&branches[j], line)
                && branches[..j].iter().any(|b| on_line(b, line))
        };
        // Whether any arm stacks arrows: the rows it would insert, on
        // copies of everything `mutate_branch_lines` changes.
        let stacks = || branches.iter().zip(&spans).any(|(b, &(start, _))| {
            mutate_branch_lines(&mut b.clone(), &mut l_map.clone(), 0, (line_num, start),
                &mut binding_columns.to_vec(), &mut arrow_tracks.clone(), false) > 0
        });
        let stacked: Vec<bool> = (0..spans.len())
            .map(|j| shares_row(j) && branches[..=j].iter().any(|b| stacks_on(b, spans[j].0)))
            .collect();
        let stacked = if stacked.iter().any(|&s| s) && stacks() { stacked } else { vec![false; spans.len()] };
        // The conditional's arms share its line, and an arm runs a
        // conditional of its own written on its line, whose arms share
        // that line too (`else if d { take(g) } else { take(h) }`), or it
        // is that inner one (`forced`).
        let nests = branch_is_collapsed(line_num, merge_point) && (forced || branches.iter().zip(&spans).any(|(b, &(start, _))| b.e_data.iter().any(|(l, e)|
            *l == start && matches!(e, ExternalEvent::Branch { split_point, merge_point, .. }
                if *split_point == start && branch_is_collapsed(*split_point, *merge_point)))));
        // Then every arm with events on the line the arm before it ends
        // on gets a row, even after arms with none (`if c { } else if d {
        // take(g) } else { take(h) }`): the outer conditional is no longer
        // one row, so the inner fork sits below the outer one.
        let own_row = |j: usize| j > 0 && spans[j].0 == spans[j - 1].1 && on_line(&branches[j], spans[j].0);
        let gap_before: Vec<bool> = (0..spans.len())
            .map(|j| stacked[j]
                || (nests && own_row(j))
                || (shares_row(j) && arms_arrows_overlap(branches, j, spans[j].0, binding_columns)))
            .collect();
        // (Only events drawn on a column that joins there: not a scope
        // end of a variable the arm declares, or the arm's value copied
        // out to the caller, on a fn's last line.)
        let joins = |r: &ResourceTy| r.extract_rap().is_some_and(|r| live_vars.contains(r));
        // Nor a move into the temporary tuple a scrutinee builds, dropped
        // at the closing `}` (`if let (Some(a), _) = (p, q) {`): its drop
        // is drawn under each arm, which ends there.
        #[expect(clippy::disallowed_methods, reason = "any() makes order irrelevant")]
        let on_joined_column = |e: &ExternalEvent| match e {
            ExternalEvent::Branch { live_vars: inner, .. } => inner.iter().any(|r| live_vars.contains(r)),
            ExternalEvent::Move { to: ResourceTy::Temporary { tuple: Some(_), .. }, .. } => false,
            // A borrow's end: its arrow lands on the lender's column. (The
            // reference's own column ends at its dot and joins a row below
            // anyway.)
            ExternalEvent::StaticDie { to, .. } | ExternalEvent::MutableDie { to, .. } => joins(to),
            e => { let (from, to) = ResourceAccessPoint_extract(e); joins(from) || joins(to) }
        };
        let join_below = !branch_is_collapsed(line_num, merge_point)
            && branches.iter().any(|b| b.e_data.iter().any(|(l, e)| *l == merge_point && on_joined_column(e)));
        let join_past_inner = join_below && branches.iter().any(|b| b.e_data.iter().any(|(l, e)| *l == merge_point
            && matches!(e, ExternalEvent::Branch { split_point, merge_point: m, .. } if *m == merge_point && branch_is_collapsed(*split_point, *m))));
        if !gap_before.iter().any(|&s| s) {
            return OneLineArmGaps { gap_before, ends_on_merge: false, join_below, join_past_inner };
        }
        let ends_on_merge = branches.iter().zip(&spans).rev()
            .find(|(b, _)| !b.e_data.is_empty())
            .is_some_and(|(b, &(_, end))| end == merge_point && on_line(b, end));
        OneLineArmGaps { gap_before, ends_on_merge, join_below, join_past_inner }
    }

    /// Whether any arm got a row of its own: a one-line conditional
    /// nested in an arm is then given rows too (`forced`).
    fn spread(&self) -> bool {
        self.gap_before.iter().any(|&g| g)
    }

    fn before_arm(&self, j: usize) -> bool {
        self.gap_before.get(j).copied().unwrap_or(false)
    }

    /// The join goes on a row of its own below the arms when they were
    /// given rows of their own (the conditional is no longer drawn as a
    /// one-row diamond, whose join sits half a row below it).
    fn before_merge(&self) -> bool {
        (self.ends_on_merge && self.gap_before.iter().any(|&g| g)) || self.join_below
    }
}

/// Shifts the lines of the arm `b_data` by the rows inserted above it
/// and below its stacked arrows. `pattern_lines` are the (unshifted)
/// lines its conditional splits on and the arm starts on, where its
/// pattern binds (`} else if let Some(a) = &x && … {`);
/// `binding_columns` collects the pattern bindings drawn side by side
/// there (`side_by_side_bindings`).
/// Ids of the arrows listed in the line map of an arm of a conditional
/// among `events` (or nested in one).
fn arm_listed_arrows(events: &[(usize, ExternalEvent)]) -> HashSet<usize> {
    let mut out = HashSet::new();
    for (_, e) in events {
        if let ExternalEvent::Branch { branches, .. } = e {
            for b in branches {
                for evs in b.line_map.values() {
                    out.extend(evs.iter().map(|x| x.get_id()));
                }
                out.extend(arm_listed_arrows(&b.e_data));
            }
        }
    }
    out
}

pub fn mutate_branch_lines(
    b_data: & mut ExtBranchData,
    l_map: & mut HashMap<usize, usize>,
    mut extra_lines: usize,
    pattern_lines: (usize, usize),
    binding_columns: &mut Vec<(u64, u64)>,
    arrow_tracks: &mut HashMap<usize, usize>,
    in_spread_arm: bool,
) -> usize {
    let old_extra_lines = extra_lines;
    let mut old_line_map = b_data.line_map.clone();
    let mut new_line_map: BTreeMap<usize, Vec<ExternalEvent>> = BTreeMap::new();
    let mut skippable_ev: HashSet<usize> = HashSet::new();
    // (source line, shift) for the rest of a line whose arrows were
    // split by `split_same_line_borrow_ends`: its later events go on
    // the row of the moved borrow ends, not the line's last row.
    let mut same_line_shift: Option<(usize, usize)> = None;
    // Rows inserted below each (pre-shift) source line for stacked
    // arrows; see the ClosureCall case below.
    let mut extras_at_line: HashMap<usize, usize> = HashMap::new();

    // mutate actual events
    let mut i: usize = 0;
    let mut size: usize = b_data.e_data.len();
    while i < size {
        if matches!(b_data.e_data[i].1, ExternalEvent::Branch { .. }) && !skippable_ev.contains(&b_data.e_data[i].1.get_id()) {
            size -= nest_arm_ref_ends(&mut b_data.e_data, i, &skippable_ev, &mut old_line_map);
        }
        let (l, e) = b_data.e_data.get_mut(i).unwrap();
        let line_num = *l;
        let new_line_num = *l + extra_lines;
        if skippable_ev.contains(&e.get_id()) {
            i = i + 1;
            continue;
        }
        match same_line_shift {
            Some((src, shift)) if src == line_num && !matches!(e, ExternalEvent::Branch { .. }) => *l += shift,
            _ => *l += extra_lines, // update starting line of event
        }
        match e {
            ExternalEvent::Branch {branches, split_point, merge_point, branch_type, live_vars, .. } => {
                *split_point = *l; // update split point
                let gaps = OneLineArmGaps::of(branches, branch_type, *merge_point, line_num, live_vars, l_map, binding_columns, arrow_tracks, in_spread_arm);
                for (j, branch) in branches.iter_mut().enumerate(){ // mutate branches
                    let (start, end) = branch_type.get_mut_start_end(j);
                    if gaps.before_arm(j) {
                        *l_map.entry(*start + extra_lines).or_insert(0) += 1;
                        extra_lines += 1;
                    }
                    let pattern_lines = (line_num, *start);
                    *start += extra_lines;
                    let b = mutate_branch_lines(branch, l_map, extra_lines, pattern_lines, binding_columns, arrow_tracks, gaps.spread());
                    extra_lines += b;
                    *end += extra_lines;
                }
                if gaps.before_merge() {
                    *l_map.entry(*merge_point + extra_lines).or_insert(0) += 1;
                    extra_lines += 1;
                }
                *merge_point += extra_lines + usize::from(gaps.join_past_inner);
            }
            _ => {
                // add extra lines if we need to
                if e.is_arrow_ev() {
                    // Only the line map's own members trigger the
                    // stacking (once per line, like the top-level loop
                    // below); an off-map arrow on the same line used
                    // to trigger it again and double the extra rows.
                    let id = e.get_id();
                    let res = old_line_map.get(&line_num)
                        .filter(|ev| ev.iter().any(|x| x.get_id() == id))
                        .cloned();
                    let side_by_side = res.as_ref().and_then(|ev| {
                        let on_pattern_line = line_num == pattern_lines.0 || line_num == pattern_lines.1;
                        on_pattern_line.then(|| side_by_side_bindings(ev, b_data)).flatten()
                            .or_else(|| chain_bindings_side_by_side(ev, binding_columns).then(Vec::new))
                    });
                    let ex = match res {
                    Some(ev) if side_by_side.is_some() => {
                        // All on the line's own row, and left out of the
                        // line map, so each is drawn as the first arrow on
                        // its line: straight along the row.
                        for e in &ev {
                            skippable_ev.insert(e.get_id());
                            if let Some((l, _)) = b_data.e_data[i..].iter_mut().find(|(_, p_e)| p_e.get_id() == e.get_id()) {
                                *l = new_line_num;
                            }
                        }
                        binding_columns.extend(side_by_side.into_iter().flatten());
                        0
                    }
                    Some(ev) => { // if there are multiple arrow events on this line
                        let (front, back) = split_same_line_borrow_ends(&ev);
                        let back_line = new_line_num + front.len();
                        for (e, row) in front.iter().map(|e| (e, new_line_num))
                            .chain(back.iter().map(|e| (e, back_line))) {
                            skippable_ev.insert(e.get_id()); // they all become skippable
                            let mut j = i;
                            while j < size { // need to mutate all their line numbers
                                let (l, p_e) = b_data.e_data.get_mut(j).unwrap();
                                if p_e.get_id() == e.get_id(){
                                    *l = row;
                                    break;
                                }
                                j += 1;
                            }
                        }
                        // As in the top-level loop: a variable bound on this
                        // line that an arrow moves out of is bound on the
                        // line's own row, before the arrow's dot (#731;
                        // `Some(pr) => pr,` binds pr.0 and moves it into
                        // m, #1200), and the uses and scope ends of a
                        // variable an arrow moves into share the arrival's
                        // dot (#1223).
                        let rest: Vec<ExternalEvent> = b_data.e_data[i + 1..].iter()
                            .filter(|(l, p_e)| *l == line_num && !skippable_ev.contains(&p_e.get_id()) && !ev.iter().any(|g| g.get_id() == p_e.get_id()))
                            .map(|(_, p_e)| p_e.clone())
                            .collect();
                        let own_row: HashSet<usize> = rest.iter()
                            .filter(|p_e| binds_source_of_line(p_e, &ev) || follows_arrival_on_line(p_e, &ev, &rest)
                                || front.iter().any(|g| part_moves_with_whole(p_e, g)))
                            .map(|p_e| p_e.get_id())
                            .collect();
                        for (l, p_e) in b_data.e_data[i + 1..].iter_mut() {
                            if own_row.contains(&p_e.get_id()) && skippable_ev.insert(p_e.get_id()) {
                                *l = new_line_num;
                            }
                        }
                        let mut ev_len = if back.is_empty() { stacked_arrow_rows(&front, false) } else { ev.len() - 1 };
                        if let Some((tracks, n)) = struct_write_tracks(&front).filter(|_| back.is_empty()) {
                            arrow_tracks.extend(tracks);
                            ev_len = half_row_tracks_extra_rows(n);
                        }
                        if is_param_value_line(&ev) {
                            ev_len = 0;
                        }
                        // (Not an arrow a conditional nested in this arm
                        // holds: that arm draws it, #1279.)
                        let nested_drawn = arm_listed_arrows(&b_data.e_data);
                        let here = |e: &ExternalEvent| !nested_drawn.contains(&e.get_id())
                            || b_data.e_data.iter().any(|(_, p_e)| p_e.get_id() == e.get_id());
                        let front: Vec<ExternalEvent> = front.into_iter().filter(|e| here(e)).collect();
                        let back: Vec<ExternalEvent> = back.into_iter().filter(|e| here(e)).collect();
                        // A stack that ends the borrow of a reference the arm
                        // declares (`let x = ra; (x.to_string(), h)`, #1277):
                        // the rest of the line went on the stack's last row,
                        // where that return arrow runs back to its lender, so
                        // a call's result moved into a column or a scope end
                        // put its dot under the arrow, which ran through it.
                        // Those go a row below the stack instead.
                        let ends_arm_ref = ev.iter().any(|g| matches!(g,
                            ExternalEvent::StaticDie { from: ResourceTy::Value(r), .. }
                            | ExternalEvent::MutableDie { from: ResourceTy::Value(r), .. } if b_data.decl_vars.contains(r)));
                        let below = ends_arm_ref && rest.iter().any(|p_e| !own_row.contains(&p_e.get_id())
                            && !matches!(p_e, ExternalEvent::ClosureCall { .. })
                            && !uses_ref_ended_on_line(p_e, &ev));
                        if ev_len > 0 && back.is_empty() && below {
                            ev_len += 1;
                        }
                        if !back.is_empty() {
                            same_line_shift = Some((line_num, back_line - line_num));
                            new_line_map.insert(back_line, back);
                        }
                        new_line_map.insert(new_line_num, front); // insert new line number into new event line map
                        l_map.insert(new_line_num, ev_len);
                        ev_len
                        },
                        None => {
                            0
                        }
                    };
                    extra_lines += ex;
                    *extras_at_line.entry(line_num).or_insert(0) += ex;
                } else if matches!(e, ExternalEvent::ClosureCall { .. })
                    || (!matches!(same_line_shift, Some((src, _)) if src == line_num)
                        && old_line_map.get(&line_num).is_some_and(|g| uses_ref_ended_on_line(e, g)))
                {
                    // Same as the top-level loop: a closure call's dot,
                    // or an event that draws no arrow (a write through
                    // `*x`, #587), stays on its line, above rows
                    // inserted for stacked arrows on that line.
                    *l -= extras_at_line.get(&line_num).copied().unwrap_or(0);
                }
                // no need for else since we already updated the line
            }
        }
        i += 1;
    }

    // replace line_map
    b_data.line_map = new_line_map;
    extra_lines - old_extra_lines

}

/// Recursively shift every source-line reference inside an
/// `ExternalEvent` by `shift(line)`. Used when we inject blank
/// source lines between back-to-back functions.
fn shift_event_lines<F: Fn(usize) -> usize>(ev: &mut ExternalEvent, shift: &F) {
    if let ExternalEvent::Branch { branches, branch_type, split_point, merge_point, .. } = ev {
        *split_point = shift(*split_point);
        *merge_point = shift(*merge_point);
        match branch_type {
            crate::svg_generator::data::BranchType::If(_, v)
            | crate::svg_generator::data::BranchType::Loop(_, v)
            | crate::svg_generator::data::BranchType::Match(_, v) => {
                for (s, e) in v.iter_mut() {
                    *s = shift(*s);
                    *e = shift(*e);
                }
            }
        }
        for branch in branches.iter_mut() {
            for (line, sub) in branch.e_data.iter_mut() {
                *line = shift(*line);
                shift_event_lines(sub, shift);
            }
            let new_line_map: BTreeMap<usize, Vec<ExternalEvent>> = branch.line_map
                .iter()
                .map(|(k, v)| {
                    let mut v = v.clone();
                    for sub in v.iter_mut() { shift_event_lines(sub, shift); }
                    (shift(*k), v)
                })
                .collect();
            branch.line_map = new_line_map;
        }
    }
}

/// `(split_point, merge_point)` of every conditional in `ev` (nested
/// ones included) whose branches share source lines and that draws
/// branch columns (some variable is live in it).
fn collect_collapsed_branches(ev: &ExternalEvent, out: &mut Vec<(usize, usize)>) {
    if let ExternalEvent::Branch { branches, split_point, merge_point, live_vars, .. } = ev {
        if branch_is_collapsed(*split_point, *merge_point) && !live_vars.is_empty() {
            out.push((*split_point, *merge_point));
        }
        for branch in branches {
            for (_, sub) in &branch.e_data {
                collect_collapsed_branches(sub, out);
            }
        }
    }
}

/// Drop the source lines the plugin marked `// rustviz: hide` (the whole fn
/// body, plus its leading blank line) and shift every line-keyed structure
/// down to close the gap.
fn hide_source_lines(annotated_src_str: &str, source_rs_str: &str, visualization_data: &mut VisualizationData, hide_lines: &HashSet<usize>) -> (String, String) {
    if hide_lines.is_empty() {
        (annotated_src_str.to_string(), source_rs_str.to_string())
    } else {
        #[expect(clippy::disallowed_methods, reason = "sorted on the next line")]
        let mut sorted: Vec<usize> = hide_lines.iter().copied().collect();
        sorted.sort();
        let removed_set: HashSet<usize> = hide_lines.clone();
        // Number of hidden lines strictly less than `l`.
        let removed_before = move |l: usize| -> usize {
            sorted.iter().take_while(|&&r| r < l).count()
        };
        let shift = |l: usize| -> usize { l - removed_before(l) };

        let pruned_a: String = annotated_src_str
            .lines()
            .enumerate()
            .filter(|(i, _)| !removed_set.contains(&(i + 1)))
            .map(|(_, l)| l)
            .collect::<Vec<_>>()
            .join("\n");
        let pruned_s: String = source_rs_str
            .lines()
            .enumerate()
            .filter(|(i, _)| !removed_set.contains(&(i + 1)))
            .map(|(_, l)| l)
            .collect::<Vec<_>>()
            .join("\n");

        // Drop fn_start_lines pointing into a hidden range (the fn
        // is gone), shift the rest.
        visualization_data
            .fn_start_lines
            .retain(|_, l| !removed_set.contains(l));
        #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "updates each value on its own")]
        for v in visualization_data.fn_start_lines.values_mut() {
            *v = shift(*v);
        }

        // Drop events on hidden lines (events inside hidden fn
        // bodies); shift line numbers on the rest. Branch events
        // recurse via `shift_event_lines` and use the same closure.
        visualization_data
            .preprocess_external_events
            .retain(|(l, _)| !removed_set.contains(l));
        for (l, ev) in visualization_data.preprocess_external_events.iter_mut() {
            *l = shift(*l);
            shift_event_lines(ev, &shift);
        }

        let shifted_elm: BTreeMap<usize, Vec<ExternalEvent>> = visualization_data
            .event_line_map
            .iter()
            .filter(|(k, _)| !removed_set.contains(k))
            .map(|(k, v)| {
                let mut v = v.clone();
                for sub in v.iter_mut() { shift_event_lines(sub, &shift); }
                (shift(*k), v)
            })
            .collect();
        visualization_data.event_line_map = shifted_elm;

        // The approximation notes' lines too (a note in a hidden fn
        // finds no dot).
        for note in visualization_data.approx_notes.iter_mut() {
            note.map_lines(|l| shift(l));
        }

        (pruned_a, pruned_s)
    }
}

/// Force a blank source line before each non-first fn that doesn't already
/// have one, and around a conditional whose branches share lines. Returns
/// the padded annotated and plain sources and the visual rows of the blanks
/// inserted.
fn insert_blank_rows(annotated_src_str: &str, source_rs_str: &str, visualization_data: &mut VisualizationData) -> (String, String, HashSet<usize>) {
    // Force a blank source line before each non-first fn that doesn't
    // already have one. Without this, per-fn labels (which are placed
    // one row above the fn signature) land on top of the previous
    // fn's last timeline row. Done in source-line space at the top of
    // render_svg so the rest of the pipeline sees consistent line
    // numbers.
    let a_lines_orig: Vec<&str> = annotated_src_str.lines().collect();
    let s_lines_orig: Vec<&str> = source_rs_str.lines().collect();

    #[expect(clippy::disallowed_methods, reason = "sorted on the next statement")]
    let mut sorted_fn_starts: Vec<usize> = visualization_data
        .fn_start_lines
        .values()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    sorted_fn_starts.sort();

    let mut needs_blank_at: Vec<usize> = Vec::new();
    for &src_f in sorted_fn_starts.iter().skip(1) {
        if src_f < 2 { continue; }
        let prev = a_lines_orig.get(src_f - 2).map(|s| s.trim()).unwrap_or("");
        if !prev.is_empty() {
            needs_blank_at.push(src_f);
        }
    }
    // A conditional whose branches share lines gets a blank row above
    // its first line and below its last, unless there already is one:
    // its branches are drawn as a diamond whose top and bottom corners
    // (the split and the join) sit on those rows (#117).
    let mut collapsed: Vec<(usize, usize)> = Vec::new();
    for (_, ev) in visualization_data.preprocess_external_events.iter() {
        collect_collapsed_branches(ev, &mut collapsed);
    }
    let is_blank = |line: usize| a_lines_orig.get(line.wrapping_sub(1)).map_or(true, |s| s.trim().is_empty());
    for &(split, merge) in &collapsed {
        if split >= 2 && !is_blank(split - 1) {
            needs_blank_at.push(split);
        }
        if merge < a_lines_orig.len() && !is_blank(merge + 1) {
            needs_blank_at.push(merge + 1);
        }
    }
    needs_blank_at.sort();
    needs_blank_at.dedup();
    // One whose split is on a fn's signature line (a one-line fn, `fn
    // pick(x, y) -> &str { if … { x } else { y } }`) forks on the row
    // above the signature, where the fn's column headers are drawn: the
    // fork dots and the condition's call logo sat on the headers (#1342).
    // The headers go a row higher (`raised_headers`). And one whose join
    // is on the last line before a fn joins on the row below that line,
    // where the next fn's headers are drawn. So a fn gets a blank row for
    // its headers, one for its fork, and one for the join above it (the
    // first fn, only one for its fork: its headers may sit above the first
    // line or on a line outside any fn, as they do anyway).
    for for_split in sorted_fn_starts.iter().copied().filter(|&f| f >= 1) {
        let first = sorted_fn_starts.first() == Some(&for_split);
        let raised = collapsed.iter().any(|(split, _)| *split == for_split);
        let blank_above = (1..for_split).rev().take_while(|&l| is_blank(l)).count();
        let above = for_split - 1 - blank_above;
        let join_above = above >= 1 && collapsed.iter().any(|(_, merge)| *merge == above);
        let rows = if first { usize::from(raised) } else { 1 + usize::from(raised) + usize::from(join_above) };
        if raised {
            #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "collects into a set")]
            for (hash, line) in visualization_data.fn_start_lines.iter() {
                if *line == for_split {
                    visualization_data.raised_headers.insert(*hash);
                }
            }
        }
        // Up to that many blank rows above the line (the ones pushed
        // above count towards them); listed once per row.
        let have = blank_above + usize::from(needs_blank_at.contains(&for_split));
        needs_blank_at.extend(std::iter::repeat_n(for_split, rows.saturating_sub(have)));
    }
    needs_blank_at.sort();

    let new_a_str: String;
    let new_s_str: String;
    // Visual-row positions (1-indexed) of the synthetic blank lines
    // we just inserted. The Kth blank lands at `needs_blank_at[K]
    // + K` because K earlier inserts have already shifted everything
    // after them by one. The code-panel renderer consults this to
    // leave those rows un-numbered in the gutter — line numbers in
    // the visualization stay 1:1 with the user's editor.
    let synthetic_blank_rows: HashSet<usize> = needs_blank_at
        .iter()
        .enumerate()
        .map(|(i, src)| src + i)
        .collect();
    if needs_blank_at.is_empty() {
        new_a_str = annotated_src_str.to_string();
        new_s_str = source_rs_str.to_string();
    } else {
        let blanks_at = |line: usize| needs_blank_at.iter().filter(|&&l| l == line).count();
        let mut new_a: Vec<String> = Vec::with_capacity(a_lines_orig.len() + needs_blank_at.len());
        let mut new_s: Vec<String> = Vec::with_capacity(s_lines_orig.len() + needs_blank_at.len());
        for (i, line) in a_lines_orig.iter().enumerate() {
            new_a.extend(std::iter::repeat_n(String::new(), blanks_at(i + 1)));
            new_a.push((*line).to_string());
        }
        for (i, line) in s_lines_orig.iter().enumerate() {
            new_s.extend(std::iter::repeat_n(String::new(), blanks_at(i + 1)));
            new_s.push((*line).to_string());
        }
        new_a_str = new_a.join("\n");
        new_s_str = new_s.join("\n");

        let blanks = needs_blank_at.clone();
        let shift = |line: usize| -> usize {
            line + blanks.iter().filter(|&&f| f <= line).count()
        };

        #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "updates each value on its own")]
        for v in visualization_data.fn_start_lines.values_mut() {
            *v = shift(*v);
        }
        for (line, ev) in visualization_data.preprocess_external_events.iter_mut() {
            *line = shift(*line);
            shift_event_lines(ev, &shift);
        }
        let shifted_elm: BTreeMap<usize, Vec<ExternalEvent>> = visualization_data
            .event_line_map
            .iter()
            .map(|(k, v)| {
                let mut v = v.clone();
                for sub in v.iter_mut() { shift_event_lines(sub, &shift); }
                (shift(*k), v)
            })
            .collect();
        visualization_data.event_line_map = shifted_elm;
    }
    (new_a_str, new_s_str, synthetic_blank_rows)
}

/// Give every external event its row, inserting the extra rows stacked arrows
/// need. Returns the number of rows inserted below each line.
fn place_external_events(source_rs_str: &str, visualization_data: &mut VisualizationData) -> HashMap<usize, usize> {
    //-----------------------update line number for external events------------------
    // This might be the worst part of the code-base
    // extra lines need to be 'inserted' when two (or more) events that produce an arrow
    // occur on the same line (since the second+ arrow is rendered like a trapezoid)
    // However, it becomes more complicated with branches. The events inside branches need to actually be
    // mutated while the global events are just re-added. Honestly there's probably a better way to do this
    // but it turned out this way because it's built on RV1 code.
    // It's disgusting because it needs to be a single pass.
    let mut i: usize = 0;
    let mut size: usize = visualization_data.preprocess_external_events.len();
    let mut event_line_map_replace: BTreeMap<usize, Vec<ExternalEvent>> = BTreeMap::new();
    let mut extra_lines: usize = 0;
    // Source lines used to absorb stacked-arrow extras into pre-existing
    // blank lines below — see the `Some(ev)` branch in the loop.
    let source_lines_for_absorb: Vec<&str> = source_rs_str.lines().collect();
    // (source_line, extras_inserted_immediately_after_this_line). Records
    // each stacked-arrow site so we can apply the same shift to
    // fn_start_lines below — otherwise per-fn label y is computed in
    // source-line space while everything else (events, code panel rows)
    // is in visual-row space, and a fn whose signature sits past a
    // stacked arrow would land its label on top of the previous fn's
    // last timeline row.
    let mut extras_at_source_line: Vec<(usize, usize)> = Vec::new();
    // Rows a conditional's arms inserted (stacked arrows in an arm, and
    // the rows `OneLineArmGaps` adds), by the conditional's source line:
    // only for placing the fns' column headers below.
    let mut branch_extras: Vec<(usize, usize)> = Vec::new();
    // A variable moved / copied into a call (`take(y)`, `takes(s, r.len())`)
    // on a line where another arrow lands (typically a borrow ending)
    // joins that line's stacked arrows, so the two are drawn on separate
    // rows instead of one covering the other (#200).
    let into_fn_extra: Vec<(usize, ExternalEvent)> = visualization_data
        .preprocess_external_events
        .iter()
        .filter(|(line, event)| {
            event.is_arrow_ev()
                && matches!(event, ExternalEvent::Move { .. } | ExternalEvent::Copy { .. })
                && visualization_data.event_line_map.get(line).map_or(false, |ev| {
                    !ev.iter().any(|x| x.get_id() == event.get_id())
                })
                && {
                    let (from, to) = ResourceAccessPoint_extract(event);
                    from.extract_rap().map_or(false, |rap| !rap.is_fn())
                        && to.extract_rap().map_or(false, |rap| rap.is_fn())
                }
        })
        .map(|(line, event)| (*line, event.clone()))
        .collect();
    for (line, event) in into_fn_extra {
        if let Some(ev) = visualization_data.event_line_map.get_mut(&line) {
            ev.push(event);
        }
    }
    let mut skippable_ev: HashSet<usize> = HashSet::new();
    // Arrows of the top level's line map that an arm holds and lists in
    // its own line map, so the arm draws them (a borrow end moved into
    // the arm a let-chain's later parts are drawn in, #1279): by id.
    let top_level_ids: HashSet<usize> = visualization_data.preprocess_external_events.iter().map(|(_, e)| e.get_id()).collect();
    let arm_drawn: HashSet<usize> = arm_listed_arrows(&visualization_data.preprocess_external_events)
        .into_iter().filter(|id| !top_level_ids.contains(id)).collect();
    let mut line_insertion_map: HashMap<usize, usize> = HashMap::new();
    // The let-chain bindings the plugin found (before the arms' own are
    // added below).
    let chain_pairs = visualization_data.binding_columns.clone();
    // Same as in `mutate_branch_lines`: (source line, visual row) for
    // the rest of a line whose borrow ends were moved down a row.
    let mut same_line_row: Option<(usize, usize)> = None;
    // Borrow ends held back until the rest of their source line has been
    // appended (see `ends_after_line_uses`): (source line, event, row).
    let mut deferred_ends: Vec<(usize, ExternalEvent, usize)> = Vec::new();
    let mut first_lines: HashMap<u64, usize> = HashMap::new();
    first_event_lines(&visualization_data.preprocess_external_events, &mut first_lines);
    while i < size {
        // Append each held-back borrow end once its line has no event
        // left that uses the reference (so before the reference's own
        // scope end on that line, which doesn't use it).
        if !deferred_ends.is_empty() {
            let pending = &visualization_data.preprocess_external_events[i..];
            // Nor while another held-back end on its row gives a borrow
            // back to it (`y`'s end of its reborrow through the parameter
            // `t`, before `t`'s own; #1047).
            let held = std::mem::take(&mut deferred_ends);
            let (still, ready): (Vec<_>, Vec<_>) = held.iter().cloned().partition(|(src, e, row)| {
                ended_ref(e).is_some_and(|r| pending.iter().any(|(l, o)| {
                    l == src && !skippable_ev.contains(&o.get_id()) && uses_rap(o, r)
                }) || held.iter().any(|(l, o, w)| l == src && w == row && o.get_id() != e.get_id() && returns_to(o, r)))
            });
            deferred_ends = still;
            for (_, e, row) in ready {
                visualization_data.append_processed_external_event(e, row);
            }
        }
        if matches!(visualization_data.preprocess_external_events[i].1, ExternalEvent::Branch { .. })
            && !skippable_ev.contains(&visualization_data.preprocess_external_events[i].1.get_id())
        {
            size -= nest_arm_ref_ends(&mut visualization_data.preprocess_external_events, i, &skippable_ev, &mut visualization_data.event_line_map);
        }
        let (line_num, event) = visualization_data.preprocess_external_events.get_mut(i).unwrap();
        let mut branch_line = 0;
        if skippable_ev.contains(&event.get_id()) {
            i += 1;
            continue;
        }
        // println!("skippable events {:#?}", skippable_ev);
        // println!("line {} event {:#?}", line_num, event);
        match event {
            // need to mutate line numbers of events inside the branch
            ExternalEvent::Branch { branches, split_point, merge_point, branch_type, live_vars, .. } => {
                *split_point += extra_lines;
                branch_line = *line_num + extra_lines;
                let gaps = OneLineArmGaps::of(branches, branch_type, *merge_point, *line_num, live_vars, &line_insertion_map, &visualization_data.binding_columns, &visualization_data.arrow_tracks, false);
                let extra_before = extra_lines;
                for (j, branch) in branches.iter_mut().enumerate() {
                    let (start, end) = branch_type.get_mut_start_end(j);
                    if gaps.before_arm(j) {
                        *line_insertion_map.entry(*start + extra_lines).or_insert(0) += 1;
                        extra_lines += 1;
                    }
                    let pattern_lines = (*line_num, *start);
                    *start += extra_lines;
                    // recurse into the branch
                    let b = mutate_branch_lines(branch, &mut line_insertion_map, extra_lines, pattern_lines, &mut visualization_data.binding_columns, &mut visualization_data.arrow_tracks, gaps.spread());
                    extra_lines += b;
                    *end += extra_lines;
                }
                if gaps.before_merge() {
                    *line_insertion_map.entry(*merge_point + extra_lines).or_insert(0) += 1;
                    extra_lines += 1;
                }
                *merge_point += extra_lines + usize::from(gaps.join_past_inner);
                if extra_lines > extra_before {
                    branch_extras.push((*line_num, extra_lines - extra_before));
                }
            }
            _ => {}
        }
        // copies
        let line_num = *line_num;
        let event = event.clone();
        let final_line_num = match same_line_row {
            _ if branch_line != 0 => branch_line,
            Some((src, row)) if src == line_num => row,
            _ => line_num + extra_lines,
        };

        // append event
        // append any events that are on the same line in event line map to avoid double counting
        if event.is_arrow_ev() {
            // `event_line_map` only holds variable-to-variable arrows.
            // An arrow that isn't in it but shares the line used to
            // stand in for the whole line: the map's events were
            // appended (again, if already done) and the arrow itself
            // was dropped. That lost e.g. `Move from String::from to
            // s.b` next to `Move from t to s.c`. A Move/Copy out of or
            // into a function call, to the caller, or into a place with no
            // column (`self.name = s`, `r.name = s`: `to` is
            // Anonymous; otherwise s would later be shown dropping a
            // resource it gave away) now gets appended on its own, as
            // does a borrow of a temporary (`let r = &{};`, `let r =
            // &S { a: 4 };`: `from` is Anonymous), which otherwise loses
            // r's "immutably borrows a resource" start whenever r's
            // borrow also ends on that line (#204).
            //
            // The off-map arrows still left out are ones whose endpoints
            // are wrong upstream, so drawing them would add a false
            // tooltip: a reference's borrow "returned" to a function it
            // never borrowed from (`for word in s.split_whitespace()`:
            // "Return immutably borrowed resource from word to
            // split_whitespace"), and calling a closure read as
            // initialising it ("add_one is initialized with a copy of
            // Anonymous resource"). A Copy from a temporary into a
            // function's argument (`Some(*x + 1)`) draws nothing either
            // way.
            let id = event.get_id();
            let line_ev = visualization_data.event_line_map.get(&line_num);
            let in_map = line_ev.map_or(false, |ev| ev.iter().any(|x| x.get_id() == id));
            if line_ev.is_some() && !in_map {
                let (from, to) = ResourceAccessPoint_extract(&event);
                let is_transfer = matches!(event, ExternalEvent::Move { .. } | ExternalEvent::Copy { .. });
                let from_fn = from.extract_rap().map_or(false, |rap| rap.is_fn());
                // A value moved / copied into a call (`take(b)`,
                // `it.sum()`): dropping it left `b` shown as still
                // owning (and later dropping) what it gave away.
                let into_fn = to.extract_rap().map_or(false, |rap| rap.is_fn())
                    && !from_fn;
                let into_unmodelled_place = matches!(to, ResourceTy::Anonymous | ResourceTy::Temporary { .. })
                    && !matches!(from, ResourceTy::Anonymous);
                // A pattern's binding of a literal's element (`n` in `(s,
                // n) => …` on `match (&t, 1)`): an arrow from nowhere,
                // which gives the variable its value. Left out, n's
                // column never got one (#1281).
                let bound_from_literal = is_transfer
                    && matches!(from, ResourceTy::Anonymous)
                    && matches!(to, ResourceTy::Value(r) if !r.is_fn() && !r.is_closure());
                let is_borrow = matches!(event, ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. });
                let borrow_of_temporary = is_borrow
                    && matches!(from, ResourceTy::Anonymous)
                    && matches!(to, ResourceTy::Value(_) | ResourceTy::Deref(_));
                // A reference parameter's borrow ending (`r`'s last use
                // in `fn f(r: &mut T)` returns the borrow to the
                // caller) shares its line with the write through `r`
                // in `r[0] = x;` / `*r = s;`.
                let ref_param_ends = matches!(event, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. })
                    && matches!(to, ResourceTy::Caller);
                // A borrow stored in a place with no column (`v[0] = &u`,
                // `rh.r = &t`, #282) and its end are drawn on the
                // lender's column only, with no arrow, so they never
                // join the line's arrows. They used to be lost whenever
                // their line had another arrow (`rh`'s borrow of `hold`
                // ending on `rh.r = &t;`, #512).
                let place_borrow = (is_borrow && matches!(to, ResourceTy::Index(..)))
                    || (matches!(event, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. })
                        && matches!(from, ResourceTy::Index(..)));
                // A borrow kept by the closure a call returned (`let g =
                // keep(|| s.len());`, #686) isn't one of the line's
                // stacked arrows (see `add_external_event`): it follows
                // the closure's arrival from the call on g's column.
                let kept_by_returned_closure = matches!(&event,
                    ExternalEvent::StaticBorrow { via, .. } | ExternalEvent::MutableBorrow { via, .. } if returned_closure(via));
                // The end, where a `for` loop ends, of the borrow its
                // iterator holds (#868) is no arrow either. It used to be
                // lost when its line had an arrow: `let r = &mut s; for e
                // in r.iter_mut() { .. }` ends r's own borrow of s there
                // too (#1046).
                let loop_iterator_end = matches!(&event,
                    ExternalEvent::StaticDie { static_lender: Some(StaticLender::LoopIterator { .. } | StaticLender::LoopHeadBorrow { .. }), .. }
                    | ExternalEvent::MutableDie { static_lender: Some(StaticLender::LoopIterator { .. } | StaticLender::LoopHeadBorrow { .. }), .. });
                // A reference parameter's end on a line where a reborrow
                // through it gives its borrow back (`*y = 7;` as the last
                // use of `y = t.get_mut(0).unwrap()`) comes after that
                // end, as for a local's (#1047): it is held back until
                // the line's arrows are appended, on the row of the one
                // that gives the borrow back.
                if let (true, Some(line_ev)) = (ref_param_ends, line_ev) {
                    if let Some(r) = ended_ref(&event).filter(|r| line_ev.iter().any(|o| !skippable_ev.contains(&o.get_id()) && returns_to(o, *r))) {
                        let (front, back) = split_same_line_borrow_ends(line_ev);
                        let row = if back.iter().any(|o| returns_to(o, r)) { final_line_num + front.len() } else { final_line_num };
                        deferred_ends.push((line_num, event.clone(), row));
                        i += 1;
                        continue;
                    }
                }
                if (is_transfer && (from_fn || into_fn || matches!(to, ResourceTy::Caller) || into_unmodelled_place))
                    || bound_from_literal
                    || borrow_of_temporary
                    || kept_by_returned_closure
                    || ref_param_ends
                    || place_borrow
                    || loop_iterator_end {
                    // The end of the caller's loan held by a column whose
                    // other borrows the line's stacked arrows give back
                    // (`k` in `let k = [a, b, c];` with `a` a moved `&mut`
                    // parameter, #1337) goes on the dot those arrows leave
                    // from, the line's own row, not on the row below the
                    // stack, where it was a second dot saying the same.
                    let row = match line_ev {
                        Some(g) if ref_param_ends && !matches!(same_line_row, Some((src, _)) if src == line_num)
                            && g.iter().filter_map(ended_ref).any(|r| ended_ref(&event) == Some(r)) =>
                            final_line_num - extras_at_source_line.iter().filter(|(s, _)| *s == line_num).map(|(_, ex)| *ex).sum::<usize>(),
                        _ => final_line_num,
                    };
                    visualization_data.append_processed_external_event(event.clone(), row);
                }
                i += 1;
                continue;
            }
            let res = line_ev.cloned();
            // The end, recorded after a conditional that splits on this
            // line, of a reference one of its arms declares (`wr` in `if
            // let Some(x) = o && { let wr = r; take(wr); true } {`, whose
            // block is drawn in the if arm, #1262): it happens after the
            // arm's events, and `nest_arm_ref_ends` moves it into that arm
            // when the conditional's turn comes. It keeps its row in the
            // stack here, but isn't drawn on it, before wr is bound.
            let nested_later: HashSet<usize> = visualization_data.preprocess_external_events[i + 1..].iter()
                .position(|(l, e)| *l == line_num && matches!(e, ExternalEvent::Branch { .. }) && !skippable_ev.contains(&e.get_id()))
                .map(|b| {
                    let ExternalEvent::Branch { branches, .. } = &visualization_data.preprocess_external_events[i + 1 + b].1 else { unreachable!() };
                    visualization_data.preprocess_external_events[i + 2 + b..].iter()
                        .filter(|(l, e)| *l == line_num && matches!(e,
                            ExternalEvent::StaticDie { from: ResourceTy::Value(r), .. }
                            | ExternalEvent::MutableDie { from: ResourceTy::Value(r), .. }
                            | ExternalEvent::RefDie { from: ResourceTy::Value(r), .. } if arm_declares(branches, r)))
                        .map(|(_, e)| e.get_id())
                        .collect()
                })
                .unwrap_or_default();
            let ex = match res {
                Some(ev) if chain_bindings_side_by_side(&ev, &chain_pairs) => {
                    // A let-chain's bindings (#584): all on the line's
                    // own row, and left out of the line map, so each is
                    // drawn as the first arrow on its line, straight
                    // along the row (see `mutate_branch_lines`).
                    for e in &ev {
                        visualization_data.append_processed_external_event(e.clone(), final_line_num);
                        skippable_ev.insert(e.get_id());
                    }
                    0
                }
                Some(ev) => { // if there are multiple arrow events on this line
                    let (front, back) = split_same_line_borrow_ends(&ev);
                    let back_row = final_line_num + front.len();
                    let rest_of_line: Vec<ExternalEvent> = visualization_data.preprocess_external_events[i + 1..]
                        .iter()
                        .filter(|(l, e)| *l == line_num && !ev.iter().any(|g| g.get_id() == e.get_id()))
                        .map(|(_, e)| e.clone())
                        .collect();
                    // A variable bound on this line and copied or moved out
                    // of by one of its arrows is bound first: its binding
                    // goes on the line's own row, before the arrow's dot
                    // (#731).
                    for e in rest_of_line.iter().filter(|e| binds_source_of_line(e, &ev)) {
                        visualization_data.append_processed_external_event(e.clone(), final_line_num);
                        skippable_ev.insert(e.get_id());
                    }
                    for (e, row) in front.iter().map(|e| (e, final_line_num))
                        .chain(back.iter().map(|e| (e, back_row))) {
                        // A reference this arrow gives a new value, whose
                        // old borrow ends on this line (`r2` in `(r1, r2)
                        // = (r2, r1);`, #751): the end comes before the
                        // new value, on the arrow's row. Left in the
                        // line's sequence it was appended after the rows
                        // inserted for the stack, below the swap.
                        for d in rest_of_line.iter().filter(|d| ref_die_before_arrow(d, e, line_num, &first_lines) || drop_before_new_borrow(d, e) || drop_with_lender_return(d, e)) {
                            if skippable_ev.contains(&d.get_id()) {
                                continue;
                            }
                            visualization_data.append_processed_external_event(d.clone(), row);
                            skippable_ev.insert(d.get_id());
                        }
                        // An arrow of the line map that an arm holds (a
                        // borrow end moved into the arm a let-chain's
                        // later parts are drawn in, #1279) is drawn
                        // there, on this row of the stack; not here too.
                        if arm_drawn.contains(&e.get_id()) {
                            skippable_ev.insert(e.get_id());
                            continue;
                        }
                        if nested_later.contains(&e.get_id()) {
                            continue;
                        }
                        // append all the events in the line map on the same line
                        if ends_after_line_uses(e, &ev, &rest_of_line) {
                            deferred_ends.push((line_num, e.clone(), row));
                        } else {
                            visualization_data.append_processed_external_event(e.clone(), row);
                        }
                        skippable_ev.insert(e.get_id()); // they all become skippable
                        // The struct's fields move with it, on its row (#1283).
                        for d in rest_of_line.iter().filter(|d| part_moves_with_whole(d, e)) {
                            if skippable_ev.insert(d.get_id()) {
                                visualization_data.append_processed_external_event(d.clone(), row);
                            }
                        }
                    }
                    // What later uses or ends a variable an arrow moved
                    // into shares the arrival's dot (#1223).
                    for e in rest_of_line.iter().filter(|e| follows_arrival_on_line(e, &ev, &rest_of_line)) {
                        if skippable_ev.insert(e.get_id()) {
                            visualization_data.append_processed_external_event(e.clone(), final_line_num);
                        }
                    }
                    // Stacked arrows draw at consecutive vertical positions
                    // starting at this line's row; we'd normally insert
                    // (n - 1) blank rows below to give them clearance.
                    // But if the source already has consecutive blank
                    // lines right after `line_num`, those blank rows do
                    // the same job — reuse them instead of stacking our
                    // own on top, which produces an awkward double gap.
                    let mut needed = if back.is_empty() { stacked_arrow_rows(&front, true) } else { ev.len() - 1 };
                    if let Some((tracks, n)) = struct_write_tracks(&front).filter(|_| back.is_empty()) {
                        visualization_data.arrow_tracks.extend(tracks);
                        needed = half_row_tracks_extra_rows(n);
                    }
                    if is_param_value_line(&ev) {
                        needed = 0;
                    }
                    let trailing_blanks = source_lines_for_absorb
                        .iter()
                        .skip(line_num) // 1-indexed → start at the line right after line_num
                        .take_while(|l| l.trim().is_empty())
                        .count();
                    let to_insert = needed.saturating_sub(trailing_blanks);
                    // (Not the arm's arrows: the arm draws them.)
                    let front: Vec<ExternalEvent> = front.into_iter().filter(|e| !arm_drawn.contains(&e.get_id())).collect();
                    let back: Vec<ExternalEvent> = back.into_iter().filter(|e| !arm_drawn.contains(&e.get_id())).collect();
                    if !back.is_empty() {
                        same_line_row = Some((line_num, back_row));
                        event_line_map_replace.insert(back_row, back);
                    }
                    event_line_map_replace.insert(final_line_num, front); // insert new line number into new event line map
                    line_insertion_map.insert(final_line_num, to_insert);
                    to_insert
                },
                None => {
                    visualization_data.append_processed_external_event(event.clone(), final_line_num);
                    0
                }
            };

            if ex > 0 {
                extras_at_source_line.push((line_num, ex));
            }
            extra_lines += ex;
        }
        else if matches!(event, ExternalEvent::ClosureCall { .. }) {
            // Blank rows inserted for stacked arrows on this same
            // source line go *below* it; the call's dot belongs on the
            // line itself, not on the first inserted row.
            let same_line: usize = extras_at_source_line.iter()
                .filter(|(s, _)| *s == line_num)
                .map(|(_, ex)| *ex)
                .sum();
            visualization_data.append_processed_external_event(event.clone(), final_line_num - same_line);
        }
        else {
            // Like a closure call, an event that draws no arrow and uses
            // a reference whose borrow ends on this line (the write in
            // `*x += 1;`) stays on the line's own row, with the end,
            // above rows inserted for the line's stacked arrows (#587).
            // It used to land on the first inserted row, below the end.
            let same_line: usize = match same_line_row {
                Some((src, _)) if src == line_num => 0,
                _ if branch_line != 0 => 0,
                _ if !visualization_data.event_line_map.get(&line_num)
                    .is_some_and(|g| uses_ref_ended_on_line(&event, g)) => 0,
                _ => extras_at_source_line.iter()
                    .filter(|(s, _)| *s == line_num)
                    .map(|(_, ex)| *ex)
                    .sum(),
            };
            visualization_data.append_processed_external_event(event.clone(), final_line_num - same_line);
        }

        i += 1;
    }
    for (_, e, row) in deferred_ends {
        visualization_data.append_processed_external_event(e, row);
    }
    for timeline in visualization_data.timelines.values_mut() {
        ends_before_scope_ends(&mut timeline.history);
    }

    info!("insert line map {:#?}", line_insertion_map);
    visualization_data.external_events.sort_by(|(l, _), (l1, _)| l.cmp(l1));
    info!("processed events {:#?}", visualization_data.external_events);
    visualization_data.event_line_map = event_line_map_replace;

    // Convert each fn_start_line from a source line number to the
    // corresponding visual row by adding the cumulative extras
    // inserted strictly before that source line. Events with the
    // same source line as a fn signature don't shift the signature
    // (the extras are placed *after* the event's row, not before).
    // A fn after a conditional whose arms inserted rows starts that much
    // lower too: its headers sat on the rows above it, over its code
    // (#1187's one-line arms add rows of their own).
    extras_at_source_line.extend(branch_extras);
    extras_at_source_line.sort_by_key(|(s, _)| *s);
    #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "updates each value on its own")]
    for src_line in visualization_data.fn_start_lines.values_mut() {
        let extras_before: usize = extras_at_source_line
            .iter()
            .take_while(|(s, _)| *s < *src_line)
            .map(|(_, n)| *n)
            .sum();
        *src_line += extras_before;
    }
    line_insertion_map
}

/// Sort the events of each line (and of each branch's lines) into the order
/// they are drawn in.
fn sort_line_map_events(visualization_data: &mut VisualizationData) {
    //------------------------sort HashMap<usize, Vec<ExternalEvent>>----------------------
    // We need to sort the event line map (the data structure that holds the events that produce arrows between timelines) by hash 
    // because when rendering the arrow we need some way to determine the direction of the arrow
    //
    // We have to sort after appending events because the order by which events are added to the processed external events matter
    // For example: say at line x: [StaticDie(a, b), StaticDie(b, c)]
    // but because of sorting the order gets switched such that at line x: [StaticDie(b, c), StaticDie(a, b)]
    // then (due to how events are appended above) b will return it's resource to c before reacquiring it from b which messes up its state
    for (_, event_vec) in &mut visualization_data.event_line_map {
        event_vec.sort_by(by_rap_hashes);
    }
    // A move into a call goes first: it sits on the line's own row, and
    // the line's other arrows (a borrow's return) stack below it (#200).
    for (_, event_vec) in &mut visualization_data.event_line_map {
        event_vec.sort_by_key(|e| !moves_into_fn(e));
    }
    // sort all the line maps in the branch events
    for (_, e) in visualization_data.preprocess_external_events.iter_mut() {
        match e {
            ExternalEvent::Branch { branches, .. } => {
                for b in branches.iter_mut() {
                    sort_branch_external_events(b);
                }
            }
            _ => {}
        }
    }
}

pub fn render_svg(
    annotated_src_str: &str,
    source_rs_str: &str,
    visualization_data: &mut VisualizationData,
    hide_lines: &HashSet<usize>,
) -> (String, String, i32){
    info!("preprocessed events : {:#?}", visualization_data.preprocess_external_events);
    info!("ev_line_map: {:#?}", visualization_data.event_line_map);

    // First, drop any source lines that the plugin marked with the
    // `// rustviz: hide` form (the whole fn body, plus its leading
    // blank line) and shift every line-keyed structure down to
    // close the gap. Done before the fn-blank-line padding pass
    // below so that pass operates on the post-hide layout — `prev`
    // checks for fn neighbours, etc., reflect what the user will
    // actually see.
    let (post_hide_a, post_hide_s) = hide_source_lines(annotated_src_str, source_rs_str, visualization_data, hide_lines);
    let annotated_src_str: &str = post_hide_a.as_str();
    let source_rs_str: &str = post_hide_s.as_str();

    let (new_a_str, new_s_str, synthetic_blank_rows) = insert_blank_rows(annotated_src_str, source_rs_str, visualization_data);
    let annotated_src_str: &str = new_a_str.as_str();
    let source_rs_str: &str = new_s_str.as_str();

    let line_insertion_map = place_external_events(source_rs_str, visualization_data);

    sort_line_map_events(visualization_data);
    info!("processed line map {:#?}", visualization_data.event_line_map);

    let svg_code_template = CODE_PANEL_TEMPLATE;
    let svg_timeline_template = TIMELINE_PANEL_TEMPLATE;
    visualization_data.compute_states();

    // println!("timelines {:#?}", visualization_data.timelines);
    
    let a_lines = annotated_src_str.lines();
    let s_lines = source_rs_str.lines();

    let mut handlebars = Handlebars::new();
    // We want to preserve the inputs `as is`, and want to make no changes based on html escape.
    handlebars.register_escape_fn(handlebars::no_escape);
    let code_svg_template = svg_code_template;
    let tl_svg_template = svg_timeline_template;
    // register the template. The template string will be verified and compiled.
    assert!(handlebars
        .register_template_string("code_svg_template", code_svg_template)
        .is_ok());
    assert!(handlebars
        .register_template_string("timeline_svg_template", tl_svg_template)
        .is_ok());

    // data for code panel
    let mut max_x_space: i64 = 0;
    let (output, line_of_code, row_lines) =
            code_panel::render_code_panel(a_lines, s_lines, &mut max_x_space, &visualization_data.event_line_map, &line_insertion_map, &synthetic_blank_rows);
    let code_panel_string = output;
    let num_lines = line_of_code;

    // data for tl panel
    visualization_data.row_lines = row_lines.clone();
    let (timeline_panel_string, max_width) = timeline_panel::render_timeline_panel(visualization_data);
    // Each element's `data-rv-id`, and its rows as code-panel lines.
    let timeline_panel_string = rv_attrs::finalize(&timeline_panel_string, &row_lines);
    let tl_rv = rv_attrs::invalid_states_attr(visualization_data, &row_lines);
    
    // Code-panel width: scale with the longest source line so long
    // function signatures (e.g. `fn compare_strings(_a: &String,
    // _b: &String) -> bool {`) aren't clipped at the SVG's right
    // edge. ~9 px per char at the rendered code font is a reasonable
    // approximation; +40 px padding accounts for the line-number
    // gutter and a little right-side margin. Cap to a 400 px floor so
    // tiny snippets keep the historical visual proportions.
    let code_panel_width = std::cmp::max(400, (max_x_space as i32) * 9 + 40);
    let mut svg_data = SvgData {
        visualization_name: "vis".to_owned(),
        // Both panels share one stylesheet, with a colour rule for
        // every hash either of them uses.
        css: theme_css()
            + CSS_TEMPLATE
            + &hash_css(max_hash_in(&code_panel_string).max(max_hash_in(&timeline_panel_string))),
        code: code_panel_string,
        diagram: timeline_panel_string,
        tl_id: "tl_".to_owned() + "vis",
        tl_width: code_panel_width,
        // The last code line's text sits at y = LINE_SPACE * (num_lines + 1).
        // Nothing in either panel is drawn more than ~5px below it (a
        // drop dot's radius, or a glyph's descender), so a fixed margin
        // below it leaves an even gap about as tall as the one above
        // the first line. It used to be a fixed 100px band (#844).
        height: (num_lines + 1) * LINE_SPACE as i32 + BOTTOM_MARGIN
            - (OLD_FIRST_ROW_Y - FIRST_ROW_Y) as i32,
        tl_rv,
    };

    let final_code_svg_content = handlebars.render("code_svg_template", &svg_data).unwrap();
    svg_data.tl_width = std::cmp::max(max_width, 200);
    let final_timeline_svg_content = handlebars
        .render("timeline_svg_template", &svg_data)
        .unwrap();

    // Prose in tooltips may quote `&&` / `<` (#1049): make every
    // attribute value valid XML before the SVG leaves the plugin.
    (
        escape_tooltip_attrs(&final_code_svg_content),
        escape_tooltip_attrs(&final_timeline_svg_content),
        svg_data.height,
    )
}
