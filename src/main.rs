// rust lib
use std::{
    env, path::Path,
    collections::BTreeMap
};
// svg_generator
mod parse;
use rustviz_lib::svg_frontend::svg_generation;
use rustviz_lib::data::VisualizationData;

fn main() {
    // verify usage
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage Error: cargo run <filename>"); 
        return;
    }

    let path_to_ex = Path::new("examples").join(&args[1]);
    if !path_to_ex.is_dir() {
        println!("Error: no corresponding directory exists in examples/!");
        return;
    }

    let filename = path_to_ex.join("main.rs");
    if !Path::new(&filename).is_file() {
        println!("Example source file (main.rs) not found in {:?}!", &filename);
        return;
    }

    /* ******************************************
            --- Parse main.rs file ---
    ****************************************** */
    println!("{:?}", filename);
    let (contents, line_num, var_map) = parse::parse_vars_to_map(filename);
    let events = parse::extract_events(contents, line_num);
    /* ******************************************
            --- Build VisualizationData ---
    ****************************************** */
    let mut vd = VisualizationData {
        timelines: BTreeMap::new(),
        external_events: Vec::new(),
        preprocess_external_events: Vec::new(),
        event_line_map: BTreeMap::new()
    };
    parse::add_events(&mut vd, var_map, events);
    /* ******************************************
            --- Render SVG images ---
    ****************************************** */
    let input_path = path_to_ex.join("input/")
        .into_os_string().into_string()
        .expect("Error in input file path!");
    let output_path = path_to_ex
        .into_os_string().into_string()
        .expect("Error in output file path!");
    svg_generation::render_svg(&input_path, &(output_path+"/"), &mut vd);
}
