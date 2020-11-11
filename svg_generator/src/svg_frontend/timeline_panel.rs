extern crate handlebars;

use crate::data::{VisualizationData, Visualizable, ExternalEvent, State, ResourceAccessPoint, Event};
use crate::svg_frontend::line_styles::{RefDataLine, RefValueLine, OwnerLine};
use handlebars::Handlebars;
use std::collections::HashMap;
use serde::Serialize;
use std::cmp;

// set style for code string
static SPAN_BEGIN : &'static str = "&lt;span style=&quot;font-family: 'Source Code Pro', Consolas, 'Ubuntu Mono', Menlo, 'DejaVu Sans Mono', monospace, monospace !important;&quot;&gt;";
static SPAN_END : &'static str = "&lt;/span&gt;";
#[derive(Debug)]
struct TimelineColumnData {
    name: String,
    x_val: i64,
    title: String,
    is_ref: bool,
}

#[derive(Serialize)]
struct TimelinePanelData {
    labels: String,
    dots: String,
    timelines: String,
    ref_line: String,
    arrows: String,
}
#[derive(Serialize)]
struct ResourceAccessPointLabelData {
    x_val: i64,
    hash: String,
    name: String,
    title: String,
}

#[derive(Serialize)]
struct EventDotData {
    hash: u64,
    dot_x: i64,
    dot_y: i64,
    title: String
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
    title: String
}

#[derive(Serialize)]
struct FunctionLogoData {
    hash: u64,
    x: i64,
    y: i64,
    title: String
}

#[derive(Serialize, Clone)]
struct VerticalLineData {
    line_class: String,
    hash: u64,
    x1: f64,
    x2: i64,
    y1: i64,
    y2: i64,
    title: String,
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
}

pub fn render_timeline_panel(visualization_data : &VisualizationData) -> (String, i32) {
    /* Template creation */
    let mut registry = Handlebars::new();
    prepare_registry(&mut registry);

    // hash -> TimelineColumnData
    let (resource_owners_layout, width) = compute_column_layout(visualization_data);

    // render resource owner labels
    let labels_string = render_labels_string(&resource_owners_layout, &registry);
    let dots_string = render_dots_string(visualization_data, &resource_owners_layout, &registry);
    let timelines_string = render_timelines(visualization_data, &resource_owners_layout, &registry);
    let ref_line_string = render_ref_line(visualization_data, &resource_owners_layout, &registry);
    let (arrows_string, fn_string) = render_arrows_string_external_events_version(visualization_data, &resource_owners_layout, &registry);
    let timeline_panel_data = TimelinePanelData {
        labels: labels_string,
        dots: dots_string + &fn_string,
        timelines: timelines_string,
        ref_line: ref_line_string,
        arrows: arrows_string
    };

    (registry.render("timeline_panel_template", &timeline_panel_data).unwrap(), width)
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

    let label_template =
        "        <text x=\"{{x_val}}\" y=\"90\" style=\"text-anchor:middle\" data-hash=\"{{hash}}\" class=\"label tooltip-trigger\" data-tooltip-text=\"{{title}}\">{{name}}</text>\n";
    let dot_template =
        "        <circle cx=\"{{dot_x}}\" cy=\"{{dot_y}}\" r=\"5\" data-hash=\"{{hash}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"/>\n";
    let function_dot_template =    
        "        <use xlink:href=\"#functionDot\" data-hash=\"{{hash}}\" x=\"{{x}}\" y=\"{{y}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"/>\n";
    let function_logo_template =
        "        <text x=\"{{x}}\" y=\"{{y}}\" data-hash=\"{{hash}}\" class=\"functionLogo tooltip-trigger fn-trigger\" data-tooltip-text=\"{{title}}\">f</text>\n";
    let arrow_template =
        // "        <polyline stroke-width=\"5px\" stroke=\"gray\" points=\"{{x2}} {{y2}} {{x1}} {{y1}} \" marker-end=\"url(#arrowHead)\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"/>\n";
        "        <polyline stroke-width=\"5px\" stroke=\"gray\" points=\"{{coordinates_hbs}}\" marker-end=\"url(#arrowHead)\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\" style=\"fill: none;\"/> \n";
    let vertical_line_template =
        "        <line data-hash=\"{{hash}}\" class=\"{{line_class}} tooltip-trigger\" x1=\"{{x1}}\" x2=\"{{x2}}\" y1=\"{{y1}}\" y2=\"{{y2}}\" data-tooltip-text=\"{{title}}\"/>\n";
    let hollow_line_template =
        "        <path data-hash=\"{{hash}}\" class=\"hollow tooltip-trigger\" style=\"fill:transparent;\" d=\"M {{x1}},{{y1}} V {{y2}} h 3.5 V {{y1}} h -3.5\" data-tooltip-text=\"{{title}}\"/>\n";
    let solid_ref_line_template =
        "        <path data-hash=\"{{hash}}\" class=\"{{line_class}} tooltip-trigger\" style=\"fill:transparent; stroke-width: 2px !important;\" d=\"M {{x1}} {{y1}} l {{dx}} {{dy}} v {{v}} l -{{dx}} {{dy}}\" data-tooltip-text=\"{{title}}\"/>\n";
    let hollow_ref_line_template =
        "        <path data-hash=\"{{hash}}\" class=\"tooltip-trigger\" style=\"fill: transparent;\" stroke-width=\"2px\" stroke-dasharray=\"3\" d=\"M {{x1}} {{y1}} l {{dx}} {{dy}} v {{v}} l -{{dx}} {{dy}}\" data-tooltip-text=\"{{title}}\"/>\n";
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
}

