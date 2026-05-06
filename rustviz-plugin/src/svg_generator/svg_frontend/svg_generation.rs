extern crate handlebars;

use crate::svg_generator::data::{ExtBranchData, ExternalEvent, ResourceAccessPoint_extract, Visualizable, VisualizationData, LINE_SPACE};
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

pub fn mutate_branch_lines(b_data: & mut ExtBranchData, l_map: & mut HashMap<usize, usize>, mut extra_lines: usize) -> usize {
    let old_extra_lines = extra_lines;
    let old_line_map = b_data.line_map.clone();
    let mut new_line_map: BTreeMap<usize, Vec<ExternalEvent>> = BTreeMap::new();
    let mut skippable_ev: HashSet<usize> = HashSet::new();

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
        *l += extra_lines; // update starting line of event
        match e {
            ExternalEvent::Branch {branches, split_point, merge_point, branch_type, .. } => {
                *split_point = *l; // update split point
                for (j, branch) in branches.iter_mut().enumerate(){ // mutate branches
                    let (start, end) = branch_type.get_mut_start_end(j);
                    *start += extra_lines;
                    let b = mutate_branch_lines(branch, l_map, extra_lines);
                    extra_lines += b;
                    *end += extra_lines;
                }
                
                *merge_point += extra_lines;
            }
            _ => {
                // add extra lines if we need to
                if e.is_arrow_ev() {
                    let res = old_line_map.get(&line_num).cloned();
                    let ex = match res {
                    Some(ev) => { // if there are multiple arrow events on this line
                        for e in ev.clone() {
                            skippable_ev.insert(e.get_id()); // they all become skippable
                            let mut j = i;
                            while j < size { // need to mutate all their line numbers
                                let (l, p_e) = b_data.e_data.get_mut(j).unwrap();
                                if p_e.get_id() == e.get_id(){
                                    *l = new_line_num;
                                    break;
                                }
                                j += 1;
                            }
                        }
                        let ev_len = ev.len() - 1;
                        new_line_map.insert(new_line_num, ev); // insert new line number into new event line map
                        l_map.insert(new_line_num, ev_len);
                        ev_len
                        },
                        None => {
                            0
                        }
                    };
                    extra_lines += ex;
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

pub fn render_svg(
    annotated_src_str: &str,
    source_rs_str: &str,
    visualization_data: &mut VisualizationData,
) -> (String, String){
    info!("preprocessed events : {:#?}", visualization_data.preprocess_external_events);
    info!("ev_line_map: {:#?}", visualization_data.event_line_map);

    // Force a blank source line before each non-first fn that doesn't
    // already have one. Without this, per-fn labels (which are placed
    // one row above the fn signature) land on top of the previous
    // fn's last timeline row. Done in source-line space at the top of
    // render_svg so the rest of the pipeline sees consistent line
    // numbers.
    let a_lines_orig: Vec<&str> = annotated_src_str.lines().collect();
    let s_lines_orig: Vec<&str> = source_rs_str.lines().collect();

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

    let new_a_str: String;
    let new_s_str: String;
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
    // (source_line, extras_inserted_immediately_after_this_line). Records
    // each stacked-arrow site so we can apply the same shift to
    // fn_start_lines below — otherwise per-fn label y is computed in
    // source-line space while everything else (events, code panel rows)
    // is in visual-row space, and a fn whose signature sits past a
    // stacked arrow would land its label on top of the previous fn's
    // last timeline row.
    let mut extras_at_source_line: Vec<(usize, usize)> = Vec::new();
    let mut skippable_ev: HashSet<usize> = HashSet::new();
    let mut line_insertion_map: HashMap<usize, usize> = HashMap::new();
    while i < size {
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
                    *start += extra_lines;
                    // recurse into the branch
                    let b = mutate_branch_lines(branch, &mut line_insertion_map, extra_lines);
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
        let final_line_num = if branch_line != 0 { branch_line } else {line_num + extra_lines};

        // append event
        // append any events that are on the same line in event line map to avoid double counting
        if event.is_arrow_ev() {
            let res = visualization_data.event_line_map.get(&line_num).cloned();
            let ex = match res {
                Some(ev) => { // if there are multiple arrow events on this line
                    for e in ev.clone() { // append all the events in the line map on the same line
                        visualization_data.append_processed_external_event(e.clone(), final_line_num);
                        skippable_ev.insert(e.get_id()); // they all become skippable 
                    }
                    let ev_len = ev.len() - 1;
                    event_line_map_replace.insert(final_line_num, ev); // insert new line number into new event line map
                    line_insertion_map.insert(final_line_num, ev_len);
                    ev_len
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
        else {
            visualization_data.append_processed_external_event(event.clone(), final_line_num);
        }

        i += 1;
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
    let css_string = CSS_TEMPLATE;
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
            code_panel::render_code_panel(a_lines, s_lines, &mut max_x_space, &visualization_data.event_line_map, &line_insertion_map);
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
        css: css_string.to_owned(),
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

    (final_code_svg_content, final_timeline_svg_content)
}
