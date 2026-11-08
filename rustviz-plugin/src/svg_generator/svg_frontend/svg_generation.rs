extern crate handlebars;

use crate::svg_generator::data::{branch_is_collapsed, returned_closure, ExtBranchData, ExternalEvent, ResourceAccessPoint, ResourceAccessPoint_extract, ResourceTy, Visualizable, VisualizationData, LINE_SPACE};
use crate::svg_generator::svg_frontend::{code_panel, timeline_panel};
use handlebars::Handlebars;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use crate::svg_generator::svg_frontend::templates::*;
use log::info;

#[derive(Serialize)]
struct SvgData {
    visualization_name: String,
    css: String,
    code: String,
    diagram: String,
    tl_id: String,
    tl_width: i32,
    height: i32,
}

fn sort_branch_external_events(b_data: & mut ExtBranchData) {
    // sort first,
    for (_, event_vec) in b_data.line_map.iter_mut() {
        event_vec.sort_by(|a, b| {
            ResourceAccessPoint_extract(a)
                .1
                .hash()
                .cmp(&ResourceAccessPoint_extract(b).1.hash())
                .then(
                    ResourceAccessPoint_extract(a)
                        .0
                        .hash()
                        .cmp(&ResourceAccessPoint_extract(b).0.hash()),
                )
        });
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
fn ends_after_line_uses(e: &ExternalEvent, group: &[ExternalEvent], rest: &[ExternalEvent]) -> bool {
    let Some(r) = ended_ref(e) else { return false };
    group.iter().chain(rest).filter(|o| o.get_id() != e.get_id()).any(|o| uses_rap(o, r))
}

/// The hash of the reference whose borrow `e` ends, if it is a
/// StaticDie / MutableDie.
fn ended_ref(e: &ExternalEvent) -> Option<u64> {
    if !matches!(e, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. }) {
        return None;
    }
    ResourceAccessPoint_extract(e).0.extract_rap().map(|r| *r.hash())
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

/// Shifts the lines of the arm `b_data` by the rows inserted above it
/// and below its stacked arrows. `pattern_lines` are the (unshifted)
/// lines its conditional splits on and the arm starts on, where its
/// pattern binds (`} else if let Some(a) = &x && … {`);
/// `binding_columns` collects the pattern bindings drawn side by side
/// there (`side_by_side_bindings`).
pub fn mutate_branch_lines(
    b_data: & mut ExtBranchData,
    l_map: & mut HashMap<usize, usize>,
    mut extra_lines: usize,
    pattern_lines: (usize, usize),
    binding_columns: &mut Vec<(u64, u64)>,
    arrow_tracks: &mut HashMap<usize, usize>,
) -> usize {
    let old_extra_lines = extra_lines;
    let old_line_map = b_data.line_map.clone();
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
    let size: usize = b_data.e_data.len();
    while i < size {
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
            ExternalEvent::Branch {branches, split_point, merge_point, branch_type, .. } => {
                *split_point = *l; // update split point
                for (j, branch) in branches.iter_mut().enumerate(){ // mutate branches
                    let (start, end) = branch_type.get_mut_start_end(j);
                    let pattern_lines = (line_num, *start);
                    *start += extra_lines;
                    let b = mutate_branch_lines(branch, l_map, extra_lines, pattern_lines, binding_columns, arrow_tracks);
                    extra_lines += b;
                    *end += extra_lines;
                }
                
                *merge_point += extra_lines;
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
                        let mut ev_len = ev.len() - 1;
                        if let Some((tracks, n)) = struct_write_tracks(&front).filter(|_| back.is_empty()) {
                            arrow_tracks.extend(tracks);
                            ev_len = half_row_tracks_extra_rows(n);
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
    let (post_hide_a, post_hide_s) = if hide_lines.is_empty() {
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

        (pruned_a, pruned_s)
    };
    let annotated_src_str: &str = post_hide_a.as_str();
    let source_rs_str: &str = post_hide_s.as_str();

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
    for (split, merge) in collapsed {
        if split >= 2 && !is_blank(split - 1) {
            needs_blank_at.push(split);
        }
        if merge < a_lines_orig.len() && !is_blank(merge + 1) {
            needs_blank_at.push(merge + 1);
        }
    }
    needs_blank_at.sort();
    needs_blank_at.dedup();

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
        let needs_set: HashSet<usize> = needs_blank_at.iter().copied().collect();
        let mut new_a: Vec<String> = Vec::with_capacity(a_lines_orig.len() + needs_blank_at.len());
        let mut new_s: Vec<String> = Vec::with_capacity(s_lines_orig.len() + needs_blank_at.len());
        for (i, line) in a_lines_orig.iter().enumerate() {
            if needs_set.contains(&(i + 1)) { new_a.push(String::new()); }
            new_a.push((*line).to_string());
        }
        for (i, line) in s_lines_orig.iter().enumerate() {
            if needs_set.contains(&(i + 1)) { new_s.push(String::new()); }
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
    let annotated_src_str: &str = new_a_str.as_str();
    let source_rs_str: &str = new_s_str.as_str();

    //-----------------------update line number for external events------------------
    // This might be the worst part of the code-base
    // extra lines need to be 'inserted' when two (or more) events that produce an arrow
    // occur on the same line (since the second+ arrow is rendered like a trapezoid)
    // However, it becomes more complicated with branches. The events inside branches need to actually be
    // mutated while the global events are just re-added. Honestly there's probably a better way to do this
    // but it turned out this way because it's built on RV1 code.
    // It's disgusting because it needs to be a single pass.
    let mut i: usize = 0;
    let size: usize = visualization_data.preprocess_external_events.len();
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
    while i < size {
        // Append each held-back borrow end once its line has no event
        // left that uses the reference (so before the reference's own
        // scope end on that line, which doesn't use it).
        if !deferred_ends.is_empty() {
            let pending = &visualization_data.preprocess_external_events[i..];
            let (still, ready): (Vec<_>, Vec<_>) = std::mem::take(&mut deferred_ends).into_iter().partition(|(src, e, _)| {
                ended_ref(e).is_some_and(|r| pending.iter().any(|(l, o)| {
                    l == src && !skippable_ev.contains(&o.get_id()) && uses_rap(o, r)
                }))
            });
            deferred_ends = still;
            for (_, e, row) in ready {
                visualization_data.append_processed_external_event(e, row);
            }
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
            ExternalEvent::Branch { branches, split_point, merge_point, branch_type, .. } => {
                *split_point += extra_lines;
                branch_line = *line_num + extra_lines;
                for (j, branch) in branches.iter_mut().enumerate() {
                    let (start, end) = branch_type.get_mut_start_end(j);
                    let pattern_lines = (*line_num, *start);
                    *start += extra_lines;
                    // recurse into the branch
                    let b = mutate_branch_lines(branch, &mut line_insertion_map, extra_lines, pattern_lines, &mut visualization_data.binding_columns, &mut visualization_data.arrow_tracks);
                    extra_lines += b;
                    *end += extra_lines;
                }

                *merge_point += extra_lines;
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
                if (is_transfer && (from_fn || into_fn || matches!(to, ResourceTy::Caller) || into_unmodelled_place))
                    || borrow_of_temporary
                    || kept_by_returned_closure
                    || ref_param_ends
                    || place_borrow {
                    visualization_data.append_processed_external_event(event.clone(), final_line_num);
                }
                i += 1;
                continue;
            }
            let res = line_ev.cloned();
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
                    for (e, row) in front.iter().map(|e| (e, final_line_num))
                        .chain(back.iter().map(|e| (e, back_row))) {
                        // append all the events in the line map on the same line
                        if ends_after_line_uses(e, &ev, &rest_of_line) {
                            deferred_ends.push((line_num, e.clone(), row));
                        } else {
                            visualization_data.append_processed_external_event(e.clone(), row);
                        }
                        skippable_ev.insert(e.get_id()); // they all become skippable
                    }
                    // Stacked arrows draw at consecutive vertical positions
                    // starting at this line's row; we'd normally insert
                    // (n - 1) blank rows below to give them clearance.
                    // But if the source already has consecutive blank
                    // lines right after `line_num`, those blank rows do
                    // the same job — reuse them instead of stacking our
                    // own on top, which produces an awkward double gap.
                    let mut needed = ev.len() - 1;
                    if let Some((tracks, n)) = struct_write_tracks(&front).filter(|_| back.is_empty()) {
                        visualization_data.arrow_tracks.extend(tracks);
                        needed = half_row_tracks_extra_rows(n);
                    }
                    let trailing_blanks = source_lines_for_absorb
                        .iter()
                        .skip(line_num) // 1-indexed → start at the line right after line_num
                        .take_while(|l| l.trim().is_empty())
                        .count();
                    let to_insert = needed.saturating_sub(trailing_blanks);
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

    info!("insert line map {:#?}", line_insertion_map);
    visualization_data.external_events.sort_by(|(l, _), (l1, _)| l.cmp(l1));
    info!("processed events {:#?}", visualization_data.external_events);
    visualization_data.event_line_map = event_line_map_replace;

    // Convert each fn_start_line from a source line number to the
    // corresponding visual row by adding the cumulative extras
    // inserted strictly before that source line. Events with the
    // same source line as a fn signature don't shift the signature
    // (the extras are placed *after* the event's row, not before).
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


    //------------------------sort HashMap<usize, Vec<ExternalEvent>>----------------------
    // We need to sort the event line map (the data structure that holds the events that produce arrows between timelines) by hash 
    // because when rendering the arrow we need some way to determine the direction of the arrow
    //
    // We have to sort after appending events because the order by which events are added to the processed external events matter
    // For example: say at line x: [StaticDie(a, b), StaticDie(b, c)]
    // but because of sorting the order gets switched such that at line x: [StaticDie(b, c), StaticDie(a, b)]
    // then (due to how events are appended above) b will return it's resource to c before reacquiring it from b which messes up its state
    for (_, event_vec) in &mut visualization_data.event_line_map {
        event_vec.sort_by(|a, b| {
            ResourceAccessPoint_extract(a)
                .1
                .hash()
                .cmp(&ResourceAccessPoint_extract(b).1.hash())
                .then(
                    ResourceAccessPoint_extract(a)
                        .0
                        .hash()
                        .cmp(&ResourceAccessPoint_extract(b).0.hash()),
                )
        });
    }
    // A move into a call goes first: it sits on the line's own row, and
    // the line's other arrows (a borrow's return) stack below it (#200).
    for (_, event_vec) in &mut visualization_data.event_line_map {
        event_vec.sort_by_key(|e| {
            let into_fn = matches!(e, ExternalEvent::Move { .. } | ExternalEvent::Copy { .. })
                && ResourceAccessPoint_extract(e).1.extract_rap().map_or(false, |rap| rap.is_fn());
            !into_fn
        });
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
    let (output, line_of_code) =
            code_panel::render_code_panel(a_lines, s_lines, &mut max_x_space, &visualization_data.event_line_map, &line_insertion_map, &synthetic_blank_rows);
    let code_panel_string = output;
    let num_lines = line_of_code;

    // data for tl panel
    let (timeline_panel_string, max_width) = timeline_panel::render_timeline_panel(visualization_data);
    
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
        css: CSS_TEMPLATE.to_owned()
            + &hash_css(max_hash_in(&code_panel_string).max(max_hash_in(&timeline_panel_string))),
        code: code_panel_string,
        diagram: timeline_panel_string,
        tl_id: "tl_".to_owned() + "vis",
        tl_width: code_panel_width,
        height: (num_lines * LINE_SPACE as i32 + 80) + 50,
    };

    let final_code_svg_content = handlebars.render("code_svg_template", &svg_data).unwrap();
    svg_data.tl_width = std::cmp::max(max_width, 200);
    let final_timeline_svg_content = handlebars
        .render("timeline_svg_template", &svg_data)
        .unwrap();

    (final_code_svg_content, final_timeline_svg_content, svg_data.height)
}