// Returns: a hashmap from the hash of the ResourceOwner to its Column information
fn compute_column_layout<'a>(visualization_data: &'a VisualizationData) -> (HashMap<&'a u64, TimelineColumnData>, i32) {
    let mut resource_owners_layout = HashMap::new();
    let mut x = 0;                   // Right-most Column x-offset.
    for (hash, timeline) in visualization_data.timelines.iter() {
        // only put variable in the column layout
        match timeline.resource_access_point {
            ResourceAccessPoint::Function(_) => {
                /* do nothing */
            },
            ResourceAccessPoint::Owner(_) | ResourceAccessPoint::MutRef(_) | ResourceAccessPoint::StaticRef(_) =>
            {
                let name = match visualization_data.get_name_from_hash(hash) {
                    Some(_name) => _name,
                    None => panic!("no matching resource owner for hash {}", hash),
                };
                let mut x_space = cmp::max(70, (&(name.len() as i64)-1)*13);
                x = x + x_space;
                let title = match visualization_data.is_mut(hash) {
                    true => String::from("mutable"),
                    false => String::from("immutable"),
                };
                let mut ref_bool = false;

                if timeline.resource_access_point.is_ref() {
                    let temp_name = name.clone() + "|*" + &name; // used for calculating x_space
                    x = x - x_space; // reset
                    x_space = cmp::max(90, (&(temp_name.len() as i64)-1)*7); // new x_space
                    x = x + x_space; // new x pos
                    ref_bool = true; // hover msg displays only "s" rather than "s|*s"
                }

                let styled_name = SPAN_BEGIN.to_string() + &name + SPAN_END;
                resource_owners_layout.insert(hash, TimelineColumnData
                    { 
                        name: name.clone(), 
                        x_val: x, 
                        title: styled_name.clone() + ", " + &title,
                        is_ref: ref_bool,
                    });
            }
        }
    }
    (resource_owners_layout, (x as i32)+100)
}

fn render_labels_string(
    resource_owners_layout: &HashMap<&u64, TimelineColumnData>,
    registry: &Handlebars
) -> String {
    let mut output = String::new();
    for (hash, column_data) in resource_owners_layout.iter() {
        let mut data = ResourceAccessPointLabelData {
            x_val: column_data.x_val,
            hash: hash.to_string(),
            name: column_data.name.clone(),
            title: column_data.title.clone(),
        };

        if column_data.is_ref {
            let new_name = column_data.name.to_owned() + "<tspan stroke=\"none\" data-tooltip-text=\""+&column_data.title+"\">|</tspan>*" + &column_data.name;
            data.name = new_name;
        }
        output.push_str(&registry.render("label_template", &data).unwrap());
    }
    output
}

