extern crate handlebars;

use crate::data::{ExternalEvent, ResourceAccessPoint_extract, Visualizable, VisualizationData, LINE_SPACE};
use crate::svg_frontend::{code_panel, timeline_panel};
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
    annotated_src_str: &str,
    source_rs_str: &str,
    visualization_data: &mut VisualizationData,
) -> (String, String){
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

    let svg_code_template = String::from(
    "<svg height=\"{{height}}px\" xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">

      <desc>{{ visualization_name }}</desc>

      <defs>
          <style type=\"text/css\">
          <![CDATA[
          {{ css }}
          ]]>
          </style>
      </defs>

      <g>
          <text id=\"caption\" x=\"30\" y=\"30\">Hover over timeline events (dots), states (vertical lines),</text>
          <text id=\"caption\" x=\"30\" y=\"50\">and actions (arrows) for extra information.</text>
      </g>

      {{ code }}

      </svg>");
  // utils::read_file_to_string(code_template_path.as_os_str())
  //       .unwrap_or("Reading template.svg failed.".to_owned());
    let svg_timeline_template = String::from("
    <svg width=\"{{tl_width}}px\" height=\"{{height}}px\" 
        xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" 
        id=\"{{tl_id}}\">

    <desc>{{ visualization_name }}</desc>

    <defs>
        <style type=\"text/css\">
        <![CDATA[
        {{ css }}
        
        text {
            user-select: none;
            -webkit-user-select: none;
            -moz-user-select: none;
            -ms-user-select: none;
        }
        ]]>
        </style>
        <!-- used when pass to function by ref -->
        <g id=\"functionDot\">
             <circle cx=\"0\" cy=\"0\" r=\"5\" fill=\"transparent\"/>
             <text class=\"functionIcon\" dx=\"-3.5\" dy=\"0\" fill=\"#6e6b5e\">f</text>
        </g>
        <marker id=\"arrowHead\" viewBox=\"0 0 10 10\"
            refX=\"0\" refY=\"4\"
            markerUnits=\"strokeWidth\"
            markerWidth=\"3px\" markerHeight=\"3px\"
            orient=\"auto\" fill=\"gray\">
            <path d=\"M 0 0 L 8.5 4 L 0 8 z\" fill=\"inherit\"/>
        </marker>
        <!-- glow highlight filter -->
        <filter id=\"glow\" x=\"-5000%\" y=\"-5000%\" width=\"10000%\" height=\"10000%\" filterUnits=\"userSpaceOnUse\">
            <feComposite in=\"flood\" result=\"mask\" in2=\"SourceGraphic\" operator=\"in\"></feComposite>
            <feGaussianBlur stdDeviation=\"2\" result=\"coloredBlur\"/>
            <feMerge>
                <feMergeNode in=\"coloredBlur\"></feMergeNode>
                <feMergeNode in=\"coloredBlur\"></feMergeNode>
                <feMergeNode in=\"coloredBlur\"></feMergeNode>
                <feMergeNode in=\"SourceGraphic\"></feMergeNode>
            </feMerge>
            <!-- increase brightness -->
            <feComponentTransfer>
                <feFuncR type=\"linear\" slope=\"2\"/>
                <feFuncG type=\"linear\" slope=\"2\"/>
                <feFuncB type=\"linear\" slope=\"2\"/>
            </feComponentTransfer>
        </filter>
    </defs>

    {{ diagram }}

    </svg>");
        // utils::read_file_to_string(svg_template_path.as_os_str())
        //     .unwrap_or("Reading template.svg failed.".to_owned());
    let css_string = String::from("/* general setup */
    :root {
        --bg-color:#f1f1f1;
        --text-color: #6e6b5e;
    }
    
    svg {
        background-color: var(--bg-color);
    }
    
    text {
        vertical-align: baseline;
        text-anchor: start;
    }
    
    #heading {
        font-size: 24px;
        font-weight: bold;
    }
    
    #caption {
        font-size: 0.875em;
        font-family: \"Open Sans\", sans-serif;
        font-style: italic;
    }
    
    /* code related styling */
    text.code {
        fill: #6e6b5e;
        white-space: pre;
        font-family: \"Source Code Pro\", Consolas, \"Ubuntu Mono\", Menlo, \"DejaVu Sans Mono\", monospace, monospace !important;
        font-size: 0.875em;
    }
    
    text.label {
        font-family: \"Source Code Pro\", Consolas, \"Ubuntu Mono\", Menlo, \"DejaVu Sans Mono\", monospace, monospace !important;
        font-size: 0.875em;
    }
    
    /* timeline/event interaction styling */
    .solid {
        stroke-width: 5px;
    }
    
    .hollow {
        stroke-width: 1.5;
    }
    
    .dotted {
        stroke-width: 5px;
        stroke-dasharray: \"2 1\";
    }
    
    .extend {
        stroke-width: 1px;
        stroke-dasharray: \"2 1\";
    }
    
    .functionIcon {
        paint-order: stroke;
        stroke-width: 3px;
        fill: var(--bg-color);
        font-size: 20px;
        font-family: times;
        font-weight: lighter;
        dominant-baseline: central;
        text-anchor: start;
        font-style: italic;
    }
    
    .functionLogo {
        font-size: 20px;
        font-style: italic;
        paint-order: stroke;
        stroke-width: 3px;
        fill: var(--bg-color) !important;
    }
    
    /* flex related styling */
    .flex-container {
        display: flex;
        flex-direction: row;
        justify-content: flex-start;
        flex-wrap: nowrap;
        flex-shrink: 0;
    }
    
    object.tl_panel {
        flex-grow: 1;
    }
    
    object.code_panel {
        flex-grow: 0;
    }
    
    .tooltip-trigger {
        cursor: default;
    }
    
    .tooltip-trigger:hover{
        filter: url(#glow);
    }
    
    /* hash based styling */
    [data-hash=\"0\"] {
        fill: #6e6b5e;
    }
    
    [data-hash=\"1\"] {
        fill: #1893ff;
        stroke: #1893ff;
    }
    
    [data-hash=\"2\"] {
        fill: #ff7f50;
        stroke: #ff7f50;
    }
    
    [data-hash=\"3\"] {
        fill: #8635ff;
        stroke: #8635ff;
    }
    
    [data-hash=\"4\"] {
        fill: #dc143c;
        stroke: #dc143c;
    }
    
    [data-hash=\"5\"] {
        fill: #0a810a;
        stroke: #0a810a;
    }
    
    [data-hash=\"6\"] {
        fill: #008080;
        stroke: #008080;
    }
    
    [data-hash=\"7\"] {
        fill: #ff6cce;
        stroke: #ff6cce;
    }
    
    [data-hash=\"8\"] {
        fill: #00d6fc;
        stroke: #00d6fc;
    }
    
    [data-hash=\"9\"] {
        fill: #b99f35;
        stroke: #b99f35;
    }");
    
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
            code_panel::render_code_panel(a_lines, s_lines, &mut max_x_space, &visualization_data.event_line_map);
    let code_panel_string = output;
    let num_lines = line_of_code;

    // data for tl panel
    let (timeline_panel_string, max_width) = timeline_panel::render_timeline_panel(visualization_data);
    
    let svg_data = SvgData {
        visualization_name: "vis".to_owned(),
        css: css_string,
        code: code_panel_string,
        diagram: timeline_panel_string,
        tl_id: "tl_".to_owned() + "vis",
        tl_width: cmp::max(max_width, 200),
        height: (num_lines * LINE_SPACE as i32 + 80) + 50,
    };

    let final_code_svg_content = handlebars.render("code_svg_template", &svg_data).unwrap();
    let final_timeline_svg_content = handlebars
        .render("timeline_svg_template", &svg_data)
        .unwrap();

    // write to file
    // utils::create_and_write_to_file(&final_code_svg_content, code_image_file_path); // write svg code
    // utils::create_and_write_to_file(&final_timeline_svg_content, timeline_image_file_path); // write svg timeline
    //println!("{}", final_code_svg_content);
    (final_code_svg_content, final_timeline_svg_content)
}
