extern crate handlebars;

use crate::data::{ExternalEvent, ResourceAccessPoint_extract, Visualizable, VisualizationData};
use crate::svg_frontend::{code_panel, timeline_panel, utils};
use handlebars::Handlebars;
use serde::Serialize;
use std::cmp;
use std::collections::BTreeMap;

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
    for (line_number, event_vec) in &mut visualization_data.event_line_map {
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
                extra_line += (event_vec.len() - 1);
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
    //---------------------------------------------------------------------------
    // debug!("-------------------------visualization data.timelines------------------");
    // debug!("{:?}", visualization_data.timelines);
    // debug!("-------------------------visualization data.external_events------------------");
    // debug!("{:?}", visualization_data.external_events);
    // debug!("-------------------------visualization data.event_line_map------------------");
    // debug!("{:?}", visualization_data.event_line_map);

    // let example_dir_path = format!("examples/book_{}_{}/", listing_id, description);
    // let code_image_file_path = format!("rustBook/src/img/vis_{}_code.svg", listing_id);
    // let timeline_image_file_path = format!("rustBook/src/img/vis_{}_timeline.svg", listing_id);
    let code_image_file_path = format!("{}vis_code.svg", output_path);
    let timeline_image_file_path = format!("{}vis_timeline.svg", output_path);

    let mut code_panel_string = String::new();
    let mut num_lines = 0;

    let svg_code_template = utils::read_file_to_string("src/svg_frontend/code_template.svg")
        .unwrap_or("Reading template.svg failed.".to_owned());
    let svg_timeline_template =
        utils::read_file_to_string("src/svg_frontend/timeline_template.svg")
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

    let css_string = utils::read_file_to_string("src/svg_frontend/book_svg_style.css")
        .unwrap_or("Reading book_svg_style.css failed.".to_owned());

    // data for code panel
    if let Ok(lines) = utils::read_lines(input_path.to_owned() + "annotated_source.rs") {
        let (output, line_of_code) =
            code_panel::render_code_panel(lines, &visualization_data.event_line_map);
        code_panel_string = output;
        num_lines = line_of_code;
    }

    // data for tl panel
    let (timeline_panel_string, max_width) =
        timeline_panel::render_timeline_panel(visualization_data);

    let svg_data = SvgData {
        visualization_name: input_path.to_owned(),
        css: css_string,
        code: code_panel_string,
        diagram: timeline_panel_string,
        tl_id: "tl_".to_owned() + input_path,
        tl_width: cmp::max(max_width, 200),
        height: (num_lines * 20 + 80) + 50,
    };

    let final_code_svg_content = handlebars.render("code_svg_template", &svg_data).unwrap();
    let final_timeline_svg_content = handlebars
        .render("timeline_svg_template", &svg_data)
        .unwrap();

    // print for debugging
    // println!("{}", final_code_svg_content);
    // println!("{}", final_timeline_svg_content);

    // write to file
    // utils::create_and_write_to_file(&final_code_svg_content, example_dir_path.clone() + "rendering_code.svg"); // write svg to /examples
    // utils::create_and_write_to_file(&final_timeline_svg_content, example_dir_path.clone() + "rendering_timeline.svg"); // write svg to /examples
    utils::create_and_write_to_file(&final_code_svg_content, code_image_file_path); // write svg code
    utils::create_and_write_to_file(&final_timeline_svg_content, timeline_image_file_path); // write svg timeline
}