fn render_dots_string(
    visualization_data: &VisualizationData,
    resource_owners_layout: &HashMap<&u64, TimelineColumnData>,
    registry: &Handlebars
) -> String {

    let timelines = &visualization_data.timelines;
    let mut output = String::new();
    for (hash, timeline) in timelines {
        // render just the name of Owners and References
        match timeline.resource_access_point {
            ResourceAccessPoint::Function(_) => {
                // nothing to be done
            },
            ResourceAccessPoint::Owner(_) | ResourceAccessPoint::MutRef(_) | ResourceAccessPoint::StaticRef(_) =>
            {
                let mut resource_hold = false;
                for (line_number, event) in timeline.history.iter() {
                    //matching the event
                    match event {
                        Event::Acquire{..} => {
                            resource_hold = true;
                        },
                        Event::Copy{..} => {
                            resource_hold = true;
                        },
                        Event::Move{..} => {
                            resource_hold = false;
                        },
                        _ => {
                            //do nothing
                        }
                    }
                    let mut data = EventDotData {
                        hash: *hash as u64,
                        dot_x: resource_owners_layout[hash].x_val,
                        dot_y: get_y_axis_pos(line_number),
                        title: "Unknown Resource Owner Value".to_owned()        // default value if the 
                                                                                // print_message_with_name() fails
                    };
                    if let Some(name) = visualization_data.get_name_from_hash(hash) {
                        match event {
                            Event::OwnerGoOutOfScope => {
                                if !resource_hold {
                                    let resource_info: &str = " resource not dropped";
                                    data.title = event.print_message_with_name(&name);
                                    data.title.push_str(resource_info);
                                } else {
                                    let resource_info: &str = " resource dropped";
                                    data.title = event.print_message_with_name(&name);
                                    data.title.push_str(resource_info);
                                }
                            },
                            _ => {
                                data.title = event.print_message_with_name(&name);
                            }
                        }
                    }
                    output.push_str(&registry.render("dot_template", &data).unwrap());
                }
            },
        }
    }
    output
}

