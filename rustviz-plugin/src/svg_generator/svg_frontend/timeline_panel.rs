extern crate handlebars;
use crate::svg_generator::data::{branch_state_converter, convert_back, string_of_external_event, BranchData, BranchType, Event, ExternalEvent, LineState, ResourceAccessPoint, ResourceAccessPoint_extract, ResourceTy, State, StructsInfo, Visualizable, VisualizationData, LINE_SPACE};
use crate::svg_generator::hover_messages;
use crate::svg_generator::svg_frontend::line_styles::OwnerLine;
use handlebars::Handlebars;
use std::collections::{BTreeMap, HashMap};
use serde::Serialize;
use std::cmp::{self, max};

// set style for code string
static SPAN_BEGIN : &'static str = "&lt;span style=&quot;font-family: 'Source Code Pro', Consolas, 'Ubuntu Mono', Menlo, 'DejaVu Sans Mono', monospace, monospace !important;&quot;&gt;";
static SPAN_END : &'static str = "&lt;/span&gt;";
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

#[derive(Serialize)]
struct ResourceAccessPointLabelData {
    x_val: i64,
    y_val: i64,
    hash: String,
    name: String,
    title: String
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
    title: String
}

#[derive(Serialize)]
struct FunctionLogoData {
    hash: u64,
    x: i64,
    y: i64,
    title: String
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
    title: String
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

pub fn render_timeline_panel(visualization_data : & mut VisualizationData) -> (String, i32) {
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
    
    // render resource owner labels
    render_timelines(&mut output, visualization_data, &resource_owners_layout, &registry); // vertical bars
    render_labels_string(&mut output, &resource_owners_layout, &visualization_data.fn_start_lines, &registry); // headers
    render_dots_string(&mut output, visualization_data, &resource_owners_layout, &registry); // dot events
    render_ref_line(&mut output, visualization_data, &resource_owners_layout, &registry); // reference lines
    render_arrows_string_external_events_version(&mut output, visualization_data, &resource_owners_layout, &registry); // arrows
    render_struct_box(&mut output, &structs_info, &visualization_data.fn_start_lines, &registry); // struct box

    let mut output_string : String = String::new();
    for (hash, (timelinepanel, member_timelinepanel)) in output{
        let struct_name;
        if hash == -1 {
            struct_name = String::from("non-struct");
        } else {
            struct_name = match visualization_data.get_name_from_hash(&(hash as u64)) {
                Some(_name) => _name,
                // Every hash that lands in `output` came from a RAP that was
                // already registered in `timelines`; the inverse lookup must
                // therefore succeed.
                None => unreachable!("hash {} present in output but not in timelines", hash),
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

    (output_string, width)

}

fn prepare_registry(registry: &mut Handlebars) {
    // We want to preserve the inputs `as is`, and want to make no changes based on html escape.
    registry.register_escape_fn(handlebars::no_escape);

    let timeline_panel_template =
        "    <g id=\"labels\">\n{{ labels }}    </g>\n\n    \
        <g id=\"timelines\">\n{{ timelines }}    </g>\n\n    \
        <g id=\"ref_line\">\n{{ ref_line }}    </g>\n\n    \
        <g id=\"events\">\n{{ dots }}    </g>\n\n    \
        <g id=\"arrows\">\n{{ arrows }}    </g>";

    let struct_template  = 
        "    <g id=\"{{struct_name}}\">\n\
             \t<g class=\"struct_instance\">\n{{ struct_instance }}</g>\n\
             \t<g class=\"struct_members\">\n{{ struct_members }}</g>\n\
             \t</g>\n    ";
    
    let label_template =
        "        <text x=\"{{x_val}}\" y=\"{{y_val}}\" style=\"text-anchor:middle\" data-hash=\"{{hash}}\" class=\"label tooltip-trigger\" data-tooltip-text=\"{{title}}\">{{name}}</text>\n";
    let dot_template =
        "        <circle cx=\"{{dot_x}}\" cy=\"{{dot_y}}\" r=\"5\" data-hash=\"{{hash}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"/>\n";
    // Used for Owner end-of-scope dots when the owner is still
    // holding the resource at that point — i.e. the resource is
    // dropped here. The visual is a normal dot with a small white
    // down-pointing triangle inside, so users can see that a drop
    // happened without having to hover for the tooltip. The
    // surrounding <g> carries the same hash + tooltip metadata as a
    // regular dot so existing UI behavior (highlighting on hash
    // hover, tooltip text) is unchanged.
    let drop_dot_template =
        "        <g data-hash=\"{{hash}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\">\n            <circle cx=\"{{dot_x}}\" cy=\"{{dot_y}}\" r=\"5\"/>\n            <polygon points=\"{{p1x}},{{p1y}} {{p2x}},{{p2y}} {{p3x}},{{p3y}}\" style=\"fill: white; stroke: none; pointer-events: none;\"/>\n        </g>\n";
    let function_dot_template =    
        "        <use xlink:href=\"#functionDot\" data-hash=\"{{hash}}\" x=\"{{x}}\" y=\"{{y}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"/>\n";
    let function_logo_template =
        "        <text x=\"{{x}}\" y=\"{{y}}\" data-hash=\"{{hash}}\" class=\"functionLogo tooltip-trigger fn-trigger\" data-tooltip-text=\"{{title}}\">f</text>\n";
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
        "        <g class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\">\n            <polyline stroke-width=\"5px\" stroke=\"gray\" points=\"{{coordinates_hbs}}\" style=\"fill: none;\"/>\n            <polygon points=\"{{head_points}}\" fill=\"gray\"/>\n        </g>\n";
    let vertical_line_template =
        "        <line data-hash=\"{{hash}}\" class=\"{{line_class}} tooltip-trigger\" x1=\"{{x1}}\" x2=\"{{x2}}\" y1=\"{{y1}}\" y2=\"{{y2}}\" data-tooltip-text=\"{{title}}\" style=\"opacity: {{opacity}}; stroke-dasharray: {{dasharray}};\"/>\n";
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
    let new_hollow_line_template = "<g class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\">\n            <line data-hash=\"{{hash}}\" class=\"hollow\" x1=\"{{x1}}\" y1=\"{{y1}}\" x2=\"{{x2}}\" y2=\"{{y2}}\" style=\"stroke-opacity: {{opacity}}; stroke-dasharray: {{dasharray}};\"/>\n            <line data-hash=\"{{hash}}\" class=\"hollow\" x1=\"{{x4}}\" y1=\"{{y4}}\" x2=\"{{x3}}\" y2=\"{{y3}}\" style=\"stroke-opacity: {{opacity}}; stroke-dasharray: {{dasharray}};\"/>\n            <polygon data-hash=\"{{hash}}\" points=\"{{x1}},{{y1}} {{x2}},{{y2}} {{x3}},{{y3}} {{x4}},{{y4}}\" style=\"fill:transparent; stroke:none; pointer-events:fill;\"/>\n        </g>\n";
    // Borrow-region trapezoids — visible as outlines (fill is
    // transparent), but `pointer-events:all` makes the fill area
    // hit-test too so hovering anywhere inside the trapezoid fires
    // the tooltip + glow, not just on the 2px outline strokes.
    let solid_ref_line_template =
        "        <path data-hash=\"{{hash}}\" class=\"mutref {{line_class}} tooltip-trigger\" style=\"fill:transparent; stroke-width: 2px !important; pointer-events: all;\" d=\"M {{x1}} {{y1}} l {{dx}} {{dy}} v {{v}} l -{{dx}} {{dy}}\" data-tooltip-text=\"{{title}}\"/>\n";
    let hollow_ref_line_template =
        "        <path data-hash=\"{{hash}}\" class=\"staticref tooltip-trigger\" style=\"fill: transparent; pointer-events: all;\" stroke-width=\"2px\" stroke-dasharray=\"3\" d=\"M {{x1}} {{y1}} l {{dx}} {{dy}} v {{v}} l -{{dx}} {{dy}}\" data-tooltip-text=\"{{title}}\"/>\n";
    let box_template =
        "        <rect id=\"{{name}}\" x=\"{{x}}\" y=\"{{y}}\" rx=\"20\" ry=\"20\" width=\"{{w}}\" height=\"{{h}}\" style=\"fill:white;stroke:black;stroke-width:3;opacity:0.1\" pointer-events=\"none\" />\n";

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

// Returns (max_left, max_right) extent in BRANCH_WEIGHT units that any
// nested branch leaf reaches from the timeline's center x. Mirrors the
// halfway-style placement formula in `update_timeline_data`, then walks
// each branch's recursive extent and maxes both sides separately.
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
            let n = branch_history.len();
            let halfway = n / 2;
            let mut centers: Vec<i64> = vec![0; n];

            let mut running: i64 = 0;
            for i in (0..halfway).rev() {
                let padding = if i == halfway - 1 { 1 } else { 0 };
                running += -(branch_history[i].width as i64 + padding);
                centers[i] = running;
                running -= 2;
            }
            running = 0;
            for i in halfway..n {
                let padding = if i == halfway { 1 } else { 0 };
                running += branch_history[i].width as i64 + padding;
                centers[i] = running;
                running += 2;
            }

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
                let mut parent_branch_data: Vec<TimelineColumnData> = Vec::new();
                let _ = ty; // BranchType doesn't affect x-positioning
                let halfway = branch_history.len() / 2;
                let mut running_x = parent_data.x_val;
                for i in (0..halfway).rev() {
                    let b_data = branch_history.get_mut(i).unwrap();
                    let b_width = b_data.width;
                    let padding = if i == halfway - 1 { 1 } else { 0 };
                    let left_side_coefficient = -1 * (b_width + padding) as i64;
                    let x = left_side_coefficient * BRANCH_WEIGHT;
                    running_x += x;
                    b_data.t_data.x_val = running_x;
                    running_x -= 2 * BRANCH_WEIGHT;
                }
                for i in 0..halfway {
                    let b_data = branch_history.get(i).unwrap();
                    parent_branch_data.push(b_data.t_data.clone());
                }

                running_x = parent_data.x_val;
                for i in halfway..branch_history.len() {
                    let b_data = branch_history.get_mut(i).unwrap();
                    let b_width = b_data.width;
                    let padding = if i == halfway { 1 } else { 0 };
                    let right_side_coefficient = (b_width + padding) as i64;
                    let x = right_side_coefficient * BRANCH_WEIGHT;
                    running_x += x;
                    b_data.t_data.x_val = running_x;
                    running_x += 2 * BRANCH_WEIGHT;
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

    for (_fn_line, hashes) in by_fn.iter() {
        let mut x: i64 = 0; // reset per fn
        let mut owner: i64 = -1;
        let mut owner_x: i64 = 0;
        let mut last_x: i64 = 0;

        for hash in hashes {
            let timeline = &visualization_data.timelines[hash];
            let name = match visualization_data.get_name_from_hash(hash) {
                Some(_name) => _name,
                // `hashes` was just populated by iterating `timelines`, so the
                // reverse lookup must succeed.
                None => unreachable!("hash {} present in timelines but missing reverse-name", hash),
            };
            let mut x_space = cmp::max(70, (&(name.len() as i64) - 1) * 13);
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
            if timeline.resource_access_point.is_ref() {
                let temp_name = name.clone() + "|*" + &name;
                x = x - x_space;
                x_space = cmp::max(90, (&(temp_name.len() as i64) - 1) * 7);
                x = x + x_space;
                ref_bool = true;
            }

            let styled_name = SPAN_BEGIN.to_string() + &name + SPAN_END;

            if (owner == -1) && timeline.resource_access_point.is_struct_group() && !timeline.resource_access_point.is_member() {
                owner = timeline.resource_access_point.hash().clone() as i64;
                owner_x = x;
            } else if (owner != -1) && timeline.resource_access_point.is_struct_group() && timeline.resource_access_point.is_member() {
                last_x = x;
            } else if (owner != -1) && !timeline.resource_access_point.is_struct_group() {
                structs_info.structs.push((owner, owner_x, last_x));
                owner = -1;
                owner_x = 0;
                last_x = 0;
            }

            resource_owners_layout.insert(*hash, TimelineColumnData
                {
                    name: name.clone(),
                    x_val: x,
                    title: styled_name.clone() + ", " + &title,
                    is_ref: ref_bool,
                    is_struct_group: timeline.resource_access_point.is_struct_group(),
                    is_member: timeline.resource_access_point.is_member(),
                    owner: timeline.resource_access_point.get_owner(),
                });
            x += right_px;
        }
        // Finalize any open struct group at the end of this fn
        // (same trailing-struct fix as before, scoped per-fn).
        if owner != -1 {
            structs_info.structs.push((owner, owner_x, last_x));
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

fn render_labels_string(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    fn_start_lines: &HashMap<u64, usize>,
    registry: &Handlebars
) {
    // Default label-y matches the legacy "all labels at the top of
    // the SVG" behavior; per-fn RAPs override below.
    const DEFAULT_LABEL_Y: i64 = 70;
    for (hash, column_data) in resource_owners_layout.iter() {
        // Position the label on the row directly above the fn's
        // first source line so each fn gets its own label header
        // adjacent to its body. `get_y_axis_pos(line)` is the
        // baseline for source line `line`; subtracting LINE_SPACE
        // puts us on the row above. Falls back to the legacy
        // top-of-svg position for RAPs without an fn association
        // (e.g. globals — none today, but defensive).
        let y_val = match fn_start_lines.get(hash) {
            Some(&line) => get_y_axis_pos(line) - LINE_SPACE,
            None => DEFAULT_LABEL_Y,
        };
        let mut data = ResourceAccessPointLabelData {
            x_val: column_data.x_val,
            y_val,
            hash: hash.to_string(),
            name: column_data.name.clone(),
            title: column_data.title.clone(),
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
    dot_data: &EventDotData,
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    timeline_data: &TimelineColumnData,
    registry: &Handlebars
) {
    let column = timeline_data;
    if column.is_struct_group {
        let owner_entry = ensure_owner_entry(output, column.owner as i64);
        if column.is_member {
            owner_entry.1.dots.push_str(&registry.render("dot_template", &dot_data).unwrap());
        } else {
            owner_entry.0.dots.push_str(&registry.render("dot_template", &dot_data).unwrap());
        }
    }
    else {
        output.get_mut(&-1).unwrap().0.dots.push_str(&registry.render("dot_template", &dot_data).unwrap());
    }
}

fn render_dot(
    hash: &u64,
    history: &Vec<(usize, Event)>,
    timeline_data: &TimelineColumnData,
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    registry: &Handlebars,
    resource_hold: bool
) {
    for (line_number, event) in history.iter() {
        // Closure-capture per-upvar dots on the closure's own
        // timeline are suppressed here: the closure's Bind-Acquire
        // dot at the same line carries a combined tooltip listing
        // every capture (see the Anonymous-from arm below). The
        // per-capture events stay in `history` because the arrow
        // renderer traverses them to find the closure column for
        // each capture arrow's endpoint.
        let is_closure_capture_event = match event {
            Event::Acquire { from: ResourceTy::Anonymous, .. } => false,
            Event::Acquire { is, .. }
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
            Event::Branch { is, branch_history, ty, merge_point, .. } => {
                // Top-of-branch dot on the parent column. Tooltip
                // explains the variable's role ("X is live in a
                // conditional expression"). Anchors the parent
                // column visually at the line where the conditional
                // begins.
                let b_data = EventDotData {
                    hash: *hash as u64,
                    dot_x: timeline_data.x_val,
                    dot_y: get_y_axis_pos(*line_number),
                    title: event.print_message_with_name(& mut is.real_name())
                };
                append_dot(&b_data, output, timeline_data, registry);

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
                for branch in branch_history.iter() {
                    render_dot(hash, &branch.e_data, &branch.t_data, output, visualization_data, registry, false);
                }

                // Per-variable merge dot, on the parent column at
                // `merge_point` (the closing-brace line). Sat at
                // `merge_point + 1` previously — a line below the
                // conditional, which read as a stray dot on
                // unrelated code below. Empty title for the
                // Unchanged case keeps it as a structural marker.
                //
                // When some branches moved the variable and at
                // least one didn't, Rust inserts an *implicit
                // drop* at the end of the non-moving branches so
                // the merged state stays consistent. Render the
                // merge dot as a drop dot (down-arrow triangle
                // inside the circle, same shape as a regular OOS
                // drop) and use a tooltip that explains the
                // semantics. The all-moved and all-alive cases
                // get the regular dot — no implicit drop is
                // inserted in those situations.
                // Look at each branch's *effective* end state — what
                // `branch.states.last()` reports — rather than just
                // walking events for syntactic Move/Acquire. That's
                // what carries the implicit-drop result of any
                // nested merge already in the column: an outer
                // branch whose body is `if c2 { consume(s) } else
                // { ... }` ends with ResourceMoved even though
                // there's no syntactic Move directly in the outer
                // branch's events.
                let any_moved = branch_history.iter().any(|b| {
                    b.states.last()
                        .map(|s| !state_is_alive(&s.2))
                        .unwrap_or(true)
                });
                let any_alive = branch_history.iter().any(|b| {
                    b.states.last()
                        .map(|s| state_is_alive(&s.2))
                        .unwrap_or(false)
                });
                // `if cond { … }` records one branch; the implicit
                // untouched else still keeps the resource alive on
                // the path the user didn't write. Treat that
                // implicit path as an alive branch when classifying
                // the merge: an all-moved-recorded if-without-else
                // is semantically a *mixed* merge (drop dot), not an
                // every-branch one.
                let has_implicit_untouched = !all_paths_present(ty, branch_history.len());
                let mixed_moved = (any_moved && any_alive)
                    || (any_moved && has_implicit_untouched);
                let all_moved = !any_alive
                    && !branch_history.is_empty()
                    && !has_implicit_untouched;
                let cx = timeline_data.x_val;
                let cy = get_y_axis_pos(*merge_point);
                if mixed_moved {
                    // Mixed: at least one branch moved, at least
                    // one didn't (counting the implicit untouched
                    // else of an `if` without an else as a
                    // didn't-move branch). Rust inserts an
                    // implicit drop in the non-moving branches so
                    // the post state matches; the drop dot at the
                    // merge makes that visible.
                    let title = hover_messages::event_dot_branch_merge_moved_with_drop(&is.real_name());
                    let drop_data = DropDotData {
                        hash: *hash as u64,
                        dot_x: cx,
                        dot_y: cy,
                        title,
                        p1x: cx - 3, p1y: cy - 1,
                        p2x: cx + 3, p2y: cy - 1,
                        p3x: cx,     p3y: cy + 3,
                    };
                    append_drop_dot(&drop_data, output, timeline_data, registry);
                } else if all_moved {
                    // Every branch ends without the resource —
                    // either by a direct move or by an implicit
                    // drop already inserted at a nested merge.
                    // Don't say "at least one"; pin it as "every".
                    let title = hover_messages::event_dot_branch_merge_all_moved(&is.real_name());
                    let m_data = EventDotData {
                        hash: *hash as u64,
                        dot_x: cx,
                        dot_y: cy,
                        title,
                    };
                    append_dot(&m_data, output, timeline_data, registry);
                } else {
                    let m_data = EventDotData {
                        hash: *hash as u64,
                        dot_x: cx,
                        dot_y: cy,
                        title: branch_join_message(branch_history, ty, &is.real_name()),
                    };
                    append_dot(&m_data, output, timeline_data, registry);
                }
                continue;
            }
            _ => {} //do nothing
        }
        
        let mut data = EventDotData {
            hash: *hash as u64,
            dot_x: timeline_data.x_val,
            dot_y: get_y_axis_pos(*line_number),
            // default value if print_message_with_name() fails
            title: "Unknown Resource Owner Value".to_owned()
        };
        if let Some(mut name) = visualization_data.get_name_from_hash(hash) {
            match event {
                Event::OwnerGoOutOfScope => {
                    let ro = &visualization_data.timelines[hash].resource_access_point;
                    let is_copy = ro.is_copy();
                    let is_closure = ro.is_closure();
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
                        let move_count = closure_move_capture_count(visualization_data, *hash);
                        if move_count > 0 {
                            let cx = timeline_data.x_val;
                            let cy = get_y_axis_pos(*line_number);
                            let title = hover_messages::event_dot_closure_go_out_of_scope(&name, move_count);
                            let drop_data = DropDotData {
                                hash: *hash as u64,
                                dot_x: cx,
                                dot_y: cy,
                                title,
                                p1x: cx - 3, p1y: cy - 1,
                                p2x: cx + 3, p2y: cy - 1,
                                p3x: cx,     p3y: cy + 3,
                            };
                            append_drop_dot(&drop_data, output, timeline_data, registry);
                            continue;
                        }
                        data.title = event.print_message_with_name(&mut name);
                    } else if !resource_hold {
                        // Resource was already moved out — same copy
                        // for both Copy and non-Copy types: just note
                        // there's nothing to drop here.
                        let resource_info: &str = ". No resource is dropped.";
                        data.title = event.print_message_with_name(& mut name);
                        data.title.push_str(resource_info);
                    } else if is_copy {
                        // Copy types have no Drop glue — going out of
                        // scope just reclaims storage. Render a plain
                        // dot (no drop triangle) and skip the
                        // "resource is dropped" suffix.
                        data.title = event.print_message_with_name(&mut name);
                    } else {
                        // Render with a down-arrow triangle inside the
                        // dot to make the drop visible at a glance.
                        // Triangle is inscribed in the 5px-radius
                        // circle: base 1px above center (~6px wide),
                        // apex 3px below center.
                        let cx = timeline_data.x_val;
                        let cy = get_y_axis_pos(*line_number);
                        let mut title = event.print_message_with_name(&mut name);
                        title.push_str(". Its resource is dropped.");
                        let drop_data = DropDotData {
                            hash: *hash as u64,
                            dot_x: cx,
                            dot_y: cy,
                            title,
                            p1x: cx - 3, p1y: cy - 1,
                            p2x: cx + 3, p2y: cy - 1,
                            p3x: cx,     p3y: cy + 3,
                        };
                        append_drop_dot(&drop_data, output, timeline_data, registry);
                        continue;
                    }
                },
                // Reassignment-drop renders the same down-arrow dot as
                // OwnerGoOutOfScope (resource is dropped at this line),
                // but the owner stays in scope — there's always a
                // resource to drop here, so no `if !resource_hold` branch.
                Event::OwnerDropAtReassign => {
                    let cx = timeline_data.x_val;
                    let cy = get_y_axis_pos(*line_number);
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
                    append_drop_dot(&drop_data, output, timeline_data, registry);
                    continue;
                },
                _ => {
                    // Closure Bind-Acquire: aggregate every capture
                    // event landing at the same line into a single
                    // tooltip listing all captured upvars, since the
                    // per-capture target-side dots are suppressed
                    // (they'd stack on this one and the topmost
                    // tooltip would mask the rest).
                    if let Event::Acquire { from: ResourceTy::Anonymous, is, .. } = event {
                        if is.is_closure() {
                            let captures = collect_closure_captures(visualization_data, *hash, *line_number);
                            data.title = hover_messages::event_dot_closure_bind_with_captures(&name, &captures);
                        } else {
                            data.title = event.print_message_with_name(& mut name);
                        }
                    } else {
                        data.title = event.print_message_with_name(& mut name);
                    }
                }
            }
        }
        // push to individual timelines
        append_dot(&data, output, timeline_data, registry);
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

fn analyze_branch_for_join(events: &[(usize, Event)]) -> (bool, bool) {
    let mut moved = false;
    let mut acquired = false;
    for (_, e) in events {
        match e {
            Event::Move { .. } => { moved = true; acquired = false; }
            Event::Acquire { .. } => { moved = false; acquired = true; }
            Event::Branch { branch_history, ty, .. } => {
                let mut nested_any_moved = false;
                let mut nested_all_acquired = !branch_history.is_empty();
                for nb in branch_history {
                    let (m, a) = analyze_branch_for_join(&nb.e_data);
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

fn compute_branch_join(branch_history: &[BranchData], ty: &BranchType) -> BranchJoin {
    let mut any_moved = false;
    let mut all_acquired = !branch_history.is_empty();
    for b in branch_history {
        let (m, a) = analyze_branch_for_join(&b.e_data);
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

fn branch_join_message(branch_history: &[BranchData], ty: &BranchType, var_name: &String) -> String {
    match compute_branch_join(branch_history, ty) {
        BranchJoin::MovedAfter => hover_messages::event_dot_branch_merge_moved(var_name),
        BranchJoin::BoundHere => hover_messages::event_dot_branch_merge_bound(var_name),
        BranchJoin::Unchanged => String::new(),
    }
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
) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    for (l, ev) in &visualization_data.external_events {
        if *l != line {
            continue;
        }
        match ev {
            ExternalEvent::Move { from, to, .. } if matches_closure(to, closure_hash) => {
                if let Some(name) = upvar_name(from) {
                    out.push((name, "moved"));
                }
            }
            ExternalEvent::StaticBorrow { from, to, .. } if matches_closure(to, closure_hash) => {
                if let Some(name) = upvar_name(from) {
                    out.push((name, "immutably borrowed"));
                }
            }
            ExternalEvent::MutableBorrow { from, to, .. } if matches_closure(to, closure_hash) => {
                if let Some(name) = upvar_name(from) {
                    out.push((name, "mutably borrowed"));
                }
            }
            _ => {}
        }
    }
    out
}

// Count of upvars move-captured by the closure identified by
// `closure_hash`. Zero ⇒ borrow-only or capture-less closure;
// distinguishes the scope-end "captured resources are dropped"
// message and the timeline state line ("owns N resources via
// capture") from their borrow-only fallbacks.
fn closure_move_capture_count(
    visualization_data: &VisualizationData,
    closure_hash: u64,
) -> usize {
    visualization_data
        .external_events
        .iter()
        .filter(|(_, ev)| matches!(ev, ExternalEvent::Move { to, .. } if matches_closure(to, closure_hash)))
        .count()
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
) -> String {
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
    // Copy-typed owners (i32 etc.) — "the owner of the resource"
    // language doesn't fit primitives; route to the value-flavoured
    // wording so the lifeline tooltip stays consistent with the
    // event-dot tooltip on the same column.
    if rap.is_copy_owner() {
        if let State::FullPrivilege { .. } = state {
            return hover_messages::state_full_privilege_copyable(rap.name());
        }
    }
    state.print_message_with_name(rap.name())
}

fn matches_closure(rty: &ResourceTy, closure_hash: u64) -> bool {
    rty.extract_rap().map_or(false, |r| *r.hash() == closure_hash)
}

fn upvar_name(rty: &ResourceTy) -> Option<String> {
    rty.extract_rap().map(|r| r.name().to_owned())
}

// Same routing logic as `append_dot` (struct-grouped vs flat
// timelines), but emits the drop-dot SVG (circle + inner triangle)
// instead of the plain circle.
fn append_drop_dot(
    drop_data: &DropDotData,
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    timeline_data: &TimelineColumnData,
    registry: &Handlebars,
) {
    let column = timeline_data;
    let rendered = registry.render("drop_dot_template", drop_data).unwrap();
    if column.is_struct_group {
        let owner_entry = ensure_owner_entry(output, column.owner as i64);
        if column.is_member {
            owner_entry.1.dots.push_str(&rendered);
        } else {
            owner_entry.0.dots.push_str(&rendered);
        }
    } else {
        output.get_mut(&-1).unwrap().0.dots.push_str(&rendered);
    }
}

fn render_dots_string(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    registry: &Handlebars
){
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
                } else { false };
                render_dot(hash, &timeline.history, &resource_owners_layout[hash], output, visualization_data, registry, resource_hold);
            },
        }
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


fn fetch_timeline<'a>(hash: &u64, vd: &'a VisualizationData, ro_layout: & 'a BTreeMap<u64, TimelineColumnData>, id: usize) -> & 'a TimelineColumnData {
    match traverse_timeline2(&ro_layout[hash], &vd.timelines[hash].history, id) {
        Some(t) => t,
        // `id` came from an ExternalEvent currently being rendered; that
        // event was already inserted into the RAP's history during
        // event-flattening, so the traversal must hit it.
        None => unreachable!("event id {} for hash {} not found in any timeline column", id, hash),
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
        // Same invariant as `fetch_timeline`: this id was produced by an
        // event already present in the event tree, so traversal must find it.
        None => unreachable!("event id {} not found in external_events tree", id),
    }
}

// render arrow
fn render_arrow (
    line_number : &usize,
    external_event: &ExternalEvent,
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
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
                        | ExternalEvent::RefDie {..} => {}
                        _ => {
                            render_arrow(l, e,  output, visualization_data, resource_owners_layout, registry);
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
        ExternalEvent::InitRefParam { param, id } => {
            let is_owned = matches!(
                param,
                ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_)
            );
            if !is_owned {
                return;
            }
            let timeline = fetch_timeline(param.hash(), visualization_data, resource_owners_layout, *id);
            let cx = timeline.x_val as f64;
            let cy = get_y_axis_pos(*line_number) as f64;

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

            let title = hover_messages::event_dot_owner_init_from_caller(&param.name().to_string());

            // Reuses the regular arrow_template shape (one polygon
            // for the head) but emitted inline because we already
            // have ArrowData populated and assembling the template
            // is overkill for one site.
            let rendered = format!(
                "        <g class=\"tooltip-trigger\" data-tooltip-text=\"{title}\">\n\
                    \x20           <polyline stroke-width=\"5px\" stroke=\"gray\" points=\"{polyline_pts}\" style=\"fill: none;\"/>\n\
                    \x20           <polygon points=\"{},{} {},{} {},{}\" fill=\"gray\"/>\n\
                    \x20       </g>\n",
                bot_v1.0, bot_v1.1, bot_v2.0, bot_v2.1, bot_v3.0, bot_v3.1,
                title = title,
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
        | ExternalEvent::Copy { from, to: ResourceTy::Caller, id, .. } => {
            let rap = match from.extract_rap() {
                Some(r) => r,
                None => return,
            };
            let timeline = fetch_timeline(rap.hash(), visualization_data, resource_owners_layout, *id);
            let cx = timeline.x_val as f64;
            let cy = get_y_axis_pos(*line_number) as f64;

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
            let source_x = cx + 5.25;
            let bend_x = cx + head_offset + leg;
            let vertical_line: f64 = leg / 2.0;
            let head_end_y = cy - vertical_line;

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

            let title = hover_messages::event_dot_move_to_caller(
                &rap.name().to_string(),
                &"the caller".to_string(),
            );

            let rendered = format!(
                "        <g class=\"tooltip-trigger\" data-tooltip-text=\"{title}\">\n\
                    \x20           <polyline stroke-width=\"5px\" stroke=\"gray\" points=\"{polyline_pts}\" style=\"fill: none;\"/>\n\
                    \x20           <polygon points=\"{},{} {},{} {},{}\" fill=\"gray\"/>\n\
                    \x20       </g>\n",
                head_v1.0, head_v1.1, head_v2.0, head_v2.1, head_v3.0, head_v3.1,
                title = title,
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
        _ => {
            // get the resource owners involved in the event
            let (from, to) = ResourceAccessPoint_extract(external_event);
            match (from, to) { // don't render arrow for anything to caller or anonymous or fn -> fn
                (ResourceTy::Anonymous, _) | (_, ResourceTy::Caller) | (_, ResourceTy::Anonymous)
                | (ResourceTy::Value(ResourceAccessPoint::Function(_)), ResourceTy::Value(ResourceAccessPoint::Function(_))) => return,
                _ => {}
            }
            let mut title = string_of_external_event(external_event);
            // complete title
            let styled_from_string = SPAN_BEGIN.to_string() + &from.name() + SPAN_END;
            title = format!("{} from {}", title, styled_from_string);
            let styled_to_string = SPAN_BEGIN.to_string() + &to.name() + SPAN_END;
            title = format!("{} to {}", title, styled_to_string);

            // order of points is to -> from
            let mut data = ArrowData {
                coordinates: Vec::new(),
                coordinates_hbs: String::new(),
                head_points: String::new(),
                title: title
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
                    let x2 = x1 + arrow_length;
                    let y1 = get_y_axis_pos(*line_number);
                    let y2 = get_y_axis_pos(*line_number);
                    data.coordinates.push((x1 as f64, y1 as f64));
                    data.coordinates.push((x2 as f64, y2 as f64));
    
                    let function_data = FunctionLogoData {
                        x: x2 + 3,
                        y: y2 + 5,
                        hash: from_function.hash.to_owned() as u64,
                        title: SPAN_BEGIN.to_string() + &from_function.name + SPAN_END,
                    };
    
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
                    let styled_fn_name = SPAN_BEGIN.to_string() + &function.name + SPAN_END;
                    let styled_from_name = SPAN_BEGIN.to_string() + &from_variable.name() + SPAN_END;

                    // get position of to_variable
                    let from_timeline = fetch_timeline(from_variable.hash(), visualization_data, resource_owners_layout, external_event.get_id());

                    let title_fn = match external_event {
                        ExternalEvent::PassByStaticReference { .. } => " reads from ",
                        ExternalEvent::PassByMutableReference { .. } => " reads from/writes to ",
                        _ => unreachable!()
                    };
                    
                    let function_dot_data = FunctionDotData {
                        x: from_timeline.x_val,
                        y: get_y_axis_pos(*line_number),
                        title: styled_fn_name + title_fn + &styled_from_name,
                        hash: from_variable.hash().to_owned() as u64,
                    };

                    if from_timeline.is_struct_group {
                        let owner_entry = ensure_owner_entry(output, from_timeline.owner as i64);
                        if from_timeline.is_member {
                            owner_entry.1.dots.push_str(&registry.render("function_dot_template", &function_dot_data).unwrap());
                        } else {
                            owner_entry.0.dots.push_str(&registry.render("function_dot_template", &function_dot_data).unwrap());
                        }
                    }
                    else {
                        output.get_mut(&-1).unwrap().0.dots.push_str(&registry.render("function_dot_template", &function_dot_data).unwrap());
                    }
                }
                (from_variable, ResourceTy::Value(ResourceAccessPoint::Function(to_function)), _e) => { // (Some(variable), Some(function), _)
                    let styled_fn_name = SPAN_BEGIN.to_string() + &to_function.name + SPAN_END;
                    //  ro1 (to_function) <- ro2 (from_variable)
                    let from_timeline = fetch_timeline(from_variable.hash(), visualization_data, resource_owners_layout, external_event.get_id());
                    // Marker-end terminates near the function logo
                    // (no dot to clear) so the gentler 10px pullback
                    // looks better than the dot-clearing 18px.
                    head_offset = 10.0;
                    let x2 = from_timeline.x_val - 5;
                    let x1 = x2 - arrow_length;
                    let y1 = get_y_axis_pos(*line_number);
                    let y2 = get_y_axis_pos(*line_number);
                    data.coordinates.push((x1 as f64, y1 as f64));
                    data.coordinates.push((x2 as f64, y2 as f64));
    
                    let function_data = FunctionLogoData {
                        // adjust Function logo pos
                        x: x1 - 10,  
                        y: y1 + 5,
                        hash: to_function.hash.to_owned() as u64,
                        title: styled_fn_name,
                    };
    
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
                (from_variable, to_variable, e) => {
                    let line_map = fetch_line_map(&visualization_data, e.get_id());
                    let mut arrow_order = line_map.get(line_number).unwrap().iter().position(|x| x == external_event).unwrap() as i64;
                    match e {
                      ExternalEvent::StaticDie { from, to, .. } | ExternalEvent::MutableDie { from, to, .. } => {
                        if from.is_same_underlying(to) {
                          arrow_order = 0;
                        }
                      }
                      _ => {}
                    }

                    let from_timeline = fetch_timeline(from_variable.hash(), visualization_data, resource_owners_layout, e.get_id());
                    let to_timeline = fetch_timeline(to_variable.hash(), visualization_data, resource_owners_layout, e.get_id());

                    let x1 = to_timeline.x_val;
                    let x2 = from_timeline.x_val;
                    let y1 = get_y_axis_pos(*line_number);
                    let y2 = get_y_axis_pos(*line_number);
                    if arrow_order > 0 && x2 <= x1{ // trapezoid
                        let x3 = from_timeline.x_val + 20;
                        let x4 = to_timeline.x_val - 20;
                        let y3 = get_y_axis_pos(*line_number)+LINE_SPACE*arrow_order;
                        let y4 = get_y_axis_pos(*line_number)+LINE_SPACE*arrow_order;
    
                        data.coordinates.push((x1 as f64, y1 as f64));
                        data.coordinates.push((x4 as f64, y4 as f64));
                        data.coordinates.push((x3 as f64, y3 as f64));
                        data.coordinates.push((x2 as f64, y2 as f64));
    
                    } else if arrow_order > 0 && x2 > x1 { // deeper trapezoid
                        let x3 = from_timeline.x_val - 20;
                        let x4 = to_timeline.x_val + 20;
                        let y3 = get_y_axis_pos(*line_number)+LINE_SPACE*arrow_order;
                        let y4 = get_y_axis_pos(*line_number)+LINE_SPACE*arrow_order;
    
                        data.coordinates.push((x1 as f64, y1 as f64));
                        data.coordinates.push((x4 as f64, y4 as f64));
                        data.coordinates.push((x3 as f64, y3 as f64));
                        data.coordinates.push((x2 as f64, y2 as f64));
    
                    } else { // straight line
                        data.coordinates.push((x1 as f64, y1 as f64));
                        data.coordinates.push((x2 as f64, y2 as f64));
                    }
                }
            }

            // draw arrow only if data.x1 is not default value
            if !data.coordinates.is_empty() {
                let last_index = data.coordinates.len() - 1;

                if data.coordinates.len() == 2 {
                    // [0]     [last index]
                    // <-------------------
                    if data.coordinates[0].0 < data.coordinates[last_index].0 {

                        data.coordinates[0].0 += head_offset;
                    }
                    // [last index]     [0]
                    // ------------------->
                    else {
                        data.coordinates[0].0 -= head_offset;
                    }
                } else {

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
                }

                while !data.coordinates.is_empty() {
                    let recent = data.coordinates.pop();
                    data.coordinates_hbs.push_str(&recent.unwrap().0.to_string());
                    data.coordinates_hbs.push_str(&String::from(" "));
                    data.coordinates_hbs.push_str(&recent.unwrap().1.to_string());
                    data.coordinates_hbs.push_str(&String::from(" "));
                }

                // will need to change this later for structs in conditionals
                if resource_owners_layout.contains_key(from.hash()) && resource_owners_layout[from.hash()].is_struct_group {
                    let is_member = resource_owners_layout[from.hash()].is_member;
                    let owner_key = resource_owners_layout[from.hash()].owner as i64;
                    let owner_entry = ensure_owner_entry(output, owner_key);
                    if is_member {
                        owner_entry.1.arrows.push_str(&registry.render("arrow_template", &data).unwrap());
                    } else {
                        owner_entry.0.arrows.push_str(&registry.render("arrow_template", &data).unwrap());
                    }
                }
                else {
                    output.get_mut(&-1).unwrap().0.arrows.push_str(&registry.render("arrow_template", &data).unwrap());
                }
            }
        }
    }
}

// render arrows that support function
fn render_arrows_string_external_events_version(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    registry: &Handlebars
){
    for (line_number, external_event) in &visualization_data.external_events {
        match external_event {
            // events that should be skipped (we don't render arrows for them)
            ExternalEvent::Bind {..} | ExternalEvent::GoOutOfScope {..}
            | ExternalEvent::RefDie {..} => {}
            // InitRefParam falls into render_arrow because owned-param
            // initializations get an L-shaped "ownership from caller"
            // arrow there. Ref params are filtered out inside
            // render_arrow itself rather than here, since the
            // event-vs-arrow split is already a render_arrow concern.

            _ => {

                // render external event arrow
                render_arrow(line_number,
                    external_event,
                    output, visualization_data, resource_owners_layout, registry)
            }
        }
    }
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
    match (state, rap.is_mut()) {
        (State::FullPrivilege{..}, true) => OwnerLine::Solid,
        (State::FullPrivilege{..}, false) => OwnerLine::Hollow,
        (State::PartialPrivilege{..}, _) => OwnerLine::Hollow,
        _ => OwnerLine::Empty,
    }
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
        State::FullPrivilege { s: LineState::Gray } | State::PartialPrivilege { s: LineState::Gray } => {
            data.dasharray = "4 4".to_owned();
        }
        _ => {}
    }
    match (state, style) {
        (State::FullPrivilege{..}, OwnerLine::Solid) | (State::PartialPrivilege{ .. }, OwnerLine::Solid) => {
            data.line_class = String::from("solid");
            // (The historical "binding can be reassigned" suffix
            // mixed a binding property into a state message; the
            // state text alone is enough.)
            registry.render("vertical_line_template", &data).unwrap()
        },
        (State::FullPrivilege{..}, OwnerLine::Hollow) | (State::PartialPrivilege{..}, OwnerLine::Hollow) => {
            let hollow_line_data = data.clone();
            let new_hollow_line_data = compute_hollow_line_data(hollow_line_data, 3.5);
            registry.render("new_hollow_line_template", &new_hollow_line_data).unwrap()
        },
        (State::OutOfScope, _) => "".to_owned(),
        // do nothing when the case is (RevokedPrivilege, false), (OutofScope, _), (ResourceMoved, false)
        (_, _) => "".to_owned()
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
        State::FullPrivilege { s: LineState::Gray } | State::PartialPrivilege { s: LineState::Gray } => {
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
            data.title += "; cannot access data.";
            registry.render("vertical_line_template", &data).unwrap()
        }
        // do nothing when the case is (RevokedPrivilege, _), (OutofScope, _), (ResourceMoved, false)
        _ => "".to_owned(),
    }
}

fn append_line(
    state: &State,
    data: & mut VerticalLineData,
    rap: &ResourceAccessPoint,
    timeline_data: &TimelineColumnData,
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    registry: &Handlebars
) {
    match rap {
        ResourceAccessPoint::Function(_) => {}, // Don't do anything
        ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_) => {
            if timeline_data.is_struct_group { //TODO: not sure if this is correct
                if !output.contains_key(&(timeline_data.owner.to_owned() as i64)) {
                    output.insert(timeline_data.owner.to_owned() as i64, (TimelinePanelData{ labels: String::new(), dots: String::new(), timelines: String::new(), 
                        ref_line: String::new(), arrows: String::new() }, TimelinePanelData{ labels: String::new(), dots: String::new(), 
                            timelines: String::new(), ref_line: String::new(), arrows: String::new() })); 
                }
                if timeline_data.is_member {
                    output.get_mut(&(timeline_data.owner.to_owned() as i64)).unwrap().1.timelines.push_str(&create_owner_line_string(rap, state, data, registry));
                } else {
                    output.get_mut(&(timeline_data.owner.to_owned() as i64)).unwrap().0.timelines.push_str(&create_owner_line_string(rap, state, data, registry));
                }
            }
            else {
                output.get_mut(&-1).unwrap().0.timelines.push_str(&create_owner_line_string(rap, state, data, registry));
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
                output.get_mut(&owner).unwrap().1.timelines.push_str(&create_reference_line_string(rap, state, data, registry));
            } else {
                output.get_mut(&-1).unwrap().0.timelines.push_str(&create_reference_line_string(rap, state, data, registry));
            }
        },
    }
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
//     hover polygon — the gap stays empty rather than getting
//     bridged.

/// Whether the variable still owns / can-borrow a resource at this
/// state. Used by the merge-dot classifier to decide between the
/// drop-dot (mixed branches), all-moved, and unchanged tooltips.
fn state_is_alive(state: &State) -> bool {
    !matches!(state, State::ResourceMoved { .. } | State::OutOfScope)
}

/// Half-width of a hollow strip — matches `compute_hollow_line_data`'s
/// `w = 3.5 / 2`, so branch strips share endpoint coordinates with
/// regular columns at the seams.
const BRANCH_HALF_W: f64 = 1.75;

/// One centerline segment of a branch's strip. The state drives
/// rendering style via `determine_owner_line_styles`.
struct BranchSeg {
    p1: (f64, f64),
    p2: (f64, f64),
    state: State,
    title: String,
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
fn render_branches(
    hash: &u64,
    rap: &ResourceAccessPoint,
    history: &Vec<(usize, Event)>,
    parent_states: &Vec<(usize, usize, State)>,
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    timeline_data: &TimelineColumnData,
) {
    if rap.is_fn() { return; }
    for (_, ev) in history {
        let Event::Branch { branch_history, split_point, merge_point, .. } = ev else { continue };

        let parent_x = timeline_data.x_val as f64;
        let split_y = get_y_axis_pos(*split_point) as f64;
        let merge_y = get_y_axis_pos(*merge_point) as f64;
        let column_top_y = get_y_axis_pos(*split_point + 1) as f64;
        let column_bot_y = get_y_axis_pos(merge_point.saturating_sub(1)) as f64;

        // Parent's state at the split, with let-as-rhs synthesized
        // to FullPrivilege::Full (the variable's about to be
        // brought into scope by acquiring inside a branch). The
        // active arm uses this directly; passive arms Gray-convert
        // it so their leading reads as inactive.
        let parent_pre = parent_states.iter()
            .find(|(s, e, _)| *s <= *split_point && *split_point <= *e)
            .map(|item| item.2.clone())
            .unwrap_or(State::OutOfScope);
        let parent_active = match parent_pre {
            State::OutOfScope => State::FullPrivilege { s: LineState::Full },
            ref s => convert_back(s),
        };

        for (i, branch) in branch_history.iter().enumerate() {
            let branch_x = branch.t_data.x_val as f64;
            // Active arm: i == 0 with non-empty body. Other arms,
            // and any empty body, render passive (Gray-converted
            // leading so it's dashed).
            let is_active = i == 0 && !branch.e_data.is_empty();
            let leading_state = if is_active {
                parent_active.clone()
            } else {
                branch_state_converter(&parent_active)
            };
            let end_state = branch.states.last()
                .map(|s| s.2.clone())
                .unwrap_or(State::OutOfScope);

            // Build segments. Empty-classified states drop out so
            // a Move (anywhere) ends the visible run there.
            let mut segs: Vec<BranchSeg> = Vec::new();
            push_seg_if_renderable(
                &mut segs, rap,
                (parent_x, split_y), (branch_x, column_top_y),
                leading_state, visualization_data,
            );
            for (top, bot, state) in &branch.states {
                if top == bot { continue; }
                push_seg_if_renderable(
                    &mut segs, rap,
                    (branch_x, get_y_axis_pos(*top) as f64),
                    (branch_x, get_y_axis_pos(*bot) as f64),
                    state.clone(), visualization_data,
                );
            }
            push_seg_if_renderable(
                &mut segs, rap,
                (branch_x, column_bot_y), (parent_x, merge_y),
                end_state, visualization_data,
            );

            let runs = split_centerline_runs(&segs);

            let mut branch_svg = String::new();
            for run in &runs {
                branch_svg.push_str(&render_branch_run(rap, *hash, run));
            }

            // Wrap in branch-group so any `:hover` inside the
            // strip glows the whole arm via
            // `.branch-group:hover .tooltip-trigger` in the CSS.
            output.entry(-1).or_insert_with(empty_panel_pair).0.timelines.push_str(&format!(
                "        <g class=\"branch-group\">\n{}        </g>\n",
                branch_svg,
            ));

            render_branches(
                hash, rap, &branch.e_data, &branch.states,
                output, visualization_data, &branch.t_data,
            );
        }
    }
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
) {
    if matches!(determine_owner_line_styles(rap, &state), OwnerLine::Empty | OwnerLine::Dotted) {
        return;
    }
    let title = timeline_segment_title(&state, rap, visualization_data);
    segs.push(BranchSeg { p1, p2, state, title });
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
/// (e.g. diagonal leading meeting vertical body), plus a single
/// transparent perimeter polygon for hover capture.
fn render_branch_run(
    rap: &ResourceAccessPoint,
    hash: u64,
    run: &[&BranchSeg],
) -> String {
    let mut svg = String::new();

    // Classify each segment up front so the bevel pass can
    // consult both neighbors without re-evaluating the
    // classifier.
    let classified: Vec<(OwnerLine, bool, &BranchSeg)> = run.iter().map(|seg| {
        let style = determine_owner_line_styles(rap, &seg.state);
        let dashed = matches!(
            seg.state,
            State::FullPrivilege { s: LineState::Gray }
            | State::PartialPrivilege { s: LineState::Gray }
        );
        (style, dashed, *seg)
    }).collect();

    // Visible lines per segment.
    for (style, dashed, seg) in &classified {
        let dasharray = if *dashed { "4 4" } else { "none" };
        match style {
            OwnerLine::Solid => svg.push_str(&format!(
                "            <line data-hash=\"{}\" class=\"solid tooltip-trigger\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" data-tooltip-text=\"{}\" style=\"stroke-dasharray: {};\"/>\n",
                hash, seg.p1.0, seg.p1.1, seg.p2.0, seg.p2.1, seg.title, dasharray,
            )),
            OwnerLine::Hollow => {
                let perp = perp_unit(seg.p1, seg.p2);
                for sign in [1.0_f64, -1.0] {
                    let ox = perp.0 * sign * BRANCH_HALF_W;
                    let oy = perp.1 * sign * BRANCH_HALF_W;
                    svg.push_str(&format!(
                        "            <line data-hash=\"{}\" class=\"hollow tooltip-trigger\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" data-tooltip-text=\"{}\" style=\"stroke-dasharray: {};\"/>\n",
                        hash, seg.p1.0 + ox, seg.p1.1 + oy, seg.p2.0 + ox, seg.p2.1 + oy, seg.title, dasharray,
                    ));
                }
            }
            OwnerLine::Empty | OwnerLine::Dotted => {} // unreachable: filtered before push
        }
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
    for window in classified.windows(2) {
        let (prev_style, prev_dashed, prev_seg) = &window[0];
        let (curr_style, _, curr_seg) = &window[1];
        if !matches!(prev_style, OwnerLine::Hollow) { continue; }
        if !matches!(curr_style, OwnerLine::Hollow) { continue; }
        let prev_perp = perp_unit(prev_seg.p1, prev_seg.p2);
        let curr_perp = perp_unit(curr_seg.p1, curr_seg.p2);
        let dasharray = if *prev_dashed { "4 4" } else { "none" };
        for sign in [1.0_f64, -1.0] {
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
                svg.push_str(&format!(
                    "            <line data-hash=\"{}\" class=\"hollow tooltip-trigger\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" data-tooltip-text=\"{}\" style=\"stroke-dasharray: {};\"/>\n",
                    hash, prev_off.0, prev_off.1, curr_off.0, curr_off.1, prev_seg.title, dasharray,
                ));
            }
        }
    }

    // Perimeter polygon. Always uses ±BRANCH_HALF_W offsets so
    // pure-solid runs still get a hover surface wider than the
    // visible centerline. `pointer-events:fill` so the transparent
    // fill catches mouse events.
    if let Some(perim) = run_perimeter(run) {
        if perim.len() >= 3 {
            let pts_str = perim.iter()
                .map(|p| format!("{},{}", p.0, p.1))
                .collect::<Vec<_>>()
                .join(" ");
            let title = run.first().map(|s| s.title.clone()).unwrap_or_default();
            svg.push_str(&format!(
                "            <polygon data-hash=\"{}\" class=\"tooltip-trigger\" points=\"{}\" data-tooltip-text=\"{}\" style=\"fill:transparent; stroke:none; pointer-events:fill;\"/>\n",
                hash, pts_str, title,
            ));
        }
    }

    svg
}

/// Closed-loop perimeter for a centerline-contiguous run: left
/// side forward, right side reversed. At corners (adjacent
/// segments with different perpendicular directions) intermediate
/// points get added so the polygon outlines the strip cleanly
/// instead of cutting across the interior.
fn run_perimeter(run: &[&BranchSeg]) -> Option<Vec<(f64, f64)>> {
    if run.is_empty() { return None; }
    let mut left: Vec<(f64, f64)> = Vec::new();
    let mut right: Vec<(f64, f64)> = Vec::new();
    let mut prev_l: Option<(f64, f64)> = None;
    let mut prev_r: Option<(f64, f64)> = None;
    for seg in run {
        let perp = perp_unit(seg.p1, seg.p2);
        let lx = perp.0 * BRANCH_HALF_W;
        let ly = perp.1 * BRANCH_HALF_W;
        let l1 = (seg.p1.0 + lx, seg.p1.1 + ly);
        let l2 = (seg.p2.0 + lx, seg.p2.1 + ly);
        let r1 = (seg.p1.0 - lx, seg.p1.1 - ly);
        let r2 = (seg.p2.0 - lx, seg.p2.1 - ly);
        match prev_l {
            Some(p) if (p.0 - l1.0).abs() < 1e-6 && (p.1 - l1.1).abs() < 1e-6 => {}
            _ => left.push(l1),
        }
        left.push(l2);
        match prev_r {
            Some(p) if (p.0 - r1.0).abs() < 1e-6 && (p.1 - r1.1).abs() < 1e-6 => {}
            _ => right.push(r1),
        }
        right.push(r2);
        prev_l = Some(l2);
        prev_r = Some(r2);
    }
    let mut perim = left;
    perim.extend(right.into_iter().rev());
    Some(perim)
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
    registry: &Handlebars
) {
    if rap.is_fn() { return; } // functions have no timelines
    // Branch columns + convergences render as polylines via
    // `render_branches`; the per-state loop below only emits
    // column lines for the parent timeline outside the branch
    // ranges (clean_states leaves an OOS placeholder over the
    // branch's split..merge span, so those rows naturally render
    // as nothing).
    render_branches(hash, rap, history, states, output, visualization_data, timeline_data);

    for (line_start, line_end, state) in states {
        // println!("{} -> start: {}, end: {}, state: {}", visualization_data.get_name_from_hash(hash).unwrap(), line_start, line_end, state); // DEBUG PURPOSES
        let mut data = VerticalLineData {
                line_class: String::new(),
                hash: *hash,
                x1: timeline_data.x_val as f64,
                y1: get_y_axis_pos(*line_start),
                x2: timeline_data.x_val as f64,
                y2: get_y_axis_pos(*line_end),
                title: timeline_segment_title(state, rap, visualization_data),
                opacity: 1.0,
                dasharray: "none".to_owned(),
        };
        if *line_start != *line_end {
            append_line(state, & mut data, rap, timeline_data, output, registry);
        }
    }
}

// render timelines (states) for RAPs using vertical lines
fn render_timelines(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<u64, TimelineColumnData>,
    registry: &Handlebars
){
    let timelines = &visualization_data.timelines;
    for (hash, timeline) in timelines {
        let rap = &timeline.resource_access_point;
        match rap {
            ResourceAccessPoint::Function(_) => {},
            _ => {
                // println!("hash {}, timeline {:#?}", hash, timeline);
                let t_data = resource_owners_layout.get(hash).unwrap();
                render_timeline(hash, rap, &timeline.history, &timeline.states, output, visualization_data, t_data, registry);
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
    registry: &Handlebars
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
                };
                for (line_start, _line_end, state) in states.iter() {
                    match state { 
                        State::OutOfScope | State::ResourceMoved{ .. } => {
                            if alive {
                                // finish line template
                                data.x2 = data.x1.clone();
                                data.y2 = get_y_axis_pos(*line_start);
                                let dv = get_y_axis_pos(*line_start)-data.y1;
                                data.v = dv - 2*dv/5;
                                data.dy = dv/5;

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
                                data.y1 = get_y_axis_pos(*line_start);

                                let styled = SPAN_BEGIN.to_string()
                                    + &visualization_data.get_name_from_hash(hash).unwrap()
                                    + SPAN_END;
                                data.title = match ro {
                                    ResourceAccessPoint::MutRef(_) =>
                                        format!("{} holds a mutable reference", styled),
                                    _ => format!("{} holds a reference", styled),
                                };
                                data.line_class = String::from("solid");
                                alive = true;
                            }
                        },
                        State::PartialPrivilege{..} => {
                            if !alive {
                                // set known vals
                                data.hash = *hash;
                                data.x1 = resource_owners_layout[hash].x_val;
                                data.y1 = get_y_axis_pos(*line_start);

                                let styled = SPAN_BEGIN.to_string()
                                    + &visualization_data.get_name_from_hash(hash).unwrap()
                                    + SPAN_END;
                                data.title = format!("{} holds an immutable reference", styled);
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
    registry: &Handlebars,
) {
    // Default y matches the legacy "all labels at the top of the
    // SVG" layout (label_y=70, box=50..80). Per-fn struct boxes
    // override below to track their fn's label row.
    const DEFAULT_BOX_Y: i64 = 50;
    for (owner, owner_x, last_x) in structs_info.structs.iter() {
        let owner_hash = *owner as u64;
        let y = match fn_start_lines.get(&owner_hash) {
            // Labels sit on the row directly above the fn signature
            // (see render_labels_string). Box centers on that label
            // row: 20px above (matches the legacy 70 - 20 = 50 offset)
            // and 30px tall.
            Some(&line) => get_y_axis_pos(line) - LINE_SPACE - 20,
            None => DEFAULT_BOX_Y,
        };
        let box_data = BoxData {
            name: owner_hash,
            hash: 0,
            x: owner_x - 20,
            y,
            w: last_x - owner_x + 60,
            h: 30,
            title: String::new(),
        };
        output.get_mut(owner).unwrap().1.arrows.push_str(&registry.render("box_template", &box_data).unwrap());
    }
}

fn get_y_axis_pos(line_number : usize) -> i64 {
    85 - LINE_SPACE + LINE_SPACE * line_number as i64
}
