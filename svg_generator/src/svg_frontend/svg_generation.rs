extern crate handlebars;

use crate::data::VisualizationData;
use crate::svg_frontend::{code_panel, timeline_panel, utils};
use handlebars::Handlebars;
use serde::Serialize;
use std::cmp;

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

pub fn render_svg(listing_id: &String, description: &String, visualization_data: &VisualizationData) {
    let example_dir_path = format!("examples/book_{}_{}/", listing_id, description);
    let code_image_file_path = format!("rustBook/src/img/vis_{}_code.svg", listing_id);
    let timeline_image_file_path = format!("rustBook/src/img/vis_{}_timeline.svg", listing_id);
    
    let mut code_panel_string = String::new();
    let mut num_lines = 0;

    let svg_code_template = utils::read_file_to_string("src/svg_frontend/code_template.svg")
        .unwrap_or("Reading template.svg failed.".to_owned());
    let svg_timeline_template = utils::read_file_to_string("src/svg_frontend/timeline_template.svg")
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
    if let Ok(lines) = utils::read_lines(example_dir_path.to_owned() + "annotated_source.rs") {
        let (output, line_of_code) = code_panel::render_code_panel(lines);
        code_panel_string = output;
        num_lines = line_of_code;
    }
        
    // data for tl panel
    let (timeline_panel_string, max_width) = timeline_panel::render_timeline_panel(visualization_data);
        
    let svg_data = SvgData {
        visualization_name: description.to_owned(),
        css: css_string,
        code: code_panel_string,
        diagram: timeline_panel_string,
        tl_id: "tl_".to_owned() + listing_id,
        tl_width: cmp::max(max_width, 200),
        height: (num_lines * 20 + 80) + 50,
    };

    let final_code_svg_content = handlebars.render("code_svg_template", &svg_data).unwrap();
    let final_timeline_svg_content = handlebars.render("timeline_svg_template", &svg_data).unwrap();

    // print for debugging
    // println!("{}", final_code_svg_content);
    // println!("{}", final_timeline_svg_content);

    // write to file
    utils::create_and_write_to_file(&final_code_svg_content, example_dir_path.clone() + "rendering_code.svg"); // write svg to /examples
    utils::create_and_write_to_file(&final_timeline_svg_content, example_dir_path.clone() + "rendering_timeline.svg"); // write svg to /examples
    utils::create_and_write_to_file(&final_code_svg_content, code_image_file_path); // write svg code
    utils::create_and_write_to_file(&final_timeline_svg_content, timeline_image_file_path); // write svg timeline
}