// render arrows that support function
fn render_arrows_string_external_events_version(
    visualization_data: &VisualizationData,
    resource_owners_layout: &HashMap<&u64, TimelineColumnData>,
    registry: &Handlebars
) -> (String, String) {

    let (mut output, mut fn_timeline) = (String::new(), String::new());
    for (line_number, external_event) in &visualization_data.external_events {
        let mut title = String::from("");
        let (from, to) = match external_event {
            ExternalEvent::Duplicate{ from: from_ro, to: to_ro } => {
                title = String::from("Copy");
                (from_ro, to_ro)
            },
            ExternalEvent::Move{ from: from_ro, to: to_ro } => {
                title = String::from("Move");
                (from_ro, to_ro)
            },
            ExternalEvent::StaticBorrow{ from: from_ro, to: to_ro } => {
                title = String::from("Immutable borrow");
                (from_ro, to_ro)
            },
            ExternalEvent::StaticReturn{ from: from_ro, to: to_ro } => {
                title = String::from("Return immutably borrowed resource");
                (from_ro, to_ro)
            },
            ExternalEvent::MutableBorrow{ from: from_ro, to: to_ro } => {
                title = String::from("Mutable borrow");
                (from_ro, to_ro)
            },
            ExternalEvent::MutableReturn{ from: from_ro, to: to_ro } => {
                title = String::from("Return mutably borrowed resource");
                (from_ro, to_ro)
            },
            ExternalEvent::PassByMutableReference{ from: from_ro, to: to_ro } => {
                title = String::from("Pass by mutable reference");
                (from_ro, to_ro)
            }
            ExternalEvent::PassByStaticReference{ from: from_ro, to: to_ro } => {
                title = String::from("Pass by immutable reference");
                (from_ro, to_ro)
            }
            _ => (&None, &None),
        };
        // complete title
        if let Some(some_from) = from {
            let from_string = match some_from {
                ResourceAccessPoint::Owner(owner) => owner.name.to_owned(),
                ResourceAccessPoint::MutRef(mutref) => mutref.name.to_owned(),
                ResourceAccessPoint::StaticRef(statref) => statref.name.to_owned(),
                ResourceAccessPoint::Function(func) => func.name.to_owned(),
            };
            let styled_from_string = SPAN_BEGIN.to_string() + &from_string + SPAN_END;
            title = format!("{} from {}", title, styled_from_string);
        };
        if let Some(some_to) = to {
            let to_string = match some_to {
                ResourceAccessPoint::Owner(owner) => owner.name.to_owned(),
                ResourceAccessPoint::MutRef(mutref) => mutref.name.to_owned(),
                ResourceAccessPoint::StaticRef(statref) => statref.name.to_owned(),
                ResourceAccessPoint::Function(func) => func.name.to_owned(),
            };
            let styled_to_string = SPAN_BEGIN.to_string() + &to_string + SPAN_END;
            title = format!("{} to {}", title, styled_to_string);
        };
        // Old structure for data
        // calc arrow data and function logo data
        // x1: 0,
        // y1: get_y_axis_pos(line_number),
        // x2: 0,
        // y2: get_y_axis_pos(line_number),
        // Q: can I replace mut with simply Vec::new()?

        // order of points is to -> from
        let mut data = ArrowData {
            coordinates: Vec::new(),
            coordinates_hbs: String::new(),
            title: title
        };

        let arrow_length = 20;
        // render title
        match (from, to, external_event) {
            (Some(ResourceAccessPoint::Function(_)), Some(ResourceAccessPoint::Function(_)), _) => {
                // do nothing for case: from = function
                // it is easier to exclude this case than list all possible cases for when ResourceAccessPoint is a variable
            },
            (Some(ResourceAccessPoint::Function(from_function)), Some(to_variable), _) => {  // (Some(function), Some(variable), _)
                // ro1 (to_variable) <- ro2 (from_function)
                // arrow go from (x2, y2) -> (x1, y1)
                let x1 = resource_owners_layout[to_variable.hash()].x_val + 3; // adjust arrow head pos
                let x2 = x1 + arrow_length;
                let y1 = get_y_axis_pos(line_number);
                let y2 = get_y_axis_pos(line_number);
                data.coordinates.push((x1 as f64, y1 as f64));
                data.coordinates.push((x2 as f64, y2 as f64));
                let function_data = FunctionLogoData {
                    x: x2 + 3,
                    y: y2 + 5,
                    hash: from_function.hash.to_owned() as u64,
                    title: SPAN_BEGIN.to_string() + &from_function.name + SPAN_END,
                };
                fn_timeline.push_str(&registry.render("function_logo_template", &function_data).unwrap());
            },
            (Some(from_variable), Some(ResourceAccessPoint::Function(function)), 
             ExternalEvent::PassByStaticReference{..}) => { // (Some(variable), Some(function), PassByStatRef)
                // get variable's position
                let styled_fn_name = SPAN_BEGIN.to_string() + &function.name + SPAN_END;
                let styled_from_name = SPAN_BEGIN.to_string() + from_variable.name() + SPAN_END;
                
                let function_dot_data = FunctionDotData {
                    x: resource_owners_layout[from_variable.hash()].x_val,
                    y: get_y_axis_pos(line_number),
                    title: styled_fn_name + " reads from " + &styled_from_name,
                    hash: from_variable.hash().to_owned() as u64,
                };
                fn_timeline.push_str(&registry.render("function_dot_template", &function_dot_data).unwrap());
            },
            (Some(from_variable), Some(ResourceAccessPoint::Function(function)), 
             ExternalEvent::PassByMutableReference{..}) => {  // (Some(variable), Some(function), PassByMutRef)
                // get variable's position
                let styled_fn_name = SPAN_BEGIN.to_string() + &function.name + SPAN_END;
                let styled_from_name = SPAN_BEGIN.to_string() + from_variable.name() + SPAN_END;

                let function_dot_data = FunctionDotData {
                x: resource_owners_layout[from_variable.hash()].x_val,
                y: get_y_axis_pos(line_number),
                title: styled_fn_name + " reads from/writes to " + &styled_from_name,
                hash: from_variable.hash().to_owned() as u64,
                };
                fn_timeline.push_str(&registry.render("function_dot_template", &function_dot_data).unwrap());
            },
            (Some(from_variable), Some(ResourceAccessPoint::Function(to_function)), _) => { // (Some(variable), Some(function), _)
                let styled_fn_name = SPAN_BEGIN.to_string() + &to_function.name + SPAN_END;
                //  ro1 (to_function) <- ro2 (from_variable)
                let x2 = resource_owners_layout[from_variable.hash()].x_val - 5;
                let x1 = x2 - arrow_length;
                let y1 = get_y_axis_pos(line_number);
                let y2 = get_y_axis_pos(line_number);
                data.coordinates.push((x1 as f64, y1 as f64));
                data.coordinates.push((x2 as f64, y2 as f64));
                let function_data = FunctionLogoData {
                    // adjust Function logo pos
                    x: x1 - 10,  
                    y: y1 + 5,
                    hash: to_function.hash.to_owned() as u64,
                    title: styled_fn_name,
                };
                fn_timeline.push_str(&registry.render("function_logo_template", &function_data).unwrap());
            },
            (Some(from_variable), Some(to_variable), _) => {
                // Only place to address overlay arrows
                // Q : can I do * to dereference?
                // Q : why we have reference stored in variables and vectors AKA containers?
                let arrow_order = visualization_data.event_line_map.get(line_number).unwrap().iter().position(|x| x == external_event).unwrap() as i64;

                let x1 = resource_owners_layout[to_variable.hash()].x_val;
                let x2 = resource_owners_layout[from_variable.hash()].x_val;
                let y1 = get_y_axis_pos(line_number);
                let y2 = get_y_axis_pos(line_number);
                // if the arrow is pointing from left to right
                if arrow_order > 0 && x2 <= x1{
                    let x3 = resource_owners_layout[from_variable.hash()].x_val + 20;
                    let x4 = resource_owners_layout[to_variable.hash()].x_val - 20;
                    let y3 = get_y_axis_pos(line_number)+20*arrow_order;
                    let y4 = get_y_axis_pos(line_number)+20*arrow_order;

                    data.coordinates.push((x1 as f64, y1 as f64));
                    data.coordinates.push((x4 as f64, y4 as f64));
                    data.coordinates.push((x3 as f64, y3 as f64));
                    data.coordinates.push((x2 as f64, y2 as f64));

                // if the arrow is pointing from right to left
                } else if arrow_order > 0 && x2 > x1 {
                    let x3 = resource_owners_layout[from_variable.hash()].x_val - 20;
                    let x4 = resource_owners_layout[to_variable.hash()].x_val + 20;
                    let y3 = get_y_axis_pos(line_number)+20*arrow_order;
                    let y4 = get_y_axis_pos(line_number)+20*arrow_order;

                    data.coordinates.push((x1 as f64, y1 as f64));
                    data.coordinates.push((x4 as f64, y4 as f64));
                    data.coordinates.push((x3 as f64, y3 as f64));
                    data.coordinates.push((x2 as f64, y2 as f64));

                } else {
                    data.coordinates.push((x1 as f64, y1 as f64));
                    data.coordinates.push((x2 as f64, y2 as f64));
                }
            },
            _ => (), // don't support other cases for now
        }
        // draw arrow only if data.x1 is not default value
        if !data.coordinates.is_empty() {
            // adjust arrow head pos
            // Q : better way to do this?
            // if arrow is straight
            let last_index = data.coordinates.len() - 1;

            if data.coordinates.len() == 2 {
                // [0]     [last index]
                // <-------------------
                if data.coordinates[0].0 < data.coordinates[last_index].0 {

                    data.coordinates[0].0 += 10 as f64;
                }
                // [last index]     [0]
                // ------------------->
                else {
                    data.coordinates[0].0 -= 10 as f64;
                }
            } else {

                if data.coordinates[0].0 < data.coordinates[last_index].0 {
                    let hypotenuse = (((data.coordinates[1].0 - data.coordinates[0].0) as f64).powi(2) + ((data.coordinates[1].1 - data.coordinates[0].1) as f64).powi(2)).sqrt();
                    let cos_ratio = ((data.coordinates[1].0 - data.coordinates[0].0) as f64) / hypotenuse;
                    let sin_ratio = ((data.coordinates[1].1 - data.coordinates[0].1) as f64) / hypotenuse;
                    data.coordinates[0].0 += cos_ratio*10 as f64;
                    data.coordinates[0].1 += sin_ratio*10 as f64;
                }
                else {
                    let hypotenuse = (((data.coordinates[0].0 - data.coordinates[1].0) as f64).powi(2) + ((data.coordinates[1].1 - data.coordinates[0].1) as f64).powi(2)).sqrt();
                    let cos_ratio = ((data.coordinates[0].0 - data.coordinates[1].0) as f64) / hypotenuse;
                    let sin_ratio = ((data.coordinates[1].1 - data.coordinates[0].1) as f64) / hypotenuse;
                    data.coordinates[0].0 -= cos_ratio*10 as f64;
                    data.coordinates[0].1 += sin_ratio*10 as f64;
                }
            }

            while !data.coordinates.is_empty() {
                let recent = data.coordinates.pop();
                data.coordinates_hbs.push_str(&recent.unwrap().0.to_string());
                data.coordinates_hbs.push_str(&String::from(" "));
                data.coordinates_hbs.push_str(&recent.unwrap().1.to_string());
                data.coordinates_hbs.push_str(&String::from(" "));
            }
            output.push_str(&registry.render("arrow_template", &data).unwrap()); 
        }
    }
    (output, fn_timeline)
}


