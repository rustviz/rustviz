extern crate handlebars;

use crate::data::{StructsInfo, VisualizationData, Visualizable, ExternalEvent, State, ResourceAccessPoint, Event, LINE_SPACE};
use crate::svg_frontend::line_styles::{RefDataLine, RefValueLine, OwnerLine};
use handlebars::Handlebars;
use std::collections::BTreeMap;
use serde::Serialize;
use std::cmp;

// set style for code string
static SPAN_BEGIN : &'static str = "&lt;span style=&quot;font-family: 'Source Code Pro', Consolas, 'Ubuntu Mono', Menlo, 'DejaVu Sans Mono', monospace, monospace !important;&quot;&gt;";
static SPAN_END : &'static str = "&lt;/span&gt;";
/* name: The name of the column.
   x_val: The x-coordinate of the column.
   title: The title of the column.
   is_ref: A boolean indicating if the column represents a reference.
   is_struct_group: A boolean indicating if the column represents a group of structs.
   is_member: A boolean indicating if the column represents a member of a struct.
   owner: The ID of the owner of the column.
*/
#[derive(Debug)]
struct TimelineColumnData {
    name: String,
    x_val: i64,
    title: String,
    is_ref: bool,
    is_struct_group: bool,
    is_member: bool,
    owner: u64
}
/* labels: A string containing the HTML code for rendering the labels.
   dots: A string containing the HTML code for rendering the dots representing events.
   timelines: A string containing the HTML code for rendering the timelines.
   ref_line: A string containing the HTML code for rendering the reference lines.
   arrows: A string containing the HTML code for rendering the arrows representing
*/
#[derive(Serialize)]
struct TimelinePanelData {
    labels: String,
    dots: String,
    timelines: String,
    ref_line: String,
    arrows: String
}
/* x_val: The x-coordinate of the label.
   hash: The hash value of the resource.
   name: The name of the resource.
   title: The title of the resource.
*/
#[derive(Serialize)]
struct ResourceAccessPointLabelData {
    x_val: i64,
    hash: String,
    name: String,
    title: String
}
/* hash: The hash value of the event.
   dot_x: The x-coordinate of the dot.
   dot_y: The y-coordinate of the dot.
   title: The title of the event.
*/
#[derive(Serialize)]
struct EventDotData {
    hash: u64,
    dot_x: i64,
    dot_y: i64,
    title: String,
}
/* hash: The hash value of the function.
   x: The x-coordinate of the dot.
   y: The y-coordinate of the dot.
   title: The title of the function.
*/
#[derive(Serialize)]
struct FunctionDotData {
    hash: u64,
    x: i64,
    y: i64,
    title: String
}
/* coordinates: A vector of (x, y) coordinates defining the path of the arrow.
   coordinates_hbs: A string containing the HTML code for rendering the arrow coordinates.
   title: The title of the arrow.
*/
#[derive(Serialize)]
struct ArrowData {
    coordinates: Vec<(f64, f64)>,
    coordinates_hbs: String,
    title: String
}
/* 
   hash: The hash value of the function.
   x: The x-coordinate of the logo.
   y: The y-coordinate of the logo.
   title: The title of the function.
*/
#[derive(Serialize)]
struct FunctionLogoData {
    hash: u64,
    x: i64,
    y: i64,
    title: String
}
/* name: The name of the box.
   hash: The hash value of the box.
   x: The x-coordinate of the box.
   y: The y-coordinate of the box.
   w: The width of the box.
   h: The height of the box.
*/
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

/* struct_name: The name of the struct.
   struct_members: A string representing the struct members.
*/
#[derive(Serialize)]
struct StructTimelinePanelData {
    struct_name: String,
    struct_members: String
}

/* line_class: The class of the line (probably for CSS styling).
   hash: The hash value associated with the line.
   x1, x2, y1, y2: The start and end coordinates of the line.
   title: The title of the line.
*/
#[derive(Serialize, Clone)]
struct VerticalLineData {
    line_class: String,
    hash: u64,
    x1: f64,
    x2: i64,
    y1: i64,
    y2: i64,
    title: String
}

