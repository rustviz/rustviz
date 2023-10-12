extern crate handlebars;

use crate::data::{ExternalEvent, ResourceAccessPoint_extract, Visualizable, VisualizationData, LINE_SPACE};
use crate::svg_frontend::{code_panel, timeline_panel, utils};
use handlebars::Handlebars;
use serde::Serialize;
use std::cmp;
use std::collections::BTreeMap;

/* visualization_name: The name of the visualization.
   css: CSS styles for the visualization.
   code: The code for generating the SVG.
   diagram: The code for the SVG diagram.
   tl_id: The ID of the timeline in the SVG.
   tl_width: The width of the timeline.
   height: The height of the SVG.
*/
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

pub fn render_svg(
    input_path: &String,
    output_path: &String,
    visualization_data: &mut VisualizationData,
) {
    //------------------------sort HashMap<usize, Vec<ExternalEvent>>----------------------
    // first by sorting "to" from small to large number then sort by "from" from small to large number
    // Q: does for loop do the "move"?
    // Q: how is this okay??
    for (_, event_vec) in &mut visualization_data.event_line_map {
        event_vec.sort_by(|a, b| {
            ResourceAccessPoint_extract(a)
                .1
                .as_ref()
                .unwrap()
                .hash()
                .cmp(&ResourceAccessPoint_extract(b).1.as_ref().unwrap().hash())
                .then(
                    ResourceAccessPoint_extract(a)
                        .0
                        .as_ref()
                        .unwrap()
                        .hash()
                        .cmp(&ResourceAccessPoint_extract(b).0.as_ref().unwrap().hash()),
                )
        });
    }

    // Q: is this a copy?
    //-----------------------update line number for external events------------------
    for (line_number, event) in visualization_data.preprocess_external_events.clone() {
        let mut extra_line: usize = 0;
        for (info_line_number, event_vec) in &visualization_data.event_line_map {
            if info_line_number < &line_number {
                extra_line += event_vec.len() - 1;
            } else {
                break;
            }
        }
        let final_line_num = line_number.clone() + extra_line;
        visualization_data.append_processed_external_event(event, final_line_num);
    }
    //-----------------------update event_line_map line number------------------
    let mut event_line_map_replace: BTreeMap<usize, Vec<ExternalEvent>> = BTreeMap::new();
    let mut extra_line_sum = 0;
    for (line_number, event_vec) in &visualization_data.event_line_map {
        event_line_map_replace.insert(line_number + extra_line_sum, event_vec.clone());
        extra_line_sum += event_vec.len() - 1;
    }
    visualization_data.event_line_map = event_line_map_replace;

    let code_image_file_path = format!("{}vis_code.svg", output_path);
    let timeline_image_file_path = format!("{}vis_timeline.svg", output_path);

    let mut code_panel_string = String::new();
    let mut num_lines = 0;

    let template_path = "svg_generator/templates/";
    let svg_code_template = utils::read_file_to_string(template_path.to_string()+"code_template.svg")
        .unwrap_or("Reading template.svg failed.".to_owned());
    let svg_timeline_template =
        utils::read_file_to_string(template_path.to_string()+"timeline_template.svg")
            .unwrap_or("Reading template.svg failed.".to_owned());


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

    let css_string = utils::read_file_to_string(template_path.to_string()+"book_svg_style.css")
        .unwrap_or("Reading book_svg_style.css failed.".to_owned());

    // data for code panel
    let mut max_x_space: i64 = 0;
    if let (Ok(annotated_lines),Ok(lines)) = 
    (utils::read_lines(input_path.to_owned() + "annotated_source.rs"), utils::read_lines(output_path.to_owned() + "source.rs")) {
        let (output, line_of_code) =
            code_panel::render_code_panel(annotated_lines, lines, &mut max_x_space, &visualization_data.event_line_map);
        code_panel_string = output;
        num_lines = line_of_code;
    }

    // data for tl panel
    let (timeline_panel_string, max_width) = timeline_panel::render_timeline_panel(visualization_data);

    let svg_data = SvgData {
        visualization_name: input_path.to_owned(),
        css: css_string,
        code: code_panel_string,
        diagram: timeline_panel_string,
        tl_id: "tl_".to_owned() + input_path,
        tl_width: cmp::max(max_width, 200),
        height: (num_lines * LINE_SPACE as i32 + 80) + 50,
    };

    let final_code_svg_content = handlebars.render("code_svg_template", &svg_data).unwrap();
    let final_timeline_svg_content = handlebars
        .render("timeline_svg_template", &svg_data)
        .unwrap();

    // write to file
    utils::create_and_write_to_file(&final_code_svg_content, code_image_file_path); // write svg code
    utils::create_and_write_to_file(&final_timeline_svg_content, timeline_image_file_path); // write svg timeline
}