fn determine_owner_line_styles(
    rap: &ResourceAccessPoint,
    state: &State
) -> OwnerLine {
    match (state, rap.is_mut()) {
        (State::FullPrivilege, true) => OwnerLine::Solid,
        (State::FullPrivilege, false) => OwnerLine::Hollow,
        (State::PartialPrivilege{..}, _) => OwnerLine::Hollow, // let (mut) a = 5;
        _ => OwnerLine::Empty,              // Otherwise its empty
    }
}


fn determine_stat_ref_line_styles(
    rap: &ResourceAccessPoint,
    state: &State
) -> (RefDataLine, RefValueLine) {
    match (state, rap.is_mut()) {
        (State::FullPrivilege, _) => (
            RefDataLine::Solid,
            if rap.is_mut() {RefValueLine::Reassignable} else {RefValueLine::NotReassignable},
        ),
        (State::PartialPrivilege{..}, false) => (
            RefDataLine::Hollow,
            RefValueLine::NotReassignable,
        ),
        (State::PartialPrivilege{..}, true) => (
            RefDataLine::Hollow,
            RefValueLine::Reassignable,     // potentially wrong. Not taking second level 
                                            // borrowing into account
        ),
        _ => (                              // TODO: not finished
            RefDataLine::Hollow,
            RefValueLine::NotReassignable,
        )
    }
}