/* line_class: The class of the line (probably for CSS styling).
   hash: The hash value associated with the line.
   x1, x2, y1, y2: The start and end coordinates of the line.
   dx, dy: The differences in x and y coordinates between the start and end of the line.
   v: A variable for additional data, perhaps for storing the magnitude of the reference.
   title: The title of the line.
   The struct is marked with #[derive(Serialize)], indicating that 
   instances of OutputStringData can be converted (serialized) to a format such as JSON or TOML.
*/
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

/* struct_name: This field stores the name of the struct. This would be the type of the struct in the source code.
   struct_instance: This field is likely to store the instance name of the struct. This would be the variable name in the source code.
   struct_members: This field is a string representation of the struct members. It likely contains the members (fields and their associated values) of the struct.
*/
#[derive(Serialize)]
struct OutputStringData {
    struct_name: String,
    struct_instance: String,
    struct_members: String
}

pub fn render_timeline_panel(visualization_data : &VisualizationData) -> (String, i32) {
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
    render_timelines(&mut output, visualization_data, &resource_owners_layout, &registry);
    render_labels_string(&mut output, &resource_owners_layout, &registry);
    render_dots_string(&mut output, visualization_data, &resource_owners_layout, &registry);
    render_ref_line(&mut output, visualization_data, &resource_owners_layout, &registry);
    render_arrows_string_external_events_version(&mut output, visualization_data, &resource_owners_layout, &registry);
    render_struct_box(&mut output, &structs_info, &registry);

    let mut output_string : String = String::new();
    for (hash, (timelinepanel, member_timelinepanel)) in output{
        let struct_name;
        if hash == -1 {
            struct_name = String::from("non-struct");
        } else {
            struct_name = match visualization_data.get_name_from_hash(&(hash as u64)) {
                Some(_name) => _name,
                None => panic!("no matching resource owner for hash {}", hash),
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
        "        <text x=\"{{x_val}}\" y=\"70\" style=\"text-anchor:middle\" data-hash=\"{{hash}}\" class=\"label tooltip-trigger\" data-tooltip-text=\"{{title}}\">{{name}}</text>\n";
    let dot_template =
        "        <circle cx=\"{{dot_x}}\" cy=\"{{dot_y}}\" r=\"5\" data-hash=\"{{hash}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"/>\n";
    let function_dot_template =    
        "        <use xlink:href=\"#functionDot\" data-hash=\"{{hash}}\" x=\"{{x}}\" y=\"{{y}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\"/>\n";
    let function_logo_template =
        "        <text x=\"{{x}}\" y=\"{{y}}\" data-hash=\"{{hash}}\" class=\"functionLogo tooltip-trigger fn-trigger\" data-tooltip-text=\"{{title}}\">f</text>\n";
    let arrow_template =
        "        <polyline stroke-width=\"5px\" stroke=\"gray\" points=\"{{coordinates_hbs}}\" marker-end=\"url(#arrowHead)\" class=\"tooltip-trigger\" data-tooltip-text=\"{{title}}\" style=\"fill: none;\"/> \n";
    let vertical_line_template =
        "        <line data-hash=\"{{hash}}\" class=\"{{line_class}} tooltip-trigger\" x1=\"{{x1}}\" x2=\"{{x2}}\" y1=\"{{y1}}\" y2=\"{{y2}}\" data-tooltip-text=\"{{title}}\"/>\n";
    let hollow_line_template =
        "        <path data-hash=\"{{hash}}\" class=\"hollow tooltip-trigger\" style=\"fill:transparent;\" d=\"M {{x1}},{{y1}} V {{y2}} h 3.5 V {{y1}} h -3.5\" data-tooltip-text=\"{{title}}\"/>\n";
    let solid_ref_line_template =
        "        <path data-hash=\"{{hash}}\" class=\"mutref {{line_class}} tooltip-trigger\" style=\"fill:transparent; stroke-width: 2px !important;\" d=\"M {{x1}} {{y1}} l {{dx}} {{dy}} v {{v}} l -{{dx}} {{dy}}\" data-tooltip-text=\"{{title}}\"/>\n";
    let hollow_ref_line_template =
        "        <path data-hash=\"{{hash}}\" class=\"staticref tooltip-trigger\" style=\"fill: transparent;\" stroke-width=\"2px\" stroke-dasharray=\"3\" d=\"M {{x1}} {{y1}} l {{dx}} {{dy}} v {{v}} l -{{dx}} {{dy}}\" data-tooltip-text=\"{{title}}\"/>\n";
    let box_template =
        "        <rect id=\"{{name}}\" x=\"{{x}}\" y=\"{{y}}\" rx=\"20\" ry=\"20\" width=\"{{w}}\" height=\"{{h}}\" style=\"fill:white;stroke:black;stroke-width:3;opacity:0.1\" pointer-events=\"none\" />\n";

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

// Returns: a binary tree map from the hash of the ResourceOwner to its Column information
fn compute_column_layout<'a>(
    visualization_data: &'a VisualizationData,
    structs_info: &'a mut StructsInfo,
) -> (BTreeMap<&'a u64, TimelineColumnData>, i32) {
    let mut resource_owners_layout = BTreeMap::new();
    let mut x = 0; // Right-most Column x-offset.
    let mut owner = -1;
    let mut owner_x = 0;
    let mut last_x = 0;
    for (hash, timeline) in visualization_data.timelines.iter() {
        // only put variable in the column layout
        match timeline.resource_access_point {
            ResourceAccessPoint::Function(_) => {
                /* do nothing */
            },
            ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_) | ResourceAccessPoint::MutRef(_) | ResourceAccessPoint::StaticRef(_) =>
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

                // render reference label
                if timeline.resource_access_point.is_ref() {
                    let temp_name = name.clone() + "|*" + &name; // used for calculating x_space
                    x = x - x_space; // reset
                    x_space = cmp::max(90, (&(temp_name.len() as i64)-1)*7); // new x_space
                    x = x + x_space; // new x pos
                    ref_bool = true; // hover msg displays only "s" rather than "s|*s"
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

                resource_owners_layout.insert(hash, TimelineColumnData
                    { 
                        name: name.clone(), 
                        x_val: x, 
                        title: styled_name.clone() + ", " + &title,
                        is_ref: ref_bool,
                        is_struct_group: timeline.resource_access_point.is_struct_group(),
                        is_member: timeline.resource_access_point.is_member(),
                        owner: timeline.resource_access_point.get_owner(),
                    });
            }
        }
    }
    (resource_owners_layout, (x as i32)+100)
}

fn render_labels_string(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    resource_owners_layout: &BTreeMap<&u64, TimelineColumnData>,
    registry: &Handlebars
) {
    for (hash, column_data) in resource_owners_layout.iter() {
        let mut data = ResourceAccessPointLabelData {
            x_val: column_data.x_val,
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
            if column_data.is_member {
                output.get_mut(&(column_data.owner.to_owned() as i64)).unwrap().1.labels.push_str(&registry.render("label_template", &data).unwrap());
            } else {
                output.get_mut(&(column_data.owner.to_owned() as i64)).unwrap().0.labels.push_str(&registry.render("label_template", &data).unwrap());
            }
        }
        else {
            output.get_mut(&-1).unwrap().0.labels.push_str(&registry.render("label_template", &data).unwrap());
        }
    }
}

fn render_dots_string(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<&u64, TimelineColumnData>,
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
                        _ => {} //do nothing
                    }
                    let mut data = EventDotData {
                        hash: *hash as u64,
                        dot_x: resource_owners_layout[hash].x_val,
                        dot_y: get_y_axis_pos(*line_number),
                        // default value if print_message_with_name() fails
                        title: "Unknown Resource Owner Value".to_owned()
                    };
                    if let Some(name) = visualization_data.get_name_from_hash(hash) {
                        match event {
                            Event::OwnerGoOutOfScope => {
                                if !resource_hold {
                                    let resource_info: &str = ". No resource is dropped.";
                                    data.title = event.print_message_with_name(&name);
                                    data.title.push_str(resource_info);
                                } else {
                                    let resource_info: &str = ". Its resource is dropped.";
                                    data.title = event.print_message_with_name(&name);
                                    data.title.push_str(resource_info);
                                }
                            },
                            _ => {
                                data.title = event.print_message_with_name(&name);
                            }
                        }
                    }
                    // push to individual timelines
                    let column = &resource_owners_layout[hash];
                    if column.is_struct_group {
                        if column.is_member {
                            output.get_mut(&(column.owner.to_owned() as i64)).unwrap().1.dots.push_str(&registry.render("dot_template", &data).unwrap());
                        } else {
                            output.get_mut(&(column.owner.to_owned() as i64)).unwrap().0.dots.push_str(&registry.render("dot_template", &data).unwrap());
                        }
                    }
                    else {
                        output.get_mut(&-1).unwrap().0.dots.push_str(&registry.render("dot_template", &data).unwrap());
                    }
                }
            },
        }
    }
}

