extern crate handlebars;

use crate::data::{ExternalEvent, ResourceAccessPoint_extract, Visualizable, VisualizationData, LINE_SPACE};
use crate::svg_frontend::{code_panel, timeline_panel, utils, lifetime_vis};
use handlebars::Handlebars;
use serde::Serialize;
use std::cmp;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Serialize)]
struct SvgData {
    visualization_name: String,
    css: String,
    code: String,
    diagram: String,
    tl_id: String,
    tl_width: i32,
    height: i32,
    code_panel_width: usize,
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
    let mut code_max_width: usize = 0;
    if let (Ok(annotated_lines),Ok(lines)) = 
    (utils::read_lines(input_path.to_owned() + "annotated_source.rs"), utils::read_lines(output_path.to_owned() + "source.rs")) {
        let (output, line_of_code, width) =
            code_panel::render_code_panel(annotated_lines, lines, &mut max_x_space, &visualization_data.event_line_map);
        code_panel_string = output;
        num_lines = line_of_code;
        code_max_width = width;
    }

    // data for tl panel
    let (timeline_panel_string, max_width) = timeline_panel::render_timeline_panel(visualization_data);

    let mut svg_data = SvgData {
        visualization_name: input_path.to_owned(),
        css: css_string,
        code: code_panel_string,
        diagram: timeline_panel_string,
        tl_id: "tl_".to_owned() + input_path,
        tl_width: cmp::max(max_width, 200),
        height: (num_lines * LINE_SPACE as i32 + 80) + 50,
        code_panel_width: code_max_width
    };

    // data for lifetime panel (optional)
    /*
     * TODO: Make sure multiple lifetime parameter can work
     */
    if let Some(lifetime_info_data) = visualization_data.lifetimes.clone(){
        let path_to_main_rs = Path::new(output_path).join("main.rs");
        let path_to_source_rs = Path::new(output_path).join("source.rs");
        let (lifetime_render_str, width, height) = lifetime_vis::render_lifetime_panel(path_to_main_rs.to_str().unwrap().to_string(), path_to_source_rs.to_str().unwrap().to_string(), &lifetime_info_data[0]);
        // println!("width: {}, height: {}", width, height);
        svg_data.diagram = lifetime_render_str;
        svg_data.height = cmp::max(svg_data.height, height as i32);
        svg_data.tl_width = width as i32;
    }
    let final_code_svg_content = handlebars.render("code_svg_template", &svg_data).unwrap();
    let final_timeline_svg_content = handlebars
        .render("timeline_svg_template", &svg_data)
        .unwrap();

    // write to file
    utils::create_and_write_to_file(&final_code_svg_content, code_image_file_path); // write svg code
    utils::create_and_write_to_file(&final_timeline_svg_content, timeline_image_file_path); // write svg timeline

}