fn determine_mut_ref_line_styles(
    rap: &ResourceAccessPoint,
    state: &State
) -> (RefDataLine, RefValueLine) {
    match (state, rap.is_mut()) {
        (State::FullPrivilege, false) => (
            RefDataLine::Solid,
            RefValueLine::NotReassignable,
        ),
        (State::FullPrivilege, true) => (
            RefDataLine::Solid,
            RefValueLine::Reassignable,
        ),
        _ => ( // TODO: not finished
            RefDataLine::Hollow,
            RefValueLine::NotReassignable,
        )
    }
}

// generate a Owner Line string from the RAP and its State
fn create_owner_line_string(
    rap: &ResourceAccessPoint,
    state: &State,
    data: &mut VerticalLineData,
    registry: &Handlebars,
) -> String {
    let style = determine_owner_line_styles(rap, state);
    match (state, style) {
        (State::FullPrivilege, OwnerLine::Solid) | (State::PartialPrivilege { .. }, OwnerLine::Solid) => {
            data.line_class = String::from("solid");
            data.title += ". The binding can be reassigned.";
            registry.render("vertical_line_template", &data).unwrap()
        },
        (State::FullPrivilege, OwnerLine::Hollow) => {
            let mut hollow_line_data = data.clone();
            hollow_line_data.title += ". The binding cannot be reassigned.";
            hollow_line_data.x1 -= 1.8; // center middle of path to center of event dot
            
            let output = registry.render("hollow_line_template", &hollow_line_data).unwrap();
            output
        },
        (State::FullPrivilege, OwnerLine::Dotted) => {
            // cannot read nor write the data from this RAP temporarily (borrowed away by a mut reference)
            "".to_owned()
        }
        (State::PartialPrivilege{..}, _) => {
            data.line_class = String::from("solid");
            registry.render("vertical_line_template", &data).unwrap()
        },
        (State::OutOfScope, _) => (
            "".to_owned()
        ),
        // do nothing when the case is (RevokedPrivilege, false), (OutofScope, _), (ResourceMoved, false)
        (_, _) => (
            "".to_owned()
        ),
    }
}