// render arrows that support function
fn render_arrows_string_external_events_version(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<&u64, TimelineColumnData>,
    registry: &Handlebars
){
    for (line_number, external_event) in &visualization_data.external_events {
        let mut title = String::from("");
        let (from, to) = match external_event {
            ExternalEvent::Bind{ from: from_ro, to: to_ro } => {
                title = String::from("Bind");
                (from_ro, to_ro)
            },
            ExternalEvent::Copy{ from: from_ro, to: to_ro } => {
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
            ExternalEvent::StaticDie{ from: from_ro, to: to_ro } => {
                title = String::from("Return immutably borrowed resource");
                (from_ro, to_ro)
            },
            ExternalEvent::MutableBorrow{ from: from_ro, to: to_ro } => {
                title = String::from("Mutable borrow");
                (from_ro, to_ro)
            },
            ExternalEvent::MutableDie{ from: from_ro, to: to_ro } => {
                title = String::from("Return mutably borrowed resource");
                (from_ro, to_ro)
            },
            ExternalEvent::PassByMutableReference{ from: from_ro, to: to_ro } => {
                title = String::from("Pass by mutable reference");
                (from_ro, to_ro)
            },
            ExternalEvent::PassByStaticReference{ from: from_ro, to: to_ro } => {
                title = String::from("Pass by immutable reference");
                (from_ro, to_ro)
            },
            _ => (&None, &None),
        };
        // complete title
        if let Some(some_from) = from {
            let from_string = match some_from {
                ResourceAccessPoint::Owner(owner) => owner.name.to_owned(),
                ResourceAccessPoint::Struct(stru) => stru.name.to_owned(),
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
                ResourceAccessPoint::Struct(stru) => stru.name.to_owned(),
                ResourceAccessPoint::MutRef(mutref) => mutref.name.to_owned(),
                ResourceAccessPoint::StaticRef(statref) => statref.name.to_owned(),
                ResourceAccessPoint::Function(func) => func.name.to_owned(),
            };
            let styled_to_string = SPAN_BEGIN.to_string() + &to_string + SPAN_END;
            title = format!("{} to {}", title, styled_to_string);
        };

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

                if resource_owners_layout[to_variable.hash()].is_struct_group {
                    if resource_owners_layout[to_variable.hash()].is_member {
                        output.get_mut(&(resource_owners_layout[to_variable.hash()].owner.to_owned() as i64)).unwrap().1.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
                    } else {
                        output.get_mut(&(resource_owners_layout[to_variable.hash()].owner.to_owned() as i64)).unwrap().0.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
                    }
                }
                else {
                    output.get_mut(&-1).unwrap().0.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
                }
            },
            (Some(from_variable), Some(ResourceAccessPoint::Function(function)), 
             ExternalEvent::PassByStaticReference{..}) => { // (Some(variable), Some(function), PassByStatRef)
                // get variable's position
                let styled_fn_name = SPAN_BEGIN.to_string() + &function.name + SPAN_END;
                let styled_from_name = SPAN_BEGIN.to_string() + from_variable.name() + SPAN_END;
                
                let function_dot_data = FunctionDotData {
                    x: resource_owners_layout[from_variable.hash()].x_val,
                    y: get_y_axis_pos(*line_number),
                    title: styled_fn_name + " reads from " + &styled_from_name,
                    hash: from_variable.hash().to_owned() as u64,
                };

                if resource_owners_layout[from_variable.hash()].is_struct_group {
                    if resource_owners_layout[from_variable.hash()].is_member {
                        output.get_mut(&(resource_owners_layout[from_variable.hash()].owner.to_owned() as i64)).unwrap().1.dots.push_str(&registry.render("function_dot_template", &function_dot_data).unwrap());
                    } else {
                        output.get_mut(&(resource_owners_layout[from_variable.hash()].owner.to_owned() as i64)).unwrap().0.dots.push_str(&registry.render("function_dot_template", &function_dot_data).unwrap());
                    }
                }
                else {
                    output.get_mut(&-1).unwrap().0.dots.push_str(&registry.render("function_dot_template", &function_dot_data).unwrap());
                }
            },
            (Some(from_variable), Some(ResourceAccessPoint::Function(function)), 
             ExternalEvent::PassByMutableReference{..}) => {  // (Some(variable), Some(function), PassByMutRef)
                // get variable's position
                let styled_fn_name = SPAN_BEGIN.to_string() + &function.name + SPAN_END;
                let styled_from_name = SPAN_BEGIN.to_string() + from_variable.name() + SPAN_END;

                let function_dot_data = FunctionDotData {
                    x: resource_owners_layout[from_variable.hash()].x_val,
                    y: get_y_axis_pos(*line_number),
                    title: styled_fn_name + " reads from/writes to " + &styled_from_name,
                    hash: from_variable.hash().to_owned() as u64,
                };
                if resource_owners_layout[from_variable.hash()].is_struct_group {
                    if resource_owners_layout[from_variable.hash()].is_member {
                        output.get_mut(&(resource_owners_layout[from_variable.hash()].owner.to_owned() as i64)).unwrap().1.dots.push_str(&registry.render("function_dot_template", &function_dot_data).unwrap());
                    } else {
                        output.get_mut(&(resource_owners_layout[from_variable.hash()].owner.to_owned() as i64)).unwrap().0.dots.push_str(&registry.render("function_dot_template", &function_dot_data).unwrap());
                    }
                }
                else {
                    output.get_mut(&-1).unwrap().0.dots.push_str(&registry.render("function_dot_template", &function_dot_data).unwrap());
                }
            },
            (Some(from_variable), Some(ResourceAccessPoint::Function(to_function)), _) => { // (Some(variable), Some(function), _)
                let styled_fn_name = SPAN_BEGIN.to_string() + &to_function.name + SPAN_END;
                //  ro1 (to_function) <- ro2 (from_variable)
                let x2 = resource_owners_layout[from_variable.hash()].x_val - 5;
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

                if resource_owners_layout[from_variable.hash()].is_struct_group {
                    if resource_owners_layout[from_variable.hash()].is_member {
                        output.get_mut(&(resource_owners_layout[from_variable.hash()].owner.to_owned() as i64)).unwrap().1.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
                    } else {
                        output.get_mut(&(resource_owners_layout[from_variable.hash()].owner.to_owned() as i64)).unwrap().0.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
                    }
                }
                else {
                    output.get_mut(&-1).unwrap().0.dots.push_str(&registry.render("function_logo_template", &function_data).unwrap());
                }
            },
            (Some(from_variable), Some(to_variable), _) => {
                let arrow_order = visualization_data.event_line_map.get(line_number).unwrap().iter().position(|x| x == external_event).unwrap() as i64;

                let x1 = resource_owners_layout[to_variable.hash()].x_val;
                let x2 = resource_owners_layout[from_variable.hash()].x_val;
                let y1 = get_y_axis_pos(*line_number);
                let y2 = get_y_axis_pos(*line_number);
                // if the arrow is pointing from left to right
                if arrow_order > 0 && x2 <= x1{
                    let x3 = resource_owners_layout[from_variable.hash()].x_val + 20;
                    let x4 = resource_owners_layout[to_variable.hash()].x_val - 20;
                    let y3 = get_y_axis_pos(*line_number)+LINE_SPACE*arrow_order;
                    let y4 = get_y_axis_pos(*line_number)+LINE_SPACE*arrow_order;

                    data.coordinates.push((x1 as f64, y1 as f64));
                    data.coordinates.push((x4 as f64, y4 as f64));
                    data.coordinates.push((x3 as f64, y3 as f64));
                    data.coordinates.push((x2 as f64, y2 as f64));

                // if the arrow is pointing from right to left
                } else if arrow_order > 0 && x2 > x1 {
                    let x3 = resource_owners_layout[from_variable.hash()].x_val - 20;
                    let x4 = resource_owners_layout[to_variable.hash()].x_val + 20;
                    let y3 = get_y_axis_pos(*line_number)+LINE_SPACE*arrow_order;
                    let y4 = get_y_axis_pos(*line_number)+LINE_SPACE*arrow_order;

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

            if let Some(ro) = from {
                if resource_owners_layout.contains_key(ro.hash()) && resource_owners_layout[ro.hash()].is_struct_group {
                    if resource_owners_layout[ro.hash()].is_member {
                        output.get_mut(&(resource_owners_layout[ro.hash()].owner.to_owned() as i64)).unwrap().1.arrows.push_str(&registry.render("arrow_template", &data).unwrap());
                    } else {
                        output.get_mut(&(resource_owners_layout[ro.hash()].owner.to_owned() as i64)).unwrap().0.arrows.push_str(&registry.render("arrow_template", &data).unwrap());
                    }
                }
                else {
                    output.get_mut(&-1).unwrap().0.arrows.push_str(&registry.render("arrow_template", &data).unwrap());
                }
            }
        }
    }
}

fn determine_owner_line_styles(
    rap: &ResourceAccessPoint,
    state: &State
) -> OwnerLine {
    match (state, rap.is_mut()) {
        (State::FullPrivilege, true) => OwnerLine::Solid,
        (State::FullPrivilege, false) => OwnerLine::Hollow,
        // cannot assign to to variable because it is borrowed
        // partialprivilege ~= immutable, otherwise it would be an error
        (State::PartialPrivilege{..}, _) => OwnerLine::Hollow, // let (mut) a = 5;
        _ => OwnerLine::Empty, // Otherwise its empty
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
    match (state, style) {
        (State::FullPrivilege, OwnerLine::Solid) | (State::PartialPrivilege{ .. }, OwnerLine::Solid) => {
            data.line_class = String::from("solid");
            data.title += ". The binding can be reassigned.";
            registry.render("vertical_line_template", &data).unwrap()
        },
        (State::FullPrivilege, OwnerLine::Hollow) | (State::PartialPrivilege{..}, OwnerLine::Hollow) => {
            let mut hollow_line_data = data.clone();
            hollow_line_data.title += ". The binding cannot be reassigned.";
            hollow_line_data.x1 -= 1.8; // center middle of path to center of event dot

            registry.render("hollow_line_template", &hollow_line_data).unwrap()
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
    match (state, rap.is_mut()) {
        (State::FullPrivilege, true) => {
            data.line_class = String::from("solid");
            if rap.is_ref() {
                data.title += "; can read and write data; can point to another piece of data.";
            } else {
                data.title += "; can read and write data";
            }
            registry.render("vertical_line_template", &data).unwrap()
        },
        (State::FullPrivilege, false) => {
            if rap.is_ref() {
                data.title += "; can read and write data; cannot point to another piece of data.";
            } else {
                data.title += "; can only read data";
            }
            
            let mut hollow_line_data = data.clone();
            hollow_line_data.x1 -= 1.8; // center middle of path to center of event dot
            
            registry.render("hollow_line_template", &hollow_line_data).unwrap()
        },
        (State::PartialPrivilege{ .. }, _) => {
            data.line_class = String::from("solid");
            data.title += "; can only read data.";
            
            let mut hollow_line_data = data.clone();
            hollow_line_data.x1 -= 1.8; // center middle of path to center of event dot
            hollow_line_data.title = data.title.to_owned();
            
            registry.render("hollow_line_template", &hollow_line_data).unwrap()
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

// render timelines (states) for RAPs using vertical lines
fn render_timelines(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<&u64, TimelineColumnData>,
    registry: &Handlebars
){
    let timelines = &visualization_data.timelines;
    for (hash, timeline) in timelines {
        let rap = &timeline.resource_access_point;
        let rap_states = visualization_data.get_states(hash);
        for (line_start, line_end, state) in rap_states.iter() {
            // println!("{} -> start: {}, end: {}, state: {}", visualization_data.get_name_from_hash(hash).unwrap(), line_start, line_end, state); // DEBUG PURPOSES
            let data = match rap {
                ResourceAccessPoint::Function(_) => None,
                _ => Some(VerticalLineData {
                    line_class: String::new(),
                    hash: *hash,
                    x1: resource_owners_layout[hash].x_val as f64,
                    y1: get_y_axis_pos(*line_start),
                    x2: resource_owners_layout[hash].x_val,
                    y2: get_y_axis_pos(*line_end),
                    title: state.print_message_with_name(rap.name())
                })
            };
            match rap {
                ResourceAccessPoint::Function(_) => {}, // Don't do anything
                ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_) => {
                    if resource_owners_layout[hash].is_struct_group { //TODO: not sure if this is correct
                        if !output.contains_key(&(resource_owners_layout[hash].owner.to_owned() as i64)) {
                            output.insert(resource_owners_layout[hash].owner.to_owned() as i64, (TimelinePanelData{ labels: String::new(), dots: String::new(), timelines: String::new(), 
                                ref_line: String::new(), arrows: String::new() }, TimelinePanelData{ labels: String::new(), dots: String::new(), 
                                    timelines: String::new(), ref_line: String::new(), arrows: String::new() })); 
                        }
                        if resource_owners_layout[hash].is_member {
                            output.get_mut(&(resource_owners_layout[hash].owner.to_owned() as i64)).unwrap().1.timelines.push_str(&create_owner_line_string(rap, state, &mut data.unwrap(), registry));
                        } else {
                            output.get_mut(&(resource_owners_layout[hash].owner.to_owned() as i64)).unwrap().0.timelines.push_str(&create_owner_line_string(rap, state, &mut data.unwrap(), registry));
                        }
                    }
                    else {
                        output.get_mut(&-1).unwrap().0.timelines.push_str(&create_owner_line_string(rap, state, &mut data.unwrap(), registry));
                    }
                },
                ResourceAccessPoint::StaticRef(_) | ResourceAccessPoint::MutRef(_) => {
                    output.get_mut(&-1).unwrap().0.timelines.push_str(&create_reference_line_string(rap, state, &mut data.unwrap(), registry));
                },
            }
        }
    }
}

// vertical lines indicating whether a reference can mutate its resource(deref as many times)
// (iff it's a MutRef && it has FullPrivilege)
fn render_ref_line(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    visualization_data: &VisualizationData,
    resource_owners_layout: &BTreeMap<&u64, TimelineColumnData>,
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
                        State::FullPrivilege => {
                            if !alive {
                                // set known vals
                                data.hash = *hash;
                                data.x1 = resource_owners_layout[hash].x_val;
                                data.y1 = get_y_axis_pos(*line_start);

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
                                data.y1 = get_y_axis_pos(*line_start);

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
            },  
        }
    }
}

fn render_struct_box(
    output: &mut BTreeMap<i64, (TimelinePanelData, TimelinePanelData)>,
    structs_info: &StructsInfo, 
    registry: &Handlebars,
) {
    for (owner, owner_x, last_x) in structs_info.structs.iter() {
        let mut box_data = BoxData {
            name: owner.clone() as u64,
            hash: 0,
            x: 0,
            y: 50,
            w: 0,
            h: 0,
            title: String::new(),
        };   
        box_data.x = owner_x - 20;
        box_data.w = last_x - owner_x + 60;
        box_data.h = 30;
        output.get_mut(owner).unwrap().1.arrows.push_str(&registry.render("box_template", &box_data).unwrap());
    }
}

fn get_y_axis_pos(line_number : usize) -> i64 {
    85 - LINE_SPACE + LINE_SPACE * line_number as i64
}