// generate Reference Line(s) string from the RAP and its State
fn create_reference_line_string(
    rap: &ResourceAccessPoint,
    state: &State,
    data: &mut VerticalLineData,
    registry: &Handlebars,
) -> String {
    match (state, rap.is_mut()) {
        (State::FullPrivilege, true) => {
            data.line_class = String::from("solid");
            if rap.is_ref() {
                data.title += "; can read and write data; can point to another piece of data";
            } else {
                data.title += "; can read and write data";
            }
            registry.render("vertical_line_template", &data).unwrap()
        },
        (State::FullPrivilege, false) => {
            if rap.is_ref() {
                data.title += "; can read and write data; cannot point to another piece of data";
            } else {
                data.title += "; can only read data";
            }
            
            let mut hollow_line_data = data.clone();
            hollow_line_data.x1 -= 1.8; // center middle of path to center of event dot
            
            let output = registry.render("hollow_line_template", &hollow_line_data).unwrap();
            output
        },
        (State::PartialPrivilege{ .. }, _) => {
            data.line_class = String::from("solid");
            data.title += "; can only read data";
            
            let mut hollow_line_data = data.clone();
            hollow_line_data.x1 -= 1.8; // center middle of path to center of event dot
            hollow_line_data.title = data.title.to_owned();
            
            let output = registry.render("hollow_line_template", &hollow_line_data).unwrap();
            output

        },
        (State::ResourceMoved{ .. }, true) => {
            data.line_class = String::from("extend");
            data.title += "; cannot access data";
            registry.render("vertical_line_template", &data).unwrap()
        }
        // do nothing when the case is (RevokedPrivilege, false), (OutofScope, _), (ResourceMoved, false)
        _ => (
            "".to_owned()
        ),
    }
}

// render timelines (states) for RAPs using vertical lines
fn render_timelines(
    visualization_data: &VisualizationData,
    resource_owners_layout: &HashMap<&u64, TimelineColumnData>,
    registry: &Handlebars
) -> String {
    let mut output = String::new();
    let timelines = &visualization_data.timelines;
    for (hash, timeline) in timelines {
        let rap = &timeline.resource_access_point;
        let rap_states = visualization_data.get_states(hash);
        for (line_start, line_end, state) in rap_states.iter() {
            // println!("{} -> start: {}, end: {}, state: {}", visualization_data.get_name_from_hash(hash).unwrap(), line_start, line_end, state); // DEBUG PURPOSES
            let data = match rap {
                ResourceAccessPoint::Function(_) => {
                    None
                },
                _ => {
                    Some (
                        VerticalLineData {
                            line_class: String::new(),
                            hash: *hash,
                            x1: resource_owners_layout[hash].x_val as f64,
                            y1: get_y_axis_pos(line_start),
                            x2: resource_owners_layout[hash].x_val,
                            y2: get_y_axis_pos(line_end),
                            title: state.print_message_with_name(rap.name())
                        }
                    )
                }
            };
            match rap {
                ResourceAccessPoint::Function(_) => {
                    // Don't do anything
                },
                ResourceAccessPoint::Owner(_) => {
                    output.push_str(
                        // the unwrap is safe as long as data is built in this branch. 
                        &create_owner_line_string(rap, state, &mut data.unwrap(), registry)
                    );
                },
                ResourceAccessPoint::StaticRef(_) | ResourceAccessPoint::MutRef(_) => {
                    output.push_str(
                        // the unwrap is safe as long as data is built in this branch. 
                        &create_reference_line_string(rap, state, &mut data.unwrap(), registry)
                    );
                },
            }
        }
    }
    output
}

// vertical lines indicating whether a reference can mutate its resource(deref as many times)
// (iff it's a MutRef && it has FullPrivilege)
fn render_ref_line(
    visualization_data: &VisualizationData,
    resource_owners_layout: &HashMap<&u64, TimelineColumnData>,
    registry: &Handlebars
) -> String {
    let timelines = &visualization_data.timelines;

    let mut output = String::new();
    for (hash, timeline) in timelines{
        match timeline.resource_access_point {
            ResourceAccessPoint::Function(_) => {
                /* do nothing */
            },
            ResourceAccessPoint::Owner(_) | ResourceAccessPoint::MutRef(_) | ResourceAccessPoint::StaticRef(_) =>
            {
                let ro = timeline.resource_access_point.to_owned();
                // verticle state lines
                let states = visualization_data.get_states(hash);

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
                    match state { // consider removing .clone()
                        State::OutOfScope | State::ResourceMoved{ .. } => {
                            if alive {
                                // finish line template
                                data.x2 = data.x1.clone();
                                data.y2 = get_y_axis_pos(line_start);
                                let dv = get_y_axis_pos(line_start)-data.y1;
                                data.v = dv - 2*dv/5;
                                data.dy = dv/5;

                                match ro {
                                    ResourceAccessPoint::MutRef(_) => {
                                        output.push_str(&registry.render("solid_ref_line_template", &data).unwrap());
                                    },
                                    ResourceAccessPoint::StaticRef(_) => {
                                        output.push_str(&registry.render("hollow_ref_line_template", &data).unwrap());
                                    },
                                    _ => (),
                                }

                                alive = false;
                            }
                        },
                        State::FullPrivilege => {
                            if !alive {
                                // set known vals
                                data.hash = *hash;
                                data.x1 = resource_owners_layout[hash].x_val;
                                data.y1 = get_y_axis_pos(line_start);

                                data.title = String::from(
                                    format!("can mutate *{}", visualization_data.get_name_from_hash(hash).unwrap())
                                );
                                data.line_class = String::from("solid");
                                alive = true;
                            }
                        },
                        State::PartialPrivilege{..} => {
                            if !alive {
                                // set known vals
                                data.hash = *hash;
                                data.x1 = resource_owners_layout[hash].x_val;
                                data.y1 = get_y_axis_pos(line_start);

                                data.title = String::from(
                                    format!("cannot mutate *{}",visualization_data.get_name_from_hash(hash).unwrap())
                                );
                                data.line_class = String::from("solid");
                                alive = true;
                            }
                        },
                        _ => (),
                    }
                }
            }
        }
    }
    output
}

fn get_y_axis_pos(line_number : &usize) -> i64 {
    (65 + 20 * line_number) as i64
}
